//! Shared broad phase for interactive modify commands.
//!
//! The scene already narrows the cursor pick. Commands still need fast access
//! to the picked analytic entity and, for intersection-heavy operations such
//! as TRIM, to nearby boundary entities. Keeping this compact command-local
//! index avoids cloning/scanning the complete drawing on every mouse move.

use document::{EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::scene::convert::tess::{entity_bounds, entity_world_aabb_f64};
use crate::scene::pick::quadtree::QuadTree;

/// [`entity_world_aabb_f64`], except that a dot-sized LINE, polyline or
/// POINT keeps its one-point box. The shared helper reads a zero box as "no
/// box", so every zero-length polyline counted as unbounded and TRIM tested
/// it against all boundaries of the drawing on each hover (#3184).
pub(super) fn modify_aabb(entity: &EntityType) -> Option<[f64; 4]> {
    entity_world_aabb_f64(entity).or_else(|| {
        if !matches!(
            entity,
            EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Point(_)
        ) {
            return None;
        }
        let (min, max) = entity_bounds(entity);
        let aabb = [min[0], min[1], max[0], max[1]];
        aabb.iter().all(|value| value.is_finite()).then_some(aabb)
    })
}

pub(crate) struct ModifyEntityIndex {
    by_handle: FxHashMap<Handle, usize>,
    tree: Option<QuadTree>,
    unbounded: Vec<Handle>,
}

impl ModifyEntityIndex {
    pub(super) fn empty() -> Self {
        Self {
            by_handle: FxHashMap::default(),
            tree: None,
            unbounded: Vec::new(),
        }
    }

    pub(super) fn build(entities: &[EntityType]) -> Self {
        let mut by_handle = FxHashMap::default();
        let mut bounded = Vec::new();
        let mut unbounded = Vec::new();
        let mut world = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];

        for (index, entity) in entities.iter().enumerate() {
            let handle = entity.common().handle;
            by_handle.insert(handle, index);
            if let Some(aabb) = modify_aabb(entity) {
                world[0] = world[0].min(aabb[0]);
                world[1] = world[1].min(aabb[1]);
                world[2] = world[2].max(aabb[2]);
                world[3] = world[3].max(aabb[3]);
                bounded.push((handle, aabb));
            } else {
                unbounded.push(handle);
            }
        }

        let tree = if bounded.is_empty() {
            None
        } else {
            let span = (world[2] - world[0]).max(world[3] - world[1]).max(1.0);
            let pad = span * 1.0e-9 + 1.0e-6;
            let mut tree = QuadTree::new([
                world[0] - pad,
                world[1] - pad,
                world[2] + pad,
                world[3] + pad,
            ]);
            for (handle, aabb) in bounded {
                tree.insert(handle, aabb);
            }
            Some(tree)
        };

        Self {
            by_handle,
            tree,
            unbounded,
        }
    }

    #[inline]
    pub(super) fn get<'a>(
        &self,
        entities: &'a [EntityType],
        handle: Handle,
    ) -> Option<&'a EntityType> {
        self.by_handle
            .get(&handle)
            .and_then(|index| entities.get(*index))
    }

    /// Take `handle` out of `entities` and the index. The last entity moves
    /// into its slot, so nothing else is re-indexed.
    pub(super) fn remove(&mut self, entities: &mut Vec<EntityType>, handle: Handle) {
        let Some(index) = self.by_handle.remove(&handle) else {
            return;
        };
        entities.swap_remove(index);
        if let Some(moved) = entities.get(index) {
            self.by_handle.insert(moved.common().handle, index);
        }
        if let Some(tree) = self.tree.as_mut() {
            tree.remove(handle);
        }
        self.unbounded.retain(|other| *other != handle);
    }

    /// Append `entity` to `entities` and index it in place.
    pub(super) fn push(&mut self, entities: &mut Vec<EntityType>, entity: EntityType) {
        let handle = entity.common().handle;
        self.by_handle.insert(handle, entities.len());
        match modify_aabb(&entity) {
            // Outside the tree's bounds it lands in the tree's overflow list,
            // which every query still returns.
            Some(aabb) => self
                .tree
                .get_or_insert_with(|| QuadTree::new(aabb))
                .insert(handle, aabb),
            None => self.unbounded.push(handle),
        }
        entities.push(entity);
    }

    /// Query a selection rectangle against the already-built command index.
    /// Entities without a finite AABB remain candidates for correctness
    /// (rays, XLINEs and other unbounded/unsupported shapes).
    pub(super) fn handles_in_aabb(&self, aabb: [f64; 4]) -> FxHashSet<Handle> {
        let mut handles: FxHashSet<Handle> = self
            .tree
            .as_ref()
            .map(|tree| tree.query_rect(aabb).into_iter().collect())
            .unwrap_or_default();
        handles.extend(self.unbounded.iter().copied());
        handles
    }

    pub(super) fn nearby_handles(
        &self,
        entities: &[EntityType],
        handle: Handle,
    ) -> Option<FxHashSet<Handle>> {
        let entity = self.get(entities, handle)?;
        let aabb = modify_aabb(entity)?;
        let span = (aabb[2] - aabb[0]).max(aabb[3] - aabb[1]).max(1.0);
        let pad = span * 1.0e-10 + 1.0e-7;
        let query = [aabb[0] - pad, aabb[1] - pad, aabb[2] + pad, aabb[3] + pad];
        let mut handles: FxHashSet<Handle> = self
            .tree
            .as_ref()
            .map(|tree| tree.query_rect(query).into_iter().collect())
            .unwrap_or_default();
        handles.extend(self.unbounded.iter().copied());
        Some(handles)
    }
}
