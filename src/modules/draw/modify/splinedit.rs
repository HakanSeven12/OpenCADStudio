// SPLINEDIT command — interactive spline editing.
//
// Phase 1: select a spline (entity pick)
// Phase 2: choose a sub-command:
//   CLOSE  — set the spline as closed (wrap last control point to first)
//   OPEN   — remove the closure
//   REVERSE— reverse the control point order
//   EXIT   — done (Enter / Escape)
//
// Control-point dragging is already supported via the grip editing system.

use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;


use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SPLINEDIT",
        label: "Spline Edit",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/spline.svg")),
        event: ModuleEvent::Command("SPLINEDIT".to_string()),
    }
}

#[derive(Clone, Copy)]
enum Step {
    SelectSpline,
    Options,
    Refine,
    Add,
    Delete,
    Elevate,
    Move { index: usize, refine: bool },
    Weight { index: usize },
    SelectVertex { weight: bool, refine: bool },
}

pub struct SplineditCommand {
    step: Step,
    handle: acadrust::Handle,
    spline: Option<acadrust::entities::Spline>,
    pending: Option<acadrust::entities::Spline>,
    history: Vec<(acadrust::Handle, acadrust::entities::Spline)>,
}

impl SplineditCommand {
    pub fn new() -> Self {
        Self { step: Step::SelectSpline, handle: acadrust::Handle::NULL, spline: None, pending: None, history: Vec::new() }
    }

    fn replace(&mut self, spline: acadrust::entities::Spline) -> CmdResult {
        self.pending = Some(spline.clone());
        CmdResult::ReplaceManyContinue(vec![(self.handle, vec![EntityType::Spline(spline)])])
    }

    fn refined(&self, point: Option<DVec3>, degree: Option<usize>) -> Option<acadrust::entities::Spline> {
        let source = self.spline.as_ref()?;
        let planar = crate::entities::curve::entity_curve(&EntityType::Spline(source.clone()))?;
        let cadkernel::geom2d::Curve::Nurbs(mut curve) = planar.curve else { return None; };
        if let Some(point) = point {
            let projected = planar.plane.project([point.x, point.y, point.z])?;
            let nearest = cadkernel::geom2d::closest_point(&cadkernel::geom2d::Curve::Nurbs(curve.clone()), projected);
            let (start, end) = curve.domain();
            let parameter = start + nearest.t * (end - start);
            if parameter <= start || parameter >= end { return None; }
            curve.insert_knot(parameter);
        }
        if let Some(degree) = degree {
            if degree <= curve.degree() || degree > 26 { return None; }
            curve = curve.elevated(degree - curve.degree())?;
        }
        let mut result = crate::modules::draw::modify::spline_ops::nurbs_to_spline(&curve, source);
        result.control_points = curve.control_points().iter().map(|point| {
            let world = planar.plane.point_at(*point);
            Vector3::new(world[0], world[1], world[2])
        }).collect();
        Some(result)
    }
}

impl CadCommand for SplineditCommand {
    fn name(&self) -> &'static str { "SPLINEDIT" }
    fn prompt(&self) -> String {
        match self.step {
            Step::SelectSpline => crate::t!("SPLINEDIT  Select spline:").into_owned(),
            Step::Options => crate::t!("SPLINEDIT  [Close/Open/Move vertex/Refine/rEverse/Undo/eXit] <eXit>:").into_owned(),
            Step::Refine => crate::t!("SPLINEDIT  [Add/Delete/Elevate order/Move/Weight/eXit] <eXit>:").into_owned(),
            Step::Add => crate::t!("SPLINEDIT  Specify a point on the spline <exit>:").into_owned(),
            Step::Delete => crate::t!("SPLINEDIT  Specify control vertex to delete:").into_owned(),
            Step::Elevate => format!("SPLINEDIT  Enter new order <{}>:", self.spline.as_ref().map_or(4, |s| s.degree + 1)),
            Step::Move { index, .. } => format!("SPLINEDIT  Vertex {}: specify new location or [Next/Previous/Select point/eXit] <Next>:", index + 1),
            Step::SelectVertex { .. } => crate::t!("SPLINEDIT  Specify control vertex:").into_owned(),
            Step::Weight { index } => format!("SPLINEDIT  Vertex {}: enter new weight or [Next/Previous/Select point/eXit] <Next>:", index + 1),
        }
    }
    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            Step::Options => vec![CmdOption::new("Close", "C"), CmdOption::new("Open", "O"), CmdOption::new("Move vertex", "M"), CmdOption::new("Refine", "R"), CmdOption::new("Reverse", "E"), CmdOption::new("Undo", "U"), CmdOption::new("Exit", "X")],
            Step::Refine => vec![CmdOption::new("Add", "A"), CmdOption::new("Delete", "D"), CmdOption::new("Elevate order", "E"), CmdOption::new("Move", "M"), CmdOption::new("Weight", "W"), CmdOption::new("Exit", "X")],
            Step::Move { .. } | Step::Weight { .. } => vec![CmdOption::new("Next", "N"), CmdOption::new("Previous", "P"), CmdOption::new("Select point", "S"), CmdOption::new("Exit", "X")],
            _ => Vec::new(),
        }
    }
    fn needs_entity_pick(&self) -> bool { matches!(self.step, Step::SelectSpline) }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.spline = match entity { EntityType::Spline(spline) => Some(spline), _ => None };
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() || self.spline.is_none() { return CmdResult::NeedPoint; }
        self.handle = handle;
        self.step = Step::Options;
        CmdResult::NeedPoint
    }
    fn on_entity_replaced(&mut self, old: acadrust::Handle, new: &[acadrust::Handle]) {
        if old == self.handle {
            if let (Some(&handle), Some(replacement)) = (new.first(), self.pending.take()) {
                if let Some(previous) = self.spline.replace(replacement) { self.history.push((old, previous)); }
                self.handle = handle;
            }
        }
    }
    fn wants_text_input(&self) -> bool { !matches!(self.step, Step::SelectSpline | Step::Add | Step::Delete | Step::Move { .. } | Step::SelectVertex { .. }) }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        if upper.is_empty() { return Some(self.on_enter()); }
        match self.step {
            Step::Options => match upper.as_str() {
                "R" | "REFINE" => self.step = Step::Refine,
                "M" | "MOVE" => self.step = Step::Move { index: 0, refine: false },
                "X" | "EXIT" => return Some(CmdResult::Cancel),
                "U" | "UNDO" => {
                    if let Some((handle, spline)) = self.history.pop() {
                        self.handle = handle;
                        self.spline = Some(spline);
                        return Some(CmdResult::UndoDocument);
                    }
                }
                "C" | "CLOSE" | "O" | "OPEN" | "E" | "REVERSE" | "REV" => {
                    let mut spline = self.spline.clone()?;
                    let op = match upper.as_str() { "C" | "CLOSE" => "__SPLINEDIT_CLOSE__", "O" | "OPEN" => "__SPLINEDIT_OPEN__", _ => "__SPLINEDIT_REVERSE__" };
                    apply_to_spline(&mut spline, op);
                    return Some(self.replace(spline));
                }
                _ => {}
            },
            Step::Refine => match upper.as_str() {
                "A" | "ADD" => self.step = Step::Add,
                "D" | "DELETE" => self.step = Step::Delete,
                "E" | "ELEVATE" => self.step = Step::Elevate,
                "M" | "MOVE" => self.step = Step::Move { index: 0, refine: true },
                "W" | "WEIGHT" => self.step = Step::Weight { index: 0 },
                "X" | "EXIT" => self.step = Step::Options,
                _ => {}
            },
            Step::Elevate => {
                if let Some(spline) = upper.parse::<usize>().ok().and_then(|degree| self.refined(None, Some(degree))) {
                    self.step = Step::Refine;
                    return Some(self.replace(spline));
                }
            }
            Step::Move { index, .. } | Step::Weight { index } => {
                let count = self.spline.as_ref().map_or(0, |s| s.control_points.len());
                if count == 0 { return Some(CmdResult::NeedPoint); }
                let next = match upper.as_str() { "N" | "NEXT" => Some((index + 1) % count), "P" | "PREVIOUS" => Some((index + count - 1) % count), _ => None };
                if let Some(next) = next {
                    self.step = match self.step { Step::Move { refine, .. } => Step::Move { index: next, refine }, _ => Step::Weight { index: next } };
                } else if matches!(upper.as_str(), "S" | "SELECT") {
                    self.step = match self.step {
                        Step::Move { refine, .. } => Step::SelectVertex { weight: false, refine },
                        _ => Step::SelectVertex { weight: true, refine: true },
                    };
                } else if matches!(upper.as_str(), "X" | "EXIT") {
                    self.step = match self.step { Step::Move { refine: false, .. } => Step::Options, _ => Step::Refine };
                } else if matches!(self.step, Step::Weight { .. }) {
                    if let Some(weight) = upper.replace(',', ".").parse::<f64>().ok().filter(|weight| weight.is_finite() && *weight > 0.0) {
                        let mut spline = self.spline.clone()?;
                        spline.weights.resize(count, 1.0);
                        spline.weights[index] = weight;
                        spline.flags.rational = true;
                        spline.fit_points.clear();
                        return Some(self.replace(spline));
                    }
                }
            }
            _ => {}
        }
        Some(CmdResult::NeedPoint)
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !point.is_finite() { return CmdResult::NeedPoint; }
        match self.step {
            Step::SelectVertex { weight, refine } => {
                if let Some(index) = self.spline.as_ref().and_then(|spline| spline.control_points.iter().enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let distance = |p: &Vector3| point.distance_squared(DVec3::new(p.x, p.y, p.z));
                        distance(a).total_cmp(&distance(b))
                    }).map(|(index, _)| index)) {
                    self.step = if weight { Step::Weight { index } } else { Step::Move { index, refine } };
                }
                CmdResult::NeedPoint
            }
            Step::Delete => {
                let Some(source) = self.spline.as_ref() else { return CmdResult::NeedPoint; };
                let Some(index) = source.control_points.iter().enumerate().min_by(|(_, a), (_, b)| {
                    let distance = |p: &Vector3| point.distance_squared(DVec3::new(p.x, p.y, p.z));
                    distance(a).total_cmp(&distance(b))
                }).map(|(index, _)| index) else { return CmdResult::NeedPoint; };
                let controls = source.control_points.iter().map(|point| [point.x, point.y, point.z]).collect();
                let weights = if source.weights.is_empty() { vec![1.0; source.control_points.len()] }
                    else { source.weights.clone() };
                let curve = cadkernel::space::NurbsCurve3::new_strict(source.degree as usize, controls,
                    source.knots.clone(), weights).map(|curve| curve.with_periodicity(source.flags.periodic || source.flags.closed));
                let Some(curve) = curve.and_then(|curve| curve.without_control_vertex(index)) else { return CmdResult::NeedPoint; };
                let mut spline = source.clone();
                spline.degree = curve.degree() as i32;
                spline.control_points = curve.control_points().iter().map(|point| Vector3::new(point[0], point[1], point[2])).collect();
                spline.weights = curve.weights().to_vec();
                spline.knots = curve.knots().to_vec();
                spline.fit_points.clear();
                spline.flags.planar = crate::entities::curve::spline_is_planar(&spline);
                self.replace(spline)
            }
            Step::Add => self.refined(Some(point), None).map_or(CmdResult::NeedPoint, |spline| self.replace(spline)),
            Step::Move { index, .. } => {
                let Some(mut spline) = self.spline.clone() else { return CmdResult::NeedPoint; };
                let Some(vertex) = spline.control_points.get_mut(index) else { return CmdResult::NeedPoint; };
                *vertex = Vector3::new(point.x, point.y, point.z);
                spline.fit_points.clear();
                self.replace(spline)
            }
            _ => CmdResult::NeedPoint,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::SelectSpline | Step::Options => CmdResult::Cancel,
            Step::Refine => { self.step = Step::Options; CmdResult::NeedPoint }
            Step::Add | Step::Delete | Step::SelectVertex { .. } => { self.step = Step::Refine; CmdResult::NeedPoint }
            Step::Elevate => {
                let degree = self.spline.as_ref().map_or(4, |s| s.degree as usize + 1);
                self.on_text_input(&degree.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            Step::Move { .. } | Step::Weight { .. } => self.on_text_input("N").unwrap_or(CmdResult::NeedPoint),
        }
    }
    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> { vec![] }
}
/// Apply a spline operation (CLOSE/OPEN/REVERSE) to a spline entity.
/// Called from `cmd_result.rs` when the ReplaceEntity sentinel is detected.
pub fn apply_spline_op(doc: &mut acadrust::CadDocument, handle: acadrust::Handle, op: &str) {
    let Some(EntityType::Spline(spline)) = doc.get_entity_mut(handle) else {
        return;
    };
    apply_to_spline(spline, op);
}

fn apply_to_spline(spline: &mut acadrust::entities::Spline, op: &str) {
    match op {
        "__SPLINEDIT_CLOSE__" => {
            if spline.control_points.len() >= 2 {
                let first = spline.control_points[0];
                spline.control_points.push(first);
                // Regenerate clamped knots.
                spline.knots = acadrust::entities::Spline::generate_clamped_knots(
                    spline.degree as usize,
                    spline.control_points.len(),
                );
                spline.fit_points.clear();
            }
        }
        "__SPLINEDIT_OPEN__" => {
            if spline.control_points.len() >= 2 {
                let n = spline.control_points.len();
                let first = spline.control_points[0];
                let last = spline.control_points[n - 1];
                if (first.x - last.x).abs() < 1e-9
                    && (first.y - last.y).abs() < 1e-9
                    && (first.z - last.z).abs() < 1e-9
                {
                    spline.control_points.pop();
                    spline.knots = acadrust::entities::Spline::generate_clamped_knots(
                        spline.degree as usize,
                        spline.control_points.len(),
                    );
                    spline.fit_points.clear();
                }
            }
        }
        "__SPLINEDIT_REVERSE__" => {
            *spline = super::reverse::reverse_spline(spline);
        }
        _ => {}
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["SPLINEDIT"] });  // SplineditCommand
