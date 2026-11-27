use super::*;

/// Pre-built entity caches returned by [`build_derived_caches`].
/// Produced in the file-load background task so the UI thread only assigns.
#[derive(Debug, Clone)]
pub struct DerivedCaches {
    pub read_stats: Option<document::ReadStats>,
    pub source_sha256: Option<String>,
    pub local_extent_max: f32,
    pub local_center: [f64; 2],
    pub hatches: HashMap<Handle, HatchModel>,
    pub images: HashMap<Handle, ImageModel>,
    pub meshes: HashMap<Handle, MeshLodSet>,
    /// Block-definition solid meshes, block-local frame (instanced per INSERT). (#123)
    pub block_meshes: HashMap<Handle, MeshLodSet>,
    /// Non-graphical DWG object relationships and Drawing-property rows.
    /// Prepared during file open so entity selection/deselection never scans
    /// the complete object store on the UI thread.
    pub object_data: crate::entities::object_data::ObjectDataCache,
    /// XREF resolution results produced by the loader worker. Keeping these in
    /// the open bundle prevents parsing and merging references on the UI thread.
    pub xrefs: Vec<crate::scene::xref_info::XrefInfo>,
    /// Model wire set and its spatial interaction index, prepared on the loader
    /// thread. Installing these prevents the first visible frame from paying a
    /// whole-drawing tessellation/index build while the progress overlay freezes.
    pub prepared_geometry: Option<PreparedOpenGeometry>,
    /// Background-thread open-phase timings in milliseconds (parse, purge,
    /// derived-cache build). Filled in by `open_path_with_phase`; surfaced in
    /// the open-complete breakdown log so open-time regressions are visible.
    pub timings: OpenTimings,
}

/// Wall-clock breakdown of the file-open phases, in milliseconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenTimings {
    pub parse_ms: u32,
    pub purge_ms: u32,
    pub caches_ms: u32,
    pub xref_ms: u32,
    /// The FINALIZING phase: `prepare_open_geometry` — first full Model
    /// tessellation plus interaction index. Previously unmeasured, which made
    /// it indistinguishable from parse time in an open that appeared to stall.
    pub finalize_ms: u32,
}

/// Build hatch / image / mesh caches from a document without needing `&mut Scene`.
/// Intended to run on a background thread during file load.
#[cfg(target_arch = "wasm32")]
pub fn build_derived_caches(doc: &Document) -> DerivedCaches {
    build_derived_caches_impl(doc, None, None)
}

/// Build open-time caches while reporting monotonic progress in 0..=10000.
///
/// The callback is UI-agnostic and may run from Rayon workers. Callers should
/// keep it cheap, normally just updating atomics.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_derived_caches_with_progress(
    doc: &Document,
    progress: &(dyn Fn(u16) + Sync),
    material_base_dir: Option<&std::path::Path>,
) -> DerivedCaches {
    build_derived_caches_impl(doc, Some(progress), material_base_dir)
}

pub(crate) fn build_derived_caches_impl(
    doc: &Document,
    progress: Option<&(dyn Fn(u16) + Sync)>,
    material_base_dir: Option<&std::path::Path>,
) -> DerivedCaches {
    // A new drawing must not inherit the previous one's resolved images — drop
    // the memoised set so each reference re-reads / re-fetches once here (and
    // stays cached across this document's later cache rebuilds).
    crate::scene::model::image_model::clear_image_cache();
    let object_data = crate::entities::object_data::build_cache(doc);
    // model-space block handle (same logic as Scene::model_space_block_handle)
    let model_block = doc
        .objects
        .values()
        .find_map(|obj| {
            if let document::objects::ObjectType::Layout(l) = obj {
                if l.name == "Model" && !l.block_record.is_null() {
                    Some(l.block_record)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            doc.block_records
                .get("*Model_Space")
                .map(|br| br.handle)
                .unwrap_or(Handle::NULL)
        });

    // world_offset selection
    //
    // Header `$EXTMIN`/`$EXTMAX` is the fast path, but it's untrustworthy:
    // the sentinel (1e20 / -1e20) when the writer never computed extents,
    // stale values when a drawing was edited and extents weren't refreshed,
    // and top-level extents that span only an Insert's bounding box rather
    // than the actual MSPACE geometry. Any of those
    // leave the precision-preserving offset wrong, so direct MSPACE
    // entities render at huge magnitudes and f32 wires lose precision.
    //
    // Cross-check the header against a per-entity AABB scan of MSPACE
    // (same `bounding_box()` API and same SANE_EXTENT/zero-placeholder
    // filters that `cache::block_cache::build_defn` already uses for block defns)
    // and prefer the entity-scan when the header center drifts more than
    // 10× its own half-span away from the entity centroid.
    use crate::par::prelude::*;

    // Single pass over entities does triple duty: classify cache-kind handle
    // lists (hatch / image / mesh) AND accumulate per-entity centroids for the
    // world_offset median. Folding the offset scan in here collapses what were
    // two O(N) `entities()` walks (offset scan + handle collection) into one.
    // Heavy tessellation runs in parallel below, reading entities via
    // `doc.get_entity(h)` (O(1) HashMap lookup); no clones in this pass.
    let prep = offset_prep(doc, model_block);
    let mut hatch_handles: Vec<Handle> = Vec::new();
    let mut image_handles: Vec<Handle> = Vec::new();
    let mut mesh_handles: Vec<Handle> = Vec::new();
    let mut centers: Vec<[f64; 3]> = Vec::new();
    //
    // The per-entity `bounding_box()` behind the centroid is the expensive
    // part, so the scan itself runs in parallel; the order-preserving collect
    // keeps the handle lists and centroids in document order, and the cheap
    // gather below stays serial.
    #[derive(Clone, Copy)]
    enum CacheKind {
        None,
        Hatch,
        Image,
        Mesh,
    }
    let entity_refs: Vec<&EntityType> = doc.entities().collect();
    let entity_total = entity_refs.len().max(1);
    let scanned_count = std::sync::atomic::AtomicUsize::new(0);
    let scanned: Vec<(CacheKind, Option<[f64; 3]>)> = entity_refs
        .par_iter()
        .map(|&e| {
            let kind = match e {
                EntityType::Hatch(_) | EntityType::Solid(_) => CacheKind::Hatch,
                EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_) => {
                    CacheKind::Image
                }
                EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_) => CacheKind::Mesh,
                _ => CacheKind::None,
            };
            let centroid = offset_centroid(e, model_block, &prep);
            let done = scanned_count.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0x1fff == 0 {
                if let Some(progress) = progress {
                    progress(((done as u64 * 4000) / entity_total as u64) as u16);
                }
            }
            (kind, centroid)
        })
        .collect();
    for (e, (kind, centroid)) in entity_refs.iter().zip(scanned) {
        let h = e.common().handle;
        match kind {
            CacheKind::Hatch => hatch_handles.push(h),
            CacheKind::Image => image_handles.push(h),
            CacheKind::Mesh => mesh_handles.push(h),
            CacheKind::None => {}
        }
        if let Some(c) = centroid {
            centers.push(c);
        }
    }
    drop(entity_refs);
    if let Some(progress) = progress {
        progress(4000);
    }
    let (local_center, local_extent_max) = cluster_extent_from_centers(centers, &doc.header);

    // Default bg adaptation target at load: the model background (paper
    // bg is only relevant after the user enters a paper layout, and
    // `synced_hatch_models` re-runs `render_style` per-frame anyway so
    // the per-layout adaptation kicks in later regardless).
    const LOAD_BG: [f32; 4] = [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0];

    let detail_total = hatch_handles
        .len()
        .saturating_add(image_handles.len())
        .saturating_add(mesh_handles.len())
        .max(1);
    let detail_done = std::sync::atomic::AtomicUsize::new(0);
    let report_detail = |done: usize| {
        if let Some(progress) = progress {
            let value = 4000u64 + done as u64 * 6000 / detail_total as u64;
            progress(value.min(10000) as u16);
        }
    };

    // hatches
    let hatches: HashMap<Handle, HatchModel> = hatch_handles
        .par_iter()
        .filter_map(|&handle| {
            let source = doc.get_entity(handle)?;
            let contextual = annotative::entity_for_annotation_context(
                doc,
                source,
                annotative::scale_handle_by_name(doc, &doc.header.current_annotation_scale),
            );
            let e = contextual.as_ref();
            let (raw, ..) = view::render::render_style_for(doc, e);
            let color = view::render::adapt_to_bg(raw, LOAD_BG);
            let model = match e {
                EntityType::Hatch(dxf) => Scene::hatch_model_from_dxf(dxf, color),
                EntityType::Solid(solid) => Some(Scene::solid_hatch_model(solid, color)),
                _ => None,
            };
            let result = model.map(|m| (handle, m));
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();

    // images
    let images: HashMap<Handle, ImageModel> = image_handles
        .par_iter()
        .filter_map(|&handle| {
            let result = match doc.get_entity(handle)? {
                EntityType::RasterImage(img) => {
                    ImageModel::from_raster_image(img).map(|m| (handle, m))
                }
                EntityType::Ole2Frame(ole) => ImageModel::from_ole2frame(ole).map(|m| (handle, m)),
                EntityType::Underlay(u) => match doc.objects.get(&u.definition_handle) {
                    Some(document::objects::ObjectType::UnderlayDefinition(def)) => {
                        // Paper-space underlays adjust to the white sheet.
                        let owner = u.common.owner_handle;
                        let background = if owner.is_null() || owner == model_block {
                            LOAD_BG
                        } else {
                            [1.0, 1.0, 1.0, 1.0]
                        };
                        ImageModel::from_underlay(u, def, background, None).map(|m| (handle, m))
                    }
                    _ => None,
                },
                _ => None,
            };
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();

    // FACETRES biases one shared chordal tolerance for all solids.
    // Top-level (layout-owned) solids are offset into the render frame; block
    // definition solids keep block-local coords for per-INSERT instancing. (#123)
    let facet_res = doc.header.facet_resolution;
    let chordal_deflection = crate::entities::solid3d::display_deflection(&doc.header, facet_res);
    let isolines = crate::entities::solid3d::clamp_header_isolines(doc.header.isolines);
    // Real layout blocks come from the Layout objects' block_record handles —
    // `BlockRecord::is_layout()` is unreliable here (it flags ordinary blocks).
    let layout_blocks: std::collections::HashSet<Handle> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            document::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                Some(l.block_record)
            }
            _ => None,
        })
        .collect();
    let built: Vec<(Handle, MeshLodSet, bool)> = mesh_handles
        .par_iter()
        .filter_map(|&handle| {
            let e = doc.get_entity(handle)?;
            let (raw, ..) = view::render::render_style_for(doc, e);
            let color = view::render::adapt_to_bg(raw, LOAD_BG);
            let material = crate::scene::model::material_model::resolve_material_with_base(
                doc,
                e,
                color,
                None,
                material_base_dir,
            );
            let top_level = layout_blocks.contains(&e.common().owner_handle);
            let result = crate::entities::solid3d::tessellate_volume(
                e,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(|mut mesh| {
                material.apply_to_with_face_overrides(&mut mesh, doc, material_base_dir);
                crate::scene::model::visual_style_model::apply_mesh_visual_style(&mut mesh, doc, e);
                let mesh = if top_level {
                    offset_mesh_lod_set(mesh)
                } else {
                    mesh
                };
                (handle, mesh, top_level)
            });
            let done = detail_done.fetch_add(1, Ordering::Relaxed) + 1;
            if done & 0xff == 0 || done == detail_total {
                report_detail(done);
            }
            result
        })
        .collect();
    let mut meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    let mut block_meshes: HashMap<Handle, MeshLodSet> = HashMap::default();
    for (handle, mut m, top_level) in built {
        if top_level {
            meshes.insert(handle, m);
        } else {
            m.prepare_instance_source(handle);
            block_meshes.insert(handle, m);
        }
    }

    if let Some(progress) = progress {
        progress(10000);
    }

    DerivedCaches {
        read_stats: None,
        source_sha256: None,
        local_extent_max,
        local_center,
        hatches,
        images,
        meshes,
        block_meshes,
        object_data,
        xrefs: Vec::new(),
        prepared_geometry: None,
        timings: OpenTimings::default(),
    }
}

/// Prepare the expensive first Model wire set and spatial interaction index on
/// the loader thread. The temporary `Scene` never crosses threads (it contains
/// `Rc`/`RefCell` state); only its Send-safe document and immutable prepared
/// geometry are returned.
#[cfg(not(target_arch = "wasm32"))]
pub fn prepare_open_geometry(
    doc: Document,
    caches: &mut DerivedCaches,
    model_bg: [f32; 4],
) -> (Document, PreparedOpenGeometry) {
    let mut scene = Scene::new();
    scene.document = doc;
    // Solid meshes give solid wires their corner snaps; lent to the scene
    // for the build and handed back below.
    scene.meshes = std::mem::take(&mut caches.meshes);
    scene.block_meshes = std::mem::take(&mut caches.block_meshes);
    scene.local_extent_max = caches.local_extent_max;
    scene.local_center = caches.local_center;
    scene.bg_color = model_bg;
    scene.sync_annotation_scale_from_header();
    scene.current_layout = "Model".to_string();
    // The Properties palette resolves each text style at its first refresh,
    // on the UI thread. Resolve them here, so that the font files of the
    // styles that no model-space text uses are read on this worker.
    for style in scene.document.text_styles.iter() {
        let _ = crate::entities::text_support::resolve_text_style(&style.name, &scene.document);
    }
    let camera = scene.camera.borrow().clone();
    let perf = crate::perf::enabled();
    let t_wires = perf.then(iced::time::Instant::now);
    let wires = scene.model_tile_wires_arc(0, &camera, 1.0, 1.0);
    let wires_ms = crate::perf::elapsed_ms(t_wires);
    // The temporary scene is discarded after this function. Keep its spatial
    // entity index so opening a large drawing does not rebuild the same O(N)
    // index synchronously on the UI thread during the 99.5% finalization step.
    let t_entity_index = perf.then(iced::time::Instant::now);
    let _ = scene.entity_index();
    let entity_index =
        std::mem::take(&mut *scene.caches.entity_index_cache.borrow_mut()).map(|(_, index)| index);
    let entity_index_ms = crate::perf::elapsed_ms(t_entity_index);
    let t_index = perf.then(iced::time::Instant::now);
    let interaction_index = if scene.interaction_index_worthwhile(&wires) {
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
        let build_ms = crate::perf::elapsed_ms(t_index);
        if perf {
            crate::perf_record!("[perf] open-index build={:.1}ms", build_ms);
        }
        Some(index)
    } else {
        None
    };
    if perf {
        crate::perf_record!(
            "[perf] open-finalize wires={:.1}ms index={:.1}ms entity_index={:.1}ms wire_models={}",
            wires_ms,
            crate::perf::elapsed_ms(t_index),
            entity_index_ms,
            wires.len(),
        );
    }
    // The hatch picking index walks every entity and needs the hatch models,
    // which stay out of the wire tessellation above. Built here it no longer
    // freezes the UI thread right after the open; the models are lent to the
    // scene for this one build and handed back untouched.
    let t_handles = perf.then(iced::time::Instant::now);
    scene.hatches = std::mem::take(&mut caches.hatches);
    let _ = scene.interaction_handle_index();
    caches.hatches = std::mem::take(&mut scene.hatches);
    let handle_index = scene
        .caches.interaction_handle_index_cache
        .borrow_mut()
        .take()
        .map(|(_, space_key, index)| (space_key, index));
    let insert_hatches = scene
        .caches.insert_hatch_cache
        .borrow_mut()
        .take()
        .map(|(_, space_key, hatches)| (space_key, hatches));
    if perf {
        crate::perf_record!(
            "[perf] open-handle-index={:.1}ms",
            crate::perf::elapsed_ms(t_handles),
        );
    }
    // Taken, not cloned: the scene is discarded on the next line.
    let tess_memo = std::mem::take(&mut *scene.caches.resident_tess_memo.borrow_mut());
    let tess_memo_backing = std::mem::take(&mut *scene.caches.resident_memo_backing.borrow_mut());
    let tess_guard = scene.resident_tess_guard.get();
    let tess_atlas = scene.resident_tess_atlas.get();
    let draw_depths = scene.caches.draw_depth_cache.borrow_mut().take();
    // The layout the loader already built for exactly these wires. Taken by
    // identity so it cannot be paired with a different assembly.
    let resident_layout = scene
        .caches.resident_wire_sets
        .borrow_mut()
        .drain()
        .find_map(|(_, set)| {
            Arc::ptr_eq(&set.wires, &wires)
                .then_some(set.layout)
                .flatten()
        });
    let block_defns: Vec<(u64, Arc<cache::block_cache::BlockCache>)> =
        std::mem::take(&mut *scene.caches.block_defn_cache.borrow_mut())
            .into_iter()
            .map(|(key, (_epoch, cache))| (key, cache))
            .collect();
    // Layer changes are interactive. Build their reverse lookup beside the
    // other loader indexes so the first pick doesn't scan the whole DWG.
    let dependency_index = scene
        .caches.dependency_index_cache
        .get_mut()
        .take()
        .unwrap_or_else(|| Arc::new(scene.rebuild_dependency_index()));
    let doc = std::mem::replace(&mut scene.document, Document::new());
    caches.meshes = std::mem::take(&mut scene.meshes);
    caches.block_meshes = std::mem::take(&mut scene.block_meshes);
    (
        doc,
        PreparedOpenGeometry {
            dependency_index: Some(dependency_index),
            wires,
            interaction_index,
            entity_index,
            tess_memo,
            tess_memo_backing,
            tess_guard,
            tess_atlas,
            block_defns,
            draw_depths,
            resident_layout,
            handle_index,
            insert_hatches,
        },
    )
}

/// Mirrors `cache::block_cache::SANE_EXTENT` — wire coords past this magnitude
/// are treated as corruption rather than precision-relevant geometry.
const CLUSTER_SANE_EXTENT: f64 = 1.0e8;

/// MSPACE-membership prep shared by the world-offset centroid scan.
///
/// The filter here MUST agree with `belongs_to_visible_block` (the
/// render-time filter): if rendering treats an entity as MSPACE but we skip
/// it here, our offset misses on-screen geometry and direct WCS-coordinate
/// wires drag f32 precision to its knees. Conversely, including block-defn
/// entities the render path drops would pull the centroid toward block-local
/// origins.
struct OffsetPrep {
    /// `Some` when the model BlockRecord enumerates its entities; the offset
    /// scan uses this set directly. `None` falls back to the legacy
    /// permissive owner-based interpretation.
    mspace_set: Option<rustc_hash::FxHashSet<Handle>>,
    any_enumerated: bool,
    owned_by_other_block: rustc_hash::FxHashSet<Handle>,
}

fn offset_prep(doc: &document::Document, model_block: Handle) -> OffsetPrep {
    let model_br = doc.block_records.iter().find(|br| br.handle == model_block);
    let mspace_set: Option<rustc_hash::FxHashSet<Handle>> = model_br
        .filter(|br| !br.entity_handles.is_empty())
        .map(|br| br.entity_handles.iter().copied().collect());
    let any_enumerated = doc
        .block_records
        .iter()
        .any(|br| !br.entity_handles.is_empty());
    let owned_by_other_block: rustc_hash::FxHashSet<Handle> = if mspace_set.is_none() {
        doc.block_records
            .iter()
            .filter(|br| br.handle != model_block)
            .flat_map(|br| br.entity_handles.iter().copied())
            .collect()
    } else {
        rustc_hash::FxHashSet::default()
    };
    OffsetPrep {
        mspace_set,
        any_enumerated,
        owned_by_other_block,
    }
}

/// Per-entity centroid for the world-offset scan, or `None` if the entity is
/// not MSPACE geometry / has no usable bbox. Single-outlier-robust because
/// the caller takes the median of these per-entity centroids rather than a
/// global min/max midpoint.
fn offset_centroid(e: &EntityType, model_block: Handle, prep: &OffsetPrep) -> Option<[f64; 3]> {
    let c = e.common();
    let h = c.handle;
    let include = if let Some(ref set) = prep.mspace_set {
        set.contains(&h)
    } else if c.owner_handle == model_block {
        true
    } else if !c.owner_handle.is_null() {
        false
    } else if prep.owned_by_other_block.contains(&h) {
        false
    } else {
        // owner null + h not enumerated by any block: legacy permissive
        // when no block enumerated at all, strict drop otherwise (same
        // as belongs_to_visible_block).
        !prep.any_enumerated
    };
    if !include {
        return None;
    }
    // Skip block-defn sentinels and AttributeDefinition — same as
    // cache::block_cache::build_defn. Their bboxes don't represent drawable
    // MSPACE geometry.
    if matches!(
        e,
        EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::AttributeDefinition(_)
    ) {
        return None;
    }
    let (bmin, bmax) = match e {
        EntityType::Insert(ins) => (ins.insert_point, ins.insert_point),
        _ => {
            let bb = e.as_entity().bounding_box();
            (bb.min, bb.max)
        }
    };
    // Empty-entity placeholder (Polyline/Hatch/Spline/Mesh with no
    // vertices). Including these would pull the centroid toward origin
    // and destroy precision on UTM-authored content.
    if bmin.x == 0.0
        && bmin.y == 0.0
        && bmin.z == 0.0
        && bmax.x == 0.0
        && bmax.y == 0.0
        && bmax.z == 0.0
    {
        return None;
    }
    let cx = (bmin.x + bmax.x) * 0.5;
    let cy = (bmin.y + bmax.y) * 0.5;
    let cz = (bmin.z + bmax.z) * 0.5;
    if !cx.is_finite() || !cy.is_finite() || !cz.is_finite() {
        return None;
    }
    if cx.abs() > CLUSTER_SANE_EXTENT || cy.abs() > CLUSTER_SANE_EXTENT {
        return None;
    }
    Some([cx, cy, cz])
}

/// Pick the model-space precision-preserving offset and the `fit_all`
/// outlier-rejection limit from the collected per-entity `centers`.
///
/// Prefers the entity-centroid median; cross-checks against header
/// `$EXTMIN/$EXTMAX` only as a fallback when the entity scan found nothing.
/// `centers` is gathered by the caller's single entity walk (see
/// [`build_derived_caches`]) so no separate AABB pass is needed.
/// Returns `(center, half_span)` of the dense entity cluster. The center is the
/// median of entity centroids — robust against a second, far cluster (e.g. a
/// small-coordinate legend beside a UTM survey), unlike the raw extents centre
/// which would land in the empty gap between them.
fn cluster_extent_from_centers(
    centers: Vec<[f64; 3]>,
    header: &document::document::HeaderVariables,
) -> ([f64; 2], f32) {
    const SANE_EXTENT: f64 = CLUSTER_SANE_EXTENT;
    let entity_ok = !centers.is_empty();

    // 95th-percentile distance from the median × 2 gives the half-span of the
    // dense cluster while leaving room for legitimate outliers (sparse leaders,
    // dimensions, scattered annotations).
    let median = |v: &mut Vec<f64>| -> f64 {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let percentile = |v: &mut Vec<f64>, frac: f64| -> f64 {
        v.sort_by(|a, b| a.total_cmp(b));
        let i = ((v.len() as f64 - 1.0) * frac).round() as usize;
        v[i]
    };
    let (ecenter, espan_max) = if entity_ok {
        let mut xs: Vec<f64> = centers.iter().map(|c| c[0]).collect();
        let mut ys: Vec<f64> = centers.iter().map(|c| c[1]).collect();
        let mx = median(&mut xs);
        let my = median(&mut ys);
        let mut dx: Vec<f64> = centers.iter().map(|c| (c[0] - mx).abs()).collect();
        let mut dy: Vec<f64> = centers.iter().map(|c| (c[1] - my).abs()).collect();
        let p95 = percentile(&mut dx, 0.95).max(percentile(&mut dy, 0.95));
        ([mx, my], (p95 * 2.0).max(1.0) as f32)
    } else {
        ([0.0, 0.0], 0.0)
    };

    // ── Header extents (fallback only) ───────────────────────────────────
    let hmin = header.model_space_extents_min;
    let hmax = header.model_space_extents_max;
    let header_ok = hmin.x < hmax.x
        && hmin.y < hmax.y
        && hmin.x.abs() < SANE_EXTENT
        && hmax.x.abs() < SANE_EXTENT
        && hmin.y.abs() < SANE_EXTENT
        && hmax.y.abs() < SANE_EXTENT;

    // Geometry reaches the GPU as absolute coordinates (the double-single
    // relative-to-eye path keeps it precise at UTM scale), so only the
    // cluster span — for camera fit and cull — is derived from the content.
    if entity_ok {
        (ecenter, espan_max)
    } else if header_ok {
        let hw = ((hmax.x - hmin.x) * 0.5) as f32;
        let hh = ((hmax.y - hmin.y) * 0.5) as f32;
        let hz = ((hmax.z - hmin.z) * 0.5).max(1.0) as f32;
        let hcenter = [(hmin.x + hmax.x) * 0.5, (hmin.y + hmax.y) * 0.5];
        (hcenter, hw.max(hh).max(hz) * 10.0)
    } else {
        ([0.0, 0.0], 1e9_f32)
    }
}

/// Shift every vertex of a freshly tessellated `MeshLodSet` into the
/// scene's local f32 space by subtracting `world_offset`. ACIS / SAT
/// tessellation hands us WCS coordinates; the wire / hatch / face3d
/// paths run in `(WCS - world_offset)` so meshes at large UTM-scale
/// origins would otherwise float far away from the rest of the
/// geometry. Also recomputes `world_aabb` so per-frame LOD / cull math
/// uses the same space.
pub(in crate::scene) fn offset_mesh_lod_set(mut set: MeshLodSet) -> MeshLodSet {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for lod in &mut set.lods {
        // Reconstruct the f64 absolute position from the double-single pair,
        // subtract world_offset in f64, then re-split into (high, low) so the
        // relative-to-eye shader keeps sub-unit precision at UTM scale.
        let has_low = lod.verts_low.len() == lod.verts.len();
        if !has_low {
            lod.verts_low = vec![[0.0; 3]; lod.verts.len()];
        }
        for (v, vl) in lod.verts.iter_mut().zip(lod.verts_low.iter_mut()) {
            let ax = v[0] as f64 + vl[0] as f64;
            let ay = v[1] as f64 + vl[1] as f64;
            let az = v[2] as f64 + vl[2] as f64;
            let hx = ax as f32;
            let hy = ay as f32;
            let hz = az as f32;
            *v = [hx, hy, hz];
            *vl = [
                (ax - hx as f64) as f32,
                (ay - hy as f64) as f32,
                (az - hz as f64) as f32,
            ];
            if hx < min_x {
                min_x = hx;
            }
            if hy < min_y {
                min_y = hy;
            }
            if hx > max_x {
                max_x = hx;
            }
            if hy > max_y {
                max_y = hy;
            }
        }
    }
    // Re-split the feature edges the same way so they track the mesh at scale.
    {
        let n = set.edge_verts.len();
        if set.edge_verts_low.len() != n {
            set.edge_verts_low = vec![[0.0; 3]; n];
        }
        for (v, vl) in set.edge_verts.iter_mut().zip(set.edge_verts_low.iter_mut()) {
            let ax = v[0] as f64 + vl[0] as f64;
            let ay = v[1] as f64 + vl[1] as f64;
            let az = v[2] as f64 + vl[2] as f64;
            let (hx, hy, hz) = (ax as f32, ay as f32, az as f32);
            *v = [hx, hy, hz];
            *vl = [
                (ax - hx as f64) as f32,
                (ay - hy as f64) as f32,
                (az - hz as f64) as f32,
            ];
        }
    }
    if min_x.is_finite() {
        set.world_aabb = [min_x, min_y, max_x, max_y];
    }
    set.recompute_aabb();
    set
}

/// Build a lightweight placement over immutable block-local geometry.
pub(in crate::scene) fn transform_block_mesh_lod_set(
    set: &MeshLodSet,
    xform: &document::types::Transform,
) -> MeshLodSet {
    use document::types::Vector3;
    let source = set.instance_source.clone().unwrap_or_else(|| {
        std::sync::Arc::new(model::mesh_model::MeshInstanceSource {
            handle: set.entity_handle().unwrap_or(Handle::new(0)),
            lods: set.lods.clone(),
            triangle_bvhs: model::mesh_model::new_triangle_bvh_slots(set.lods.len()),
            edge_verts: set.edge_verts.clone(),
            edge_verts_low: set.edge_verts_low.clone(),
            curved_gens: set.curved_gens.clone(),
            display_lods: set.display_lods.clone(),
        })
    });
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for x in [set.world_aabb[0], set.world_aabb[2]] {
        for y in [set.world_aabb[1], set.world_aabb[3]] {
            for z in [set.z_aabb[0], set.z_aabb[1]] {
                let point = xform.apply(Vector3::new(x as f64, y as f64, z as f64));
                for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                    bounds[axis] = bounds[axis].min(value);
                    bounds[axis + 3] = bounds[axis + 3].max(value);
                }
            }
        }
    }
    MeshLodSet {
        lods: Vec::new(),
        triangle_bvhs: Default::default(),
        effective_layer: set.effective_layer.clone(),
        material: set.material.clone(),
        face_materials: set.face_materials.clone(),
        visual_style: set.visual_style.clone(),
        complete: set.complete,
        edge_verts: Vec::new(),
        edge_verts_low: Vec::new(),
        edge_vert_colors: Vec::new(),
        curved_gens: Vec::new(),
        metrics: set.metrics,
        world_aabb: [
            bounds[0] as f32,
            bounds[1] as f32,
            bounds[3] as f32,
            bounds[4] as f32,
        ],
        z_aabb: [bounds[2] as f32, bounds[5] as f32],
        instance_source: Some(source),
        instance_transform: Some(*xform),
        instance_handle: None,
        instance_color: set.display_color(),
        edge_color: set.edge_color,
        instance_aabb: bounds
            .iter()
            .all(|value| value.is_finite())
            .then_some(bounds),
        cached_interaction_aabb: Default::default(),
        display_lods: Default::default(),
    }
}

impl Scene {
    /// Called by the render pipeline once per frame so camera fit operations
    /// know the active widget's aspect ratio.
    pub fn set_render_aspect(&self, aspect: f32) {
        if aspect.is_finite() && aspect > 0.0 {
            self.last_render_aspect.set(aspect);
        }
    }

    /// World units per screen pixel at the current viewport size. Returns
    /// `None` until the first render captures real bounds.
    ///
    /// Also returns `None` in paper space: `last_world_per_pixel` tracks the
    /// model camera, so a cached value applied to mm-sheet entity AABBs would
    /// be a stale model-world wpp and cull every paper-space annotation.
    /// Paper-space callers use their own scale instead.
    /// Rasterise the underlays again when the view's scale has moved to
    /// another quarter-octave step and stayed there briefly (not on every
    /// frame of a zoom), and likewise the OLE pictures of the current layout
    /// and the underlays on its sheet.
    pub fn refresh_underlay_resolution(&mut self, now: iced::time::Instant) {
        let generation = self.camera_generation;
        if let Some((wpp, view)) = self.model_view_scale() {
            let layout = self.current_layout.clone();
            if self
                .underlay_scale
                .settle(wpp, view, &layout, generation, now)
                && self
                    .document
                    .entities()
                    .any(|e| matches!(e, EntityType::Underlay(_)))
            {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // With a sheet in view, the pages of the model alone: one
                    // on a sheet follows that sheet's view (below).
                    let mut sheets = HashSet::default();
                    if layout != "Model" {
                        sheets = self.layout_block_handles();
                        sheets.remove(&self.model_space_block_handle());
                    }
                    self.queue_reseed(|e| {
                        matches!(e, EntityType::Underlay(u)
                            if !sheets.contains(&u.common.owner_handle))
                    });
                }
                #[cfg(target_arch = "wasm32")]
                self.reseed_underlays();
            }
        }
        if let Some(wpp) = self.layout_world_per_pixel() {
            let view = self.layout_plan_view_rect();
            let layout = self.current_layout.clone();
            let sheet_underlays = self.sheet_may_hold_underlays();
            if self.ole_scale.settle(wpp, view, &layout, generation, now)
                && (self.has_ole_frames() || sheet_underlays)
            {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let sheet = self.current_layout_block_handle();
                    self.queue_reseed(|e| match e {
                        EntityType::Ole2Frame(_) => true,
                        EntityType::Underlay(u) => {
                            sheet_underlays && u.common.owner_handle == sheet
                        }
                        _ => false,
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    if self.has_ole_frames() {
                        self.reseed_ole_frames();
                    }
                    if sheet_underlays {
                        self.reseed_underlays();
                    }
                }
            }
        }
    }

    /// The view has moved to another scale step than the underlay or OLE
    /// rasters were made for, or out of their sharp region (frames are
    /// requested until they are made again).
    pub fn underlay_resolution_stale(&self) -> bool {
        let underlays = self.has_underlays()
            && self.model_view_scale().is_some_and(|(wpp, view)| {
                self.underlay_scale.stale(wpp, view, &self.current_layout)
            });
        underlays
            || ((self.has_ole_frames() || self.sheet_may_hold_underlays())
                && self.layout_world_per_pixel().is_some_and(|wpp| {
                    self.ole_scale
                        .stale(wpp, self.layout_plan_view_rect(), &self.current_layout)
                }))
    }

    /// The scale the model is seen at now (model units per pixel) and the
    /// plan rectangle of it in sight: those of the model view, or with a
    /// sheet in view those of its viewports. `None` when the model is not in
    /// sight, or before a frame has told the size of the view.
    pub(in crate::scene) fn model_view_scale(&self) -> Option<(f32, Option<[f64; 4]>)> {
        match self.world_per_pixel() {
            Some(wpp) => Some((wpp, self.plan_view_rect())),
            None => self.sheet_model_view(),
        }
    }

    /// A paper layout is in view and the drawing has underlays: some may lie
    /// on its sheet, where they follow the sheet's view.
    pub(in crate::scene) fn sheet_may_hold_underlays(&self) -> bool {
        self.current_layout != "Model" && self.has_underlays()
    }

    /// World units per screen pixel of the current layout's view: the model
    /// camera's in Model, the sheet camera's in a paper layout. `None` until
    /// a render captured real bounds.
    pub(in crate::scene) fn layout_world_per_pixel(&self) -> Option<f32> {
        let v = self.last_world_per_pixel.get();
        (v > 0.0 && v.is_finite()).then_some(v)
    }

    /// World XY rectangle of the model view when it looks straight down
    /// (orthographic plan view); `None` otherwise.
    pub(in crate::scene) fn plan_view_rect(&self) -> Option<[f64; 4]> {
        if self.current_layout != "Model" {
            return None;
        }
        self.layout_plan_view_rect()
    }

    /// World XY rectangle of the current layout's view when it looks straight
    /// down (orthographic plan view) — model space in Model, the sheet in a
    /// paper layout; `None` otherwise.
    pub(in crate::scene) fn layout_plan_view_rect(&self) -> Option<[f64; 4]> {
        Self::plan_rect_of(&self.camera.borrow(), self.view_aspect())
    }

    /// World XY rectangle a camera shows in a view `aspect` wide for its
    /// height, when it looks straight down (orthographic plan view); `None`
    /// otherwise.
    pub(crate) fn plan_rect_of(cam: &view::camera::Camera, aspect: f64) -> Option<[f64; 4]> {
        if !matches!(cam.projection, view::camera::Projection::Orthographic) {
            return None;
        }
        if (cam.rotation * glam::Vec3::NEG_Z).z > -0.9999 {
            return None;
        }
        let h = f64::from(cam.ortho_size());
        let w = h * aspect;
        let right = (cam.rotation * glam::Vec3::X).as_dvec3();
        let up = (cam.rotation * glam::Vec3::Y).as_dvec3();
        let (hx, hy) = (
            (right.x * w).abs() + (up.x * h).abs(),
            (right.y * w).abs() + (up.y * h).abs(),
        );
        let t = cam.target;
        let rect = [t.x - hx, t.y - hy, t.x + hx, t.y + hy];
        rect.iter().all(|v| v.is_finite()).then_some(rect)
    }

    /// Width over height of the view last drawn (1 before the first frame).
    pub(crate) fn view_aspect(&self) -> f64 {
        let aspect = f64::from(self.last_view_aspect.get());
        if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        }
    }

    /// The root drawing's geographic location (GEODATA), when it has one.
    /// Xref and paper-space GEODATA never become the scene CRS by handle order.
    pub(crate) fn geo_data(&self) -> Option<&document::objects::GeoData> {
        crate::catalog::crs::model_space_geo_data(&self.document)
    }

    pub(crate) fn refresh_object_data_cache(&mut self) {
        self.object_data_cache = crate::entities::object_data::build_cache(&self.document);
    }

    pub(crate) fn world_per_pixel(&self) -> Option<f32> {
        if self.current_layout != "Model" {
            return None;
        }
        let v = self.last_world_per_pixel.get();
        if v > 0.0 && v.is_finite() {
            Some(v)
        } else {
            None
        }
    }

    /// Called from the render path with the current widget bounds so the
    /// LOD pixel-size culler knows how big one world unit projects to.
    pub fn set_render_pixel_scale(&self, width_px: f32, height_px: f32) {
        if !width_px.is_finite() || !height_px.is_finite() || height_px <= 0.0 {
            return;
        }
        let cam = self.camera.borrow();
        // Orthographic only. (Perspective varies with depth — we'd want a
        // depth-aware scale per entity. Skipped for now.)
        self.last_view_aspect.set(width_px / height_px);
        let h = cam.ortho_size();
        let world_per_px = (2.0 * h) / height_px;
        if world_per_px.is_finite() && world_per_px > 0.0 {
            self.last_world_per_pixel.set(world_per_px);
        }
    }

    /// Get (or build on miss) the block-definition cache for the current epoch.
    /// Built single-threaded — recursive nested expansion makes parallelization
    /// fiddly and the cache only rebuilds when geometry actually changes.
    pub(crate) fn block_cache_arc_for(
        &self,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Arc<cache::block_cache::BlockCache> {
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let mut key = annotation_scale_handle
            .map(|handle| handle.value())
            .unwrap_or(0);
        for component in bg {
            key = key.rotate_left(13) ^ component.to_bits() as u64;
        }
        key = key.rotate_left(13) ^ all_visible as u64;
        key = key.rotate_left(13) ^ viewport.map(|handle| handle.value()).unwrap_or(0);
        // Loading a linetype does not move the block epoch, and a cache built
        // before the drawing had a complex one kept no path for its children.
        key = key.rotate_left(13) ^ crate::catalog::linetypes::defines_complex_lt(&self.document) as u64;
        let stale = {
            let cache = self.caches.block_defn_cache.borrow();
            match cache.get(&key) {
                Some((epoch, arc)) if *epoch == self.block_epoch => return Arc::clone(arc),
                Some((epoch, arc)) => Some((*epoch, Arc::clone(arc))),
                None => None,
            }
        };
        // Only layers were switched since this cache was built: rebuild the
        // definitions that hold their objects, keep the rest.
        if let (Some((epoch, old)), Some((from, to, names))) =
            (stale, self.block_visibility_edits.as_ref())
        {
            if epoch == *from && *to == self.block_epoch {
                let started = crate::perf::enabled().then(iced::time::Instant::now);
                let solid_snaps = self.block_solid_snaps();
                let mut built = old.with_rebuilt_blocks(
                    &self.document,
                    names,
                    1.0,
                    annotation_scale_handle,
                    all_visible,
                    bg,
                    viewport,
                    &self.draw_depth_map(),
                    &solid_snaps,
                );
                if self.constraint_shading_active() {
                    built.disable_prototypes();
                }
                if started.is_some() {
                    crate::perf_record!(
                        "[perf] blk_cache-rebuild total={:.1}ms changed={}",
                        crate::perf::elapsed_ms(started),
                        names.len(),
                    );
                }
                let arc = Arc::new(built);
                let mut cache = self.caches.block_defn_cache.borrow_mut();
                self.bound_block_defn_cache(&mut cache);
                cache.insert(key, (self.block_epoch, Arc::clone(&arc)));
                return arc;
            }
        }
        // Block definitions are cached at block-local size (annotation scale
        // 1.0). An annotative block scales as ONE unit at the INSERT level, so
        // its internal geometry / text / attributes must NOT be scaled
        // individually because that would apply the same scale twice.
        let solid_snaps = self.block_solid_snaps();
        let mut built = cache::block_cache::BlockCache::build_with_solid_snaps(
            &self.document,
            1.0,
            annotation_scale_handle,
            all_visible,
            bg,
            viewport,
            &self.draw_depth_map(),
            &solid_snaps,
        );
        if self.constraint_shading_active() {
            built.disable_prototypes();
        }
        let arc = Arc::new(built);
        let mut cache = self.caches.block_defn_cache.borrow_mut();
        cache.retain(|_, (epoch, _)| *epoch == self.block_epoch);
        self.bound_block_defn_cache(&mut cache);
        cache.insert(key, (self.block_epoch, Arc::clone(&arc)));
        arc
    }

    /// Snap points of the solids inside block definitions. Parsing their
    /// bodies for face centres took over half a second on a drawing with many
    /// block solids, and every viewport's block cache asked again (#2861), so
    /// the map is kept until the geometry changes.
    pub(in crate::scene) fn block_solid_snaps(&self) -> Arc<BlockSolidSnaps> {
        if let Some((epoch, snaps)) = self.caches.block_solid_snaps_cache.borrow().as_ref() {
            if *epoch == self.geometry_epoch {
                return Arc::clone(snaps);
            }
        }
        let snaps: Arc<BlockSolidSnaps> = Arc::new(
            self.block_meshes
                .keys()
                .map(|handle| (*handle, self.solid_snap_points(*handle)))
                .filter(|(_, points)| !points.is_empty())
                .collect(),
        );
        *self.caches.block_solid_snaps_cache.borrow_mut() =
            Some((self.geometry_epoch, Arc::clone(&snaps)));
        snaps
    }

    /// Each entry holds every block definition built for one viewport, and
    /// layout switches leave the block epoch alone: one entry per viewport
    /// ever shown piled up until memory ran out (#3260). Keep what one frame
    /// draws (model tiles count as 4) plus a little slack.
    pub(in crate::scene) fn bound_block_defn_cache(
        &self,
        cache: &mut HashMap<u64, (u64, Arc<cache::block_cache::BlockCache>)>,
    ) {
        if cache.len() > self.frame_wire_set_count().max(4) + 2 {
            cache.clear();
        }
    }

    /// Install loader-thread geometry into this scene's Model resident cache.
    ///
    /// The target scene has a different epoch from the temporary loader scene,
    /// so only immutable geometry is transferred and re-stamped here. The
    /// optional interaction index is cached against the exact same `Arc`.
    pub fn install_prepared_open_geometry(&self, prepared: PreparedOpenGeometry) {
        // The loader tessellated before the constraints were read, so its
        // wires still hold the dynamic dimensions that must stay hidden.
        if !self.hidden_dynamic_dimensions.is_empty() {
            return;
        }
        *self.caches.dependency_index_cache.borrow_mut() = prepared.dependency_index;
        let block = self.model_space_block_handle();
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        let key = self.resident_wire_key(
            block,
            self.bg_color,
            None,
            scale,
            self.annotation_all_visible(),
            None,
            None,
        );
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        self.caches.resident_wire_sets.borrow_mut().insert(
            key,
            ResidentWireSet {
                estimated_bytes: Self::resident_wire_bytes(&prepared.wires),
                epoch: self.geometry_epoch,
                gen,
                used: std::cell::Cell::new(resident_touch()),
                wires: Arc::clone(&prepared.wires),
                layout: prepared.resident_layout,
            },
        );
        // Adopt the loader thread's tessellation memo so the first edit patches
        // the entities it touched instead of rebuilding every one of them.
        if !prepared.tess_memo.is_empty() {
            *self.caches.resident_tess_memo.borrow_mut() = prepared.tess_memo;
            *self.caches.resident_memo_backing.borrow_mut() = prepared.tess_memo_backing;
            self.resident_tess_guard.set(prepared.tess_guard);
            self.resident_tess_atlas.set(prepared.tess_atlas);
            // The loader tessellates model space, outside any viewport.
            self.resident_tess_complex_lt.set(0);
        }
        // Reuse the loader's draw-order map with this scene's epoch.
        if let Some(mut depths) = prepared.draw_depths {
            depths.epoch = self.geometry_epoch;
            *self.caches.draw_depth_cache.borrow_mut() = Some(depths);
        }
        // Same for the block definitions, stamped with this scene's block
        // epoch: the document has not been touched between the loader
        // finishing and this call, so they describe it exactly.
        if !prepared.block_defns.is_empty() {
            let epoch = self.block_epoch;
            let mut cache = self.caches.block_defn_cache.borrow_mut();
            for (key, defns) in prepared.block_defns {
                cache.insert(key, (epoch, defns));
            }
        }
        if let Some(index) = prepared.interaction_index {
            self.cache_interaction_index(self.geometry_epoch, prepared.wires, index);
        }
        if let Some(index) = prepared.entity_index {
            *self.caches.entity_index_cache.borrow_mut() = Some((self.geometry_epoch, index));
        }
        if let Some((space_key, index)) = prepared.handle_index {
            *self.caches.interaction_handle_index_cache.borrow_mut() =
                Some((self.geometry_epoch, space_key, index));
        }
        if let Some((space_key, hatches)) = prepared.insert_hatches {
            *self.caches.insert_hatch_cache.borrow_mut() = Some((self.geometry_epoch, space_key, hatches));
        }
    }

}
