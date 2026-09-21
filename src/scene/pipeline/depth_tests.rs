//! Pixel regressions for projection and draw-order clipping, using the actual
//! wire pipelines in both storage and WebGL-compatible packed modes.
use super::*;
use crate::scene::view::camera::{Camera, Projection};
use iced::futures::executor::block_on;

const SIZE: u32 = 512;

fn pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    uniforms: &Uniforms,
    wires: &[WireModel],
    depths: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Vec<u8> {
    let size = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("draw-order regression"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("draw-order readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    pipeline.ensure_depth_texture(device, Size::new(SIZE, SIZE));
    pipeline.upload_uniforms(device, queue, uniforms);
    pipeline.gpu_wires = std::sync::Arc::new(WireGpu::from_batch(
        device,
        queue,
        wires,
        depths,
        pipeline.wire_const_bgl.as_ref(),
    ));
    let mut encoder = device.create_command_encoder(&Default::default());
    let rect = Rectangle {
        x: 0,
        y: 0,
        width: SIZE,
        height: SIZE,
    };
    pipeline.render(
        &mut encoder,
        &view,
        Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32)),
        rect,
        rect,
        [0., 0., 0., 1.],
        false,
        false,
        false,
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let result = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    result
}

fn color_at(bytes: &[u8], ndc_x: f32) -> [u32; 3] {
    let x = ((ndc_x * 0.5 + 0.5) * SIZE as f32) as usize;
    let mut color = [0; 3];
    for col in x - 2..=x + 2 {
        let offset = (SIZE as usize / 2 * SIZE as usize + col) * 4;
        for c in 0..3 {
            color[c] += bytes[offset + c] as u32;
        }
    }
    color
}

#[test]
#[ignore = "requires a GPU adapter"]
fn wipeout_respects_close_block_draw_order() {
    use acadrust::{entities::Wipeout, types::Vector2, EntityType};
    use crate::scene::{model::wire_model::TangentGeom, Scene};

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    for packed in [false, true] {
        let mut limits = adapter.limits();
        if packed {
            limits.max_storage_buffers_per_shader_stage = 0;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .expect("GPU device");
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
        let mut uniforms = Uniforms::new(&Camera::default(), bounds, false);
        uniforms.view_rot = glam::Mat4::IDENTITY;
        uniforms.eye_high = [0.; 3];
        uniforms.eye_low = [0.; 3];
        let mut scene = Scene::new();
        scene.add_entity(EntityType::Wipeout(Wipeout::polygonal(
            &[
                Vector2::new(-0.7, -0.7),
                Vector2::new(0.7, -0.7),
                Vector2::new(0.7, 0.7),
                Vector2::new(-0.7, 0.7),
            ],
            0.5,
        )));
        let mut masks = scene.wipeout_models_arc().as_ref().clone();
        assert_eq!(masks.len(), 1);
        masks[0].color = [0., 0., 0., 1.];
        masks[0].draw_depth = 0.1;
        pipeline.upload_wipeouts(&device, &queue, &masks);
        // Block children share a narrow sub-range of their insert's order.
        // The mask must hide earlier geometry without erasing later geometry.
        let depths = [(1, [0.0998, 0.]), (2, [0.1002, 0.])].into_iter().collect();
        for kind in ["line", "circle", "arc", "ellipse"] {
            let wires: Vec<_> = [(-0.3, 1), (0.3, 2)]
                .into_iter()
                .map(|(x, id)| {
                    let mut wire = WireModel::solid(
                        id.to_string(),
                        Vec::new(),
                        [1.; 4],
                        false,
                    );
                    let center = [x as f64 - 0.1, 0., 0.5];
                    let axis_x = [1., 0., 0.];
                    let axis_y = [0., 1., 0.];
                    match kind {
                        "circle" => wire.tangent_geoms.push(TangentGeom::PlanarCircle {
                            center, axis_x, axis_y, radius: 0.1,
                        }),
                        "arc" => wire.tangent_geoms.push(TangentGeom::Arc {
                            center, axis_x, axis_y, radius: 0.1,
                            start_angle: -std::f64::consts::FRAC_PI_2,
                            end_angle: std::f64::consts::FRAC_PI_2,
                        }),
                        "ellipse" => wire.tangent_geoms.push(TangentGeom::PlanarEllipse {
                            center, major_axis: [0.1, 0., 0.], normal: [0., 0., 1.],
                            minor_axis_ratio: 0.6, start_param: 0., end_param: std::f64::consts::TAU,
                        }),
                        _ => wire.points = vec![[x, -0.65, 0.5], [x, 0.65, 0.5]],
                    }
                    wire
                })
                .collect();
            pipeline.gpu_circles = std::sync::Arc::new(pipeline.upload_circles(&device, &queue, &wires, &depths));
            pipeline.gpu_ellipses = std::sync::Arc::new(pipeline.upload_ellipses(&device, &queue, &wires, &depths));
            let bytes = pixels(&device, &queue, &mut pipeline, &uniforms, &wires, &depths);
            assert_eq!(color_at(&bytes, -0.3), [0; 3], "mask must cover earlier {kind}");
            assert_ne!(color_at(&bytes, 0.3), [0; 3], "mask erased later {kind}, packed={packed}");
        }
        assert!(block_on(validation.pop()).is_none(), "GPU validation failed");
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn perspective_draw_order_keeps_flat_lines_visible() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
    let mut failures = Vec::new();
    for packed in [false, true] {
        let mut limits = adapter.limits();
        if packed {
            limits.max_storage_buffers_per_shader_stage = 0;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .expect("GPU device");
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let camera = Camera {
                distance: 102.03651,
                fov_y: 0.47108996,
                rotation: glam::Quat::IDENTITY,
                target: glam::DVec3::ZERO,
                projection,
                ..Camera::default()
            };
            // Same 17-entity draw-order labels as the reported drawing. Its
            // missing line (45EB) occupies index 1 with depth -0.5794896.
            let depths = (0..17)
                .map(|i| {
                    (
                        i + 1,
                        [
                            (-800_000_000_i64 + 88_888_888 * (i + 1) as i64) as f32
                                / 1_073_741_824.,
                            0.,
                        ],
                    )
                })
                .collect();
            let wires: Vec<_> = (0..17)
                .map(|i| {
                    let x = (-0.8 + i as f32 * 0.1) * camera.ortho_size();
                    let h = camera.ortho_size() * 0.65;
                    WireModel::solid(
                        (i + 1).to_string(),
                        vec![[x, -h, 0.], [x, h, 0.]],
                        if i < 2 { [1., 0.3, 0.2, 1.] } else { [1.; 4] },
                        false,
                    )
                })
                .collect();
            let bytes = pixels(
                &device,
                &queue,
                &mut pipeline,
                &Uniforms::new(&camera, bounds, false),
                &wires,
                &depths,
            );
            if let Ok(path) = std::env::var("OCS_DEPTH_TEST_IMAGES") {
                let path = std::path::Path::new(&path);
                std::fs::create_dir_all(path).unwrap();
                image::save_buffer(
                    path.join(format!(
                        "{projection:?}-{}.png",
                        if packed { "packed" } else { "storage" }
                    )),
                    &bytes,
                    SIZE,
                    SIZE,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            for i in 0..17 {
                if color_at(&bytes, -0.8 + i as f32 * 0.1) == [0; 3] {
                    failures.push(format!("{projection:?}, packed={packed}, line {i}"));
                }
            }
        }
        assert!(
            block_on(validation.pop()).is_none(),
            "GPU validation failed"
        );
    }
    assert!(
        failures.is_empty(),
        "visible lines were clipped: {failures:?}"
    );
}

#[test]
fn all_draw_order_shaders_validate() {
    for source in [
        draw_order_shader!("wire.wgsl"),
        draw_order_shader!("wire_indexed.wgsl"),
        draw_order_shader!("block_wire.wgsl"),
        draw_order_shader!("block_wire_storage.wgsl"),
        draw_order_shader!("circle.wgsl"),
        draw_order_shader!("ellipse.wgsl"),
        draw_order_shader!("text.wgsl"),
        draw_order_shader!("block_text.wgsl"),
        draw_order_shader!("face3d.wgsl"),
        draw_order_shader!("block_face3d.wgsl"),
        draw_order_shader!("hatch.wgsl"),
        draw_order_shader!("hatch_texture.wgsl"),
        draw_order_shader!("image.wgsl"),
        draw_order_shader!("wipeout.wgsl"),
    ] {
        let module = naga::front::wgsl::parse_str(source).expect("composed shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("composed shader validates");
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn draw_order_preserves_clip_boundaries_sorting_and_mesh_occlusion() {
    use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    for packed in [false, true] {
        let mut limits = adapter.limits();
        if packed {
            limits.max_storage_buffers_per_shader_stage = 0;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .expect("GPU device");
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
        let mut uniforms = Uniforms::new(&Camera::default(), bounds, false);
        uniforms.view_rot = glam::Mat4::IDENTITY;
        uniforms.eye_high = [0.; 3];
        uniforms.eye_low = [0.; 3];
        let line = |id: u64, x: f32, z: f32, color: [f32; 4]| {
            WireModel::solid(
                id.to_string(),
                vec![[x, -0.65, z], [x, 0.65, z]],
                color,
                false,
            )
        };
        // Both bias signs, the paper's -2 order, and already-clipped geometry.
        let cases = [
            (0.00001, 2., true),
            (0.00001, -2., true),
            (0.99999, 2., true),
            (0.99999, -2., true),
            (-0.00001, -2., false),
            (1.00001, 2., false),
            (0., 2., true),
            (1., -2., true),
            (0.5, 0., true),
        ];
        let wires: Vec<_> = cases
            .iter()
            .enumerate()
            .map(|(i, (z, _, _))| line(i as u64 + 1, -0.8 + i as f32 * 0.2, *z, [1.; 4]))
            .collect();
        let depths = cases
            .iter()
            .enumerate()
            .map(|(i, (_, order, _))| (i as u64 + 1, [*order, 0.]))
            .collect();
        let bytes = pixels(&device, &queue, &mut pipeline, &uniforms, &wires, &depths);
        for (i, (z, order, visible)) in cases.iter().enumerate() {
            assert_eq!(
                color_at(&bytes, -0.8 + i as f32 * 0.2) != [0; 3],
                *visible,
                "packed={packed}, depth={z}, order={order}"
            );
        }

        // Coplanar draw order must work close to both planes, for negative
        // ranks too, regardless of the order in which the GPU sees the wires.
        for z in [0.0001, 0.5, 0.9999] {
            for (back, front) in [(-0.7, 0.7), (-2., -0.9), (0.3, 0.8)] {
                let depths = rustc_hash::FxHashMap::from_iter([(1, [back, 0.]), (2, [front, 0.])]);
                let mut wires = [
                    line(1, 0., z, [1., 0., 0., 1.]),
                    line(2, 0., z, [0., 1., 0., 1.]),
                ];
                for _ in 0..2 {
                    let bytes = pixels(&device, &queue, &mut pipeline, &uniforms, &wires, &depths);
                    let [red, green, _] = color_at(&bytes, 0.);
                    assert!(
                        green > 0 && red == 0,
                        "draw order reversed at z={z}, packed={packed}"
                    );
                    wires.reverse();
                }
            }
        }

        // A real triangle mesh still occludes a wire behind it. A wire in front
        // stays visible: neutral draw order must retain geometric depth.
        pipeline.upload_mesh_batch(
            &device,
            &queue,
            &[MeshLodSet::from_single(MeshModel {
                name: "100".into(),
                verts: vec![
                    [-0.6, -0.8, 0.5],
                    [0.6, -0.8, 0.5],
                    [0.6, 0.8, 0.5],
                    [-0.6, 0.8, 0.5],
                ],
                verts_low: Vec::new(),
                normals: vec![[0., 0., 1.]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                triangle_material_handles: Vec::new(),
                triangle_colors: Vec::new(),
                color: [0., 1., 0., 1.],
                selected: false,
            })],
        );
        let mesh_only = pixels(
            &device,
            &queue,
            &mut pipeline,
            &uniforms,
            &[],
            &Default::default(),
        );
        assert_ne!(
            color_at(&mesh_only, -0.3),
            [0; 3],
            "mesh must produce pixels"
        );
        let bytes = pixels(
            &device,
            &queue,
            &mut pipeline,
            &uniforms,
            &[
                line(1, -0.3, 0.6, [1., 0., 0., 1.]),
                line(2, 0.3, 0.4, [0., 0., 1., 1.]),
            ],
            &Default::default(),
        );
        assert_eq!(
            color_at(&bytes, -0.3),
            color_at(&mesh_only, -0.3),
            "mesh failed to occlude the rear wire, packed={packed}"
        );
        let [red, _, blue] = color_at(&bytes, 0.3);
        assert!(blue > red, "mesh hid the front wire, packed={packed}");
        assert!(
            block_on(validation.pop()).is_none(),
            "GPU validation failed"
        );
    }
}
