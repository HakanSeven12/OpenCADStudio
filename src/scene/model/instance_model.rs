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
    let Some(_) = stub_wire.stub.as_ref() else {
        out.push(stub_wire.clone());
        return;
    };
    // Expand nested instances with a *single* translation and allocation per
    // visible leaf. Previously each nesting level cloned and translated every
    // intermediate WireModel (including its vertex arrays), only to discard
    // that intermediate copy on the next recursive call. Tree drawings with
    // many repeated low-poly parts paid the cost at every nesting depth.
    expand_stub_near_recursive(
        stub_wire,
        &stub_wire.name,
        [0.0; 3],
        0.0,
        stub_wire.selected,
        stub_wire.display_visible,
        near,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn expand_stub_near_recursive(
    wire: &WireModel,
    owner_name: &str,
    offset: [f64; 3],
    depth_offset: f32,
    owner_selected: bool,
    parent_visible: bool,
    near: Option<[f64; 4]>,
    out: &mut Vec<WireModel>,
) {
    let Some(stub) = wire.stub.as_ref() else {
        let mut copy = crate::scene::cache::block_cache::translated_prototype_wire(
            wire, owner_name, offset,
        );
        shift_depth(&mut copy, depth_offset);
        copy.selected = owner_selected;
        copy.display_visible &= parent_visible;
        out.push(copy);
        return;
    };
    let offset = [
        offset[0] + stub.delta[0],
        offset[1] + stub.delta[1],
        offset[2] + stub.delta[2],
    ];
    let depth_offset = depth_offset + stub.depth_delta;
    let parent_visible = parent_visible && wire.display_visible;
    for child in stub.prototype.iter() {
        if let Some(query) = near {
            let [x0, y0, x1, y1] = child.aabb.map(f64::from);
            if x0.is_finite()
                && y0.is_finite()
                && x1.is_finite()
                && y1.is_finite()
                && (x1 + offset[0] < query[0]
                    || x0 + offset[0] > query[2]
                    || y1 + offset[1] < query[1]
                    || y0 + offset[1] > query[3])
            {
                continue;
            }
        }
        expand_stub_near_recursive(
            child,
            owner_name,
            offset,
            depth_offset,
            owner_selected,
            parent_visible,
            near,
            out,
        );
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

#[cfg(test)]
mod nested_stub_perf_tests {
    use super::*;

    #[test]
    fn nested_stub_expands_only_local_leaves_and_combines_transforms() {
        let mut leaf = WireModel::default();
        leaf.name = "original".to_string();
        leaf.aabb = [0.0, 0.0, 1.0, 1.0];
        leaf.points = vec![[0.5, 0.5, 0.0]];
        leaf.key_vertices = vec![[0.5, 0.5, 0.0]];
        leaf.depth_override = Some(4.0);
        leaf.render_instance = Some(RenderInstance {
            source_id: 5,
            translation: [0.0; 3],
        });
        let inner = stub_wire(
            Arc::new(vec![leaf]),
            [0.0, 0.0, 1.0, 1.0],
            [100.0, 50.0, 0.0],
            0.5,
        );
        let mut inner = inner;
        inner.display_visible = false;
        let mut root = stub_wire(
            Arc::new(vec![inner]),
            [100.0, 50.0, 101.0, 51.0],
            [2.0, -10.0, 0.0],
            1.25,
        );
        root.name = "77".to_string();
        root.selected = true;

        let near = [101.0, 39.0, 104.0, 42.0];
        let mut result = Vec::new();
        expand_stub_near_into(&root, Some(near), &mut result);
        assert_eq!(result.len(), 1);
        let got = &result[0];
        assert_eq!(got.name, "77");
        assert!(got.selected);
        assert!(!got.display_visible);
        assert_eq!(got.render_instance.unwrap().translation, [102.0, 40.0, 0.0]);
        assert_eq!(got.depth_override, Some(5.75));
        assert!((got.key_vertices[0][0] - 102.5).abs() < 1e-7);
        assert!((got.key_vertices[0][1] - 40.5).abs() < 1e-7);
        assert!((got.points[0][0] - 102.5).abs() < 1e-4);
        assert!((got.points[0][1] - 40.5).abs() < 1e-4);

        result.clear();
        expand_stub_near_into(&root, Some([0.0, 0.0, 1.0, 1.0]), &mut result);
        assert!(result.is_empty(), "a distant nested block must not materialize");
    }

    #[test]
    fn nonstub_wire_is_untouched_by_near_query() {
        let mut wire = WireModel::default();
        wire.name = "plain".to_string();
        let mut result = Vec::new();
        expand_stub_near_into(&wire, Some([100.0, 100.0, 101.0, 101.0]), &mut result);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "plain");
    }
}
