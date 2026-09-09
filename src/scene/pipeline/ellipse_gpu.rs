//! GPU-instanced analytical ellipse buffer & pipeline layout.
//!
//! Renders 2D/planar ellipses and elliptical arcs as analytical screen/plane-aligned quads
//! with sub-pixel screen-space derivative anti-aliasing. Each ellipse is a single instance
//! (6 vertices for a 2-triangle quad), eliminating CPU re-tessellation on zoom and providing
//! mathematically exact curvature at any magnification.

use iced::wgpu;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EllipseInstance {
    pub center_high: [f32; 3],
    pub _pad0: f32,
    pub center_low: [f32; 3],
    pub minor_axis_ratio: f32,
    pub major_axis: [f32; 3],
    pub start_param: f32,
    pub normal: [f32; 3],
    pub end_param: f32,
    pub color: [f32; 4],
    /// `[half_width, pattern_length, draw_depth, padding]`
    pub params: [f32; 4],
    pub pat0: [f32; 4],
    pub pat1: [f32; 4],
}

impl EllipseInstance {
    pub fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: &[wgpu::VertexAttribute] = &[
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, center_high) as u64,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, center_low) as u64,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, major_axis) as u64,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, normal) as u64,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, color) as u64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, params) as u64,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, pat0) as u64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(EllipseInstance, pat1) as u64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<EllipseInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRS,
        }
    }
}

pub struct EllipseGpu {
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,
}

impl EllipseGpu {
    pub fn from_instances(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        instances: &[EllipseInstance],
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

/// Build the base and xray ellipse pipelines.
pub fn create_pipelines(
    device: &wgpu::Device,
    frame_bgl: &wgpu::BindGroupLayout,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
    content_stencil: &wgpu::StencilState,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ellipse.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/ellipse.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ellipse.pipeline.layout"),
        bind_group_layouts: &[Some(frame_bgl)],
        immediate_size: 0,
    });
    let ellipse_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ellipse.pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[EllipseInstance::layout()],
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
    let ellipse_xray_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ellipse_xray.pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[EllipseInstance::layout()],
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
    (ellipse_pipeline, ellipse_xray_pipeline)
}

/// Helper to extract an analytical ellipse instance from a single `TangentGeom`.
pub fn extract_ellipse_instance_from_geom(
    geom: &crate::scene::model::wire_model::TangentGeom,
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<EllipseInstance> {
    let crate::scene::model::wire_model::TangentGeom::PlanarEllipse {
        center,
        major_axis,
        normal,
        minor_axis_ratio,
        start_param,
        end_param,
    } = *geom else {
        return None;
    };

    let major_len_sq = major_axis[0] * major_axis[0] + major_axis[1] * major_axis[1] + major_axis[2] * major_axis[2];
    let norm_len_sq = normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2];
    if major_len_sq <= 1e-12 || !major_len_sq.is_finite() || major_len_sq > 1e12
        || norm_len_sq <= 1e-12 || !norm_len_sq.is_finite()
        || minor_axis_ratio <= 1e-6 || !minor_axis_ratio.is_finite()
        || !start_param.is_finite() || !end_param.is_finite()
    {
        return None;
    }

    let (ch, cl) = split_ds_xyz(center[0], center[1], center[2]);
    let hw = wire.line_weight_px * 0.5;
    let pat0 = [wire.pattern[0], wire.pattern[1], wire.pattern[2], wire.pattern[3]];
    let pat1 = [wire.pattern[4], wire.pattern[5], wire.pattern[6], wire.pattern[7]];

    Some(EllipseInstance {
        center_high: ch,
        _pad0: 0.0,
        center_low: cl,
        minor_axis_ratio: minor_axis_ratio.min(1.0) as f32,
        major_axis: [major_axis[0] as f32, major_axis[1] as f32, major_axis[2] as f32],
        start_param: start_param as f32,
        normal: [normal[0] as f32, normal[1] as f32, normal[2] as f32],
        end_param: end_param as f32,
        color: wire.color,
        params: [hw, wire.pattern_length, draw_depth, 0.0],
        pat0,
        pat1,
    })
}

/// Helper to extract all analytical ellipse instances from a `WireModel`.
pub fn extract_ellipse_instances(
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<Vec<EllipseInstance>> {
    if wire.tangent_geoms.is_empty()
        || !wire.fill_tris.is_empty()
        || wire.fill_is_3d
        || !wire.text_verts.is_empty()
        || wire.render_instance.is_some()
    {
        return None;
    }

    let mut instances = Vec::with_capacity(wire.tangent_geoms.len());
    for geom in &wire.tangent_geoms {
        if let Some(inst) = extract_ellipse_instance_from_geom(geom, wire, draw_depth) {
            instances.push(inst);
        } else {
            return None;
        }
    }
    Some(instances)
}

/// Helper to extract a single analytical ellipse instance from a `WireModel`.
pub fn extract_ellipse_instance(
    wire: &crate::scene::WireModel,
    draw_depth: f32,
) -> Option<EllipseInstance> {
    if wire.tangent_geoms.len() == 1 {
        extract_ellipse_instances(wire, draw_depth).and_then(|mut v| v.pop())
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
    fn ellipse_instance_size_matches_stride() {
        assert_eq!(std::mem::size_of::<EllipseInstance>(), 128);
    }

    #[test]
    fn ellipse_shader_validates_with_naga() {
        let source = include_str!("../../shaders/ellipse.wgsl");
        let module = naga::front::wgsl::parse_str(source).expect("ellipse.wgsl parses cleanly");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("ellipse.wgsl validates cleanly with naga");
    }

    #[test]
    fn extract_ellipse_instance_success() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarEllipse {
            center: [100.0, 200.0, 300.0],
            major_axis: [50.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            minor_axis_ratio: 0.5,
            start_param: 0.0,
            end_param: std::f64::consts::TAU,
        });
        wire.line_weight_px = 2.0;
        wire.color = [1.0, 0.5, 0.0, 1.0];

        let inst = extract_ellipse_instance(&wire, 1.0).expect("should extract ellipse");
        assert_eq!(inst.center_high, [100.0, 200.0, 300.0]);
        assert_eq!(inst.major_axis, [50.0, 0.0, 0.0]);
        assert_eq!(inst.minor_axis_ratio, 0.5);
        assert_eq!(inst.start_param, 0.0);
        assert_eq!(inst.end_param, std::f32::consts::TAU);
        assert_eq!(inst.color, [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(inst.params[0], 1.0);
        assert_eq!(inst.params[2], 1.0);
    }

    #[test]
    fn extract_elliptical_arc_instance_success() {
        let mut wire = crate::scene::WireModel::default();
        wire.tangent_geoms.push(crate::scene::model::wire_model::TangentGeom::PlanarEllipse {
            center: [0.0, 0.0, 0.0],
            major_axis: [10.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            minor_axis_ratio: 0.8,
            start_param: 0.5,
            end_param: 2.5,
        });
        wire.line_weight_px = 1.0;
        wire.color = [0.0, 1.0, 0.0, 1.0];

        let inst = extract_ellipse_instance(&wire, 0.5).expect("should extract elliptical arc");
        assert_eq!(inst.minor_axis_ratio, 0.8);
        assert_eq!(inst.start_param, 0.5);
        assert_eq!(inst.end_param, 2.5);
        assert_eq!(inst.params[0], 0.5); // half-width
        assert_eq!(inst.params[2], 0.5); // draw-depth
    }

    fn rust_closest_point_ellipse(p_in: [f32; 2], ab_in: [f32; 2]) -> [f32; 2] {
        let px = p_in[0].abs();
        let py = p_in[1].abs();
        let a = ab_in[0];
        let b = ab_in[1];
        if (a - b).abs() < 1e-6 {
            let len = (px * px + py * py).sqrt();
            let q = if len > 1e-6 {
                [px / len * a, py / len * a]
            } else {
                [a, 0.0]
            };
            return [
                if p_in[0] >= 0.0 { q[0] } else { -q[0] },
                if p_in[1] >= 0.0 { q[1] } else { -q[1] },
            ];
        }

        // Initial estimate from eccentric anomaly of radial direction
        let mut t = (py * a).atan2(px * b);
        let k = a * a - b * b;

        // 3 iterations of Newton-Raphson
        for _ in 0..3 {
            let (st, ct) = t.sin_cos();
            let f = k * st * ct - px * a * st + py * b * ct;
            let f_prime = k * (ct * ct - st * st) - px * a * ct - py * b * st;
            if f_prime.abs() > 1e-7 {
                t -= f / f_prime;
            }
        }
        t = t.clamp(0.0, std::f32::consts::FRAC_PI_2);
        let q = [a * t.cos(), b * t.sin()];
        [
            if p_in[0] >= 0.0 { q[0] } else { -q[0] },
            if p_in[1] >= 0.0 { q[1] } else { -q[1] },
        ]
    }

    #[test]
    fn closest_point_ellipse_accuracy() {
        let a = 100.0f32;
        let b = 40.0f32;
        // Test points directly on the ellipse: closest point should be itself!
        for i in 0..36 {
            let angle = (i as f32) * std::f32::consts::TAU / 36.0;
            let px = a * angle.cos();
            let py = b * angle.sin();
            let q = rust_closest_point_ellipse([px, py], [a, b]);
            let dist = ((q[0] - px).powi(2) + (q[1] - py).powi(2)).sqrt();
            assert!(
                dist < 0.2,
                "Point on ellipse at angle {angle} failed: px={px}, py={py}, q={:?}, dist={dist}",
                q
            );
            // Verify q is on the ellipse (x/a)^2 + (y/b)^2 == 1
            let eq = (q[0] / a).powi(2) + (q[1] / b).powi(2);
            assert!((eq - 1.0).abs() < 0.01, "eq={eq} for angle {angle}");
        }

        // Test points offset from ellipse (inside and outside)
        for i in 0..36 {
            let angle = (i as f32) * std::f32::consts::TAU / 36.0;
            for offset in [-5.0f32, -1.0, 1.0, 5.0] {
                let norm = [angle.cos() / a, angle.sin() / b];
                let norm_len = (norm[0] * norm[0] + norm[1] * norm[1]).sqrt();
                let un = [norm[0] / norm_len, norm[1] / norm_len];
                let px = a * angle.cos() + offset * un[0];
                let py = b * angle.sin() + offset * un[1];
                let q = rust_closest_point_ellipse([px, py], [a, b]);
                let eq = (q[0] / a).powi(2) + (q[1] / b).powi(2);
                assert!(
                    (eq - 1.0).abs() < 0.05,
                    "Offset point at angle {angle}, offset {offset}: q={:?}, eq={eq}",
                    q
                );
                let measured_dist = ((q[0] - px).powi(2) + (q[1] - py).powi(2)).sqrt();
                assert!(
                    (measured_dist - offset.abs()).abs() < 0.2,
                    "Distance error at angle {angle}, offset {offset}: measured={measured_dist}"
                );
            }
        }
    }
}
