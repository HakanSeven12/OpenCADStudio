//! Logical live GPU allocation sizes. Handles, not draw ranges or Arc wrapper
//! addresses, identify shared resources. This excludes driver heaps, alignment,
//! command/staging memory, pipelines and resources owned by iced/ViewCube.

use iced::wgpu;
use rustc_hash::FxHashSet;

#[derive(Default)]
pub(super) struct ResourceCounter {
    buffers: FxHashSet<wgpu::Buffer>,
    textures: FxHashSet<wgpu::Texture>,
    block_arenas: FxHashSet<usize>,
}

impl ResourceCounter {
    pub(super) fn buffer(&mut self, buffer: &wgpu::Buffer) -> u64 {
        if self.buffers.insert(buffer.clone()) {
            buffer.size()
        } else {
            0
        }
    }

    pub(super) fn texture(&mut self, texture: &wgpu::Texture) -> u64 {
        if !self.textures.insert(texture.clone()) {
            return 0;
        }
        texture_bytes(
            texture.size(),
            texture.dimension(),
            texture.format(),
            texture.mip_level_count(),
            texture.sample_count(),
        )
    }

    pub(super) fn blocks(&mut self, blocks: &[super::wire_gpu::BlockWireGpu]) -> u64 {
        super::wire_gpu::block_arena_bytes(blocks, &mut self.block_arenas)
    }

    pub(super) fn wires(&mut self, wires: &[super::WireGpu]) -> u64 {
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

    pub(super) fn text(&mut self, text: &[super::text_gpu::TextGpu]) -> u64 {
        text.iter()
            .map(|text| self.buffer(&text.vertex_buffer))
            .sum()
    }

    pub(super) fn block_text(&mut self, text: &[super::text_gpu::BlockTextGpu]) -> u64 {
        text.iter()
            .map(|text| self.buffer(&text.vertex_buffer) + self.buffer(&text.instance_buffer))
            .sum()
    }

    pub(super) fn meshes(&mut self, chunks: &[super::mesh_gpu::MeshBatchChunk]) -> (u64, u64) {
        let mut buffers = 0;
        let mut materials = 0;
        for chunk in chunks {
            for buffer in [
                &chunk.vertex_buffer,
                &chunk.index_buffer,
                &chunk.transp_index_buffer,
                &chunk.wire_vertex_buffer,
                &chunk.edge_vertex_buffer,
                &chunk.instance_buffer,
            ] {
                buffers += self.buffer(buffer);
            }
            if let Some(memory) = &chunk.material_memory {
                materials += memory.gpu_bytes(self);
            }
        }
        (buffers, materials)
    }
}

fn texture_bytes(
    size: wgpu::Extent3d,
    dimension: wgpu::TextureDimension,
    format: wgpu::TextureFormat,
    mips: u32,
    samples: u32,
) -> u64 {
    let (block_w, block_h) = format.block_dimensions();
    // Depth24Plus has implementation-defined storage. Four bytes per texel is
    // a logical estimate, not the driver's physical allocation size.
    let block_bytes = u64::from(format.block_copy_size(None).unwrap_or(4));
    (0..mips)
        .map(|mip| {
            let width = (size.width >> mip).max(1).div_ceil(block_w);
            let height = (size.height >> mip).max(1).div_ceil(block_h);
            let depth = if dimension == wgpu::TextureDimension::D3 {
                (size.depth_or_array_layers >> mip).max(1)
            } else {
                size.depth_or_array_layers
            };
            u64::from(width)
                * u64::from(height)
                * u64::from(depth)
                * block_bytes
                * u64::from(samples)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_sizes_include_mips_samples_layers_and_compression() {
        let size = wgpu::Extent3d {
            width: 8,
            height: 4,
            depth_or_array_layers: 2,
        };
        assert_eq!(
            texture_bytes(
                size,
                wgpu::TextureDimension::D2,
                wgpu::TextureFormat::Rgba8Unorm,
                1,
                4
            ),
            1024
        );
        assert_eq!(
            texture_bytes(
                size,
                wgpu::TextureDimension::D2,
                wgpu::TextureFormat::Rgba8Unorm,
                3,
                1
            ),
            336
        );
        let size = wgpu::Extent3d {
            width: 7,
            height: 5,
            depth_or_array_layers: 1,
        };
        assert_eq!(
            texture_bytes(
                size,
                wgpu::TextureDimension::D2,
                wgpu::TextureFormat::Bc1RgbaUnorm,
                1,
                1
            ),
            32
        );
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn cloned_handles_count_once_and_new_snapshots_release_old_resources() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("GPU device");
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4096,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mut counter = ResourceCounter::default();
        assert_eq!(counter.buffer(&buffer), 4096);
        assert_eq!(counter.buffer(&buffer.clone()), 0);
        assert_eq!(counter.texture(&texture), 512);
        assert_eq!(
            counter.texture(texture.create_view(&Default::default()).texture()),
            0
        );
        drop(counter);
        let mut next_frame = ResourceCounter::default();
        assert_eq!(next_frame.buffer(&buffer), 4096);

        let mut pipeline =
            super::super::Pipeline::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
        let before = pipeline.collect_gpu_live(&mut ResourceCounter::default());
        let wire = super::super::WireGpu {
            instance_buffer: buffer.clone(),
            first_instance: 0,
            instance_count: 1,
            is_3d_mesh_edge: false,
            const_bind_group: None,
            const_buffer: None,
        };
        pipeline.gpu_wires = std::sync::Arc::new(vec![wire.clone()]);
        pipeline.gpu_preview_wires = vec![wire];
        let after = pipeline.collect_gpu_live(&mut ResourceCounter::default());
        assert_eq!(after.total() - before.total(), 4096);
        assert_eq!(after.wires - before.wires, 4096);
        assert_eq!(after.overlays, before.overlays);
        let mut shared = ResourceCounter::default();
        assert!(pipeline.mesh_default_material_memory.gpu_bytes(&mut shared) >= 7 * 4);
        assert_eq!(
            pipeline
                .mesh_default_material_memory
                .clone()
                .gpu_bytes(&mut shared),
            0
        );
        pipeline.gpu_wires = Default::default();
        pipeline.gpu_preview_wires.clear();
        assert_eq!(
            pipeline
                .collect_gpu_live(&mut ResourceCounter::default())
                .total(),
            before.total()
        );
    }
}
