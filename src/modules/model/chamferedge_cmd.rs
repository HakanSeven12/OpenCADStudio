use acadrust::Handle;
use cadkernel::brep::{Body, EdgeKey, FaceKey};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec,
};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChamferStep {
    Selecting,
    PickingLoop,
    LoopConfirm,
    PreviewConfirm,
    Distance1,
    Distance2,
    Expression,
}

pub struct ChamferEdgeCommand {
    target: Option<Handle>,
    bodies: Vec<(Handle, Body)>,
    handle: Option<Handle>,
    selected_edges: Vec<EdgeKey>,
    base_face: Option<FaceKey>,
    selection_batches: Vec<(Vec<EdgeKey>, DVec3)>,
    step: ChamferStep,
    value_return: ChamferStep,
    expression_return: ChamferStep,
    loop_candidates: Vec<(FaceKey, Vec<EdgeKey>)>,
    loop_index: usize,
    last_pick: Option<DVec3>,
    distance1: f64,
    distance2: f64,
    preview_color: [f32; 4],
    preview_wires: Vec<WireModel>,
    preview_hidden: Vec<Handle>,
}

impl ChamferEdgeCommand {
    pub fn new(
        target: Option<Handle>,
        bodies: Vec<(Handle, Body)>,
        preview_color: [f32; 4],
        distances: (f64, f64),
    ) -> Self {
        Self {
            target,
            bodies,
            handle: None,
            selected_edges: Vec::new(),
            base_face: None,
            selection_batches: Vec::new(),
            step: ChamferStep::Selecting,
            value_return: ChamferStep::Selecting,
            expression_return: ChamferStep::Distance1,
            loop_candidates: Vec::new(),
            loop_index: 0,
            last_pick: None,
            distance1: positive_default(distances.0),
            distance2: positive_default(distances.1),
            preview_color,
            preview_wires: Vec::new(),
            preview_hidden: Vec::new(),
        }
    }

    pub fn distances(&self) -> (f64, f64) {
        (self.distance1, self.distance2)
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

    fn current_loop(&self) -> Option<&(FaceKey, Vec<EdgeKey>)> {
        self.loop_candidates.get(self.loop_index)
    }

    fn preview_edges(&self) -> Vec<EdgeKey> {
        let mut edges = self.selected_edges.clone();
        if self.step == ChamferStep::LoopConfirm {
            if let Some((_, loop_edges)) = self.current_loop() {
                for edge in loop_edges {
                    if !edges.contains(edge) {
                        edges.push(*edge);
                    }
                }
            }
        }
        edges
    }

    fn preview_for_values(
        &self,
        distance1: f64,
        distance2: f64,
    ) -> Option<(Handle, Vec<WireModel>)> {
        let edges = self.preview_edges();
        if edges.is_empty() {
            return None;
        }
        let (handle, body) = self.active_body()?;
        let base_face = self.base_face.or_else(|| self.current_loop().map(|item| item.0))?;
        let result =
            cadkernel::brep::chamfer_edges(body, &edges, base_face, distance1, distance2).ok()?;
        let mut wires = crate::scene::model::solid_model::grip_preview_wires(&result, handle);
        for wire in &mut wires {
            wire.color = self.preview_color;
            wire.name = format!("{}-CHAMFEREDGE-PREVIEW", handle.value());
        }
        (!wires.is_empty()).then_some((handle, wires))
    }

    fn rebuild_preview(&mut self) {
        self.preview_wires.clear();
        self.preview_hidden.clear();
        if let Some((handle, wires)) = self.preview_for_values(self.distance1, self.distance2) {
            self.preview_wires = wires;
            self.preview_hidden.push(handle);
        }
    }

    fn begin_distances(&mut self, return_to: ChamferStep) -> CmdResult {
        self.value_return = return_to;
        self.step = ChamferStep::Distance1;
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn accept_distance(&mut self, value: f64) -> Option<CmdResult> {
        if value <= 0.0 || !value.is_finite() {
            return None;
        }
        match self.step {
            ChamferStep::Distance1 => {
                self.distance1 = value;
                self.step = ChamferStep::Distance2;
            }
            ChamferStep::Distance2 => {
                self.distance2 = value;
                self.step = self.value_return;
            }
            _ => return None,
        }
        self.rebuild_preview();
        Some(CmdResult::NeedPoint)
    }

    fn accept_loop(&mut self) -> CmdResult {
        if let (Some((face, edges)), Some(anchor)) = (self.current_loop().cloned(), self.last_pick) {
            if self.base_face.is_none() {
                self.base_face = Some(face);
            }
            self.add_batch(edges, anchor);
        }
        self.loop_candidates.clear();
        self.loop_index = 0;
        self.step = ChamferStep::Selecting;
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn finish(&self) -> CmdResult {
        let (Some(handle), Some(base_face)) = (self.handle, self.base_face) else {
            return CmdResult::Cancel;
        };
        if self.selected_edges.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::SolidChamferEdges {
            handle,
            edges: self.selected_edges.clone(),
            base_face,
            distance1: self.distance1,
            distance2: self.distance2,
        }
    }
}

impl CadCommand for ChamferEdgeCommand {
    fn name(&self) -> &'static str {
        "CHAMFEREDGE"
    }

    fn prompt(&self) -> String {
        match self.step {
            ChamferStep::Selecting => crate::t!("Select an edge or [Loop/Distance]:").into_owned(),
            ChamferStep::PickingLoop => {
                crate::t!("Select edge of loop or [Edge/Distance]:").into_owned()
            }
            ChamferStep::LoopConfirm => {
                crate::t!("Enter an option [Accept/Next] <Accept>:").into_owned()
            }
            ChamferStep::PreviewConfirm => {
                crate::t!("Press Enter to accept the chamfer or [Distance]:").into_owned()
            }
            ChamferStep::Distance1 => {
                crate::tf!("Specify Distance1 or [Expression] <{:.4}>:", self.distance1)
                    .into_owned()
            }
            ChamferStep::Distance2 => {
                crate::tf!("Specify Distance2 or [Expression] <{:.4}>:", self.distance2)
                    .into_owned()
            }
            ChamferStep::Expression => crate::t!("Enter expression:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            ChamferStep::Selecting => vec![
                CmdOption::new("Loop", "L"),
                CmdOption::new("Distance", "D"),
            ],
            ChamferStep::PickingLoop => vec![
                CmdOption::new("Edge", "E"),
                CmdOption::new("Distance", "D"),
            ],
            ChamferStep::LoopConfirm => vec![
                CmdOption::new("Accept", "A"),
                CmdOption::new("Next", "N"),
            ],
            ChamferStep::PreviewConfirm => vec![CmdOption::new("Distance", "D")],
            ChamferStep::Distance1 | ChamferStep::Distance2 => {
                vec![CmdOption::new("Expression", "E")]
            }
            ChamferStep::Expression => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, ChamferStep::Selecting | ChamferStep::PickingLoop)
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

        if self.step == ChamferStep::PickingLoop {
            let mut candidates = edge_loops(self.body(handle).expect("pick body exists"), seed);
            if let Some(base_face) = self.base_face {
                candidates.retain(|(face, _)| *face == base_face);
            }
            self.loop_candidates = candidates;
            self.loop_index = 0;
            self.step = if self.loop_candidates.is_empty() {
                ChamferStep::Selecting
            } else {
                ChamferStep::LoopConfirm
            };
            self.rebuild_preview();
            return CmdResult::NeedPoint;
        }

        if self.base_face.is_none() {
            self.base_face = self
                .body(handle)
                .and_then(|body| nearest_edge_face(body, seed, point.to_array()));
        }
        let Some(base_face) = self.base_face else {
            return CmdResult::NeedPoint;
        };
        if !edge_belongs_to_face(self.body(handle).expect("pick body exists"), seed, base_face) {
            return CmdResult::NeedPoint;
        }
        self.add_batch(vec![seed], point);
        self.rebuild_preview();
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !matches!(self.step, ChamferStep::Distance1 | ChamferStep::Distance2) {
            return CmdResult::NeedPoint;
        }
        let Some(anchor) = self.last_pick else {
            return CmdResult::NeedPoint;
        };
        self.accept_distance(point.distance(anchor))
            .unwrap_or(CmdResult::NeedPoint)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        match self.step {
            ChamferStep::Selecting => match keyword.as_str() {
                "L" | "LOOP" => {
                    self.step = ChamferStep::PickingLoop;
                    Some(CmdResult::NeedPoint)
                }
                "D" | "DISTANCE" => Some(self.begin_distances(ChamferStep::Selecting)),
                _ => None,
            },
            ChamferStep::PickingLoop => match keyword.as_str() {
                "E" | "EDGE" => {
                    self.step = ChamferStep::Selecting;
                    Some(CmdResult::NeedPoint)
                }
                "D" | "DISTANCE" => Some(self.begin_distances(ChamferStep::PickingLoop)),
                _ => None,
            },
            ChamferStep::LoopConfirm => match keyword.as_str() {
                "A" | "ACCEPT" => Some(self.accept_loop()),
                "N" | "NEXT" if !self.loop_candidates.is_empty() => {
                    self.loop_index = (self.loop_index + 1) % self.loop_candidates.len();
                    self.rebuild_preview();
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            ChamferStep::PreviewConfirm => match keyword.as_str() {
                "D" | "DISTANCE" => Some(self.begin_distances(ChamferStep::PreviewConfirm)),
                _ => None,
            },
            ChamferStep::Distance1 | ChamferStep::Distance2 => {
                if matches!(keyword.as_str(), "E" | "EXPRESSION") {
                    self.expression_return = self.step;
                    self.step = ChamferStep::Expression;
                    Some(CmdResult::NeedPoint)
                } else {
                    self.accept_distance(keyword.parse::<f64>().ok()?)
                }
            }
            ChamferStep::Expression => {
                let value = crate::app::expr_eval::eval_number(text)?;
                self.step = self.expression_return;
                self.accept_distance(value)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            ChamferStep::Selecting if !self.selected_edges.is_empty() => {
                self.step = ChamferStep::PreviewConfirm;
                self.rebuild_preview();
                CmdResult::NeedPoint
            }
            ChamferStep::LoopConfirm => self.accept_loop(),
            ChamferStep::PreviewConfirm => self.finish(),
            ChamferStep::Distance1 => self
                .accept_distance(self.distance1)
                .unwrap_or(CmdResult::NeedPoint),
            ChamferStep::Distance2 => self
                .accept_distance(self.distance2)
                .unwrap_or(CmdResult::NeedPoint),
            ChamferStep::Expression => CmdResult::NeedPoint,
            _ => CmdResult::Cancel,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.step == ChamferStep::LoopConfirm {
            self.loop_candidates.clear();
            self.loop_index = 0;
            self.step = ChamferStep::Selecting;
            self.rebuild_preview();
            return Some(CmdResult::NeedPoint);
        }
        let (batch, _) = self.selection_batches.pop()?;
        self.selected_edges.retain(|edge| !batch.contains(edge));
        if self.selected_edges.is_empty() {
            self.handle = None;
            self.base_face = None;
        }
        self.last_pick = self.selection_batches.last().map(|(_, anchor)| *anchor);
        self.step = ChamferStep::Selecting;
        self.rebuild_preview();
        Some(CmdResult::NeedPoint)
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            ChamferStep::Distance1 | ChamferStep::Distance2 | ChamferStep::Expression
        )
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.step, ChamferStep::Distance1 | ChamferStep::Distance2)
    }

    fn dyn_spec(&self) -> Option<DynSpec> {
        let pick = self.last_pick?;
        matches!(self.step, ChamferStep::Distance1 | ChamferStep::Distance2).then(|| DynSpec {
            anchor: DynAnchor::Point(pick),
            fields: vec![DynFieldSpec::new(DynRole::Distance)],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        matches!(self.step, ChamferStep::Distance1 | ChamferStep::Distance2)
            .then(|| self.last_pick.map(|pick| cursor.distance(pick)))
            .flatten()
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        if matches!(self.step, ChamferStep::Distance1 | ChamferStep::Distance2) {
            if let Some(value) = self.last_pick.map(|pick| cursor.distance(pick)) {
                if value > 0.0 && value.is_finite() {
                    let values = if self.step == ChamferStep::Distance2 {
                        (self.distance1, value)
                    } else {
                        (value, self.distance2)
                    };
                    if let Some((handle, wires)) = self.preview_for_values(values.0, values.1) {
                        self.preview_wires = wires;
                        self.preview_hidden = vec![handle];
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

inventory::submit!(crate::command::CommandRegistration {
    names: &["CHAMFEREDGE"]
});
