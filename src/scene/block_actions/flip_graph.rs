//! Ordered reference evaluation for coupled Flip selections.
use super::instance::ParameterFrame;
use super::*;
use codec::xdata::XDataValue as X;
use std::collections::{BTreeMap, BTreeSet};
pub(super) const INPUTS: &str = "OCS_BLOCK_INPUT_FRAMES";
const FRAME_ERROR: &str = "The block parameter frame is unavailable.";
struct Step {
    action: Handle,
    axis: Handle,
    targets: BTreeSet<Handle>,
}
pub(super) struct Plan {
    steps: Vec<Step>,
    parameters: Vec<Parameter>,
}
impl Plan {
    pub fn new(doc: &Document, def: Handle) -> Option<Self> {
        let all = actions(doc, def);
        if all.iter().filter(|(_, _, k, _)| *k == Kind::Flip).count() < 2 {
            return None;
        }
        if super::flip_frames::Plan::new(doc, def).is_some() {
            return None;
        }
        let graph = authoring::graph(doc, def).ok()??;
        let ObjectType::DynamicBlock(object) = &doc.objects[&graph] else {
            return None;
        };
        let DynamicBlockData::EvaluationGraph(g) = &object.data else {
            return None;
        };
        let members: BTreeSet<_> = g.nodes.iter().map(|n| n.expression).collect();
        let parameters = all_parameters(doc, def);
        if parameters
            .iter()
            .map(|p| p.id)
            .collect::<BTreeSet<_>>()
            .len()
            != parameters.len()
        {
            return None;
        }
        let order = chain_order(doc, def, &all).ok()?;
        let mut steps = Vec::new();
        for index in order.iter().copied() {
            let (handle, action, kind, owner) = &all[index];
            if *kind != Kind::Flip {
                continue;
            }
            let axis = parameters.iter().find(|p| p.id == *owner && p.flip)?;
            let mut targets = BTreeSet::new();
            let mut grips = BTreeSet::new();
            for dep in &action.dependencies {
                if let Some(e) = doc.get_entity(*dep) {
                    if e.common().owner_handle != def || !supported(e) {
                        return None;
                    }
                    continue;
                }
                if !members.contains(dep) || doc.object_owner(*dep) != Some(graph) {
                    return None;
                }
                if let Some(p) = parameters.iter().find(|p| p.handle == *dep) {
                    if !p.linear && !p.flip {
                        return None;
                    }
                    if p.handle != axis.handle {
                        targets.insert(p.handle);
                    }
                    continue;
                }
                let ObjectType::DynamicBlock(o) = doc.objects.get(dep)? else {
                    return None;
                };
                match &o.data {
                    DynamicBlockData::BasePointParameter(_)
                    | DynamicBlockData::LookupParameter(_)
                    | DynamicBlockData::LookupAction(_)
                    | DynamicBlockData::LookupGrip(_) => {}
                    DynamicBlockData::LinearGrip(_) | DynamicBlockData::FlipGrip(_) => {
                        grips.insert(*dep);
                    }
                    _ if all.iter().enumerate().any(|(other_index, (h, _, _, _))| {
                        h == dep
                            && order.iter().position(|i| *i == other_index)
                                < order.iter().position(|i| *i == index)
                    }) => {}
                    _ => return None,
                }
            }
            grips.remove(&axis.grip);
            for p in parameters.iter().filter(|p| targets.contains(&p.handle)) {
                if grips.is_empty() {
                    break;
                }
                grips.remove(&p.grip);
                for grip in crate::scene::block_grips::config::locations(doc, def, p.handle).ok()? {
                    grips.remove(&grip.handle);
                }
            }
            if !grips.is_empty() {
                return None;
            }
            // A parameter's geometric actions must be evaluated before it is
            // reflected, and their entire geometry selection must follow it.
            for (other_index, (h, linked, k, id)) in all.iter().enumerate() {
                if !parameters
                    .iter()
                    .any(|p| targets.contains(&p.handle) && p.id == *id)
                {
                    continue;
                }
                let geometry: Vec<_> = linked
                    .dependencies
                    .iter()
                    .filter(|d| doc.get_entity(**d).is_some())
                    .collect();
                if geometry.is_empty() {
                    continue;
                }
                if matches!(k, Kind::Array | Kind::Flip)
                    || !action.dependencies.contains(h)
                    || order.iter().position(|i| *i == other_index)?
                        >= order.iter().position(|i| *i == index)?
                    || geometry.iter().any(|d| !action.dependencies.contains(d))
                {
                    return None;
                }
            }
            steps.push(Step {
                action: *handle,
                axis: axis.handle,
                targets,
            });
        }
        // An axis may be selected by a later Flip. Selecting a future axis
        // requires another ordering model and is refused here.
        for (index, step) in steps.iter().enumerate() {
            if steps
                .iter()
                .skip(index + 1)
                .any(|later| step.targets.contains(&later.axis))
            {
                return None;
            }
        }
        steps
            .iter()
            .any(|step| !step.targets.is_empty())
            .then_some(Self { steps, parameters })
    }
    fn states(
        &self,
        value: impl Fn(&Parameter) -> Result<Value, &'static str>,
    ) -> Result<Vec<bool>, &'static str> {
        self.steps
            .iter()
            .map(|s| {
                let p = self
                    .parameters
                    .iter()
                    .find(|p| p.handle == s.axis)
                    .ok_or(FRAME_ERROR)?;
                match value(p)? {
                    Value::Flip(v) => Ok(v),
                    _ => Err(FRAME_ERROR),
                }
            })
            .collect()
    }
    fn reflected(point: DVec3, axis: ParameterFrame) -> Result<DVec3, &'static str> {
        if !axis.base.is_finite() || !axis.end.is_finite() || axis.base.distance(axis.end) < 1e-9 {
            return Err("The parameter direction is unavailable.");
        }
        Ok(crate::scene::view::transform::reflected_point(
            point,
            axis.base,
            axis.end,
            DVec3::Z,
        ))
    }
    fn input(
        &self,
        parameter: Handle,
        mut input: Value,
        states: &[bool],
        axes: &BTreeMap<Handle, ParameterFrame>,
    ) -> Result<Value, &'static str> {
        for (step, active) in self.steps.iter().zip(states).rev() {
            if *active && step.targets.contains(&parameter) {
                let axis = *axes.get(&step.axis).ok_or(FRAME_ERROR)?;
                Self::reflected(axis.base, axis)?;
                input = input.map_points(|p| Self::reflected(p, axis).unwrap());
            }
        }
        Ok(input)
    }
    fn read(
        &self,
        doc: &Document,
        insert: Handle,
    ) -> Result<Option<BTreeMap<Handle, ParameterFrame>>, &'static str> {
        let common = doc.get_entity(insert).ok_or(FRAME_ERROR)?.common();
        let Some(record) = common.extended_data.get_record(INPUTS) else {
            return Ok(None);
        };
        let Some((X::Integer16(1), rest)) = record.values.split_first() else {
            return Err(FRAME_ERROR);
        };
        if rest.len() != self.parameters.len() * 5 {
            return Err(FRAME_ERROR);
        }
        let mut frames = BTreeMap::new();
        for chunk in rest.chunks_exact(5) {
            let [X::Integer32(id), X::Point3D(base), X::Point3D(end), X::Point3D(a), X::Point3D(b)] =
                chunk
            else {
                return Err(FRAME_ERROR);
            };
            let p = self
                .parameters
                .iter()
                .find(|p| p.id == *id)
                .ok_or(FRAME_ERROR)?;
            if v(*base).distance(p.base) > 1e-8
                || v(*end).distance(p.end) > 1e-8
                || !v(*a).is_finite()
                || !v(*b).is_finite()
                || (a.z - p.base.z).abs() > 1e-8
                || (b.z - p.end.z).abs() > 1e-8
            {
                return Err(FRAME_ERROR);
            }
            if frames
                .insert(
                    p.handle,
                    ParameterFrame {
                        base: v(*a),
                        end: v(*b),
                    },
                )
                .is_some()
            {
                return Err(FRAME_ERROR);
            }
        }
        Ok(Some(frames))
    }
    fn write(&self, doc: &mut Document, insert: Handle, frames: &BTreeMap<Handle, ParameterFrame>) {
        let mut data = vec![X::Integer16(1)];
        for p in &self.parameters {
            let f = frames[&p.handle];
            data.extend([
                X::Integer32(p.id),
                X::Point3D(native(p.base)),
                X::Point3D(native(p.end)),
                X::Point3D(native(f.base)),
                X::Point3D(native(f.end)),
            ]);
        }
        crate::scene::view::dispatch::set_entity_xdata(doc, insert, INPUTS, Some(data));
    }
    pub fn present_grips(
        &self,
        doc: &Document,
        baseline: &Document,
        def: Handle,
        insert: Handle,
        parameter: &Parameter,
        value: Value,
        testing: bool,
        grips: &mut [crate::scene::block_grips::runtime::Grip],
    ) -> Result<(), &'static str> {
        if grips.is_empty()
            || !self
                .steps
                .iter()
                .any(|step| step.targets.contains(&parameter.handle))
        {
            return Ok(());
        }
        let states = self.states(|p| {
            let state = if testing {
                instance::test_value(doc, insert, p)
            } else {
                instance::value(doc, insert, p)
            };
            Ok(state.unwrap_or(Value::Flip(false)))
        })?;
        if !self
            .steps
            .iter()
            .zip(&states)
            .any(|(step, active)| *active && step.targets.contains(&parameter.handle))
        {
            return Ok(());
        }
        let frames = self.read(doc, insert)?.ok_or(FRAME_ERROR)?;
        let graph = chains::Graph::new(baseline, def)?;
        let values = frames.iter().map(|(h, f)| (*h, f.end)).collect();
        let bases = frames.iter().map(|(h, f)| (*h, f.base)).collect();
        let raw = graph.autonomous(doc, Some(&values), Some(&bases))?;
        let locations =
            crate::scene::block_grips::config::locations(baseline, def, parameter.handle)?;
        let original = self
            .parameters
            .iter()
            .find(|p| p.handle == parameter.handle)
            .ok_or(FRAME_ERROR)?;
        for grip in grips {
            let native = locations
                .iter()
                .find(|g| g.role == grip.role)
                .ok_or(FRAME_ERROR)?;
            let mut offset = v(native.point - native.key_point);
            let mut direction = if original.flip {
                super::grip_direction(original, value)
            } else {
                native.direction.map(v)
            };
            for (step, active) in self.steps.iter().zip(&states) {
                if !*active || !step.targets.contains(&parameter.handle) {
                    continue;
                }
                let effective = *frames.get(&step.axis).ok_or(FRAME_ERROR)?;
                let axis = graph.active_frame(step.axis, effective, &raw);
                let zero = Self::reflected(DVec3::ZERO, axis)?;
                offset = Self::reflected(offset, axis)? - zero;
                direction = direction
                    .map(|d| Self::reflected(d, axis).map(|p| p - zero))
                    .transpose()?;
            }
            grip.point += offset - grip.offset;
            grip.offset = offset;
            grip.direction = direction;
        }
        Ok(())
    }
    pub fn evaluate(
        &self,
        doc: &Document,
        baseline: &Document,
        def: Handle,
        insert: Handle,
        parameter: Handle,
        input: Value,
        testing: bool,
    ) -> Result<Option<Document>, &'static str> {
        let mut staged = doc.clone();
        let visible = if testing {
            instance::load_test_frames(doc, &mut staged, insert, &self.parameters)?
        } else {
            instance::load_frames(doc, &mut staged, insert, &self.parameters)?
        };
        let states = self.states(|p| super::value(&staged, p))?;
        let mut frames = match self.read(doc, insert)? {
            Some(frames) => frames,
            None if states.iter().all(|v| !v) => visible,
            None => return Err(FRAME_ERROR),
        };
        for p in &self.parameters {
            if !p.flip {
                super::set_value(&mut staged, p, Value::Point(frames[&p.handle].end))?;
            }
        }
        let graph = chains::Graph::new(baseline, def)?;
        let values = frames.iter().map(|(h, f)| (*h, f.end)).collect();
        let bases = frames.iter().map(|(h, f)| (*h, f.base)).collect();
        let raw = graph.autonomous(&staged, Some(&values), Some(&bases))?;
        let axes = frames
            .iter()
            .map(|(h, f)| (*h, graph.active_frame(*h, *f, &raw)))
            .collect();
        let mut input = self.input(parameter, input, &states, &axes)?;
        let previous = frames.clone();
        let mut base_changed = false;
        if let Value::Frame { base, end } = input {
            let f = frames.get_mut(&parameter).ok_or(FRAME_ERROR)?;
            base_changed = f.base.distance(base) > 1e-10;
            f.base = base;
            input = Value::Point(end);
        }
        let mut resolved = BTreeMap::new();
        let mut active = BTreeMap::new();
        let actions: Vec<_> = self.steps.iter().map(|s| s.action).collect();
        if !super::evaluate_framed(
            &mut staged,
            baseline,
            def,
            parameter,
            input,
            &frames,
            base_changed,
            &previous,
            &mut resolved,
            &mut active,
            None,
            &actions,
        )? {
            return Ok(None);
        }
        frames.extend(resolved);
        let selected = self
            .parameters
            .iter()
            .find(|p| p.handle == parameter)
            .ok_or(FRAME_ERROR)?;
        if let Value::Point(end) = super::value(&staged, selected)? {
            frames.get_mut(&parameter).ok_or(FRAME_ERROR)?.end = end;
        }
        let canonical = frames.clone();
        let states = self.states(|p| super::value(&staged, p))?;
        for (step, enabled) in self.steps.iter().zip(states) {
            if !enabled {
                continue;
            }
            let axis = *active
                .get(&step.axis)
                .or_else(|| canonical.get(&step.axis))
                .ok_or(FRAME_ERROR)?;
            for target in &step.targets {
                let f = frames.get_mut(target).ok_or(FRAME_ERROR)?;
                f.base = Self::reflected(f.base, axis)?;
                f.end = Self::reflected(f.end, axis)?;
            }
        }
        let mut result = if testing {
            staged.clone()
        } else {
            instance::materialize(doc, insert, def, &staged)?
        };
        instance::save_resolved_frames(&mut result, &self.parameters, insert, &frames)?;
        if selected.flip {
            instance::save(
                &mut result,
                insert,
                selected,
                super::value(&staged, selected)?,
            )?;
        }
        self.write(&mut result, insert, &canonical);
        if testing {
            let EntityType::Insert(reference) = result.get_entity(insert).ok_or(FRAME_ERROR)?
            else {
                return Err(FRAME_ERROR);
            };
            let name = reference.block_name.clone();
            super::sync_representation(&mut result, def, &name)?;
        }
        Ok(Some(result))
    }
}
