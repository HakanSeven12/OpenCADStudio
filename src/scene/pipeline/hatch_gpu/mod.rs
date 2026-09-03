//! Capability-selected hatch renderer.
//!
//! `Pipeline` sees one [`HatchGpu`]. This module owns the renderer selection,
//! render pipeline, resident/preview uploads, visibility updates, and draw
//! dispatch. The storage and texture transports remain private implementation
//! details because their bind groups and upload layouts are fundamentally
//! different.

mod storage;
mod texture;

use super::device_capabilities::DeviceCapabilities;
use crate::scene::model::hatch_model::HatchModel;
use iced::wgpu;

use storage::StorageHatchBatch;
use texture::TextureHatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HatchBackendKind {
    Storage,
    Texture,
}

impl HatchBackendKind {
    fn select(capabilities: DeviceCapabilities, force_compatibility: bool) -> Self {
        if !force_compatibility && capabilities.supports_batched_hatch() {
            Self::Storage
        } else {
            Self::Texture
        }
    }
}

enum HatchBackend {
    Storage {
        resident: Vec<StorageHatchBatch>,
        preview: Vec<StorageHatchBatch>,
    },
    Texture {
        resident: Vec<TextureHatch>,
        preview: Vec<TextureHatch>,
    },
}

/// One hatch renderer whose transport is selected from the active device.
pub struct HatchGpu {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    backend: HatchBackend,
}

impl HatchGpu {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        frame_bind_group_layout: &wgpu::BindGroupLayout,
        content_stencil: &wgpu::StencilState,
        capabilities: DeviceCapabilities,
        force_compatibility: bool,
    ) -> Self {
        let backend_kind = HatchBackendKind::select(capabilities, force_compatibility);
        let uses_storage = backend_kind == HatchBackendKind::Storage;
        let bind_group_layout = if uses_storage {
            StorageHatchBatch::bind_group_layout(device)
        } else {
            TextureHatch::bind_group_layout(device)
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hatch.pipeline_layout"),
            bind_group_layouts: &[frame_bind_group_layout, &bind_group_layout].map(Some),
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(if uses_storage {
                "hatch.storage.shader"
            } else {
                "hatch.texture.shader"
            }),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(if uses_storage {
                include_str!("../../../shaders/hatch.wgsl")
            } else {
                include_str!("../../../shaders/hatch_texture.wgsl")
            })),
        });
        let vertex_layouts = if uses_storage {
            vec![storage::HatchVertex::layout(), storage::HatchPlacement::layout()]
        } else {
            vec![texture::vertex_layout(), texture::TextureHatchPlacement::layout()]
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if uses_storage {
                "hatch.storage.pipeline"
            } else {
                "hatch.texture.pipeline"
            }),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &vertex_layouts,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: super::MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let backend = match backend_kind {
            HatchBackendKind::Storage => HatchBackend::Storage {
                resident: Vec::new(),
                preview: Vec::new(),
            },
            HatchBackendKind::Texture => HatchBackend::Texture {
                resident: Vec::new(),
                preview: Vec::new(),
            },
        };
        Self {
            pipeline,
            bind_group_layout,
            backend,
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            HatchBackend::Storage { .. } => "storage",
            HatchBackend::Texture { .. } => "texture",
        }
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hatches: &[HatchModel]) {
        match &mut self.backend {
            HatchBackend::Storage { resident, .. } => {
                let renderable: Vec<HatchModel> = hatches
                    .iter()
                    .filter(|hatch| hatch.boundary.len() >= 3)
                    .cloned()
                    .collect();
                *resident = StorageHatchBatch::build(device, queue, &self.bind_group_layout, &renderable);
            }
            HatchBackend::Texture { resident, .. } => {
                let renderable: Vec<HatchModel> = hatches
                    .iter()
                    .filter(|hatch| hatch.boundary.len() >= 3)
                    .cloned()
                    .collect();
                *resident = TextureHatch::from_models(
                    device,
                    queue,
                    &renderable,
                    &self.bind_group_layout,
                );
            }
        }
    }

    pub fn upload_preview(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hatches: &[HatchModel],
    ) {
        match &mut self.backend {
            HatchBackend::Storage { preview, .. } => {
                let renderable: Vec<HatchModel> = hatches
                    .iter()
                    .filter(|hatch| hatch.boundary.len() >= 3)
                    .cloned()
                    .collect();
                *preview = StorageHatchBatch::build(device, queue, &self.bind_group_layout, &renderable);
            }
            HatchBackend::Texture { preview, .. } => {
                let renderable: Vec<HatchModel> = hatches
                    .iter()
                    .filter(|hatch| hatch.boundary.len() >= 3)
                    .cloned()
                    .collect();
                *preview = TextureHatch::from_models(
                    device,
                    queue,
                    &renderable,
                    &self.bind_group_layout,
                );
            }
        }
    }

    /// Refresh resident hatch visibility. Texture hatches retain their existing
    /// per-draw behavior and return zero because they have no visibility buffer.
    pub fn update_visibility(
        &mut self,
        queue: &wgpu::Queue,
        mut is_visible: impl FnMut([f32; 4]) -> bool,
    ) -> usize {
        let HatchBackend::Storage { resident, .. } = &mut self.backend
        else {
            return 0;
        };
        let mut updated = 0;
        for batch in resident {
            for index in 0..batch.unique_source_count {
                batch.source_visibility[index] = u32::from(is_visible(batch.source_aabbs[index]));
            }
            for index in batch.unique_source_count..batch.source_visibility.len() {
                batch.source_visibility[index] = 1;
            }
            for (index, aabb) in batch.instance_aabbs.iter().copied().enumerate() {
                batch.placements[index].visible = if index == 0 && batch.unique_source_count > 0 {
                    1
                } else {
                    u32::from(is_visible(aabb))
                };
            }
            batch.upload_visibility(queue);
            updated += batch.instance_aabbs.len() + batch.unique_source_count;
        }
        updated
    }

    pub fn draw<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        frame_bind_group: &'pass wgpu::BindGroup,
        stencil_reference: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_stencil_reference(stencil_reference);
        match &self.backend {
            HatchBackend::Storage { resident, preview } => {
                for batch in resident.iter().chain(preview) {
                    pass.set_bind_group(1, &batch.bind_group, &[]);
                    pass.set_vertex_buffer(0, batch.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, batch.placement_buffer.slice(..));
                    pass.set_index_buffer(batch.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    for (indices, instances) in &batch.draws {
                        pass.draw_indexed(indices.clone(), 0, instances.clone());
                    }
                }
            }
            HatchBackend::Texture { resident, preview } => {
                for hatch in resident.iter().chain(preview) {
                    pass.set_bind_group(1, &hatch.bind_group, &[]);
                    pass.set_vertex_buffer(0, hatch.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, hatch.placement_buffer.slice(..));
                    pass.set_index_buffer(
                        hatch.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..hatch.index_count, 0, 0..hatch.instance_count);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceCapabilities, HatchBackendKind};

    fn capabilities(storage_bindings: u32) -> DeviceCapabilities {
        DeviceCapabilities {
            max_storage_buffers_per_shader_stage: storage_bindings,
            max_inter_stage_shader_components: 31,
            max_vertex_attributes: 16,
        }
    }

    #[test]
    fn selects_backend_from_device_limits() {
        assert_eq!(
            HatchBackendKind::select(capabilities(4), false),
            HatchBackendKind::Storage
        );
        assert_eq!(
            HatchBackendKind::select(capabilities(3), false),
            HatchBackendKind::Texture
        );
    }

    #[test]
    fn compatibility_override_selects_texture_backend() {
        assert_eq!(
            HatchBackendKind::select(capabilities(8), true),
            HatchBackendKind::Texture
        );
    }
}
