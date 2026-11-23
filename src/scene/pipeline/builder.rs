use super::*;

impl Pipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // ── Shared frame uniform buffer (view_proj etc.) ───────────────────
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer.uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let selection_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer.selection_uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout 0 — shared by wire and hatch pipelines.
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer.frame_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow.frame_bgl"),
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
        });

        let placeholder_texture = |label: &'static str, rgba: [u8; 4]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            texture
        };
        let background_texture = placeholder_texture("background.placeholder", [0, 0, 0, 255]);
        let environment_texture =
            placeholder_texture("environment.placeholder", [128, 128, 128, 255]);
        let background_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("background.sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let shadow_fallback_texture = create_shadow_texture(device, 1);
        let shadow_fallback_view =
            shadow_fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let background_view =
            background_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let environment_view =
            environment_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let frame_views = FrameViews {
            background: &background_view,
            environment: &environment_view,
            background_sampler: &background_sampler,
            shadow: &shadow_fallback_view,
            shadow_sampler: &shadow_sampler,
        };
        let uniform_bind_group =
            frame_bind_group(device, &frame_bgl, &uniform_buffer, &frame_views);
        let selection_uniform_bind_group =
            frame_bind_group(device, &frame_bgl, &selection_uniform_buffer, &frame_views);
        let shadow_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow.frame_bind_group"),
            layout: &shadow_frame_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // ── Wire pipeline ──────────────────────────────────────────────────
        // Select the renderer tier from device limits.
        let device_caps = DeviceCapabilities::detect(device);
        #[cfg(not(target_arch = "wasm32"))]
        let force_compat_renderer = crate::cli::gui_config().compat_renderer;
        #[cfg(target_arch = "wasm32")]
        let force_compat_renderer = false;
        let wire_mode = wire_gpu::WirePipelineMode::select(device_caps, force_compat_renderer);
        let wire_const_bgl = wire_mode
            .uses_storage()
            .then(|| wire_gpu::WireConst::bind_group_layout(device));
        let mut wire_bgls: Vec<Option<&wgpu::BindGroupLayout>> = vec![Some(&frame_bgl)];
        if let Some(bgl) = &wire_const_bgl {
            wire_bgls.push(Some(bgl));
        }
        let wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wire.pipeline_layout"),
            bind_group_layouts: &wire_bgls,
            immediate_size: 0,
        });
        let block_wire_const_bgl =
            wire_gpu::block_const_bind_group_layout(device, wire_mode.uses_storage());
        let block_wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("block_wire.pipeline_layout"),
            bind_group_layouts: &[Some(&frame_bgl), Some(&block_wire_const_bgl)],
            immediate_size: 0,
        });

        let depth_tex = create_depth_texture(device, Size::new(1, 1));
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let wire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wire.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(match wire_mode {
                wire_gpu::WirePipelineMode::IndexedStorage => {
                    draw_order_shader!("wire_indexed.wgsl")
                }
                wire_gpu::WirePipelineMode::Packed => draw_order_shader!("wire.wgsl"),
            })),
        });
        let block_wire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("block_wire.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                if wire_mode.uses_storage() {
                    draw_order_shader!("block_wire_storage.wgsl")
                } else {
                    draw_order_shader!("block_wire.wgsl")
                },
            )),
        });

        // Stencil test shared by every paper content pipeline: draw only where
        // the stencil equals the bound reference. Non-clipped content binds
        // reference 0 (matching the frame's stencil, cleared to 0); a clip
        // region's content binds reference 1 after its boundary has been stamped
        // into the stencil by the clip-mask pipeline. In Model space nothing
        // masks the stencil, so it stays 0 and reference 0 always passes.
        let content_stencil_face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        let content_stencil = wgpu::StencilState {
            front: content_stencil_face,
            back: content_stencil_face,
            read_mask: 0xff,
            write_mask: 0x00,
        };

        let background_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("background.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/background.wgsl"
            ))),
        });
        let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background.pipeline_layout"),
            bind_group_layouts: &[Some(&frame_bgl)],
            immediate_size: 0,
        });
        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background.pipeline"),
            layout: Some(&background_layout),
            vertex: wgpu::VertexState {
                module: &background_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &background_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_mode.layout()],
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
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
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

        // ── Clip-mask pipeline ─────────────────────────────────────────────
        // Stamps a viewport / XCLIP boundary polygon into the stencil buffer:
        // no colour, no depth, stencil `Invert` (even-odd fill → interior marked
        // for any polygon, convex or not). The boundary is drawn as a triangle
        // fan in paper coordinates and transformed exactly like wires.
        let clip_mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clip_mask.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/clip_mask.wgsl"
            ))),
        });
        let clip_mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clip_mask.pipeline_layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let clip_mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clip_mask.pipeline"),
            layout: Some(&clip_mask_layout),
            vertex: wgpu::VertexState {
                module: &clip_mask_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Invert,
                    },
                    back: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Invert,
                    },
                    read_mask: 0xff,
                    write_mask: 0xff,
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &clip_mask_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // Black variant of `wire_pipeline` — same geometry/depth, black fragment
        // — for 3D mesh outline edges in filled modes (see the wire draw loop).
        let wire_black_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire.black.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_mode.layout()],
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
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
                entry_point: Some("fs_black"),
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

        // Selection overlay variant: renders selected wires on top of everything
        // (depth_compare=Always), without writing depth. Ensures selected entities
        // are always fully visible regardless of occluding geometry.
        let wire_xray_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire_xray.pipeline"),
            layout: Some(&wire_layout),
            vertex: wgpu::VertexState {
                module: &wire_shader,
                entry_point: Some("vs_main"),
                buffers: &[wire_mode.layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: content_stencil.clone(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wire_shader,
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
        let wire_preview_overlay_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("wire_preview_overlay.pipeline"),
                layout: Some(&wire_layout),
                vertex: wgpu::VertexState {
                    module: &wire_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wire_mode.layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &wire_shader,
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
        let block_wire_buffers: &[wgpu::VertexBufferLayout<'_>] = if wire_mode.uses_storage() {
            &[wire_gpu::BlockWireInstance::layout()]
        } else {
            &[
                wire_gpu::BlockWireVertex::layout(),
                wire_gpu::BlockWireInstance::layout(),
            ]
        };
        let make_block_wire_pipeline =
            |label: &'static str,
             fragment: &'static str,
             depth_write_enabled: bool,
             depth_compare: wgpu::CompareFunction| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&block_wire_layout),
                    vertex: wgpu::VertexState {
                        module: &block_wire_shader,
                        entry_point: Some("vs_main"),
                        // Storage mode has no geometry vertex buffer: the
                        // instances become slot 0 and the segments arrive through
                        // the bind group.
                        buffers: block_wire_buffers,
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth24PlusStencil8,
                        depth_write_enabled: Some(depth_write_enabled),
                        depth_compare: Some(depth_compare),
                        stencil: content_stencil.clone(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: msaa_samples(),
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &block_wire_shader,
                        entry_point: Some(fragment),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let block_wire_pipeline = make_block_wire_pipeline(
            "block_wire.pipeline",
            "fs_main",
            true,
            wgpu::CompareFunction::LessEqual,
        );
        let block_wire_black_pipeline = make_block_wire_pipeline(
            "block_wire.black.pipeline",
            "fs_black",
            true,
            wgpu::CompareFunction::LessEqual,
        );
        let block_wire_xray_pipeline = make_block_wire_pipeline(
            "block_wire.xray.pipeline",
            "fs_main",
            false,
            wgpu::CompareFunction::Always,
        );

        // ── Hatch pipeline ─────────────────────────────────────────────────
        // Binding 0 carries the fill plane; boundary and family data are
        // fragment-only.
        let hatch_entry = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: binding < 2,
                min_binding_size: None,
            },
            count: None,
        };
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vert_frag = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let wipeout_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wipeout.bgl1"),
            entries: &[
                hatch_entry(0, vert_frag),
                hatch_entry(1, frag),
                hatch_entry(2, frag),
            ],
        });

        let wipeout_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wipeout.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &wipeout_bgl1].map(Some),
            immediate_size: 0,
        });

        let wipeout_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wipeout.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(draw_order_shader!(
                "wipeout.wgsl"
            ))),
        });

        let wipeout_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wipeout.pipeline"),
            layout: Some(&wipeout_layout),
            vertex: wgpu::VertexState {
                module: &wipeout_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wipeout_gpu::HatchVertex::layout(),
                    wipeout_gpu::WipeoutPlacement::layout(),
                ],
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
                // The shader already applies draw order. A raster bias can
                // jump across a block's narrow range and erase its foreground
                // — block text clears its siblings' wipes by only 9-16 depth
                // quanta, so the pipeline must stay bias-free. Coincident ties
                // still resolve toward the mask, which draws later under
                // LessEqual. WIPEOUT_DEPTH_BIAS_QUANTA is the budget the
                // block-text regression asserts against if a bias ever
                // returns.
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &wipeout_shader,
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

        // ── Hatch renderer ─────────────────────────────────────────────────
        // The façade owns capability selection plus both backend lifecycles.
        let hatch_gpu = hatch_gpu::HatchGpu::new(
            device,
            format,
            &frame_bgl,
            &content_stencil,
            device_caps,
            force_compat_renderer,
        );
        let hatch_translucent_gpu = hatch_gpu::HatchGpu::new(
            device,
            format,
            &frame_bgl,
            &content_stencil,
            device_caps,
            force_compat_renderer,
        );
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!(
                "renderer pipelines: wire={} hatch={} mesh=vertex (storage buffers/stage: {})",
                if wire_mode.uses_storage() {
                    "storage"
                } else {
                    "packed"
                },
                hatch_gpu.backend_name(),
                device.limits().max_storage_buffers_per_shader_stage
            );
        }
        #[cfg(target_arch = "wasm32")]
        log::info!(
            "renderer pipelines: wire={} hatch={} mesh={} (storage buffers/stage: {})",
            if wire_mode.uses_storage() {
                "storage"
            } else {
                "packed"
            },
            hatch_gpu.backend_name(),
            "vertex",
            device.limits().max_storage_buffers_per_shader_stage
        );

        // ── Mesh pipeline ──────────────────────────────────────────────────
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/mesh.wgsl"
            ))),
        });
        let mesh_material_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh.material.bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 12,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 14,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 16,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let (mesh_default_material_bind_group, mesh_default_material_memory) =
            mesh_gpu::create_material_bind_group(device, queue, &mesh_material_bgl, None);

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &mesh_material_bgl].map(Some),
            immediate_size: 0,
        });

        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/shadow.wgsl"
            ))),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow.pipeline_layout"),
            bind_group_layouts: &[&shadow_frame_bgl, &mesh_material_bgl].map(Some),
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow.pipeline"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    mesh_gpu::MeshVertex::layout(),
                    mesh_gpu::MeshInstanceGpu::layout(),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let shadow_plain_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow.plain.pipeline"),
                layout: Some(&shadow_layout),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        mesh_gpu::MeshPlainVertex::layout(),
                        mesh_gpu::MeshInstanceGpu::layout(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh.pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    mesh_gpu::MeshVertex::layout(),
                    mesh_gpu::MeshInstanceGpu::layout(),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None, // two-sided: ACIS import winding is unreliable
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
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
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
        let mesh_plain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh.plain.pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main_plain"),
                buffers: &[
                    mesh_gpu::MeshPlainVertex::layout(),
                    mesh_gpu::MeshInstanceGpu::layout(),
                ],
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
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
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

        // Transparent variant — identical to `mesh_pipeline` but with depth
        // writes disabled. Non-opaque solids are drawn after the opaque fills
        // with this pipeline so they blend over the geometry behind them (which
        // has already written depth) instead of erasing it.
        let mesh_transparent_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh.transparent.pipeline"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        mesh_gpu::MeshVertex::layout(),
                        mesh_gpu::MeshInstanceGpu::layout(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24PlusStencil8,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: content_stencil.clone(),
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_shader,
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
        let mesh_plain_transparent_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh.plain.transparent.pipeline"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_main_plain"),
                    buffers: &[
                        mesh_gpu::MeshPlainVertex::layout(),
                        mesh_gpu::MeshInstanceGpu::layout(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24PlusStencil8,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: content_stencil.clone(),
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_shader,
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

        // Highlight variants reuse the resident mesh buffers and differ only in
        // their fixed fragment tint. No selected mesh geometry is re-uploaded.
        let make_mesh_highlight_pipeline =
            |label: &'static str,
             vertex_entry: &'static str,
             vertex_layout: wgpu::VertexBufferLayout<'static>,
             fragment_entry: &'static str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&mesh_layout),
                    vertex: wgpu::VertexState {
                        module: &mesh_shader,
                        entry_point: Some(vertex_entry),
                        buffers: &[vertex_layout, mesh_gpu::MeshInstanceGpu::layout()],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth24PlusStencil8,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: content_stencil.clone(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: msaa_samples(),
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &mesh_shader,
                        entry_point: Some(fragment_entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let mesh_selected_pipeline = make_mesh_highlight_pipeline(
            "mesh.highlight.selected.pipeline",
            "vs_main",
            mesh_gpu::MeshVertex::layout(),
            "fs_highlight_selected",
        );
        let mesh_plain_selected_pipeline = make_mesh_highlight_pipeline(
            "mesh.plain.highlight.selected.pipeline",
            "vs_main_plain",
            mesh_gpu::MeshPlainVertex::layout(),
            "fs_highlight_selected",
        );
        let mesh_hover_pipeline = make_mesh_highlight_pipeline(
            "mesh.highlight.hover.pipeline",
            "vs_main",
            mesh_gpu::MeshVertex::layout(),
            "fs_highlight_hover",
        );
        let mesh_plain_hover_pipeline = make_mesh_highlight_pipeline(
            "mesh.plain.highlight.hover.pipeline",
            "vs_main_plain",
            mesh_gpu::MeshPlainVertex::layout(),
            "fs_highlight_hover",
        );

        // Wireframe variant — same shader / vertex layout / depth state,
        // only the input topology changes (LineList) and back-face
        // culling drops out (each triangle edge is shared between two
        // faces, one of which would otherwise hide the edge).
        // Edge/wireframe pipeline (LineList). `fs_edge` outputs the flat entity
        // colour — no lighting — for the lines-only modes. A `fs_edge_black`
        // twin (below) forces black for the edge overlay in filled modes.
        let make_edge_pipeline =
            |label: &'static str,
             vs: &'static str,
             fs: &'static str,
             vertex_layout: wgpu::VertexBufferLayout<'static>,
             depth_compare: wgpu::CompareFunction| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&mesh_layout),
                    vertex: wgpu::VertexState {
                        module: &mesh_shader,
                        entry_point: Some(vs),
                        buffers: &[vertex_layout, mesh_gpu::MeshInstanceGpu::layout()],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::LineList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth24PlusStencil8,
                        // The occluded pass reads the depth the visible
                        // edges wrote and leaves it as it is.
                        depth_write_enabled: Some(
                            depth_compare == wgpu::CompareFunction::LessEqual,
                        ),
                        depth_compare: Some(depth_compare),
                        stencil: content_stencil.clone(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: msaa_samples(),
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &mesh_shader,
                        entry_point: Some(fs),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let mesh_wireframe_pipeline = make_edge_pipeline(
            "mesh.wireframe.pipeline",
            "vs_edge",
            "fs_edge",
            mesh_gpu::MeshVertex::edge_layout(),
            wgpu::CompareFunction::LessEqual,
        );
        let mesh_edge_black_pipeline = make_edge_pipeline(
            "mesh.edge_black.pipeline",
            "vs_edge",
            "fs_edge_black",
            mesh_gpu::MeshVertex::edge_layout(),
            wgpu::CompareFunction::LessEqual,
        );
        let mesh_occluded_pipeline = make_edge_pipeline(
            "mesh.occluded.pipeline",
            "vs_edge_occluded",
            "fs_edge_occluded",
            mesh_gpu::MeshVertex::edge_layout(),
            wgpu::CompareFunction::Greater,
        );
        let silhouette_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh.silhouette.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/silhouette.wgsl"
            ))),
        });
        let silhouette_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh.silhouette.layout"),
            bind_group_layouts: &[Some(&frame_bgl)],
            immediate_size: 0,
        });
        let make_silhouette_pipeline = |label: &'static str, fragment: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&silhouette_layout),
                vertex: wgpu::VertexState {
                    module: &silhouette_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        mesh_gpu::SilhouetteVertex::layout(),
                        mesh_gpu::SilhouetteInstance::layout(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24PlusStencil8,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: content_stencil.clone(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &silhouette_shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let silhouette_pipeline = make_silhouette_pipeline("mesh.silhouette.pipeline", "fs_main");
        let silhouette_black_pipeline =
            make_silhouette_pipeline("mesh.silhouette_black.pipeline", "fs_black");

        // Depth-only variant — TriangleList, back-face culling stays on
        // (we only want front-facing fragments to write depth so wires
        // on the far side of the mesh stay hidden), `write_mask` zero
        // so no fragment ever reaches the colour buffer.
        let mesh_depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh.depth.pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    mesh_gpu::MeshVertex::layout(),
                    mesh_gpu::MeshInstanceGpu::layout(),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None, // two-sided: ACIS import winding is unreliable
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
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let mesh_plain_depth_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh.plain.depth.pipeline"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs_main_plain"),
                    buffers: &[
                        mesh_gpu::MeshPlainVertex::layout(),
                        mesh_gpu::MeshInstanceGpu::layout(),
                    ],
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
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            });

        // ── Face3D pipeline ────────────────────────────────────────────────
        let face3d_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("face3d.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(draw_order_shader!(
                "face3d.wgsl"
            ))),
        });
        let block_face3d_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("block_face3d.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(draw_order_shader!(
                "block_face3d.wgsl"
            ))),
        });

        let face3d_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("face3d.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl].map(Some),
            immediate_size: 0,
        });

        let face3d_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("face3d.pipeline"),
            layout: Some(&face3d_layout),
            vertex: wgpu::VertexState {
                module: &face3d_shader,
                entry_point: Some("vs_main"),
                buffers: &[face3d_gpu::Face3DVertex::layout()],
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
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &face3d_shader,
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

        // Depth-only variant — write_mask zero, no blend. The face3d
        // shader still runs but its colour output is discarded, so we
        // get a pure depth prepass for HiddenLine.
        let face3d_depth_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("face3d.depth.pipeline"),
                layout: Some(&face3d_layout),
                vertex: wgpu::VertexState {
                    module: &face3d_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[face3d_gpu::Face3DVertex::layout()],
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
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &face3d_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            });
        let make_block_face3d_pipeline = |label: &'static str, depth_only: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&face3d_layout),
                vertex: wgpu::VertexState {
                    module: &block_face3d_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        face3d_gpu::Face3DVertex::layout(),
                        face3d_gpu::Face3DInstance::layout(),
                    ],
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
                    count: msaa_samples(),
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &block_face3d_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: (!depth_only).then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: if depth_only {
                            wgpu::ColorWrites::empty()
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let block_face3d_pipeline = make_block_face3d_pipeline("block_face3d.pipeline", false);
        let block_face3d_depth_pipeline =
            make_block_face3d_pipeline("block_face3d.depth.pipeline", true);

        // ── Image pipeline ─────────────────────────────────────────────────
        let image_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image.bgl1"),
            entries: &[
                // binding 0: texture
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
                // binding 1: sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // binding 2: ImageParams uniform (read in vertex for the
                // draw-order z bias and in fragment for opacity).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &image_bgl1].map(Some),
            immediate_size: 0,
        });

        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(draw_order_shader!(
                "image.wgsl"
            ))),
        });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image.pipeline"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    image_gpu::ImageVertex::layout(),
                    image_gpu::ImageInstance::layout(),
                ],
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
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
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

        // ── Point cloud sprites ────────────────────────────────────────────
        let point_cloud_bgl1 = point_cloud_gpu::params_layout(device);
        let point_cloud_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point_cloud.pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &point_cloud_bgl1].map(Some),
            immediate_size: 0,
        });
        let point_cloud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point_cloud.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/shaders/point_cloud.wgsl"
            )))),
        });
        let point_cloud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point_cloud.pipeline"),
            layout: Some(&point_cloud_layout),
            vertex: wgpu::VertexState {
                module: &point_cloud_shader,
                entry_point: Some("vs_main"),
                buffers: &[point_cloud_gpu::instance_layout()],
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
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &point_cloud_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let gpu_point_clouds = point_cloud_gpu::PointCloudGpu::new(device, &point_cloud_bgl1);

        // ── Text (SDF glyph quads) ─────────────────────────────────────────
        let text_atlas_bgl = text_gpu::TextAtlasGpu::bind_group_layout(device);
        let (
            text_pipeline,
            text_highlight_pipeline,
            block_text_pipeline,
            block_text_highlight_pipeline,
        ) = text_gpu::create_pipelines(
            device,
            &frame_bgl,
            &text_atlas_bgl,
            format,
            msaa_samples(),
            &content_stencil,
        );

        let (circle_pipeline, circle_xray_pipeline) = circle_gpu::create_pipelines(
            device,
            &frame_bgl,
            format,
            msaa_samples(),
            &content_stencil,
        );

        let (ellipse_pipeline, ellipse_xray_pipeline) = ellipse_gpu::create_pipelines(
            device,
            &frame_bgl,
            format,
            msaa_samples(),
            &content_stencil,
        );

        let viewcube = ViewCubePipeline::new(device, queue, format);

        let init_size = Size::new(1, 1);
        let resolve_tex = create_resolve_texture(device, init_size, format);
        let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());
        // At 1 sample there is nothing to resolve: draw into the resolve texture.
        let msaa_view = if msaa_samples() > 1 {
            create_msaa_texture(device, init_size, format)
                .create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            resolve_tex.create_view(&wgpu::TextureViewDescriptor::default())
        };

        // ── Blit pipeline (resolve texture → surface target) ──────────────
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/blit.wgsl"
            ))),
        });

        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit.bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // UV crop uniform: [uv_offset_x, uv_offset_y, uv_scale_x, uv_scale_y]
        // padded to 16 bytes (std140 vec2 alignment). Defaulted to the
        // identity crop (offset 0, scale 1) for the common on-canvas case.
        let blit_uniform_buffer = gpu_upload::upload_buffer(
            device,
            queue,
            "blit.uniform_buffer",
            &[0.0f32, 0.0, 1.0, 1.0],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );

        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit.pipeline_layout"),
            bind_group_layouts: &[&blit_bgl].map(Some),
            immediate_size: 0,
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit.pipeline"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied-alpha blend: the geometry passes already
                    // wrote into a transparent MSAA target with standard
                    // `SrcAlpha / 1-SrcAlpha` blending, so AA-edge fragments
                    // sit as `(rgb * a, a)` in the resolve texture. Treating
                    // that as straight alpha during the surface blit would
                    // multiply by alpha a second time and darken thin lines
                    // / curves. `PREMULTIPLIED_ALPHA_BLENDING` uses `One` as
                    // the source colour factor and leaves the dst weighted
                    // by `1-SrcAlpha`, which is the correct compositing
                    // operator for already-premultiplied content.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit.sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit.bind_group"),
            layout: &blit_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: blit_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            background_pipeline,
            shadow_pipeline,
            shadow_plain_pipeline,
            wire_pipeline,
            block_wire_pipeline,
            clip_mask_pipeline,
            wire_black_pipeline,
            block_wire_black_pipeline,
            wire_xray_pipeline,
            wire_preview_overlay_pipeline,
            preview_overlay_direct: false,
            circle_pipeline,
            circle_xray_pipeline,
            ellipse_pipeline,
            ellipse_xray_pipeline,
            block_wire_xray_pipeline,
            wire_const_bgl,
            block_wire_const_bgl,
            block_wire_mode: wire_mode,
            gpu_error_epoch: gpu_errors_seen(),
            wipeout_pipeline,
            hatch_gpu,
            hatch_translucent_gpu,
            image_pipeline,
            point_cloud_pipeline,
            text_pipeline,
            block_text_pipeline,
            text_highlight_pipeline,
            block_text_highlight_pipeline,
            text_atlas_bgl,
            text_atlas_gpu: None,
            text_gpu: Vec::new(),
            block_text_gpu: Vec::new(),
            block_text_highlight_gpu: Vec::new(),
            text_highlight_gpu: Vec::new(),
            text_preview_gpu: Vec::new(),
            silhouette_chunks: Vec::new(),
            silhouette_source_key: (usize::MAX, usize::MAX, u64::MAX),
            silhouette_source_groups: Vec::new(),
            mesh_pipeline,
            mesh_plain_pipeline,
            mesh_transparent_pipeline,
            mesh_plain_transparent_pipeline,
            mesh_selected_pipeline,
            mesh_plain_selected_pipeline,
            mesh_hover_pipeline,
            mesh_plain_hover_pipeline,
            mesh_wireframe_pipeline,
            mesh_edge_black_pipeline,
            mesh_occluded_pipeline,
            silhouette_pipeline,
            silhouette_black_pipeline,
            mesh_depth_pipeline,
            mesh_plain_depth_pipeline,
            mesh_material_bgl,
            mesh_default_material_bind_group,
            mesh_default_material_memory,
            face3d_pipeline,
            block_face3d_pipeline,
            face3d_depth_pipeline,
            block_face3d_depth_pipeline,
            uniform_buffer,
            frame_bgl,
            uniform_bind_group,
            selection_uniform_buffer,
            selection_uniform_bind_group,
            shadow_uniform_bind_group,
            background_texture,
            environment_texture,
            background_sampler,
            background_source_id: 0,
            environment_source_id: 0,
            shadow_full: None,
            _shadow_fallback_texture: shadow_fallback_texture,
            shadow_fallback_view,
            shadow_sampler,
            shadow_enabled: false,
            wipeout_bgl1,
            image_bgl1,
            depth_texture_size: Size::new(1, 1),
            viewport: None,
            // (0, 0) forces the first `ensure_depth_texture` to allocate at the
            // real rounded size — the constructor textures above are placeholders.
            alloc_size: Size::new(0, 0),
            depth_view,
            msaa_view,
            resolve_view,
            wire_highlight_target: None,
            wire_highlight_dirty: false,
            blit_pipeline,
            blit_bind_group_layout: blit_bgl,
            blit_sampler,
            blit_bind_group,
            blit_uniform_buffer,
            surface_format: format,
            gpu_wires: std::sync::Arc::new(vec![]),
            gpu_circles: std::sync::Arc::new(vec![]),
            gpu_ellipses: std::sync::Arc::new(vec![]),
            gpu_block_wires: std::sync::Arc::new(vec![]),
            wire_arena: None,
            wire_arena_mesh: None,
            wire_arena_fallback: std::sync::Arc::new(Vec::new()),
            wire_arena_fallback_kind: None,
            wire_arena_fallback_handles: rustc_hash::FxHashSet::default(),
            wire_arena_id: u64::MAX,
            wire_arena_depth_generation: u64::MAX,
            dense_highlight_consts: Vec::new(),
            partition_contributors: wire_arena::ContributorSet::default(),
            analytic_slots: None,
            partition_depth_generation: u64::MAX,
            wire_cull_key: (u64::MAX, u64::MAX, 0, 0),
            hatch_lod_key: (usize::MAX, u64::MAX, 0, 0, false),
            wipeout_lod_key: (usize::MAX, u64::MAX, 0, 0, false),
            silhouette_key: (usize::MAX, u64::MAX, [u32::MAX; 3], false),
            clip_boundary: None,
            gpu_selected_wires: vec![],
            selected_highlight_gpu_cache: None,
            highlight_wire_cache: rustc_hash::FxHashMap::default(),
            highlight_wire_cache_key: None,
            highlight_wire_cache_bytes: 0,
            highlight_wire_chunks: rustc_hash::FxHashMap::default(),
            highlight_wire_chunk_bytes: 0,
            gpu_selected_circles: vec![],
            gpu_selected_ellipses: vec![],
            gpu_selected_block_wires: vec![],
            gpu_preview_wires: vec![],
            gpu_preview_block_wires: vec![],
            gpu_preview_circles: vec![],
            gpu_preview_ellipses: vec![],
            gpu_wipeouts: vec![],
            wipeout_skip_flags: vec![],
            gpu_images: vec![],
            gpu_point_clouds,
            gpu_mesh_batch: Default::default(),
            gpu_mesh_dynamic: vec![],
            mesh_disabled_chunks: rustc_hash::FxHashSet::default(),
            mesh_dynamic_handles: rustc_hash::FxHashSet::default(),
            cached_mesh_content_id: u64::MAX,
            mesh_ranges_by_handle: rustc_hash::FxHashMap::default(),
            mesh_lod_view: None,
            mesh_lod_levels: rustc_hash::FxHashMap::default(),
            mesh_lod_draws: rustc_hash::FxHashMap::default(),
            mesh_highlight_draws: vec![],
            cached_highlight_key: (u64::MAX, u64::MAX),
            cached_mesh_hover_generation: None,
            gpu_face3d_fill: None,
            gpu_face3d_edges: vec![],
            viewcube,
            cached_hatch_source: None,
            cached_preview_hatch_source: None,
            cached_wipeout_source: None,
            cached_image_source: None,
            cached_point_cloud_source: None,
            cached_text_source: None,
            cached_annotation_highlight_source: None,
            cached_selected_annotation_source: None,
            cached_mesh_source: None,
            cached_face3d_source: None,
            cached_face3d_depth_source: None,
            cached_fill_mode: false,
            cached_epoch: (u64::MAX, u64::MAX, u64::MAX),
            cached_wire_id: u64::MAX,
            cached_selection: (u64::MAX, u64::MAX),
            cached_hover_generation: None,
            hover_text_gpu_cache: None,
            cached_face3d_key: (u64::MAX, false, false, u64::MAX),
            cached_solid_visibility: (u64::MAX, [u32::MAX; 3], u64::MAX),
            wire_handle_index: std::sync::Arc::new(rustc_hash::FxHashMap::default()),
            render_sig: u64::MAX,
            skip_geometry: false,
            skip_hatch_frame: false,
            skip_background: false,
            occluded_edges: false,
            slot_id: u64::MAX,
            slot_owner: std::sync::Weak::new(),
        }
    }

    /// Upload block placements while reusing unchanged geometry.
    pub fn upload_block_wires(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[wire_gpu::BlockRef<'_>],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        block_geometry: &mut wire_gpu::BlockGeometryCache,
    ) -> Vec<BlockWireGpu> {
        BlockWireGpu::from_wires(
            device,
            queue,
            wires,
            depth_map,
            None,
            wire_gpu::BlockWireTarget {
                const_bgl: &self.block_wire_const_bgl,
                mode: self.block_wire_mode,
            },
            Some(block_geometry),
        )
    }

    /// Upload GPU-instanced analytical circles from a wire slice.
    pub fn upload_circles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) -> Vec<CircleGpu> {
        let mut instances: Vec<CircleInstance> = Vec::new();
        for wire in wires {
            if !wire.display_visible {
                continue;
            }
            let depth = wire_gpu::wire_draw_depth(wire, depth_map);
            if let Some(insts) = circle_gpu::extract_circle_instances(wire, depth) {
                instances.extend(insts);
            }
        }
        self.upload_circles_from_instances(device, queue, &instances)
    }

    /// Upload GPU-instanced analytical circles from pre-extracted instances.
    pub fn upload_circles_from_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[CircleInstance],
    ) -> Vec<CircleGpu> {
        if instances.is_empty() {
            vec![]
        } else {
            let mut sorted = instances.to_vec();
            {
                // A stable parallel sort: the same order, a quarter-million
                // instances sorted across the cores.
                #[cfg(not(target_arch = "wasm32"))]
                use rayon::prelude::*;
                #[cfg(not(target_arch = "wasm32"))]
                sorted.par_sort_by(|a, b| a.params[2].total_cmp(&b.params[2]));
                #[cfg(target_arch = "wasm32")]
                sorted.sort_by(|a, b| a.params[2].total_cmp(&b.params[2]));
            }
            vec![CircleGpu::from_instances(
                device,
                queue,
                "viewer.circles",
                &sorted,
            )]
        }
    }

    /// Upload GPU-instanced analytical ellipses from a wire slice.
    pub fn upload_ellipses(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[WireModel],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) -> Vec<EllipseGpu> {
        let mut instances: Vec<EllipseInstance> = Vec::new();
        for wire in wires {
            if !wire.display_visible {
                continue;
            }
            let depth = wire_gpu::wire_draw_depth(wire, depth_map);
            if let Some(insts) = ellipse_gpu::extract_ellipse_instances(wire, depth) {
                instances.extend(insts);
            }
        }
        self.upload_ellipses_from_instances(device, queue, &instances)
    }

    /// Upload GPU-instanced analytical ellipses from pre-extracted instances.
    pub fn upload_ellipses_from_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[EllipseInstance],
    ) -> Vec<EllipseGpu> {
        if instances.is_empty() {
            vec![]
        } else {
            let mut sorted = instances.to_vec();
            {
                #[cfg(not(target_arch = "wasm32"))]
                use rayon::prelude::*;
                #[cfg(not(target_arch = "wasm32"))]
                sorted.par_sort_by(|a, b| a.params[2].total_cmp(&b.params[2]));
                #[cfg(target_arch = "wasm32")]
                sorted.sort_by(|a, b| a.params[2].total_cmp(&b.params[2]));
            }
            vec![EllipseGpu::from_instances(
                device,
                queue,
                "viewer.ellipses",
                &sorted,
            )]
        }
    }

    /// `upload_block_wires`, also reporting where every placement landed.
    pub fn upload_block_wires_tracked(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wires: &[wire_gpu::BlockRef<'_>],
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
        block_geometry: &mut wire_gpu::BlockGeometryCache,
    ) -> (Vec<BlockWireGpu>, Option<wire_gpu::BlockTracking>) {
        BlockWireGpu::from_wires_tracked(
            device,
            queue,
            wires,
            depth_map,
            wire_gpu::BlockWireTarget {
                const_bgl: &self.block_wire_const_bgl,
                mode: self.block_wire_mode,
            },
            Some(block_geometry),
        )
    }

    /// `upload_circles_from_instances`, also reporting where each instance
    /// landed in the depth-sorted buffer, paired with the reference that owns it.
    pub fn upload_circles_tracked(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[CircleInstance],
        owners: &[document::Handle],
    ) -> (
        Vec<CircleGpu>,
        Vec<(document::Handle, analytic_slots::CurveSlot)>,
    ) {
        if instances.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let (sorted, position) = analytic_slots::sort_by_depth(instances, |c| c.params[2]);
        let gpu = CircleGpu::from_instances(device, queue, "viewer.circles", &sorted);
        let slots = owners
            .iter()
            .zip(position)
            .enumerate()
            .map(|(original, (owner, index))| {
                (
                    *owner,
                    analytic_slots::CurveSlot {
                        index,
                        depth_bits: instances[original].params[2].to_bits(),
                    },
                )
            })
            .collect();
        (vec![gpu], slots)
    }

    /// The ellipse counterpart of `upload_circles_tracked`.
    pub fn upload_ellipses_tracked(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[EllipseInstance],
        owners: &[document::Handle],
    ) -> (
        Vec<EllipseGpu>,
        Vec<(document::Handle, analytic_slots::CurveSlot)>,
    ) {
        if instances.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let (sorted, position) = analytic_slots::sort_by_depth(instances, |e| e.params[2]);
        let gpu = EllipseGpu::from_instances(device, queue, "viewer.ellipses", &sorted);
        let slots = owners
            .iter()
            .zip(position)
            .enumerate()
            .map(|(original, (owner, index))| {
                (
                    *owner,
                    analytic_slots::CurveSlot {
                        index,
                        depth_bits: instances[original].params[2].to_bits(),
                    },
                )
            })
            .collect();
        (vec![gpu], slots)
    }
}
