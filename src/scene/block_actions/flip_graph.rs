//! Ordered reference evaluation for coupled Flip selections.
use super::instance::ParameterFrame;
use super::*;
use document::xdata::XDataValue as X;
use std::collections::{BTreeMap, BTreeSet};
mod recovery;
pub(super) const INPUTS: &str = "OCS_BLOCK_INPUT_FRAMES";
pub(super) const FRAME_ERROR: &str = "The block parameter frame is unavailable.";
const SELECTION_ERROR: &str =
    "Select all objects driven by the linked parameters and ensure their actions precede the Flip.";

/// Native action nodes follow their selected Linear parameter. Geometry stays
/// explicit so a parameter pick cannot silently expand the reflection.
pub(super) fn authored_dependencies(
    doc: &Document,
    def: Handle,
    source: i32,
    selected: &[Handle],
    previous: &[Handle],
) -> Result<Vec<Handle>, &'static str> {
    let parameters = all_parameters(doc, def);
    let actions = actions(doc, def);
    let mut additions = BTreeSet::from_iter(selected.iter().copied());
    let mut dependencies = previous
        .iter()
        .copied()
        .filter(|h| additions.contains(h) || doc.get_entity(*h).is_none())
        .collect::<Vec<_>>();
    for p in parameters
        .iter()
        .filter(|p| p.is_action_target(Kind::Flip, source))
    {
        let include = additions.contains(&p.handle);
        if !include && !previous.contains(&p.handle) {
            continue;
        }
        if !include {
            let grips = crate::scene::block_grips::config::locations(doc, def, p.handle)?;
            dependencies.retain(|h| *h != p.handle && !grips.iter().any(|g| g.handle == *h));
        }
        for (handle, _, _, _) in actions
            .iter()
            .filter(|(_, _, _, id)| p.linear && *id == p.id)
        {
            if include {
                additions.insert(*handle);
            } else {
                dependencies.retain(|h| h != handle);
            }
        }
    }
    for handle in &dependencies {
        additions.remove(handle);
    }
    dependencies.extend(additions);
    Ok(dependencies)
}

pub(super) fn validate_authored_selection(
    doc: &Document,
    def: Handle,
    action: Handle,
    source: i32,
    dependencies: &[Handle],
) -> Result<(), &'static str> {
    if Plan::for_rebuild(doc, def)
        .is_some_and(|plan| plan.steps.iter().any(|step| step.action == action))
        || super::targets::geometry(doc, def, Kind::Flip, source, dependencies).is_ok()
    {
        return Ok(());
    }
    Err(SELECTION_ERROR)
}

/// Refuse edits that leave a previously supported coupling in the graph
/// without an evaluation plan. Removing the last coupling is allowed.
pub(super) fn preserve_authored_selection(
    before: &Document,
    after: &Document,
    def: Handle,
) -> Result<(), &'static str> {
    if !actions(before, def).iter().any(|(_, action, kind, _)| {
        *kind == Kind::Flip
            && action
                .dependencies
                .iter()
                .any(|h| before.get_entity(*h).is_none())
    }) {
        return Ok(());
    }
    let Some(previous) = Plan::for_rebuild(before, def) else {
        return Ok(());
    };
    let remains_linked = actions(after, def).iter().any(|(_, action, kind, _)| {
        *kind == Kind::Flip
            && previous.steps.iter().any(|step| {
                step.targets
                    .iter()
                    .any(|target| action.dependencies.contains(target))
            })
    });
    if remains_linked && Plan::for_rebuild(after, def).is_none() {
        return Err(SELECTION_ERROR);
    }
    Ok(())
}

struct Step {
    action: Handle,
    axis: Handle,
    targets: BTreeSet<Handle>,
}

fn perpendicular(first: ParameterFrame, second: ParameterFrame) -> bool {
    (first.end - first.base)
        .try_normalize()
        .zip((second.end - second.base).try_normalize())
        .is_some_and(|(a, b)| a.dot(b).abs() <= 1e-8)
}

/// A linked dependent Scale uses the reflected pivot. Evaluating it just
/// before the reflecting actions is equivalent and keeps the pivot canonical.
pub(super) fn order_linked_scales(
    doc: &Document,
    definition: Handle,
    actions: &[(Handle, &BlockAction, Kind, i32)],
    managed_flips: &[Handle],
    order: &mut [usize],
) {
    if managed_flips.is_empty() {
        return;
    }
    let parameters = all_parameters(doc, definition);
    for index in 1..order.len() {
        let (handle, scale, kind, owner) = actions[order[index]];
        if kind != Kind::Scale || !options::base(doc, handle).is_some_and(|base| base.dependent) {
            continue;
        }
        let Some(parameter) = parameters.iter().find(|p| p.id == owner) else {
            continue;
        };
        let mut position = index;
        while position > 0 {
            let (handle, flip, _, _) = actions[order[position - 1]];
            if !managed_flips.contains(&handle)
                || !flip.action_ids.contains(&owner)
                || !flip.dependencies.contains(&parameter.handle)
                || scale.dependencies.iter().any(|target| {
                    doc.get_entity(*target).is_some() && !flip.dependencies.contains(target)
                })
            {
                break;
            }
            order.swap(position - 1, position);
            position -= 1;
        }
    }
}

pub(in crate::scene) struct Plan {
    steps: Vec<Step>,
    parameters: Vec<Parameter>,
    legacy: Option<super::flip_frames::Plan>,
    keep_cache: bool,
}
impl Plan {
    pub fn new(doc: &Document, def: Handle) -> Option<Self> {
        let all = actions(doc, def);
        if !all.iter().any(|(_, _, kind, _)| *kind == Kind::Flip) {
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
        let mut order = chain_order(doc, def, &all).ok()?;
        let flips: Vec<_> = all.iter().filter(|entry| entry.2 == Kind::Flip).map(|entry| entry.0).collect();
        order_linked_scales(doc, def, &all, &flips, &mut order);
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
                if *k == Kind::Flip {
                    // Native reciprocal Flips may name each other's parameters.
                    // With perpendicular axes each reflection preserves the
                    // other axis as a line, so geometry order stays valid.
                    if action.action_ids.contains(id)
                        && linked.action_ids.contains(owner)
                        && linked.dependencies.contains(&axis.handle)
                        && geometry.iter().all(|d| action.dependencies.contains(d))
                    {
                        continue;
                    }
                    return None;
                }
                if *k == Kind::Array
                    || !(action.dependencies.contains(h) || action.action_ids.contains(id))
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
        let plan = steps
            .iter()
            .any(|step| !step.targets.is_empty())
            .then_some(Self {
                steps,
                parameters,
                legacy: None,
                keep_cache: true,
            })?;
        plan.validate_axes(|handle| {
            plan.parameters
                .iter()
                .find(|p| p.handle == handle)
                .map(|p| ParameterFrame {
                    base: p.base,
                    end: p.end,
                })
        })
        .ok()?;
        Some(plan)
    }

    fn validate_axes(
        &self,
        frame: impl Fn(Handle) -> Option<ParameterFrame>,
    ) -> Result<(), &'static str> {
        for (index, step) in self.steps.iter().enumerate() {
            for later in &self.steps[index + 1..] {
                if step.targets.contains(&later.axis)
                    && !perpendicular(
                        frame(step.axis).ok_or(FRAME_ERROR)?,
                        frame(later.axis).ok_or(FRAME_ERROR)?,
                    )
                {
                    return Err(FRAME_ERROR);
                }
            }
        }
        Ok(())
    }

    pub fn for_rebuild(doc: &Document, def: Handle) -> Option<Self> {
        Self::new(doc, def).or_else(|| {
            let legacy = super::flip_frames::Plan::new(doc, def)?;
            let step = Step {
                action: legacy.action,
                axis: legacy.axis(),
                targets: legacy.targets().collect(),
            };
            Self::native(doc, def, vec![step], Some(legacy)).ok()
        })
    }

    pub(super) fn exit(doc: &Document, def: Handle) -> Result<Self, &'static str> {
        Self::native(doc, def, Vec::new(), None)
    }

    pub(super) fn retains_inputs(&self) -> bool {
        self.keep_cache
    }

    fn native(
        doc: &Document,
        def: Handle,
        steps: Vec<Step>,
        legacy: Option<super::flip_frames::Plan>,
    ) -> Result<Self, &'static str> {
        for (handle, action, kind, owner) in actions(doc, def) {
            if kind == Kind::Flip && !steps.iter().any(|step| step.action == handle) {
                super::targets::geometry(doc, def, kind, owner, &action.dependencies)?;
            }
        }
        let parameters = all_parameters(doc, def);
        if parameters
            .iter()
            .map(|p| p.id)
            .collect::<BTreeSet<_>>()
            .len()
            != parameters.len()
        {
            return Err(FRAME_ERROR);
        }
        Ok(Self {
            steps,
            parameters,
            legacy,
            keep_cache: false,
        })
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
        self.validate_axes(|axis| axes.get(&axis).copied())?;
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
            if !v(*base).is_finite()
                || !v(*end).is_finite()
                || v(*base).distance(p.base) > 1e-8
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
    fn display_frames(
        &self,
        states: &[bool],
        canonical: &BTreeMap<Handle, ParameterFrame>,
        active: &BTreeMap<Handle, ParameterFrame>,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        self.validate_axes(|axis| active.get(&axis).or_else(|| canonical.get(&axis)).copied())?;
        let mut frames = canonical.clone();
        for (step, enabled) in self.steps.iter().zip(states) {
            if !*enabled {
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
        Ok(frames)
    }

    pub fn load_canonical(
        &self,
        doc: &Document,
        evaluated: &mut Document,
        insert: Handle,
        testing: bool,
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let mut visible = if testing {
            instance::load_test_frames(doc, evaluated, insert, &self.parameters)?
        } else {
            instance::load_frames(doc, evaluated, insert, &self.parameters)?
        };
        let canonical = match self.read(doc, insert)? {
            Some(frames) => frames,
            None if self.legacy.is_some() => {
                let plan = self.legacy.as_ref().ok_or(FRAME_ERROR)?;
                let definition = doc
                    .dynamic_definition_for_insert(insert)
                    .ok_or(FRAME_ERROR)?;
                instance::canonical_flip_input(
                    plan,
                    doc,
                    evaluated,
                    insert,
                    definition,
                    &self.parameters,
                    &mut visible,
                    plan.axis(),
                    Value::Flip(false),
                    testing,
                )?;
                visible
            }
            None => {
                let states = self.states(|p| super::value(evaluated, p))?;
                if states.iter().any(|enabled| *enabled) {
                    let definition = doc
                        .dynamic_definition_for_insert(insert)
                        .ok_or(FRAME_ERROR)?;
                    self.recover_native(doc, evaluated, definition, insert, &states)?
                } else {
                    visible
                }
            }
        };
        for p in &self.parameters {
            if !p.flip {
                super::set_value(evaluated, p, Value::Point(canonical[&p.handle].end))?;
            }
        }
        Ok(canonical)
    }

    pub fn rebuild_reference(
        &self,
        doc: &Document,
        evaluated: &mut Document,
        baseline: &Document,
        def: Handle,
        insert: Handle,
        frames: &BTreeMap<Handle, ParameterFrame>,
        previous: super::RebuildInputs<'_>,
    ) -> Result<Document, &'static str> {
        let actions: Vec<_> = self.steps.iter().map(|s| s.action).collect();
        let mut active = BTreeMap::new();
        let mut canonical = frames.clone();
        let action_bases = instance::load_action_bases(doc, insert, def)?;
        let resolved = super::rebuild_framed_with_flips(
            evaluated,
            baseline,
            def,
            &canonical,
            Some(&action_bases),
            Some(previous),
            &actions,
            Some(&mut active),
        )?;
        for p in &self.parameters {
            if let Some(frame) = resolved.get(&p.handle) {
                canonical.insert(p.handle, *frame);
            } else if previous.requested.contains(&p.handle) && !p.flip {
                canonical.get_mut(&p.handle).ok_or(FRAME_ERROR)?.end = super::current(evaluated, p);
            }
        }
        let states = self.states(|p| super::value(evaluated, p))?;
        let visible = self.display_frames(&states, &canonical, &active)?;
        let mut result = instance::materialize(doc, insert, def, evaluated)?;
        instance::save_resolved_frames(&mut result, &self.parameters, insert, &visible)?;
        instance::save_action_bases(&mut result, insert, def, &action_bases)?;
        if self.keep_cache {
            self.write(&mut result, insert, &canonical);
        } else {
            instance::clear_canonical_inputs(&mut result, insert);
        }
        Ok(result)
    }

    fn migrate_frame(
        old: &Parameter,
        new: &Parameter,
        frame: ParameterFrame,
    ) -> Result<ParameterFrame, &'static str> {
        if (old.base.z - new.base.z).abs() > 1e-8 || (old.end.z - new.end.z).abs() > 1e-8 {
            return Err(FRAME_ERROR);
        }
        if new.point && old.point {
            return Ok(ParameterFrame {
                base: new.base,
                end: new.end + (frame.end - old.end),
            });
        }
        let angular = new.polar && old.polar || new.rotation && old.rotation;
        if !(new.linear && old.linear || new.flip && old.flip || angular) {
            return Err(FRAME_ERROR);
        }
        let old_axis = old.end - old.base;
        let new_axis = new.end - new.base;
        if old_axis.z.abs() > 1e-8 || new_axis.z.abs() > 1e-8 {
            return Err(FRAME_ERROR);
        }
        let old_direction = old_axis.try_normalize().ok_or(FRAME_ERROR)?;
        let new_direction = new_axis.try_normalize().ok_or(FRAME_ERROR)?;
        let length = frame.base.distance(frame.end);
        let default_length = (length - old_axis.length()).abs() <= 1e-8;
        if angular {
            let direction = (frame.end - frame.base)
                .try_normalize()
                .ok_or(FRAME_ERROR)?;
            let direction = if direction.distance(old_direction) <= 1e-8 {
                new_direction
            } else {
                direction
            };
            let radius = if new.rotation || default_length {
                new_axis.length()
            } else {
                length
            };
            let base = new.base + (frame.base - old.base);
            return Ok(ParameterFrame {
                base,
                end: base + direction * radius,
            });
        }
        let cosine = old_direction.dot(new_direction);
        let sine = old_direction.cross(new_direction).z;
        let rotate =
            |v: DVec3| DVec3::new(cosine * v.x - sine * v.y, sine * v.x + cosine * v.y, v.z);
        let base = new.base + rotate(frame.base - old.base);
        let delta = if new.flip {
            rotate(frame.end - frame.base) * (new_axis.length() / old_axis.length())
        } else if default_length {
            new_axis
        } else {
            new_direction * length
        };
        Ok(ParameterFrame {
            base,
            end: base + delta,
        })
    }

    pub fn refresh(
        &self,
        doc: &Document,
        before: &Document,
        def: Handle,
        insert: Handle,
        previous_plan: Option<&Self>,
    ) -> Result<Document, &'static str> {
        let (before, previous_plan, cached) = match previous_plan {
            Some(plan) => match plan.read(doc, insert) {
                Ok(cached) => (before, Some(plan), cached),
                Err(error) => {
                    // A live authoring edit can refresh the reference before
                    // BSAVE revisits it with the editor's older checkpoint.
                    if self.read(doc, insert).ok().flatten().is_none() {
                        return Err(error);
                    }
                    (doc, None, None)
                }
            },
            None => (before, None, None),
        };
        let mut cleaned = None;
        let mut evaluated = doc.clone();
        let mut migrated_inputs = BTreeMap::new();
        let (frames, previous_frames) = if let Some(plan) = previous_plan {
            let previous_frames = match cached {
                Some(frames) => frames,
                None => plan.load_canonical(doc, &mut before.clone(), insert, false)?,
            };
            let old_parameters: BTreeMap<_, _> =
                plan.parameters.iter().map(|p| (p.handle, p)).collect();
            let new_handles: BTreeSet<_> = self.parameters.iter().map(|p| p.handle).collect();
            let reset_ids: BTreeSet<_> = plan
                .parameters
                .iter()
                .filter(|p| !new_handles.contains(&p.handle))
                .chain(
                    self.parameters
                        .iter()
                        .filter(|p| !old_parameters.contains_key(&p.handle)),
                )
                .map(|p| p.id)
                .collect();
            if !reset_ids.is_empty() {
                let reference = cleaned.insert(doc.clone());
                for id in reset_ids {
                    instance::remove_cache(reference, insert, id)?;
                }
            }
            let mut frames = BTreeMap::new();
            let mut previous_autonomous = None;
            for p in &self.parameters {
                let authored = ParameterFrame {
                    base: p.base,
                    end: p.end,
                };
                let (frame, value) = if let Some(old) = old_parameters.get(&p.handle) {
                    if old.id != p.id {
                        return Err(FRAME_ERROR);
                    }
                    if p.rotation
                        && old.rotation
                        && (p.reference - p.base).distance(old.reference - old.base) > 1e-8
                        && instance::cache(doc, insert, old.id)?.is_some()
                    {
                        instance::migrate_rotation_reference(
                            cleaned.get_or_insert_with(|| doc.clone()),
                            insert,
                            old,
                            p,
                        )?;
                    }
                    let mut frame = previous_frames[&p.handle];
                    if old.base.distance(p.base) > 1e-8 || old.end.distance(p.end) > 1e-8 {
                        if previous_autonomous.is_none() {
                            previous_autonomous = Some(
                                chains::Graph::new(before, def)?.autonomous(
                                    before,
                                    Some(
                                        &previous_frames.iter().map(|(h, f)| (*h, f.end)).collect(),
                                    ),
                                    Some(
                                        &previous_frames
                                            .iter()
                                            .map(|(h, f)| (*h, f.base))
                                            .collect(),
                                    ),
                                )?,
                            );
                        }
                        let autonomous = previous_autonomous
                            .as_ref()
                            .and_then(|frames| frames.get(&p.handle));
                        frame = Self::migrate_frame(old, p, autonomous.copied().unwrap_or(frame))?;
                        migrated_inputs.insert(p.handle, frame);
                    }
                    let value = if p.flip {
                        instance::value(doc, insert, old).unwrap_or(Value::Flip(false))
                    } else {
                        Value::Point(frame.end)
                    };
                    (frame, value)
                } else {
                    let value = if p.flip {
                        Value::Flip(false)
                    } else {
                        Value::Point(p.end)
                    };
                    (authored, value)
                };
                super::set_value(&mut evaluated, p, value)?;
                frames.insert(p.handle, frame);
            }
            (frames, Some(previous_frames))
        } else {
            let frames = if !self.steps.is_empty()
                && self.read(doc, insert)?.is_none()
                && Self::exit(before, def).is_ok()
            {
                // The previous definition had geometry-only Flips. Its
                // native frames are inputs to the newly linked actions.
                instance::load_frames(before, &mut evaluated, insert, &self.parameters)?
            } else {
                self.load_canonical(doc, &mut evaluated, insert, false)?
            };
            (frames, None)
        };
        let reference = cleaned.as_ref().unwrap_or(doc);
        let previous_frames = previous_frames.as_ref().unwrap_or(&frames);
        self.rebuild_reference(
            reference,
            &mut evaluated,
            doc,
            def,
            insert,
            &frames,
            super::RebuildInputs {
                document: before,
                frames: previous_frames,
                values: &previous_frames.iter().map(|(h, f)| (*h, f.end)).collect(),
                requested: &BTreeSet::new(),
                migrated_inputs: Some(&migrated_inputs),
            },
        )
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
        let frames = match self.read(doc, insert)? {
            Some(frames) => frames,
            None => self.recover_native(doc, baseline, def, insert, &states)?,
        };
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
        let mut frames = self.load_canonical(doc, &mut staged, insert, testing)?;
        let states = self.states(|p| super::value(&staged, p))?;
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
            f.end = end;
            input = Value::Point(end);
        } else if let Value::Point(end) = input {
            if let Some(f) = frames.get_mut(&parameter) {
                f.end = end;
            }
        }
        let action_bases = instance::update_action_bases(
            doc,
            insert,
            def,
            &self.parameters,
            &previous,
            &frames,
            parameter,
        )?;
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
            Some(&action_bases),
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
        let states = self.states(|p| super::value(&staged, p))?;
        let visible = self.display_frames(&states, &frames, &active)?;
        let mut result = if testing {
            staged.clone()
        } else {
            instance::materialize(doc, insert, def, &staged)?
        };
        instance::save_resolved_frames(&mut result, &self.parameters, insert, &visible)?;
        instance::save_action_bases(&mut result, insert, def, &action_bases)?;
        if selected.flip {
            instance::save(
                &mut result,
                insert,
                selected,
                super::value(&staged, selected)?,
            )?;
        }
        self.write(&mut result, insert, &frames);
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
