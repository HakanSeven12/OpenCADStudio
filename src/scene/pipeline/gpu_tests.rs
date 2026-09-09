use super::*;
use crate::scene::model::{mesh_model::MeshLodSet, mesh_model::MeshModel, wire_model::WireModel};
use acadrust::{types::Transform, Handle};
use iced::futures::executor::block_on;

#[test]
#[ignore = "requires a GPU adapter"]
fn block_edits_preserve_cache_coordinates_and_arena_partition() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    // Definition geometry is shared across viewport slots, so it lives on the
    // MultiPipeline; a single-slot test supplies its own.
    let mut block_geometry = wire_gpu::BlockGeometryCache::default();
    let block = |handle: u64, x: f64| WireModel {
        name: handle.to_string(),
        points: vec![[x as f32, 0.0, 0.0], [x as f32 + 1.0, 0.0, 0.0]],
        render_instance: Some(crate::scene::model::instance_model::RenderInstance {
            source_id: 1,
            translation: [x, 0.0, 0.0],
        }),
        ..Default::default()
    };
    let first = block(1, 10.0);
    let second = block(2, 20.0);
    let depth = rustc_hash::FxHashMap::from_iter([(1, [0.1, 0.01]), (2, [0.2, 0.01])]);
    let original = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth, &mut block_geometry);
    let unchanged = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth, &mut block_geometry);
    assert_eq!(original[0].geometry_id(), unchanged[0].geometry_id());
    assert_eq!(original[0].instance_count, 2);

    let removed = pipeline.upload_block_wires(&device, &queue, &[&second], &depth, &mut block_geometry);
    assert_ne!(
        original[0].geometry_id(),
        removed[0].geometry_id(),
        "removing the base instance must replace vertices baked at its old position"
    );
    let moved = block(2, 30.0);
    let moved_gpu = pipeline.upload_block_wires(&device, &queue, &[&moved], &depth, &mut block_geometry);
    assert_ne!(removed[0].geometry_id(), moved_gpu[0].geometry_id());
    let restored = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth, &mut block_geometry);
    assert_ne!(moved_gpu[0].geometry_id(), restored[0].geometry_id());
    assert_eq!(restored[0].instance_count, 2);
    assert_eq!(
        block_geometry.len(),
        1,
        "discard obsolete geometry"
    );

    let line = WireModel {
        render_instance: None,
        ..first.clone()
    };
    let changed = [second];
    let (regular, _) = wire_arena::split_wires(&changed);
    let runs = rustc_hash::FxHashMap::from_iter([(Handle::new(2), regular)]);
    for layout in [pipeline.wire_const_bgl.as_ref(), None] {
        let mut arena = wire_arena::PersistentWireArena::build(
            &device,
            &queue,
            &[&line],
            &depth,
            layout,
            false,
        )
        .unwrap();
        assert!(arena.patch(
            &queue,
            &[(Handle::new(2), crate::scene::ChangeKind::Added)],
            &runs,
            true,
            &depth
        ));
        assert_eq!(
            arena
                .wire_gpus()
                .iter()
                .map(|gpu| gpu.instance_count)
                .sum::<u32>(),
            1,
            "the block must not also enter the regular arena"
        );
    }
    queue.submit([]);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert!(
        block_on(validation.pop()).is_none(),
        "GPU validation failed"
    );
}

// OCS_GPU_CHUNK_MIB=1 cargo test --lib bounded_gpu_uploads -- --ignored --nocapture
#[test]
#[ignore = "requires a GPU adapter and OCS_GPU_CHUNK_MIB=1"]
fn bounded_gpu_uploads_preserve_geometry_and_instancing() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    eprintln!("GPU regression adapter: {:?}", adapter.get_info());
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let budget = gpu_budget::buffer_budget(&device) as u64;
    assert_eq!(budget, 1024 * 1024, "run with OCS_GPU_CHUNK_MIB=1");
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    let line = WireModel {
        points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        ..Default::default()
    };
    let count = gpu_budget::max_storage_elements::<wire_gpu::WireConst>(&device) + 2;
    let wires = vec![line; count];
    for bgl in [pipeline.wire_const_bgl.as_ref(), None] {
        let chunks =
            wire_gpu::WireGpu::from_run(&device, &queue, &wires, &Default::default(), false, bgl);
        assert!(chunks.len() > 1);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.instance_count as usize)
                .sum::<usize>(),
            count
        );
        assert!(chunks
            .iter()
            .all(|chunk| chunk.instance_buffer.size() <= budget));
    }
    let segments = gpu_budget::max_elements::<wire_gpu::WireInstance>(&device) + 1;
    let dense = WireModel {
        points: (0..=segments).map(|i| [i as f32, 0.0, 0.0]).collect(),
        ..Default::default()
    };
    let chunks = wire_gpu::WireGpu::from_run(
        &device,
        &queue,
        &[dense],
        &Default::default(),
        false,
        pipeline.wire_const_bgl.as_ref(),
    );
    assert!(chunks.len() > 1);
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| chunk.instance_count as usize)
            .sum::<usize>(),
        segments
    );
    assert!(chunks
        .iter()
        .all(|chunk| chunk.instance_buffer.size() <= budget));

    let vertices = vec![
        bytemuck::Zeroable::zeroed();
        gpu_budget::max_elements_grouped::<text_gpu::TextVertex>(&device, 6) + 6
    ];
    let text = text_gpu::upload_vertices(&device, &queue, &vertices);
    assert_eq!(text.len(), 2);
    assert_eq!(
        text.iter()
            .map(|chunk| chunk.vertex_count as usize)
            .sum::<usize>(),
        vertices.len()
    );
    assert!(text
        .iter()
        .all(|chunk| chunk.vertex_count % 6 == 0 && chunk.vertex_buffer.size() <= budget));

    let source = |triangles: usize| {
        let mut set = MeshLodSet::from_single(MeshModel {
            name: "7".to_owned(),
            verts: (0..triangles)
                .flat_map(|_| [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
                .collect(),
            verts_low: Vec::new(),
            normals: Vec::new(),
            indices: (0..triangles as u32 * 3).collect(),
            triangle_material_handles: Vec::new(),
            triangle_colors: Vec::new(),
            color: [1.0; 4],
            selected: false,
        });
        set.prepare_instance_source(Handle::new(7));
        set.lods.clear();
        set.instance_transform = Some(Transform::identity());
        set.instance_handle = Some(Handle::new(101));
        set
    };
    let triangles = gpu_budget::max_elements::<mesh_gpu::MeshVertex>(&device) / 3 + 1;
    let first = source(triangles);
    let mut second = first.clone();
    second.instance_handle = Some(Handle::new(102));
    let (mesh, total) = mesh_gpu::build_mesh_batch(&device, &queue, &[first, second]);
    assert!(mesh.len() > 1);
    assert_eq!(total, (triangles * 2) as u64);
    assert_eq!(
        mesh.iter()
            .map(|chunk| chunk.index_count as usize / 3)
            .sum::<usize>(),
        triangles
    );
    for chunk in &mesh {
        assert_eq!(
            chunk.instance_count, 2,
            "large geometry must remain instanced"
        );
        assert_eq!(chunk.handles.len(), 2);
        assert_eq!(chunk.highlight_ranges.len(), 2);
        assert!(chunk
            .highlight_ranges
            .iter()
            .all(|range| range.index_count == chunk.index_count && range.instance_start < 2));
        for buffer in [
            &chunk.vertex_buffer,
            &chunk.index_buffer,
            &chunk.transp_index_buffer,
            &chunk.edge_vertex_buffer,
            &chunk.wire_vertex_buffer,
            &chunk.instance_buffer,
        ] {
            assert!(
                buffer.size() <= budget,
                "buffer size {} exceeds {budget}",
                buffer.size()
            );
        }
    }

    let instance_count = gpu_budget::max_elements::<mesh_gpu::MeshInstanceGpu>(&device) + 1;
    let mut repeated = vec![source(1); instance_count];
    for (index, set) in repeated.iter_mut().enumerate() {
        set.instance_handle = Some(Handle::new(1000 + index as u64));
    }
    let (mesh, total) = mesh_gpu::build_mesh_batch(&device, &queue, &repeated);
    assert_eq!(mesh.len(), 2);
    assert_eq!(total, instance_count as u64);
    assert_eq!(mesh[0].vertex_buffer, mesh[1].vertex_buffer);
    assert_eq!(mesh[0].index_buffer, mesh[1].index_buffer);
    assert_eq!(
        mesh.iter()
            .map(|chunk| chunk.instance_count as usize)
            .sum::<usize>(),
        instance_count
    );
    for chunk in &mesh {
        assert!(chunk.instance_buffer.size() <= budget);
        assert_eq!(chunk.highlight_ranges.len(), chunk.instance_count as usize);
        assert!(chunk
            .highlight_ranges
            .iter()
            .all(|range| range.instance_start < chunk.instance_count));
    }

    queue.submit([]);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert!(
        block_on(validation.pop()).is_none(),
        "GPU validation failed"
    );
}

/// The shadow map is 16 MiB and every slot used to hold one whether or not its
/// visual style enabled shadows. Now it is allocated on the transition, which
/// means the frame bind group is rebuilt while the device is watching — the one
/// path the other GPU tests never take.
#[test]
#[ignore = "requires a GPU adapter"]
fn toggling_shadows_reallocates_without_upsetting_the_device() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    assert!(
        pipeline.shadow_full.is_none(),
        "a fresh slot must not hold a shadow map"
    );
    let camera = crate::scene::view::camera::Camera::default();
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(64.0, 64.0));
    let with_shadows = |on: bool| {
        let mut u = Uniforms::new(&camera, bounds, false);
        u.shadow_params[0] = if on { 1.0 } else { 0.0 };
        u
    };

    // Off → on → off → on, so both transitions are exercised twice and the
    // second `on` proves the release did not leave the bind group stale.
    for expected in [true, false, true] {
        pipeline.upload_uniforms(&device, &queue, &with_shadows(expected));
        assert_eq!(
            pipeline.shadow_full.is_some(),
            expected,
            "shadow target should follow shadow_params[0]"
        );
    }
    // Releasing a cold slot gives it back and must rebuild the bind group too.
    pipeline.release_heavy_resources(&device);
    assert!(pipeline.shadow_full.is_none(), "a released slot holds none");

    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert!(
        block_on(validation.pop()).is_none(),
        "GPU validation failed"
    );
}

#[test]
#[ignore = "requires a GPU adapter"]
fn urgent_release_preserves_visible_panes_and_frees_cold_slots() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut pipeline = <MultiPipeline as iced::widget::shader::Pipeline>::new(
        &device,
        &queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let first = pipeline.resolve_slots(&device, &queue, &[10])[0];
    pipeline.inners[first].slot_id = 10;
    pipeline.inners[first].ensure_depth_texture(&device, Size::new(512, 512));
    let second = pipeline.resolve_slots(&device, &queue, &[20])[0];
    pipeline.inners[second].slot_id = 20;
    pipeline.inners[second].ensure_depth_texture(&device, Size::new(512, 512));
    pipeline.release_idle_slots(&device, &[second], true);
    assert_eq!(
        pipeline.inners[first].alloc_size,
        Size::new(512, 512),
        "a sibling pane prepared immediately before this pane is still visible"
    );

    pipeline
        .frame_rendered
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let next = pipeline.resolve_slots(&device, &queue, &[20]);
    assert_eq!(
        pipeline.release_idle_slots(&device, &next, true),
        0,
        "a previous-frame pane may still be prepared later in this frame"
    );
    pipeline
        .frame_rendered
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let next = pipeline.resolve_slots(&device, &queue, &[20]);
    assert_eq!(pipeline.release_idle_slots(&device, &next, true), 1);
    assert_eq!(pipeline.inners[first].alloc_size, Size::new(0, 0));
    assert_eq!(pipeline.inners[second].alloc_size, Size::new(512, 512));
}

#[test]
#[ignore = "requires a GPU adapter"]
fn late_device_errors_invalidate_uploaded_content_and_targets() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut pipeline = <MultiPipeline as iced::widget::shader::Pipeline>::new(
        &device,
        &queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let inner = &mut pipeline.inners[0];
    inner.cached_wire_id = 7;
    inner.wire_arena_id = 7;
    inner.ensure_depth_texture(&device, Size::new(512, 512));
    let before = gpu_errors_seen();
    let _rejected = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rejected test allocation"),
        size: device.limits().max_buffer_size + 4,
        usage: wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert!(gpu_errors_seen() > before);
    inner.sync_gpu_error_epoch();
    assert_eq!(inner.cached_wire_id, u64::MAX);
    assert_eq!(inner.wire_arena_id, u64::MAX);
    assert_eq!(inner.alloc_size, Size::new(0, 0));
    assert_eq!(inner.background_source_id, usize::MAX);
    inner.cached_wire_id = 8;
    inner.sync_gpu_error_epoch();
    assert_eq!(
        inner.cached_wire_id, 8,
        "the same error must not invalidate twice"
    );
}

#[test]
#[ignore = "requires a GPU adapter"]
fn test_selected_circle_arc_ellipse_highlight_overlay() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    let depth = rustc_hash::FxHashMap::default();

    let mut circle_wire = WireModel::default();
    circle_wire.name = "circle_wire".into();
    circle_wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarCircle {
        center: [0.0, 0.0, 0.0],
        axis_x: [1.0, 0.0, 0.0],
        axis_y: [0.0, 1.0, 0.0],
        radius: 10.0,
    });

    let handle = acadrust::Handle::new(100);
    let mut selected_handles = rustc_hash::FxHashSet::default();
    selected_handles.insert(handle);
    let hover_handles = rustc_hash::FxHashSet::default();

    let mut index = rustc_hash::FxHashMap::default();
    index.insert(handle.value(), vec![0]);
    pipeline.wire_handle_index = std::sync::Arc::new(index);

    let wires = vec![circle_wire];
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &selected_handles,
        &hover_handles,
        &[],
        &depth,
        Some([0.0, 1.0, 1.0, 1.0]),
    );

    assert_eq!(pipeline.gpu_selected_circles.len(), 1);
    assert_eq!(pipeline.gpu_selected_circles[0].instance_count, 1);
    assert!(pipeline.gpu_selected_wires.is_empty());

    // Verify classic highlight mode (SELECTIONEFFECT 0 / selected_tint: None)
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &selected_handles,
        &hover_handles,
        &[],
        &depth,
        None,
    );
    assert_eq!(pipeline.gpu_selected_circles.len(), 1);
    assert_eq!(pipeline.gpu_selected_circles[0].instance_count, 1);

    // Verify hover highlight
    let mut circle_hover = rustc_hash::FxHashSet::default();
    circle_hover.insert(handle);
    let empty_selected = rustc_hash::FxHashSet::default();
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &empty_selected,
        &circle_hover,
        &[],
        &depth,
        None,
    );
    assert_eq!(pipeline.gpu_selected_circles.len(), 1);
    assert_eq!(pipeline.gpu_selected_circles[0].instance_count, 1);

    pipeline.ensure_depth_texture(&device, iced::Size::new(512, 512));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test_target"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_encoder"),
    });
    pipeline.render(
        &mut encoder,
        &target,
        iced::Size::new(512, 512),
        iced::Rectangle { x: 0, y: 0, width: 512, height: 512 },
        [0.0, 0.0, 0.0, 1.0],
        false,
        false,
        false,
    );
    queue.submit(Some(encoder.finish()));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}

#[test]
#[ignore = "requires a GPU adapter"]
fn test_thick_and_tapered_arc_gpu_rendering() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    // 1. Wide circular arc with pick triangles
    let mut wide_arc = WireModel::default();
    wide_arc.name = "wide_arc".into();
    wide_arc.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
        center: [50.0, 50.0, 0.0],
        axis_x: [1.0, 0.0, 0.0],
        axis_y: [0.0, 1.0, 0.0],
        radius: 30.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::PI,
    });
    wide_arc.world_width = 10.0;
    wide_arc.color = [1.0, 0.2, 0.2, 1.0];
    wide_arc.pick_tris = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    wide_arc.pick_tris_low = vec![[0.0; 3]; 3];

    // 2. Tapered circular arc
    let mut tapered_arc = WireModel::default();
    tapered_arc.name = "tapered_arc".into();
    tapered_arc.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
        center: [150.0, 50.0, 0.0],
        axis_x: [1.0, 0.0, 0.0],
        axis_y: [0.0, 1.0, 0.0],
        radius: 40.0,
        start_angle: 0.2,
        end_angle: 2.8,
    });
    tapered_arc.world_width = 16.0;
    tapered_arc.taper_widths = vec![2.0, 16.0];
    tapered_arc.color = [0.2, 0.8, 1.0, 1.0];

    let wires = vec![wide_arc, tapered_arc];
    let depth_map = rustc_hash::FxHashMap::default();
    let circles = pipeline.upload_circles(&device, &queue, &wires, &depth_map);

    // Both curves must be routed to GPU analytical circle instances!
    assert!(!circles.is_empty(), "analytical circles must be uploaded");
    assert_eq!(circles[0].instance_count, 2);
    pipeline.gpu_circles = std::sync::Arc::new(circles);
    assert_eq!(pipeline.gpu_circles.len(), 1);
    assert_eq!(pipeline.gpu_circles[0].instance_count, 2);

    pipeline.ensure_depth_texture(&device, iced::Size::new(512, 512));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("thick_arc_target"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("thick_arc_encoder"),
    });
    pipeline.render(
        &mut encoder,
        &target,
        iced::Size::new(512, 512),
        iced::Rectangle { x: 0, y: 0, width: 512, height: 512 },
        [0.0, 0.0, 0.0, 1.0],
        false,
        false,
        false,
    );
    queue.submit(Some(encoder.finish()));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}

#[test]
#[ignore = "requires a GPU adapter"]
fn test_tilted_3d_donut_and_thick_arc_gpu_rendering() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    // Setup 3D perspective camera with acute tilt
    let mut camera = crate::scene::view::camera::Camera::default();
    camera.projection = crate::scene::view::camera::Projection::Perspective;
    camera.distance = 200.0;
    camera.target = glam::DVec3::new(50.0, 50.0, 0.0);
    camera.rotation = glam::Quat::from_axis_angle(glam::Vec3::X, 1.1); // ~63 degree tilt (acute perspective)
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(512.0, 512.0));
    let uniforms = Uniforms::new(&camera, bounds, false);
    pipeline.upload_uniforms(&device, &queue, &uniforms);

    // Donut circular arc segments
    let mut donut = WireModel::default();
    donut.name = "donut".into();
    donut.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
        center: [50.0, 50.0, 0.0],
        axis_x: [1.0, 0.0, 0.0],
        axis_y: [0.0, 1.0, 0.0],
        radius: 30.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::PI,
    });
    donut.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
        center: [50.0, 50.0, 0.0],
        axis_x: [1.0, 0.0, 0.0],
        axis_y: [0.0, 1.0, 0.0],
        radius: 30.0,
        start_angle: std::f64::consts::PI,
        end_angle: std::f64::consts::TAU,
    });
    donut.world_width = 12.0;
    donut.color = [0.2, 0.9, 0.3, 1.0];

    let wires = vec![donut];
    let depth_map = rustc_hash::FxHashMap::default();
    let circles = pipeline.upload_circles(&device, &queue, &wires, &depth_map);

    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].instance_count, 2);
    pipeline.gpu_circles = std::sync::Arc::new(circles);

    // Wide straight polyline segment on the 3D plane
    let mut straight_wide = WireModel::default();
    straight_wide.name = "wide_straight".into();
    straight_wide.points = vec![[0.0, 0.0, 0.0], [50.0, 50.0, 0.0]];
    straight_wide.world_width = 10.0;
    straight_wide.color = [0.9, 0.2, 0.3, 1.0];

    let wide_wires = vec![straight_wide];
    let gpu_wires = wire_gpu::WireGpu::from_run(
        &device,
        &queue,
        &wide_wires,
        &depth_map,
        false,
        pipeline.wire_const_bgl.as_ref(),
    );
    assert!(!gpu_wires.is_empty());
    pipeline.gpu_wires = std::sync::Arc::new(gpu_wires);

    pipeline.ensure_depth_texture(&device, iced::Size::new(512, 512));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tilted_donut_target"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("tilted_donut_encoder"),
    });
    pipeline.render(
        &mut encoder,
        &target,
        iced::Size::new(512, 512),
        iced::Rectangle { x: 0, y: 0, width: 512, height: 512 },
        [0.0, 0.0, 0.0, 1.0],
        false,
        false,
        false,
    );
    queue.submit(Some(encoder.finish()));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}

#[test]
#[ignore = "requires a GPU adapter"]
fn test_selected_ellipse_overlay() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    let depth = rustc_hash::FxHashMap::default();

    let mut ellipse_wire = WireModel::default();
    ellipse_wire.name = "ellipse_wire".into();
    ellipse_wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarEllipse {
        center: [0.0, 0.0, 0.0],
        major_axis: [10.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        minor_axis_ratio: 0.5,
        start_param: 0.0,
        end_param: std::f64::consts::TAU,
    });

    let handle = acadrust::Handle::new(200);
    let mut selected_handles = rustc_hash::FxHashSet::default();
    selected_handles.insert(handle);
    let hover_handles = rustc_hash::FxHashSet::default();

    let mut index = rustc_hash::FxHashMap::default();
    index.insert(handle.value(), vec![0]);
    pipeline.wire_handle_index = std::sync::Arc::new(index);

    let wires = vec![ellipse_wire];
    // Standard tint
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &selected_handles,
        &hover_handles,
        &[],
        &depth,
        Some([0.0, 1.0, 1.0, 1.0]),
    );

    assert_eq!(pipeline.gpu_selected_ellipses.len(), 1);
    assert_eq!(pipeline.gpu_selected_ellipses[0].instance_count, 1);
    assert!(pipeline.gpu_selected_wires.is_empty());

    // Classic highlight mode (SELECTIONEFFECT 0 / selected_tint: None)
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &selected_handles,
        &hover_handles,
        &[],
        &depth,
        None,
    );
    assert_eq!(pipeline.gpu_selected_ellipses.len(), 1);
    assert_eq!(pipeline.gpu_selected_ellipses[0].instance_count, 1);

    // Hover highlight
    let mut ellipse_hover = rustc_hash::FxHashSet::default();
    ellipse_hover.insert(handle);
    let empty_selected = rustc_hash::FxHashSet::default();
    pipeline.upload_selected_wires(
        &device,
        &queue,
        &wires,
        &empty_selected,
        &ellipse_hover,
        &[],
        &depth,
        None,
    );
    assert_eq!(pipeline.gpu_selected_ellipses.len(), 1);
    assert_eq!(pipeline.gpu_selected_ellipses[0].instance_count, 1);

    pipeline.ensure_depth_texture(&device, iced::Size::new(512, 512));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test_target_ellipse"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_encoder_ellipse"),
    });
    pipeline.render(
        &mut encoder,
        &target,
        iced::Size::new(512, 512),
        iced::Rectangle { x: 0, y: 0, width: 512, height: 512 },
        [0.0, 0.0, 0.0, 1.0],
        false,
        false,
        false,
    );
    queue.submit(Some(encoder.finish()));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
}

#[test]
#[ignore = "requires a GPU adapter"]
fn test_pline_arc_switch_preview_and_render() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    let depth = rustc_hash::FxHashMap::default();
    use crate::command::CadCommand;

    let mut cmd = crate::modules::draw::draw::polyline::PlineCommand::new();
    cmd.on_point(glam::DVec3::new(0.0, 0.0, 0.0));
    let _res = cmd.on_point(glam::DVec3::new(10.0, 0.0, 0.0));
    cmd.set_live_handle(acadrust::Handle::new(1));
    cmd.on_text_input("A");

    let test_points = [
        glam::DVec3::new(10.0, 0.0, 0.0),
        glam::DVec3::new(10.0000001, 0.0, 0.0),
        glam::DVec3::new(10.0, 0.0000001, 0.0),
        glam::DVec3::new(10.0, -0.0000001, 0.0),
        glam::DVec3::new(9.9999999, 0.0, 0.0),
        glam::DVec3::new(15.0, 5.0, 0.0),
        glam::DVec3::new(10.0, 10.0, 0.0),
        glam::DVec3::new(5.0, 5.0, 0.0),
        glam::DVec3::new(0.0, 0.0, 0.0),
    ];

    pipeline.ensure_depth_texture(&device, iced::Size::new(512, 512));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test_target"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());

    for pt in test_points {
        let wire = cmd.on_mouse_move(pt);
        if let Some(w) = wire {
            pipeline.upload_preview_wires(&device, &queue, &[w], &depth);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("test_encoder"),
            });
            pipeline.render(
                &mut encoder,
                &target,
                iced::Size::new(512, 512),
                iced::Rectangle { x: 0, y: 0, width: 512, height: 512 },
                [0.0, 0.0, 0.0, 1.0],
                false,
                false,
                false,
            );
            queue.submit(Some(encoder.finish()));
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
    }
}
