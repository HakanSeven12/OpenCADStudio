use super::*;
use crate::perf::audit;
use document::{entities::Line, types::Vector3, Handle};
use std::{hint::black_box, time::Instant};

// Pre-change policy copied from c0ccb87d0; its original estimate lives next to
// HistorySnapshot under the same opt-in test configuration.
fn legacy_trim(
    history: &mut HistoryState,
    max_entries: usize,
    max_bytes: usize,
) -> Vec<HistorySnapshot> {
    let mut entries = history
        .undo_stack
        .len()
        .saturating_add(history.redo_stack.len());
    let mut bytes = history
        .undo_stack
        .iter()
        .chain(&history.redo_stack)
        .fold(0usize, |sum, item| {
            sum.saturating_add(item.performance_audit_legacy_bytes())
        });
    let mut discarded = Vec::new();
    while entries > max_entries || bytes > max_bytes {
        let item = if !history.undo_stack.is_empty() {
            history.undo_stack.remove(0)
        } else if !history.redo_stack.is_empty() {
            history.redo_stack.remove(0)
        } else {
            break;
        };
        entries -= 1;
        bytes = bytes.saturating_sub(item.performance_audit_legacy_bytes());
        discarded.push(item);
    }
    discarded
}

fn fixture(entries: usize, width: usize, distinct: bool) -> HistoryState {
    let mut history = HistoryState::default();
    let mut versions: Vec<_> = (0..width)
        .map(|i| {
            Arc::new(EntityType::Line(Line::from_points(
                Vector3::new(i as f64, 0.0, 0.0),
                Vector3::new(i as f64, 1.0, 0.0),
            )))
        })
        .collect();
    for step in 0..entries {
        let mut entry = super::tests::snapshot(versions[0].clone());
        let HistorySnapshot::Delta(delta) = &mut entry else {
            unreachable!()
        };
        delta.entities = versions
            .iter_mut()
            .enumerate()
            .map(|(i, current)| {
                let before = current.clone();
                if distinct {
                    *current = Arc::new(EntityType::Line(Line::from_points(
                        Vector3::new(i as f64, step as f64, 0.0),
                        Vector3::new(i as f64, step as f64 + 1.0, 0.0),
                    )));
                }
                (
                    Handle::new(i as u64 + 1),
                    Some(before),
                    Some(current.clone()),
                )
            })
            .collect();
        history.undo_stack.push(entry);
    }
    history
}

fn timed(source: &HistoryState, modern: bool, entries: usize, bytes: usize) -> f64 {
    // Cloning the shared fixture is setup, not the history-trimming operation.
    let mut history = HistoryState {
        undo_stack: source.undo_stack.clone(),
        redo_stack: source.redo_stack.clone(),
        pending: None,
    };
    let start = Instant::now();
    let discarded = if modern {
        trim(&mut history, entries, bytes)
    } else {
        legacy_trim(&mut history, entries, bytes)
    };
    let elapsed = start.elapsed().as_secs_f64() * 1e6;
    black_box((&history, &discarded));
    // Snapshot destruction is deferred by the production caller. Exclude it
    // from both arms, but include temporary accounting-map destruction.
    elapsed
}

#[test]
#[ignore = "release performance audit"]
fn performance_audit_history() {
    assert!(!cfg!(debug_assertions), "use --release");
    let mut results = Vec::new();
    for (name, entries, width, distinct, limit, bytes) in [
        ("small_256x1", 256, 1, true, 256, 512 * 1024 * 1024),
        ("medium_128x100", 128, 100, true, 256, 512 * 1024 * 1024),
        (
            "dense_shared_256x1000",
            256,
            1000,
            false,
            256,
            512 * 1024 * 1024,
        ),
        (
            "dense_versions_64x1000",
            64,
            1000,
            true,
            256,
            512 * 1024 * 1024,
        ),
        (
            "entry_eviction_default_budget",
            512,
            1,
            true,
            256,
            512 * 1024 * 1024,
        ),
        (
            "entry_eviction_unlimited_bytes",
            512,
            1,
            true,
            256,
            usize::MAX,
        ),
    ] {
        let source = fixture(entries, width, distinct);
        for _ in 0..3 {
            timed(&source, false, limit, bytes);
            timed(&source, true, limit, bytes);
        }
        let comparison = audit::paired(
            100,
            || timed(&source, false, limit, bytes),
            || timed(&source, true, limit, bytes),
        );
        let mut old = HistoryState {
            undo_stack: source.undo_stack.clone(),
            ..Default::default()
        };
        let mut new = HistoryState {
            undo_stack: source.undo_stack.clone(),
            ..Default::default()
        };
        drop(legacy_trim(&mut old, limit, bytes));
        drop(trim(&mut new, limit, bytes));
        assert_eq!(
            old.undo_stack.len(),
            new.undo_stack.len(),
            "fixture must compare equivalent retained histories"
        );
        eprintln!(
            "HISTORY {name}: {:.3} -> {:.3} us median; {:.3} -> {:.3} us p95",
            comparison.before.median_us,
            comparison.after.median_us,
            comparison.before.p95_us,
            comparison.after.p95_us
        );
        results.push(serde_json::json!({"case":name,"entries":entries,"images_per_entry":width,
            "distinct_versions":distinct,"remaining_entries":new.undo_stack.len(),"timing":comparison}));
    }
    audit::write_report(
        "history",
        serde_json::json!({
            "scope":"history accounting/eviction CPU; excludes fixture cloning and deferred snapshot destruction; identical retained entry counts in both arms; baseline policy/estimate copied from c0ccb87d0",
            "cases":results,
        }),
    );
}
