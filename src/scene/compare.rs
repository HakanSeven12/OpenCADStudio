//! COMPARE: what differs between this drawing and another one. Objects are
//! matched by what they draw — type, layer and their tessellated geometry —
//! so a copy that kept its geometry matches whatever its handle, and an
//! object that moved or changed shows as removed in one place and added in
//! the other — unless both drawings still give it the same handle (a
//! revision of one file), when it shows as modified with both states.

use document::{Document, Handle};
use glam::DVec3;
use rustc_hash::FxHashMap;

use crate::scene::{Scene, WireModel};

/// How a comparison is drawn and how closely geometry must agree.
#[derive(Clone, Copy, Debug)]
pub struct CompareSettings {
    /// Added, removed and modified colours.
    pub colors: [[u8; 3]; 3],
    /// Points closer than this (drawing units) count as the same.
    pub tolerance: f64,
}

impl Default for CompareSettings {
    fn default() -> Self {
        Self {
            colors: [[64, 217, 89], [242, 77, 64], [242, 166, 38]],
            tolerance: 1e-4,
        }
    }
}

/// How an object differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Only this drawing has it.
    Added,
    /// Only the other drawing has it.
    Removed,
    /// Both have it (same handle) but it draws differently.
    Modified,
}

impl Change {
    pub fn name(self) -> &'static str {
        match self {
            Change::Added => "Added",
            Change::Removed => "Removed",
            Change::Modified => "Modified",
        }
    }
}

/// One difference, for stepping through and the report.
#[derive(Clone, Debug)]
pub struct Difference {
    pub change: Change,
    pub object: String,
    pub layer: String,
    pub handle: Handle,
    /// Extent of every state shown for it.
    pub bounds: (DVec3, DVec3),
}

/// The result of a comparison, ready to draw over the current drawing.
pub struct Comparison {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    /// Their outlines in the settings' colours; a modified object's old
    /// state shows faded in the removed colour.
    pub overlay: Vec<WireModel>,
    /// The differences in reading order.
    pub differences: Vec<Difference>,
}

impl Comparison {
    /// The differences as CSV: change, object, layer, handle, extent.
    pub fn csv(&self, current: &str, other: &str) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = format!(
            "# Current,{}\n# Other,{}\nChange,Object,Layer,Handle,Min X,Min Y,Max X,Max Y\n",
            quote(current),
            quote(other)
        );
        for d in &self.differences {
            let (min, max) = d.bounds;
            out.push_str(&format!(
                "{},{},{},{:X},{},{},{},{}\n",
                d.change.name(),
                quote(&d.object),
                quote(&d.layer),
                d.handle.value(),
                min.x,
                min.y,
                max.x,
                max.y
            ));
        }
        out
    }
}

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

/// An object's type and layer names, empty when it is not in `document`.
fn describe(document: &Document, handle: Handle) -> (String, String) {
    match document.get_entity(handle) {
        Some(entity) => (
            crate::entities::names::ui_name(entity).to_string(),
            entity.common().layer.clone(),
        ),
        None => (String::new(), String::new()),
    }
}

/// What an object draws, as a comparable key: its type, its layer and its
/// outline points rounded to `tolerance`.
fn signature(document: &Document, handle: Handle, wires: &[WireModel], tolerance: f64) -> String {
    let (object, layer) = describe(document, handle);
    let mut key = format!("{object}|{layer}");
    for wire in wires {
        key.push('#');
        for index in 0..wire.points.len() {
            let p = point(wire, index);
            if !p.is_finite() {
                key.push('/');
                continue;
            }
            let q = (p / tolerance).round();
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

fn rgba([r, g, b]: [u8; 3], alpha: f32) -> [f32; 4] {
    [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, alpha]
}

/// Compare `current` with the drawing `other`, in the space `current`
/// shows.
pub fn compare(current: &Scene, other: Document, settings: CompareSettings) -> Comparison {
    let tolerance = settings.tolerance.max(1e-9);
    let [added_color, removed_color, modified_color] = settings.colors;
    let mut other_scene = Scene::new();
    other_scene.document = other;
    other_scene.bump_geometry();
    let here = wires_by_entity(current);
    let there = wires_by_entity(&other_scene);

    let mut unmatched: FxHashMap<String, Vec<Handle>> = FxHashMap::default();
    for (handle, wires) in &there {
        unmatched
            .entry(signature(&other_scene.document, *handle, wires, tolerance))
            .or_default()
            .push(*handle);
    }
    let mut only_here = Vec::new();
    for (handle, wires) in &here {
        let key = signature(&current.document, *handle, wires, tolerance);
        if unmatched.get_mut(&key).and_then(Vec::pop).is_none() {
            only_here.push(*handle);
        }
    }
    let mut only_there: rustc_hash::FxHashSet<Handle> = unmatched.into_values().flatten().collect();

    let mut overlay = Vec::new();
    let mut differences = Vec::new();
    let mut paint = |wires: &[WireModel], color: [f32; 4]| {
        overlay.extend(wires.iter().cloned().map(|mut wire| {
            wire.color = color;
            wire.selected = false;
            wire.line_weight_px = wire.line_weight_px.max(2.0);
            wire
        }));
    };
    for handle in only_here {
        let wires = &here[&handle];
        let (object, layer) = describe(&current.document, handle);
        // The same object in both, drawn differently: show both states.
        let old = only_there
            .contains(&handle)
            .then(|| there.get(&handle))
            .flatten()
            .filter(|_| describe(&other_scene.document, handle).0 == object);
        let (change, bounds) = match old {
            Some(old) => {
                only_there.remove(&handle);
                paint(old, rgba(removed_color, 0.5));
                paint(wires, rgba(modified_color, 1.0));
                let bounds = match (extent(wires), extent(old)) {
                    (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
                    (a, b) => a.or(b),
                };
                (Change::Modified, bounds)
            }
            None => {
                paint(wires, rgba(added_color, 1.0));
                (Change::Added, extent(wires))
            }
        };
        if let Some(bounds) = bounds {
            differences.push(Difference { change, object, layer, handle, bounds });
        }
    }
    for handle in only_there {
        if let Some(wires) = there.get(&handle) {
            paint(wires, rgba(removed_color, 1.0));
            let (object, layer) = describe(&other_scene.document, handle);
            if let Some(bounds) = extent(wires) {
                differences.push(Difference {
                    change: Change::Removed,
                    object,
                    layer,
                    handle,
                    bounds,
                });
            }
        }
    }
    // Reading order: top to bottom, then left to right.
    differences.sort_by(|a, b| {
        b.bounds
            .1
            .y
            .partial_cmp(&a.bounds.1.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.bounds
                    .0
                    .x
                    .partial_cmp(&b.bounds.0.x)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let count = |change| differences.iter().filter(|d| d.change == change).count();
    Comparison {
        added: count(Change::Added),
        removed: count(Change::Removed),
        modified: count(Change::Modified),
        overlay,
        differences,
    }
}
