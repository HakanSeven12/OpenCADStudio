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

use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use acadrust::types::{Color as AcadColor, LineWeight, Transform, Vector3};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::convert::tessellate;
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};

const MAX_NESTING_DEPTH: usize = 32;
/// Skip wires whose world-AABB projects to fewer than this many pixels in
/// the active view. Picks up tiny detail at zoom-out so the tessellator
/// doesn't waste time on geometry that contributes a few sub-pixel marks
/// to the final image. 2 px is the AutoCAD-default "small element" floor
/// — visibly the same image, dramatically fewer wires.
const MIN_PIXEL_SIZE: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct LocalWire {
    pub points: Vec<[f32; 3]>,
    /// Low-bit residual paired with `points` so block-instance wires keep
    /// sub-f32 precision once the renderer translates them to world space.
    pub points_low: Vec<[f32; 3]>,
    pub key_vertices: Vec<[f64; 3]>,
    pub snap_pts: Vec<(glam::DVec3, SnapHint)>,
    pub tangent_geoms: Vec<TangentGeom>,
    pub fill_tris: Vec<[f32; 3]>,
    pub fill_tris_low: Vec<[f32; 3]>,
    /// Per-wire colour from the tessellator output. For most entities this
    /// equals the sub-entity's resolved colour. For colour-split MTEXT
    /// (`\C`/`\c` inline overrides) each wire carries its own override colour.
    pub color: [f32; 4],
    pub aci: u8,
    pub pattern_length: f32,
    pub pattern: [f32; 8],
    pub line_weight_px: f32,
    pub plinegen: bool,
    /// Set at construction; used to discriminate fill-only GPU batches from
    /// stroke batches in [`StyleKey`]. Derived from
    /// `points.is_empty() && !fill_tris.is_empty()`.
    pub is_fill_only: bool,
    pub color_is_byblock: bool,
    pub lt_is_byblock: bool,
    pub lw_is_byblock: bool,
    /// Set when this child sits on layer "0" and the matching property is
    /// ByLayer. At expand time the value is then taken from the INSERT's
    /// *layer* (the layer-0 inheritance rule) instead of the cached layer-0
    /// value baked here.
    pub color_l0: bool,
    pub lt_l0: bool,
    pub lw_l0: bool,
    /// XY bounding box of this wire in block-local coordinates.
    /// `[min_x, min_y, max_x, max_y]`. Used for view-frustum culling at
    /// expand-time: transform corners by the Insert transform → world AABB
    /// → test against the camera's world-space view rect.
    pub aabb_local: [f32; 4],
    /// For Text / MText subs: the entity's anno-scaled glyph height in
    /// local units. Lets `emit_wire` apply the same LOD ladder used for
    /// top-level text (cull / greek / full) to text that's been baked into
    /// a block defn. `None` for non-text entities.
    pub text_height_local: Option<f32>,
    /// For Text / MText subs: the 4 OBB corners in block-local coords
    /// (rotation, anchor offsets and width-approximation already applied).
    /// Emitted at greek time so the rect matches the text's orientation
    /// instead of falling back to the axis-aligned bbox.
    pub text_obb_local: Option<[[f64; 3]; 4]>,
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
    /// The nested INSERT's own properties are ByLayer. Combined with
    /// `layer_is_zero` these drive the layer-0 rule for the nested insert
    /// *itself* (so its ByBlock leaves inherit the outer layer, not layer 0).
    pub color_is_bylayer: bool,
    pub lt_is_bylayer: bool,
    pub lw_is_bylayer: bool,
    /// The nested INSERT itself sits on layer "0": its own layer-0 children
    /// chain up to the outer insert's layer rather than resolving to `l0`.
    pub layer_is_zero: bool,
    /// The nested INSERT's own layer style — the layer-0 inheritance target
    /// for its children when the nested insert is not itself on layer 0.
    pub l0: crate::scene::view::render::InheritStyle,
    pub instance_offsets: Vec<[f64; 3]>,
    /// XCLIP boundary for this nested insert, in the parent defn's local frame
    /// (`None` = unclipped). Baked at build time because the clip's spatial
    /// filter lives in `doc.objects`, which isn't reachable at expand time; on
    /// expansion it is mapped to world by the accumulated transform and the
    /// nested wires are clipped to it.
    pub clip_poly: Option<Vec<[f64; 2]>>,
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
    /// the wire renderer is 2D-dominant. Expressed in this defn's *offset*
    /// Absolute world-space XY (the double-single render path keeps it precise).
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
        use crate::par::prelude::*;
        let mut cache = Self::new();
        let referenced = collect_referenced_blocks(doc);
        // Each defn is built independently: nested INSERTs are stored as
        // by-name references (`LocalSub::Nested`), never expanded here, so a
        // block's build never depends on another block's defn. That makes the
        // builds embarrassingly parallel over the read-only `doc` — no
        // topological ordering required. `compute_block_aabbs` stays a serial
        // post-pass (it resolves nested references and is comparatively cheap).
        cache.defns = referenced
            .par_iter()
            .map(|name| (name.clone(), Arc::new(build_defn(doc, name, anno_scale, bg_color))))
            .collect();
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
        use crate::par::prelude::*;
        // Phase 1 (parallel, read-only): each defn's union AABB is resolved
        // by `defn_aabb_recursive`, which only *reads* `self.defns` (the map
        // is fully built by now). There's no memoization, so a defn shared by
        // many parents is re-walked per parent — real work on block-heavy
        // drawings — and the per-name walks are independent, so they fan out.
        let this: &Self = self;
        let resolved: Vec<(&String, [f32; 4])> = names
            .par_iter()
            .map(|name| {
                let mut visited: Vec<String> = Vec::new();
                (name, this.defn_aabb_recursive(name, &mut visited))
            })
            .collect();
        // Phase 2 (serial): store the AABB back into each defn.
        for (name, aabb) in resolved {
            if let Some(defn_arc) = self.defns.get_mut(name) {
                let mut defn = (**defn_arc).clone();
                defn.aabb_local = aabb;
                *defn_arc = Arc::new(defn);
            }
        }
    }

    /// Returns the union AABB for `block_name`'s defn, expressed in **that
    /// defn's offset frame** (so its caller can store it in
    /// `BlockDefn.aabb_local` without a coordinate-frame mismatch).
    ///
    /// LocalWire contributions are already in the parent defn's offset
    /// frame. Nested-INSERT contributions live in the *child* defn's offset
    /// frame, so we re-add `child.local_offset` (f64), apply the nested
    /// Insert's transform to get parent-native coordinates, then subtract
    /// `parent.local_offset` to land back in the parent's offset frame.
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
                    let nested = self.defn_aabb_recursive(&nref.block_name, visited);
                    transform_aabb_xy(nested, &nref.xform)
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
    use rustc_hash::FxHashSet as HashSet;
    let mut seen: HashSet<String> = HashSet::default();
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

/// True when `layer` is turned off or frozen — entities on it never render.
fn layer_hidden(doc: &CadDocument, layer: &str) -> bool {
    doc.layers
        .get(layer)
        .map(|l| l.flags.off || l.flags.frozen)
        .unwrap_or(false)
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


    // ── Pass 2: tessellate each sub with the chosen offset so stored
    // coordinates fit into f32 without precision loss.
    let cap = br.entity_handles.len();
    let mut subs: Vec<LocalSub> = Vec::with_capacity(cap);
    for &eh in &br.entity_handles {
        let Some(entity) = doc.get_entity(eh) else {
            continue;
        };
        // Skip entities flagged invisible. Dynamic blocks (e.g. a visibility-
        // state parametric block) keep the geometry for every state in one
        // anonymous block and mark all but the active state's entities
        // invisible — honouring the flag is what shows a single profile
        // instead of every variant stacked on top of each other.
        if entity.common().invisible {
            continue;
        }
        // A sub-entity on a layer that is off or frozen must not render, same
        // as a top-level entity on that layer. The defn cache is rebuilt on
        // every layer off/freeze toggle (bump_geometry bumps block_epoch), so
        // baking the visibility here stays in sync.
        if layer_hidden(doc, &entity.common().layer) {
            continue;
        }
        match entity {
            EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::AttributeDefinition(_) => continue,
            EntityType::Insert(nested_ins) => {
                subs.push(LocalSub::Nested(build_nested_ref(nested_ins, doc, bg_color)));
            }
            _ => {
                for lw in tessellate_sub_local(doc, entity, anno_scale, bg_color) {
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
    // Store the RAW colour — `adapt_to_bg` runs at emit time
    // (`Batches::finalize`) so the same cached defn can serve renders
    // against different backgrounds without rebuilding.
    let (ins_color, ins_pat_len, ins_pat, ins_lw_px, _) =
        crate::scene::view::render::render_style_for(doc, &EntityType::Insert(nested_ins.clone()));
    // The nested insert's own layer style — the layer-0 target for its
    // children (raw colour; adapted at emit like `ins_color`).
    let l0 = crate::scene::view::render::layer_render_style(doc, &nested_ins.common.layer);
    let _ = bg_color;

    // Bake the XCLIP boundary (parent-defn-local) so the nested insert keeps
    // its clip when the parent block is expanded — the spatial filter object
    // isn't reachable at expand time.
    let clip_poly = crate::scene::pick::xclip::insert_spatial_filter(doc, nested_ins)
        .map(|sf| crate::scene::pick::xclip::world_clip_polygon_f64(sf, nested_ins));

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
        color_is_bylayer: nested_ins.common.color == AcadColor::ByLayer,
        lt_is_bylayer: {
            let lt = &nested_ins.common.linetype;
            lt.is_empty() || lt.eq_ignore_ascii_case("bylayer")
        },
        lw_is_bylayer: matches!(
            nested_ins.common.line_weight,
            LineWeight::ByLayer | LineWeight::Default
        ),
        layer_is_zero: nested_ins.common.layer == "0",
        l0,
        instance_offsets: array_offsets(nested_ins),
        clip_poly,
    }
}

fn tessellate_sub_local(
    doc: &CadDocument,
    sub: &EntityType,
    anno_scale: f32,
    bg_color: [f32; 4],
) -> Vec<LocalWire> {
    let h = sub.common().handle;

    // Sanity guard: skip sub-entities whose primary dimension is so large
    // that adaptive tessellation will explode into hundreds of millions
    // of points. These are typically corrupt-radius primitives that slipped
    // past purge_corrupt_entities (finite but absurd values).
    if is_unreasonable_extent(sub) {
        return vec![];
    }

    // Store the RAW colour. `Batches::finalize` applies `adapt_to_bg`
    // with the per-render bg, so the cache no longer has to rebuild on
    // BACKGROUND / layout-switch — the dynamic adaptation tracks the
    // live bg at render time.
    let (sub_color, pat_len, pat, lw_px, aci) = crate::scene::view::render::render_style_for(doc, sub);
    let _ = bg_color;

    let color_is_byblock = sub.common().color == AcadColor::ByBlock;
    let lt_is_byblock = sub.common().linetype.eq_ignore_ascii_case("byblock");
    let lw_is_byblock = matches!(sub.common().line_weight, LineWeight::ByBlock);

    // Layer-0 rule: a child on layer "0" with ByLayer properties inherits the
    // INSERT's layer at expand time. Flag each ByLayer property so emit_wire
    // can override the cached (layer-0-resolved) value with the insert layer's.
    let on_l0 = sub.common().layer == "0";
    let color_l0 = on_l0 && sub.common().color == AcadColor::ByLayer;
    let lt_l0 = on_l0 && {
        let lt = &sub.common().linetype;
        lt.is_empty() || lt.eq_ignore_ascii_case("bylayer")
    };
    let lw_l0 = on_l0
        && matches!(
            sub.common().line_weight,
            LineWeight::ByLayer | LineWeight::Default
        );

    // Pass `local_offset` as the f64 world-offset so tessellate subtracts it
    // before casting to f32 — same precision-preservation trick used for
    // top-level entities, applied per-defn.
    let wires_out = tessellate::tessellate(
        doc, h, sub, false, sub_color, pat_len, pat, lw_px, anno_scale, None,
    );
    if wires_out.is_empty() {
        return vec![];
    }

    let text_height_local: Option<f32> = match sub {
        EntityType::Text(t) => Some((t.height * anno_scale as f64) as f32),
        EntityType::MText(m) => Some((m.height * anno_scale as f64) as f32),
        _ => None,
    };

    // Pre-compute the OBB corners (absolute f64) for Text / MText so the greek
    // path can emit a rect that matches the entity's rotation instead of
    // falling back to the axis-aligned `aabb_local`. Kept in f64 so it stays
    // precise at UTM scale (the emit path splits to double-single). For MText
    // the OBB height is built from the wrap-expanded line count so
    // emit_greeked_text can split it into per-line rows.
    let mtext_lines = match sub {
        EntityType::MText(m) => Some(
            crate::entities::text_support::mtext_line_count(m, doc, anno_scale),
        ),
        _ => None,
    };
    let text_obb_local: Option<[[f64; 3]; 4]> =
        crate::entities::text_support::text_obb_corners_native(sub, anno_scale, mtext_lines);

    let mut result = Vec::with_capacity(wires_out.len());
    for wire in wires_out {
        // Per-wire point-count cap: a single wire that exceeds this is skipped
        // rather than aborting the whole sub-entity — with per-wire separation,
        // other colour segments from the same entity still render.
        if wire.points.len() > 100_000 {
            continue;
        }

        // Geometry is stored absolute; the double-single (high/low) render path
        // keeps it precise at UTM scale, so no per-defn offset is subtracted.
        let aabb_local = aabb_from_points_iter(
            wire.points.iter().copied().chain(wire.fill_tris.iter().copied()),
        );
        let is_fill_only = wire.points.is_empty() && !wire.fill_tris.is_empty();

        // A wire whose colour differs from the entity's resolved base colour
        // carries an explicit per-segment override (e.g. an MTEXT `\C1;` inline
        // colour). ByBlock / layer-0 inheritance only applies to wires still on
        // the base colour — folding explicit segments into the inherited colour
        // collapses colour-split text to one colour.
        let wire_on_base_color = wire.color == sub_color;

        result.push(LocalWire {
            points: wire.points,
            points_low: wire.points_low,
            key_vertices: wire.key_vertices,
            snap_pts: wire.snap_pts,
            tangent_geoms: wire.tangent_geoms,
            fill_tris: wire.fill_tris,
            fill_tris_low: wire.fill_tris_low,
            color: wire.color,
            aci,
            pattern_length: pat_len,
            pattern: pat,
            line_weight_px: lw_px,
            plinegen: wire.plinegen,
            is_fill_only,
            color_is_byblock: color_is_byblock && wire_on_base_color,
            lt_is_byblock,
            lw_is_byblock,
            color_l0: color_l0 && wire_on_base_color,
            lt_l0,
            lw_l0,
            aabb_local,
            text_height_local,
            text_obb_local,
        });
    }
    result
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

/// Transform an absolute XY AABB by `t` and return the world-space XY AABB of
/// the transformed corners (computed in f64 so it stays accurate for distant
/// content).
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
    // The INSERT's own layer style — layer-0 inheritance target for children.
    ins_layer: crate::scene::view::render::InheritStyle,
    selected: bool,
    pslt_factor: f32,
    // World-space XY view AABB (with world_offset already subtracted, so the
    // comparison is in the same f32 space as emitted wires). `None` disables
    // frustum culling — every cached sub is emitted.
    view_aabb: Option<[f32; 4]>,
    // World units per screen pixel. When `Some`, wires whose AABB projects
    // smaller than `MIN_PIXEL_SIZE` get skipped entirely (LOD).
    world_per_pixel: Option<f32>,
    // True when `ins.block_name` resolves to an xref BlockRecord. All emitted
    // colors are faded toward `bg_color` so xrefs are visually distinguishable
    // from native content.
    is_xref: bool,
    bg_color: [f32; 4],
) -> Option<Vec<WireModel>> {
    let defn = cache.defn(&ins.block_name)?;
    let xform = ins.get_transform();
    let name = ins_handle.value().to_string();
    let mut batches = Batches::default();
    let mut visited: Vec<String> = Vec::with_capacity(8);

    // `defn.aabb_local` is in the defn's offset frame — re-add
    // `defn.local_offset` (f64) before transforming so the world AABB is
    // accurate for distant content.
    let insert_world = transform_aabb_xy(defn.aabb_local, &xform);
    let insert_local = [
        insert_world[0] as f32,
        insert_world[1] as f32,
        insert_world[2] as f32,
        insert_world[3] as f32,
    ];

    // Whole-Insert frustum cull.
    if let Some(view) = view_aabb {
        if aabb_disjoint_xy(insert_local, view) {
            return Some(vec![]);
        }
    }
    // Whole-Insert pixel-size LOD: if the entire Insert footprint projects
    // to sub-pixel size, skip it entirely.
    if let Some(wpp) = world_per_pixel {
        if aabb_pixel_size(insert_local, wpp) < MIN_PIXEL_SIZE {
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
            l0: ins_layer,
            selected,
            pslt_factor,
            view_aabb,
            world_per_pixel,
            is_xref,
            bg_color,
        };
        expand_defn(defn, &base_xform, &ctx, &mut batches, &mut visited, 0);
    }
    Some(batches.finalize(&name, selected, bg_color))
}

/// Emit a greeked filled rect (2 triangles) for a text LocalWire. Stays in
/// the text's own color — the face3d pipeline skips its 0.45 dim for wires
/// whose `points` are empty, so the rect lands at full intensity. Clamped
/// to a single line's height so multi-line MText doesn't blow the box up
/// to the full block. Mirrors `text_greek_obb_tris` in scene/mod.rs.
/// Falls back to the axis-aligned `local_aabb` when no OBB was cached;
/// xref fade still applies via the shared color resolution.
fn emit_greeked_text(
    lw: &LocalWire,
    local_aabb: [f32; 4],
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
) {
    // Transform absolute f64 corners and keep the result in f64 so the
    // per-vertex double-single split below stays precise at UTM-scale coords.
    let xf = |p: [f64; 3]| -> [f64; 3] {
        let w = accum_xform.apply(Vector3::new(p[0], p[1], p[2]));
        [w.x, w.y, w.z]
    };

    // Per-line rects: split the OBB by `h_local` so each visible wrap
    // line gets its own box. Falls back to the axis-aligned AABB when no
    // OBB / height was cached.
    let tris: Vec<[f64; 3]> = if let (Some(obb), Some(h_local)) =
        (lw.text_obb_local, lw.text_height_local)
    {
        if h_local <= 0.0 {
            return;
        }
        let h = h_local as f64;
        let (ux, uy, uz) = (
            obb[3][0] - obb[0][0],
            obb[3][1] - obb[0][1],
            obb[3][2] - obb[0][2],
        );
        let ulen = (ux * ux + uy * uy + uz * uz).sqrt();
        if ulen < 1e-9 {
            return;
        }
        let n_lines = ((ulen / h).round() as usize).max(1);
        let (nx, ny, nz) = (ux / ulen, uy / ulen, uz / ulen);
        let bl_local = obb[0];
        let br_local = obb[1];
        let mut acc = Vec::with_capacity(n_lines * 6);
        for i in 0..n_lines {
            let top_along = (ulen - (i as f64) * h).max(0.0);
            let bot_along = (ulen - ((i + 1) as f64) * h).max(0.0);
            let tl = xf([
                bl_local[0] + nx * top_along,
                bl_local[1] + ny * top_along,
                bl_local[2] + nz * top_along,
            ]);
            let tr = xf([
                br_local[0] + nx * top_along,
                br_local[1] + ny * top_along,
                br_local[2] + nz * top_along,
            ]);
            let bl = xf([
                bl_local[0] + nx * bot_along,
                bl_local[1] + ny * bot_along,
                bl_local[2] + nz * bot_along,
            ]);
            let br = xf([
                br_local[0] + nx * bot_along,
                br_local[1] + ny * bot_along,
                br_local[2] + nz * bot_along,
            ]);
            acc.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
        }
        acc
    } else {
        let [x0, y0, x1, y1] = local_aabb;
        let bl = xf([x0 as f64, y0 as f64, 0.0]);
        let br = xf([x1 as f64, y0 as f64, 0.0]);
        let tr = xf([x1 as f64, y1 as f64, 0.0]);
        let tl = xf([x0 as f64, y1 as f64, 0.0]);
        vec![bl, br, tr, bl, tr, tl]
    };
    if tris.is_empty() {
        return;
    }

    let final_color = resolve_wire_color(lw, ctx);

    let key = style_key(final_color, 0.0, [0.0; 8], 1.0, lw.aci, true, true);
    let entry = out
        .by_style
        .entry(key)
        .or_insert_with(|| BatchEntry::new(final_color, 0.0, [0.0; 8], 1.0, lw.aci, true, true));

    for p in tris {
        let (hx, lx) = WireModel::split_ds(p[0]);
        let (hy, ly) = WireModel::split_ds(p[1]);
        let (hz, lz) = WireModel::split_ds(p[2]);
        if hx < entry.min_x {
            entry.min_x = hx;
        }
        if hy < entry.min_y {
            entry.min_y = hy;
        }
        if hx > entry.max_x {
            entry.max_x = hx;
        }
        if hy > entry.max_y {
            entry.max_y = hy;
        }
        entry.fill_tris.push([hx, hy, hz]);
        entry.fill_tris_low.push([lx, ly, lz]);
    }
}

/// Emit a 2-point baseline line for a sub-pixel text LocalWire — same color
/// resolution as `emit_wire`, but the wire's stored glyph points are
/// replaced with the OBB's baseline edge (`obb[0] → obb[1]`). Falls back to
/// a no-op when no OBB is cached (rotation/attachment unknown).
fn emit_text_baseline(
    lw: &LocalWire,
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
    wpp: f32,
) {
    let Some(obb) = lw.text_obb_local else {
        return;
    };
    let Some(h_local) = lw.text_height_local else {
        return;
    };
    if h_local <= 0.0 {
        return;
    }
    let xf = |p: [f64; 3]| -> [f64; 3] {
        let w = accum_xform.apply(Vector3::new(p[0], p[1], p[2]));
        [w.x, w.y, w.z]
    };

    // Single line: just obb[0] → obb[1]. Skip when the projected length
    // falls under 2 px (single-char text seen edge-on). All wrap lines
    // share the same baseline length, so checking once is enough.
    let p0_world = xf(obb[0]);
    let p1_world = xf(obb[1]);
    let dx = p1_world[0] - p0_world[0];
    let dy = p1_world[1] - p0_world[1];
    let len_px = (dx * dx + dy * dy).sqrt() / wpp as f64;
    if len_px < 2.0 {
        return;
    }

    // Compute the per-line baseline endpoints in *local* frame, then
    // transform — keeps precision for distant text and matches the
    // greek/baseline math used by the rect path.
    let (ux, uy, uz) = (
        obb[3][0] - obb[0][0],
        obb[3][1] - obb[0][1],
        obb[3][2] - obb[0][2],
    );
    let ulen = (ux * ux + uy * uy + uz * uz).sqrt();
    let n_lines = if ulen < 1e-9 {
        1
    } else {
        ((ulen / h_local as f64).round() as usize).max(1)
    };
    let (nx, ny, nz) = if ulen < 1e-9 {
        (0.0, 0.0, 0.0)
    } else {
        (ux / ulen, uy / ulen, uz / ulen)
    };

    let final_color = resolve_wire_color(lw, ctx);

    let key = style_key(final_color, 0.0, [0.0; 8], 1.0, lw.aci, true, false);
    let entry = out
        .by_style
        .entry(key)
        .or_insert_with(|| BatchEntry::new(final_color, 0.0, [0.0; 8], 1.0, lw.aci, true, false));

    let bl_local = obb[0];
    let br_local = obb[1];
    for i in 0..n_lines {
        let along = if ulen < 1e-9 {
            0.0
        } else {
            (ulen - ((i + 1) as f64) * h_local as f64).max(0.0)
        };
        let p0 = xf([
            bl_local[0] + nx * along,
            bl_local[1] + ny * along,
            bl_local[2] + nz * along,
        ]);
        let p1 = xf([
            br_local[0] + nx * along,
            br_local[1] + ny * along,
            br_local[2] + nz * along,
        ]);

        let needs_sep = !entry.points.is_empty()
            && !entry.points.last().map(|p| p[0].is_nan()).unwrap_or(false);
        if needs_sep {
            entry.points.push([f32::NAN; 3]);
            entry.points_low.push([0.0; 3]);
        }
        for q in [p0, p1] {
            let (hx, lx) = WireModel::split_ds(q[0]);
            let (hy, ly) = WireModel::split_ds(q[1]);
            let (hz, lz) = WireModel::split_ds(q[2]);
            if hx < entry.min_x {
                entry.min_x = hx;
            }
            if hy < entry.min_y {
                entry.min_y = hy;
            }
            if hx > entry.max_x {
                entry.max_x = hx;
            }
            if hy > entry.max_y {
                entry.max_y = hy;
            }
            entry.points.push([hx, hy, hz]);
            entry.points_low.push([lx, ly, lz]);
        }
    }
}

fn aabb_pixel_size(local_aabb: [f32; 4], world_per_pixel: f32) -> f32 {
    let w = (local_aabb[2] - local_aabb[0]).abs();
    let h = (local_aabb[3] - local_aabb[1]).abs();
    w.max(h) / world_per_pixel
}

struct ExpandCtx<'a> {
    cache: &'a BlockCache,
    ins_color: [f32; 4],
    ins_pat_len: f32,
    ins_pat: [f32; 8],
    ins_lw_px: f32,
    /// Layer-0 inheritance target — the current INSERT's *layer* style, used
    /// for child wires on layer "0" whose properties are ByLayer.
    l0: crate::scene::view::render::InheritStyle,
    selected: bool,
    pslt_factor: f32,
    // World-space XY view AABB (post world_offset). `None` = no culling.
    view_aabb: Option<[f32; 4]>,
    // World units per screen pixel. `None` = no pixel-size LOD.
    world_per_pixel: Option<f32>,
    // True when this expansion descends from an xref INSERT. Causes emitted
    // colors to be faded toward `bg_color` so the user can tell at a glance
    // which geometry comes from an external reference.
    is_xref: bool,
    bg_color: [f32; 4],
}

/// Fade `color` toward `bg` by 50%, preserving alpha. Used to mark xref
/// geometry — the hue stays recognizable but the contrast against the
/// background drops, reading as "washed out".
pub(crate) fn fade_toward_bg(color: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
    const T: f32 = 0.5;
    [
        color[0] * (1.0 - T) + bg[0] * T,
        color[1] * (1.0 - T) + bg[1] * T,
        color[2] * (1.0 - T) + bg[2] * T,
        color[3],
    ]
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
    /// Marks batches that emit only `fill_tris` with no wire `points`. The
    /// face3d pipeline uses `wire.points.is_empty()` as the "skip dim"
    /// discriminator, so greek fills must stay in their own batches even
    /// when their color/style would otherwise collide with regular wires.
    is_fill_only: bool,
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
    points_low: Vec<[f32; 3]>,
    snap_pts: Vec<(glam::DVec3, SnapHint)>,
    key_vertices: Vec<[f64; 3]>,
    tangent_geoms: Vec<TangentGeom>,
    fill_tris: Vec<[f32; 3]>,
    /// Double-single low residual paired with `fill_tris`, so block fills stay
    /// precise at UTM-scale coordinates (the renderer's relative-to-eye path
    /// reconstructs `high + low`). Without it absolute f32 fills quantize to
    /// ~0.5 m and the greek-text rectangles shear.
    fill_tris_low: Vec<[f32; 3]>,
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
    /// Already-finalized wires that bypass the point batcher — currently the
    /// clipped output of an XCLIP'd nested insert, which is produced as whole
    /// WireModels by `clip_wires`. Appended verbatim at `finalize` (only their
    /// name/selected flag are stamped to match the host insert).
    extra_wires: Vec<WireModel>,
}

impl BatchEntry {
    fn new(
        color: [f32; 4],
        pat_len: f32,
        pat: [f32; 8],
        lw_px: f32,
        aci: u8,
        plinegen: bool,
        _is_fill_only: bool,
    ) -> Self {
        // `is_fill_only` is part of the StyleKey hash so greek fills never
        // share a batch with regular wires (otherwise the finalized
        // WireModel would have both `points` and `fill_tris`, defeating
        // the face3d-dim discriminator). It isn't stored on the entry
        // itself — the empty `points` field is enough at finalize time.
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
    fn finalize(self, name: &str, selected: bool, bg_color: [f32; 4]) -> Vec<WireModel> {
        let extra = self.extra_wires;
        let mut out: Vec<WireModel> = self
            .closed
            .into_iter()
            .chain(self.by_style.into_values())
            .map(|b| {
                let aabb = if b.min_x.is_infinite() {
                    WireModel::UNBOUNDED_AABB
                } else {
                    [b.min_x, b.min_y, b.max_x, b.max_y]
                };
                // RAW colour came from `tessellate_sub_local` (and from
                // `expand_defn`'s ByBlock fallbacks); apply `adapt_to_bg`
                // now so each render against a different bg gets the
                // right pure-black ↔ pure-white flip without rebuilding
                // the cached defn.
                let color = crate::scene::view::render::adapt_to_bg(b.color, bg_color);
                WireModel {
                    name: name.to_string(),
                    points: b.points,
                    points_low: b.points_low,
                    color,
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
                    fill_tris_low: b.fill_tris_low,
                }
            })
            .collect();
        // Clipped nested-insert wires are already whole WireModels; stamp the
        // host insert's name/selected so picking maps them back correctly.
        for mut w in extra {
            w.name = name.to_string();
            w.selected = selected;
            out.push(w);
        }
        out
    }
}

fn style_key(
    color: [f32; 4],
    pat_len: f32,
    pat: [f32; 8],
    lw_px: f32,
    aci: u8,
    plinegen: bool,
    is_fill_only: bool,
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
        is_fill_only,
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
    // Track the last text OBB that received a LOD substitute (baseline line
    // or greeked box). Colour-split MTEXT produces one outline wire per
    // \C/\c segment; only the first outline from a source entity should
    // emit the substitute — subsequent segments use the same full-document
    // OBB and would draw overlapping rectangles with the wrong colour.
    let mut last_lod_obb: Option<[[f64; 3]; 4]> = None;
    for sub in &defn.subs {
        match sub {
            LocalSub::Wire(lw) => {
                // `lw.aabb_local` is in the defn's offset frame; re-add
                // `defn_lo` (in f64) before composing with `accum_xform`
                // so culling uses correct world-space corners.
                let world = transform_aabb_xy(lw.aabb_local, accum_xform);
                let local = [
                    world[0] as f32,
                    world[1] as f32,
                    world[2] as f32,
                    world[3] as f32,
                ];
                if let Some(view) = ctx.view_aabb {
                    if aabb_disjoint_xy(local, view) {
                        continue;
                    }
                }
                if let Some(wpp) = ctx.world_per_pixel {
                    // Text/MText follows its own ladder below (baseline-line
                    // / greek / full) — must reach it even when the wire's
                    // AABB falls under MIN_PIXEL_SIZE.
                    let is_text = lw.text_height_local.is_some();
                    if !is_text && aabb_pixel_size(local, wpp) < MIN_PIXEL_SIZE {
                        continue;
                    }
                    // Text LOD ladder: text inside a block follows the same
                    // 1 / 5 px baseline/greek/full rules as top-level text.
                    // We apply the Insert's transform scale to the stored
                    // local glyph height to get the screen height.
                    if let Some(h_local) = lw.text_height_local {
                        let m = &accum_xform.matrix.m;
                        let sy = ((m[1][0] * m[1][0]
                            + m[1][1] * m[1][1]
                            + m[1][2] * m[1][2]) as f64)
                            .sqrt() as f32;
                        let h_world = h_local * sy;
                        let h_px = h_world / wpp;
                        if lw.is_fill_only {
                            // Fill-only wire: suppress at text LOD thresholds.
                            // The outline sibling carries the LOD substitute
                            // (baseline line or greeked box). Emitting every
                            // glyph triangle at sub-pixel zoom defeats the
                            // purpose of the LOD ladder.
                            if h_px < 5.0 {
                                continue;
                            }
                        } else {
                            if h_px < 1.0 {
                                // Colour-split MTEXT: only the first outline
                                // wire from a source entity emits the baseline
                                // substitute. Siblings share the same
                                // text_obb_local and would stack identical
                                // overlapping lines (last colour wins) — the
                                // same dedup the greek branch does, one tier
                                // down.
                                if lw.text_obb_local == last_lod_obb {
                                    continue;
                                }
                                last_lod_obb = lw.text_obb_local;
                                emit_text_baseline(lw, accum_xform, ctx, out, wpp);
                                continue;
                            }
                            if h_px < 5.0 {
                                // Colour-split MTEXT: only the first outline
                                // wire from a source entity emits the greek
                                // substitute. Subsequent segments share the
                                // same text_obb_local and would draw
                                // overlapping rectangles with the wrong
                                // (last-wins) colour.
                                if lw.text_obb_local == last_lod_obb {
                                    continue;
                                }
                                last_lod_obb = lw.text_obb_local;
                                emit_greeked_text(lw, local, accum_xform, ctx, out);
                                continue;
                            }
                        }
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
                // Nested-INSERT cull: union AABB of the nested defn,
                // transformed by composed xform, vs view rect + pixel size.
                // `nested_defn.aabb_local` lives in the nested defn's offset
                // frame — re-add `nested_defn.local_offset` in f64 before
                // composing with the parent transforms.
                let composed = nref.xform.then(accum_xform);
                let world = transform_aabb_xy(nested_defn.aabb_local, &composed);
                let local = [
                    world[0] as f32,
                    world[1] as f32,
                    world[2] as f32,
                    world[3] as f32,
                ];
                if let Some(view) = ctx.view_aabb {
                    if aabb_disjoint_xy(local, view) {
                        continue;
                    }
                }
                if let Some(wpp) = ctx.world_per_pixel {
                    if aabb_pixel_size(local, wpp) < MIN_PIXEL_SIZE {
                        continue;
                    }
                }
                // Resolve the nested insert's own style against the outer ctx:
                // ByBlock inherits the outer insert; a nested insert that is
                // itself on layer "0" with ByLayer props inherits the outer
                // layer-0 target (so its ByBlock leaves resolve to that layer,
                // not layer 0). Mirrors the leaf resolution in emit_wire.
                let nested_color = if nref.color_is_byblock {
                    ctx.ins_color
                } else if nref.layer_is_zero && nref.color_is_bylayer {
                    ctx.l0.color
                } else {
                    nref.ins_color
                };
                let (nested_pat_len, nested_pat) = if nref.lt_is_byblock {
                    (ctx.ins_pat_len, ctx.ins_pat)
                } else if nref.layer_is_zero && nref.lt_is_bylayer {
                    (ctx.l0.pat_len, ctx.l0.pat)
                } else {
                    (nref.ins_pat_len, nref.ins_pat)
                };
                let nested_lw_px = if nref.lw_is_byblock {
                    ctx.ins_lw_px
                } else if nref.layer_is_zero && nref.lw_is_bylayer {
                    ctx.l0.lw_px
                } else {
                    nref.ins_lw_px
                };
                // Layer-0 target for the nested expansion: a nested insert that
                // is itself on layer 0 chains up to the outer target; otherwise
                // its layer-0 children resolve to the nested insert's own layer.
                let nested_l0 = if nref.layer_is_zero { ctx.l0 } else { nref.l0 };
                let inner_ctx = ExpandCtx {
                    cache: ctx.cache,
                    ins_color: nested_color,
                    ins_pat_len: nested_pat_len,
                    ins_pat: nested_pat,
                    ins_lw_px: nested_lw_px,
                    l0: nested_l0,
                    selected: ctx.selected,
                    pslt_factor: ctx.pslt_factor,
                    view_aabb: ctx.view_aabb,
                    world_per_pixel: ctx.world_per_pixel,
                    is_xref: ctx.is_xref,
                    bg_color: ctx.bg_color,
                };
                visited.push(nref.block_name.clone());
                if let Some(cp) = &nref.clip_poly {
                    // XCLIP'd nested insert: expand into an isolated batch set,
                    // finalize to whole wires, clip them to the boundary mapped
                    // into world by the accumulated transform, then carry the
                    // clipped wires out via `extra_wires`. Single instance only —
                    // an array of clipped nested inserts is vanishingly rare, so
                    // the array case is not handled (boundary placement per
                    // instance would differ).
                    let composed = nref.xform.then(accum_xform);
                    let mut sub = Batches::default();
                    expand_defn(nested_defn, &composed, &inner_ctx, &mut sub, visited, depth + 1);
                    let mut wires = sub.finalize("", ctx.selected, ctx.bg_color);
                    let world_poly: Vec<[f64; 2]> = cp
                        .iter()
                        .map(|&[x, y]| {
                            let w = accum_xform.apply(Vector3::new(x, y, 0.0));
                            [w.x, w.y]
                        })
                        .collect();
                    crate::scene::pick::xclip::clip_wires(&mut wires, &world_poly);
                    out.extra_wires.append(&mut wires);
                } else {
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
                }
                visited.pop();
            }
        }
    }
}

/// Resolve a cached LocalWire's final colour against the current expansion
/// context: selection override first, then ByBlock → insert colour, then the
/// layer-0 rule → insert-layer colour, else the cached colour; finally xref
/// fade. Shared by the stroke, fill, and greeked-text emit paths.
fn resolve_wire_color(lw: &LocalWire, ctx: &ExpandCtx) -> [f32; 4] {
    let c = if ctx.selected {
        WireModel::SELECTED
    } else if lw.color_is_byblock {
        ctx.ins_color
    } else if lw.color_l0 {
        // Inherit the insert layer's RGB but keep the child's own transparency.
        [ctx.l0.color[0], ctx.l0.color[1], ctx.l0.color[2], lw.color[3]]
    } else {
        lw.color
    };
    if ctx.is_xref && !ctx.selected {
        fade_toward_bg(c, ctx.bg_color)
    } else {
        c
    }
}

fn emit_wire(
    lw: &LocalWire,
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
) {
    if lw.points.is_empty() && lw.fill_tris.is_empty() {
        return;
    }

    // Resolve final style for this LocalWire against the outer Insert ctx
    // before we hash it into a batch.
    let final_color = resolve_wire_color(lw, ctx);
    let (final_pat_len, final_pat) = if lw.lt_is_byblock {
        (ctx.ins_pat_len, ctx.ins_pat)
    } else if lw.lt_l0 {
        (ctx.l0.pat_len, ctx.l0.pat)
    } else {
        (lw.pattern_length, lw.pattern)
    };
    let final_lw_px = if lw.lw_is_byblock {
        ctx.ins_lw_px
    } else if lw.lw_l0 {
        ctx.l0.lw_px
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
        lw.is_fill_only,
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
            lw.is_fill_only,
        )
    });

    // NaN separator between previously-appended geometry and this wire so the
    // GPU shader treats them as disconnected polylines within one buffer.
    let needs_sep = !entry.points.is_empty()
        && !entry.points.last().map(|p| p[0].is_nan()).unwrap_or(false);

    if !lw.points.is_empty() {
        if needs_sep {
            entry.points.push([f32::NAN; 3]);
            entry.points_low.push([0.0; 3]);
        }
        // Iterate paired with the matching low residual so the GPU keeps
        // sub-f32 precision once the INSERT transform lands the wire in
        // world space at UTM-scale coordinates.
        for (idx, p) in lw.points.iter().enumerate() {
            if p[0].is_nan() {
                entry.points.push([f32::NAN; 3]);
                entry.points_low.push([0.0; 3]);
                continue;
            }
            let pl = lw.points_low.get(idx).copied().unwrap_or([0.0; 3]);
            // Reconstruct the f64 source from (high, low) before applying the
            // insert transform — otherwise the low half is silently dropped.
            let v = accum_xform.apply(Vector3::new(
                p[0] as f64 + pl[0] as f64,
                p[1] as f64 + pl[1] as f64,
                p[2] as f64 + pl[2] as f64,
            ));
            let qx = (v.x) as f32;
            let qy = (v.y) as f32;
            let qz = (v.z) as f32;
            let qx_l = ((v.x) - qx as f64) as f32;
            let qy_l = ((v.y) - qy as f64) as f32;
            let qz_l = ((v.z) - qz as f64) as f32;
            let q = [qx, qy, qz];
            if qx < entry.min_x {
                entry.min_x = qx;
            }
            if qy < entry.min_y {
                entry.min_y = qy;
            }
            if qx > entry.max_x {
                entry.max_x = qx;
            }
            if qy > entry.max_y {
                entry.max_y = qy;
            }
            entry.points.push(q);
            entry.points_low.push([qx_l, qy_l, qz_l]);
        }
    }

    for p in &lw.key_vertices {
        let v = accum_xform.apply(Vector3::new(
            p[0] as f64,
            p[1] as f64,
            p[2] as f64,
        ));
        entry.key_vertices.push([v.x, v.y, v.z]);
    }
    for (p, hint) in &lw.snap_pts {
        let v = accum_xform.apply(Vector3::new(
            p.x as f64,
            p.y as f64,
            p.z as f64,
        ));
        entry.snap_pts.push((
            glam::DVec3::new(v.x, v.y, v.z),
            *hint,
        ));
    }
    for tg in &lw.tangent_geoms {
        entry
            .tangent_geoms
            .push(transform_tangent(tg, accum_xform));
    }
    debug_assert_eq!(lw.fill_tris.len(), lw.fill_tris_low.len());
    for (idx, p) in lw.fill_tris.iter().enumerate() {
        // Empty/short fill_tris_low means "no low half" (all-zero), per the
        // WireModel contract — same panic-safe access the other fill consumers
        // use (face3d_gpu, xclip). A Leader with a filled arrowhead nested in a
        // block reaches here with populated fill_tris but empty fill_tris_low;
        // a raw `[idx]` would panic in release (bounds checks are not gated by
        // debug-assertions). The debug_assert above stays as a tripwire.
        let pl = lw.fill_tris_low.get(idx).copied().unwrap_or([0.0; 3]);
        let v = accum_xform.apply(Vector3::new(
            p[0] as f64 + pl[0] as f64,
            p[1] as f64 + pl[1] as f64,
            p[2] as f64 + pl[2] as f64,
        ));
        let (hx, lx) = WireModel::split_ds(v.x);
        let (hy, ly) = WireModel::split_ds(v.y);
        let (hz, lz) = WireModel::split_ds(v.z);
        entry.fill_tris.push([hx, hy, hz]);
        entry.fill_tris_low.push([lx, ly, lz]);
    }
}

fn transform_tangent(
    tg: &TangentGeom,
    t: &Transform,
) -> TangentGeom {
    match tg {
        TangentGeom::Line { p1, p2 } => {
            let q1 = t.apply(Vector3::new(
                p1[0] as f64,
                p1[1] as f64,
                p1[2] as f64,
            ));
            let q2 = t.apply(Vector3::new(
                p2[0] as f64,
                p2[1] as f64,
                p2[2] as f64,
            ));
            TangentGeom::Line {
                p1: [(q1.x) as f32, (q1.y) as f32, (q1.z) as f32],
                p2: [(q2.x) as f32, (q2.y) as f32, (q2.z) as f32],
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
                center: [(c.x) as f32, (c.y) as f32, (c.z) as f32],
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

