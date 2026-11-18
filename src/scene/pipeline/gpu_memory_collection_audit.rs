//! Same-process A/B for the old wire census and the borrowed, batch-aware one.
use super::*;
use crate::perf::audit;
use std::{hint::black_box, mem::size_of, time::Instant};

#[derive(Default)]
struct BeforeWireCounter {
    buffers: FxHashSet<wgpu::Buffer>,
}

impl BeforeWireCounter {
    // Exact pre-change buffer/wire counting operations; other categories are
    // covered by the full renderer audit using the preserved before binary.
    fn buffer(&mut self, buffer: &wgpu::Buffer) -> u64 {
        if self.buffers.insert(buffer.clone()) {
            buffer.size()
        } else {
            0
        }
    }

    fn wires(&mut self, wires: &[WireGpu]) -> u64 {
        wires
            .iter()
            .map(|wire| {
                self.buffer(&wire.instance_buffer)
                    + wire
                        .const_buffer
                        .as_ref()
                        .map_or(0, |buffer| self.buffer(buffer))
            })
            .sum()
    }
}

fn before(batches: &[Arc<Vec<WireGpu>>]) -> u64 {
    let mut counter = BeforeWireCounter::default();
    batches.iter().map(|batch| counter.wires(batch)).sum()
}

fn after(batches: &[Arc<Vec<WireGpu>>]) -> u64 {
    let mut counter = ResourceCounter::default();
    batches.iter().map(|batch| counter.wires(batch)).sum()
}

fn sample(batches: &[Arc<Vec<WireGpu>>], collect: fn(&[Arc<Vec<WireGpu>>]) -> u64) -> f64 {
    let start = Instant::now();
    for _ in 0..10 {
        black_box(collect(black_box(batches)));
    }
    start.elapsed().as_secs_f64() * 1e6 / 10.0
}

#[test]
#[ignore = "release wire collection A/B audit; requires adapter"]
fn performance_audit_gpu_collection_wire_paths() {
    assert!(!cfg!(debug_assertions), "use --release");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let info = adapter.get_info();
    let (device, _) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let make_buffer = |size| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        })
    };
    let constants = make_buffer(2048);
    let mut cases = Vec::new();
    for (name, slots, same_batch, unique) in [
        ("shared_batch_1", 1, true, false),
        ("shared_batch_4", 4, true, false),
        ("shared_batch_8", 8, true, false),
        ("separate_batches_shared_buffers_8", 8, false, false),
        ("unique_resources_1", 1, false, true),
        ("unique_resources_8", 8, false, true),
    ] {
        let count = if unique { slots * 1000 } else { 32 };
        let buffers: Vec<_> = (0..count).map(|_| make_buffer(4096)).collect();
        let build = |slot| {
            Arc::new(
                (0..1000)
                    .map(|i| WireGpu {
                        instance_buffer: buffers[if unique { slot * 1000 + i } else { i % count }]
                            .clone(),
                        first_instance: 0,
                        instance_count: 1,
                        is_3d_mesh_edge: false,
                        const_bind_group: None,
                        const_buffer: Some(constants.clone()),
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let first = build(0);
        let mut batches: Vec<_> = (0..slots)
            .map(|slot| {
                if same_batch {
                    first.clone()
                } else {
                    build(slot)
                }
            })
            .collect();
        // The cold cache aliases the first viewport's batch.
        batches.push(batches[0].clone());
        let expected = count as u64 * 4096 + 2048;
        assert_eq!(before(&batches), expected);
        assert_eq!(after(&batches), expected);
        for _ in 0..3 {
            sample(&batches, before);
            sample(&batches, after);
        }
        let timing = audit::paired(100, || sample(&batches, before), || sample(&batches, after));
        let mut old = BeforeWireCounter::default();
        let mut new = ResourceCounter::default();
        for batch in &batches {
            old.wires(batch);
            new.wires(batch);
        }
        let before_key_bytes = old.buffers.capacity() * size_of::<wgpu::Buffer>();
        let after_key_bytes = new.buffers.capacity() * size_of::<&wgpu::Buffer>()
            + new.wire_batches.capacity() * size_of::<(usize, usize)>();
        assert_eq!(old.buffers.len(), new.buffers.len());
        assert!(after_key_bytes < before_key_bytes);
        eprintln!(
            "GPU_WIRES {name}: {:.3} -> {:.3} us median; keys {} -> {} bytes",
            timing.before.median_us, timing.after.median_us, before_key_bytes, after_key_bytes
        );
        cases.push(
            serde_json::json!({"case":name,"slots":slots,"wire_ranges_per_slot":1000,
            "unique_buffers":old.buffers.len(),"visited_batches_after":new.wire_batches.len(),
            "logical_buffer_bytes":expected,"reserved_key_capacity_bytes_before":before_key_bytes,
            "reserved_key_capacity_bytes_after":after_key_bytes,"timing":timing}),
        );
    }
    audit::write_report(
        "gpu-collection-wire-paths",
        serde_json::json!({
            "adapter":format!("{} ({:?})",info.name,info.backend),
            "scope":"wire collection component, not full renderer; old owned-handle inserts and all-range traversal versus borrowed handles and per-census batch deduplication; 100 alternating pairs after three warmups, each averaging 10 calls including counter construction/destruction; GPU allocation/setup excluded; identical logical bytes checked",
            "key_storage_scope":"reserved key capacity times Rust key size only; excludes bucket/control/alignment, allocator overhead and table headers; not measured heap peak or VRAM",
            "buffer_key_bytes_before":size_of::<wgpu::Buffer>(),"buffer_key_bytes_after":size_of::<&wgpu::Buffer>(),
            "wire_record_bytes":size_of::<WireGpu>(),"cases":cases,
        }),
    );
}
