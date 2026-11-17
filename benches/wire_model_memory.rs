//! Deterministic resident-memory and clone probe for the scene's wire models.
//!
//! The scene keeps one resident `Vec<WireModel>` per space/block and shares it
//! by `Arc`; the copies that still reach the heap are the ones interaction
//! makes — a grip drag clones the wires it draws as its drag-start ghost, and
//! the paper dressing clones the whole set. Each wire carries ~a dozen `Vec`s,
//! so those copies duplicate geometry that did not change.
//!
//! Fixed synthetic drawing, allocated in a fixed order, so the numbers stay
//! comparable across runs. Requested bytes only (no allocator overhead, no GPU
//! memory), same counting allocator as `cache_memory_budget`.
//!
//! Run: `cargo bench --bench wire_model_memory`

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use document::entities::{Insert, Line};
use document::types::Vector3;
use document::EntityType;
use OpenCADStudio::scene::{Scene, WireModel};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);

fn add_live(bytes: usize) {
    LIVE.fetch_add(bytes, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add_live(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            add_live(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                add_live(new_size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

const MODEL_LINES: usize = 20_000;
const BLOCK_DEFS: usize = 20;
const ENTITIES_PER_BLOCK: usize = 40;
const SAMPLES: usize = 7;

fn build_scene() -> Scene {
    let mut scene = Scene::new();
    for b in 0..BLOCK_DEFS {
        let name = format!("Blk{b}");
        let mut ents = Vec::with_capacity(ENTITIES_PER_BLOCK);
        for i in 0..ENTITIES_PER_BLOCK {
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(0.0, i as f64, 0.0),
                Vector3::new(10.0, i as f64, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
        scene.add_entity(EntityType::Insert(Insert::new(
            &name,
            Vector3::new((b as f64) * 20.0, -50.0, 0.0),
        )));
    }
    for i in 0..MODEL_LINES {
        let x = (i % 500) as f64 * 10.0;
        let y = (i / 500) as f64 * 10.0;
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 8.0, y + 2.0, 0.0),
        )));
    }
    scene
}

/// What the drawing holds, so two runs prove they measured the same work.
fn sink(wires: &[WireModel]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for wire in wires {
        hash = hash
            .wrapping_mul(0x100_0000_01b3)
            .wrapping_add(wire.points.len() as u64);
        for p in &wire.points {
            hash = hash
                .wrapping_mul(0x100_0000_01b3)
                .wrapping_add(p[0].to_bits() as u64);
            hash = hash
                .wrapping_mul(0x100_0000_01b3)
                .wrapping_add(p[1].to_bits() as u64);
        }
    }
    hash
}

/// Requested heap bytes a wire's vectors hold, per field group, so the
/// resident total can be attributed.
fn heap_breakdown(wires: &[WireModel]) -> Vec<(&'static str, usize)> {
    let v3 = std::mem::size_of::<[f32; 3]>();
    let f32s = std::mem::size_of::<f32>();
    let mut points = 0;
    let mut points_low = 0;
    let mut tris = 0;
    let mut text = 0;
    let mut snap = 0;
    let mut tangents = 0;
    let mut keys = 0;
    let mut stations = 0;
    let mut taper = 0;
    for wire in wires {
        let elem = |n: usize, bytes: usize| n * bytes;
        points += elem(wire.points.capacity(), v3);
        points_low += elem(wire.points_low.capacity(), v3);
        tris += elem(
            wire.fill_tris.capacity() + wire.fill_tris_low.capacity(),
            v3,
        );
        tris += elem(
            wire.pick_tris.capacity() + wire.pick_tris_low.capacity(),
            v3,
        );
        text += elem(
            wire.text_verts.capacity(),
            wire.text_verts.first().map_or(0, std::mem::size_of_val),
        );
        snap += elem(
            wire.snap_pts.capacity(),
            wire.snap_pts.first().map_or(0, std::mem::size_of_val),
        );
        tangents += elem(
            wire.tangent_geoms.capacity(),
            wire.tangent_geoms.first().map_or(0, std::mem::size_of_val),
        );
        keys += elem(
            wire.key_vertices.capacity(),
            std::mem::size_of::<[f64; 3]>(),
        );
        stations += elem(wire.pattern_stations.capacity(), f32s);
        taper += elem(wire.taper_widths.capacity(), f32s);
    }
    vec![
        ("points", points),
        ("points_low", points_low),
        ("tris", tris),
        ("text_verts", text),
        ("snap_pts", snap),
        ("tangent_geoms", tangents),
        ("key_vertices", keys),
        ("pattern_stations", stations),
        ("taper_widths", taper),
    ]
}

fn main() {
    let before_build = live();
    let mut scene = build_scene();
    let cam = scene.camera.borrow().clone();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    let resident = live() - before_build;
    let wire_count = wires.len();
    let expected = sink(&wires);
    drop(wires);

    let mut clone_bytes = Vec::with_capacity(SAMPLES);
    let mut clone_us = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        let before = live();
        let started = Instant::now();
        let ghost = black_box(wires.as_ref().clone());
        let elapsed = started.elapsed().as_secs_f64() * 1e6;
        let bytes = live() - before;
        assert_eq!(sink(&ghost), expected, "the ghost copy is the same drawing");
        drop(ghost);
        clone_bytes.push(bytes);
        clone_us.push(elapsed);
    }
    clone_bytes.sort_unstable_by(|a, b| a.partial_cmp(b).expect("finite bytes"));
    clone_us.sort_unstable_by(|a, b| a.partial_cmp(b).expect("finite micros"));
    let median_bytes = clone_bytes[clone_bytes.len() / 2];
    let median_us = clone_us[clone_us.len() / 2];

    let _ = black_box(&mut scene);
    println!(
        "wire_struct      {} B  ({} wires, array alone {:.3} MB)",
        std::mem::size_of::<WireModel>(),
        wire_count,
        (wire_count * std::mem::size_of::<WireModel>()) as f64 / (1024.0 * 1024.0),
    );
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    for (field, bytes) in heap_breakdown(&wires) {
        println!("  {field:<17} {:.3} MB", bytes as f64 / (1024.0 * 1024.0));
    }
    drop(wires);
    println!(
        "scene_resident   {:.3} MB  ({} wires, {:.1} B/wire)",
        resident as f64 / (1024.0 * 1024.0),
        wire_count,
        resident as f64 / wire_count.max(1) as f64,
    );
    println!(
        "ghost_clone      {:.3} MB  {:.1} us  ({:.1} B/wire, sink={expected:016x})",
        median_bytes as f64 / (1024.0 * 1024.0),
        median_us,
        median_bytes as f64 / wire_count.max(1) as f64,
    );
}
