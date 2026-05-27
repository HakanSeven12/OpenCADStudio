// GPU rendering primitives, shader::Program / shader::Primitive impls,
// and entity render-style helpers for the Scene.

use acadrust::tables::LineType;
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::{CadDocument, EntityType, Handle};
use glam::Mat4;
use iced::mouse;
use iced::widget::shader::{self, Viewport};
use iced::{Rectangle, Size};

use std::sync::Arc;

use super::pipeline::viewcube::{hover_id, VIEWCUBE_PX};
use super::pipeline::MultiPipeline;
use super::tess_util;
use super::{HatchModel, ImageModel, MeshLodSet, Scene, Uniforms, ViewportInstance, WireModel};

// ── PaperViewportPipeline / PaperViewportPrimitive ────────────────────────
//
// Newtype wrappers around MultiPipeline / Primitive so that the active-MSPACE
// viewport widget gets its own Iced storage entry (keyed by TypeId of the
// Pipeline type).  This prevents the shared-pipeline prepare() overwrite
// that occurs when PaperSheet and the viewport widget both use the same type.

/// Dedicated pipeline for the MSPACE active-viewport shader widget.
pub struct PaperViewportPipeline(pub(super) MultiPipeline);

impl iced::widget::shader::Pipeline for PaperViewportPipeline {
    fn new(
        device: &iced::wgpu::Device,
        queue: &iced::wgpu::Queue,
        format: iced::wgpu::TextureFormat,
    ) -> Self {
        Self(<MultiPipeline as iced::widget::shader::Pipeline>::new(
            device, queue, format,
        ))
    }
}

/// Primitive returned by `PaperViewportPane`; delegates everything to the
/// inner `Primitive` via the dedicated `PaperViewportPipeline`.
#[derive(Debug)]
pub struct PaperViewportPrimitive(pub(super) Primitive);

impl shader::Primitive for PaperViewportPrimitive {
    type Pipeline = PaperViewportPipeline;

    fn prepare(
        &self,
        pipeline: &mut PaperViewportPipeline,
        device: &iced::wgpu::Device,
        queue: &iced::wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        self.0
            .prepare(&mut pipeline.0, device, queue, bounds, viewport);
    }

    fn render(
        &self,
        pipeline: &PaperViewportPipeline,
        encoder: &mut iced::wgpu::CommandEncoder,
        target: &iced::wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        self.0.render(&pipeline.0, encoder, target, clip);
    }
}

// ── Camera hover state (shader::Program::State) ───────────────────────────

#[derive(Clone, Default)]
pub struct CameraState {
    pub hover_region: Option<usize>,
}

// ── GPU primitive ─────────────────────────────────────────────────────────

/// Normalized rectangle covering the whole widget — used for a single
/// full-window viewport (Model view or active paper viewport).
const FULL_VIEWPORT_RECT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

/// Everything needed to render one viewport: its geometry, camera, render
/// mode, and the screen rectangle it occupies. The unified renderer carries
/// a `Vec<ViewportData>` (one per tiled / floating viewport); each gets its
/// own inner `Pipeline` instance drawn into its own rectangle.
#[derive(Debug)]
pub struct ViewportData {
    pub(super) wires: Arc<Vec<WireModel>>,
    /// 3DFACE entity wires — separated so they are uploaded to the dedicated
    /// face3d pipeline (fill + batched edges) instead of N individual WireGpu.
    pub(super) face3d_wires: Arc<Vec<WireModel>>,
    pub(super) hatches: Arc<Vec<HatchModel>>,
    /// Wipeout fills — rendered in a separate pass AFTER wires.
    pub(super) wipeout_hatches: Arc<Vec<HatchModel>>,
    pub(super) images: Arc<Vec<ImageModel>>,
    pub(super) meshes: Arc<Vec<MeshLodSet>>,
    pub(super) uniforms: Uniforms,
    /// Camera rotation matrix derived from the quaternion.
    /// Used by the ViewCube pipeline — no gimbal lock.
    pub(super) cam_rotation: Mat4,
    pub(super) hover_region: Option<usize>,
    pub(super) show_viewcube: bool,
    /// Header.fill_mode (FILLMODE): when false, hatch / wipeout / face3d-fill
    /// uploads short-circuit so the renderer draws only wireframe.
    pub(super) fill_mode: bool,
    /// Per-view "Wireframe vs Solid" toggle. When `true`, 3D face fills
    /// are dropped on the upload path so 3D faces draw as edges only.
    /// Hatch / wipeout uploads are deliberately *not* gated by this flag —
    /// the user toggle should only affect 3D solids, not 2D fills.
    pub(super) view_wireframe: bool,
    /// Whether the active render mode wants 3D mesh fills uploaded. Off
    /// in `Wireframe2D` / `Wireframe3D`; on for every shaded variant. Set
    /// at the same point `view_wireframe` is computed so the two stay in
    /// lock-step for the gating logic in `prepare()`.
    pub(super) mesh_fill: bool,
    /// Whether the active render mode wants 3D mesh / face edges
    /// rendered on top of fills. Most shaded modes turn this off; the
    /// `*WithEdges` variants and the pure wireframes leave it on.
    pub(super) show_3d_edges: bool,
    /// HiddenLine routes 3D fills through a depth-only prepass so edges
    /// occluded by closer geometry are culled by the LessEqual depth
    /// test on the wire passes that follow.
    pub(super) hidden_line: bool,
    pub(super) geometry_epoch: u64,
    /// Camera generation captured when this Primitive was assembled. Paired
    /// with `geometry_epoch` so the wire buffers re-upload when the view
    /// changes (frustum culling produces a different wire list).
    pub(super) camera_generation: u64,
    /// Screen rectangle this viewport fills, **normalized** to the widget
    /// bounds (each component in 0..1). A single full-widget view is
    /// `(0, 0, 1, 1)`; tiled / floating viewports are sub-rectangles.
    /// Normalized form lets `render()` derive the physical sub-clip from
    /// the surface clip without needing the scale factor.
    pub(super) screen_rect: Rectangle,
}

#[derive(Debug)]
pub struct Primitive {
    /// One entry per viewport drawn this frame (≥1).
    pub(super) viewports: Vec<ViewportData>,
    /// Background color used to clear each viewport's MSAA buffer.
    pub(super) bg_color: [f32; 4],
}

/// Flags the render pipeline consumes, derived from
/// [`acadrust::entities::ViewportRenderMode`]. Each shaded variant fills
/// 3D faces and meshes; the pure wireframes drop the fill and keep only
/// edges. `*WithEdges` variants render both. HiddenLine uses a depth
/// prepass: face/mesh fills are uploaded but routed through depth-only
/// pipelines so hidden edges drop out. `FlatShaded` vs `GouraudShaded`
/// differ in shader uniform only and produce identical fill flags here.
#[derive(Clone, Copy, Debug)]
pub struct RenderModeFlags {
    pub face3d_fill: bool,
    pub mesh_fill: bool,
    pub show_3d_edges: bool,
    pub hidden_line: bool,
    /// `true` for FlatShaded / FlatShadedWithEdges. The mesh shader
    /// reads `Uniforms.flat_shade` and replaces the smooth per-vertex
    /// normal with a per-triangle face normal so each triangle reads
    /// as a single tone.
    pub flat_shade: bool,
}

pub fn render_mode_flags(
    mode: acadrust::entities::ViewportRenderMode,
) -> RenderModeFlags {
    use acadrust::entities::ViewportRenderMode as M;
    match mode {
        M::Wireframe2D | M::Wireframe3D => RenderModeFlags {
            face3d_fill: false,
            mesh_fill: false,
            show_3d_edges: true,
            hidden_line: false,
            flat_shade: false,
        },
        M::HiddenLine => RenderModeFlags {
            face3d_fill: true,
            mesh_fill: true,
            show_3d_edges: true,
            hidden_line: true,
            flat_shade: false,
        },
        M::FlatShaded => RenderModeFlags {
            face3d_fill: true,
            mesh_fill: true,
            show_3d_edges: false,
            hidden_line: false,
            flat_shade: true,
        },
        M::GouraudShaded => RenderModeFlags {
            face3d_fill: true,
            mesh_fill: true,
            show_3d_edges: false,
            hidden_line: false,
            flat_shade: false,
        },
        M::FlatShadedWithEdges => RenderModeFlags {
            face3d_fill: true,
            mesh_fill: true,
            show_3d_edges: true,
            hidden_line: false,
            flat_shade: true,
        },
        M::GouraudShadedWithEdges => RenderModeFlags {
            face3d_fill: true,
            mesh_fill: true,
            show_3d_edges: true,
            hidden_line: false,
            flat_shade: false,
        },
    }
}

// ── shader::Primitive impl ────────────────────────────────────────────────

impl shader::Primitive for Primitive {
    type Pipeline = MultiPipeline;

    fn prepare(
        &self,
        pipeline: &mut MultiPipeline,
        device: &iced::wgpu::Device,
        queue: &iced::wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let phys = viewport.physical_size();
        let full_size = Size::new(phys.width, phys.height);
        let scale = viewport.scale_factor() as f32;
        pipeline.ensure_len(device, queue, self.viewports.len());

        for (i, vp) in self.viewports.iter().enumerate() {
            let inner = &mut pipeline.inners[i];
            // Physical MSAA/depth size for this viewport's sub-rect.
            let clip_size = Size::new(
                (vp.screen_rect.width * bounds.width * scale).ceil().max(1.0) as u32,
                (vp.screen_rect.height * bounds.height * scale).ceil().max(1.0) as u32,
            );
            inner.ensure_depth_texture(device, clip_size);
            inner.viewcube.ensure_depth_texture(device, full_size);
            inner.upload_uniforms(queue, &vp.uniforms);
            let cur_key = (vp.geometry_epoch, vp.camera_generation);
            let fill_mode = vp.fill_mode;
            // 3D face fill requires *both* the doc-level FILLMODE *and* the
            // per-view Solid toggle. Hatches / wipeouts deliberately ignore
            // the view toggle so 2D fills stay on even when the user picks
            // the Wireframe overlay style.
            let face3d_fill_active = fill_mode && !vp.view_wireframe;
            if cur_key != inner.cached_epoch {
                // Static buffers (hatches/images/meshes) only need refresh on
                // a real geometry change, not on every camera tick.
                if vp.geometry_epoch != inner.cached_epoch.0 {
                    if fill_mode {
                        inner.upload_hatches(device, &vp.hatches[..]);
                        inner.upload_wipeouts(device, &vp.wipeout_hatches[..]);
                    } else {
                        inner.upload_hatches(device, &[]);
                        inner.upload_wipeouts(device, &[]);
                    }
                    inner.upload_images(device, queue, &vp.images[..]);
                    inner.upload_meshes(device, &vp.meshes[..]);
                }
                // Wires re-upload on every camera change because the visible
                // subset shifts under frustum culling.
                inner.upload_wires(device, &vp.wires[..]);
                inner.upload_face3d(
                    device,
                    &vp.face3d_wires[..],
                    &vp.wires[..],
                    !face3d_fill_active,
                );
                inner.cached_epoch = cur_key;
            }
            let vproj = vp.uniforms.view_proj;
            inner.compute_wire_scissors(vproj, clip_size.width, clip_size.height);
            inner.compute_wipeout_scissors(vproj, clip_size.width, clip_size.height);
            inner.compute_image_scissors(vproj, clip_size.width, clip_size.height);
            inner.compute_hatch_lod(queue, vproj, clip_size.width, clip_size.height);
            inner.compute_wipeout_lod(vproj, clip_size.width, clip_size.height);
            inner.compute_mesh_lod(vproj, clip_size.width, clip_size.height);
            if vp.show_viewcube {
                inner.viewcube.upload(
                    queue,
                    vp.cam_rotation,
                    (vp.screen_rect.width * bounds.width) as u32,
                    (vp.screen_rect.height * bounds.height) as u32,
                    vp.hover_region,
                );
            }
        }
    }

    fn render(
        &self,
        pipeline: &MultiPipeline,
        encoder: &mut iced::wgpu::CommandEncoder,
        target: &iced::wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        for (i, vp) in self.viewports.iter().enumerate() {
            let Some(inner) = pipeline.inners.get(i) else {
                break;
            };
            // Derive this viewport's physical sub-clip from the normalized
            // screen_rect and the surface clip.
            let cw = clip.width as f32;
            let ch = clip.height as f32;
            let vp_clip = Rectangle {
                x: clip.x + (vp.screen_rect.x * cw) as u32,
                y: clip.y + (vp.screen_rect.y * ch) as u32,
                width: (vp.screen_rect.width * cw).max(1.0) as u32,
                height: (vp.screen_rect.height * ch).max(1.0) as u32,
            };
            // `mesh_fill` is false for Wireframe 2D / Wireframe 3D — flip
            // the draw path so meshes use the wireframe pipeline + the
            // pre-built triangle-edge index buffer.
            let mesh_wireframe = !vp.mesh_fill;
            inner.render(
                encoder,
                target,
                vp_clip,
                self.bg_color,
                mesh_wireframe,
                vp.hidden_line,
                vp.show_3d_edges,
            );
            if vp.show_viewcube {
                inner.viewcube.render(encoder, target, vp_clip);
            }
        }
    }
}

// ── Render-style helpers (impl Scene) ────────────────────────────────────

impl Scene {
    /// Returns (entity_color, pattern_length, pattern, line_weight_px, aci).
    pub(super) fn render_style(&self, e: &EntityType) -> ([f32; 4], f32, [f32; 8], f32, u8) {
        let (color, pl, pat, lw, aci) = render_style_for(&self.document, e);
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        (adapt_to_bg(color, bg), pl, pat, lw, aci)
    }
}

// ── Document-only render-style helpers (no &self, safe to call from parallel contexts) ──

/// Resolves the effective linetype name for an entity, falling back to the
/// layer's linetype when the entity's own linetype is "ByLayer".
pub(super) fn linetype_name_for<'a>(document: &'a CadDocument, e: &'a EntityType) -> &'a str {
    let elt = &e.common().linetype;
    if elt.is_empty() || elt.eq_ignore_ascii_case("bylayer") {
        document
            .layers
            .get(&e.common().layer)
            .map(|l| l.line_type.as_str())
            .unwrap_or("Continuous")
    } else {
        elt.as_str()
    }
}

/// Returns `(entity_color, pattern_length, pattern, line_weight_px, aci)` for
/// an entity, resolving ByLayer color and linetype from the document.
pub(super) fn render_style_for(
    document: &CadDocument,
    e: &EntityType,
) -> ([f32; 4], f32, [f32; 8], f32, u8) {
    let layer_name = &e.common().layer;
    let (entity_color, aci) = {
        let ec = &e.common().color;
        let resolved = if *ec == AcadColor::ByLayer {
            document
                .layers
                .get(layer_name)
                .map(|l| &l.color)
                .unwrap_or(&AcadColor::WHITE)
        } else {
            ec
        };
        let aci = match resolved {
            AcadColor::Index(i) => *i,
            _ => 0,
        };
        let [r, g, b, _] = tess_util::aci_to_rgba(resolved);
        let alpha = 1.0 - e.common().transparency.as_percent() as f32;
        ([r, g, b, alpha], aci)
    };

    let lt_name = linetype_name_for(document, e);
    // Effective scale = global LTSCALE × per-entity scale (both default to 1.0).
    let lt_scale = document.header.linetype_scale as f32 * e.common().linetype_scale as f32;
    let (pattern_length, pattern) = resolve_pattern(&document.line_types, lt_name, lt_scale);

    let line_weight_px = {
        // LWDISPLAY is no longer evaluated here — the toggle is now applied in
        // the wire shader via `Uniforms.lwdisplay_enable`, so we always bake the
        // entity's resolved (layer-inherited) weight. Toggling lineweight
        // visibility costs only a uniform write, not a retessellate.
        let ew = &e.common().line_weight;
        let resolved = match ew {
            LineWeight::ByLayer | LineWeight::ByBlock | LineWeight::Default => document
                .layers
                .get(layer_name)
                .map(|l| &l.line_weight)
                .unwrap_or(&LineWeight::Default),
            _ => ew,
        };
        const MM_TO_PX: f32 = 96.0 / 25.4;
        resolved
            .millimeters()
            .map(|mm| (mm as f32 * MM_TO_PX).max(1.0))
            .unwrap_or(1.0)
    };

    (entity_color, pattern_length, pattern, line_weight_px, aci)
}

/// Like `render_style_for` but resolves ByBlock properties by inheriting from
/// the INSERT entity's already-resolved style. Call this for exploded block
/// sub-entities so that ByBlock color/linetype/lineweight propagate correctly.
pub(crate) fn render_style_for_block_sub(
    document: &CadDocument,
    e: &EntityType,
    insert_color: [f32; 4],
    insert_pat_len: f32,
    insert_pat: [f32; 8],
    insert_lw_px: f32,
) -> ([f32; 4], f32, [f32; 8], f32, u8) {
    let (color, pat_len, pat, lw_px, aci) = render_style_for(document, e);
    let common = e.common();

    let final_color = if common.color == AcadColor::ByBlock {
        insert_color
    } else {
        color
    };

    let (final_pat_len, final_pat) = if common.linetype.eq_ignore_ascii_case("byblock") {
        (insert_pat_len, insert_pat)
    } else {
        (pat_len, pat)
    };

    let final_lw = if matches!(common.line_weight, LineWeight::ByBlock) {
        insert_lw_px
    } else {
        lw_px
    };

    (final_color, final_pat_len, final_pat, final_lw, aci)
}

/// Adapt white→black or black→white based on background luminance.
/// White entities on light backgrounds become black, black entities on dark
/// backgrounds become white. All other colors pass through unchanged.
pub(crate) fn adapt_to_bg(color: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
    let lum = 0.299 * bg[0] + 0.587 * bg[1] + 0.114 * bg[2];
    let is_white = color[0] > 0.95 && color[1] > 0.95 && color[2] > 0.95;
    let is_black = color[0] < 0.05 && color[1] < 0.05 && color[2] < 0.05;
    if is_white && lum > 0.5 {
        [0.0, 0.0, 0.0, color[3]]
    } else if is_black && lum <= 0.5 {
        [1.0, 1.0, 1.0, color[3]]
    } else {
        color
    }
}

// ── Primitive builder helpers (called by ViewportPane's shader::Program impl) ──

impl Scene {
    /// Build a full-scene Primitive for the model or paper view (the camera
    /// stored in `self.camera` is used as-is).
    pub(super) fn build_primitive(
        &self,
        hover_region: Option<usize>,
        bounds: Rectangle,
        show_viewcube: bool,
        render_mode: acadrust::entities::ViewportRenderMode,
    ) -> Primitive {
        let flags = render_mode_flags(render_mode);
        let view_wireframe = !flags.face3d_fill;
        let cam = self.camera.borrow();
        self.selection.borrow_mut().vp_size = (bounds.width, bounds.height);
        // Record the active widget's aspect so view_world_aabb() can compute
        // a correct culling rectangle before entity_wires_arc() runs.
        if bounds.height > 0.0 {
            self.set_render_aspect(bounds.width / bounds.height);
            self.set_render_pixel_scale(bounds.width, bounds.height);
        }

        let entity_arc = self.entity_wires_arc();
        let (face3d_wires, other_wires) = split_face3d_wires(&entity_arc, &self.document);
        let all_wires = if self.interim_wire.is_none() && self.preview_wires.is_empty() {
            Arc::new(other_wires)
        } else {
            let mut v = other_wires;
            if let Some(iw) = &self.interim_wire {
                v.push(iw.clone());
            }
            v.extend(self.preview_wires.iter().cloned());
            Arc::new(v)
        };

        let bg_color = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };

        let mut uniforms = Uniforms::new(&cam, bounds, self.document.header.lineweight_display);
        uniforms.flat_shade = if flags.flat_shade { 1.0 } else { 0.0 };

        let data = ViewportData {
            wires: all_wires,
            face3d_wires: Arc::new(face3d_wires),
            hatches: self.hatch_models_arc(),
            wipeout_hatches: self.wipeout_models_arc(),
            images: self.images_arc(),
            meshes: self.meshes_arc(),
            uniforms,
            cam_rotation: cam.view_rotation_mat(),
            hover_region,
            show_viewcube,
            fill_mode: self.document.header.fill_mode,
            view_wireframe,
            mesh_fill: flags.mesh_fill,
            show_3d_edges: flags.show_3d_edges,
            hidden_line: flags.hidden_line,
            geometry_epoch: self.geometry_epoch,
            camera_generation: self.camera_generation,
            screen_rect: FULL_VIEWPORT_RECT,
        };
        Primitive {
            viewports: vec![data],
            bg_color,
        }
    }

    /// Build the unified multi-viewport `Primitive` for the current layout.
    /// Model layout → one full-window viewport (more once tiled); paper
    /// layout → one viewport per floating content viewport. Each entry is
    /// rendered into its own screen rectangle by its own inner pipeline.
    pub(super) fn build_viewports(
        &self,
        bounds: Rectangle,
        model_render_mode: acadrust::entities::ViewportRenderMode,
        hover_region: Option<usize>,
    ) -> Primitive {
        self.selection.borrow_mut().vp_size = (bounds.width, bounds.height);
        if bounds.height > 0.0 {
            self.set_render_aspect(bounds.width / bounds.height);
            self.set_render_pixel_scale(bounds.width, bounds.height);
        }
        let canvas = (bounds.width.max(1.0), bounds.height.max(1.0));
        let instances = self.active_viewports(canvas.0, canvas.1, model_render_mode);
        let bg_color = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let viewports: Vec<ViewportData> = instances
            .iter()
            .map(|inst| self.viewport_data_for(inst, canvas, hover_region))
            .collect();
        Primitive {
            viewports: if viewports.is_empty() {
                // Paper layout with no content viewports still needs a
                // (blank) pass so the widget clears to the sheet colour.
                vec![self.viewport_data_for(
                    &ViewportInstance {
                        handle: acadrust::Handle::NULL,
                        screen_rect: Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: canvas.0,
                            height: canvas.1,
                        },
                        camera: self.camera.borrow().clone(),
                        render_mode: model_render_mode,
                        active: false,
                    },
                    canvas,
                    None,
                )]
            } else {
                viewports
            },
            bg_color,
        }
    }

    /// Build one `ViewportData` from a `ViewportInstance`: gathers the
    /// viewport's geometry (full model for the Model view / `Handle::NULL`,
    /// or the layer-frozen subset for a paper viewport), its camera
    /// uniforms, and the normalized screen rectangle.
    fn viewport_data_for(
        &self,
        inst: &ViewportInstance,
        canvas: (f32, f32),
        hover_region: Option<usize>,
    ) -> ViewportData {
        let flags = render_mode_flags(inst.render_mode);
        let view_wireframe = !flags.face3d_fill;

        let base_arc = if inst.handle == acadrust::Handle::NULL {
            self.entity_wires_arc()
        } else {
            self.model_wires_for_viewport_arc(inst.handle)
        };
        let (face3d_wires, other_wires) = split_face3d_wires(&base_arc, &self.document);
        let all_wires = if self.interim_wire.is_none() && self.preview_wires.is_empty() {
            Arc::new(other_wires)
        } else {
            let mut v = other_wires;
            if let Some(iw) = &self.interim_wire {
                v.push(iw.clone());
            }
            v.extend(self.preview_wires.iter().cloned());
            Arc::new(v)
        };

        // Camera uniforms use the viewport's pixel size for aspect / ortho.
        let bounds_px = Rectangle {
            x: 0.0,
            y: 0.0,
            width: inst.screen_rect.width.max(1.0),
            height: inst.screen_rect.height.max(1.0),
        };
        let mut uniforms =
            Uniforms::new(&inst.camera, bounds_px, self.document.header.lineweight_display);
        uniforms.flat_shade = if flags.flat_shade { 1.0 } else { 0.0 };

        let screen_rect = Rectangle {
            x: inst.screen_rect.x / canvas.0,
            y: inst.screen_rect.y / canvas.1,
            width: inst.screen_rect.width / canvas.0,
            height: inst.screen_rect.height / canvas.1,
        };

        ViewportData {
            wires: all_wires,
            face3d_wires: Arc::new(face3d_wires),
            hatches: self.hatch_models_arc(),
            wipeout_hatches: self.wipeout_models_arc(),
            images: self.images_arc(),
            meshes: self.meshes_arc(),
            uniforms,
            cam_rotation: inst.camera.view_rotation_mat(),
            // Only the active viewport gets the hovered-region highlight.
            hover_region: if inst.active { hover_region } else { None },
            show_viewcube: inst.active,
            fill_mode: self.document.header.fill_mode,
            view_wireframe,
            mesh_fill: flags.mesh_fill,
            show_3d_edges: flags.show_3d_edges,
            hidden_line: flags.hidden_line,
            geometry_epoch: self.geometry_epoch,
            camera_generation: self.camera_generation,
            screen_rect,
        }
    }

    /// Build a Primitive that renders model-space content through a specific
    /// paper-space viewport's camera, applying its layer-freeze list. The
    /// render mode is read from the viewport entity itself (each paper-space
    /// viewport carries its own visual style) — the model-space pick_list
    /// only governs the Model layout.
    pub(super) fn build_viewport_primitive(
        &self,
        vp_handle: Handle,
        hover_region: Option<usize>,
        bounds: Rectangle,
        show_viewcube: bool,
    ) -> Primitive {
        let render_mode = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp.render_mode,
            _ => acadrust::entities::ViewportRenderMode::Wireframe2D,
        };
        let flags = render_mode_flags(render_mode);
        let view_wireframe = !flags.face3d_fill;
        let cam = match self.camera_for_viewport(vp_handle) {
            Some(c) => c,
            None => return self.build_primitive(hover_region, bounds, false, render_mode),
        };

        let base_arc = self.model_wires_for_viewport_arc(vp_handle);
        let (face3d_wires, other_wires) = split_face3d_wires(&base_arc, &self.document);
        let all_wires = if self.interim_wire.is_none() && self.preview_wires.is_empty() {
            Arc::new(other_wires)
        } else {
            let mut v = other_wires;
            if let Some(iw) = &self.interim_wire {
                v.push(iw.clone());
            }
            v.extend(self.preview_wires.iter().cloned());
            Arc::new(v)
        };

        let mut uniforms = Uniforms::new(&cam, bounds, self.document.header.lineweight_display);
        uniforms.flat_shade = if flags.flat_shade { 1.0 } else { 0.0 };

        let data = ViewportData {
            wires: all_wires,
            face3d_wires: Arc::new(face3d_wires),
            hatches: self.hatch_models_arc(),
            wipeout_hatches: self.wipeout_models_arc(),
            images: self.images_arc(),
            meshes: self.meshes_arc(),
            uniforms,
            cam_rotation: cam.view_rotation_mat(),
            hover_region,
            show_viewcube,
            fill_mode: self.document.header.fill_mode,
            view_wireframe,
            mesh_fill: flags.mesh_fill,
            show_3d_edges: flags.show_3d_edges,
            hidden_line: flags.hidden_line,
            geometry_epoch: self.geometry_epoch,
            camera_generation: self.camera_generation,
            screen_rect: FULL_VIEWPORT_RECT,
        };
        Primitive {
            viewports: vec![data],
            bg_color: self.bg_color,
        }
    }

    /// Wrap `build_viewport_primitive()` in `PaperViewportPrimitive` for use
    /// by `PaperViewportPane`, which needs its own dedicated pipeline type.
    pub(super) fn build_active_viewport_primitive(
        &self,
        vp_handle: Handle,
        hover_region: Option<usize>,
        bounds: Rectangle,
    ) -> PaperViewportPrimitive {
        // The active (double-clicked) viewport shows the ViewCube gizmo,
        // mirroring the model-space view.
        PaperViewportPrimitive(
            self.build_viewport_primitive(vp_handle, hover_region, bounds, true),
        )
    }

    /// Update viewcube hover state from cursor position within `bounds`.
    pub(super) fn update_viewcube_state(
        &self,
        state: &mut CameraState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) {
        let pos = cursor.position_in(bounds);
        let cam_rotation = self.camera.borrow().view_rotation_mat();
        if let Some(p) = pos {
            state.hover_region = hover_id(
                p.x,
                p.y,
                bounds.width,
                bounds.height,
                cam_rotation,
                VIEWCUBE_PX,
            );
        } else {
            state.hover_region = None;
        }
    }

    pub(super) fn viewcube_mouse_interaction(&self, state: &CameraState) -> mouse::Interaction {
        if state.hover_region.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

// ── Linetype pattern helper ───────────────────────────────────────────────

pub(crate) fn resolve_pattern(
    table: &acadrust::tables::Table<LineType>,
    name: &str,
    scale: f32,
) -> (f32, [f32; 8]) {
    let solid = (0.0, [0.0f32; 8]);
    if name.eq_ignore_ascii_case("continuous")
        || name.eq_ignore_ascii_case("bylayer")
        || name.eq_ignore_ascii_case("byblock")
        || name.is_empty()
    {
        return solid;
    }
    let lt = match table.get(name) {
        Some(lt) => lt,
        None => return solid,
    };
    if lt.is_continuous() || lt.elements.is_empty() {
        return solid;
    }

    let mut pat = [0.0f32; 8];
    let mut pat_len = 0.0f32;
    for (i, el) in lt.elements.iter().take(8).enumerate() {
        let raw = el.length as f32 * scale;
        let encoded = if raw == 0.0 {
            0.01 * scale.max(0.01)
        } else {
            raw
        };
        pat[i] = encoded;
        pat_len += encoded.abs();
    }
    if pat_len < 1e-6 {
        return solid;
    }
    (pat_len, pat)
}

/// Partition a wire list into (face3d_wires, other_wires).
///
/// Uses a document handle lookup so no changes to WireModel are needed.
/// O(N) per geometry epoch — acceptable since it runs once per epoch.
fn split_face3d_wires(
    wires: &[WireModel],
    document: &acadrust::CadDocument,
) -> (Vec<WireModel>, Vec<WireModel>) {
    let mut face3d = Vec::new();
    let mut others = Vec::new();
    for w in wires {
        let is_face3d = w
            .name
            .parse::<u64>()
            .ok()
            .and_then(|v| document.get_entity(Handle::new(v)))
            .map(|e| matches!(e, EntityType::Face3D(_)))
            .unwrap_or(false);
        if is_face3d {
            face3d.push(w.clone());
        } else {
            others.push(w.clone());
        }
    }
    (face3d, others)
}
