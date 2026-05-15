// Mesh GPU buffers — TriangleList rendering for solid objects.
//
// Vertex layout (40 bytes):
//   position   [f32; 3]   offset  0   12 B
//   normal     [f32; 3]   offset 12   12 B
//   color      [f32; 4]   offset 24   16 B
//                                ------
//                                 40 B / vertex

use crate::scene::mesh_model::{MeshLodSet, MeshModel};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

// ── Vertex layout ─────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

impl MeshVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

// ── GPU handle ────────────────────────────────────────────────────────────

pub struct MeshGpu {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
}

/// GPU-side bundle of MeshLodSet — one MeshGpu per available LOD plus
/// the world-XY AABB needed to pick a level per frame.
pub struct MeshLodGpu {
    pub lods: Vec<MeshGpu>,
    pub world_aabb: [f32; 4],
}

impl MeshLodGpu {
    pub fn new(device: &wgpu::Device, set: &MeshLodSet) -> Self {
        Self {
            lods: set
                .lods
                .iter()
                .filter(|m| !m.indices.is_empty())
                .map(|m| MeshGpu::new(device, m))
                .collect(),
            world_aabb: set.world_aabb,
        }
    }
}

impl MeshGpu {
    pub fn new(device: &wgpu::Device, mesh: &MeshModel) -> Self {
        let has_normals = mesh.normals.len() == mesh.verts.len();
        let vertices: Vec<MeshVertex> = mesh
            .verts
            .iter()
            .enumerate()
            .map(|(i, &pos)| {
                let normal = if has_normals {
                    mesh.normals[i]
                } else {
                    [0.0, 1.0, 0.0]
                };
                MeshVertex {
                    position: pos,
                    normal,
                    color: mesh.color,
                }
            })
            .collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("mesh.vbuf.{}", mesh.name)),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("mesh.ibuf.{}", mesh.name)),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        }
    }
}
