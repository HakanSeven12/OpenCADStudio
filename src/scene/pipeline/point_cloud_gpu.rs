// Point cloud GPU buffers — one instance buffer per placed cloud, drawn as
// screen-space square sprites (6 vertices per instance, no vertex buffer).
//
// Group 1 binding 0 — PointParams (sprite size in pixels, lighting mode and
// light source, the clip-plane count, the two light directions, then up to
// six live-section clip planes; 144 bytes).
//
// Buffers are keyed by `PlacedCloud::key`: a new render set re-uploads only
// the clouds whose points changed; the rest keep their buffers.

use crate::scene::model::point_cloud::{PointCloudSet, PointInstance};
use iced::wgpu;

pub fn instance_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
    const ATTRS: &[wgpu::VertexAttribute] = &[
        wgpu::VertexAttribute {
            offset: std::mem::offset_of!(PointInstance, pos) as u64,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: std::mem::offset_of!(PointInstance, pos_low) as u64,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: std::mem::offset_of!(PointInstance, color) as u64,
            shader_location: 2,
            format: wgpu::VertexFormat::Unorm8x4,
        },
        wgpu::VertexAttribute {
            offset: std::mem::offset_of!(PointInstance, normal) as u64,
            shader_location: 3,
            format: wgpu::VertexFormat::Snorm8x4,
        },
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<PointInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: ATTRS,
    }
}

pub fn params_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point_cloud.bgl1"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// A live section bounds what it keeps with at most six planes (a volume).
const MAX_CLIP_PLANES: usize = 6;
const PARAMS: usize = 12 + 4 * MAX_CLIP_PLANES;

struct CloudBuffer {
    key: u64,
    buffer: wgpu::Buffer,
    count: u32,
}

/// The resident point clouds of one viewport pipeline.
pub struct PointCloudGpu {
    clouds: Vec<CloudBuffer>,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    point_size: f32,
    params_written: [f32; PARAMS],
    /// The share of each cloud's points drawn (real-time density).
    fraction: f32,
}

impl PointCloudGpu {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point_cloud.params"),
            size: (PARAMS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point_cloud.bind_group1"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        Self {
            clouds: Vec::new(),
            params,
            bind_group,
            point_size: f32::NAN,
            params_written: [f32::NAN; PARAMS],
            fraction: 1.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.clouds.is_empty()
    }

    /// Make `set` resident, keeping the buffers of clouds it already had.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, set: &PointCloudSet) {
        let mut old = std::mem::take(&mut self.clouds);
        for cloud in set
            .clouds
            .iter()
            .filter(|cloud| !cloud.instances.is_empty())
        {
            if let Some(at) = old.iter().position(|resident| resident.key == cloud.key) {
                self.clouds.push(old.swap_remove(at));
                continue;
            }
            self.clouds.push(CloudBuffer {
                key: cloud.key,
                buffer: super::gpu_upload::upload_buffer(
                    device,
                    queue,
                    "point_cloud.instances",
                    &cloud.instances,
                    wgpu::BufferUsages::VERTEX,
                ),
                count: cloud.instances.len() as u32,
            });
        }
        self.point_size = set.point_size;
    }

    /// The view's lighting (`[mode, source, fraction, _, light x, y, z, _]`)
    /// with the sprite size and the eye-relative clip planes (points with
    /// `n·rel + d < 0` are left out), written when they change.
    pub fn set_view(&mut self, queue: &wgpu::Queue, light: [f32; 12], clip: &[[f32; 4]]) {
        self.fraction = light[2].clamp(0.0, 1.0);
        let mut params = [0.0; PARAMS];
        params[..12].copy_from_slice(&light);
        params[0] = self.point_size;
        params[1] = light[0];
        params[2] = light[1];
        let clip = &clip[..clip.len().min(MAX_CLIP_PLANES)];
        params[3] = clip.len() as f32;
        for (slot, plane) in params[12..].chunks_exact_mut(4).zip(clip) {
            slot.copy_from_slice(plane);
        }
        if params != self.params_written {
            self.params_written = params;
            queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
        }
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_bind_group(1, &self.bind_group, &[]);
        for cloud in &self.clouds {
            pass.set_vertex_buffer(0, cloud.buffer.slice(..));
            let count = ((cloud.count as f32 * self.fraction).ceil() as u32).clamp(1, cloud.count);
            pass.draw(0..6, 0..count);
        }
    }
}
