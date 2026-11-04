//! Canonical and visible frames for supported Flip-to-Linear action links.
use super::instance::ParameterFrame;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Plan {
    pub action: Handle,
    axis: Handle,
    targets: BTreeSet<Handle>,
}

impl Plan {
    /// Accept only ordered, complete links whose actions precede the Flip.
    /// Other retained graphs keep the existing unsupported-target guard.
    pub fn new(doc: &Document, definition: Handle) -> Option<Self> {
        let all = actions(doc, definition);
        let flips: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(_, (_, _, kind, _))| *kind == Kind::Flip)
            .collect();
        let parameters = all_parameters(doc, definition);
        let candidates: Vec<_> = flips
            .iter()
            .copied()
            .filter(|(_, (_, record, _, id))| {
                let own_grip = parameters
                    .iter()
                    .find(|p| p.id == *id && p.flip)
                    .map(|p| p.grip);
                record
                    .dependencies
                    .iter()
                    .any(|h| doc.get_entity(*h).is_none() && Some(*h) != own_grip)
            })
            .collect();
        if candidates.len() != 1 {
            return None;
        }
        let (flip_graph_index, (action, record, _, id)) = candidates[0];
        let order = chain_order(doc, definition, &all).ok()?;
        let flip_index = order.iter().position(|index| *index == flip_graph_index)?;
        let axis = parameters.iter().find(|p| p.id == *id && p.flip)?;
        if parameters.iter().filter(|p| p.id == axis.id).count() != 1 {
            return None;
        }
        // Independent geometry-only Flips need no frame conversion. Overlapping
        // or nested selections still require ordered reflection composition.
        for (_, (other, selection, _, owner)) in &flips {
            if other == action {
                continue;
            }
            let own_grip = parameters.iter().find(|p| p.id == *owner && p.flip)?.grip;
            for handle in &selection.dependencies {
                if *handle == own_grip {
                    continue;
                }
                if doc.get_entity(*handle).is_none() || record.dependencies.contains(handle) {
                    return None;
                }
            }
        }
        let mut targets = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut linked_actions = BTreeSet::new();
        for handle in &record.dependencies {
            if *handle == Handle::NULL {
                return None;
            }
            if doc.get_entity(*handle).is_some() || *handle == axis.grip {
                continue;
            }
            if let Some(p) = parameters.iter().find(|p| p.linear && p.handle == *handle) {
                targets.insert(p.handle);
                continue;
            }
            if let Some(p) = parameters.iter().find(|p| p.linear && p.grip == *handle) {
                aliases.insert(p.handle);
                continue;
            }
            if let Some((index, (_, _, kind, _))) =
                all.iter().enumerate().find(|(_, (h, _, _, _))| h == handle)
            {
                let position = order.iter().position(|entry| *entry == index)?;
                if position >= flip_index || matches!(kind, Kind::Flip | Kind::Array) {
                    return None;
                }
                linked_actions.insert(*handle);
                continue;
            }
            return None;
        }
        if targets.is_empty() || !aliases.is_subset(&targets) {
            return None;
        }
        for handle in &targets {
            let parameter = parameters.iter().find(|p| p.handle == *handle)?;
            if parameters.iter().filter(|p| p.id == parameter.id).count() != 1 {
                return None;
            }
        }
        // A relocated display grip needs its own reflected offset. Until that
        // is represented, do not publish a misleading grip location.
        for handle in &targets {
            if crate::scene::block_grips::config::locations(doc, definition, *handle)
                .ok()?
                .iter()
                .any(|grip| v(grip.point - grip.key_point).length() > 1e-8)
            {
                return None;
            }
        }
        for (handle, linked, kind, owner) in &all {
            let belongs = parameters
                .iter()
                .any(|p| targets.contains(&p.handle) && p.id == *owner);
            if belongs
                && (!linked_actions.contains(handle) || matches!(kind, Kind::Array | Kind::Flip))
            {
                return None;
            }
            if linked_actions.contains(handle) && !belongs {
                return None;
            }
            if linked_actions.contains(handle) {
                let p = parameters.iter().find(|p| p.id == *owner)?;
                let own_grips: BTreeSet<_> =
                    crate::scene::block_grips::config::locations(doc, definition, p.handle)
                        .ok()?
                        .into_iter()
                        .map(|g| g.handle)
                        .collect();
                for dependency in &linked.dependencies {
                    if doc.get_entity(*dependency).is_some() {
                        if !record.dependencies.contains(dependency) {
                            return None;
                        }
                    } else if !own_grips.contains(dependency) {
                        return None;
                    }
                }
            }
            // A reflected Linear cannot itself drive a transported frame until
            // both graphs share a single ordered composition.
            if *kind == Kind::Move
                && linked.dependencies.iter().any(|target| {
                    targets.contains(target)
                        || (belongs && (*target == axis.handle || *target == axis.grip))
                })
            {
                return None;
            }
        }
        Some(Self {
            action: *action,
            axis: axis.handle,
            targets,
        })
    }

    fn point(point: DVec3, axis: ParameterFrame) -> DVec3 {
        crate::scene::view::transform::reflected_point(point, axis.base, axis.end, DVec3::Z)
    }

    pub fn reflect_frames(
        &self,
        frames: &mut BTreeMap<Handle, ParameterFrame>,
        axis: ParameterFrame,
    ) -> Result<(), &'static str> {
        if !axis.base.is_finite() || !axis.end.is_finite() || axis.base.distance(axis.end) < 1e-9 {
            return Err("The parameter direction is unavailable.");
        }
        for target in &self.targets {
            let frame = frames
                .get_mut(target)
                .ok_or("The block parameter frame is unavailable.")?;
            frame.base = Self::point(frame.base, axis);
            frame.end = Self::point(frame.end, axis);
        }
        Ok(())
    }

    pub fn reflect_input(
        &self,
        parameter: Handle,
        input: Value,
        axis: ParameterFrame,
    ) -> Result<Value, &'static str> {
        if !self.targets.contains(&parameter) {
            return Ok(input);
        }
        if !axis.base.is_finite() || !axis.end.is_finite() || axis.base.distance(axis.end) < 1e-9 {
            return Err("The parameter direction is unavailable.");
        }
        Ok(input.map_points(|point| Self::point(point, axis)))
    }

    pub fn targets(&self) -> impl Iterator<Item = Handle> + '_ {
        self.targets.iter().copied()
    }
    pub fn contains(&self, parameter: Handle) -> bool {
        self.targets.contains(&parameter)
    }
    pub fn axis(&self) -> Handle {
        self.axis
    }
}
