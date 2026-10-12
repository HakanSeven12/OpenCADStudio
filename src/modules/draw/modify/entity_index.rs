//! Shared broad phase for interactive modify commands.
//!
//! The scene already narrows the cursor pick. Commands still need fast access
//! to the picked analytic entity and, for intersection-heavy operations such
//! as TRIM, to nearby boundary entities. Keeping this compact command-local
//! index avoids cloning/scanning the complete drawing on every mouse move.

use codec::{EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::scene::convert::tess::entity_world_aabb_f64;
use crate::scene::pick::quadtree::QuadTree;

pub(super) struct ModifyEntityIndex {
    by_handle: FxHashMap<Handle, usize>,
    tree: Option<QuadTree>,
    unbounded: Vec<Handle>,
}

impl ModifyEntityIndex {
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
            if let Some(aabb) = entity_world_aabb_f64(entity) {
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

    pub(super) fn nearby_handles(
        &self,
        entities: &[EntityType],
        handle: Handle,
    ) -> Option<FxHashSet<Handle>> {
        let entity = self.get(entities, handle)?;
        let aabb = entity_world_aabb_f64(entity)?;
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
