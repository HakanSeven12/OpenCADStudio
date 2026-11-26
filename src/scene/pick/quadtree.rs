// Spatial index for 2D entities. Each leaf holds up to `LEAF_CAPACITY`
// items; on overflow the node splits into 4 children and re-distributes.
// Straddling items that accumulate at internal nodes beyond `LEAF_CAPACITY`
// are promoted into a node-local `rstar::RTree` so straddler-heavy drawings
// keep O(log N) query times while ordinary bounded entities enjoy fast O(1)
// quadrant descent.
//
// Items are keyed by `document::Handle`. AABBs are stored in WCS f64
// (NOT world_offset-subtracted) so changing `world_offset` doesn't
// invalidate the index.
//
// API:
//   - `QuadTree::new(world_bounds)` — build an empty tree spanning a
//     fixed root rect. Items outside the root are clamped: they go into
//     the root's overflow list and surface on every query.
//   - `QuadTree::from_items(world_bounds, items)` — bulk-build a tree
//     and pack internal straddler buckets into R*-trees via `RTree::bulk_load`.
//   - `insert(handle, aabb)`, `remove(handle)`, `update(handle, aabb)`
//   - `query_rect(aabb) -> Vec<Handle>` — returns every handle whose
//     stored AABB intersects `aabb`, plus all overflow handles.
//   - `len()` / `is_empty()` for sanity.

use document::Handle;
use rstar::{RTree, RTreeObject, AABB};

pub type Aabb = [f64; 4]; // [xmin, ymin, xmax, ymax]

/// Items per leaf before it splits.
const LEAF_CAPACITY: usize = 32;

/// Minimum straddlers at an internal node before building a node-local R*-tree.
const STRADDLER_RTREE_THRESHOLD: usize = 64;

/// Hard cap on tree depth — prevents pathological recursion when many
/// items pile up at the same point.
const MAX_DEPTH: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Item {
    handle: Handle,
    aabb: Aabb,
}

impl RTreeObject for Item {
    type Envelope = AABB<[f64; 2]>;

    #[inline]
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.aabb[0], self.aabb[1]], [self.aabb[2], self.aabb[3]])
    }
}

#[derive(Debug, Clone)]
struct Node {
    bounds: Aabb,
    /// Items stored at this node (either a leaf's items, or straddlers at
    /// an internal node when below `STRADDLER_RTREE_THRESHOLD`).
    items: Vec<Item>,
    /// When an internal node accumulates many straddling items (e.g., long
    /// grid/section lines crossing quadrant midlines), they are indexed in
    /// an R*-tree (`rstar::RTree`) instead of scanned linearly on every query.
    straddler_rtree: Option<RTree<Item>>,
    /// 4 children: NW, NE, SW, SE. `None` until the node splits.
    children: Option<[u32; 4]>,
    depth: u8,
}

#[derive(Debug, Clone)]
pub struct QuadTree {
    nodes: Vec<Node>,
    /// `handle → (node_idx, item_idx_within_node)` for O(1) lookup.
    /// When `item_idx == u32::MAX`, the item is stored in `nodes[node_idx].straddler_rtree`.
    locator: rustc_hash::FxHashMap<Handle, (u32, u32)>,
    /// Stored AABB for items promoted into a node's `straddler_rtree`.
    rtree_aabbs: rustc_hash::FxHashMap<Handle, Aabb>,
    /// Items whose AABB falls outside the root bounds. Surfaced on
    /// every query — small set in practice (typically empty for
    /// well-bounded drawings).
    overflow: Vec<Item>,
}

impl QuadTree {
    pub fn new(world_bounds: Aabb) -> Self {
        let root = Node {
            bounds: world_bounds,
            items: Vec::new(),
            straddler_rtree: None,
            children: None,
            depth: 0,
        };
        Self {
            nodes: vec![root],
            locator: rustc_hash::FxHashMap::default(),
            rtree_aabbs: rustc_hash::FxHashMap::default(),
            overflow: Vec::new(),
        }
    }

    pub fn from_items(
        world_bounds: Aabb,
        items: impl IntoIterator<Item = (Handle, Aabb)>,
    ) -> Self {
        let iter = items.into_iter();
        let cap = iter.size_hint().0;
        let mut tree = Self::new(world_bounds);
        tree.locator.reserve(cap);
        for (handle, aabb) in iter {
            tree.insert_without_rtree_promote(handle, aabb);
        }
        // Bulk-load any internal node that accumulated >= STRADDLER_RTREE_THRESHOLD straddlers.
        for node_idx in 0..tree.nodes.len() {
            if tree.nodes[node_idx].children.is_some()
                && tree.nodes[node_idx].items.len() >= STRADDLER_RTREE_THRESHOLD
            {
                tree.promote_node_straddlers(node_idx as u32);
            }
        }
        tree
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.locator.len() + self.overflow.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&mut self, handle: Handle, aabb: Aabb) {
        let node_idx = self.insert_without_rtree_promote(handle, aabb);
        if let Some(idx) = node_idx {
            if self.nodes[idx as usize].children.is_some()
                && self.nodes[idx as usize].straddler_rtree.is_none()
                && self.nodes[idx as usize].items.len() >= STRADDLER_RTREE_THRESHOLD
            {
                self.promote_node_straddlers(idx);
            }
        }
    }

    fn insert_without_rtree_promote(&mut self, handle: Handle, aabb: Aabb) -> Option<u32> {
        let aabb = normalize_aabb(aabb);
        if !aabb.iter().all(|x| x.is_finite()) || !aabb_contained(self.nodes[0].bounds, aabb) {
            self.overflow.push(Item { handle, aabb });
            return None;
        }
        let node_idx = self.descend_to_leaf(0, aabb);
        let node = &mut self.nodes[node_idx as usize];
        if let Some(rtree) = node.straddler_rtree.as_mut() {
            rtree.insert(Item { handle, aabb });
            self.locator.insert(handle, (node_idx, u32::MAX));
            self.rtree_aabbs.insert(handle, aabb);
            return Some(node_idx);
        }
        let item_idx = node.items.len() as u32;
        node.items.push(Item { handle, aabb });
        self.locator.insert(handle, (node_idx, item_idx));

        if self.nodes[node_idx as usize].items.len() > LEAF_CAPACITY
            && self.nodes[node_idx as usize].depth < MAX_DEPTH
            && self.nodes[node_idx as usize].children.is_none()
        {
            self.split(node_idx);
        }
        Some(node_idx)
    }

    fn promote_node_straddlers(&mut self, node_idx: u32) {
        let items = std::mem::take(&mut self.nodes[node_idx as usize].items);
        self.rtree_aabbs.reserve(items.len());
        for item in &items {
            self.locator.insert(item.handle, (node_idx, u32::MAX));
            self.rtree_aabbs.insert(item.handle, item.aabb);
        }
        self.nodes[node_idx as usize].straddler_rtree = Some(RTree::bulk_load(items));
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, handle: Handle) -> bool {
        if let Some((node_idx, item_idx)) = self.locator.remove(&handle) {
            if item_idx == u32::MAX {
                if let Some(aabb) = self.rtree_aabbs.remove(&handle) {
                    if let Some(rtree) = self.nodes[node_idx as usize].straddler_rtree.as_mut() {
                        rtree.remove(&Item { handle, aabb });
                    }
                }
            } else {
                let node = &mut self.nodes[node_idx as usize];
                node.items.swap_remove(item_idx as usize);
                if let Some(moved) = node.items.get(item_idx as usize) {
                    self.locator.insert(moved.handle, (node_idx, item_idx));
                }
            }
            return true;
        }
        // Try overflow.
        if let Some(pos) = self.overflow.iter().position(|i| i.handle == handle) {
            self.overflow.swap_remove(pos);
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub fn update(&mut self, handle: Handle, aabb: Aabb) {
        self.remove(handle);
        self.insert(handle, aabb);
    }

    pub fn query_rect(&self, query: Aabb) -> Vec<Handle> {
        let mut out = Vec::new();
        for i in &self.overflow {
            if aabb_intersects(i.aabb, query) {
                out.push(i.handle);
            }
        }
        let envelope = AABB::from_corners([query[0], query[1]], [query[2], query[3]]);
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node_idx = stack[sp];
            let node = &self.nodes[node_idx as usize];
            if !aabb_intersects(node.bounds, query) {
                continue;
            }
            if let Some(rtree) = &node.straddler_rtree {
                out.extend(
                    rtree
                        .locate_in_envelope_intersecting(envelope)
                        .map(|item| item.handle),
                );
            } else {
                for i in &node.items {
                    if aabb_intersects(i.aabb, query) {
                        out.push(i.handle);
                    }
                }
            }
            if let Some(children) = node.children {
                for c in children {
                    stack[sp] = c;
                    sp += 1;
                }
            }
        }
        out
    }

    // ── internal helpers ────────────────────────────────────────────

    fn descend_to_leaf(&self, mut node_idx: u32, aabb: Aabb) -> u32 {
        loop {
            let node = &self.nodes[node_idx as usize];
            let Some(children) = node.children else {
                return node_idx;
            };
            let mut fits_child: Option<u32> = None;
            for &c in &children {
                if aabb_contained(self.nodes[c as usize].bounds, aabb) {
                    fits_child = Some(c);
                    break;
                }
            }
            match fits_child {
                Some(c) => node_idx = c,
                None => return node_idx,
            }
        }
    }

    fn split(&mut self, node_idx: u32) {
        let bounds = self.nodes[node_idx as usize].bounds;
        let depth = self.nodes[node_idx as usize].depth + 1;
        let [xmin, ymin, xmax, ymax] = bounds;
        let mx = (xmin + xmax) * 0.5;
        let my = (ymin + ymax) * 0.5;
        // NW, NE, SW, SE
        let child_bounds = [
            [xmin, my, mx, ymax],
            [mx, my, xmax, ymax],
            [xmin, ymin, mx, my],
            [mx, ymin, xmax, my],
        ];
        let first_child = self.nodes.len() as u32;
        for &b in &child_bounds {
            self.nodes.push(Node {
                bounds: b,
                items: Vec::new(),
                straddler_rtree: None,
                children: None,
                depth,
            });
        }
        let children = [
            first_child,
            first_child + 1,
            first_child + 2,
            first_child + 3,
        ];
        self.nodes[node_idx as usize].children = Some(children);

        // Redistribute existing items. Straddlers stay at the parent.
        let existing = std::mem::take(&mut self.nodes[node_idx as usize].items);
        let mut keep_at_parent = Vec::new();
        for item in existing {
            let mut placed = false;
            for &c in &children {
                if aabb_contained(self.nodes[c as usize].bounds, item.aabb) {
                    let idx = self.nodes[c as usize].items.len() as u32;
                    self.nodes[c as usize].items.push(item);
                    self.locator.insert(item.handle, (c, idx));
                    placed = true;
                    break;
                }
            }
            if !placed {
                let idx = keep_at_parent.len() as u32;
                self.locator.insert(item.handle, (node_idx, idx));
                keep_at_parent.push(item);
            }
        }
        self.nodes[node_idx as usize].items = keep_at_parent;
    }
}

// ── AABB helpers ────────────────────────────────────────────────────

pub fn normalize_aabb(mut aabb: Aabb) -> Aabb {
    if aabb[0] > aabb[2] {
        aabb.swap(0, 2);
    }
    if aabb[1] > aabb[3] {
        aabb.swap(1, 3);
    }
    aabb
}

fn aabb_contained(outer: Aabb, inner: Aabb) -> bool {
    inner[0] >= outer[0] && inner[1] >= outer[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

fn aabb_intersects(a: Aabb, b: Aabb) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: u64) -> Handle {
        Handle::from(v)
    }

    #[test]
    fn empty_tree_returns_nothing() {
        let t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        assert!(t.is_empty());
        assert!(t.query_rect([0.0, 0.0, 100.0, 100.0]).is_empty());
    }

    #[test]
    fn insert_and_query_hit() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        t.insert(h(1), [10.0, 10.0, 20.0, 20.0]);
        let hits = t.query_rect([0.0, 0.0, 50.0, 50.0]);
        assert_eq!(hits, vec![h(1)]);
    }

    #[test]
    fn query_miss() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        t.insert(h(1), [10.0, 10.0, 20.0, 20.0]);
        let hits = t.query_rect([50.0, 50.0, 90.0, 90.0]);
        assert!(hits.is_empty());
    }

    #[test]
    fn split_on_overflow() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        for i in 0..40u64 {
            let x = (i as f64) * 0.5;
            let y = (i as f64) * 0.5;
            t.insert(h(i), [x, y, x + 0.1, y + 0.1]);
        }
        assert_eq!(t.len(), 40);
        assert!(t.nodes.len() > 1);
        let hits = t.query_rect([0.0, 0.0, 100.0, 100.0]);
        assert_eq!(hits.len(), 40);
    }

    #[test]
    fn remove_works() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        t.insert(h(1), [10.0, 10.0, 20.0, 20.0]);
        t.insert(h(2), [30.0, 30.0, 40.0, 40.0]);
        assert!(t.remove(h(1)));
        assert!(!t.remove(h(1))); // already gone
        let hits = t.query_rect([0.0, 0.0, 100.0, 100.0]);
        assert_eq!(hits, vec![h(2)]);
    }

    #[test]
    fn update_moves_item() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        t.insert(h(1), [10.0, 10.0, 20.0, 20.0]);
        t.update(h(1), [70.0, 70.0, 80.0, 80.0]);
        let lo = t.query_rect([0.0, 0.0, 50.0, 50.0]);
        let hi = t.query_rect([60.0, 60.0, 90.0, 90.0]);
        assert!(lo.is_empty());
        assert_eq!(hi, vec![h(1)]);
    }

    #[test]
    fn out_of_bounds_goes_to_overflow() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        t.insert(h(1), [1000.0, 1000.0, 1010.0, 1010.0]);
        let near = t.query_rect([0.0, 0.0, 50.0, 50.0]);
        assert!(near.is_empty());
        let far = t.query_rect([900.0, 900.0, 1100.0, 1100.0]);
        assert_eq!(far, vec![h(1)]);
    }

    #[test]
    fn many_straddlers_dont_orphan_subtree() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        for i in 0..40u64 {
            t.insert(h(i), [1.0, 1.0, 2.0, 2.0]);
        }
        for i in 100..200u64 {
            t.insert(h(i), [10.0, 10.0, 90.0, 90.0]);
        }
        let huge = [-1e9, -1e9, 1e9, 1e9];
        let hits = t.query_rect(huge);
        let expected = 40 + 100;
        assert_eq!(
            hits.len(),
            expected,
            "lost items: tree.len={} hits.len={}",
            t.len(),
            hits.len()
        );
    }

    #[test]
    fn straddler_stays_at_parent() {
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        for i in 0..40u64 {
            t.insert(h(i + 100), [1.0, 1.0, 2.0, 2.0]);
        }
        t.insert(h(1), [40.0, 40.0, 60.0, 60.0]);
        let nw_hits = t.query_rect([0.0, 50.0, 50.0, 100.0]);
        assert!(nw_hits.contains(&h(1)));
        let se_hits = t.query_rect([50.0, 0.0, 100.0, 50.0]);
        assert!(se_hits.contains(&h(1)));
    }

    #[test]
    fn insert_handles_nan_and_infinite_bounds_safely() {
        let root = [0.0, 0.0, 100.0, 100.0];
        let mut tree = QuadTree::new(root);
        tree.insert(h(1), [10.0, 10.0, 20.0, 20.0]);
        tree.insert(h(2), [f64::NAN, 10.0, 20.0, 20.0]);
        tree.insert(h(3), [10.0, f64::INFINITY, 20.0, 20.0]);
        tree.insert(h(4), [20.0, 20.0, 10.0, 10.0]);

        assert_eq!(tree.len(), 4);
        assert_eq!(tree.overflow.len(), 2);

        let normal_hits = tree.query_rect([5.0, 5.0, 25.0, 25.0]);
        assert!(normal_hits.contains(&h(1)));
        assert!(normal_hits.contains(&h(4)));
    }

    #[test]
    fn normalize_aabb_flips_inverted_extents() {
        let inverted = [50.0, 60.0, 10.0, 20.0];
        let normalized = normalize_aabb(inverted);
        assert_eq!(normalized, [10.0, 20.0, 50.0, 60.0]);
    }
}
