//! GPU buffers for SDF text quads (Phase 2b of the text-shader initiative).
//!
//! Unlike images (one texture per entity), all glyphs share ONE atlas texture
//! and every glyph is a quad in a single instance-less vertex buffer, so the
//! whole layout draws in one call. Group 1 is just `{ atlas texture, sampler }`;
//! per-glyph colour and draw-order live in the vertices.
//!
//! Vertex positions use the same double-single relative-to-eye encoding as
//! wires/hatches/images so text stays precise at large drawing coordinates.

// Not yet driven by the render loop — that hook-up (Pipeline fields, per-frame
// upload, a render pass, and suppressing the old stroke text) is the final
// integration step, done with the app running to verify pixels.
#![allow(dead_code)]

use iced::wgpu;
use iced::wgpu::util::DeviceExt;

use crate::scene::text::glyph_quads::GlyphQuad;
use crate::scene::text::sdf_atlas::GlyphAtlas;

// ── Vertex ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextVertex {
    pub pos: [f32; 3],
    pub pos_low: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub draw_depth: f32,
} // 52 bytes, no padding holes (all f32)

impl TextVertex {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(TextVertex, pos) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(TextVertex, pos_low) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(TextVertex, uv) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(TextVertex, color) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(TextVertex, draw_depth) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlockTextInstance {
    pub translation: [f32; 3],
    pub translation_low: [f32; 3],
    pub draw_depth: f32,
    pub _pad: [f32; 3],
}

impl BlockTextInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockTextInstance, translation) as u64, shader_location: 5, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockTextInstance, translation_low) as u64, shader_location: 6, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: std::mem::offset_of!(BlockTextInstance, draw_depth) as u64, shader_location: 7, format: wgpu::VertexFormat::Float32 },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

pub struct BlockTextGpu {
    pub vertex_buffer: wgpu::Buffer,
    pub instance_buffer: wgpu::Buffer,
    pub vertex_count: u32,
    pub instance_count: u32,
}

/// Split an f64 into the double-single (high f32, low residual f32) pair the
/// shaders reconstruct relative-to-eye.
pub(crate) fn split_ds(v: f64) -> (f32, f32) {
    let high = v as f32;
    (high, (v - high as f64) as f32)
}

/// Append the two triangles (6 vertices) for each glyph quad of one text run
/// into `out`, placing the run-local quad corners into world space:
/// `world = corner * anno + origin`, then double-single split.
///
/// UV mapping (atlas row 0 = top; quad corners are BL, BR, TR, TL):
///   BL -> (u_min, v_max)  BR -> (u_max, v_max)
///   TR -> (u_max, v_min)  TL -> (u_min, v_min)
pub fn push_glyph_vertices(
    out: &mut Vec<TextVertex>,
    quads: &[GlyphQuad],
    origin: [f64; 3],
    anno: f64,
    color: [f32; 4],
    draw_depth: f32,
) {
    for q in quads {
        let mk = |ci: usize, uv: [f32; 2]| -> TextVertex {
            let c = q.corners[ci];
            let wx = c[0] as f64 * anno + origin[0];
            let wy = c[1] as f64 * anno + origin[1];
            let wz = origin[2];
            let (xh, xl) = split_ds(wx);
            let (yh, yl) = split_ds(wy);
            let (zh, zl) = split_ds(wz);
            TextVertex {
                pos: [xh, yh, zh],
                pos_low: [xl, yl, zl],
                uv,
                color,
                draw_depth,
            }
        };
        let bl = [q.uv_min[0], q.uv_max[1]];
        let br = [q.uv_max[0], q.uv_max[1]];
        let tr = [q.uv_max[0], q.uv_min[1]];
        let tl = [q.uv_min[0], q.uv_min[1]];
        out.push(mk(0, bl));
        out.push(mk(1, br));
        out.push(mk(2, tr));
        out.push(mk(0, bl));
        out.push(mk(2, tr));
        out.push(mk(3, tl));
    }
}

/// Slide every glyph vertex by a world-space delta, re-splitting the
/// double-single position. Lets a grip drag move already-shaped text by
/// translating the drag-start glyphs each frame instead of re-tessellating
/// (re-shaping) the run on every cursor move (issue #316).
pub fn translate_verts(verts: &[TextVertex], delta: [f64; 3]) -> Vec<TextVertex> {
    verts
        .iter()
        .map(|v| {
            let wx = v.pos[0] as f64 + v.pos_low[0] as f64 + delta[0];
            let wy = v.pos[1] as f64 + v.pos_low[1] as f64 + delta[1];
            let wz = v.pos[2] as f64 + v.pos_low[2] as f64 + delta[2];
            let (xh, xl) = split_ds(wx);
            let (yh, yl) = split_ds(wy);
            let (zh, zl) = split_ds(wz);
            TextVertex {
                pos: [xh, yh, zh],
                pos_low: [xl, yl, zl],
                ..*v
            }
        })
        .collect()
}

// ── Atlas texture ───────────────────────────────────────────────────────────

/// The shared glyph atlas on the GPU: a single-channel (R8) SDF texture plus
/// its sampler and bind group.
pub struct TextAtlasGpu {
    pub bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    _sampler: wgpu::Sampler,
}

impl TextAtlasGpu {
    /// Bind-group layout for group 1: `{ R8 atlas texture, linear sampler }`.
    pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text.atlas.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    /// Upload the atlas texels (R8Unorm) and build the bind group.
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &GlyphAtlas,
        bgl1: &wgpu::BindGroupLayout,
    ) -> Self {
        let (w, h) = (atlas.width(), atlas.height());
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text.atlas.texture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            atlas.data(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w), // R8: 1 byte per texel
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text.atlas.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text.atlas.bind_group"),
            layout: bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            bind_group,
            _texture: texture,
            _sampler: sampler,
        }
    }
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

/// Build the base and highlight text pipelines. The base mirrors the image
/// pipeline (`LessEqual` depth with write); the highlight variant always draws
/// without changing depth, matching the selected-wire xray overlay.
/// `frame_bgl` is group 0 (shared uniforms), `atlas_bgl` is group 1 (the atlas).
pub fn create_pipelines(
    device: &wgpu::Device,
    frame_bgl: &wgpu::BindGroupLayout,
    atlas_bgl: &wgpu::BindGroupLayout,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
    content_stencil: &wgpu::StencilState,
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("text.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/text.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text.pipeline.layout"),
        bind_group_layouts: &[frame_bgl, atlas_bgl].map(Some),
        immediate_size: 0,
    });
    let create = |label, depth_write_enabled, depth_compare| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TextVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled,
                depth_compare,
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        })
    };
    let block_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("block_text.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/block_text.wgsl").into()),
    });
    let create_block = |label, depth_write_enabled, depth_compare| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &block_shader,
                entry_point: Some("vs_main"),
                buffers: &[TextVertex::layout(), BlockTextInstance::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &block_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled,
                depth_compare,
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        })
    };
    (
        create(
            "text.pipeline",
            Some(true),
            Some(wgpu::CompareFunction::LessEqual),
        ),
        create(
            "text.highlight.pipeline",
            Some(false),
            Some(wgpu::CompareFunction::Always),
        ),
        create_block(
            "block_text.pipeline",
            Some(true),
            Some(wgpu::CompareFunction::LessEqual),
        ),
        create_block(
            "block_text.highlight.pipeline",
            Some(false),
            Some(wgpu::CompareFunction::Always),
        ),
    )
}

pub fn upload_block_vertices(
    device: &wgpu::Device,
    wires: &[crate::scene::model::wire_model::WireModel],
    depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Vec<BlockTextGpu> {
    let refs: Vec<&crate::scene::model::wire_model::WireModel> = wires.iter().collect();
    upload_block_vertex_refs(device, &refs, depth_map, None)
}

pub fn upload_block_vertex_refs(
    device: &wgpu::Device,
    wires: &[&crate::scene::model::wire_model::WireModel],
    depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    tint: Option<[f32; 4]>,
) -> Vec<BlockTextGpu> {
    let mut slots = rustc_hash::FxHashMap::default();
    let mut groups: Vec<Vec<&crate::scene::model::wire_model::WireModel>> = Vec::new();
    for &wire in wires {
        if tint.is_none() && !wire.display_visible {
            continue;
        }
        let Some(instance) = wire.render_instance else {
            continue;
        };
        if wire.text_verts.is_empty() {
            continue;
        }
        let slot = *slots.entry(instance.source_id).or_insert_with(|| {
            let slot = groups.len();
            groups.push(Vec::new());
            slot
        });
        groups[slot].push(wire);
    }
    let mut out = Vec::new();
    for group in groups {
        let Some(&source) = group.first() else {
            continue;
        };
        let Some(base) = source.render_instance else {
            continue;
        };
        let tinted;
        let vertices = if let Some(tint) = tint {
            tinted = source
                .text_verts
                .iter()
                .map(|vertex| TextVertex {
                    color: [tint[0], tint[1], tint[2], vertex.color[3]],
                    ..*vertex
                })
                .collect::<Vec<_>>();
            tinted.as_slice()
        } else {
            source.text_verts.as_slice()
        };
        let Some(vertex_buffer) = upload_vertices(device, vertices) else {
            continue;
        };
        let instances: Vec<BlockTextInstance> = group
            .iter()
            .filter_map(|wire| {
                let instance = wire.render_instance?;
                let delta = [
                    instance.translation[0] - base.translation[0],
                    instance.translation[1] - base.translation[1],
                    instance.translation[2] - base.translation[2],
                ];
                let high = delta.map(|value| value as f32);
                Some(BlockTextInstance {
                    translation: high,
                    translation_low: [
                        (delta[0] - high[0] as f64) as f32,
                        (delta[1] - high[1] as f64) as f32,
                        (delta[2] - high[2] as f64) as f32,
                    ],
                    draw_depth: super::wire_gpu::wire_draw_depth(wire, depth_map),
                    _pad: [0.0; 3],
                })
            })
            .collect();
        let max_instances = ((device.limits().max_buffer_size as usize / 10) * 9
            / std::mem::size_of::<BlockTextInstance>())
            .max(1);
        for chunk in instances.chunks(max_instances) {
            let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("block_text.instances"),
                contents: bytemuck::cast_slice(chunk),
                usage: wgpu::BufferUsages::VERTEX,
            });
            out.push(BlockTextGpu {
                vertex_buffer: vertex_buffer.clone(),
                instance_buffer,
                vertex_count: vertices.len() as u32,
                instance_count: chunk.len() as u32,
            });
        }
    }
    out
}

/// Upload a finished vertex list; `None` if empty.
pub fn upload_vertices(device: &wgpu::Device, verts: &[TextVertex]) -> Option<wgpu::Buffer> {
    if verts.is_empty() {
        return None;
    }
    Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("text.vbuf"),
        contents: bytemuck::cast_slice(verts),
        usage: wgpu::BufferUsages::VERTEX,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn recon(v: &TextVertex) -> [f64; 3] {
        [
            v.pos[0] as f64 + v.pos_low[0] as f64,
            v.pos[1] as f64 + v.pos_low[1] as f64,
            v.pos[2] as f64 + v.pos_low[2] as f64,
        ]
    }

    #[test]
    fn quad_places_corners_and_uv() {
        let q = GlyphQuad {
            corners: [[0.0, 0.0], [8.0, 0.0], [8.0, 9.0], [0.0, 9.0]],
            uv_min: [0.10, 0.20],
            uv_max: [0.30, 0.40],
        };
        let mut out = Vec::new();
        // Large origin exercises the double-single precision path.
        push_glyph_vertices(&mut out, &[q], [1_000_000.0, 2_000_000.0, 0.0], 1.0, [1.0; 4], 0.0);
        assert_eq!(out.len(), 6, "two triangles per glyph");

        // Vertex 0 = BL corner (0,0) -> world origin, uv = (u_min, v_max).
        let bl = recon(&out[0]);
        assert!((bl[0] - 1_000_000.0).abs() < 1e-2 && (bl[1] - 2_000_000.0).abs() < 1e-2);
        assert_eq!(out[0].uv, [0.10, 0.40]);

        // Vertex 2 = TR corner (8,9) -> origin + (8,9), uv = (u_max, v_min).
        let tr = recon(&out[2]);
        assert!((tr[0] - 1_000_008.0).abs() < 1e-2 && (tr[1] - 2_000_009.0).abs() < 1e-2);
        assert_eq!(out[2].uv, [0.30, 0.20]);
    }

    #[test]
    fn annotation_scale_scales_corner_offsets() {
        let q = GlyphQuad {
            corners: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
        };
        let mut out = Vec::new();
        push_glyph_vertices(&mut out, &[q], [0.0, 0.0, 0.0], 2.0, [1.0; 4], 0.0);
        // TR corner (10,10) at anno 2.0 -> (20, 20).
        let tr = recon(&out[2]);
        assert!((tr[0] - 20.0).abs() < 1e-4 && (tr[1] - 20.0).abs() < 1e-4);
    }

    #[test]
    fn empty_run_yields_no_vertices() {
        let mut out = Vec::new();
        push_glyph_vertices(&mut out, &[], [0.0, 0.0, 0.0], 1.0, [1.0; 4], 0.0);
        assert!(out.is_empty());
    }
}
