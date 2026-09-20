// Spatial index for 2D entities. Each leaf holds up to `LEAF_CAPACITY`
// items; on overflow the node splits into 4 children and re-distributes.
//
// Items are keyed by `acadrust::Handle`. AABBs are stored in WCS f64
// (NOT world_offset-subtracted) so changing `world_offset` doesn't
// invalidate the index.
//
// API:
//   - `QuadTree::new(world_bounds)` — build an empty tree spanning a
//     fixed root rect. Items outside the root are clamped: they go into
//     the root's overflow list and surface on every query.
//   - `insert(handle, aabb)`, `remove(handle)`, `update(handle, aabb)`
//   - `query_rect(aabb) -> Vec<Handle>` — returns every handle whose
//     stored AABB intersects `aabb`, plus all overflow handles.
//   - `len()` / `is_empty()` for sanity.
//
// This is a flat-vector (Vec-of-nodes) implementation: node indices
// are `usize`, child links are `Option<u32>` to keep nodes compact and
// cache-friendly. No rebalancing — once split, nodes don't merge back
// (rare in CAD workflows where most edits are local and few entities
// move long distances).

use acadrust::Handle;

pub type Aabb = [f64; 4]; // [xmin, ymin, xmax, ymax]

/// Items per leaf before it splits. Tuned: too small → deep tree, lots
/// of nodes; too large → poor culling per leaf. 32 is a typical sweet
/// spot for CAD-style content (many small entities in clusters).
const LEAF_CAPACITY: usize = 32;

/// Hard cap on tree depth — prevents pathological recursion when many
/// items pile up at the same point.
const MAX_DEPTH: u8 = 16;

#[derive(Debug, Clone, Copy)]
struct Item {
    handle: Handle,
    aabb: Aabb,
}

#[derive(Debug)]
struct Node {
    bounds: Aabb,
    /// Items stored at this node. Only populated when the node is a
    /// leaf, OR when a child split would put a straddling item at the
    /// parent level (straddlers stay at the current node — they don't
    /// fit cleanly into one quadrant).
    items: Vec<Item>,
    /// 4 children: NW, NE, SW, SE. `None` until the node splits.
    children: Option<[u32; 4]>,
    depth: u8,
}

#[derive(Debug)]
pub struct QuadTree {
    nodes: Vec<Node>,
    /// `handle → (node_idx, item_idx_within_node)` so removal/update
    /// is O(1) without walking the tree.
    locator: rustc_hash::FxHashMap<Handle, (u32, u32)>,
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
            children: None,
            depth: 0,
        };
        Self {
            nodes: vec![root],
            locator: rustc_hash::FxHashMap::default(),
            overflow: Vec::new(),
        }
    }

    // `len` / `is_empty` / `remove` / `update` are part of the public
    // quadtree API and exercised by the unit tests, but production
    // callers haven't landed yet (Scene::add/erase/transform paths
    // still trigger an epoch-based full rebuild rather than mutating
    // the index in place). The `#[allow(dead_code)]` is intentional —
    // dropping them would shrink the public surface only to put it
    // back once incremental updates ship.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.locator.len() + self.overflow.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&mut self, handle: Handle, aabb: Aabb) {
        if !aabb_contained(self.nodes[0].bounds, aabb) {
            self.overflow.push(Item { handle, aabb });
            return;
        }
        let node_idx = self.descend_to_leaf(0, aabb);
        let item_idx = self.nodes[node_idx as usize].items.len() as u32;
        self.nodes[node_idx as usize]
            .items
            .push(Item { handle, aabb });
        self.locator.insert(handle, (node_idx, item_idx));

        // Split only LEAF nodes. Internal nodes accumulate straddlers
        // (items whose AABB crosses a child boundary); calling split on
        // an internal node would overwrite its `children` pointer and
        // orphan the entire existing subtree. Straddlers can't fit
        // smaller children anyway, so splitting is futile here.
        if self.nodes[node_idx as usize].items.len() > LEAF_CAPACITY
            && self.nodes[node_idx as usize].depth < MAX_DEPTH
            && self.nodes[node_idx as usize].children.is_none()
        {
            self.split(node_idx);
        }
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, handle: Handle) -> bool {
        if let Some((node_idx, item_idx)) = self.locator.remove(&handle) {
            let node = &mut self.nodes[node_idx as usize];
            // swap_remove: re-locate whoever took the moved item's slot.
            node.items.swap_remove(item_idx as usize);
            if let Some(moved) = node.items.get(item_idx as usize) {
                self.locator.insert(moved.handle, (node_idx, item_idx));
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
        // Overflow always surfaces — these items have no usable spatial
        // info (out-of-root) so we can't cull them spatially.
        for i in &self.overflow {
            if aabb_intersects(i.aabb, query) {
                out.push(i.handle);
            }
        }
        self.query_node(0, query, &mut out);
        out
    }

    // ── internal helpers ────────────────────────────────────────────

    fn descend_to_leaf(&self, mut node_idx: u32, aabb: Aabb) -> u32 {
        loop {
            let node = &self.nodes[node_idx as usize];
            let Some(children) = node.children else {
                return node_idx;
            };
            // Pick the child that fully contains `aabb`; if `aabb`
            // straddles a split boundary, stash it at the current node.
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

    fn query_node(&self, node_idx: u32, query: Aabb, out: &mut Vec<Handle>) {
        let node = &self.nodes[node_idx as usize];
        if !aabb_intersects(node.bounds, query) {
            return;
        }
        for i in &node.items {
            if aabb_intersects(i.aabb, query) {
                out.push(i.handle);
            }
        }
        if let Some(children) = node.children {
            for c in children {
                self.query_node(c, query, out);
            }
        }
    }
}

// ── AABB helpers ────────────────────────────────────────────────────

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
        // Push beyond LEAF_CAPACITY (=32) into different quadrants
        // so the split actually distributes them.
        for i in 0..40u64 {
            let x = (i as f64) * 0.5;
            let y = (i as f64) * 0.5;
            t.insert(h(i), [x, y, x + 0.1, y + 0.1]);
        }
        assert_eq!(t.len(), 40);
        // The root should have split — there are children now.
        assert!(t.nodes.len() > 1);
        // All items recoverable.
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
        // Way outside root.
        t.insert(h(1), [1000.0, 1000.0, 1010.0, 1010.0]);
        // Query a region that doesn't touch the overflow item — but
        // overflow always surfaces only if its AABB intersects.
        let near = t.query_rect([0.0, 0.0, 50.0, 50.0]);
        assert!(near.is_empty());
        // Query that does intersect the overflow item.
        let far = t.query_rect([900.0, 900.0, 1100.0, 1100.0]);
        assert_eq!(far, vec![h(1)]);
    }

    #[test]
    fn many_straddlers_dont_orphan_subtree() {
        // Regression: previously, when straddlers piled up at an
        // internal node and exceeded LEAF_CAPACITY, split() was called
        // again — overwriting the node's `children` and orphaning every
        // descendant. Symptom: thousands of items inserted, only ~5%
        // reachable from root via a huge_query.
        let mut t = QuadTree::new([0.0, 0.0, 100.0, 100.0]);
        // First, fill one leaf so it splits.
        for i in 0..40u64 {
            t.insert(h(i), [1.0, 1.0, 2.0, 2.0]);
        }
        // Now flood the root with straddlers (AABBs that cross center).
        for i in 100..200u64 {
            t.insert(h(i), [10.0, 10.0, 90.0, 90.0]);
        }
        // After the regression-trigger inserts, every handle must still
        // come back from a huge query.
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
        // Force a split via 40 small items in NW quadrant.
        for i in 0..40u64 {
            t.insert(h(i + 100), [1.0, 1.0, 2.0, 2.0]);
        }
        // A straddler crossing center.
        t.insert(h(1), [40.0, 40.0, 60.0, 60.0]);
        // Querying any quadrant should still find the straddler.
        let nw_hits = t.query_rect([0.0, 50.0, 50.0, 100.0]);
        assert!(nw_hits.contains(&h(1)));
        let se_hits = t.query_rect([50.0, 0.0, 100.0, 50.0]);
        assert!(se_hits.contains(&h(1)));
    }
}
