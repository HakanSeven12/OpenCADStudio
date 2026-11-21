//! SCN-012 bench: widget surface serialization and widget patch scaling at
//! the public API-crate surface, recorded as a JSON run record.
//!
//! # Surface honesty (binding decision for this bench — the narrow scope,
//! declared per scenario and in the delivery)
//!
//! A bench target compiles as a SEPARATE external crate, so it can only see
//! `pub` items of the crates it links. The host-side widget STATE layer —
//! `WidgetUiState` (`toggle_expand`/`set_selection`/`sync`/
//! `request_children_if_lazy`), `visible_window`, `materialize_ids`,
//! `WidgetStateMutations`, `FlatRow`/`WindowSpec`/`VisibleWindow` — is
//! `pub(crate)` in `src/ui/window/plugin_widgets.rs` and therefore NOT
//! benchable from a bench target. Likewise the scene-side host projection
//! (`plugin_scene`/`plugin_scene_cache`) is `pub(crate)`. This bench
//! measures the public `ocs_plugin_api::widgets` surface ONLY:
//!
//! - `WidgetSurface` (`validate` / `apply_patch` / `widget_summary`) with
//!   the Family-1 vocabulary (`ItemListDef`, `WidgetTreeDef`,
//!   `IconActionDef`, `WidgetGroup`, …) — there is NO pub widget fixture
//!   generator in the crate, so the bench builds its fixtures from the pub
//!   definitions (test/bench-side construction only; nothing is added to
//!   the crate).
//! - `WidgetRange` (`bounds_ok`) — the one pub piece of the window
//!   contract.
//!
//! # Deferred (recorded, not silently missing)
//!
//! - Window math wall-clock at 100k rows (`visible_window` /
//!   `materialize_ids`): pub(crate). Scenario 3 prints `WidgetRange`
//!   measurements, a clearly-labeled BENCH-SIDE EMULATION of the documented
//!   O(window) materialize contract (not the pub(crate) implementation),
//!   and the payload-cap gate evidence (4096 == cap passes, 4097 rejects)
//!   that FORCES windowing at 100k scale. The authoritative host-side
//!   window evidence is the in-source unit tests
//!   (`window_basics_and_degenerate_specs`,
//!   `expanding_one_branch_mutates_only_that_branch`,
//!   `state_survives_untouched_patch`); wall-clock 100k windowing waits for
//!   the mount task where the state layer goes pub (SCN-012 narrowed scope
//!   + open question for the protocol owner).
//! - Host-state mutation counters (`WidgetStateMutations`): pub(crate);
//!   asserted by `mutation_counters_are_the_defect_probe` — deferred
//!   counters in the run record, no wall-clock numbers claimed.
//! - There is NO widget-side `AppliedPatchReport` equivalent in the API
//!   crate today (`WidgetSurface::apply_patch` returns `Result<_,
//!   WidgetError>` only) — the patch scenarios compute changed/touched
//!   counts bench-side from the op payloads and record the missing
//!   report as an API-surface deficit.
//!
//! # Scenarios
//!
//! 1. `widget_surface_serialize_at_scale` — List and Tree surfaces at
//!    rows/items ∈ {100, 1000, 4096-cap}: bincode serialize bytes+ns,
//!    `validate` ns, `widget_summary` ns.
//! 2. `widget_patch_scaling` — Update/Add/Remove mixes at k ∈ {1, 100,
//!    1000} via `WidgetSurface::apply_patch` + patch payload bytes + the
//!    big-list whole-payload Update anti-pattern (the full-collection
//!    retransmit `ARCHITECTURE.md` §14 names a defect — measured as the
//!    reason windowing exists).
//! 3. `selection_window_math` — `WidgetRange::bounds_ok` timings, the
//!    emulated O(window) id materialization at window sizes {16, 64, 256}
//!    against a virtual 100 000-row list (per-sample `materialized <
//!    total` lines), the O(total) anti-pattern measurement, and the
//!    payload-cap gate (4096/4097).
//!
//! Run: `cargo bench --bench widget_state_scaling` (bench profile =
//! release; per-scenario sample Ns documented beside the constants).

use std::cell::Cell;
use std::collections::HashSet;
use std::hint::black_box;

use ocs_plugin_api::scene::{SceneRevision, StableId};
use ocs_plugin_api::widgets::{
    Axis, DomainRef, IconActionDef, IconRef, ItemDef, ItemListDef, TreeCapabilities, TreeRowDef,
    WidgetError, WidgetGroup, WidgetNode, WidgetOp, WidgetRange, WidgetSurface,
    WidgetSurfacePatch, WidgetTreeDef, MAX_WIDGET_RANGE_LEN, MAX_WIDGET_ROWS_INITIAL,
};

#[path = "support/scene_support.rs"]
mod scene_support;

use scene_support::{
    acceptance_line, emit_run_record, param, timed_ns, timed_samples, Counter, ExtraMetric,
    ScenarioRecord, WallPercentiles,
};

// ---------------------------------------------------------------------------
// Fixed workload contract
// ---------------------------------------------------------------------------

/// Rows/items per payload the scale scenario sweeps: 100, 1000, and exactly
/// the per-payload cap (4096 == `MAX_WIDGET_ROWS_INITIAL` still validates;
/// one more row rejects).
const SURFACE_ROWS: [usize; 3] = [100, 1000, 4096];

/// Op counts of the patch scenario.
const PATCH_K: [usize; 3] = [1, 100, 1000];

/// Sample policy (fast ops ≥ 100, slow ops ≥ 20 — README discipline).
const SAMPLES_FAST: usize = 100;
const SAMPLES_READS: usize = 200;
const SAMPLES_SLOW_APPLY: usize = 24;
const SAMPLES_SLOW_SER: usize = 40;

/// Base revision of every surface the bench creates; patches base here and
/// bump to `BASE_REVISION + 1`.
const BASE_REVISION: u64 = 1;

// The surface-wide id namespace (ONE scope across nodes, rows, items):
const ROOT_GROUP_ID: u64 = 1;
const BIG_LIST_ID: u64 = 2;
const BIG_ITEM_BASE: u64 = 10_000; // big list items: 10_000 .. 10_000+n
const SMALL_LIST_BASE: u64 = 3_000; // small update-target lists: 3_000+L
const SMALL_ITEM_BASE: u64 = 3_000_000; // their items: 3_000_000 + L*100 + i
const BUTTON_BASE: u64 = 5_000; // 256 pre-built buttons: 5_000 + j
const PATCH_ADD_BASE: u64 = 7_000_000; // fresh added buttons: 7_000_000 + j
const PATCH_ITEM_BASE: u64 = 9_000_000; // update-payload item ids: 9_000_000 + j*100 + i
const TREE_ROW_BASE: u64 = 20_000; // tree rows: 20_000 + idx
/// Patch banner: buttons the REMOVEs target (256 pre-built distinct ids).
const PREBUILT_BUTTONS: usize = 256;
const SMALL_LISTS: usize = 40;
const SMALL_ITEMS_PER_LIST: usize = 4;
/// Window sizes for the materialize emulation rows.
const WINDOW_SIZES: [usize; 3] = [16, 64, 256];
/// The virtual row total behind the window evidence (the 100k scale the
/// addendum's budget discipline protects).
const VIRTUAL_ROWS: usize = 100_000;

// ---------------------------------------------------------------------------
// Fixture construction from the pub vocabulary (bench-side only)
// ---------------------------------------------------------------------------

/// One list item of the pub vocabulary, deterministic per index.
fn item(id: StableId, idx: usize) -> ItemDef {
    ItemDef {
        id,
        label: format!("item-{idx}"),
        icon: None,
        enabled: true,
        visible: true,
        command: Some("bench.item.activate".to_owned()),
        domain_ref: Some(DomainRef {
            kind: "bench-domain".to_owned(),
            id: idx as u64,
        }),
    }
}

/// One tree row; `parent` must name an EARLIER row (the no-forward rule).
fn row(parent: Option<StableId>, idx: usize) -> TreeRowDef {
    TreeRowDef {
        id: StableId(TREE_ROW_BASE + idx as u64),
        parent,
        label: format!("row-{idx}"),
        icon: None,
        status: Some("static".to_owned()),
        expandable: parent.is_none(),
        enabled: true,
        lazy_children: parent.is_none(),
        domain_ref: None,
    }
}

/// A List widget with `items` items (ids `BIG_ITEM_BASE + idx`).
fn list_widget(id: u64, items: usize, id_ns: u64) -> WidgetNode {
    WidgetNode::List(ItemListDef {
        id: StableId(id),
        name: None,
        enabled: true,
        visible: true,
        label: None,
        items: (0..items)
            .map(|i| item(StableId(id_ns + i as u64), i))
            .collect(),
    })
}

/// A Tree widget with `rows` shallow rows (64 roots, then children —
/// parents within the same payload, always earlier; chain depth ≤ 2).
fn tree_widget(items: usize) -> WidgetNode {
    let roots = items.min(64);
    let mut rows = Vec::with_capacity(items);
    for i in 0..roots {
        rows.push(row(None, i));
    }
    for r in roots..items {
        rows.push(row(
            Some(StableId(TREE_ROW_BASE + (r - roots) as u64 % 64)),
            r,
        ));
    }
    WidgetNode::Tree(WidgetTreeDef {
        id: StableId(BIG_LIST_ID),
        name: None,
        enabled: true,
        visible: true,
        label: None,
        capabilities: TreeCapabilities {
            multi_select: false,
            filterable: false,
            lazy_children: true,
        },
        rows,
    })
}

/// One pre-built IconButton (the Remove targets and the Add payloads).
fn icon_button(id: u64, seq: usize) -> WidgetNode {
    WidgetNode::IconButton(IconActionDef {
        id: StableId(id),
        accessible_name: format!("bench button {seq}"),
        tooltip: None,
        icon: IconRef {
            uri: "bench://icon".to_owned(),
            size: 16.0,
        },
        enabled: true,
        visible: true,
        active: false,
        command: "bench.cmd".to_owned(),
        confirmation: None,
    })
}

/// The patch base surface: root group + big list (1000 items) + small
/// update-target lists + 256 buttons. `widgets` (semantic nodes) = 1 + 1 +
/// SMALL_LISTS + PREBUILT_BUTTONS = 299. Validates at construction (and
/// stays within the per-panel widget cap; the k=1000 mix's adds are chosen
/// to keep the POST-patch surface inside `MAX_WIDGETS_PER_PANEL` too).
fn base_surface() -> WidgetSurface {
    let mut children = vec![
        list_widget(BIG_LIST_ID, 1000, BIG_ITEM_BASE),
        WidgetNode::Group(WidgetGroup {
            id: StableId(ROOT_GROUP_ID + 100),
            name: None,
            enabled: true,
            visible: true,
            direction: Axis::Vertical,
            children: (0..SMALL_LISTS)
                .map(|l| {
                    list_widget(
                        SMALL_LIST_BASE + l as u64,
                        SMALL_ITEMS_PER_LIST,
                        SMALL_ITEM_BASE + l as u64 * 100,
                    )
                })
                .collect(),
            split_ratio: None,
        }),
    ];
    for j in 0..PREBUILT_BUTTONS {
        children.push(icon_button(BUTTON_BASE + j as u64, j));
    }
    let surface = WidgetSurface {
        revision: SceneRevision(BASE_REVISION),
        tables: std::collections::BTreeMap::new(),
        roots: vec![WidgetNode::Group(WidgetGroup {
            id: StableId(ROOT_GROUP_ID),
            name: Some("patch base".to_owned()),
            enabled: true,
            visible: true,
            direction: Axis::Vertical,
            children,
            split_ratio: None,
        })],
    };
    surface
        .validate()
        .expect("bench fixtures must validate (unique surface-wide ids)");
    surface
}

/// Patch-banner mirror: the surface's whole row/item payload count (the
/// 100k-discipline denominator: rows/items are the payload, nodes are the
/// widgets).
fn surface_row_item_total(surface: &WidgetSurface) -> usize {
    let mut total = 0usize;
    let mut stack: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            WidgetNode::List(def) => total += def.items.len(),
            WidgetNode::Tree(def) => total += def.rows.len(),
            WidgetNode::Group(group) => stack.extend(group.children.iter()),
            WidgetNode::IconButton(_) | WidgetNode::Menu(_) | WidgetNode::Table(_) => {}
        }
    }
    total
}

/// The big list's item count (the 3000-denominator untouched claim walks it).
fn big_list_items(surface: &WidgetSurface) -> usize {
    let mut stack: Vec<&WidgetNode> = surface.roots.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            WidgetNode::List(def) if def.id == StableId(BIG_LIST_ID) => {
                return def.items.len();
            }
            WidgetNode::Group(group) => stack.extend(group.children.iter()),
            _ => {}
        }
    }
    0
}

/// bincode byte length of a serde wire type from the api crate.
fn bincode_bytes<T: serde::Serialize>(value: &T) -> usize {
    bincode::serialize(value)
        .expect("bincode always serializes the widget wire types")
        .len()
}

// === scenario_1 ===

/// Scenario 1: List and Tree surfaces at the three payload scales. A
/// record per (kind, rows): serialize wall/clock + bytes + validate +
/// widget_summary reads.
fn scenario_surface_scale(out: &mut Vec<ScenarioRecord>) {
    let scenario = "widget_surface_serialize_at_scale";
    for kind in ["list", "tree"] {
        for &rows in &SURFACE_ROWS {
            let single = match kind {
                "list" => list_widget(BIG_LIST_ID, rows, BIG_ITEM_BASE),
                _ => tree_widget(rows),
            };
            let surface = WidgetSurface {
                revision: SceneRevision(BASE_REVISION),
                tables: std::collections::BTreeMap::new(),
                roots: vec![WidgetNode::Group(WidgetGroup {
                    id: StableId(ROOT_GROUP_ID),
                    name: None,
                    enabled: true,
                    visible: true,
                    direction: Axis::Vertical,
                    children: vec![single],
                    split_ratio: None,
                })],
            };
            surface.validate().expect("bench fixtures must validate");
            assert_eq!(surface.widget_summary().widgets, 2); // root group + control

            let bytes = bincode_bytes(&surface);
            let label = format!("kind={kind} rows={rows} bytes={bytes}");

            let ser_ns = timed_ns(serde_samples_for(rows), 3, || {
                let bytes =
                    bincode::serialize(black_box(&surface)).expect("surface must serialize");
                black_box(bytes.len());
            });
            let ser_ns = WallPercentiles::from_samples(&ser_ns);

            let val_ns = timed_ns(SAMPLES_FAST, 3, || {
                black_box(&surface).validate().expect("fixture stays valid");
            });
            let val_ns = WallPercentiles::from_samples(&val_ns);

            let sum_ns = timed_ns(SAMPLES_READS, 3, || {
                let summary = black_box(&surface).widget_summary();
                black_box((summary.widgets, summary.root_ids_truncated));
            });
            let sum_ns = WallPercentiles::from_samples(&sum_ns);

            println!(
                "[{scenario}/{kind}{rows}] serialize p50={:.0} p95={:.0} p99={:.0} ns (n={}) \
                 {label} | validate p50={:.0} ns | summary p50={:.0} ns",
                ser_ns.p50_ns, ser_ns.p95_ns, ser_ns.p99_ns, ser_ns.n, val_ns.p50_ns, sum_ns.p50_ns,
            );

            out.push(ScenarioRecord {
                scenario: format!("{scenario}:kind={kind}:rows={rows}"),
                surface: "pub ocs_plugin_api::widgets::WidgetSurface (validate, widget_summary) \
                          + serde/bincode serialize"
                    .to_owned(),
                deferred: vec![],
                params: vec![
                    param("kind", kind),
                    param("rows_items", rows),
                    param("rows_items_cap", MAX_WIDGET_ROWS_INITIAL),
                    param("samples_serialize", serde_samples_for(rows)),
                    param("samples_validate", SAMPLES_FAST),
                    param("samples_summary", SAMPLES_READS),
                ],
                samples_n: ser_ns.n,
                wall_ns: ser_ns,
                extra_metrics: vec![
                    ExtraMetric {
                        label: "validate".to_owned(),
                        samples_n: val_ns.n,
                        wall_ns: val_ns,
                        payload_bytes: None,
                        note: None,
                    },
                    ExtraMetric {
                        label: "widget_summary_read".to_owned(),
                        samples_n: sum_ns.n,
                        wall_ns: sum_ns,
                        payload_bytes: None,
                        note: None,
                    },
                ],
                payload_bytes: Some(bytes as u64),
                payload_bytes_note: Some(
                    "bincode len of the whole WidgetSurface snapshot at this scale"
                        .to_owned(),
                ),
                counters: vec![Counter {
                    name: "payload_rows_items".to_owned(),
                    scope: "measured",
                    value: Some(rows as u64),
                    note: Some(format!(
                        "cap={MAX_WIDGET_ROWS_INITIAL} (the windowing constant): {rows} \
                         rows/items serialize as one payload"
                    )),
                }],
                changed_elements: None,
                total_elements: Some(rows as u64),
                changed_less_than_total: None,
                sink: format!(
                    "every sample must serialize to exactly {bytes} bytes; the surface \
                     must validate and the summary must report 3 widgets"
                ),
                notes: vec![],
            });
        }
    }
}

/// Per-size sample counts (4096-row serialize/apply are the slow lane).
fn serde_samples_for(rows: usize) -> usize {
    if rows >= MAX_WIDGET_ROWS_INITIAL {
        SAMPLES_SLOW_SER
    } else {
        SAMPLES_FAST
    }
}

// === scenario_2 ===

/// Scenario 2: Update/Add/Remove patches at k ∈ {1, 100, 1000} through
/// `WidgetSurface::apply_patch`, plus the big-list whole-payload Update
/// anti-pattern that justifies windowing. Changed/touched counts are
/// computed bench-side from the op payloads: the widget surface patch does
/// NOT produce a report (API deficit recorded below).
fn scenario_widget_patch_scaling(out: &mut Vec<ScenarioRecord>) {
    let scenario = "widget_patch_scaling";

    for &k in &PATCH_K {
        let base = base_surface();
        let total_payload = surface_row_item_total(&base);
        let (patch, mix) = build_widget_mix(k);

        let patch_bytes = bincode_bytes(&patch);
        let acceptance = acceptance_line(mix.item_regions, total_payload);
        let line = format!(
            "k={k} (update {} over {} slots / add {} / remove {}) patch_bytes={patch_bytes} \
             changed_nodes_distinct={} {acceptance} big_list_untouched=1000/1000",
            mix.updates,
            mix.distinct_update_slots,
            mix.adds,
            mix.removes,
            mix.nodes,
        );

        let ns = timed_samples(
            scenario,
            if k >= 1000 {
                SAMPLES_SLOW_APPLY
            } else {
                SAMPLES_FAST
            },
            3,
            || (base.clone(), patch.clone()),
            |(surface, patch)| {
                let mut surface = surface;
                surface
                    .apply_patch(black_box(patch))
                    .expect("the widget mix patch must apply");
                surface.revision.0
            },
            |_| line.clone(),
        );
        let ns = WallPercentiles::from_samples(&ns);

        // Untimed correctness pass on the SAME mix: the big list is never
        // re-carried by any mix op — assert the 1000-item payload survives
        // the apply untouched.
        let mut check: WidgetSurface = base.clone();
        check
            .apply_patch(black_box(patch.clone()))
            .expect("must apply");
        assert_eq!(check.revision.0, BASE_REVISION + 1);
        assert_eq!(
            big_list_items(&check),
            1000,
            "the mix never re-carrys the big list payload"
        );

        println!(
            "[{scenario}:k={k}] apply p50={:.0} p95={:.0} p99={:.0} ns (n={}) | \
             per-op p50={:.1} ns | {line}",
            ns.p50_ns, ns.p95_ns, ns.p99_ns, ns.n, ns.p50_ns / k as f64,
        );

        out.push(ScenarioRecord {
            scenario: format!("{scenario}:k={k}"),
            surface: "pub ocs_plugin_api::widgets::WidgetSurface::apply_patch (pub items \
                      only; NO report exists on this surface — API deficit recorded)"
                .to_owned(),
            deferred: vec![
                "the widget-side patch REPORT (touched/retired facts, mirror of \
                 AppliedPatchReport) does not exist in the api crate today — \
                 WidgetSurface::apply_patch returns Result<(), WidgetError>; the \
                 scene-side report contract has no widget mirror yet"
                    .to_owned(),
                "host widget-state mutation counters (WidgetStateMutations) are \
                 pub(crate) in src/ui/window/plugin_widgets.rs; evidence = "
                    .to_owned()
                    + "`mutation_counters_are_the_defect_probe` unit test; deferred",
            ],
            params: vec![
                param("k_ops", k),
                param("updates", mix.updates),
                param("distinct_update_slots", mix.distinct_update_slots),
                param("adds", mix.adds),
                param("removes", mix.removes),
                param("base_widgets_nodes", surface_widget_nodes(&base)),
                param("base_payload_rows_items", total_payload),
                param("patch_bytes_bincode", patch_bytes),
                param(
                    "samples",
                    if k >= 1000 {
                        SAMPLES_SLOW_APPLY
                    } else {
                        SAMPLES_FAST
                    },
                ),
            ],
            samples_n: ns.n,
            wall_ns: ns,
            extra_metrics: vec![],
            payload_bytes: Some(patch_bytes as u64),
            payload_bytes_note: Some(
                "bincode len of the WidgetSurfacePatch payload for k ops".to_owned(),
            ),
            counters: vec![
                Counter {
                    name: "changed_item_regions".to_owned(),
                    scope: "bench-emulated",
                    value: Some(mix.item_regions as u64),
                    note: Some(format!(
                        "bench-side count of rows/items re-carried by the ops' payloads \
                         (the 1000-item big list never appears in any of these ops): \
                         change/total denominators are bench bookkeeping because the \
                         surface emits no report"
                    )),
                },
                Counter {
                    name: "distinct_touched_widget_nodes".to_owned(),
                    scope: "bench-emulated",
                    value: Some(mix.nodes as u64),
                    note: Some("bench-side distinct-node churn over the ops".to_owned()),
                },
                Counter {
                    name: "host::widget_state.mutates".to_owned(),
                    scope: "deferred",
                    value: None,
                    note: Some(
                        "pub(crate) WidgetStateMutations (plugin_widgets.rs) — asserted \
                         by in-source tests today, not benchable"
                            .to_owned(),
                    ),
                },
            ],
            changed_elements: Some(mix.item_regions as u64),
            total_elements: Some(total_payload as u64),
            changed_less_than_total: Some(mix.item_regions < total_payload),
            sink: format!(
                "every sample must apply cleanly at revision {BASE_REVISION} → \
                 {BASE_REVISION}+1; the same mix and byte length for every sample"
            ),
            notes: vec![
                "documented expectation: apply ns grows with the op payloads (touched \
                 regions), not with surface widget count — the base surface stays \
                 identical at every k here; the k axis is recorded raw, the sanity \
                 style bound of the scene bench is not imposed on this fly-weight \
                 surface"
                    .to_owned(),
            ],
        });
    }

    // The anti-pattern (ARCHITECTURE §14): ONE Update whose payload carries
    // the whole 4096-item list — the full-collection retransmit — measured
    // explicitly against the small-payload cost floor above.
    let fresh = big_list_surface(MAX_WIDGET_ROWS_INITIAL);
    let anti_patch = WidgetSurfacePatch {
        base_revision: SceneRevision(BASE_REVISION),
        revision: SceneRevision(BASE_REVISION + 1),
        ops: vec![WidgetOp::Update {
            id: StableId(BIG_LIST_ID),
            content: list_widget(BIG_LIST_ID, MAX_WIDGET_ROWS_INITIAL, BIG_ITEM_BASE),
        }],
    };
    let anti_ns = timed_samples(
        scenario,
        SAMPLES_SLOW_APPLY,
        3,
        || (fresh.clone(), anti_patch.clone()),
        |(surface, patch)| {
            let mut surface = surface;
            surface
                .apply_patch(black_box(patch))
                .expect("the same-shape exact re-adopt must apply");
            surface.revision.0
        },
        |_| "antipattern: the whole 4096-item payload re-carried by one Update"
            .to_owned(),
    );
    let anti_ns = WallPercentiles::from_samples(&anti_ns);
    let anti_patch_bytes = bincode_bytes(&anti_patch);
    println!(
        "[{scenario}:antipattern] full-window Update retransmit (4096 items) \
         p50={:.0} p95={:.0} p99={:.0} ns (n={}) | patch_bytes={anti_patch_bytes}",
        anti_ns.p50_ns, anti_ns.p95_ns, anti_ns.p99_ns, anti_ns.n,
    );
    // BASIS: the anti-pattern's per-op payload carries the whole window.
    let anti_record_note = format!(
        "ANTIPATTERN evidence: one Update of the {}-item list re-carries the whole \
         payload ({anti_patch_bytes} bytes bincode) — the full-collection retransmit \
         §14 calls a defect; the mix patches above re-carry only their touched \
         payloads",
        MAX_WIDGET_ROWS_INITIAL,
    );
    if let Some(record) = out.last_mut() {
        record.notes.push(anti_record_note.clone());
    }

    out.push(ScenarioRecord {
        scenario: format!("{scenario}:antipattern_full_window_update"),
        surface: "pub ocs_plugin_api::widgets::WidgetSurface::apply_patch — one Update \
                  carrying the whole 4096-item payload"
            .to_owned(),
        deferred: vec![],
        params: vec![
            param("payload_items", MAX_WIDGET_ROWS_INITIAL),
            param("samples", SAMPLES_SLOW_APPLY),
            param("patch_bytes_bincode", anti_patch_bytes),
        ],
        samples_n: anti_ns.n,
        wall_ns: anti_ns,
        extra_metrics: vec![],
        payload_bytes: Some(anti_patch_bytes as u64),
        payload_bytes_note: Some("the whole-payload retransmit measurement".to_owned()),
        counters: vec![],
        changed_elements: Some(MAX_WIDGET_ROWS_INITIAL as u64),
        total_elements: Some(MAX_WIDGET_ROWS_INITIAL as u64),
        changed_less_than_total: Some(false),
        sink: "same-shape exact re-adopt of the whole payload must apply".to_owned(),
        notes: vec![
            anti_record_note,
            "changed == total is INTENTIONAL here (changed_less_than_total records \
             false on purpose): the anti-pattern IS the exact re-carriage of the \
             whole window — the cost the incremental mixes avoid"
                .to_owned(),
        ],
    });
}

/// A 4096-scale single-list surface for the anti-pattern lane. One root
/// group + one list; ids distinct (a duplicate here previously made the
/// planner's `path_of` walk a self-referencing parent chain forever — the
/// OOM lesson — so every bench surface validates at construction).
fn big_list_surface(items: usize) -> WidgetSurface {
    let surface = WidgetSurface {
        revision: SceneRevision(BASE_REVISION),
        tables: std::collections::BTreeMap::new(),
        roots: vec![WidgetNode::Group(WidgetGroup {
            id: StableId(ROOT_GROUP_ID),
            name: None,
            enabled: true,
            visible: true,
            direction: Axis::Vertical,
            children: vec![list_widget(BIG_LIST_ID, items, BIG_ITEM_BASE)],
            split_ratio: None,
        })],
    };
    surface
        .validate()
        .expect("bench fixtures must validate (unique surface-wide ids)");
    surface
}

/// Widget semantic-node count of a surface (the 512-cap denominator).
fn surface_widget_nodes(surface: &WidgetSurface) -> usize {
    surface.widget_summary().widgets
}

/// The widget mix facts (bench-side bookkeeping of exactly what each patch
/// re-carries).
struct WidgetMix {
    updates: usize,
    distinct_update_slots: usize,
    adds: usize,
    removes: usize,
    /// Distinct widget nodes churned per patch.
    nodes: usize,
    /// Rows/items re-carried by the ops (update payload items only; adds
    /// and removes of buttons carry no row/item payload).
    item_regions: usize,
}

/// Build the deterministic k-op widget mix: the first `k/10` ops (capped at
/// 100) Remove pre-built buttons; after that every third op Adds a fresh
/// button and the other two Update small-list slots `(upd_counter * 7) % 40`
/// with a fresh-item 4-row payload. The THREE-way rhythm is capped-state
/// safe: the planner enforces the non-group widget budget per adopted node
/// (`MAX_WIDGETS_PER_PANEL`), and adds ≤ (512 - base widgets + removes)
/// keeps the post-patch surface inside the cap for every k in `PATCH_K`.
/// Bases at `BASE_REVISION`, successor +1.
fn build_widget_mix(k: usize) -> (WidgetSurfacePatch, WidgetMix) {
    let removes = (k / 10).min(100);
    let mut ops = Vec::new();
    let mut upd_counter = 0usize;
    let mut adds = 0usize;
    let mut slots: HashSet<usize> = HashSet::new();
    for j in 0..k {
        if j < removes {
            ops.push(WidgetOp::Remove {
                id: StableId(BUTTON_BASE + j as u64),
            });
        } else if (j - removes) % 3 == 0 {
            ops.push(WidgetOp::Add {
                parent: Option::Some(StableId(ROOT_GROUP_ID)),
                widget: icon_button(PATCH_ADD_BASE + j as u64, j),
            });
            adds += 1;
        } else {
            let slot = (upd_counter * 7) % SMALL_LISTS;
            ops.push(WidgetOp::Update {
                id: StableId(SMALL_LIST_BASE + slot as u64),
                content: list_widget(
                    SMALL_LIST_BASE + slot as u64,
                    SMALL_ITEMS_PER_LIST,
                    PATCH_ITEM_BASE + j as u64 * 100,
                ),
            });
            slots.insert(slot);
            upd_counter += 1;
        }
    }
    let mix = WidgetMix {
        updates: upd_counter,
        distinct_update_slots: slots.len(),
        adds,
        removes,
        nodes: removes + slots.len() + adds,
        // DISTINCT row/item regions the updates re-carry (revisited slots
        // count once): the per-sync projection cost is per region, not per
        // op.
        item_regions: slots.len() * SMALL_ITEMS_PER_LIST,
    };
    (
        WidgetSurfacePatch {
            base_revision: SceneRevision(BASE_REVISION),
            revision: SceneRevision(BASE_REVISION + 1),
            ops,
        },
        mix,
    )
}

// === scenario_3 ===

/// Scenario 3: selection/window math. Three lanes with explicit honesty:
/// (a) the ONE pub window type (`WidgetRange::bounds_ok`), (b) a
/// BENCH-SIDE EMULATION of the documented O(window) id materialization
/// (the real `materialize_ids`/`visible_window` are pub(crate) — recorded,
/// not measured here), and (c) the O(total) anti-pattern the windowing
/// avoids, plus the payload cap gate that structurally FORCES windowing
/// past 4096 rows (4097 rejects at validate).
fn scenario_selection_window_math(out: &mut Vec<ScenarioRecord>) {
    let scenario = "selection_window_math";

    // (a) WidgetRange::bounds_ok across real window shapes, rotating per
    // sample; the len > cap case must Err.
    let ranges: [(usize, usize, bool); 5] = [
        (0, 1, true),
        (0, 64, true),
        (VIRTUAL_ROWS - 64, 64, true),
        (1, MAX_WIDGET_RANGE_LEN, true),
        (3, MAX_WIDGET_RANGE_LEN + 1, false),
    ];
    let rot = Cell::new(0usize);
    let range_ns = timed_samples(
        scenario,
        SAMPLES_READS,
        5,
        || ranges[rot.get() % ranges.len()],
        |(start, len, ok)| {
            let range = WidgetRange { start, len };
            let result = range.bounds_ok();
            assert_eq!(result.is_ok(), ok, "bounds_ok must follow the contract");
            result.is_ok()
        },
        |valid| {
            format!(
                "WidgetRange bounds_ok ({} start/len combos) contract={valid}",
                ranges.len()
            )
        },
    );
    let range_ns = WallPercentiles::from_samples(&range_ns);
    println!(
        "[{scenario}/bounds_ok] p50={:.0} p95={:.0} p99={:.0} ns (n={})",
        range_ns.p50_ns, range_ns.p95_ns, range_ns.p99_ns, range_ns.n,
    );
    out.push(ScenarioRecord {
        scenario: format!("{scenario}:range_bounds_ok"),
        surface: "pub ocs_plugin_api::widgets::WidgetRange::bounds_ok — the ONLY pub \
                  window-math type today"
            .to_owned(),
        deferred: vec![
            "visible_window / materialize_ids / VisibleWindow are pub(crate) in \
             src/ui/window/plugin_widgets.rs — windowing wall-clock waits for the \
             mount task; today's evidence: `window_basics_and_degenerate_specs` and \
             `expanding_one_branch_mutates_only_that_branch` unit tests"
                .to_owned(),
        ],
        params: vec![
            param("combos", ranges.len()),
            param("range_len_cap", MAX_WIDGET_RANGE_LEN),
            param("samples", SAMPLES_READS),
        ],
        samples_n: range_ns.n,
        wall_ns: range_ns,
        extra_metrics: vec![],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![],
        changed_elements: None,
        total_elements: None,
        changed_less_than_total: None,
        sink: "bounds_ok must accept the in-cap combos and reject len=4097 exactly as \
               the contract states".to_owned(),
        notes: vec![],
    });

    // (b) The EMULATED O(window) materialization over a virtual 100 000-row
    // list: per sample, w ids materialized for a rotating offset.
    for &w in &WINDOW_SIZES {
        let rot = Cell::new(0usize);
        let win_ns = timed_samples(
            scenario,
            SAMPLES_READS,
            5,
            || rot.get() % (VIRTUAL_ROWS - w),
            |offset| {
                let ids: Vec<StableId> = (0..w)
                    .map(|i| StableId(50_000_000 + (offset + i) as u64))
                    .collect();
                black_box(ids)
            },
            |ids| {
                format!(
                    "materialized {} of {VIRTUAL_ROWS} rows ok=O(window) {}",
                    ids.len(),
                    acceptance_line(w, VIRTUAL_ROWS),
                )
            },
        );
        let win_ns = WallPercentiles::from_samples(&win_ns);
        assert_eq!(win_ns.n, SAMPLES_READS);
        println!(
            "[{scenario}/w={w}] emulate-materialize p50={:.0} p95={:.0} p99={:.0} \
             ns (n={}) | {w} of {VIRTUAL_ROWS} rows",
            win_ns.p50_ns, win_ns.p95_ns, win_ns.p99_ns, win_ns.n,
        );
        out.push(ScenarioRecord {
            scenario: format!("{scenario}:materialize_window={w}"),
            surface: "BENCH-SIDE EMULATION of the documented O(window) materialize \
                      contract (windowed list, arithmetic ids) — NOT the pub(crate) \
                      implementation"
                .to_owned(),
            deferred: vec![
                "the REAL materialize_ids is pub(crate) (plugin_widgets.rs) — its cost \
                 discipline is asserted by unit tests; wall-clock numbers deferred to \
                 the mount task (SCN-012 narrowed scope)"
                    .to_owned(),
            ],
            params: vec![
                param("window_rows", w),
                param("virtual_rows", VIRTUAL_ROWS),
                param("samples", SAMPLES_READS),
            ],
            samples_n: win_ns.n,
            wall_ns: win_ns,
            extra_metrics: vec![],
            payload_bytes: None,
            payload_bytes_note: None,
            counters: vec![Counter {
                name: "host::materialize_ids".to_owned(),
                scope: "deferred",
                value: None,
                note: Some(
                    "pub(crate); the emulation above only demonstrates the documented \
                     O(window) contract shape"
                        .to_owned(),
                ),
            }],
            changed_elements: Some(w as u64),
            total_elements: Some(VIRTUAL_ROWS as u64),
            changed_less_than_total: Some(w < VIRTUAL_ROWS),
            sink: format!(
                "every sample materializes exactly {w} ids — a W-shape O({w}) window \
                 over a 100 000-row virtual list"
            ),
            notes: vec![],
        });
    }

    // (c) The anti-pattern: materializing the FULL virtual list.
    let full_ns = timed_ns(20, 2, || {
        let ids: Vec<StableId> = (0..VIRTUAL_ROWS)
            .map(|i| StableId(50_000_000 + i as u64))
            .collect();
        black_box(ids);
    });
    let full_ns = WallPercentiles::from_samples(&full_ns);
    println!(
        "[{scenario}/full_100k] antipattern p50={:.0} p95={:.0} p99={:.0} ns (n={})",
        full_ns.p50_ns, full_ns.p95_ns, full_ns.p99_ns, full_ns.n,
    );
    out.push(ScenarioRecord {
        scenario: format!("{scenario}:materialize_full_100k_antipattern"),
        surface: "bench-side O(total) anti-pattern (full id materialization)".to_owned(),
        deferred: vec![],
        params: vec![param("virtual_rows", VIRTUAL_ROWS), param("samples", 20)],
        samples_n: full_ns.n,
        wall_ns: full_ns,
        extra_metrics: vec![],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![],
        changed_elements: Some(VIRTUAL_ROWS as u64),
        total_elements: Some(VIRTUAL_ROWS as u64),
        changed_less_than_total: Some(false),
        sink: "the O(total) lane exists to give the window lanes a denominator \
               reference, not an acceptance number"
            .to_owned(),
        notes: vec![
            "compare against the O(window) lanes above: this is the cost 100k rows \
             would pay per draw if the windowing discipline were dropped"
                .to_owned(),
        ],
    });

    // (d) The payload cap gate: 4096 == cap validates; 4097 rejects with the
    // exact first-crossing budget error.
    let cap_surface = surface_with_list(MAX_WIDGET_ROWS_INITIAL);
    assert!(matches!(
        cap_surface.validate(),
        Ok(())
    ), "4096 rows == cap must validate");
    let over_surface = surface_with_list(MAX_WIDGET_ROWS_INITIAL + 1);
    let gate_ns = timed_ns(SAMPLES_FAST, 3, || {
        match black_box(&over_surface).validate() {
            Err(WidgetError::BudgetExceeded { current, max }) => {
                assert_eq!(current, MAX_WIDGET_ROWS_INITIAL + 1);
                assert_eq!(max, MAX_WIDGET_ROWS_INITIAL);
            }
            other => panic!("4097 items must reject at validate, got {other:?}"),
        }
    });
    let gate_ns = WallPercentiles::from_samples(&gate_ns);
    println!(
        "[{scenario}/cap_gate] reject-4097 p50={:.0} p95={:.0} p99={:.0} ns (n={})",
        gate_ns.p50_ns, gate_ns.p95_ns, gate_ns.p99_ns, gate_ns.n,
    );
    out.push(ScenarioRecord {
        scenario: format!("{scenario}:payload_cap_gate"),
        surface: "pub ocs_plugin_api::widgets::WidgetSurface::validate — the 100k \
                  windowing gate at the payload cap"
            .to_owned(),
        deferred: vec![],
        params: vec![
            param("accepted_rows", MAX_WIDGET_ROWS_INITIAL),
            param("rejected_rows", MAX_WIDGET_ROWS_INITIAL + 1),
            param("samples", SAMPLES_FAST),
        ],
        samples_n: gate_ns.n,
        wall_ns: gate_ns,
        extra_metrics: vec![],
        payload_bytes: None,
        payload_bytes_note: None,
        counters: vec![Counter {
            name: "rows_past_cap_rejected".to_owned(),
            scope: "measured",
            value: Some(1),
            note: Some(format!(
                "the 100k discipline is structural: a {}-item payload cannot reach the \
                 host ledger — plugins that back 100 000 rows MUST window via windowed \
                 payloads and lazy children",
                MAX_WIDGET_ROWS_INITIAL + 1,
            )),
        }],
        changed_elements: None,
        total_elements: Some(MAX_WIDGET_ROWS_INITIAL as u64),
        changed_less_than_total: None,
        sink: format!(
            "4096-item payload validates; {}-item payload rejects with \
             BudgetExceeded(current={}, max={}) — asserted per sample",
            MAX_WIDGET_ROWS_INITIAL + 1,
            MAX_WIDGET_ROWS_INITIAL + 1,
            MAX_WIDGET_ROWS_INITIAL,
        ),
        notes: vec![
            "this gate is the CONTRACT basis for the 100k-item claim: surfaces back \
             arbitrary row counts only through windowed payloads + lazy children"
                .to_owned(),
        ],
    });
}

/// Single-list surface builder for the cap gate.
fn surface_with_list(items: usize) -> WidgetSurface {
    WidgetSurface {
        revision: SceneRevision(BASE_REVISION),
        tables: std::collections::BTreeMap::new(),
        roots: vec![WidgetNode::Group(WidgetGroup {
            id: StableId(ROOT_GROUP_ID),
            name: None,
            enabled: true,
            visible: true,
            direction: Axis::Vertical,
            children: vec![list_widget(BIG_LIST_ID, items, BIG_ITEM_BASE)],
            split_ratio: None,
        })],
    }
}

fn main() {
    let mut scenarios: Vec<ScenarioRecord> = Vec::new();
    scenario_surface_scale(&mut scenarios);
    scenario_widget_patch_scaling(&mut scenarios);
    scenario_selection_window_math(&mut scenarios);
    emit_run_record("widget_state_scaling", scenarios);
}
