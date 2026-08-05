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

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use acadrust::types::{Color as AcadColor, LineWeight, Transform, Vector3};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::convert::tessellate;
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};

const MAX_NESTING_DEPTH: usize = 32;
/// Skip wires whose world-AABB projects to fewer than this many pixels in
/// the active view. Picks up tiny detail at zoom-out so the tessellator
/// doesn't waste time on geometry that contributes a few sub-pixel marks
/// to the final image. Two pixels is a practical small-element floor:
/// — visibly the same image, dramatically fewer wires.
const MIN_PIXEL_SIZE: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct LocalWire {
    pub points: Vec<[f32; 3]>,
    /// Low-bit residual paired with `points` so block-instance wires keep
    /// sub-f32 precision once the renderer translates them to world space.
    pub points_low: Vec<[f32; 3]>,
    /// SDF glyph quads for block-internal text, in block-local coordinates.
    /// Non-empty only when SDF text is on and this sub is a TEXT/MTEXT. The
    /// expand-time transform (`emit_wire`) maps each vertex to world exactly
    /// like `points`, so block-instance text lands at the right place/scale.
    pub text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
    pub key_vertices: Vec<[f64; 3]>,
    pub snap_pts: Vec<(glam::DVec3, SnapHint)>,
    pub tangent_geoms: Vec<TangentGeom>,
    pub fill_tris: Vec<[f32; 3]>,
    pub fill_tris_low: Vec<[f32; 3]>,
    /// Preserves the planar SOLID classification through block expansion so
    /// it never gets merged with unrelated annotation fills of the same style.
    pub fill_is_2d_solid: bool,
    /// Thickness-wall pick geometry, transformed by the insert like `points`
    /// so a block child's extruded wall stays selectable at the instance's
    /// place and scale. Never reaches the GPU.
    pub pick_tris: Vec<[f32; 3]>,
    pub pick_tris_low: Vec<[f32; 3]>,
    /// Per-wire colour from the tessellator output. For most entities this
    /// equals the sub-entity's resolved colour. For colour-split MTEXT
    /// (`\C`/`\c` inline overrides) each wire carries its own override colour.
    pub color: [f32; 4],
    pub aci: u8,
    pub pattern_length: f32,
    pub pattern: [f32; 8],
    pub line_weight_px: f32,
    /// World-space band width for a wide polyline (see `WireModel.world_width`).
    /// Block-local; the expand-time transform scales it by the insert so the
    /// shader band grows with a scaled insert. `0.0` = a normal wire.
    pub world_width: f32,
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
    pub transparency_l0: bool,
    pub lt_l0: bool,
    pub lw_l0: bool,
    /// XY bounding box of this wire in block-local coordinates.
    /// `[min_x, min_y, max_x, max_y]`. Used for view-frustum culling at
    /// expand-time: transform corners by the Insert transform → world AABB
    /// → test against the camera's world-space view rect.
    pub aabb_local: [f32; 4],
    /// This child's signed draw-order rank in (-1,1) within its owning block
    /// (from the scene draw-depth map, so SortEntitiesTable overrides apply).
    /// Composed at expand time into a `depth_override` for wide-polyline
    /// bands so a band orders against its block siblings.
    pub local_rank: f32,
}

#[derive(Clone, Debug)]
pub struct NestedRef {
    pub block_name: String,
    pub xform: Transform,
    pub style: crate::scene::render_graph::InsertStyleSpec,
    pub instance_offsets: Vec<[f64; 3]>,
    /// XCLIP boundary for this nested insert, in the parent defn's local frame
    /// (`None` = unclipped). Baked at build time because the clip's spatial
    /// filter lives in `doc.objects`, which isn't reachable at expand time; on
    /// expansion it is mapped to world by the accumulated transform and the
    /// nested wires are clipped to it.
    pub clip_poly: Option<Vec<[f64; 2]>>,
    /// The nested INSERT's own signed draw-order rank in (-1,1) within the
    /// parent block — narrows the depth sub-range its children may occupy.
    pub local_rank: f32,
}

#[derive(Clone, Debug)]
pub enum LocalSub {
    Wire(LocalWire),
    Nested(NestedRef),
}

#[derive(Clone, Debug, Default)]
pub struct BlockDefn {
    pub subs: Vec<LocalSub>,
    pub base_point: Vector3,
    /// Union of every sub's local AABB (including nested-INSERT contributions
    /// resolved at expand time via their own defn's `aabb_local`). XY only —
    /// the wire renderer is 2D-dominant. Expressed in this defn's *offset*
    /// Absolute world-space XY (the double-single render path keeps it precise).
    pub aabb_local: [f32; 4],
    /// Raw entity count of the source block record (`entity_handles.len()`).
    /// Divisor for nested depth composition: a nested insert's children get a
    /// sub-range of `parent_scale / (child_count + 1)`, shared with the scene
    /// graph so bands and fills stay on one depth scale.
    pub child_count: usize,
}

#[derive(Default, Debug)]
pub struct BlockCache {
    defns: HashMap<String, Arc<BlockDefn>>,
    prototype_blocks: HashSet<String>,
    /// Fully expanded prototype for repeated, non-array inserts. The key omits
    /// translation but includes the linear transform and every inherited style
    /// input. A matching insert therefore reuses all nested expansion/style
    /// work and only applies its translation to the immutable prototype.
    expansion_prototypes: std::sync::Mutex<
        HashMap<ExpansionPrototypeKey, Arc<std::sync::Mutex<Option<Arc<CachedExpansion>>>>>,
    >,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ExpansionPrototypeKey {
    block_name: String,
    linear: [u64; 9],
    insert_style: Vec<u32>,
    selected: bool,
    is_xref: bool,
}

#[derive(Debug)]
struct CachedExpansion {
    translation: [f64; 3],
    wires: Arc<Vec<WireModel>>,
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
    pub fn build(
        doc: &CadDocument,
        anno_scale: f32,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        bg_color: [f32; 4],
        viewport: Option<Handle>,
        // Scene draw-depth map ([depth, half] per handle) — source of each
        // block child's in-block rank, shared by cached wires and graph leaves.
        depth_map: &HashMap<u64, [f32; 2]>,
    ) -> Self {
        use crate::par::prelude::*;
        let mut cache = Self::new();
        let referenced = collect_referenced_blocks(doc);
        let mut reference_counts: HashMap<String, usize> = HashMap::default();
        for entity in doc.entities() {
            if let EntityType::Insert(insert) = entity {
                *reference_counts
                    .entry(insert.block_name.clone())
                    .or_default() += insert.instance_count();
            }
        }
        cache.prototype_blocks = reference_counts
            .into_iter()
            .filter_map(|(name, count)| (count > 1).then_some(name))
            .collect();
        // Each defn is built independently: nested INSERTs are stored as
        // by-name references (`LocalSub::Nested`), never expanded here, so a
        // block's build never depends on another block's defn. That makes the
        // builds embarrassingly parallel over the read-only `doc` — no
        // topological ordering required. `compute_block_aabbs` stays a serial
        // post-pass (it resolves nested references and is comparatively cheap).
        cache.defns = referenced
            .par_iter()
            .map(|name| {
                (
                    name.clone(),
                    Arc::new(build_defn(
                        doc,
                        name,
                        anno_scale,
                        annotation_scale_handle,
                        all_visible,
                        bg_color,
                        viewport,
                        depth_map,
                    )),
                )
            })
            .collect();
        cache.compute_block_aabbs(&referenced);
        cache
    }

    /// Build a small cache rooted at one block. Used only when a synthetic or
    /// newly-added Insert is not present in the resident cache yet.
    pub fn build_for_block(
        doc: &CadDocument,
        block_name: &str,
        anno_scale: f32,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        bg_color: [f32; 4],
        viewport: Option<Handle>,
        depth_map: &HashMap<u64, [f32; 2]>,
    ) -> Self {
        use crate::par::prelude::*;
        let mut cache = Self::new();
        let referenced = referenced_block_tree(doc, block_name);
        cache.defns = referenced
            .par_iter()
            .map(|name| {
                (
                    name.clone(),
                    Arc::new(build_defn(
                        doc,
                        name,
                        anno_scale,
                        annotation_scale_handle,
                        all_visible,
                        bg_color,
                        viewport,
                        depth_map,
                    )),
                )
            })
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

fn referenced_block_tree(doc: &CadDocument, root: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::default();
    let mut queue = vec![root.to_string()];
    seen.insert(root.to_string());
    while let Some(name) = queue.pop() {
        let Some(record) = doc.block_records.get(&name) else {
            continue;
        };
        for &handle in &record.entity_handles {
            let Some(EntityType::Insert(insert)) = doc.get_entity(handle) else {
                continue;
            };
            if seen.insert(insert.block_name.clone()) {
                queue.push(insert.block_name.clone());
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
    annotation_scale_handle: Option<Handle>,
    all_visible: bool,
    bg_color: [f32; 4],
    viewport: Option<Handle>,
    depth_map: &HashMap<u64, [f32; 2]>,
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
        let Some(source_entity) = doc.get_entity(eh) else {
            continue;
        };
        let contextual = crate::scene::annotative::entity_for_annotation_context(
            doc,
            source_entity,
            annotation_scale_handle,
        );
        let entity = contextual.as_ref();
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
        // Annotative scale representation: bake only the current scale's copy
        // into the defn so off-scale representations don't stack (e.g. a 1×
        // copy under a 10×). See `annotative::annotative_offscale`.
        if crate::scene::annotative::annotative_offscale_for(
            doc,
            entity.common(),
            annotation_scale_handle,
            all_visible,
        ) {
            continue;
        }
        match entity {
            EntityType::Block(_) | EntityType::BlockEnd(_) => continue,
            // A non-constant ATTDEF is only a template — the insert supplies an
            // ATTRIB with the real value (tessellated separately). A CONSTANT
            // attribute has no ATTRIB; its value lives in the block itself, so
            // it must render as part of the block content (unless flagged
            // invisible). With an empty value there is nothing to draw: the
            // ATTDEF tessellator would fall back to its tag-placeholder
            // preview, which belongs to a standalone definition, not to
            // placed block content.
            EntityType::AttributeDefinition(ad)
                if !ad.flags.constant || ad.flags.invisible || ad.default_value.is_empty() =>
            {
                continue
            }
            EntityType::Insert(nested_ins) => {
                subs.push(LocalSub::Nested(build_nested_ref(
                    nested_ins, doc, bg_color, viewport, depth_map,
                )));
            }
            EntityType::Dimension(_) => {
                for wire in tessellate_sub_local(
                    doc,
                    entity,
                    anno_scale,
                    annotation_scale_handle,
                    bg_color,
                    viewport,
                    depth_map,
                ) {
                    subs.push(LocalSub::Wire(wire));
                }
            }
            // A nested table's stored graphics use the same scene-graph
            // traversal as top-level table content. The resulting leaf wires
            // remain local to this cached definition; the parent instance
            // transform is composed later by the cache expansion.
            EntityType::Table(table) => {
                let baked = table.block_record_handle.and_then(|handle| {
                    doc.block_records
                        .iter()
                        .find(|record| record.handle == handle)
                });
                if let Some(record) = baked.filter(|record| {
                    !record.entity_handles.is_empty()
                }) {
                    let mut insert = acadrust::entities::Insert::new(
                        record.name.clone(),
                        table.insertion_point,
                    );
                    insert.rotation = table
                        .horizontal_direction
                        .y
                        .atan2(table.horizontal_direction.x);
                    insert.common = table.common.clone();
                    let graph = crate::scene::render_graph::RenderSceneGraph::new(
                        doc,
                        None,
                        annotation_scale_handle,
                        all_visible,
                        depth_map,
                    )
                    .with_viewport(viewport);
                    graph.walk_insert(
                        &insert,
                        table.common.handle,
                        |_, _| true,
                        |leaf, context| {
                            let mut placed = leaf.clone();
                            placed.apply_transform(&context.transform);
                            for wire in tessellate_sub_local(
                                doc,
                                &placed,
                                anno_scale,
                                annotation_scale_handle,
                                bg_color,
                                viewport,
                                depth_map,
                            ) {
                                subs.push(LocalSub::Wire(wire));
                            }
                        },
                    );
                } else {
                    for wire in tessellate_sub_local(
                        doc,
                        entity,
                        anno_scale,
                        annotation_scale_handle,
                        bg_color,
                        viewport,
                        depth_map,
                    ) {
                        subs.push(LocalSub::Wire(wire));
                    }
                    for insert in crate::entities::table::block_cell_inserts(
                        table,
                        doc,
                        anno_scale,
                    ) {
                        subs.push(LocalSub::Nested(build_nested_ref(
                            &insert,
                            doc,
                            bg_color,
                            viewport,
                            depth_map,
                        )));
                    }
                }
            }
            _ => {
                // A wide polyline inside a block carries its `world_width` on
                // the LocalWire; `emit_wire` scales it by the insert transform
                // so the shader band matches the scaled geometry (same band the
                // top-level path draws — depth-tested + linetype-dashed).
                for lw in tessellate_sub_local(
                    doc,
                    entity,
                    anno_scale,
                    annotation_scale_handle,
                    bg_color,
                    viewport,
                    depth_map,
                ) {
                    subs.push(LocalSub::Wire(lw));
                }
            }
        }
    }
    BlockDefn {
        subs,
        base_point: crate::scene::render_graph::block_base_point(doc, block_name),
        aabb_local: [0.0; 4],
        child_count: br.entity_handles.len(),
    }
}

fn build_nested_ref(
    nested_ins: &acadrust::entities::Insert,
    doc: &CadDocument,
    bg_color: [f32; 4],
    viewport: Option<Handle>,
    depth_map: &HashMap<u64, [f32; 2]>,
) -> NestedRef {
    let _ = bg_color;

    // Bake the XCLIP boundary (parent-defn-local) so the nested insert keeps
    // its clip when the parent block is expanded — the spatial filter object
    // isn't reachable at expand time.
    let xform = crate::scene::render_graph::insert_transform(doc, nested_ins);
    let clip_poly = crate::scene::pick::xclip::insert_spatial_filter(doc, nested_ins)
        .map(|filter| {
            crate::scene::pick::xclip::world_clip_polygon_for_transform(filter, &xform)
        });

    NestedRef {
        block_name: nested_ins.block_name.clone(),
        xform,
        style: crate::scene::render_graph::InsertStyleSpec::new(doc, nested_ins, viewport),
        instance_offsets: crate::scene::render_graph::array_offsets(nested_ins),
        clip_poly,
        local_rank: depth_map
            .get(&nested_ins.common.handle.value())
            .map_or(0.0, |d| d[0]),
    }
}

fn tessellate_sub_local(
    doc: &CadDocument,
    sub: &EntityType,
    anno_scale: f32,
    annotation_scale_handle: Option<Handle>,
    bg_color: [f32; 4],
    viewport: Option<Handle>,
    depth_map: &HashMap<u64, [f32; 2]>,
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
    let (sub_color, pat_len, pat, lw_px, aci) =
        crate::scene::view::render::render_style_for_viewport(doc, sub, viewport);
    let _ = bg_color;

    let has_book_color =
        crate::scene::view::render::has_resolved_book_color(doc, sub);
    let color_is_byblock =
        !has_book_color && sub.common().color == AcadColor::ByBlock;
    let lt_is_byblock = sub.common().linetype.eq_ignore_ascii_case("byblock");
    let lw_is_byblock = matches!(sub.common().line_weight, LineWeight::ByBlock);

    // Layer-0 rule: a child on layer "0" with ByLayer properties inherits the
    // INSERT's layer at expand time. Flag each ByLayer property so emit_wire
    // can override the cached (layer-0-resolved) value with the insert layer's.
    let on_l0 = crate::scene::view::render::is_effective_layer_zero(&sub.common().layer);
    let color_l0 =
        !has_book_color && on_l0 && sub.common().color == AcadColor::ByLayer;
    let transparency_l0 = on_l0 && sub.common().transparency.alpha() == 0;
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
    let wires_out = if let EntityType::Dimension(dimension) = sub {
        use crate::entities::dimension::DimensionTess;
        dimension.tessellate(
            doc,
            h,
            false,
            sub_color,
            lw_px,
            anno_scale,
            &HashSet::default(),
            None,
            bg_color,
            None,
            None,
        )
    } else {
        tessellate::tessellate(
            doc,
            h,
            sub,
            false,
            sub_color,
            pat_len,
            pat,
            lw_px,
            anno_scale,
            annotation_scale_handle,
            None,
            bg_color,
            false,
        )
    };
    if wires_out.is_empty() {
        return vec![];
    }

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
        // SDF text wires have no points/fills — fold in the glyph-quad positions
        // so the view-frustum cull uses the text's real box, not a degenerate
        // point at the block origin (which would drop the text entirely).
        // `pick_tris` is in here for the same reason `fill_tris` is: hit-testing
        // rejects on this box before it looks at the triangles, so a box drawn
        // only around `points` would make a block child's thickness wall or wide
        // polyline band unpickable — `points` merely bounds those.
        let aabb_local = aabb_from_points_iter(
            wire.points
                .iter()
                .copied()
                .chain(wire.fill_tris.iter().copied())
                .chain(wire.pick_tris.iter().copied())
                .chain(wire.text_verts.iter().map(|v| v.pos)),
        );
        let is_fill_only = wire.points.is_empty() && !wire.fill_tris.is_empty();
        // A wire whose colour differs from the entity's resolved base colour
        // carries an explicit per-segment override (e.g. an MTEXT `\C1;` inline
        // colour). ByBlock / layer-0 inheritance applies only to wires still on
        // the base colour — folding an explicit segment into the inherited
        // colour would collapse colour-split geometry to one colour. (PR #301,
        // Kevin Griffin — extended to SDF text per-vertex in emit_wire.)
        let wire_on_base_color = wire.color == sub_color;

        result.push(LocalWire {
            points: wire.points,
            points_low: wire.points_low,
            text_verts: wire.text_verts,
            key_vertices: wire.key_vertices,
            snap_pts: wire.snap_pts,
            tangent_geoms: wire.tangent_geoms,
            fill_tris: wire.fill_tris,
            fill_tris_low: wire.fill_tris_low,
            fill_is_2d_solid: wire.fill_is_2d_solid,
            pick_tris: wire.pick_tris,
            pick_tris_low: wire.pick_tris_low,
            color: wire.color,
            aci,
            pattern_length: pat_len,
            pattern: pat,
            line_weight_px: lw_px,
            world_width: wire.world_width,
            plinegen: wire.plinegen,
            is_fill_only,
            color_is_byblock: color_is_byblock && wire_on_base_color,
            lt_is_byblock,
            lw_is_byblock,
            color_l0: color_l0 && wire_on_base_color,
            transparency_l0: transparency_l0 && wire_on_base_color,
            lt_l0,
            lw_l0,
            aabb_local,
            local_rank: depth_map.get(&h.value()).map_or(0.0, |d| d[0]),
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
    // Current annotation scale. An annotative block scales as one uniform unit
    // about its insertion point; a non-annotative block is unaffected.
    anno_scale: f32,
) -> Option<Vec<WireModel>> {
    let defn = cache.defn(&ins.block_name)?;
    let base = defn.base_point;
    let mut xform = Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z))
        .then(&ins.get_transform());
    // Annotative blocks (the flag lives on the block definition; the instance is
    // marked with the AcAnnotativeData XDATA) scale as ONE uniform unit about
    // their insertion point — internal geometry/text/attributes are carried by
    // this transform, never scaled individually (which would double-scale).
    if (anno_scale - 1.0).abs() > 1e-6
        && ins
            .common
            .extended_data
            .get_record("AcAnnotativeData")
            .is_some()
    {
        let p = ins.insert_point;
        let scale_about_p = Transform::from_translation(Vector3::new(-p.x, -p.y, -p.z))
            .then(&Transform::from_scale(anno_scale as f64))
            .then(&Transform::from_translation(Vector3::new(p.x, p.y, p.z)));
        xform = xform.then(&scale_about_p);
    }
    let name = ins_handle.value().to_string();
    let prototype_key = if view_aabb.is_none()
        && world_per_pixel.is_none()
        && !ins.is_array()
        && cache.prototype_blocks.contains(&ins.block_name)
    {
        Some(expansion_prototype_key(
            ins,
            &xform,
            ins_resolved_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            ins_layer,
            selected,
            pslt_factor,
            is_xref,
            bg_color,
            anno_scale,
        ))
    } else {
        None
    };
    let prototype_slot = prototype_key.as_ref().map(|key| {
        let mut prototypes = cache
            .expansion_prototypes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            prototypes
                .entry(key.clone())
                .or_insert_with(|| Arc::new(std::sync::Mutex::new(None))),
        )
    });
    let mut prototype_guard = prototype_slot
        .as_ref()
        .map(|slot| slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    if let Some(cached) = prototype_guard
        .as_ref()
        .and_then(|guard| guard.as_ref())
        .cloned()
    {
        let translation = transform_translation(&xform);
        let delta = [
            translation[0] - cached.translation[0],
            translation[1] - cached.translation[1],
            translation[2] - cached.translation[2],
        ];
        return Some(
            cached
                .wires
                .iter()
                .map(|wire| translated_prototype_wire(wire, &name, delta))
                .collect(),
        );
    }
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

    for offset in &crate::scene::render_graph::array_offsets(ins) {
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
        expand_defn(defn, &base_xform, &ctx, &mut batches, &mut visited, 0, (0.0, 1.0));
    }
    let wires = batches.finalize(&name, selected, bg_color);
    if let Some(guard) = prototype_guard.as_mut() {
        let cached = Arc::new(CachedExpansion {
            translation: transform_translation(&xform),
            wires: Arc::new(wires.clone()),
        });
        **guard = Some(cached);
    }
    Some(wires)
}

fn transform_translation(transform: &Transform) -> [f64; 3] {
    [
        transform.matrix.m[0][3],
        transform.matrix.m[1][3],
        transform.matrix.m[2][3],
    ]
}

#[allow(clippy::too_many_arguments)]
fn expansion_prototype_key(
    ins: &acadrust::entities::Insert,
    transform: &Transform,
    ins_color: [f32; 4],
    ins_pat_len: f32,
    ins_pat: [f32; 8],
    ins_lw_px: f32,
    ins_layer: crate::scene::view::render::InheritStyle,
    selected: bool,
    pslt_factor: f32,
    is_xref: bool,
    bg_color: [f32; 4],
    anno_scale: f32,
) -> ExpansionPrototypeKey {
    let matrix = &transform.matrix.m;
    let linear = [
        matrix[0][0].to_bits(),
        matrix[0][1].to_bits(),
        matrix[0][2].to_bits(),
        matrix[1][0].to_bits(),
        matrix[1][1].to_bits(),
        matrix[1][2].to_bits(),
        matrix[2][0].to_bits(),
        matrix[2][1].to_bits(),
        matrix[2][2].to_bits(),
    ];
    let mut insert_style = Vec::with_capacity(32);
    insert_style.extend(ins_color.map(f32::to_bits));
    insert_style.push(ins_pat_len.to_bits());
    insert_style.extend(ins_pat.map(f32::to_bits));
    insert_style.push(ins_lw_px.to_bits());
    insert_style.extend(ins_layer.color.map(f32::to_bits));
    insert_style.push(ins_layer.pat_len.to_bits());
    insert_style.extend(ins_layer.pat.map(f32::to_bits));
    insert_style.push(ins_layer.lw_px.to_bits());
    insert_style.push(pslt_factor.to_bits());
    insert_style.extend(bg_color.map(f32::to_bits));
    insert_style.push(anno_scale.to_bits());
    ExpansionPrototypeKey {
        block_name: ins.block_name.clone(),
        linear,
        insert_style,
        selected,
        is_xref,
    }
}

fn translated_prototype_wire(source: &WireModel, name: &str, delta: [f64; 3]) -> WireModel {
    let mut wire = source.clone();
    wire.name = name.to_string();
    translate_double_single(&mut wire.points, &mut wire.points_low, delta);
    translate_double_single(&mut wire.fill_tris, &mut wire.fill_tris_low, delta);
    translate_double_single(&mut wire.pick_tris, &mut wire.pick_tris_low, delta);
    for (point, _) in &mut wire.snap_pts {
        point.x += delta[0];
        point.y += delta[1];
        point.z += delta[2];
    }
    for point in &mut wire.key_vertices {
        point[0] += delta[0];
        point[1] += delta[1];
        point[2] += delta[2];
    }
    let delta_f32 = [delta[0] as f32, delta[1] as f32, delta[2] as f32];
    for tangent in &mut wire.tangent_geoms {
        match tangent {
            TangentGeom::Line { p1, p2 } => {
                for axis in 0..3 {
                    p1[axis] += delta_f32[axis];
                    p2[axis] += delta_f32[axis];
                }
            }
            TangentGeom::Circle { center, .. } => {
                for axis in 0..3 {
                    center[axis] += delta_f32[axis];
                }
            }
        }
    }
    if !wire.text_verts.is_empty() {
        wire.text_verts =
            crate::scene::model::wire_model::map_text_verts(&wire.text_verts, |x, y, z| {
                (x + delta[0], y + delta[1], z + delta[2])
            });
    }
    if wire.aabb != WireModel::UNBOUNDED_AABB {
        wire.aabb[0] += delta_f32[0];
        wire.aabb[1] += delta_f32[1];
        wire.aabb[2] += delta_f32[0];
        wire.aabb[3] += delta_f32[1];
    }
    wire
}

fn translate_double_single(points: &mut [[f32; 3]], lows: &mut Vec<[f32; 3]>, delta: [f64; 3]) {
    if points.is_empty() {
        return;
    }
    if lows.len() != points.len() {
        lows.resize(points.len(), [0.0; 3]);
    }
    for (point, low) in points.iter_mut().zip(lows.iter_mut()) {
        for axis in 0..3 {
            let value = point[axis] as f64 + low[axis] as f64 + delta[axis];
            let high = value as f32;
            point[axis] = high;
            low[axis] = (value - high as f64) as f32;
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
    /// Wide-polyline band width (bit-cast). Keeps bands of different widths — and
    /// bands vs thin wires of the same colour/style — in separate batches so the
    /// finalized WireModel carries one correct `world_width`.
    world_width: u32,
    aci: u8,
    plinegen: bool,
    /// Marks batches that emit only `fill_tris` with no wire `points`. The
    /// face3d pipeline uses `wire.points.is_empty()` as the "skip dim"
    /// discriminator, so greek fills must stay in their own batches even
    /// when their color/style would otherwise collide with regular wires.
    is_fill_only: bool,
    /// Part of the batch key so planar SOLID fills remain independently
    /// switchable after block geometry is merged by style.
    fill_is_2d_solid: bool,
    /// Bit-cast composed block-local depth for band wires (`0` = no override).
    /// Keeps bands of different in-block draw ranks in separate batches so
    /// each finalized WireModel carries one correct `depth_override`.
    depth_bits: u32,
}

#[derive(Default, Debug)]
struct BatchEntry {
    color: [f32; 4],
    pattern_length: f32,
    pattern: [f32; 8],
    line_weight_px: f32,
    world_width: f32,
    aci: u8,
    plinegen: bool,
    /// Composed block-local draw-order offset for a band batch (see
    /// `WireModel::depth_override`). `None` for every other batch.
    local_depth: Option<f32>,
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
    fill_is_2d_solid: bool,
    /// Accumulated thickness-wall pick geometry, paired high/low like
    /// `fill_tris`. Pick-only — no GPU batch reads this.
    pick_tris: Vec<[f32; 3]>,
    pick_tris_low: Vec<[f32; 3]>,
    /// Accumulated SDF glyph quads (world space) for block-instance text.
    text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
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
        world_width: f32,
        aci: u8,
        plinegen: bool,
        _is_fill_only: bool,
        fill_is_2d_solid: bool,
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
            world_width,
            aci,
            plinegen,
            fill_is_2d_solid,
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
                    taper_widths: Vec::new(),
                    world_width: b.world_width,
                    depth_override: b.local_depth,
                    fill_is_3d: false,
                    fill_is_2d_solid: b.fill_is_2d_solid,
                    pick_tris: b.pick_tris,
                    pick_tris_low: b.pick_tris_low,
                    dash_from_start: false,
                    dash_align_end: None,
                    text_verts: b.text_verts,
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
    world_width: f32,
    aci: u8,
    plinegen: bool,
    is_fill_only: bool,
    fill_is_2d_solid: bool,
    local_depth: Option<f32>,
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
        world_width: world_width.to_bits(),
        aci,
        plinegen,
        is_fill_only,
        fill_is_2d_solid,
        depth_bits: local_depth.map_or(0, f32::to_bits),
    }
}

fn expand_defn(
    defn: &BlockDefn,
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
    visited: &mut Vec<String>,
    depth: usize,
    // Block-local depth sub-range `(base, scale)` accumulated through nested
    // inserts: a child at rank r lands at `base + r * scale`, all within
    // (-1,1) of the top-level insert. Seeded `(0.0, 1.0)` by `expand_insert`.
    d_range: (f32, f32),
) {
    if depth > MAX_NESTING_DEPTH {
        eprintln!("block_cache: nested-block depth > {MAX_NESTING_DEPTH}, truncating");
        return;
    }
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
                    // SDF text wires carry glyph quads but no points; they
                    // render at every zoom (no text LOD), so exempt them from
                    // the sub-pixel cull that drops tiny stroke / fill geometry.
                    let is_text = !lw.text_verts.is_empty();
                    if !is_text && aabb_pixel_size(local, wpp) < MIN_PIXEL_SIZE {
                        continue;
                    }
                }
                emit_wire(lw, accum_xform, ctx, out, d_range);
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
                let parent_style = crate::scene::render_graph::BlockStyle {
                    insert: (
                        ctx.ins_color,
                        ctx.ins_pat_len,
                        ctx.ins_pat,
                        ctx.ins_lw_px,
                        0,
                    ),
                    layer0: ctx.l0,
                    layer0_aci: 0,
                };
                let nested_style = nref.style.resolve(parent_style);
                let inner_ctx = ExpandCtx {
                    cache: ctx.cache,
                    ins_color: nested_style.insert.0,
                    ins_pat_len: nested_style.insert.1,
                    ins_pat: nested_style.insert.2,
                    ins_lw_px: nested_style.insert.3,
                    l0: nested_style.layer0,
                    selected: ctx.selected,
                    pslt_factor: ctx.pslt_factor,
                    view_aabb: ctx.view_aabb,
                    world_per_pixel: ctx.world_per_pixel,
                    is_xref: ctx.is_xref,
                    bg_color: ctx.bg_color,
                };
                visited.push(nref.block_name.clone());
                // Children of this nested insert stack inside the slot its own
                // rank owns — same composition the scene graph applies.
                let nested_range = (
                    d_range.0 + nref.local_rank * d_range.1,
                    d_range.1 / (nested_defn.child_count.max(1) as f32 + 1.0),
                );
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
                    expand_defn(
                        nested_defn,
                        &composed,
                        &inner_ctx,
                        &mut sub,
                        visited,
                        depth + 1,
                        nested_range,
                    );
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
                            nested_range,
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
        let alpha = if lw.transparency_l0 {
            ctx.l0.color[3]
        } else {
            lw.color[3]
        };
        [ctx.l0.color[0], ctx.l0.color[1], ctx.l0.color[2], alpha]
    } else {
        lw.color
    };
    if ctx.is_xref && !ctx.selected {
        fade_toward_bg(c, ctx.bg_color)
    } else {
        c
    }
}

/// Effective linetype scale along this wire after an INSERT transform.
///
/// A non-uniform INSERT has no single global scale. Weight each segment by its
/// local length, producing the exact factor for a line and a stable
/// path-weighted approximation for a polyline or tessellated curve.
fn transformed_wire_length_scale(lw: &LocalWire, xform: &Transform) -> f32 {
    let mut local_length = 0.0_f64;
    let mut transformed_length = 0.0_f64;
    let mut previous: Option<Vector3> = None;

    for (index, point) in lw.points.iter().enumerate() {
        if !point.iter().all(|v| v.is_finite()) {
            previous = None;
            continue;
        }
        let low = lw.points_low.get(index).copied().unwrap_or([0.0; 3]);
        let current = Vector3::new(
            point[0] as f64 + low[0] as f64,
            point[1] as f64 + low[1] as f64,
            point[2] as f64 + low[2] as f64,
        );
        if let Some(prev) = previous {
            let delta = current - prev;
            let segment_length = (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt();
            if segment_length > 1e-12 {
                let transformed = xform.matrix.transform_direction(delta);
                local_length += segment_length;
                transformed_length += (transformed.x * transformed.x
                    + transformed.y * transformed.y
                    + transformed.z * transformed.z)
                    .sqrt();
            }
        }
        previous = Some(current);
    }

    if local_length > 1e-12 && transformed_length.is_finite() {
        (transformed_length / local_length) as f32
    } else {
        1.0
    }
}

fn emit_wire(
    lw: &LocalWire,
    accum_xform: &Transform,
    ctx: &ExpandCtx,
    out: &mut Batches,
    d_range: (f32, f32),
) {
    if lw.points.is_empty()
        && lw.fill_tris.is_empty()
        && lw.text_verts.is_empty()
        && lw.pick_tris.is_empty()
    {
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

    // Pattern distances are stored in block-local units. Scale them by the
    // wire's actual path-length ratio so uniform and non-uniform INSERTs both
    // stay dimensionally consistent with their transformed geometry.
    let pattern_scale = if final_pat_len > 0.0 {
        transformed_wire_length_scale(lw, accum_xform)
    } else {
        1.0
    };
    let final_pat_len = final_pat_len * ctx.pslt_factor * pattern_scale;
    let final_pat = final_pat.map(|v| v * ctx.pslt_factor * pattern_scale);

    // A wide polyline's band width is baked in block-local units; scale it by
    // the insert transform so the shader band matches the scaled geometry.
    // Average the X and Y axis image lengths — exact for a uniform insert, a
    // sensible mean for a non-uniform one (the band carries one width).
    let final_world_width = if lw.world_width > 0.0 {
        let o = accum_xform.apply(Vector3::new(0.0, 0.0, 0.0));
        let ax = accum_xform.apply(Vector3::new(1.0, 0.0, 0.0));
        let ay = accum_xform.apply(Vector3::new(0.0, 1.0, 0.0));
        let sx = ((ax.x - o.x).powi(2) + (ax.y - o.y).powi(2) + (ax.z - o.z).powi(2)).sqrt();
        let sy = ((ay.x - o.x).powi(2) + (ay.y - o.y).powi(2) + (ay.z - o.z).powi(2)).sqrt();
        lw.world_width * ((sx + sy) * 0.5) as f32
    } else {
        0.0
    };

    // Only band wires take a per-child composed depth: their solid area is
    // what covers siblings, and their width already splits them into their own
    // batches — thin wires keep the shared whole-insert depth so same-style
    // batches stay merged.
    let local_depth = (lw.world_width > 0.0)
        .then(|| d_range.0 + lw.local_rank * d_range.1);

    let key = style_key(
        final_color,
        final_pat_len,
        final_pat,
        final_lw_px,
        final_world_width,
        lw.aci,
        lw.plinegen,
        lw.is_fill_only,
        lw.fill_is_2d_solid,
        local_depth,
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
            final_world_width,
            lw.aci,
            lw.plinegen,
            lw.is_fill_only,
            lw.fill_is_2d_solid,
        )
    });
    entry.local_depth = local_depth;

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
    // Per the WireModel contract an empty `fill_tris_low` means "all-zero low
    // half" (e.g. a Leader / dimension arrowhead fill, which the tessellator
    // emits without a low half). Only a *partially* populated low half is a
    // real bug — keep the tripwire for that, but permit the empty case so debug
    // builds don't panic on legitimate geometry.
    debug_assert!(
        lw.fill_tris_low.is_empty() || lw.fill_tris.len() == lw.fill_tris_low.len(),
        "fill_tris_low must be empty or the same length as fill_tris (got {} vs {})",
        lw.fill_tris.len(),
        lw.fill_tris_low.len(),
    );
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
    // Thickness walls: same reconstruct → transform → re-split as the fills
    // above, so a block child's wall tracks the insert's placement and scale.
    debug_assert!(
        lw.pick_tris_low.is_empty() || lw.pick_tris.len() == lw.pick_tris_low.len(),
        "pick_tris_low must be empty or the same length as pick_tris (got {} vs {})",
        lw.pick_tris.len(),
        lw.pick_tris_low.len(),
    );
    for (idx, p) in lw.pick_tris.iter().enumerate() {
        let pl = lw.pick_tris_low.get(idx).copied().unwrap_or([0.0; 3]);
        let v = accum_xform.apply(Vector3::new(
            p[0] as f64 + pl[0] as f64,
            p[1] as f64 + pl[1] as f64,
            p[2] as f64 + pl[2] as f64,
        ));
        let (hx, lx) = WireModel::split_ds(v.x);
        let (hy, ly) = WireModel::split_ds(v.y);
        let (hz, lz) = WireModel::split_ds(v.z);
        entry.pick_tris.push([hx, hy, hz]);
        entry.pick_tris_low.push([lx, ly, lz]);
    }
    // SDF glyph quads: reconstruct each block-local f64 position, apply the
    // insert transform, re-split — same path as points/fills so block-instance
    // text lands at the right world place and scale. Colour resolves to the
    // batch's final colour (ByBlock / layer-0 block text follows the insert).
    for tv in &lw.text_verts {
        let wx = tv.pos[0] as f64 + tv.pos_low[0] as f64;
        let wy = tv.pos[1] as f64 + tv.pos_low[1] as f64;
        let wz = tv.pos[2] as f64 + tv.pos_low[2] as f64;
        let v = accum_xform.apply(Vector3::new(wx, wy, wz));
        let (hx, lx) = WireModel::split_ds(v.x);
        let (hy, ly) = WireModel::split_ds(v.y);
        let (hz, lz) = WireModel::split_ds(v.z);
        // Grow the batch AABB by the glyph extent so a text-only block wire
        // (no points) still finalizes to a bounded pick box.
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
        // Base glyphs inherit the resolved (ByBlock / layer-0) colour; a glyph
        // carrying an inline `\C` / `\c` override — colour differs from the
        // wire's base — keeps it, so block-nested colour-split MTEXT stays
        // multi-colour. Per-vertex analogue of PR #301's wire-level gate.
        let rgb = if [tv.color[0], tv.color[1], tv.color[2]]
            == [lw.color[0], lw.color[1], lw.color[2]]
        {
            [final_color[0], final_color[1], final_color[2]]
        } else {
            [tv.color[0], tv.color[1], tv.color[2]]
        };
        entry.text_verts.push(crate::scene::pipeline::text_gpu::TextVertex {
            pos: [hx, hy, hz],
            pos_low: [lx, ly, lz],
            uv: tv.uv,
            color: [rgb[0], rgb[1], rgb[2], tv.color[3]],
            draw_depth: tv.draw_depth,
        });
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
