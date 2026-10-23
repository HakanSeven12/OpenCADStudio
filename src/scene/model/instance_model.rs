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
        // Do not cancel the whole INSERT proxy when one *non-stub* leaf
        // has an unbounded/sentinel AABB or is 3-D. Those leaves remain
        // untouched in the preview/highlight, while the expensive valid
        // block stubs can still be represented by their own bounds.
        if wire.fill_is_3d || !good_bounds(&wire.aabb) {
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

/// Whether an ordinary selected/preview wire has been covered by a
/// conservative per-INSERT proxy. Non-stub leaves with invalid bounds,
/// 3-D geometry, or invisible/snap-only duties must remain intact. Never
/// discard a leaf merely because its handle matches a proxied block.
pub fn mass_stub_wire_is_proxied(
    wire: &WireModel,
    replaced: &rustc_hash::FxHashSet<String>,
) -> bool {
    if !replaced.contains(&wire.name) {
        return false;
    }
    if wire.stub.is_some() {
        return true;
    }
    wire.display_visible
        && !wire.snap_only
        && !wire.fill_is_3d
        && wire.aabb.iter().all(|v| v.is_finite() && v.abs() < 1.0e15)
        && wire.aabb[0] <= wire.aabb[2]
        && wire.aabb[1] <= wire.aabb[3]
}

/// Cache just the one bounding proxy in the command at creation time.
//// This avoids constructing, translating, rotating and allocating 80k child
//// preview wires on *every* pointer move and snap-dwell tick. The authoritative
//// command remains handle-based; only its disposable ghost changes LOD.
pub fn cached_command_preview_for_large_2d_selection(
    handles: usize,
    wires: Vec<WireModel>,
) -> Vec<WireModel> {
    const MIN_SELECTED: usize = 512;
    const MIN_WIRES: usize = 2048;
    // Do not replace a small selection with a bounding box merely because
    // the nested prototype is complex. The raster capture is optional and
    // may fail, so keep its real geometry as the guaranteed visual fallback.
    if handles <= MIN_SELECTED || wires.len() < MIN_WIRES {
        return wires;
    }
    if let Some(proxy) = mass_transform_group_preview(&wires, MIN_WIRES) {
        crate::perf_record!(
            "[perf] mass-command-preview-cache selected={} original_wires={} proxy_wires={}",
            handles, wires.len(), proxy.len()
        );
        proxy
    } else {
        wires
    }
}

/// Emergency LOD for a very large interactive transform preview.
//// No document geometry, entity handles, or snap sources are changed.
//// Convert thousands of preview children into *one* overall XY extent
//// while preserving lightweight acquisition/tracking guide wires.
//// All expensive command preview entry points (normal pointer movement
//// and deferred SNAP_DWELL refresh) can use this same helper.
pub fn mass_transform_group_preview(
    wires: &[WireModel],
    min_wires: usize,
) -> Option<Vec<WireModel>> {
    if wires.len() < min_wires {
        return None;
    }
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut counted = 0usize;
    let mut guides = Vec::new();
    for wire in wires {
        // Tracking aids are not part of the entity extents. Their endpoints
        // can be intentionally thousands of world units away.
        if wire.name.starts_with("__") {
            guides.push(wire.clone());
            continue;
        }
        if wire.snap_only || !wire.display_visible {
            continue;
        }
        // Never flatten spatial geometry to XY: keep exact 3-D preview,
        // including wires at a nonzero constant elevation.
        if wire.fill_is_3d
            || wire.stub.as_ref().is_some_and(|stub| stub.delta[2].abs() > 1e-7)
            || wire.points.iter().any(|p| !p[2].is_finite() || p[2].abs() > 1e-5)
        {
            return None;
        }
        let bb = wire.aabb.map(f64::from);
        if bb.iter().all(|v| v.is_finite() && v.abs() < 1e15)
            && bb[0] <= bb[2] && bb[1] <= bb[3]
        {
            b[0] = b[0].min(bb[0]);
            b[1] = b[1].min(bb[1]);
            b[2] = b[2].max(bb[2]);
            b[3] = b[3].max(bb[3]);
            counted += 1;
        } else {
            // A SENTINEL/UNBOUNDED AABB on an analytic child should not
            // enlarge the whole preview to +/- infinity. Its explicit
            // points, if present, are still good conservative extents.
            for p in &wire.points {
                let x = f64::from(p[0]);
                let y = f64::from(p[1]);
                if x.is_finite() && y.is_finite()
                    && x.abs() < 1e15 && y.abs() < 1e15
                {
                    b[0] = b[0].min(x);
                    b[1] = b[1].min(y);
                    b[2] = b[2].max(x);
                    b[3] = b[3].max(y);
                    counted += 1;
                }
            }
        }
    }
    if counted == 0
        || b.iter().any(|v| !v.is_finite())
        || b[2] <= b[0]
        || b[3] <= b[1]
    {
        return None;
    }
    let mut box_wire = WireModel::solid_f64(
        "__mass_transform_group_extent__".to_owned(),
        vec![
            [b[0], b[1], 0.0],
            [b[2], b[1], 0.0],
            [b[2], b[3], 0.0],
            [b[0], b[3], 0.0],
            [b[0], b[1], 0.0],
        ],
        WireModel::SELECTED,
        false,
    );
    // Mark as an ordinary visual-only overlay, not an editable entity.
    box_wire.selected = false;
    guides.push(box_wire);
    Some(guides)
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
    fn non_stub_unbounded_or_3d_children_remain_exact_and_do_not_disable_stubs() {
        let proto = Arc::new(vec![WireModel::default()]);
        let mut wires = Vec::new();
        for i in 0..300 {
            let mut stub = stub_wire(
                Arc::clone(&proto), [0.0, 0.0, 1.0, 1.0],
                [i as f64 * 5.0, 0.0, 0.0], 0.0,
            );
            stub.name = i.to_string();
            wires.push(stub);
            let mut unbounded = WireModel::solid(
                i.to_string(),
                vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
                WireModel::WHITE, false,
            );
            unbounded.aabb = WireModel::UNBOUNDED_AABB;
            wires.push(unbounded);
            let mut regular = WireModel::solid(
                i.to_string(),
                vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
                WireModel::WHITE, false,
            );
            regular.aabb = [i as f32 * 5.0, 0.0, i as f32 * 5.0+1.0, 1.0];
            wires.push(regular);
        }
        let (proxies, replaced) = mass_stub_outlines(wires.iter(), 256);
        assert_eq!(replaced.len(), 300);
        assert_eq!(proxies.len(), 300);
        let remaining: Vec<_> = wires.iter().filter(
            |wire| !mass_stub_wire_is_proxied(wire, &replaced)
        ).collect();
        assert_eq!(remaining.len(), 300);
        assert!(remaining.iter().all(|wire| wire.aabb == WireModel::UNBOUNDED_AABB));
    }

    #[test]
    fn one_group_proxy_covers_all_heavy_instances_and_ignores_distant_guides() {
        let proto = Arc::new(vec![WireModel::default()]);
        let mut refs = Vec::new();
        for i in 0..700 {
            let mut w = stub_wire(
                Arc::clone(&proto), [0.0, 0.0, 1.0, 1.0],
                [i as f64 * 2.0, 0.0, 0.0], 0.0,
            );
            w.name = i.to_string();
            refs.push(w);
        }
        let guide = WireModel::solid_f64(
            "__otrack_guide__".to_string(),
            vec![[-1e5, -1e5, 0.0], [1e5, 1e5, 0.0]],
            WireModel::WHITE, false,
        );
        refs.push(guide);
        let result = mass_transform_group_preview(&refs, 512).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "__otrack_guide__");
        assert_eq!(result[1].points.len(), 5);
        assert!((result[1].points[2][0] as f64 - 1399.0).abs() < 0.001);
    }

    #[test]
    fn group_proxy_only_enters_after_threshold_and_skips_3d() {
        let mut many = Vec::new();
        for i in 0..1000 {
            let mut w = WireModel::solid_f64(
                i.to_string(),
                vec![[i as f64, 0.0, 0.0], [i as f64+1.0, 1.0, 0.0]],
                WireModel::WHITE, false,
            );
            w.aabb = [i as f32, 0.0, i as f32+1.0, 1.0];
            many.push(w);
        }
        assert!(mass_transform_group_preview(&many, 2000).is_none());
        assert_eq!(mass_transform_group_preview(&many, 512).unwrap().len(), 1);
        many[5].fill_is_3d = true;
        assert!(mass_transform_group_preview(&many, 512).is_none());
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
