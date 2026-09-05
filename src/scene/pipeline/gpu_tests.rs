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
    let original = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth);
    let unchanged = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth);
    assert_eq!(original[0].geometry_id(), unchanged[0].geometry_id());
    assert_eq!(original[0].instance_count, 2);

    let removed = pipeline.upload_block_wires(&device, &queue, &[&second], &depth);
    assert_ne!(
        original[0].geometry_id(),
        removed[0].geometry_id(),
        "removing the base instance must replace vertices baked at its old position"
    );
    let moved = block(2, 30.0);
    let moved_gpu = pipeline.upload_block_wires(&device, &queue, &[&moved], &depth);
    assert_ne!(removed[0].geometry_id(), moved_gpu[0].geometry_id());
    let restored = pipeline.upload_block_wires(&device, &queue, &[&first, &second], &depth);
    assert_ne!(moved_gpu[0].geometry_id(), restored[0].geometry_id());
    assert_eq!(restored[0].instance_count, 2);
    assert_eq!(
        pipeline.block_geometry.len(),
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
