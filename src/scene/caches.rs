use super::*;

/// Interior-mutable rendering, picking, layout, and tessellation caches
/// owned by [`Scene`].
pub(in crate::scene) struct SceneCaches {
    /// Cache of the gathered SDF text vertex list for one viewport, keyed on
    /// the wire-buffer content id. The glyph quads ride on each entity's wire
    /// (built by the tessellator); this caches the per-viewport gather so it is
    /// not re-walked every frame while the wire set is unchanged. Keyed on the
    /// wire content id so it re-gathers exactly when the wires (geometry or
    /// selection) change. See [`Scene::gather_text_verts`].
    /// Small map (not a single slot): a paper frame renders several wire
    /// sources (sheet + content viewports), each with its own stable content
    /// id — one slot would thrash between them every frame.
    pub(in crate::scene) sdf_text_cache:
        RefCell<HashMap<u64, std::sync::Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>>>,
    /// Extra scale representations for highlighted annotative entities. These
    /// stay outside the resident wire set so rollover and selection never
    /// invalidate normal geometry, picking, or snapping caches.
    pub(in crate::scene) annotation_highlight_cache: RefCell<HashMap<u64, Arc<Vec<WireModel>>>>,
    // The selected part is independent of box/rollover hover changes.
    pub(in crate::scene) annotation_selected_cache: RefCell<HashMap<u64, AnnotationSelectedEntry>>,
    /// The step of scale the outlines of highlighted solids were last built
    /// for in each view (its viewport handle and model tile; see
    /// `outline_detail`). A view that is being zoomed keeps its step until
    /// it rests.
    pub(in crate::scene) outline_steps: RefCell<HashMap<(Handle, Option<usize>), Option<i32>>>,
    /// Per wire-source `(geometry_epoch, gathered text)` of the most recent
    /// SDF-text build. When a new content id misses the cache but the journal
    /// shows no text-bearing entity changed since that epoch, the same glyphs
    /// are reused instead of re-walking every wire — an edit on plain geometry
    /// never rebuilds the text. Keyed by the render source (sheet / tile /
    /// content viewport / implicit view): a paper frame gathers several wire
    /// sets in one frame, and a single slot handed the sheet's glyphs to the
    /// content viewports, hiding every model-space text entity (#403).
    pub(in crate::scene) last_sdf_text: RefCell<
        HashMap<
            u64,
            (
                u64,
                std::sync::Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>,
            ),
        >,
    >,
    /// Native AcDbLight/Sun inputs, separated by rendered block and viewport
    /// frozen-layer signature. Invalidated only when a light entity changes;
    /// ordinary geometry edits never rescan the drawing's lights.
    pub(in crate::scene) lighting_cache: RefCell<HashMap<(Handle, u64), Vec<SceneLight>>>,
    pub(in crate::scene) constraint_status_cache: RefCell<parametric_solve::status::Cache>,
    /// Lazily rebuilt snapshot of every selectable group's member list, keyed
    /// by `(groups_epoch, PICKSTYLE group bit)`. Group-selection expansion used
    /// to scan ALL document objects on every pointer move, box-preview tick and
    /// rendered frame; with the snapshot those walk only the groups.
    pub(in crate::scene) groups_snapshot: RefCell<
        Option<(
            u64,
            bool,
            std::sync::Arc<Vec<(Handle, std::sync::Arc<Vec<Handle>>)>>,
        )>,
    >,
    /// Cached per-frame `ViewportData` interaction sets (bit-encoded
    /// `(selection_generation, hover_generation)` keys → selected handles,
    /// hover handles, selected signature). Building them used to clone the
    /// selected set, expand the hover set and hash the selection on EVERY
    /// rendered frame per viewport — idle CPU that scaled with the selection
    /// size even when nothing moved. The two generations cover every input:
    /// selection mutations, sub-segment picks and table-cell picks all bump
    /// the selection generation; hover sets bump the hover one.
    pub(in crate::scene) viewport_interaction_sets: RefCell<
        Option<(
            u64,
            u64,
            std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
            std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
            u64,
        )>,
    >,
    /// Cached tessellation of all visible entity wires for the current layout.
    /// Keyed by `(geometry_epoch, camera_generation)` so a camera change
    /// invalidates the cull-dependent wire list as well as a geometry change.
    /// Uses `Arc` so `build_primitive()` avoids a full Vec clone during navigation.
    /// The middle `u64` is the set's [`WIRE_CONTENT_GEN`] content id.
    pub(in crate::scene) wire_cache: RefCell<Option<((u64, u64), u64, Arc<Vec<WireModel>>)>>,
    /// Shared wire + segment broad phase for snapping, hover, click/cycling and
    /// area selection. The source pointer is part of the key because a camera
    /// cull can replace the wire Arc without changing `geometry_epoch`.
    pub(in crate::scene) interaction_index_cache: RefCell<
        Vec<(
            u64,
            usize,
            std::sync::Weak<Vec<WireModel>>,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Large resident interaction index kept as an immutable base across small
    /// entity edits. Geometry-journal handles form a tombstone/delta overlay;
    /// the exact index is rebuilt only after the overlay grows too large.
    pub(in crate::scene) interaction_base_index_cache: RefCell<
        Option<(
            u64,
            u64,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Index of only the live handles changed since the immutable interaction
    /// base. Reused across pointer events so a large unchanged block is never
    /// re-indexed merely because one top-level entity was added or removed.
    pub(in crate::scene) interaction_overlay_index_cache: RefCell<
        Option<(
            u64,
            u64,
            u64,
            Arc<Vec<WireModel>>,
            Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        )>,
    >,
    /// Geometry-journal replay for the overlay base, keyed on (base epoch,
    /// geometry epoch, space key). Pointer moves consult the overlay base
    /// several times per gesture; replay the journal once per key instead
    /// of once per consult.
    pub(in crate::scene) interaction_overlay_changes: RefCell<Option<OverlayChanges>>,
    /// Auxiliary broad phase for hatch-only objects.
    pub(in crate::scene) interaction_handle_index_cache: RefCell<
        Option<(
            u64,
            u64,
            Arc<crate::scene::pick::interaction_index::InteractionHandleIndex>,
        )>,
    >,
    /// Index built from every SortEntitiesTable in the document.
    /// Maps block_handle → (entity_handle.value() → sort_handle.value()).
    /// Replaces the O(objects) linear scan inside `wires_for_block()` with an O(1) lookup.
    pub(in crate::scene) sort_cache: RefCell<Option<(u64, HashMap<Handle, HashMap<u64, u64>>)>>,
    /// Per-entity stable draw-order depth keyed by entity handle. Initial order
    /// is assigned sparse integer labels; Add/Delete never renormalizes existing
    /// siblings, so a structural edit updates only the changed entity's GPU
    /// constants. `[depth, half]` retains a fixed child sub-range for block
    /// composition. Full sort/layout/block changes rebuild the labels.
    pub(in crate::scene) draw_depth_cache: RefCell<Option<DrawDepthCache>>,
    /// [`Scene::block_solid_snaps`] for one geometry epoch.
    pub(in crate::scene) block_solid_snaps_cache: RefCell<Option<(u64, Arc<BlockSolidSnaps>)>>,
    /// Cached hatch fill models, keyed by target block and geometry_epoch. View culling
    /// is handled at draw time via `hatch_skip_flags` in the pipeline,
    /// not at build time — that lets the GPU buffer stay stable across
    /// pan/zoom while still skipping out-of-view hatches.
    /// Keyed by geometry and selection in paper space. Model-space selection
    /// tint is drawn through the small live overlay so selecting one hatch
    /// does not rebuild and re-upload every resident hatch.
    pub(in crate::scene) hatch_cache: RefCell<HashMap<(Handle, String), (u64, u64, Arc<Vec<HatchModel>>)>>,
    /// For each `hatch_cache` entry, the top-level reference that draws each
    /// model (`Handle::NULL` for a fill drawn directly), index for index. Held
    /// with the list it describes, so an edit of one reference can replace
    /// just its fills instead of rebuilding every model.
    pub(in crate::scene) hatch_hosts:
        RefCell<HashMap<(Handle, String), (std::sync::Weak<Vec<HatchModel>>, Arc<Vec<Handle>>)>>,
    /// Small selected-hatch tint overlay, keyed independently from the
    /// resident hatch set. The rendered preview cache retains the source Arcs
    /// so pointer identity is ABA-safe when command/grip previews change.
    pub(in crate::scene) selected_hatch_overlay_cache: RefCell<Option<(u64, u64, Arc<Vec<HatchModel>>)>>,
    pub(in crate::scene) combined_preview_hatch_cache: RefCell<
        Option<(
            Arc<Vec<HatchModel>>,
            Arc<Vec<HatchModel>>,
            Arc<Vec<HatchModel>>,
        )>,
    >,
    /// Cached wipeout fill models, keyed by target block and geometry_epoch. Same
    /// reasoning as `hatch_cache`.
    pub(in crate::scene) wipeout_cache: RefCell<HashMap<(Handle, String), (u64, Arc<Vec<HatchModel>>)>>,
    /// Cached image models, keyed by target block and geometry_epoch. No camera
    /// key needed here.
    pub(in crate::scene) image_cache: RefCell<HashMap<Handle, (u64, Arc<Vec<ImageModel>>)>>,
    /// Cached mesh models, keyed by target block and geometry_epoch.
    pub(in crate::scene) mesh_cache: RefCell<HashMap<(Handle, String), (u64, Arc<Vec<MeshLodSet>>)>>,
    /// Picking mesh source for a non-model active space, keyed by geometry epoch
    /// and interaction block. Model/MSPACE reuse `mesh_cache` directly.
    pub(in crate::scene) interaction_mesh_cache: RefCell<Option<(u64, u64, Arc<Vec<MeshLodSet>>)>>,
    /// Direct handle → expanded mesh-set indices for the current renderer mesh
    /// source. Block instances may contribute several sets under one Insert
    /// handle. The weak source guard prevents stale pointer reuse.
    #[allow(clippy::type_complexity)]
    pub(in crate::scene) mesh_pick_lookup_cache: RefCell<
        Option<(
            usize,
            std::sync::Weak<Vec<MeshLodSet>>,
            Arc<HashMap<Handle, Vec<u32>>>,
        )>,
    >,
    /// The box around the vertices of each mesh the current renderer mesh
    /// source draws, by the mesh's address: the references of a block share
    /// one. The box and lasso tests tell the solids clear of a selection from
    /// it. Guarded by the weak source like the lookup above.
    #[allow(clippy::type_complexity)]
    pub(in crate::scene) mesh_pick_boxes_cache: RefCell<
        Option<(
            usize,
            std::sync::Weak<Vec<MeshLodSet>>,
            Arc<rustc_hash::FxHashMap<usize, [glam::DVec3; 2]>>,
        )>,
    >,
    /// Per-viewport (VP-frozen-layer) filtered fill / image / mesh sets, keyed by
    /// the order-independent signature of the viewport's frozen-layer set. A
    /// content viewport that freezes layers must hide their hatches, 2-D solids,
    /// images/OLE and 3-D solids too — not just wires — so these hold the
    /// filtered variants. Viewports sharing a frozen set share one entry (like
    /// the resident wire set). Empty for a viewport with no frozen layers (it
    /// reuses the unfiltered `*_arc` sets directly).
    pub(in crate::scene) frozen_hatch_cache: RefCell<HashMap<(Handle, String, u64), (u64, u64, Arc<Vec<HatchModel>>)>>,
    pub(in crate::scene) frozen_wipeout_cache: RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<HatchModel>>)>>,
    pub(in crate::scene) frozen_image_cache: RefCell<HashMap<(Handle, u64), (u64, Arc<Vec<ImageModel>>)>>,
    /// Point clouds shown by the model content, keyed by target block and
    /// frozen-layer signature, per geometry epoch.
    pub(in crate::scene) point_cloud_cache: RefCell<
        HashMap<(Handle, u64), (u64, Arc<crate::scene::model::point_cloud::PointCloudSet>)>,
    >,
    pub(in crate::scene) frozen_mesh_cache: RefCell<HashMap<(Handle, String, u64), (u64, Arc<Vec<MeshLodSet>>)>>,
    /// Viewports that carry layer color/alpha/linetype/lineweight overrides.
    /// Cached per geometry epoch so ordinary viewports can share render data.
    pub(in crate::scene) viewport_style_override_cache: RefCell<Option<(u64, HashSet<Handle>)>>,
    /// Model-space INSERTs that carry a view-representation binding, as
    /// `(insert, bound viewport)` pairs, cached for the geometry epoch and
    /// model-space block they were collected for. Every content viewport asks
    /// which references hide inside it on every frame; without this the answer
    /// came from a full document scan per viewport.
    pub(in crate::scene) view_rep_reference_cache: RefCell<Option<(u64, Handle, Arc<Vec<(Handle, Handle)>>)>>,
    /// Cached block-instance hatches for hit-testing, keyed by geometry_epoch.
    /// The set is geometry-derived, so a camera move or hover never invalidates
    /// the shared graph result.
    pub(in crate::scene) insert_hatch_cache: RefCell<Option<(u64, u64, Arc<HashMap<Handle, Vec<HatchModel>>>)>>,
    /// Sheet "dressing" cache over the unified resident set: the paper sheet
    /// drops its own border wire and appends the printable-area guide.
    /// Per-layout `(geometry_epoch, content gen, wires)` — the base is
    /// camera-independent, so paper pan/zoom and Model↔Paper tab switches keep
    /// every already-visited sheet warm.
    pub(in crate::scene) paper_sheet_cache: RefCell<HashMap<String, (u64, u64, Arc<Vec<WireModel>>)>>,
    /// Layout viewport handles, collected from the owning block record once per
    /// geometry epoch. Avoids walking every document entity on every Paper
    /// pan/zoom frame just to rediscover the same handful of viewports.
    pub(in crate::scene) paper_viewport_cache: RefCell<HashMap<String, PaperViewportCache>>,
    /// Stable Paper sheet fill/image sources. Without this cache every camera
    /// movement scanned the whole document for wipeouts and recreated the same
    /// Arcs, making Paper frame construction CPU-bound on large drawings.
    pub(in crate::scene) paper_sheet_render_cache: RefCell<HashMap<String, PaperSheetRenderCache>>,
    /// The sheet's models split at the first content viewport's draw order.
    pub(in crate::scene) paper_sheet_split_cache: RefCell<paper::SheetSplitCache>,
    pub(in crate::scene) display_plot_style_cache:
        RefCell<HashMap<(String, String), Option<Arc<crate::catalog::plot_style::PlotStyleTable>>>>,
    pub(in crate::scene) styled_wire_cache: RefCell<HashMap<(u64, String), (u64, Arc<Vec<WireModel>>)>>,
    pub(in crate::scene) styled_hatch_cache: RefCell<HashMap<(u64, usize, usize, String, u32), Arc<Vec<HatchModel>>>>,
    pub(in crate::scene) styled_wire_fill_cache: RefCell<HashMap<(u64, String, u32, u64), Arc<Vec<HatchModel>>>>,
    /// Per-viewport projected wire cache for paper-space content viewports.
    /// Stores projected + clipped wires in paper-space coordinates.
    /// Maps vp_handle → (geometry_epoch, Vec<WireModel>).
    pub(in crate::scene) paper_projected_cache: RefCell<HashMap<Handle, (u64, Vec<WireModel>)>>,
    /// Cached model-space bounding box, keyed by geometry_epoch.
    /// Avoids re-tessellating all entities on every ZOOM E / auto-fit call.
    pub(in crate::scene) model_extents_cache: RefCell<Option<(u64, Option<(glam::Vec3, glam::Vec3)>)>>,
    /// Reverse map: entity_handle → block_record_handle, built from entity_handles lists.
    /// Keyed by geometry_epoch. Eliminates the O(B) fallback scan in belongs_to_visible_block.
    pub(in crate::scene) entity_block_map_cache: RefCell<Option<(u64, HashMap<Handle, Handle>)>>,
    /// Candidate handles per block, in document order and keyed by geometry epoch.
    pub(in crate::scene) block_members_cache: RefCell<Option<BlockMembers>>,
    pub(in crate::scene) leaders_by_annotation_cache: RefCell<Option<(u64, HashMap<Handle, Vec<Handle>>)>>,
    /// Insert/Viewport/Block/BlockEnd handles omitted by the spatial index.
    pub(in crate::scene) unindexable_cache: RefCell<Option<(u64, Vec<Handle>)>>,
    /// `(epoch, layout block, present type names, list exposed to the UI)`.
    /// The set lets pure additions update without rebuilding the list.
    pub(in crate::scene) layout_type_names_cache: RefCell<
        Option<(
            u64,
            Handle,
            std::collections::BTreeSet<String>,
            std::sync::Arc<Vec<String>>,
        )>,
    >,
    pub(in crate::scene) layout_names_cache: RefCell<Option<(u64, std::sync::Arc<Vec<String>>)>>,
    pub(in crate::scene) scale_picker_cache: RefCell<
        Option<(
            u64,
            f64,
            Option<bool>,
            String,
            std::sync::Arc<Vec<(String, f32, f64)>>,
        )>,
    >,
    pub(in crate::scene) ucs_names_cache: RefCell<Option<(u64, std::sync::Arc<[String]>)>>,
    /// Reverse dependencies from layer/style/block definitions to the top-level
    /// entities whose resident wire runs actually change. Kept independent from
    /// `geometry_epoch`: a layer colour toggle can reuse the index, invalidate
    /// only its dependants, and avoid a whole-document scan on every toggle.
    pub(in crate::scene) dependency_index_cache: RefCell<Option<Arc<SceneDependencyIndex>>>,
    /// Boundary source → associative hatch handles. Source edits reuse this
    /// index instead of scanning the document during every drag step.
    pub(in crate::scene) associative_hatch_source_cache: RefCell<Option<HashMap<Handle, Vec<Handle>>>>,
    /// Centre-line source → carrier lines. Built on first use and dropped
    /// whenever an entity or an association may have changed, so an ordinary
    /// edit neither walks every line nor parses its xdata.
    pub(in crate::scene) associative_centerline_source_cache:
        RefCell<Option<crate::scene::centerline::CenterLineSourceIndex>>,
    /// Centre-mark source → carrier lines. See
    /// [`Self::associative_centerline_source_cache`].
    pub(in crate::scene) associative_centermark_source_cache: RefCell<Option<HashMap<Handle, Vec<Handle>>>>,
    /// Cached resolved document render environment (fog and background image), keyed by (geometry_epoch, document.objects.len()).
    pub(in crate::scene) document_render_env_cache: RefCell<
        Option<(
            u64,
            usize,
            crate::scene::view::render::CachedDocumentRenderEnvironment,
        )>,
    >,
    /// Memoized background image resolution to avoid repeated filesystem probes on each render frame.
    pub(in crate::scene) background_image_cache:
        RefCell<HashMap<String, Option<crate::scene::model::image_model::DecodedImage>>>,
    /// Memoised constraint-glyph placements for the latest [`GlyphKey`](parametric_constraints::GlyphKey)
    /// (Task 3; overlay/hit consumers adopt it in Task 4). Single-slot memo:
    /// within one frame view + hit-test + dwell + click share the key (hits),
    /// camera motion misses once per frame by design, and edits just miss on
    /// the stale entry — no leak possible. `RefCell` + `Arc` shape mirrors
    /// `layout_names_cache`; a missed constraint bump serves stale glyphs, so
    /// every set mutation must bump `constraints_epoch`.
    pub(in crate::scene) glyph_cache: RefCell<
        Option<(
            parametric_constraints::GlyphKey,
            Arc<[parametric_constraints::GlyphEntry]>,
        )>,
    >,
    /// Tessellated block definitions in block-local coords, keyed by render
    /// background and block epoch. Model and Paper adapt black/white colours
    /// differently; retaining both variants prevents a full block rebuild on
    /// every layout-tab switch.
    pub(in crate::scene) block_defn_cache: RefCell<HashMap<u64, (u64, Arc<cache::block_cache::BlockCache>)>>,
    /// Spatial index + always-emit list for top-level entities
    /// (Phase 2.1). Lazily rebuilt by `entity_index()` on
    /// `geometry_epoch` change. See `EntityIndex` for what each side
    /// holds and why both are needed.
    pub(in crate::scene) entity_index_cache: RefCell<Option<(u64, EntityIndex)>>,
    /// Pending REDRAW force set, keyed by viewport `instance_id`. Populated by
    /// `request_refresh` and drained per-id by `refresh_consume` from whichever
    /// builder owns that instance. Never touches geometry_epoch/block_epoch.
    pub(in crate::scene) pending_refresh: std::cell::RefCell<rustc_hash::FxHashSet<u64>>,
    /// Unified static-hold wire cache — ONE infrastructure for EVERY space
    /// (Model tiles, BEDIT block editor, the paper sheet block, and paper
    /// content viewports): the FULL, un-culled, LOD-free tessellation per
    /// `(block, ambient bg, anno scale, vp-frozen set)` key, held resident and
    /// reused for every camera. Each entry carries a stable
    /// [`WIRE_CONTENT_GEN`] id so the GPU upload gate and `render_signature`
    /// can tell "same content" from "changed" — no per-frame re-upload and no
    /// camera-keyed re-tessellation anywhere. Rebuilt per key when
    /// `geometry_epoch` changes; stale entries evicted on insert.
    #[allow(clippy::type_complexity)]
    pub(in crate::scene) resident_wire_sets: RefCell<HashMap<u64, ResidentWireSet>>,
    /// Memoized `(face3d, other)` split of a resident wire set, keyed by its
    /// [`WIRE_CONTENT_GEN`] id. `split_face3d_wires` is an O(N) per-wire
    /// handle lookup + clone that otherwise re-runs every frame. A map, not a
    /// slot: a paper frame walks several sources (sheet + viewports).
    ///
    /// `None` for the second element means "no Face3D wire in this set — use
    /// the base set itself". The base `Arc` is deliberately NOT stored here:
    /// `try_resident_patch` can only move a resident set out for an
    /// incremental patch while its `Arc` is uniquely held, so pinning it in
    /// this cache would silently force a full rebuild on every edit (#358).
    #[allow(clippy::type_complexity)]
    pub(in crate::scene) split_cache: RefCell<HashMap<u64, (Arc<Vec<WireModel>>, Option<Arc<Vec<WireModel>>>)>>,
    /// Cached `selected ∪ hover` handle set for the GPU xray overlay, keyed by
    /// `selection_generation`. Rebuilt only when the selection changes so
    /// `build_primitive` doesn't clone the set every frame.
    /// Per-entity tessellation memo for the culled Model render path (Phase
    /// 2.2). Maps a top-level handle to its already-tessellated wires so a
    /// single-entity edit re-tessellates only the changed entity and reuses the
    /// rest, instead of re-running the whole model. Keyed implicitly by
    /// `tess_memo_guard` (tol / view / anno / offset / bg); a guard mismatch
    /// (zoom, layout, …) clears it. `bump_geometry` clears it (structural
    /// change); `mark_entity_dirty` drops one handle (incremental edit).
    pub(in crate::scene) tess_memo: RefCell<HashMap<Handle, MemoRun>>,
    /// One unrestricted block-stub expansion, kept so repeated whole-set
    /// snap/pick queries over the same wires do not redo it. Keyed by the
    /// source wire set's identity and `geometry_epoch`.
    ///
    /// Measured need: on a paper-space viewport the resident-identity gate in
    /// `interaction_candidates_near_impl` fails, so every pointer move took
    /// the unindexed path and re-expanded the same 47 353 wires into the same
    /// 202 830 at ~75 ms. 355 such moves in one recorded session.
    ///
    /// The source is held weakly, not by address: a dropped `Arc` can be
    /// replaced at the same address within one epoch, and a bare pointer
    /// compare would then hand back another set's expansion.
    pub(in crate::scene) stub_expansion_cache: RefCell<
        Option<(
            std::sync::Weak<Vec<WireModel>>,
            u64,
            u64,
            Arc<Vec<WireModel>>,
        )>,
    >,
    /// Viewport wire sets with the block lines on their frozen layers left
    /// out, one for each resident set and frozen-set signature: the resident
    /// set they were made from (held weakly, its address in the key) and the
    /// result, `None` when the set holds no such line.
    #[allow(clippy::type_complexity)]
    pub(in crate::scene) viewport_frozen_member_cache: RefCell<
        HashMap<(usize, u64), (std::sync::Weak<Vec<WireModel>>, Option<Arc<Vec<WireModel>>>)>,
    >,
    /// Camera-independent model tessellation, separate from the culled memo.
    pub(in crate::scene) resident_tess_memo: RefCell<HashMap<Handle, MemoRun>>,
    /// The resident set `resident_tess_memo`'s `InSet` runs read from.
    pub(in crate::scene) resident_memo_backing: RefCell<MemoBacking>,
    /// The factor each viewport's complex linetypes are baked at. It stays put
    /// while the view moves, so a wheel tick doesn't rebuild the resident set.
    pub(in crate::scene) viewport_complex_lt_baked: RefCell<HashMap<Handle, f32>>,
    /// Per-mutation delta journal: which handles changed on each `geometry_epoch`
    /// bump. Bounded ring ([`GEOMETRY_JOURNAL_CAP`]); a derived cache replays the
    /// entries past its last-synced epoch and patches per-handle instead of
    /// re-walking the whole document. See [`Scene::replay_since`].
    pub(in crate::scene) geometry_deltas: RefCell<std::collections::VecDeque<GeometryDelta>>,
    /// Before-category hints staged by erase/history immediately before the
    /// entity disappears, then consumed by the matching geometry delta.
    pub(in crate::scene) pending_removed_categories: RefCell<HashMap<Handle, u16>>,
    /// Handoff for the GPU wire arena (`OCS_WIRE_GPU_PATCH`): when the MODEL
    /// resident set was brought up to date by an incremental patch, this records
    /// `(prev_gen, new_gen, changed handles)` so the render layer can patch just
    /// those entities' instance slabs instead of re-uploading every wire. The
    /// render layer matches `new_gen` against the viewport's content id and
    /// `prev_gen` against what the GPU currently holds; a mismatch just rebuilds.
    ///
    /// Keyed by the viewport whose resident set it belongs to. It used to be a
    /// single slot, and on a paper layout with two content viewports that was
    /// the whole reason dragging was slow: each viewport's full rebuild
    /// cleared the other's pending handoff, so neither ever had one, every
    /// `arena-base` test failed, and every dragged pointer move paid a full
    /// ~90 ms `arena-build` per viewport instead of an in-place patch. The
    /// sets are independent and already carry their own generations, so they
    /// get their own entries.
    pub(in crate::scene) model_wire_gpu_patch: RefCell<HashMap<Option<Handle>, (u64, u64, Arc<WireGpuPatch>, bool)>>,
}

impl SceneCaches {
    pub(in crate::scene) fn new() -> Self {
        Self {
            sdf_text_cache: RefCell::new(HashMap::default()),
            annotation_highlight_cache: RefCell::new(HashMap::default()),
            outline_steps: RefCell::new(HashMap::default()),
            annotation_selected_cache: RefCell::new(HashMap::default()),
            last_sdf_text: RefCell::new(HashMap::default()),
            lighting_cache: RefCell::new(HashMap::default()),
            groups_snapshot: RefCell::new(None),
            viewport_interaction_sets: RefCell::new(None),
            wire_cache: RefCell::new(None),
            interaction_index_cache: RefCell::new(Vec::new()),
            interaction_base_index_cache: RefCell::new(None),
            interaction_overlay_index_cache: RefCell::new(None),
            interaction_overlay_changes: RefCell::new(None),
            interaction_handle_index_cache: RefCell::new(None),
            sort_cache: RefCell::new(None),
            draw_depth_cache: RefCell::new(None),
            block_solid_snaps_cache: RefCell::new(None),
            hatch_cache: RefCell::new(HashMap::default()),
            hatch_hosts: RefCell::new(HashMap::default()),
            selected_hatch_overlay_cache: RefCell::new(None),
            combined_preview_hatch_cache: RefCell::new(None),
            wipeout_cache: RefCell::new(HashMap::default()),
            image_cache: RefCell::new(HashMap::default()),
            mesh_cache: RefCell::new(HashMap::default()),
            interaction_mesh_cache: RefCell::new(None),
            mesh_pick_lookup_cache: RefCell::new(None),
            mesh_pick_boxes_cache: RefCell::new(None),
            frozen_hatch_cache: RefCell::new(HashMap::default()),
            frozen_wipeout_cache: RefCell::new(HashMap::default()),
            frozen_image_cache: RefCell::new(HashMap::default()),
            point_cloud_cache: RefCell::new(HashMap::default()),
            frozen_mesh_cache: RefCell::new(HashMap::default()),
            viewport_style_override_cache: RefCell::new(None),
            view_rep_reference_cache: RefCell::new(None),
            insert_hatch_cache: RefCell::new(None),
            paper_sheet_cache: RefCell::new(HashMap::default()),
            paper_viewport_cache: RefCell::new(HashMap::default()),
            paper_sheet_render_cache: RefCell::new(HashMap::default()),
            paper_sheet_split_cache: RefCell::new(paper::SheetSplitCache::default()),
            display_plot_style_cache: RefCell::new(HashMap::default()),
            styled_wire_cache: RefCell::new(HashMap::default()),
            styled_hatch_cache: RefCell::new(HashMap::default()),
            styled_wire_fill_cache: RefCell::new(HashMap::default()),
            paper_projected_cache: RefCell::new(HashMap::default()),
            constraint_status_cache: Default::default(),
            model_extents_cache: RefCell::new(None),
            entity_block_map_cache: RefCell::new(None),
            block_members_cache: RefCell::new(None),
            leaders_by_annotation_cache: RefCell::new(None),
            unindexable_cache: RefCell::new(None),
            layout_type_names_cache: RefCell::new(None),
            layout_names_cache: RefCell::new(None),
            scale_picker_cache: RefCell::new(None),
            ucs_names_cache: RefCell::new(None),
            dependency_index_cache: RefCell::new(None),
            associative_hatch_source_cache: RefCell::new(None),
            associative_centerline_source_cache: RefCell::new(None),
            associative_centermark_source_cache: RefCell::new(None),
            glyph_cache: RefCell::new(None),
            document_render_env_cache: RefCell::new(None),
            background_image_cache: RefCell::new(HashMap::default()),
            block_defn_cache: RefCell::new(HashMap::default()),
            entity_index_cache: RefCell::new(None),
            pending_refresh: std::cell::RefCell::new(rustc_hash::FxHashSet::default()),
            resident_wire_sets: RefCell::new(HashMap::default()),
            split_cache: RefCell::new(HashMap::default()),
            tess_memo: RefCell::new(HashMap::default()),
            stub_expansion_cache: RefCell::new(None),
            viewport_frozen_member_cache: RefCell::new(HashMap::default()),
            resident_tess_memo: RefCell::new(HashMap::default()),
            resident_memo_backing: RefCell::new(MemoBacking::default()),
            viewport_complex_lt_baked: RefCell::new(HashMap::default()),
            geometry_deltas: RefCell::new(std::collections::VecDeque::new()),
            pending_removed_categories: RefCell::new(HashMap::default()),
            model_wire_gpu_patch: RefCell::new(HashMap::default()),
        }
    }
}
