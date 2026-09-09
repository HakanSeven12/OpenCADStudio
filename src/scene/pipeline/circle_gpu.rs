//! GPU-instanced analytical circle buffer & pipeline layout.
//!
//! Renders 2D/planar circles as analytical screen/plane-aligned quads with
//! screen-space derivative anti-aliasing. Each circle is a single instance
//! (6 vertices for a 2-triangle quad), eliminating CPU re-tessellation on zoom
//! and providing infinite visual smoothness.

use iced::wgpu;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CircleInstance {
    pub center_high: [f32; 3],
    pub start_width: f32,
    pub center_low: [f32; 3],
    pub radius: f32,
    pub axis_x: [f32; 3],
    pub start_angle: f32,
    pub axis_y: [f32; 3],
    pub end_angle: f32,
    pub color: [f32; 4],
    /// `[half_width, pattern_length, draw_depth, end_width]`
    pub params: [f32; 4],
    pub pat0: [f32; 4],
    pub pat1: [f32; 4],
}

impl CircleInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, center_high) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, center_low) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, axis_x) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, axis_y) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, color) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, params) as u64,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, pat0) as u64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(CircleInstance, pat1) as u64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CircleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

pub struct CircleGpu {
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
}

impl CircleGpu {
    pub fn from_instances(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        instances: &[CircleInstance],
    ) -> Self {
        let instance_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            label,
            instances,
            wgpu::BufferUsages::VERTEX,
        );
        Self {
            instance_buffer,
            instance_count: instances.len() as u32,
        }
    }
}

/// Build the base and xray circle pipelines.
pub fn create_pipelines(
    device: &wgpu::Device,
    frame_bgl: &wgpu::BindGroupLayout,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
    content_stencil: &wgpu::StencilState,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("circle.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/circle.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("circle.pipeline.layout"),
        bind_group_layouts: &[Some(frame_bgl)],
        immediate_size: 0,
    });
    let circle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("circle.pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[CircleInstance::layout()],
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
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
    });
    let circle_xray_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("circle_xray.pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[CircleInstance::layout()],
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
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
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
    });
    (circle_pipeline, circle_xray_pipeline)
}

/// Helper to extract an analytical circle or circular arc instance from a single `TangentGeom`.
pub fn extract_circle_instance_from_geom(
    geom: &crate::scene::model::wire_model::TangentGeom,
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<CircleInstance> {
    extract_circle_instance_from_geom_indexed(geom, wire, draw_depth, 0)
}

/// Helper to extract an analytical circle or circular arc instance at a specific segment index.
pub fn extract_circle_instance_from_geom_indexed(
    geom: &crate::scene::model::wire_model::TangentGeom,
    wire: &crate::scene::WireModel,
    draw_depth: f32,
    geom_index: usize,
) -> Option<CircleInstance> {
    let (center, axis_x, axis_y, radius, start_angle, end_angle) = match *geom {
        crate::scene::model::wire_model::TangentGeom::Circle { center, radius } => (
            [center[0] as f64, center[1] as f64, center[2] as f64],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            radius as f64,
            0.0f32,
            std::f32::consts::TAU,
        ),
        crate::scene::model::wire_model::TangentGeom::PlanarCircle {
            center,
            axis_x,
            axis_y,
            radius,
        } => (center, axis_x, axis_y, radius, 0.0f32, std::f32::consts::TAU),
        crate::scene::model::wire_model::TangentGeom::Arc {
            center,
            axis_x,
            axis_y,
            radius,
            start_angle,
            end_angle,
        } => (
            center,
            axis_x,
            axis_y,
            radius,
            start_angle as f32,
            end_angle as f32,
        ),
        _ => return None,
    };
    if radius <= 0.0 || !radius.is_finite() || !start_angle.is_finite() || !end_angle.is_finite() || radius > 1e6 {
        return None;
    }

    let (ch, cl) = split_ds_xyz(center[0], center[1], center[2]);
    let hw = wire.line_weight_px * 0.5;
    let pat0 = [wire.pattern[0], wire.pattern[1], wire.pattern[2], wire.pattern[3]];
    let pat1 = [wire.pattern[4], wire.pattern[5], wire.pattern[6], wire.pattern[7]];

    let (start_width, end_width) = if !wire.taper_widths.is_empty() {
        if wire.taper_widths.len() >= 2 && wire.tangent_geoms.len() == 1 {
            (wire.taper_widths[0], wire.taper_widths[1])
        } else if wire.taper_widths.len() > geom_index {
            let sw = wire.taper_widths[geom_index];
            let ew = wire.taper_widths.get(geom_index + 1).copied().unwrap_or(sw);
            (sw, ew)
        } else {
            (wire.world_width, wire.world_width)
        }
    } else if wire.world_width > 0.0 {
        (wire.world_width, wire.world_width)
    } else {
        (0.0, 0.0)
    };

    Some(CircleInstance {
        center_high: ch,
        start_width,
        center_low: cl,
        radius: radius as f32,
        axis_x: [axis_x[0] as f32, axis_x[1] as f32, axis_x[2] as f32],
        start_angle,
        axis_y: [axis_y[0] as f32, axis_y[1] as f32, axis_y[2] as f32],
        end_angle,
        color: wire.color,
        params: [hw, wire.pattern_length, draw_depth, end_width],
        pat0,
        pat1,
    })
}

/// Helper to extract all analytical circle/arc instances from a `WireModel`.
///
/// Returns `Some(instances)` if the wire is non-empty, contains solely analytical
/// circle/arc tangent geometries, and has no mesh fills, text, or block instance.
pub fn extract_circle_instances(
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<Vec<CircleInstance>> {
    if wire.tangent_geoms.is_empty()
        || !wire.fill_tris.is_empty()
        || wire.fill_is_3d
        || !wire.text_verts.is_empty()
        || wire.render_instance.is_some()
    {
        return None;
    }

    let mut instances = Vec::with_capacity(wire.tangent_geoms.len());
    for (i, geom) in wire.tangent_geoms.iter().enumerate() {
        if let Some(inst) = extract_circle_instance_from_geom_indexed(geom, wire, draw_depth, i) {
            instances.push(inst);
        } else {
            return None;
        }
    }
    Some(instances)
}

/// Helper to extract a single analytical circle or circular arc instance from a `WireModel`.
pub fn extract_circle_instance(
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<CircleInstance> {
    if wire.tangent_geoms.len() == 1 {
        extract_circle_instances(wire, draw_depth).and_then(|mut v| v.pop())
    } else {
        None
    }
}

fn split_ds_xyz(x: f64, y: f64, z: f64) -> ([f32; 3], [f32; 3]) {
    let hx = x as f32;
    let lx = (x - hx as f64) as f32;
    let hy = y as f32;
    let ly = (y - hy as f64) as f32;
    let hz = z as f32;
    let lz = (z - hz as f64) as f32;
    ([hx, hy, hz], [lx, ly, lz])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_instance_size_matches_stride() {
        assert_eq!(std::mem::size_of::<CircleInstance>(), 128);
    }

    #[test]
    fn circle_shader_validates_with_naga() {
        let source = include_str!("../../shaders/circle.wgsl");
        let module = naga::front::wgsl::parse_str(source).expect("circle.wgsl parses cleanly");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("circle.wgsl validates cleanly with naga");
    }

    #[test]
    fn extract_circle_instance_planar_success() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarCircle {
            center: [100.0, 200.0, 300.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 50.0,
        });
        wire.line_weight_px = 2.0;
        wire.color = [1.0, 0.0, 0.0, 1.0];

        let inst = extract_circle_instance(&wire, 1.5).expect("should extract planar circle");
        assert_eq!(inst.radius, 50.0);
        assert_eq!(inst.start_angle, 0.0);
        assert_eq!(inst.end_angle, std::f32::consts::TAU);
        assert_eq!(inst.center_high, [100.0, 200.0, 300.0]);
        assert_eq!(inst.color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(inst.params[0], 1.0); // half lineweight
        assert_eq!(inst.params[2], 1.5); // draw depth
    }

    #[test]
    fn extract_circle_instance_arc_success() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
            center: [10.0, 20.0, 30.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 25.0,
            start_angle: 0.5,
            end_angle: 2.5,
        });
        wire.line_weight_px = 3.0;
        wire.color = [0.0, 1.0, 0.0, 1.0];

        let inst = extract_circle_instance(&wire, 0.5).expect("should extract circular arc");
        assert_eq!(inst.radius, 25.0);
        assert_eq!(inst.start_angle, 0.5);
        assert_eq!(inst.end_angle, 2.5);
        assert_eq!(inst.center_high, [10.0, 20.0, 30.0]);
        assert_eq!(inst.color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(inst.params[0], 1.5);
    }

    #[test]
    fn extract_circle_instance_rejects_non_planar_or_complex() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarCircle {
            center: [0.0, 0.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 10.0,
        });
        // 1. With fill_tris (e.g. 3D solid or hatch)
        wire.fill_tris = vec![[0.0; 3]];
        assert!(extract_circle_instance(&wire, 0.0).is_none());

        // 2. Clear fill_tris, add render_instance (block instance)
        wire.fill_tris.clear();
        wire.render_instance = Some(crate::scene::model::instance_model::RenderInstance {
            source_id: 1,
            translation: [0.0; 3],
        });
        assert!(extract_circle_instance(&wire, 0.0).is_none());

        // 3. Invalid radius
        wire.render_instance = None;
        wire.tangent_geoms[0] = crate::scene::model::wire_model::TangentGeom::PlanarCircle {
            center: [0.0, 0.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: -5.0,
        };
        assert!(extract_circle_instance(&wire, 0.0).is_none());
    }

    #[test]
    fn extract_circle_instance_constant_wide_arc() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
            center: [100.0, 50.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 40.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        });
        wire.world_width = 12.0;
        wire.color = [1.0, 0.5, 0.0, 1.0];

        let inst = extract_circle_instance(&wire, 0.0).expect("extract constant wide arc");
        assert_eq!(inst.start_width, 12.0);
        assert_eq!(inst.params[3], 12.0); // end_width
        assert_eq!(inst.radius, 40.0);
    }

    #[test]
    fn extract_circle_instance_tapered_arc() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Arc {
            center: [0.0, 0.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 30.0,
            start_angle: 0.2,
            end_angle: 1.8,
        });
        wire.world_width = 20.0;
        wire.taper_widths = vec![4.0, 20.0];
        wire.color = [0.2, 0.8, 1.0, 1.0];

        let inst = extract_circle_instance(&wire, 0.0).expect("extract tapered arc");
        assert_eq!(inst.start_width, 4.0);
        assert_eq!(inst.params[3], 20.0); // end_width
        assert_eq!(inst.radius, 30.0);
    }

    #[test]
    fn extract_circle_instance_accepts_pick_tris() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::Circle {
            center: [5.0, 5.0, 0.0],
            radius: 15.0,
        });
        wire.world_width = 6.0;
        wire.pick_tris = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        wire.pick_tris_low = vec![[0.0; 3]; 3];

        let insts = extract_circle_instances(&wire, 0.0).expect("wide curve with pick_tris should extract");
        assert_eq!(insts.len(), 1);
        assert_eq!(insts[0].start_width, 6.0);
        assert_eq!(insts[0].params[3], 6.0);
    }

    #[test]
    fn test_donut_polyline_tessellates_to_analytical_arcs() {
        use acadrust::CadDocument;
        let mut doc = CadDocument::new();
        let donut = crate::modules::draw::draw::donut::make_donut(10.0, 20.0, 0.0, 10.0, 30.0);
        let h = doc.add_entity(donut).unwrap();
        let entity = doc.get_entity(h).unwrap();
        let selected = rustc_hash::FxHashSet::default();
        let wires = crate::scene::convert::tess::tessellate_entity(
            &doc,
            &selected,
            None,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
            None,
            entity,
            None,
            None,
            None,
            false,
        );
        assert_eq!(wires.len(), 2, "donut should split into 2 analytical arc wires");
        for w in &wires {
            assert_eq!(w.world_width, 20.0);
            assert_eq!(w.tangent_geoms.len(), 1);
            assert!(matches!(w.tangent_geoms[0], crate::scene::model::wire_model::TangentGeom::Arc { .. }));
        }

        let depth_map = rustc_hash::FxHashMap::default();
        let partitioned = crate::scene::pipeline::wire_arena::partition_wires(&wires, &depth_map);
        assert_eq!(partitioned.circle_instances.len(), 2);
        assert_eq!(partitioned.regular.len(), 0);
        assert_eq!(partitioned.circle_instances[0].start_width, 20.0);
        assert_eq!(partitioned.circle_instances[0].params[3], 20.0);
        assert_eq!(partitioned.circle_instances[1].start_width, 20.0);
        assert_eq!(partitioned.circle_instances[1].params[3], 20.0);
    }
}
