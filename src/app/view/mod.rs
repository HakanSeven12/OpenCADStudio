use super::document::DocumentTab;
use super::document::DynComponent;
use super::helpers::grid_plane_from_camera;
use super::history::history_dropdown_labels;
use super::{Message, OpenCADStudio};
use crate::scene::pick::grip::{grips_to_screen, grips_to_screen_paper, grips_to_screen_rte};
use crate::scene::view::viewport_pane::ViewportPane;
use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
use iced::widget::{
    button, column, container, mouse_area, pane_grid, responsive, row, shader, stack, text, Row,
    Space,
};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Subscription, Task, Theme};

mod controls;
mod modal;
mod overlay;
mod viewcube;

use controls::{dyn_component_value, viewport_controls};
use overlay::{
    layout_context_menu_overlay, mtext_editor_overlay, position_canvas_overlay, qselect_overlay,
    text_inline_overlay, viewport_context_menu_overlay,
};
use viewcube::{viewcube_nav_controls, viewcube_ucs_picker, UCS_PICKER_W};

// Re-export the text-input element ids so sibling modules can address them at
// the `view::` path as before the split.
pub(in crate::app) use overlay::{MTEXT_TEXT_ID, TEXT_INLINE_ID};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_REGION_PX;

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
        let is_paper = tab.scene.current_layout != "Model";
        // Start tab: render welcome page in place of the viewport.
        // Surrounding chrome (tab bar, status bar) stays; the welcome widget
        // returned here also flags the rest of `view` to skip drawing-only
        // overlays via `tab.is_start`.
        // Unified GPU widget for both layouts. A paper layout renders through
        // the same shader as model space: a full-canvas top-locked "sheet"
        // viewport draws the layout's own geometry (white sheet + entities +
        // borders) and the floating content viewports blit on top.
        let viewport_3d: Element<'_, Message> = if tab.is_start {
            start_page_view()
        } else if is_paper {
            shader(ViewportPane::model(
                &tab.scene,
                self.show_viewcube,
                tab.render_mode,
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
            let show_viewcube = self.show_viewcube;
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
                (o.as_dvec3(), (ux, uy, uz))
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
                                (o.as_dvec3(), (ux, uy, uz))
                            }
                            None => (
                                glam::DVec3::ZERO,
                                (glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z),
                            ),
                        }
                    } else {
                        model_basis
                    };
                    let plane = grid_plane_from_camera(cam.pitch, cam.yaw);
                    crate::ui::overlay::GridParams {
                        view_rot: cam.view_proj_rte(bounds),
                        eye: cam.eye(),
                        bounds,
                        plane,
                        origin,
                        axes,
                    }
                })
                .collect();
            crate::ui::overlay::grid_overlay(grid)
        };

        let selection_overlay = {
            let sel = tab.scene.selection.borrow().clone();
            let snap_info = tab.snap_result.map(|s| (s.screen, s.snap_type));

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
                                .map_or(false, |g| Some(g.handle) == sel_h && g.grip_id == grip_id);
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
                    axes: (ux, uy, uz),
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
                            axes: (ux, uy, uz),
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
            // view-projection cancels catastrophically in f32).
            let ost_points: Vec<crate::ui::overlay::OstTrackPoint> = if self.snapper.otrack_enabled {
                let (view_rot, eye) = {
                    let cam = tab.scene.camera.borrow();
                    (cam.view_proj_rte(vp_bounds), cam.eye())
                };
                self.snapper
                    .tracking_points
                    .iter()
                    .map(|&wp| {
                        let ndc = view_rot.project_point3((wp.as_dvec3() - eye).as_vec3());
                        crate::ui::overlay::OstTrackPoint {
                            screen: iced::Point::new(
                                (ndc.x + 1.0) * 0.5 * vp_bounds.width,
                                (1.0 - ndc.y) * 0.5 * vp_bounds.height,
                            ),
                        }
                    })
                    .collect()
            } else {
                vec![]
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

            crate::ui::overlay::selection_overlay(
                sel,
                snap_info,
                grips,
                ucs_icons,
                ost_points,
                tab.last_cursor_screen,
                !is_paper && self.show_viewcube,
                dividers,
                pane_move_rect,
                pane_drop_rect,
                tab.pan_mode,
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
                let live = tab.active_cmd.as_ref().and_then(|c| c.dyn_live_value(w.as_dvec3()));
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
                            _ => dyn_component_value(f, w, base, &tab.ucs_xform()),
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
            // Unified control chip: split buttons + render-mode picker +
            // grid / grid-snap toggles, for the active Model tile.
            let bar = viewport_controls(
                tab.render_mode,
                self.show_grid,
                self.snapper.grid_snap(),
                true,
                tab.scene.model_tiles.borrow().len(),
            );
            // Position the bar at the active model tile's top-left corner so
            // it follows the active panel in a tiled layout (full canvas when
            // a single tile fills the window). Leading Spaces offset it.
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let rect = tab.scene.active_model_tile_bounds(vw, vh);
            let bar_layer = column![
                Space::new().height(iced::Length::Fixed(rect.y.max(0.0))),
                row![
                    Space::new().width(iced::Length::Fixed(rect.x.max(0.0))),
                    iced::widget::opaque(bar),
                ],
            ]
            .width(Fill)
            .height(Fill);
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
            let border_layer = column![
                Space::new().height(iced::Length::Fixed(y)),
                row![Space::new().width(iced::Length::Fixed(x)), border_frame,],
            ]
            .width(Fill)
            .height(Fill);
            viewport_stack = viewport_stack.push(border_layer);

            let vp_mode = tab
                .scene
                .active_viewport_render_mode()
                .unwrap_or(acadrust::entities::ViewportRenderMode::Wireframe2D);
            let picker_layer = column![
                Space::new().height(iced::Length::Fixed(y + 4.0)),
                row![
                    Space::new().width(iced::Length::Fixed(x + 4.0)),
                    iced::widget::opaque(viewport_controls(
                        vp_mode,
                        self.show_grid,
                        self.snapper.grid_snap(),
                        false,
                        0,
                    )),
                ],
            ]
            .width(Fill)
            .height(Fill);
            viewport_stack = viewport_stack.push(picker_layer);

            if self.show_viewcube {
                let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
                let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);

                let controls = column![
                    Space::new().height(iced::Length::Fixed(cube_y)),
                    row![
                        Space::new().width(iced::Length::Fixed(cube_x)),
                        viewcube_nav_controls(),
                    ],
                ]
                .width(Fill)
                .height(Fill);
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
                let picker = column![
                    Space::new().height(iced::Length::Fixed(cube_y + VIEWCUBE_HIT_SIZE + 6.0)),
                    row![
                        Space::new()
                            .width(iced::Length::Fixed(cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5)),
                        iced::widget::opaque(viewcube_ucs_picker(ucs_current, ucs_names)),
                    ],
                ]
                .width(Fill)
                .height(Fill);
                viewport_stack = viewport_stack.push(picker);
            }
        }

        if self.show_viewcube && !is_paper && !tab.is_start {
            // Place the ViewCube hit area in the active model tile's top-right
            // corner so it tracks the active panel in a tiled layout. The hit
            // test in update.rs already maps clicks through the active tile.
            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let rect = tab.scene.active_model_tile_bounds(vw, vh);
            let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
            let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);

            // Cube hit area + nav controls (home / roll / nudge) as one layer.
            let controls = column![
                Space::new().height(iced::Length::Fixed(cube_y)),
                row![
                    Space::new().width(iced::Length::Fixed(cube_x)),
                    viewcube_nav_controls(),
                ],
            ]
            .width(Fill)
            .height(Fill);
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
            let picker = column![
                Space::new().height(iced::Length::Fixed(cube_y + VIEWCUBE_HIT_SIZE + 6.0)),
                row![
                    Space::new().width(iced::Length::Fixed(cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5)),
                    iced::widget::opaque(viewcube_ucs_picker(ucs_current, ucs_names)),
                ],
            ]
            .width(Fill)
            .height(Fill);
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
                    let btn = button(text(label).size(12).color(Color::WHITE))
                        .on_press(Message::GripMenuPick(idx))
                        .padding([3, 10])
                        .width(Fill)
                        .style(move |_: &Theme, status| iced::widget::button::Style {
                            background: Some(Background::Color(match (is_sel, status) {
                                (true, _) => Color {
                                    r: 0.20,
                                    g: 0.45,
                                    b: 0.95,
                                    a: 1.0,
                                },
                                (_, iced::widget::button::Status::Hovered) => Color {
                                    r: 0.22,
                                    g: 0.22,
                                    b: 0.22,
                                    a: 1.0,
                                },
                                _ => Color::TRANSPARENT,
                            })),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 0.0.into(),
                            },
                            text_color: Color::WHITE,
                            ..Default::default()
                        });
                    col = col.push(btn);
                }
                let menu_panel = container(col)
                    .padding(2)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(Color {
                            r: 0.10,
                            g: 0.10,
                            b: 0.10,
                            a: 0.95,
                        })),
                        border: Border {
                            color: Color {
                                r: 0.40,
                                g: 0.40,
                                b: 0.40,
                                a: 1.0,
                            },
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
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
                        crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, Color::WHITE)
                    } else {
                        Space::new().width(11).into()
                    };
                    let btn = button(
                        row![
                            container(mark).width(16),
                            text(name).size(12).color(Color::WHITE),
                        ]
                        .spacing(2)
                        .align_y(iced::Center),
                    )
                    .on_press(Message::VisibilityPick(idx))
                        .padding([3, 10])
                        .width(Fill)
                        .style(move |_: &Theme, status| iced::widget::button::Style {
                            background: Some(Background::Color(match status {
                                iced::widget::button::Status::Hovered => Color {
                                    r: 0.20,
                                    g: 0.45,
                                    b: 0.95,
                                    a: 1.0,
                                },
                                _ => Color::TRANSPARENT,
                            })),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 0.0.into(),
                            },
                            text_color: Color::WHITE,
                            ..Default::default()
                        });
                    col = col.push(btn);
                }
                let panel = container(iced::widget::scrollable(col).height(iced::Length::Shrink))
                    .max_height(360.0)
                    .padding(2)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(Color {
                            r: 0.10,
                            g: 0.10,
                            b: 0.10,
                            a: 0.95,
                        })),
                        border: Border {
                            color: Color {
                                r: 0.40,
                                g: 0.40,
                                b: 0.40,
                                a: 1.0,
                            },
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
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

        // Quick Properties: compact floating property panel on selection,
        // anchored at the canvas top-left so it doesn't track the cursor.
        if self.quick_properties && !tab.is_start {
            if let Some(panel) = tab.properties.quick_view() {
                viewport_stack = viewport_stack
                    .push(position_canvas_overlay(iced::Point::new(12.0, 12.0), panel));
            }
        }

        // Frame-budget HUD (Phase 5.3): toggle with the PERF command. Shows
        // the cost of the most recent wire re-tessellation — the work avoided
        // by a warm wire cache — so render-path changes can be compared
        // PR-to-PR. Reads ~0 ms while panning/zooming on a hit cache.
        if self.perf_hud && !tab.is_start {
            let s = &tab.scene;
            let label = format!(
                "tess {:.1} ms · {} wires · epoch {}",
                s.last_tess_ms.get(),
                s.last_tess_wires.get(),
                s.geometry_epoch,
            );
            let panel = container(text(label).size(12).color(Color {
                r: 0.6,
                g: 1.0,
                b: 0.6,
                a: 1.0,
            }))
            .padding(6)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    r: 0.08,
                    g: 0.08,
                    b: 0.08,
                    a: 0.85,
                })),
                border: Border {
                    color: Color {
                        r: 0.35,
                        g: 0.35,
                        b: 0.35,
                        a: 1.0,
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
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
                viewport_stack = viewport_stack.push(mtext_editor_overlay(ed, styles, canvas));
            }
            if let Some(ed) = &self.text_inline {
                viewport_stack = viewport_stack.push(text_inline_overlay(ed, canvas));
            }
        }

        // Properties / layers panels carry no useful state on the Start tab.
        // Replace the properties panel with a Recent Documents list there.
        let properties_el: Element<'_, Message> = if tab.is_start {
            recent_files_panel(&self.app_menu.recent)
        } else if self.show_properties && !self.clean_screen {
            tab.properties.view()
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
            iced::widget::container(self.command_line.view(allow_autocomplete, dyn_capturing))
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
                    let cursor_coord = {
                        let lc = tab.last_cursor_world;
                        // The readout follows the active pane's UCS — model space
                        // or inside a floating viewport (no-op without a UCS).
                        if tab.editing_model_space() {
                            tab.ucs_xform().to_ucs(lc)
                        } else {
                            lc
                        }
                    };
                    self.status_bar.view(
                        &self.snapper,
                        self.snap_popup_open,
                        self.ortho_mode,
                        self.polar_mode,
                        self.polar_increment_deg,
                        self.dyn_input,
                        self.snapper.otrack_enabled,
                        tab.scene.layout_names(),
                        tab.scene.current_layout.clone(),
                        self.layout_rename_state.as_ref(),
                        tab.scene.first_viewport_scale(),
                        tab.scene.viewport_count(),
                        tab.scene.active_viewport.is_some(),
                        self.show_layout_tabs,
                        tab.scene.annotation_scale,
                        self.scale_popup_open,
                        scale_pill_enabled,
                        tab.scene.document.header.lineweight_display,
                        cursor_coord,
                        self.clean_screen,
                        tab.scene.document.header.insertion_units,
                        self.units_popup_open,
                        tab.scene.is_isolation_active(),
                        tab.scene.transparency_display,
                        self.quick_properties,
                        tab.scene.selection_filter_active(),
                        self.selection_cycling,
                        &self.statusbar_config,
                    )
                })
                .width(Fill)
                .height(Fill)
        })
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color {
                r: 0.11,
                g: 0.11,
                b: 0.11,
                a: 1.0,
            })),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill);

        let snap_layer: Element<'_, Message> = if self.snap_popup_open {
            crate::ui::popup::snap_popup::snap_popup_overlay(&self.snapper, 4.0)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let scale_layer: Element<'_, Message> = if self.scale_popup_open {
            let is_model = tab.scene.current_layout == "Model";
            crate::ui::popup::scale_popup::scale_popup_overlay(
                is_model,
                tab.scene.annotation_scale,
                tab.scene.first_viewport_scale(),
                tab.scene.scale_list(),
            )
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let statusbar_menu_layer: Element<'_, Message> = if self.statusbar_menu_open {
            crate::ui::statusbar::statusbar_menu::statusbar_menu_overlay(&self.statusbar_config)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let layout_list_layer: Element<'_, Message> = if self.layout_list_open && !tab.is_start {
            crate::ui::statusbar::statusbar_menu::layout_list_overlay(
                &tab.scene.layout_names(),
                &tab.scene.current_layout,
            )
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let units_layer: Element<'_, Message> = if self.units_popup_open {
            crate::ui::popup::units_popup::units_popup_overlay(tab.scene.document.header.insertion_units)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let isolate_layer: Element<'_, Message> = if self.isolate_popup_open {
            crate::ui::popup::isolate_popup::isolate_popup_overlay(
                !tab.scene.selected.is_empty(),
                tab.scene.is_isolation_active(),
            )
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let sel_filter_layer: Element<'_, Message> = if self.selection_filter_popup_open {
            let types: Vec<String> = tab
                .scene
                .entity_type_names_in_layout()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            crate::ui::popup::selection_filter_popup::selection_filter_popup_overlay(
                types,
                &tab.scene.selection_filter,
            )
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let dropdown_layer: Element<'_, Message> = self
            .ribbon
            .dropdown_overlay(
                &history_dropdown_labels(&self.tabs[self.active_tab].history.undo_stack),
                &history_dropdown_labels(&self.tabs[self.active_tab].history.redo_stack),
            )
            .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into());

        let layout_ctx_layer: Element<'_, Message> = if let Some(name) = &self.layout_context_menu {
            layout_context_menu_overlay(name)
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
            self.app_menu.view(),
            snap_layer,
            scale_layer,
            statusbar_menu_layer,
            layout_list_layer,
            units_layer,
            isolate_layer,
            sel_filter_layer,
            dropdown_layer,
            layout_ctx_layer,
            qselect_layer,
            open_progress_layer,
        ];

        // ── In-canvas modal dialogs (Plan B) ───────────────────────────────
        // Former pop-up windows render as overlays here, so they work on both
        // the native (single main window) and web builds.
        let base: Element<'_, Message> = match self.modal_content() {
            Some(content) => {
                crate::ui::modal::modal(composed, content, Message::CloseModal, self.modal_offset)
            }
            None => composed.into(),
        };
        // The colour picker is a nested modal: it stacks over whichever dialog
        // (style editor, properties, …) requested it.
        if self.color_pick_target.is_some() {
            crate::ui::modal::modal(
                base,
                iced::widget::container(crate::ui::color_select::color_grid_window(
                    Message::ColorWindowPick,
                ))
                .width(iced::Length::Fixed(420.0))
                .height(iced::Length::Fixed(470.0)),
                Message::CloseColorPicker,
                iced::Vector::ZERO,
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
            PluginManager => (520, 460),
            UpdateNotice => (560, 460),
            Layers => (900, 360),
            PageSetup => (520, 460),
            LayoutManager => (640, 320),
            Plotstyle => (780, 540),
            TextStyle => (860, 480),
            MlStyle => (620, 420),
            TableStyle => (620, 420),
            MLeaderStyle => (560, 560),
            DimStyle => (720, 560),
            AssocPrompt => (440, 210),
            Unsaved => (420, 160),
            SaveDialog => (560, 480),
            PointStyle => (360, 470),
        };
        Some((w as f32 + EXTRA_W, h as f32 + EXTRA_H))
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
        iced::Subscription::batch([
            frames,
            history_tick,
            grip_dwell,
            hover_dwell,
            caret_blink,
            web_fonts,
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
                    iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        Some(Message::SetShiftDown(m.shift()))
                    }
                    iced::Event::Keyboard(keyboard::Event::KeyPressed {
                        key,
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
                                "c" => Some(Message::Command("COPYCLIP".to_string())),
                                "x" => Some(Message::Command("CUTCLIP".to_string())),
                                "v" => Some(Message::PasteShortcut),
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

pub(super) fn doc_tab_bar<'a>(tabs: &'a [DocumentTab], active_tab: usize) -> Element<'a, Message> {
    const BAR_BG: Color = Color {
        r: 0.13,
        g: 0.13,
        b: 0.13,
        a: 1.0,
    };
    const TAB_ACTIVE: Color = Color {
        r: 0.22,
        g: 0.22,
        b: 0.22,
        a: 1.0,
    };
    const TAB_HOVER: Color = Color {
        r: 0.18,
        g: 0.18,
        b: 0.18,
        a: 1.0,
    };
    const TAB_INACTIVE: Color = Color {
        r: 0.13,
        g: 0.13,
        b: 0.13,
        a: 1.0,
    };
    const ACCENT: Color = Color {
        r: 0.20,
        g: 0.55,
        b: 0.90,
        a: 1.0,
    };
    const TEXT_ACTIVE: Color = Color::WHITE;
    const TEXT_INACTIVE: Color = Color {
        r: 0.60,
        g: 0.60,
        b: 0.60,
        a: 1.0,
    };
    const CLOSE_HOVER: Color = Color {
        r: 0.70,
        g: 0.22,
        b: 0.22,
        a: 1.0,
    };
    const BORDER_COLOR: Color = Color {
        r: 0.25,
        g: 0.25,
        b: 0.25,
        a: 1.0,
    };

    let mut bar = Row::new().spacing(0).align_y(iced::Center);

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = idx == active_tab;
        let name = crate::ui::text_util::elide(&tab.tab_display_name(), 24);
        let title_inner: Element<'_, Message> = if tab.dirty {
            row![
                crate::ui::icons::tinted(
                    crate::ui::icons::DOT,
                    7.0,
                    Color {
                        r: 0.90,
                        g: 0.75,
                        b: 0.30,
                        a: 1.0,
                    },
                ),
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
            .padding([5, 12])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match (is_active, status) {
                    (true, _) => TAB_ACTIVE,
                    (false, button::Status::Hovered) => TAB_HOVER,
                    _ => TAB_INACTIVE,
                })),
                text_color: if is_active {
                    TEXT_ACTIVE
                } else {
                    TEXT_INACTIVE
                },
                border: Border {
                    color: if is_active {
                        ACCENT
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if is_active { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: false,
            });

        // Start tab is fixed — no close button. Every other tab gets a close.
        let row_inner: Row<'_, Message> = if tab.is_start {
            row![title_btn].spacing(0).align_y(iced::Center)
        } else {
            let close_btn = button(crate::ui::icons::tinted(
                crate::ui::icons::CLOSE,
                10.0,
                Color {
                    r: 0.55,
                    g: 0.55,
                    b: 0.55,
                    a: 1.0,
                },
            ))
            .on_press(Message::TabClose(idx))
            .padding([3, 5])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => CLOSE_HOVER,
                    _ => {
                        if is_active {
                            TAB_ACTIVE
                        } else {
                            TAB_INACTIVE
                        }
                    }
                })),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            row![title_btn, close_btn].spacing(0).align_y(iced::Center)
        };

        bar = bar.push(
            container(row_inner).style(move |_: &Theme| container::Style {
                border: Border {
                    color: if is_active {
                        BORDER_COLOR
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if is_active { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
        );
    }

    let new_btn = button(text("+").size(14).color(Color {
        r: 0.65,
        g: 0.65,
        b: 0.65,
        a: 1.0,
    }))
    .on_press(Message::TabNew)
    .padding([4, 10])
    .style(|_: &Theme, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => TAB_HOVER,
            _ => Color::TRANSPARENT,
        })),
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    bar = bar.push(new_btn);
    bar = bar.push(iced::widget::Space::new().width(Fill));

    container(bar)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BAR_BG)),
            border: Border {
                color: BORDER_COLOR,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .height(30)
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
// The page picks up the application icon's red-brown (#B03020) as a tint so
// it visually belongs to OpenCADStudio without overpowering the dark workspace.

const BRAND: Color = Color {
    r: 0.690,
    g: 0.188,
    b: 0.125,
    a: 1.0,
}; // #B03020
const BRAND_DARK: Color = Color {
    r: 0.45,
    g: 0.12,
    b: 0.08,
    a: 1.0,
};

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

pub(super) fn start_page_view<'a>() -> Element<'a, Message> {
    const TEXT: Color = Color {
        r: 0.94,
        g: 0.93,
        b: 0.92,
        a: 1.0,
    };
    const MUTED: Color = Color {
        r: 0.62,
        g: 0.62,
        b: 0.62,
        a: 1.0,
    };
    const CARD_BG: Color = Color {
        r: 0.12,
        g: 0.12,
        b: 0.13,
        a: 1.0,
    };
    const CARD_BORDER: Color = Color {
        r: 0.20,
        g: 0.20,
        b: 0.22,
        a: 1.0,
    };

    // Brand-tinted "Welcome to" — the "OpenCADStudio" word takes the accent colour
    // (Thunderbird-style coloured headline split).
    let headline = row![
        text("Welcome to ").size(40).color(TEXT),
        text("Open CAD Studio").size(40).color(BRAND),
    ]
    .align_y(iced::Center);

    let subtitle = text(
        "Open CAD Studio is an open-source CAD viewer and editor — a gift from contributors like you. \
         Open a DWG/DXF file, start a new drawing, or help shape what comes next.",
    )
    .size(13)
    .color(MUTED);

    // Plain outlined button (Open / New / Help / Contribute).
    let outline_btn = |label: &'static str, msg: Message| {
        button(text(label).size(14).color(TEXT))
            .on_press(msg)
            .padding([10, 22])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => Color {
                        r: 0.18,
                        g: 0.18,
                        b: 0.20,
                        a: 1.0,
                    },
                    _ => Color {
                        r: 0.13,
                        g: 0.13,
                        b: 0.15,
                        a: 1.0,
                    },
                })),
                text_color: TEXT,
                border: Border {
                    color: Color {
                        r: 0.30,
                        g: 0.30,
                        b: 0.33,
                        a: 1.0,
                    },
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
    };

    // Donate — the prominent call-to-action. Solid brand fill, white text.
    let donate_btn = {
        button(
            row![
                crate::ui::icons::tinted(crate::ui::icons::HEART, 14.0, Color::WHITE),
                text("Donate").size(14).color(Color::WHITE),
            ]
            .spacing(5)
            .align_y(iced::Center),
        )
        .on_press(Message::RibbonToolClick {
            tool_id: "DONATE".to_string(),
            event: crate::modules::ModuleEvent::Command("DONATE".to_string()),
        })
        .padding([12, 28])
        .style(|_: &Theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => BRAND_DARK,
                _ => BRAND,
            })),
            text_color: Color::WHITE,
            border: Border {
                color: BRAND_DARK,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: iced::Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.4,
                },
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
    };

    let primary_row = row![
        outline_btn("New Drawing", Message::TabNew),
        outline_btn("Open File…", Message::OpenFile),
        donate_btn,
    ]
    .spacing(12)
    .align_y(iced::Center);

    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut secondary_row = row![
        outline_btn(
            "Send Feedback",
            Message::RibbonToolClick {
                tool_id: "REPORT".to_string(),
                event: crate::modules::ModuleEvent::Command("REPORT".to_string()),
            },
        ),
        outline_btn(
            "Release Notes",
            Message::RibbonToolClick {
                tool_id: "CHANGELOG".to_string(),
                event: crate::modules::ModuleEvent::Command("CHANGELOG".to_string()),
            },
        ),
        outline_btn("Plugins", Message::PluginManagerOpen),
    ];
    // The web build is already in the browser, so only the desktop offers a
    // link to the web version.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Bright ribbon blue (matches the active-tool accent), filled.
        secondary_row = secondary_row.push(
            button(text("OCS Web").size(14).color(Color::WHITE))
                .on_press(Message::RibbonToolClick {
                    tool_id: "WEBVERSION".to_string(),
                    event: crate::modules::ModuleEvent::Command("WEBVERSION".to_string()),
                })
                .padding([10, 22])
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered => Color {
                            r: 0.15,
                            g: 0.45,
                            b: 0.78,
                            a: 1.0,
                        },
                        _ => Color {
                            r: 0.20,
                            g: 0.55,
                            b: 0.90,
                            a: 1.0,
                        },
                    })),
                    text_color: Color::WHITE,
                    border: Border {
                        color: Color {
                            r: 0.20,
                            g: 0.55,
                            b: 0.90,
                            a: 1.0,
                        },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
        );
    }
    let secondary_row = secondary_row.spacing(12).align_y(iced::Center);

    // Intro video: a clickable thumbnail with a play badge. Native iced has no
    // embedded web player, so a click opens the video in the system browser.
    const INTRO_VIDEO_URL: &str = "https://youtu.be/uN9zxM7p_fc";
    // Build the image Handle ONCE — `Handle::from_bytes` mints a fresh unique id
    // on every call, so doing it per-view re-decodes + re-uploads the JPEG each
    // frame (the thumbnail appeared only after a long delay). A cached Handle
    // decodes once; cloning it per view shares the id + bytes.
    let thumb_handle = {
        use std::sync::OnceLock;
        static H: OnceLock<iced::widget::image::Handle> = OnceLock::new();
        H.get_or_init(|| {
            iced::widget::image::Handle::from_bytes(
                include_bytes!("../../../assets/intro_thumb.jpg").to_vec(),
            )
        })
        .clone()
    };
    let thumb = iced::widget::image(thumb_handle)
        .width(Fill)
        .height(Fill)
        .content_fit(iced::ContentFit::Cover);
    // White play triangle on a translucent dark disc, centred over the thumb.
    let play_badge = container(
        container(crate::ui::icons::arrow_right(30.0, Color::WHITE))
            .width(iced::Length::Fixed(72.0))
            .height(iced::Length::Fixed(72.0))
            .center_x(iced::Length::Fixed(72.0))
            .center_y(iced::Length::Fixed(72.0))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.55,
                })),
                border: Border {
                    color: Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.85,
                    },
                    width: 2.0,
                    radius: 36.0.into(),
                },
                ..Default::default()
            }),
    )
    .center_x(Fill)
    .center_y(Fill);
    let cards = container(
        mouse_area(
            container(iced::widget::stack![thumb, play_badge])
                .width(iced::Length::Fixed(720.0))
                .height(iced::Length::Fixed(405.0))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(CARD_BG)),
                    border: Border {
                        color: CARD_BORDER,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
                .clip(true),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::OpenUrl(INTRO_VIDEO_URL.to_string())),
    )
    .center_x(Fill);

    // Buttons sit on a transparent container with a large, brand-tinted
    // ambient shadow (offset = 0, big blur) — produces a soft halo behind
    // the action row, matching the Thunderbird coloured-glow look against
    // the dark page.
    let primary_glow = container(primary_row)
        .padding(iced::Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            shadow: iced::Shadow {
                color: Color {
                    r: BRAND.r,
                    g: BRAND.g,
                    b: BRAND.b,
                    a: 0.45,
                },
                offset: iced::Vector::ZERO,
                blur_radius: 80.0,
            },
            ..Default::default()
        });

    let content = column![
        Space::new().height(iced::Length::Fixed(28.0)),
        container(headline).center_x(Fill),
        container(subtitle).center_x(Fill).padding([10, 60]),
        Space::new().height(iced::Length::Fixed(22.0)),
        container(primary_glow).center_x(Fill),
        Space::new().height(iced::Length::Fixed(10.0)),
        container(secondary_row).center_x(Fill),
        Space::new().height(iced::Length::Fixed(40.0)),
        cards,
        Space::new().height(Fill),
    ]
    .spacing(0)
    .width(Fill)
    .height(Fill);

    // Page background reverts to plain dark — the glow alone provides the
    // brand colour cue, the rest of the page stays neutral so it reads as
    // "workspace area" not "advertising banner".
    const PAGE_BG: Color = Color {
        r: 0.08,
        g: 0.08,
        b: 0.085,
        a: 1.0,
    };
    container(content)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(PAGE_BG)),
            ..Default::default()
        })
        .padding(iced::Padding {
            top: 40.0,
            right: 60.0,
            bottom: 40.0,
            left: 60.0,
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
pub(super) fn recent_files_panel<'a>(recents: &'a [std::path::PathBuf]) -> Element<'a, Message> {
    const PANEL_BG: Color = Color {
        r: 0.10,
        g: 0.10,
        b: 0.11,
        a: 1.0,
    };
    const PANEL_BORDER: Color = Color {
        r: 0.18,
        g: 0.18,
        b: 0.20,
        a: 1.0,
    };
    const ITEM_HOVER: Color = Color {
        r: 0.16,
        g: 0.16,
        b: 0.18,
        a: 1.0,
    };
    const TEXT: Color = Color {
        r: 0.92,
        g: 0.91,
        b: 0.90,
        a: 1.0,
    };
    const MUTED: Color = Color {
        r: 0.60,
        g: 0.60,
        b: 0.62,
        a: 1.0,
    };

    let header = container(text("Recent Documents").size(11).color(MUTED)).padding(iced::Padding {
        top: 12.0,
        right: 14.0,
        bottom: 8.0,
        left: 14.0,
    });

    let body: Element<'a, Message> = if recents.is_empty() {
        container(
            text("Files you open will show up here.")
                .size(11)
                .color(MUTED),
        )
        .padding([10, 14])
        .into()
    } else {
        let mut col = column![].spacing(0);
        for path in recents {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let dir = path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            let path_for_open = path.clone();
            let open_btn = button(
                column![
                    text(crate::ui::text_util::elide(&name, 32))
                        .size(12)
                        .color(TEXT),
                    text(crate::ui::text_util::elide(&dir, 42))
                        .size(10)
                        .color(MUTED),
                ]
                .spacing(2),
            )
            .on_press(Message::OpenRecent(path_for_open))
            .padding([6, 12])
            .width(Fill)
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => ITEM_HOVER,
                    _ => Color::TRANSPARENT,
                })),
                text_color: TEXT,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

            let path_for_remove = path.clone();
            let remove_btn = button(crate::ui::icons::tinted(crate::ui::icons::CLOSE, 11.0, MUTED))
                .on_press(Message::RecentRemove(path_for_remove))
                .padding([4, 8])
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered => Color {
                            r: 0.45,
                            g: 0.15,
                            b: 0.15,
                            a: 1.0,
                        },
                        _ => Color::TRANSPARENT,
                    })),
                    text_color: MUTED,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

            col = col.push(row![open_btn, remove_btn].spacing(0).align_y(iced::Center));
        }
        iced::widget::scrollable(col).into()
    };

    container(column![header, body])
        .width(iced::Length::Fixed(280.0))
        .height(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(PANEL_BG)),
            border: Border {
                color: PANEL_BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

