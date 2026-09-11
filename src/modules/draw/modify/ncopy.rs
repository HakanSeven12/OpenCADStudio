use acadrust::{CadDocument, EntityType, Handle};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use crate::command::{CadCommand, CmdOption, CmdResult, EntityTransform};
use crate::entities::traits::EntityTypeOps;
use crate::scene::convert::acad_to_render::RenderObject;

enum Step { Select, Base, Target(DVec3), ArrayCount(DVec3), ArrayTarget(DVec3, usize, bool) }
pub struct NcopyCommand {
    nested: HashMap<Handle, Vec<EntityType>>,
    hit_paths: HashMap<Handle, Vec<Vec<[f64; 3]>>>,
    selected: Vec<(Handle, usize)>,
    step: Step,
    multiple: bool,
    placements: usize,
}
impl NcopyCommand {
    pub fn new(document: &CadDocument) -> Self {
        fn leaves(entity: &EntityType, document: &CadDocument, path: &mut HashSet<String>, output: &mut Vec<EntityType>) {
            let EntityType::Insert(insert) = entity else { output.push(entity.clone()); return; };
            let name = insert.block_name.to_ascii_uppercase();
            if !path.insert(name.clone()) { return; }
            for child in super::explode::explode_entity(entity, document) {
                leaves(&child, document, path, output);
            }
            path.remove(&name);
        }
        let nested: HashMap<_, _> = document.entities().filter(|e| matches!(e, EntityType::Insert(_)))
            .map(|e| {
                let mut entities = Vec::new();
                leaves(e, document, &mut HashSet::new(), &mut entities);
                (e.common().handle, entities)
            }).collect();
        let hit_paths = nested.iter().map(|(&handle, entities)| {
            let paths = entities.iter().map(|entity| {
                if crate::entities::curve::entity_curve(entity).is_some() { return Vec::new(); }
                match entity.to_render_entity(document).map(|render| render.object) {
                    Some(RenderObject::Dot(point)) => vec![point],
                    Some(RenderObject::Lines(points) | RenderObject::SegmentedLines(points)
                        | RenderObject::TaperedLines(points, _) | RenderObject::BoundaryLines { points, .. }) => points,
                    _ => Vec::new(),
                }
            }).collect();
            (handle, paths)
        }).collect();
        Self { nested, hit_paths, selected: Vec::new(), step: Step::Select, multiple: false, placements: 0 }
    }
    fn place(&self, delta: DVec3) -> CmdResult {
        if !delta.is_finite() { return CmdResult::NeedPoint; }
        let copies = self.selected.iter().filter_map(|(h, index)| self.nested.get(h)?.get(*index)).cloned()
            .map(|mut entity| {
                crate::scene::view::dispatch::apply_transform(&mut entity, &EntityTransform::Translate(delta));
                entity.common_mut().handle = Handle::NULL;
                entity
            }).collect();
        if self.multiple { CmdResult::CommitEntities(copies) }
        else { CmdResult::CommitEntitiesAndExit(copies) }
    }
}
impl CadCommand for NcopyCommand {
    fn name(&self) -> &'static str { "NCOPY" }
    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select nested objects:".into(),
            Step::Base => "Specify base point or [Displacement/Multiple] <Displacement>:".into(),
            Step::Target(_) => "Specify second point or [Array] <use first point as displacement>:".into(),
            Step::ArrayCount(_) => "Enter number of items to array:".into(),
            Step::ArrayTarget(_, _, false) => "Specify second point or [Fit]:".into(),
            Step::ArrayTarget(_, _, true) => "Specify last point:".into(),
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Base => vec![CmdOption::new("Displacement", "D"), CmdOption::new("Multiple", "M")],
            Step::Target(_) if self.multiple && self.placements > 0 => vec![CmdOption::new("Array", "A"), CmdOption::new("Exit", "E"), CmdOption::new("Undo", "U")],
            Step::Target(_) => vec![CmdOption::new("Array", "A")],
            Step::ArrayTarget(_, _, false) => vec![CmdOption::new("Fit", "F")],
            _ => Vec::new(),
        }
    }
    fn preserve_commit_style(&self) -> bool { true }
    fn preserve_commit_layer(&self) -> bool { true }
    fn needs_entity_pick(&self) -> bool { matches!(self.step, Step::Select) }
    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if let Some(entities) = self.nested.get(&handle) {
            let nearest = entities.iter().enumerate().filter_map(|(index, entity)| {
                let distance = if let Some(curve) = crate::entities::curve::entity_curve(entity) {
                    let projected = curve.plane.project([point.x, point.y, point.z])?;
                    cadkernel::geom2d::closest_point(&curve.curve, projected).distance
                        .hypot(curve.plane.distance_to(point.to_array())?)
                } else {
                    let path = self.hit_paths.get(&handle)?.get(index)?;
                    let point = cadkernel::space::Vec3::from(point.to_array());
                    if path.len() == 1 {
                        point.distance(cadkernel::space::Vec3::from(path[0]))
                    } else {
                        path.windows(2).filter(|segment| segment.iter().flatten().all(|v| v.is_finite()))
                            .map(|segment| point.distance_to_segment(segment[0].into(), segment[1].into()))
                            .min_by(f64::total_cmp)?
                    }
                };
                Some((index, distance))
            }).min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((index, _)) = nearest {
                if !self.selected.contains(&(handle, index)) { self.selected.push((handle, index)); }
            }
        }
        CmdResult::NeedPoint
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Select => CmdResult::NeedPoint,
            Step::Base => { self.step = Step::Target(point); CmdResult::NeedPoint }
            Step::Target(base) => {
                if !point.is_finite() { return CmdResult::NeedPoint; }
                self.placements += 1;
                self.place(point - base)
            }
            Step::ArrayCount(_) => CmdResult::NeedPoint,
            Step::ArrayTarget(base, count, fit) => {
                let delta = (point - base) / if fit { (count - 1) as f64 } else { 1.0 };
                if !delta.is_finite() { return CmdResult::NeedPoint; }
                let mut copies = Vec::new();
                for index in 0..count {
                    if let CmdResult::CommitEntities(mut entities) | CmdResult::CommitEntitiesAndExit(mut entities) = self.place(delta * index as f64) {
                        copies.append(&mut entities);
                    }
                }
                CmdResult::CommitEntitiesAndExit(copies)
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select if !self.selected.is_empty() => { self.step = Step::Base; CmdResult::NeedPoint }
            Step::Base => { self.step = Step::Target(DVec3::ZERO); CmdResult::NeedPoint }
            Step::Target(base) if !self.multiple => self.place(base),
            _ => CmdResult::Cancel,
        }
    }
    fn point_step_accepts_keywords(&self) -> bool { matches!(self.step, Step::Base | Step::Target(_) | Step::ArrayTarget(..)) }
    fn wants_text_input(&self) -> bool { !matches!(self.step, Step::Select) }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Step::ArrayCount(base) = self.step {
            if let Ok(count) = text.trim().parse::<usize>() {
                if (2..=32767).contains(&count) { self.step = Step::ArrayTarget(base, count, false); }
            }
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_uppercase().as_str() {
            "A" | "ARRAY" => {
                if let Step::Target(base) = self.step { self.step = Step::ArrayCount(base); }
                Some(CmdResult::NeedPoint)
            }
            "F" | "FIT" => {
                if let Step::ArrayTarget(base, count, _) = self.step { self.step = Step::ArrayTarget(base, count, true); }
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" if self.multiple && self.placements > 0 => {
                self.placements -= 1;
                Some(CmdResult::UndoDocument)
            }
            "E" | "EXIT" if self.multiple => Some(CmdResult::Cancel),
            "M" | "MULTIPLE" if matches!(self.step, Step::Base) => { self.multiple = true; Some(CmdResult::NeedPoint) }
            "D" | "DISPLACEMENT" if matches!(self.step, Step::Base) => { self.step = Step::Target(DVec3::ZERO); Some(CmdResult::NeedPoint) }
            _ => None,
        }
    }
}
