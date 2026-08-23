//! Shared spatial broad phase for cursor and area interactions.
//!
//! Exact snap and selection rules remain in their consumers. This index only
//! narrows large resident wire sets to local wire/sub-primitive candidates:
//! compact world-XY grids for plan views and projected 3D BVHs for tilted or
//! perspective views.

use crate::scene::model::wire_model::WireModel;
use glam::{DVec3, Mat4};
use iced::Rectangle;
use std::sync::Arc;

const TARGET_ENTRIES_PER_CELL: usize = 8;
const MAX_SPAN_CELLS: u64 = 64;
const MAX_AXIS_CELLS: u32 = 8_192;
const BVH_LEAF_SIZE: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SegmentRef {
    pub wire: u32,
    pub start: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SnapPointRef {
    pub wire: u32,
    pub index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyVertexRef {
    pub wire: u32,
    pub index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeySegmentRef {
    pub wire: u32,
    pub start: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TriangleRef {
    pub wire: u32,
    pub start: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphRef {
    pub wire: u32,
    pub start: u32,
}

#[derive(Clone, Copy)]
struct Entry3<T> {
    aabb: [f64; 6],
    value: T,
}

fn entry_aabb2<T>(entry: &Entry3<T>) -> [f64; 4] {
    [entry.aabb[0], entry.aabb[1], entry.aabb[3], entry.aabb[4]]
}

#[derive(Clone, Copy)]
struct BvhNode2 {
    aabb: [f64; 4],
    left: u32,
    right: u32,
    start: u32,
    len: u32,
}

struct SpatialBvh2 {
    nodes: Vec<BvhNode2>,
    order: Vec<u32>,
}

impl SpatialBvh2 {
    fn build<T>(entries: &[Entry3<T>], mut order: Vec<u32>) -> Self {
        let mut nodes = Vec::new();
        if !order.is_empty() {
            build_bvh2_node(entries, &mut order, 0, &mut nodes);
        }
        Self { nodes, order }
    }

    fn query<T>(&self, entries: &[Entry3<T>], query: [f64; 4], out: &mut Vec<u32>) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(node_idx) = stack.pop() {
            let node = self.nodes[node_idx as usize];
            if !aabb_overlaps(node.aabb, query) {
                continue;
            }
            if node.left == u32::MAX {
                for &entry_idx in &self.order[node.start as usize..(node.start + node.len) as usize]
                {
                    if aabb_overlaps(entry_aabb2(&entries[entry_idx as usize]), query) {
                        out.push(entry_idx);
                    }
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
    }
}

fn build_bvh2_node<T>(
    entries: &[Entry3<T>],
    order: &mut [u32],
    base: usize,
    nodes: &mut Vec<BvhNode2>,
) -> u32 {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &idx in order.iter() {
        let aabb = entry_aabb2(&entries[idx as usize]);
        bounds[0] = bounds[0].min(aabb[0]);
        bounds[1] = bounds[1].min(aabb[1]);
        bounds[2] = bounds[2].max(aabb[2]);
        bounds[3] = bounds[3].max(aabb[3]);
    }
    let node_idx = nodes.len() as u32;
    nodes.push(BvhNode2 {
        aabb: bounds,
        left: u32::MAX,
        right: u32::MAX,
        start: base as u32,
        len: order.len() as u32,
    });
    if order.len() <= BVH_LEAF_SIZE {
        return node_idx;
    }
    let axis = usize::from(bounds[3] - bounds[1] > bounds[2] - bounds[0]);
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        let aa = entry_aabb2(&entries[a as usize]);
        let bb = entry_aabb2(&entries[b as usize]);
        (aa[axis] + aa[axis + 2]).total_cmp(&(bb[axis] + bb[axis + 2]))
    });
    let (left_order, right_order) = order.split_at_mut(mid);
    let left = build_bvh2_node(entries, left_order, base, nodes);
    let right = build_bvh2_node(entries, right_order, base + mid, nodes);
    nodes[node_idx as usize].left = left;
    nodes[node_idx as usize].right = right;
    node_idx
}

#[derive(Clone, Copy)]
struct BvhNode3 {
    aabb: [f64; 6],
    left: u32,
    right: u32,
    start: u32,
    len: u32,
}

struct SpatialBvh3 {
    nodes: Vec<BvhNode3>,
    order: Vec<u32>,
}

impl SpatialBvh3 {
    fn build<T>(entries: &[Entry3<T>]) -> Self {
        let mut order: Vec<u32> = (0..entries.len() as u32).collect();
        let mut nodes = Vec::new();
        if !order.is_empty() {
            build_bvh3_node(entries, &mut order, 0, &mut nodes);
        }
        Self { nodes, order }
    }

    fn query_screen<T: Copy + Ord>(
        &self,
        entries: &[Entry3<T>],
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> Vec<T> {
        if self.nodes.is_empty() {
            return Vec::new();
        }
        let mut stack = vec![0u32];
        let mut entry_indices = Vec::new();
        while let Some(node_idx) = stack.pop() {
            let node = self.nodes[node_idx as usize];
            if !aabb3_projects_into(node.aabb, screen_rect, view_rot, eye, bounds) {
                continue;
            }
            if node.left == u32::MAX {
                for &entry_idx in &self.order[node.start as usize..(node.start + node.len) as usize]
                {
                    let entry = entries[entry_idx as usize];
                    if aabb3_projects_into(entry.aabb, screen_rect, view_rot, eye, bounds) {
                        entry_indices.push(entry.value);
                    }
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        entry_indices.sort_unstable();
        entry_indices.dedup();
        entry_indices
    }
}

fn build_bvh3_node<T>(
    entries: &[Entry3<T>],
    order: &mut [u32],
    base: usize,
    nodes: &mut Vec<BvhNode3>,
) -> u32 {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &idx in order.iter() {
        let aabb = entries[idx as usize].aabb;
        for axis in 0..3 {
            bounds[axis] = bounds[axis].min(aabb[axis]);
            bounds[axis + 3] = bounds[axis + 3].max(aabb[axis + 3]);
        }
    }
    let node_idx = nodes.len() as u32;
    nodes.push(BvhNode3 {
        aabb: bounds,
        left: u32::MAX,
        right: u32::MAX,
        start: base as u32,
        len: order.len() as u32,
    });
    if order.len() <= BVH_LEAF_SIZE {
        return node_idx;
    }
    let mut axis = 0;
    for candidate in 1..3 {
        if bounds[candidate + 3] - bounds[candidate] > bounds[axis + 3] - bounds[axis] {
            axis = candidate;
        }
    }
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        let aa = entries[a as usize].aabb;
        let bb = entries[b as usize].aabb;
        (aa[axis] + aa[axis + 3]).total_cmp(&(bb[axis] + bb[axis + 3]))
    });
    let (left_order, right_order) = order.split_at_mut(mid);
    let left = build_bvh3_node(entries, left_order, base, nodes);
    let right = build_bvh3_node(entries, right_order, base + mid, nodes);
    nodes[node_idx as usize].left = left;
    nodes[node_idx as usize].right = right;
    node_idx
}

struct SpatialGrid {
    min: [f64; 2],
    cell: f64,
    cols: u32,
    rows: u32,
    cell_offsets: Vec<u32>,
    cell_entries: Vec<u32>,
    oversized: SpatialBvh2,
}

impl SpatialGrid {
    fn build<T: Sync>(entries: &[Entry3<T>]) -> Self {
        if entries.is_empty() {
            return Self {
                min: [0.0; 2],
                cell: 1.0,
                cols: 1,
                rows: 1,
                cell_offsets: vec![0, 0],
                cell_entries: Vec::new(),
                oversized: SpatialBvh2 {
                    nodes: Vec::new(),
                    order: Vec::new(),
                },
            };
        }

        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for entry in entries {
            let aabb = entry_aabb2(entry);
            min[0] = min[0].min(aabb[0]);
            min[1] = min[1].min(aabb[1]);
            max[0] = max[0].max(aabb[2]);
            max[1] = max[1].max(aabb[3]);
        }
        let ext_x = (max[0] - min[0]).max(1e-9);
        let ext_y = (max[1] - min[1]).max(1e-9);
        let target_cells = ((entries.len() / TARGET_ENTRIES_PER_CELL).max(1)) as f64;
        let cell = ((ext_x * ext_y) / target_cells)
            .sqrt()
            .max(ext_x / (MAX_AXIS_CELLS - 1) as f64)
            .max(ext_y / (MAX_AXIS_CELLS - 1) as f64)
            .max(1e-9);
        let cols = (((ext_x / cell).ceil() as u64) + 1).clamp(1, MAX_AXIS_CELLS as u64) as u32;
        let rows = (((ext_y / cell).ceil() as u64) + 1).clamp(1, MAX_AXIS_CELLS as u64) as u32;
        let cell_count = cols as usize * rows as usize;

        let col_of = |x: f64| (((x - min[0]) / cell).floor()).clamp(0.0, (cols - 1) as f64) as u32;
        let row_of = |y: f64| (((y - min[1]) / cell).floor()).clamp(0.0, (rows - 1) as f64) as u32;

        #[cfg(not(target_arch = "wasm32"))]
        let (counts, oversized) = {
            use crate::par::prelude::*;
            use std::sync::atomic::{AtomicU32, Ordering};

            let counts: Vec<AtomicU32> =
                (0..cell_count).map(|_| AtomicU32::new(0)).collect();
            let oversized: Vec<u32> = entries
                .par_iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    let aabb = entry_aabb2(entry);
                    let c0 = col_of(aabb[0]);
                    let c1 = col_of(aabb[2]);
                    let r0 = row_of(aabb[1]);
                    let r1 = row_of(aabb[3]);
                    let span = (c1 - c0 + 1) as u64 * (r1 - r0 + 1) as u64;
                    if span > MAX_SPAN_CELLS {
                        return Some(idx as u32);
                    }
                    for row in r0..=r1 {
                        let base = row as usize * cols as usize;
                        for col in c0..=c1 {
                            counts[base + col as usize].fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    None
                })
                .collect();
            (
                counts
                    .into_iter()
                    .map(AtomicU32::into_inner)
                    .collect::<Vec<u32>>(),
                oversized,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let (counts, oversized) = {
            let mut counts = vec![0u32; cell_count];
            let mut oversized = Vec::new();
            for (idx, entry) in entries.iter().enumerate() {
                let aabb = entry_aabb2(entry);
                let c0 = col_of(aabb[0]);
                let c1 = col_of(aabb[2]);
                let r0 = row_of(aabb[1]);
                let r1 = row_of(aabb[3]);
                let span = (c1 - c0 + 1) as u64 * (r1 - r0 + 1) as u64;
                if span > MAX_SPAN_CELLS {
                    oversized.push(idx as u32);
                    continue;
                }
                for row in r0..=r1 {
                    let base = row as usize * cols as usize;
                    for col in c0..=c1 {
                        counts[base + col as usize] += 1;
                    }
                }
            }
            (counts, oversized)
        };
        let mut cell_offsets = Vec::with_capacity(cell_count + 1);
        cell_offsets.push(0);
        for count in counts {
            cell_offsets.push(cell_offsets.last().copied().unwrap_or(0) + count);
        }

        #[cfg(not(target_arch = "wasm32"))]
        let cell_entries = {
            use crate::par::prelude::*;
            use std::sync::atomic::{AtomicU32, Ordering};

            let cursors: Vec<AtomicU32> = cell_offsets[..cell_count]
                .iter()
                .copied()
                .map(AtomicU32::new)
                .collect();
            let slots: Vec<AtomicU32> = (0..*cell_offsets.last().unwrap_or(&0) as usize)
                .map(|_| AtomicU32::new(0))
                .collect();
            entries.par_iter().enumerate().for_each(|(idx, entry)| {
                let aabb = entry_aabb2(entry);
                let c0 = col_of(aabb[0]);
                let c1 = col_of(aabb[2]);
                let r0 = row_of(aabb[1]);
                let r1 = row_of(aabb[3]);
                let span = (c1 - c0 + 1) as u64 * (r1 - r0 + 1) as u64;
                if span > MAX_SPAN_CELLS {
                    return;
                }
                for row in r0..=r1 {
                    let base = row as usize * cols as usize;
                    for col in c0..=c1 {
                        let cell_idx = base + col as usize;
                        let cursor = cursors[cell_idx].fetch_add(1, Ordering::Relaxed);
                        slots[cursor as usize].store(idx as u32, Ordering::Relaxed);
                    }
                }
            });
            slots.into_iter().map(AtomicU32::into_inner).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let cell_entries = {
            let mut cell_entries = vec![0u32; *cell_offsets.last().unwrap_or(&0) as usize];
            let mut cursors = cell_offsets[..cell_count].to_vec();
            for (idx, entry) in entries.iter().enumerate() {
                let aabb = entry_aabb2(entry);
                let c0 = col_of(aabb[0]);
                let c1 = col_of(aabb[2]);
                let r0 = row_of(aabb[1]);
                let r1 = row_of(aabb[3]);
                let span = (c1 - c0 + 1) as u64 * (r1 - r0 + 1) as u64;
                if span > MAX_SPAN_CELLS {
                    continue;
                }
                for row in r0..=r1 {
                    let base = row as usize * cols as usize;
                    for col in c0..=c1 {
                        let cell_idx = base + col as usize;
                        let cursor = &mut cursors[cell_idx];
                        cell_entries[*cursor as usize] = idx as u32;
                        *cursor += 1;
                    }
                }
            }
            cell_entries
        };
        let oversized = SpatialBvh2::build(&entries, oversized);

        Self {
            min,
            cell,
            cols,
            rows,
            cell_offsets,
            cell_entries,
            oversized,
        }
    }

    fn query<T: Copy + Ord>(&self, entries: &[Entry3<T>], query: [f64; 4]) -> Vec<T> {
        let mut entry_indices = Vec::new();
        self.oversized.query(entries, query, &mut entry_indices);
        let grid_max_x = self.min[0] + self.cols as f64 * self.cell;
        let grid_max_y = self.min[1] + self.rows as f64 * self.cell;
        if query[2] >= self.min[0]
            && query[3] >= self.min[1]
            && query[0] <= grid_max_x
            && query[1] <= grid_max_y
        {
            let col = |x: f64| {
                (((x - self.min[0]) / self.cell).floor()).clamp(0.0, (self.cols - 1) as f64) as u32
            };
            let row = |y: f64| {
                (((y - self.min[1]) / self.cell).floor()).clamp(0.0, (self.rows - 1) as f64) as u32
            };
            for r in row(query[1])..=row(query[3]) {
                let base = r as usize * self.cols as usize;
                for c in col(query[0])..=col(query[2]) {
                    let cell_idx = base + c as usize;
                    let start = self.cell_offsets[cell_idx] as usize;
                    let end = self.cell_offsets[cell_idx + 1] as usize;
                    entry_indices.extend_from_slice(&self.cell_entries[start..end]);
                }
            }
        }

        entry_indices.sort_unstable();
        entry_indices.dedup();
        let mut out: Vec<T> = entry_indices
            .into_iter()
            .filter_map(|idx| {
                let entry = entries.get(idx as usize)?;
                aabb_overlaps(entry_aabb2(entry), query).then_some(entry.value)
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

struct SpatialSet<T> {
    entries: Vec<Entry3<T>>,
    xy: SpatialGrid,
    xyz: std::sync::OnceLock<SpatialBvh3>,
}

impl<T: Copy + Ord + Sync> SpatialSet<T> {
    fn build(entries: Vec<Entry3<T>>) -> Self {
        let xy = SpatialGrid::build(&entries);
        Self {
            entries,
            xy,
            xyz: std::sync::OnceLock::new(),
        }
    }

    fn query_xy(&self, aabb: [f64; 4]) -> Vec<T> {
        self.xy.query(&self.entries, aabb)
    }

    fn prepare_screen(&self) {
        self.xyz
            .get_or_init(|| SpatialBvh3::build(&self.entries));
    }

    fn query_screen(
        &self,
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> Vec<T> {
        self.xyz
            .get_or_init(|| SpatialBvh3::build(&self.entries))
            .query_screen(&self.entries, screen_rect, view_rot, eye, bounds)
    }
}

pub struct InteractionIndex {
    wires: SpatialSet<u32>,
    /// Stable entity handle for each source wire. Lets a stale-but-valid base
    /// index feed an incremental overlay after small edits without retaining
    /// the old heavyweight wire set or trusting shifted vector indices.
    wire_handles: Vec<Option<u64>>,
    /// Stable occurrence number of each wire inside its owning entity run.
    /// Small-edit overlays use `(handle, ordinal)` to recover only the exact
    /// nearby wires from the current resident memo instead of cloning every
    /// wire emitted by a large INSERT.
    wire_ordinals: Vec<Option<u32>>,
    segments: SpatialSet<SegmentRef>,
    snap_points: SpatialSet<SnapPointRef>,
    key_vertices: SpatialSet<KeyVertexRef>,
    key_segments: SpatialSet<KeySegmentRef>,
    fill_triangles: SpatialSet<TriangleRef>,
    pick_triangles: SpatialSet<TriangleRef>,
    glyphs: SpatialSet<GlyphRef>,
    unbounded_wires: Vec<u32>,
    screen_unbounded_wires: Vec<u32>,
    max_line_half_width_px: f32,
    max_marker_radius_fraction: f32,
}

pub struct InteractionHandleIndex {
    handles: SpatialSet<u64>,
}

struct WireIndexEntries {
    wire: Option<Entry3<u32>>,
    segments: Vec<Entry3<SegmentRef>>,
    snap_points: Vec<Entry3<SnapPointRef>>,
    key_vertices: Vec<Entry3<KeyVertexRef>>,
    key_segments: Vec<Entry3<KeySegmentRef>>,
    fill_triangles: Vec<Entry3<TriangleRef>>,
    pick_triangles: Vec<Entry3<TriangleRef>>,
    glyphs: Vec<Entry3<GlyphRef>>,
    unbounded: bool,
    screen_unbounded: bool,
    max_line_half_width_px: f32,
    max_marker_radius_fraction: f32,
}

fn collect_wire_index_entries(wire_idx: u32, wire: &WireModel) -> WireIndexEntries {
    let mut entries = WireIndexEntries {
        wire: finite_wire_aabb3(wire).map(|aabb| Entry3 {
            aabb,
            value: wire_idx,
        }),
        segments: Vec::with_capacity(wire.points.len().saturating_sub(1)),
        snap_points: Vec::with_capacity(wire.snap_pts.len()),
        key_vertices: Vec::with_capacity(wire.key_vertices.len()),
        key_segments: Vec::with_capacity(wire.key_vertices.len().saturating_sub(1)),
        fill_triangles: Vec::with_capacity(wire.fill_tris.len() / 3),
        pick_triangles: Vec::with_capacity(wire.pick_tris.len() / 3),
        glyphs: Vec::with_capacity(wire.text_verts.len() / 6),
        unbounded: false,
        screen_unbounded: wire.point_marker.is_some(),
        max_line_half_width_px: if wire.line_weight_px.is_finite() {
            (wire.line_weight_px * 0.5).max(0.0)
        } else {
            0.0
        },
        max_marker_radius_fraction: wire.point_marker.map_or(0.0, |marker| {
            marker.viewport_percent.max(0.0) * 0.01 * std::f32::consts::SQRT_2
        }),
    };
    entries.unbounded = entries.wire.is_none();

    if wire.point_marker.is_none() {
        for start in 0..wire.points.len().saturating_sub(1) {
            let Some(aabb) = points_aabb3([
                wire_point(wire, start),
                wire_point(wire, start + 1),
            ]) else {
                continue;
            };
            entries.segments.push(Entry3 {
                aabb,
                value: SegmentRef {
                    wire: wire_idx,
                    start: start as u32,
                },
            });
        }
    }
    for (index, (point, _)) in wire.snap_pts.iter().enumerate() {
        if point.is_finite() {
            entries.snap_points.push(Entry3 {
                aabb: [point.x, point.y, point.z, point.x, point.y, point.z],
                value: SnapPointRef {
                    wire: wire_idx,
                    index: index as u32,
                },
            });
        }
    }
    for (index, &point) in wire.key_vertices.iter().enumerate() {
        if point.iter().all(|value| value.is_finite()) {
            entries.key_vertices.push(Entry3 {
                aabb: [point[0], point[1], point[2], point[0], point[1], point[2]],
                value: KeyVertexRef {
                    wire: wire_idx,
                    index: index as u32,
                },
            });
        }
    }
    for start in 0..wire.key_vertices.len().saturating_sub(1) {
        let Some(aabb) =
            points_aabb3([wire.key_vertices[start], wire.key_vertices[start + 1]])
        else {
            continue;
        };
        entries.key_segments.push(Entry3 {
            aabb,
            value: KeySegmentRef {
                wire: wire_idx,
                start: start as u32,
            },
        });
    }
    append_triangle_entries(
        wire_idx,
        &wire.fill_tris,
        &wire.fill_tris_low,
        &mut entries.fill_triangles,
    );
    append_triangle_entries(
        wire_idx,
        &wire.pick_tris,
        &wire.pick_tris_low,
        &mut entries.pick_triangles,
    );
    for start in (0..wire.text_verts.len()).step_by(6) {
        let Some(quad) = wire.text_verts.get(start..start + 6) else {
            break;
        };
        let Some(aabb) = points_aabb3(quad.iter().map(|vertex| {
            [
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
            ]
        })) else {
            continue;
        };
        entries.glyphs.push(Entry3 {
            aabb,
            value: GlyphRef {
                wire: wire_idx,
                start: start as u32,
            },
        });
    }
    entries
}

fn flatten_entry_parts<T>(mut parts: Vec<Vec<T>>) -> Vec<T> {
    let Some((largest, _)) = parts.iter().enumerate().max_by_key(|(_, part)| part.len()) else {
        return Vec::new();
    };
    let mut output = parts.swap_remove(largest);
    let remaining: usize = parts.iter().map(Vec::len).sum();
    output.reserve(remaining);
    for mut part in parts {
        output.append(&mut part);
    }
    output
}

impl InteractionHandleIndex {
    pub fn build(entries: impl IntoIterator<Item = (u64, [f64; 6])>) -> Self {
        Self {
            handles: SpatialSet::build(
                entries
                    .into_iter()
                    .filter(|(_, aabb)| {
                        aabb.iter().all(|value| value.is_finite())
                            && aabb[0] <= aabb[3]
                            && aabb[1] <= aabb[4]
                            && aabb[2] <= aabb[5]
                    })
                    .map(|(value, aabb)| Entry3 { aabb, value })
                    .collect(),
            ),
        }
    }

    pub fn query_xy(&self, aabb: [f64; 4]) -> Vec<u64> {
        self.handles.query_xy(aabb)
    }

    pub fn query_screen(
        &self,
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> Vec<u64> {
        self.handles
            .query_screen(screen_rect, view_rot, eye, bounds)
    }
}

impl InteractionIndex {
    pub fn estimated_work(wires: &[WireModel]) -> usize {
        wires.iter().fold(0usize, |total, wire| {
            total
                .saturating_add(wire.points.len())
                .saturating_add(wire.key_vertices.len())
                .saturating_add(wire.snap_pts.len() * 2)
                .saturating_add(wire.fill_tris.len())
                .saturating_add(wire.pick_tris.len())
                .saturating_add(wire.text_verts.len() / 2)
        })
    }

    pub fn build(wires: &[WireModel]) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let perf = crate::perf::enabled();
        #[cfg(not(target_arch = "wasm32"))]
        let build_started = iced::time::Instant::now();
        let wire_handles: Vec<Option<u64>> = wires
            .iter()
            .map(|wire| wire.name.parse::<u64>().ok())
            .collect();
        let mut next_ordinal: rustc_hash::FxHashMap<u64, u32> =
            rustc_hash::FxHashMap::default();
        let wire_ordinals: Vec<Option<u32>> = wire_handles
            .iter()
            .map(|handle| {
                let handle = (*handle)?;
                let ordinal = next_ordinal.entry(handle).or_default();
                let current = *ordinal;
                *ordinal += 1;
                Some(current)
            })
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let handles_elapsed = build_started.elapsed();
        #[cfg(not(target_arch = "wasm32"))]
        let collect_started = iced::time::Instant::now();
        let mut wire_entries = Vec::with_capacity(wires.len());
        let mut unbounded_wires = Vec::new();
        let mut screen_unbounded_wires = Vec::new();
        let mut max_line_half_width_px = 0.0f32;
        let mut max_marker_radius_fraction = 0.0f32;

        #[cfg(not(target_arch = "wasm32"))]
        let per_wire: Vec<WireIndexEntries> = {
            use crate::par::prelude::*;
            wires
                .par_iter()
                .enumerate()
                .map(|(index, wire)| collect_wire_index_entries(index as u32, wire))
                .collect()
        };
        #[cfg(target_arch = "wasm32")]
        let per_wire: Vec<WireIndexEntries> = wires
            .iter()
            .enumerate()
            .map(|(index, wire)| collect_wire_index_entries(index as u32, wire))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let collect_elapsed = collect_started.elapsed();
        #[cfg(not(target_arch = "wasm32"))]
        let flatten_started = iced::time::Instant::now();
        let mut segment_parts = Vec::with_capacity(per_wire.len());
        let mut snap_point_parts = Vec::with_capacity(per_wire.len());
        let mut key_vertex_parts = Vec::with_capacity(per_wire.len());
        let mut key_segment_parts = Vec::with_capacity(per_wire.len());
        let mut fill_triangle_parts = Vec::with_capacity(per_wire.len());
        let mut pick_triangle_parts = Vec::with_capacity(per_wire.len());
        let mut glyph_parts = Vec::with_capacity(per_wire.len());
        for (wire_idx, entries) in per_wire.into_iter().enumerate() {
            max_line_half_width_px =
                max_line_half_width_px.max(entries.max_line_half_width_px);
            max_marker_radius_fraction =
                max_marker_radius_fraction.max(entries.max_marker_radius_fraction);
            if let Some(entry) = entries.wire {
                wire_entries.push(entry);
            }
            if entries.unbounded {
                unbounded_wires.push(wire_idx as u32);
            }
            if entries.screen_unbounded {
                screen_unbounded_wires.push(wire_idx as u32);
            }
            segment_parts.push(entries.segments);
            snap_point_parts.push(entries.snap_points);
            key_vertex_parts.push(entries.key_vertices);
            key_segment_parts.push(entries.key_segments);
            fill_triangle_parts.push(entries.fill_triangles);
            pick_triangle_parts.push(entries.pick_triangles);
            glyph_parts.push(entries.glyphs);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let (
            ((segment_entries, snap_point_entries), (key_vertex_entries, key_segment_entries)),
            ((fill_triangle_entries, pick_triangle_entries), glyph_entries),
        ) = rayon::join(
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || flatten_entry_parts(segment_parts),
                            || flatten_entry_parts(snap_point_parts),
                        )
                    },
                    || {
                        rayon::join(
                            || flatten_entry_parts(key_vertex_parts),
                            || flatten_entry_parts(key_segment_parts),
                        )
                    },
                )
            },
            || {
                let (fill, pick) = rayon::join(
                    || flatten_entry_parts(fill_triangle_parts),
                    || flatten_entry_parts(pick_triangle_parts),
                );
                ((fill, pick), flatten_entry_parts(glyph_parts))
            },
        );
        #[cfg(target_arch = "wasm32")]
        let (
            segment_entries,
            snap_point_entries,
            key_vertex_entries,
            key_segment_entries,
            fill_triangle_entries,
            pick_triangle_entries,
            glyph_entries,
        ) = (
            flatten_entry_parts(segment_parts),
            flatten_entry_parts(snap_point_parts),
            flatten_entry_parts(key_vertex_parts),
            flatten_entry_parts(key_segment_parts),
            flatten_entry_parts(fill_triangle_parts),
            flatten_entry_parts(pick_triangle_parts),
            flatten_entry_parts(glyph_parts),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let flatten_elapsed = flatten_started.elapsed();
        #[cfg(not(target_arch = "wasm32"))]
        let spatial_started = iced::time::Instant::now();

        #[cfg(not(target_arch = "wasm32"))]
        let (
            ((wires, segments), (snap_points, key_vertices)),
            ((key_segments, fill_triangles), (pick_triangles, glyphs)),
        ) = rayon::join(
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || SpatialSet::build(wire_entries),
                            || SpatialSet::build(segment_entries),
                        )
                    },
                    || {
                        rayon::join(
                            || SpatialSet::build(snap_point_entries),
                            || SpatialSet::build(key_vertex_entries),
                        )
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || SpatialSet::build(key_segment_entries),
                            || SpatialSet::build(fill_triangle_entries),
                        )
                    },
                    || {
                        rayon::join(
                            || SpatialSet::build(pick_triangle_entries),
                            || SpatialSet::build(glyph_entries),
                        )
                    },
                )
            },
        );
        #[cfg(target_arch = "wasm32")]
        let (
            wires,
            segments,
            snap_points,
            key_vertices,
            key_segments,
            fill_triangles,
            pick_triangles,
            glyphs,
        ) = (
            SpatialSet::build(wire_entries),
            SpatialSet::build(segment_entries),
            SpatialSet::build(snap_point_entries),
            SpatialSet::build(key_vertex_entries),
            SpatialSet::build(key_segment_entries),
            SpatialSet::build(fill_triangle_entries),
            SpatialSet::build(pick_triangle_entries),
            SpatialSet::build(glyph_entries),
        );
        #[cfg(not(target_arch = "wasm32"))]
        if perf {
            crate::perf_record!(
                "[perf] interaction-index-detail total={:.1}ms handles={:.1} collect={:.1} flatten={:.1} spatial={:.1}",
                build_started.elapsed().as_secs_f64() * 1000.0,
                handles_elapsed.as_secs_f64() * 1000.0,
                collect_elapsed.as_secs_f64() * 1000.0,
                flatten_elapsed.as_secs_f64() * 1000.0,
                spatial_started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        Self {
            wires,
            wire_handles,
            wire_ordinals,
            segments,
            snap_points,
            key_vertices,
            key_segments,
            fill_triangles,
            pick_triangles,
            glyphs,
            unbounded_wires,
            screen_unbounded_wires,
            max_line_half_width_px,
            max_marker_radius_fraction,
        }
    }

    /// Build every projected 3D broad phase before the index reaches the UI
    /// thread. Perspective/orbit hover must never pay this one-time cost.
    pub fn prepare_screen(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        rayon::join(
            || {
                rayon::join(
                    || rayon::join(|| self.wires.prepare_screen(), || self.segments.prepare_screen()),
                    || {
                        rayon::join(
                            || self.snap_points.prepare_screen(),
                            || self.key_vertices.prepare_screen(),
                        )
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || self.key_segments.prepare_screen(),
                            || self.fill_triangles.prepare_screen(),
                        )
                    },
                    || {
                        rayon::join(
                            || self.pick_triangles.prepare_screen(),
                            || self.glyphs.prepare_screen(),
                        )
                    },
                )
            },
        );
        #[cfg(target_arch = "wasm32")]
        {
            self.wires.prepare_screen();
            self.segments.prepare_screen();
            self.snap_points.prepare_screen();
            self.key_vertices.prepare_screen();
            self.key_segments.prepare_screen();
            self.fill_triangles.prepare_screen();
            self.pick_triangles.prepare_screen();
            self.glyphs.prepare_screen();
        }
    }

    pub fn pick_radius_px(
        &self,
        base_radius_px: f32,
        viewport_height_px: f32,
        include_line_weight: bool,
    ) -> f32 {
        let radius = base_radius_px
            .max(self.max_marker_radius_fraction * viewport_height_px.max(0.0));
        if include_line_weight {
            radius.max(self.max_line_half_width_px)
        } else {
            radius
        }
    }

    fn queried_wire_keys(
        &self,
        mut indices: Vec<u32>,
        include_screen_unbounded: bool,
    ) -> Vec<(u64, u32)> {
        indices.extend_from_slice(&self.unbounded_wires);
        if include_screen_unbounded {
            indices.extend_from_slice(&self.screen_unbounded_wires);
        }
        let mut keys: Vec<(u64, u32)> = indices
            .into_iter()
            .filter_map(|index| {
                self.wire_handles
                    .get(index as usize)
                    .copied()
                    .flatten()
                    .zip(
                        self.wire_ordinals
                            .get(index as usize)
                            .copied()
                            .flatten(),
                    )
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    pub fn query_wire_keys_xy(&self, aabb: [f64; 4]) -> Vec<(u64, u32)> {
        self.queried_wire_keys(self.wires.query_xy(aabb), false)
    }

    pub fn query_wire_keys_screen(
        &self,
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> Vec<(u64, u32)> {
        self.queried_wire_keys(
            self.wires.query_screen(screen_rect, view_rot, eye, bounds),
            true,
        )
    }

    fn remap_wire_index(
        &self,
        index: u32,
        slots: &rustc_hash::FxHashMap<(u64, u32), u32>,
    ) -> Option<u32> {
        let handle = self
            .wire_handles
            .get(index as usize)
            .copied()
            .flatten()?;
        let ordinal = self
            .wire_ordinals
            .get(index as usize)
            .copied()
            .flatten()?;
        slots.get(&(handle, ordinal)).copied()
    }

    fn remap_wire_indices(
        &self,
        mut indices: Vec<u32>,
        slots: &rustc_hash::FxHashMap<(u64, u32), u32>,
        include_screen_unbounded: bool,
    ) -> Vec<u32> {
        indices.extend_from_slice(&self.unbounded_wires);
        if include_screen_unbounded {
            indices.extend_from_slice(&self.screen_unbounded_wires);
        }
        let mut local: Vec<u32> = indices
            .into_iter()
            .filter_map(|index| self.remap_wire_index(index, slots))
            .collect();
        local.sort_unstable();
        local.dedup();
        local
    }

    fn remap_refs<T>(
        &self,
        values: Vec<T>,
        slots: &rustc_hash::FxHashMap<(u64, u32), u32>,
        wire_of: impl Fn(&T) -> u32,
        set_wire: impl Fn(&mut T, u32),
    ) -> Vec<T> {
        values
            .into_iter()
            .filter_map(|mut value| {
                let wire = self.remap_wire_index(wire_of(&value), slots)?;
                set_wire(&mut value, wire);
                Some(value)
            })
            .collect()
    }

    pub(crate) fn query_remapped_xy(
        &self,
        wires: Arc<Vec<WireModel>>,
        slots: &rustc_hash::FxHashMap<(u64, u32), u32>,
        aabb: [f64; 4],
    ) -> InteractionCandidates {
        InteractionCandidates {
            wires,
            wire_indices: Some(self.remap_wire_indices(
                self.wires.query_xy(aabb),
                slots,
                false,
            )),
            segments: Some(self.remap_refs(
                self.segments.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            snap_points: Some(self.remap_refs(
                self.snap_points.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            key_vertices: Some(self.remap_refs(
                self.key_vertices.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            key_segments: Some(self.remap_refs(
                self.key_segments.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            fill_triangles: Some(self.remap_refs(
                self.fill_triangles.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            pick_triangles: Some(self.remap_refs(
                self.pick_triangles.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            glyphs: Some(self.remap_refs(
                self.glyphs.query_xy(aabb),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            query_aabb: Some(aabb),
            screen_rect: None,
            screen_view: None,
        }
    }

    pub(crate) fn query_remapped_screen(
        &self,
        wires: Arc<Vec<WireModel>>,
        slots: &rustc_hash::FxHashMap<(u64, u32), u32>,
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> InteractionCandidates {
        InteractionCandidates {
            wires,
            wire_indices: Some(self.remap_wire_indices(
                self.wires
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                true,
            )),
            segments: Some(self.remap_refs(
                self.segments
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            snap_points: Some(self.remap_refs(
                self.snap_points
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            key_vertices: Some(self.remap_refs(
                self.key_vertices
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            key_segments: Some(self.remap_refs(
                self.key_segments
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            fill_triangles: Some(self.remap_refs(
                self.fill_triangles
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            pick_triangles: Some(self.remap_refs(
                self.pick_triangles
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            glyphs: Some(self.remap_refs(
                self.glyphs
                    .query_screen(screen_rect, view_rot, eye, bounds),
                slots,
                |value| value.wire,
                |value, wire| value.wire = wire,
            )),
            query_aabb: None,
            screen_rect: Some(screen_rect),
            screen_view: Some((view_rot, eye, bounds)),
        }
    }

    pub fn query_xy(&self, wires: Arc<Vec<WireModel>>, aabb: [f64; 4]) -> InteractionCandidates {
        let mut wire_indices = self.wires.query_xy(aabb);
        wire_indices.extend_from_slice(&self.unbounded_wires);
        wire_indices.sort_unstable();
        wire_indices.dedup();
        InteractionCandidates {
            wires,
            wire_indices: Some(wire_indices),
            segments: Some(self.segments.query_xy(aabb)),
            snap_points: Some(self.snap_points.query_xy(aabb)),
            key_vertices: Some(self.key_vertices.query_xy(aabb)),
            key_segments: Some(self.key_segments.query_xy(aabb)),
            fill_triangles: Some(self.fill_triangles.query_xy(aabb)),
            pick_triangles: Some(self.pick_triangles.query_xy(aabb)),
            glyphs: Some(self.glyphs.query_xy(aabb)),
            query_aabb: Some(aabb),
            screen_rect: None,
            screen_view: None,
        }
    }

    pub fn query_screen(
        &self,
        wires: Arc<Vec<WireModel>>,
        screen_rect: [f32; 4],
        view_rot: Mat4,
        eye: DVec3,
        bounds: Rectangle,
    ) -> InteractionCandidates {
        let mut wire_indices = self.wires.query_screen(screen_rect, view_rot, eye, bounds);
        wire_indices.extend_from_slice(&self.unbounded_wires);
        wire_indices.extend_from_slice(&self.screen_unbounded_wires);
        wire_indices.sort_unstable();
        wire_indices.dedup();
        InteractionCandidates {
            wires,
            wire_indices: Some(wire_indices),
            segments: Some(
                self.segments
                    .query_screen(screen_rect, view_rot, eye, bounds),
            ),
            snap_points: Some(
                self.snap_points
                    .query_screen(screen_rect, view_rot, eye, bounds),
            ),
            key_vertices: Some(
                self.key_vertices
                    .query_screen(screen_rect, view_rot, eye, bounds),
            ),
            key_segments: Some(
                self.key_segments
                    .query_screen(screen_rect, view_rot, eye, bounds),
            ),
            fill_triangles: Some(self.fill_triangles.query_screen(
                screen_rect,
                view_rot,
                eye,
                bounds,
            )),
            pick_triangles: Some(self.pick_triangles.query_screen(
                screen_rect,
                view_rot,
                eye,
                bounds,
            )),
            glyphs: Some(self.glyphs.query_screen(screen_rect, view_rot, eye, bounds)),
            query_aabb: None,
            screen_rect: Some(screen_rect),
            screen_view: Some((view_rot, eye, bounds)),
        }
    }
}

impl std::fmt::Debug for InteractionIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionIndex")
            .field("wires", &self.wire_handles.len())
            .field("segments", &self.segments.entries.len())
            .field("fill_triangles", &self.fill_triangles.entries.len())
            .field("pick_triangles", &self.pick_triangles.entries.len())
            .finish()
    }
}

pub struct InteractionCandidates {
    wires: Arc<Vec<WireModel>>,
    wire_indices: Option<Vec<u32>>,
    segments: Option<Vec<SegmentRef>>,
    snap_points: Option<Vec<SnapPointRef>>,
    key_vertices: Option<Vec<KeyVertexRef>>,
    key_segments: Option<Vec<KeySegmentRef>>,
    fill_triangles: Option<Vec<TriangleRef>>,
    pick_triangles: Option<Vec<TriangleRef>>,
    glyphs: Option<Vec<GlyphRef>>,
    query_aabb: Option<[f64; 4]>,
    screen_rect: Option<[f32; 4]>,
    screen_view: Option<(Mat4, DVec3, Rectangle)>,
}

pub trait WireSource {
    fn iter(&self) -> WireIter<'_>;
    fn len(&self) -> usize;
    fn get(&self, index: usize) -> Option<&WireModel>;
    fn segments(&self) -> Option<&[SegmentRef]> {
        None
    }
    fn snap_points(&self) -> Option<&[SnapPointRef]> {
        None
    }
    fn key_vertices(&self) -> Option<&[KeyVertexRef]> {
        None
    }
    fn key_segments(&self) -> Option<&[KeySegmentRef]> {
        None
    }
    fn fill_triangles(&self) -> Option<&[TriangleRef]> {
        None
    }
    fn pick_triangles(&self) -> Option<&[TriangleRef]> {
        None
    }
    fn glyphs(&self) -> Option<&[GlyphRef]> {
        None
    }
    fn source_wire(&self, index: u32) -> Option<&WireModel>;
}

impl InteractionCandidates {
    pub fn all(wires: Arc<Vec<WireModel>>) -> Self {
        Self {
            wires,
            wire_indices: None,
            segments: None,
            snap_points: None,
            key_vertices: None,
            key_segments: None,
            fill_triangles: None,
            pick_triangles: None,
            glyphs: None,
            query_aabb: None,
            screen_rect: None,
            screen_view: None,
        }
    }

    /// Empty but explicitly indexed candidate set. Used while a large source
    /// is still being prepared off-thread so UI input never falls back to a
    /// full-scene scan.
    pub fn pending(wires: Arc<Vec<WireModel>>) -> Self {
        Self {
            wires,
            wire_indices: Some(Vec::new()),
            segments: Some(Vec::new()),
            snap_points: Some(Vec::new()),
            key_vertices: Some(Vec::new()),
            key_segments: Some(Vec::new()),
            fill_triangles: Some(Vec::new()),
            pick_triangles: Some(Vec::new()),
            glyphs: Some(Vec::new()),
            query_aabb: None,
            screen_rect: None,
            screen_view: None,
        }
    }

    pub fn is_indexed(&self) -> bool {
        self.wire_indices.is_some()
    }

    pub fn iter(&self) -> WireIter<'_> {
        match &self.wire_indices {
            Some(indices) => WireIter::Indexed {
                wires: &self.wires,
                indices: indices.iter(),
            },
            None => WireIter::All(self.wires.as_slice().iter()),
        }
    }

    pub fn len(&self) -> usize {
        self.wire_indices
            .as_ref()
            .map_or(self.wires.len(), Vec::len)
    }

    pub fn get(&self, index: usize) -> Option<&WireModel> {
        match &self.wire_indices {
            Some(indices) => indices
                .get(index)
                .and_then(|&wire| self.wires.get(wire as usize)),
            None => self.wires.get(index),
        }
    }

    pub fn segments(&self) -> Option<&[SegmentRef]> {
        self.segments.as_deref()
    }

    pub fn snap_points(&self) -> Option<&[SnapPointRef]> {
        self.snap_points.as_deref()
    }

    pub fn key_vertices(&self) -> Option<&[KeyVertexRef]> {
        self.key_vertices.as_deref()
    }

    pub fn key_segments(&self) -> Option<&[KeySegmentRef]> {
        self.key_segments.as_deref()
    }

    pub fn fill_triangles(&self) -> Option<&[TriangleRef]> {
        self.fill_triangles.as_deref()
    }

    pub fn pick_triangles(&self) -> Option<&[TriangleRef]> {
        self.pick_triangles.as_deref()
    }

    pub fn glyphs(&self) -> Option<&[GlyphRef]> {
        self.glyphs.as_deref()
    }

    pub fn source_wire(&self, index: u32) -> Option<&WireModel> {
        self.wires.get(index as usize)
    }

    pub fn query_aabb(&self) -> Option<[f64; 4]> {
        self.query_aabb
    }

    pub fn screen_query(&self) -> Option<([f32; 4], Mat4, DVec3, Rectangle)> {
        self.screen_rect
            .zip(self.screen_view)
            .map(|(rect, (view, eye, bounds))| (rect, view, eye, bounds))
    }

    pub(crate) fn extend_indexed(&mut self, other: Self) {
        debug_assert!(Arc::ptr_eq(&self.wires, &other.wires));

        fn extend<T: Ord>(target: &mut Option<Vec<T>>, incoming: Option<Vec<T>>) {
            let (Some(target), Some(incoming)) = (target.as_mut(), incoming) else {
                return;
            };
            target.extend(incoming);
            target.sort_unstable();
            target.dedup();
        }

        extend(&mut self.wire_indices, other.wire_indices);
        extend(&mut self.segments, other.segments);
        extend(&mut self.snap_points, other.snap_points);
        extend(&mut self.key_vertices, other.key_vertices);
        extend(&mut self.key_segments, other.key_segments);
        extend(&mut self.fill_triangles, other.fill_triangles);
        extend(&mut self.pick_triangles, other.pick_triangles);
        extend(&mut self.glyphs, other.glyphs);
    }
}

impl WireSource for InteractionCandidates {
    fn iter(&self) -> WireIter<'_> {
        self.iter()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> Option<&WireModel> {
        self.get(index)
    }

    fn segments(&self) -> Option<&[SegmentRef]> {
        self.segments()
    }

    fn snap_points(&self) -> Option<&[SnapPointRef]> {
        self.snap_points()
    }

    fn key_vertices(&self) -> Option<&[KeyVertexRef]> {
        self.key_vertices()
    }

    fn key_segments(&self) -> Option<&[KeySegmentRef]> {
        self.key_segments()
    }

    fn fill_triangles(&self) -> Option<&[TriangleRef]> {
        self.fill_triangles()
    }

    fn pick_triangles(&self) -> Option<&[TriangleRef]> {
        self.pick_triangles()
    }

    fn glyphs(&self) -> Option<&[GlyphRef]> {
        self.glyphs()
    }

    fn source_wire(&self, index: u32) -> Option<&WireModel> {
        self.source_wire(index)
    }
}

impl WireSource for [WireModel] {
    fn iter(&self) -> WireIter<'_> {
        WireIter::All(<[WireModel]>::iter(self))
    }

    fn len(&self) -> usize {
        <[WireModel]>::len(self)
    }

    fn get(&self, index: usize) -> Option<&WireModel> {
        <[WireModel]>::get(self, index)
    }

    fn source_wire(&self, index: u32) -> Option<&WireModel> {
        self.get(index as usize)
    }
}

impl WireSource for Vec<WireModel> {
    fn iter(&self) -> WireIter<'_> {
        WireIter::All(self.as_slice().iter())
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<&WireModel> {
        self.as_slice().get(index)
    }

    fn source_wire(&self, index: u32) -> Option<&WireModel> {
        self.get(index as usize)
    }
}

impl<const N: usize> WireSource for [WireModel; N] {
    fn iter(&self) -> WireIter<'_> {
        WireIter::All(self.as_slice().iter())
    }

    fn len(&self) -> usize {
        N
    }

    fn get(&self, index: usize) -> Option<&WireModel> {
        self.as_slice().get(index)
    }

    fn source_wire(&self, index: u32) -> Option<&WireModel> {
        self.get(index as usize)
    }
}

#[derive(Clone)]
pub enum WireIter<'a> {
    All(std::slice::Iter<'a, WireModel>),
    Indexed {
        wires: &'a [WireModel],
        indices: std::slice::Iter<'a, u32>,
    },
}

impl<'a> Iterator for WireIter<'a> {
    type Item = &'a WireModel;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(iter) => iter.next(),
            Self::Indexed { wires, indices } => {
                indices.next().and_then(|&idx| wires.get(idx as usize))
            }
        }
    }
}

fn finite_wire_aabb3(wire: &WireModel) -> Option<[f64; 6]> {
    if let Some(marker) = wire.point_marker {
        if !marker.origin.is_finite() {
            return None;
        }
        let normal = marker.normal.try_normalize().unwrap_or(DVec3::Z);
        let mut min_axial = 0.0f64;
        let mut max_axial = 0.0f64;
        for index in 0..wire.points.len() {
            let point = DVec3::from_array(wire_point(wire, index));
            if point.is_finite() {
                let axial = (point - marker.origin).dot(normal);
                min_axial = min_axial.min(axial);
                max_axial = max_axial.max(axial);
            }
        }
        return points_aabb3([
            (marker.origin + normal * min_axial).to_array(),
            (marker.origin + normal * max_axial).to_array(),
        ]);
    }
    let [min_x, min_y, max_x, max_y] = wire.aabb;
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    let mag = wire.aabb.iter().fold(0.0f32, |m, c| m.max(c.abs()));
    let pad = (mag * f32::EPSILON * 2.0) as f64;
    let mut aabb = [
        min_x as f64 - pad,
        min_y as f64 - pad,
        f64::INFINITY,
        max_x as f64 + pad,
        max_y as f64 + pad,
        f64::NEG_INFINITY,
    ];
    let mut include = |point: [f64; 3]| {
        if point.iter().all(|value| value.is_finite()) {
            aabb[0] = aabb[0].min(point[0]);
            aabb[1] = aabb[1].min(point[1]);
            aabb[2] = aabb[2].min(point[2]);
            aabb[3] = aabb[3].max(point[0]);
            aabb[4] = aabb[4].max(point[1]);
            aabb[5] = aabb[5].max(point[2]);
        }
    };
    for index in 0..wire.points.len() {
        include(wire_point(wire, index));
    }
    for &(point, _) in &wire.snap_pts {
        include(point.to_array());
    }
    for &point in &wire.key_vertices {
        include(point);
    }
    for (index, &point) in wire.fill_tris.iter().enumerate() {
        include(point_with_low(point, &wire.fill_tris_low, index));
    }
    for (index, &point) in wire.pick_tris.iter().enumerate() {
        include(point_with_low(point, &wire.pick_tris_low, index));
    }
    for vertex in &wire.text_verts {
        include([
            vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
            vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
            vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
        ]);
    }
    if !aabb[2].is_finite() {
        aabb[2] = 0.0;
        aabb[5] = 0.0;
    }
    Some(aabb)
}

fn wire_point(wire: &WireModel, index: usize) -> [f64; 3] {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    [
        high[0] as f64 + low[0] as f64,
        high[1] as f64 + low[1] as f64,
        high[2] as f64 + low[2] as f64,
    ]
}

fn point_with_low(point: [f32; 3], low: &[[f32; 3]], index: usize) -> [f64; 3] {
    let residual = low.get(index).copied().unwrap_or([0.0; 3]);
    [
        point[0] as f64 + residual[0] as f64,
        point[1] as f64 + residual[1] as f64,
        point[2] as f64 + residual[2] as f64,
    ]
}

fn points_aabb3(points: impl IntoIterator<Item = [f64; 3]>) -> Option<[f64; 6]> {
    let mut aabb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut any = false;
    for point in points {
        if !point.iter().all(|value| value.is_finite()) {
            return None;
        }
        any = true;
        for axis in 0..3 {
            aabb[axis] = aabb[axis].min(point[axis]);
            aabb[axis + 3] = aabb[axis + 3].max(point[axis]);
        }
    }
    any.then_some(aabb)
}

fn append_triangle_entries(
    wire: u32,
    points: &[[f32; 3]],
    low: &[[f32; 3]],
    out: &mut Vec<Entry3<TriangleRef>>,
) {
    for start in (0..points.len()).step_by(3) {
        if start + 2 >= points.len() {
            break;
        }
        let Some(aabb) =
            points_aabb3((start..start + 3).map(|index| point_with_low(points[index], low, index)))
        else {
            continue;
        };
        out.push(Entry3 {
            aabb,
            value: TriangleRef {
                wire,
                start: start as u32,
            },
        });
    }
}

fn aabb3_projects_into(
    aabb: [f64; 6],
    screen_rect: [f32; 4],
    view_rot: Mat4,
    eye: DVec3,
    bounds: Rectangle,
) -> bool {
    let mut projected = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for x in [aabb[0], aabb[3]] {
        for y in [aabb[1], aabb[4]] {
            for z in [aabb[2], aabb[5]] {
                let clip = view_rot * (DVec3::new(x, y, z) - eye).as_vec3().extend(1.0);
                if !clip.is_finite() || clip.w <= 1e-6 {
                    return true;
                }
                let ndc = clip.truncate() / clip.w;
                let sx = (ndc.x + 1.0) * 0.5 * bounds.width;
                let sy = (1.0 - ndc.y) * 0.5 * bounds.height;
                projected[0] = projected[0].min(sx);
                projected[1] = projected[1].min(sy);
                projected[2] = projected[2].max(sx);
                projected[3] = projected[3].max(sy);
            }
        }
    }
    projected[2] >= screen_rect[0]
        && projected[0] <= screen_rect[2]
        && projected[3] >= screen_rect[1]
        && projected[1] <= screen_rect[3]
}

fn aabb_overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[2] >= b[0] && a[0] <= b[2] && a[3] >= b[1] && a[1] <= b[3]
}
