//! COMPARE: what differs between this drawing and another one. Objects are
//! matched by what they draw — type, layer and their tessellated geometry —
//! so a copy that kept its geometry matches whatever its handle, and an
//! object that moved or changed shows as removed in one place and added in
//! the other.

use codec::{CadDocument, Handle};
use glam::DVec3;
use rustc_hash::FxHashMap;

use crate::scene::{Scene, WireModel};

/// The result of a comparison, ready to draw over the current drawing.
pub struct Comparison {
    /// Objects only this drawing has.
    pub added: usize,
    /// Objects only the other drawing has.
    pub removed: usize,
    /// Their outlines: added in green, removed in red.
    pub overlay: Vec<WireModel>,
    /// Each difference's extent, in reading order, to step through.
    pub spots: Vec<(DVec3, DVec3)>,
}

const ADDED: [f32; 4] = [0.25, 0.85, 0.35, 1.0];
const REMOVED: [f32; 4] = [0.95, 0.3, 0.25, 1.0];

fn wires_by_entity(scene: &Scene) -> FxHashMap<Handle, Vec<WireModel>> {
    let mut map: FxHashMap<Handle, Vec<WireModel>> = FxHashMap::default();
    for wire in scene.entity_wires().iter() {
        if let Some(handle) = Scene::handle_from_wire_name(&wire.name) {
            map.entry(handle).or_default().push(wire.clone());
        }
    }
    map
}

fn point(wire: &WireModel, index: usize) -> DVec3 {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    DVec3::new(
        f64::from(high[0]) + f64::from(low[0]),
        f64::from(high[1]) + f64::from(low[1]),
        f64::from(high[2]) + f64::from(low[2]),
    )
}

/// What an object draws, as a comparable key: its type, its layer and its
/// outline points rounded to 1e-4 drawing units.
fn signature(document: &CadDocument, handle: Handle, wires: &[WireModel]) -> String {
    let mut key = match document.get_entity(handle) {
        Some(entity) => format!(
            "{}|{}",
            crate::entities::names::ui_name(entity),
            entity.common().layer
        ),
        None => String::new(),
    };
    for wire in wires {
        key.push('#');
        for index in 0..wire.points.len() {
            let p = point(wire, index);
            if !p.is_finite() {
                key.push('/');
                continue;
            }
            let q = (p * 1e4).round();
            key.push_str(&format!("{},{},{};", q.x, q.y, q.z));
        }
    }
    key
}

fn extent(wires: &[WireModel]) -> Option<(DVec3, DVec3)> {
    let mut bounds: Option<(DVec3, DVec3)> = None;
    for wire in wires {
        for index in 0..wire.points.len() {
            let p = point(wire, index);
            if !p.is_finite() {
                continue;
            }
            bounds = Some(match bounds {
                None => (p, p),
                Some((min, max)) => (min.min(p), max.max(p)),
            });
        }
    }
    bounds
}

/// Compare `current` with the drawing `other`, in the space `current`
/// shows.
pub fn compare(current: &Scene, other: CadDocument) -> Comparison {
    let mut other_scene = Scene::new();
    other_scene.document = other;
    other_scene.bump_geometry();
    let here = wires_by_entity(current);
    let there = wires_by_entity(&other_scene);

    let mut unmatched: FxHashMap<String, Vec<Handle>> = FxHashMap::default();
    for (handle, wires) in &there {
        unmatched
            .entry(signature(&other_scene.document, *handle, wires))
            .or_default()
            .push(*handle);
    }
    let mut overlay = Vec::new();
    let mut spots = Vec::new();
    let mut added = 0;
    let mut mark = |wires: &[WireModel], color: [f32; 4]| {
        if let Some(bounds) = extent(wires) {
            spots.push(bounds);
        }
        overlay.extend(wires.iter().cloned().map(|mut wire| {
            wire.color = color;
            wire.selected = false;
            wire.line_weight_px = wire.line_weight_px.max(2.0);
            wire
        }));
    };
    for (handle, wires) in &here {
        let key = signature(&current.document, *handle, wires);
        match unmatched.get_mut(&key).and_then(Vec::pop) {
            Some(_) => {}
            None => {
                added += 1;
                mark(wires, ADDED);
            }
        }
    }
    let mut removed = 0;
    for handle in unmatched.into_values().flatten() {
        if let Some(wires) = there.get(&handle) {
            removed += 1;
            mark(wires, REMOVED);
        }
    }
    // Reading order: top to bottom, then left to right.
    spots.sort_by(|a, b| {
        b.1.y
            .partial_cmp(&a.1.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.x.partial_cmp(&b.0.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    Comparison {
        added,
        removed,
        overlay,
        spots,
    }
}
