use super::*;
use crate::perf::audit;
use std::{hint::black_box, time::Instant};

const LIMIT: usize = 512 * 1024 * 1024;

fn initialize(history: &mut HistoryState, modern: bool, bytes: usize) {
    if modern {
        assert!(trim(history, usize::MAX, bytes).is_empty());
    }
}

fn timed_append(
    source: &HistoryState,
    entry: &HistorySnapshot,
    modern: bool,
    entries: usize,
    bytes: usize,
) -> f64 {
    let mut history = raw_clone(source);
    initialize(&mut history, modern, bytes);
    // Reserve equally in both arms, matching a running stack with headroom.
    history.undo_stack.0.reserve(1);
    let entry = entry.clone();
    let start = Instant::now();
    let (redo, discarded) = if modern {
        history.push_undo(entry);
        let redo = history.take_redo();
        (redo, trim(&mut history, entries, bytes))
    } else {
        history.undo_stack.0.push(entry);
        let redo = std::mem::take(&mut history.redo_stack.0);
        (redo, scan_trim(&mut history, entries, bytes))
    };
    let elapsed = start.elapsed().as_secs_f64() * 1e6;
    black_box((&history, &redo, &discarded));
    assert_eq!(
        history.undo_stack.len(),
        (source.undo_stack.len() + 1).min(entries)
    );
    assert!(history.redo_stack.is_empty());
    if modern && bytes != usize::MAX {
        assert_census(&history);
        assert!(history.budget.as_ref().unwrap().total() <= bytes);
    }
    elapsed
}

fn timed_check(source: &HistoryState, modern: bool) -> f64 {
    let mut history = raw_clone(source);
    initialize(&mut history, modern, LIMIT);
    let start = Instant::now();
    let discarded = if modern {
        trim(&mut history, 256, LIMIT)
    } else {
        scan_trim(&mut history, 256, LIMIT)
    };
    let elapsed = start.elapsed().as_secs_f64() * 1e6;
    black_box((&history, &discarded));
    assert!(discarded.is_empty());
    elapsed
}

fn timed_replay(source: &HistoryState, modern: bool) -> f64 {
    let mut history = raw_clone(source);
    initialize(&mut history, modern, LIMIT);
    history.redo_stack.0.reserve(1);
    let start = Instant::now();
    for _ in 0..1000 {
        if modern {
            let step = history.take_replay(true).unwrap();
            history.finish_replay(black_box(step), true);
            let step = history.take_replay(false).unwrap();
            history.finish_replay(black_box(step), false);
        } else {
            let step = history.undo_stack.0.pop().unwrap();
            history.redo_stack.0.push(black_box(step));
            let step = history.redo_stack.0.pop().unwrap();
            history.undo_stack.0.push(black_box(step));
        }
    }
    let elapsed = start.elapsed().as_secs_f64() * 1e6 / 1000.0;
    black_box(&history);
    if modern {
        assert_census(&history);
    }
    elapsed
}

fn entry(source: &HistoryState, width: usize, generation: f64) -> HistorySnapshot {
    let Some(HistorySnapshot::Delta(previous)) = source.undo_stack.last() else {
        unreachable!()
    };
    let mut entry = snapshot(Arc::new(EntityType::Line(Line::new())));
    let HistorySnapshot::Delta(delta) = &mut entry else {
        unreachable!()
    };
    delta.entities = (0..width)
        .map(|i| {
            let line = Line::from_points(
                document::types::Vector3::new(i as f64, generation, 0.0),
                document::types::Vector3::new(i as f64, generation + 1.0, 0.0),
            );
            (
                Handle::new(i as u64 + 1),
                previous.entities[i].2.clone(),
                Some(Arc::new(EntityType::Line(line))),
            )
        })
        .collect();
    entry
}

#[test]
#[ignore = "release incremental history A/B audit, no allocation counting"]
fn performance_audit_history_incremental_latency() {
    assert!(!cfg!(debug_assertions), "use --release");
    let mut cases = Vec::new();
    for (name, entries, width, distinct, edit_width, limit, bytes) in [
        ("small_append_1", 256, 1, true, 1, 256, LIMIT),
        ("medium_append_100", 128, 100, true, 100, 256, LIMIT),
        (
            "dense_shared_append_1000",
            256,
            1000,
            false,
            1000,
            256,
            LIMIT,
        ),
        (
            "dense_versions_append_1000",
            64,
            1000,
            true,
            1000,
            256,
            LIMIT,
        ),
        ("dense_versions_append_1", 64, 1000, true, 1, 256, LIMIT),
        ("small_append_unlimited", 256, 1, true, 1, 256, usize::MAX),
    ] {
        let source = super::super::performance_audit::fixture(entries, width, distinct);
        let edit = entry(&source, edit_width, 10000.0);
        for _ in 0..3 {
            timed_append(&source, &edit, false, limit, bytes);
            timed_append(&source, &edit, true, limit, bytes);
        }
        let timing = audit::paired(
            100,
            || timed_append(&source, &edit, false, limit, bytes),
            || timed_append(&source, &edit, true, limit, bytes),
        );
        eprintln!(
            "INCREMENTAL {name}: {:.3} -> {:.3} us median; {:.3} -> {:.3} us p95",
            timing.before.median_us,
            timing.after.median_us,
            timing.before.p95_us,
            timing.after.p95_us
        );
        cases.push(serde_json::json!({"case":name,"retained_entries":entries,
            "images_per_retained_entry":width,"entities_per_new_entry":edit_width,
            "new_entry_images":"before-image shared with preceding after-image; newly allocated after-image",
            "operation":"push new undo entry, clear redo, trim; no deferred destruction",
            "timing":timing}));
    }
    let mut redo = super::super::performance_audit::fixture(64, 100, true);
    let tail = redo.undo_stack.0.split_off(32);
    redo.redo_stack.0 = tail;
    let edit = entry(&redo, 100, 10000.0);
    let timing = audit::paired(
        100,
        || timed_append(&redo, &edit, false, 256, LIMIT),
        || timed_append(&redo, &edit, true, 256, LIMIT),
    );
    eprintln!(
        "INCREMENTAL redo_clear_32x100: {:.3} -> {:.3} us median",
        timing.before.median_us, timing.after.median_us
    );
    cases.push(serde_json::json!({"case":"redo_clear_32x100", "undo_entries":32,
        "redo_entries":32,"images_per_retained_entry":100,"entities_per_new_entry":100,"timing":timing}));
    let dense = super::super::performance_audit::fixture(64, 1000, true);
    let check = audit::paired(
        100,
        || timed_check(&dense, false),
        || timed_check(&dense, true),
    );
    let replay = audit::paired(
        100,
        || timed_replay(&dense, false),
        || timed_replay(&dense, true),
    );
    eprintln!(
        "INCREMENTAL no-op check: {:.3} -> {:.3} us; replay roundtrip: {:.3} -> {:.3} us",
        check.before.median_us,
        check.after.median_us,
        replay.before.median_us,
        replay.after.median_us
    );
    audit::write_report(
        "history-incremental-latency",
        serde_json::json!({
            "baseline":"full-scan trim copied from ea48570fc; same payload estimator and current dependencies",
            "scope":"CPU bookkeeping; 100 alternating paired samples after warmup for append cases; fixture/census construction and deferred snapshot destruction excluded; incremental push/refcounts/redo invalidation included; exact census verified outside timing; replay is stack/accounting only, not full scene replay",
            "cases":cases,"dense_noop_check":check,"replay_roundtrip_1000_images":replay,
        }),
    );
}

fn retained_vertices(history: &HistoryState) -> usize {
    let mut seen = rustc_hash::FxHashSet::default();
    history
        .undo_stack
        .iter()
        .chain(&history.redo_stack)
        .flat_map(|entry| {
            let HistorySnapshot::Delta(delta) = entry else {
                unreachable!()
            };
            delta
                .entities
                .iter()
                .flat_map(|(_, before, after)| before.iter().chain(after))
        })
        .filter(|entity| seen.insert(Arc::as_ptr(entity) as usize))
        .map(|entity| {
            let EntityType::LwPolyline(line) = entity.as_ref() else {
                unreachable!()
            };
            capacity(&line.vertices)
        })
        .sum()
}

fn retention_arm(strategy: &str) -> serde_json::Value {
    let mut history = HistoryState::default();
    for i in 0..160 {
        let mut line = LwPolyline::new();
        line.vertices.reserve_exact(100_000);
        for vertex in 0..100_000 {
            line.vertices.push(document::entities::LwVertex::new(
                document::types::Vector2::new(vertex as f64, i as f64),
            ));
        }
        let mut entry = snapshot(Arc::new(EntityType::LwPolyline(line)));
        let HistorySnapshot::Delta(delta) = &mut entry else {
            unreachable!()
        };
        delta.entities[0].0 = Handle::new(i + 1);
        delta.entities[0].2 = None;
        if strategy == "after_incremental" {
            history.push_undo(entry);
            drop(trim(&mut history, 256, LIMIT));
            assert_census(&history);
            assert!(history.budget.as_ref().unwrap().total() <= LIMIT);
        } else {
            history.undo_stack.0.push(entry);
            if strategy == "before_full_scan" {
                drop(scan_trim(&mut history, 256, LIMIT));
            } else {
                let mut estimate: usize = history
                    .undo_stack
                    .iter()
                    .map(HistorySnapshot::performance_audit_legacy_bytes)
                    .sum();
                while history.undo_stack.len() > 256 || estimate > LIMIT {
                    let removed = history.undo_stack.0.remove(0);
                    estimate -= removed.performance_audit_legacy_bytes();
                    drop(removed);
                }
            }
        }
    }
    let mut census = HistoryBudget::default();
    for entry in &history.undo_stack {
        census.retain(entry);
    }
    let metadata = history.budget.as_ref().map_or(0, |b| b.total() - b.bytes);
    serde_json::json!({"strategy":strategy,"retained_entries":history.undo_stack.len(),
        "retained_vertex_capacity_bytes":retained_vertices(&history),
        "retained_payload_and_snapshot_estimate_bytes":census.bytes,
        "persistent_census_inline_bytes":if strategy == "after_incremental" { size_of::<Option<HistoryBudget>>() } else { 0 },
        "persistent_census_metadata_allowance_bytes":metadata,
        "total_budget_estimate_bytes":census.bytes + metadata})
}

#[test]
#[ignore = "release retained memory audit; about 733 MiB in the legacy arm"]
fn performance_audit_history_incremental_retention() {
    assert!(!cfg!(debug_assertions), "use --release");
    let mut runs = Vec::new();
    for _ in 0..3 {
        let legacy = retention_arm("legacy_before_payload_budget");
        let before = retention_arm("before_full_scan");
        let after = retention_arm("after_incremental");
        assert_eq!(legacy["retained_entries"], 160);
        assert_eq!(before["retained_entries"], 111);
        assert_eq!(before["retained_entries"], after["retained_entries"]);
        assert_eq!(
            before["retained_vertex_capacity_bytes"],
            after["retained_vertex_capacity_bytes"]
        );
        assert_eq!(
            before["retained_payload_and_snapshot_estimate_bytes"],
            after["retained_payload_and_snapshot_estimate_bytes"]
        );
        assert!(after["total_budget_estimate_bytes"].as_u64().unwrap() <= LIMIT as u64);
        eprintln!(
            "RETENTION {}",
            serde_json::json!({"legacy":legacy,"before":before,"after":after})
        );
        runs.push(serde_json::json!({"legacy":legacy,"before":before,"after":after}));
    }
    audit::write_report(
        "history-incremental-retention",
        serde_json::json!({
            "scope":"three sequential runs of each strategy; 160 populated polylines x 100000 vertices; enforce budget after each recorded edit and synchronously drop discarded entries; unique vertex-buffer capacity, snapshot/payload estimator and conservative persistent-census metadata allowance; excludes allocator overhead, RSS and deferred-drop backlog; no counting allocator",
            "byte_budget":LIMIT,"entry_budget":256,"runs":runs,
        }),
    );
}
