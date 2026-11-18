use super::super::{MultiPipeline, Pipeline, WireGpu};
use super::*;
use std::sync::Arc;

#[cfg(feature = "performance-audit")]
#[path = "gpu_memory_collection_audit.rs"]
mod audit;

#[test]
#[ignore = "requires a GPU adapter"]
fn shared_batches_prefixes_cold_cache_and_eviction_keep_exact_counts() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let buffer = |size| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        })
    };
    let a = buffer(4096);
    let b = buffer(8192);
    let constants = buffer(2048);
    let wire = |instance_buffer: &wgpu::Buffer, constants: Option<&wgpu::Buffer>| WireGpu {
        instance_buffer: instance_buffer.clone(),
        first_instance: 0,
        instance_count: 1,
        is_3d_mesh_edge: false,
        const_bind_group: None,
        const_buffer: constants.cloned(),
    };
    let wires = Arc::new(vec![wire(&a, Some(&constants)), wire(&b, Some(&constants))]);
    let aliases: Vec<_> = wires.iter().cloned().collect();
    let same_size_distinct = buffer(4096);
    let distinct = vec![wire(&same_size_distinct, None)];
    {
        let mut counter = ResourceCounter::default();
        assert_eq!(counter.wires(&[]), 0);
        assert_eq!(counter.wires(&wires[..1]), 4096 + 2048);
        assert_eq!(
            counter.wires(&wires),
            8192,
            "a prefix must not hide its suffix"
        );
        assert_eq!(counter.wires(&wires), 0);
        assert_eq!(
            counter.wires(&aliases),
            0,
            "separate batches can share handles"
        );
        assert_eq!(
            counter.wires(&distinct),
            4096,
            "equal size is not resource identity"
        );
        assert_eq!(counter.buffers.len(), 4);
        assert_eq!(counter.wire_batches.len(), 4);
    }
    assert_eq!(ResourceCounter::default().wires(&wires), 14336);
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut pipeline =
        <MultiPipeline as iced::widget::shader::Pipeline>::new(&device, &queue, format);
    while pipeline.inners.len() < 8 {
        pipeline.inners.push(Pipeline::new(&device, &queue, format));
    }
    let baseline = pipeline.gpu_live_bytes();
    for slot in &mut pipeline.inners {
        slot.gpu_wires = wires.clone();
        slot.gpu_preview_wires = aliases.clone();
    }
    pipeline.wire_buffer_cache.insert(
        1,
        (
            wires.clone(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        ),
    );
    let shared = pipeline.gpu_live_bytes();
    assert_eq!(shared.total() - baseline.total(), 14336);
    assert_eq!(shared.wires - baseline.wires, 14336);
    assert_eq!(shared.overlays, baseline.overlays);
    for slot in &mut pipeline.inners {
        slot.gpu_wires = Default::default();
        slot.gpu_preview_wires.clear();
    }
    assert_eq!(
        pipeline.gpu_live_bytes().total(),
        shared.total(),
        "cold cache remains live"
    );
    pipeline.wire_buffer_cache.clear();
    let replacement = buffer(16384);
    pipeline.wire_buffer_cache.insert(
        2,
        (
            Arc::new(vec![wire(&replacement, None)]),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        ),
    );
    assert_eq!(pipeline.gpu_live_bytes().wires - baseline.wires, 16384);
    assert_eq!(pipeline.gpu_live_bytes().total() - baseline.total(), 16384);
    pipeline.wire_buffer_cache.clear();
    assert!(
        pipeline.gpu_live_bytes() == baseline,
        "new census must not retain evicted resources"
    );
}
