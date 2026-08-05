//! `viewport` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    axis_lock_apply, axis_lock_capture, ortho_constrain, parse_coord, polar_constrain_near,
    ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::model::object::GripApply;
use crate::scene::pick::grip::{
    find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit, GripEditMode, GripTarget,
};
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};
use std::sync::Arc;

/// Pixel radius for grabbing a UCS-icon grip (origin dot or an axis tip).
const UCS_GRIP_HIT_PX: f32 = 9.0;
/// Pixel reach for hovering/clicking the icon body (origin, tips, or an arm).
const UCS_ICON_PICK_PX: f32 = 10.0;
/// Pixel half-width for clicking an icon arm (line segment).
const UCS_ICON_ARM_PX: f32 = 6.0;

fn pt_pt_d2(a: Point, b: Point) -> f32 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

fn is_added_polyline_vertex(
    original: &AcadEntityType,
    current: &AcadEntityType,
    vertex_id: usize,
) -> bool {
    match (original, current) {
        (AcadEntityType::LwPolyline(before), AcadEntityType::LwPolyline(after)) => {
            after.vertices.len() == before.vertices.len() + 1
                && vertex_id < after.vertices.len()
        }
        (AcadEntityType::Polyline2D(before), AcadEntityType::Polyline2D(after)) => {
            after.vertices.len() == before.vertices.len() + 1
                && vertex_id < after.vertices.len()
        }
        _ => false,
    }
}

/// Return the source bulge when `vertex_id` is the provisional point inserted
/// into an arc segment. Straight segments stay straight during placement.
fn added_arc_bulge(
    original: &AcadEntityType,
    current: &AcadEntityType,
    vertex_id: usize,
) -> Option<f64> {
    if !is_added_polyline_vertex(original, current, vertex_id) {
        return None;
    }
    let prev = vertex_id.checked_sub(1)?;
    let (n, closed, bulge) = match original {
        AcadEntityType::LwPolyline(polyline) => (
            polyline.vertices.len(),
            polyline.is_closed,
            polyline.vertices.get(prev)?.bulge,
        ),
        AcadEntityType::Polyline2D(polyline) => (
            polyline.vertices.len(),
            polyline.is_closed(),
            polyline.vertices.get(prev)?.bulge,
        ),
        _ => return None,
    };
    ((closed || prev + 1 < n) && bulge.abs() >= 1e-9).then_some(bulge)
}

/// Squared distance from `p` to the segment `a`–`b`.
fn pt_seg_d2(p: Point, a: Point, b: Point) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 1e-6 {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    pt_pt_d2(p, Point::new(a.x + t * vx, a.y + t * vy))
}

/// Which UCS grip (if any) the cursor `p` is on.
fn ucs_grip_under(p: Point, h: &crate::ui::overlay::UcsIconHit) -> Option<crate::app::UcsGripKind> {
    let r2 = UCS_GRIP_HIT_PX * UCS_GRIP_HIT_PX;
    if pt_pt_d2(p, h.origin) <= r2 {
        Some(crate::app::UcsGripKind::Origin)
    } else if pt_pt_d2(p, h.tips[0]) <= r2 {
        Some(crate::app::UcsGripKind::XAxis)
    } else if pt_pt_d2(p, h.tips[1]) <= r2 {
        Some(crate::app::UcsGripKind::YAxis)
    } else {
        None
    }
}

/// True when `p` is over the icon body (origin, a tip, or an arm) — the pick
/// region for hover-highlight and select.
fn over_ucs_icon(p: Point, h: &crate::ui::overlay::UcsIconHit) -> bool {
    let pick2 = UCS_ICON_PICK_PX * UCS_ICON_PICK_PX;
    if pt_pt_d2(p, h.origin) <= pick2 || h.tips.iter().any(|t| pt_pt_d2(p, *t) <= pick2) {
        return true;
    }
    let arm2 = UCS_ICON_ARM_PX * UCS_ICON_ARM_PX;
    h.tips.iter().any(|t| pt_seg_d2(p, h.origin, *t) <= arm2)
}

impl OpenCADStudio {
    /// Write PDSIZE from the dialog buffer with the current relative/absolute
    /// sign. A relative size is stored negative; absolute positive. Switching to
    /// absolute with an empty/zero size seeds a positive value from the current
    /// on-screen size so the point stays representable (PDSIZE 0 always reads as
    /// relative, so absolute needs a non-zero magnitude).
    pub(in crate::app) fn apply_point_size(&mut self) {
        let i = self.active_tab;
        let mut mag = self
            .point_size_buf
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0)
            .abs();
        if !self.point_size_relative && mag == 0.0 {
            let wpp = self.tabs[i].scene.world_per_pixel().unwrap_or(0.0);
            mag = if wpp > 0.0 {
                crate::entities::point::relative_world_size(0.0, wpp)
            } else {
                1.0
            };
            self.point_size_buf = format!("{mag:.4}");
        }
        let next = if self.point_size_relative { -mag } else { mag };
        self.push_undo_snapshot(i, "PDSIZE");
        self.tabs[i].scene.document.header.point_display_size = next;
        self.tabs[i].scene.invalidate_point_dependencies();
        self.tabs[i].dirty = true;
    }

    /// Replace the `mask` bits of PDMODE with `value`, rebuild the point glyphs
    /// and mark the document dirty. Used by the Point Style (DDPTYPE) dialog.
    pub(in crate::app) fn set_point_mode_bits(&mut self, mask: i16, value: i16) {
        let i = self.active_tab;
        let cur = self.tabs[i].scene.document.header.point_display_mode;
        let next = (cur & !mask) | (value & mask);
        if next == cur {
            return;
        }
        self.push_undo_snapshot(i, "PDMODE");
        self.tabs[i].scene.document.header.point_display_mode = next;
        self.tabs[i].scene.invalidate_point_dependencies();
        self.tabs[i].dirty = true;
    }

    /// Mirror the live grid display + grid-snap toggles onto tab `i`'s active
    /// model tile so a save writes them to that viewport's VPort entry (#121).
    pub(in crate::app) fn sync_vport_display(&mut self, i: usize) {
        let grid_on = self.show_grid;
        let snap_on = self.snapper.grid_snap();
        self.tabs[i]
            .scene
            .set_active_tile_grid_snap(grid_on, snap_on);
    }

    /// Adopt the active viewport's grid *display* into the live toggle. Called
    /// on load and whenever the active tab or viewport changes, so the grid
    /// drawing follows the active viewport.
    ///
    /// Grid *snap* (`SnapType::Grid`) is deliberately NOT adopted here: it stays
    /// off by default everywhere and is controlled solely by the user's snap
    /// toggle, so it never silently leaks into object-snap when entering a
    /// viewport whose stored `snap_on` flag happens to be set.
    pub(in crate::app) fn adopt_view_display(&mut self, i: usize) {
        if let Some((grid_on, _snap_on)) = self.tabs[i].scene.active_tile_grid_snap() {
            self.show_grid = grid_on;
        }
    }

    pub(in crate::app) fn sync_render_mode_to_active_tile(&mut self, i: usize) {
        use acadrust::entities::ViewportRenderMode as M;
        if self.tabs[i].scene.current_layout != "Model" {
            return;
        }
        let mode = self.tabs[i].scene.active_model_tile_render_mode();
        if self.tabs[i].render_mode == mode {
            return;
        }
        let label = match mode {
            M::Wireframe2D => "Wireframe 2D",
            M::Wireframe3D => "Wireframe 3D",
            M::HiddenLine => "Hidden Line",
            M::FlatShaded => "Flat Shaded",
            M::GouraudShaded => "Gouraud Shaded",
            M::FlatShadedWithEdges => "Flat Shaded + Edges",
            M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
        };
        self.tabs[i].render_mode = mode;
        let wf = matches!(mode, M::Wireframe2D | M::Wireframe3D);
        self.tabs[i].wireframe = wf;
        self.ribbon.set_wireframe(wf);
        self.tabs[i].visual_style = label.into();
        self.tabs[i].scene.bump_geometry_no_blocks();
    }

    /// Project a pane-local cursor onto the active drawing plane and return a
    /// **model-space** point. Inside a floating viewport `edit_cam` is the
    /// viewport's own camera (a target-plane / UCS pick there already yields
    /// model coords); otherwise the paper camera projects onto the sheet and
    /// the result is mapped paper→model. Used by the readout, snap and click
    /// paths so all three agree on the cursor's model location.

    pub(in crate::app) fn cursor_model_point(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        p: iced::Point,
        bounds: iced::Rectangle,
    ) -> glam::DVec3 {
        // Model-space input always belongs to a drawing plane. With no active
        // UCS that plane is world XY; using the camera target plane and only
        // clearing Z afterwards shifts the picked point on screen in an
        // oblique view. Plain paper space still uses the camera target plane.
        let plane = if self.tabs[i].editing_model_space() {
            Some(match self.tabs[i].active_ucs.as_ref() {
                Some(ucs) => (
                    ucs_z_axis(ucs),
                    glam::DVec3::new(ucs.origin.x, ucs.origin.y, ucs.origin.z),
                ),
                None => (glam::DVec3::Z, glam::DVec3::ZERO),
            })
        } else {
            None
        };
        let pick = |cam: &crate::scene::view::camera::Camera| match plane {
            Some((normal, origin)) => cam.pick_on_plane(p, bounds, normal.as_vec3(), origin),
            None => cam.pick_on_target_plane(p, bounds),
        };
        match edit_cam {
            Some(cam) => pick(cam),
            None => {
                let paper = {
                    let c = self.tabs[i].scene.camera.borrow();
                    pick(&c)
                };
                self.tabs[i].scene.paper_to_model(paper)
            }
        }
    }

    /// Projection + hit-test wires for the active pane. Inside a floating
    /// viewport (`edit_cam` Some) it returns the viewport camera and the live
    /// **model** wires, so wire / hatch picking lands on the entity under the
    /// cursor exactly where the GPU draws it; otherwise the model/paper camera
    /// and the normal hit-test wires. `bounds` is the pane-local rectangle.

    pub(in crate::app) fn pick_view(
        &self,
        i: usize,
        edit_cam: &Option<crate::scene::view::camera::Camera>,
        bounds: iced::Rectangle,
    ) -> (
        glam::Mat4,
        glam::DVec3,
        std::sync::Arc<Vec<crate::scene::WireModel>>,
    ) {
        match edit_cam {
            Some(cam) => {
                let wires = match self.tabs[i].scene.active_viewport {
                    Some(h) => self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height),
                    None => self.tabs[i].scene.hit_test_wires(),
                };
                (cam.view_proj_rte(bounds), cam.eye(), wires)
            }
            None => {
                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };
                (view_rot, eye, self.tabs[i].scene.hit_test_wires())
            }
        }
    }

    fn grip_edit_for_hit(
        &mut self,
        i: usize,
        handle: Handle,
        grip_id: usize,
        is_translate: bool,
        world: glam::DVec3,
    ) -> GripEdit {
        if !self.tabs[i].hot_grips.contains(&(handle, grip_id)) {
            self.tabs[i].hot_grips.clear();
            return GripEdit::single(handle, grip_id, is_translate, world);
        }

        let mut targets: Vec<GripTarget> = self.tabs[i]
            .selected_grip_handles
            .iter()
            .copied()
            .zip(self.tabs[i].selected_grips.iter())
            .filter(|(owner, grip)| self.tabs[i].hot_grips.contains(&(*owner, grip.id)))
            .filter(|(_, grip)| grip.id != crate::app::visibility::VIS_GRIP_ID)
            .map(|(owner, grip)| GripTarget {
                handle: owner,
                grip_id: grip.id,
                is_translate: grip.is_midpoint,
                last_world: grip.world,
            })
            .collect();

        // A midpoint/centre grip translates its whole entity. If that entity also
        // has hot point grips, applying both would move it twice; one translate
        // target owns the entity in that case.
        let translate_handles: rustc_hash::FxHashSet<_> = targets
            .iter()
            .filter(|target| target.is_translate)
            .map(|target| target.handle)
            .collect();
        let mut used_translates = rustc_hash::FxHashSet::default();
        targets.retain(|target| {
            if translate_handles.contains(&target.handle) {
                target.is_translate && used_translates.insert(target.handle)
            } else {
                true
            }
        });
        if targets.is_empty() {
            return GripEdit::single(handle, grip_id, is_translate, world);
        }
        GripEdit {
            handle,
            grip_id,
            origin_world: world,
            last_world: world,
            mode: GripEditMode::Stretch,
            targets,
        }
    }

    pub(in crate::app) fn update_grip_hover(&mut self, i: usize, p: iced::Point) {
        const HOVER_OPEN_MS: u128 = 1_000;
        const POPUP_DISMISS_PX: f32 = 80.0;
        if self.tabs[i].active_cmd.is_some()
            || self.tabs[i].active_grip.is_some()
            || self.tabs[i].selected_grips.is_empty()
        {
            self.grip_hover = None;
            self.grip_popup = None;
            return;
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        // In-viewport grips are model-space — project with the viewport camera
        // at the viewport's own rect; the cursor is mapped into that rect.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let hit = if let Some((cam, full)) = &edit_frame {
            let local = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: full.width,
                height: full.height,
            };
            let p_local = iced::Point::new(p.x - full.x, p.y - full.y);
            find_hit_grip_rte(
                p_local,
                &self.tabs[i].selected_grips,
                cam.view_proj_rte(local),
                cam.eye(),
                local,
            )
        } else if is_paper {
            let cam = self.tabs[i].scene.camera.borrow();
            let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
            let half_h = cam.ortho_size();
            let half_w = half_h * aspect;
            let tx = cam.target.x as f32;
            let ty = cam.target.y as f32;
            drop(cam);
            find_hit_grip_paper(
                p,
                &self.tabs[i].selected_grips,
                tx,
                ty,
                half_w,
                half_h,
                bounds,
            )
        } else {
            let cam = self.tabs[i].scene.camera.borrow();
            find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
        };
        match hit {
            Some((grip_index, grip_id, _, _)) => {
                let Some(&handle) = self.tabs[i].selected_grip_handles.get(grip_index) else {
                    self.grip_hover = None;
                    self.grip_popup = None;
                    return;
                };
                let same = self
                    .grip_hover
                    .as_ref()
                    .map_or(false, |h| h.handle == handle && h.grip_id == grip_id);
                if !same {
                    self.grip_hover = Some(crate::app::GripHover {
                        handle,
                        grip_id,
                        screen: p,
                        started: iced::time::Instant::now(),
                    });
                    self.grip_popup = None;
                } else if let Some(h) = self.grip_hover.as_mut() {
                    h.screen = p;
                }
                // Open popup once dwell crosses the threshold. The visibility
                // grip has its own click-to-open dropdown, so it gets no
                // hover grip-menu.
                if self.grip_popup.is_none()
                    && grip_id != crate::app::visibility::VIS_GRIP_ID
                    && self
                        .grip_hover
                        .as_ref()
                        .map_or(false, |h| h.started.elapsed().as_millis() >= HOVER_OPEN_MS)
                {
                    let entity_opt = self.tabs[i].scene.document.get_entity(handle);
                    if let Some(e) = entity_opt {
                        use crate::entities::traits::EntityTypeOps;
                        let items = e.grip_menu(grip_id);
                        if !items.is_empty() {
                            self.grip_popup = Some(crate::app::GripPopup {
                                handle,
                                grip_id,
                                anchor: p,
                                items,
                                selected: 0,
                            });
                        }
                    }
                }
            }
            None => {
                self.grip_hover = None;
                if let Some(popup) = &self.grip_popup {
                    let dx = p.x - popup.anchor.x;
                    let dy = p.y - popup.anchor.y;
                    if (dx * dx + dy * dy).sqrt() > POPUP_DISMISS_PX {
                        self.grip_popup = None;
                    }
                }
            }
        }
    }

    pub(super) fn on_tick(&mut self, t: Instant) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.update(t - self.start);

        // If the camera moved since we last synced, write it back to
        // the document and mark the file dirty.
        let gen = self.tabs[i].scene.camera_generation;
        if gen != self.tabs[i].last_synced_camera_gen {
            self.tabs[i].last_synced_camera_gen = gen;
            if self.tabs[i].active_block_edit.is_some() {
                let camera = self.tabs[i].scene.camera.borrow().clone();
                if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
                    session.editor_camera = camera;
                }
            } else if self.tabs[i].scene.active_viewport.is_some() {
                // Floating-viewport navigation writes its camera straight to
                // the viewport entity. Syncing the separate main camera here
                // can overwrite the saved Model/Paper view with a camera that
                // does not own this change.
                self.tabs[i].dirty = true;
            } else if self.tabs[i].scene.sync_camera_to_document() {
                self.tabs[i].dirty = true;
            }
        }

        // Surface any plugin-guard panic messages that piled up since
        // the last tick (the host singleton isn't reachable from inside
        // the plugin hooks, so they queue and we flush here). (#145)
        #[cfg(not(target_arch = "wasm32"))]
        for msg in crate::plugin::drain_errors() {
            self.command_line.push_error(&msg);
        }

        Task::none()
    }

    pub(super) fn on_set_render_mode(
        &mut self,
        mode: acadrust::entities::ViewportRenderMode,
    ) -> Task<Message> {
        use acadrust::entities::ViewportRenderMode as M;
        let i = self.active_tab;
        let label = match mode {
            M::Wireframe2D => "Wireframe 2D",
            M::Wireframe3D => "Wireframe 3D",
            M::HiddenLine => "Hidden Line",
            M::FlatShaded => "Flat Shaded",
            M::GouraudShaded => "Gouraud Shaded",
            M::FlatShadedWithEdges => "Flat Shaded + Edges",
            M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
        };
        // In a paper layout with an active (double-clicked)
        // viewport, the picker drives that viewport entity's own
        // render mode; the model-layout tab style is untouched.
        if self.tabs[i].scene.set_active_viewport_render_mode(mode) {
            self.tabs[i].scene.bump_geometry_no_blocks();
            self.command_line
                .push_output(crate::tf!("Viewport visual style: {label}").as_ref());
            return Task::none();
        }
        self.tabs[i].render_mode = mode;
        // Write the style onto the active Model tile alone so it
        // sticks when that tile loses focus and the other tiles keep
        // their own styles.
        self.tabs[i].scene.set_active_model_tile_render_mode(mode);
        // Keep the legacy `wireframe` bool synced — both wireframe
        // modes set it, everything else clears it.
        let wf = matches!(mode, M::Wireframe2D | M::Wireframe3D);
        self.tabs[i].wireframe = wf;
        self.ribbon.set_wireframe(wf);
        self.tabs[i].visual_style = label.into();
        // Re-upload face3d fills on the next frame — the render
        // pipeline keys its upload cache off `geometry_epoch`.
        self.tabs[i].scene.bump_geometry_no_blocks();
        self.command_line
            .push_output(crate::tf!("Visual style: {label}").as_ref());
        Task::none()
    }

    pub(super) fn on_cursor_moved(
        &mut self,
        p: Point,
        expected_viewport: Option<acadrust::Handle>,
    ) -> Task<Message> {
        if self.color_pick_target.is_some() {
            return Task::none();
        }

        // `p` is relative to the ViewCube hit area's top-left. Map
        // it back to full-canvas coordinates so ViewportClick's
        // hit-test lines up. The hit area sits in the top-right of
        // the full canvas in model space, or of the active
        // viewport's screen rectangle in a paper layout.
        let i = self.active_tab;
        if self.tabs[i].scene.active_viewport != expected_viewport
            || (expected_viewport.is_none()
                && self.tabs[i].scene.current_layout != "Model")
        {
            return Task::none();
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        let (ox, oy) = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
        {
            Some(rect) => (
                rect.x + rect.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                rect.y + VIEWCUBE_PAD,
            ),
            None => {
                // Model layout: the cube sits in the active tile's
                // top-right corner.
                let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                (
                    tb.x + tb.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                    tb.y + VIEWCUBE_PAD,
                )
            }
        };
        self.cursor_pos = iced::Point::new(ox + p.x, oy + p.y);

        // Drive the ViewCube hover highlight directly from this
        // message — it fires whenever the cube's hit-area overlay
        // sees motion, so we don't depend on the shader widget's
        // `Program::update` receiving the same event (overlays sit
        // above the shader and can mask it). Map the cursor into
        // the active viewport's local box and use that box's size,
        // since that's where the cube is actually drawn.
        let tile = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
        {
            Some(rect) => rect,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let cam_rot = self.tabs[i].scene.active_view_rotation_mat();
        let hover = hover_id(
            self.cursor_pos.x - tile.x,
            self.cursor_pos.y - tile.y,
            tile.width,
            tile.height,
            cam_rot,
            VIEWCUBE_PX,
        );
        self.tabs[i].scene.viewcube_hover.set(hover);
        Task::none()
    }

    pub(super) fn on_viewport_move(&mut self, p: Point) -> Task<Message> {
        // A ribbon dropdown is open over the viewport. Its backdrop
        // cannot swallow cursor motion — in iced 0.14 mouse_area/opaque
        // capture only button presses, never CursorMoved — so the move
        // leaks through the stack to the pane mouse_area beneath and
        // would track the crosshair over the dropdown's empty areas.
        // Drop the move here instead. (#227)
        if self.ribbon.open_dropdown.is_some() || self.color_pick_target.is_some() {
            return Task::none();
        }
        let i = self.active_tab;
        let perf_move = crate::perf::enabled();
        let move_started = Instant::now();

        // UCS icon grip drag: map the cursor onto the UCS plane and
        // slide the origin / rotate the axis. Short-circuits pan & snap.
        if let Some(kind) = self.ucs_grip_drag {
            self.drag_ucs_grip(i, kind, p);
            self.tabs[i].scene.selection.borrow_mut().last_move_pos = Some(p);
            return Task::none();
        }

        // Keep the ViewCube hover in sync as the cursor leaves the
        // hit-area overlay and moves over the rest of the viewport.
        // `hover_id` returns None outside the cube box, which clears
        // any stale highlight from the previous `CursorMoved`.
        let (svw, svh) = self.tabs[i].scene.selection.borrow().vp_size;
        let cube_tile = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (svw, svh)))
        {
            Some(rect) => rect,
            None => self.tabs[i].scene.active_model_tile_bounds(svw, svh),
        };
        let cam_rot = self.tabs[i].scene.active_view_rotation_mat();
        self.tabs[i].scene.viewcube_hover.set(hover_id(
            p.x - cube_tile.x,
            p.y - cube_tile.y,
            cube_tile.width,
            cube_tile.height,
            cam_rot,
            VIEWCUBE_PX,
        ));

        let navigating = self.tabs[i].scene.selection.borrow().middle_down;
        if navigating {
            self.clear_navigation_hover(i);
        } else {
            // Multi-functional grip hover: detect cursor sitting on a
            // selected entity's grip and, after a dwell, open the
            // popup menu. See scene::model::object::GripMenuItem.
            self.update_grip_hover(i, p);

            // UCS icon hover highlight (suppressed mid grip-drag).
            self.ucs_icon_hover = self.ucs_grip_drag.is_none()
                && self
                    .ucs_icon_hit_info(i, svw, svh)
                    .map(|h| over_ucs_icon(p, &h))
                    .unwrap_or(false);
        }

        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.last_move_pos = Some(p);

        if sel.left_down {
            let press = sel.left_press_pos.unwrap_or(p);
            let dx = p.x - press.x;
            let dy = p.y - press.y;
            let dist2 = dx * dx + dy * dy;
            let elapsed_ms = sel
                .left_press_time
                .map(|t| Instant::now().duration_since(t).as_millis())
                .unwrap_or(u128::MAX);
            if !sel.left_dragging && elapsed_ms >= POLY_START_DELAY_MS && dist2 > 9.0 {
                sel.left_dragging = true;
                if self.pick_drag_rect {
                    // PICKDRAG 1 (#226): press-drag spans a RECTANGLE
                    // marquee — drive the existing box machinery (its
                    // overlay and completion) instead of the lasso.
                    sel.box_anchor = Some(press);
                    sel.box_current = Some(p);
                    sel.box_crossing = p.x < press.x;
                } else {
                    sel.poly_active = true;
                    sel.poly_crossing = p.x < press.x;
                    sel.poly_points.clear();
                    sel.poly_points.push(press);
                    sel.poly_points.push(p);
                }
            } else if sel.left_dragging && sel.poly_active {
                if sel.poly_points.last().map_or(true, |lp| {
                    let ddx = p.x - lp.x;
                    let ddy = p.y - lp.y;
                    ddx * ddx + ddy * ddy > 16.0
                }) {
                    sel.poly_points.push(p);
                }
            } else if sel.left_dragging {
                if let Some(a) = sel.box_anchor {
                    sel.box_current = Some(p);
                    sel.box_crossing = p.x < a.x;
                }
            }
        } else if sel.box_anchor.is_some() {
            sel.box_current = Some(p);
            if let Some(a) = sel.box_anchor {
                sel.box_crossing = p.x < a.x;
            }
        }

        let (mid_down, mid_last, vp_size) = (sel.middle_down, sel.middle_last_pos, sel.vp_size);
        if mid_down {
            if let Some(last) = mid_last {
                let (dx, dy) = (p.x - last.x, p.y - last.y);
                // Shift+MMB drag orbits the model view instead of panning
                // — the requested Zoom=wheel / Pan=MMB / Rotate=Shift+MMB
                // scheme (#229). Floating viewports and paper keep the
                // plain MMB pan.
                if self.shift_down || self.tabs[i].orbit_mode {
                    if self.tabs[i].scene.active_viewport.is_some() {
                        // Orbit the floating viewport's own model view.
                        drop(sel);
                        self.tabs[i].scene.orbit_active_viewport(dx, dy);
                        self.tabs[i].scene.camera_generation += 1;
                        self.tabs[i]
                            .scene
                            .record_nav_perf(crate::scene::NavPerfOp::Rotate, move_started);
                        self.tabs[i].scene.selection.borrow_mut().middle_last_pos = Some(p);
                        return Task::none();
                    } else if self.tabs[i].scene.current_layout == "Model" {
                        if sel.orbit_pivot.is_none() {
                            // Selection centre when something is selected,
                            // otherwise the point under the cursor. (#229)
                            sel.orbit_pivot = self.tabs[i]
                                .scene
                                .orbit_pivot()
                                .or(Some(self.tabs[i].last_cursor_world));
                        }
                        let pivot = sel.orbit_pivot;
                        drop(sel);
                        self.tabs[i].scene.camera.borrow_mut().orbit(dx, dy, pivot);
                        self.tabs[i].scene.camera_generation += 1;
                        self.tabs[i]
                            .scene
                            .record_nav_perf(crate::scene::NavPerfOp::Rotate, move_started);
                        self.tabs[i].scene.selection.borrow_mut().middle_last_pos = Some(p);
                        return Task::none();
                    }
                    // Paper sheet is top-locked. Shift+MMB keeps its existing
                    // pan fallback; the explicit orbit tool does nothing here.
                    if self.tabs[i].orbit_mode {
                        sel.middle_last_pos = Some(p);
                        return Task::none();
                    }
                }
                // Pan scale uses the active tile's size (ortho size
                // is relative to viewport height), so a tiled pane
                // pans at the correct rate.
                let bounds = self.tabs[i]
                    .scene
                    .active_model_tile_bounds(vp_size.0, vp_size.1);
                // Drop `sel` before calling mutable scene methods.
                drop(sel);
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i].scene.pan_active_viewport(dx, dy, bounds);
                    // Bump so the GPU re-uploads the viewport's re-culled
                    // wire set — otherwise newly-revealed lines stay
                    // invisible until MSPACE is exited.
                    self.tabs[i].scene.camera_generation += 1;
                } else {
                    // `bounds` is the active tile; pan by its height so
                    // the point under the cursor tracks correctly.
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .pan_screen(dx, dy, bounds.height);
                    self.tabs[i].scene.camera_generation += 1;
                    // Keep an in-progress box selection pinned to the
                    // drawing as the view pans under it (#234).
                    self.reproject_box_anchor(i, vp_size.0, vp_size.1);
                }
                self.tabs[i]
                    .scene
                    .record_nav_perf(crate::scene::NavPerfOp::Pan, move_started);
                self.tabs[i].scene.selection.borrow_mut().middle_last_pos = Some(p);
                return Task::none();
            }
            sel.middle_last_pos = Some(p);
        }

        let dragging = sel.left_down || sel.right_down || sel.middle_down;
        let vp_size = sel.vp_size;
        drop(sel);

        // The active pane already follows the cursor (each pane's own
        // mouse_area focuses it via `PaneMove` → `focus_model_pane`), so
        // the camera + tile bounds used for picking below are already the
        // pane the cursor is in.

        // Tile-relative picking: shadow `p` with the cursor mapped
        // into the active Model tile and `vp_size` with the tile's
        // size, so every pick / snap / view_proj below operates in
        // the active pane. `p_full` keeps the canvas-space cursor
        // for screen overlays (cursor marker, snap glyph).
        let p_full = p;
        // Inside a floating viewport (MSPACE) the active "pane" is the
        // viewport's own screen rectangle and its own camera — the very
        // camera the GPU draws the content with. Routing picking / snap
        // / projection through it makes in-viewport editing behave like
        // the main model view (model coords, no paper round-trip) and
        // track the viewport's pan / zoom / twist exactly.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(vp_size);
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i]
                .scene
                .active_model_tile_bounds(vp_size.0, vp_size.1),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_b.x,
            y: p_full.y - tile_b.y,
        };
        let vp_size = (tile_b.width, tile_b.height);

        // ── Grip drag ─────────────────────────────────────────────
        if let Some(grip) = self.tabs[i].active_grip.clone() {
            let grip_started = Instant::now();
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            // In a viewport, pick the cursor's model point with the
            // viewport camera directly; otherwise project on the paper
            // sheet and map to model. Either way `raw` is model space.
            let raw = match &edit_cam {
                Some(cam) => cam.pick_on_target_plane(p, bounds),
                None => {
                    let paper = self.tabs[i]
                        .scene
                        .camera
                        .borrow()
                        .pick_on_target_plane(p, bounds);
                    self.tabs[i].scene.paper_to_model(paper)
                }
            };
            let (view_rot, eye) = match &edit_cam {
                Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (cam.view_proj_rte(bounds), cam.eye())
                }
            };

            let mut seen_handles = rustc_hash::FxHashSet::default();
            let edited_handles: Vec<_> = grip
                .targets
                .iter()
                .map(|target| target.handle)
                .filter(|handle| seen_handles.insert(*handle))
                .collect();

            // First move of this drag: hide every edited entity from the base
            // tessellation so subsequent moves refresh only the overlay.
            if self.grip_preview_handles != edited_handles {
                if self.grip_dirty_before.is_none() {
                    self.grip_dirty_before = Some(self.tabs[i].dirty);
                }
                for handle in std::mem::take(&mut self.grip_preview_handles) {
                    self.tabs[i].scene.preview_hidden.remove(&handle);
                }
                // Interactive Add Vertex seeds this with the entity from
                // before insertion so append + placement is one undo step.
                // Normal grip drags still snapshot their current entities here.
                if self.grip_originals.is_empty() {
                    self.grip_originals = edited_handles
                        .iter()
                        .filter_map(|&handle| {
                            self.tabs[i]
                                .scene
                                .document
                                .get_entity(handle)
                                .cloned()
                                .map(|entity| (handle, entity))
                        })
                        .collect();
                }
                for &handle in &edited_handles {
                    self.tabs[i].scene.preview_hidden.insert(handle);
                }
                let changes: Vec<_> = edited_handles
                    .iter()
                    .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.grip_preview_handles = edited_handles.clone();
                // Snapshot the entity's glyph quads once so each move can
                // slide the already-shaped text rather than re-shaping it
                // (issue #316). The fast slide path only runs for a rigid
                // whole-entity move of pure text: no wire geometry (so a
                // dimension re-tessellates) and a Square insertion grip (so
                // an MTEXT width handle, a Triangle, still re-tessellates so
                // the re-wrap is exact).
                let snap = self.tabs[i].scene.wire_models_for(&edited_handles);
                self.grip_text_verts = snap
                    .iter()
                    .flat_map(|w| w.text_verts.iter().copied())
                    .collect();
                let square_grip = self.tabs[i]
                    .selected_grip_handles
                    .iter()
                    .copied()
                    .zip(self.tabs[i].selected_grips.iter())
                    .find(|(owner, grip_def)| {
                        *owner == grip.handle && grip_def.id == grip.grip_id
                    })
                    .map(|(_, grip_def)| {
                        grip_def.shape == crate::scene::model::object::GripShape::Square
                    })
                    .unwrap_or(false);
                self.grip_text_slide = edited_handles.len() == 1
                    && grip.targets.len() == 1
                    && !self.grip_text_verts.is_empty()
                    && snap.iter().all(|w| w.points.is_empty())
                    && square_grip;
            }
            let setup_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            let snap_started = Instant::now();

            // The edited entity is hidden, so it's already absent from
            // `hit_test_wires` — snap against the set directly, no clone
            // and no self-snap.
            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                raw,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            // The engaged grip is the rubber-band origin. Perpendicular
            // snapping must drop its foot from this point, including when a
            // hot-grip set is moved by the same drag vector.
            self.snapper.from_point = Some(grip.origin_world.as_vec3());
            let (go, gr) = self.tabs[i].ucs_grid_basis();
            // `raw` is already model space (viewport camera or paper→model),
            // and the wires are model space, so the snap result is model.
            let snap_hit =
                self.snapper
                    .snap(raw, p, &snap_candidates, view_rot, eye, bounds, go, gr);
            let mut snapped = snap_hit.map(|s| s.world).unwrap_or(raw);
            self.tabs[i].snap_result = snap_hit;
            if let Some(s) = self.tabs[i].snap_result.as_mut() {
                s.screen.x += tile_b.x;
                s.screen.y += tile_b.y;
            }

            // Hard axis lock (#312) works for grip drags too: Shift
            // pins the drag to the nearest polar/ortho ray from the
            // grip's origin, osnap hits included.
            if self.shift_down {
                if self.axis_lock_dir.is_none() {
                    let ucs_xf = self.tabs[i].ucs_xform();
                    self.axis_lock_dir = axis_lock_capture(
                        raw,
                        grip.origin_world,
                        self.polar_mode,
                        self.polar_increment_deg,
                        &ucs_xf,
                    );
                }
            } else {
                self.axis_lock_dir = None;
            }
            if let Some(dir) = self.axis_lock_dir {
                snapped = axis_lock_apply(snapped, grip.origin_world, dir);
            } else if !snap_hit.is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid) {
                let base = grip.origin_world;
                let ucs_xf = self.tabs[i].ucs_xform();
                if self.ortho_mode {
                    snapped = ortho_constrain(snapped, base, &ucs_xf);
                } else if self.polar_mode {
                    snapped = polar_constrain_near(
                        snapped,
                        base,
                        self.polar_increment_deg,
                        view_rot,
                        eye,
                        bounds,
                        self.snapper.osnap_radius_px,
                        &ucs_xf,
                    );
                }
            }

            let snap_ms = snap_started.elapsed().as_secs_f64() * 1000.0;
            let apply_started = Instant::now();
            let delta = snapped - grip.last_world;
            let lengthen = grip.mode == GripEditMode::Lengthen;
            let actions: Vec<_> = if lengthen {
                Vec::new()
            } else {
                grip.targets
                    .iter()
                    .map(|target| {
                        let apply = if target.is_translate {
                            GripApply::Translate(delta)
                        } else {
                            GripApply::Absolute(target.last_world + delta)
                        };
                        (target.handle, target.grip_id, apply)
                    })
                    .collect()
            };
            if lengthen {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(handle, _)| *handle == grip.handle)
                    .map(|(_, entity)| entity.clone());
                if let Some(original) = original {
                    let action = crate::scene::model::object::GripMenuAction::Lengthen;
                    let value = crate::scene::view::dispatch::grip_menu_point_value(
                        &original,
                        grip.grip_id,
                        action,
                        snapped,
                    );
                    if let Some(value) = value {
                        if let Some(current) =
                            self.tabs[i].scene.document.get_entity_mut(grip.handle)
                        {
                            *current = original;
                            crate::entities::traits::EntityTypeOps::apply_grip_menu_value(
                                current,
                                grip.grip_id,
                                action,
                                value,
                            );
                        }
                    }
                }
            }
            // Arc point grips are coupled: moving one point must rebuild the
            // circle from the drag-start start/middle/end set, otherwise the
            // supposedly fixed points drift a little on every mouse event.
            let mut arc_grip_edits: rustc_hash::FxHashMap<
                Handle,
                Vec<(usize, glam::DVec3)>,
            > = rustc_hash::FxHashMap::default();
            for (handle, grip_id, apply) in &actions {
                if let GripApply::Absolute(point) = apply {
                    if (1..=3).contains(grip_id) {
                        arc_grip_edits
                            .entry(*handle)
                            .or_default()
                            .push((*grip_id, *point));
                    }
                }
            }
            let mut rebuilt_arcs = rustc_hash::FxHashSet::default();
            for (handle, edits) in arc_grip_edits {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(original_handle, _)| *original_handle == handle)
                    .map(|(_, entity)| entity.clone());
                let Some(original) = original else {
                    continue;
                };
                let Some(current) = self.tabs[i].scene.document.get_entity_mut(handle) else {
                    continue;
                };
                if crate::scene::view::dispatch::refit_arc_grips(
                    current,
                    &original,
                    &edits,
                )
                .is_some()
                {
                    rebuilt_arcs.insert(handle);
                }
            }
            let refit_arc_targets: Vec<_> = grip
                .targets
                .iter()
                .filter_map(|target| {
                    let original = self
                        .grip_originals
                        .iter()
                        .find(|(handle, _)| *handle == target.handle)
                        .map(|(_, entity)| entity)?;
                    let current = self.tabs[i]
                        .scene
                        .document
                        .get_entity(target.handle)?;
                    added_arc_bulge(original, current, target.grip_id)
                        .map(|bulge| (target.handle, target.grip_id, bulge))
                })
                .collect();
            for (handle, grip_id, apply) in actions {
                if !rebuilt_arcs.contains(&handle) {
                    self.tabs[i].scene.apply_grip(handle, grip_id, apply);
                }
            }
            for (handle, vertex_id, original_bulge) in refit_arc_targets {
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::entities::lwpolyline::refit_added_arc_vertex(
                        entity,
                        vertex_id,
                        original_bulge,
                    );
                }
            }
            self.tabs[i].scene.set_preview_hatches(&edited_handles);
            self.tabs[i].dirty = true;
            if let Some(active) = self.tabs[i].active_grip.as_mut() {
                active.last_world = snapped;
                for target in &mut active.targets {
                    target.last_world += delta;
                }
            }
            let apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
            let preview_started = Instant::now();
            // Overlay the moved entity (hidden from the base). Pure text
            // moved as a whole slides its drag-start glyphs — no re-shaping
            // and no wire re-tess. Anything else (wire geometry, or a point
            // grip that reshapes text) is re-tessellated for an exact
            // preview; either way the glyphs ride the preview-text buffer
            // so dragged text never vanishes mid-drag (issue #316).
            if self.grip_text_slide {
                let d = snapped - grip.origin_world;
                let slid = crate::scene::pipeline::text_gpu::translate_verts(
                    &self.grip_text_verts,
                    [d.x, d.y, d.z],
                );
                self.tabs[i].scene.set_preview_text(slid);
                self.tabs[i].scene.set_preview_wires(Vec::new());
            } else {
                // Wire geometry, or a reshape grip: re-tessellate. The
                // preview WireModels carry the entity's glyphs (gathered
                // for the preview-text buffer in the render path), so a
                // dimension / MTEXT-width drag keeps its text visible too.
                let preview = self.tabs[i].scene.wire_models_for(&edited_handles);
                self.tabs[i].scene.set_preview_wires(preview);
            }
            let preview_ms = preview_started.elapsed().as_secs_f64() * 1000.0;
            let geometry_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            self.refresh_selected_grips();
            let grips_ms = grip_started.elapsed().as_secs_f64() * 1000.0 - geometry_ms;
            // Properties are refreshed when the grip is committed or cancelled.
            // Rebuilding the inspector on every pointer event adds no drawing
            // value and can monopolize the UI thread during a drag.
            let total_ms = grip_started.elapsed().as_secs_f64() * 1000.0;
            if perf_move && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] grip-move          {:>7.1}ms setup={:.1} snap={:.1} apply={:.1} preview={:.1} grips={:.1}",
                    total_ms,
                    setup_ms,
                    snap_ms,
                    apply_ms,
                    preview_ms,
                    grips_ms,
                );
            }
            return Task::none();
        }

        // Keep the coordinate readout live on every move, even with no
        // active command. When a command is running the snap path below
        // overwrites this with the snapped point.
        {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };
            let world = self.cursor_model_point(i, &edit_cam, p, bounds);
            self.tabs[i].last_cursor_world = world;
        }

        // Rollover highlight: when idle (no active command, no
        // drag), defer the pick until the cursor stops. The full
        // pick (wires + hatches + block hatches + shaded meshes) is
        // O(N) per frame and stalls the cursor on large drawings,
        // so each move resets the dwell timer — `HoverDwellTick` runs the hit-test only
        // once the cursor has been still for `HOVER_DWELL_MS`.
        if !dragging && self.tabs[i].active_cmd.is_none() {
            // On dense drawings, clearing a rollover immediately schedules a
            // second full scene frame just as motion resumes. Keep the previous
            // highlight until the next settled pick replaces it.
            if self.tabs[i].scene.last_tess_wires.get()
                < crate::app::HOVER_DWELL_DENSE_WIRES
            {
                self.tabs[i].scene.set_hover_highlight(None);
            }
            self.hover_dwell = Some(crate::app::HoverDwell {
                last_move_at: Instant::now(),
                point: p,
                tile_size: vp_size,
                tab: i,
            });
        } else {
            // Suppress the rollover during a command or a drag.
            self.tabs[i].scene.set_hover_highlight(None);
            self.hover_dwell = None;
        }

        if self.tabs[i].active_cmd.is_some() {
            let (vw, vh) = vp_size;
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };
            // Inside a floating viewport the cursor, camera and wires are
            // all model-space (the viewport's own camera draws the
            // content), so snap / hit-test / preview run exactly like the
            // main model view — no paper projection, tracks pan/zoom/twist.
            let cursor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
            let (view_rot, eye, grid_spacing) = match &edit_cam {
                Some(cam) => (
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    crate::ui::overlay::compute_grid_step(cam.distance, cam.fov_y, bounds),
                ),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        crate::ui::overlay::compute_grid_step(cam.distance, cam.fov_y, bounds),
                    )
                }
            };
            // Sync grid-snap spacing to the adaptive spacing of the visible grid.
            self.snapper.grid_spacing = grid_spacing;
            // Cursor and wires are model-space; the snap result is model.
            let snap_cursor = cursor_world;

            let all_wires =
                if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                    self.tabs[i]
                        .scene
                        .model_wires_for_viewport_arc(h, bounds.height)
                } else {
                    self.tabs[i].scene.hit_test_wires()
                };
            let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                all_wires,
                snap_cursor,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );
            let needs_entity = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false);
            let needs_structure = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_structure_point_pick())
                .unwrap_or(false);
            let is_gathering = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.is_selection_gathering())
                .unwrap_or(false);
            // A selection-window corner (STRETCH crossing window) is a
            // free point — Ortho/Polar must not pin it to an axis through
            // the first corner, or the rectangle collapses to a line
            // (#291).
            let is_window_corner = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.window_corner_pick())
                .unwrap_or(false);
            let needs_tan = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_tangent_pick())
                .unwrap_or(false);
            self.tabs[i].snap_result = if needs_entity || is_gathering || needs_structure {
                None
            } else if needs_tan {
                self.snapper.snap_tangent_only(
                    snap_cursor.as_vec3(),
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                )
            } else {
                let (go, gr) = self.tabs[i].ucs_grid_basis();
                // The snapper is a screen-space (f32) engine; the f64
                // base only matters for typed-input precision, so hand it
                // the downcast point here.
                self.snapper.from_point = self.last_point.map(|p| p.as_vec3());
                self.snapper.snap(
                    snap_cursor,
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    go,
                    gr,
                )
            };

            // Object Snap Tracking: update dwell, then align the cursor
            // to a tracking ray (and store the alignment so a typed
            // distance can place a point along it — issue #69).
            let otrack_hit = {
                let snap_world = self.tabs[i].snap_result.map(|s| s.world);
                self.snapper.update_otrack_dwell(
                    snap_world,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    Instant::now(),
                );
                // Parallel snap: acquire the reference line under the
                // cursor (independent of OTRACK). (#277)
                self.snapper.update_parallel(
                    cursor_world.as_vec3(),
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    Instant::now(),
                );
                if self.tabs[i].snap_result.is_none() {
                    // A window corner is a free point, so neither the
                    // Polar increment nor the Ortho lock may pull the
                    // OTRACK alignment onto an axis (#291).
                    let step = if self.polar_mode && !is_window_corner {
                        Some(self.polar_increment_deg)
                    } else {
                        None
                    };
                    let (_, ucs_x, ucs_y, _) = self.tabs[i].ucs_xform().axes();
                    self.snapper.otrack_snap(
                        cursor_world,
                        view_rot,
                        eye,
                        bounds,
                        step,
                        self.last_point,
                        self.ortho_mode && !is_window_corner,
                        ucs_x,
                        ucs_y,
                    )
                } else {
                    None
                }
            };
            self.otrack_active = otrack_hit.map(|h| (h.base, h.dir));

            // Parallel snap: with nothing else snapped or tracked, lock
            // the point onto the line through last_point parallel to the
            // acquired reference, and drive the alignment guide off it.
            // (#277)
            if self.tabs[i].snap_result.is_none() && otrack_hit.is_none() {
                if let Some(par) = self.snapper.parallel_snap(
                    cursor_world.as_vec3(),
                    self.last_point.map(|p| p.as_vec3()),
                    view_rot,
                    eye,
                    bounds,
                ) {
                    if let (Some(base), Some((dir, _))) =
                        (self.last_point, self.snapper.parallel_ref)
                    {
                        self.otrack_active = Some((base, dir.as_dvec3()));
                    }
                    self.tabs[i].snap_result = Some(par);
                }
            }

            let effective = {
                let mut pt: glam::DVec3 = if let Some(h) = otrack_hit {
                    // Tracking alignment wins over the free cursor;
                    // ortho/polar don't re-constrain it.
                    h.aligned
                } else {
                    // Snap runs in model space (viewport camera or the
                    // model/paper view), so the result is already model.
                    let mut pt = self.tabs[i]
                        .snap_result
                        .map(|s| s.world)
                        .unwrap_or(cursor_world);
                    // Hard axis lock (#312): Shift during a rubber-band
                    // pick captures the nearest polar/ortho direction
                    // and pins the pick to that ray — even an osnap hit
                    // contributes only its along-axis component.
                    // Only while an active command is asking for a
                    // POINT — Shift has other meanings during entity
                    // picks (TRIM/EXTEND swap) and selection.
                    let wants_point = self.tabs[i].active_cmd.as_ref().is_some_and(|c| {
                        !c.needs_entity_pick()
                            && !c.needs_tangent_pick()
                            && !c.is_selection_gathering()
                    });
                    if let Some(base) = self.last_point {
                        if self.shift_down && wants_point && !is_window_corner {
                            if self.axis_lock_dir.is_none() {
                                let ucs_xf = self.tabs[i].ucs_xform();
                                self.axis_lock_dir = axis_lock_capture(
                                    cursor_world,
                                    base,
                                    self.polar_mode,
                                    self.polar_increment_deg,
                                    &ucs_xf,
                                );
                            }
                        } else if !self.shift_down {
                            self.axis_lock_dir = None;
                        }
                    } else if !self.shift_down {
                        self.axis_lock_dir = None;
                    }
                    if let (Some(dir), Some(base)) = (self.axis_lock_dir, self.last_point) {
                        pt = axis_lock_apply(pt, base, dir);
                        pt
                    } else {
                        // Object snap wins over ortho/polar: a snapped point
                        // is taken as-is so it isn't pulled onto the ortho
                        // axis. Grid snap is positional, not an object snap,
                        // so it still combines with ortho/polar. (#132)
                        let osnap_locked = self.tabs[i]
                            .snap_result
                            .is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid);
                        if !osnap_locked && !is_window_corner {
                            if let Some(base) = self.last_point {
                                let ucs_xf = self.tabs[i].ucs_xform();
                                if self.ortho_mode {
                                    pt = ortho_constrain(pt, base, &ucs_xf);
                                } else if self.polar_mode {
                                    pt = polar_constrain_near(
                                        pt,
                                        base,
                                        self.polar_increment_deg,
                                        view_rot,
                                        eye,
                                        bounds,
                                        self.snapper.osnap_radius_px,
                                        &ucs_xf,
                                    );
                                }
                            }
                        }
                        pt
                    }
                };
                // Clamp to world XY only when no UCS is active; with a
                // UCS the point already lies on the UCS XY plane.
                if self.tabs[i].active_cmd.is_some() && self.tabs[i].active_ucs.is_none() {
                    pt.z = 0.0;
                }
                pt
            };
            // Dynamic-input locked fields constrain the preview point
            // (#356): a typed angle pins the direction, a typed
            // distance pins the radius — the same resolution the Enter
            // commit uses, so preview and commit agree. The lock wins
            // over osnap/ortho/polar.
            let effective = {
                let locked = self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some());
                if locked {
                    self.tabs[i].last_cursor_world = effective;
                    self.dyn_resolve_point().unwrap_or(effective)
                } else {
                    effective
                }
            };
            self.tabs[i].last_cursor_world = effective;
            self.tabs[i].last_cursor_screen = p_full;
            // Project the step anchor (an explicit `dyn_anchor` or the
            // last point) so the dynamic-input overlay can place its
            // guide geometry and labels.
            // `proj` returns a pane-local pixel; shift by the active pane
            // origin (`tile_b`, the viewport rect inside a viewport) so
            // the DYN guide/labels share the canvas frame with
            // `last_cursor_screen` (= p_full, canvas space).
            let proj = |bp: glam::DVec3| {
                let ndc = view_rot.project_point3((bp - eye).as_vec3());
                iced::Point::new(
                    (ndc.x + 1.0) * 0.5 * bounds.width + tile_b.x,
                    (1.0 - ndc.y) * 0.5 * bounds.height + tile_b.y,
                )
            };
            // Anchors are stored in model coords and `proj` (view_rot /
            // eye) is the model→screen view — the viewport camera inside
            // a viewport, the model/paper camera otherwise — so feed them
            // straight through; no paper mapping needed.
            let anchor = self.tabs[i].dyn_anchor.or(self.last_point);
            let dyn_ref = self.tabs[i].dyn_ref;
            let lps = anchor.map(|a| proj(a));
            let drs = dyn_ref.map(|r| proj(r));
            self.tabs[i].last_point_screen = lps;
            self.tabs[i].dyn_ref_screen = drs;

            // Point-picked selection window (STRETCH): draw a filled
            // crossing marquee from the first corner to the cursor so it
            // reads like a normal box selection, not a bare outline (#291).
            let window_first = self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|c| c.window_first_corner());
            self.tabs[i].scene.selection.borrow_mut().preview_box = if is_window_corner {
                window_first.map(|c1| (proj(c1), p_full, true))
            } else {
                None
            };

            // Entity-pick previews (TRIM/EXTEND/FILLET…) compare the
            // cursor against WCS document entities and return WCS wires.
            // `effective` is offset-relative, so build a WCS copy for
            // the click and shift the returned wires back to the
            // offset-relative frame the renderer expects (model space
            // only; paper-space entities use sheet coordinates).
            let wo_local = if self.tabs[i].scene.current_layout == "Model" {
                [0.0_f64; 3]
            } else {
                [0.0; 3]
            };
            let wo_f = glam::DVec3::new(wo_local[0], wo_local[1], wo_local[2]);
            let effective_wcs = effective + wo_f;

            // Orange object snap (plugin commands implement resolve_object_pick).
            if needs_structure {
                use crate::snap::{SnapResult, SnapType};
                let pick = self.tabs[i].active_cmd.as_ref().and_then(|c| {
                    c.resolve_object_pick(
                        &self.tabs[i].scene,
                        effective.x as f64,
                        effective.y as f64,
                    )
                });
                if let Some(pick) = pick {
                    let world = glam::DVec3::new(pick.x, pick.y, effective.z);
                    let ndc = view_rot.project_point3((world - eye).as_vec3());
                    let screen = iced::Point::new(
                        (ndc.x + 1.0) * 0.5 * bounds.width,
                        (1.0 - ndc.y) * 0.5 * bounds.height,
                    );
                    self.tabs[i].snap_result = Some(SnapResult {
                        world,
                        screen,
                        snap_type: SnapType::ObjectPick,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                    });
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        cmd.set_acquisition_hint(Some(pick.label));
                    }
                } else if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                    cmd.set_acquisition_hint(None);
                }
            }

            // Snap glyph is positioned in canvas space; shift the
            // tile-local snap screen point back to the full canvas.
            if let Some(s) = self.tabs[i].snap_result.as_mut() {
                s.screen.x += tile_b.x;
                s.screen.y += tile_b.y;
            }

            // Give the command the current UCS + Ctrl state before it
            // builds its rubber-band preview (and, by persistence, its
            // commit).
            self.push_ucs_to_cmd(i);
            self.push_ctrl_to_cmd(i);
            let mut previews = if needs_structure {
                let mut p = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.object_pick_hover_previews(&self.tabs[i].scene, effective))
                    .unwrap_or_default();
                if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                    p.extend(cmd.on_preview_wires(effective));
                }
                p
            } else if needs_entity {
                let include_fills = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_includes_fills())
                    .unwrap_or(false);
                let candidate_handles = if include_fills {
                    self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&snap_candidates)
                } else {
                    None
                };
                let hovered = scene::pick::hit_test::click_hit(
                    p,
                    &snap_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                )
                .and_then(|s| Scene::handle_from_wire_name(s))
                .or_else(|| {
                    if !include_fills {
                        return None;
                    }
                    scene::pick::hit_test::click_hit_hatch(
                        p,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    )
                })
                .or_else(|| {
                    include_fills.then(|| {
                        scene::pick::hit_test::click_hit_insert_hatch(
                            p,
                            self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_hover_hit(
                            p,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                });
                let highlights_hover = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_highlights_hover())
                    .unwrap_or(false);
                if highlights_hover {
                    self.tabs[i].scene.set_hover_highlight(hovered);
                }
                let hover_handle = hovered.unwrap_or(acadrust::Handle::NULL);
                let shift = self.shift_down;
                let mut p = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|c| {
                        // Live shift state drives TRIM/EXTEND's
                        // shift-swap preview (#336).
                        c.set_shift(shift);
                        c.on_hover_entity(hover_handle, effective_wcs)
                    })
                    .unwrap_or_default();
                // on_hover_entity returns WCS wires; shift to the
                // offset-relative frame so the preview lands on the
                // geometry on large-coordinate drawings.
                if wo_f != glam::DVec3::ZERO {
                    for w in p.iter_mut() {
                        for pt in w.points.iter_mut() {
                            pt[0] -= wo_f.x as f32;
                            pt[1] -= wo_f.y as f32;
                            pt[2] -= wo_f.z as f32;
                        }
                    }
                }
                if !hover_handle.is_null() {
                    if let Some(cmd) = self.tabs[i].active_cmd.as_ref() {
                        p.extend(
                            cmd.entity_pick_acquire_previews(&self.tabs[i].scene, hover_handle),
                        );
                    }
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        if let Some(hint) = cmd.entity_pick_acquire_hint(hover_handle) {
                            cmd.set_acquisition_hint(Some(hint));
                        }
                    }
                }
                p
            } else {
                self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|c| c.on_preview_wires(effective))
                    .unwrap_or_default()
            };
            // Polar tracking guide line: dotted line from last_point along
            // the snapped angle direction, extending across the drawing.
            if self.polar_mode && !needs_entity {
                if let Some(base) = self.last_point {
                    // Screen-space dotted guide (render geometry is f32).
                    let base = base.as_vec3();
                    let effective = effective.as_vec3();
                    let dx = effective.x - base.x;
                    let dy = effective.y - base.y;
                    // Only show the guide while POLAR is actually engaged
                    // — i.e. the point is snapped onto a polar ray, not
                    // floating free near no angle (issue #70).
                    let step = self.polar_increment_deg.to_radians();
                    let angle = dy.atan2(dx);
                    let snapped =
                        step > 1e-6 && ((angle / step).round() * step - angle).abs() < 1e-3;
                    if snapped && (dx * dx + dy * dy).sqrt() > 1e-4 {
                        let far = 1e5_f32;
                        let dir = glam::Vec3::new(dx, dy, 0.0).normalize();
                        let far_pos = base + dir * far;
                        let far_neg = base - dir * far;
                        let guide = crate::scene::WireModel {
                            taper_widths: Vec::new(),
                            world_width: 0.0,
                            depth_override: None,
                            fill_is_3d: false,
                            fill_is_2d_solid: false,
                            pick_tris: Vec::new(),
                            pick_tris_low: Vec::new(),
                            dash_from_start: false,
                            dash_align_end: None,
                            text_verts: Vec::new(),
                            name: "__polar_guide__".into(),
                            points: vec![
                                [far_neg.x, far_neg.y, far_neg.z],
                                [far_pos.x, far_pos.y, far_pos.z],
                            ],
                            points_low: Vec::new(),
                            color: [0.2, 0.7, 0.9, 0.6],
                            selected: false,
                            aci: 0,
                            pattern_length: 0.8,
                            pattern: [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                            line_weight_px: 1.0,
                            snap_pts: vec![],
                            tangent_geoms: vec![],
                            key_vertices: vec![],
                            aabb: crate::scene::WireModel::UNBOUNDED_AABB,
                            plinegen: true,
                            fill_tris: vec![],
                            fill_tris_low: Vec::new(),
                        };
                        previews.push(guide);
                    }
                }
            }
            // OTRACK alignment guide: dashed ray through the tracking
            // point along the aligned direction (issue #69).
            if let Some(h) = otrack_hit {
                if !needs_entity {
                    let far = 1e5_f64;
                    let far_pos = h.base + h.dir * far;
                    let far_neg = h.base - h.dir * far;
                    let mut guide = crate::scene::WireModel::solid_f64(
                        "__otrack_guide__".into(),
                        vec![
                            [far_neg.x, far_neg.y, far_neg.z],
                            [far_pos.x, far_pos.y, far_pos.z],
                        ],
                        [0.2, 0.9, 0.5, 0.6],
                        false,
                    );
                    guide.pattern_length = 0.8;
                    guide.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                    previews.push(guide);
                }
            }
            self.tabs[i].scene.set_preview_wires(previews);
        } else {
            self.tabs[i].snap_result = None;
        }

        self.sync_dyn_fields();
        let move_ms = move_started.elapsed().as_secs_f64() * 1000.0;
        if perf_move
            && (self.tabs[i].active_cmd.is_some()
                || self.tabs[i].scene.active_viewport.is_some())
            && move_ms >= 16.7
        {
            let mode = if self.tabs[i].active_cmd.is_some() {
                "command"
            } else {
                "MSPACE"
            };
            crate::perf_record!(
                "[perf] pointer-move mode={mode:<7} {:>7.1}ms",
                move_ms,
            );
        }
        Task::none()
    }

    pub(crate) fn on_viewport_exit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.left_down = false;
        sel.left_press_pos = None;
        sel.left_press_time = None;
        sel.left_dragging = false;
        sel.right_down = false;
        sel.right_press_pos = None;
        sel.right_press_time = None;
        sel.right_last_pos = None;
        sel.right_dragging = false;
        sel.right_click_entered = false;
        sel.middle_down = false;
        sel.middle_last_pos = None;
        sel.orbit_pivot = None;
        sel.box_anchor = None;
        sel.box_anchor_world = None;
        sel.box_current = None;
        sel.box_crossing = false;
        sel.poly_active = false;
        sel.poly_points.clear();
        sel.poly_crossing = false;
        drop(sel);
        // Clear the rollover highlight when the cursor leaves the
        // viewport so it doesn't stick while the mouse is over the
        // ribbon / panels.
        self.tabs[i].scene.set_hover_highlight(None);
        // Don't touch `context_menu` here. ViewportExit also fires
        // when an upper overlay (the right-click menu panel) takes
        // the cursor, so clearing the menu state on every exit
        // would close the menu the moment it opens. Outside-click
        // dismiss is handled in `ViewportLeftPress`.
        Task::none()
    }

    /// Screen positions of the UCS-icon grips (origin + axis tips, absolute px)
    /// for the active pane, or `None` when the icon is not shown / not anchored
    /// at its on-screen origin / a command is active. The single source of truth
    /// for both hover and grip hit-testing, so they match what is drawn.
    fn ucs_icon_hit_info(
        &self,
        i: usize,
        vw: f32,
        vh: f32,
    ) -> Option<crate::ui::overlay::UcsIconHit> {
        if !self.show_ucs_icon || self.tabs[i].active_cmd.is_some() {
            return None;
        }
        let tab = &self.tabs[i];
        let (_, ux, uy, uz) = tab.ucs_xform().axes();
        // Project through whichever pane owns the icon — a floating viewport's
        // own camera, else the active model tile. Bare paper space shows none.
        let (cam, bounds) = if let Some((c, full)) = tab.scene.viewport_edit_frame((vw, vh)) {
            (c, full)
        } else if tab.scene.current_layout == "Model" {
            (
                tab.scene.camera.borrow().clone(),
                tab.scene.active_model_tile_bounds(vw, vh),
            )
        } else {
            return None;
        };
        // Mirror the draw: anchor at the projected origin only when ORigin mode
        // is on AND the origin is on-screen; otherwise `None` parks it in the
        // corner — still selectable/draggable there.
        let os = if self.ucs_icon_at_origin {
            cam.project(tab.ucs_origin_world(), bounds)
                .map(|q| Point::new(bounds.x + q.x, bounds.y + q.y))
        } else {
            None
        };
        crate::ui::overlay::ucs_icon_hit(
            cam.view_proj_rte(bounds),
            bounds,
            (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()),
            os,
        )
    }

    /// Apply one frame of a UCS-icon grip drag: map the cursor onto the active
    /// UCS plane (so the move stays in-plane) and either slide the origin there
    /// or rotate the chosen axis to point at it, keeping a right-handed frame
    /// with Z fixed. Live — the commit (persist) happens on release.
    fn drag_ucs_grip(&mut self, i: usize, kind: crate::app::UcsGripKind, p_full: Point) {
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        if vw <= 1.0 || vh <= 1.0 {
            return;
        }
        // Same pane framing as the rest of the move handler: pane-local cursor
        // and origin-zero bounds, with the floating-viewport camera when inside
        // one. `cursor_model_point` handles the paper→model round-trip.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = Point::new(p_full.x - tile_b.x, p_full.y - tile_b.y);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        let raw = self.cursor_model_point(i, &edit_cam, p, bounds);

        // Object/grid snap, same path as an entity grip or command drag: the
        // dragged UCS point sticks to endpoints/midpoints/grid under the cursor,
        // and the snap marker is published via `snap_result`.
        let (view_rot, eye, grid_spacing) = match &edit_cam {
            Some(cam) => (
                cam.view_proj_rte(bounds),
                cam.eye(),
                crate::ui::overlay::compute_grid_step(cam.distance, cam.fov_y, bounds),
            ),
            None => {
                let cam = self.tabs[i].scene.camera.borrow();
                (
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    crate::ui::overlay::compute_grid_step(cam.distance, cam.fov_y, bounds),
                )
            }
        };
        let all_wires = if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport)
        {
            self.tabs[i]
                .scene
                .model_wires_for_viewport_arc(h, bounds.height)
        } else {
            self.tabs[i].scene.hit_test_wires()
        };
        let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
            all_wires,
            raw,
            view_rot,
            eye,
            bounds,
            self.snapper.osnap_radius_px,
        );
        self.snapper.grid_spacing = grid_spacing;
        // No rubber-band origin (perp/extension feet don't apply to a free drag).
        self.snapper.from_point = None;
        let (go, gr) = self.tabs[i].ucs_grid_basis();
        let snap_hit = self
            .snapper
            .snap(raw, p, &snap_candidates, view_rot, eye, bounds, go, gr);
        let world = snap_hit.map(|s| s.world).unwrap_or(raw);
        self.tabs[i].snap_result = snap_hit;
        if let Some(s) = self.tabs[i].snap_result.as_mut() {
            // Snap marker is pane-local; lift it to absolute canvas px.
            s.screen.x += tile_b.x;
            s.screen.y += tile_b.y;
        }

        use acadrust::types::Vector3;
        let v3 = |d: glam::DVec3| Vector3::new(d.x, d.y, d.z);
        {
            let ucs = self.tabs[i]
                .active_ucs
                .get_or_insert_with(|| acadrust::tables::Ucs::new("*ACTIVE*"));
            match kind {
                crate::app::UcsGripKind::Origin => {
                    ucs.origin = v3(world);
                }
                crate::app::UcsGripKind::XAxis | crate::app::UcsGripKind::YAxis => {
                    let o = glam::dvec3(ucs.origin.x, ucs.origin.y, ucs.origin.z);
                    let x = glam::dvec3(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z);
                    let y = glam::dvec3(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z);
                    let z = x.cross(y).normalize_or(glam::DVec3::Z);
                    // Direction from origin to cursor, flattened into the UCS
                    // plane; bail on a degenerate (cursor on the origin).
                    let dir = world - o;
                    let d = dir - z * dir.dot(z);
                    if d.length_squared() < 1e-12 {
                        return;
                    }
                    let d = d.normalize();
                    // Z fixed; the dragged axis = d, the other = right-handed
                    // completion (z×x = y, y×z = x).
                    let (nx, ny) = match kind {
                        crate::app::UcsGripKind::XAxis => (d, z.cross(d)),
                        _ => (d.cross(z), d),
                    };
                    ucs.x_axis = v3(nx);
                    ucs.y_axis = v3(ny);
                }
            }
        }
        self.tabs[i].sync_ucs_to_scene();
        self.tabs[i].scene.camera_generation += 1;
    }

    /// Commit the live UCS to its owning space. Model/viewport panes persist
    /// their normal UCS fields. A BEDIT pane has no persistent UCS of its own:
    /// its transient origin and axes are baked into the block definition, then
    /// the editor returns to canonical local coordinates.
    pub(in crate::app) fn commit_active_ucs_change(&mut self, i: usize, label: &'static str) {
        if let Some((session_index, block_record)) = self.tabs[i]
            .active_block_edit
            .and_then(|index| {
                self.tabs[i]
                    .block_edits
                    .get(index)
                    .map(|session| (index, session.br_handle))
            })
        {
            let frame = self.tabs[i].ucs_xform();
            if !frame.is_identity() {
                self.push_undo_snapshot(i, label);
                let local_from_old = frame.to_ucs_transform();
                let changed = self.tabs[i]
                    .scene
                    .reframe_block_definition(block_record, &local_from_old);
                if changed > 0 {
                    // Carry the editor camera through the same rigid transform
                    // so changing the block coordinate frame does not make its
                    // contents jump on screen.
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .apply_rigid_transform(&local_from_old);
                    self.tabs[i].scene.camera_generation += 1;
                    self.tabs[i].dirty = true;
                }
            }

            // Blocks do not own a saved UCS. The chosen frame has now become
            // their identity coordinates.
            self.tabs[i].active_ucs = None;
            let editor_camera = self.tabs[i].scene.camera.borrow().clone();
            if let Some(session) = self.tabs[i].block_edits.get_mut(session_index) {
                session.editor_ucs = None;
                session.editor_camera = editor_camera;
            }
            self.tabs[i].sync_ucs_to_scene();
            return;
        }

        let persisted = if let Some(handle) = self.tabs[i].scene.active_viewport {
            self.tabs[i].ucs_from_viewport(handle)
        } else if self.tabs[i].scene.current_layout == "Model" {
            self.tabs[i].model_ucs_from_header()
        } else {
            None
        };
        let same_basis = |a: Option<&acadrust::tables::Ucs>,
                          b: Option<&acadrust::tables::Ucs>| {
            match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.origin == b.origin
                        && a.x_axis == b.x_axis
                        && a.y_axis == b.y_axis
                        && a.handle == b.handle
                        && a.name.eq_ignore_ascii_case(&b.name)
                }
                _ => false,
            }
        };
        if !same_basis(self.tabs[i].active_ucs.as_ref(), persisted.as_ref()) {
            self.push_undo_snapshot(i, label);
            self.tabs[i].persist_active_ucs();
            self.tabs[i].dirty = true;
        }
        self.tabs[i].sync_ucs_to_scene();
    }

    // ── Per-pane Model viewport (pane_grid) ───────────────────────────────

    /// Focus Model pane `idx` (cursor entered it): swap in its camera, sync the
    /// render-mode / grid display. No-op if already active.
    pub(super) fn focus_model_pane(&mut self, idx: usize) {
        let i = self.active_tab;
        if self.tabs[i].scene.set_active_model_tile(idx) {
            self.tabs[i].scene.camera_generation += 1;
            self.sync_render_mode_to_active_tile(i);
            self.adopt_view_display(i);
        }
    }

    /// Convert a pane-local cursor point (from the pane's `mouse_area`) to the
    /// canvas-relative point the viewport handlers expect.
    pub(super) fn pane_canvas_point(&self, idx: usize, local: Point) -> Point {
        let i = self.active_tab;
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        let o = self.tabs[i].scene.pane_origin_px(idx, vw, vh);
        Point::new(o.x + local.x, o.y + local.y)
    }

    pub(super) fn on_pane_resized(
        &mut self,
        ev: iced::widget::pane_grid::ResizeEvent,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.model_panes.resize(ev.split, ev.ratio);
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        self.tabs[i].scene.sync_tiles_from_panes(vw, vh);
        self.tabs[i].scene.camera_generation += 1;
        Task::none()
    }

    pub(super) fn on_pane_clicked(&mut self, pane: iced::widget::pane_grid::Pane) -> Task<Message> {
        let i = self.active_tab;
        if let Some(&idx) = self.tabs[i].scene.model_panes.get(pane) {
            self.focus_model_pane(idx);
        }
        Task::none()
    }

    pub(super) fn on_pane_dragged(
        &mut self,
        ev: iced::widget::pane_grid::DragEvent,
    ) -> Task<Message> {
        if let iced::widget::pane_grid::DragEvent::Dropped { pane, target } = ev {
            let i = self.active_tab;
            self.tabs[i].scene.model_panes.drop(pane, target);
            let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
            self.tabs[i].scene.sync_tiles_from_panes(vw, vh);
            self.tabs[i].scene.camera_generation += 1;
        }
        Task::none()
    }

    pub(super) fn on_viewport_left_press(&mut self) -> Task<Message> {
        let i = self.active_tab;
        // A left-click during a command resets the right-click cycle, so
        // the next right-click acts as Enter again rather than opening
        // the context menu.
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .right_click_entered = false;
        // A click in the viewport dismisses any open ribbon dropdown
        // (e.g. the annotation style combo), which has no backdrop of
        // its own to catch outside clicks.
        self.ribbon.close_dropdown();
        // Likewise dismiss the Properties color dropdowns — a viewport
        // press starts a box selection, so without this they'd only
        // close on the second click (issue #104).
        self.tabs[i].properties.color_picker_open = false;
        self.tabs[i].properties.color_palette_open = false;
        // Click anywhere outside the popup dismisses it. The
        // menu's own buttons live above this mouse_area, so a
        // press that reaches here means the cursor is not on
        // any menu item.
        if self.grip_popup.take().is_some() {
            self.grip_hover = None;
            return Task::none();
        }
        // Outside-click dismiss for the visibility-state dropdown
        // (its buttons sit above this mouse_area).
        if self.visibility_popup.take().is_some() {
            return Task::none();
        }
        // Same dismiss-on-outside-click for the right-click
        // context menu: its panel is opaque, so a press that
        // reaches here is outside the menu.
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            if sel.context_menu.take().is_some() {
                return Task::none();
            }
        }
        let (p, vp_size) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let p = match sel.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            (p, sel.vp_size)
        };
        let (vw, vh) = vp_size;

        // Interactive navigation tools reuse the middle-button movement path,
        // so no selection/pick logic runs while the left button drives them.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode {
            self.clear_navigation_hover(i);
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.middle_down = true;
            sel.middle_last_pos = Some(p);
            return Task::none();
        }

        if vw > 1.0 && vh > 1.0 {
            let rot = self.tabs[i].scene.active_view_rotation_mat();
            // Map the cursor into whichever area owns the cube (active
            // viewport rect in paper, active tile in model) so the
            // consume-check lines up with the gizmo — same framing as
            // ViewportClick's snap hit-test.
            let (cx, cy, w, h) = match self.tabs[i]
                .scene
                .active_viewport
                .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
            {
                Some(rect) => (p.x - rect.x, p.y - rect.y, rect.width, rect.height),
                None => {
                    let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                    (p.x - tb.x, p.y - tb.y, tb.width, tb.height)
                }
            };
            if scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX).is_some() {
                return Task::none();
            }
        }

        // UCS icon: click to select (grips appear), then drag a grip to
        // move/rotate. A click off the icon clears the selection. Works
        // in the corner too (parked icon), not just at the origin.
        if self.show_ucs_icon && self.tabs[i].active_cmd.is_none() {
            if let Some(hit) = self.ucs_icon_hit_info(i, vw, vh) {
                // Already selected → a press on a grip starts the drag.
                if self.ucs_icon_selected {
                    if let Some(kind) = ucs_grip_under(p, &hit) {
                        self.ucs_grip_drag = Some(kind);
                        return Task::none();
                    }
                }
                // Press on the icon body selects it (and shows grips).
                if over_ucs_icon(p, &hit) {
                    self.ucs_icon_selected = true;
                    self.ucs_icon_hover = true;
                    return Task::none();
                }
            }
            // Press elsewhere drops the selection, then falls through to
            // the normal pick / box-select so the click still lands.
            self.ucs_icon_selected = false;
        }

        // Divider resize is handled natively by the input pane_grid
        // (`on_resize`); the active pane already follows the cursor via
        // `focus_model_pane`. So a press here goes straight to picking.

        // From here the click targets the active tile: map the
        // cursor into it and use the tile's size for picking, so
        // grip / selection hit-tests land in the right pane.
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // + camera (matches the GPU); otherwise the active model tile.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_b.x,
            y: p_full.y - tile_b.y,
        };
        let (vw, vh) = (tile_b.width, tile_b.height);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };

        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && !self.tabs[i].selected_grips.is_empty()
        {
            {
                let is_paper = self.tabs[i].scene.current_layout != "Model";
                // In-viewport grips are model-space; project them with the
                // viewport camera so they hit-test where the GPU draws
                // them. Paper-space entities use the 2-D paper transform;
                // the model tab uses the model camera.
                let grip_hit = if let Some(cam) = &edit_cam {
                    find_hit_grip_rte(
                        p,
                        &self.tabs[i].selected_grips,
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        bounds,
                    )
                } else if is_paper {
                    let cam = self.tabs[i].scene.camera.borrow();
                    let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                    let half_h = cam.ortho_size();
                    let half_w = half_h * aspect;
                    let tx = cam.target.x as f32;
                    let ty = cam.target.y as f32;
                    drop(cam);
                    find_hit_grip_paper(
                        p,
                        &self.tabs[i].selected_grips,
                        tx,
                        ty,
                        half_w,
                        half_h,
                        bounds,
                    )
                } else {
                    let cam = self.tabs[i].scene.camera.borrow();
                    find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
                };
                if let Some((grip_index, grip_id, is_translate, world)) = grip_hit {
                    let Some(&handle) = self.tabs[i].selected_grip_handles.get(grip_index) else {
                        return Task::none();
                    };
                    // The visibility (lookup) grip opens a state
                    // dropdown instead of starting a stretch drag.
                    if grip_id == crate::app::visibility::VIS_GRIP_ID {
                        self.open_visibility_popup(p);
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    if self.shift_down {
                        let key = (handle, grip_id);
                        if !self.tabs[i].hot_grips.remove(&key) {
                            self.tabs[i].hot_grips.insert(key);
                        }
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    self.tabs[i].active_grip = Some(self.grip_edit_for_hit(
                        i,
                        handle,
                        grip_id,
                        is_translate,
                        world,
                    ));
                    self.grip_hover = None;
                    self.grip_popup = None;
                    return Task::none();
                }
            }
        }

        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.left_down = true;
        // Stored in full-canvas space (like ViewportMove's cursor and
        // the overlay box / lasso drawing); release maps it into the
        // active tile. Tile-local here would double-offset the anchor.
        sel.left_press_pos = Some(p_full);
        sel.left_press_time = Some(Instant::now());
        sel.left_dragging = false;
        Task::none()
    }

    pub(super) fn on_viewport_left_release(&mut self) -> Task<Message> {
        let i = self.active_tab;

        // Navigation mode: end this drag but keep the tool armed for the next
        // left drag (exit is Esc / another command). Mirror of the press.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.middle_down = false;
            sel.middle_last_pos = None;
            sel.orbit_pivot = None;
            drop(sel);
            self.arm_hover_after_navigation(i);
            return Task::none();
        }

        // Commit a UCS icon grip drag: persist the new UCS so it
        // round-trips, and clear the lingering press state. In BEDIT this
        // rebases only the block; elsewhere it writes the model/viewport UCS.
        if self.ucs_grip_drag.take().is_some() {
            self.commit_active_ucs_change(i, "UCS");
            self.tabs[i].snap_result = None;
            self.snapper.from_point = None;
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.left_down = false;
            sel.left_press_pos = None;
            sel.left_dragging = false;
            return Task::none();
        }

        let (p, is_click, is_down) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let p = match sel.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            (p, !sel.left_dragging, sel.left_down)
        };

        // Grip editing: click-move-click (plus legacy press-drag).
        // The grip engages on press (active_grip set). This release
        // commits only if the grip has actually moved, or if it was a
        // press-drag. A bare engaging click (no movement yet) keeps the
        // grip hot so the user can move the cursor and click again to
        // place it. Escape cancels (handled elsewhere).
        if let Some(grip) = self.tabs[i].active_grip.clone() {
            // Reset mouse state so the lingering press from the engaging
            // click doesn't read as an in-progress drag on later moves.
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.left_down = false;
                sel.left_press_pos = None;
                sel.left_press_time = None;
                sel.left_dragging = false;
            }
            let moved = grip.last_world != grip.origin_world;
            if is_click && !moved {
                // Engaging click — stay hot, wait for the placement click.
                return Task::none();
            }
            if grip.mode == GripEditMode::Lengthen {
                self.grip_pending = None;
                self.command_line.input.clear();
            }
            let added_vertex_focus = grip.targets.iter().find_map(|target| {
                let original = self
                    .grip_originals
                    .iter()
                    .find(|(handle, _)| *handle == target.handle)
                    .map(|(_, entity)| entity)?;
                let current = self.tabs[i]
                    .scene
                    .document
                    .get_entity(target.handle)?;
                is_added_polyline_vertex(original, current, target.grip_id)
                    .then_some(target.grip_id)
            });
            self.tabs[i].active_grip = None;
            // Commit the grip drag as one undoable group, then put every
            // edited entity back into the resident tessellation.
            let handles = std::mem::take(&mut self.grip_preview_handles);
            let originals = std::mem::take(&mut self.grip_originals);
            let dirty_before = self.grip_dirty_before.take().unwrap_or(self.tabs[i].dirty);
            if !handles.is_empty() {
                if !originals.is_empty() {
                    self.push_entity_group_history(
                        i,
                        "GRIP",
                        originals
                            .into_iter()
                            .map(|(handle, entity)| (handle, std::sync::Arc::new(entity)))
                            .collect(),
                        dirty_before,
                    );
                    self.tabs[i].dirty = true;
                }
                self.grip_text_verts = Vec::new();
                self.grip_text_slide = false;
                for &handle in &handles {
                    self.tabs[i].scene.preview_hidden.remove(&handle);
                }
                self.tabs[i].scene.clear_preview_wire();
                let changes: Vec<_> = handles
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
            }
            // Placement confirmed — keep the just-added leader.
            self.grip_add_provisional = None;
            self.tabs[i].snap_result = None;
            if let Some(vertex_id) = added_vertex_focus {
                self.tabs[i].properties.prop_vertex = vertex_id;
                self.tabs[i].properties.prop_vertex_indicator_active = true;
                crate::scene::view::dispatch::set_prop_current_vertex(vertex_id);
            }
            self.refresh_properties();
            return Task::none();
        }

        // Map the release point into the active Model tile so the
        // click's pick / on_point / selection use the active pane's
        // camera + bounds. `p_full` keeps the canvas point for the
        // box/poly selection rectangle (drawn in canvas space).
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // and camera (see the MoveCursor path); pick / snap then run in
        // model space exactly where the GPU draws the content.
        let canvas_sz = self.tabs[i].scene.selection.borrow().vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(canvas_sz);
        let (tile_vw, tile_vh, tile_off) = match &edit_frame {
            Some((_, full)) => (full.width, full.height, iced::Point::new(full.x, full.y)),
            None => {
                let tb = self.tabs[i]
                    .scene
                    .active_model_tile_bounds(canvas_sz.0, canvas_sz.1);
                (tb.width, tb.height, iced::Point::new(tb.x, tb.y))
            }
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_off.x,
            y: p_full.y - tile_off.y,
        };

        let is_gathering = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.is_selection_gathering())
            .unwrap_or(false);
        let selection_box_active = self.tabs[i]
            .scene
            .selection
            .borrow()
            .box_anchor
            .is_some();
        let selection_pick_add = self.pick_add
            || self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.selection_forces_add());
        // A committed window corner must stay free of the Ortho/Polar
        // lock too, so the picked rectangle isn't flattened (#291).
        let is_window_corner = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.window_corner_pick())
            .unwrap_or(false);

        if is_down
            && is_click
            && self.tabs[i].active_cmd.is_some()
            && !is_gathering
            && !selection_box_active
        {
            let (vw, vh) = (tile_vw, tile_vh);
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };

            let snap_taken = self.tabs[i].snap_result.take();
            let tangent_obj_at_click = snap_taken.and_then(|s| s.tangent_obj);

            let world_pt = {
                // Cursor → model point (viewport camera inside a viewport,
                // else paper sheet → model). Model space throughout.
                let raw = self.cursor_model_point(i, &edit_cam, p, bounds);
                let (view_rot, eye) = match &edit_cam {
                    Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                    None => {
                        let c = self.tabs[i].scene.camera.borrow();
                        (c.view_proj_rte(bounds), c.eye())
                    }
                };
                let snap_cursor = raw;
                let all_wires =
                    if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                        self.tabs[i]
                            .scene
                            .model_wires_for_viewport_arc(h, bounds.height)
                    } else {
                        self.tabs[i].scene.hit_test_wires()
                    };
                let snap_candidates = self.tabs[i].scene.interaction_candidates_near(
                    all_wires,
                    snap_cursor,
                    view_rot,
                    eye,
                    bounds,
                    self.snapper.osnap_radius_px,
                );
                let needs_tan = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.needs_tangent_pick())
                    .unwrap_or(false);
                let needs_entity_click = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.needs_entity_pick())
                    .unwrap_or(false);
                let snap_hit = if needs_entity_click {
                    None
                } else if needs_tan {
                    self.snapper.snap_tangent_only(
                        snap_cursor.as_vec3(),
                        p,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    let (go, gr) = self.tabs[i].ucs_grid_basis();
                    self.snapper.from_point = self.last_point.map(|p| p.as_vec3());
                    self.snapper.snap(
                        snap_cursor,
                        p,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                        go,
                        gr,
                    )
                };
                // Snap runs in model space; the result is already model.
                let mut pt = snap_hit.map(|s| s.world).unwrap_or(raw);
                // When no UCS is active clamp to world XY; with a UCS the point is
                // already constrained to that plane by the ray–plane intersection.
                if self.tabs[i].active_ucs.is_none() {
                    pt.z = 0.0;
                }
                let mut world_pt_locked = false;
                // Hard axis lock (#312): the held Shift lock projects
                // the committed point — osnap hits included — onto the
                // locked ray, matching the preview exactly.
                if let (Some(dir), Some(base)) = (self.axis_lock_dir, self.last_point) {
                    pt = axis_lock_apply(pt, base, dir);
                    if self.tabs[i].active_ucs.is_none() {
                        pt.z = 0.0;
                    }
                    world_pt_locked = true;
                }
                // OTRACK alignment wins over ortho/polar; otherwise apply
                // ortho/polar relative to the last point.
                let otrack = if !world_pt_locked && snap_hit.is_none() {
                    let step = if self.polar_mode && !is_window_corner {
                        Some(self.polar_increment_deg)
                    } else {
                        None
                    };
                    let (_, ucs_x, ucs_y, _) = self.tabs[i].ucs_xform().axes();
                    self.snapper.otrack_snap(
                        raw,
                        view_rot,
                        eye,
                        bounds,
                        step,
                        self.last_point,
                        self.ortho_mode && !is_window_corner,
                        ucs_x,
                        ucs_y,
                    )
                } else {
                    None
                };
                if let Some(h) = otrack {
                    pt = h.aligned;
                    if self.tabs[i].active_ucs.is_none() {
                        pt.z = 0.0;
                    }
                } else if !world_pt_locked
                    && !is_window_corner
                    && !snap_hit.is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid)
                {
                    // Object snap wins over ortho/polar — a snapped point
                    // commits as-is. Grid snap still combines. (#132)
                    if let Some(base) = self.last_point {
                        let ucs_xf = self.tabs[i].ucs_xform();
                        if self.ortho_mode {
                            pt = ortho_constrain(pt, base, &ucs_xf);
                        } else if self.polar_mode {
                            pt = polar_constrain_near(
                                pt,
                                base,
                                self.polar_increment_deg,
                                view_rot,
                                eye,
                                bounds,
                                self.snapper.osnap_radius_px,
                                &ucs_xf,
                            );
                        }
                    }
                }
                // A click while dynamic-input fields hold typed values
                // commits the CONSTRAINED point — the same resolution
                // the preview shows and Enter would commit (#356).
                if self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some())
                {
                    self.tabs[i].last_cursor_world = pt;
                    if let Some(r) = self.dyn_resolve_point() {
                        pt = r;
                    }
                }
                pt
            };

            // `world_pt` is in offset-relative (local) space, matching
            // the camera and the point-creation commands. Entity-pick /
            // tangent / structure-pick commands instead compare the
            // click against WCS document entities, so they need the
            // world_offset added back (model space only — paper-space
            // entities are already in sheet coordinates). Without this,
            // TRIM/EXTEND/FILLET pick the wrong side on UTM-scale files.
            let pick_wcs = {
                let wo = if self.tabs[i].scene.current_layout == "Model" {
                    [0.0_f64; 3]
                } else {
                    [0.0; 3]
                };
                world_pt + glam::DVec3::new(wo[0], wo[1], wo[2])
            };

            let result = if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_structure_point_pick())
                .unwrap_or(false)
            {
                let pick = self.tabs[i].active_cmd.as_ref().and_then(|c| {
                    c.resolve_object_pick(&self.tabs[i].scene, pick_wcs.x as f64, pick_wcs.y as f64)
                });
                if let Some(pick) = pick {
                    let center = glam::DVec3::new(pick.x, pick.y, pick_wcs.z);
                    let result = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_structure_pick(pick.handle, center));
                    self.command_line
                        .push_info(crate::tf!("{} acquired.", pick.label).as_ref());
                    result
                } else {
                    let msg = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.object_pick_miss_message())
                        .unwrap_or("No object near click.");
                    self.command_line.push_error(msg);
                    None
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false)
            {
                let (view_rot2, eye2, all_wires2) = self.pick_view(i, &edit_cam, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires2,
                    world_pt,
                    view_rot2,
                    eye2,
                    bounds,
                    scene::pick::hit_test::CLICK_THRESHOLD_PX * 2.0,
                );
                let include_fills = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_includes_fills())
                    .unwrap_or(false);
                let candidate_handles = if include_fills {
                    self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&click_candidates)
                } else {
                    None
                };
                let hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot2,
                    eye2,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                )
                .and_then(|s| Scene::handle_from_wire_name(s))
                .or_else(|| {
                    if !include_fills {
                        return None;
                    }
                    scene::pick::hit_test::click_hit_hatch(
                        p,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot2,
                        eye2,
                        bounds,
                        candidate_handles.as_ref(),
                    )
                })
                .or_else(|| {
                    include_fills.then(|| {
                        scene::pick::hit_test::click_hit_insert_hatch(
                            p,
                            self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_click_hit(
                            p,
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                });
                if let Some(handle) = hit {
                    // Some commands (e.g. SS_CATCHMENT) need the entity
                    // body before `on_entity_pick` can advance.
                    let inject_first = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.inject_before_entity_pick())
                        .unwrap_or(false);
                    if inject_first {
                        let surface_area = self.tabs[i]
                            .scene
                            .meshes
                            .get(&handle)
                            .or_else(|| self.tabs[i].scene.block_meshes.get(&handle))
                            .map(|mesh| mesh.metrics.surface_area);
                        if let Some(entity) =
                            self.tabs[i].scene.document.get_entity(handle).cloned()
                        {
                            if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                cmd.inject_picked_entity(entity);
                                if let Some(area) = surface_area {
                                    cmd.inject_picked_surface_area(area);
                                }
                            }
                        }
                    }

                    let shift = self.shift_down;
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                        // Shift-swap state for TRIM/EXTEND (#336).
                        c.set_shift(shift);
                        c.on_entity_pick(handle, pick_wcs)
                    });
                    // HATCHEDIT: after pick, inject hatch model data into the command.
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.name() == "HATCHEDIT")
                        .unwrap_or(false)
                    {
                        if let Some(model) = self.tabs[i].scene.hatches.get(&handle).cloned() {
                            use crate::command::CadCommand;
                            use crate::modules::draw::draw::hatchedit::HatcheditCommand;
                            let cmd: Box<dyn CadCommand> = Box::new(HatcheditCommand::with_handle(
                                handle,
                                model.name.clone(),
                                model.scale,
                                model.angle_offset,
                            ));
                            self.command_line.push_info(&cmd.prompt());
                            self.tabs[i].active_cmd = Some(cmd);
                        } else {
                            self.command_line
                                .push_error(crate::t!("HATCHEDIT: not a hatch entity.").as_ref());
                            self.tabs[i].active_cmd = None;
                        }
                    }
                    // DIMTEDIT / MLEADERADD / MLEADERREMOVE: inject cloned entity via trait.
                    {
                        let needs_inject = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| {
                                matches!(c.name(), "DIMTEDIT" | "MLEADERADD" | "MLEADERREMOVE")
                            })
                            .unwrap_or(false);
                        if needs_inject {
                            if let Some(entity) =
                                self.tabs[i].scene.document.get_entity(handle).cloned()
                            {
                                if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                    cmd.inject_picked_entity(entity);
                                    let prompt = cmd.prompt();
                                    self.command_line.push_info(&prompt);
                                }
                            }
                        }
                    }
                    result
                } else if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.accepts_drag_selection())
                {
                    let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                    let mut selection = self.tabs[i].scene.selection.borrow_mut();
                    selection.box_anchor = Some(p_full);
                    selection.box_current = Some(p_full);
                    selection.box_anchor_world = Some(anchor_world);
                    selection.box_crossing = false;
                    None
                } else {
                    self.command_line
                        .push_info(crate::t!("Nothing found at that point.").as_ref());
                    None
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_tangent_pick())
                .unwrap_or(false)
            {
                if let Some(obj) = tangent_obj_at_click {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_tangent_point(obj, pick_wcs))
                } else {
                    self.command_line.push_info(crate::t!("Select a tangent object.").as_ref());
                    None
                }
            } else if !self.command_point_allowed(i, world_pt) {
                None
            } else {
                // A scalar typed into the dynamic-input box but not
                // yet confirmed with Enter is applied before the
                // point pick — e.g. an OFFSET distance typed and then
                // clicked takes effect rather than being discarded.
                let wants_text = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.wants_text_input())
                    .unwrap_or(false);
                if wants_text {
                    if let Some(text) = self.tabs[i]
                        .dyn_fields
                        .iter()
                        .find_map(|f| f.buffer.clone())
                    {
                        let text = crate::app::expr_eval::eval_to_string(text.trim());
                        if let Some(c) = self.tabs[i].active_cmd.as_mut() {
                            c.on_text_input(&text);
                        }
                        for f in self.tabs[i].dyn_fields.iter_mut() {
                            f.buffer = None;
                        }
                        self.tabs[i].dyn_active = 0;
                    }
                }
                self.last_point = Some(world_pt);
                // The one-shot snap override is spent by this pick —
                // restore the running osnap configuration (#337).
                self.snapper.clear_override();
                self.dyn_user_reshaped = false;
                self.dyn_coord_absolute = false;
                self.sync_dyn_fields();
                self.reset_tracking_after_point();
                // A running Tangent snap may carry a tangent object; a
                // command can consume it to resolve a deferred tangent
                // (LINE tangent to two circles, which needs both). When
                // it does, sync last_point to the command's resolved
                // anchor since it replaced the picked coordinate.
                let handled = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_point_with_tangent(world_pt, tangent_obj_at_click));
                if handled.is_some() {
                    if let Some(a) = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .and_then(|c| c.resolved_anchor())
                    {
                        self.last_point = Some(a);
                    }
                    handled
                } else {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_point(world_pt))
                }
            };

            if let Some(r) = result {
                let task = self.apply_cmd_result(r);
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.left_down = false;
                sel.left_press_pos = None;
                sel.left_press_time = None;
                sel.left_dragging = false;
                return task;
            }
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.left_down = false;
            sel.left_press_pos = None;
            sel.left_press_time = None;
            sel.left_dragging = false;
            return Task::none();
        }

        let (is_down2, is_dragging, box_anchor, box_crossing, _vp_size, poly_drag) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (
                sel.left_down,
                sel.left_dragging,
                sel.box_anchor,
                sel.box_crossing,
                sel.vp_size,
                sel.poly_active,
            )
        };

        let mut selection_just_completed = false;

        // Active-tile-local selection: tile-sized bounds and the box
        // anchor mapped into the tile, so box / crossing selection
        // matches the active pane (p is already tile-local).
        let vp_size = (tile_vw, tile_vh);
        let box_anchor = box_anchor.map(|a| iced::Point {
            x: a.x - tile_off.x,
            y: a.y - tile_off.y,
        });

        if is_down2 {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };

            if is_dragging {
                let drag_geometry = if poly_drag {
                    let mut points: Vec<iced::Point> = self.tabs[i]
                        .scene
                        .selection
                        .borrow()
                        .poly_points
                        .iter()
                        .map(|point| iced::Point {
                            x: point.x - tile_off.x,
                            y: point.y - tile_off.y,
                        })
                        .collect();
                    if points.last().is_none_or(|last| {
                        (last.x - p.x).abs() > f32::EPSILON
                            || (last.y - p.y).abs() > f32::EPSILON
                    }) {
                        points.push(p);
                    }
                    if let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    Some((fence, None))
                } else if let Some(anchor) = box_anchor {
                    let points = [
                        anchor,
                        iced::Point::new(anchor.x, p.y),
                        p,
                        iced::Point::new(p.x, anchor.y),
                        anchor,
                    ];
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (min, max) = fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    Some((fence, Some((min, max))))
                } else {
                    None
                };

                if let Some((fence, window)) = drag_geometry {
                    let result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(&fence, window)
                    });
                    if let Some(result) = result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.left_down = false;
                        selection.left_press_pos = None;
                        selection.left_press_time = None;
                        selection.left_dragging = false;
                        selection.poly_active = false;
                        selection.poly_points.clear();
                        selection.poly_crossing = false;
                        selection.box_anchor = None;
                        selection.box_anchor_world = None;
                        selection.box_current = None;
                        selection.box_crossing = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }
                }

                if !poly_drag {
                    // PICKDRAG 1 (#226): the press-drag spanned a
                    // rectangle through the box machinery — complete
                    // it here on release.
                    if let Some(a) = box_anchor {
                        let crossing = box_crossing;
                        let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                        let world_aabb =
                            [a, iced::Point::new(a.x, p.y), p, iced::Point::new(p.x, a.y)]
                                .map(|point| self.cursor_model_point(i, &edit_cam, point, bounds))
                                .into_iter()
                                .fold(
                                    [
                                        f64::INFINITY,
                                        f64::INFINITY,
                                        f64::NEG_INFINITY,
                                        f64::NEG_INFINITY,
                                    ],
                                    |mut aabb, world| {
                                        aabb[0] = aabb[0].min(world.x);
                                        aabb[1] = aabb[1].min(world.y);
                                        aabb[2] = aabb[2].max(world.x);
                                        aabb[3] = aabb[3].max(world.y);
                                        aabb
                                    },
                                );
                        let area_candidates = self.tabs[i].scene.interaction_candidates_in_aabb(
                            all_wires,
                            world_aabb,
                            [a.x.min(p.x), a.y.min(p.y), a.x.max(p.x), a.y.max(p.y)],
                            view_rot,
                            eye,
                            bounds,
                        );
                        let candidate_handles = self.tabs[i]
                            .scene
                            .interaction_candidate_handles(&area_candidates);
                        let mut handles: Vec<Handle> = scene::pick::hit_test::box_hit(
                            a,
                            p,
                            crossing,
                            &area_candidates,
                            view_rot,
                            eye,
                            bounds,
                        )
                        .into_iter()
                        .filter_map(|s| Scene::handle_from_wire_name(s))
                        .collect();
                        handles.extend(scene::pick::hit_test::box_hit_hatch(
                            a,
                            p,
                            crossing,
                            &self.tabs[i]
                                .scene
                                .visible_hatches_for_click(candidate_handles.as_ref()),
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        ));
                        handles.extend(scene::pick::hit_test::box_hit_insert_hatch(
                            a,
                            p,
                            crossing,
                            self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        ));
                        handles.extend(self.tabs[i].scene.mesh_box_hit(
                            a,
                            p,
                            crossing,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        ));
                        handles.extend(self.tabs[i].scene.block_mesh_box_hit(
                            a,
                            p,
                            crossing,
                            view_rot,
                            eye,
                            bounds,
                            candidate_handles.as_ref(),
                        ));
                        // Objects on a locked layer aren't selectable.
                        handles.retain(|&h| !self.tabs[i].scene.is_layer_locked(h));
                        // Box/lasso accumulates like individual picks
                        // (issue #83): a plain box adds to the current
                        // selection, Shift+box removes the boxed
                        // entities. Esc / empty-space click still clears.
                        // PICKADD 0 (#226): a plain box REPLACES.
                        if self.shift_down {
                            for h in &handles {
                                self.tabs[i].scene.deselect_entity(*h);
                            }
                        } else {
                            if !selection_pick_add && !handles.is_empty() {
                                self.tabs[i].scene.deselect_all();
                            }
                            for h in &handles {
                                self.tabs[i].scene.select_entity(*h, false);
                            }
                            self.tabs[i].scene.expand_selection_for_groups(&handles);
                        }
                        self.refresh_properties();
                        selection_just_completed = true;
                    }
                } else {
                    let (poly_pts, crossing) = {
                        let sel = self.tabs[i].scene.selection.borrow();
                        // Map lasso points into the active tile.
                        let pts: Vec<iced::Point> = sel
                            .poly_points
                            .iter()
                            .map(|pp| iced::Point {
                                x: pp.x - tile_off.x,
                                y: pp.y - tile_off.y,
                            })
                            .collect();
                        (pts, sel.poly_crossing)
                    };
                    self.tabs[i].scene.selection.borrow_mut().poly_last_crossing = crossing;
                    let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                    let world_aabb = poly_pts.iter().fold(
                        [
                            f64::INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::NEG_INFINITY,
                        ],
                        |mut aabb, &point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            aabb[0] = aabb[0].min(world.x);
                            aabb[1] = aabb[1].min(world.y);
                            aabb[2] = aabb[2].max(world.x);
                            aabb[3] = aabb[3].max(world.y);
                            aabb
                        },
                    );
                    let area_candidates = self.tabs[i].scene.interaction_candidates_in_aabb(
                        all_wires,
                        world_aabb,
                        poly_pts.iter().fold(
                            [
                                f32::INFINITY,
                                f32::INFINITY,
                                f32::NEG_INFINITY,
                                f32::NEG_INFINITY,
                            ],
                            |mut rect, point| {
                                rect[0] = rect[0].min(point.x);
                                rect[1] = rect[1].min(point.y);
                                rect[2] = rect[2].max(point.x);
                                rect[3] = rect[3].max(point.y);
                                rect
                            },
                        ),
                        view_rot,
                        eye,
                        bounds,
                    );
                    let candidate_handles = self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&area_candidates);
                    let mut handles: Vec<Handle> = scene::pick::hit_test::poly_hit(
                        &poly_pts,
                        crossing,
                        &area_candidates,
                        view_rot,
                        eye,
                        bounds,
                    )
                    .into_iter()
                    .filter_map(|s| Scene::handle_from_wire_name(s))
                    .collect();
                    handles.extend(scene::pick::hit_test::poly_hit_hatch(
                        &poly_pts,
                        crossing,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(scene::pick::hit_test::poly_hit_insert_hatch(
                        &poly_pts,
                        crossing,
                        self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(self.tabs[i].scene.mesh_poly_hit(
                        &poly_pts,
                        crossing,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(self.tabs[i].scene.block_mesh_poly_hit(
                        &poly_pts,
                        crossing,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    // Selection filter: keep only allowed types.
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    // Objects on a locked layer aren't selectable.
                    handles.retain(|&h| !self.tabs[i].scene.is_layer_locked(h));
                    // Accumulate like the box path (issue #83): plain
                    // lasso adds, Shift+lasso removes. An empty lasso
                    // leaves the current selection untouched so a stray
                    // drag never discards hard-won picks.
                    if self.shift_down {
                        for h in &handles {
                            self.tabs[i].scene.deselect_entity(*h);
                        }
                    } else {
                        // PICKADD 0 (#226): a plain marquee REPLACES
                        // the selection (empty results still leave it
                        // alone, matching the box rule).
                        if !selection_pick_add && !handles.is_empty() {
                            self.tabs[i].scene.deselect_all();
                        }
                        for h in &handles {
                            self.tabs[i].scene.select_entity(*h, false);
                        }
                        self.tabs[i].scene.expand_selection_for_groups(&handles);
                    }
                    self.refresh_properties();
                    selection_just_completed = true;
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.poly_active = false;
                sel.poly_points.clear();
                sel.poly_crossing = false;
                sel.box_anchor = None;
                sel.box_anchor_world = None;
                sel.box_current = None;
            } else {
                if box_anchor.is_none() {
                    let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                    let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                    let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                        all_wires,
                        click_world,
                        view_rot,
                        eye,
                        bounds,
                        scene::pick::hit_test::CLICK_THRESHOLD_PX * 2.0,
                    );
                    let candidate_handles = self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&click_candidates);

                    // Selection cycling: where two or more objects
                    // overlap, open a list box to pick which one; a
                    // single object falls through to the normal click.
                    // Gated behind the toggle, so default picking is
                    // unchanged when off.
                    let mut handled_by_cycling = false;
                    if self.selection_cycling {
                        let cands: Vec<Handle> = scene::pick::hit_test::click_hits_all(
                            p,
                            &click_candidates,
                            view_rot,
                            eye,
                            bounds,
                            self.tabs[i].scene.document.header.lineweight_display,
                        )
                        .into_iter()
                        .filter_map(|s| Scene::handle_from_wire_name(s))
                        .filter(|&h| self.tabs[i].scene.passes_selection_filter(h))
                        .filter(|&h| !self.tabs[i].scene.is_layer_locked(h))
                        .collect();
                        if cands.len() >= 2 {
                            // Overlap: open the list box at the cursor.
                            self.cycle_candidates = Some((p_full, cands));
                            handled_by_cycling = true;
                        }
                    }

                    if !handled_by_cycling {
                        let hit = scene::pick::hit_test::click_hit(
                            p,
                            &click_candidates,
                            view_rot,
                            eye,
                            bounds,
                            self.tabs[i].scene.document.header.lineweight_display,
                        )
                        .and_then(|s| Scene::handle_from_wire_name(s))
                        .or_else(|| {
                            scene::pick::hit_test::click_hit_hatch(
                                p,
                                &self.tabs[i]
                                    .scene
                                    .visible_hatches_for_click(candidate_handles.as_ref()),
                                view_rot,
                                eye,
                                bounds,
                                candidate_handles.as_ref(),
                            )
                        })
                        .or_else(|| {
                            // Block-internal hatch: resolve to the
                            // parent Insert (AutoCAD behaviour).
                            scene::pick::hit_test::click_hit_insert_hatch(
                                p,
                                self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                                view_rot,
                                eye,
                                bounds,
                                candidate_handles.as_ref(),
                            )
                        })
                        .or_else(|| {
                            // 3D solids: click anywhere on the shaded
                            // body — top-level solids and block-internal
                            // ones together, front-most wins (a block in
                            // front of a solid resolves to the block).
                            self.tabs[i].scene.solid_click_hit(
                                p,
                                view_rot,
                                eye,
                                bounds,
                                candidate_handles.as_ref(),
                            )
                        });
                        // Selection filter: drop a pick whose type is excluded.
                        let hit = hit.filter(|&h| self.tabs[i].scene.passes_selection_filter(h));
                        if let Some(handle) = hit {
                            if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
                                // Locked layer: visible + snappable but
                                // not selectable. Report and do nothing
                                // else — in particular do NOT set
                                // `selection_just_completed`, or a
                                // gather command (MOVE's "select
                                // objects") would wrongly finish.
                                self.command_line.push_info(crate::tf!(
                                            "Object is on locked layer \"{layer}\" — unlock the layer to select or edit it."
                                        ).as_ref());
                            } else {
                                // Individual picks accumulate (issue #47):
                                // each plain click adds to the selection,
                                // Shift+click removes the picked entity.
                                // PICKADD 0 (#226): a plain click
                                // REPLACES the selection instead and
                                // Shift+click toggles membership.
                                if self.shift_down {
                                    if !selection_pick_add
                                        && !self.tabs[i].scene.selected.contains(&handle)
                                    {
                                        self.tabs[i].scene.select_entity(handle, false);
                                        self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                                    } else {
                                        self.tabs[i].scene.deselect_entity(handle);
                                    }
                                } else {
                                    self.tabs[i]
                                        .scene
                                        .select_entity(handle, !selection_pick_add);
                                    self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                                }
                                self.refresh_properties();
                                selection_just_completed = true;
                            }
                        } else {
                            // Empty-space click only ARMS a box here; it
                            // no longer clears the selection, so a box can
                            // add to it (issue #83). The box completion
                            // (or Esc) decides what happens to the
                            // selection.
                            // PICKADD 0 (#226): OS convention — the
                            // empty click also drops the selection.
                            if !selection_pick_add && !self.shift_down {
                                self.tabs[i].scene.deselect_all();
                                self.refresh_properties();
                            }
                            // Pin the anchor to the world point under it
                            // so a zoom/pan mid-drag re-projects it
                            // instead of leaving it frozen in pixels
                            // (#234). Computed before the selection
                            // borrow so the &self projection can't clash.
                            let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                            let mut sel = self.tabs[i].scene.selection.borrow_mut();
                            // Full-canvas space: ViewportMove updates
                            // box_current in canvas coords and the overlay
                            // draws there; release maps back into the tile.
                            sel.box_anchor = Some(p_full);
                            sel.box_current = Some(p_full);
                            sel.box_anchor_world = Some(anchor_world);
                            sel.box_crossing = false;
                        }
                    }
                } else {
                    let a = box_anchor.unwrap();
                    let box_points = [
                        a,
                        iced::Point::new(a.x, p.y),
                        p,
                        iced::Point::new(p.x, a.y),
                        a,
                    ];
                    let command_fence = box_points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (command_min, command_max) = command_fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    let command_result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(
                            &command_fence,
                            Some((command_min, command_max)),
                        )
                    });
                    if let Some(result) = command_result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.left_down = false;
                        selection.left_press_pos = None;
                        selection.left_press_time = None;
                        selection.left_dragging = false;
                        selection.box_anchor = None;
                        selection.box_anchor_world = None;
                        selection.box_current = None;
                        selection.box_crossing = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }

                    let crossing = box_crossing;
                    let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                    let world_aabb = [a, iced::Point::new(a.x, p.y), p, iced::Point::new(p.x, a.y)]
                        .map(|point| self.cursor_model_point(i, &edit_cam, point, bounds))
                        .into_iter()
                        .fold(
                            [
                                f64::INFINITY,
                                f64::INFINITY,
                                f64::NEG_INFINITY,
                                f64::NEG_INFINITY,
                            ],
                            |mut aabb, world| {
                                aabb[0] = aabb[0].min(world.x);
                                aabb[1] = aabb[1].min(world.y);
                                aabb[2] = aabb[2].max(world.x);
                                aabb[3] = aabb[3].max(world.y);
                                aabb
                            },
                        );
                    let area_candidates = self.tabs[i].scene.interaction_candidates_in_aabb(
                        all_wires,
                        world_aabb,
                        [a.x.min(p.x), a.y.min(p.y), a.x.max(p.x), a.y.max(p.y)],
                        view_rot,
                        eye,
                        bounds,
                    );
                    let candidate_handles = self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&area_candidates);
                    let mut handles: Vec<Handle> = scene::pick::hit_test::box_hit(
                        a,
                        p,
                        crossing,
                        &area_candidates,
                        view_rot,
                        eye,
                        bounds,
                    )
                    .into_iter()
                    .filter_map(|s| Scene::handle_from_wire_name(s))
                    .collect();
                    handles.extend(scene::pick::hit_test::box_hit_hatch(
                        a,
                        p,
                        crossing,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(scene::pick::hit_test::box_hit_insert_hatch(
                        a,
                        p,
                        crossing,
                        self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(self.tabs[i].scene.mesh_box_hit(
                        a,
                        p,
                        crossing,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    handles.extend(self.tabs[i].scene.block_mesh_box_hit(
                        a,
                        p,
                        crossing,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    ));
                    // Selection filter: keep only allowed types.
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    // Objects on a locked layer aren't selectable.
                    handles.retain(|&h| !self.tabs[i].scene.is_layer_locked(h));
                    // Accumulate (issue #83): a plain box adds to the
                    // current selection, Shift+box removes the boxed
                    // entities. An empty box leaves the selection alone
                    // so an accidental empty drag never discards it.
                    // PICKADD 0 (#226): a plain box REPLACES instead.
                    if self.shift_down {
                        for h in &handles {
                            self.tabs[i].scene.deselect_entity(*h);
                        }
                    } else {
                        if !selection_pick_add && !handles.is_empty() {
                            self.tabs[i].scene.deselect_all();
                        }
                        for h in &handles {
                            self.tabs[i].scene.select_entity(*h, false);
                        }
                        self.tabs[i].scene.expand_selection_for_groups(&handles);
                    }
                    self.refresh_properties();
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.box_last = Some((a, p));
                    sel.box_last_crossing = crossing;
                    sel.box_anchor = None;
                    sel.box_anchor_world = None;
                    sel.box_current = None;
                    sel.box_crossing = false;
                    selection_just_completed = true;
                }
            }

            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.left_down = false;
            sel.left_press_pos = None;
            sel.left_press_time = None;
            sel.left_dragging = false;
        }

        if selection_just_completed {
            self.quick_properties_anchor = p_full;
        }

        if is_gathering && selection_just_completed {
            let handles: Vec<Handle> = self.tabs[i]
                .scene
                .selected_entities()
                .into_iter()
                .map(|(h, _)| h)
                .collect();
            return self.feed_command(crate::command::StepInput::SelectionComplete(handles));
        }

        // ── Double-click in Model Space: DDEDIT for Text/MText ────
        if is_click
            && is_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let now = Instant::now();
            let is_double_model = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double_model {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };
                let all_wires = self.tabs[i].scene.hit_test_wires();
                let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires,
                    click_world,
                    view_rot,
                    eye,
                    bounds,
                    scene::pick::hit_test::CLICK_THRESHOLD_PX * 2.0,
                );
                let candidate_handles = self.tabs[i]
                    .scene
                    .interaction_candidate_handles(&click_candidates);
                // Resolve the double-clicked object — its wire, or (for a
                // block/solid with no wire under the cursor) its shaded
                // body, which maps to the parent INSERT.
                let hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                )
                .and_then(|s| Scene::handle_from_wire_name(s))
                .or_else(|| {
                    self.tabs[i].scene.solid_click_hit(
                        p,
                        view_rot,
                        eye,
                        bounds,
                        candidate_handles.as_ref(),
                    )
                });
                if let Some(handle) = hit {
                    // Locked layer: double-click must not open any editor
                    // (text / attribute / in-place block edit).
                    if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
                        self.command_line.push_info(crate::tf!(
                            "Object is on locked layer \"{layer}\" — unlock the layer to edit it."
                        ).as_ref());
                        return Task::none();
                    }
                    // Any text-bearing entity opens its in-place editor
                    // (plain box or rich MText editor, per type). A
                    // Leader resolves to the entity it annotates.
                    let is_editable_text = self.tabs[i]
                        .scene
                        .document
                        .get_entity(handle)
                        .is_some_and(|e| {
                            crate::app::text_inline::read_text_field(e).is_some()
                                || matches!(e, AcadEntityType::Leader(_))
                        });
                    if is_editable_text {
                        return self.begin_text_edit(handle);
                    }
                    // Double-clicking a block with attributes opens the
                    // attribute editor (edit its values); a block with
                    // no attributes opens the block editor (BEDIT) — a
                    // space tab scoped to the block's own geometry, so
                    // edits reflect in every instance. (#136, #192, #261)
                    let insert_has_attrs = matches!(
                        self.tabs[i].scene.document.get_entity(handle),
                        Some(AcadEntityType::Insert(ins)) if !ins.attributes.is_empty()
                    );
                    if insert_has_attrs && self.tabs[i].active_block_edit.is_none() {
                        return Task::done(Message::AttrEditorOpen(handle));
                    }
                    let is_insert = matches!(
                        self.tabs[i].scene.document.get_entity(handle),
                        Some(AcadEntityType::Insert(_))
                    );
                    if is_insert && self.tabs[i].refedit_session.is_none() {
                        return Task::done(Message::Command(format!(
                            "BEDIT_BEGIN:{}",
                            handle.value()
                        )));
                    }
                }
            }
        }

        // ── Double-click: enter/exit MSPACE ───────────────────────
        // Only when no command is running, no drag, and we're in paper space.
        if is_click
                    && is_down   // ensures there was a matching left-press
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout != "Model"
        {
            let now = Instant::now();
            let is_double = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };

                // 1) Try direct wire hit — works when the border is clicked.
                let wire_hit: Option<acadrust::Handle> = {
                    let (view_rot, eye) = {
                        let c = self.tabs[i].scene.camera.borrow();
                        (c.view_proj_rte(bounds), c.eye())
                    };
                    let all_wires = self.tabs[i].scene.hit_test_wires();
                    let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                    let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                        all_wires,
                        click_world,
                        view_rot,
                        eye,
                        bounds,
                        scene::pick::hit_test::CLICK_THRESHOLD_PX * 2.0,
                    );
                    scene::pick::hit_test::click_hit(
                        p,
                        &click_candidates,
                        view_rot,
                        eye,
                        bounds,
                        self.tabs[i].scene.document.header.lineweight_display,
                    )
                    .and_then(|s| Scene::handle_from_wire_name(s))
                    .and_then(|h| {
                        if let Some(AcadEntityType::Viewport(vp)) =
                            self.tabs[i].scene.document.get_entity(h)
                        {
                            if Scene::is_content_viewport(vp) {
                                Some(h)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                };

                // 2) Decide which viewport (if any) the click enters,
                //    keyed on the *visible* on-screen rectangle. Screen-space
                //    (not the full paper rect) so a click on the empty area
                //    beside an off-screen viewport doesn't match its
                //    partly-off-canvas rect and switch to it by mistake.
                //    The wire hit only refines *which* visible viewport
                //    (border precision / overlap), and only when the click
                //    actually lands inside THAT viewport's visible rect —
                //    otherwise its border-wire pick tolerance could match a
                //    far viewport whose edge merely passes near the cursor
                //    (e.g. clicking the overlap of two viewports while a
                //    third's border runs nearby) and enter the wrong one.
                let screen_hit = self.tabs[i]
                    .scene
                    .viewport_at_screen_point(p.x, p.y, (vw, vh));
                let wire_in_visible = wire_hit.filter(|&h| {
                    self.tabs[i]
                        .scene
                        .viewport_screen_rect(h, (vw, vh))
                        .is_some_and(|r| {
                            let x0 = r.x.max(0.0);
                            let y0 = r.y.max(0.0);
                            let x1 = (r.x + r.width).min(vw);
                            let y1 = (r.y + r.height).min(vh);
                            p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
                        })
                });
                let hit_vp = wire_in_visible.or(screen_hit);

                if let Some(handle) = hit_vp {
                    return Task::done(Message::EnterViewport(handle));
                } else if self.tabs[i].scene.active_viewport.is_some() {
                    // Double-clicked outside all viewports while in MSPACE → exit.
                    return Task::done(Message::ExitViewport);
                }
            }
        }

        Task::none()
    }

    /// Remove cursor-derived highlights and cancel a queued rollover pick before
    /// moving the camera. A later idle cursor move re-arms hover normally.
    pub(in crate::app) fn clear_navigation_hover(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs[i].scene.set_hover_highlight(None);
        self.hover_dwell = None;
        self.grip_hover = None;
        self.ucs_icon_hover = false;
    }

    /// Queue a fresh rollover pick at the stationary cursor after camera motion
    /// ends. Repeated wheel events replace this timestamp, so the pick runs only
    /// after the final zoom step settles.
    pub(in crate::app) fn arm_hover_after_navigation(&mut self, i: usize) {
        if i >= self.tabs.len() || self.tabs[i].active_cmd.is_some() {
            return;
        }
        let (cursor, canvas_size) = {
            let selection = self.tabs[i].scene.selection.borrow();
            (selection.last_move_pos, selection.vp_size)
        };
        let Some(cursor) = cursor else {
            return;
        };
        let tile = self.tabs[i]
            .scene
            .viewport_edit_frame(canvas_size)
            .map(|(_, rect)| rect)
            .unwrap_or_else(|| {
                self.tabs[i]
                    .scene
                    .active_model_tile_bounds(canvas_size.0, canvas_size.1)
            });
        self.hover_dwell = Some(crate::app::HoverDwell {
            last_move_at: Instant::now(),
            point: Point::new(cursor.x - tile.x, cursor.y - tile.y),
            tile_size: (tile.width, tile.height),
            tab: i,
        });
    }

    pub(super) fn on_viewport_middle_press(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.clear_navigation_hover(i);
        self.ribbon.close_dropdown();
        let now = Instant::now();
        let is_double = {
            let sel = self.tabs[i].scene.selection.borrow();
            sel.middle_last_press_time
                .map(|t| now.duration_since(t).as_millis() < 300)
                .unwrap_or(false)
        };
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            let Some(p) = sel.last_move_pos else {
                return Task::none();
            };
            sel.middle_down = true;
            sel.middle_last_pos = Some(p);
            sel.middle_last_press_time = Some(now);
        }
        if is_double {
            self.tabs[i].scene.fit_all();
            self.tabs[i]
                .scene
                .record_nav_perf(crate::scene::NavPerfOp::Zoom, now);
            self.command_line.push_output(crate::t!("Zoom Extents").as_ref());
        }
        Task::none()
    }

    pub(super) fn on_viewport_scroll(&mut self, delta: mouse::ScrollDelta) -> Task<Message> {
        let nav_started = Instant::now();
        let s = match delta {
            mouse::ScrollDelta::Lines { y, .. } => y,
            mouse::ScrollDelta::Pixels { y, .. } => y * 0.01,
        };
        let i = self.active_tab;
        self.clear_navigation_hover(i);
        let cursor = self.tabs[i].scene.selection.borrow().last_move_pos;
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };
        if self.tabs[i].scene.active_viewport.is_some() {
            // In MSPACE: zoom the active viewport's model-space view,
            // keeping the model point under the cursor stationary.
            let cursor_paper = cursor.map(|cp| {
                let pt = self.tabs[i]
                    .scene
                    .camera
                    .borrow()
                    .pick_on_target_plane(cp, bounds);
                glam::Vec2::new(pt.x as f32, pt.y as f32)
            });
            self.tabs[i].scene.zoom_active_viewport(s, cursor_paper);
            // Bump so the GPU re-uploads the viewport's re-culled wire
            // set after zooming inside it.
            self.tabs[i].scene.camera_generation += 1;
        } else {
            // Model space: zoom about the cursor within the active
            // tile so the point under it stays put in that pane.
            let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
            let mut cam = self.tabs[i].scene.camera.borrow_mut();
            if let Some(cursor) = cursor {
                let local = iced::Point {
                    x: cursor.x - tile_b.x,
                    y: cursor.y - tile_b.y,
                };
                let tb = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: tile_b.width,
                    height: tile_b.height,
                };
                cam.zoom_about_point(local, tb, s);
            } else {
                cam.zoom(s);
            }
            drop(cam);
            self.tabs[i].scene.camera_generation += 1;
            // Keep an in-progress box selection pinned to the drawing
            // as the view zooms under it (#234).
            self.reproject_box_anchor(i, vw, vh);
        }
        self.tabs[i]
            .scene
            .record_nav_perf(crate::scene::NavPerfOp::Zoom, nav_started);
        self.arm_hover_after_navigation(i);
        Task::none()
    }

    /// Re-pin an active box-selection anchor to its stored world point after the
    /// model-space view moves (zoom/pan), so the selection rectangle tracks the
    /// drawing instead of staying frozen at its original pixel. No-op when no
    /// box is in progress. (#234)
    pub(super) fn reproject_box_anchor(&mut self, i: usize, vw: f32, vh: f32) {
        let world = self.tabs[i].scene.selection.borrow().box_anchor_world;
        let Some(world) = world else { return };
        let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let tile_local = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: tile_b.width,
            height: tile_b.height,
        };
        if let Some(sp) = self.tabs[i]
            .scene
            .camera
            .borrow()
            .project(world, tile_local)
        {
            self.tabs[i].scene.selection.borrow_mut().box_anchor =
                Some(iced::Point::new(sp.x + tile_b.x, sp.y + tile_b.y));
        }
    }

    pub(super) fn on_viewport_click(
        &mut self,
        expected_viewport: Option<acadrust::Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.active_viewport != expected_viewport
            || (expected_viewport.is_none()
                && self.tabs[i].scene.current_layout != "Model")
        {
            return Task::none();
        }
        let rot = self.tabs[i].scene.active_view_rotation_mat();
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        // The ViewCube draws in the top-right of whichever area
        // owns it: the full canvas in model space, or the active
        // viewport's screen rectangle in a paper layout. Map the
        // cursor into that area before hit-testing so paper-space
        // picks line up with the gizmo.
        let (cx, cy, w, h) = match self.tabs[i]
            .scene
            .active_viewport
            .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
        {
            Some(rect) => (
                self.cursor_pos.x - rect.x,
                self.cursor_pos.y - rect.y,
                rect.width,
                rect.height,
            ),
            None => {
                // Model layout: hit-test within the active tile.
                let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                (
                    self.cursor_pos.x - tb.x,
                    self.cursor_pos.y - tb.y,
                    tb.width,
                    tb.height,
                )
            }
        };
        // Prefer the currently-highlighted region: hover is recomputed
        // on every move straight from the cube's own overlay, so it is
        // immune to `cursor_pos` being overwritten by the viewport's
        // move handler between the last move and this press.
        if let Some(id) = self.tabs[i].scene.viewcube_hover.get() {
            let region = if id < 6 {
                scene::CubeRegion::Face(id)
            } else if id < 18 {
                scene::CubeRegion::Edge(id)
            } else {
                scene::CubeRegion::Corner(id)
            };
            return self.on_view_cube_snap(region);
        }
        if let Some(region) = scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX) {
            return self.on_view_cube_snap(region);
        }
        // Compass cardinals are world-fixed: hit-test through the camera-
        // only rotation (strip the UCS) so the target matches the drawn
        // N/E/S/W, and snap in world frame.
        let rot_world = rot * self.tabs[i].scene.viewcube_ucs_mat().inverse();
        if let Some(card) = scene::hit_test_cardinal(cx, cy, w, h, rot_world, VIEWCUBE_PX) {
            return self.on_view_cube_snap_world(card.face_region());
        }
        Task::none()
    }

    pub(super) fn on_view_cube_snap(&mut self, region: CubeRegion) -> Task<Message> {
        // The cube is oriented in the active UCS, so snap in the UCS frame.
        let r_ucs = self.tabs[self.active_tab].scene.viewcube_ucs_mat();
        self.snap_view_region(region, r_ucs)
    }

    /// Compass cardinals are world-fixed, so they snap in the world frame
    /// (no UCS composition).
    pub(super) fn on_view_cube_snap_world(&mut self, region: CubeRegion) -> Task<Message> {
        self.snap_view_region(region, glam::Mat4::IDENTITY)
    }

    fn snap_view_region(&mut self, region: CubeRegion, r_ucs: glam::Mat4) -> Task<Message> {
        let i = self.active_tab;
        self.clear_navigation_hover(i);
        let mut region = region;
        // "Already there → flip to opposite" check: compare the
        // current gaze direction with the region's target gaze.
        let target_dir = r_ucs.transform_vector3(region.snap_direction());
        let cur_dir = self.tabs[i].scene.active_gaze_dir();
        if cur_dir.dot(target_dir) > 0.9999 {
            region = region.opposite();
        }
        let eye_dir = r_ucs.transform_vector3(region.snap_direction());

        // Faces snap to a canonical upright orientation (never upside
        // down); edges/corners keep the current up-sense so they spin
        // smoothly around the clicked feature.
        let is_face = matches!(region, scene::CubeRegion::Face(_));
        if self.tabs[i].scene.active_viewport.is_some() {
            if is_face {
                self.tabs[i]
                    .scene
                    .mutate_active_viewport_camera(|c| c.snap_to_face(eye_dir, r_ucs));
            } else {
                self.tabs[i]
                    .scene
                    .snap_active_viewport_to_direction(eye_dir, r_ucs);
            }
        } else {
            let mut cam = self.tabs[i].scene.camera.borrow_mut();
            if is_face {
                cam.snap_to_face(eye_dir, r_ucs);
            } else {
                cam.snap_to_direction(eye_dir, r_ucs);
            }
        }
        self.tabs[i].scene.camera_generation += 1;
        self.command_line
            .push_output(crate::tf!("View: {}", region.label()).as_ref());
        Task::none()
    }

    pub(super) fn on_hover_dwell_tick(&mut self) -> Task<Message> {
        let Some(dwell) = self.hover_dwell.clone() else {
            return Task::none();
        };
        let dwell_ms = crate::app::HOVER_DWELL_MS;
        if Instant::now()
            .duration_since(dwell.last_move_at)
            .as_millis()
            < dwell_ms
        {
            return Task::none();
        }
        let perf = crate::perf::enabled();
        let hover_started = Instant::now();
        let i = dwell.tab;
        // Re-check the gate — drag / command may have started
        // between the move that armed the dwell and this tick.
        if i >= self.tabs.len() {
            self.hover_dwell = None;
            return Task::none();
        }
        let navigating = self.tabs[i].scene.selection.borrow().middle_down;
        if self.tabs[i].active_cmd.is_some() || navigating {
            self.clear_navigation_hover(i);
            return Task::none();
        }
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: dwell.tile_size.0,
            height: dwell.tile_size.1,
        };
        let p = dwell.point;
        // Inside a viewport, hover-pick through the viewport camera +
        // model wires so the rollover highlights the entity under the
        // cursor (dwell.point / dwell.tile_size are already pane-local).
        let canvas_sz = self.tabs[i].scene.selection.borrow().vp_size;
        let edit_cam = self.tabs[i]
            .scene
            .viewport_edit_frame(canvas_sz)
            .map(|(cam, _)| cam);
        let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
        if let Some(task) =
            self.prepare_interaction_index_task(i, Arc::clone(&all_wires), bounds.height)
        {
            // Keep the dwell armed. While the index is pending later timer
            // ticks return immediately; once installed, the next tick performs
            // the exact pick at the latest cursor position.
            return task;
        }
        let hover_world = self.cursor_model_point(i, &edit_cam, p, bounds);
        let candidate_started = Instant::now();
        let hover_candidates = self.tabs[i].scene.interaction_hover_candidates_near(
            all_wires,
            hover_world,
            view_rot,
            eye,
            bounds,
            scene::pick::hit_test::CLICK_THRESHOLD_PX * 2.0,
        );
        let candidate_ms = candidate_started.elapsed().as_secs_f64() * 1000.0;
        let candidate_count = hover_candidates.len();
        let handles_started = Instant::now();
        let candidate_handles = self.tabs[i]
            .scene
            .interaction_candidate_handles(&hover_candidates);
        let handles_ms = handles_started.elapsed().as_secs_f64() * 1000.0;
        // Mirror the click-selection pick order so the rollover
        // highlights every selectable object: wire → hatch →
        // block-internal hatch → shaded 3D solid body.
        let wire_started = Instant::now();
        let mut hovered = scene::pick::hit_test::click_hit(
            p,
            &hover_candidates,
            view_rot,
            eye,
            bounds,
            self.tabs[i].scene.document.header.lineweight_display,
        )
        .and_then(Scene::handle_from_wire_name);
        let wire_ms = wire_started.elapsed().as_secs_f64() * 1000.0;
        let mut hatch_ms = 0.0;
        let mut insert_ms = 0.0;
        let mut solid_ms = 0.0;
        if hovered.is_none() {
            let started = Instant::now();
            hovered = scene::pick::hit_test::click_hit_hatch(
                p,
                &self.tabs[i]
                    .scene
                    .visible_hatches_for_click(candidate_handles.as_ref()),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            );
            hatch_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if hovered.is_none() {
            let started = Instant::now();
            hovered = scene::pick::hit_test::click_hit_insert_hatch(
                p,
                self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            );
            insert_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if hovered.is_none() {
            let started = Instant::now();
            hovered = self.tabs[i]
                .scene
                .solid_hover_hit(p, view_rot, eye, bounds, candidate_handles.as_ref());
            solid_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        self.tabs[i].scene.set_hover_highlight(hovered);
        self.hover_dwell = None;
        let hover_ms = hover_started.elapsed().as_secs_f64() * 1000.0;
        if perf && hover_ms >= 5.0 {
            crate::perf_record!(
                "[perf] hover-detail       query={candidate_ms:.1} handles={handles_ms:.1} wire={wire_ms:.1} hatch={hatch_ms:.1} insert={insert_ms:.1} solid={solid_ms:.1} candidates={candidate_count}",
            );
            crate::perf_record!(
                "[perf] hover-dwell        {:>7.1}ms wires={} hit={}",
                hover_ms,
                self.tabs[i].scene.last_tess_wires.get(),
                hovered.is_some(),
            );
        }
        Task::none()
    }

    pub(in crate::app) fn prepare_interaction_index_task(
        &mut self,
        i: usize,
        wires: Arc<Vec<crate::scene::WireModel>>,
        screen_height_px: f32,
    ) -> Option<Task<Message>> {
        let (epoch, source) = self.tabs[i]
            .scene
            .interaction_index_build_key(&wires, screen_height_px)?;
        let tab_id = self.tabs[i].id;
        let key = (tab_id, epoch, source);
        if let Some(active) = self.active_interaction_index {
            if active != key {
                if let Some(position) = self
                    .queued_interaction_indices
                    .iter()
                    .position(|(queued_tab, ..)| *queued_tab == tab_id)
                {
                    self.queued_interaction_indices.remove(position);
                }
                self.queued_interaction_indices.push_back((
                    tab_id,
                    epoch,
                    source,
                    wires,
                    screen_height_px,
                ));
            }
            return Some(Task::none());
        }
        self.tabs[i]
            .scene
            .mark_interaction_index_pending(epoch, source);
        self.active_interaction_index = Some(key);
        let weak = Arc::downgrade(&wires);
        Some(Task::perform(
            async move {
                let started = iced::time::Instant::now();
                let index = Arc::new(
                    crate::scene::pick::interaction_index::InteractionIndex::build(&wires),
                );
                index.prepare_screen();
                Message::InteractionIndexReady {
                    tab_id,
                    epoch,
                    source,
                    wires: weak,
                    index,
                    build_ms: started.elapsed().as_secs_f64() * 1000.0,
                }
            },
            |message| message,
        ))
    }

    pub(crate) fn on_layout_switch(&mut self, name: String) -> Task<Message> {
        self.on_layout_switch_inner(name, false)
    }

    pub(crate) fn on_block_edit_switch(&mut self, name: String) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Task::none();
        }
        let Some(target_index) = self.tabs[i]
            .block_edits
            .iter()
            .position(|session| session.block_name == name)
        else {
            self.command_line
                .push_error(crate::tf!("BEDIT: block tab \"{name}\" is not open.").as_ref());
            return Task::none();
        };
        if self.tabs[i].active_block_edit == Some(target_index) {
            return Task::none();
        }

        let cancel_task = self.cancel_active_command_for_space_change();
        let current_camera = self.tabs[i].scene.camera.borrow().clone();
        let current_ucs = self.tabs[i].active_ucs.clone();
        if let Some(active_index) = self.tabs[i].active_block_edit {
            if let Some(session) = self.tabs[i].block_edits.get_mut(active_index) {
                session.editor_camera = current_camera;
                session.editor_ucs = current_ucs;
            }
        } else {
            self.tabs[i].scene.sync_camera_to_document();
        }

        let (br_handle, editor_camera) = {
            let session = &self.tabs[i].block_edits[target_index];
            (session.br_handle, session.editor_camera.clone())
        };
        self.layout_rename_state = None;
        self.tabs[i].scene.active_viewport = None;
        self.tabs[i].scene.set_current_layout("Model".to_string());
        self.tabs[i].scene.block_edit_block = Some(br_handle);
        self.tabs[i].active_block_edit = Some(target_index);
        *self.tabs[i].scene.camera.borrow_mut() = editor_camera;
        self.tabs[i].scene.camera_generation += 1;
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].scene.bump_geometry_no_blocks();
        self.refresh_properties();
        self.adopt_view_display(i);
        self.sync_dyn_fields();
        cancel_task
    }

    /// MVIEW's "Insert view > New" flow deliberately visits Model space to
    /// define a view and then returns to its paper layout. This is the only
    /// layout transition allowed to preserve an active command.
    pub(crate) fn on_layout_switch_preserving_command(
        &mut self,
        name: String,
    ) -> Task<Message> {
        self.on_layout_switch_inner(name, true)
    }

    fn on_layout_switch_inner(
        &mut self,
        name: String,
        preserve_active_command: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to switch layouts.").as_ref());
            return Task::none();
        }
        let perf = crate::perf::enabled();
        let perf_total = Instant::now();
        let leaving_block_edit = self.tabs[i].active_block_edit.is_some();
        let perf_from = self.tabs[i]
            .active_block_edit_session()
            .map(|session| session.block_name.clone())
            .unwrap_or_else(|| self.tabs[i].scene.current_layout.clone());
        let context_changed = leaving_block_edit
            || self.tabs[i].scene.current_layout != name
            || self.tabs[i].scene.active_viewport.is_some();
        let preserve_active_command = preserve_active_command
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.name() == "MVIEW");
        if context_changed {
            if preserve_active_command {
                // MVIEW keeps its command-owned step data, but all host-owned
                // cursor/snap/dynamic-input state belongs to the old space.
                self.reset_space_interaction_state();
            }
        }
        let cancel_task = if context_changed && !preserve_active_command {
            self.cancel_active_command_for_space_change()
        } else {
            Task::none()
        };
        if let Some(active_index) = self.tabs[i].active_block_edit.take() {
            let camera = self.tabs[i].scene.camera.borrow().clone();
            let editor_ucs = self.tabs[i].active_ucs.clone();
            let (return_layout, return_camera) = {
                let session = &mut self.tabs[i].block_edits[active_index];
                session.editor_camera = camera;
                session.editor_ucs = editor_ucs;
                (session.return_layout.clone(), session.return_camera.clone())
            };
            let return_layout = if self.tabs[i].scene.layout_names().contains(&return_layout) {
                return_layout
            } else {
                "Model".to_string()
            };
            self.tabs[i].scene.block_edit_block = None;
            self.tabs[i].scene.set_current_layout(return_layout);
            *self.tabs[i].scene.camera.borrow_mut() = return_camera;
            self.tabs[i].scene.camera_generation += 1;
            self.tabs[i].scene.bump_geometry_no_blocks();
        }
        let going_to_paper = name != "Model";
        // Persist the camera of the layout we're leaving BEFORE switching
        // so returning to it restores where the user left off (the
        // periodic sync only fires on a tick, which may not have run
        // since the last pan/zoom).
        let perf_phase = Instant::now();
        self.tabs[i].scene.sync_camera_to_document();
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        let sync_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Cancel any pending rename and active viewport when switching.
        self.layout_rename_state = None;
        self.tabs[i].scene.active_viewport = None;
        let perf_phase = Instant::now();
        self.tabs[i].scene.set_current_layout(name.clone());
        self.tabs[i].scene.deselect_all();
        let switch_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // UCS follows the pane: model header UCS in the Model tab, none
        // in plain paper space (a viewport's UCS is adopted on entry).
        let perf_phase = Instant::now();
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].scene.restore_saved_camera();
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        // `deselect_all` invalidates the scene highlight, but grips and the
        // Properties panel are separate caches owned by the app.
        self.refresh_properties();
        // Grid/snap are per-view: load the layout we just entered (its
        // sheet viewport in paper space, the model tile in model space)
        // so model and each layout keep independent grid state.
        self.adopt_view_display(i);
        let restore_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Paper-space tools live in the right-edge side toolbar now, so
        // entering/leaving a layout no longer hijacks the ribbon tab.
        let _ = going_to_paper;
        // Refresh VP freeze columns for the new layout.
        let perf_phase = Instant::now();
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        let layers_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        if perf {
            crate::perf_record!(
                "[perf] layout-switch from={} to={} total={:.2}ms sync={:.2}ms switch={:.2}ms restore={:.2}ms layers={:.2}ms epoch={}",
                perf_from,
                name,
                perf_total.elapsed().as_secs_f64() * 1000.0,
                sync_ms,
                switch_ms,
                restore_ms,
                layers_ms,
                self.tabs[i].scene.geometry_epoch,
            );
        }
        if context_changed {
            self.sync_dyn_fields();
        }
        cancel_task
    }

    pub(super) fn on_layout_create(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to add a layout.").as_ref());
            return Task::none();
        }
        let cancel_task = self.cancel_active_command_for_space_change();
        // Find a unique name (e.g. Layout2, Layout3, ...).
        let existing = self.tabs[i].scene.layout_names();
        let mut idx = existing.len();
        let new_name = loop {
            let candidate = format!("Layout{}", idx);
            if !existing.contains(&candidate) {
                break candidate;
            }
            idx += 1;
        };
        self.push_undo_snapshot(i, "LAYOUT");
        match self.tabs[i].scene.document.add_layout(&new_name) {
            Ok(_) => {
                // Override the acadrust default limits (12×9 imperial) with A4 landscape.
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let acadrust::objects::ObjectType::Layout(l) = obj {
                        if l.name == new_name {
                            l.min_limits = (0.0, 0.0);
                            l.max_limits = (297.0, 210.0);
                            l.min_extents = (0.0, 0.0, 0.0);
                            l.max_extents = (297.0, 210.0, 0.0);
                            l.paper_width = 297.0;
                            l.paper_height = 210.0;
                            l.plot_paper_units = 1;
                            l.plot_scale_numerator = 1.0;
                            l.plot_scale_denominator = 1.0;
                            l.paper_size = "ISO_A4_(297.00_x_210.00_MM)".into();
                            break;
                        }
                    }
                }
                // Safety net — `add_layout` already creates the overall
                // sheet viewport; this covers any path that doesn't.
                self.tabs[i].scene.ensure_sheet_viewport(&new_name);
                let switch_task = self.on_layout_switch(new_name.clone());
                self.tabs[i].scene.fit_all();
                self.command_line.push_output(crate::tf!(
                    "Layout \"{new_name}\" created — use MVIEW to add a viewport"
                ).as_ref());
                self.tabs[i].dirty = true;
                return Task::batch([cancel_task, switch_task]);
            }
            Err(e) => self
                .command_line
                .push_error(crate::tf!("Failed to create layout: {e}").as_ref()),
        }
        cancel_task
    }

    pub(super) fn on_layout_rename_commit(&mut self) -> Task<Message> {
        if let Some((orig, new_name)) = self.layout_rename_state.take() {
            let new_name = new_name.trim().to_string();
            if !new_name.is_empty() && new_name != orig {
                let i = self.active_tab;
                // BEDIT block tab: renaming it renames the BLOCK itself —
                // its record, marker and every INSERT reference. (#261)
                let is_block_tab = self.tabs[i]
                    .active_block_edit_session()
                    .is_some_and(|session| session.block_name == orig);
                if is_block_tab {
                    if self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&new_name)
                        .is_some()
                    {
                        self.command_line
                            .push_error(crate::tf!("\"{}\" name already in use", new_name).as_ref());
                    } else {
                        self.push_undo_snapshot(i, "BLOCK RENAME");
                        if self.tabs[i].scene.rename_block(&orig, &new_name) {
                            if let Some(session) =
                                self.tabs[i].active_block_edit_session_mut()
                            {
                                session.block_name = new_name.clone();
                            }
                            for session in &mut self.tabs[i].block_edits {
                                if session.return_block.as_deref() == Some(orig.as_str()) {
                                    session.return_block = Some(new_name.clone());
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("Block \"{orig}\" → \"{new_name}\"").as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("Could not rename block \"{orig}\"").as_ref());
                        }
                    }
                    return Task::none();
                }
                let exists = self.tabs[i]
                    .scene
                    .layout_names()
                    .iter()
                    .any(|n| *n == new_name);
                if exists {
                    self.command_line
                        .push_error(crate::tf!("\"{}\" name already in use", new_name).as_ref());
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&orig, &new_name);
                    if self.tabs[i].scene.current_layout == orig {
                        self.tabs[i].scene.set_current_layout(new_name.clone());
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("Layout \"{orig}\" → \"{new_name}\"").as_ref());
                }
            }
        }
        Task::none()
    }
}
