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

use crate::scene::pipeline::viewcube::{hover_id, VIEWCUBE_PX};
use crate::scene::pipeline::MultiPipeline;
use crate::scene::convert::tess_util;
use crate::scene::{
    vp_effective_scale, HatchModel, ImageModel, MeshLodSet, NavPerfSample, Scene, SceneLight,
    Uniforms, ViewportInstance, WireModel,
};

// ── Camera hover state (shader::Program::State) ───────────────────────────

#[derive(Clone, Default)]
pub struct CameraState {
    pub hover_region: Option<usize>,
}

// ── GPU primitive ─────────────────────────────────────────────────────────

/// Everything needed to render one viewport: its geometry, camera, render
/// mode, and the screen rectangle it occupies. The unified renderer carries
/// a `Vec<ViewportData>` (one per tiled / floating viewport); each gets its
/// own inner `Pipeline` instance drawn into its own rectangle.
#[derive(Debug)]
pub struct ViewportData {
    /// Stable identity of the source viewport (its entity handle / tile index /
    /// sheet role). The renderer addresses pipeline slots by list index but
    /// drops off-canvas viewports, so this lets the slot detect when it has been
    /// reused by a different viewport and reset its (index-addressed) caches.
    pub(in crate::scene) instance_id: u64,
    /// Weak resident source. Scene owns the strong Arc; retaining another one
    /// in the previous shader Primitive prevented the next UI event from
    /// splicing a changed entity in place and forced a full rebuild.
    pub(in crate::scene) wires: std::sync::Weak<Vec<WireModel>>,
    /// This content viewport's non-rectangular clip boundary (paper layouts
    /// only), as a polygon already projected into the viewport's render-target
    /// NDC. The GPU stamps it into the stencil so content is clipped to the
    /// shape. Empty for rectangular viewports, the paper sheet, and Model space.
    pub(in crate::scene) clip_boundary_ndc: Arc<Vec<[f32; 2]>>,
    /// Live command-preview / interim / grip-drag overlay wires. Kept out of
    /// the main `wires` buffer so a drag re-uploads only this small set each
    /// frame, never the resident base buffer. Drawn on top in the wire pass.
    pub(in crate::scene) preview_wires: Arc<Vec<WireModel>>,
    /// 3DFACE entity wires — separated so they are uploaded to the dedicated
    /// face3d pipeline (fill + batched edges) instead of N individual WireGpu.
    pub(in crate::scene) face3d_wires: Arc<Vec<WireModel>>,
    /// SDF text-quad vertices (Phase 2b). Empty unless `OCS_TEXT_SDF` is set.
    pub(in crate::scene) text_verts: Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>,
    /// Live grip-drag / command-preview glyph quads. Kept out of the epoch-cached
    /// `text_verts` and uploaded to a per-frame buffer, so text dragged by a grip
    /// stays visible even though it's hidden from the base text set (issue #316).
    pub(in crate::scene) preview_text_verts:
        Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>>,
    /// Per-entity normalized draw-order depth (handle.value() → (0,1)), used
    /// by the wire / face3d pipelines as a clip-z bias. WireModels carry no
    /// depth field (84 construction sites); the bias is looked up by handle
    /// at GPU-upload time from this map instead.
    pub(in crate::scene) draw_depths: Arc<rustc_hash::FxHashMap<u64, [f32; 2]>>,
    pub(in crate::scene) hatches: Arc<Vec<HatchModel>>,
    /// Wipeout fills — rendered in a separate pass AFTER wires.
    pub(in crate::scene) wipeout_hatches: Arc<Vec<HatchModel>>,
    pub(in crate::scene) images: Arc<Vec<ImageModel>>,
    pub(in crate::scene) meshes: Arc<Vec<MeshLodSet>>,
    pub(in crate::scene) uniforms: Uniforms,
    /// World-space camera forward — the parallel view direction. DISPSILH
    /// silhouettes use this alone (not the eye position), so the outline follows
    /// the view angle and doesn't shift with pan / perspective foreshortening.
    pub(in crate::scene) view_dir: glam::Vec3,
    /// Camera rotation matrix derived from the quaternion.
    /// Used by the ViewCube pipeline — no gimbal lock.
    pub(in crate::scene) cam_rotation: Mat4,
    /// Camera-only rotation (no UCS) for the world-fixed compass cardinals, so
    /// N/E/S/W stay aligned to world even as the cube reorients with the UCS.
    pub(in crate::scene) compass_rotation: Mat4,
    pub(in crate::scene) hover_region: Option<usize>,
    pub(in crate::scene) show_viewcube: bool,
    /// Header.fill_mode (FILLMODE): when false, hatch / wipeout / face3d-fill
    /// uploads short-circuit so the renderer draws only wireframe.
    pub(in crate::scene) fill_mode: bool,
    /// Per-view "Wireframe vs Solid" toggle. When `true`, 3D face fills
    /// are dropped on the upload path so 3D faces draw as edges only.
    /// Hatch / wipeout uploads are deliberately *not* gated by this flag —
    /// the user toggle should only affect 3D solids, not 2D fills.
    pub(in crate::scene) view_wireframe: bool,
    /// Whether the active render mode wants 3D mesh fills uploaded. Off
    /// in `Wireframe2D` / `Wireframe3D`; on for every shaded variant. Set
    /// at the same point `view_wireframe` is computed so the two stay in
    /// lock-step for the gating logic in `prepare()`.
    pub(in crate::scene) mesh_fill: bool,
    /// Whether the active render mode wants 3D mesh / face edges
    /// rendered on top of fills. Most shaded modes turn this off; the
    /// `*WithEdges` variants and the pure wireframes leave it on.
    pub(in crate::scene) show_3d_edges: bool,
    /// DISPSILH — draw view-dependent silhouette outlines on curved solid faces.
    /// A document-header global (default off), orthogonal to the visual style.
    pub(in crate::scene) display_silhouette: bool,
    /// HiddenLine routes 3D fills through a depth-only prepass so edges
    /// occluded by closer geometry are culled by the LessEqual depth
    /// test on the wire passes that follow.
    pub(in crate::scene) hidden_line: bool,
    /// Interaction LOD: when true the (per-pixel, GPU-dominating) hatch pass is
    /// skipped this frame because the view is actively being navigated. Folded
    /// into the render signature so the settle frame re-renders hatches once and
    /// the scene-render cache holds it. See [`Scene::navigating_lod`].
    pub(in crate::scene) skip_hatch: bool,
    pub(in crate::scene) geometry_epoch: u64,
    /// Camera generation captured when this Primitive was assembled. Paired
    /// with `geometry_epoch` so the per-frame scissor / LOD recompute runs.
    pub(in crate::scene) camera_generation: u64,
    /// Content id of `wires`. Stable across camera moves (the Model wire set is
    /// held static), so `prepare` skips re-uploading the world-space wire buffer
    /// when only the camera moved. Non-tile and preview/interim frames carry a
    /// fresh id each time → always re-upload.
    pub(in crate::scene) wire_content_id: u64,
    /// GPU wire-arena handoff (`OCS_WIRE_GPU_PATCH`): `(prev_gen, changed)` when
    /// this Model set reached `wire_content_id` by an incremental resident patch,
    /// so `prepare` can patch just those entities' slabs. `None` ⇒ full build.
    pub(in crate::scene) wire_patch: Option<(u64, Arc<crate::scene::WireGpuPatch>)>,
    /// Selected handles only (no hover) — solid meshes tint these blue.
    pub(in crate::scene) selected_handles: Arc<rustc_hash::FxHashSet<acadrust::Handle>>,
    /// Currently hovered handle — solid meshes tint it orange.
    pub(in crate::scene) hover_handle: Option<acadrust::Handle>,
    /// Bumped on selection / hover change. Paired with `wire_content_id` to
    /// decide when the xray overlay batch needs rebuilding.
    pub(in crate::scene) selection_generation: u64,
    /// Signature of the *selected set* only (not hover). Gates the static-buffer
    /// re-upload (hatch tint, issue #71) so a hover doesn't re-upload every
    /// hatch / face3d buffer on hatch-heavy drawings.
    pub(in crate::scene) selected_sig: u64,
    /// Screen rectangle this viewport fills, **normalized** to the widget
    /// bounds (each component in 0..1). A single full-widget view is
    /// `(0, 0, 1, 1)`; tiled / floating viewports are sub-rectangles.
    /// Normalized form lets `render()` derive the physical sub-clip from
    /// the surface clip without needing the scale factor.
    pub(in crate::scene) screen_rect: Rectangle,
}

#[derive(Debug)]
pub struct Primitive {
    /// One entry per viewport drawn this frame (≥1).
    pub(in crate::scene) viewports: Vec<ViewportData>,
    /// Background color used to clear each viewport's MSAA buffer.
    pub(in crate::scene) bg_color: [f32; 4],
    /// Active Iced theme text colour for GPU-rendered ViewCube labels.
    pub(in crate::scene) viewcube_text_color: [f32; 4],
    /// One input-to-render sample, carried only when PERF tracing is enabled.
    pub(in crate::scene) nav_perf: Option<NavPerfSample>,
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
        let nav_prepare_started = iced::time::Instant::now();
        let phys = viewport.physical_size();
        let full_size = Size::new(phys.width, phys.height);
        let scale = viewport.scale_factor() as f32;
        let instance_ids: Vec<u64> = self.viewports.iter().map(|vp| vp.instance_id).collect();
        let slots = pipeline.resolve_slots(device, queue, &instance_ids);

        for (i, vp) in self.viewports.iter().enumerate() {
            let inner = &mut pipeline.inners[slots[i]];
            // Pipeline slots are addressed by list index, but off-canvas
            // viewports are dropped from the list — so a slot can be reused by a
            // DIFFERENT viewport across frames (e.g. the first viewport scrolls
            // off the canvas and the second slides into its slot). When that
            // happens every cache key below belongs to the previous occupant;
            // reset them so wires, text, hatches, meshes and Face3D all
            // re-upload for the new viewport instead of showing the previous
            // one's (differently frustum-culled) content — which otherwise makes
            // the surviving viewport's text/geometry vanish.
            if inner.slot_id != vp.instance_id {
                inner.slot_id = vp.instance_id;
                inner.cached_epoch = (u64::MAX, u64::MAX, u64::MAX);
                inner.cached_wire_id = u64::MAX;
                inner.cached_selection = (u64::MAX, u64::MAX);
                inner.cached_mesh_content_id = u64::MAX;
                inner.cached_face3d_key = (u64::MAX, false);
                inner.cached_hatch_source = None;
                inner.cached_wipeout_source = None;
                inner.cached_image_source = None;
                inner.cached_text_source = None;
                inner.cached_mesh_source = None;
                inner.cached_face3d_source = None;
                inner.cached_face3d_depth_source = None;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    inner.wire_cull_key = (u64::MAX, u64::MAX, 0, 0);
                }
                inner.render_sig = u64::MAX;
            }
            // The MSAA / depth / resolve textures are always sized to the
            // FULL viewport rectangle (not the on-canvas-visible portion)
            // so the camera matrices render at consistent aspect / scale.
            // The blit step picks the visible sub-rectangle out via the
            // shader's UV crop uniform, which lets partially off-canvas
            // viewports composite to their visible surface area without
            // drift.
            let clip_size = Size::new(
                (vp.screen_rect.width * bounds.width * scale).ceil().max(1.0) as u32,
                (vp.screen_rect.height * bounds.height * scale).ceil().max(1.0) as u32,
            );
            inner.ensure_depth_texture(device, clip_size);
            inner.viewcube.ensure_depth_texture(device, full_size);
            // Compute the UV crop for this viewport. `screen_rect` is in
            // normalized canvas units (0..1) but may extend negative or
            // beyond 1 when the viewport hangs off the canvas. The on-
            // canvas portion in viewport-local UV is straightforward to
            // derive from how much sticks out on each side.
            let sr = vp.screen_rect;
            let (uo_x, us_x) = uv_crop_axis(sr.x, sr.width);
            let (uo_y, us_y) = uv_crop_axis(sr.y, sr.height);
            inner.upload_blit_uv(queue, [uo_x, uo_y], [us_x, us_y]);
            inner.upload_uniforms(queue, &vp.uniforms);

            // ── Scene-render cache ────────────────────────────────────────
            // A pure cursor move — or any frame where the view, geometry,
            // selection and live preview are all unchanged — produces a
            // pixel-identical image. The resolve texture still holds it, so we
            // skip every geometry pass + the MSAA resolve (in `Pipeline::render`
            // via `skip_geometry`) and its per-frame O(N) scissor / LOD
            // recompute below, letting the frame reduce to a single blit. This
            // is the main fix for the per-mouse-move stall that scales with
            // drawing size. The ViewCube is excluded from the signature and
            // keeps updating in its own always-on pass, so cube hover still
            // tracks while the scene is cached.
            let sig = render_signature(vp, clip_size.width, clip_size.height);
            let skip = inner.render_sig != u64::MAX && sig == inner.render_sig;
            inner.render_sig = sig;
            inner.skip_geometry = skip;
            // Interaction LOD: skip the hatch draw this frame while navigating.
            inner.skip_hatch_frame = vp.skip_hatch;
            if skip {
                if vp.show_viewcube {
                    inner.viewcube.upload(
                        queue,
                        vp.cam_rotation,
                        vp.compass_rotation,
                        (vp.screen_rect.width * bounds.width) as u32,
                        (vp.screen_rect.height * bounds.height) as u32,
                        vp.hover_region,
                        self.viewcube_text_color,
                    );
                }
                continue;
            }
            let Some(vp_wires) = vp.wires.upgrade() else {
                continue;
            };
            // Third component is the *selected-set* signature (not
            // selection_generation, which also bumps on hover) so a rollover
            // doesn't re-upload the static hatch / face3d buffers.
            let cur_key = (vp.geometry_epoch, vp.camera_generation, vp.selected_sig);
            let fill_mode = vp.fill_mode;
            // 3D face fill requires *both* the doc-level FILLMODE *and* the
            // per-view Solid toggle. Hatches / wipeouts deliberately ignore
            // the view toggle so 2D fills stay on even when the user picks
            // the Wireframe overlay style.
            let face3d_fill_active = fill_mode && !vp.view_wireframe;
            let fill_changed = inner.cached_fill_mode != fill_mode;
            let hatch_changed = inner
                .cached_hatch_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.hatches));
            let wipeout_changed = inner
                .cached_wipeout_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.wipeout_hatches));
            if hatch_changed || fill_changed {
                inner.upload_hatches(
                    device,
                    queue,
                    if fill_mode { &vp.hatches[..] } else { &[] },
                );
                inner.cached_hatch_source = Some(Arc::clone(&vp.hatches));
            }
            if wipeout_changed || fill_changed {
                inner.upload_wipeouts(
                    device,
                    if fill_mode {
                        &vp.wipeout_hatches[..]
                    } else {
                        &[]
                    },
                );
                inner.cached_wipeout_source = Some(Arc::clone(&vp.wipeout_hatches));
            }
            if inner
                .cached_image_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.images))
            {
                inner.upload_images(device, queue, &vp.images[..]);
                inner.cached_image_source = Some(Arc::clone(&vp.images));
            }
            if inner
                .cached_text_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.text_verts))
            {
                inner.upload_text(device, queue, &vp.text_verts[..]);
                inner.cached_text_source = Some(Arc::clone(&vp.text_verts));
            }
            inner.cached_fill_mode = fill_mode;
            inner.cached_epoch = cur_key;
            // Face3D edge/fill buffers are world-space and selection-independent
            // (upload_face3d takes no selection input), so they only change with
            // the geometry or the 3D-fill toggle — never on a pan/orbit. Gating
            // on its category sources plus the stable wire content id avoids
            // rebuilding it when another entity category alone changes. Never
            // retain `vp.wires` here: Scene needs unique ownership to splice a
            // one-entity edit into the resident set.
            let face_pass_unchanged = vp
                .wire_patch
                .as_ref()
                .is_some_and(|(_, patch)| !patch.face_pass_changed);
            let face3d_changed = inner
                .cached_face3d_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.face3d_wires))
                || inner
                    .cached_face3d_depth_source
                    .as_ref()
                    .map_or(true, |source| !Arc::ptr_eq(source, &vp.draw_depths))
                || (inner.cached_face3d_key.0 != vp.wire_content_id
                    && !face_pass_unchanged);
            if face3d_changed || face3d_fill_active != inner.cached_face3d_key.1 {
                inner.upload_face3d(
                    device,
                    &vp.face3d_wires[..],
                    &vp_wires[..],
                    !face3d_fill_active,
                    &vp.draw_depths,
                );
                inner.cached_face3d_source = Some(Arc::clone(&vp.face3d_wires));
                inner.cached_face3d_depth_source = Some(Arc::clone(&vp.draw_depths));
            }
            inner.cached_face3d_key = (vp.wire_content_id, face3d_fill_active);
            // Wire buffers are world-space, so a camera move alone doesn't
            // change them — only the view_proj uniform (uploaded every frame).
            // Gate the upload on the wire content id instead of the camera tick:
            // the Model wire set is held static, so its id is unchanged across
            // camera moves and the vertex re-pack + GPU write is skipped. Kept
            // independent of the `cur_key` block so a preview/interim wire change
            // still uploads even when the camera didn't move.
            if vp.wire_content_id != inner.cached_wire_id {
                // Persistent per-entity wire arena (OCS_WIRE_GPU_PATCH): patch
                // just the changed entities' instance slabs instead of rebuilding
                // the whole wire buffer. Only for the scissor-free, mesh-free
                // (single-batch) Model set; scissored paper viewports and mixed
                // 2D/3D sets fall through to the shared batched path below.
                #[cfg(not(target_arch = "wasm32"))]
                let mut arena_served = false;
                #[cfg(target_arch = "wasm32")]
                let arena_served = false;
                #[cfg(not(target_arch = "wasm32"))]
                let _perf = crate::perf::enabled();
                #[cfg(not(target_arch = "wasm32"))]
                let _t0 = iced::time::Instant::now();
                #[cfg(not(target_arch = "wasm32"))]
                let mut _patched = false;
                #[cfg(not(target_arch = "wasm32"))]
                if crate::scene::wire_gpu_patch_enabled() && inner.wire_const_bgl.is_some() {
                    use crate::scene::pipeline::wire_arena::{self, WireArena};
                    let bgl = inner.wire_const_bgl.as_ref().unwrap();
                    let base_ok = vp
                        .wire_patch
                        .as_ref()
                        .map_or(false, |(base, patch)| {
                            inner.wire_arena_id == *base && !patch.changes.is_empty()
                        });
                    let patch = vp.wire_patch.as_ref().map(|(_, patch)| patch);
                    if _perf {
                        crate::perf_record!(
                            "[perf] arena-base ok={} held={} patch={:?} changes={}",
                            base_ok,
                            inner.wire_arena_id,
                            vp.wire_patch.as_ref().map(|(base, _)| *base),
                            patch.map_or(0, |p| p.changes.len()),
                        );
                    }

                    // Split only changed runs on a patch. The previous path
                    // scanned/parses all resident wires repeatedly here even
                    // though WireArena emits just one changed entity.
                    let mut regular_changed: rustc_hash::FxHashMap<
                        acadrust::Handle,
                        Vec<&crate::scene::WireModel>,
                    > = rustc_hash::FxHashMap::default();
                    let mut mesh_changed: rustc_hash::FxHashMap<
                        acadrust::Handle,
                        Vec<&crate::scene::WireModel>,
                    > = rustc_hash::FxHashMap::default();
                    if let Some(patch) = patch {
                        for &(handle, _) in patch.changes.iter() {
                            let run = patch
                                .runs
                                .get(&handle)
                                .map(|wires| wires.as_slice())
                                .unwrap_or(&[]);
                            let mesh_entity = run
                                .iter()
                                .any(|w| !w.fill_tris.is_empty() && !w.fill_tris_low.is_empty());
                            regular_changed.insert(
                                handle,
                                run.iter()
                                    .filter(|w| !w.points.is_empty() && !mesh_entity)
                                    .collect(),
                            );
                            mesh_changed.insert(
                                handle,
                                run.iter()
                                    .filter(|w| !w.points.is_empty() && mesh_entity)
                                    .collect(),
                            );
                        }
                    }
                    let fallback_touched = |mesh_edge: bool| {
                        patch.map_or(true, |patch| {
                            patch.changes.iter().any(|(handle, _)| {
                                inner.wire_arena_fallback_handles.contains(handle)
                                    || if mesh_edge {
                                        mesh_changed.get(handle).is_some_and(|run| !run.is_empty())
                                    } else {
                                        regular_changed
                                            .get(handle)
                                            .is_some_and(|run| !run.is_empty())
                                    }
                            })
                        })
                    };
                    let reg_ok = base_ok
                        && if let Some(arena) = inner.wire_arena.as_mut() {
                            let patch = patch.unwrap();
                            arena.patch(
                                queue,
                                &patch.changes,
                                &regular_changed,
                                patch.new_handles_are_suffix,
                                &vp.draw_depths,
                            )
                        } else {
                            inner.wire_arena_fallback_kind == Some(false)
                                && !fallback_touched(false)
                        };
                    let mesh_ok = base_ok
                        && if let Some(arena) = inner.wire_arena_mesh.as_mut() {
                            let patch = patch.unwrap();
                            arena.patch(
                                queue,
                                &patch.changes,
                                &mesh_changed,
                                patch.new_handles_are_suffix,
                                &vp.draw_depths,
                            )
                        } else {
                            inner.wire_arena_fallback_kind == Some(true)
                                && !fallback_touched(true)
                        };
                    if !reg_ok || !mesh_ok {
                        // Initial upload or a patch that outgrew arena capacity:
                        // only then pay the full regular/mesh split.
                        let mesh_names: rustc_hash::FxHashSet<u64> = vp_wires
                            .iter()
                            .filter(|w| !w.fill_tris.is_empty() && !w.fill_tris_low.is_empty())
                            .filter_map(|w| w.name.parse::<u64>().ok())
                            .collect();
                        let regular: Vec<&crate::scene::WireModel> = vp_wires
                            .iter()
                            .filter(|w| {
                                !w.points.is_empty()
                                    && !wire_arena::is_mesh_edge(w, &mesh_names)
                            })
                            .collect();
                        let mesh: Vec<&crate::scene::WireModel> = vp_wires
                            .iter()
                            .filter(|w| wire_arena::is_mesh_edge(w, &mesh_names))
                            .collect();
                        if !reg_ok {
                            inner.wire_arena = WireArena::build(
                                device,
                                queue,
                                &regular,
                                &vp.draw_depths,
                                bgl,
                                false,
                            );
                            if inner.wire_arena.is_none() && !regular.is_empty() {
                                inner.wire_arena_fallback = std::sync::Arc::new(
                                    crate::scene::pipeline::WireGpu::from_run_refs(
                                        device,
                                        &regular,
                                        &vp.draw_depths,
                                        false,
                                        bgl,
                                    ),
                                );
                                inner.wire_arena_fallback_kind = Some(false);
                                inner.wire_arena_fallback_handles = regular
                                    .iter()
                                    .filter_map(|wire| {
                                        wire.name
                                            .parse::<u64>()
                                            .ok()
                                            .map(acadrust::Handle::new)
                                    })
                                    .collect();
                            } else if inner.wire_arena_fallback_kind == Some(false) {
                                inner.wire_arena_fallback =
                                    std::sync::Arc::new(Vec::new());
                                inner.wire_arena_fallback_kind = None;
                                inner.wire_arena_fallback_handles.clear();
                            }
                        }
                        if !mesh_ok {
                            inner.wire_arena_mesh = WireArena::build(
                                device,
                                queue,
                                &mesh,
                                &vp.draw_depths,
                                bgl,
                                true,
                            );
                            if inner.wire_arena_mesh.is_none() && !mesh.is_empty() {
                                inner.wire_arena_fallback = std::sync::Arc::new(
                                    crate::scene::pipeline::WireGpu::from_run_refs(
                                        device,
                                        &mesh,
                                        &vp.draw_depths,
                                        true,
                                        bgl,
                                    ),
                                );
                                inner.wire_arena_fallback_kind = Some(true);
                                inner.wire_arena_fallback_handles = mesh
                                    .iter()
                                    .filter_map(|wire| {
                                        wire.name
                                            .parse::<u64>()
                                            .ok()
                                            .map(acadrust::Handle::new)
                                    })
                                    .collect();
                            } else if inner.wire_arena_fallback_kind == Some(true) {
                                inner.wire_arena_fallback =
                                    std::sync::Arc::new(Vec::new());
                                inner.wire_arena_fallback_kind = None;
                                inner.wire_arena_fallback_handles.clear();
                            }
                        }
                    }
                    _patched = reg_ok && mesh_ok;

                    let regular_ready = inner.wire_arena.is_some()
                        || inner.wire_arena_fallback_kind == Some(false);
                    let mesh_ready = inner.wire_arena_mesh.is_some()
                        || inner.wire_arena_fallback_kind == Some(true);
                    if regular_ready
                        && mesh_ready
                        && (inner.wire_arena.is_some() || inner.wire_arena_mesh.is_some())
                    {
                        let mut gpus = if inner.wire_arena_fallback_kind == Some(false) {
                            inner.wire_arena_fallback.as_ref().clone()
                        } else {
                            inner
                                .wire_arena
                                .as_ref()
                                .map(WireArena::wire_gpus)
                                .unwrap_or_default()
                        };
                        if inner.wire_arena_fallback_kind == Some(true) {
                            gpus.extend(inner.wire_arena_fallback.iter().cloned());
                        } else if let Some(arena) = inner.wire_arena_mesh.as_ref() {
                            gpus.extend(arena.wire_gpus());
                        }
                        inner.gpu_wires = std::sync::Arc::new(gpus);
                        if _patched {
                            wire_arena::patch_handle_index(
                                &mut inner.wire_handle_index,
                                &patch.unwrap().index_edits,
                            );
                        } else {
                            inner.wire_handle_index =
                                wire_arena::build_handle_index(&vp_wires[..]);
                        }
                        inner.wire_arena_id = vp.wire_content_id;
                        arena_served = true;
                    } else {
                        inner.wire_arena = None;
                        inner.wire_arena_mesh = None;
                        inner.wire_arena_fallback = std::sync::Arc::new(Vec::new());
                        inner.wire_arena_fallback_kind = None;
                        inner.wire_arena_fallback_handles.clear();
                        inner.wire_arena_id = u64::MAX;
                    }
                }
                // Share one copy of the resident wire buffers across every slot
                // (and every pane — one MultiPipeline backs them all) rendering
                // this content id: build on a cache miss, then hand out Arc
                // clones. Two paper viewports showing the same model, or four
                // Model tiles, upload the wire vertices once between them.
                // `.cloned()` releases the immutable cache borrow before the
                // miss branch takes a mutable one.
                if !arena_served {
                    let cached = pipeline
                        .wire_buffer_cache
                        .get(&vp.wire_content_id)
                        .cloned();
                    let built = match cached {
                        Some(entry) => entry,
                        None => {
                            let entry =
                                inner.build_wire_buffers(device, &vp_wires[..], &vp.draw_depths);
                            pipeline
                                .wire_buffer_cache
                                .insert(vp.wire_content_id, entry.clone());
                            // Evict entries no slot still holds (only the cache
                            // references them). An entry drawn by any pane keeps a
                            // strong count ≥ 2, so this never drops live geometry.
                            if pipeline.wire_buffer_cache.len() > 16 {
                                pipeline
                                    .wire_buffer_cache
                                    .retain(|_, (w, _)| std::sync::Arc::strong_count(w) > 1);
                            }
                            entry
                        }
                    };
                    inner.gpu_wires = built.0;
                    inner.wire_handle_index = built.1;
                } // end !arena_served
                inner.cached_wire_id = vp.wire_content_id;
                #[cfg(not(target_arch = "wasm32"))]
                if _perf {
                    let gi: u32 = inner.gpu_wires.iter().map(|w| w.instance_count).sum();
                    let outcome = if !arena_served {
                        "shared-fullupload"
                    } else if _patched {
                        "arena-patch"
                    } else if inner.wire_arena_fallback_kind.is_some() {
                        "arena-hybrid"
                    } else {
                        "arena-build"
                    };
                    crate::perf_record!(
                        "[perf] wire {:>7.1}ms  {:<18} wires={} gpu_instances={}",
                        _t0.elapsed().as_secs_f64() * 1000.0,
                        outcome,
                        vp_wires.len(),
                        gi,
                    );
                }
            }
            // Selection xray overlay — rebuilt when the selection changes or the
            // underlying wires changed. A pick bumps only selection_generation,
            // so this refreshes without re-tessellating or re-uploading the main
            // wire buffers.
            let sel_key = (vp.wire_content_id, vp.selection_generation);
            if sel_key != inner.cached_selection {
                inner.upload_selected_wires(
                    device,
                    &vp_wires[..],
                    &vp.selected_handles,
                    vp.hover_handle,
                    &vp.draw_depths,
                );
                // Text highlight rides the same selection key: a pick / rollover
                // recolours the selected / hovered glyphs without touching the
                // base text buffer.
                inner.upload_text_highlight(
                    device,
                    &vp_wires[..],
                    &vp.selected_handles,
                    vp.hover_handle,
                );
                inner.cached_selection = sel_key;
            }
            // Batched solid meshes stay resident while unrelated entity
            // categories change.
            if inner
                .cached_mesh_source
                .as_ref()
                .map_or(true, |source| !Arc::ptr_eq(source, &vp.meshes))
            {
                let patched = vp.wire_patch.as_ref().is_some_and(|(previous, patch)| {
                    *previous == inner.cached_mesh_content_id
                        && inner.patch_mesh_batch(
                            device,
                            queue,
                            &vp.meshes[..],
                            &patch.changes,
                        )
                });
                if !patched {
                    inner.upload_mesh_batch(device, queue, &vp.meshes[..]);
                }
                inner.cached_mesh_source = Some(Arc::clone(&vp.meshes));
            }
            inner.cached_mesh_content_id = vp.wire_content_id;
            // Selection / hover highlight overlay — tinted copies of just the
            // picked solids, rebuilt only when the highlight set (or geometry)
            // changes. Drawn over the static batch so the base never re-packs.
            let hl_key = (
                Arc::as_ptr(&vp.meshes) as usize as u64,
                vp.selection_generation,
            );
            if hl_key != inner.cached_highlight_key {
                inner.update_mesh_highlight(&vp.selected_handles, vp.hover_handle);
                inner.cached_highlight_key = hl_key;
            }
            // Live overlay (command preview / interim / grip drag) — small and
            // refreshed every frame it's present, so a drag never re-uploads
            // the resident base wire buffer.
            inner.upload_preview_wires(device, &vp.preview_wires[..], &vp.draw_depths);
            inner.upload_preview_text(device, queue, &vp.preview_text_verts[..]);
            // Cull / scissor / LOD project AABBs relative-to-eye (matching the
            // GPU's RTE path) so the math stays precise at UTM-scale coords.
            let view_rot = vp.uniforms.view_rot;
            let eye = glam::DVec3::new(
                vp.uniforms.eye_high[0] as f64 + vp.uniforms.eye_low[0] as f64,
                vp.uniforms.eye_high[1] as f64 + vp.uniforms.eye_low[1] as f64,
                vp.uniforms.eye_high[2] as f64 + vp.uniforms.eye_low[2] as f64,
            );
            // DISPSILH: rebuild view-dependent silhouettes each frame (off by
            // default, so this is a no-op unless the drawing enables it). Only
            // in modes that draw edges — pure shaded hides them.
            if vp.display_silhouette && (vp.view_wireframe || vp.show_3d_edges) {
                inner.upload_silhouettes(device, &vp.meshes[..], vp.view_dir);
            } else {
                inner.upload_silhouettes(device, &[], vp.view_dir);
            }
            inner.upload_clip_boundary(device, &vp.clip_boundary_ndc);
            inner.compute_hatch_lod(queue, view_rot, eye, clip_size.width, clip_size.height);
            inner.compute_wipeout_lod(view_rot, eye, clip_size.width, clip_size.height);
            inner.compute_mesh_lod(
                queue,
                view_rot,
                eye,
                clip_size.width,
                clip_size.height,
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                let cull_key = (
                    vp.wire_content_id,
                    vp.camera_generation,
                    clip_size.width,
                    clip_size.height,
                );
                if inner.wire_arena_id == vp.wire_content_id
                    && inner.wire_cull_key != cull_key
                {
                    let mut visible = if inner.wire_arena_fallback_kind == Some(false) {
                        inner.wire_arena_fallback.as_ref().clone()
                    } else {
                        inner
                            .wire_arena
                            .as_ref()
                            .map(|arena| {
                                arena.wire_gpus_visible(
                                    view_rot,
                                    eye,
                                    clip_size.width,
                                    clip_size.height,
                                )
                            })
                            .unwrap_or_default()
                    };
                    if inner.wire_arena_fallback_kind == Some(true) {
                        visible.extend(inner.wire_arena_fallback.iter().cloned());
                    } else if let Some(arena) = inner.wire_arena_mesh.as_ref() {
                        visible.extend(arena.wire_gpus_visible(
                            view_rot,
                            eye,
                            clip_size.width,
                            clip_size.height,
                        ));
                    }
                    inner.gpu_wires = std::sync::Arc::new(visible);
                    inner.wire_cull_key = cull_key;
                }
            }
            if vp.show_viewcube {
                inner.viewcube.upload(
                    queue,
                    vp.cam_rotation,
                    vp.compass_rotation,
                    (vp.screen_rect.width * bounds.width) as u32,
                    (vp.screen_rect.height * bounds.height) as u32,
                    vp.hover_region,
                    self.viewcube_text_color,
                );
            }
        }
        if let Some(sample) = self.nav_perf {
            crate::perf::record(format_args!(
                "[perf] nav-prepare op={} space={} mode={} input={:.2}ms build={:.2}ms prepare={:.2}ms elapsed={:.2}ms viewports={}",
                sample.op.label(),
                sample.space,
                sample.mode,
                sample.input_ms,
                sample.build_ms,
                nav_prepare_started.elapsed().as_secs_f64() * 1000.0,
                sample.started.elapsed().as_secs_f64() * 1000.0,
                self.viewports.len(),
            ));
        }
    }

    fn render(
        &self,
        pipeline: &MultiPipeline,
        encoder: &mut iced::wgpu::CommandEncoder,
        target: &iced::wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        let nav_render_started = iced::time::Instant::now();
        let cw = clip.width as f32;
        let ch = clip.height as f32;
        let clip_right = clip.x + clip.width;
        let clip_bottom = clip.y + clip.height;
        for vp in &self.viewports {
            let Some(slot) = pipeline.slot_by_instance.get(&vp.instance_id) else {
                continue;
            };
            let Some(inner) = pipeline.inners.get(*slot) else {
                continue;
            };
            // Where the viewport would land on the surface in absolute
            // pixels (i32 because either edge may stick off the canvas).
            let vp_full_x = clip.x as i32 + (vp.screen_rect.x * cw) as i32;
            let vp_full_y = clip.y as i32 + (vp.screen_rect.y * ch) as i32;
            let vp_full_w = (vp.screen_rect.width * cw).max(1.0) as i32;
            let vp_full_h = (vp.screen_rect.height * ch).max(1.0) as i32;
            // Intersect with the surface clip — that's the slice we blit.
            let dest_x = vp_full_x.max(clip.x as i32);
            let dest_y = vp_full_y.max(clip.y as i32);
            let dest_right = (vp_full_x + vp_full_w).min(clip_right as i32);
            let dest_bottom = (vp_full_y + vp_full_h).min(clip_bottom as i32);
            if dest_right <= dest_x || dest_bottom <= dest_y {
                continue;
            }
            let surface_dest = Rectangle {
                x: dest_x as u32,
                y: dest_y as u32,
                width: (dest_right - dest_x) as u32,
                height: (dest_bottom - dest_y) as u32,
            };
            let vp_size = Size::new(vp_full_w.max(1) as u32, vp_full_h.max(1) as u32);
            // `mesh_fill` is false for Wireframe 2D / Wireframe 3D — flip
            // the draw path so meshes use the wireframe pipeline + the
            // pre-built triangle-edge index buffer.
            let mesh_wireframe = !vp.mesh_fill;
            inner.render(
                encoder,
                target,
                vp_size,
                surface_dest,
                self.bg_color,
                mesh_wireframe,
                vp.hidden_line,
                vp.show_3d_edges,
            );
            // The ViewCube renders directly to the surface at the full
            // viewport rect. Skip it when the viewport's top-right corner
            // (where the cube sits) is off-canvas — wgpu's `set_viewport`
            // rejects negative origins, and a clamped cube would scale
            // distortedly. The active viewport is normally fully visible.
            if vp.show_viewcube
                && vp_full_x >= clip.x as i32
                && vp_full_y >= clip.y as i32
                && vp_full_x + vp_full_w <= clip_right as i32
                && vp_full_y + vp_full_h <= clip_bottom as i32
            {
                let vp_clip = Rectangle {
                    x: vp_full_x as u32,
                    y: vp_full_y as u32,
                    width: vp_full_w as u32,
                    height: vp_full_h as u32,
                };
                inner.viewcube.render(encoder, target, vp_clip);
            }
        }
        if let Some(sample) = self.nav_perf {
            crate::perf::record(format_args!(
                "[perf] nav-render op={} space={} mode={} encode={:.2}ms elapsed={:.2}ms viewports={}",
                sample.op.label(),
                sample.space,
                sample.mode,
                nav_render_started.elapsed().as_secs_f64() * 1000.0,
                sample.started.elapsed().as_secs_f64() * 1000.0,
                self.viewports.len(),
            ));
        }
    }
}

/// Hash of everything that determines one viewport's rendered scene image.
/// Two consecutive frames with the same signature are pixel-identical, so the
/// second may skip the geometry passes and re-blit the resolve texture (see the
/// scene-render cache in `Primitive::prepare` / `Pipeline::render`).
///
/// Deliberately EXCLUDES `hover_region` — the ViewCube highlight renders in its
/// own always-on pass, so cube hover must not force a full scene re-render. The
/// live preview IS included (its coordinates), so a rubber-band tracking the
/// cursor still renders, and the frame where the preview clears erases it
/// instead of freezing the last overlay on screen.
fn render_signature(vp: &ViewportData, clip_w: u32, clip_h: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    // Camera + per-view shading flags all live in the uniforms (view_rot, eye
    // high/low, viewport size, lineweight, flat_shade, transparency) — hashing
    // the raw POD bytes captures every pan / zoom / orbit / twist and toggle in
    // one shot. Identical camera state recomputes to identical bits, so a still
    // view never spuriously misses the cache.
    bytemuck::bytes_of(&vp.uniforms).hash(&mut h);
    vp.geometry_epoch.hash(&mut h);
    vp.selection_generation.hash(&mut h);
    vp.selected_sig.hash(&mut h);
    vp.wire_content_id.hash(&mut h);
    vp.fill_mode.hash(&mut h);
    vp.view_wireframe.hash(&mut h);
    vp.mesh_fill.hash(&mut h);
    vp.show_3d_edges.hash(&mut h);
    vp.hidden_line.hash(&mut h);
    // ViewCube visibility is excluded from the *scene* signature elsewhere only
    // for the live-hover pass; here it MUST invalidate the cache so toggling the
    // cube (NAVVCUBE) re-renders and actually clears the last cube frame
    // instead of leaving its stale pixels on the cached surface.
    vp.show_viewcube.hash(&mut h);
    // Interaction-LOD hatch suppression: differs the signature so the settle
    // frame (skip_hatch flips false) re-renders with hatches and re-caches.
    vp.skip_hatch.hash(&mut h);
    clip_w.hash(&mut h);
    clip_h.hash(&mut h);
    // Live overlay (command preview / interim / grip drag). Small — a handful
    // of wires — so hashing its coordinates is cheap and catches the endpoint
    // moving with the cursor as well as the preview appearing / clearing.
    for w in vp.preview_wires.iter() {
        w.points.len().hash(&mut h);
        for p in &w.points {
            p[0].to_bits().hash(&mut h);
            p[1].to_bits().hash(&mut h);
            p[2].to_bits().hash(&mut h);
        }
    }
    // Grip-drag / command-preview glyph quads (issue #316). A pure-text slide
    // leaves `preview_wires` empty and moves ONLY these, so a signature that
    // ignored them would let the scene-render cache freeze the dragged text at
    // its first frame (re-blitting a stale texture) until release. Small — one
    // dragged entity — so hashing every vertex is cheap. Hash the high AND low
    // halves of the double-single position: a sub-unit slide at UTM scale shifts
    // only the low residual, so hashing the high f32 alone would miss it.
    vp.preview_text_verts.len().hash(&mut h);
    for v in vp.preview_text_verts.iter() {
        v.pos[0].to_bits().hash(&mut h);
        v.pos[1].to_bits().hash(&mut h);
        v.pos_low[0].to_bits().hash(&mut h);
        v.pos_low[1].to_bits().hash(&mut h);
    }
    h.finish()
}

/// On-canvas-visible UV crop on one axis. `pos` and `size` are in the
/// shader widget's normalized 0..1 coords. Returns `(uv_offset, uv_scale)`
/// applied as `actual_uv = quad_uv * uv_scale + uv_offset` in the blit
/// shader — identity `(0.0, 1.0)` for fully on-canvas viewports.
fn uv_crop_axis(pos: f32, size: f32) -> (f32, f32) {
    if size <= 0.0 {
        return (0.0, 1.0);
    }
    let left_off = (-pos).max(0.0);
    let right_off = (pos + size - 1.0).max(0.0);
    let visible = (size - left_off - right_off).max(0.0);
    (left_off / size, visible / size)
}

/// Apply a clip-space crop to `view_proj` so the sub-rect of the original
/// view defined by UV offset `(uo, vo)` + scale `(us, vs)` is remapped to
/// NDC `[-1, 1]^2`. Identity transform when the sub-rect is the whole
/// view (`uo=vo=0`, `us=vs=1`). Used by viewports that hang off the
/// canvas — the camera frustum stays at full-vp aspect, but only the
/// visible portion lands in the MSAA target.
fn crop_view_proj(view_proj: glam::Mat4, uo: f32, vo: f32, us: f32, vs: f32) -> glam::Mat4 {
    // Build the matrix that maps the visible clip-space sub-rect
    //   x ∈ [2uo - 1, 2(uo+us) - 1]
    //   y ∈ [1 - 2(vo+vs), 1 - 2vo]
    // back to NDC [-1, 1]^2. (Texture v is top-down → camera y flips.)
    let us = us.max(1e-6);
    let vs = vs.max(1e-6);
    let sx = 1.0 / us;
    let sy = 1.0 / vs;
    let tx = (1.0 - 2.0 * uo - us) / us;
    let ty = -(1.0 - 2.0 * vo - vs) / vs;
    let crop = glam::Mat4::from_cols_array(&[
        sx, 0.0, 0.0, 0.0, // col 0
        0.0, sy, 0.0, 0.0, // col 1
        0.0, 0.0, 1.0, 0.0, // col 2
        tx, ty, 0.0, 1.0, // col 3
    ]);
    crop * view_proj
}

// ── Render-style helpers (impl Scene) ────────────────────────────────────

impl Scene {
    fn build_lighting_cache(&self) -> Vec<SceneLight> {
        use acadrust::objects::{ClassObjectData, ObjectType};

        fn normalized(value: [f64; 3], fallback: [f32; 3]) -> [f32; 3] {
            let length =
                (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
            if length <= 1e-12 {
                fallback
            } else {
                [
                    (value[0] / length) as f32,
                    (value[1] / length) as f32,
                    (value[2] / length) as f32,
                ]
            }
        }

        fn solar_direction(
            sun: &acadrust::objects::Sun,
            geo: &acadrust::objects::GeoData,
        ) -> Option<[f32; 3]> {
            if sun.julian_day < 1_000_000 {
                return None;
            }
            let daylight_ms = if sun.is_daylight_savings_on {
                3_600_000.0
            } else {
                0.0
            };
            let jd = sun.julian_day as f64
                + (sun.milliseconds as f64 - daylight_ms) / 86_400_000.0;
            let days = jd - 2_451_545.0;
            let mean_longitude = (280.460 + 0.985_647_4 * days).to_radians();
            let mean_anomaly = (357.528 + 0.985_600_3 * days).to_radians();
            let ecliptic_longitude = mean_longitude
                + (1.915 * mean_anomaly.sin()
                    + 0.020 * (2.0 * mean_anomaly).sin())
                    .to_radians();
            let obliquity = (23.439 - 0.000_000_4 * days).to_radians();
            let right_ascension = (obliquity.cos() * ecliptic_longitude.sin())
                .atan2(ecliptic_longitude.cos());
            let declination =
                (obliquity.sin() * ecliptic_longitude.sin()).asin();
            let local_sidereal = (280.460_618_37
                + 360.985_647_366_29 * days
                + geo.reference_point.x)
                .to_radians();
            let hour_angle = (local_sidereal - right_ascension + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            let latitude = geo.reference_point.y.to_radians();
            let east_component = -declination.cos() * hour_angle.sin();
            let north_component = declination.sin() * latitude.cos()
                - declination.cos() * hour_angle.cos() * latitude.sin();
            let up_component = declination.sin() * latitude.sin()
                + declination.cos() * hour_angle.cos() * latitude.cos();
            if up_component <= 0.0 {
                return None;
            }
            let north = normalized(
                [geo.north_direction.x, geo.north_direction.y, 0.0],
                [0.0, 1.0, 0.0],
            );
            let east = [north[1], -north[0], 0.0];
            let up = normalized(
                [geo.up_direction.x, geo.up_direction.y, geo.up_direction.z],
                [0.0, 0.0, 1.0],
            );
            Some(normalized(
                [
                    -(east[0] as f64 * east_component
                        + north[0] as f64 * north_component
                        + up[0] as f64 * up_component),
                    -(east[1] as f64 * east_component
                        + north[1] as f64 * north_component
                        + up[1] as f64 * up_component),
                    -(east[2] as f64 * east_component
                        + north[2] as f64 * north_component
                        + up[2] as f64 * up_component),
                ],
                [0.0, 0.0, -1.0],
            ))
        }

        fn converted(scene: &Scene, light: &acadrust::entities::Light) -> Option<SceneLight> {
            if !light.status {
                return None;
            }
            let direction = normalized(
                [
                    light.target.x - light.position.x,
                    light.target.y - light.position.y,
                    light.target.z - light.position.z,
                ],
                [0.0, 0.0, -1.0],
            );
            let color_layer = if light.light_color.rgb().is_some() {
                None
            } else {
                Some(light.common.layer.clone())
            };
            let rgba = if color_layer.is_none() {
                tess_util::aci_to_rgba(&light.light_color)
            } else {
                scene.layer_color(&light.common.layer)
            };
            Some(SceneLight {
                handle: light.common.handle,
                color_layer,
                light_type: light.light_type as f32,
                position: [light.position.x, light.position.y, light.position.z],
                direction,
                color: [rgba[0], rgba[1], rgba[2]],
                intensity: light.intensity.max(0.0) as f32,
                hotspot_cos: if light.hotspot_angle > 0.0 {
                    (light.hotspot_angle * 0.5).cos() as f32
                } else {
                    1.0
                },
                falloff_cos: if light.falloff_angle > 0.0 {
                    (light.falloff_angle * 0.5).cos() as f32
                } else {
                    -1.0
                },
                attenuation_type: light.attenuation_type as f32,
                attenuation_start: if light.use_attenuation_limits {
                    light.attenuation_start_limit as f32
                } else {
                    0.0
                },
                attenuation_end: if light.use_attenuation_limits {
                    light.attenuation_end_limit as f32
                } else {
                    0.0
                },
            })
        }

        let mut lights = Vec::new();
        for &handle in crate::entities::object_data::light_entities(
            &self.object_data_cache,
        ) {
            if lights.len() >= 4 {
                break;
            }
            if let Some(EntityType::Light(light)) = self.document.get_entity(handle) {
                if let Some(light) = converted(self, light) {
                    lights.push(light);
                }
            }
        }

        if lights.len() < 4 {
            let geo = crate::entities::object_data::geo_objects(
                &self.object_data_cache,
            )
            .iter()
            .find_map(|handle| match self.document.objects.get(handle) {
                Some(ObjectType::GeoData(value))
                    if value.coordinate_type == 3
                        && value.reference_point.x.is_finite()
                        && value.reference_point.y.is_finite()
                        && value.reference_point.x.abs() <= 180.0
                        && value.reference_point.y.abs() <= 90.0 => Some(value),
                _ => None,
            });
            for handle in crate::entities::object_data::sun_objects(
                &self.object_data_cache,
            ) {
                let Some(ObjectType::ClassObject(value)) =
                    self.document.objects.get(handle)
                else {
                    continue;
                };
                let ClassObjectData::Sun(sun) = &value.data else {
                    continue;
                };
                if !sun.is_on {
                    continue;
                }
                let Some(geo) = geo else {
                    break;
                };
                let Some(direction) = solar_direction(sun, geo) else {
                    break;
                };
                let rgba = tess_util::aci_to_rgba(&sun.color);
                lights.push(SceneLight {
                    handle: value.handle,
                    color_layer: None,
                    light_type: 1.0,
                    position: [0.0; 3],
                    direction,
                    color: [rgba[0], rgba[1], rgba[2]],
                    intensity: sun.intensity.max(0.0) as f32,
                    hotspot_cos: 1.0,
                    falloff_cos: -1.0,
                    attenuation_type: 0.0,
                    attenuation_start: 0.0,
                    attenuation_end: 0.0,
                });
                break;
            }
        }
        lights
    }

    fn apply_document_lighting(&self, uniforms: &mut Uniforms) {
        if self.lighting_cache.borrow().is_none() {
            let lights = self.build_lighting_cache();
            *self.lighting_cache.borrow_mut() = Some(lights);
        }
        let cache = self.lighting_cache.borrow();
        let lights = cache.as_deref().unwrap_or_default();
        let eye = [
            uniforms.eye_high[0] as f64 + uniforms.eye_low[0] as f64,
            uniforms.eye_high[1] as f64 + uniforms.eye_low[1] as f64,
            uniforms.eye_high[2] as f64 + uniforms.eye_low[2] as f64,
        ];
        uniforms.lighting[0] = lights.len().min(4) as f32;
        for (index, light) in lights.iter().take(4).enumerate() {
            let color = light
                .color_layer
                .as_deref()
                .map(|layer| self.layer_color(layer))
                .map(|rgba| [rgba[0], rgba[1], rgba[2]])
                .unwrap_or(light.color);
            uniforms.light_position_type[index] = [
                (light.position[0] - eye[0]) as f32,
                (light.position[1] - eye[1]) as f32,
                (light.position[2] - eye[2]) as f32,
                light.light_type,
            ];
            uniforms.light_direction_intensity[index] = [
                light.direction[0],
                light.direction[1],
                light.direction[2],
                light.intensity,
            ];
            uniforms.light_color_hotspot[index] = [
                color[0],
                color[1],
                color[2],
                light.hotspot_cos,
            ];
            uniforms.light_attenuation[index] = [
                light.attenuation_type,
                light.attenuation_start,
                light.attenuation_end,
                light.falloff_cos,
            ];
        }
    }

    /// Returns (entity_color, pattern_length, pattern, line_weight_px, aci).
    pub(in crate::scene) fn render_style(&self, e: &EntityType) -> ([f32; 4], f32, [f32; 8], f32, u8) {
        let (color, pl, pat, lw, aci) = render_style_for(&self.document, e);
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        // Objects on a locked layer are dimmed toward the background so they
        // read as "not editable" (they stay visible and snappable).
        let adapted = adapt_to_bg(color, bg);
        let final_color = if layer_locked(&self.document, e) {
            crate::scene::cache::block_cache::fade_toward_bg(adapted, bg)
        } else {
            adapted
        };
        (final_color, pl, pat, lw, aci)
    }
}

/// Whether an entity sits on a locked layer (via the document's layer table).
/// Document-only so it is safe from the parallel tessellation path.
pub(in crate::scene) fn layer_locked(document: &CadDocument, e: &EntityType) -> bool {
    document
        .layers
        .get(&e.common().layer)
        .map(|l| l.is_locked())
        .unwrap_or(false)
}

// ── Document-only render-style helpers (no &self, safe to call from parallel contexts) ──

/// Resolves the effective linetype name for an entity, falling back to the
/// layer's linetype when the entity's own linetype is "ByLayer".
pub(in crate::scene) fn linetype_name_for<'a>(document: &'a CadDocument, e: &'a EntityType) -> &'a str {
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
pub(in crate::scene) fn render_style_for(
    document: &CadDocument,
    e: &EntityType,
) -> ([f32; 4], f32, [f32; 8], f32, u8) {
    let layer_name = &e.common().layer;
    let (entity_color, aci) = {
        let common = e.common();
        let book_color = common
            .color_book_handle
            .filter(|handle| handle.is_valid())
            .and_then(|handle| document.objects.get(&handle))
            .and_then(|object| match object {
                acadrust::objects::ObjectType::BookColor(book) => Some(&book.color),
                _ => None,
            });
        let ec = book_color.unwrap_or(&common.color);
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
        lineweight_to_px(resolved)
    };

    (entity_color, pattern_length, pattern, line_weight_px, aci)
}

pub(crate) fn has_resolved_book_color(document: &CadDocument, e: &EntityType) -> bool {
    e.common()
        .color_book_handle
        .filter(|handle| handle.is_valid())
        .and_then(|handle| document.objects.get(&handle))
        .is_some_and(|object| matches!(object, acadrust::objects::ObjectType::BookColor(_)))
}

/// Resolved render style used as the inheritance source for a block child's
/// ByBlock properties (the INSERT's own style) or its layer-0 properties (the
/// INSERT's *layer* style). Bundled so it threads through the block-expansion
/// call chain as a single value.
#[derive(Clone, Copy, Debug)]
pub struct InheritStyle {
    pub color: [f32; 4],
    pub pat_len: f32,
    pub pat: [f32; 8],
    pub lw_px: f32,
}

/// Convert a concrete (already layer-resolved) lineweight to display pixels.
pub(crate) fn lineweight_to_px(lw: &LineWeight) -> f32 {
    const MM_TO_PX: f32 = 96.0 / 25.4;
    // CAD apps display model-space lineweights larger than their true physical
    // size so the gradations stay legible on screen — at true scale a 0.5 mm
    // line is ~2 px and is indistinguishable from thinner weights (which all
    // floor to 1 px). Apply the same legibility boost so weights are pronounced
    // and tell apart, matching other DWG editors. (#147)
    const LWT_DISPLAY_BOOST: f32 = 2.0;
    lw.millimeters()
        .map(|mm| (mm as f32 * MM_TO_PX * LWT_DISPLAY_BOOST).max(1.0))
        .unwrap_or(1.0)
}

/// Resolve a layer's own color / linetype / lineweight to concrete render
/// values — what a fully-ByLayer entity on that layer would draw as. Used for
/// the layer-0 block rule: a block child on layer "0" inherits the block
/// reference's layer through this. Color is returned RAW (background adaptation
/// happens at emit time). Falls back to white / Continuous / 1 px when the
/// layer is missing.
pub(crate) fn layer_render_style(document: &CadDocument, layer_name: &str) -> InheritStyle {
    let layer = document.layers.get(layer_name);
    let color = layer.map(|l| &l.color).unwrap_or(&AcadColor::WHITE);
    let [r, g, b, _] = tess_util::aci_to_rgba(color);
    let lt_name = layer.map(|l| l.line_type.as_str()).unwrap_or("Continuous");
    let lt_scale = document.header.linetype_scale as f32;
    let (pat_len, pat) = resolve_pattern(&document.line_types, lt_name, lt_scale);
    let lw = layer.map(|l| &l.line_weight).unwrap_or(&LineWeight::Default);
    InheritStyle {
        color: [r, g, b, 1.0],
        pat_len,
        pat,
        lw_px: lineweight_to_px(lw),
    }
}

/// Whether a block child uses layer-0 inheritance semantics.
///
/// XREF merge keeps dependent layers distinct by namespacing them as
/// `xref|layer`; its source layer `0` therefore becomes `xref|0` but must still
/// inherit through the containing INSERT exactly like an unprefixed layer 0.
pub(crate) fn is_effective_layer_zero(layer_name: &str) -> bool {
    layer_name.eq_ignore_ascii_case("0")
        || layer_name
            .rsplit_once('|')
            .is_some_and(|(_, dependent)| dependent.eq_ignore_ascii_case("0"))
}

/// Like `render_style_for` but resolves a block sub-entity's inherited
/// properties: ByBlock inherits the INSERT's style, and (the layer-0 rule) a
/// sub-entity on layer "0" with ByLayer properties inherits the INSERT's
/// *layer* style (`l0`). Explicit properties always win. Call this for
/// exploded block sub-entities so color/linetype/lineweight propagate right.
pub(crate) fn render_style_for_block_sub(
    document: &CadDocument,
    e: &EntityType,
    insert_color: [f32; 4],
    insert_pat_len: f32,
    insert_pat: [f32; 8],
    insert_lw_px: f32,
    l0: InheritStyle,
) -> ([f32; 4], f32, [f32; 8], f32, u8) {
    let (color, pat_len, pat, lw_px, aci) = render_style_for(document, e);
    let common = e.common();
    let on_l0 = is_effective_layer_zero(&common.layer);

    let has_book_color = has_resolved_book_color(document, e);
    let final_color = if !has_book_color && common.color == AcadColor::ByBlock {
        insert_color
    } else if !has_book_color && on_l0 && common.color == AcadColor::ByLayer {
        // Inherit the insert layer's RGB but keep the child's own transparency.
        [l0.color[0], l0.color[1], l0.color[2], color[3]]
    } else {
        color
    };

    let lt_bylayer =
        common.linetype.is_empty() || common.linetype.eq_ignore_ascii_case("bylayer");
    let (final_pat_len, final_pat) = if common.linetype.eq_ignore_ascii_case("byblock") {
        (insert_pat_len, insert_pat)
    } else if on_l0 && lt_bylayer {
        (l0.pat_len, l0.pat)
    } else {
        (pat_len, pat)
    };

    let final_lw = if matches!(common.line_weight, LineWeight::ByBlock) {
        insert_lw_px
    } else if on_l0 && matches!(common.line_weight, LineWeight::ByLayer | LineWeight::Default) {
        l0.lw_px
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
    /// Gather the SDF glyph quads carried on a viewport's wire set into one
    /// flat vertex list for the text render pass. The tessellator attaches the
    /// quads to each entity's own wire (and the block-expand loop transforms
    /// block-instance quads to world), so gathering is a cheap walk. Cached on
    /// `wire_content_id` — the wire-buffer content id — so an unchanged wire
    /// set (pan / zoom) is walked once, not every frame; the id changes when
    /// geometry or selection rebuilds the wires, re-tinting selected glyphs.
    /// Empty when SDF text is disabled.
    pub(in crate::scene) fn gather_text_verts(
        &self,
        wires: &[WireModel],
        wire_content_id: u64,
        source_key: u64,
        depth_map: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    ) -> std::sync::Arc<Vec<crate::scene::pipeline::text_gpu::TextVertex>> {
        use std::sync::Arc;
        {
            let cache = self.sdf_text_cache.borrow();
            if let Some(verts) = cache.get(&wire_content_id) {
                return verts.clone();
            }
        }
        // A new content id (every geometry edit) misses the cache — but if no
        // text-bearing entity changed since the last build, the glyphs are
        // identical, so reuse them instead of re-walking every wire. Reuse is
        // per `source_key`: another source's glyphs are a different wire set,
        // not an older build of this one (#403). The glyphs also bake the
        // entity draw-order depth (below), which shifts whenever an entity is
        // added or removed — reuse only across rank-stable (all-Modified)
        // edits.
        {
            let reuse = {
                let last = self.last_sdf_text.borrow();
                match last.get(&source_key) {
                    Some((epoch, arc))
                        if self.text_unchanged(*epoch) && self.draw_ranks_stable(*epoch) =>
                    {
                        Some(arc.clone())
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                self.sdf_text_cache
                    .borrow_mut()
                    .insert(wire_content_id, arc.clone());
                self.last_sdf_text
                    .borrow_mut()
                    .insert(source_key, (self.geometry_epoch, arc.clone()));
                return arc;
            }
        }
        let mut out: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();
        for w in wires {
            if !w.text_verts.is_empty() {
                // Bake the host wire's draw-order depth into its glyphs so
                // text layers like the rest of the entity: its own background
                // fill lands at the same biased depth (glyphs draw after fills
                // and win the LessEqual test), and a hatch later in draw order
                // still covers the text. A tessellation-time value (block-local
                // compose) survives as an offset on top of the wire's depth.
                let d = crate::scene::pipeline::wire_gpu::wire_draw_depth(w, depth_map);
                if d == 0.0 {
                    out.extend_from_slice(&w.text_verts);
                } else {
                    out.extend(w.text_verts.iter().map(|tv| {
                        let mut tv = *tv;
                        tv.draw_depth += d;
                        tv
                    }));
                }
            }
        }
        let verts = Arc::new(out);
        {
            let mut cache = self.sdf_text_cache.borrow_mut();
            // Ids change on rebuild, so old keys die naturally; cap bounds churn.
            if cache.len() > 8 {
                cache.clear();
            }
            cache.insert(wire_content_id, verts.clone());
        }
        {
            let mut last = self.last_sdf_text.borrow_mut();
            if last.len() > 8 {
                last.clear();
            }
            last.insert(source_key, (self.geometry_epoch, verts.clone()));
        }
        verts
    }

    /// Build the unified multi-viewport `Primitive` for the current layout.
    /// Model layout → one full-window viewport (more once tiled); paper
    /// layout → one viewport per floating content viewport. Each entry is
    /// rendered into its own screen rectangle by its own inner pipeline.
    pub(in crate::scene) fn build_viewports(
        &self,
        bounds: Rectangle,
        model_render_mode: acadrust::entities::ViewportRenderMode,
        _hover_region: Option<usize>,
        show_viewcube: bool,
        viewcube_text_color: [f32; 4],
    ) -> Primitive {
        let nav_build_started = iced::time::Instant::now();
        let perf_nav = self.take_nav_perf();
        // Hover comes from the scene cell driven by the app-level
        // `CursorMoved` handler — the cube overlay sits above the shader
        // and would otherwise mask the move event from `Program::update`.
        let hover_region = self.viewcube_hover.get();
        self.selection.borrow_mut().vp_size = (bounds.width, bounds.height);
        if bounds.height > 0.0 {
            self.set_render_aspect(bounds.width / bounds.height);
            self.set_render_pixel_scale(bounds.width, bounds.height);
        }
        let canvas = (bounds.width.max(1.0), bounds.height.max(1.0));
        let instances = self.active_viewports(canvas.0, canvas.1, model_render_mode);
        // Transparent clear — outside drawn geometry the resolve texture
        // stays at alpha=0, so the alpha-blended blit reveals the container
        // background (model bg, or the desk colour in a paper layout).
        let bg_color = [0.0, 0.0, 0.0, 0.0];
        let viewports: Vec<ViewportData> = instances
            .iter()
            .filter_map(|inst| self.viewport_data_for(inst, canvas, hover_region, show_viewcube))
            .collect();
        // Empty viewports → blit nothing; the container background (model bg
        // or the paper desk colour) stays visible.
        let perf_nav = perf_nav.map(|mut sample| {
            sample.build_ms = nav_build_started.elapsed().as_secs_f64() * 1000.0;
            sample
        });
        Primitive {
            viewports,
            bg_color,
            viewcube_text_color,
            nav_perf: perf_nav,
        }
    }

    /// Build a single-pane Model primitive: the viewport for tile `tile_idx`,
    /// filling the shader widget's own `bounds` (= the pane rectangle the
    /// `pane_grid` laid out). Each Model pane is its own shader widget, so the
    /// camera matrices use the pane aspect for free and the primitive owns
    /// pipeline slot `tile_idx`. The active tile renders the live camera /
    /// render-mode; the rest use their stored snapshot.
    pub(in crate::scene) fn build_viewport_for_pane(
        &self,
        bounds: Rectangle,
        tile_idx: usize,
        model_render_mode: acadrust::entities::ViewportRenderMode,
        show_viewcube: bool,
        viewcube_text_color: [f32; 4],
    ) -> Primitive {
        let hover_region = self.viewcube_hover.get();
        let canvas = (bounds.width.max(1.0), bounds.height.max(1.0));
        let bg_color = [0.0, 0.0, 0.0, 0.0];
        let tiles = self.model_tiles.borrow();
        let Some(tile) = tiles.get(tile_idx) else {
            return Primitive {
                viewports: vec![],
                bg_color,
                viewcube_text_color,
                nav_perf: None,
            };
        };
        let active = self.active_model_tile.get();
        let is_active = tile_idx == active;
        let nav_build_started = iced::time::Instant::now();
        let perf_nav = if is_active {
            self.take_nav_perf()
        } else {
            None
        };
        let camera = if is_active {
            self.camera.borrow().clone()
        } else {
            tile.camera.clone()
        };
        let inst = ViewportInstance {
            handle: Handle::NULL,
            tile_idx: Some(tile_idx),
            // Fills the whole widget (= pane); normalized rect is (0,0,1,1).
            screen_rect: Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas.0,
                height: canvas.1,
            },
            camera,
            render_mode: if is_active {
                model_render_mode
            } else {
                tile.render_mode
            },
            active: is_active,
            grid_on: tile.grid_on,
            paper_sheet: false,
        };
        let viewports = self
            .viewport_data_for(&inst, canvas, hover_region, show_viewcube)
            .into_iter()
            .collect();
        let perf_nav = perf_nav.map(|mut sample| {
            sample.build_ms = nav_build_started.elapsed().as_secs_f64() * 1000.0;
            sample
        });
        Primitive {
            viewports,
            bg_color,
            viewcube_text_color,
            nav_perf: perf_nav,
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
        show_viewcube: bool,
    ) -> Option<ViewportData> {
        let flags = render_mode_flags(inst.render_mode);
        let view_wireframe = !flags.face3d_fill;

        // Clip the viewport rect to the canvas; size the per-viewport MSAA
        // / depth / resolve textures to that visible portion. Sizing them
        // to the full vp rect would blow past wgpu's per-dimension texture
        // limit (8192 on common GPUs) once paper-space zoom grows the rect
        // far enough off the canvas.
        let full = inst.screen_rect;
        if full.width <= 0.0 || full.height <= 0.0 {
            return None;
        }
        let visible_x = full.x.max(0.0);
        let visible_y = full.y.max(0.0);
        let visible_x_end = (full.x + full.width).min(canvas.0);
        let visible_y_end = (full.y + full.height).min(canvas.1);
        let visible_w = (visible_x_end - visible_x).max(0.0);
        let visible_h = (visible_y_end - visible_y).max(0.0);
        if visible_w < 1.0 || visible_h < 1.0 {
            return None;
        }
        let uo = ((visible_x - full.x) / full.width).clamp(0.0, 1.0);
        let vo = ((visible_y - full.y) / full.height).clamp(0.0, 1.0);
        let us = (visible_w / full.width).clamp(0.0, 1.0);
        let vs = (visible_h / full.height).clamp(0.0, 1.0);

        // EVERY wire source is resident + camera-independent now (unified
        // static-hold, `resident_wires_for`) and stamps its stable
        // [`WIRE_CONTENT_GEN`] id into `last_model_wire_gen` — Model tiles,
        // the paper sheet, content viewports and the pick composite alike. So
        // the GPU wire upload, the Face3D split and the SDF text gather below
        // are all skipped while the content is unchanged, and
        // `render_signature` stays stable so paper hits the single-blit scene
        // cache exactly like Model.
        let base_arc = if let Some(tile_idx) = inst.tile_idx {
            let aspect = if full.height > 0.0 {
                full.width / full.height
            } else {
                1.0
            };
            self.model_tile_wires_arc(tile_idx, &inst.camera, aspect, full.height)
        } else if inst.paper_sheet {
            // The sheet renders the paper block's own entities + viewport
            // borders — NOT the projected viewport content (the GPU content
            // viewports draw that themselves).
            self.paper_sheet_wires_arc()
        } else if inst.handle == acadrust::Handle::NULL {
            self.entity_wires_arc()
        } else {
            self.model_wires_for_viewport_arc(inst.handle, full.height)
        };
        // Wire-buffer content id for the upload gate. Preview / interim wires
        // are NOT part of this buffer anymore (they go in a separate per-frame
        // overlay buffer below), so the base id is the source's stable content
        // gen — a drag or camera move never re-uploads the base wire set.
        let wire_content_id = self.last_model_wire_gen.get();
        let wire_patch = self.model_wire_patch_for(wire_content_id);
        // Split Face3D wires from the rest. The split is content-only (keyed
        // by the wire-set content id), so while the geometry is unchanged it's
        // memoized rather than re-walking every wire (handle lookup + clone)
        // each frame — for every source, since all ids are stable now.
        let (face3d_wires, other_arc) = {
            let cached = { self.split_cache.borrow().get(&wire_content_id).cloned() };
            let inherited_empty = if let Some((base, patch)) = wire_patch.as_ref() {
                if patch.face_pass_changed {
                    None
                } else {
                    self.split_cache
                        .borrow()
                        .get(base)
                        .filter(|(_, others)| others.is_none())
                        .cloned()
                }
            } else {
                None
            };
            let (fa, oa) = cached.or(inherited_empty).unwrap_or_else(|| {
                // No Face3D wire at all (pure 2-D drawings, mesh imports):
                // "others" would be a wire-for-wire copy of the base set —
                // mark it `None` and use the base set directly instead of
                // duplicating it (#358). The base Arc itself must not be
                // stored in the cache (see the `split_cache` field docs).
                let (fa, oa) = if base_arc.iter().any(|w| is_face3d_wire(w, &self.document)) {
                    let (f, o) = split_face3d_wires(&base_arc, &self.document);
                    (Arc::new(f), Some(Arc::new(o)))
                } else {
                    (Arc::new(Vec::new()), None)
                };
                let mut c = self.split_cache.borrow_mut();
                // Ids change on rebuild, so old keys die naturally; the cap
                // just bounds pathological churn.
                if c.len() > 8 {
                    c.clear();
                }
                c.insert(wire_content_id, (fa.clone(), oa.clone()));
                (fa, oa)
            });
            self.split_cache
                .borrow_mut()
                .entry(wire_content_id)
                .or_insert_with(|| (fa.clone(), oa.clone()));
            (fa, oa.unwrap_or_else(|| Arc::clone(&base_arc)))
        };
        // Base wire set — the cached `other` Arc directly, never cloned to
        // append overlays. Preview / interim wires ride in their own small
        // per-frame buffer so the (potentially huge) base buffer stays resident
        // and unchanged while a command preview or grip drag is live.
        let all_wires = other_arc;
        let preview_wires = if self.interim_wire.is_none() && self.preview_wires.is_empty() {
            Arc::new(Vec::new())
        } else {
            let mut v: Vec<WireModel> = Vec::with_capacity(self.preview_wires.len() + 1);
            if let Some(iw) = &self.interim_wire {
                v.push(iw.clone());
            }
            v.extend(self.preview_wires.iter().cloned());
            Arc::new(v)
        };

        // Build the camera at the *full* viewport's aspect so the ortho
        // frustum matches what the viewport entity stores, then post-
        // multiply by a clip-space "zoom into the visible sub-rect" that
        // maps the visible portion to NDC [-1, 1]. Geometry passes
        // rasterize into a visible-sized MSAA, so `viewport_size` (used
        // by the wire shader to extrude line thickness in screen pixels)
        // must be the visible size — but `world_per_pixel` is invariant
        // under cropping (full_h cancels with vs) so the value computed
        // from the full bounds is the one we want.
        let full_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: full.width.max(1.0),
            height: full.height.max(1.0),
        };
        let mut uniforms =
            Uniforms::new(&inst.camera, full_bounds, self.document.header.lineweight_display);
        if self.document.header.paper_space_linetype_scaling
            && !inst.paper_sheet
            && inst.tile_idx.is_none()
            && inst.handle != Handle::NULL
        {
            if let Some(EntityType::Viewport(vp)) = self.document.get_entity(inst.handle) {
                let viewport_scale =
                    vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
                if viewport_scale.is_finite() && viewport_scale > 1e-9 {
                    uniforms.linetype_scale = (1.0 / viewport_scale) as f32;
                }
            }
        }
        // Crop the rotation-only RTE view-projection to the visible sub-rect.
        uniforms.view_rot = crop_view_proj(uniforms.view_rot, uo, vo, us, vs);
        uniforms.viewport_size = [visible_w, visible_h];
        uniforms.flat_shade = if flags.flat_shade { 1.0 } else { 0.0 };
        uniforms.transparency_enable = if self.transparency_display { 1.0 } else { 0.0 };
        self.apply_document_lighting(&mut uniforms);

        // `screen_rect` carries the *visible* sub-rectangle in normalized
        // canvas coords — that's what `Pipeline::prepare` uses to size
        // the per-viewport textures and what `Primitive::render` uses to
        // pick the surface destination. The UV crop uniform reads as
        // identity here, since the texture already covers exactly the
        // visible portion.
        let screen_rect = Rectangle {
            x: visible_x / canvas.0,
            y: visible_y / canvas.1,
            width: visible_w / canvas.0,
            height: visible_h / canvas.1,
        };

        // The paper sheet instance renders only the paper layout block's own
        // fills (plus a synthetic white fill for the printable area) — NOT the
        // model-block hatches. Those belong inside the floating content
        // viewports; rendering them on the full-canvas sheet would let them
        // bleed past the viewport borders whenever model coords overlap the
        // paper area. Content viewports keep the full set (the model camera +
        // per-viewport scissor place / clip them correctly).
        // Per-viewport frozen-layer set for a paper content viewport. Content
        // viewports must hide FILLS / IMAGES / SOLIDS on their frozen layers too,
        // not just wires (which `model_wires_for_viewport_arc` already filters).
        // Empty for the sheet, model tiles and the implicit-model view — none of
        // which carry a per-viewport freeze — so those reuse the shared sets.
        let vp_frozen: rustc_hash::FxHashSet<Handle> = if !inst.paper_sheet
            && inst.tile_idx.is_none()
            && inst.handle != acadrust::Handle::NULL
        {
            match self.document.get_entity(inst.handle) {
                Some(EntityType::Viewport(vp)) => vp.frozen_layers.iter().cloned().collect(),
                _ => rustc_hash::FxHashSet::default(),
            }
        } else {
            rustc_hash::FxHashSet::default()
        };

        let (hatches, wipeout_hatches, paper_images) = if inst.paper_sheet {
            let (hatches, wipeouts, images) = self.paper_sheet_render_models();
            (hatches, wipeouts, Some(images))
        } else {
            (
                self.hatch_models_for_viewport(&vp_frozen),
                self.wipeout_models_for_viewport(&vp_frozen),
                None,
            )
        };
        let images = if let Some(images) = paper_images {
            images
        } else {
            self.images_for_viewport(&vp_frozen)
        };
        // The paper sheet shows the layout's own 2-D content (fills, borders,
        // annotation) — never the model's 3-D solids. Those are drawn inside
        // the floating content viewports, whose model camera + per-viewport
        // scissor place and clip them correctly. Feeding the model mesh set to
        // the sheet piles every solid onto the paper origin, because the sheet
        // camera works in paper coordinates, not model space — the same reason
        // the sheet excludes model hatches and wires above.
        let meshes = if inst.paper_sheet {
            Arc::new(Vec::new())
        } else {
            self.meshes_for_viewport(&vp_frozen)
        };

        // SDF text quads (behind OCS_TEXT_SDF). The glyph quads ride on each
        // entity's own wire (produced by the tessellator, transformed for
        // block instances by the block-expand loop), so here we simply gather
        // them from this viewport's wire set. This covers model text, block-
        // internal text and the paper sheet's own annotation alike — each set
        // draws only the text that belongs to it. Cached on the wire content
        // id so an unchanged wire set is not re-walked every frame.
        // The reuse fallback inside the gather is keyed per wire SOURCE — the
        // sheet, a Model tile, a content viewport and the implicit view carry
        // different glyph sets even at the same geometry epoch (#403). Paper
        // sheets must also include their layout block: switching layouts does
        // not change the geometry epoch, and a role-only key reused the prior
        // sheet's glyph coordinates while its wires moved correctly. Tiles
        // share the resident Model set, so they share one key; the implicit
        // view mixes in the current layout block (BEDIT swaps sets without a
        // geometry delta).
        let text_source_key: u64 = if inst.tile_idx.is_some() {
            0x1000_0000_0000_0000
        } else if inst.paper_sheet {
            0x2000_0000_0000_0000 | self.current_layout_block_handle().value()
        } else if inst.handle == acadrust::Handle::NULL {
            0x4000_0000_0000_0000 | self.current_layout_block_handle().value()
        } else {
            0x3000_0000_0000_0000 | inst.handle.value()
        };
        let text_verts = self.gather_text_verts(
            &all_wires,
            wire_content_id,
            text_source_key,
            &self.draw_depth_map(),
        );
        // Grip-drag / command-preview glyphs, excluded from the epoch-cached base
        // gather above. Two sources, both tiny (one operation's worth) and walked
        // per frame: the overlay wires' own glyphs (MOVE / COPY / ROTATE / SCALE /
        // STRETCH / MIRROR ghosts carry text_verts) and `self.preview_text` (the
        // grip-slide fast path, which emits bare glyphs with empty preview_wires).
        // The two never overlap — a slide leaves preview_wires empty — so a plain
        // concat is correct, no double-draw (issue #316).
        let preview_text_verts = {
            let mut pv: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();
            for w in preview_wires.iter() {
                if !w.text_verts.is_empty() {
                    pv.extend_from_slice(&w.text_verts);
                }
            }
            pv.extend_from_slice(&self.preview_text);
            Arc::new(pv)
        };
        // Stable per-viewport identity (tagged so tile / sheet / content /
        // implicit-model instances never collide), so a reused pipeline slot
        // can tell it changed occupant and reset its caches.
        let instance_id: u64 = if let Some(t) = inst.tile_idx {
            0x1000_0000_0000_0000 | (t as u64)
        } else if inst.paper_sheet {
            0x2000_0000_0000_0000
        } else {
            0x3000_0000_0000_0000 | inst.handle.value()
        };
        // A paper content viewport with a non-rectangular clip entity gets its
        // boundary projected into this render target's NDC (paper shape mapped
        // through the same visible-sub-rect crop as the content). Rectangular
        // viewports, the paper sheet and Model tiles clip via their own render
        // rectangle and need no stencil boundary.
        let clip_boundary_ndc = if !inst.paper_sheet
            && inst.tile_idx.is_none()
            && inst.handle != acadrust::Handle::NULL
            && self.current_layout != "Model"
        {
            Arc::new(self.viewport_clip_boundary_ndc(inst.handle, uo, vo, us, vs))
        } else {
            Arc::new(vec![])
        };
        Some(ViewportData {
            instance_id,
            wires: Arc::downgrade(&all_wires),
            clip_boundary_ndc,
            preview_wires,
            face3d_wires,
            text_verts,
            preview_text_verts,
            draw_depths: self.draw_depth_map(),
            hatches,
            wipeout_hatches,
            images,
            meshes,
            uniforms,
            view_dir: (inst.camera.rotation * glam::Vec3::NEG_Z).normalize_or(glam::Vec3::NEG_Z),
            cam_rotation: inst.camera.view_rotation_mat() * self.viewcube_ucs_mat(),
            compass_rotation: inst.camera.view_rotation_mat(),
            // Only the active viewport gets the hovered-region highlight.
            hover_region: if inst.active { hover_region } else { None },
            // The cube shows only on the active viewport, and only while the
            // caller (the widget) says there is room for it beside the render
            // bar — so it hides adaptively when the viewport gets narrow.
            show_viewcube: inst.active && show_viewcube,
            fill_mode: self.document.header.fill_mode,
            view_wireframe,
            mesh_fill: flags.mesh_fill,
            show_3d_edges: flags.show_3d_edges,
            display_silhouette: self.document.header.display_silhouette,
            hidden_line: flags.hidden_line,
            // Interaction LOD: suppress the costly hatch pass while the view is
            // actively moving; the scene-render cache holds the full-quality
            // (hatched) frame once it settles. Only applied to the on-screen
            // Model / paper content — the paper *sheet* keeps its fills.
            skip_hatch: self.hatch_lod_enabled() && !inst.paper_sheet && self.navigating_lod(),
            geometry_epoch: self.geometry_epoch,
            camera_generation: self.camera_generation,
            wire_content_id,
            wire_patch,
            selected_handles: Arc::new(self.selected.iter().copied().collect()),
            hover_handle: self.hover_highlight,
            selection_generation: self.selection_generation,
            selected_sig: self.selected_set_sig(),
            screen_rect,
        })
    }

    /// Update viewcube hover state from cursor position within `bounds`.
    ///
    /// The cube draws in the top-right of the *active model tile* (which fills
    /// the canvas when there is a single tile), so the hover hit-test maps the
    /// cursor into that tile's local space and uses the tile's dimensions.
    pub(in crate::scene) fn update_viewcube_state(
        &self,
        state: &mut CameraState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) {
        let pos = cursor.position_in(bounds);
        let cam_rotation = self.camera.borrow().view_rotation_mat() * self.viewcube_ucs_mat();
        if let Some(p) = pos {
            let tile = self.active_model_tile_bounds(bounds.width, bounds.height);
            state.hover_region = hover_id(
                p.x - tile.x,
                p.y - tile.y,
                tile.width,
                tile.height,
                cam_rotation,
                VIEWCUBE_PX,
            );
        } else {
            state.hover_region = None;
        }
    }

    pub(in crate::scene) fn viewcube_mouse_interaction(&self, state: &CameraState) -> mouse::Interaction {
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

    // Keep dots (element length exactly 0) as 0.0 so the shader can render
    // them as a fixed ~1 px mark; trailing array slots stay 0.0 padding and
    // the shader tells the two apart by position (a 0.0 before the last
    // non-zero element is a dot, trailing 0.0s are padding). The old code
    // encoded dots as `0.01 * scale` — a tiny world-length dash that went
    // sub-pixel at normal zoom and dragged the pattern's `min_elem` below one
    // pixel, so the dash LOD collapsed dotted / dash-dot lines to solid (or,
    // at larger LTSCALE, left only invisible sub-pixel dots between big
    // gaps). (#149)
    let mut pat = [0.0f32; 8];
    let mut pat_len = 0.0f32;
    for (i, el) in lt.elements.iter().take(8).enumerate() {
        // positive = dash, negative = gap, exactly 0 = dot.
        let v = el.length as f32 * scale;
        pat[i] = v;
        pat_len += v.abs();
    }
    if pat_len < 1e-6 {
        return solid;
    }
    (pat_len, pat)
}

/// Whether a wire belongs to a Face3D entity, by document handle lookup —
/// so no changes to WireModel are needed.
fn is_face3d_wire(w: &WireModel, document: &acadrust::CadDocument) -> bool {
    w.name
        .parse::<u64>()
        .ok()
        .and_then(|v| document.get_entity(Handle::new(v)))
        .map(|e| matches!(e, EntityType::Face3D(_)))
        .unwrap_or(false)
}

/// Partition a wire list into (face3d_wires, other_wires).
///
/// O(N) per geometry epoch — acceptable since it runs once per epoch.
fn split_face3d_wires(
    wires: &[WireModel],
    document: &acadrust::CadDocument,
) -> (Vec<WireModel>, Vec<WireModel>) {
    let mut face3d = Vec::new();
    let mut others = Vec::new();
    for w in wires {
        if is_face3d_wire(w, document) {
            face3d.push(w.clone());
        } else {
            others.push(w.clone());
        }
    }
    (face3d, others)
}

// ── Layer-0 block inheritance (#221) ──────────────────────────────────────
// A block child on layer "0" with ByLayer properties inherits the block
// reference's *layer*; every other layer is "sticky" (keeps its own layer);
// ByBlock inherits the insert's own style; explicit properties always win.
#[cfg(test)]
mod layer0_inherit_tests {
    use super::*;
    use acadrust::entities::Line;
    use acadrust::tables::Layer;
    use acadrust::types::{Color, Transparency};

    // ACI: 1 = red, 3 = green, 7 = white. Distinct, so the assertions below
    // can tell "inherited the insert layer" from "kept layer 0".
    fn doc() -> CadDocument {
        let mut d = CadDocument::new();
        let mut walls = Layer::new("Walls");
        walls.color = Color::Index(1); // red
        d.layers.add_or_replace(walls);
        let mut zero = Layer::new("0");
        zero.color = Color::Index(7); // white
        d.layers.add_or_replace(zero);
        let mut other = Layer::new("Other");
        other.color = Color::Index(3); // green
        d.layers.add_or_replace(other);
        d
    }

    fn child(layer: &str, color: Color) -> EntityType {
        let mut l = Line::new();
        l.common.layer = layer.to_string();
        l.common.color = color;
        EntityType::Line(l)
    }

    fn resolve(d: &CadDocument, e: &EntityType, ins: [f32; 4]) -> [f32; 4] {
        // Insert sits on "Walls"; its layer style is the layer-0 target.
        let l0 = layer_render_style(d, "Walls");
        render_style_for_block_sub(d, e, ins, l0.pat_len, l0.pat, l0.lw_px, l0).0
    }

    #[test]
    fn layer0_bylayer_inherits_insert_layer() {
        let d = doc();
        let walls = layer_render_style(&d, "Walls").color;
        let zero = layer_render_style(&d, "0").color;
        let c = resolve(&d, &child("0", Color::ByLayer), walls);
        assert_eq!(&c[..3], &walls[..3], "layer-0 child must show the insert's layer (Walls)");
        assert_ne!(&c[..3], &zero[..3], "layer-0 child must NOT show layer 0's own color");
    }

    #[test]
    fn nonzero_layer_is_sticky() {
        let d = doc();
        let walls = layer_render_style(&d, "Walls").color;
        let other = layer_render_style(&d, "Other").color;
        let c = resolve(&d, &child("Other", Color::ByLayer), walls);
        assert_eq!(&c[..3], &other[..3], "a child on a normal layer keeps its own layer");
    }

    #[test]
    fn byblock_inherits_insert_color() {
        let d = doc();
        let ins = [0.2, 0.4, 0.6, 1.0];
        let c = resolve(&d, &child("0", Color::ByBlock), ins);
        assert_eq!(&c[..3], &ins[..3], "ByBlock child uses the insert's color");
    }

    // A *top-level* (non-block-child) entity on layer 0 with ByLayer colour
    // resolves layer 0's own colour and follows it when the layer is recoloured.
    // Regression guard for the issue 231 layer-0 repaint path.
    #[test]
    fn toplevel_layer0_bylayer_follows_layer_color() {
        let mut d = doc();
        let e = child("0", Color::ByLayer);
        let before = render_style_for(&d, &e).0;
        if let Some(l) = d.layers.get_mut("0") {
            l.color = Color::Index(3); // recolour layer 0 -> green
        }
        let after = render_style_for(&d, &e).0;
        let green = tess_util::aci_to_rgba(&Color::Index(3));
        assert_eq!(&after[..3], &green[..3], "top-level layer-0 ByLayer must follow layer 0's colour");
        assert_ne!(&before[..3], &after[..3], "colour must change after recolour");
    }

    #[test]
    fn explicit_color_wins_even_on_layer0() {
        let d = doc();
        let walls = layer_render_style(&d, "Walls").color;
        let green = tess_util::aci_to_rgba(&Color::Index(3));
        let c = resolve(&d, &child("0", Color::Index(3)), walls);
        assert_eq!(&c[..3], &green[..3], "an explicit color must win even on layer 0");
    }

    #[test]
    fn layer0_preserves_child_transparency() {
        let d = doc();
        let walls = layer_render_style(&d, "Walls").color;
        let mut l = Line::new();
        l.common.layer = "0".to_string();
        l.common.color = Color::ByLayer;
        l.common.transparency = Transparency::from_percent(0.5); // 50% transparent
        let c = resolve(&d, &EntityType::Line(l), walls);
        assert_eq!(&c[..3], &walls[..3], "RGB inherited from the insert layer");
        assert!((c[3] - 0.5).abs() < 0.02, "child's own 50% transparency is kept, got {}", c[3]);
    }
}
