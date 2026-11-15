//! Retained per-panel scene-projection layer caches + rebuild counters
//! (SCN-011a, the layer-retention half).
//!
//! [`project_scene`](crate::ui::window::plugin_scene::project_scene) is the
//! renderer-free flatten of a [`PanelScene`]; this module is the retention
//! layer on top of it, mirroring `src/ui/overlay.rs`'s `CachedOverlayLayer`
//! precedent exactly:
//!
//! - one [`CachedOverlayLayer`] per panel scene, keyed by a
//!   [`SceneLayerKey`] that carries the scene revision, the paint bounds and
//!   a palette fingerprint — a key EQUAL means the geometry inputs are
//!   unchanged, so the cached layer replays verbatim (`bounds` equality is
//!   what `draw_with_bounds`'s early return needs; revision equality means
//!   the prim list is identical at this pane size; the fingerprint stands in
//!   for palette `PartialEq`, which the key never relies on);
//! - dynamic content stays out: SCN-003 gives no interaction-phase transform
//!   yet (pan/zoom is a later task), so there is no `transient_layer`
//!   equivalent here — per-frame rebuilds reclassify as revision bumps (the
//!   panel-scene's own monotonic revision is the invalidation contract).
//!
//! Counter policy (the "full-rebuild is a defect" instrumentation): probes
//! never mutate counters.
//!
//! - [`SceneProjectionCache::probe_draw_decision`] is the PURE decision
//!   classifier (headless, no side effects);
//! - the counted counterpart [`SceneProjectionCache::decide_counted`] runs
//!   the same classification and then advances the counters; both the
//!   renderer path ([`SceneProjectionCache::draw`]) and the headless seam
//!   ([`SceneProjectionCache::apply_build`]) funnel through it, so the two
//!   paths cannot drift;
//! - tests pair a pure [`probe`](SceneProjectionCache::probe_draw_decision)
//!   against an [`apply_build`](SceneProjectionCache::apply_build)
//!   (a "simulated draw") on identically-primed caches and assert identical
//!   decision sequences — pixel-path correctness itself is live-host work
//!   (milestone 6+), exactly like the overlay tests which assert decisions
//!   and key math headlessly.
//!
//! Scope: layer-level retention (SCN-011a) plus the report-driven per-subtree
//! prim-store hook (SCN-011b). Pending-request cancellation
//! (dropping in-flight patch requests when a surface goes away) is a
//! wire-era concern for a later task and is likewise documented as
//! out of scope here.
//!
//! SCN-011b layered truth (per-subtree retention): the cache optionally
//! carries a [`ScenePrimStore`](crate::ui::window::plugin_scene::ScenePrimStore)
//! that retains the projection SEGMENTED at root-level subtrees and refreshes,
//! per applied patch, ONLY the segments the `AppliedPatchReport` names. The
//! `CachedOverlayLayer` stays the pixel-path carrier: the layer still rebuilds
//! on a revision bump (`layer_builds` — the cheap emit), but when the store is
//! fresh the build paints from the store's segment Arcs instead of re-running
//! the semantic flatten, and the honest cost proof is the store's
//! `StoreCounters.subtree_reprojects` (segments actually re-projected, ==
//! segments touched) vs `layer_builds`: prim re-projection avoided for
//! untouched segments is the win being instrumented. The SCN-012 benchmark
//! consumes these counters.
//!
//! Palette-fingerprint scope decision: [`palette_fingerprint`] hashes EVERY
//! bound slot named in its doc — deliberately WIDER than
//! [`scene_color`](crate::ui/window/plugin_scene::scene_color)'s table
//! actually reads (that table reads only `background.base.text`,
//! `primary.base.color`, `primary.strong.color`, `success/warning/danger
//! base.text` and the passthrough `Rgba` slot). Over-invalidation of the
//! layer bitmap is acceptable at patch cadence (a theme slot change is
//! coarse-grained, and reprojecting costs one prim walk); a stale-colored
//! wrong result after an unbound-slot change is not. SIMPLE > subtle.

#![allow(dead_code)]

use std::collections::HashMap;

use iced::widget::canvas;
use iced::{Color, Rectangle};
use ocs_plugin_api::scene::{
    AppliedPatchReport, PanelScene, SceneError, SceneRevision, SceneSummary, StableId,
};
use std::sync::Arc;

use crate::ui::overlay::CachedOverlayLayer;
use crate::ui::window::plugin_scene::{
    count_prims, emit_prims, project_scene, ScenePrim, ScenePrimStore, StoreCounters,
};

// ── Layer keys + palette fingerprinting ─────────────────────────────────────

/// FNV-1a-64 offset basis and prime, mirrored verbatim from
/// `ocs_plugin_api::version_info::wire_schema` (the crate's established
/// fingerprint hash — same constants so both fingerprints read as one
/// family and the choice stays reviewable in one place).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a-64 prime (see [`FNV_OFFSET_BASIS`]).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Fold `bytes` into an FNV-1a-64 hash state.
fn fnv1a_fold(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Fold one `f32` channel: fixed little-endian byte order. The fingerprint
/// never crosses a wire (it is compared against itself in-process), so the
/// endianness choice is free but pinned by the field-order tests.
fn fnv1a_f32(hash: u64, value: f32) -> u64 {
    fnv1a_fold(hash, &value.to_le_bytes())
}

/// Fold one `Color` in fixed `r, g, b, a` channel order.
fn fnv1a_color(hash: u64, color: &Color) -> u64 {
    let hash = fnv1a_f32(hash, color.r);
    let hash = fnv1a_f32(hash, color.g);
    let hash = fnv1a_f32(hash, color.b);
    fnv1a_f32(hash, color.a)
}

/// Fold one `Pair` in fixed `color, text` order.
fn fnv1a_pair(hash: u64, pair: &iced::theme::palette::Pair) -> u64 {
    let hash = fnv1a_color(hash, &pair.color);
    fnv1a_color(hash, &pair.text)
}

/// Fold one `Swatch` in fixed `base, weak, strong` order (each a `Pair`).
fn fnv1a_swatch(hash: u64, swatch: &iced::theme::palette::Swatch) -> u64 {
    let hash = fnv1a_pair(hash, &swatch.base);
    let hash = fnv1a_pair(hash, &swatch.weak);
    fnv1a_pair(hash, &swatch.strong)
}

/// Deterministic FNV-1a-64 fingerprint of the host theme palette, over the
/// palette's float bytes in a FIXED documented field order:
///
/// 1. `background.base` pair (`{color, text}`),
/// 2. the five swatches in struct order — `primary, secondary, success,
///    warning, danger` — each folded `{base, weak, strong}` x `{color, text}`,
/// 3. `is_dark` (one byte, `0`/`1`).
///
/// Any change to a bound slot therefore yields a different fingerprint, and
/// today's concrete palettes hash to different values already.
///
/// This is deliberately WIDER than
/// [`scene_color`](crate::ui/window/plugin_scene::scene_color)'s table: the
/// projection reads only `background.base.text`, `primary.base.color`,
/// `primary.strong.color`, `success/warning/danger base.text` (plus
/// passthrough `Rgba`). Hashing every bound slot means a theme change in an
/// UNREAD slot (say `secondary.weak.color`) still invalidates the layer —
/// over-invalidation is acceptable at patch cadence, a stale (wrong) panel
/// is not; see the module docs. Background's seven non-base pairs are NOT
/// bound (the projection and the scene tier cannot reach them), which the
/// fingerprint tests pin as the documented boundary.
///
/// The fingerprint is the single palette comparison surface: keys never rely
/// on `Palette: PartialEq` (the vendored type does not promise it and the
/// rewrite rules inside `Pair::new` would make structural equality a moving
/// target).
pub(crate) fn palette_fingerprint(palette: &iced::theme::Palette) -> u64 {
    let hash = fnv1a_pair(FNV_OFFSET_BASIS, &palette.background.base);
    let hash = fnv1a_swatch(hash, &palette.primary);
    let hash = fnv1a_swatch(hash, &palette.secondary);
    let hash = fnv1a_swatch(hash, &palette.success);
    let hash = fnv1a_swatch(hash, &palette.warning);
    let hash = fnv1a_swatch(hash, &palette.danger);
    // is_dark last, as one byte — the only non-float slot in the digest.
    fnv1a_fold(hash, &[u8::from(palette.is_dark)])
}

/// Layer key for one panel-scene projection: a key EQUAL means the geometry
/// inputs are unchanged and the cached layer may replay verbatim.
///
/// - `bounds` equality is what `CachedOverlayLayer`'s
///   `draw_with_bounds` early return needs (a changed pane size is in the
///   key by construction, as in `overlay.rs`'s `DividerLayerKey`);
/// - `revision` equality means the prim stream is identical at this pane
///   size (the panel-scene's revision is the only valid invalidation signal
///   the scene tier offers);
/// - `palette_fingerprint` stands in for palette identity (SIMPLE > subtle:
///   no `PartialEq` reliance — see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneLayerKey {
    /// Scene revision the cached projection was built from.
    pub revision: SceneRevision,
    /// Paint bounds of the layer (frame surface).
    pub bounds: Rectangle,
    /// [`palette_fingerprint`] of the palette the projection resolved
    /// against.
    pub palette_fingerprint: u64,
}

// ── Decision classification ─────────────────────────────────────────────────

/// Why one draw decision was a cache miss (rebuild) rather than a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RebuildReason {
    /// No key has ever been installed on this cache (fresh cache, or the
    /// previous build failed validation and left the layer EMPTY).
    FirstBuild,
    /// The scene revision bumped since the cached build.
    RevisionChanged,
    /// The paint bounds changed since the cached build (either direction —
    /// bounds are part of the key, so shrinking back to a previous size
    /// rebuilds too).
    BoundsChanged,
    /// The palette fingerprint changed since the cached build.
    PaletteChanged,
    /// The layer was explicitly [`SceneProjectionCache::clear`]ed (evicted or
    /// manually dropped) since the last successful build.
    Cleared,
}

/// The decision the draw path made (or would make): replay the cached
/// layer, or rebuild it for a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawDecision {
    /// The cached layer replays (zero path work).
    LayerHit,
    /// Rebuild the layer (the reason advances the matching counters).
    LayerRebuild(RebuildReason),
}

/// Pure classifier for one key against a stored key. Reason precedence on a
/// multi-field miss is fixed and documented: revision, then bounds, then
/// palette fingerprint (reported as the FIRST mismatched field).
fn classify(cached: Option<&SceneLayerKey>, key: &SceneLayerKey, cleared: bool) -> DrawDecision {
    match cached {
        None => {
            if cleared {
                DrawDecision::LayerRebuild(RebuildReason::Cleared)
            } else {
                DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
            }
        }
        Some(old) => {
            if old == key {
                DrawDecision::LayerHit
            } else if old.revision != key.revision {
                DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
            } else if old.bounds != key.bounds {
                DrawDecision::LayerRebuild(RebuildReason::BoundsChanged)
            } else {
                DrawDecision::LayerRebuild(RebuildReason::PaletteChanged)
            }
        }
    }
}

/// Cheap proxy for the path work one build emitted: the sum of polygon
/// vertex counts across the prim list. Text runs and clip boundaries carry
/// no path vertices. Headless walk local to THIS module — `plugin_scene.rs`
/// is outside this task's edit surface, and this avoids cross-task churn
/// (the "renderer-free sink" pattern of `count_prims`).
fn prim_vertex_count(prims: &[ScenePrim]) -> usize {
    prims
        .iter()
        .map(|prim| match prim {
            ScenePrim::Fill { vertices, .. }
            | ScenePrim::Stroke { vertices, .. }
            | ScenePrim::FillStroke { vertices, .. } => vertices.len(),
            ScenePrim::Text { .. } | ScenePrim::ClipPush { .. } | ScenePrim::ClipPop { .. } => 0,
        })
        .sum()
}

// ── Rebuild counters ────────────────────────────────────────────────────────

/// Rebuild instrumentation for one panel scene's projection layer: the
/// numbers a reviewer reads to prove "full rebuilds are the defect, not the
/// norm". All advances happen in the COUNTED decision funnel
/// ([`SceneProjectionCache::decide_counted`]) or in the build outcomes the
/// funnel drives — never in the pure probe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectionCounters {
    /// Cache-miss rebuilds of any reason (including the very first build and
    /// post-`clear` rebuilds — but not `rebuilds_cleared`'s own count, which
    /// the explicit `clear()` call itself advances).
    pub layer_builds: u64,
    /// Rebuilds caused by a revision bump.
    pub rebuilds_revision: u64,
    /// Rebuilds caused by a bounds change (either direction).
    pub rebuilds_bounds: u64,
    /// Rebuilds caused by a palette fingerprint change.
    pub rebuilds_palette: u64,
    /// Explicit `clear()` calls (evict/manual). The rebuild a cleared state
    /// causes is NOT counted here again — the rebuild lands in
    /// `layer_builds`, this counter only measures the explicit call.
    pub rebuilds_cleared: u64,
    /// Draw decisions that reused the layer instead of rebuilding. The name
    /// is the historical `probe` shape of the API surface; semantics are
    /// "decision-funnel hits" (probes NEVER advance it — see the policy doc).
    pub probe_hits: u64,
    /// Prim count of the last committed build (0 when the last build failed
    /// validation, leaving the layer blank).
    pub prims_last_build: usize,
    /// Sum of emitted polygon vertices at the last committed build (the
    /// cheap path-work proxy; see [`prim_vertex_count`]).
    pub path_work_last_build: usize,
    /// `Display` of the last `project_scene` error, if any. An invalid scene
    /// is a defect-capture surface, not a panic: the validated-first
    /// projection renders the panel BLANK and the error surfaces here
    /// instead. Sticky by design ("last" is the honest name): a later
    /// successful build does not erase the evidence that one failed.
    pub last_error: Option<String>,
}

// ── Per-panel retained projection ───────────────────────────────────────────

/// The paint plan one committed rebuild produced: either the classic
/// full flatten (a fresh prim list), or the store's segment lists
/// (Arc-shared, paint order; the report-driven retention products).
/// Both paint with `emit_prims`; the segments variant emits segment-wise
/// — each segment is clip-balanced at root boundaries (the store's v1
/// segmentation rule), so segment-wise emit equals the whole-list emit.
enum PaintPlan {
    /// The classic `project_scene` output.
    Prims(Vec<ScenePrim>),
    /// The store's live segment prim lists in paint order.
    Segments(Vec<Arc<Vec<ScenePrim>>>),
}

/// One retained projection of one panel scene: the identity (`surface_id`),
/// the one cached layer, the counters, and a local mirror of the last
/// successfully-built scene's [`SceneSummary`].
///
/// The summary mirror rides each successful build (the API crate computes it
/// in one walk; imported as-is, no mirror type needed here). It is consumed
/// by later host integration (skip-rebuild heuristics / invalidation log),
/// and is diagnostic: nothing in the draw path keys on it.
pub(crate) struct SceneProjectionCache {
    /// The panel-scene identity this cache serves (the PluginPanel's stable
    /// scene id; today assigned per cache instance by the caller).
    surface_id: StableId,
    /// The one retained layer (the overlay retention precedent, keyed by
    /// [`SceneLayerKey`]).
    layer: CachedOverlayLayer<SceneLayerKey>,
    counters: ProjectionCounters,
    /// Optional per-subtree prim store (SCN-011b): when attached,
    /// `sync_after_patch` refreshes exactly the report-named segments and a
    /// fresh store feeds the layer build (see the module docs' layered
    /// truth). `None` keeps the classic full-flatten build path.
    store: Option<ScenePrimStore>,
    /// Local mirror of the last SUCCESSFUL build's scene summary (see the
    /// struct docs). Reset on a failed build.
    last_summary: Option<SceneSummary>,
    /// `true` between an explicit `clear()` and the next successful build:
    /// turns the next decision's reason into
    /// [`RebuildReason::Cleared`] instead of [`RebuildReason::FirstBuild`].
    cleared: bool,
}

/// Manual `Debug`: the retained layer's geometry is opaque renderer state
/// (`canvas::Cache` in the vendored iced), so only its STORED KEY is
/// printed, exactly as reviewers would want alongside the counters.
impl std::fmt::Debug for SceneProjectionCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = self.layer.key.borrow().clone();
        f.debug_struct("SceneProjectionCache")
            .field("surface_id", &self.surface_id)
            .field("layer_key", &key)
            .field("counters", &self.counters)
            .field("store_attached", &self.store.is_some())
            .field("last_summary", &self.last_summary)
            .field("cleared", &self.cleared)
            .finish()
    }
}

impl SceneProjectionCache {
    /// A fresh cache for `surface_id`: no key, no geometry, counters at
    /// defaults (store detached until `attach_store`).
    pub(crate) fn new(surface_id: StableId) -> Self {
        Self {
            surface_id,
            layer: CachedOverlayLayer::default(),
            counters: ProjectionCounters::default(),
            store: None,
            last_summary: None,
            cleared: false,
        }
    }

    /// The panel-scene identity this cache serves.
    pub(crate) fn surface_id(&self) -> StableId {
        self.surface_id
    }

    /// The counters (read side).
    pub(crate) fn counters(&self) -> &ProjectionCounters {
        &self.counters
    }

    /// The counters (write side, for host integration that must externalize
    /// loss signals into them).
    pub(crate) fn counters_mut(&mut self) -> &mut ProjectionCounters {
        &mut self.counters
    }

    /// The local mirror of the last successful build's scene summary.
    pub(crate) fn last_summary(&self) -> Option<&SceneSummary> {
        self.last_summary.as_ref()
    }

    /// Attach the per-subtree prim store (SCN-011b): from here on
    /// [`Self::sync_after_patch`] refreshes exactly the report-named segments
    /// and a fresh store feeds the layer build (the module docs' layered
    /// truth). Idempotent.
    pub(crate) fn attach_store(&mut self) {
        self.store
            .get_or_insert_with(|| ScenePrimStore::new(self.surface_id));
    }

    /// The attached store's counters (read side); `None` while detached.
    pub(crate) fn store_counters(&self) -> Option<&StoreCounters> {
        self.store.as_ref().map(|store| store.counters())
    }

    /// Full re-provisioning of the attached store (panel snapshot / first
    /// provisioning flows): the whole-tree flatten the report path falls
    /// back to. A no-op while detached. `Err` when the scene fails
    /// validation — the store ends up empty and never-synced (see
    /// `ScenePrimStore::sync_full`).
    pub(crate) fn sync_store_full(
        &mut self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
    ) -> Result<(), SceneError> {
        match &mut self.store {
            Some(store) => store.sync_full(scene, palette),
            None => Ok(()),
        }
    }

    /// The report-driven seam (SCN-011b): refresh the attached store's
    /// segments from the patch report (O(touched) — the store's own counters
    /// prove it), then run the layer's counted decision funnel. The LAYER
    /// still rebuilds on the revision bump (the pixel-path carrier), but the
    /// rebuild paints from the freshly synced segment lists — no semantic
    /// flatten — and reuses the report's post-patch summary (computed once
    /// by the single mutation path, `apply_patch_report`). Returns the same
    /// decision `draw` would make.
    ///
    /// Detached store: the layer decision still runs (the classic path).
    /// A store sync failure (an invalid post-patch scene) leaves the store
    /// empty and never-synced, so the build below falls back to the classic
    /// flatten and records the error exactly like a failed build.
    #[doc(hidden)]
    pub(crate) fn sync_after_patch(
        &mut self,
        scene: &PanelScene,
        report: &AppliedPatchReport,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
    ) -> DrawDecision {
        if let Some(store) = self.store.as_mut() {
            let _ = store.sync_incremental(scene, report, palette);
        }
        let (decision, paint) = self.prepare_build(scene, palette, bounds, Some(&report.summary));
        if let Some((key, _paint)) = paint {
            // Key install AFTER the build commit (overlay discipline), the
            // same post-build step the draw path performs.
            *self.layer.key.borrow_mut() = Some(key);
        }
        decision
    }

    /// Clone of the currently stored layer key, if any.
    pub(crate) fn layer_key(&self) -> Option<SceneLayerKey> {
        self.layer.key.borrow().clone()
    }

    /// Evict the retained layer: the cached geometry is dropped, the stored
    /// key is reset (so the next draw rebuilds with reason
    /// [`RebuildReason::Cleared`]) and [`ProjectionCounters::rebuilds_cleared`]
    /// advances once for THIS call — the eventual rebuild is only counted in
    /// `layer_builds`.
    pub(crate) fn clear(&mut self) {
        *self.layer.key.borrow_mut() = None;
        self.cleared = true;
        self.layer.cache.clear();
        self.counters.rebuilds_cleared += 1;
    }

    /// Pure key math: what the decision for these draw inputs must compare.
    fn key_for(
        &self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
    ) -> SceneLayerKey {
        SceneLayerKey {
            revision: scene.revision,
            bounds,
            palette_fingerprint: palette_fingerprint(palette),
        }
    }

    /// PURE headless equivalent of the draw-path decision: what `draw` WOULD
    /// do for these inputs, read off the stored key — with ZERO side effects
    /// on counters (that is the policy: probes never mutate counters;
    /// counters only advance in the counted funnel both real paths share).
    pub(crate) fn probe_draw_decision(
        &self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
    ) -> DrawDecision {
        let key = self.key_for(scene, palette, bounds);
        let cached = self.layer.key.borrow().clone();
        classify(cached.as_ref(), &key, self.cleared)
    }

    /// COUNTED decision funnel (the only counter-advancing classifier): same
    /// classification as [`Self::probe_draw_decision`] by construction (the
    /// shared [`classify`]), then the decision's counters. Both real paths —
    /// [`Self::draw`] (renderer) and [`Self::apply_build`] (headless seam) —
    /// funnel through here, so their counter outcomes cannot drift.
    fn decide_counted(&mut self, key: &SceneLayerKey) -> DrawDecision {
        // Scoped clone: the RefCell borrow must end before counter mutation.
        let cached = self.layer.key.borrow().clone();
        let decision = classify(cached.as_ref(), key, self.cleared);
        match decision {
            DrawDecision::LayerHit => self.counters.probe_hits += 1,
            DrawDecision::LayerRebuild(reason) => {
                self.counters.layer_builds += 1;
                match reason {
                    RebuildReason::RevisionChanged => self.counters.rebuilds_revision += 1,
                    RebuildReason::BoundsChanged => self.counters.rebuilds_bounds += 1,
                    RebuildReason::PaletteChanged => self.counters.rebuilds_palette += 1,
                    // A fresh cache's first build IS `layer_builds`' job; a
                    // post-`clear` rebuild likewise. The `clear()` call
                    // itself already advanced `rebuilds_cleared`.
                    RebuildReason::FirstBuild | RebuildReason::Cleared => {}
                }
            }
        }
        decision
    }

    /// Record one committed build's outcome exactly as the draw paths commit
    /// it (shared by the renderer, headless, and store-fed paths — the only
    /// shared MUTATION of the outcome stats). `summary_override` is the
    /// report's post-patch [`SceneSummary`] when the build rides a patch
    /// report (computed once by the single mutation path and reused as-is);
    /// `None` recomputes the scene's own bounded walk, exactly like the
    /// pre-store builds did.
    fn record_committed(
        &mut self,
        prims_len: usize,
        path_work: usize,
        summary_override: Option<&SceneSummary>,
        scene: &PanelScene,
    ) {
        self.counters.prims_last_build = prims_len;
        self.counters.path_work_last_build = path_work;
        self.last_summary = Some(match summary_override {
            Some(summary) => summary.clone(),
            None => scene.scene_summary(),
        });
        // The `cleared` flag is spent the moment a build actually commits.
        self.cleared = false;
    }

    /// Record a FAILED build outcome exactly as the draw path commits it:
    /// the last error is captured as `Display` text, the prim/path-work
    /// counters are zeroed, the summary mirror is dropped, and the layer
    /// stays EMPTY (no key installed), so the next draw rebuilds. The blank
    /// render is the validated-first contract: an invalid scene NEVER panics
    /// — it paints nothing and the failure surfaces in
    /// [`ProjectionCounters::last_error`].
    fn record_error(&mut self, error: &SceneError) {
        self.counters.last_error = Some(error.to_string());
        self.counters.prims_last_build = 0;
        self.counters.path_work_last_build = 0;
        self.last_summary = None;
    }

    /// Shared first half of [`Self::draw`], [`Self::apply_build`], and
    /// [`Self::sync_after_patch`]: key math, the counted decision, the
    /// projection on a rebuild, and outcome recording (stats; the key install
    /// is returned to the caller so it lands after the frame work, matching
    /// the overlay's key-write discipline). Returns the decision plus, on a
    /// rebuild, the [`PaintPlan`] to paint together with the key to install
    /// (`None` both on hits — replay — and on failed rebuilds — blank layer,
    /// key stays unset).
    ///
    /// Paint-source rule (the SCN-011b layered truth): the decision and its
    /// counters are exactly the SCN-011a key math, but when the attached
    /// store is FRESH at this scene revision + palette fingerprint the build
    /// paints from the store's segment lists instead of re-running the
    /// semantic flatten — the report-driven sync already refreshed exactly
    /// the touched segments, and [`Self::record_committed`] reuses the
    /// report's post-patch summary instead of re-walking the scene.
    fn prepare_build(
        &mut self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
        report_summary: Option<&SceneSummary>,
    ) -> (DrawDecision, Option<(SceneLayerKey, PaintPlan)>) {
        let key = self.key_for(scene, palette, bounds);
        let decision = self.decide_counted(&key);
        if matches!(decision, DrawDecision::LayerHit) {
            return (decision, None);
        }
        // Owned paint plan: the store borrow ends here, before the outcome
        // recording below can take the cache mutably.
        let from_store = self
            .store
            .as_ref()
            .filter(|store| store.is_fresh(scene.revision, key.palette_fingerprint))
            .map(|store| store.live_arcs());
        let rebuilt = match from_store {
            Some(parts) => {
                let prims_len: usize = parts.iter().map(|part| part.len()).sum();
                let path_work: usize = parts.iter().map(|part| prim_vertex_count(part)).sum();
                self.record_committed(prims_len, path_work, report_summary, scene);
                Some((key, PaintPlan::Segments(parts)))
            }
            None => match project_scene(scene, palette, bounds.size()) {
                Ok(prims) => {
                    self.record_committed(
                        count_prims(&prims),
                        prim_vertex_count(&prims),
                        report_summary,
                        scene,
                    );
                    Some((key, PaintPlan::Prims(prims)))
                }
                Err(error) => {
                    self.record_error(&error);
                    None
                }
            },
        };
        (decision, rebuilt)
    }

    /// Draw the current revision of `scene` into the panel's layer,
    /// rebuilding only on a key miss (revision bumped, bounds changed, or
    /// palette fingerprint changed). On a hit the cached geometry REPLAYS —
    /// the no-op closure short-circuits inside iced's
    /// `draw_with_bounds` and `project_scene` never runs.
    ///
    /// The projection's `Err` case emits NOTHING (the layer paints blank)
    /// and records the failure in
    /// [`ProjectionCounters::last_error`] — an invalid scene never panics;
    /// it renders a blank panel and the counters surface the defect.
    pub(crate) fn draw(
        &mut self,
        renderer: &iced::Renderer,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
    ) -> canvas::Geometry {
        let (decision, paint) = self.prepare_build(scene, palette, bounds, None);
        match decision {
            DrawDecision::LayerHit => {}
            DrawDecision::LayerRebuild(_) => {
                // Clear FIRST, exactly like `CachedOverlayLayer::draw`: the
                // closure below must run even when the bounds happen to
                // match the previously cached frame (iced short-circuits a
                // non-cleared cache on a bounds match).
                self.layer.cache.clear();
            }
        }
        match paint {
            Some((key, PaintPlan::Prims(prims))) => {
                let geometry = self
                    .layer
                    .cache
                    .draw_with_bounds(renderer, bounds, move |frame| {
                        emit_prims(frame, &prims);
                    });
                // Key install AFTER the build commit (overlay discipline).
                *self.layer.key.borrow_mut() = Some(key);
                geometry
            }
            // Store-fed build: segment-wise emit over the (possibly
            // partially refreshed) segment Arcs — the pixel path identical
            // to painting the concatenated list; no semantic flatten ran.
            Some((key, PaintPlan::Segments(parts))) => {
                let geometry = self
                    .layer
                    .cache
                    .draw_with_bounds(renderer, bounds, move |frame| {
                        for part in parts {
                            emit_prims(frame, &part);
                        }
                    });
                // Key install AFTER the build commit (overlay discipline).
                *self.layer.key.borrow_mut() = Some(key);
                geometry
            }
            // Hits replay the cached geometry; failed rebuilds paint the
            // blank (empty) frame the invalid-scene contract promises.
            None => self
                .layer
                .cache
                .draw_with_bounds(renderer, bounds, |_frame| {}),
        }
    }

    /// Headless mirror of [`Self::draw`] minus every renderer-requiring step
    /// (the bench/test seam, mirroring `overlay.rs`'s `store_pane_key`):
    /// the same key math, the same counted decision funnel
    /// ([`Self::decide_counted`]), the same projection run and the same
    /// outcome recording as `draw` — and the same key install — so tests can
    /// prove decision/counter parity with the renderer path WITHOUT
    /// constructing an `iced::Renderer` (the pixel path itself is live-host
    /// / milestone-6 concern). Returns the same decision `draw` would make.
    #[doc(hidden)]
    pub(crate) fn apply_build(
        &mut self,
        scene: &PanelScene,
        palette: &iced::theme::Palette,
        bounds: Rectangle,
    ) -> DrawDecision {
        let (decision, paint) = self.prepare_build(scene, palette, bounds, None);
        if let Some((key, _paint)) = paint {
            // Prims recorded in `record_committed`, then dropped: there is no
            // renderer frame to paint them onto. The install mirrors the
            // draw path's post-build step.
            *self.layer.key.borrow_mut() = Some(key);
        }
        decision
    }
}

// ── Multi-panel independence ────────────────────────────────────────────────

/// Aggregate release statistics for a [`SceneProjectionCacheSet`].
///
/// Dropping a surface (tab/plugin close) releases its whole retained layer
/// AND its counters with it — the release is recorded as ONE number here,
/// not as a per-surface counter log (nothing downstream reads the dropped
/// surfaces' counters; a per-surface audit trail would contradict the
/// release). Pending-request cancellation when a surface goes away is a
/// wire-era concern and lands with the per-subtree retention task's wire
/// surface (documented as out of scope for SCN-011a).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SceneCacheSetStats {
    /// Surfaces released by [`SceneProjectionCacheSet::retain_alive`]
    /// (cumulative).
    pub surfaces_released: u64,
}

/// Per-panel independent set of retained projection caches: one
/// [`SceneProjectionCache`] per panel scene, keyed by its surface id —
/// the scene-side analogue of the overlay's per-pane `GridCanvasState`
/// (a change to one surface's inputs never rebuilds another's layer).
#[derive(Debug, Default)]
pub(crate) struct SceneProjectionCacheSet {
    by_surface: HashMap<StableId, SceneProjectionCache>,
    stats: SceneCacheSetStats,
}

impl SceneProjectionCacheSet {
    /// An empty set.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The cache of `surface_id`, created (empty, on a FirstBuild decision)
    /// if absent.
    pub(crate) fn get_or_insert(&mut self, surface_id: StableId) -> &mut SceneProjectionCache {
        self.by_surface
            .entry(surface_id)
            .or_insert_with(|| SceneProjectionCache::new(surface_id))
    }

    /// Read only, no insert.
    pub(crate) fn get(&self, surface_id: StableId) -> Option<&SceneProjectionCache> {
        self.by_surface.get(&surface_id)
    }

    /// `true` when a cache is retained for `surface_id`.
    pub(crate) fn contains(&self, surface_id: StableId) -> bool {
        self.by_surface.contains_key(&surface_id)
    }

    /// Release the caches of dropped surfaces: `live` names every surface
    /// that still exists (tab/plugin close drops the rest). Each release
    /// drops the surface's layer + counters wholesale and advances
    /// [`SceneCacheSetStats::surfaces_released`] once per surface; calling
    /// `retain_alive` twice with the same `live` list is a no-op.
    pub(crate) fn retain_alive(&mut self, live: &[StableId]) {
        let before = self.by_surface.len();
        self.by_surface.retain(|surface_id, _| live.contains(surface_id));
        self.stats.surfaces_released += (before - self.by_surface.len()) as u64;
    }

    /// The set's aggregate stats.
    pub(crate) fn stats(&self) -> SceneCacheSetStats {
        self.stats
    }
}

// ── Tests (headless: decisions, keys, counters — no Renderer) ───────────────
//
// Same discipline as the overlay's headless tests: decision+key math is
// fully assertable without a renderer; the pixel path stays live-host work.
// No sleeps, no randomness, no iced::Renderer construction anywhere.

#[cfg(test)]
mod tests {
    use super::*;
    use iced::theme::palette::{Background, Pair, Swatch};
    use ocs_plugin_api::scene::{SceneElement, SceneColor, ThemeColor, Vec2};

    // -- fixtures ------------------------------------------------------------

    fn tint(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgba8(r, g, b, 1.0)
    }

    fn pair(color: Color, text: Color) -> Pair {
        Pair { color, text }
    }

    fn null_pair() -> Pair {
        Pair {
            color: Color::TRANSPARENT,
            text: Color::TRANSPARENT,
        }
    }

    /// The empty digest's palette: every slot transparent-black, `is_dark`
    /// false. Its fingerprint is the FNV offset basis propagated through the
    /// fixed field order (non-zero by construction; pinned below).
    fn zero_palette() -> iced::theme::Palette {
        iced::theme::Palette {
            background: Background {
                base: null_pair(),
                weakest: null_pair(),
                weaker: null_pair(),
                weak: null_pair(),
                neutral: null_pair(),
                strong: null_pair(),
                stronger: null_pair(),
                strongest: null_pair(),
            },
            primary: Swatch {
                base: null_pair(),
                weak: null_pair(),
                strong: null_pair(),
            },
            secondary: Swatch {
                base: null_pair(),
                weak: null_pair(),
                strong: null_pair(),
            },
            success: Swatch {
                base: null_pair(),
                weak: null_pair(),
                strong: null_pair(),
            },
            warning: Swatch {
                base: null_pair(),
                weak: null_pair(),
                strong: null_pair(),
            },
            danger: Swatch {
                base: null_pair(),
                weak: null_pair(),
                strong: null_pair(),
            },
            is_dark: false,
        }
    }

    /// A struct-literal palette with distinctive channel values in the bound
    /// slots (the same construction discipline as `plugin_scene.rs`'s
    /// `pinned_palette`), so slot flips are provable one at a time.
    fn distinct_palette(is_dark: bool) -> iced::theme::Palette {
        let mut palette = zero_palette();
        // background.base: {20, 21}
        palette.background.base.color = tint(20, 0, 0);
        palette.background.base.text = tint(21, 0, 0);
        // primary.base {30, 31}, primary.strong {40, 41}
        palette.primary.base = pair(tint(30, 0, 0), tint(31, 0, 0));
        palette.primary.strong = pair(tint(40, 0, 0), tint(41, 0, 0));
        // secondary.base {50, 51}
        palette.secondary.base = pair(tint(50, 0, 0), tint(51, 0, 0));
        // success.base {60, 61}
        palette.success.base = pair(tint(60, 0, 0), tint(61, 0, 0));
        // warning.base {70, 71}
        palette.warning.base = pair(tint(70, 0, 0), tint(71, 0, 0));
        // danger.base {80, 81}
        palette.danger.base = pair(tint(80, 0, 0), tint(81, 0, 0));
        palette.is_dark = is_dark;
        palette
    }

    fn v2(x: f64, y: f64) -> Vec2 {
        Vec2([x, y])
    }

    /// A fill-only rectangle (one prim per element on projection).
    fn filled_rect(id: u64, x: f64) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            actions: Vec::new(),
            drag: None,
            pos: v2(x, 0.0),
            size: v2(10.0, 8.0),
            fill: Some(SceneColor::Theme(ThemeColor::Normal)),
            stroke: None,
        }
    }

    /// A valid scene of `n_rects` distinct rects at `revision`.
    fn scene_at(revision: u64, n_rects: usize) -> PanelScene {
        PanelScene {
            revision: SceneRevision(revision),
            elements: (0..n_rects as u64).map(|id| filled_rect(id, id as f64)).collect(),
        }
    }

    /// An INVALID scene: two elements sharing one stable id
    /// (`SceneError::DuplicateId` on the validator the projection re-runs).
    fn invalid_scene_at(revision: u64) -> PanelScene {
        PanelScene {
            revision: SceneRevision(revision),
            elements: vec![
                SceneElement::Rect {
                    id: StableId(7),
                    name: None,
                    actions: Vec::new(),
                    drag: None,
                    pos: v2(0.0, 0.0),
                    size: v2(10.0, 8.0),
                    fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                    stroke: None,
                },
                SceneElement::Rect {
                    id: StableId(7),
                    name: None,
                    actions: Vec::new(),
                    drag: None,
                    pos: v2(20.0, 0.0),
                    size: v2(10.0, 8.0),
                    fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                    stroke: None,
                },
            ],
        }
    }

    fn fixture_bounds() -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 160.0,
        }
    }

    fn bounds_at(x: f32, y: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width: 200.0,
            height: 160.0,
        }
    }

    // -- fingerprint test scaffolding -----------------------------------------

    /// Identifies ONE hash-bound slot of [`palette_fingerprint`], in the
    /// digest's FIELD ORDER (this enumeration IS the documented order pin).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum BoundSlot {
        BackgroundBaseColor,
        BackgroundBaseText,
        /// Swatch index in field order (0=primary, 1=secondary, 2=success,
        /// 3=warning, 4=danger), pair (0=base, 1=weak, 2=strong), and
        /// `true` for the pair's `color` half / `false` for its `text` half.
        SwatchSlot { swatch: usize, pair: usize, color: bool },
        IsDark,
    }

    /// Every hash-bound slot (2 + 5x3x2 + 1), in digest field order.
    fn all_bound_slots() -> Vec<BoundSlot> {
        let mut slots = vec![BoundSlot::BackgroundBaseColor, BoundSlot::BackgroundBaseText];
        for swatch in 0..5usize {
            for pair in 0..3usize {
                slots.push(BoundSlot::SwatchSlot {
                    swatch,
                    pair,
                    color: true,
                });
                slots.push(BoundSlot::SwatchSlot {
                    swatch,
                    pair,
                    color: false,
                });
            }
        }
        slots.push(BoundSlot::IsDark);
        slots
    }

    fn swatch_by_index(palette: &mut iced::theme::Palette, index: usize) -> &mut Swatch {
        match index {
            0 => &mut palette.primary,
            1 => &mut palette.secondary,
            2 => &mut palette.success,
            3 => &mut palette.warning,
            4 => &mut palette.danger,
            other => unreachable!("five swatches only, got {other}"),
        }
    }

    /// Paint one slot with `mark`.
    fn paint_slot(palette: &mut iced::theme::Palette, slot: BoundSlot, mark: Color) {
        match slot {
            BoundSlot::BackgroundBaseColor => palette.background.base.color = mark,
            BoundSlot::BackgroundBaseText => palette.background.base.text = mark,
            BoundSlot::SwatchSlot {
                swatch,
                pair,
                color,
            } => {
                let swatch = swatch_by_index(palette, swatch);
                let slot_pair = match pair {
                    0 => &mut swatch.base,
                    1 => &mut swatch.weak,
                    2 => &mut swatch.strong,
                    other => unreachable!("three pairs only, got {other}"),
                };
                if color {
                    slot_pair.color = mark;
                } else {
                    slot_pair.text = mark;
                }
            }
            BoundSlot::IsDark => palette.is_dark = !palette.is_dark,
        }
    }

    // -- 1. decisions on a fresh cache ---------------------------------------

    #[test]
    fn first_draw_decision_is_first_build() {
        let palette = distinct_palette(false);
        let scene = scene_at(1, 2);
        let cache = SceneProjectionCache::new(StableId(42));
        // The pure probe classifies without any counter effect.
        assert_eq!(
            cache.probe_draw_decision(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild),
            "a fresh cache has no stored key: the first decision is FirstBuild"
        );
        // Policy: probes NEVER mutate counters; the bookkeeping is exactly
        // the pre-build default.
        assert_eq!(
            cache.counters(),
            &ProjectionCounters::default(),
            "probe must leave counters untouched"
        );
        // An explicitly cleared cache reports Cleared, not FirstBuild (the
        // documented distinction between "never built" and "evicted").
        let mut cleared = SceneProjectionCache::new(StableId(42));
        cleared.clear();
        assert_eq!(
            cleared.probe_draw_decision(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::Cleared),
            "an explicit clear must be visible in the next decision's reason"
        );
        assert_eq!(cleared.counters().rebuilds_cleared, 1);
    }

    // -- 2. same inputs replay the cached layer ------------------------------

    #[test]
    fn same_inputs_replay_cached_layer() {
        let palette = distinct_palette(false);
        let scene = scene_at(1, 2);
        let mut cache = SceneProjectionCache::new(StableId(1));
        // Parity policy in action: the pure probe and the counted funnel
        // agree on the fresh cache (both say FirstBuild).
        assert_eq!(
            cache.probe_draw_decision(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(
            cache.apply_build(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild),
            "simulated draw: the counted funnel returned the same decision"
        );
        // Same inputs again: the cache hits; probe and funnel agree again.
        assert_eq!(
            cache.probe_draw_decision(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
        assert_eq!(
            cache.apply_build(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
        // Counters: exactly one build, exactly one decision-funnel hit, and
        // the pure probes in between did NOT advance anything.
        let counters = cache.counters();
        assert_eq!(counters.layer_builds, 1);
        assert_eq!(counters.probe_hits, 1, "only the counted hit advanced");
        assert_eq!(counters.rebuilds_revision, 0);
        assert_eq!(counters.rebuilds_bounds, 0);
        assert_eq!(counters.rebuilds_palette, 0);
        assert_eq!(counters.rebuilds_cleared, 0);
        assert_eq!(counters.prims_last_build, 2);
        assert_eq!(counters.path_work_last_build, 10, "two closed rect rings of 5 vertices");
        assert_eq!(counters.last_error, None);
    }

    // -- 3. revision bumps rebuild exactly once ------------------------------

    #[test]
    fn revision_bump_rebuilds_once() {
        let palette = distinct_palette(false);
        let mut cache = SceneProjectionCache::new(StableId(2));
        // Three consecutive draws of the SAME revision.
        let scene1 = scene_at(1, 3);
        assert_eq!(
            cache.apply_build(&scene1, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(
            cache.apply_build(&scene1, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
        assert_eq!(
            cache.apply_build(&scene1, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
        // The pure probe already predicted the bumped revision's miss before
        // the actual build consumes it.
        let scene2 = scene_at(2, 4);
        assert_eq!(
            cache.probe_draw_decision(&scene2, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
        );
        // The bumped revision rebuilds exactly once.
        assert_eq!(
            cache.apply_build(&scene2, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
        );
        assert_eq!(cache.counters().layer_builds, 2);
        assert_eq!(cache.counters().rebuilds_revision, 1);
        assert_eq!(cache.counters().probe_hits, 2, "two same-revision hits");
        // prims_last_build reflects the NEW build (4 rects, 1:1 prims).
        assert_eq!(cache.counters().prims_last_build, 4);
    }

    // -- 4. bounds changes rebuild (both directions) --------------------------

    #[test]
    fn bounds_change_rebuilds() {
        let palette = distinct_palette(false);
        let scene = scene_at(1, 2);
        let mut cache = SceneProjectionCache::new(StableId(3));
        assert_eq!(
            cache.apply_build(&scene, &palette, bounds_at(0.0, 0.0)),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(
            cache.apply_build(&scene, &palette, bounds_at(8.0, 0.0)),
            DrawDecision::LayerRebuild(RebuildReason::BoundsChanged),
            "bounds are part of the layer key (draw_with_bounds's early return)"
        );
        assert_eq!(cache.counters().rebuilds_bounds, 1);
        assert_eq!(cache.counters().layer_builds, 2);
    }

    #[test]
    fn bounds_back_rebuilds_again() {
        let palette = distinct_palette(false);
        let scene = scene_at(1, 2);
        let mut cache = SceneProjectionCache::new(StableId(4));
        assert_eq!(
            cache.apply_build(&scene, &palette, bounds_at(0.0, 0.0)),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(
            cache.apply_build(&scene, &palette, bounds_at(8.0, 0.0)),
            DrawDecision::LayerRebuild(RebuildReason::BoundsChanged)
        );
        // Shrinking BACK to the original bounds is a miss too: bounds are in
        // the key, so there is no shape-only memo of the first build.
        assert_eq!(
            cache.apply_build(&scene, &palette, bounds_at(0.0, 0.0)),
            DrawDecision::LayerRebuild(RebuildReason::BoundsChanged)
        );
        assert_eq!(cache.counters().rebuilds_bounds, 2);
        assert_eq!(cache.counters().layer_builds, 3);
    }

    // -- 5. palette changes invalidate the layer ------------------------------

    #[test]
    fn palette_change_invalidates_layer() {
        let scene = scene_at(1, 2);
        let mut cache = SceneProjectionCache::new(StableId(5));
        assert_eq!(
            cache.apply_build(&scene, &distinct_palette(false), fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        // The same palette again: hit (the fingerprint is the comparison
        // surface; no Palette::PartialEq reliance).
        assert_eq!(
            cache.apply_build(&scene, &distinct_palette(false), fixture_bounds()),
            DrawDecision::LayerHit
        );
        // A BOUND slot change: the fingerprint moves, the key misses, and
        // the reason is the fingerprint's own counter.
        let mut darkened = distinct_palette(false);
        darkened.background.base.text = tint(99, 0, 0);
        assert_ne!(
            palette_fingerprint(&distinct_palette(false)),
            palette_fingerprint(&darkened),
            "bound-slot change must move the fingerprint"
        );
        assert_eq!(
            cache.apply_build(&scene, &darkened, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::PaletteChanged)
        );
        assert_eq!(cache.counters().rebuilds_palette, 1);
        assert_eq!(cache.counters().probe_hits, 1);
        assert_eq!(cache.counters().layer_builds, 2);
    }

    // -- 6. invalid scene: blank layer + recorded error -----------------------

    #[test]
    fn invalid_scene_renders_blank_and_records_error() {
        let palette = distinct_palette(false);
        let bad = invalid_scene_at(1);
        let mut cache = SceneProjectionCache::new(StableId(6));
        assert_eq!(
            cache.apply_build(&bad, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild),
            "the build attempt is still counted (a build ran and failed)"
        );
        // Defect-capture surface, not a panic: the error is recorded as
        // Display text, the counters describe the BLANK outcome.
        let counters = cache.counters();
        let error = counters.last_error.as_ref().expect("error must be captured");
        assert!(!error.is_empty(), "the Display text is the capture surface");
        assert_eq!(counters.prims_last_build, 0, "nothing was emitted");
        assert_eq!(counters.path_work_last_build, 0);
        assert!(
            cache.last_summary().is_none(),
            "a failed build leaves no summary mirror"
        );
        // The layer stays EMPTY (no key installed): the next draw rebuilds.
        let fixed = scene_at(2, 1);
        assert_eq!(
            cache.probe_draw_decision(&fixed, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild),
            "no key was installed, so the next decision is a fresh build"
        );
        assert_eq!(
            cache.apply_build(&fixed, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        // A later successful build leaves the last-error evidence in place
        // (documented stickiness: "last" is the honest name) and repopulates
        // the outcome counters.
        assert!(cache.counters().last_error.is_some());
        assert_eq!(cache.counters().prims_last_build, 1);
        // The REAL draw paints the blank frame for this case; the recording
        // half proven here is exactly what that branch calls.
    }

    // -- 7. multi-surface independence (the §16 scene-side acceptance) --------

    #[test]
    fn multi_surfaces_are_independent() {
        let palette = distinct_palette(false);
        let mut set = SceneProjectionCacheSet::new();
        let surface_a = StableId(101);
        let surface_b = StableId(202);
        let scene1 = scene_at(1, 2);
        // Both surfaces absorb their own first build. Borrow discipline: a
        // `&mut` borrow of the set covers the whole map, so surface work is
        // always sequential here — the INDEPENDENCE being tested is decision
        // isolation, which draws of A and B never need simultaneously.
        {
            let decision_a = set.get_or_insert(surface_a).apply_build(&scene1, &palette, fixture_bounds());
            assert_eq!(
                decision_a,
                DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
            );
            let decision_b = set.get_or_insert(surface_b).apply_build(&scene1, &palette, fixture_bounds());
            assert_eq!(
                decision_b,
                DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
            );
        }
        // A patch that bumps ONLY surface A's revision (same bounds, same
        // palette): the host feeds each panel its OWN scene, so B is
        // re-probed at B's own unchanged inputs.
        let scene2 = scene_at(2, 3);
        let decision_a = set
            .get_or_insert(surface_a)
            .apply_build(&scene2, &palette, fixture_bounds());
        assert_eq!(
            decision_a,
            DrawDecision::LayerRebuild(RebuildReason::RevisionChanged),
            "A's own bump is exactly a RevisionChanged rebuild"
        );
        // B stayed at ITS scene (still the old revision): no global key or
        // counter pollution may turn B's decision into a rebuild.
        let decision_b = set
            .get_or_insert(surface_b)
            .probe_draw_decision(&scene1, &palette, fixture_bounds());
        assert_eq!(
            decision_b,
            DrawDecision::LayerHit,
            "B's inputs are untouched: its layer replays"
        );
        // And the counters agree: A's revision counter advanced, B's didn't.
        let a_counters = set.get_or_insert(surface_a).counters().clone();
        let b_counters = set.get_or_insert(surface_b).counters().clone();
        assert_eq!(a_counters.rebuilds_revision, 1);
        assert_eq!(a_counters.layer_builds, 2);
        assert_eq!(a_counters.prims_last_build, 3);
        assert_eq!(b_counters.rebuilds_revision, 0);
        assert_eq!(b_counters.layer_builds, 1, "B never rebuilt past its first build");
        assert_eq!(b_counters.prims_last_build, 2, "B's first build was 2 rects");
    }

    // -- 8. retain_alive releases dropped surfaces ----------------------------

    #[test]
    fn retain_alive_releases_dropped_surfaces() {
        let palette = distinct_palette(false);
        let scene = scene_at(1, 1);
        let mut set = SceneProjectionCacheSet::new();
        let surface_a = StableId(3);
        let surface_b = StableId(4);
        set.get_or_insert(surface_a).apply_build(&scene, &palette, fixture_bounds());
        set.get_or_insert(surface_b).apply_build(&scene, &palette, fixture_bounds());

        // The dropped surface's layer + counters release wholesale; the
        // release count is recorded exactly once per dropped surface.
        set.retain_alive(&[surface_b]);
        assert_eq!(set.stats().surfaces_released, 1);
        assert!(!set.contains(surface_a));
        assert!(set.contains(surface_b));
        // A no-op retain does not advance the recorded release count.
        set.retain_alive(&[surface_b]);
        assert_eq!(set.stats().surfaces_released, 1);
        // B's cache survived the release with its state intact (the live
        // surface keeps counting in place).
        let decision_b = set.get_or_insert(surface_b).apply_build(&scene, &palette, fixture_bounds());
        assert_eq!(decision_b, DrawDecision::LayerHit);
        assert_eq!(set.get_or_insert(surface_b).counters().layer_builds, 1);
        // A released surface that comes back is a NEW default cache.
        let decision_a = set.get_or_insert(surface_a).probe_draw_decision(&scene, &palette, fixture_bounds());
        assert_eq!(
            decision_a,
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild),
            "released state is gone; a returning surface starts clean"
        );
        assert_eq!(set.get_or_insert(surface_a).counters().layer_builds, 0);
        assert_eq!(set.stats().surfaces_released, 1);
    }

    // -- 9. the fingerprint is complete, stable, and order-pinned -------------

    #[test]
    fn palette_fingerprint_is_complete_and_stable() {
        // Stability: identical palettes hash identically.
        assert_eq!(
            palette_fingerprint(&distinct_palette(false)),
            palette_fingerprint(&distinct_palette(false)),
            "the fingerprint must be deterministic for identical inputs"
        );

        // The all-baseline palette still hashes NON-ZERO (the FNV offset
        // basis survives an empty digest).
        assert_ne!(palette_fingerprint(&zero_palette()), 0);

        // COMPLETENESS + FIELD-ORDER PIN: flipping each bound slot in turn
        // (to the same mark value) yields fingerprints that (a) differ from
        // the baseline and (b) are all pairwise distinct — slot position
        // matters, not just the value's bytes.
        let baseline = distinct_palette(false);
        let baseline_fp = palette_fingerprint(&baseline);
        let mark = tint(240, 0, 0);
        let mut flip_fps = Vec::new();
        for slot in all_bound_slots() {
            let mut flipped = distinct_palette(false);
            paint_slot(&mut flipped, slot, mark);
            let fingerprint = palette_fingerprint(&flipped);
            assert_ne!(
                fingerprint, baseline_fp,
                "flipping bound slot {slot:?} must move the fingerprint"
            );
            flip_fps.push(fingerprint);
        }
        for (i, &fp) in flip_fps.iter().enumerate() {
            for &other in &flip_fps[i + 1..] {
                assert_ne!(
                    fp, other,
                    "the same mark in different bound slots must hash differently (field-order pin)"
                );
            }
        }

        // FIELD-ORDER PIN, shape level: swapping the SAME two values across
        // the base pair's two slots breaks equality.
        let mut swapped_a = distinct_palette(false);
        swapped_a.background.base.color = tint(150, 0, 0);
        swapped_a.background.base.text = tint(151, 0, 0);
        let mut swapped_b = distinct_palette(false);
        swapped_b.background.base.color = tint(151, 0, 0);
        swapped_b.background.base.text = tint(150, 0, 0);
        assert_ne!(
            palette_fingerprint(&swapped_a),
            palette_fingerprint(&swapped_b),
            "field order is part of the digest, not a commutative mix"
        );

        // CHANNEL-ORDER PIN: within one color, swapped r/g channel bytes are
        // a different byte stream.
        let mut rg_a = distinct_palette(false);
        rg_a.primary.base.color = Color::from_rgba8(1, 2, 0, 1.0);
        let mut rg_b = distinct_palette(false);
        rg_b.primary.base.color = Color::from_rgba8(2, 1, 0, 1.0);
        assert_ne!(
            palette_fingerprint(&rg_a),
            palette_fingerprint(&rg_b),
            "channels are hashed in fixed r,g,b,a little-endian order"
        );

        // DOCUMENTED BOUNDARY: background's non-base pairs are NOT bound —
        // the projection cannot reach them, so a change there does not
        // invalidate (over-invalidation was accepted only where simple).
        let mut unbound = distinct_palette(false);
        unbound.background.stronger.color = mark;
        assert_eq!(
            palette_fingerprint(&unbound),
            baseline_fp,
            "unbound slot flips deliberately keep the fingerprint (documented boundary)"
        );
    }

    // -- 10. the stored key's fingerprint equals the recomputation ------------

    #[test]
    fn fingerprint_matches_cached_equality() {
        let palette = distinct_palette(false);
        let mut cache = SceneProjectionCache::new(StableId(9));
        let scene = scene_at(1, 2);
        cache.apply_build(&scene, &palette, fixture_bounds());
        let footprint = palette_fingerprint(&palette);
        let stored = cache.layer_key().expect("successful build installed a key");
        assert_eq!(stored.palette_fingerprint, footprint);
        let recomputed = SceneLayerKey {
            revision: scene.revision,
            bounds: fixture_bounds(),
            palette_fingerprint: palette_fingerprint(&palette),
        };
        assert_eq!(
            stored, recomputed,
            "key equality (thus LayerHit) flows from the fingerprint surface"
        );
        // The recompute-equality proves the hit side of the decision path
        // end to end: an identical rebuild-from-scratch key hits the layer.
        assert_eq!(
            cache.probe_draw_decision(&scene, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
    }

    // -- 11. the summary mirror rides successful builds -----------------------

    #[test]
    fn last_summary_mirrors_the_built_scene() {
        let palette = distinct_palette(false);
        let mut cache = SceneProjectionCache::new(StableId(11));
        assert!(
            cache.last_summary().is_none(),
            "a fresh cache holds no summary"
        );
        let scene = scene_at(1, 3);
        cache.apply_build(&scene, &palette, fixture_bounds());
        let summary = cache.last_summary().expect("successful build stores a mirror");
        assert_eq!(summary.revision, SceneRevision(1));
        assert_eq!(summary.nodes, 3);
        // A later revision updates the mirror in place.
        let newer = scene_at(5, 1);
        cache.apply_build(&newer, &palette, fixture_bounds());
        let summary = cache.last_summary().expect("mirror refreshed");
        assert_eq!(summary.revision, SceneRevision(5));
        assert_eq!(summary.nodes, 1);
    }

    // -- SCN-011b additions (store + cache parity, counters only) ------------

    use ocs_plugin_api::scene::{PanelScenePatch, SceneOp};

    /// A patch fixture for the report-driven flows (mirrors the API crate's
    /// `patch` helper shape; local to these tests).
    fn patch(base: u64, next: u64, ops: Vec<SceneOp>) -> PanelScenePatch {
        PanelScenePatch {
            base_revision: SceneRevision(base),
            revision: SceneRevision(next),
            ops,
        }
    }

    #[test]
    fn store_driven_sync_reprojects_only_touched_segments() {
        let palette = distinct_palette(false);
        // 60 root-level fill-only rect subtrees; the store syncs full once
        // (the snapshot flow), then two patches ride the report path.
        let mut cache = SceneProjectionCache::new(StableId(31));
        assert!(cache.store_counters().is_none(), "detached by default");
        cache.attach_store();
        let scene1 = scene_at(1, 60);
        cache
            .sync_store_full(&scene1, &palette)
            .expect("full store sync");
        let store = cache.store_counters().expect("store attached");
        assert_eq!(store.full_reprojects, 1);
        assert_eq!(store.full_reprojects_explicit, 1);
        assert_eq!(store.subtree_reprojects, 0);

        // First build: the layer rebuild paints from the fresh segments (no
        // semantic flatten; the store counters stay put).
        assert_eq!(
            cache.apply_build(&scene1, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::FirstBuild)
        );
        assert_eq!(cache.counters().layer_builds, 1);
        assert_eq!(cache.counters().prims_last_build, 60);
        let store = cache.store_counters().unwrap();
        assert_eq!(
            store.full_reprojects, 1,
            "the store-fed build re-flattens nothing"
        );

        // Patch 1 replaces root subtree 7 wholesale (a real report from the
        // single mutation path); the sync re-projects exactly that segment
        // while the layer rebuild (RevisionChanged) repaints from segments.
        let mut scene2 = scene1.clone();
        let report = scene2
            .apply_patch_report(patch(
                1,
                2,
                vec![SceneOp::Replace {
                    element: filled_rect(7, 99.0),
                }],
            ))
            .expect("patch 1 applies");
        assert_eq!(
            cache.sync_after_patch(&scene2, &report, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
        );
        let store = cache.store_counters().unwrap();
        assert_eq!(store.subtree_reprojects, 1);
        assert_eq!(store.subtree_retained_hits, 59);
        assert_eq!(store.full_reprojects, 1, "no fallback fired");
        // The layer build reused the report's post-patch summary (no re-walk).
        assert_eq!(cache.counters().layer_builds, 2);
        assert_eq!(cache.counters().prims_last_build, 60);
        assert_eq!(cache.last_summary().unwrap().revision, SceneRevision(2));
        assert_eq!(
            *cache.last_summary().expect("mirror refreshed from the report"),
            report.summary,
            "the report's summary is reused as-is"
        );

        // The installed key replays: the same scene draws as a hit.
        assert_eq!(
            cache.apply_build(&scene2, &palette, fixture_bounds()),
            DrawDecision::LayerHit
        );
        assert_eq!(cache.counters().probe_hits, 1);

        // Patch 2 touches a DIFFERENT subtree: the total re-projection count
        // grows by exactly one — untouched segments are NEVER re-projected
        // (the counter-only proof of the retention win).
        let mut scene3 = scene2.clone();
        let report2 = scene3
            .apply_patch_report(patch(
                2,
                3,
                vec![SceneOp::Replace {
                    element: filled_rect(40, 77.0),
                }],
            ))
            .expect("patch 2 applies");
        assert_eq!(
            cache.sync_after_patch(&scene3, &report2, &palette, fixture_bounds()),
            DrawDecision::LayerRebuild(RebuildReason::RevisionChanged)
        );
        let store = cache.store_counters().unwrap();
        assert_eq!(
            store.subtree_reprojects, 2,
            "second run touched one more segment, nothing else"
        );
        assert_eq!(store.subtree_retained_hits, 118, "59 + 59 retained");
        assert_eq!(store.full_reprojects, 1, "still no fallback");
        assert_eq!(cache.counters().layer_builds, 3);
    }
}
