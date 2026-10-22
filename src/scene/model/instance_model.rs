use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::wire_model::WireModel;

static NEXT_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderInstance {
    pub source_id: u64,
    pub translation: [f64; 3],
}

pub fn next_source_id() -> u64 {
    NEXT_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Translated copies of one nested block expansion, kept as a single wire
/// instead of one wire per copied primitive (#1698): the copy is `prototype`
/// moved by `delta`, its draw depths shifted by `depth_delta`. Prototype
/// wires are block-instanced (or stubs themselves), so the GPU draws them
/// from the shared source geometry and CPU users expand them on demand.
#[derive(Debug)]
pub struct InstanceStub {
    pub prototype: Arc<Vec<WireModel>>,
    pub delta: [f64; 3],
    pub depth_delta: f32,
}

/// Whether a prototype wire may be represented by a stub: one the block
/// renderer draws from its source geometry alone (no glyphs, fills or
/// markers of its own), or a stub already.
pub fn stubbable(wire: &WireModel) -> bool {
    // Kill-switch: `OCS_NO_BLOCK_STUBS` keeps every copy as its own wires.
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *OFF.get_or_init(|| std::env::var("OCS_NO_BLOCK_STUBS").is_ok()) {
        return false;
    }
    wire.stub.is_some()
        || (wire.render_instance.is_some()
            && wire.text_verts.is_empty()
            && wire.fill_tris.is_empty()
            && wire.pick_tris.is_empty()
            && !wire.fill_is_3d
            && wire.point_marker.is_none()
            && wire.searchable_text.is_empty())
}

/// The XY box a prototype's wires cover, before any copy's offset.
pub fn prototype_aabb(prototype: &[WireModel]) -> [f32; 4] {
    prototype.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |acc, wire| {
            [
                acc[0].min(wire.aabb[0]),
                acc[1].min(wire.aabb[1]),
                acc[2].max(wire.aabb[2]),
                acc[3].max(wire.aabb[3]),
            ]
        },
    )
}

/// A stub wire standing for `prototype` moved by `delta`.
pub fn stub_wire(
    prototype: Arc<Vec<WireModel>>,
    prototype_aabb: [f32; 4],
    delta: [f64; 3],
    depth_delta: f32,
) -> WireModel {
    let aabb = [
        (prototype_aabb[0] as f64 + delta[0]) as f32,
        (prototype_aabb[1] as f64 + delta[1]) as f32,
        (prototype_aabb[2] as f64 + delta[0]) as f32,
        (prototype_aabb[3] as f64 + delta[1]) as f32,
    ];
    WireModel {
        aabb,
        stub: Some(Arc::new(InstanceStub {
            prototype,
            delta,
            depth_delta,
        })),
        ..Default::default()
    }
}

/// Every wire `wires` stands for, stubs expanded into translated copies
/// named and selected like the stub that holds them.
pub fn expand_stubs(wires: &[WireModel]) -> std::borrow::Cow<'_, [WireModel]> {
    if !wires.iter().any(|wire| wire.stub.is_some()) {
        return std::borrow::Cow::Borrowed(wires);
    }
    let mut out = Vec::with_capacity(wires.len());
    for wire in wires {
        if wire.stub.is_some() {
            expand_stub_into(wire, &mut out);
        } else {
            out.push(wire.clone());
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Append the translated copies a stub wire stands for.
pub fn expand_stub_into(stub_wire: &WireModel, out: &mut Vec<WireModel>) {
    expand_stub_near_into(stub_wire, None, out);
}

/// `expand_stub_into`, keeping only the copies whose XY box meets `near`
/// (or has no finite box). The test runs on the prototype before anything
/// is cloned, so a block copy under the cursor costs only its wires near
/// the cursor, not the whole block (#1891).
pub fn expand_stub_near_into(
    stub_wire: &WireModel,
    near: Option<[f64; 4]>,
    out: &mut Vec<WireModel>,
) {
    let Some(stub) = stub_wire.stub.as_ref() else {
        out.push(stub_wire.clone());
        return;
    };
    let meets = |wire: &WireModel| {
        let Some(query) = near else {
            return true;
        };
        let [x0, y0, x1, y1] = wire.aabb.map(f64::from);
        let (dx, dy) = (stub.delta[0], stub.delta[1]);
        !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite())
            || (x1 + dx >= query[0]
                && x0 + dx <= query[2]
                && y1 + dy >= query[1]
                && y0 + dy <= query[3])
    };
    for wire in stub.prototype.iter().filter(|wire| meets(wire)) {
        let mut copy = crate::scene::cache::block_cache::translated_prototype_wire(
            wire,
            &stub_wire.name,
            stub.delta,
        );
        shift_depth(&mut copy, stub.depth_delta);
        copy.selected = stub_wire.selected;
        copy.display_visible &= stub_wire.display_visible;
        if copy.stub.is_some() {
            expand_stub_near_into(&copy, near, out);
        } else {
            out.push(copy);
        }
    }
}

/// Shift a block-local draw depth by `delta`, through a stub as well.
pub fn shift_depth(wire: &mut WireModel, delta: f32) {
    if let Some(depth) = wire.depth_override.as_mut() {
        *depth += delta;
    }
    if let Some(stub) = wire.stub.as_mut() {
        *stub = Arc::new(InstanceStub {
            prototype: Arc::clone(&stub.prototype),
            delta: stub.delta,
            depth_delta: stub.depth_delta + delta,
        });
    }
}


/// A per-INSERT, conservative 2-D bounding preview for extreme selections.
/// Only handles containing block-instance stubs qualify; once a handle
/// qualifies we include *all* its visible child wires in the bounding box,
/// including analytic arcs and ordinary wires. Suppressing only the stub left
/// five other child models per palm and defeated the fast path (#2057).
///
/// Any child without a trustworthy 2-D extent keeps that INSERT on the exact
/// rendering path; small selections likewise remain exact.
pub fn mass_stub_outlines<'a>(
    wires: impl IntoIterator<Item = &'a WireModel>,
    min_stub_wires: usize,
) -> (Vec<WireModel>, rustc_hash::FxHashSet<String>) {
    use rustc_hash::{FxHashMap, FxHashSet};
    let all: Vec<&WireModel> = wires.into_iter().collect();
    // name -> (conservative XY bounds, representative colour, selected)
    let mut groups: FxHashMap<String, ([f32; 4], [f32; 4], bool)> =
        FxHashMap::default();
    let mut unsafe_names: FxHashSet<String> = FxHashSet::default();
    let mut stubs = 0usize;
    let good_bounds = |b: &[f32; 4]| {
        b.iter().all(|v| v.is_finite() && v.abs() < 1.0e15)
            && b[0] <= b[2]
            && b[1] <= b[3]
    };
    for &wire in &all {
        if wire.stub.is_none() || wire.name.is_empty() {
            continue;
        }
        stubs += 1;
        let b = wire.aabb;
        if wire.fill_is_3d
            || wire.stub.as_ref().is_some_and(|stub| stub.delta[2].abs() > 1e-7)
            || !good_bounds(&b)
        {
            unsafe_names.insert(wire.name.clone());
            continue;
        }
        groups
            .entry(wire.name.clone())
            .and_modify(|(rect, _, _)| {
                rect[0] = rect[0].min(b[0]);
                rect[1] = rect[1].min(b[1]);
                rect[2] = rect[2].max(b[2]);
                rect[3] = rect[3].max(b[3]);
            })
            .or_insert((b, wire.color, wire.selected));
    }
    if stubs < min_stub_wires || groups.len() < 256 {
        return (Vec::new(), FxHashSet::default());
    }
    // The other wires of the same INSERT are not necessarily stubs. Expand
    // the proxy's bounds to cover every visible child that will be dropped.
    for &wire in &all {
        let Some((rect, _, _)) = groups.get_mut(&wire.name) else {
            continue;
        };
        if wire.snap_only || !wire.display_visible {
            continue;
        }
        if wire.fill_is_3d || !good_bounds(&wire.aabb) {
            unsafe_names.insert(wire.name.clone());
            continue;
        }
        let b = wire.aabb;
        rect[0] = rect[0].min(b[0]);
        rect[1] = rect[1].min(b[1]);
        rect[2] = rect[2].max(b[2]);
        rect[3] = rect[3].max(b[3]);
    }
    let mut replaced = FxHashSet::default();
    let mut outlines = Vec::with_capacity(groups.len());
    for (name, (b, color, selected)) in groups {
        if unsafe_names.contains(&name) {
            continue;
        }
        let [x0, y0, x1, y1] = b.map(f64::from);
        let mut outline = WireModel::solid_f64(
            name.clone(),
            vec![
                [x0, y0, 0.0],
                [x1, y0, 0.0],
                [x1, y1, 0.0],
                [x0, y1, 0.0],
                [x0, y0, 0.0],
            ],
            color,
            selected,
        );
        outline.aabb = b;
        outlines.push(outline);
        replaced.insert(name);
    }
    (outlines, replaced)
}

#[cfg(test)]
mod mass_stub_outline_tests {
    use super::*;

    #[test]
    fn many_repeated_stub_leaves_collapse_per_block_without_losing_guides() {
        let proto = Arc::new(vec![WireModel::solid(
            "proto".to_string(),
            vec![[0.0, 0.0, 0.0], [3.0, 2.0, 0.0]],
            WireModel::WHITE, false,
        )]);
        let mut refs = Vec::new();
        for i in 0..300 {
            for k in 0..8 {
                let mut w = stub_wire(
                    Arc::clone(&proto),
                    [0.0, 0.0, 3.0, 2.0],
                    [i as f64 * 10.0, k as f64, 0.0],
                    0.0,
                );
                w.name = (i + 1).to_string();
                refs.push(w);
            }
        }
        let (outlines, replaced) = mass_stub_outlines(refs.iter(), 2048);
        assert_eq!(outlines.len(), 300);
        assert_eq!(replaced.len(), 300);
        let first = outlines.iter().find(|w| w.name == "1").unwrap();
        assert_eq!(first.aabb, [0.0, 0.0, 3.0, 9.0]);
        assert_eq!(first.points.len(), 5);
    }

    #[test]
    fn proxies_cover_non_stub_wires_belonging_to_each_insert() {
        let proto = Arc::new(vec![WireModel::default()]);
        let mut refs = Vec::new();
        for i in 0..300 {
            let mut a = stub_wire(
                Arc::clone(&proto), [0.0, 0.0, 1.0, 1.0],
                [i as f64 * 5.0, 0.0, 0.0], 0.0,
            );
            a.name = i.to_string();
            refs.push(a);
            for k in 0..7 {
                let mut extra = WireModel::solid(
                    i.to_string(), vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
                    WireModel::WHITE, false,
                );
                extra.aabb = [i as f32 * 5.0 - 2.0, -(k as f32),
                    i as f32 * 5.0 + 3.0, 2.0];
                refs.push(extra);
            }
        }
        let (outlines, names) = mass_stub_outlines(refs.iter(), 256);
        assert_eq!(outlines.len(), 300);
        assert_eq!(names.len(), 300);
        let first = outlines.iter().find(|w| w.name == "0").unwrap();
        assert_eq!(first.aabb, [-2.0, -6.0, 3.0, 2.0]);
        assert_eq!(refs.iter().filter(|wire| !names.contains(&wire.name)).count(), 0);
    }

    #[test]
    fn invalid_stub_bounds_are_not_replaced() {
        let proto = Arc::new(vec![WireModel::default()]);
        let mut refs = Vec::new();
        for i in 0..300 {
            for k in 0..8 {
                let mut w = stub_wire(
                    Arc::clone(&proto), [0.0, 0.0, 1.0, 1.0],
                    [i as f64 * 5.0, k as f64, 0.0], 0.0,
                );
                w.name = i.to_string();
                if i == 42 && k == 3 { w.aabb = WireModel::UNBOUNDED_AABB; }
                refs.push(w);
            }
        }
        let (outlines, replaced) = mass_stub_outlines(refs.iter(), 2048);
        assert!(!replaced.contains("42"));
        assert_eq!(outlines.len(), 299);
    }
}
