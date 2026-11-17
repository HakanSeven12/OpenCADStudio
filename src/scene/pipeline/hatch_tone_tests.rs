//! A pattern hatch whose lines come closer than 2 px on screen is drawn as an
//! even tone covering the share of the area its lines cover, in both the
//! storage-buffer and the texture-packed hatch shaders.
use super::*;
use crate::scene::model::hatch_model::{HatchPattern, PatFamily};
use crate::scene::view::camera::Camera;
use iced::futures::executor::block_on;

const SIZE: u32 = 256;

/// A 100 x 100 square hatched with horizontal lines `spacing` apart.
fn lines_hatch(spacing: f32) -> HatchModel {
    HatchModel {
        entity: None,
        effective_layer: None,
        pattern_origin: None,
        render_instance: None,
        world_origin: [0.0, 0.0],
        boundary: std::sync::Arc::new(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: document::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Pattern(vec![PatFamily {
            angle_deg: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: spacing,
            dashes: Vec::new(),
        }]),
        name: "LINES".into(),
        color: [1.0, 1.0, 1.0, 1.0],
        aci: 7,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    }
}

/// Share of the pixels in the middle of the square that the hatch lights,
/// with the square filling the frame.
fn lit_share(device: &wgpu::Device, queue: &wgpu::Queue, hatch: HatchModel) -> f32 {
    let mut pipeline = Pipeline::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
    let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
    let mut camera = Camera::default();
    camera.fit_to_bounds(
        glam::Vec3::new(0.0, 0.0, 0.0),
        glam::Vec3::new(100.0, 100.0, 0.0),
        1.0,
    );
    let uniforms = Uniforms::new(&camera, bounds, false);
    pipeline.ensure_depth_texture(device, Size::new(SIZE, SIZE));
    pipeline.upload_uniforms(device, queue, &uniforms);
    pipeline.upload_hatches(device, queue, &[hatch]);
    pipeline.compute_hatch_lod(
        queue,
        camera.view_proj_rte(bounds),
        camera.eye(),
        SIZE,
        SIZE,
    );
    let size = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hatch tone"),
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
        label: Some("hatch tone readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
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
        bounds,
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
    let bytes = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    // The middle half of the frame lies well inside the square.
    let (from, to) = (SIZE as usize / 4, SIZE as usize * 3 / 4);
    let mut lit = 0usize;
    for y in from..to {
        for x in from..to {
            if bytes[(y * SIZE as usize + x) * 4] > 127 {
                lit += 1;
            }
        }
    }
    lit as f32 / ((to - from) * (to - from)) as f32
}

#[test]
#[ignore = "requires a GPU adapter"]
fn dense_pattern_lines_draw_a_tone_not_a_solid_fill() {
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
        // About 0.43 world units a pixel: lines 3 units apart are 7 px apart
        // and drawn as lines; 0.6 units apart they are 1.4 px apart.
        let sparse = lit_share(&device, &queue, lines_hatch(3.0));
        let dense = lit_share(&device, &queue, lines_hatch(0.6));
        assert!(
            sparse > 0.05 && sparse < 0.3,
            "lines 7 px apart light {sparse} of the area, packed={packed}"
        );
        assert!(
            dense > 0.45 && dense < 0.9,
            "1-px lines 1.4 px apart light {dense} of the area, packed={packed}"
        );
        assert!(
            block_on(validation.pop()).is_none(),
            "GPU validation failed"
        );
    }
}
