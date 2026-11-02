use super::*;
use iced::futures::executor::block_on;

#[test]
fn packing_aligns_records_and_chunks_at_buffer_and_offset_limits() {
    for alignment in [256, 512] {
        let limits = wgpu::Limits {
            min_uniform_buffer_offset_alignment: alignment,
            max_buffer_size: 3 * 16384,
            ..Default::default()
        };
        let packing = WipeoutPacking::new(&limits).unwrap();
        assert_eq!(packing.capacity, 3);
        assert_eq!(1000usize.div_ceil(packing.capacity), 334);
        for index in 0..packing.capacity {
            let offsets = packing.offsets(index);
            for (offset, size) in offsets.into_iter().zip([
                std::mem::size_of::<HatchUniformData>(),
                std::mem::size_of::<BoundaryData>(),
            ]) {
                assert_eq!(offset % alignment, 0);
                assert!(offset as u64 + size as u64 <= limits.max_buffer_size);
            }
        }
        assert_eq!(packing.offsets(0), [0, 0]);
    }
    let packing = WipeoutPacking::new(&wgpu::Limits {
        max_buffer_size: 1 << 34,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(packing.capacity, ((1u64 << 32) / 16384) as usize);
    assert_eq!(packing.offsets(packing.capacity - 1)[1], u32::MAX - 16383);
}

#[test]
fn thousand_wipeouts_share_bind_groups() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    if cfg!(target_os = "windows") {
        descriptor.backends = wgpu::Backends::DX12;
    }
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("SKIP thousand_wipeouts_share_bind_groups: no GPU adapter");
        return;
    };
    eprintln!("wipeout test adapter: {:?}", adapter.get_info());
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_non_sampler_bindings: 2048,
            ..Default::default()
        },
        ..Default::default()
    }))
    .expect("GPU device");
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("wipeout test layout"),
        entries: &std::array::from_fn::<_, 3, _>(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: binding < 2,
                min_binding_size: None,
            },
            count: None,
        }),
    });
    let models: Vec<_> = (0..1000)
        .map(|index| HatchModel {
            pattern_origin: None,
            render_instance: None,
            world_origin: [index as f64 * 2.0, 0.0],
            boundary: std::sync::Arc::new(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: codec::entities::HatchStyleType::Normal,
            pattern: HatchPattern::Solid,
            name: "WIPEOUT".into(),
            color: [0.0, 0.0, 0.0, 1.0],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: index as f32 / 1000.0,
        })
        .collect();
    assert!(WipeoutGpu::from_models(&device, &queue, &[], &layout).is_empty());
    let gpu = WipeoutGpu::from_models(&device, &queue, &models, &layout);
    queue.submit([]);
    assert!(block_on(validation.pop()).is_none());
    assert_eq!(gpu.len(), 1000);
    let groups: std::collections::HashSet<_> = gpu.iter().map(|mask| &mask.bind_group).collect();
    assert_eq!(groups.len(), 1, "1000 masks must share a bind group");
    for (index, mask) in gpu.iter().enumerate() {
        assert_eq!(mask.instance_count, 1);
        assert_eq!(
            mask.world_aabb,
            [index as f32 * 2.0, 0.0, index as f32 * 2.0 + 1.0, 1.0]
        );
        assert_eq!(
            mask.dynamic_offsets,
            WipeoutPacking::new(&device.limits())
                .unwrap()
                .offsets(index)
        );
    }
}

#[test]
fn packing_rejects_insufficient_limits_without_panicking() {
    for limits in [
        wgpu::Limits {
            max_uniform_buffer_binding_size: 16383,
            ..Default::default()
        },
        wgpu::Limits {
            max_buffer_size: 16383,
            ..Default::default()
        },
        wgpu::Limits {
            max_buffer_size: 16384,
            min_uniform_buffer_offset_alignment: 32768,
            ..Default::default()
        },
    ] {
        assert!(WipeoutPacking::new(&limits).is_none());
    }
}
