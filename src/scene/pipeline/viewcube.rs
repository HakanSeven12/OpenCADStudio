// ViewCube wgpu pipeline — OpenCADStudio-style interactive 3D navigation cube.
//
// 26 selectable regions:  6 faces + 12 edges + 8 corners.
// Phong shading + hover highlight passed as uniform.
// Hit-test is 100% CPU — no GPU readback needed.
//
// The ViewCube rotation matrix is derived directly from the camera quaternion
// (cam_rotation: Mat4) everywhere — shader, text labels, hit-test, hover-id.
// This eliminates gimbal lock at top/bottom views and keeps the cube in sync
// with arcball orbit at all angles.

use bytemuck::{Pod, Zeroable};
use glam::camera::rh::proj::directx::orthographic;
use glam::{Mat4, Vec3, Vec4};
use iced::wgpu;
use iced::{Rectangle, Size};
use crate::t;

#[path = "viewcube_text_atlas.rs"]
mod viewcube_text_atlas;

use viewcube_text_atlas::{
    build_label_atlas, empty_label_atlas, AtlasTile, ATLAS_HEIGHT, ATLAS_WIDTH,
    CARDINAL_TILE_START, FACE_TILE_COUNT, TILE_COUNT,
};

const VIEWCUBE_MSAA_SAMPLES: u32 = 4;

// ── ViewCube layout ───────────────────────────────────────────────────────
pub const VIEWCUBE_PX: u32 = 84; // 30% smaller cube (was 120)
pub const VIEWCUBE_SCALE: f32 = 0.36;
pub const VIEWCUBE_DRAW_PX: f32 = VIEWCUBE_PX as f32 * VIEWCUBE_SCALE * 2.0;
pub const VIEWCUBE_PAD: f32 = 12.0;
/// The cube centre is inset from the screen corner by this multiple of the
/// cube half-size, leaving room for the compass ring and the nav controls
/// (home / roll / nudge) drawn around it. Tighter than before so the controls
/// hug the cube in the corner instead of floating in a large dead region.
pub const NAV_INSET_F: f32 = 2.0;
/// Side of the whole nav widget (cube + compass ring + controls) in pixels.
pub const VIEWCUBE_REGION_PX: f32 = VIEWCUBE_DRAW_PX * NAV_INSET_F;
pub const VIEWCUBE_RENDER_PX: f32 = VIEWCUBE_REGION_PX + 20.0;
/// Z height of the compass ring + cardinals in cube-local space. The cube
/// spans ±1, so −1 parks the ring at the cube's base — it sits *under* the
/// cube in 3D views and reads as a ground compass.
const RING_Z: f32 = -1.0;
/// Horizontal radius of the N/E/S/W cardinal letters — the ring's mid-line, so
/// they sit painted on the ring band.
const R_CARD: f32 = 1.57;

/// Compass cardinal directions, mapped to a side-face snap when clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinal {
    North,
    East,
    South,
    West,
}

/// 90° view nudge directions (tip the cube up/down or spin it left/right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeDir {
    Up,
    Down,
    Left,
    Right,
}

/// Face labels in the current UI language.
fn face_labels() -> [std::borrow::Cow<'static, str>; 6] {
    [
        t!("ViewCube Top"),
        t!("ViewCube Bottom"),
        t!("ViewCube Front"),
        t!("ViewCube Back"),
        t!("ViewCube Right"),
        t!("ViewCube Left"),
    ]
}

const FACE_CENTERS: [[f32; 3]; 6] = [
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
];

fn face_label_axes(face: usize) -> (Vec3, Vec3) {
    match face {
        FACE_TOP => (Vec3::X, Vec3::Y),
        FACE_BOTTOM => (Vec3::X, -Vec3::Y),
        FACE_FRONT => (Vec3::X, Vec3::Z),
        FACE_BACK => (-Vec3::X, Vec3::Z),
        FACE_RIGHT => (Vec3::Y, Vec3::Z),
        FACE_LEFT => (-Vec3::Y, Vec3::Z),
        _ => (Vec3::X, Vec3::Y),
    }
}

pub const FACE_TOP: usize = 0;
pub const FACE_BOTTOM: usize = 1;
pub const FACE_FRONT: usize = 2;
pub const FACE_BACK: usize = 3;
pub const FACE_RIGHT: usize = 4;
pub const FACE_LEFT: usize = 5;
pub const EDGE_TOP_FRONT: usize = 6;
pub const EDGE_TOP_BACK: usize = 7;
pub const EDGE_TOP_RIGHT: usize = 8;
pub const EDGE_TOP_LEFT: usize = 9;
pub const EDGE_BOT_FRONT: usize = 10;
pub const EDGE_BOT_BACK: usize = 11;
pub const EDGE_BOT_RIGHT: usize = 12;
pub const EDGE_BOT_LEFT: usize = 13;
pub const EDGE_FRONT_RIGHT: usize = 14;
pub const EDGE_FRONT_LEFT: usize = 15;
pub const EDGE_BACK_RIGHT: usize = 16;
pub const EDGE_BACK_LEFT: usize = 17;
pub const CORNER_TPF_R: usize = 18;
pub const CORNER_TPF_L: usize = 19;
pub const CORNER_TBK_R: usize = 20;
pub const CORNER_TBK_L: usize = 21;
pub const CORNER_BTF_R: usize = 22;
pub const CORNER_BTF_L: usize = 23;
pub const CORNER_BBK_R: usize = 24;
pub const CORNER_BBK_L: usize = 25;
pub const NUM_REGIONS: usize = 26;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CubeRegion {
    Face(usize),
    Edge(usize),
    Corner(usize),
}

impl CubeRegion {
    pub fn id(self) -> usize {
        match self {
            Self::Face(i) | Self::Edge(i) | Self::Corner(i) => i,
        }
    }

    /// Unit eye-direction vector (from target toward the camera) that
    /// looks straight at this region. Used by `Camera::snap_to_direction`
    /// which derives the full orientation by re-using the current
    /// camera's up vector, projected onto the plane perpendicular to
    /// this direction — so clicking an edge spins the cube around the
    /// edge without rolling the user's "up" sense.
    pub fn snap_direction(self) -> glam::Vec3 {
        let c = region_centroids()[self.id()];
        glam::Vec3::new(c[0], c[1], c[2]).normalize_or(glam::Vec3::Z)
    }

    pub fn opposite(self) -> CubeRegion {
        match self {
            CubeRegion::Face(FACE_TOP) => CubeRegion::Face(FACE_BOTTOM),
            CubeRegion::Face(FACE_BOTTOM) => CubeRegion::Face(FACE_TOP),
            CubeRegion::Face(FACE_FRONT) => CubeRegion::Face(FACE_BACK),
            CubeRegion::Face(FACE_BACK) => CubeRegion::Face(FACE_FRONT),
            CubeRegion::Face(FACE_RIGHT) => CubeRegion::Face(FACE_LEFT),
            CubeRegion::Face(FACE_LEFT) => CubeRegion::Face(FACE_RIGHT),
            other => other,
        }
    }

    pub fn label(self) -> &'static str {
        match self.id() {
            0 => "TOP",
            1 => "BOTTOM",
            2 => "FRONT",
            3 => "BACK",
            4 => "RIGHT",
            5 => "LEFT",
            6 => "Top Front",
            7 => "Top Back",
            8 => "Top Right",
            9 => "Top Left",
            10 => "Bot Front",
            11 => "Bot Back",
            12 => "Bot Right",
            13 => "Bot Left",
            14 => "Front Right",
            15 => "Front Left",
            16 => "Back Right",
            17 => "Back Left",
            18 => "Top Front Right",
            19 => "Top Front Left",
            20 => "Top Back Right",
            21 => "Top Back Left",
            22 => "Bot Front Right",
            23 => "Bot Front Left",
            24 => "Bot Back Right",
            25 => "Bot Back Left",
            _ => "?",
        }
    }
}

// ── Vertex ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct CubeVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub region_f: f32,
}

impl CubeVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32,
    ];
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct LineVertex {
    pos: [f32; 3],
}

impl LineVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct CubeUniforms {
    pub view_proj: [f32; 16],
    pub rotation: [f32; 16],
    pub hover_region: [f32; 4],
}

impl CubeUniforms {
    /// Build uniforms from the camera quaternion-derived rotation matrix.
    /// `cam_rotation` = `Mat4::from_quat(camera.rotation)`.
    pub fn new(
        cam_rotation: Mat4,
        cube_px: u32,
        vp_w: u32,
        vp_h: u32,
        hover: Option<usize>,
    ) -> Self {
        let (hw, hh) = (vp_w as f32 * 0.5, vp_h as f32 * 0.5);
        let cube_half = cube_px as f32 * VIEWCUBE_SCALE;
        // Inset the cube centre to leave room for the ring + controls.
        let inset = cube_half * NAV_INSET_F;
        let cx = hw - inset - VIEWCUBE_PAD;
        let cy = hh - inset - VIEWCUBE_PAD;
        let view_proj = orthographic(-hw, hw, -hh, hh, -2000.0, 2000.0)
            * Mat4::from_translation(Vec3::new(cx, cy, 0.0))
            * Mat4::from_scale(Vec3::splat(cube_px as f32 * VIEWCUBE_SCALE));
        Self {
            view_proj: view_proj.to_cols_array(),
            rotation: cam_rotation.to_cols_array(),
            hover_region: [
                hover.map(|h| h as f32 / 25.0).unwrap_or(-1.0),
                0.0,
                0.0,
                0.0,
            ],
        }
    }
}

// ── Shaped label text ─────────────────────────────────────────────────────

const MAX_VERTS: usize = TILE_COUNT * 6;
const FACE_LABEL_HEIGHT: f32 = 0.46;
const CARDINAL_LABEL_HEIGHT: f32 = FACE_LABEL_HEIGHT;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TextUniforms {
    screen: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TextVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x2, 2 => Float32x4,
    ];
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

fn face_label_strings() -> [String; FACE_TILE_COUNT] {
    let labels = face_labels();
    std::array::from_fn(|index| labels[index].to_string())
}

fn write_label_atlas(queue: &wgpu::Queue, texture: &wgpu::Texture, pixels: &[u8]) {
    queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(ATLAS_WIDTH),
            rows_per_image: Some(ATLAS_HEIGHT),
        },
        wgpu::Extent3d {
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
            depth_or_array_layers: 1,
        },
    );
}

struct ViewCubeText {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas_texture: wgpu::Texture,
    tiles: [AtlasTile; TILE_COUNT],
    labels: [String; FACE_TILE_COUNT],
    font_generation: u64,
}

impl ViewCubeText {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let labels = face_label_strings();
        let atlas = build_label_atlas(&labels).unwrap_or_else(empty_label_atlas);
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vc.text_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_WIDTH,
                height: ATLAS_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_label_atlas(queue, &atlas_tex, &atlas.pixels);
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vc.text_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vc.text_uniform"),
            size: std::mem::size_of::<TextUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vc.text_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vc.text_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vc.text_layout"),
            bind_group_layouts: &[&bgl].map(Some),
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vc.text_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/viewcube_text.wgsl"
            ))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vc.text_pipe"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TextVertex::desc()],
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
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: VIEWCUBE_MSAA_SAMPLES,
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
        let vertex_capacity = MAX_VERTS as u32;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vc.text_vb"),
            size: (vertex_capacity as usize * std::mem::size_of::<TextVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            vertex_buffer,
            vertex_count: 0,
            uniform_buffer,
            bind_group,
            atlas_texture: atlas_tex,
            tiles: atlas.tiles,
            labels,
            font_generation: crate::scene::text::web_font::generation(),
        }
    }

    /// Update text labels using the quaternion-derived rotation matrix.
    fn update(
        &mut self,
        queue: &wgpu::Queue,
        cam_rotation: Mat4,
        compass_rotation: Mat4,
        vp_w: u32,
        vp_h: u32,
        cube_px: u32,
        _text_color: [f32; 4],
    ) {
        let (vw, vh) = (vp_w as f32, vp_h as f32);
        let cube_half = cube_px as f32 * VIEWCUBE_SCALE;
        let inset = cube_half * NAV_INSET_F;
        let (hw, hh) = (vw * 0.5, vh * 0.5);
        let view_proj = orthographic(-hw, hw, -hh, hh, -2000.0, 2000.0)
            * Mat4::from_translation(Vec3::new(
                hw - inset - VIEWCUBE_PAD,
                hh - inset - VIEWCUBE_PAD,
                0.0,
            ))
            * Mat4::from_scale(Vec3::splat(cube_px as f32 * VIEWCUBE_SCALE));

        let labels = face_label_strings();
        let font_generation = crate::scene::text::web_font::generation();
        if labels != self.labels || font_generation != self.font_generation {
            if let Some(atlas) = build_label_atlas(&labels) {
                write_label_atlas(queue, &self.atlas_texture, &atlas.pixels);
                self.tiles = atlas.tiles;
            }
            self.labels = labels;
            self.font_generation = font_generation;
        }

        let mut verts: Vec<TextVertex> = Vec::with_capacity(MAX_VERTS);
        let view_dir = Vec3::Z;
        // Local cube point → screen pixel.
        let project = |local: Vec3| -> Option<[f32; 3]> {
            let world = cam_rotation.transform_point3(local);
            let clip = view_proj * Vec4::new(world.x, world.y, world.z, 1.0);
            if clip.w.abs() < 1e-6 {
                return None;
            }
            Some([
                (clip.x / clip.w + 1.0) * 0.5 * vw,
                (1.0 - clip.y / clip.w) * 0.5 * vh,
                clip.z / clip.w,
            ])
        };

        for (fi, &c) in FACE_CENTERS.iter().enumerate() {
            let face_n = Vec3::from(c);
            let world_n = cam_rotation.transform_vector3(face_n).normalize();
            let dot = world_n.dot(view_dir);
            if dot < 0.12 {
                continue;
            }
            let color = [0.0, 0.0, 0.0, 1.0];
            let (u, v) = face_label_axes(fi);
            let center = face_n * 1.002;
            let tile = self.tiles[fi];
            if tile.aspect <= 0.0 {
                continue;
            }
            let label_width = FACE_LABEL_HEIGHT * tile.aspect;
            let corner = |lx: f32, ly: f32| center + u * lx + v * ly;
            let tl = project(corner(-label_width * 0.5, FACE_LABEL_HEIGHT * 0.5));
            let tr = project(corner(label_width * 0.5, FACE_LABEL_HEIGHT * 0.5));
            let br = project(corner(label_width * 0.5, -FACE_LABEL_HEIGHT * 0.5));
            let bl = project(corner(-label_width * 0.5, -FACE_LABEL_HEIGHT * 0.5));
            if let (Some(tl), Some(tr), Some(br), Some(bl)) = (tl, tr, br, bl) {
                let mk = |pos: [f32; 3], uv: [f32; 2]| TextVertex { pos, uv, color };
                verts.push(mk(tl, tile.uv_min));
                verts.push(mk(tr, [tile.uv_max[0], tile.uv_min[1]]));
                verts.push(mk(br, tile.uv_max));
                verts.push(mk(tl, tile.uv_min));
                verts.push(mk(br, tile.uv_max));
                verts.push(mk(bl, [tile.uv_min[0], tile.uv_max[1]]));
            }
        }

        // ── Compass cardinals (N / E / S / W) ──────────────────────────────
        // Painted flat onto the ring band (z = RING_Z) so they foreshorten with
        // the ring and read upright in plan. Local axes u = +X, v = +Y give
        // u × v = +Z → never mirrored when seen from above.
        //
        // The compass is world-fixed: it projects through `compass_rotation`
        // (camera only, no UCS), so N/E/S/W stay aligned to world even when the
        // cube reorients with the active UCS.
        let project_world = |local: Vec3| -> Option<[f32; 3]> {
            let world = compass_rotation.transform_point3(local);
            let clip = view_proj * Vec4::new(world.x, world.y, world.z, 1.0);
            if clip.w.abs() < 1e-6 {
                return None;
            }
            Some([
                (clip.x / clip.w + 1.0) * 0.5 * vw,
                (1.0 - clip.y / clip.w) * 0.5 * vh,
                clip.z / clip.w,
            ])
        };
        let cardinals = [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ];
        for (index, dir) in cardinals.into_iter().enumerate() {
            let tile = self.tiles[CARDINAL_TILE_START + index];
            if tile.aspect <= 0.0 {
                continue;
            }
            let card_width = CARDINAL_LABEL_HEIGHT * tile.aspect;
            let center = Vec3::new(dir.x * R_CARD, dir.y * R_CARD, RING_Z + 0.004);
            let color = [0.0, 0.0, 0.0, 1.0];
            let corner = |lx: f32, ly: f32| center + Vec3::X * lx + Vec3::Y * ly;
            let tl = project_world(corner(-card_width * 0.5, CARDINAL_LABEL_HEIGHT * 0.5));
            let tr = project_world(corner(card_width * 0.5, CARDINAL_LABEL_HEIGHT * 0.5));
            let br = project_world(corner(card_width * 0.5, -CARDINAL_LABEL_HEIGHT * 0.5));
            let bl = project_world(corner(-card_width * 0.5, -CARDINAL_LABEL_HEIGHT * 0.5));
            if let (Some(tl), Some(tr), Some(br), Some(bl)) = (tl, tr, br, bl) {
                let mk = |pos: [f32; 3], uv: [f32; 2]| TextVertex { pos, uv, color };
                verts.push(mk(tl, tile.uv_min));
                verts.push(mk(tr, [tile.uv_max[0], tile.uv_min[1]]));
                verts.push(mk(br, tile.uv_max));
                verts.push(mk(tl, tile.uv_min));
                verts.push(mk(br, tile.uv_max));
                verts.push(mk(bl, [tile.uv_min[0], tile.uv_max[1]]));
            }
        }
        self.vertex_count = verts.len() as u32;
        if self.vertex_count > 0 {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&verts));
        }
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&TextUniforms {
                screen: [vw, vh],
                _pad: [0.0; 2],
            }),
        );
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        clip: Rectangle<u32>,
    ) {
        if self.vertex_count == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vc.text_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            clip.x as f32,
            clip.y as f32,
            clip.width as f32,
            clip.height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

// ── Geometry ──────────────────────────────────────────────────────────────

const F: f32 = 0.80;
const E: f32 = 1.00;
const SURFACE_RGB: [f32; 3] = [0.62, 0.76, 0.84];

fn push_quad(
    corners: [[f32; 3]; 4],
    rgb: [f32; 3],
    region: usize,
    vs: &mut Vec<CubeVertex>,
    is: &mut Vec<u32>,
) {
    let mut cs = corners;
    let center = {
        let s = Vec3::from(cs[0]) + Vec3::from(cs[1]) + Vec3::from(cs[2]) + Vec3::from(cs[3]);
        (s * 0.25).normalize_or_zero()
    };
    let mut n = (Vec3::from(cs[1]) - Vec3::from(cs[0]))
        .cross(Vec3::from(cs[3]) - Vec3::from(cs[0]))
        .normalize_or_zero();
    if n.dot(center) < 0.0 {
        cs = [cs[0], cs[3], cs[2], cs[1]];
        n = (Vec3::from(cs[1]) - Vec3::from(cs[0]))
            .cross(Vec3::from(cs[3]) - Vec3::from(cs[0]))
            .normalize_or_zero();
    }
    let n = n.to_array();
    let rf = region as f32 / 25.0;
    let base = vs.len() as u32;
    for pos in cs {
        vs.push(CubeVertex {
            pos,
            normal: n,
            color: rgb,
            region_f: rf,
        });
    }
    is.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_tri(
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    rgb: [f32; 3],
    region: usize,
    vs: &mut Vec<CubeVertex>,
    is: &mut Vec<u32>,
) {
    let mut b = b;
    let mut c = c;
    let center = {
        let s = Vec3::from(a) + Vec3::from(b) + Vec3::from(c);
        (s / 3.0).normalize_or_zero()
    };
    let mut n = (Vec3::from(b) - Vec3::from(a))
        .cross(Vec3::from(c) - Vec3::from(a))
        .normalize_or_zero();
    if n.dot(center) < 0.0 {
        std::mem::swap(&mut b, &mut c);
        n = (Vec3::from(b) - Vec3::from(a))
            .cross(Vec3::from(c) - Vec3::from(a))
            .normalize_or_zero();
    }
    let n = n.to_array();
    let rf = region as f32 / 25.0;
    let base = vs.len() as u32;
    for pos in [a, b, c] {
        vs.push(CubeVertex {
            pos,
            normal: n,
            color: rgb,
            region_f: rf,
        });
    }
    is.extend_from_slice(&[base, base + 1, base + 2]);
}

pub fn build_geometry() -> (Vec<CubeVertex>, Vec<u32>) {
    let (mut vs, mut is) = (Vec::<CubeVertex>::new(), Vec::<u32>::new());
    push_quad(
        [[-F, -F, E], [F, -F, E], [F, F, E], [-F, F, E]],
        SURFACE_RGB,
        FACE_TOP,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, F, -E], [F, F, -E], [F, -F, -E], [-F, -F, -E]],
        SURFACE_RGB,
        FACE_BOTTOM,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, -E, -F], [-F, -E, -F], [-F, -E, F], [F, -E, F]],
        SURFACE_RGB,
        FACE_FRONT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, E, -F], [F, E, -F], [F, E, F], [-F, E, F]],
        SURFACE_RGB,
        FACE_BACK,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[E, F, -F], [E, -F, -F], [E, -F, F], [E, F, F]],
        SURFACE_RGB,
        FACE_RIGHT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-E, -F, -F], [-E, F, -F], [-E, F, F], [-E, -F, F]],
        SURFACE_RGB,
        FACE_LEFT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, -F, E], [-F, -F, E], [-F, -E, F], [F, -E, F]],
        SURFACE_RGB,
        EDGE_TOP_FRONT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, F, E], [F, F, E], [F, E, F], [-F, E, F]],
        SURFACE_RGB,
        EDGE_TOP_BACK,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, F, E], [F, -F, E], [E, -F, F], [E, F, F]],
        SURFACE_RGB,
        EDGE_TOP_RIGHT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, -F, E], [-F, F, E], [-E, F, F], [-E, -F, F]],
        SURFACE_RGB,
        EDGE_TOP_LEFT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, -F, -E], [-F, -F, -E], [-F, -E, -F], [F, -E, -F]],
        SURFACE_RGB,
        EDGE_BOT_FRONT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, F, -E], [F, F, -E], [F, E, -F], [-F, E, -F]],
        SURFACE_RGB,
        EDGE_BOT_BACK,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, F, -E], [F, -F, -E], [E, -F, -F], [E, F, -F]],
        SURFACE_RGB,
        EDGE_BOT_RIGHT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, -F, -E], [-F, F, -E], [-E, F, -F], [-E, -F, -F]],
        SURFACE_RGB,
        EDGE_BOT_LEFT,
        &mut vs,
        &mut is,
    );
    // Side edges: diagonal chamfer strips connecting vertical face pairs.
    // Each strip spans from one face edge to the adjacent face edge — not flat in one plane.
    push_quad(
        [[F, -E, -F], [F, -E, F], [E, -F, F], [E, -F, -F]],
        SURFACE_RGB,
        EDGE_FRONT_RIGHT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, -E, F], [-F, -E, -F], [-E, -F, -F], [-E, -F, F]],
        SURFACE_RGB,
        EDGE_FRONT_LEFT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[F, E, F], [F, E, -F], [E, F, -F], [E, F, F]],
        SURFACE_RGB,
        EDGE_BACK_RIGHT,
        &mut vs,
        &mut is,
    );
    push_quad(
        [[-F, E, F], [-F, E, -F], [-E, F, -F], [-E, F, F]],
        SURFACE_RGB,
        EDGE_BACK_LEFT,
        &mut vs,
        &mut is,
    );
    for &([sx, sy, sz], region) in &[
        ([1.0f32, 1.0, 1.0], CORNER_TBK_R), // sy=+1 → BACK direction
        ([-1.0, 1.0, 1.0], CORNER_TBK_L),
        ([1.0, 1.0, -1.0], CORNER_BBK_R),
        ([-1.0, 1.0, -1.0], CORNER_BBK_L),
        ([1.0, -1.0, 1.0], CORNER_TPF_R), // sy=-1 → FRONT direction
        ([-1.0, -1.0, 1.0], CORNER_TPF_L),
        ([1.0, -1.0, -1.0], CORNER_BTF_R),
        ([-1.0, -1.0, -1.0], CORNER_BTF_L),
    ] {
        push_tri(
            [sx * F, sy * F, sz * E],
            [sx * F, sy * E, sz * F],
            [sx * E, sy * F, sz * F],
            SURFACE_RGB,
            region,
            &mut vs,
            &mut is,
        );
    }
    (vs, is)
}

fn build_ring_geometry() -> (Vec<CubeVertex>, Vec<u32>) {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    build_ring(&mut vertices, &mut indices);
    (vertices, indices)
}

/// A flat compass ring in the cube's local XY plane (the ground plane),
/// surrounding the cube. Pushed with a sentinel `region_f = -1.0` so the
/// shader never highlights it on hover.
fn build_ring(vs: &mut Vec<CubeVertex>, is: &mut Vec<u32>) {
    const SEG: usize = 64;
    const R0: f32 = 1.40; // inner radius — clear gap to the cube faces
    const R1: f32 = 1.74; // outer radius — wider, thicker band
    for s in 0..SEG {
        let a0 = s as f32 / SEG as f32 * std::f32::consts::TAU;
        let a1 = (s + 1) as f32 / SEG as f32 * std::f32::consts::TAU;
        let (c0, s0) = (a0.cos(), a0.sin());
        let (c1, s1) = (a1.cos(), a1.sin());
        let quad = [
            [c0 * R0, s0 * R0, RING_Z],
            [c1 * R0, s1 * R0, RING_Z],
            [c1 * R1, s1 * R1, RING_Z],
            [c0 * R1, s0 * R1, RING_Z],
        ];
        let base = vs.len() as u32;
        for pos in quad {
            vs.push(CubeVertex {
                pos,
                normal: [0.0, 0.0, 1.0],
                color: SURFACE_RGB,
                region_f: -1.0,
            });
        }
        is.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn surface_edge_lines(vertices: &[CubeVertex], indices: &[u32]) -> Vec<LineVertex> {
    use std::collections::HashMap;

    type PointKey = [i32; 3];
    type EdgeKey = (u32, PointKey, PointKey);

    fn point_key(point: [f32; 3]) -> PointKey {
        point.map(|value| (value * 1_000_000.0).round() as i32)
    }

    let mut edges: HashMap<EdgeKey, ([f32; 3], [f32; 3], u32)> = HashMap::new();
    for triangle in indices.chunks_exact(3) {
        let region = vertices[triangle[0] as usize].region_f.to_bits();
        for (a_index, b_index) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let a = vertices[a_index as usize].pos;
            let b = vertices[b_index as usize].pos;
            let a_key = point_key(a);
            let b_key = point_key(b);
            let (start_key, end_key, start, end) = if a_key <= b_key {
                (a_key, b_key, a, b)
            } else {
                (b_key, a_key, b, a)
            };
            let entry = edges
                .entry((region, start_key, end_key))
                .or_insert((start, end, 0));
            entry.2 += 1;
        }
    }

    edges
        .into_values()
        .filter(|(_, _, count)| *count == 1)
        .flat_map(|(start, end, _)| [LineVertex { pos: start }, LineVertex { pos: end }])
        .collect()
}

pub fn region_centroids() -> [[f32; 3]; NUM_REGIONS] {
    let m = (F + E) * 0.5;
    [
        [0.0, 0.0, E],  // FACE_TOP
        [0.0, 0.0, -E], // FACE_BOTTOM
        [0.0, -E, 0.0], // FACE_FRONT  (geometry Y=-E)
        [0.0, E, 0.0],  // FACE_BACK   (geometry Y=+E)
        [E, 0.0, 0.0],  // FACE_RIGHT
        [-E, 0.0, 0.0], // FACE_LEFT
        [0.0, -m, m],   // EDGE_TOP_FRONT
        [0.0, m, m],    // EDGE_TOP_BACK
        [m, 0.0, m],    // EDGE_TOP_RIGHT
        [-m, 0.0, m],   // EDGE_TOP_LEFT
        [0.0, -m, -m],  // EDGE_BOT_FRONT
        [0.0, m, -m],   // EDGE_BOT_BACK
        [m, 0.0, -m],   // EDGE_BOT_RIGHT
        [-m, 0.0, -m],  // EDGE_BOT_LEFT
        [m, -m, 0.0],   // EDGE_FRONT_RIGHT
        [-m, -m, 0.0],  // EDGE_FRONT_LEFT
        [m, m, 0.0],    // EDGE_BACK_RIGHT
        [-m, m, 0.0],   // EDGE_BACK_LEFT
        [m, -m, m],     // CORNER_TPF_R  (geometry sy=-1 = FRONT)
        [-m, -m, m],    // CORNER_TPF_L
        [m, m, m],      // CORNER_TBK_R  (geometry sy=+1 = BACK)
        [-m, m, m],     // CORNER_TBK_L
        [m, -m, -m],    // CORNER_BTF_R  (geometry sy=-1 = FRONT)
        [-m, -m, -m],   // CORNER_BTF_L
        [m, m, -m],     // CORNER_BBK_R  (geometry sy=+1 = BACK)
        [-m, m, -m],    // CORNER_BBK_L
    ]
}

fn threshold_sq(id: usize, cube_half_px: f32) -> f32 {
    let r = if id < 6 {
        cube_half_px * 0.92
    } else if id < 18 {
        cube_half_px * 0.38
    } else {
        cube_half_px * 0.28
    };
    r * r
}

// ── Pipeline ──────────────────────────────────────────────────────────────

pub struct ViewCubePipeline {
    pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    line_vertex_buffer: wgpu::Buffer,
    line_vertex_count: u32,
    ring_vertex_buffer: wgpu::Buffer,
    ring_index_buffer: wgpu::Buffer,
    ring_index_count: u32,
    ring_line_vertex_buffer: wgpu::Buffer,
    ring_line_vertex_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    ring_uniform_buffer: wgpu::Buffer,
    ring_uniform_bind_group: wgpu::BindGroup,
    depth_texture_size: Size<u32>,
    alloc_size: Size<u32>,
    depth_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_sampler: wgpu::Sampler,
    composite_bind_group: wgpu::BindGroup,
    composite_uniform_buffer: wgpu::Buffer,
    surface_format: wgpu::TextureFormat,
    pub cube_px: u32,
    text: ViewCubeText,
}

impl ViewCubePipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let (verts, idxs) = build_geometry();
        let line_verts = surface_edge_lines(&verts, &idxs);
        let (ring_verts, ring_idxs) = build_ring_geometry();
        let ring_line_verts = surface_edge_lines(&ring_verts, &ring_idxs);
        let cube_px = VIEWCUBE_PX;
        let vertex_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "vc.vb",
            &verts,
            wgpu::BufferUsages::VERTEX,
        );
        let index_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "vc.ib",
            &idxs,
            wgpu::BufferUsages::INDEX,
        );
        let line_vertex_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "vc.line_vb",
            &line_verts,
            wgpu::BufferUsages::VERTEX,
        );
        let ring_vertex_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "vc.ring_vb",
            &ring_verts,
            wgpu::BufferUsages::VERTEX,
        );
        let ring_index_buffer = super::gpu_upload::upload_buffer(
            device,
            queue,
            "vc.ring_ib",
            &ring_idxs,
            wgpu::BufferUsages::INDEX,
        );
        let ring_line_vertex_buffer =
            super::gpu_upload::upload_buffer(
                device,
                queue,
                "vc.ring_line_vb",
                &ring_line_verts,
                wgpu::BufferUsages::VERTEX,
            );
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vc.ub"),
            size: std::mem::size_of::<CubeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ring_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vc.ring_ub"),
            size: std::mem::size_of::<CubeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vc.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vc.bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let ring_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vc.ring_bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ring_uniform_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vc.layout"),
            bind_group_layouts: &[&bgl].map(Some),
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vc.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/viewcube.wgsl"
            ))),
        });
        let init_size = Size::new(1, 1);
        let depth_tex = create_depth_texture(device, init_size);
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa_tex = create_msaa_texture(device, init_size, format);
        let msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_tex = create_resolve_texture(device, init_size, format);
        let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vc.pipe"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[CubeVertex::desc()],
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
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: VIEWCUBE_MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vc.line_pipe"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("line_vs_main"),
                buffers: &[LineVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: VIEWCUBE_MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("line_fs_main"),
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
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vc.composite_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../shaders/viewcube_composite.wgsl"
            ))),
        });
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vc.composite_bgl"),
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
        let composite_uniform_buffer =
            super::gpu_upload::upload_buffer(
                device,
                queue,
                "vc.composite_uniform",
                &[1.0f32, 1.0, 0.0, 0.0],
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
        let composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vc.composite_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vc.composite_bg"),
            layout: &composite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&composite_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: composite_uniform_buffer.as_entire_binding(),
                },
            ],
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vc.composite_layout"),
            bind_group_layouts: &[&composite_bind_group_layout].map(Some),
            immediate_size: 0,
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vc.composite_pipe"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
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
                module: &composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let text = ViewCubeText::new(device, queue, format);
        Self {
            pipeline,
            line_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: idxs.len() as u32,
            line_vertex_buffer,
            line_vertex_count: line_verts.len() as u32,
            ring_vertex_buffer,
            ring_index_buffer,
            ring_index_count: ring_idxs.len() as u32,
            ring_line_vertex_buffer,
            ring_line_vertex_count: ring_line_verts.len() as u32,
            uniform_buffer,
            uniform_bind_group,
            ring_uniform_buffer,
            ring_uniform_bind_group,
            depth_texture_size: Size::new(1, 1),
            alloc_size: Size::new(0, 0),
            depth_view,
            msaa_view,
            resolve_view,
            composite_pipeline,
            composite_bind_group_layout,
            composite_sampler,
            composite_bind_group,
            composite_uniform_buffer,
            surface_format: format,
            cube_px,
            text,
        }
    }

    /// Upload using the quaternion rotation matrix.
    /// `cam_rotation` = `camera.view_rotation_mat()` = `Mat4::from_quat(camera.rotation)`.
    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        cam_rotation: Mat4,
        compass_rotation: Mat4,
        hover: Option<usize>,
        text_color: [f32; 4],
    ) {
        let render_size = VIEWCUBE_RENDER_PX.ceil() as u32;
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&CubeUniforms::new(
                cam_rotation,
                self.cube_px,
                render_size,
                render_size,
                hover,
            )),
        );
        queue.write_buffer(
            &self.ring_uniform_buffer,
            0,
            bytemuck::bytes_of(&CubeUniforms::new(
                compass_rotation,
                self.cube_px,
                render_size,
                render_size,
                None,
            )),
        );
        let alloc_w = self.alloc_size.width.max(1) as f32;
        let alloc_h = self.alloc_size.height.max(1) as f32;
        queue.write_buffer(
            &self.composite_uniform_buffer,
            0,
            bytemuck::cast_slice(&[
                self.depth_texture_size.width as f32 / alloc_w,
                self.depth_texture_size.height as f32 / alloc_h,
                0.0,
                0.0,
            ]),
        );
        self.text
            .update(
                queue,
                cam_rotation,
                compass_rotation,
                render_size,
                render_size,
                self.cube_px,
                text_color,
            );
    }

    pub fn ensure_depth_texture(&mut self, device: &wgpu::Device, size: Size<u32>) {
        self.depth_texture_size = size;
        let alloc = Size::new(
            round_up_viewcube_texture(size.width),
            round_up_viewcube_texture(size.height),
        );
        if self.alloc_size != alloc {
            let depth_tex = create_depth_texture(device, alloc);
            self.depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let msaa_tex = create_msaa_texture(device, alloc, self.surface_format);
            self.msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let resolve_tex = create_resolve_texture(device, alloc, self.surface_format);
            let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vc.composite_bg"),
                layout: &self.composite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&resolve_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.composite_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.composite_uniform_buffer.as_entire_binding(),
                    },
                ],
            });
            self.resolve_view = resolve_view;
            self.alloc_size = alloc;
        }
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: Rectangle<u32>,
    ) {
        let render_width = self.depth_texture_size.width.max(1);
        let render_height = self.depth_texture_size.height.max(1);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vc.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0.0, 0.0, render_width as f32, render_height as f32, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
        pass.set_bind_group(0, &self.ring_uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.ring_vertex_buffer.slice(..));
        pass.set_index_buffer(
            self.ring_index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..self.ring_index_count, 0, 0..1);
        pass.set_pipeline(&self.line_pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
        pass.draw(0..self.line_vertex_count, 0..1);
        pass.set_bind_group(0, &self.ring_uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.ring_line_vertex_buffer.slice(..));
        pass.draw(0..self.ring_line_vertex_count, 0..1);
        drop(pass);
        let local_clip = Rectangle {
            x: 0,
            y: 0,
            width: render_width,
            height: render_height,
        };
        self.text
            .render(encoder, &self.msaa_view, &self.depth_view, local_clip);
        {
            let _resolve = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vc.resolve_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&self.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let mut composite = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vc.composite_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let dest_width = render_width.min(clip.width);
        let dest_height = render_height.min(clip.height);
        composite.set_viewport(
            (clip.x + clip.width - dest_width) as f32,
            clip.y as f32,
            dest_width as f32,
            dest_height as f32,
            0.0,
            1.0,
        );
        composite.set_pipeline(&self.composite_pipeline);
        composite.set_bind_group(0, &self.composite_bind_group, &[]);
        composite.draw(0..6, 0..1);
    }
}

impl iced::widget::shader::Pipeline for ViewCubePipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::new(device, queue, format)
    }
}

fn create_depth_texture(device: &wgpu::Device, size: Size<u32>) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vc.depth_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: VIEWCUBE_MSAA_SAMPLES,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn create_msaa_texture(
    device: &wgpu::Device,
    size: Size<u32>,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vc.msaa_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: VIEWCUBE_MSAA_SAMPLES,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn create_resolve_texture(
    device: &wgpu::Device,
    size: Size<u32>,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vc.resolve_texture"),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn round_up_viewcube_texture(value: u32) -> u32 {
    const GRID: u32 = 128;
    ((value.max(1) + GRID - 1) / GRID) * GRID
}

// ── Hit test ──────────────────────────────────────────────────────────────
//
// hit_test and hover_id now take cam_rotation: Mat4 (same matrix the shader
// uses) so click regions always match what is drawn — including after free
// arcball orbit from any angle.

/// Returns the ViewCube region under screen position (mx, my), or None.
/// `cam_rotation` must be `camera.view_rotation_mat()`.
pub fn hit_test(
    mx: f32,
    my: f32,
    vp_w: f32,
    vp_h: f32,
    cam_rotation: Mat4,
    cube_px: u32,
) -> Option<CubeRegion> {
    let half = cube_px as f32 * VIEWCUBE_SCALE;
    let inset = half * NAV_INSET_F;
    let cx = vp_w - inset - VIEWCUBE_PAD;
    let cy = inset + VIEWCUBE_PAD;
    if (mx - cx).abs() > half || (my - cy).abs() > half {
        return None;
    }

    let (hw, hh) = (vp_w * 0.5, vp_h * 0.5);
    let vp = orthographic(-hw, hw, -hh, hh, -2000.0, 2000.0)
        * Mat4::from_translation(Vec3::new(
            hw - inset - VIEWCUBE_PAD,
            hh - inset - VIEWCUBE_PAD,
            0.0,
        ))
        * Mat4::from_scale(Vec3::splat(cube_px as f32 * VIEWCUBE_SCALE));

    let view_dir = Vec3::Z;
    let centroids = region_centroids();
    let (mut best, mut best_d) = (None, f32::MAX);

    for (id, &c) in centroids.iter().enumerate() {
        let world = cam_rotation.transform_point3(Vec3::from(c));
        if world.normalize().dot(view_dir) < 0.05 {
            continue;
        }
        let clip = vp * Vec4::new(world.x, world.y, world.z, 1.0);
        if clip.w.abs() < 1e-6 {
            continue;
        }
        let sx = (clip.x / clip.w + 1.0) * 0.5 * vp_w;
        let sy = (1.0 - clip.y / clip.w) * 0.5 * vp_h;
        let d = (sx - mx).powi(2) + (sy - my).powi(2);
        if d < threshold_sq(id, half) && d < best_d {
            best_d = d;
            best = Some(if id < 6 {
                CubeRegion::Face(id)
            } else if id < 18 {
                CubeRegion::Edge(id)
            } else {
                CubeRegion::Corner(id)
            });
        }
    }
    best
}

/// Returns the hovered region id (0-25), or None.
pub fn hover_id(
    mx: f32,
    my: f32,
    vp_w: f32,
    vp_h: f32,
    cam_rotation: Mat4,
    cube_px: u32,
) -> Option<usize> {
    hit_test(mx, my, vp_w, vp_h, cam_rotation, cube_px).map(|r| r.id())
}

impl Cardinal {
    /// The side-elevation face a compass letter snaps to when clicked.
    pub fn face_region(self) -> CubeRegion {
        CubeRegion::Face(match self {
            Cardinal::North => FACE_BACK,
            Cardinal::South => FACE_FRONT,
            Cardinal::East => FACE_RIGHT,
            Cardinal::West => FACE_LEFT,
        })
    }
}

/// Returns the compass cardinal under (mx, my), or None. Projects each of the
/// four ring letters and accepts the nearest within a small pixel radius — the
/// caller tries the cube body first, so this only fires out on the ring.
pub fn hit_test_cardinal(
    mx: f32,
    my: f32,
    vp_w: f32,
    vp_h: f32,
    cam_rotation: Mat4,
    cube_px: u32,
) -> Option<Cardinal> {
    let half = cube_px as f32 * VIEWCUBE_SCALE;
    let inset = half * NAV_INSET_F;
    let cx = vp_w - inset - VIEWCUBE_PAD;
    let cy = inset + VIEWCUBE_PAD;
    if (mx - cx).abs() > inset || (my - cy).abs() > inset {
        return None;
    }

    let (hw, hh) = (vp_w * 0.5, vp_h * 0.5);
    let vp = orthographic(-hw, hw, -hh, hh, -2000.0, 2000.0)
        * Mat4::from_translation(Vec3::new(
            hw - inset - VIEWCUBE_PAD,
            hh - inset - VIEWCUBE_PAD,
            0.0,
        ))
        * Mat4::from_scale(Vec3::splat(cube_px as f32 * VIEWCUBE_SCALE));

    let dirs = [
        (Cardinal::North, Vec3::new(0.0, 1.0, 0.0)),
        (Cardinal::East, Vec3::new(1.0, 0.0, 0.0)),
        (Cardinal::South, Vec3::new(0.0, -1.0, 0.0)),
        (Cardinal::West, Vec3::new(-1.0, 0.0, 0.0)),
    ];
    let thresh = (half * 0.34).powi(2);
    let (mut best, mut best_d) = (None, f32::MAX);
    for (card, dir) in dirs {
        let anchor = Vec3::new(dir.x * R_CARD, dir.y * R_CARD, RING_Z);
        let world = cam_rotation.transform_point3(anchor);
        let clip = vp * Vec4::new(world.x, world.y, world.z, 1.0);
        if clip.w.abs() < 1e-6 {
            continue;
        }
        let sx = (clip.x / clip.w + 1.0) * 0.5 * vp_w;
        let sy = (1.0 - clip.y / clip.w) * 0.5 * vp_h;
        let d = (sx - mx).powi(2) + (sy - my).powi(2);
        if d < thresh && d < best_d {
            best_d = d;
            best = Some(card);
        }
    }
    best
}
