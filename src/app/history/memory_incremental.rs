//! Regression coverage and release A/B evidence for the incremental census.
use super::tests::snapshot;
use super::*;
use document::{
    entities::{Line, LwPolyline, RawRecord},
    Handle,
};

// Exact full-scan policy from ea48570fc, with raw stack access for the A/B arm.
#[cfg(feature = "performance-audit")]
mod scan_reference {
    use super::*;
    #[derive(Default)]
    struct SharedBytes(FxHashMap<usize, (usize, usize)>);

    impl SharedBytes {
        fn retain(&mut self, key: usize, cost: impl FnOnce() -> usize) -> Option<usize> {
            use std::collections::hash_map::Entry;
            match self.0.entry(key) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().1 += 1;
                    None
                }
                Entry::Vacant(entry) => {
                    let bytes = cost();
                    entry.insert((bytes, 1));
                    Some(bytes)
                }
            }
        }

        fn release(&mut self, key: usize) -> Option<usize> {
            let entry = self.0.get_mut(&key).expect("retained history allocation");
            entry.1 -= 1;
            if entry.1 == 0 {
                self.0.remove(&key).map(|entry| entry.0)
            } else {
                None
            }
        }
    }

    #[derive(Default)]
    struct EntityBudget {
        entities: SharedBytes,
        records: SharedBytes,
    }

    impl EntityBudget {
        fn retain(&mut self, entity: &Arc<EntityType>) -> usize {
            let Some(mut bytes) = self
                .entities
                .retain(Arc::as_ptr(entity) as usize, || entity_bytes(entity))
            else {
                return 0;
            };
            if let Some(record) = &entity.common().raw_record {
                bytes = bytes.saturating_add(
                    self.records
                        .retain(Arc::as_ptr(record) as usize, || {
                            size_of_val(record.as_ref())
                                + 2 * size_of::<usize>()
                                + record.data.capacity()
                        })
                        .unwrap_or(0),
                );
            }
            bytes
        }

        fn release(&mut self, entity: &Arc<EntityType>) -> usize {
            let Some(mut bytes) = self.entities.release(Arc::as_ptr(entity) as usize) else {
                return 0;
            };
            if let Some(record) = &entity.common().raw_record {
                bytes = bytes.saturating_add(
                    self.records
                        .release(Arc::as_ptr(record) as usize)
                        .unwrap_or(0),
                );
            }
            bytes
        }

        fn snapshot(&mut self, snapshot: &HistorySnapshot, retain: bool) -> usize {
            let HistorySnapshot::Delta(delta) = snapshot else {
                return 0;
            };
            delta
                .entities
                .iter()
                .flat_map(|(_, before, after)| before.iter().chain(after))
                .fold(0usize, |bytes, entity| {
                    bytes.saturating_add(if retain {
                        self.retain(entity)
                    } else {
                        self.release(entity)
                    })
                })
        }
    }

    pub(super) fn trim(
        history: &mut HistoryState,
        max_entries: usize,
        max_bytes: usize,
    ) -> Vec<HistorySnapshot> {
        let mut entities = EntityBudget::default();
        let mut entries = history
            .undo_stack
            .len()
            .saturating_add(history.redo_stack.len());
        let mut bytes = 0usize;
        if max_bytes != usize::MAX {
            for snapshot in history.undo_stack.iter().chain(&history.redo_stack) {
                bytes = bytes
                    .saturating_add(snapshot.estimated_overhead_bytes())
                    .saturating_add(entities.snapshot(snapshot, true));
            }
        }
        let mut undo = 0;
        let mut redo = 0;
        for (is_redo, snapshot) in history
            .undo_stack
            .iter()
            .map(|s| (false, s))
            .chain(history.redo_stack.iter().map(|s| (true, s)))
        {
            if entries <= max_entries && bytes <= max_bytes {
                break;
            }
            entries -= 1;
            if max_bytes != usize::MAX {
                bytes = bytes
                    .saturating_sub(snapshot.estimated_overhead_bytes())
                    .saturating_sub(entities.snapshot(snapshot, false));
            }
            if is_redo {
                redo += 1;
            } else {
                undo += 1;
            }
        }
        let mut discarded = Vec::with_capacity(undo + redo);
        discarded.extend(history.undo_stack.0.drain(..undo));
        discarded.extend(history.redo_stack.0.drain(..redo));
        discarded
    }
}

#[cfg(feature = "performance-audit")]
use scan_reference::trim as scan_trim;

#[cfg(feature = "performance-audit")]
fn raw_clone(source: &HistoryState) -> HistoryState {
    HistoryState {
        undo_stack: HistoryStack(source.undo_stack.0.clone()),
        redo_stack: HistoryStack(source.redo_stack.0.clone()),
        ..Default::default()
    }
}

fn assert_census(history: &HistoryState) {
    let budget = history.budget.as_ref().expect("initialized census");
    let mut reference = HistoryBudget::default();
    for snapshot in history.undo_stack.iter().chain(&history.redo_stack) {
        reference.retain(snapshot);
    }
    assert_eq!(budget.bytes, reference.bytes, "cached retained bytes");
    assert_eq!(
        budget.entities.entities.0, reference.entities.entities.0,
        "entity identities/refcounts"
    );
    assert_eq!(
        budget.entities.records.0, reference.entities.records.0,
        "raw record identities/refcounts"
    );
}

#[test]
fn incremental_metadata_keeps_allocation_high_water_until_rebuilt() {
    let mut shared = SharedBytes::default();
    for key in 0..1000 {
        shared.retain(key, || 64);
    }
    let allocated = shared.1;
    for key in 0..900 {
        shared.release(key);
    }
    assert_eq!(shared.1, allocated);
    assert_eq!(
        shared.metadata_bytes(),
        allocated * 2 * (size_of::<(usize, (usize, usize))>() + 1)
    );
    shared.compact();
    assert!(shared.1 < allocated);
    for key in 900..1000 {
        shared.release(key);
    }
    shared.compact();
    assert_eq!(shared.metadata_bytes(), 0);
}

#[test]
fn incremental_census_tracks_edits_replay_redo_clear_and_discards() {
    let record = Arc::new(RawRecord {
        data: Vec::with_capacity(4096),
        handle_bits: 0,
        version: document::DxfVersion::AC1032,
    });
    let images: Vec<_> = (0..17)
        .map(|i| {
            let mut line = Line::new();
            line.common.layer = format!("layer {i}");
            line.common.raw_record = Some(record.clone());
            Arc::new(EntityType::Line(line))
        })
        .collect();
    let mut history = HistoryState::default();
    drop(trim(&mut history, 23, 64 * 1024));
    let mut random = 0x938a_1347u64;
    for step in 0..2000 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        match random % 7 {
            0 | 1 => history.push_undo(snapshot(images[step % images.len()].clone())),
            2 => {
                drop(history.pop_undo());
            }
            3 => {
                drop(history.take_redo());
            }
            4 | 5 => {
                let undo = random % 7 == 4;
                if let Some(mut replay) = history.take_replay(undo) {
                    if let HistorySnapshot::Delta(delta) = &mut replay.snapshot {
                        // Replay can change structure-owned overhead while
                        // retaining identical entity images.
                        delta.label = format!("replay {step}").into();
                    }
                    history.finish_replay(replay, undo);
                }
            }
            _ => {
                if let Some(mut entry) = history.last_undo_mut() {
                    if let HistorySnapshot::Delta(delta) = &mut *entry {
                        delta.entities.push((
                            Handle::new(step as u64 + 1),
                            None,
                            Some(images[step % images.len()].clone()),
                        ));
                        delta.entities[0].2 = None;
                        delta.selected_after.push(Handle::new(step as u64 + 1));
                    }
                }
            }
        }
        assert_census(&history);
        drop(trim(&mut history, 23, 64 * 1024));
        assert_census(&history);
        assert!(history.budget.as_ref().unwrap().total() <= 64 * 1024);
    }
    drop(trim(&mut history, 0, 0));
    assert_census(&history);
    assert_eq!(history.budget.as_ref().unwrap().total(), 0);
    assert_eq!(Arc::strong_count(&record), images.len() + 1);
}

#[test]
fn incremental_census_releases_payloads_and_compacts_metadata() {
    let mut history = HistoryState::default();
    drop(trim(&mut history, usize::MAX, 512 * 1024 * 1024));
    let mut weak = Vec::new();
    for _ in 0..4096 {
        let image = Arc::new(EntityType::Line(Line::new()));
        weak.push(Arc::downgrade(&image));
        history.push_undo(snapshot(image));
    }
    assert!(
        history
            .budget
            .as_ref()
            .unwrap()
            .entities
            .entities
            .0
            .capacity()
            >= 4096
    );
    drop(trim(&mut history, 1, 512 * 1024 * 1024));
    assert_census(&history);
    assert_eq!(
        weak.iter()
            .filter(|image| image.upgrade().is_some())
            .count(),
        1
    );
    assert!(
        history
            .budget
            .as_ref()
            .unwrap()
            .entities
            .entities
            .0
            .capacity()
            <= 14
    );
    drop(history.pop_undo());
    assert_eq!(history.budget.as_ref().unwrap().total(), 0);
    assert!(weak.iter().all(|image| image.upgrade().is_none()));
}

#[test]
fn incremental_census_unlimited_budget_stays_allocation_free() {
    let mut history = HistoryState::default();
    for _ in 0..100 {
        history.push_undo(snapshot(Arc::new(EntityType::Line(Line::new()))));
        drop(trim(&mut history, 256, usize::MAX));
    }
    assert!(history.budget.is_none());
    drop(trim(&mut history, 256, 512 * 1024 * 1024));
    assert_census(&history);
    drop(trim(&mut history, 256, usize::MAX));
    assert!(history.budget.is_none());
}

#[test]
fn incremental_census_enforces_budget_after_live_after_image_grows() {
    let mut history = HistoryState::default();
    history.push_undo(snapshot(Arc::new(EntityType::Line(Line::new()))));
    drop(trim(&mut history, 256, 64 * 1024));
    {
        let mut entry = history.last_undo_mut().unwrap();
        let HistorySnapshot::Delta(delta) = &mut *entry else {
            unreachable!()
        };
        let mut line = LwPolyline::new();
        line.vertices.reserve_exact(100_000);
        delta.entities[0].2 = Some(Arc::new(EntityType::LwPolyline(line)));
    }
    assert_census(&history);
    assert_eq!(trim(&mut history, 256, 64 * 1024).len(), 1);
    assert_eq!(history.budget.as_ref().unwrap().total(), 0);
}

#[cfg(feature = "performance-audit")]
#[path = "memory_incremental_audit.rs"]
mod audit;
