//! Undo budget estimates, not an allocator census. Count shared entity images
//! and raw DWG records once across both stacks. Capacity-based payload costs
//! cover the common large geometry/text/ACIS cases; structure/style estimates,
//! uncommon entity payloads and allocator overhead remain approximate.

use crate::app::tab_document::{HistorySnapshot, HistoryState};
use document::{entities::EntityCommon, EntityType};
use rustc_hash::FxHashMap;
use std::{
    mem::{size_of, size_of_val},
    sync::Arc,
};

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

/// Determine both prefixes before draining: removing index zero repeatedly
/// shifts every remaining snapshot. Shared bytes disappear only when the last
/// history reference is removed, even when it lives on the other stack.
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
    discarded.extend(history.undo_stack.drain(..undo));
    discarded.extend(history.redo_stack.drain(..redo));
    discarded
}

fn capacity<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(size_of::<T>())
}

fn common_bytes(common: &EntityCommon) -> usize {
    use document::xdata::XDataValue;
    let mut bytes = common.layer.capacity()
        + common.linetype.capacity()
        + common.color_name.as_ref().map_or(0, String::capacity)
        + common.graphic_data.as_ref().map_or(0, capacity)
        + capacity(&common.reactors)
        + common
            .r12_metadata
            .as_ref()
            .map_or(0, |metadata| capacity(&metadata.child_viewports))
        + capacity(&common.extended_data.raw_dwg_eed);
    for (_, data) in &common.extended_data.raw_dwg_eed {
        bytes = bytes.saturating_add(data.capacity());
    }
    // The records accessor exposes a slice, so its backing capacity is unknown.
    bytes = bytes.saturating_add(size_of_val(common.extended_data.records()));
    for record in common.extended_data.records() {
        bytes = bytes
            .saturating_add(record.application_name.capacity())
            .saturating_add(capacity(&record.values));
        for value in &record.values {
            bytes = bytes.saturating_add(match value {
                XDataValue::String(s) | XDataValue::ControlString(s) | XDataValue::LayerName(s) => {
                    s.capacity()
                }
                XDataValue::BinaryData(data) => data.capacity(),
                _ => 0,
            });
        }
    }
    bytes
}

fn acis_bytes(data: &document::entities::solid3d::AcisData) -> usize {
    let mut bytes = data.sat_data.capacity() + data.sab_data.capacity() + capacity(&data.materials);
    // Imported extra ACIS data can be chained. Walk it without recursive stack growth.
    let mut next = data.extra_acis_data.as_deref();
    while let Some(data) = next {
        bytes = bytes
            .saturating_add(size_of_val(data))
            .saturating_add(data.sat_data.capacity())
            .saturating_add(data.sab_data.capacity())
            .saturating_add(capacity(&data.materials));
        next = data.extra_acis_data.as_deref();
    }
    bytes
}

fn wires_bytes(
    wires: &Vec<document::entities::solid3d::Wire>,
    silhouettes: &Vec<document::entities::solid3d::Silhouette>,
) -> usize {
    capacity(wires)
        + wires
            .iter()
            .map(|wire| capacity(&wire.points))
            .sum::<usize>()
        + capacity(silhouettes)
        + silhouettes
            .iter()
            .map(|silhouette| {
                capacity(&silhouette.wires)
                    + silhouette
                        .wires
                        .iter()
                        .map(|wire| capacity(&wire.points))
                        .sum::<usize>()
            })
            .sum::<usize>()
}

fn paths_bytes(paths: &Vec<document::entities::hatch::BoundaryPath>) -> usize {
    use document::entities::hatch::BoundaryEdge;
    capacity(paths)
        + paths
            .iter()
            .map(|path| {
                capacity(&path.edges)
                    + capacity(&path.boundary_handles)
                    + path
                        .edges
                        .iter()
                        .map(|edge| match edge {
                            BoundaryEdge::Spline(spline) => {
                                capacity(&spline.knots)
                                    + capacity(&spline.control_points)
                                    + capacity(&spline.fit_points)
                            }
                            BoundaryEdge::Polyline(polyline) => capacity(&polyline.vertices),
                            _ => 0,
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
}

fn entity_bytes(entity: &EntityType) -> usize {
    let payload = match entity {
        EntityType::LwPolyline(p) => capacity(&p.vertices),
        EntityType::Polyline(p) => capacity(&p.vertices),
        EntityType::Polyline2D(p) => capacity(&p.vertices),
        EntityType::Polyline3D(p) => {
            capacity(&p.vertices) + p.vertices.iter().map(|v| v.layer.capacity()).sum::<usize>()
        }
        EntityType::Spline(s) => {
            capacity(&s.knots)
                + capacity(&s.control_points)
                + capacity(&s.weights)
                + capacity(&s.fit_points)
        }
        EntityType::Mesh(m) => {
            capacity(&m.vertices)
                + capacity(&m.faces)
                + capacity(&m.edges)
                + m.faces
                    .iter()
                    .map(|face| capacity(&face.vertices))
                    .sum::<usize>()
        }
        EntityType::PolyfaceMesh(m) => {
            capacity(&m.vertices)
                + capacity(&m.faces)
                + m.vertices
                    .iter()
                    .map(|v| common_bytes(&v.common))
                    .sum::<usize>()
                + m.faces
                    .iter()
                    .map(|f| common_bytes(&f.common))
                    .sum::<usize>()
        }
        EntityType::PolygonMesh(m) => {
            capacity(&m.vertices)
                + m.vertices
                    .iter()
                    .map(|v| common_bytes(&v.common))
                    .sum::<usize>()
        }
        EntityType::Text(t) => t.value.capacity() + t.style.capacity(),
        EntityType::Insert(insert) => {
            insert.block_name.capacity()
                + capacity(&insert.attributes)
                + insert
                    .attributes
                    .iter()
                    .map(|attribute| common_bytes(&attribute.common) + attribute_bytes(attribute))
                    .sum::<usize>()
        }
        EntityType::AttributeEntity(attribute) => attribute_bytes(attribute),
        EntityType::MText(t) => {
            size_of_val(t.as_ref())
                + t.value.capacity()
                + t.style.capacity()
                + capacity(&t.column_data.heights)
        }
        EntityType::Hatch(h) => {
            size_of_val(h.as_ref())
                + h.pattern.name.capacity()
                + h.pattern.description.capacity()
                + capacity(&h.pattern.lines)
                + h.pattern
                    .lines
                    .iter()
                    .map(|line| capacity(&line.dash_lengths))
                    .sum::<usize>()
                + paths_bytes(&h.paths)
                + paths_bytes(&h.mpolygon_invalid_loops)
                + capacity(&h.seed_points)
                + h.gradient_color.name.capacity()
                + capacity(&h.gradient_color.colors)
                + h.gradient_color
                    .colors
                    .iter()
                    .map(|color| {
                        color.color_name.as_ref().map_or(0, String::capacity)
                            + color.book_name.as_ref().map_or(0, String::capacity)
                    })
                    .sum::<usize>()
        }
        EntityType::Solid3D(s) => {
            s.uid.capacity() + acis_bytes(&s.acis_data) + wires_bytes(&s.wires, &s.silhouettes)
        }
        EntityType::Region(s) => {
            s.uid.capacity() + acis_bytes(&s.acis_data) + wires_bytes(&s.wires, &s.silhouettes)
        }
        EntityType::Body(s) => {
            s.uid.capacity() + acis_bytes(&s.acis_data) + wires_bytes(&s.wires, &s.silhouettes)
        }
        EntityType::Helix(value) => size_of_val(value.as_ref()),
        EntityType::Dimension(value) => size_of_val(value.as_ref()),
        EntityType::Viewport(value) => size_of_val(value.as_ref()),
        EntityType::MultiLeader(value) => size_of_val(value.as_ref()),
        EntityType::Surface(value) => size_of_val(value.as_ref()),
        EntityType::Table(value) => size_of_val(value.as_ref()),
        EntityType::Ole2Frame(value) => size_of_val(value.as_ref()),
        EntityType::Light(value) => size_of_val(value.as_ref()),
        EntityType::Extended(value) => size_of_val(value.as_ref()),
        _ => 0,
    };
    size_of::<EntityType>()
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(common_bytes(entity.common()))
        .saturating_add(payload)
}

fn attribute_bytes(attribute: &document::entities::AttributeEntity) -> usize {
    attribute.tag.capacity()
        + attribute.value.capacity()
        + attribute.text_style.capacity()
        + attribute.embedded_mtext.as_deref().map_or(0, |text| {
            size_of_val(text)
                + common_bytes(&text.common)
                + text.value.capacity()
                + text.style.capacity()
                + capacity(&text.column_data.heights)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tab_document::DeltaSnapshot;
    use document::{
        entities::{Line, LwPolyline},
        Handle,
    };

    fn snapshot(entity: Arc<EntityType>) -> HistorySnapshot {
        HistorySnapshot::Delta(Box::new(DeltaSnapshot {
            visibility_states: None,
            entities: vec![(Handle::new(1), Some(entity.clone()), Some(entity))],
            current_layout_before: String::new(),
            current_layout_after: String::new(),
            selected_before: vec![],
            selected_after: vec![],
            dirty_before: false,
            dirty_after: true,
            view: None,
            active_layer: None,
            layiso_state: None,
            structure: None,
            parametric_constraints: vec![],
            reference_designations: vec![],
            reference_session: None,
            graph: None,
            named_parameters: None,
            label: "memory test".into(),
        }))
    }

    fn large_polyline() -> Arc<EntityType> {
        let mut line = LwPolyline::new();
        // Spare capacity is retained memory even when the vector is empty.
        line.vertices.reserve_exact(100_000);
        Arc::new(EntityType::LwPolyline(line))
    }

    #[test]
    fn undo_budget_counts_capacity_in_large_geometry_text_and_acis() {
        let polyline = large_polyline();
        assert!(entity_bytes(&polyline) > 1_000_000);
        let mut mesh = document::entities::Mesh::new();
        mesh.vertices.reserve_exact(100_000);
        assert!(
            entity_bytes(&EntityType::Mesh(mesh))
                >= 100_000 * size_of::<document::types::Vector3>()
        );
        let mut solid = document::entities::Solid3D::new();
        solid.acis_data.sab_data.reserve_exact(1_000_000);
        assert!(entity_bytes(&EntityType::Solid3D(solid)) >= 1_000_000);
        let mut text = document::entities::MText::new();
        text.value.reserve_exact(1_000_000);
        assert!(entity_bytes(&EntityType::MText(Box::new(text))) >= 1_000_000);
        // Serialized sizes would miss these runtime-only graphic bytes.
        let mut line = Line::new();
        line.common.graphic_data = Some(Vec::with_capacity(1_000_000));
        assert!(entity_bytes(&EntityType::Line(line)) >= 1_000_000);
    }

    #[test]
    fn undo_budget_keeps_shared_payload_until_its_last_reference() {
        let entity = large_polyline();
        let mut budget = EntityBudget::default();
        let bytes = budget.retain(&entity);
        assert_eq!(budget.retain(&entity), 0);
        assert_eq!(budget.release(&entity), 0);
        assert_eq!(budget.release(&entity), bytes);
    }

    #[test]
    fn undo_budget_deduplicates_raw_records_between_distinct_entity_images() {
        let mut line = Line::new();
        line.common.raw_record = Some(Arc::new(document::entities::RawRecord {
            data: Vec::with_capacity(1_000_000),
            handle_bits: 0,
            version: document::DxfVersion::AC1032,
        }));
        // EntityCommon::clone deliberately drops passthrough DWG bytes.
        // Explicitly share the record to exercise two independent images.
        let mut previous = line.clone();
        previous.common.raw_record = line.common.raw_record.clone();
        let before = Arc::new(EntityType::Line(previous));
        let after = Arc::new(EntityType::Line(line));
        let mut budget = EntityBudget::default();
        let first = budget.retain(&before);
        let second = budget.retain(&after);
        assert!(first >= 1_000_000);
        assert!(second < 1_000_000);
        assert_eq!(budget.release(&before), entity_bytes(&before));
        assert_eq!(
            budget.release(&after),
            first + second - entity_bytes(&before)
        );
    }

    #[test]
    fn undo_budget_counts_shared_images_across_both_stacks_once() {
        let entity = large_polyline();
        let mut history = HistoryState::default();
        history.undo_stack.push(snapshot(entity.clone()));
        history.redo_stack.push(snapshot(entity.clone()));
        let overhead: usize = history
            .undo_stack
            .iter()
            .chain(&history.redo_stack)
            .map(HistorySnapshot::estimated_overhead_bytes)
            .sum();
        assert!(trim(&mut history, 256, overhead + entity_bytes(&entity)).is_empty());
        let discarded = trim(&mut history, 1, usize::MAX);
        assert_eq!(discarded.len(), 1);
        assert!(history.undo_stack.is_empty());
        assert_eq!(history.redo_stack.len(), 1);
    }

    #[test]
    fn undo_budget_evicts_large_old_entries_and_keeps_the_newest_small_edit() {
        let mut history = HistoryState::default();
        history.undo_stack.push(snapshot(large_polyline()));
        history
            .undo_stack
            .push(snapshot(Arc::new(EntityType::Line(Line::new()))));
        let discarded = trim(&mut history, 256, 64 * 1024);
        assert_eq!(discarded.len(), 1);
        assert_eq!(history.undo_stack.len(), 1);
        let HistorySnapshot::Delta(delta) = &history.undo_stack[0] else {
            panic!()
        };
        assert!(matches!(
            delta.entities[0].1.as_deref(),
            Some(EntityType::Line(_))
        ));
    }

    #[test]
    fn undo_budget_entry_limit_drains_prefixes_without_changing_order() {
        let entity = Arc::new(EntityType::Line(Line::new()));
        let mut history = HistoryState::default();
        for _ in 0..10 {
            history.undo_stack.push(snapshot(entity.clone()));
        }
        for _ in 0..5 {
            history.redo_stack.push(snapshot(entity.clone()));
        }
        assert_eq!(trim(&mut history, 3, usize::MAX).len(), 12);
        assert!(history.undo_stack.is_empty());
        assert_eq!(history.redo_stack.len(), 3);
        assert_eq!(trim(&mut history, 0, 0).len(), 3);
    }

    #[test]
    #[ignore = "manual history-accounting latency probe; use --release for comparisons"]
    fn undo_budget_dense_history_probe() {
        let entities: Vec<_> = (0..1000)
            .map(|_| Arc::new(EntityType::Line(Line::new())))
            .collect();
        let mut history = HistoryState::default();
        for _ in 0..256 {
            let mut entry = snapshot(entities[0].clone());
            let HistorySnapshot::Delta(delta) = &mut entry else {
                unreachable!()
            };
            delta.entities = entities
                .iter()
                .enumerate()
                .map(|(i, entity)| {
                    (
                        Handle::new(i as u64 + 1),
                        Some(entity.clone()),
                        Some(entity.clone()),
                    )
                })
                .collect();
            history.undo_stack.push(entry);
        }
        let start = std::time::Instant::now();
        for _ in 0..5 {
            assert!(trim(&mut history, 256, 512 * 1024 * 1024).is_empty());
        }
        eprintln!(
            "undo budget: 256 entries x 1000 shared images, {:.3} ms/check ({})",
            start.elapsed().as_secs_f64() * 1000.0 / 5.0,
            if cfg!(debug_assertions) {
                "debug, not a release timing"
            } else {
                "release"
            }
        );
    }

    #[test]
    #[ignore = "manual before/after retention probe; allocates about 733 MiB per sequential arm"]
    fn undo_budget_before_after_retention_probe() {
        const ENTRIES: usize = 160;
        const VERTICES: usize = 100_000;
        const LIMIT: usize = 512 * 1024 * 1024;

        fn fixture() -> HistoryState {
            let mut history = HistoryState::default();
            for i in 0..ENTRIES {
                let mut polyline = LwPolyline::new();
                polyline.vertices.reserve_exact(VERTICES);
                for vertex in 0..VERTICES {
                    polyline.vertices.push(document::entities::LwVertex::new(
                        document::types::Vector2::new(vertex as f64, i as f64),
                    ));
                }
                let mut entry = snapshot(Arc::new(EntityType::LwPolyline(polyline)));
                let HistorySnapshot::Delta(delta) = &mut entry else {
                    unreachable!()
                };
                // An erased polyline is held only by its undo before-image.
                delta.entities[0].0 = Handle::new(i as u64 + 1);
                delta.entities[0].2 = None;
                history.undo_stack.push(entry);
            }
            history
        }

        fn legacy_bytes(entry: &HistorySnapshot) -> usize {
            let HistorySnapshot::Delta(delta) = entry else {
                unreachable!()
            };
            // Exact pre-change estimate for this fixed entity-only fixture:
            // all other history channels are empty and the label is literal.
            assert!(
                delta.structure.is_none()
                    && delta.parametric_constraints.is_empty()
                    && delta.named_parameters.is_none()
                    && delta.visibility_states.is_none()
                    && delta.reference_designations.is_empty()
                    && delta.reference_session.is_none()
                    && delta.active_layer.is_none()
                    && delta.selected_before.is_empty()
                    && delta.selected_after.is_empty()
            );
            delta.entities.len() * 512 + entry.label().len()
        }

        fn retained_vertices(history: &HistoryState) -> usize {
            let mut seen = rustc_hash::FxHashSet::default();
            history
                .undo_stack
                .iter()
                .chain(&history.redo_stack)
                .map(|entry| {
                    let HistorySnapshot::Delta(delta) = entry else {
                        unreachable!()
                    };
                    delta
                        .entities
                        .iter()
                        .flat_map(|(_, before, after)| before.iter().chain(after))
                        .filter(|entity| seen.insert(Arc::as_ptr(entity) as usize))
                        .map(|entity| match entity.as_ref() {
                            EntityType::LwPolyline(polyline) => capacity(&polyline.vertices),
                            _ => unreachable!(),
                        })
                        .sum::<usize>()
                })
                .sum()
        }

        // Run arms sequentially so the baseline cannot keep the new arm's
        // allocations alive. Sample after discarded entries have been dropped.
        let before = {
            let mut history = fixture();
            let mut bytes: usize = history.undo_stack.iter().map(legacy_bytes).sum();
            let mut discarded = Vec::new();
            while history.undo_stack.len() > 256 || bytes > LIMIT {
                let entry = history.undo_stack.remove(0);
                bytes -= legacy_bytes(&entry);
                discarded.push(entry);
            }
            drop(discarded);
            (history.undo_stack.len(), retained_vertices(&history), bytes)
        };
        let after = {
            let mut history = fixture();
            drop(trim(&mut history, 256, LIMIT));
            let mut entities = EntityBudget::default();
            let estimated: usize = history
                .undo_stack
                .iter()
                .map(|entry| entry.estimated_overhead_bytes() + entities.snapshot(entry, true))
                .sum();
            assert!(estimated <= LIMIT);
            (
                history.undo_stack.len(),
                retained_vertices(&history),
                estimated,
            )
        };
        assert_eq!(before.0, ENTRIES);
        assert!(after.0 < before.0 && after.1 < before.1);
        eprintln!(
            "{}",
            serde_json::json!({
                "benchmark": "undo_budget_before_after_retention", "schema": 1,
                "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
                "entries": ENTRIES, "vertices_per_entry": VERTICES,
                "vertex_size_bytes": size_of::<document::entities::LwVertex>(),
                "limit_bytes": LIMIT, "entry_limit": 256,
                "before": {"retained_entries": before.0, "retained_vertex_capacity_bytes": before.1, "budget_estimate_bytes": before.2},
                "after": {"retained_entries": after.0, "retained_vertex_capacity_bytes": after.1, "budget_estimate_bytes": after.2},
                "scope": "unique retained polyline vertex-buffer capacity after eviction and synchronous drop; excludes other allocations, allocator overhead, RSS and deferred-drop backlog; baseline is the pre-change policy on the same populated fixture"
            })
        );
    }
}
