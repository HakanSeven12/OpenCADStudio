//! Bounded array planning shared by committed and preview evaluation.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Aggregate generated top-level entities per definition evaluation. Check this
/// before allocating handles or copying geometry, including across actions.
pub(super) const MAX_GENERATED_ENTITIES: usize = 100_000;
pub(super) const LIMIT_ERROR: &str =
    "Block evaluation would generate more than 100000 array objects. Reduce the distance or increase the spacing.";

#[derive(Debug)]
pub(super) struct Plan {
    columns: usize,
    cells: usize,
    column_step: DVec3,
    row_step: DVec3,
}

impl Plan {
    pub(super) fn new(
        parameter: &Parameter,
        base: DVec3,
        end: DVec3,
        action: &BlockArrayAction,
        targets: usize,
        remaining: &mut usize,
    ) -> Result<Self, &'static str> {
        if !base.is_finite() || !end.is_finite() {
            return Err("The parameter value produces non-finite geometry.");
        }
        let mut result = Self {
            columns: 1,
            cells: 1,
            column_step: DVec3::ZERO,
            row_step: DVec3::ZERO,
        };
        // Self-grip dependencies do not produce array copies.
        if targets == 0 {
            return Ok(result);
        }
        let count = |span: f64, spacing: f64| -> Result<usize, &'static str> {
            let cells = ((span + 1e-9) / spacing).floor() + 1.;
            if !span.is_finite()
                || !cells.is_finite()
                || cells < 1.
                || cells > (MAX_GENERATED_ENTITIES + 1) as f64
            {
                return Err(LIMIT_ERROR);
            }
            Ok(cells as usize)
        };
        let (columns, rows) = if parameter.xy {
            let delta = end - base;
            result.column_step =
                DVec3::X * action.column_offset * if end.x >= base.x { 1. } else { -1. };
            result.row_step = DVec3::Y * action.row_offset * if end.y >= base.y { 1. } else { -1. };
            (
                count(delta.x.abs(), action.column_offset)?,
                count(delta.y.abs(), action.row_offset)?,
            )
        } else {
            let (span, axis) = if parameter.linear {
                let axis = (parameter.end - parameter.base).normalize();
                ((end - base).dot(axis).max(0.), axis)
            } else {
                let delta = end - base;
                let span = delta.length();
                let axis = if span > 1e-9 {
                    delta / span
                } else {
                    (parameter.end - parameter.base).normalize()
                };
                (span, axis)
            };
            result.column_step = axis * action.column_offset;
            (count(span, action.column_offset)?, 1)
        };
        let cells = columns.checked_mul(rows).ok_or(LIMIT_ERROR)?;
        let generated = (cells - 1).checked_mul(targets).ok_or(LIMIT_ERROR)?;
        let budget = remaining.checked_sub(generated).ok_or(LIMIT_ERROR)?;
        if !result.column_step.is_finite() || !result.row_step.is_finite() {
            return Err("The parameter value produces non-finite geometry.");
        }
        result.columns = columns;
        result.cells = cells;
        *remaining = budget;
        Ok(result)
    }

    /// Row-major native ordering; the source cell is excluded. Constant memory
    /// keeps preview planning independent of the number of generated copies.
    pub(super) fn displacements(&self) -> impl Iterator<Item = (usize, DVec3)> + '_ {
        (1..self.cells).map(|index| {
            let column = index % self.columns;
            let row = index / self.columns;
            (
                index,
                self.column_step * column as f64 + self.row_step * row as f64,
            )
        })
    }

    fn generated(&self, sources: usize) -> Result<usize, &'static str> {
        (self.cells - 1).checked_mul(sources).ok_or(LIMIT_ERROR)
    }
}

/// Copies follow only named downstream actions targeting their source member.
pub(super) struct Sequence(BTreeMap<usize, BTreeSet<usize>>);

impl Sequence {
    pub(super) fn new(
        all: &[(Handle, &BlockAction, Kind, i32)],
        successors: &[Vec<usize>],
    ) -> Self {
        let incoming: BTreeSet<_> = successors.iter().flatten().copied().collect();
        let mut arrays = BTreeMap::new();
        for (index, (_, _, kind, _)) in all.iter().enumerate() {
            if *kind != Kind::Array || (successors[index].is_empty() && !incoming.contains(&index))
            {
                continue;
            }
            let mut downstream = BTreeSet::new();
            let mut pending = successors[index].clone();
            while let Some(next) = pending.pop() {
                if downstream.insert(next) {
                    pending.extend(&successors[next]);
                }
            }
            arrays.insert(index, downstream);
        }
        Self(arrays)
    }

    pub(super) fn contains(&self, action: usize) -> bool {
        self.0.contains_key(&action)
    }

    pub(super) fn follows(&self, producer: usize, action: usize) -> bool {
        self.0[&producer].contains(&action)
    }

    /// Ordinary planning already counts copies of original members. Charge
    /// nested copies before creating any geometry or allocating their handles.
    pub(super) fn reserve_nested(
        &self,
        all: &[(Handle, &BlockAction, Kind, i32)],
        order: &[usize],
        targets: &BTreeMap<Handle, Vec<Handle>>,
        plans: &BTreeMap<Handle, Plan>,
        remaining: &mut usize,
    ) -> Result<(), &'static str> {
        if self.0.is_empty() {
            return Ok(());
        }
        let ordered: Vec<_> = order
            .iter()
            .copied()
            .filter(|i| self.contains(*i))
            .collect();
        let sources: BTreeSet<_> = ordered
            .iter()
            .flat_map(|&i| targets[&all[i].0].iter().copied())
            .collect();
        for source in sources {
            let mut generated = Vec::new();
            for &index in &ordered {
                let handle = all[index].0;
                if !targets[&handle].contains(&source) {
                    continue;
                }
                let parents: usize = generated
                    .iter()
                    .filter(|&&(producer, _)| self.follows(producer, index))
                    .map(|&(_, count)| count)
                    .sum();
                let extra = plans[&handle].generated(parents)?;
                *remaining = remaining.checked_sub(extra).ok_or(LIMIT_ERROR)?;
                generated.push((index, plans[&handle].generated(1)? + extra));
            }
        }
        Ok(())
    }
}

pub(super) struct Copy {
    pub entity: EntityType,
    pub producer: usize,
    pub id: i32,
}

/// Keep stable IDs for ordered copy paths, while retaining the old IDs for
/// unchained arrays where they are unique.
pub(super) struct Tags(BTreeSet<i32>);

impl Tags {
    pub(super) fn new(doc: &Document, definition: Handle) -> Self {
        Self(
            authoring::members(doc, definition)
                .into_iter()
                .filter_map(|h| doc.get_entity(h).and_then(authoring::element_id))
                .collect(),
        )
    }

    pub(super) fn ordered(&mut self, source: u64, action: Handle, cell: usize) -> i32 {
        let key = source.wrapping_mul(0x9e3779b97f4a7c15)
            ^ action.value().rotate_left(23)
            ^ (cell as u64).wrapping_mul(0xbf58476d1ce4e5b9);
        let mixed = (key ^ (key >> 30)).wrapping_mul(0x94d049bb133111eb);
        let mut id = 1_000_000_000 + ((mixed ^ (mixed >> 32)) & 0x1fff_ffff) as i32;
        while !self.0.insert(id) {
            id += 1;
        }
        id
    }

    pub(super) fn legacy(
        &mut self,
        source: &EntityType,
        action: Handle,
        cell: usize,
    ) -> Option<i32> {
        let base = authoring::element_id(source)?;
        let id = base.wrapping_add((cell as i32).wrapping_mul(100_000));
        Some(if self.0.insert(id) {
            id
        } else {
            self.ordered(base as u64, action, cell)
        })
    }
}

pub(super) fn displaced(source: &EntityType, delta: DVec3) -> Result<EntityType, &'static str> {
    let mut copy = source.clone();
    super::super::view::dispatch::apply_transform(
        &mut copy,
        &crate::command::EntityTransform::Translate(delta),
    );
    let bounds = copy.as_entity().bounding_box();
    if !v(bounds.min).is_finite() || !v(bounds.max).is_finite() {
        return Err("The parameter value produces non-finite geometry.");
    }
    Ok(copy)
}

pub(super) fn publish(
    doc: &mut Document,
    definition: Handle,
    mut copy: EntityType,
    id: Option<i32>,
) -> Result<(), &'static str> {
    let handle = doc.allocate_handle();
    copy.common_mut().handle = handle;
    copy.common_mut().owner_handle = definition;
    copy.common_mut().raw_record = None;
    copy.common_mut().xdictionary_handle = None;
    copy.common_mut().reactors.clear();
    crate::scene::Scene::reset_clone_subhandles(doc, &mut copy);
    doc.add_entity(copy)
        .map_err(|_| "Could not add array item.")?;
    if let Some(id) = id {
        authoring::set_entity_tag(doc, handle, authoring::element_tag(id, handle));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
