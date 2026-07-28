use super::document::DocumentTab;
use super::document::DynComponent;
use super::history::history_dropdown_labels;
use super::{Message, OpenCADStudio};
use crate::scene::pick::grip::{grips_to_screen, grips_to_screen_paper, grips_to_screen_rte};
use crate::scene::view::viewport_pane::ViewportPane;
use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
use crate::ui::wrap_bar::DensitySwap;
use crate::ui::wrap_bar::WrapFlow;
use iced::widget::{
    button, canvas, column, container, mouse_area, pane_grid, responsive, row, scrollable, shader,
    stack, text, Row, Space,
};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Subscription, Task, Theme};
use iced_aw::ContextMenu;

mod controls;
mod modal;
mod overlay;
mod viewcube;

use controls::{dyn_component_value, viewport_controls};
use overlay::{
    mtext_editor_overlay, position_canvas_overlay, qselect_overlay, text_inline_overlay,
    viewport_context_menu_overlay,
};
use viewcube::{viewcube_nav_controls, viewcube_ucs_picker, UCS_PICKER_W};

// Re-export the text-input element ids so sibling modules can address them at
// the `view::` path as before the split.
pub(in crate::app) use overlay::{MTEXT_TEXT_ID, TEXT_INLINE_ID};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_REGION_PX;

/// Clear gap (px) kept between the render-mode bar (top-left) and the ViewCube
/// (top-right) before the cube is judged to collide and hides.
const VIEWCUBE_GAP: f32 = 12.0;

/// True when a viewport `tile_w` px wide still has room for the ViewCube beside
/// a render-mode bar of measured width `bar_w`. When it doesn't, the cube hides
/// first (the bar keeps priority); the bar itself hides separately, only when it
/// no longer fits at all (its `DensitySwap`).
fn viewcube_has_room(bar_w: f32, tile_w: f32) -> bool {
    bar_w + VIEWCUBE_GAP + VIEWCUBE_REGION_PX + VIEWCUBE_PAD <= tile_w
}

/// `ViewportRenderMode` enum carries the raw DXF integers, not a label,
/// so wrap it locally with a friendly name renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderModeChoice(pub acadrust::entities::ViewportRenderMode);

impl std::fmt::Display for RenderModeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use acadrust::entities::ViewportRenderMode as M;
        f.write_str(match self.0 {
            M::Wireframe2D => "Wireframe 2D",
            M::Wireframe3D => "Wireframe 3D",
            M::HiddenLine => "Hidden Line",
            M::FlatShaded => "Flat Shaded",
            M::GouraudShaded => "Gouraud Shaded",
            M::FlatShadedWithEdges => "Flat Shaded + Edges",
            M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
        })
    }
}

impl OpenCADStudio {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        // ── Floating panel windows ─────────────────────────────────────────
        // All dialogs are in-canvas modals now (Plan B); view_main stacks the
        // active one. `window_id` is unused — there is only the main window.
        let _ = window_id;
        self.view_main()
    }

    /// The primary window: viewport, ribbon, tab bar, status bar. Split out of
    /// `view` so the single-window web build can render it directly, bypassing
    /// the multi-window id dispatch above (the web build has no extra windows).
    pub fn view_main(&self) -> Element<'_, Message> {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let theme_text = self.active_theme.extended_palette().background.base.text;
        let viewcube_text_color = [
            theme_text.r,
            theme_text.g,
            theme_text.b,
            theme_text.a,
        ];
        let is_paper = tab.scene.current_layout != "Model";
        // Adaptive corner widgets: the ViewCube shows only while the active
        // viewport is wide enough to hold it *beside* the render-mode bar, whose
        // real width is measured each frame by its `DensitySwap` and read back
        // here (from the previous frame). This drives the GPU cube (via the pane
        // flag), the ViewCube nav/UCS widgets, and the hover hit-test alike, so
        // they never overlap. The bar itself hides independently (its own
        // DensitySwap) only when it no longer fits at all.
        let render_bar_w =
            f32::from_bits(self.render_bar_w.load(std::sync::atomic::Ordering::Relaxed));
        let active_vp_w = if is_paper {
            tab.scene
                .active_viewport
                .and_then(|h| {
                    let (cw, ch) = tab.scene.selection.borrow().vp_size;
                    tab.scene.viewport_screen_rect(h, (cw, ch))
                })
                .map(|r| r.width)
                .unwrap_or(f32::INFINITY)
        } else {
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            tab.scene.active_model_tile_bounds(vw, vh).width
        };
        let viewcube_visible =
            self.show_viewcube && !tab.is_start && viewcube_has_room(render_bar_w, active_vp_w);
        // Start tab: render welcome page in place of the viewport.
        // Surrounding chrome (tab bar, status bar) stays; the welcome widget
        // returned here also flags the rest of `view` to skip drawing-only
        // overlays via `tab.is_start`.
        // Unified GPU widget for both layouts. A paper layout renders through
        // the same shader as model space: a full-canvas top-locked "sheet"
        // viewport draws the layout's own geometry (white sheet + entities +
        // borders) and the floating content viewports blit on top.
        let viewport_3d: Element<'_, Message> = if tab.is_start {
            responsive(|size| {
                start_page_view(
                    &self.patrons,
                    &self.videos,
                    self.videos_loading,
                    &self.video_thumbs,
                    &self.discussions,
                    self.discussions_loading,
                    &self.recent_files,
                    &self.recent_thumbs,
                    self.recent_limit,
                    &self.recent_limit_input,
                    size.width,
                    self.start_action_w.clone(),
                    self.start_section,
                )
            })
            .into()
        } else if is_paper {
            shader(ViewportPane::model(
                &tab.scene,
                viewcube_visible,
                tab.render_mode,
                viewcube_text_color,
            ))
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            // Model space: a pane_grid of per-pane shader widgets (rendering
            // only). The input mouse_areas live in a SECOND, identical pane_grid
            // layered ABOVE the crosshair overlay (`model_input_layer` below) —
            // they must sit above the selection overlay, whose `Hidden` cursor
            // interaction otherwise "levitates" the cursor and starves any layer
            // beneath it of mouse events. A separate eventless `responsive`
            // Space captures the area size to keep `vp_size` / tile rects
            // current (building the pane_grid inside `responsive` resets the
            // mouse_areas' hover state and drops their move events).
            let scene = &tab.scene;
            let show_viewcube = viewcube_visible;
            let render_mode = tab.render_mode;
            let size_probe: Element<'_, Message> = responsive(move |size| {
                {
                    let mut sel = scene.selection.borrow_mut();
                    sel.vp_size = (size.width, size.height);
                }
                scene.sync_tiles_from_panes(size.width, size.height);
                Space::new().width(Fill).height(Fill).into()
            })
            .into();
            let shaders = pane_grid::PaneGrid::new(
                &scene.model_panes,
                move |_pane, &idx, _maximized| {
                    pane_grid::Content::new(
                        shader(ViewportPane::for_pane(
                            scene,
                            show_viewcube,
                            render_mode,
                            idx,
                            viewcube_text_color,
                        ))
                        .width(Fill)
                        .height(Fill),
                    )
                },
            )
            .width(Fill)
            .height(Fill)
            .spacing(crate::scene::TILE_DIVIDER_PX);
            stack![size_probe, shaders].width(Fill).height(Fill).into()
        };

        // Per-pane input layer: a pane_grid of transparent mouse_areas matching
        // the shader pane_grid (same `model_panes` → identical layout). Layered
        // above the crosshair overlay so it actually receives mouse events, and
        // it owns the divider resize. Only built for the Model layout.
        let model_input_layer: Option<Element<'_, Message>> = if is_paper || tab.is_start {
            None
        } else {
            let scene = &tab.scene;
            Some(
                pane_grid::PaneGrid::new(&scene.model_panes, |_pane, &idx, _maximized| {
                    pane_grid::Content::new(pane_mouse_area(idx))
                })
                .width(Fill)
                .height(Fill)
                .spacing(crate::scene::TILE_DIVIDER_PX)
                .on_resize(6.0, Message::PaneResized)
                .into(),
            )
        };

        let grid_overlay = {
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let model_basis = {
                let (o, ux, uy, uz) = tab.ucs_xform().axes();
                (o, (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()))
            };
            let grid: Vec<crate::ui::overlay::GridParams> = tab
                .scene
                .grid_views(vw, vh)
                .into_iter()
                .map(|(bounds, cam, handle)| {
                    let (origin, axes): (glam::DVec3, _) = if is_paper {
                        match tab.ucs_from_viewport(handle) {
                            Some(u) => {
                                let (o, ux, uy, uz) =
                                    super::helpers::UcsXform::from_ucs(&u).axes();
                                (o, (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()))
                            }
                            None => (
                                glam::DVec3::ZERO,
                                (glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z),
                            ),
                        }
                    } else {
                        model_basis
                    };
                    crate::ui::overlay::GridParams {
                        view_rot: cam.view_proj_rte(bounds),
                        eye: cam.eye(),
                        bounds,
                        step: crate::ui::overlay::compute_grid_step(
                            cam.distance,
                            cam.fov_y,
                            bounds,
                        ),
                        origin,
                        axes,
                        limits: tab.scene.grid_limits_for_viewport(handle),
                    }
                })
                .collect();
            crate::ui::overlay::grid_overlay(grid)
        };

        let selection_overlay = {
            let sel = tab.scene.selection.borrow().clone();
            let snap_info = tab.snap_result.map(|s| (s.screen, s.snap_type));
            let snap_ext_base = tab.snap_result.and_then(|s| s.extension_base);
            let snap_ext_base2 = tab.snap_result.and_then(|s| s.extension_base2);

            let grips: Vec<crate::ui::overlay::GripMarker> =
                if tab.active_cmd.is_none() && !tab.selected_grips.is_empty() {
                    let (vw, vh) = tab.scene.selection.borrow().vp_size;
                    // Overlays project through the active tile's camera, so
                    // they must use the active tile's screen rectangle (with
                    // its canvas offset) — not the whole canvas — or they
                    // land in the wrong place in a tiled layout.
                    // Inside a floating viewport the pane is the viewport's own
                    // rect + camera; otherwise the active model tile.
                    let edit_frame = tab.scene.viewport_edit_frame((vw, vh));
                    let bounds = match &edit_frame {
                        Some((_, full)) => *full,
                        None => tab.scene.active_model_tile_bounds(vw, vh),
                    };
                    let sel_h = tab.selected_handle;
                    // The Current Vertex the Properties panel is focused on:
                    // mark that grip hot so the navigated vertex is visible in
                    // the drawing. Only for a single selected polyline, whose
                    // vertex grips are ids 0..n. (Properties vertex stepper)
                    let current_vertex_grip: Option<usize> = sel_h.and_then(|h| {
                        matches!(
                            tab.scene.document.get_entity(h),
                            Some(acadrust::EntityType::LwPolyline(_))
                                | Some(acadrust::EntityType::Polyline2D(_))
                        )
                        .then_some(tab.properties.prop_vertex)
                    });
                    // In-viewport grips are model-space; project them with the
                    // viewport camera so they sit on the wire the GPU draws.
                    // Paper entities use the 2-D paper transform; the model tab
                    // uses the model camera.
                    let screen_grips = if let Some((cam, _)) = &edit_frame {
                        grips_to_screen_rte(
                            &tab.selected_grips,
                            cam.view_proj_rte(bounds),
                            cam.eye(),
                            bounds,
                        )
                    } else if is_paper {
                        let cam = tab.scene.camera.borrow();
                        let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                        let half_h = cam.ortho_size();
                        let half_w = half_h * aspect;
                        let tx = cam.target.x as f32;
                        let ty = cam.target.y as f32;
                        drop(cam);
                        grips_to_screen_paper(&tab.selected_grips, tx, ty, half_w, half_h, bounds)
                    } else {
                        let cam = tab.scene.camera.borrow();
                        grips_to_screen(&tab.selected_grips, &cam, bounds)
                    };
                    screen_grips
                        .into_iter()
                        .filter(|(_, screen, _, _, _)| {
                            screen.x.is_finite()
                                && screen.y.is_finite()
                                && screen.x >= -bounds.width
                                && screen.x <= bounds.width * 2.0
                                && screen.y >= -bounds.height
                                && screen.y <= bounds.height * 2.0
                        })
                        .map(|(grip_id, screen, _is_midpoint, shape, dir)| {
                            let is_hot = tab
                                .active_grip
                                .as_ref()
                                .map_or(false, |g| Some(g.handle) == sel_h && g.grip_id == grip_id)
                                || Some(grip_id) == current_vertex_grip;
                            crate::ui::overlay::GripMarker {
                                pos: screen,
                                shape,
                                is_hot,
                                dir,
                            }
                        })
                        .collect()
                } else {
                    vec![]
                };
            let grip_clip = if grips.is_empty() {
                None
            } else {
                let (vw, vh) = tab.scene.selection.borrow().vp_size;
                Some(
                    tab.scene
                        .viewport_edit_frame((vw, vh))
                        .map(|(_, bounds)| bounds)
                        .unwrap_or_else(|| tab.scene.active_model_tile_bounds(vw, vh)),
                )
            };

            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            // Active tile rectangle (canvas-offset included) so grid / UCS
            // icon / crosshair project through the active pane's camera at
            // the correct place and scale.
            let vp_bounds = tab.scene.active_model_tile_bounds(vw, vh);

            // The UCS icon shows the active pane's UCS tripod: the model view, or
            // (inside a floating viewport) projected through the viewport camera
            // at the viewport's rect so it tracks the in-viewport UCS.
            // Rotation-only projection (view_proj_rte): the icon shows axis
            // DIRECTIONS only, so the full view_proj's huge UTM translation would
            // cancel catastrophically in f32 and make the tripod jitter.
            let ucs_icons: Vec<crate::ui::overlay::UcsIconParams> = if !self.show_ucs_icon {
                vec![]
            } else if let Some((vp_cam, full)) = tab.scene.viewport_edit_frame((vw, vh)) {
                let (_, ux, uy, uz) = tab.ucs_xform().axes();
                let origin_screen = self.ucs_icon_at_origin.then(|| {
                    vp_cam
                        .project(tab.ucs_origin_world(), full)
                        .map(|p| iced::Point::new(full.x + p.x, full.y + p.y))
                }).flatten();
                vec![crate::ui::overlay::UcsIconParams {
                    view_proj: vp_cam.view_proj_rte(full),
                    bounds: full,
                    axes: (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()),
                    origin_screen,
                    hover: self.ucs_icon_hover,
                    selected: self.ucs_icon_selected,
                }]
            } else if !is_paper {
                // One icon per Model pane — each at its own UCS origin, projected
                // through that pane's camera at its pane rect. Only the active
                // pane carries the interactive hover / selected grips.
                let (_, ux, uy, uz) = tab.ucs_xform().axes();
                let origin_w = tab.ucs_origin_world();
                let active = tab.scene.active_model_tile.get();
                let live = tab.scene.camera.borrow().clone();
                tab.scene
                    .model_tiles
                    .borrow()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let b = iced::Rectangle {
                            x: t.rect.x * vw,
                            y: t.rect.y * vh,
                            width: (t.rect.width * vw).max(1.0),
                            height: (t.rect.height * vh).max(1.0),
                        };
                        let cam = if i == active { live.clone() } else { t.camera.clone() };
                        let origin_screen = self.ucs_icon_at_origin.then(|| {
                            cam.project(origin_w, b)
                                .map(|p| iced::Point::new(b.x + p.x, b.y + p.y))
                        }).flatten();
                        crate::ui::overlay::UcsIconParams {
                            view_proj: cam.view_proj_rte(b),
                            bounds: b,
                            axes: (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()),
                            origin_screen,
                            hover: i == active && self.ucs_icon_hover,
                            selected: i == active && self.ucs_icon_selected,
                        }
                    })
                    .collect()
            } else {
                vec![]
            };

            // OST tracking points → screen positions, projected relative-to-eye
            // so they stay precise at UTM-scale coordinates (the full
            // view-projection cancels catastrophically in f32). Must project
            // through the active pane's camera at its rect (canvas offset
            // included) — like the grips and UCS icon above — or the marker
            // lands in the wrong place (tiled layout) or off-screen (floating
            // viewport). Without the `ob.x/ob.y` offset it silently vanishes.
            // Shared OTRACK projection basis: the active pane's camera + rect
            // (canvas offset included), matching the grips / UCS icon above.
            let otrack_proj: Option<(glam::Mat4, glam::DVec3, iced::Rectangle)> =
                if self.snapper.alignment_active() {
                    Some(
                        if let Some((vp_cam, full)) = tab.scene.viewport_edit_frame((vw, vh)) {
                            (vp_cam.view_proj_rte(full), vp_cam.eye(), full)
                        } else {
                            let cam = tab.scene.camera.borrow();
                            (cam.view_proj_rte(vp_bounds), cam.eye(), vp_bounds)
                        },
                    )
                } else {
                    None
                };
            let ost_project =
                |w: glam::DVec3, view_rot: glam::Mat4, eye: glam::DVec3, ob: iced::Rectangle| {
                    let ndc = view_rot.project_point3((w - eye).as_vec3());
                    iced::Point::new(
                        ob.x + (ndc.x + 1.0) * 0.5 * ob.width,
                        ob.y + (1.0 - ndc.y) * 0.5 * ob.height,
                    )
                };
            let ost_points: Vec<crate::ui::overlay::OstTrackPoint> =
                if let Some((view_rot, eye, ob)) = otrack_proj {
                    self.snapper
                        .tracking_points
                        .iter()
                        .filter_map(|&wp| {
                            let s = ost_project(wp, view_rot, eye, ob);
                            (s.x.is_finite() && s.y.is_finite())
                                .then_some(crate::ui::overlay::OstTrackPoint { screen: s })
                        })
                        .collect()
                } else {
                    vec![]
                };
            // The active alignment vector: a dashed guide from the acquired
            // tracking point through the locked cursor, so the user sees the
            // extension / tracking line they are snapped to (#219).
            let otrack_line: Option<(iced::Point, iced::Point)> =
                match (otrack_proj, self.otrack_active) {
                    (Some((view_rot, eye, ob)), Some((base, _dir))) => {
                        let b = ost_project(base, view_rot, eye, ob);
                        let a = ost_project(tab.last_cursor_world, view_rot, eye, ob);
                        (b.x.is_finite() && a.x.is_finite()).then_some((b, a))
                    }
                    _ => None,
                };
            // The acquired Parallel-snap reference, marked on its line (#277).
            let parallel_ref_marker: Option<iced::Point> =
                match (otrack_proj, self.snapper.parallel_ref) {
                    (Some((view_rot, eye, ob)), Some((_, pt))) => {
                        let s = ost_project(pt.as_dvec3(), view_rot, eye, ob);
                        (s.x.is_finite() && s.y.is_finite()).then_some(s)
                    }
                    _ => None,
                };

            // Model-space pane dividers (none in paper / single-pane layouts).
            let dividers = if !is_paper {
                let (vw, vh) = tab.scene.selection.borrow().vp_size;
                tab.scene.model_pane_dividers(vw, vh)
            } else {
                vec![]
            };

            // Pane move (drag-to-swap) visuals: source pane rect + the drop
            // target pane under the cursor.
            let (pane_move_rect, pane_drop_rect) = match self.pane_move_from {
                Some(from) if !is_paper => {
                    let (vw, vh) = tab.scene.selection.borrow().vp_size;
                    let cursor = tab.scene.selection.borrow().last_move_pos;
                    let tiles = tab.scene.model_tiles.borrow();
                    let px = |t: &crate::scene::ModelTile| iced::Rectangle {
                        x: t.rect.x * vw,
                        y: t.rect.y * vh,
                        width: t.rect.width * vw,
                        height: t.rect.height * vh,
                    };
                    let src = tiles.get(from).map(px);
                    let drop = cursor.and_then(|c| {
                        tiles.iter().enumerate().find(|(i, t)| {
                            *i != from
                                && c.x >= t.rect.x * vw
                                && c.x < (t.rect.x + t.rect.width) * vw
                                && c.y >= t.rect.y * vh
                                && c.y < (t.rect.y + t.rect.height) * vh
                        })
                    });
                    (src, drop.map(|(_, t)| px(t)))
                }
                _ => (None, None),
            };

            let hover_locked = tab
                .scene
                .hover_highlight
                .map(|h| tab.scene.is_layer_locked(h))
                .unwrap_or(false);
            crate::ui::overlay::selection_overlay(
                sel,
                snap_info,
                snap_ext_base,
                snap_ext_base2,
                grips,
                grip_clip,
                ucs_icons,
                ost_points,
                otrack_line,
                parallel_ref_marker,
                // ViewCube hover region matches the drawn cube — gone when hidden.
                !is_paper && viewcube_visible,
                dividers,
                pane_move_rect,
                pane_drop_rect,
                tab.pan_mode,
                self.ribbon.open_dropdown.is_some(),
                hover_locked,
            )
        };

        let viewport_mouse = mouse_area(container(
            iced::widget::Space::new().width(Fill).height(Fill),
        ))
        .on_move(Message::ViewportMove)
        .on_press(Message::ViewportLeftPress)
        .on_release(Message::ViewportLeftRelease)
        .on_right_press(Message::ViewportRightPress)
        .on_right_release(Message::ViewportRightRelease)
        .on_middle_press(Message::ViewportMiddlePress)
        .on_middle_release(Message::ViewportMiddleRelease)
        .on_scroll(Message::ViewportScroll)
        .on_exit(Message::ViewportExit);

        let bg_color = if is_paper {
            // Desk color — matches the DESK constant in paper_canvas.rs.
            Color {
                r: 0.22,
                g: 0.24,
                b: 0.28,
                a: 1.0,
            }
        } else {
            tab.bg_color
                .map(|[r, g, b, a]| Color { r, g, b, a })
                .unwrap_or(Color {
                    // Default model background: RGB (33, 40, 48).
                    r: 33.0 / 255.0,
                    g: 40.0 / 255.0,
                    b: 48.0 / 255.0,
                    a: 1.0,
                })
        };

        // Dynamic input overlay — editable boxes near the cursor, one per
        // quantity the active command is asking for (X/Y, or polar
        // distance+angle, or a single distance/angle). TAB moves focus
        // between boxes; typing locks a box to a fixed value while the
        // rest keep tracking the cursor. The field set is maintained in
        // `tab.dyn_fields` by `sync_dyn_fields`.
        // A pick step (object selection) has no input box, but still shows
        // its prompt ("Select first object …") near the cursor as a hint.
        let dyn_picks_object = tab
            .active_cmd
            .as_ref()
            .map(|c| c.needs_entity_pick() || c.needs_structure_point_pick())
            .unwrap_or(false);
        let dyn_input_overlay: Option<Element<'_, Message>> =
            if self.dyn_input
                && tab.active_cmd.is_some()
                && (!tab.dyn_fields.is_empty() || dyn_picks_object)
            {
                let w = tab.last_cursor_world;
                let base = self.last_point;
                // A command may drive a typed scalar by mouse (e.g. a
                // perpendicular distance to a picked object); show that live
                // value in the box until the user types over it.
                let live = tab.active_cmd.as_ref().and_then(|c| c.dyn_live_value(w));
                let boxes: Vec<crate::ui::overlay::DynBox> = tab
                    .dyn_fields
                    .iter()
                    .enumerate()
                    .map(|(idx, f)| {
                        let value = match (&f.buffer, live) {
                            (Some(b), _) => b.clone(),
                            // An angle step with a command-supplied live value
                            // (ARC span / direction) shows it in degrees.
                            (None, Some(lv)) if f.component == DynComponent::Angle => {
                                format!("{lv:.1}")
                            }
                            (None, Some(lv))
                                if matches!(
                                    f.component,
                                    DynComponent::Scalar | DynComponent::Distance
                                ) =>
                            {
                                format!("{lv:.4}")
                            }
                            _ => dyn_component_value(
                                f,
                                w,
                                base,
                                &tab.ucs_xform(),
                                self.dyn_user_reshaped,
                            ),
                        };
                        crate::ui::overlay::DynBox {
                            label: f.role.label().to_string(),
                            value,
                            active: idx == tab.dyn_active,
                            locked: f.locked(),
                            role: f.role,
                        }
                    })
                    .collect();
                let prompt = tab
                    .active_cmd
                    .as_ref()
                    .map(|c| c.prompt())
                    .unwrap_or_default();
                Some(crate::ui::overlay::dynamic_input_overlay(
                    tab.last_cursor_screen,
                    tab.last_point_screen,
                    tab.dyn_ref_screen,
                    tab.dyn_guide,
                    boxes,
                    prompt,
                ))
            } else {
                None
            };

        let mut viewport_stack = if tab.is_start {
            // Start tab: only the welcome widget over a flat background.
            // Skip every drawing-only overlay (selection markers, snap info,
            // mouse-area capturing draw clicks, viewcube, nav toolbar, …).
            stack![container(viewport_3d)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(bg_color)),
                    ..Default::default()
                })
                .width(Fill)
                .height(Fill)]
            .width(Fill)
            .height(Fill)
        } else if is_paper {
            // Paper layout: the GPU shader renders everything — the desk is the
            // container background, the white sheet + paper entities + borders
            // come from the full-canvas top-locked "sheet" viewport, and the
            // floating content viewports overlay it (same path as model space).
            const DESK: Color = Color {
                r: 0.22,
                g: 0.24,
                b: 0.28,
                a: 1.0,
            };
            stack![
                container(grid_overlay)
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(DESK)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                viewport_3d,
                selection_overlay,
                viewport_mouse,
            ]
            .width(Fill)
            .height(Fill)
        } else {
            stack![
                container(grid_overlay)
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(bg_color)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                viewport_3d,
                selection_overlay,
            ]
            .width(Fill)
            .height(Fill)
        };

        // Per-pane input pane_grid goes ABOVE the crosshair overlay so it
        // receives mouse events (the overlay's `Hidden` cursor would otherwise
        // starve any layer beneath it). The controls bar is pushed on top of it.
        if let Some(input) = model_input_layer {
            viewport_stack = viewport_stack.push(input);
        }

        // Model-space render-mode picker, top-left. Sits ABOVE the
        // viewport mouse_area so clicks inside its bounds reach it
        // instead of the shader behind it; `opaque` stops them bubbling
        // further. Outside the chip the Fill container is transparent so
        // viewport drawing / selection is unaffected. In a paper layout
        // the active viewport gets its own picker (below) instead.
        if !is_paper && !tab.is_start {
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let rect = tab.scene.active_model_tile_bounds(vw, vh);
            // Unified control chip: split buttons + render-mode picker +
            // grid / grid-snap toggles, for the active Model tile.
            let bar = viewport_controls(
                tab.render_mode,
                self.show_grid,
                self.snapper.grid_snap(),
                true,
                tab.scene.model_tiles.borrow().len(),
            );
            // Adaptive: DensitySwap measures the bar's real width every frame
            // (reported into `render_bar_w`, which the ViewCube reads to decide
            // overlap) and swaps it for an empty spacer only when it no longer
            // fits the tile. The fixed-width container bounds that fit decision
            // to the tile, not the whole canvas.
            let adaptive: Element<'_, Message> = DensitySwap::new(vec![
                iced::widget::opaque(bar),
                Space::new()
                    .width(iced::Length::Fixed(0.0))
                    .height(iced::Length::Fixed(0.0))
                    .into(),
            ])
            .report_width0(self.render_bar_w.clone())
            .into();
            // Pin the bar to the active model tile's top-left corner so it
            // follows the active panel in a tiled layout.
            let bar_layer = crate::ui::pin_at(
                iced::Point::new(rect.x, rect.y),
                container(adaptive).width(iced::Length::Fixed(rect.width.max(1.0))),
            );
            viewport_stack = viewport_stack.push(bar_layer);
        }

        // Active paper-space viewport overlays: a render-mode picker in
        // its top-left corner and a ViewCube hit area in its top-right,
        // both layered ABOVE the viewport mouse_area so they receive
        // clicks (the shader viewport sits below it). Positioned with
        // leading Spaces sized to the viewport's screen rectangle.
        let active_vp_rect: Option<iced::Rectangle> = if is_paper && !tab.is_start {
            tab.scene.active_viewport.and_then(|h| {
                let (cw, ch) = tab.scene.selection.borrow().vp_size;
                tab.scene.viewport_screen_rect(h, (cw, ch))
            })
        } else {
            None
        };
        if let Some(rect) = active_vp_rect {
            // Clip the outline to the visible canvas. Clamping only the origin
            // (max(0.0)) while keeping the full width/height shifted the whole
            // outline inward when the viewport ran off the top/left edge, so
            // its drawn border no longer matched the real viewport — clicks
            // that looked outside landed in (and activated) another viewport.
            let (cw, ch) = tab.scene.selection.borrow().vp_size;
            let x = rect.x.max(0.0);
            let y = rect.y.max(0.0);
            let vw = ((rect.x + rect.width).min(cw) - x).max(1.0);
            let vh = ((rect.y + rect.height).min(ch) - y).max(1.0);
            // Highlight the active viewport with a 2-px border so its
            // boundary is always visible over the GPU shader.
            const VP_BORDER: Color = Color {
                r: 0.18,
                g: 0.52,
                b: 0.95,
                a: 1.0,
            };
            let border_frame = container(
                Space::new()
                    .width(iced::Length::Fixed(vw))
                    .height(iced::Length::Fixed(vh)),
            )
            .style(move |_: &Theme| container::Style {
                border: iced::Border {
                    color: VP_BORDER,
                    width: 2.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });
            let border_layer = crate::ui::pin_at(iced::Point::new(x, y), border_frame);
            viewport_stack = viewport_stack.push(border_layer);

            let vp_mode = tab
                .scene
                .active_viewport_render_mode()
                .unwrap_or(acadrust::entities::ViewportRenderMode::Wireframe2D);
            // Adaptive (same as model): the picker measures its real width into
            // `render_bar_w` and swaps to an empty spacer only when the viewport
            // can't hold it; the ViewCube reads that width to decide overlap.
            let bar = viewport_controls(
                vp_mode,
                self.show_grid,
                self.snapper.grid_snap(),
                false,
                0,
            );
            let adaptive: Element<'_, Message> = DensitySwap::new(vec![
                iced::widget::opaque(bar),
                Space::new()
                    .width(iced::Length::Fixed(0.0))
                    .height(iced::Length::Fixed(0.0))
                    .into(),
            ])
            .report_width0(self.render_bar_w.clone())
            .into();
            let picker_layer = crate::ui::pin_at(
                iced::Point::new(x + 4.0, y + 4.0),
                container(adaptive).width(iced::Length::Fixed(rect.width.max(1.0))),
            );
            viewport_stack = viewport_stack.push(picker_layer);

            // Hide the ViewCube first — before the render bar — when they collide.
            if viewcube_visible {
                let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
                let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);

                let controls = crate::ui::pin_at(
                    iced::Point::new(cube_x, cube_y),
                    viewcube_nav_controls(),
                );
                viewport_stack = viewport_stack.push(controls);

                let ucs_current = tab
                    .active_ucs
                    .as_ref()
                    .map(|u| u.name.clone())
                    .unwrap_or_default();
                let ucs_names: Vec<String> = tab
                    .scene
                    .document
                    .ucss
                    .iter()
                    .map(|u| u.name.clone())
                    .filter(|n| !n.is_empty())
                    .collect();
                let picker = crate::ui::pin_at(
                    iced::Point::new(
                        cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                        cube_y + VIEWCUBE_HIT_SIZE + 6.0,
                    ),
                    iced::widget::opaque(viewcube_ucs_picker(ucs_current, ucs_names)),
                );
                viewport_stack = viewport_stack.push(picker);
            }
        }

        if viewcube_visible && !is_paper {
            // Place the ViewCube hit area in the active model tile's top-right
            // corner so it tracks the active panel in a tiled layout. The hit
            // test in update.rs already maps clicks through the active tile.
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let rect = tab.scene.active_model_tile_bounds(vw, vh);
            let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
            let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);

            // Cube hit area + nav controls (home / roll / nudge) as one layer.
            let controls = crate::ui::pin_at(
                iced::Point::new(cube_x, cube_y),
                viewcube_nav_controls(),
            );
            viewport_stack = viewport_stack.push(controls);

            // WCS / named-UCS selector under the cube.
            let ucs_current = tab
                .active_ucs
                .as_ref()
                .map(|u| u.name.clone())
                .unwrap_or_default();
            let ucs_names: Vec<String> = tab
                .scene
                .document
                .ucss
                .iter()
                .map(|u| u.name.clone())
                .filter(|n| !n.is_empty())
                .collect();
            let picker = crate::ui::pin_at(
                iced::Point::new(
                    cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                    cube_y + VIEWCUBE_HIT_SIZE + 6.0,
                ),
                iced::widget::opaque(viewcube_ucs_picker(ucs_current, ucs_names)),
            );
            viewport_stack = viewport_stack.push(picker);
        }

        if let Some(dyn_ol) = dyn_input_overlay {
            if !tab.is_start {
                viewport_stack = viewport_stack.push(dyn_ol);
            }
        }

        // Multi-functional grip popup (Phase 2). One bordered container
        // wraps a column of borderless item buttons so the popup reads
        // as a single widget instead of stacked tiles.
        if let Some(popup) = self.grip_popup.as_ref() {
            if !tab.is_start {
                // Size the row to the widest label so the selection
                // highlight fills the whole row instead of just the
                // text glyphs. ~7 px per character at size 12 + the
                // horizontal padding (10 + 10).
                let max_len = popup
                    .items
                    .iter()
                    .map(|i| i.label.chars().count())
                    .max()
                    .unwrap_or(8) as f32;
                let row_w = max_len * 7.0 + 24.0;
                let mut col = column![].spacing(0).width(iced::Length::Fixed(row_w));
                for (idx, item) in popup.items.iter().enumerate() {
                    let is_sel = idx == popup.selected;
                    let label = item.label;
                    let btn = button(text(label).size(12))
                        .on_press(Message::GripMenuPick(idx))
                        .padding([3, 10])
                        .width(Fill)
                        .style(move |theme: &Theme, status| {
                            let palette = theme.extended_palette();
                            let pair = match (is_sel, status) {
                                (true, _) => Some(palette.primary.strong),
                                (_, iced::widget::button::Status::Hovered) => {
                                    Some(palette.background.strong)
                                }
                                _ => None,
                            };
                            iced::widget::button::Style {
                            background: pair.map(|p| Background::Color(p.color)),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 0.0.into(),
                            },
                            text_color: pair
                                .map(|p| p.text)
                                .unwrap_or(palette.background.base.text),
                            ..Default::default()
                            }
                        });
                    col = col.push(btn);
                }
                let menu_panel = container(col)
                    .padding(2)
                    .style(|theme: &Theme| {
                        let palette = theme.extended_palette();
                        container::Style {
                        background: Some(Background::Color(palette.background.weak.color)),
                        border: Border {
                            color: palette.background.neutral.color,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                        }
                    });
                // Offset the menu by 12 px so the cursor doesn't land on
                // the first item immediately, matching the right-click
                // context menu's "panel below the click point" feel.
                let anchor = iced::Point::new(popup.anchor.x + 12.0, popup.anchor.y + 12.0);
                viewport_stack =
                    viewport_stack.push(position_canvas_overlay(anchor, menu_panel.into()));
            }
        }

        // Dynamic-block visibility-state dropdown.
        if let Some(popup) = self.visibility_popup.as_ref() {
            if !tab.is_start {
                let max_len = popup
                    .items
                    .iter()
                    .map(|s| s.chars().count())
                    .max()
                    .unwrap_or(4) as f32;
                // +2 chars for the leading "✓ " / "  " marker column.
                let row_w = (max_len + 2.0) * 7.0 + 24.0;
                let mut col = column![].spacing(0).width(iced::Length::Fixed(row_w));
                for (idx, name) in popup.items.iter().enumerate() {
                    let is_cur = popup.current == Some(idx);
                    let mark: Element<'_, Message> = if is_cur {
                        crate::ui::icons::themed_check_cell(true)
                    } else {
                        Space::new().width(11).into()
                    };
                    let btn = button(
                        row![
                            container(mark).width(16),
                            text(name).size(12),
                        ]
                        .spacing(2)
                        .align_y(iced::Center),
                    )
                    .on_press(Message::VisibilityPick(idx))
                        .padding([3, 10])
                        .width(Fill)
                        .style(move |theme: &Theme, status| {
                            let palette = theme.extended_palette();
                            iced::widget::button::Style {
                            background: matches!(
                                status,
                                iced::widget::button::Status::Hovered
                            )
                            .then_some(Background::Color(palette.primary.weak.color)),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 0.0.into(),
                            },
                            text_color: palette.background.base.text,
                            ..Default::default()
                            }
                        });
                    col = col.push(btn);
                }
                let panel = container(iced::widget::scrollable(col).height(iced::Length::Shrink))
                    .max_height(360.0)
                    .padding(2)
                    .style(|theme: &Theme| {
                        let palette = theme.extended_palette();
                        container::Style {
                        background: Some(Background::Color(palette.background.weak.color)),
                        border: Border {
                            color: palette.background.neutral.color,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                        }
                    });
                let anchor = iced::Point::new(popup.anchor.x + 12.0, popup.anchor.y + 12.0);
                viewport_stack =
                    viewport_stack.push(position_canvas_overlay(anchor, panel.into()));
            }
        }

        // Paper-space context actions: a right-edge vertical toolbar
        // (viewport / page setup / plot) instead of a contextual ribbon tab.
        if is_paper && !tab.is_start {
            if let Some(tb) = crate::ui::side_toolbar::view(
                &crate::modules::layout::paper_space_tools(),
            ) {
                viewport_stack = viewport_stack.push(tb);
            }
        }

        // In-place block edit (REFEDIT): right-edge toolbar with Save / Discard
        // so the edit can be finished by clicking. (#136)
        if tab.refedit_session.is_some() && !tab.is_start {
            if let Some(tb) = crate::ui::side_toolbar::view(
                &crate::modules::draw::modify::refedit::refedit_tools(),
            ) {
                viewport_stack = viewport_stack.push(tb);
            }
        }

        // BEDIT block editor: right-edge Save Block / Discard toolbar (#261).
        if tab.block_edit.is_some() && !tab.is_start {
            if let Some(tb) = crate::ui::side_toolbar::view(
                &crate::modules::draw::modify::block_edit::block_edit_tools(),
            ) {
                viewport_stack = viewport_stack.push(tb);
            }
        }

        // Quick Properties: compact floating property panel on selection,
        // anchored at the canvas top-left so it doesn't track the cursor.
        if self.quick_properties && !tab.is_start {
            if let Some(panel) = tab.properties.quick_view() {
                viewport_stack = viewport_stack
                    .push(position_canvas_overlay(iced::Point::new(12.0, 12.0), panel));
            }
        }

        // Shared performance panel: terminal PERF lines plus the current
        // tessellation summary. Copy / Clear mirror the command-history panel.
        if self.perf_hud {
            let s = &tab.scene;
            let perf_w = if render_bar_w.is_finite() && render_bar_w > 1.0 {
                render_bar_w
            } else {
                320.0
            };
            let summary = format!(
                "tess {:.1} ms · {} wires · epoch {}",
                s.last_tess_ms.get(),
                s.last_tess_wires.get(),
                s.geometry_epoch,
            );
            let trace = crate::perf::snapshot_text();
            let trace = if trace.is_empty() {
                "No samples yet".to_string()
            } else {
                trace
            };
            let perf_button_style = |theme: &Theme, status: button::Status| {
                let palette = theme.extended_palette();
                let pair = if matches!(status, button::Status::Hovered) {
                    palette.background.strong
                } else {
                    palette.background.weak
                };
                button::Style {
                background: Some(Background::Color(pair.color)),
                text_color: pair.text,
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
                }
            };
            let copy_btn = button(
                row![
                    crate::ui::icons::themed_primary(crate::ui::icons::COPY, 11.0),
                    text("Copy").size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::PerfCopy)
            .style(perf_button_style)
            .padding([2, 6]);
            let clear_btn = button(
                row![
                    crate::ui::icons::themed_danger(crate::ui::icons::TRASH, 11.0),
                    text("Clear").size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::PerfClear)
            .style(perf_button_style)
            .padding([2, 6]);
            let header = row![
                text("PERF").size(12).style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().success.base.color),
                }),
                Space::new().width(iced::Length::Fill),
                copy_btn,
                clear_btn,
            ]
            .spacing(6)
            .align_y(iced::Center);
            let log = scrollable(text(trace).size(11))
                .height(iced::Length::Fixed(220.0))
                .width(iced::Length::Fill);
            let panel = container(
                column![
                    header,
                    text(summary).size(11).style(|theme: &Theme| iced::widget::text::Style {
                        color: Some(theme.extended_palette().success.base.color),
                    }),
                    log,
                ]
                .spacing(5),
            )
            .width(iced::Length::Fixed(perf_w))
            .padding(6)
            .style(|_: &Theme| container::Style {
                background: None,
                border: Border::default(),
                ..Default::default()
            });
            viewport_stack = viewport_stack.push(position_canvas_overlay(
                iced::Point::new(12.0, 40.0),
                panel.into(),
            ));
        }

        // Selection-cycling list box: pick among overlapping objects.
        if let Some((pt, cands)) = &self.cycle_candidates {
            if !tab.is_start {
                let items: Vec<(acadrust::Handle, String)> = cands
                    .iter()
                    .filter_map(|&h| {
                        tab.scene
                            .document
                            .get_entity(h)
                            .map(|e| (h, crate::entities::traits::entity_type_name(e).to_string()))
                    })
                    .collect();
                if !items.is_empty() {
                    viewport_stack = viewport_stack
                        .push(crate::ui::popup::cycle_popup::cycle_popup_overlay(*pt, items));
                }
            }
        }

        // Right-click context menu. Lives inside the viewport stack so
        // the cursor position (canvas-relative) anchors the menu under
        // the cursor instead of drifting into window-relative space.
        if !tab.is_start {
            let (ctx_pos, draworder_open) = {
                let sel = tab.scene.selection.borrow();
                (sel.context_menu, sel.draworder_submenu)
            };
            if let Some(p) = ctx_pos {
                let has_cmd = tab.active_cmd.is_some();
                let has_selection = !tab.scene.selected.is_empty();
                let isolation_active = tab.scene.is_isolation_active();
                let last_cmds: Vec<String> = self
                    .command_line
                    .recent_commands
                    .iter()
                    .rev()
                    .take(3)
                    .cloned()
                    .collect();
                viewport_stack = viewport_stack.push(viewport_context_menu_overlay(
                    p,
                    has_cmd,
                    has_selection,
                    isolation_active,
                    last_cmds,
                    draworder_open,
                ));
            }
        }

        // In-place MText editor (toolbar + text area), anchored at the
        // insertion-point click.
        if !tab.is_start {
            let canvas = tab.scene.selection.borrow().vp_size;
            if let Some(ed) = &self.mtext_editor {
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                viewport_stack = viewport_stack.push(mtext_editor_overlay(
                    ed,
                    styles,
                    canvas,
                    self.modal_offset,
                    self.modal_resize,
                ));
            }
            if let Some(ed) = &self.text_inline {
                viewport_stack = viewport_stack.push(text_inline_overlay(ed, canvas));
            }
        }

        // The Start tab shows its own three-panel page (Recent · Welcome ·
        // Supporters) in place of the viewport, so the left properties slot is
        // empty there.
        let properties_el: Element<'_, Message> = if tab.is_start {
            Space::new().into()
        } else if self.show_properties && !self.clean_screen {
            // On a narrow window the panel collapses to a vertical "Properties"
            // bar to free width for the viewport; clicking the bar expands it.
            let narrow = self.win_size.0 < 1000.0;
            let bar = || collapse_bar("Properties", Message::TogglePropertiesBar);
            if narrow && !self.props_expanded {
                bar()
            } else if narrow {
                row![bar(), tab.properties.view()].into()
            } else {
                tab.properties.view()
            }
        } else {
            Space::new().into()
        };

        // Command-line sits as a bottom-centre overlay on top of the
        // viewport stack rather than as a separate row in the main
        // column — frees up vertical space when no command is active
        // and keeps the input close to where the cursor is drawing.
        // Autocomplete shows only when no command is collecting its
        // own input (otherwise typed prefixes are coordinates / values).
        let allow_autocomplete = tab.active_cmd.is_none();
        // Dynamic input captures keystrokes when its fields are showing,
        // so the command-line field must release focus / its on_input.
        // The MText preview also captures keystrokes (typing edits it), so the
        // command line must likewise release its on_input there.
        let dyn_capturing =
            (self.dyn_input && tab.active_cmd.is_some() && !tab.dyn_fields.is_empty())
                || self.mtext_editor.as_ref().is_some_and(|e| e.show_preview)
                || self.text_inline.is_some();
        let command_line_overlay =
            iced::widget::container(self.command_line.view(
                allow_autocomplete,
                dyn_capturing,
                &self.history_content,
            ))
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: 2.0,
                    left: 0.0,
                });

        let center_stack = iced::widget::stack![
            row![properties_el, viewport_stack].width(Fill).height(Fill),
            command_line_overlay,
        ]
        .width(Fill)
        .height(Fill);

        let main_ui = container({
            // Clean-screen mode drops the ribbon for a full-canvas view; the
            // status bar stays so the mode can be toggled back off.
            let mut col = column![];
            if !self.clean_screen {
                col = col.push(self.ribbon.view(
                    is_paper,
                    self.tabs[self.active_tab].is_start,
                    self.tabs[self.active_tab].history.undo_stack.len(),
                    self.tabs[self.active_tab].history.redo_stack.len(),
                ));
            }
            if self.show_file_tabs {
                col = col.push(doc_tab_bar(&self.tabs, self.active_tab));
            }
            col.push(center_stack)
                .push({
                    let is_model = tab.scene.current_layout == "Model";
                    let scale_pill_enabled = is_model
                        || tab.scene.active_viewport.is_some()
                        || tab.scene.has_selected_viewport();
                    // The cursor is tracked in local render space; re-add the
                    // model-space world offset so the readout shows true
                    // drawing coordinates (paper space carries no offset), then
                    // report it in the active UCS — the readout follows the
                    // user's coordinate system, not raw WCS (no-op without UCS).
                    let to_readout = |p: glam::DVec3| {
                        // The readout follows the active pane's UCS — model space
                        // or inside a floating viewport (no-op without a UCS).
                        if tab.editing_model_space() {
                            tab.ucs_xform().to_ucs(p)
                        } else {
                            p
                        }
                    };
                    let cursor_coord = to_readout(tab.last_cursor_world);
                    // The last picked point (same UCS as the cursor) drives the
                    // static ($COORDS 0) and polar ($COORDS 2) readouts.
                    let last_coord = self.last_point.map(to_readout);
                    let coords_mode = tab.scene.document.header.coords_mode;
                    let picking = tab.active_cmd.is_some();
                    let layout_names = tab.scene.layout_names();
                    let mut displayed_layouts = layout_names.clone();
                    if let Some(be) = &tab.block_edit {
                        displayed_layouts.push(be.block_name.clone());
                    }
                    let status_menu_data = crate::ui::statusbar::StatusMenuData {
                        layout_names: layout_names.clone(),
                        polar_custom_input: &self.polar_custom_input,
                        scale_is_model: is_model,
                        scale_list: tab.scene.scale_list(),
                        has_selection: !tab.scene.selected.is_empty(),
                        selection_types: tab
                            .scene
                            .entity_type_names_in_layout()
                            .into_iter()
                            .map(|name| name.to_string())
                            .collect(),
                        selection_filter: &tab.scene.selection_filter,
                        tooltip_hidden: self.status_menu_tooltip_hidden,
                    };
                    self.status_bar.view(
                        &self.snapper,
                        self.ortho_mode,
                        self.polar_mode,
                        self.polar_increment_deg,
                        self.dyn_input,
                        self.snapper.otrack_enabled,
                        displayed_layouts,
                        layout_names.into_iter().skip(1).collect(),
                        tab.block_edit
                            .as_ref()
                            .map(|be| be.block_name.clone())
                            .unwrap_or_else(|| tab.scene.current_layout.clone()),
                        tab.is_start,
                        self.layout_rename_state.as_ref(),
                        tab.scene.first_viewport_scale(),
                        tab.scene.viewport_count(),
                        tab.scene.active_viewport.is_some(),
                        self.show_layout_tabs,
                        tab.scene.annotation_scale,
                        scale_pill_enabled,
                        tab.scene.document.header.lineweight_display,
                        cursor_coord,
                        coords_mode,
                        last_coord,
                        picking,
                        self.clean_screen,
                        tab.scene.document.header.insertion_units,
                        tab.scene.is_isolation_active(),
                        tab.scene.transparency_display,
                        self.quick_properties,
                        tab.scene.selection_filter_active(),
                        self.selection_cycling,
                        &self.statusbar_config,
                        status_menu_data,
                    )
                })
                .width(Fill)
                .height(Fill)
        })
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill);

        let dropdown_layer: Element<'_, Message> = self
            .ribbon
            .dropdown_overlay(
                &history_dropdown_labels(&self.tabs[self.active_tab].history.undo_stack),
                &history_dropdown_labels(&self.tabs[self.active_tab].history.redo_stack),
                self.win_size,
            )
            .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into());

        let snap_override_layer: Element<'_, Message> =
            if let Some(pos) = self.snap_override_popup {
                overlay::snap_override_overlay(pos)
            } else {
                iced::widget::Space::new().width(0).height(0).into()
            };

        let qselect_layer: Element<'_, Message> = if let Some(state) = &self.qselect {
            let types = tab.scene.entity_type_names_in_layout();
            let properties = tab.scene.qselect_properties(state.type_filter.as_deref());
            qselect_overlay(state, &types, &properties)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let open_progress_layer: Element<'_, Message> = if let Some(p) = &self.opening {
            crate::ui::window::open_progress::view(p, iced::time::Instant::now())
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let composed = stack![
            main_ui,
            dropdown_layer,
            qselect_layer,
            snap_override_layer,
            open_progress_layer,
        ];

        // ── In-canvas modal dialogs (Plan B) ───────────────────────────────
        // Former pop-up windows render as overlays here, so they work on both
        // the native (single main window) and web builds.
        let base: Element<'_, Message> = match self.modal_content() {
            Some(content) => {
                crate::ui::modal::modal(
                    composed,
                    self.modal_title(),
                    // Content width — outer size minus the frame padding.
                    self.modal_outer_size().map(|s| s.0 - 20.0).unwrap_or(420.0),
                    content,
                    Message::CloseModal,
                    self.modal_offset,
                    true,
                )
            }
            None => composed.into(),
        };
        // The colour picker is a nested modal: it stacks over whichever dialog
        // (style editor, properties, …) requested it.
        if self.color_pick_target.is_some() {
            crate::ui::modal::modal(
                base,
                "Select Color",
                420.0,
                iced::widget::container(crate::ui::color_select::color_grid_window(
                    Message::ColorWindowPick,
                ))
                .width(iced::Length::Fixed(420.0))
                .height(iced::Length::Fixed(470.0)),
                Message::CloseColorPicker,
                iced::Vector::ZERO,
                false,
            )
        } else {
            base
        }
    }

    /// Outer pixel size (content + title-bar/padding chrome) of the active
    /// modal, used to clamp drag so it cannot be pushed off-screen. Mirrors the
    /// `sized(..)` dimensions in [`Self::modal_content`]; keep the two in sync.
    /// `None` has no active modal. About (content-sized) uses a safe estimate.
    pub(crate) fn modal_outer_size(&self) -> Option<(f32, f32)> {
        use super::ModalKind::*;
        // Title bar (~26) + spacing (6) + frame padding (10·2) → ~52 vertical;
        // frame padding → ~20 horizontal.
        const EXTRA_W: f32 = 20.0;
        const EXTRA_H: f32 = 52.0;
        let (w, h) = match self.active_modal? {
            About => (440, 360),
            Shortcuts => (720, 520),
            Options => (480, 190),
            PluginManager => (520, 460),
            UpdateNotice => (560, 460),
            Layers => (900, 360),
            Plot => (760, 540),
            LayoutManager => (640, 320),
            Plotstyle => (780, 540),
            TextStyle => (860, 480),
            MlStyle => (620, 420),
            TableStyle => (620, 420),
            MLeaderStyle => (560, 560),
            DimStyle => (720, 560),
            AssocPrompt => (440, 210),
            Unsaved => (420, 160),
            AecDropWarning => (480, 230),
            #[cfg(not(target_arch = "wasm32"))]
            FileInUse => (560, 250),
            #[cfg(not(target_arch = "wasm32"))]
            ExternalChange => (620, 250),
            #[cfg(target_arch = "wasm32")]
            SaveDialog => (420, 200),
            #[cfg(not(target_arch = "wasm32"))]
            SaveDialog => (420, 150),
            PointStyle => (360, 470),
            AttributeEditor => (640, 500),
            LayerDeleteWarning => (440, 200),
            Aliases => (480, 520),
            ScaleManager => (520, 360),
            AnnoObjectScale => (360, 420),
        };
        // Include the user's corner-resize growth so the drag clamp tracks the
        // dialog's actual footprint.
        Some((
            w as f32 + EXTRA_W + self.modal_resize.x,
            h as f32 + EXTRA_H + self.modal_resize.y,
        ))
    }
}

impl OpenCADStudio {
    pub fn subscription(&self) -> Subscription<Message> {
        use iced::event;
        // Only request per-frame ticks while something on screen is animating
        // (currently just the open-progress indicator). Without this gate the
        // app burned 2-3% CPU continuously redrawing an unchanged view.
        // See #18.
        let needs_frames = self.opening.is_some();
        let frames = if needs_frames {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // While the command-line overlay is still displaying any
        // recently-pushed history entry, re-render every frame so the
        // entry disappears at the moment its visible window expires.
        // The subscription auto-stops once no entry is fresh enough
        // (typically within a few seconds of the last command).
        let history_tick = if self.command_line.has_visible_history() {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // While the cursor sits over a grip, request animation frames
        // so the multi-functional popup opens even when the user keeps
        // the mouse perfectly still — `ViewportMove` alone would never
        // fire again. Auto-stops once the hover clears or the popup is
        // already open.
        let grip_dwell = if self.grip_hover.is_some() && self.grip_popup.is_none() {
            window::frames().map(|_| Message::GripDwellTick)
        } else {
            Subscription::none()
        };
        // While a rollover pick is queued, drive ticks so it fires the
        // moment the cursor has been still for the dwell window — without
        // this `ViewportMove` alone never re-fires once the user stops.
        let hover_dwell = if self.hover_dwell.is_some() {
            window::frames().map(|_| Message::HoverDwellTick)
        } else {
            Subscription::none()
        };
        // Interaction LOD: while the view is (or just was) navigating, keep
        // requesting frames a little past the settle point. Panning itself is
        // driven by input events, but once the cursor stops no event would fire
        // the one full-quality frame that re-renders hatches — this tick does,
        // then the scene-render cache holds it and the subscription auto-stops.
        let nav_settle = if self.tabs[self.active_tab].scene.is_settling() {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // Blink the MText preview caret while the editor is open.
        let caret_blink = if self.mtext_editor.is_some() {
            iced::time::every(std::time::Duration::from_millis(530))
                .map(|_| Message::MTextCaretBlink)
        } else {
            Subscription::none()
        };
        // Web: poll for per-script fonts that a drawing's text needs but hasn't
        // fetched yet. Cheap — `PollWebFonts` is a no-op when nothing is
        // pending. Native has system fonts, so no polling. (#141)
        #[cfg(target_arch = "wasm32")]
        let web_fonts =
            iced::time::every(std::time::Duration::from_millis(300)).map(|_| Message::PollWebFonts);
        #[cfg(not(target_arch = "wasm32"))]
        let web_fonts = Subscription::none();
        // Periodic autosave to a `.sv$` recovery file (native only). SAVETIME is
        // the interval in minutes; 0 disables it.
        #[cfg(not(target_arch = "wasm32"))]
        let autosave = if self.savetime_min > 0 {
            iced::time::every(std::time::Duration::from_secs(self.savetime_min as u64 * 60))
                .map(|_| Message::AutoSave)
        } else {
            Subscription::none()
        };
        #[cfg(target_arch = "wasm32")]
        let autosave = Subscription::none();
        // Drawings handed over by a second launch (single instance). Inert in a
        // process that lost the port election, so it costs nothing there.
        #[cfg(not(target_arch = "wasm32"))]
        let single_instance = crate::io::single_instance::subscribe().map(Message::OpenExternal);
        #[cfg(target_arch = "wasm32")]
        let single_instance = Subscription::none();
        iced::Subscription::batch([
            frames,
            history_tick,
            grip_dwell,
            hover_dwell,
            nav_settle,
            caret_blink,
            web_fonts,
            autosave,
            single_instance,
            event::listen_with(|ev, status, win_id| {
                use iced::event::Status;
                match ev {
                    iced::Event::Window(window::Event::CloseRequested) => {
                        Some(Message::WindowCloseRequested(win_id))
                    }
                    iced::Event::Window(window::Event::Closed) => {
                        Some(Message::OsWindowClosed(win_id))
                    }
                    iced::Event::Window(window::Event::Resized(sz)) => {
                        Some(Message::WindowResized(sz.width as f32, sz.height as f32))
                    }
                    iced::Event::Window(window::Event::FileDropped(path)) => {
                        Some(Message::FileDropped(path))
                    }
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        // `command()` is Cmd on macOS, Ctrl elsewhere — the
                        // platform multi-select modifier for the layer list.
                        Some(Message::SetModifiers {
                            shift: m.shift(),
                            ctrl: m.command() || m.control(),
                        })
                    }
                    iced::Event::Keyboard(keyboard::Event::KeyPressed {
                        key,
                        physical_key,
                        modifiers,
                        text,
                        ..
                    }) => {
                        // Platform-aware accelerator modifier: Cmd on macOS,
                        // Ctrl on Windows/Linux. Using `command()` rather than
                        // `control()` makes Cmd+C/V/S/Z etc. work on Mac, per
                        // the platform's keyboard conventions.
                        let accel = modifiers.command();
                        let shift = modifiers.shift();
                        // Any key that produces a printable glyph types it,
                        // even when its logical key resolves to navigation
                        // (NumLock-on Numpad8 / Numpad2 arrive as
                        // ArrowUp / ArrowDown but still carry text "8" /
                        // "2"). Checked before the Arrow / history arms so
                        // those numpad digits aren't swallowed as history
                        // navigation. Whitespace / control text (Space,
                        // Enter, Tab) falls through to the named handlers.
                        // The numpad decimal key emits the OS-layout separator
                        // (a comma on German/European layouts), which the
                        // coordinate parser rejects. Force it to a decimal point
                        // from the physical key, independent of layout.
                        if !accel
                            && status == Status::Ignored
                            && matches!(
                                physical_key,
                                keyboard::key::Physical::Code(
                                    keyboard::key::Code::NumpadDecimal
                                        | keyboard::key::Code::NumpadComma
                                )
                            )
                        {
                            return Some(Message::CommandAppendChar(".".to_string()));
                        }
                        if !accel && status == Status::Ignored {
                            if let Some(t) = text.as_deref() {
                                if !t.is_empty()
                                    && t.chars().all(|c| !c.is_control() && !c.is_whitespace())
                                {
                                    return Some(Message::CommandAppendChar(t.to_string()));
                                }
                            }
                        }
                        match key {
                            // Space is a literal space inside the MText preview
                            // but finalises a command otherwise; the handler
                            // decides based on editor state.
                            keyboard::Key::Named(keyboard::key::Named::Space)
                                if status == Status::Ignored =>
                            {
                                Some(Message::CommandSpace)
                            }
                            keyboard::Key::Named(keyboard::key::Named::Enter)
                                if status == Status::Ignored =>
                            {
                                Some(Message::CommandFinalize)
                            }
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                Some(Message::CommandEscape)
                            }
                            keyboard::Key::Named(keyboard::key::Named::Delete)
                                if status == Status::Ignored =>
                            {
                                Some(Message::DeleteSelected)
                            }
                            keyboard::Key::Named(keyboard::key::Named::Backspace)
                                if status == Status::Ignored =>
                            {
                                Some(Message::CommandBackspace)
                            }
                            keyboard::Key::Named(keyboard::key::Named::Tab)
                                if status == Status::Ignored =>
                            {
                                Some(Message::DynTabNext)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp)
                                if status == Status::Ignored =>
                            {
                                Some(Message::CommandHistoryPrev)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown)
                                if status == Status::Ignored =>
                            {
                                Some(Message::CommandHistoryNext)
                            }
                            // Caret movement in the MText preview (no-op
                            // otherwise; these arrows are unused elsewhere).
                            keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
                                if status == Status::Ignored =>
                            {
                                Some(Message::MTextCaretMove(-1))
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowRight)
                                if status == Status::Ignored =>
                            {
                                Some(Message::MTextCaretMove(1))
                            }
                            keyboard::Key::Named(keyboard::key::Named::F3) => {
                                Some(Message::ToggleSnapEnabled)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F7) => {
                                Some(Message::ToggleGrid)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F8) => {
                                Some(Message::ToggleOrtho)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F9) => {
                                Some(Message::ToggleGridSnap)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F10) => {
                                Some(Message::TogglePolar)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F11) => {
                                Some(Message::ToggleOTrack)
                            }
                            keyboard::Key::Named(keyboard::key::Named::F12) => {
                                Some(Message::ToggleDynInput)
                            }
                            keyboard::Key::Character(c) if accel => match c.as_str() {
                                "n" => Some(Message::TabNew),
                                "o" => Some(Message::OpenFile),
                                "s" if !shift => Some(Message::SaveFile),
                                "s" if shift => Some(Message::SaveAs),
                                "z" if !shift => Some(Message::Undo),
                                "z" if shift => Some(Message::Redo),
                                "y" => Some(Message::Redo),
                                // Ctrl/Cmd+A: select all layer rows when the
                                // Layer Manager is open, else all objects. The
                                // update handler branches on the active modal.
                                "a" if status == Status::Ignored => {
                                    Some(Message::SelectAllShortcut)
                                }
                                // Clipboard accelerators defer to a focused
                                // text widget: when the command-line history
                                // editor (or any text field) captures Ctrl+C/
                                // X/V it copies/cuts/pastes its own text and
                                // marks the event Captured, so we must NOT also
                                // fire the drawing's COPYCLIP/CUTCLIP/paste.
                                // Only when nothing captured (the drawing has
                                // focus, status Ignored) do these run. (#232)
                                "c" if status == Status::Ignored => {
                                    Some(Message::Command("COPYCLIP".to_string()))
                                }
                                "x" if status == Status::Ignored => {
                                    Some(Message::Command("CUTCLIP".to_string()))
                                }
                                "v" if status == Status::Ignored => Some(Message::PasteShortcut),
                                // Web: a focused text field captured Ctrl+C/V,
                                // but iced's clipboard is a no-op there — route
                                // through the async browser clipboard (#346).
                                #[cfg(target_arch = "wasm32")]
                                "v" => Some(Message::WebFieldPaste),
                                #[cfg(target_arch = "wasm32")]
                                "c" => Some(Message::WebFieldCopy),
                                _ => None,
                            },
                            // Printable glyphs are already handled by the
                            // text guard above the match; anything reaching
                            // here is a non-typing key we don't bind.
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }),
        ])
    }

    pub(super) fn focus_cmd_input(&self) -> Task<Message> {
        iced::widget::operation::focus(iced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID))
    }
}

// ── Document tab bar ───────────────────────────────────────────────────────

/// Right-click actions for a drawing tab. `ContextMenu` owns opening,
/// cursor-relative placement, boundary clamping, and dismissal.
fn doc_tab_context_menu(
    tab_idx: usize,
    has_current_path: bool,
    has_other_drawings: bool,
) -> Element<'static, Message> {
    const MENU_W: f32 = 210.0;

    let item = |label: &'static str, msg: Option<Message>| {
        let mut item = button(text(label).size(12))
        .style(button::subtle)
        .padding([4, 12])
        .width(Fill);
        if let Some(msg) = msg {
            item = item.on_press(msg);
        }
        item
    };

    let native_path_actions = cfg!(not(target_arch = "wasm32")) && has_current_path;
    container(
        column![
            item("Save All", Some(Message::DocTabSaveAll)),
            item("Close All", Some(Message::DocTabCloseAll)),
            item(
                "Close All Other Drawings",
                has_other_drawings.then_some(Message::DocTabCloseOthers(tab_idx)),
            ),
            item(
                "Copy Full File Path",
                native_path_actions.then_some(Message::DocTabCopyFullPath(tab_idx)),
            ),
            item(
                "Open File Location",
                native_path_actions.then_some(Message::DocTabOpenFileLocation(tab_idx)),
            ),
        ]
        .spacing(0)
        .width(MENU_W),
    )
    .style(container::bordered_box)
    .padding([4, 0])
    .width(iced::Length::Fixed(MENU_W))
    .into()
}

pub(super) fn doc_tab_bar<'a>(tabs: &'a [DocumentTab], active_tab: usize) -> Element<'a, Message> {
    // Document tabs live in a flex-wrap flow so they spill onto lower rows when
    // there are more tabs than the width can hold on one line.
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    let drag_targets: std::sync::Arc<[usize]> = tabs
        .iter()
        .enumerate()
        .filter_map(|(idx, tab)| (!tab.is_start).then_some(idx))
        .collect::<Vec<_>>()
        .into();

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = idx == active_tab;
        let name = crate::ui::text_util::elide(&tab.tab_display_name(), 24);
        let title_inner: Element<'_, Message> = if tab.dirty {
            row![
                crate::ui::icons::themed_warning(crate::ui::icons::DIRTY_DOT, 14.0),
                text(name).size(12),
            ]
            .spacing(5)
            .align_y(iced::Center)
            .into()
        } else {
            text(name).size(12).into()
        };

        let title_btn = button(title_inner)
            .on_press(Message::TabSwitch(idx))
            .height(Fill)
            .padding([4, 12])
            .style(move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                let background = match (is_active, status) {
                    (false, button::Status::Hovered) => {
                        Some(Background::Color(palette.background.weak.color))
                    }
                    _ => None,
                };
                button::Style {
                    background,
                    text_color: if is_active {
                        palette.primary.weak.text
                    } else {
                        palette.background.base.text.scale_alpha(0.72)
                    },
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: false,
                }
            });
        let title_btn: Element<'_, Message> = if tab.is_start {
            title_btn.into()
        } else {
            crate::ui::wrap_bar::ReorderTab::document(
                idx,
                drag_targets.clone(),
                title_btn,
            )
            .into()
        };

        // Start tab is fixed — no close button. Every other tab gets a close.
        let row_inner: Row<'_, Message> = if tab.is_start {
            row![title_btn].spacing(0).align_y(iced::Center)
        } else {
            let close_btn = button(text("×").size(12))
                .on_press(Message::TabClose(idx))
                .height(Fill)
                .padding([4, 8])
                .style(button::subtle);
            row![title_btn, close_btn]
                .spacing(0)
                .height(Fill)
                .align_y(iced::Center)
        };

        let tab_container = container(row_inner)
            .height(iced::Length::Fixed(23.0))
            .style(move |theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(if is_active {
                        palette.primary.weak.color
                    } else {
                        palette.background.base.color
                    })),
                    border: Border {
                        color: if is_active {
                            palette.primary.base.color
                        } else {
                            Color::TRANSPARENT
                        },
                        width: if is_active { 1.0 } else { 0.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            });

        let tab_element: Element<'_, Message> = if tab.is_start {
            tab_container.into()
        } else {
            let has_current_path = tab.current_path.is_some();
            let has_other_drawings = drag_targets.len() > 1;
            let tab_target: Element<'_, Message> = if let Some(path) = &tab.current_path {
                iced::widget::tooltip(
                    tab_container,
                    container(text(path.to_string_lossy().into_owned()).size(11))
                        .style(container::bordered_box)
                        .padding([4, 8]),
                    iced::widget::tooltip::Position::Bottom,
                )
                .gap(4)
                .into()
            } else {
                tab_container.into()
            };
            crate::ui::wrap_bar::PosReport::owned(
                format!("DOC_TAB:{idx}"),
                ContextMenu::new(tab_target, move || {
                    doc_tab_context_menu(idx, has_current_path, has_other_drawings)
                }),
            )
            .into()
        };
        items.push(tab_element);
    }

    let new_btn = button(text("+").size(14))
        .on_press(Message::TabNew)
        .padding([4, 10])
        .style(button::subtle);

    items.push(new_btn.into());

    container(WrapFlow::new(items).spacing_x(0.0).row_h(30.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color,
            )),
            border: Border {
                color: theme.extended_palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .width(Fill)
        .padding([0, 2])
        .into()
}

// ── Layout context-menu overlay ────────────────────────────────────────────

// ── Canvas-relative overlay positioning ────────────────────────────────────

/// Wraps `panel` in a column+row of `Space` widgets so it sits at
/// canvas-relative coordinates `(anchor.x, anchor.y)`. `panel` is wrapped
/// in `iced::widget::opaque` so mouse events on the panel itself do not
/// fall through to the viewport mouse area underneath; outside-click
/// dismissal is the caller's responsibility (handled via `ViewportLeftPress`
/// in `update.rs`, identical to how the multi-functional grip popup is
/// dismissed). Pushed into `viewport_stack` so the anchor is interpreted
/// in canvas-relative space, not window-relative.
// ── Start / Welcome page ──────────────────────────────────────────────────
//
// Renders in place of the model-space viewport when the active tab is the
// fixed Start tab (`DocumentTab::is_start`). English-only by design — this
// is the public welcome screen and stays consistent across locales.
//
fn start_muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn start_primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

/// Transparent input layer for one Model pane: a `mouse_area` filling the pane
/// that emits pane-tagged viewport events (`idx` = the pane's tile index). The
/// handlers offset the pane-local point to canvas coords and focus the pane.
fn pane_mouse_area<'a>(idx: usize) -> Element<'a, Message> {
    mouse_area(container(Space::new().width(Fill).height(Fill)))
        .on_move(move |p| Message::PaneMove(idx, p))
        .on_press(Message::PanePress(idx))
        .on_release(Message::PaneRelease(idx))
        .on_right_press(Message::PaneRightPress(idx))
        .on_right_release(Message::PaneRightRelease(idx))
        .on_middle_press(Message::PaneMiddlePress(idx))
        .on_middle_release(Message::PaneMiddleRelease(idx))
        .on_scroll(move |d| Message::PaneScroll(idx, d))
        .on_exit(Message::ViewportExit)
        .into()
}

/// Canvas that draws a label rotated 90° (for a collapsed panel's bar).
struct VBarLabel {
    text: String,
}

impl canvas::Program<Message> for VBarLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.with_save(|frame| {
            frame.translate(iced::Vector::new(bounds.width / 2.0, bounds.height / 2.0));
            frame.rotate(iced::Radians(std::f32::consts::FRAC_PI_2));
            frame.fill_text(canvas::Text {
                content: self.text.clone(),
                position: iced::Point::ORIGIN,
                color: theme.extended_palette().background.base.text.scale_alpha(0.72),
                size: iced::Pixels(13.0),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                shaping: iced::advanced::text::Shaping::Advanced,
                ..Default::default()
            });
        });
        vec![frame.into_geometry()]
    }
}

/// A collapsed panel rendered as a tall narrow bar with its name written along
/// it, rotated 90°. Pressing it emits `on_press`.
pub(super) fn collapse_bar<'a>(name: &str, on_press: Message) -> Element<'a, Message> {
    let label = canvas(VBarLabel {
        text: name.to_string(),
    })
    .width(Fill)
    .height(Fill);

    mouse_area(
        container(label)
            .width(iced::Length::Fixed(26.0))
            .height(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.base.color,
                )),
                border: Border {
                    color: theme.extended_palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
    )
    .interaction(iced::mouse::Interaction::Pointer)
    .on_press(on_press)
    .into()
}

pub(super) fn start_page_view<'a>(
    patrons: &'a [(String, i64)],
    videos: &'a [crate::videos::VideoEntry],
    videos_loading: bool,
    video_thumbs: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    discussions: &'a [crate::discussions::DiscussionEntry],
    discussions_loading: bool,
    recents: &'a [std::path::PathBuf],
    thumbs: &'a std::collections::HashMap<
        std::path::PathBuf,
        Option<iced::widget::image::Handle>,
    >,
    recent_limit: usize,
    recent_limit_input: &'a str,
    avail_w: f32,
    action_width_out: std::sync::Arc<std::sync::atomic::AtomicU32>,
    active: super::StartSection,
) -> Element<'a, Message> {
    let headline = text("Open CAD Studio").size(40).style(start_primary_style);

    // Plain outlined button (Open / New / Help / Contribute).
    let outline_btn = |label: &'static str, msg: Message| {
        button(text(label).size(14))
            .on_press(msg)
            .padding([10, 22])
            .style(move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                let pair = match status {
                    button::Status::Hovered => palette.background.strong,
                    _ => palette.background.weak,
                };
                button::Style {
                background: Some(Background::Color(pair.color)),
                text_color: pair.text,
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
                }
            })
    };

    // Donate — the prominent call-to-action, using the theme's danger role.
    let donate_btn = {
        button(
            row![
                crate::ui::icons::themed_danger_text(crate::ui::icons::HEART, 14.0),
                text("Donate").size(14),
            ]
            .spacing(5)
            .align_y(iced::Center),
        )
        .on_press(Message::RibbonToolClick {
            tool_id: "DONATE".to_string(),
            event: crate::modules::ModuleEvent::Command("DONATE".to_string()),
        })
        .padding([12, 28])
        .style(button::danger)
    };

    let primary_row = WrapFlow::new(vec![
        outline_btn("New Drawing", Message::TabNew).into(),
        outline_btn("Open File…", Message::OpenFile).into(),
        donate_btn.into(),
    ])
    .spacing_x(12.0)
    .row_h(48.0)
    .report_natural_width(action_width_out.clone());

    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut secondary_items: Vec<Element<'a, Message>> = vec![
        outline_btn(
            "Send Feedback",
            Message::RibbonToolClick {
                tool_id: "REPORT".to_string(),
                event: crate::modules::ModuleEvent::Command("REPORT".to_string()),
            },
        )
        .into(),
        outline_btn("Options", Message::OptionsOpen).into(),
        outline_btn("Plugins", Message::PluginManagerOpen).into(),
    ];
    // The web build is already in the browser, so only the desktop offers a
    // link to the web version.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Filled with the active theme's primary colour.
        secondary_items.push(
            button(text("OCS Web").size(14))
                .on_press(Message::RibbonToolClick {
                    tool_id: "WEBVERSION".to_string(),
                    event: crate::modules::ModuleEvent::Command("WEBVERSION".to_string()),
                })
                .padding([10, 22])
                .style(button::primary)
                .into(),
        );
    }
    let secondary_row = WrapFlow::new(secondary_items)
        .spacing_x(12.0)
        .row_h(44.0)
        .report_natural_width(action_width_out.clone());

    let content = column![
        Space::new().height(iced::Length::Fixed(28.0)),
        container(headline).center_x(Fill),
        Space::new().height(iced::Length::Fixed(22.0)),
        container(primary_row).center_x(Fill),
        Space::new().height(iced::Length::Fixed(10.0)),
        container(secondary_row).center_x(Fill),
        Space::new().height(Fill),
    ]
    .spacing(0)
    .width(Fill)
    .height(Fill);

    // Collapse side panels one at a time as width shrinks: Tutorials first,
    // then Discussions, Supporters, and Recent Documents last.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum StartLayout {
        AllPanels,
        WithoutVideos,
        WithoutVideosAndDiscussions,
        RecentAndWelcome,
        Compact,
    }
    let panel_w = 280.0f32;
    const VIDEO_PANEL_PADDING: f32 = 16.0;
    const VIDEO_SCROLL_GUTTER: f32 = 14.0;
    let measured_action_w = f32::from_bits(
        action_width_out.load(std::sync::atomic::Ordering::Relaxed)
    );
    let welcome_wide_min = measured_action_w.max(360.0);
    let avail = (avail_w - 16.0).max(0.0); // minus the page's l/r padding
    let panel_widths = [panel_w; 4];
    let mut panel_visible = [true, true, true, true];
    let required_width = |visible: &[bool; 4]| {
        let visible_panels = visible.iter().filter(|&&shown| shown).count();
        welcome_wide_min
            + panel_widths
                .iter()
                .zip(visible)
                .filter_map(|(width, shown)| shown.then_some(*width))
                .sum::<f32>()
            + visible_panels as f32 * 16.0
    };
    // Re-measure after every collapse. There are no independent breakpoints:
    // the available width and the panels' preferred widths decide the state.
    for panel in [1usize, 2, 3, 0] {
        if required_width(&panel_visible) <= avail {
            break;
        }
        panel_visible[panel] = false;
    }
    let start_layout = match panel_visible {
        [true, true, true, true] => StartLayout::AllPanels,
        [true, false, true, true] => StartLayout::WithoutVideos,
        [true, false, false, true] => StartLayout::WithoutVideosAndDiscussions,
        [true, false, false, false] => StartLayout::RecentAndWelcome,
        _ => StartLayout::Compact,
    };

    let recent = recent_files_panel(
        recents,
        thumbs,
        recent_limit,
        recent_limit_input,
        match start_layout {
            StartLayout::AllPanels
            | StartLayout::WithoutVideos
            | StartLayout::WithoutVideosAndDiscussions
            | StartLayout::RecentAndWelcome => iced::Length::Fixed(panel_w),
            StartLayout::Compact => iced::Length::Fill,
        },
    );
    let welcome = container(content).width(Fill).height(Fill);

    // Tutorial-videos rail: the official playlist, fetched at boot (cached on
    // disk) — thumbnail card + title per video, click opens the browser.
    let videos_panel: Element<'a, Message> = {
        // Derive the 16:9 cover box from the actual shared list width so the
        // whole thumbnail remains visible when that width changes.
        let thumb_h =
            (panel_w - VIDEO_PANEL_PADDING * 2.0 - VIDEO_SCROLL_GUTTER) * 9.0 / 16.0;
        let mut list = column![text("Tutorials").size(15)]
            .spacing(10)
            .width(Fill)
            // Keep the scrollbar off the thumbnails.
            .padding(iced::Padding {
                right: VIDEO_SCROLL_GUTTER,
                ..iced::Padding::ZERO
            });
        for v in videos {
            let mut card = column![].spacing(6).width(Fill);
            if let Some(handle) = video_thumbs.get(&v.id) {
                card = card.push(
                    container(
                        iced::widget::image(handle.clone())
                            .width(Fill)
                            .height(iced::Length::Fixed(thumb_h))
                            .content_fit(iced::ContentFit::Contain),
                    )
                    .width(Fill)
                    .height(iced::Length::Fixed(thumb_h))
                    .style(|theme: &Theme| container::Style {
                        border: Border {
                            color: theme.extended_palette().background.neutral.color,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    })
                    .clip(true),
                );
            }
            card = card.push(text(v.title.clone()).size(12).style(start_muted_style));
            list = list.push(
                mouse_area(card)
                    .interaction(iced::mouse::Interaction::Pointer)
                    .on_press(Message::OpenUrl(crate::videos::watch_url(&v.id))),
            );
        }
        if videos.is_empty() {
            let note = if videos_loading {
                "Loading videos…"
            } else {
                "Videos load from the internet."
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let playlist_btn = mouse_area(
            container(text("Open playlist on YouTube").size(12))
            .padding([6, 10])
            .width(Fill)
            .center_x(Fill)
            .style(|theme: &Theme| {
                let pair = theme.extended_palette().danger.base;
                container::Style {
                background: Some(Background::Color(pair.color)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 6.0.into(),
                },
                text_color: Some(pair.text),
                ..Default::default()
                }
            }),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::OpenUrl(crate::videos::PLAYLIST_URL.to_string()));
        container(column![
            iced::widget::scrollable(list).height(Fill),
            Space::new().height(iced::Length::Fixed(12.0)),
            playlist_btn,
        ])
        .width(match start_layout {
            StartLayout::AllPanels => iced::Length::Fixed(panel_w),
            StartLayout::WithoutVideos
            | StartLayout::WithoutVideosAndDiscussions
            | StartLayout::RecentAndWelcome
            | StartLayout::Compact => iced::Length::Fill,
        })
        .height(Fill)
        .padding(VIDEO_PANEL_PADDING)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
            }
        })
        .into()
    };

    // GitHub Discussions rail. Native builds refresh from GitHub's public feed;
    // web builds read the CI-generated snapshot. Both sources mark pinned
    // discussions and sort them before the rest of the list.
    let discussions_panel: Element<'a, Message> = {
        let mut list = column![text("Discussions").size(15)]
            .spacing(8)
            .width(Fill);
        for discussion in discussions {
            let mut meta = iced::widget::row![
                text(format!("#{}", discussion.number))
                    .size(10)
                    .style(start_muted_style),
            ]
            .spacing(6)
            .align_y(iced::Center);
            if discussion.pinned {
                meta = meta.push(
                    text("Pinned")
                        .size(10)
                        .style(start_primary_style),
                );
            }
            if !discussion.author.is_empty() {
                meta = meta.push(
                    text(format!("@{}", discussion.author))
                        .size(10)
                        .style(start_muted_style),
                );
            }
            let card = container(
                column![
                    text(discussion.title.clone()).size(12),
                    meta,
                ]
                .spacing(4),
            )
            .padding([8, 10])
            .width(Fill)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(
                        palette.background.base.color.scale_alpha(0.42),
                    )),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            });
            list = list.push(
                mouse_area(card)
                    .interaction(iced::mouse::Interaction::Pointer)
                    .on_press(Message::OpenUrl(discussion.url.clone())),
            );
        }
        if discussions.is_empty() {
            let note = if discussions_loading {
                "Loading discussions…"
            } else {
                "Discussions load from GitHub."
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let open_btn = mouse_area(
            container(text("Open Discussions on GitHub").size(12))
                .padding([6, 10])
                .width(Fill)
                .center_x(Fill)
                .style(|theme: &Theme| {
                    let pair = theme.extended_palette().primary.base;
                    container::Style {
                        background: Some(Background::Color(pair.color)),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 6.0.into(),
                        },
                        text_color: Some(pair.text),
                        ..Default::default()
                    }
                }),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::OpenUrl(
            crate::discussions::DISCUSSIONS_URL.to_string(),
        ));
        container(column![
            iced::widget::scrollable(list).height(Fill),
            Space::new().height(iced::Length::Fixed(12.0)),
            open_btn,
        ])
        .width(match start_layout {
            StartLayout::AllPanels | StartLayout::WithoutVideos => {
                iced::Length::Fixed(panel_w)
            }
            StartLayout::WithoutVideosAndDiscussions
            | StartLayout::RecentAndWelcome
            | StartLayout::Compact => iced::Length::Fill,
        })
        .height(Fill)
        .padding(16)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
    };

    // Right rail: Patreon supporters, fetched at boot. When the list is empty
    // (no token configured / offline) only the "Support on Patreon" button
    // shows, so the rail always invites support.
    let supporters: Element<'a, Message> = {
        let mut list = column![
            text("Supporters").size(15),
            Space::new().height(iced::Length::Fixed(12.0)),
        ]
        .spacing(6)
        .width(Fill);
        for (name, cents) in patrons {
            // Cents → the campaign currency's main unit. Symbol assumed "$";
            // adjust if the campaign bills in another currency.
            let amount = format!("${:.2}", *cents as f64 / 100.0);
            list = list.push(
                iced::widget::row![
                    text(name).size(12).style(start_muted_style).width(Fill),
                    text(amount).size(12).style(start_muted_style),
                ]
                .spacing(6),
            );
        }
        let support_btn = mouse_area(
            container(
                iced::widget::row![
                    crate::ui::icons::themed_danger_text(crate::ui::icons::HEART, 13.0),
                    text("Support on Patreon").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .padding([6, 10])
            .width(Fill)
            .center_x(Fill)
            .style(|theme: &Theme| {
                let pair = theme.extended_palette().danger.base;
                container::Style {
                background: Some(Background::Color(pair.color)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 6.0.into(),
                },
                text_color: Some(pair.text),
                ..Default::default()
                }
            }),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::OpenUrl(
            "https://patreon.com/HakanSeven12".to_string(),
        ));
        container(column![
            iced::widget::scrollable(list).height(Fill),
            Space::new().height(iced::Length::Fixed(12.0)),
            support_btn,
        ])
        .width(match start_layout {
            StartLayout::AllPanels
            | StartLayout::WithoutVideos
            | StartLayout::WithoutVideosAndDiscussions => {
                iced::Length::Fixed(panel_w)
            }
            StartLayout::RecentAndWelcome
            | StartLayout::Compact => iced::Length::Fill,
        })
        .height(Fill)
        .padding(20)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
            }
        })
        .into()
    };

    let body: Element<'a, Message> = match start_layout {
        StartLayout::AllPanels => iced::widget::row![
            recent,
            videos_panel,
            welcome,
            discussions_panel,
            supporters,
        ]
        .spacing(16)
        .height(Fill)
        .into(),
        StartLayout::WithoutVideos => {
            iced::widget::row![recent, welcome, discussions_panel, supporters]
                .spacing(16)
                .height(Fill)
                .into()
        }
        StartLayout::WithoutVideosAndDiscussions => {
            iced::widget::row![recent, welcome, supporters]
                .spacing(16)
                .height(Fill)
                .into()
        }
        StartLayout::RecentAndWelcome => iced::widget::row![recent, welcome]
            .spacing(16)
            .height(Fill)
            .into(),
        StartLayout::Compact => {
            let tab_btn = |label: &'static str, section: super::StartSection| {
                let is_active = active == section;
                button(text(label).size(14))
                    .on_press(Message::StartSectionSelect(section))
                    .padding([8, 18])
                    .style(move |theme: &Theme, status| {
                        let palette = theme.extended_palette();
                        let pair = match (is_active, status) {
                            (true, _) => Some(palette.primary.weak),
                            (false, button::Status::Hovered) => {
                                Some(palette.background.strong)
                            }
                            _ => None,
                        };
                        button::Style {
                        background: pair.map(|p| Background::Color(p.color)),
                        text_color: pair
                            .map(|p| p.text)
                            .unwrap_or(palette.background.base.text.scale_alpha(0.68)),
                        border: Border {
                            color: if is_active {
                                palette.primary.base.color
                            } else {
                                Color::TRANSPARENT
                            },
                            width: if is_active { 1.0 } else { 0.0 },
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                        }
                    })
            };
            let tab_bar = WrapFlow::new(vec![
                tab_btn("Recent Files", super::StartSection::Recent).into(),
                tab_btn("Videos", super::StartSection::Videos).into(),
                tab_btn("Welcome", super::StartSection::Welcome).into(),
                tab_btn("Discussions", super::StartSection::Discussions).into(),
                tab_btn("Supporters", super::StartSection::Supporters).into(),
            ])
            .spacing_x(6.0)
            .row_h(38.0);
            let section_body: Element<'a, Message> = match active {
                super::StartSection::Recent => container(recent)
                    .width(Fill)
                    .height(Fill)
                    .center_x(Fill)
                    .into(),
                super::StartSection::Videos => container(videos_panel)
                    .width(Fill)
                    .height(Fill)
                    .center_x(Fill)
                    .into(),
                super::StartSection::Welcome => welcome.into(),
                super::StartSection::Discussions => container(discussions_panel)
                    .width(Fill)
                    .height(Fill)
                    .center_x(Fill)
                    .into(),
                super::StartSection::Supporters => container(supporters)
                    .width(Fill)
                    .height(Fill)
                    .center_x(Fill)
                    .into(),
            };
            column![
                container(tab_bar).center_x(Fill),
                Space::new().height(iced::Length::Fixed(12.0)),
                section_body,
            ]
            .width(Fill)
            .height(Fill)
            .into()
        }
    };

    container(body)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color
        )),
        ..Default::default()
    })
    .padding(iced::Padding {
        top: 16.0,
        right: 8.0,
        bottom: 16.0,
        left: 8.0,
    })
    .width(Fill)
    .height(Fill)
    .into()
}

// ── Recent Documents panel (Start tab left rail) ──────────────────────────
//
// Slots into the same `row![properties_el, viewport_stack]` position the
// Properties panel normally occupies, but only when the active tab is the
// Start tab. The list is restored from disk at boot and re-saved on every
// open — entries persist across sessions.
pub(super) fn recent_files_panel<'a>(
    recents: &'a [std::path::PathBuf],
    thumbs: &'a std::collections::HashMap<
        std::path::PathBuf,
        Option<iced::widget::image::Handle>,
    >,
    limit: usize,
    limit_input: &'a str,
    width: iced::Length,
) -> Element<'a, Message> {
    // Title mirrors the Supporters rail: size 15 in the bright text colour,
    // followed by a 12px gap before the content.
    let title = text("Recent Documents").size(15);

    let body: Element<'a, Message> = if recents.is_empty() {
        container(
            text("Files you open will show up here.")
                .size(12)
                .style(start_muted_style)
        )
            .height(Fill)
            .into()
    } else {
        // Right padding reserves a gutter for the scrollbar so it doesn't sit on
        // top of the row's ✕ remove button.
        let mut col = column![].spacing(0).padding(iced::Padding {
            right: 12.0,
            ..iced::Padding::ZERO
        });
        for path in recents {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let dir = path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Leading DWG preview thumbnail (fixed box keeps rows aligned even
            // when a file has no readable preview).
            let thumb: Element<'a, Message> = match thumbs.get(path).and_then(|o| o.as_ref()) {
                Some(h) => container(
                    iced::widget::image(h.clone())
                        .content_fit(iced::ContentFit::Contain)
                        .width(Fill)
                        .height(Fill),
                )
                .width(46)
                .height(34)
                .into(),
                None => iced::widget::Space::new().width(46).height(34).into(),
            };

            let path_for_open = path.clone();
            let open_btn = button(
                row![
                    thumb,
                    column![
                        text(crate::ui::text_util::elide(&name, 28))
                            .size(12),
                        text(crate::ui::text_util::elide(&dir, 38))
                            .size(10)
                            .style(start_muted_style),
                    ]
                    .spacing(2),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .on_press(Message::OpenRecent(path_for_open))
            .padding([6, 12])
            .width(Fill)
            .style(move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                button::Style {
                background: matches!(status, button::Status::Hovered).then_some(
                    Background::Color(palette.background.strong.color)
                ),
                text_color: palette.background.base.text,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            });

            let path_for_remove = path.clone();
            let remove_btn = button(crate::ui::icons::themed_secondary(
                crate::ui::icons::CLOSE,
                11.0,
            ))
                .on_press(Message::RecentRemove(path_for_remove))
                .padding([4, 8])
                .style(|theme: &Theme, status| {
                    let palette = theme.extended_palette();
                    button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then_some(Background::Color(palette.danger.weak.color)),
                    text_color: palette.background.base.text.scale_alpha(0.68),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                    }
                });

            col = col.push(row![open_btn, remove_btn].spacing(0).align_y(iced::Center));
        }
        iced::widget::scrollable(col).height(Fill).into()
    };

    // Footer: how many recent files to keep — [-] [editable count] [+] with the
    // max shown. The count box takes keyboard input (applied on Enter); the
    // update handler clamps to [MIN, MAX] and persists (see `set_recent_limit`),
    // so an over-max entry snaps to the max.
    const STEP: usize = 5;
    let step_style = |theme: &Theme, status: button::Status| {
        let palette = theme.extended_palette();
        button::Style {
        background: matches!(status, button::Status::Hovered).then_some(
            Background::Color(palette.background.strong.color)
        ),
        text_color: palette.background.base.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
        }
    };
    // +/- step from whatever is currently shown in the box (mid-edit included).
    let shown = limit_input.parse::<usize>().unwrap_or(limit);
    let count_box = iced::widget::text_input("", limit_input)
        .on_input(Message::RecentLimitInput)
        .on_submit(Message::SetRecentLimit(shown))
        .size(13)
        .padding([2, 6])
        .width(iced::Length::Fixed(46.0));
    let limit_row = row![
        text("Keep recent files").size(11).style(start_muted_style).width(Fill),
        button(crate::ui::icons::themed(crate::ui::icons::MINUS, 11.0))
            .on_press(Message::SetRecentLimit(shown.saturating_sub(STEP)))
            .padding([3, 6])
            .style(step_style),
        count_box,
        button(crate::ui::icons::themed(crate::ui::icons::PLUS, 11.0))
            .on_press(Message::SetRecentLimit(shown + STEP))
            .padding([3, 6])
            .style(step_style),
        text(format!("/ {}", super::recent::RECENT_MAX))
            .size(11)
            .style(start_muted_style),
    ]
    .spacing(6)
    .align_y(iced::Center);

    container(
        column![
            title,
            Space::new().height(iced::Length::Fixed(12.0)),
            body,
            Space::new().height(iced::Length::Fixed(12.0)),
            limit_row,
        ]
        .width(Fill),
    )
    .width(width)
    .height(Fill)
    .padding(20)
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();
        container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
        }
    })
    .into()
}
