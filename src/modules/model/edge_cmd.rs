use std::sync::atomic::{AtomicU64, Ordering};

use acadrust::Handle;
use cadkernel::brep::{Body, EdgeKey, FaceKey};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec,
};
use crate::scene::model::wire_model::WireModel;

static FILLET_RADIUS_BITS: AtomicU64 = AtomicU64::new(1.0f64.to_bits());

fn fillet_radius() -> f64 {
    f64::from_bits(FILLET_RADIUS_BITS.load(Ordering::Relaxed))
}

fn set_fillet_radius(value: f64) {
    FILLET_RADIUS_BITS.store(value.to_bits(), Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EdgeOperation {
    Fillet,
    Chamfer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeSelectionMode {
    Edge,
    Chain,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeStep {
    Selecting,
    PickingLoop,
    LoopConfirm,
    PreviewConfirm,
    Radius,
    Expression,
    ChamferDistance1,
    ChamferDistance2,
}

pub struct SolidEdgeCommand {
    operation: EdgeOperation,
    target: Option<Handle>,
    bodies: Vec<(Handle, Body)>,
    handle: Option<Handle>,
    selected_edges: Vec<EdgeKey>,
    base_face: Option<FaceKey>,
    selection_batches: Vec<(Vec<EdgeKey>, DVec3)>,
    selection_mode: EdgeSelectionMode,
    step: EdgeStep,
    value_return: EdgeStep,
    expression_return: EdgeStep,
    loop_candidates: Vec<(FaceKey, Vec<EdgeKey>)>,
    loop_index: usize,
    last_pick: Option<DVec3>,
    default_value: f64,
    other_value: f64,
    preview_color: [f32; 4],
    preview_wires: Vec<WireModel>,
    preview_hidden: Vec<Handle>,
}

impl SolidEdgeCommand {
    pub fn new(
        operation: EdgeOperation,
        target: Option<Handle>,
        bodies: Vec<(Handle, Body)>,
        preview_color: [f32; 4],
    ) -> Self {
        Self {
            operation,
            target,
            bodies,
            handle: None,
            selected_edges: Vec::new(),
            base_face: None,
            selection_batches: Vec::new(),
            selection_mode: EdgeSelectionMode::Edge,
            step: EdgeStep::Selecting,
            value_return: EdgeStep::Selecting,
            expression_return: EdgeStep::Radius,
            loop_candidates: Vec::new(),
            loop_index: 0,
            last_pick: None,
            default_value: if operation == EdgeOperation::Fillet {
                fillet_radius()
            } else {
                1.0
            },
            other_value: 1.0,
            preview_color,
            preview_wires: Vec::new(),
            preview_hidden: Vec::new(),
        }
    }

    pub fn new_chamfer(
        target: Option<Handle>,
        bodies: Vec<(Handle, Body)>,
        preview_color: [f32; 4],
        distances: (f64, f64),
    ) -> Self {
        let mut command = Self::new(EdgeOperation::Chamfer, target, bodies, preview_color);
        command.default_value = positive_default(distances.0);
        command.other_value = positive_default(distances.1);
        command
    }

    pub fn chamfer_distances(&self) -> (f64, f64) {
        (self.default_value, self.other_value)
    }

    fn body(&self, handle: Handle) -> Option<&Body> {
        self.bodies
            .iter()
            .find_map(|(candidate, body)| (*candidate == handle).then_some(body))
    }

    fn active_body(&self) -> Option<(Handle, &Body)> {
        let handle = self.handle?;
        self.body(handle).map(|body| (handle, body))
    }

    fn pick_allowed(&self, handle: Handle) -> bool {
        !handle.is_null()
            && self.target.is_none_or(|target| target == handle)
            && self.handle.is_none_or(|selected| selected == handle)
            && self.body(handle).is_some()
    }

    fn add_batch(&mut self, edges: Vec<EdgeKey>, anchor: DVec3) -> bool {
        let batch = edges
            .into_iter()
            .filter(|edge| !self.selected_edges.contains(edge))
            .collect::<Vec<_>>();
        if batch.is_empty() {
            return false;
        }
        self.selected_edges.extend(batch.iter().copied());
        self.selection_batches.push((batch, anchor));
        true
    }

    fn current_loop(&self) -> Option<&[EdgeKey]> {
        self.loop_candidates
            .get(self.loop_index)
            .map(|(_, edges)| edges.as_slice())
    }

    fn current_loop_face(&self) -> Option<FaceKey> {
        self.loop_candidates
            .get(self.loop_index)
            .map(|(face, _)| *face)
    }

    fn preview_edges(&self) -> Vec<EdgeKey> {
        let mut edges = self.selected_edges.clone();
        if self.step == EdgeStep::LoopConfirm {
            if let Some(loop_edges) = self.current_loop() {
                for edge in loop_edges {
                    if !edges.contains(edge) {
                        edges.push(*edge);
                    }
                }
            }
        }
        edges
    }

    fn preview_active(&self) -> bool {
        matches!(
            self.step,
            EdgeStep::Selecting
                | EdgeStep::PickingLoop
                | EdgeStep::LoopConfirm
                | EdgeStep::PreviewConfirm
                | EdgeStep::Radius
                | EdgeStep::Expression
                | EdgeStep::ChamferDistance1
                | EdgeStep::ChamferDistance2
        )
    }

    fn preview_for_values(
        &self,
        value: f64,
        other_value: f64,
    ) -> Option<(Handle, Vec<WireModel>)> {
        if !self.preview_active() {
            return None;
        }
        let edges = self.preview_edges();
        if edges.is_empty() {
            return None;
        }
        let (handle, body) = self.active_body()?;
        let result = match self.operation {
            EdgeOperation::Fillet => cadkernel::brep::fillet_edges(body, &edges, value).ok()?,
            EdgeOperation::Chamfer => cadkernel::brep::chamfer_edges(
                body,
                &edges,
                self.base_face.or_else(|| self.current_loop_face())?,
                value,
                other_value,
            )
            .ok()?,
        };
        let mut wires = crate::scene::model::solid_model::grip_preview_wires(&result, handle);
        for wire in &mut wires {
            wire.color = self.preview_color;
            wire.name = format!("{}-{}-PREVIEW", handle.value(), self.name());
        }
        (!wires.is_empty()).then_some((handle, wires))
    }

    fn rebuild_preview(&mut self) {
        self.preview_wires.clear();
        self.preview_hidden.clear();
        if let Some((handle, wires)) =
            self.preview_for_values(self.default_value, self.other_value)
        {
            self.preview_wires = wires;
            self.preview_hidden.push(handle);
        }
    }

    fn begin_radius(&mut self, return_to: EdgeStep) -> CmdResult {
        self.value_return = return_to;
        self.step = EdgeStep::Radius;
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn accept_radius(&mut self, value: f64) -> Option<CmdResult> {
        if value <= 0.0 || !value.is_finite() {
            return None;
        }
        self.default_value = value;
        if self.operation == EdgeOperation::Fillet {
            set_fillet_radius(value);
        }
        self.step = self.value_return;
        self.rebuild_preview();
        Some(CmdResult::NeedPoint)
    }

    fn begin_distances(&mut self, return_to: EdgeStep) -> CmdResult {
        self.value_return = return_to;
        self.step = EdgeStep::ChamferDistance1;
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn accept_distance(&mut self, value: f64) -> Option<CmdResult> {
        if value <= 0.0 || !value.is_finite() {
            return None;
        }
        match self.step {
            EdgeStep::ChamferDistance1 => {
                self.default_value = value;
                self.step = EdgeStep::ChamferDistance2;
            }
            EdgeStep::ChamferDistance2 => {
                self.other_value = value;
                self.step = self.value_return;
            }
            _ => return None,
        }
        self.rebuild_preview();
        Some(CmdResult::NeedPoint)
    }

    fn accept_loop(&mut self) -> CmdResult {
        if let (Some(edges), Some(anchor)) = (
            self.current_loop().map(|edges| edges.to_vec()),
            self.last_pick,
        ) {
            if self.operation == EdgeOperation::Chamfer && self.base_face.is_none() {
                self.base_face = self.current_loop_face();
            }
            self.add_batch(edges, anchor);
        }
        self.loop_candidates.clear();
        self.loop_index = 0;
        self.step = EdgeStep::Selecting;
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn finish(&self) -> CmdResult {
        let Some(handle) = self.handle else {
            return CmdResult::Cancel;
        };
        if self.selected_edges.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::SolidEdgeBlend {
            handle,
            edges: self.selected_edges.clone(),
            base_face: self.base_face,
            value: self.default_value,
            other_value: self.other_value,
            fillet: self.operation == EdgeOperation::Fillet,
        }
    }
}

impl CadCommand for SolidEdgeCommand {
    fn name(&self) -> &'static str {
        match self.operation {
            EdgeOperation::Fillet => "FILLETEDGE",
            EdgeOperation::Chamfer => "CHAMFEREDGE",
        }
    }

    fn prompt(&self) -> String {
        match (self.operation, self.step, self.selection_mode) {
            (EdgeOperation::Chamfer, EdgeStep::Selecting, _) => {
                crate::t!("Select an edge or [Loop/Distance]:").into_owned()
            }
            (EdgeOperation::Chamfer, EdgeStep::PickingLoop, _) => {
                crate::t!("Select edge of loop or [Edge/Distance]:").into_owned()
            }
            (EdgeOperation::Chamfer, EdgeStep::PreviewConfirm, _) => {
                crate::t!("Press Enter to accept the chamfer or [Distance]:").into_owned()
            }
            (EdgeOperation::Chamfer, EdgeStep::ChamferDistance1, _) => crate::tf!(
                "Specify Distance1 or [Expression] <{:.4}>:",
                self.default_value
            )
            .into_owned(),
            (EdgeOperation::Chamfer, EdgeStep::ChamferDistance2, _) => crate::tf!(
                "Specify Distance2 or [Expression] <{:.4}>:",
                self.other_value
            )
            .into_owned(),
            (_, EdgeStep::Selecting, EdgeSelectionMode::Edge) => {
                crate::t!("Select an edge or [Chain/Loop/Radius]:").into_owned()
            }
            (_, EdgeStep::Selecting, EdgeSelectionMode::Chain) => {
                crate::t!("Select an edge chain or [Edge/Radius]:").into_owned()
            }
            (_, EdgeStep::PickingLoop, _) => {
                crate::t!("Select edge of loop or [Edge/Chain/Radius]:").into_owned()
            }
            (_, EdgeStep::LoopConfirm, _) => {
                crate::t!("Enter an option [Accept/Next] <Accept>:").into_owned()
            }
            (_, EdgeStep::PreviewConfirm, _) => {
                crate::t!("Press Enter to accept the fillet or [Radius]:").into_owned()
            }
            (_, EdgeStep::Radius, _) if self.value_return == EdgeStep::PreviewConfirm => crate::tf!(
                "Specify Radius or [Expression] <{:.4}>:",
                self.default_value
            )
            .into_owned(),
            (_, EdgeStep::Radius, _) => crate::tf!(
                "Enter fillet radius or [Expression] <{:.4}>:",
                self.default_value
            )
            .into_owned(),
            (_, EdgeStep::Expression, _) => crate::t!("Enter expression:").into_owned(),
            _ => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match (self.operation, self.step, self.selection_mode) {
            (EdgeOperation::Chamfer, EdgeStep::Selecting, _) => vec![
                CmdOption::new("Loop", "L"),
                CmdOption::new("Distance", "D"),
            ],
            (EdgeOperation::Chamfer, EdgeStep::PickingLoop, _) => vec![
                CmdOption::new("Edge", "E"),
                CmdOption::new("Distance", "D"),
            ],
            (EdgeOperation::Chamfer, EdgeStep::LoopConfirm, _) => vec![
                CmdOption::new("Accept", "A"),
                CmdOption::new("Next", "N"),
            ],
            (EdgeOperation::Chamfer, EdgeStep::PreviewConfirm, _) => {
                vec![CmdOption::new("Distance", "D")]
            }
            (
                EdgeOperation::Chamfer,
                EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2,
                _,
            ) => vec![CmdOption::new("Expression", "E")],
            (EdgeOperation::Fillet, EdgeStep::Selecting, EdgeSelectionMode::Edge) => vec![
                CmdOption::new("Chain", "C"),
                CmdOption::new("Loop", "L"),
                CmdOption::new("Radius", "R"),
            ],
            (EdgeOperation::Fillet, EdgeStep::Selecting, EdgeSelectionMode::Chain) => vec![
                CmdOption::new("Edge", "E"),
                CmdOption::new("Radius", "R"),
            ],
            (EdgeOperation::Fillet, EdgeStep::PickingLoop, _) => vec![
                CmdOption::new("Edge", "E"),
                CmdOption::new("Chain", "C"),
                CmdOption::new("Radius", "R"),
            ],
            (EdgeOperation::Fillet, EdgeStep::LoopConfirm, _) => vec![
                CmdOption::new("Accept", "A"),
                CmdOption::new("Next", "N"),
            ],
            (EdgeOperation::Fillet, EdgeStep::PreviewConfirm, _) => {
                vec![CmdOption::new("Radius", "R")]
            }
            (EdgeOperation::Fillet, EdgeStep::Radius, _) => {
                vec![CmdOption::new("Expression", "E")]
            }
            _ => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, EdgeStep::Selecting | EdgeStep::PickingLoop)
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if !self.pick_allowed(handle) {
            return CmdResult::NeedPoint;
        }
        let Some(seed) = self
            .body(handle)
            .and_then(|body| crate::scene::model::solid_model::nearest_edge(body, point.to_array()))
        else {
            return CmdResult::NeedPoint;
        };
        self.handle = Some(handle);
        self.last_pick = Some(point);

        if self.step == EdgeStep::PickingLoop {
            self.loop_candidates = edge_loops(self.body(handle).expect("pick body exists"), seed);
            if self.operation == EdgeOperation::Chamfer {
                if let Some(base_face) = self.base_face {
                    self.loop_candidates
                        .retain(|(face, _)| *face == base_face);
                }
            }
            self.loop_index = 0;
            self.step = if self.loop_candidates.is_empty() {
                EdgeStep::Selecting
            } else {
                EdgeStep::LoopConfirm
            };
            self.rebuild_preview();
            return CmdResult::NeedPoint;
        }

        if self.operation == EdgeOperation::Chamfer {
            if self.base_face.is_none() {
                self.base_face = self.body(handle).and_then(|body| {
                    nearest_edge_face(body, seed, point.to_array())
                });
            }
            let Some(base_face) = self.base_face else {
                return CmdResult::NeedPoint;
            };
            if !edge_belongs_to_face(self.body(handle).expect("pick body exists"), seed, base_face)
            {
                return CmdResult::NeedPoint;
            }
            self.add_batch(vec![seed], point);
            self.rebuild_preview();
            return CmdResult::NeedPoint;
        }

        let edges = match self.selection_mode {
            EdgeSelectionMode::Edge => vec![seed],
            EdgeSelectionMode::Chain => edge_chain(self.body(handle).expect("pick body exists"), seed),
        };
        self.add_batch(edges, point);
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !matches!(
            self.step,
            EdgeStep::Radius | EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2
        ) {
            return CmdResult::NeedPoint;
        }
        let Some(anchor) = self.last_pick else {
            return CmdResult::NeedPoint;
        };
        let value = point.distance(anchor);
        if value <= 0.0 || !value.is_finite() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            EdgeStep::Radius => self.accept_radius(value).unwrap_or(CmdResult::NeedPoint),
            EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2 => {
                self.accept_distance(value).unwrap_or(CmdResult::NeedPoint)
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        if self.operation == EdgeOperation::Chamfer {
            return match self.step {
                EdgeStep::Selecting => match keyword.as_str() {
                    "L" | "LOOP" => {
                        self.step = EdgeStep::PickingLoop;
                        Some(CmdResult::NeedPoint)
                    }
                    "D" | "DISTANCE" => Some(self.begin_distances(EdgeStep::Selecting)),
                    _ => None,
                },
                EdgeStep::PickingLoop => match keyword.as_str() {
                    "E" | "EDGE" => {
                        self.step = EdgeStep::Selecting;
                        Some(CmdResult::NeedPoint)
                    }
                    "D" | "DISTANCE" => Some(self.begin_distances(EdgeStep::PickingLoop)),
                    _ => None,
                },
                EdgeStep::LoopConfirm => match keyword.as_str() {
                    "A" | "ACCEPT" => Some(self.accept_loop()),
                    "N" | "NEXT" if !self.loop_candidates.is_empty() => {
                        self.loop_index = (self.loop_index + 1) % self.loop_candidates.len();
                        self.rebuild_preview();
                        Some(CmdResult::NeedPoint)
                    }
                    _ => None,
                },
                EdgeStep::PreviewConfirm => match keyword.as_str() {
                    "D" | "DISTANCE" => Some(self.begin_distances(EdgeStep::PreviewConfirm)),
                    _ => None,
                },
                EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2 => {
                    if matches!(keyword.as_str(), "E" | "EXPRESSION") {
                        self.expression_return = self.step;
                        self.step = EdgeStep::Expression;
                        Some(CmdResult::NeedPoint)
                    } else {
                        let value = keyword.parse::<f64>().ok()?;
                        self.accept_distance(value)
                    }
                }
                EdgeStep::Expression => {
                    let value = crate::app::expr_eval::eval_number(text)?;
                    self.step = self.expression_return;
                    self.accept_distance(value)
                }
                EdgeStep::Radius => None,
            };
        }

        match self.step {
            EdgeStep::Selecting => match keyword.as_str() {
                "R" | "RADIUS" => Some(self.begin_radius(EdgeStep::Selecting)),
                "C" | "CHAIN" => {
                    self.selection_mode = EdgeSelectionMode::Chain;
                    Some(CmdResult::NeedPoint)
                }
                "E" | "EDGE" if self.selection_mode == EdgeSelectionMode::Chain => {
                    self.selection_mode = EdgeSelectionMode::Edge;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LOOP" => {
                    self.step = EdgeStep::PickingLoop;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            EdgeStep::PickingLoop => match keyword.as_str() {
                "E" | "EDGE" => {
                    self.selection_mode = EdgeSelectionMode::Edge;
                    self.step = EdgeStep::Selecting;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CHAIN" => {
                    self.selection_mode = EdgeSelectionMode::Chain;
                    self.step = EdgeStep::Selecting;
                    Some(CmdResult::NeedPoint)
                }
                "R" | "RADIUS" => Some(self.begin_radius(EdgeStep::PickingLoop)),
                _ => None,
            },
            EdgeStep::LoopConfirm => match keyword.as_str() {
                "A" | "ACCEPT" => Some(self.accept_loop()),
                "N" | "NEXT" if !self.loop_candidates.is_empty() => {
                    self.loop_index = (self.loop_index + 1) % self.loop_candidates.len();
                    self.rebuild_preview();
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            EdgeStep::PreviewConfirm => match keyword.as_str() {
                "R" | "RADIUS" => Some(self.begin_radius(EdgeStep::PreviewConfirm)),
                _ => None,
            },
            EdgeStep::Radius => {
                if matches!(keyword.as_str(), "E" | "EXPRESSION") {
                    self.step = EdgeStep::Expression;
                    return Some(CmdResult::NeedPoint);
                }
                let value = keyword.parse::<f64>().ok()?;
                self.accept_radius(value)
            }
            EdgeStep::Expression => {
                let value = crate::app::expr_eval::eval_number(text)?;
                self.accept_radius(value)
            }
            EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2 => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match (self.operation, self.step) {
            (EdgeOperation::Chamfer, EdgeStep::Selecting) if !self.selected_edges.is_empty() => {
                self.step = EdgeStep::PreviewConfirm;
                self.rebuild_preview();
                CmdResult::NeedPoint
            }
            (EdgeOperation::Chamfer, EdgeStep::LoopConfirm) => self.accept_loop(),
            (EdgeOperation::Chamfer, EdgeStep::PreviewConfirm) => self.finish(),
            (EdgeOperation::Chamfer, EdgeStep::ChamferDistance1) => self
                .accept_distance(self.default_value)
                .unwrap_or(CmdResult::NeedPoint),
            (EdgeOperation::Chamfer, EdgeStep::ChamferDistance2) => self
                .accept_distance(self.other_value)
                .unwrap_or(CmdResult::NeedPoint),
            (EdgeOperation::Chamfer, EdgeStep::Expression) => CmdResult::NeedPoint,
            (EdgeOperation::Chamfer, _) => CmdResult::Cancel,
            (_, EdgeStep::Selecting) if !self.selected_edges.is_empty() => {
                self.step = EdgeStep::PreviewConfirm;
                self.rebuild_preview();
                CmdResult::NeedPoint
            }
            (_, EdgeStep::LoopConfirm) => self.accept_loop(),
            (_, EdgeStep::PreviewConfirm) => self.finish(),
            (_, EdgeStep::Radius) => self
                .accept_radius(self.default_value)
                .unwrap_or(CmdResult::NeedPoint),
            (_, EdgeStep::Expression) => CmdResult::NeedPoint,
            _ => CmdResult::Cancel,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.step == EdgeStep::LoopConfirm {
            self.loop_candidates.clear();
            self.loop_index = 0;
            self.step = EdgeStep::Selecting;
            self.rebuild_preview();
            return Some(CmdResult::NeedPoint);
        }
        let (batch, _) = self.selection_batches.pop()?;
        for edge in batch {
            if let Some(index) = self.selected_edges.iter().position(|candidate| *candidate == edge) {
                self.selected_edges.remove(index);
            }
        }
        if self.selected_edges.is_empty() {
            self.handle = None;
            self.base_face = None;
        }
        self.last_pick = self
            .selection_batches
            .last()
            .map(|(_, anchor)| *anchor);
        self.step = EdgeStep::Selecting;
        self.rebuild_preview();
        Some(CmdResult::NeedPoint)
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            EdgeStep::Radius
                | EdgeStep::Expression
                | EdgeStep::ChamferDistance1
                | EdgeStep::ChamferDistance2
        )
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(
            self.step,
            EdgeStep::Radius | EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2
        )
    }

    fn dyn_spec(&self) -> Option<DynSpec> {
        let pick = self.last_pick?;
        matches!(
            self.step,
            EdgeStep::Radius | EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2
        )
        .then(|| DynSpec {
            anchor: DynAnchor::Point(pick),
            fields: vec![DynFieldSpec::new(match self.operation {
                EdgeOperation::Fillet => DynRole::Radius,
                EdgeOperation::Chamfer => DynRole::Distance,
            })],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        matches!(
            self.step,
            EdgeStep::Radius | EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2
        )
            .then(|| self.last_pick.map(|pick| cursor.distance(pick)))
            .flatten()
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        if matches!(
            self.step,
            EdgeStep::Radius | EdgeStep::ChamferDistance1 | EdgeStep::ChamferDistance2
        ) {
            if let Some(value) = self.last_pick.map(|pick| cursor.distance(pick)) {
                if value > 0.0 && value.is_finite() {
                    let values = match self.step {
                        EdgeStep::ChamferDistance2 => (self.default_value, value),
                        _ => (value, self.other_value),
                    };
                    if let Some((handle, wires)) = self.preview_for_values(values.0, values.1) {
                        self.preview_wires = wires;
                        self.preview_hidden.clear();
                        self.preview_hidden.push(handle);
                    } else {
                        self.preview_wires.clear();
                        self.preview_hidden.clear();
                    }
                }
            }
        }
        self.preview_wires.clone()
    }

    fn preview_hidden_handles(&self) -> &[Handle] {
        &self.preview_hidden
    }
}

fn edge_loops(body: &Body, seed: EdgeKey) -> Vec<(FaceKey, Vec<EdgeKey>)> {
    let Some(edge) = body.edges.get(seed) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for coedge_key in &edge.coedges {
        let Some(coedge) = body.coedges.get(*coedge_key) else {
            continue;
        };
        let Some(edge_loop) = body.loops.get(coedge.owner) else {
            continue;
        };
        let edges = edge_loop
            .coedges
            .iter()
            .filter_map(|key| body.coedges.get(*key).map(|coedge| coedge.edge))
            .collect::<Vec<_>>();
        let candidate = (edge_loop.owner, edges);
        if !candidate.1.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn edge_belongs_to_face(body: &Body, edge: EdgeKey, face: FaceKey) -> bool {
    body.edges.get(edge).is_some_and(|edge| {
        edge.coedges.iter().any(|coedge| {
            body.coedges
                .get(*coedge)
                .and_then(|coedge| body.loops.get(coedge.owner))
                .is_some_and(|edge_loop| edge_loop.owner == face)
        })
    })
}

fn nearest_edge_face(body: &Body, edge: EdgeKey, point: [f64; 3]) -> Option<FaceKey> {
    let edge = body.edges.get(edge)?;
    edge.coedges
        .iter()
        .filter_map(|coedge| {
            let edge_loop = body.loops.get(body.coedges.get(*coedge)?.owner)?;
            let face = body.faces.get(edge_loop.owner)?;
            let surface = body.surfaces.get(face.surface)?;
            let distance = surface.distance_to(point).abs();
            distance.is_finite().then_some((edge_loop.owner, distance))
        })
        .min_by(|first, second| first.1.total_cmp(&second.1))
        .map(|(face, _)| face)
}

fn positive_default(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

fn edge_chain(body: &Body, seed: EdgeKey) -> Vec<EdgeKey> {
    let mut chain = vec![seed];
    let mut next = 0;
    while next < chain.len() {
        let current_key = chain[next];
        next += 1;
        let Some(current) = body.edges.get(current_key) else {
            continue;
        };
        for vertex in [current.start, current.end] {
            let Some(current_tangent) = edge_tangent_from_vertex(body, current_key, vertex) else {
                continue;
            };
            for candidate_key in body.edge_keys() {
                if chain.contains(&candidate_key) {
                    continue;
                }
                let Some(candidate) = body.edges.get(candidate_key) else {
                    continue;
                };
                if candidate.start != vertex && candidate.end != vertex {
                    continue;
                }
                let Some(candidate_tangent) =
                    edge_tangent_from_vertex(body, candidate_key, vertex)
                else {
                    continue;
                };
                if current_tangent.dot(candidate_tangent) < -0.999 {
                    chain.push(candidate_key);
                }
            }
        }
    }
    chain
}

fn edge_tangent_from_vertex(
    body: &Body,
    edge_key: EdgeKey,
    vertex: cadkernel::brep::VertexKey,
) -> Option<DVec3> {
    let edge = body.edges.get(edge_key)?;
    let curve = body.curves.get(edge.curve)?;
    let span = edge.end_parameter - edge.start_parameter;
    if !span.is_finite() || span.abs() <= f64::EPSILON {
        return None;
    }
    let step = span * 1.0e-6;
    let (at, inside) = if edge.start == vertex {
        (edge.start_parameter, edge.start_parameter + step)
    } else if edge.end == vertex {
        (edge.end_parameter, edge.end_parameter - step)
    } else {
        return None;
    };
    let tangent = DVec3::from(curve.point_at(inside)) - DVec3::from(curve.point_at(at));
    (tangent.length_squared() > 0.0 && tangent.is_finite()).then(|| tangent.normalize())
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["FILLETEDGE", "SOLIDFILLET", "CHAMFEREDGE", "SOLIDCHAMFER"]
});
