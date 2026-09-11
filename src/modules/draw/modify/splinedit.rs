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
    Join,
    PolylinePrecision,
    FitOptions,
    FitMove { index: usize },
    FitSelectMove,
    FitDelete,
    FitAddPick,
    FitAddNew { index: usize, first: bool },
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
    pick_context: Option<crate::command::PointPickContext>,
    join_candidates: Vec<crate::command::SelectionEntity>,
    delete_source: bool,
}

impl SplineditCommand {
    pub fn new() -> Self {
        Self { step: Step::SelectSpline, handle: acadrust::Handle::NULL, spline: None, pending: None, history: Vec::new(), pick_context: None, join_candidates: Vec::new(), delete_source: true }
    }

    pub fn with_delete_source(mut self, delete: bool) -> Self { self.delete_source = delete; self }

    fn convert_polyline(&mut self, precision: u8) -> CmdResult {
        let result = self.spline.as_ref().and_then(|source| {
            let curve = spatial_spline(source)?;
            let approximation = curve.to_polyline_precision(precision)?;
            let mut points = approximation.points;
            if self.closed() {
                if points.first() != points.last() { return None; }
                points.pop();
            }
            if points.len() < 2 { return None; }
            let entity = if let Some(planar) = crate::entities::curve::entity_curve(&EntityType::Spline(source.clone())) {
                let normal = planar.plane.normal()?;
                let elevation = cadkernel::space::Vec3::from(points[0]).dot(cadkernel::space::Vec3::from(normal));
                let normal = Vector3::new(normal[0], normal[1], normal[2]);
                let plane = crate::entities::curve::ocs_plane(normal.clone(), elevation);
                let mut polyline = acadrust::LwPolyline::new();
                polyline.common = source.common.clone();
                polyline.elevation = elevation; polyline.normal = normal; polyline.is_closed = self.closed();
                polyline.vertices = points.iter().map(|point| {
                    let uv = plane.project(*point)?;
                    Some(acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(uv[0], uv[1])))
                }).collect::<Option<Vec<_>>>()?;
                EntityType::LwPolyline(polyline)
            } else {
                let mut polyline = acadrust::entities::Polyline3D::from_points(points.iter().map(|p| Vector3::new(p[0],p[1],p[2])).collect());
                polyline.common = source.common.clone(); polyline.flags.closed = self.closed();
                EntityType::Polyline3D(polyline)
            };
            Some(entity)
        });
        let Some(mut entity) = result else { return CmdResult::ReportError(crate::t!("Spline cannot be converted within the requested precision.").into_owned()); };
        if self.delete_source { CmdResult::ReplaceMany(vec![(self.handle,vec![entity])], Vec::new()) }
        else { entity.common_mut().handle = acadrust::Handle::NULL; CmdResult::ReplaceMany(Vec::new(), vec![entity]) }
    }

    fn picked_vertex(&self, point: DVec3) -> Option<usize> { self.picked_from_points(point, &self.spline.as_ref()?.control_points) }
    fn picked_fit_point(&self, point: DVec3) -> Option<usize> { self.picked_from_points(point, &self.spline.as_ref()?.fit_points) }
    fn has_fit_data(&self) -> bool { self.spline.as_ref().is_some_and(|s| s.fit_points.len() >= 3) }

    fn replace_fit_points(&mut self, points: Vec<Vector3>) -> CmdResult {
        let Some(source) = self.spline.as_ref() else { return CmdResult::NeedPoint; };
        if points == source.fit_points { return CmdResult::NeedPoint; }
        if source.fit_tolerance != 0.0 || source.weights.windows(2).any(|w| w[0] != w[1]) {
            return CmdResult::ReportError(crate::t!("Editing weighted or tolerance-fitted interpolation data is not supported.").into_owned());
        }
        let mut result = source.clone();
        result.fit_points = points; result.control_points.clear(); result.knots.clear(); result.weights.clear();
        let Some(curve) = spatial_spline(&result).and_then(|curve| curve.compact_knots(source.control_tolerance.max(1e-9))) else {
            return CmdResult::ReportError(crate::t!("Fit points do not define a valid spline.").into_owned());
        };
        result.degree = curve.degree() as i32;
        result.control_points = curve.control_points().iter().map(|p| Vector3::new(p[0],p[1],p[2])).collect();
        result.knots = curve.knots().to_vec(); result.weights = curve.weights().to_vec();
        result.dwg_flags1 |= 1; result.dxf_flags |= 32 | 1024;
        result.flags.rational = false;
        result.flags.planar = crate::entities::curve::spline_is_planar(&result);
        self.replace(result)
    }

    fn picked_from_points(&self, point: DVec3, points: &[Vector3]) -> Option<usize> {
        let context = self.pick_context?;
        let project = |point: DVec3| {
            let clip = context.view * (point - context.eye).as_vec3().extend(1.0);
            if !clip.is_finite() || clip.w <= 0.0 { return None; }
            let screen = crate::scene::pick::hit_test::world_to_screen(
                point, context.view, context.eye, context.bounds);
            (screen.x.is_finite() && screen.y.is_finite()
                && screen.x >= 0.0 && screen.x <= context.bounds.width
                && screen.y >= 0.0 && screen.y <= context.bounds.height).then_some(screen)
        };
        let cursor = project(point)?;
        points.iter().enumerate().filter_map(|(index, vertex)| {
            let screen = project(DVec3::new(vertex.x, vertex.y, vertex.z))?;
            let distance = (screen.x - cursor.x).hypot(screen.y - cursor.y);
            (distance.is_finite() && distance <= context.aperture_px).then_some((index, distance))
        }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
    }

    fn closed(&self) -> bool {
        self.spline.as_ref().is_some_and(|spline| spline.flags.closed || spline.flags.periodic)
    }

    fn replace(&mut self, spline: acadrust::entities::Spline) -> CmdResult {
        self.pending = Some(spline.clone());
        CmdResult::ReplaceManyContinue(vec![(self.handle, vec![EntityType::Spline(spline)])])
    }

    fn finish_join(&mut self) -> CmdResult {
        self.step = Step::Options;
        let candidates = std::mem::take(&mut self.join_candidates);
        let Some(source) = self.spline.as_ref() else { return CmdResult::NeedPoint; };
        let selected: Vec<_> = candidates.iter().filter(|item| item.handle != self.handle)
            .map(|item| (item.handle, &item.entity)).collect();
        let Some((EntityType::Spline(mut spline), consumed)) = super::join::join_to_source(
            &EntityType::Spline(source.clone()), &selected) else { return CmdResult::NeedPoint; };
        spline.dwg_flags1 &= !1;
        spline.dxf_flags &= !(32 | 1024);
        self.pending = Some(spline.clone());
        let mut replacements = vec![(self.handle, vec![EntityType::Spline(spline)])];
        replacements.extend(consumed.into_iter().map(|handle| (handle, Vec::new())));
        // The host stores one document snapshot, including every consumed curve.
        // The existing command-local Undo restores that snapshot and the source cache.
        CmdResult::ReplaceManyContinue(replacements)
    }

    fn refined(&self, point: Option<DVec3>, degree: Option<usize>) -> Option<acadrust::entities::Spline> {
        let source = self.spline.as_ref()?;
        if let Some(degree) = degree {
            let current = usize::try_from(source.degree).ok()?;
            if degree <= current || degree > 25 { return None; }
            let curve = spatial_spline(source)?;
            let curve = curve.with_periodicity(source.flags.closed || source.flags.periodic)
                .elevated(degree - current)?
                .compact_knots(source.control_tolerance.max(1e-9))?;
            let mut result = source.clone();
            result.degree = curve.degree() as i32;
            result.control_points = curve.control_points().iter().map(|point| Vector3::new(point[0], point[1], point[2])).collect();
            result.knots = curve.knots().to_vec();
            result.weights = curve.weights().to_vec();
            result.fit_points.clear();
            result.begin_tangent = Vector3::ZERO;
            result.end_tangent = Vector3::ZERO;
            result.dwg_flags1 &= !1;
            result.dxf_flags &= !(32 | 1024);
            result.flags.rational = curve.is_rational();
            result.flags.planar = crate::entities::curve::spline_is_planar(&result);
            return Some(result);
        }
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
            Step::FitOptions => crate::t!("SPLINEDIT  Fit data [Add/Delete/Move/eXit] <eXit>:").into_owned(),
            Step::FitMove { .. } => crate::t!("SPLINEDIT  Specify new location or [Next/Previous/Select point/eXit] <Next>:").into_owned(),
            Step::FitSelectMove | Step::FitDelete | Step::FitAddPick => crate::t!("SPLINEDIT  Specify existing fit point on spline <exit>:").into_owned(),
            Step::FitAddNew { first: true, .. } => crate::t!("SPLINEDIT  Specify new point or [After/Before] <exit>:").into_owned(),
            Step::FitAddNew { .. } => crate::t!("SPLINEDIT  Specify new fit point to add <exit>:").into_owned(),
            Step::Options if self.has_fit_data() && self.closed() => crate::t!("SPLINEDIT  [Fit data/Open/Move vertex/Refine/rEverse/convert to Polyline/Undo/eXit] <eXit>:").into_owned(),
            Step::Options if self.has_fit_data() => crate::t!("SPLINEDIT  [Fit data/Close/Join/Move vertex/Refine/rEverse/convert to Polyline/Undo/eXit] <eXit>:").into_owned(),
            Step::SelectSpline => crate::t!("SPLINEDIT  Select spline:").into_owned(),
            Step::Options if self.closed() => crate::t!("SPLINEDIT  [Open/Move vertex/Refine/rEverse/convert to Polyline/Undo/eXit] <eXit>:").into_owned(),
            Step::Options => crate::t!("SPLINEDIT  [Close/Join/Move vertex/Refine/rEverse/convert to Polyline/Undo/eXit] <eXit>:").into_owned(),
            Step::PolylinePrecision => crate::t!("SPLINEDIT  Specify precision 0-99 <10> (straight segments):").into_owned(),
            Step::Join => crate::t!("SPLINEDIT  Select any open curves to join to source:").into_owned(),
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
            Step::FitOptions => vec![CmdOption::new("Add", "A"), CmdOption::new("Delete", "D"), CmdOption::new("Move", "M"), CmdOption::new("Exit", "X")],
            Step::FitMove { .. } => vec![CmdOption::new("Next", "N"), CmdOption::new("Previous", "P"), CmdOption::new("Select point", "S"), CmdOption::new("Exit", "X")],
            Step::FitAddNew { first: true, .. } => vec![CmdOption::new("After", "A"), CmdOption::new("Before", "B")],
            Step::Options => {
                let mut options = vec![if self.closed() { CmdOption::new("Open", "O") } else { CmdOption::new("Close", "C") }];
                if self.has_fit_data() { options.insert(0, CmdOption::new("Fit data", "F")); }
                if !self.closed() { options.push(CmdOption::new("Join", "J")); }
                options.extend([CmdOption::new("Move vertex", "M"), CmdOption::new("Refine", "R"), CmdOption::new("Reverse", "E"), CmdOption::new("Polyline (lines)", "P"), CmdOption::new("Undo", "U"), CmdOption::new("Exit", "X")]);
                options
            },
            Step::Refine => vec![CmdOption::new("Add", "A"), CmdOption::new("Delete", "D"), CmdOption::new("Elevate order", "E"), CmdOption::new("Move", "M"), CmdOption::new("Weight", "W"), CmdOption::new("Exit", "X")],
            Step::Move { .. } | Step::Weight { .. } => vec![CmdOption::new("Next", "N"), CmdOption::new("Previous", "P"), CmdOption::new("Select point", "S"), CmdOption::new("Exit", "X")],
            _ => Vec::new(),
        }
    }
    fn is_selection_gathering(&self) -> bool { matches!(self.step, Step::Join) }
    fn selection_entities_exclude_locked(&self) -> bool { matches!(self.step, Step::Join) }
    fn inject_selection_entities(&mut self, entities: Vec<crate::command::SelectionEntity>) {
        if matches!(self.step, Step::Join) { self.join_candidates = entities; }
    }
    fn on_selection_complete(&mut self, handles: Vec<acadrust::Handle>) -> CmdResult {
        self.join_candidates.retain(|item| handles.contains(&item.handle) && item.handle != self.handle);
        CmdResult::NeedPoint
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
    fn wants_text_input(&self) -> bool { !matches!(self.step, Step::SelectSpline | Step::Join | Step::Add | Step::Delete | Step::Move { .. } | Step::SelectVertex { .. } | Step::FitMove { .. } | Step::FitSelectMove | Step::FitDelete | Step::FitAddPick | Step::FitAddNew { .. }) }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        if upper.is_empty() { return Some(self.on_enter()); }
        match self.step {
            Step::Options => match upper.as_str() {
                "F" | "FIT" if self.has_fit_data() => self.step = Step::FitOptions,
                "P" | "POLYLINE" => self.step = Step::PolylinePrecision,
                "J" | "JOIN" if !self.closed() => { self.join_candidates.clear(); self.step = Step::Join; },
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
                    if (matches!(upper.as_str(), "C" | "CLOSE") && self.closed())
                        || (matches!(upper.as_str(), "O" | "OPEN") && !self.closed()) {
                        return Some(CmdResult::NeedPoint);
                    }
                    let mut spline = self.spline.clone()?;
                    let op = match upper.as_str() { "C" | "CLOSE" => "__SPLINEDIT_CLOSE__", "O" | "OPEN" => "__SPLINEDIT_OPEN__", _ => "__SPLINEDIT_REVERSE__" };
                    apply_to_spline(&mut spline, op);
                    if self.spline.as_ref() == Some(&spline) { return Some(CmdResult::NeedPoint); }
                    return Some(self.replace(spline));
                }
                _ => {}
            },
            Step::FitOptions => match upper.as_str() {
                "A" | "ADD" => self.step = Step::FitAddPick,
                "D" | "DELETE" => self.step = Step::FitDelete,
                "M" | "MOVE" => self.step = Step::FitMove { index: 0 },
                "X" | "EXIT" => self.step = Step::Options,
                _ => {}
            },
            Step::FitMove { index } => {
                let count = self.spline.as_ref()?.fit_points.len();
                if count == 0 { return Some(CmdResult::NeedPoint); }
                self.step = match upper.as_str() {
                    "N" | "NEXT" => Step::FitMove { index: (index + 1) % count },
                    "P" | "PREVIOUS" => Step::FitMove { index: (index + count - 1) % count },
                    "S" | "SELECT" | "SELECT POINT" => Step::FitSelectMove,
                    "X" | "EXIT" => Step::FitOptions, _ => self.step,
                };
            }
            Step::FitAddNew { first: true, .. } => match upper.as_str() {
                "A" | "AFTER" => self.step = Step::FitAddNew { index: 1, first: false },
                "B" | "BEFORE" => self.step = Step::FitAddNew { index: 0, first: false },
                _ => {}
            },
            Step::PolylinePrecision => {
                let Ok(precision) = upper.parse::<u8>() else { return Some(CmdResult::ReportError(crate::t!("Requires an integer between 0 and 99.").into_owned())); };
                if precision > 99 { return Some(CmdResult::ReportError(crate::t!("Requires an integer between 0 and 99.").into_owned())); }
                return Some(self.convert_polyline(precision));
            }
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
                let current_order = self.spline.as_ref()?.degree.max(1) as usize + 1;
                let order = upper.parse::<usize>().ok().filter(|order| *order >= current_order && *order <= 26)?;
                if order == current_order {
                    self.step = Step::Refine;
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(spline) = self.refined(None, Some(order - 1)) {
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
    fn wants_point_pick_context(&self) -> bool {
        matches!(self.step, Step::Delete | Step::SelectVertex { .. } | Step::FitDelete | Step::FitSelectMove | Step::FitAddPick)
    }
    fn set_point_pick_context(&mut self, context: Option<crate::command::PointPickContext>) {
        self.pick_context = context;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !point.is_finite() { return CmdResult::NeedPoint; }
        match self.step {
            Step::FitSelectMove | Step::FitDelete | Step::FitAddPick => {
                let Some(index) = self.picked_fit_point(point) else { return CmdResult::NeedPoint; };
                match self.step {
                    Step::FitSelectMove => { self.step = Step::FitMove { index }; CmdResult::NeedPoint }
                    Step::FitAddPick => { self.step = Step::FitAddNew { index: index + 1, first: index == 0 }; CmdResult::NeedPoint }
                    _ => {
                        let mut points = self.spline.as_ref().unwrap().fit_points.clone();
                        if points.len() <= 3 { self.step = Step::FitOptions; return CmdResult::ReportError(crate::t!("Cannot delete beyond this.").into_owned()); }
                        points.remove(index); self.replace_fit_points(points)
                    }
                }
            }
            Step::FitMove { index } => {
                let mut points = self.spline.as_ref().unwrap().fit_points.clone();
                let Some(vertex) = points.get_mut(index) else { return CmdResult::NeedPoint; };
                *vertex = Vector3::new(point.x,point.y,point.z); self.replace_fit_points(points)
            }
            Step::FitAddNew { index, .. } => {
                let mut points = self.spline.as_ref().unwrap().fit_points.clone();
                if index > points.len() { return CmdResult::NeedPoint; }
                points.insert(index,Vector3::new(point.x,point.y,point.z));
                let result = self.replace_fit_points(points);
                if matches!(&result, CmdResult::ReplaceManyContinue(_)) { self.step = Step::FitAddNew { index: index + 1, first: false }; }
                result
            }
            Step::SelectVertex { weight, refine } => {
                if let Some(index) = self.picked_vertex(point) {
                    self.step = if weight { Step::Weight { index } } else { Step::Move { index, refine } };
                }
                CmdResult::NeedPoint
            }
            Step::Delete => {
                let Some(source) = self.spline.as_ref() else { return CmdResult::NeedPoint; };
                let Some(index) = self.picked_vertex(point) else { return CmdResult::NeedPoint; };
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
            Step::FitOptions => { self.step = Step::Options; CmdResult::NeedPoint }
            Step::FitDelete | Step::FitAddPick | Step::FitSelectMove => { self.step = Step::FitOptions; CmdResult::NeedPoint }
            Step::FitAddNew { .. } => { self.step = Step::FitAddPick; CmdResult::NeedPoint }
            Step::FitMove { .. } => self.on_text_input("N").unwrap_or(CmdResult::NeedPoint),
            Step::Join => self.finish_join(),
            Step::PolylinePrecision => self.convert_polyline(10),
            Step::Refine => { self.step = Step::Options; CmdResult::NeedPoint }
            Step::Add | Step::Delete | Step::SelectVertex { .. } => { self.step = Step::Refine; CmdResult::NeedPoint }
            Step::Elevate => {
                let order = self.spline.as_ref().map_or(4, |s| s.degree as usize + 1);
                self.on_text_input(&order.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            Step::Move { .. } | Step::Weight { .. } => self.on_text_input("N").unwrap_or(CmdResult::NeedPoint),
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        if matches!(self.step, Step::Join | Step::PolylinePrecision) {
            self.join_candidates.clear();
            self.step = Step::Options;
            CmdResult::NeedPoint
        } else { CmdResult::Cancel }
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
    let result = match op {
        "__SPLINEDIT_CLOSE__" => change_closure(spline, true),
        "__SPLINEDIT_OPEN__" => change_closure(spline, false),
        "__SPLINEDIT_REVERSE__" => Some(super::reverse::reverse_spline(spline)),
        _ => None,
    };
    if let Some(result) = result { *spline = result; }
}

fn change_closure(source: &acadrust::entities::Spline, closed: bool) -> Option<acadrust::entities::Spline> {
    use cadkernel::space::{NurbsCurve3, Parameterization};
    if (source.flags.closed || source.flags.periodic) == closed { return None; }
    let fit_method = !source.fit_points.is_empty() || source.dwg_flags1 & 1 != 0 || source.dxf_flags & 32 != 0;
    let parameterization = match source.knot_parameterization {
        1 => Parameterization::Centripetal, 2 => Parameterization::Uniform, _ => Parameterization::Chord,
    };
    let xyz = |point: &Vector3| [point.x, point.y, point.z];
    let controls: Vec<_> = source.control_points.iter().map(xyz).collect();
    let degree = usize::try_from(source.degree).ok()?;
    let weights = if source.weights.is_empty() { vec![1.0; controls.len()] } else { source.weights.clone() };
    if fit_method && weights.windows(2).any(|pair| pair[0] != pair[1]) { return None; }
    let stored_curve = || NurbsCurve3::new_strict(degree, controls.clone(), source.knots.clone(), weights.clone());
    let mut fit_points = Vec::new();
    let curve = if fit_method {
        fit_points = source.fit_points.iter().map(xyz).collect();
        if fit_points.is_empty() {
            let curve = stored_curve()?;
            let (start, end) = curve.domain();
            let mut parameters: Vec<_> = curve.knots().iter().copied()
                .filter(|parameter| *parameter >= start && if closed { *parameter <= end } else { *parameter < end }).collect();
            parameters.dedup();
            fit_points = parameters.into_iter().map(|parameter| curve.point_at_knot(parameter)).collect();
        }
        let curve = if closed {
            NurbsCurve3::interpolate_periodic(&fit_points, parameterization)?
        } else {
            NurbsCurve3::interpolate_fit(&fit_points, None, None, parameterization)?
        };
        curve.compact_knots(source.control_tolerance.max(1e-9))?
    } else if closed {
        NurbsCurve3::from_weighted_control_polygon(degree, &controls, &weights, true)?
    } else {
        let curve = stored_curve()?;
        curve.without_control_vertex(controls.len().checked_sub(1)?)?
    };
    let mut result = source.clone();
    result.degree = curve.degree() as i32;
    result.control_points = curve.control_points().iter().map(|point| Vector3::new(point[0], point[1], point[2])).collect();
    result.knots = curve.knots().to_vec();
    result.weights = curve.weights().to_vec();
    result.flags.closed = closed;
    result.flags.periodic = closed;
    if closed { result.dxf_flags |= 2048; } else { result.dxf_flags &= !2048; }
    result.flags.rational = if fit_method { false } else { source.flags.rational || curve.is_rational() };
    result.fit_points = if !closed && fit_method {
        fit_points.iter().map(|point| Vector3::new(point[0], point[1], point[2])).collect()
    } else { Vec::new() };
    if fit_method {
        result.dwg_flags1 |= 1;
        result.dxf_flags |= 32 | 1024;
        result.begin_tangent = Vector3::ZERO;
        result.end_tangent = Vector3::ZERO;
    }
    result.flags.planar = crate::entities::curve::spline_is_planar(&result);
    Some(result)
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["SPLINEDIT"] });  // SplineditCommand

fn spatial_spline(source: &acadrust::entities::Spline) -> Option<cadkernel::space::NurbsCurve3> {
    let current = usize::try_from(source.degree).ok()?;
    let weights = if source.weights.is_empty() { vec![1.0; source.control_points.len()] } else { source.weights.clone() };
    let curve = if source.control_points.is_empty() && source.fit_points.len() >= 2 {
        use cadkernel::space::{NurbsCurve3, Parameterization};
        let points: Vec<_> = source.fit_points.iter().map(|point| [point.x, point.y, point.z]).collect();
        let parameterization = match source.knot_parameterization {
            1 => Parameterization::Centripetal, 2 => Parameterization::Uniform, _ => Parameterization::Chord,
        };
        let tangent = |point: &Vector3| (point.x != 0.0 || point.y != 0.0 || point.z != 0.0).then_some([point.x, point.y, point.z]);
        if source.flags.closed || source.flags.periodic {
            NurbsCurve3::interpolate_periodic(&points, parameterization)?
        } else {
            NurbsCurve3::interpolate_fit(&points, tangent(&source.begin_tangent), tangent(&source.end_tangent), parameterization)?
        }
    } else {
        cadkernel::space::NurbsCurve3::new_strict(current,
            source.control_points.iter().map(|point| [point.x, point.y, point.z]).collect(),
            source.knots.clone(), weights)?
    };
    Some(curve.with_periodicity(source.flags.closed || source.flags.periodic))
}
