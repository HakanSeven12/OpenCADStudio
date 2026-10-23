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

/// Emergency preview LOD for extremely large 2-D transform commands.
/// Keep a separate outline for each CAD entity rather than reducing the whole
/// selection to one anonymous bounding rectangle. Exact document geometry
/// and command results are unchanged.
pub fn compact_mass_transform_preview(
    handles: usize,
    wires: Vec<WireModel>,
) -> Vec<WireModel> {
    if handles < 1024 || wires.len() < 20_000 {
        return wires;
    }
    use rustc_hash::FxHashMap;
    let mut grouped: FxHashMap<String, ([f32; 4], [f32; 4], bool)> =
        FxHashMap::default();
    let mut order = Vec::new();
    for wire in &wires {
        // Some block-instanced wires have an unset or unbounded cached AABB.
        // Recover a finite preview envelope from their wire positions rather
        // than disabling LOD for the entire 13k-block selection.
        let mut b = wire.aabb;
        if !b.iter().all(|n| n.is_finite() && n.abs() < 1.0e15)
            || b[0] > b[2] || b[1] > b[3]
        {
            b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for p in &wire.points {
                if p[0].is_finite() && p[1].is_finite() {
                    b[0] = b[0].min(p[0]);
                    b[1] = b[1].min(p[1]);
                    b[2] = b[2].max(p[0]);
                    b[3] = b[3].max(p[1]);
                }
            }
        }
        // An INSERT stub may have no own points and an unbounded AABB.
        // It can still be represented by the union of its *other* wires
        // with the same name. Do not abort the entire batch on that stub.
        if wire.stub.is_some()
            && (!b.iter().all(|n| n.is_finite() && n.abs() < 1.0e15)
                || b[0] > b[2] || b[1] > b[3])
        {
            continue;
        }
        if !wire.display_visible
            || wire.fill_is_3d
            || !wire.fill_tris.is_empty()
            || !wire.text_verts.is_empty()
            || !wire.pick_tris.is_empty()
            || wire.point_marker.is_some()
            || !b.iter().all(|n| n.is_finite() && n.abs() < 1.0e15)
            || b[0] > b[2] || b[1] > b[3]
        {
            // Preserve exact rendering when a mixed selection is unsupported.
            crate::perf_record!(
                "[perf] compact-mass-transform-preview fallback handles={} wires={} name={} visible={} fill3d={} fill={} text={} pick={} marker={} bounds={:?}",
                handles, wires.len(), wire.name, wire.display_visible,
                wire.fill_is_3d, !wire.fill_tris.is_empty(),
                !wire.text_verts.is_empty(), !wire.pick_tris.is_empty(),
                wire.point_marker.is_some(), b
            );
            return wires;
        }
        let item = grouped.entry(wire.name.clone()).or_insert_with(|| {
            order.push(wire.name.clone());
            (b, wire.color, wire.selected)
        });
        item.0[0] = item.0[0].min(b[0]);
        item.0[1] = item.0[1].min(b[1]);
        item.0[2] = item.0[2].max(b[2]);
        item.0[3] = item.0[3].max(b[3]);
    }
    // Never replace many separate selections by one misleading group box.
    // Never silently omit an INSERT whose only wire was an unbounded stub.
    // Require enough distinct bounded representatives for the selected
    // handles; otherwise preserve the exact preview.
    if grouped.len() < handles || grouped.len() >= wires.len() / 3 {
        crate::perf_record!(
            "[perf] compact-mass-transform-preview insufficient-reduction handles={} wires={} groups={}",
            handles, wires.len(), grouped.len()
        );
        return wires;
    }
    let mut result = Vec::with_capacity(grouped.len());
    for name in order {
        if let Some(([x0, y0, x1, y1], color, selected)) = grouped.remove(&name) {
            let mut outline = WireModel::solid_f64(
                name,
                vec![
                    [x0 as f64, y0 as f64, 0.0],
                    [x1 as f64, y0 as f64, 0.0],
                    [x1 as f64, y1 as f64, 0.0],
                    [x0 as f64, y1 as f64, 0.0],
                    [x0 as f64, y0 as f64, 0.0],
                ],
                color,
                selected,
            );
            outline.aabb = [x0, y0, x1, y1];
            result.push(outline);
        }
    }
    crate::perf_record!(
        "[perf] compact-mass-transform-preview original={} outlines={} handles={}",
        wires.len(),
        result.len(),
        handles
    );
    result
}

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
            crate::perf_record!(
                "[perf] mass-preview-reject reason=nonplanar name={} fill3d={} stub_z={:?} points={}",
                wire.name, wire.fill_is_3d,
                wire.stub.as_ref().map(|stub| stub.delta[2]), wire.points.len()
            );
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
        crate::perf_record!(
            "[perf] mass-preview-reject reason=invalid-extent counted={} bounds={:?} wires={}",
            counted, b, wires.len()
        );
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
