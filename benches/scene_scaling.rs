//! SCN-012 bench: snapshot serialization and O(touched) patch scaling for
//! plugin panel scenes, recorded as a JSON run record.
//!
//! # Surface honesty (binding decision for this bench)
//!
//! A bench target compiles as a SEPARATE external crate, so it can only see
//! `pub` items of the crates it links. This bench therefore measures the
//! public `ocs_plugin_api` scene surface only:
//!
//! - `scene::PanelScene` (`apply_patch` / `apply_patch_report` /
//!   `scene_summary`), `AppliedPatchReport`, `SceneSummary`, `SceneOp`,
//! - `scene_builder::{PatchBuilder, try_fixture_graph_scene,
//!   fixture_graph_scene, fixture_graph_scene_plan}`,
//! - bincode serialization of the wire types (`PanelScene`/`PanelScenePatch`).
//!
//! The host-side projection internals — `project_scene`, `SubtreePrims`,
//! `ScenePrimStore` (`sync_full`/`sync_incremental` +
//! `subtree_reprojects`/`subtree_retained_hits`/`full_reprojects`/
//! `retired_purges`), `SceneProjectionCache` counters and
//! `palette_fingerprint` — are `pub(crate)` in the host crate and are NOT
//! benchable from a bench target. Scenario 4 runs the API-side O(touched)
//! equivalent, and the store-counter half is emitted as `scope: "deferred"`
//! counters pointing at the in-source unit tests that hold that evidence
//! today (`src/ui/window/plugin_scene.rs` tests:
//! `incremental_sync_touches_only_touched_segments`,
//! `store_driven_sync_reprojects_only_touched_segments`; widget-state
//! analogue: `expanding_one_branch_mutates_only_that_branch` in
//! `plugin_widgets.rs`). Wall-clock benching of the host store at 100k
//! scale is deferred to the mount task where that state layer goes public
//! (documented narrowed scope of SCN-012, not silently missing).
//!
//! # Scenarios (SCN-012 minimum set)
//!
//! 1. `serialize_scene_at_scale` — `try_fixture_graph_scene` at nodes =
//!    125/400/1250 (1000/3200/10 000 elements): bincode serialize bytes+ns
//!    and `scene_summary` ns per size.
//! 2. `patch_scaling_o_touched` — base 400-node fixture (3 200 elements),
//!    patches of k ∈ {1, 10, 100, 1000} root-level ops (Replace whole node
//!    subtrees + Add fresh root groups + Remove root-level edges):
//!    `apply_patch_report` ns + report facts + the acceptance metric
//!    `changed < total` printed per sample. Documented expectation: cost
//!    grows with k (touched regions), not base size — validated only as a
//!    sanity bound (ns(k=1000) < ns(k=100) × 20), raw numbers recorded, no
//!    stronger claim over-asserted.
//! 3. `stale_rejection_cost` — stale-base single-op patch →
//!    `Err(StaleBase)` with nothing written (revision immobility asserted
//!    per sample).
//! 4. `retention_evidence` — one-op patches on a fresh base per sample:
//!    the API-side per-touched-subtree walk (O(touched), bench-side root
//!    index mirroring the store's per-root-segment granularity) vs the
//!    O(scene) full read (`scene_summary`); emulated counter deltas
//!    printed, store counters deferred (see the honesty block above).
//! 5. `snapshot_recovery_payload` — the bounded recovery story in bytes:
//!    measured full-snapshot bincode bytes (1250 nodes) against the capped
//!    `SceneSummary`/`AppliedPatchReport` field inventory (hand-shaped byte
//!    math — bounded reads are in-process types, not serde wire types; the
//!    wire shape at mount time is a protocol-owner open question).
//!
//! Run: `cargo bench --bench scene_scaling` (bench profile = release;
//! each scenario's sample N is documented beside the constants below).

use std::collections::{HashMap, HashSet};
use std::hint::black_box;

use ocs_plugin_api::scene::{
    PanelScenePatch, SceneColor, SceneElement, SceneError, SceneOp, StableId,
    Stroke, ThemeColor, Vec2, MAX_REPORT_RETIRED, MAX_REPORT_TOUCHED, MAX_SUMMARY_ROOT_IDS,
};
use ocs_plugin_api::scene_builder::{
    fixture_graph_scene_plan, try_fixture_graph_scene, PatchBuilder,
};

#[path = "support/scene_support.rs"]
mod scene_support;

use scene_support::{
    acceptance_line, emit_run_record, param, timed_ns, timed_samples, Counter, ExtraMetric,
    ScenarioRecord, WallPercentiles,
};

// ---------------------------------------------------------------------------
// Fixed workload contract (a re-run must reuse exactly these constants)
// ---------------------------------------------------------------------------

/// Node sizes for scenario 1: from a 1 000-element snapshot to the fixture
/// maximum (1 250 nodes = 10 000 elements).
const SERIALIZE_NODES: [usize; 3] = [125, 400, 1250];

/// Base scene size of the patch scenarios: 400 fixture nodes = 3 200
/// elements — the population the O(touched) claim stays small against.
const BASE_NODES: usize = 400;
const BASE_ELEMENTS: usize = fixture_graph_scene_plan(BASE_NODES);

/// Fixture seed: part of the fixture contract (the LCG stream IS the
/// layout); re-runs must keep it.
const SEED: u64 = 11;

/// Root revision of the per-sample base scenes; patches base here and bump
/// to `BASE_REVISION + 1`.
const BASE_REVISION: u64 = 1;

/// Fixture id scheme: node `i` owns ids `i*10 ..= i*10+9`, edges own
/// `400*10 + i`, so the patch scenarios address roots by id.
const NODE_ID_STRIDE: u64 = 10;
const EDGE_ID_BASE: u64 = NODE_ID_STRIDE * BASE_NODES as u64;

/// Patch payload namespaces (fresh child ids so every op really retires
/// the outgoing children and reports them; never collides with live fixture
/// ids, whose whole live space stays under `4399`): replaced groups keep
/// their fixture id (`slot*10`), their fresh children at
/// `100_000 + slot*100 + role`, fresh added groups at `200_000 + op*10 ..`
/// (one exclusive slot per op).
const PATCH_CHILD_BASE: u64 = 100_000;
const PATCH_ADD_BASE: u64 = 200_000;

/// Distinct node slots the replaces cycle across: a busy plugin re-touches
/// a bounded window of the tree, which keeps `changed < total` at every k.
/// 37 and 40 are coprime, so `(replace_counter * 37) % 40` visits all 40
/// slots before repeating, and revisits re-install an identical payload for
/// the slot — legal by contract (the planner retires the outgoing subtree
/// before the fresh-id check, so its own re-adoptions never collide).
const PATCH_REPLACE_SLOTS: usize = 40;
const SLOT_STRIDE: usize = 37;

/// Op-count set of scenario 2 (the k axis of the O(touched) claim).
const PATCH_K: [usize; 4] = [1, 10, 100, 1000];

/// Sample policy (`benches/README.md` discipline: fast ops ≥ 100 samples,
/// slow ops ≥ 20; each scenario prints its own N):
const SAMPLES_FAST: usize = 100; // µs-scale ops
const SAMPLES_READS: usize = 200; // sub-µs bounded reads
const SAMPLES_SLOW_APPLY: usize = 24; // ms-scale applies (k = 1000)
const SAMPLES_SLOW_SER: usize = 40; // 10 000-element serialize
const SAMPLES_RETENTION: usize = 60; // one-op applies on fresh bases

// ---------------------------------------------------------------------------
// Payload construction (all outside the timed regions)
// ---------------------------------------------------------------------------

/// Elements in one element subtree (document-order walk).
fn payload_element_count(element: &SceneElement) -> usize {
    let mut count = 0usize;
    let mut stack = vec![element];
    while let Some(element) = stack.pop() {
        count += 1;
        if let SceneElement::Group { children, .. } = element {
            stack.extend(children.iter());
        }
    }
    count
}

/// The touched-subtree root id of one op (the report's per-op touched id:
/// `Add` reports the inserted id, `Replace` the target, `Remove` the
/// removed id).
fn op_touched_id(op: &SceneOp) -> StableId {
    match op {
        SceneOp::Add { element, .. } => element.stable_id(),
        SceneOp::Replace { element } => element.stable_id(),
        SceneOp::Remove { id } => *id,
    }
}

/// Replacement subtree for replace slot `i`: same group id as fixture node
/// `i` (slot's own group id, `slot*10` — the Replace swap key is the payload
/// id itself, so the payload MUST carry the live fixture id), same shape
/// (card rect + 4 ports + label), fresh child ids, fresh label — the apply
/// retires the 6 outgoing child ids and re-adopts only the group id.
fn replace_subtree(slot: usize) -> SceneElement {
    let fill = if slot % 2 == 0 {
        SceneColor::Theme(ThemeColor::Accent)
    } else {
        SceneColor::Theme(ThemeColor::Muted)
    };
    let child = |role: u64| StableId(PATCH_CHILD_BASE + (slot as u64) * 100 + role);
    let port = |role: u64, at: [f64; 2]| SceneElement::Port {
        id: child(role),
        name: None,
        actions: Vec::new(),
        drag: None,
        at: Vec2(at),
        label: None,
    };
    SceneElement::Group {
        id: StableId(slot as u64 * NODE_ID_STRIDE),
        name: Some(format!("retouched-{slot}")),
        actions: Vec::new(),
        drag: None,
        children: vec![
            SceneElement::Rect {
                id: child(21),
                name: None,
                actions: Vec::new(),
                drag: None,
                pos: Vec2([0.0, 0.0]),
                size: Vec2([96.0, 40.0]),
                fill: Some(fill),
                stroke: Some(Stroke {
                    color: SceneColor::Theme(ThemeColor::Strong),
                    width: 1.0,
                }),
            },
            port(22, [0.0, 20.0]),
            port(23, [96.0, 20.0]),
            port(24, [48.0, 0.0]),
            port(25, [48.0, 40.0]),
            SceneElement::Text {
                id: child(27),
                name: None,
                actions: Vec::new(),
                drag: None,
                content: format!("retouched node {slot}"),
                pos: Vec2([4.0, 28.0]),
                size: 12.0,
                color: SceneColor::Theme(ThemeColor::Strong),
            },
        ],
        clip: None,
    }
}

/// Small fresh root group installed by the `Add` ops (3 nodes: group + rect
/// + text); op index `j` owns its id slots exclusively.
fn added_group(j: usize) -> SceneElement {
    let base = PATCH_ADD_BASE + j as u64 * 10;
    let x = (j % 64) as f64 * 24.0;
    SceneElement::Group {
        id: StableId(base),
        name: Some(format!("bench-add-{j}")),
        actions: Vec::new(),
        drag: None,
        children: vec![
            SceneElement::Rect {
                id: StableId(base + 1),
                name: None,
                actions: Vec::new(),
                drag: None,
                pos: Vec2([x, 0.0]),
                size: Vec2([24.0, 16.0]),
                fill: Some(SceneColor::Theme(ThemeColor::Muted)),
                stroke: None,
            },
            SceneElement::Text {
                id: StableId(base + 2),
                name: None,
                actions: Vec::new(),
                drag: None,
                content: format!("add {j}"),
                pos: Vec2([x + 2.0, 4.0]),
                size: 11.0,
                color: SceneColor::Theme(ThemeColor::Strong),
            },
        ],
        clip: None,
    }
}

/// What one mixed patch changes, computed from the op mix (bench-side
/// bookkeeping of exactly the work one per-sync projection must do).
struct MixFacts {
    removes: usize,
    replaces: usize,
    /// Distinct node slots the replaces hit (≤ [`PATCH_REPLACE_SLOTS`]).
    distinct_replace_slots: usize,
    adds: usize,
    /// DISTINCT-region element churn — the elements whose prims one per-sync
    /// projection of this patch would recompose: 7 per distinct replaced
    /// node subtree + 1 per removed edge + 3 per added group; revisited
    /// slots count once. THE acceptance numerator.
    changed_elements: usize,
    /// Retired-id candidates per apply, pre-cap: 6 per replace op (the
    /// outgoing children; the group id re-adopts) + 1 per removed edge.
    retired_candidates: usize,
}

/// 7 nodes per replaced node subtree, 3 per added group.
const REPLACED_NODE_ELEMENTS: usize = 7;
const ADDED_GROUP_ELEMENTS: usize = 3;

/// The mixed root-level patch of k ops, deterministic per k (the documented
/// composition of "Add/Replace/Remove mixes that stay root-level"): the
/// first `k/5` ops (capped at 200) Remove distinct root-level edges; later
/// ops alternate — even slots Replace fixture node
/// `(replace_counter * 37) % 40`, odd slots Add a fresh root group. Bases
/// at [`BASE_REVISION`], successor `BASE_REVISION + 1`.
fn build_mix_patch(k: usize) -> (PanelScenePatch, MixFacts) {
    let removes = (k / 5).min(200);
    let mut patch = PatchBuilder::new(BASE_REVISION);
    let mut replace_counter = 0usize;
    let mut adds = 0usize;
    let mut touched_slots: HashMap<usize, ()> = HashMap::new();
    for j in 0..k {
        if j < removes {
            patch.remove(EDGE_ID_BASE + j as u64);
        } else if (j - removes) % 2 == 0 {
            let slot = (replace_counter * SLOT_STRIDE) % PATCH_REPLACE_SLOTS;
            patch.replace(replace_subtree(slot));
            touched_slots.insert(slot, ());
            replace_counter += 1;
        } else {
            patch.add(None, added_group(j));
            adds += 1;
        }
    }
    let retired_candidates = replace_counter * 6 + removes;
    let facts = MixFacts {
        removes,
        replaces: replace_counter,
        distinct_replace_slots: touched_slots.len(),
        adds,
        changed_elements: touched_slots.len() * REPLACED_NODE_ELEMENTS
            + removes
            + adds * ADDED_GROUP_ELEMENTS,
        retired_candidates,
    };
    (
        patch
            .finish(BASE_REVISION + 1)
            .expect("the deterministic mix patch is valid at every k"),
        facts,
    )
}

/// bincode byte length of a serde wire type from the api crate.
fn bincode_bytes<T: serde::Serialize>(value: &T) -> usize {
    bincode::serialize(value)
        .expect("bincode always serializes the scene wire types")
        .len()
}

// === scenario_1 ===

/// Per-size sample counts: the whole-budget serialize (10 000-element tree)
/// is a comparatively slow op — 40 samples; smaller sizes get the full 100.
fn serde_samples_for(elements: usize) -> usize {
    if elements >= 10_000 {
        SAMPLES_SLOW_SER
    } else {
        SAMPLES_FAST
    }
}

fn scenario_serialize_at_scale(out: &mut Vec<ScenarioRecord>) {
    let scenario = "serialize_scene_at_scale";
    for &nodes in &SERIALIZE_NODES {
        let elements = fixture_graph_scene_plan(nodes);
        let scene = try_fixture_graph_scene(BASE_REVISION, nodes, SEED)
            .expect("fixture must validate at every documented node count");
        assert_eq!(scene.scene_summary().nodes, elements);

        let bytes = bincode_bytes(&scene);

        // Serialize the WHOLE snapshot: the full-retransmit cost the patch
        // path exists to avoid. Each sample re-serializes and sinks the byte
        // count so a silent payload-shape change cannot go unnoticed.
        let size_label = format!("nodes={nodes} elements={elements} bytes={bytes}");
        let ser_ns = timed_ns(serde_samples_for(elements), 3, || {
            let bytes =
                bincode::serialize(black_box(&scene)).expect("snapshot must serialize");
            black_box(bytes.len());
        });
        let ser_ns = WallPercentiles::from_samples(&ser_ns);

        // The bounded one-walk read (the O(scene) full pass on this surface).
        let sum_ns = timed_ns(SAMPLES_READS, 3, || {
            let summary = black_box(&scene).scene_summary();
            black_box((summary.nodes, summary.root_ids_truncated));
        });
        let sum_ns = WallPercentiles::from_samples(&sum_ns);

        println!(
            "[{scenario}/nodes{nodes}] serialize p50={:.0} p95={:.0} p99={:.0} ns (n={}) \
             | {size_label} | summary p50={:.0} p95={:.0} p99={:.0} ns (n={})",
            ser_ns.p50_ns,
            ser_ns.p95_ns,
            ser_ns.p99_ns,
            ser_ns.n,
            sum_ns.p50_ns,
            sum_ns.p95_ns,
            sum_ns.p99_ns,
            sum_ns.n,
        );

        out.push(ScenarioRecord {
            scenario: format!("{scenario}:nodes={nodes}"),
            surface: "pub ocs_plugin_api::scene::PanelScene + bincode/serde serialize; \
                      PanelScene::scene_summary"
                .to_owned(),
            deferred: vec![],
            params: vec![
                param("nodes", nodes),
                param("elements", elements),
                param("seed", SEED),
                param("revision", BASE_REVISION),
                param("samples_serialize", serde_samples_for(elements)),
                param("samples_summary", SAMPLES_READS),
            ],
            samples_n: ser_ns.n,
            wall_ns: ser_ns.clone(),
            extra_metrics: vec![ExtraMetric {
                label: "scene_summary_full_walk".to_owned(),
                samples_n: sum_ns.n,
                wall_ns: sum_ns,
                payload_bytes: None,
                note: None,
            }],
            payload_bytes: Some(bytes as u64),
            payload_bytes_note: Some(
                "bincode len of the whole PanelScene snapshot — the full-retransmit \
                 cost the incremental patch path avoids"
                    .to_owned(),
            ),
            counters: vec![Counter {
                name: "scene_summary_full_walk_nodes".to_owned(),
                scope: "measured",
                value: Some(elements as u64),
                note: Some(
                    "the summary read walks every element (O(scene)) — the bounded \
                     recovery read, not the incremental path"
                        .to_owned(),
                ),
            }],
            changed_elements: None,
            total_elements: Some(elements as u64),
            changed_less_than_total: None,
            sink: format!(
                "every sample must serialize to exactly {bytes} bytes; \
                 scene_summary().nodes must report {elements} at every sample"
            ),
            notes: vec![],
        });
    }
}

// === scenario_2 ===

/// Documented sanity bound for the O(touched) claim across k: ns(k=1000)
/// must stay far below ns(k=100) × 20 (linear-in-k growth already accounts
/// for ~10×; the headroom tolerates the fixed O(scene) mirror/summary
/// floors inside every apply). The raw numbers are recorded; the claim is
/// no stronger than the bound.
const SANITY_BOUND_FACTOR: f64 = 20.0;

fn scenario_patch_scaling(out: &mut Vec<ScenarioRecord>) {
    let scenario = "patch_scaling_o_touched";
    let mut p50_by_k: Vec<(usize, f64)> = Vec::new();

    for &k in &PATCH_K {
        let base = try_fixture_graph_scene(BASE_REVISION, BASE_NODES, SEED)
            .expect("400-node fixture base must validate");
        let (patch, mix) = build_mix_patch(k);

        let patch_bytes = bincode_bytes(&patch);
        let mut distinct: HashSet<StableId> = HashSet::new();
        for op in &patch.ops {
            distinct.insert(op_touched_id(op));
        }

        // Expected report shape (asserted in every sample): per-op touched
        // entries until the cap, then exact truncation counts.
        let expected_report_touched = k.min(MAX_REPORT_TOUCHED);
        let expected_report_retired = mix.retired_candidates.min(MAX_REPORT_RETIRED);

        let acceptance = acceptance_line(mix.changed_elements, BASE_ELEMENTS);
        let line = format!(
            "k={k} (replace {} over {} slots / add {} / remove {}) \
             patch_bytes={patch_bytes} report_touched={expected_report_touched}+{}trun \
             {acceptance}",
            mix.replaces,
            mix.distinct_replace_slots,
            mix.adds,
            mix.removes,
            k - expected_report_touched,
        );

        let ns = timed_samples(
            scenario,
            if k >= 1000 {
                SAMPLES_SLOW_APPLY
            } else {
                SAMPLES_FAST
            },
            3,
            // Untimed per-sample prep: fresh base clone + fresh patch clone.
            || (base.clone(), patch.clone()),
            // Timed: the two-phase apply + report. The asserts are the sink
            // — a report that drifts from the exact-truncation contract must
            // fail the run, not corrupt the numbers.
            |(scene, patch)| {
                let mut scene = scene;
                let report = scene
                    .apply_patch_report(black_box(patch))
                    .expect("the deterministic mix patch must apply");
                assert_eq!(
                    report.touched_subtree_ids.len() + report.touched_truncated,
                    k,
                    "touched plan carries one entry per op up to the cap"
                );
                assert_eq!(
                    report.retired_ids.len() + report.retired_truncated,
                    mix.retired_candidates,
                    "retired candidates are exact: kept (capped) + truncated = total"
                );
                assert_eq!(report.revision.0, BASE_REVISION + 1);
                (report.touched_subtree_ids.len(), report.retired_ids.len())
            },
            |_| line.clone(),
        );
        let ns = WallPercentiles::from_samples(&ns);
        p50_by_k.push((k, ns.p50_ns));

        println!(
            "[{scenario}:k={k}] apply p50={:.0} p95={:.0} p99={:.0} ns (n={}) \
             | per-op p50={:.1} ns | {line}",
            ns.p50_ns,
            ns.p95_ns,
            ns.p99_ns,
            ns.n,
            ns.p50_ns / k as f64,
        );

        out.push(ScenarioRecord {
            scenario: format!("{scenario}:base=400:k={k}"),
            surface: "pub ocs_plugin_api::scene::PanelScene::apply_patch_report via \
                      scene_builder::PatchBuilder (pub items only)"
                .to_owned(),
            deferred: vec![
                "host store counters (subtree_reprojects / subtree_retained_hits / \
                 full_reprojects / retired_purges) are pub(crate) in \
                 src/ui/window/plugin_scene.rs — not benchable here; their evidence \
                 stays with the in-source unit tests until the mount task publishes \
                 the state layer"
                    .to_owned(),
            ],
            params: vec![
                param("base_nodes", BASE_NODES),
                param("base_elements", BASE_ELEMENTS),
                param("seed", SEED),
                param("k_ops", k),
                param("replace_ops", mix.replaces),
                param("distinct_replace_slots", mix.distinct_replace_slots),
                param("add_ops", mix.adds),
                param("remove_ops", mix.removes),
                param("patch_bytes_bincode", patch_bytes),
                param(
                    "samples",
                    if k >= 1000 {
                        SAMPLES_SLOW_APPLY
                    } else {
                        SAMPLES_FAST
                    },
                ),
                param("report_touched_entries", expected_report_touched),
                param("report_touched_truncated", k - expected_report_touched),
                param("report_retired_entries", expected_report_retired),
                param(
                    "report_retired_truncated",
                    mix.retired_candidates - expected_report_retired,
                ),
            ],
            samples_n: ns.n,
            wall_ns: ns,
            extra_metrics: vec![],
            payload_bytes: Some(patch_bytes as u64),
            payload_bytes_note: Some(
                "bincode len of the PanelScenePatch payload itself (the op list the \
                 wire carries for k ops)"
                    .to_owned(),
            ),
            counters: vec![
                Counter {
                    name: "distinct_touched_subtree_roots".to_owned(),
                    scope: "bench-emulated",
                    value: Some(distinct.len() as u64),
                    note: Some(
                        "bench-side distinct-region count over the patch ops (the report \
                         itself is per-op and capped at 64); the host store's real \
                         granularity is its pub(crate) root segment map"
                            .to_owned(),
                    ),
                },
                Counter {
                    name: "host::store.subtree_reprojects".to_owned(),
                    scope: "deferred",
                    value: None,
                    note: Some(
                        "pub(crate) ScenePrimStore counter (plugin_scene.rs); asserted \
                         per-op by in-source tests today"
                            .to_owned(),
                    ),
                },
            ],
            changed_elements: Some(mix.changed_elements as u64),
            total_elements: Some(BASE_ELEMENTS as u64),
            changed_less_than_total: Some(mix.changed_elements < BASE_ELEMENTS),
            sink: format!(
                "each sample applies the same patch to the same fresh base: report touched \
                 plan must be exactly k per-op entries ({} before the 64-cap), retired \
                 candidates exactly {} ({} before the 256-cap), revision bumped to {}",
                k, mix.retired_candidates, expected_report_retired, BASE_REVISION + 1,
            ),
            notes: vec![
                "documented expectation: apply ns grows ~linearly in k (touched regions), \
                 NOT in base size — the base stays 3 200 elements at every k; recorded as \
                 the sanity bound only (no base-size sweep here)"
                    .to_owned(),
            ],
        });
    }

    // Cross-k sanity bound, printed once at the end of the scenario.
    let p50_k100 = p50_by_k[2].1;
    let p50_k1000 = p50_by_k[3].1;
    let bound = p50_k100 * SANITY_BOUND_FACTOR;
    let ok = p50_k1000 < bound;
    println!(
        "[{scenario}] sanity bound: ns(k=1000) p50={p50_k1000:.0} \
         < ns(k=100) p50={p50_k100:.0} ×{SANITY_BOUND_FACTOR} = {bound:.0} ns → {}",
        if ok { "ok" } else { "FAIL" },
    );
    if let Some(record) = out.last_mut() {
        record.notes.push(if ok {
            format!(
                "sanity bound PASSED: ns(k=1000) p50 {p50_k1000:.0} < \
                 {SANITY_BOUND_FACTOR} × ns(k=100) p50 {p50_k100:.0}"
            )
        } else {
            format!(
                "sanity bound FAILED: ns(k=1000) p50 {p50_k1000:.0} ≥ \
                 {SANITY_BOUND_FACTOR} × ns(k=100) p50 {p50_k100:.0}"
            )
        });
    }
}

// === scenario_3 ===

/// Scenario 3: a stale-base patch must be the cheap phase-0 refusal —
/// `Err(StaleBase)` with nothing planned, nothing written, revision standing.
fn scenario_stale_rejection(out: &mut Vec<ScenarioRecord>) {
    let scenario = "stale_rejection_cost";
    let base = try_fixture_graph_scene(BASE_REVISION, BASE_NODES, SEED)
        .expect("400-node fixture base must validate");
    let stale_patch = PatchBuilder::new(BASE_REVISION + 8)
        .replace(replace_subtree(0))
        .finish(BASE_REVISION + 9)
        .expect("the stale patch is internally valid (only its base is stale)");
    let stale_bytes = bincode_bytes(&stale_patch);
    let label = format!(
        "stale base={} → Err(StaleBase) patch_bytes={stale_bytes} (scene untouched)",
        BASE_REVISION + 8,
    );

    let ns = timed_samples(
        scenario,
        SAMPLES_READS,
        5,
        || (base.clone(), stale_patch.clone()),
        |(scene, patch)| {
            let mut scene = scene;
            match scene.apply_patch_report(black_box(patch)) {
                Err(SceneError::StaleBase { current, .. }) => {
                    assert_eq!(current.0, BASE_REVISION, "the base revision stands");
                    assert_eq!(scene.revision.0, BASE_REVISION, "nothing was written");
                }
                other => panic!("stale patch must be Err(StaleBase), got {other:?}"),
            }
            scene.revision.0
        },
        |_| label.clone(),
    );
    let ns = WallPercentiles::from_samples(&ns);
    println!(
        "[{scenario}] rejection p50={:.0} p95={:.0} p99={:.0} ns (n={}) | {label}",
        ns.p50_ns, ns.p95_ns, ns.p99_ns, ns.n,
    );

    out.push(ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "pub ocs_plugin_api::scene::PanelScene::apply_patch_report error arm \
                  (SceneError::StaleBase)"
            .to_owned(),
        deferred: vec![
            "stale-report DEFENSE on the host cache side (SceneProjectionCache \
             full_reprojects_stale) is pub(crate) — asserted by in-source tests only"
                .to_owned(),
        ],
        params: vec![
            param("base_nodes", BASE_NODES),
            param("base_elements", BASE_ELEMENTS),
            param("seed", SEED),
            param("patch_ops", 1),
            param("declared_base", BASE_REVISION + 8),
            param("patch_bytes_bincode", stale_bytes),
            param("samples", SAMPLES_READS),
        ],
        samples_n: ns.n,
        wall_ns: ns,
        extra_metrics: vec![],
        payload_bytes: Some(stale_bytes as u64),
        payload_bytes_note: Some("bincode len of the rejected single-op payload".to_owned()),
        counters: vec![Counter {
            name: "revision_standing_after_rejection".to_owned(),
            scope: "measured",
            value: Some(BASE_REVISION),
            note: Some("asserted per sample: the scene revision is untouched".to_owned()),
        }],
        changed_elements: Some(0),
        total_elements: Some(BASE_ELEMENTS as u64),
        changed_less_than_total: Some(true),
        sink: "every sample must return Err(StaleBase) naming the CURRENT revision and \
               leave the scene byte-identical (revision immobility)"
            .to_owned(),
        notes: vec![
            "documented expectation: trivially cheap — a bounded recovery read this \
             cheap is what lets a host recover by re-projecting whole"
                .to_owned(),
        ],
    });
}

/// Scenario 4: per-subtree retention evidence on the API surface — ONE-op
/// patches (a whole node subtree replaced by a same-shape fresh payload)
/// cost O(touched) to RE-READ, versus the O(scene) full read the unpatched
/// re-projection needs. The HOST store's counter half (which of the two
/// paths the store actually took) is pub(crate) and stays with the
/// in-source unit tests (see module docs) — this scenario prints the
/// emulated counter deltas both paths would have produced, clearly
/// labeled, and records the hold-out explicitly.
fn scenario_retention_evidence(out: &mut Vec<ScenarioRecord>) {
    let scenario = "retention_evidence_api_side";
    let base = try_fixture_graph_scene(BASE_REVISION, BASE_NODES, SEED)
        .expect("400-node fixture base must validate");

    // Pass A — timed: the one-op apply + its report facts.
    let op_slot_counter = std::cell::Cell::new(0usize);
    let apply_ns = timed_samples(
        scenario,
        SAMPLES_RETENTION,
        3,
        || {
            let slot = op_slot_counter.get() % BASE_NODES;
            op_slot_counter.set(op_slot_counter.get() + 1);
            let patch = PatchBuilder::new(BASE_REVISION)
                .replace(replace_subtree(slot))
                .finish(BASE_REVISION + 1)
                .expect("one-op patch is always valid");
            (base.clone(), patch, slot)
        },
        |(scene, patch, slot)| {
            let mut scene = scene;
            let report = scene
                .apply_patch_report(black_box(patch))
                .expect("one-op patch must apply");
            assert_eq!(report.touched_subtree_ids.len(), 1, "below the 64 cap");
            assert_eq!(
                report.touched_subtree_ids[0],
                StableId(slot as u64 * NODE_ID_STRIDE),
                "the touched id names the replaced node's group"
            );
            assert_eq!(
                report.retired_ids.len() + report.retired_truncated,
                REPLACED_NODE_ELEMENTS - 1,
                "one-op replace retires the 6 outgoing children (fresh payload re-adopts \
                 only the group id)"
            );
            (slot, report.retired_ids.len())
        },
        |(slot, retired)| {
            format!(
                "slot={slot} touch=1 retired={retired} {}",
                acceptance_line(REPLACED_NODE_ELEMENTS, BASE_ELEMENTS)
            )
        },
    );
    let apply_ns = WallPercentiles::from_samples(&apply_ns);

    // Pass B — timed: the O(touched) touched-subtree read. ONE post-patch
    // state is built untimed (fresh base + the one-op apply + the root
    // index the store also keeps, root_id → position); the walk is a pure
    // read of that state, so samples repeat it while the state stays ALIVE
    // outside the timed closure — drop-free, unlike the apply lane whose
    // per-sample fresh-state wall necessarily carries the constant setup
    // drops. The visited count is a deterministic constant (asserted here).
    let (touch_state, touch_index, touched_id) = {
        let patch = PatchBuilder::new(BASE_REVISION)
            .replace(replace_subtree(0))
            .finish(BASE_REVISION + 1)
            .expect("one-op patch is always valid");
        let mut scene = base.clone();
        scene
            .apply_patch_report(patch)
            .expect("one-op patch must apply");
        let mut index: HashMap<StableId, usize> =
            HashMap::with_capacity(scene.elements.len());
        for (i, element) in scene.elements.iter().enumerate() {
            index.insert(element.stable_id(), i);
        }
        (scene, index, StableId(0))
    };
    let touch_ns = timed_ns(SAMPLES_RETENTION, 3, || {
        let position = *touch_index
            .get(&touched_id)
            .expect("the touched root must exist post-patch");
        let visited = payload_element_count(&touch_state.elements[position]);
        assert_eq!(visited, REPLACED_NODE_ELEMENTS);
        black_box(visited);
    });
    let touch_ns = WallPercentiles::from_samples(&touch_ns);

    // Pass C — timed: the O(scene) full read on the SAME post-patch state.
    let full_ns = timed_ns(SAMPLES_RETENTION, 3, || {
        let summary = black_box(&base).scene_summary();
        black_box((summary.nodes, summary.root_ids.len()));
    });
    let full_ns = WallPercentiles::from_samples(&full_ns);

    let unchanged_per_sample = BASE_ELEMENTS - REPLACED_NODE_ELEMENTS;
    println!(
        "[{scenario}] apply p50={:.0} p95={:.0} p99={:.0} ns (n={}) | \
         touched_region visit p50={:.0} ns (n={}) vs full read p50={:.0} ns (n={}) \
         | per sample: {} elements recomposed vs {} unchanged \
         ({:.2}% of the scene; {} < {} ok=true)",
        apply_ns.p50_ns, apply_ns.p95_ns, apply_ns.p99_ns, apply_ns.n,
        touch_ns.p50_ns, touch_ns.n, full_ns.p50_ns, full_ns.n,
        REPLACED_NODE_ELEMENTS, unchanged_per_sample,
        unchanged_per_sample as f64 / BASE_ELEMENTS as f64 * 100.0,
        REPLACED_NODE_ELEMENTS, BASE_ELEMENTS,
    );

    out.push(ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "pub ocs_plugin_api::scene (apply_patch_report + bench-side touched \
                  subtree walk + scene_summary) — the measured O(touched) half; the \
                  host store half is pub(crate), see deferred"
            .to_owned(),
        deferred: vec![
            "store counters subtree_reprojects / subtree_retained_hits / full_reprojects / \
             retired_purges live in pub(crate) ScenePrimStore (src/ui/window/plugin_scene.rs \
             sync_incremental); the counter ratios they produce are pinned by the in-source \
             tests `incremental_sync_touches_only_touched_segments` and \
             `store_driven_sync_reprojects_only_touched_segments`; wall-clock store \
             numbers at 100k scale defer to the mount task (SCN-012 narrowed scope)"
                .to_owned(),
        ],
        params: vec![
            param("base_nodes", BASE_NODES),
            param("base_elements", BASE_ELEMENTS),
            param("seed", SEED),
            param("ops_per_patch", 1),
            param("touched_subtree_elements", REPLACED_NODE_ELEMENTS),
            param("samples_apply", SAMPLES_RETENTION),
            param("samples_touched_walk", SAMPLES_RETENTION),
            param("samples_full_read", SAMPLES_RETENTION),
        ],
        samples_n: apply_ns.n,
        wall_ns: apply_ns,
        extra_metrics: vec![
            ExtraMetric {
                label: "touched_subtree_walk (O(touched))".to_owned(),
                samples_n: touch_ns.n,
                wall_ns: touch_ns,
                payload_bytes: None,
                note: Some(format!(
                    "bench-side root index (O(scene) build, untimed) + the touched \
                     subtree walk: exactly {REPLACED_NODE_ELEMENTS} elements visited \
                     of {BASE_ELEMENTS} (asserted per sample); the walked state stays \
                     alive outside the timed closure, so the sample is drop-free"
                )),
            },
            ExtraMetric {
                label: "scene_summary_full_walk (O(scene))".to_owned(),
                samples_n: full_ns.n,
                wall_ns: full_ns,
                payload_bytes: None,
                note: Some(format!(
                    "the full bounded read: {BASE_ELEMENTS} elements visited per sample"
                )),
            },
        ],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![
            Counter {
                name: "subtree_reprojects".to_owned(),
                scope: "bench-emulated",
                value: Some(1),
                note: Some(format!(
                    "per sample: the one touched subtree ({BASE_ELEMENTS} elements) \
                     recomposes — BENCH-EMULATED delta, the authoritative counter is \
                     pub(crate)"
                )),
            },
            Counter {
                name: "subtree_retained_hits".to_owned(),
                scope: "bench-emulated",
                value: Some(unchanged_per_sample as u64),
                note: Some(format!(
                    "per sample: {unchanged_per_sample} of {BASE_ELEMENTS} elements keep \
                     their projection untouched — BENCH-EMULATED delta (touched-region \
                     complement); the store's real ratio is pinned by its unit tests"
                )),
            },
            Counter {
                name: "full_reprojects".to_owned(),
                scope: "bench-emulated",
                value: Some(0),
                note: Some(
                    "per sample: no whole-tree flatten is required by a 1-op report \
                     without truncation — BENCH-EMULATED delta"
                        .to_owned(),
                ),
            },
        ],
        changed_elements: Some(REPLACED_NODE_ELEMENTS as u64),
        total_elements: Some(BASE_ELEMENTS as u64),
        changed_less_than_total: Some(true),
        sink: format!(
            "every sample must touch exactly {REPLACED_NODE_ELEMENTS} elements of \
             {BASE_ELEMENTS} ({REPLACED_NODE_ELEMENTS} < {BASE_ELEMENTS} printed per \
             sample) and retire exactly 6 outgoing ids"
        ),
        notes: vec![
            "HONESTY SPLIT (declared, not silent): this scenario is the API-crate \
             equivalent the task pinned for a pub(crate)-blocked host store; the \
             store-side counter story remains unit-test evidence"
                .to_owned(),
            "wall honesty: the apply lane's per-sample wall includes the fresh state \
             clones' drops (constant across k — the per-sample prepare rebuilds base + \
             patch untimed, the closure owns them); the walk/read lanes are drop-free \
             pure reads, so their absolute numbers are clean"
                .to_owned(),
        ],
    });
}

// === scenario_5 ===

/// Scenario 5: the bounded recovery story in bytes — a measured full
/// snapshot (what a host WITHOUT the summary read would re-transmit)
/// against the capped recovery-read inventory (hand-shaped field math:
/// `SceneSummary`/`AppliedPatchReport` are in-process types, not serde wire
/// types, in the api crate today). Also runs the k=1000 mix once to record
/// the REAL report facts at the caps (truncation behaviour), and the 400
/// base's acceptance line for the same patch.
fn scenario_snapshot_recovery(out: &mut Vec<ScenarioRecord>) {
    let scenario = "snapshot_recovery_payload";
    let scene = try_fixture_graph_scene(BASE_REVISION, 1250, SEED)
        .expect("the 1250-node fixture is at the node budget");
    assert_eq!(scene.scene_summary().nodes, 10_000);
    let snapshot_bytes = bincode_bytes(&scene);

    let ser_ns = timed_ns(SAMPLES_SLOW_SER, 3, || {
        let bytes =
            bincode::serialize(black_box(&scene)).expect("snapshot must serialize");
        black_box(bytes.len());
    });
    let ser_ns = WallPercentiles::from_samples(&ser_ns);

    // The real report facts at the caps: applying the k=1000 mix once, the
    // report must carry exactly the capped lists with exact truncation.
    let (mix_patch, mix) = build_mix_patch(1000);
    let mut base = try_fixture_graph_scene(BASE_REVISION, BASE_NODES, SEED)
        .expect("400-node base must validate");
    let report = base
        .apply_patch_report(black_box(mix_patch))
        .expect("the k=1000 mix must apply");
    assert_eq!(report.touched_subtree_ids.len(), MAX_REPORT_TOUCHED);
    assert_eq!(report.touched_truncated, 1000 - MAX_REPORT_TOUCHED);
    assert_eq!(report.retired_ids.len(), MAX_REPORT_RETIRED);
    assert_eq!(
        report.retired_truncated,
        mix.retired_candidates - MAX_REPORT_RETIRED
    );
    let summary = report.summary.clone();
    // Post-patch total: the mix removed `removes` root edges and ADDED
    // `adds` 3-element groups (the replaces are 1:1), so the summary's node
    // count is not the base — computed exactly here.
    let post_patch_nodes = BASE_ELEMENTS - mix.removes + mix.adds * ADDED_GROUP_ELEMENTS;
    assert_eq!(summary.nodes, post_patch_nodes);

    // Hand-shaped in-process field inventory of the recovery read (byte
    // math from the documented caps; NOT a wire payload — see notes).
    let touched_cap_bytes = MAX_REPORT_TOUCHED * std::mem::size_of::<StableId>();
    let retired_cap_bytes = MAX_REPORT_RETIRED * std::mem::size_of::<StableId>();
    let report_fixups = 2 * std::mem::size_of::<usize>() + std::mem::size_of::<u64>();
    let summary_shaped_bytes = std::mem::size_of::<u64>()
        * (1 + 1 + 9 + MAX_SUMMARY_ROOT_IDS + 1)
        + std::mem::size_of::<bool>();
    let inventory = touched_cap_bytes + retired_cap_bytes + report_fixups + summary_shaped_bytes;

    println!(
        "[{scenario}] full 1250-node snapshot: p50={:.0} p95={:.0} p99={:.0} ns (n={}) \
        {snapshot_bytes} bytes | recovery read at the caps: touched {touched_cap_bytes} B \
        + retired {retired_cap_bytes} B + fixups {report_fixups} B + summary {summary_shaped_bytes} B \
        = {inventory} B (hand-shaped) | k=1000 report facts: touched={}+{}trun \
        retired={}+{}trun",
        ser_ns.p50_ns,
        ser_ns.p95_ns,
        ser_ns.p99_ns,
        ser_ns.n,
        report.touched_subtree_ids.len(),
        report.touched_truncated,
        report.retired_ids.len(),
        report.retired_truncated,
    );

    out.push(ScenarioRecord {
        scenario: scenario.to_owned(),
        surface: "pub ocs_plugin_api::scene::{
                    bincode serialize of PanelScene (measured); report facts via
                    apply_patch_report (measured); SceneSummary/AppliedPatchReport caps
                    (hand-shaped byte math — not serde types)"
        .replace('\n', " ")
        .to_owned(),
        deferred: vec![
            "the WIRE serialization of the recovery read (summary/report) does not exist \
             in the api crate today: those reads are in-process types — their wire shape \
             is a protocol-owner question recorded as an SCN-012 open question"
                .to_owned(),
        ],
        params: vec![
            param("fixture_nodes", 1250),
            param("fixture_elements", 10_000),
            param("base_nodes", BASE_NODES),
            param("base_elements", BASE_ELEMENTS),
            param("report_probe_patch_ops", 1000),
            param("samples_full_snapshot", SAMPLES_SLOW_SER),
            param("touched_cap_entries", MAX_REPORT_TOUCHED),
            param("retired_cap_entries", MAX_REPORT_RETIRED),
            param("summary_root_cap_entries", MAX_SUMMARY_ROOT_IDS),
        ],
        samples_n: ser_ns.n,
        wall_ns: ser_ns,
        extra_metrics: vec![],
        payload_bytes: Some(snapshot_bytes as u64),
        payload_bytes_note: Some(format!(
            "measured: bincode bytes of the FULL 1250-node snapshot (the retransmit a \
             stale host without the summary read pays); capped recovery read at the caps: \
             touched {touched_cap_bytes} + retired {retired_cap_bytes} + fixups \
             {report_fixups} + summary {summary_shaped_bytes} = {inventory} bytes \
             (hand-shaped), i.e. the recovery read is fixed-size regardless of the \
             10 000-element tree"
        )),
        counters: vec![
            Counter {
                name: "report_touched_reported_at_cap".to_owned(),
                scope: "measured",
                value: Some(MAX_REPORT_TOUCHED as u64),
                note: Some("from the measured k=1000 apply".to_owned()),
            },
            Counter {
                name: "report_touched_truncated".to_owned(),
                scope: "measured",
                value: Some((1000 - MAX_REPORT_TOUCHED) as u64),
                note: Some(
                    "exact truncation: the store treats any truncation as a full \
                     re-projection trigger (documented defensive fallback)"
                        .to_owned(),
                ),
            },
            Counter {
                name: "report_retired_reported_at_cap".to_owned(),
                scope: "measured",
                value: Some(MAX_REPORT_RETIRED as u64),
                note: Some("from the measured k=1000 apply".to_owned()),
            },
            Counter {
                name: "report_retired_truncated".to_owned(),
                scope: "measured",
                value: Some(
                    (mix.retired_candidates - MAX_REPORT_RETIRED) as u64,
                ),
                note: None,
            },
            Counter {
                name: "summary_nodes_post_patch".to_owned(),
                scope: "measured",
                value: Some(summary.nodes as u64),
                note: Some(format!(
                    "the report's summary read reports {BASE_ELEMENTS} post-patch nodes \
                     from a bounded one-walk"
                )),
            },
        ],
        changed_elements: Some(mix.changed_elements as u64),
        total_elements: Some(BASE_ELEMENTS as u64),
        changed_less_than_total: Some(mix.changed_elements < BASE_ELEMENTS),
        sink: format!(
            "report caps asserted exactly ({MAX_REPORT_TOUCHED} touched + {} truncations, \
             {MAX_REPORT_RETIRED} retired + {} truncations); the 1250-node snapshot must \
             serialize to a constant {snapshot_bytes} bytes at every sample",
            1000 - MAX_REPORT_TOUCHED,
            mix.retired_candidates - MAX_REPORT_RETIRED,
        ),
        notes: vec![
            "the acceptance line's mix basis is the 400-node base / k=1000 patch \
             (changed regions = distinct replaced slots + removed edges + added groups)"
                .to_owned(),
        ],
    });
}

fn main() {
    let mut scenarios: Vec<ScenarioRecord> = Vec::new();
    scenario_serialize_at_scale(&mut scenarios);
    scenario_patch_scaling(&mut scenarios);
    scenario_stale_rejection(&mut scenarios);
    scenario_retention_evidence(&mut scenarios);
    scenario_snapshot_recovery(&mut scenarios);
    emit_run_record("scene_scaling", scenarios);
}