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
    let Some(stub) = stub_wire.stub.as_ref() else {
        out.push(stub_wire.clone());
        return;
    };
    for wire in stub.prototype.iter() {
        let mut copy = crate::scene::cache::block_cache::translated_prototype_wire(
            wire,
            &stub_wire.name,
            stub.delta,
        );
        shift_depth(&mut copy, stub.depth_delta);
        copy.selected = stub_wire.selected;
        copy.display_visible &= stub_wire.display_visible;
        if copy.stub.is_some() {
            expand_stub_into(&copy, out);
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
