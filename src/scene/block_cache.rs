// Block-definition tessellation cache.
//
// Each block record is tessellated once into block-local coordinates and
// stored as a list of `LocalSub` (either a tessellated primitive wire OR
// an unexpanded reference to a nested INSERT). At Insert use-time we walk
// the defn, transform-copy primitives, and recurse into nested references —
// each nested defn is itself a cache hit, never re-tessellated.
//
// This shape (lazy nested expansion) is essential: a single block like
// `xref-PLANKOTE` can hold ~4700 nested INSERTs, so build-time inlining
// produces a combinatorial blowup. Storing references and expanding on
// demand keeps build work proportional to total entity count.
//
// Cycle detection: at expand-time we maintain a recursion-depth limit and
// a visited set so a self-referential block produces a marker rather than
// recursing forever.

use std::collections::HashMap;
use std::sync::Arc;

use acadrust::types::{Color as AcadColor, LineWeight, Transform, Vector3};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::tessellate;
use crate::scene::wire_model::{SnapHint, TangentGeom, WireModel};

const MAX_NESTING_DEPTH: usize = 32;

#[derive(Clone, Debug)]
pub struct LocalWire {
    pub points: Vec<[f32; 3]>,
    pub key_vertices: Vec<[f32; 3]>,
    pub snap_pts: Vec<(glam::Vec3, SnapHint)>,
    pub tangent_geoms: Vec<TangentGeom>,
    pub fill_tris: Vec<[f32; 3]>,
    pub color: [f32; 4],
    pub aci: u8,
    pub pattern_length: f32,
    pub pattern: [f32; 8],
    pub line_weight_px: f32,
    pub plinegen: bool,
    pub color_is_byblock: bool,
    pub lt_is_byblock: bool,
    pub lw_is_byblock: bool,
    /// XY bounding box of this wire in block-local coordinates.
    /// `[min_x, min_y, max_x, max_y]`. Used for view-frustum culling at
    /// expand-time: transform corners by the Insert transform → world AABB
    /// → test against the camera's world-space view rect.
    pub aabb_local: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct NestedRef {
    pub block_name: String,
    pub xform: Transform,
    /// Nested INSERT's own resolved style (used when child wires need
    /// to inherit something via ByBlock).
    pub ins_color: [f32; 4],
    pub ins_pat_len: f32,
    pub ins_pat: [f32; 8],
    pub ins_lw_px: f32,
    pub color_is_byblock: bool,
    pub lt_is_byblock: bool,
    pub lw_is_byblock: bool,
    pub instance_offsets: Vec<[f64; 3]>,
}

#[derive(Clone, Debug)]
pub enum LocalSub {
    Wire(LocalWire),
    Nested(NestedRef),
}

#[derive(Clone, Debug, Default)]
pub struct BlockDefn {
    pub subs: Vec<LocalSub>,
    /// Union of every sub's local AABB (including nested-INSERT contributions
    /// resolved at expand time via their own defn's `aabb_local`). XY only —
    /// the wire renderer is 2D-dominant.
    pub aabb_local: [f32; 4],
}

#[derive(Default, Debug)]
pub struct BlockCache {
    defns: HashMap<String, Arc<BlockDefn>>,
}

impl BlockCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn defn(&self, block_name: &str) -> Option<&Arc<BlockDefn>> {
        self.defns.get(block_name)
    }

    /// Build (flat) defns only for block records actually referenced by
    /// Inserts in the document — transitively, so nested-insert targets are
    /// included too. The Model_Space / Paper_Space block_records are skipped
    /// because their entities are emitted as top-level wires, not via the
    /// cache.
    pub fn build(doc: &CadDocument, anno_scale: f32, bg_color: [f32; 4]) -> Self {
        let mut cache = Self::new();
        let referenced = collect_referenced_blocks(doc);
        for name in &referenced {
            let defn = build_defn(doc, name, anno_scale, bg_color);
            cache.defns.insert(name.clone(), Arc::new(defn));
        }
        cache.compute_block_aabbs(&referenced);
        cache
    }

    /// Compute and store the `aabb_local` for every cached defn. Direct wires
    /// contribute their own aabb_local; nested INSERT references look up the
    /// nested defn (already cached) and transform its aabb_local by the
    /// nested Insert's transform before unioning.
    ///
    /// Run as a post-pass so it doesn't matter which order build_defn was
    /// called in. Cycle guard: a self-referential block keeps an empty AABB
    /// (will fail every frustum test → not emitted, which is correct).
    fn compute_block_aabbs(&mut self, names: &[String]) {
        // Snapshot defn pointers up front — we mutate the map below.
        let names: Vec<String> = names.to_vec();
        for name in &names {
            let mut visited: Vec<String> = Vec::new();
            let aabb = self.defn_aabb_recursive(name, &mut visited);
            if let Some(defn_arc) = self.defns.get_mut(name) {
                let mut defn = (**defn_arc).clone();
                defn.aabb_local = aabb;
                *defn_arc = Arc::new(defn);
            }
        }
    }

    fn defn_aabb_recursive(&self, block_name: &str, visited: &mut Vec<String>) -> [f32; 4] {
        if visited.iter().any(|n| n == block_name) {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let Some(defn) = self.defns.get(block_name) else {
            return [0.0, 0.0, 0.0, 0.0];
        };
        visited.push(block_name.to_string());
        let mut acc = [0.0_f32, 0.0, 0.0, 0.0];
        for sub in &defn.subs {
            let aabb = match sub {
                LocalSub::Wire(lw) => lw.aabb_local,
                LocalSub::Nested(nref) => {
                    let nested_local = self.defn_aabb_recursive(&nref.block_name, visited);
                    transform_aabb_xy(nested_local, &nref.xform)
                }
            };
            acc = aabb_union(acc, aabb);
        }
        visited.pop();
        acc
    }
}

/// Walk all entities + all block_record contents collecting every distinct
/// `block_name` that appears in an Insert (transitively).
fn collect_referenced_blocks(doc: &CadDocument) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();

    for entity in doc.entities() {
        if let EntityType::Insert(ins) = entity {
            if seen.insert(ins.block_name.clone()) {
                queue.push(ins.block_name.clone());
            }
        }
    }
    while let Some(name) = queue.pop() {
        let Some(br) = doc.block_records.get(&name) else {
            continue;
        };
        for &eh in &br.entity_handles {
            let Some(entity) = doc.get_entity(eh) else {
                continue;
            };
            if let EntityType::Insert(ins) = entity {
                if seen.insert(ins.block_name.clone()) {
                    queue.push(ins.block_name.clone());
                }
            }
        }
    }
    seen.into_iter().collect()
}

fn build_defn(
    doc: &CadDocument,
    block_name: &str,
    anno_scale: f32,
    bg_color: [f32; 4],
) -> BlockDefn {
    let br = match doc.block_records.get(block_name) {
        Some(br) => br,
        None => return BlockDefn::default(),
    };
    let cap = br.entity_handles.len();
    let mut subs: Vec<LocalSub> = Vec::with_capacity(cap);
    for &eh in &br.entity_handles {
        let Some(entity) = doc.get_entity(eh) else {
            continue;
        };
        match entity {
            EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::AttributeDefinition(_) => continue,
            EntityType::Insert(nested_ins) => {
                subs.push(LocalSub::Nested(build_nested_ref(nested_ins, doc, bg_color)));
            }
            _ => {
                if let Some(lw) = tessellate_sub_local(doc, entity, anno_scale, bg_color) {
                    subs.push(LocalSub::Wire(lw));
                }
            }
        }
    }
    BlockDefn {
        subs,
        aabb_local: [0.0; 4],
    }
}

fn build_nested_ref(
    nested_ins: &acadrust::entities::Insert,
    doc: &CadDocument,
    bg_color: [f32; 4],
) -> NestedRef {
    let (mut ins_color, ins_pat_len, ins_pat, ins_lw_px, _) =
        crate::scene::render::render_style_for(doc, &EntityType::Insert(nested_ins.clone()));
    ins_color = crate::scene::render::adapt_to_bg(ins_color, bg_color);

    NestedRef {
        block_name: nested_ins.block_name.clone(),
        xform: nested_ins.get_transform(),
        ins_color,
        ins_pat_len,
        ins_pat,
        ins_lw_px,
        color_is_byblock: nested_ins.common.color == AcadColor::ByBlock,
        lt_is_byblock: nested_ins.common.linetype.eq_ignore_ascii_case("byblock"),
        lw_is_byblock: matches!(nested_ins.common.line_weight, LineWeight::ByBlock),
        instance_offsets: array_offsets(nested_ins),
    }
}

fn tessellate_sub_local(
    doc: &CadDocument,
    sub: &EntityType,
    anno_scale: f32,
    bg_color: [f32; 4],
) -> Option<LocalWire> {
    let h = sub.common().handle;

    // Sanity guard: skip sub-entities whose primary dimension is so large
    // that adaptive tessellation will explode into hundreds of millions
    // of points. These are typically corrupt-radius primitives that slipped
    // past purge_corrupt_entities (finite but absurd values).
    if is_unreasonable_extent(sub) {
        return None;
    }

    let (sub_color, pat_len, pat, lw_px, aci) = crate::scene::render::render_style_for(doc, sub);
    let sub_color = crate::scene::render::adapt_to_bg(sub_color, bg_color);

    let color_is_byblock = sub.common().color == AcadColor::ByBlock;
    let lt_is_byblock = sub.common().linetype.eq_ignore_ascii_case("byblock");
    let lw_is_byblock = matches!(sub.common().line_weight, LineWeight::ByBlock);

    let wire = tessellate::tessellate(
        doc, h, sub, false, sub_color, pat_len, pat, lw_px, [0.0; 3], anno_scale,
    );

    if wire.points.len() > 100_000 {
        return None;
    }

    let aabb_local = aabb_from_points_iter(
        wire.points.iter().copied().chain(wire.fill_tris.iter().copied()),
    );

    Some(LocalWire {
        points: wire.points,
        key_vertices: wire.key_vertices,
        snap_pts: wire.snap_pts,
        tangent_geoms: wire.tangent_geoms,
        fill_tris: wire.fill_tris,
        color: sub_color,
        aci,
        pattern_length: pat_len,
        pattern: pat,
        line_weight_px: lw_px,
        plinegen: wire.plinegen,
        color_is_byblock,
        lt_is_byblock,
        lw_is_byblock,
        aabb_local,
    })
}

fn aabb_from_points_iter<I: IntoIterator<Item = [f32; 3]>>(pts: I) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in pts {
        if !p[0].is_finite() {
            continue;
        }
        if p[0] < min_x {
            min_x = p[0];
        }
        if p[1] < min_y {
            min_y = p[1];
        }
        if p[0] > max_x {
            max_x = p[0];
        }
        if p[1] > max_y {
            max_y = p[1];
        }
    }
    if min_x.is_infinite() {
        [0.0, 0.0, 0.0, 0.0]
    } else {
        [min_x, min_y, max_x, max_y]
    }
}

/// Transform a local-space XY AABB by `t` and return the world-space XY AABB
/// of the transformed corners. For non-rotated transforms the corners stay
/// axis-aligned; for arbitrary OCS we still get a correct (looser) AABB by
/// taking the min/max of the four transformed corner points.
fn transform_aabb_xy(local: [f32; 4], t: &Transform) -> [f32; 4] {
    let [x0, y0, x1, y1] = local;
    let corners = [
        Vector3::new(x0 as f64, y0 as f64, 0.0),
        Vector3::new(x1 as f64, y0 as f64, 0.0),
        Vector3::new(x1 as f64, y1 as f64, 0.0),
        Vector3::new(x0 as f64, y1 as f64, 0.0),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in corners {
        let v = t.apply(c);
        if v.x < min_x {
            min_x = v.x;
        }
        if v.y < min_y {
            min_y = v.y;
        }
        if v.x > max_x {
            max_x = v.x;
        }
        if v.y > max_y {
            max_y = v.y;
        }
    }
    [min_x as f32, min_y as f32, max_x as f32, max_y as f32]
}

fn aabb_union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    // [0,0,0,0] is the "empty AABB" sentinel produced by aabb_from_points_iter
    // when a wire has no finite points — treat it as if the other side wins.
    if a == [0.0, 0.0, 0.0, 0.0] {
        return b;
    }
    if b == [0.0, 0.0, 0.0, 0.0] {
        return a;
    }
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

pub fn aabb_disjoint_xy(a: [f32; 4], b: [f32; 4]) -> bool {
    a[2] < b[0] || a[0] > b[2] || a[3] < b[1] || a[1] > b[3]
}

// ── Use-time expansion ───────────────────────────────────────────────────────

/// Expand one top-level INSERT into world-space WireModels via the cache.
///
/// Returns `None` if no defn is cached for `ins.block_name`. Returns
/// `Some(empty)` if the defn exists but is empty.
pub fn expand_insert(
    cache: &BlockCache,
    ins: &acadrust::entities::Insert,
    ins_handle: Handle,
    ins_resolved_color: [f32; 4],
    ins_pat_len: f32,
    ins_pat: [f32; 8],
    ins_lw_px: f32,
    selected: bool,
    world_offset: [f64; 3],
    pslt_factor: f32,
    // World-space XY view AABB (with world_offset already subtracted, so the
    // comparison is in the same f32 space as emitted wires). `None` disables
    // frustum culling — every cached sub is emitted.
    view_aabb: Option<[f32; 4]>,
) -> Option<Vec<WireModel>> {
    let defn = cache.defn(&ins.block_name)?;
    let xform = ins.get_transform();
    let name = ins_handle.value().to_string();
    let mut batches = Batches::default();
    let mut visited: Vec<String> = Vec::with_capacity(8);
    let [ox, oy, _] = world_offset;

    // Whole-Insert cull: if the Insert's world AABB doesn't intersect the
    // view, bail without doing any per-sub work.
    if let Some(view) = view_aabb {
        let insert_world = transform_aabb_xy(defn.aabb_local, &xform);
        let insert_local = [
            (insert_world[0] - ox as f32),
            (insert_world[1] - oy as f32),
            (insert_world[2] - ox as f32),
            (insert_world[3] - oy as f32),
        ];
        if aabb_disjoint_xy(insert_local, view) {
            return Some(vec![]);
        }
    }

    for offset in &array_offsets(ins) {
        let base_xform = if offset == &[0.0; 3] {
            xform.clone()
        } else {
            let translation = Transform::from_translation(Vector3::new(
                offset[0], offset[1], offset[2],
            ));
            translation.then(&xform)
        };
        let ctx = ExpandCtx {
            cache,
            ins_color: ins_resolved_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            selected,
            world_offset,
            pslt_factor,
            view_aabb,
        };
        expand_defn(defn, &base_xform, &ctx, &mut batches, &mut visited, 0);
    }
    Some(batches.finalize(&name, selected))
}

struct ExpandCtx<'a> {
    cache: &'a BlockCache,
    ins_color: [f32; 4],
    ins_pat_len: f32,
    ins_pat: [f32; 8],
    ins_lw_px: f32,
    selected: bool,
    world_offset: [f64; 3],
    pslt_factor: f32,
    // World-space XY view AABB (post world_offset). `None` = no culling.
    view_aabb: Option<[f32; 4]>,
}

/// Style fingerprint used to group local wires into a single GPU buffer.
/// f32 fields are bit-cast to u32 to make the key Hash + Eq.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StyleKey {
    color: [u32; 4],
    pattern_length: u32,
    pattern: [u32; 8],
    line_weight_px: u32,
    aci: u8,
    plinegen: bool,
}

#[derive(Default, Debug)]
struct BatchEntry {
    color: [f32; 4],
    pattern_length: f32,
    pattern: [f32; 8],
    line_weight_px: f32,
    aci: u8,
    plinegen: bool,
    points: Vec<[f32; 3]>,
    snap_pts: Vec<(glam::Vec3, SnapHint)>,
    key_vertices: Vec<[f32; 3]>,
    tangent_geoms: Vec<TangentGeom>,
    fill_tris: Vec<[f32; 3]>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

/// Hard cap on point count for a single batched WireModel. Above this the
/// current batch is finalized (pushed into `closed`) and a fresh one is
/// started under the same style. Each WireModel point becomes ~6 GPU
/// vertices of 96 bytes — 200k points fits well under wgpu's 256 MB
/// per-buffer ceiling.
const MAX_POINTS_PER_BATCH: usize = 200_000;

#[derive(Default, Debug)]
struct Batches {
    by_style: HashMap<StyleKey, BatchEntry>,
    /// Batches that overflowed `MAX_POINTS_PER_BATCH` and have been closed.
    closed: Vec<BatchEntry>,
}

impl BatchEntry {
    fn new(color: [f32; 4], pat_len: f32, pat: [f32; 8], lw_px: f32, aci: u8, plinegen: bool) -> Self {
        Self {
            color,
            pattern_length: pat_len,
            pattern: pat,
            line_weight_px: lw_px,
            aci,
            plinegen,
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            ..Default::default()
        }
    }
}

impl Batches {
    fn finalize(self, name: &str, selected: bool) -> Vec<WireModel> {
        self.closed
            .into_iter()
            .chain(self.by_style.into_values())
            .map(|b| {
                let aabb = if b.min_x.is_infinite() {
                    WireModel::UNBOUNDED_AABB
                } else {
                    [b.min_x, b.min_y, b.max_x, b.max_y]
                };
                WireModel {
                    name: name.to_string(),
                    points: b.points,
                    color: b.color,
                    selected,
                    pattern_length: b.pattern_length,
                    pattern: b.pattern,
                    line_weight_px: b.line_weight_px,
                    aci: b.aci,
                    snap_pts: b.snap_pts,
                    tangent_geoms: b.tangent_geoms,
                    key_vertices: b.key_vertices,
                    aabb,
                    plinegen: b.plinegen,
                    vp_scissor: None,
                    fill_tris: b.fill_tris,
                }
            })
            .collect()
    }
}

fn style_key(
    color: [f32; 4],
    pat_len: f32,
    pat: [f32; 8],
    lw_px: f32,
    aci: u8,
    plinegen: bool,
) -> StyleKey {
    StyleKey {
        color: [
            color[0].to_bits(),
            color[1].to_bits(),
            color[2].to_bits(),
            color[3].to_bits(),
        ],
        pattern_length: pat_len.to_bits(),
        pattern: [
            pat[0].to_bits(),
            pat[1].to_bits(),
            pat[2].to_bits(),
            pat[3].to_bits(),
            pat[4].to_bits(),
            pat[5].to_bits(),
            pat[6].to_bits(),
            pat[7].to_bits(),
        ],
        line_weight_px: lw_px.to_bits(),
        aci,
        plinegen,
    }
}

fn expand_defn(
    defn: &BlockDefn,
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
    visited: &mut Vec<String>,
    depth: usize,
) {
    if depth > MAX_NESTING_DEPTH {
        eprintln!("block_cache: nested-block depth > {MAX_NESTING_DEPTH}, truncating");
        return;
    }
    for sub in &defn.subs {
        match sub {
            LocalSub::Wire(lw) => {
                if let Some(view) = ctx.view_aabb {
                    let world = transform_aabb_xy(lw.aabb_local, accum_xform);
                    let [ox, oy, _] = ctx.world_offset;
                    let local = [
                        world[0] - ox as f32,
                        world[1] - oy as f32,
                        world[2] - ox as f32,
                        world[3] - oy as f32,
                    ];
                    if aabb_disjoint_xy(local, view) {
                        continue;
                    }
                }
                emit_wire(lw, accum_xform, ctx, out);
            }
            LocalSub::Nested(nref) => {
                if visited.iter().any(|n| n == &nref.block_name) {
                    // Cycle — skip.
                    continue;
                }
                let Some(nested_defn) = ctx.cache.defn(&nref.block_name) else {
                    continue;
                };
                // Nested-INSERT cull: union AABB of the nested defn, transformed
                // by composed xform, vs view rect.
                if let Some(view) = ctx.view_aabb {
                    let composed = nref.xform.then(accum_xform);
                    let world = transform_aabb_xy(nested_defn.aabb_local, &composed);
                    let [ox, oy, _] = ctx.world_offset;
                    let local = [
                        world[0] - ox as f32,
                        world[1] - oy as f32,
                        world[2] - ox as f32,
                        world[3] - oy as f32,
                    ];
                    if aabb_disjoint_xy(local, view) {
                        continue;
                    }
                }
                // Resolve ByBlock for this nested ref against the outer ctx.
                let nested_color = if nref.color_is_byblock {
                    ctx.ins_color
                } else {
                    nref.ins_color
                };
                let (nested_pat_len, nested_pat) = if nref.lt_is_byblock {
                    (ctx.ins_pat_len, ctx.ins_pat)
                } else {
                    (nref.ins_pat_len, nref.ins_pat)
                };
                let nested_lw_px = if nref.lw_is_byblock {
                    ctx.ins_lw_px
                } else {
                    nref.ins_lw_px
                };
                let inner_ctx = ExpandCtx {
                    cache: ctx.cache,
                    ins_color: nested_color,
                    ins_pat_len: nested_pat_len,
                    ins_pat: nested_pat,
                    ins_lw_px: nested_lw_px,
                    selected: ctx.selected,
                    world_offset: ctx.world_offset,
                    pslt_factor: ctx.pslt_factor,
                    view_aabb: ctx.view_aabb,
                };
                visited.push(nref.block_name.clone());
                for offset in &nref.instance_offsets {
                    let composed = if offset == &[0.0; 3] {
                        nref.xform.then(accum_xform)
                    } else {
                        let translation = Transform::from_translation(Vector3::new(
                            offset[0], offset[1], offset[2],
                        ));
                        translation.then(&nref.xform).then(accum_xform)
                    };
                    expand_defn(
                        nested_defn,
                        &composed,
                        &inner_ctx,
                        out,
                        visited,
                        depth + 1,
                    );
                }
                visited.pop();
            }
        }
    }
}

fn emit_wire(lw: &LocalWire, accum_xform: &Transform, ctx: &ExpandCtx, out: &mut Batches) {
    if lw.points.is_empty() && lw.fill_tris.is_empty() {
        return;
    }
    let [ox, oy, oz] = ctx.world_offset;

    // Resolve final style for this LocalWire against the outer Insert ctx
    // before we hash it into a batch.
    let final_color = if ctx.selected {
        WireModel::SELECTED
    } else if lw.color_is_byblock {
        ctx.ins_color
    } else {
        lw.color
    };
    let (final_pat_len, final_pat) = if lw.lt_is_byblock {
        (ctx.ins_pat_len, ctx.ins_pat)
    } else {
        (lw.pattern_length, lw.pattern)
    };
    let final_lw_px = if lw.lw_is_byblock {
        ctx.ins_lw_px
    } else {
        lw.line_weight_px
    };
    let final_pat_len = final_pat_len * ctx.pslt_factor;
    let final_pat = final_pat.map(|v| v * ctx.pslt_factor);

    let key = style_key(
        final_color,
        final_pat_len,
        final_pat,
        final_lw_px,
        lw.aci,
        lw.plinegen,
    );

    // If the open batch for this style would exceed wgpu's per-buffer limit
    // after appending this wire, finalize it now and start a fresh batch.
    if let Some(existing) = out.by_style.get(&key) {
        if existing.points.len() + lw.points.len() + 1 > MAX_POINTS_PER_BATCH {
            if let Some(closed) = out.by_style.remove(&key) {
                out.closed.push(closed);
            }
        }
    }
    let entry = out.by_style.entry(key).or_insert_with(|| {
        BatchEntry::new(
            final_color,
            final_pat_len,
            final_pat,
            final_lw_px,
            lw.aci,
            lw.plinegen,
        )
    });

    // NaN separator between previously-appended geometry and this wire so the
    // GPU shader treats them as disconnected polylines within one buffer.
    let needs_sep = !entry.points.is_empty()
        && !entry.points.last().map(|p| p[0].is_nan()).unwrap_or(false);

    if !lw.points.is_empty() {
        if needs_sep {
            entry.points.push([f32::NAN; 3]);
        }
        for p in &lw.points {
            if p[0].is_nan() {
                entry.points.push([f32::NAN; 3]);
                continue;
            }
            let v = accum_xform.apply(Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            let q = [(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32];
            if q[0] < entry.min_x {
                entry.min_x = q[0];
            }
            if q[1] < entry.min_y {
                entry.min_y = q[1];
            }
            if q[0] > entry.max_x {
                entry.max_x = q[0];
            }
            if q[1] > entry.max_y {
                entry.max_y = q[1];
            }
            entry.points.push(q);
        }
    }

    for p in &lw.key_vertices {
        let v = accum_xform.apply(Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        entry
            .key_vertices
            .push([(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32]);
    }
    for (p, hint) in &lw.snap_pts {
        let v = accum_xform.apply(Vector3::new(p.x as f64, p.y as f64, p.z as f64));
        entry.snap_pts.push((
            glam::Vec3::new(
                (v.x - ox) as f32,
                (v.y - oy) as f32,
                (v.z - oz) as f32,
            ),
            *hint,
        ));
    }
    for tg in &lw.tangent_geoms {
        entry
            .tangent_geoms
            .push(transform_tangent(tg, accum_xform, [ox, oy, oz]));
    }
    for p in &lw.fill_tris {
        let v = accum_xform.apply(Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        entry
            .fill_tris
            .push([(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32]);
    }

}

fn transform_tangent(tg: &TangentGeom, t: &Transform, woff: [f64; 3]) -> TangentGeom {
    let [ox, oy, oz] = woff;
    match tg {
        TangentGeom::Line { p1, p2 } => {
            let q1 = t.apply(Vector3::new(p1[0] as f64, p1[1] as f64, p1[2] as f64));
            let q2 = t.apply(Vector3::new(p2[0] as f64, p2[1] as f64, p2[2] as f64));
            TangentGeom::Line {
                p1: [(q1.x - ox) as f32, (q1.y - oy) as f32, (q1.z - oz) as f32],
                p2: [(q2.x - ox) as f32, (q2.y - oy) as f32, (q2.z - oz) as f32],
            }
        }
        TangentGeom::Circle { center, radius } => {
            let c = t.apply(Vector3::new(
                center[0] as f64,
                center[1] as f64,
                center[2] as f64,
            ));
            let m = &t.matrix.m;
            let sx = ((m[0][0] * m[0][0] + m[0][1] * m[0][1] + m[0][2] * m[0][2]) as f64).sqrt();
            let sy = ((m[1][0] * m[1][0] + m[1][1] * m[1][1] + m[1][2] * m[1][2]) as f64).sqrt();
            let s = ((sx + sy) * 0.5) as f32;
            TangentGeom::Circle {
                center: [(c.x - ox) as f32, (c.y - oy) as f32, (c.z - oz) as f32],
                radius: radius * s,
            }
        }
    }
}

/// Radius / coordinate cap above which adaptive curve tessellation will
/// allocate hundreds of millions of points. `parameter_division` samples
/// to a fixed chord tolerance, so a Circle of radius 1e10 already produces
/// tens of millions of points.
const SANE_EXTENT: f64 = 1.0e8;

fn is_unreasonable_extent(e: &EntityType) -> bool {
    // Adaptive curve tessellation also explodes on degenerate primitives
    // (radius = 0, axes of length 0): `parameter_division` allocates
    // proportional to range/tolerance, which underflows when the curve
    // collapses to a point. Drop both ends of the spectrum.
    match e {
        EntityType::Circle(c) => c.radius.abs() < 1.0e-9 || c.radius.abs() > SANE_EXTENT,
        EntityType::Arc(a) => a.radius.abs() < 1.0e-9 || a.radius.abs() > SANE_EXTENT,
        EntityType::Ellipse(el) => {
            let mx = el.major_axis.x.abs() + el.major_axis.y.abs() + el.major_axis.z.abs();
            mx < 1.0e-9
                || el.major_axis.x.abs() > SANE_EXTENT
                || el.major_axis.y.abs() > SANE_EXTENT
                || el.major_axis.z.abs() > SANE_EXTENT
        }
        _ => false,
    }
}

fn array_offsets(ins: &acadrust::entities::Insert) -> Vec<[f64; 3]> {
    if !ins.is_minsert() {
        return vec![[0.0; 3]];
    }
    let mut offsets = Vec::with_capacity(ins.instance_count());
    for row in 0..ins.row_count {
        for col in 0..ins.column_count {
            offsets.push([
                col as f64 * ins.column_spacing,
                row as f64 * ins.row_spacing,
                0.0,
            ]);
        }
    }
    offsets
}

