use super::document::DocumentTab;
use super::document::DynComponent;
use super::history::history_dropdown_labels;
use super::{ArrowKey, Message, OpenCADStudio};
use crate::scene::pick::grip::{grips_to_screen, grips_to_screen_paper, grips_to_screen_rte};
use crate::scene::view::viewport_pane::ViewportPane;
use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
use crate::ui::window::block_palette::BlockPaletteMsg;
use crate::ui::wrap_bar::DensitySwap;
use crate::ui::wrap_bar::WrapFlow;
use iced::widget::{
    button, canvas, column, container, mouse_area, pane_grid, responsive, row, scrollable, shader,
    stack, text, Row, Space,
};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Length, Subscription, Task, Theme};
use iced_aw::ContextMenu;
use crate::t;

mod controls;
mod modal;
pub(in crate::app) mod overlay;
mod viewcube;

use controls::{dyn_component_value, viewport_controls};
use overlay::{
    mtext_editor_overlay, position_canvas_overlay, position_canvas_overlay_near_cursor,
    qselect_overlay, text_inline_overlay, viewport_context_menu_overlay,
};
use viewcube::{viewcube_nav_controls, viewcube_ucs_picker, UCS_PICKER_W};

// Re-export the text-input element ids so sibling modules can address them at
// the `view::` path as before the split.
pub(in crate::app) use overlay::{MTEXT_TEXT_ID, TEXT_INLINE_ID};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_REGION_PX;
const PAPER_SPACE_BACKGROUND: Color = Color {
    r: 138.0 / 255.0,
    g: 138.0 / 255.0,
    b: 138.0 / 255.0,
    a: 1.0,
};

/// Base surface directly under the crosshair. Paper content viewports render
/// transparently over the sheet/desk, including while MSPACE input is active.
fn crosshair_background(tab: &DocumentTab, is_paper: bool) -> [f32; 4] {
    if !is_paper {
        return tab.scene.bg_color;
    }

    let desk = [
        PAPER_SPACE_BACKGROUND.r,
        PAPER_SPACE_BACKGROUND.g,
        PAPER_SPACE_BACKGROUND.b,
        PAPER_SPACE_BACKGROUND.a,
    ];
    let (cursor, viewport_size) = {
        let selection = tab.scene.selection.borrow();
        (selection.last_move_pos, selection.vp_size)
    };
    let Some(cursor) = cursor else {
        return desk;
    };
    if viewport_size.0 <= 0.0 || viewport_size.1 <= 0.0 {
        return desk;
    }
    let on_sheet = tab
        .scene
        .paper_sheet_screen_rect(viewport_size)
        .is_some_and(|rect| rect.contains(cursor));
    if on_sheet {
        tab.scene.paper_bg_color
    } else {
        desk
    }
}

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

fn hatch_pattern_key_event(
    event: iced::Event,
    status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    if !matches!(status, iced::event::Status::Captured) {
        return None;
    }
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    match key {
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
            Some(Message::PropHatchPatternNavigate(-1))
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
            Some(Message::PropHatchPatternNavigate(1))
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
            Some(Message::PropHatchPatternNavigate(-2))
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            Some(Message::PropHatchPatternNavigate(2))
        }
        _ => None,
    }
}

fn shortcut_key_name(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<String> {
    let key = match key {
        keyboard::Key::Character(value) if !value.is_empty() => value.to_uppercase(),
        keyboard::Key::Named(named) => {
            let name = format!("{named:?}");
            match name.as_str() {
                "ArrowUp" => "UP".to_string(),
                "ArrowDown" => "DOWN".to_string(),
                "ArrowLeft" => "LEFT".to_string(),
                "ArrowRight" => "RIGHT".to_string(),
                "PageUp" => "PAGEUP".to_string(),
                "PageDown" => "PAGEDOWN".to_string(),
                "Enter" | "Space" | "Escape" | "Delete" | "Backspace" | "Tab" | "Home"
                | "End" | "Insert" => name.to_uppercase(),
                _ if name.starts_with('F')
                    && name[1..].chars().all(|ch| ch.is_ascii_digit()) =>
                {
                    name
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let mut parts = Vec::with_capacity(5);
    if modifiers.control() {
        parts.push("CTRL".to_string());
    }
    if modifiers.logo() {
        parts.push("CMD".to_string());
    }
    if modifiers.alt() {
        parts.push("ALT".to_string());
    }
    if modifiers.shift() {
        parts.push("SHIFT".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
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
        let theme_text = self.active_theme.palette().background.base.text;
        let viewcube_text_color = [
            theme_text.r,
            theme_text.g,
            theme_text.b,
            theme_text.a,
        ];
        let is_paper = tab.scene.current_layout != "Model";
        let committed_render_mode = if is_paper {
            tab.scene
                .active_viewport_render_mode()
                .unwrap_or(tab.render_mode)
        } else {
            tab.render_mode
        };
        // Gallery hover is a non-destructive live preview: only the shader's
        // input changes. Dismissal restores the committed mode, while clicking
        // a row follows the normal SetRenderMode path and persists it.
        let viewport_render_mode = if self.render_mode_menu_open {
            self.render_mode_preview.unwrap_or(committed_render_mode)
        } else {
            committed_render_mode
        };
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
                self.start_action_w.clone(),
                self.start_section,
            )
        } else if is_paper {
            shader(ViewportPane::model(
                &tab.scene,
                viewcube_visible,
                viewport_render_mode,
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
            let render_mode = viewport_render_mode;
            let size_probe: Element<'_, Message> = responsive(move |size| {
                {
                    let mut sel = scene.selection.borrow_mut();
                    sel.vp_size = (size.width, size.height);
                }
                scene.sync_tiles_from_panes(size.width, size.height);
                Space::new().width(Fill).height(Fill)
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
            .min_size(scene.model_pane_min_px())
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
                .min_size(scene.model_pane_min_px())
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
                    let current_vertex_grip: Option<usize> = tab
                        .properties
                        .prop_vertex_indicator_active
                        .then(|| sel_h)
                        .flatten()
                        .and_then(|h| {
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
                        .enumerate()
                        .filter(|(_, (_, screen, _, _, _))| {
                            screen.x.is_finite()
                                && screen.y.is_finite()
                                && screen.x >= -bounds.width
                                && screen.x <= bounds.width * 2.0
                                && screen.y >= -bounds.height
                                && screen.y <= bounds.height * 2.0
                        })
                        .map(|(index, (grip_id, screen, _is_midpoint, shape, dir))| {
                            let owner = tab.selected_grip_handles.get(index).copied();
                            let is_hot = owner.is_some_and(|handle| {
                                tab.hot_grips.contains(&(handle, grip_id))
                                    || tab.active_grip.as_ref().is_some_and(|edit| {
                                        edit.targets.iter().any(|target| {
                                            target.handle == handle && target.grip_id == grip_id
                                        })
                                    })
                                    || (Some(handle) == sel_h
                                        && Some(grip_id) == current_vertex_grip)
                            });
                            let is_hovered = owner.is_some_and(|handle| {
                                self.grip_hover.as_ref().is_some_and(|hover| {
                                    hover.handle == handle && hover.grip_id == grip_id
                                })
                            });
                            crate::ui::overlay::GripMarker {
                                pos: screen,
                                shape,
                                is_hot,
                                is_hovered,
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
                tab.pan_mode || tab.orbit_mode,
                self.ribbon.open_dropdown.is_some(),
                hover_locked,
                crosshair_background(tab, is_paper),
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
            PAPER_SPACE_BACKGROUND
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
                                self.dyn_coord_absolute,
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
            stack![
                container(grid_overlay)
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(PAPER_SPACE_BACKGROUND)),
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
                self.render_mode_menu_open,
                self.render_mode_preview,
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
            .report_width0(tab.scene.model_pane_min_reporter())
            .into();
            // Pin the bar to the active model tile's top-left corner so it
            // follows the active panel in a tiled layout.
            let bar_layer = iced::widget::pin(
                container(adaptive).width(iced::Length::Fixed(rect.width.max(1.0))),
            )
            .position(iced::Point::new(rect.x.max(0.0), rect.y.max(0.0)));
            viewport_stack = viewport_stack.push(bar_layer);
        }

        // Active paper-space viewport overlays: a render-mode picker in
        // its top-left corner and a ViewCube hit area in its top-right,
        // both layered ABOVE the viewport mouse_area so they receive
        // clicks (the shader viewport sits below it). Positioned with
        // leading Spaces sized to the viewport's screen rectangle.
        let active_vp_rect: Option<(acadrust::Handle, iced::Rectangle)> =
            if is_paper && !tab.is_start {
                tab.scene.active_viewport.and_then(|h| {
                    let (cw, ch) = tab.scene.selection.borrow().vp_size;
                    tab.scene
                        .viewport_screen_rect(h, (cw, ch))
                        .map(|rect| (h, rect))
                })
            } else {
                None
            };
        if let Some((active_vp, rect)) = active_vp_rect {
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
            let border_layer =
                iced::widget::pin(border_frame).position(iced::Point::new(x, y));
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
                self.render_mode_menu_open,
                self.render_mode_preview,
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
            let picker_layer = iced::widget::pin(
                container(adaptive).width(iced::Length::Fixed(rect.width.max(1.0))),
            )
            .position(iced::Point::new(x + 4.0, y + 4.0));
            viewport_stack = viewport_stack.push(picker_layer);

            // Hide the ViewCube first — before the render bar — when they collide.
            if viewcube_visible {
                let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
                let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);

                let controls = iced::widget::pin(viewcube_nav_controls(Some(active_vp)))
                    .position(iced::Point::new(cube_x, cube_y));
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
                let picker = iced::widget::pin(iced::widget::opaque(viewcube_ucs_picker(
                    ucs_current,
                    ucs_names,
                )))
                .position(iced::Point::new(
                        cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                        cube_y + VIEWCUBE_HIT_SIZE + 6.0,
                    ));
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
            let controls = iced::widget::pin(viewcube_nav_controls(None))
                .position(iced::Point::new(cube_x, cube_y));
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
            let picker = iced::widget::pin(iced::widget::opaque(viewcube_ucs_picker(
                ucs_current,
                ucs_names,
            )))
            .position(iced::Point::new(
                    cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                    cube_y + VIEWCUBE_HIT_SIZE + 6.0,
                ));
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
                            let palette = theme.palette();
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
                        let palette = theme.palette();
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
                            let palette = theme.palette();
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
                    .height(iced::Length::Fit.max(360.0))
                    .padding(2)
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
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
        if tab.active_block_edit.is_some() && !tab.is_start {
            if let Some(tb) = crate::ui::side_toolbar::view(
                &crate::modules::draw::modify::block_edit::block_edit_tools(),
            ) {
                viewport_stack = viewport_stack.push(tb);
            }
        }

        // Reserve the overlaid command line when placing cursor-anchored panels.
        let command_line_inset = if self.command_line.history_open {
            self.command_line.history_height.clamp(
                crate::ui::command_line::HISTORY_HEIGHT_MIN,
                crate::ui::command_line::history_max_height(self.win_size.1),
            ) + 72.0
        } else {
            34.0
        };

        // Quick Properties: stay near the selection cursor, flipping around
        // it as needed to remain inside the visible drawing area.
        if self.quick_properties && !tab.is_start {
            if let Some(panel) = tab.properties.quick_view() {
                viewport_stack = viewport_stack.push(position_canvas_overlay_near_cursor(
                    self.quick_properties_anchor,
                    command_line_inset,
                    panel,
                ));
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
            let trace = crate::perf::snapshot_tail_text(80);
            let trace = if trace.is_empty() {
                "No samples yet".to_string()
            } else {
                trace
            };
            let perf_button_style = |theme: &Theme, status: button::Status| {
                let palette = theme.palette();
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
                    text(t!("Copy")).size(11),
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
                    text(t!("Clear")).size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::PerfClear)
            .style(perf_button_style)
            .padding([2, 6]);
            let header = row![
                text(t!("PERF")).size(12).style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().success.base.color),
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
                        color: Some(theme.palette().success.base.color),
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
                    command_line_inset,
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
                    self.modal_offset,
                    self.modal_resize,
                    self.modal_content_size,
                ));
            }
            if let Some(ed) = &self.text_inline {
                viewport_stack = viewport_stack.push(text_inline_overlay(ed, canvas));
            }
        }

        // Docked Properties panel. It keeps its pixel width when moved between
        // edges; auto-collapse swaps the full panel for a hoverable rail.
        let show_properties = !tab.is_start && self.show_properties && !self.clean_screen;
        let properties_width = self
            .properties_width
            .min((self.win_size.0 * 0.45).clamp(220.0, 600.0));
        let properties_el: Option<Element<'_, Message>> = show_properties.then(|| {
            let narrow_collapsed = self.win_size.0 < 1000.0
                && !self.props_expanded
                && !self.properties_hovered;
            let auto_collapsed = self.properties_auto_collapse
                && !self.properties_hovered
                && !self.properties_dragging
                && !self.properties_resizing;
            if narrow_collapsed || auto_collapsed {
                collapse_bar(
                    "Properties",
                    self.properties_side,
                    Message::TogglePropertiesBar,
                    Message::PropertiesHover(true),
                    26.0,
                )
            } else {
                let panel = tab
                    .properties
                    .view(properties_width, self.properties_auto_collapse);
                let divider = properties_divider();
                let group: Element<'_, Message> = match self.properties_side {
                    crate::app::config::DockSide::Left => row![panel, divider].into(),
                    crate::app::config::DockSide::Right => row![divider, panel].into(),
                };
                mouse_area(group)
                    .on_enter(Message::PropertiesHover(true))
                    .on_exit(Message::PropertiesHover(false))
                    .into()
            }
        });

        // Drawing viewports keep the command line as a bottom-centre overlay so
        // the input stays close to the cursor. The Start page gives it a real
        // layout row instead: its panels and action buttons must end above the
        // command line rather than rendering behind it (#546).
        // The block palette docks on the right, mirroring the Properties panel on the
        // left. The collapse button reduces it to a vertical bar whose width equals
        // the title height, with the title rotated 90° (see `collapse_bar`).
        let block_palette_el: Element<'_, Message> = if tab.is_start {
            Space::new().into()
        } else if self.show_block_palette && !self.clean_screen {
            if self.block_palette_expanded {
                crate::ui::window::block_palette::view(&self.block_palette)
            } else {
                // Collapsed bar width matches the title bar height (icon button
                // 30px + 5px top/bottom padding) so expand/collapse is seamless.
                collapse_bar(
                    "Block Palette",
                    crate::app::config::DockSide::Right,
                    Message::BlockPalette(BlockPaletteMsg::ToggleBar),
                    Message::Noop,
                    40.0,
                )
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
        let workspace: Element<'_, Message> = match (properties_el, self.properties_side) {
            (Some(properties), crate::app::config::DockSide::Left) => {
                row![properties, viewport_stack].width(Fill).height(Fill).into()
            }
            (Some(properties), crate::app::config::DockSide::Right) => {
                row![viewport_stack, properties].width(Fill).height(Fill).into()
            }
            (None, _) => container(viewport_stack).width(Fill).height(Fill).into(),
        };
        let workspace: Element<'_, Message> = if self.properties_dragging {
            let preview_side = self.properties_dock_preview.unwrap_or(self.properties_side);
            let preview = container(Space::new())
                .width(Length::Fixed(properties_width))
                .height(Fill)
                .style(|theme: &Theme| {
                    let palette = theme.palette();
                    container::Style {
                        background: Some(Background::Color(
                            palette.primary.weak.color.scale_alpha(0.72),
                        )),
                        border: Border {
                            color: palette.primary.base.color,
                            width: 2.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }
                });
            let preview = container(preview)
                .width(Fill)
                .height(Fill)
                .align_x(match preview_side {
                    crate::app::config::DockSide::Left => iced::alignment::Horizontal::Left,
                    crate::app::config::DockSide::Right => iced::alignment::Horizontal::Right,
                });
            stack![workspace, preview].width(Fill).height(Fill).into()
        } else {
            workspace
        };
        let workspace: Element<'_, Message> = if self.properties_dragging
            || self.properties_resizing
        {
            mouse_area(workspace)
                .on_move(Message::PropertiesDragMove)
                .on_release(Message::PropertiesDragRelease)
                .interaction(if self.properties_resizing {
                    iced::mouse::Interaction::ResizingHorizontally
                } else {
                    iced::mouse::Interaction::Grabbing
                })
                .into()
        } else {
            workspace
        };
        let workspace = row![workspace, block_palette_el]
            .width(Fill)
            .height(Fill);
        let command_line = self.command_line.view(
            allow_autocomplete,
            dyn_capturing,
            &self.history_content,
            self.win_size.1,
        );
        let center_stack: Element<'_, Message> = if tab.is_start {
            column![
                workspace,
                iced::widget::container(command_line)
                    .width(Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .padding(iced::Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 2.0,
                        left: 0.0,
                    }),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            let command_line_overlay = iced::widget::container(command_line)
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
            iced::widget::stack![workspace, command_line_overlay]
                .width(Fill)
                .height(Fill)
                .into()
        };

        let center_stack: Element<'_, Message> = if self.command_history_resizing {
            mouse_area(center_stack)
                .on_move(Message::CommandHistoryResizeMove)
                .on_release(Message::CommandHistoryResizeRelease)
                .interaction(iced::mouse::Interaction::ResizingVertically)
                .into()
        } else {
            center_stack
        };

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
                col = col.push(doc_tab_bar(
                    &self.tabs,
                    self.active_tab,
                    self.hovered_doc_tab,
                ));
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
                    let block_tabs = tab
                        .block_edits
                        .iter()
                        .map(|session| session.block_name.clone())
                        .collect();
                    let active_block = tab
                        .active_block_edit_session()
                        .map(|session| session.block_name.clone());
                    let status_menu_data = crate::ui::statusbar::StatusMenuData {
                        layout_names: layout_names.clone(),
                        polar_custom_input: &self.polar_custom_input,
                        scale_is_model: is_model,
                        current_scale_name: tab.scene.displayed_annotation_scale_name(),
                        scale_list: tab.scene.scale_picker_list(),
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
                        layout_names.clone(),
                        block_tabs,
                        layout_names.into_iter().skip(1).collect(),
                        tab.scene.current_layout.clone(),
                        active_block,
                        tab.is_start,
                        self.layout_rename_state.as_ref(),
                        tab.scene.first_viewport_scale(),
                        tab.scene.viewport_count(),
                        tab.scene.active_viewport.is_some(),
                        self.show_layout_tabs,
                        tab.scene.annotation_scale,
                        scale_pill_enabled,
                        tab.scene.annotation_all_visible(),
                        self.annotation_auto_scale > 0,
                        tab.scene.viewport_annotation_scale_synced(),
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
                theme.palette().background.base.color
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
                self.tabs[self.active_tab].is_start,
            )
            .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into());

        let snap_override_layer: Element<'_, Message> =
            if let Some(pos) = self.snap_override_popup {
                overlay::snap_override_overlay(pos)
            } else {
                iced::widget::Space::new().width(0).height(0).into()
            };

        let qselect_layer: Element<'_, Message> = if let Some(state) = &self.qselect {
            qselect_overlay(
                state,
                &state.available_types,
                &state.available_properties,
                state.candidate_count,
                self.modal_offset,
                self.modal_resize,
            )
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let open_progress_layer: Element<'_, Message> = if let Some(p) = self
            .opening
            .as_ref()
            .filter(|progress| progress.recovery_error.is_none())
        {
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
                let modal_options = if cfg!(target_arch = "wasm32")
                    && matches!(self.active_modal, Some(super::ModalKind::PluginManager))
                {
                    crate::ui::modal::ModalOptions::NOTICE
                } else {
                    crate::ui::modal::ModalOptions::STANDARD
                };
                crate::ui::modal::modal(
                    composed,
                    self.modal_title(),
                    content,
                    Message::CloseModal,
                    self.modal_offset,
                    modal_options,
                )
            }
            None => composed.into(),
        };
        // iced_aw owns the colour-picker overlay and keeps it above whichever
        // application modal requested it.
        if let Some((_, current)) = self.color_pick_target.as_ref() {
            let initial = crate::ui::properties::acad_color_display(*current).0;
            let modal_base =
                crate::ui::modal::backdrop(base, Message::CloseColorPicker);
            iced_aw::ColorPicker::new(
                true,
                initial,
                modal_base,
                Message::CloseColorPicker,
                |color| {
                    Message::ColorWindowPick(
                        crate::ui::color_select::iced_to_acad_color(color),
                    )
                },
            )
            .into()
        } else {
            base
        }
    }

    /// Measured outer pixel bounds for whichever shared modal is visible, used
    /// to keep its frame on-screen while dragging.
    pub(crate) fn modal_outer_size(&self) -> Option<(f32, f32)> {
        let size = self.modal_content_size?;
        // Frame padding is 10 px per side; title + body spacing is 30 px.
        Some((size.width + 20.0, size.height + 50.0))
    }
}

impl OpenCADStudio {
    pub fn subscription(&self) -> Subscription<Message> {
        use iced::event;
        // Only request per-frame ticks while something on screen is animating
        // (currently just the open-progress indicator). Without this gate the
        // app burned 2-3% CPU continuously redrawing an unchanged view.
        // See #18.
        let needs_frames = self
            .opening
            .as_ref()
            .is_some_and(|progress| progress.recovery_error.is_none());
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
        let hatch_pattern_keys = if self.tabs[self.active_tab]
            .properties
            .hatch_pattern_picker_open
        {
            event::listen_with(hatch_pattern_key_event)
        } else {
            Subscription::none()
        };
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
            hatch_pattern_keys,
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
                        #[cfg(target_arch = "wasm32")]
                        let accel = modifiers.command();
                        let shortcut_modifier =
                            modifiers.control() || modifiers.alt() || modifiers.logo();
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
                        if !shortcut_modifier
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
                        if !shortcut_modifier && status == Status::Ignored {
                            if let Some(t) = text.as_deref() {
                                if !t.is_empty()
                                    && t.chars().all(|c| !c.is_control() && !c.is_whitespace())
                                {
                                    return Some(Message::CommandAppendChar(t.to_string()));
                                }
                            }
                        }
                        let has_printable_text = text.as_deref().is_some_and(|value| {
                            !value.is_empty()
                                && value
                                    .chars()
                                    .all(|ch| !ch.is_control() && !ch.is_whitespace())
                        });
                        let arrow = match &key {
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                                Some(ArrowKey::Up)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                                Some(ArrowKey::Down)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                                Some(ArrowKey::Left)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                                Some(ArrowKey::Right)
                            }
                            _ => None,
                        };
                        if !has_printable_text && status == Status::Ignored {
                            if let Some(direction) = arrow {
                                return Some(Message::ArrowKeyPressed {
                                    direction,
                                    shortcut: shortcut_key_name(&key, modifiers)?,
                                    extend_selection: modifiers.shift(),
                                });
                            }
                        }
                        if !has_printable_text
                            && status == Status::Captured
                            && matches!(arrow, Some(ArrowKey::Up | ArrowKey::Down))
                        {
                            return Some(Message::CommandLineArrowProbe {
                                direction: arrow?,
                            });
                        }
                        // A focused web text field needs the browser clipboard;
                        // drawing shortcuts only run for ignored C/V events.
                        #[cfg(target_arch = "wasm32")]
                        if accel && status == Status::Captured {
                            if let keyboard::Key::Character(value) = &key {
                                if value.eq_ignore_ascii_case("v") {
                                    return Some(Message::WebFieldPaste);
                                }
                                if value.eq_ignore_ascii_case("c") {
                                    return Some(Message::WebFieldCopy);
                                }
                            }
                        }
                        let shortcut = shortcut_key_name(&key, modifiers)?;
                        (status == Status::Ignored
                            || crate::app::shortcuts::is_global_key(&shortcut))
                        .then_some(Message::ShortcutPressed(shortcut))
                    }
                    _ => None,
                }
            }),
        ])
    }

    pub(super) fn focus_cmd_input(&self) -> Task<Message> {
        iced::widget::operation::focus(iced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID))
    }

    pub(super) fn unfocus_widgets(&self) -> Task<Message> {
        iced::advanced::widget::operate(
            iced::advanced::widget::operation::focusable::unfocus(),
        )
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

    let mut menu = column![
        item("Save All", Some(Message::DocTabSaveAll)),
        item("Close All", Some(Message::DocTabCloseAll)),
        item(
            "Close All Other Drawings",
            has_other_drawings.then_some(Message::DocTabCloseOthers(tab_idx)),
        ),
    ];
    if cfg!(not(target_arch = "wasm32")) {
        menu = menu
            .push(item(
                "Copy Full File Path",
                has_current_path.then_some(Message::DocTabCopyFullPath(tab_idx)),
            ))
            .push(item(
                "Open File Location",
                has_current_path.then_some(Message::DocTabOpenFileLocation(tab_idx)),
            ));
    }

    container(menu.spacing(0).width(MENU_W))
    .style(container::bordered_box)
    .padding([4, 0])
    .width(iced::Length::Fixed(MENU_W))
    .into()
}

pub(super) fn doc_tab_bar<'a>(
    tabs: &'a [DocumentTab],
    active_tab: usize,
    hovered_tab: Option<usize>,
) -> Element<'a, Message> {
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
        let is_hovered = hovered_tab == Some(idx);
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
            .padding([5, 14])
            .style(move |theme: &Theme, _status| {
                let palette = theme.palette();
                button::Style {
                    background: None,
                    text_color: if is_active {
                        palette.primary.weak.text
                    } else if is_hovered {
                        palette.background.weak.text
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
                .padding([5, 9])
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    button::Style {
                        background: matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        )
                        .then_some(Background::Color(palette.warning.weak.color)),
                        text_color: if matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        ) {
                            palette.warning.weak.text
                        } else if is_active {
                            palette.primary.weak.text
                        } else if is_hovered {
                            palette.background.weak.text
                        } else {
                            palette.background.base.text.scale_alpha(0.72)
                        },
                        border: Border::default(),
                        shadow: iced::Shadow::default(),
                        snap: false,
                    }
                });
            row![title_btn, close_btn]
                .spacing(0)
                .height(Fill)
                .align_y(iced::Center)
        };

        let tab_container = container(row_inner)
            .height(iced::Length::Fixed(28.0))
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let background = if is_active {
                    palette.primary.weak.color
                } else if is_hovered {
                    palette.background.weak.color
                } else {
                    palette.background.base.color
                };
                container::Style {
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: if is_active {
                            palette.primary.base.color
                        } else {
                            palette.background.neutral.color
                        },
                        width: 1.0,
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
        items.push(
            mouse_area(tab_element)
                .on_enter(Message::DocTabHover(Some(idx)))
                .on_exit(Message::DocTabHover(None))
                .into(),
        );
    }

    let new_btn = button(text("+").size(14))
        .on_press(Message::TabNew)
        .height(iced::Length::Fixed(28.0))
        .padding([5, 10])
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            let hovered = matches!(
                status,
                button::Status::Hovered | button::Status::Pressed
            );
            button::Style {
                background: Some(Background::Color(if hovered {
                    palette.background.weak.color
                } else {
                    palette.background.base.color
                })),
                text_color: palette.background.base.text,
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: false,
            }
        });

    items.push(
        container(new_btn)
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 6.0,
            })
            .into(),
    );

    container(
        Row::with_children(items)
            .spacing(0.0)
            .align_y(iced::Center)
            .wrap()
            .vertical_spacing(2.0),
    )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color,
            )),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .width(Fill)
        .padding([2, 2])
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
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn start_primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
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
    clockwise: bool,
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
            frame.rotate(iced::Radians(if self.clockwise {
                std::f32::consts::FRAC_PI_2
            } else {
                -std::f32::consts::FRAC_PI_2
            }));
            frame.fill_text(canvas::Text {
                content: self.text.clone(),
                position: iced::Point::ORIGIN,
                color: theme.palette().background.base.text.scale_alpha(0.72),
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
/// it. It can be clicked on narrow windows or expanded by hover when auto-hide
/// is enabled.
pub(super) fn collapse_bar<'a>(
    name: &str,
    side: crate::app::config::DockSide,
    on_press: Message,
    on_enter: Message,
    width: f32,
) -> Element<'a, Message> {
    let label = canvas(VBarLabel {
        text: name.to_string(),
        clockwise: side == crate::app::config::DockSide::Left,
    })
    .width(Fill)
    .height(Fill);

    mouse_area(
        container(label)
            .width(iced::Length::Fixed(width))
            .height(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.palette().background.base.color,
                )),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
    )
    .interaction(iced::mouse::Interaction::Pointer)
    .on_press(on_press)
    .on_enter(on_enter)
    .into()
}

/// Grabbable separator between the docked panel and drawing view. The visible
/// line is wider than a single pixel so it remains discoverable in every theme.
fn properties_divider() -> Element<'static, Message> {
    let line = container(Space::new())
        .width(Length::Fixed(5.0))
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color,
            )),
            ..Default::default()
        });
    mouse_area(line)
        .on_press(Message::PropertiesResizeGrab)
        .on_double_click(Message::PropertiesWidthReset)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

const START_ACTION_RADIUS: f32 = 6.0;

fn start_action_shape(mut style: button::Style) -> button::Style {
    style.border.radius = START_ACTION_RADIUS.into();
    style
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
    action_width_out: std::sync::Arc<std::sync::atomic::AtomicU32>,
    active: super::StartSection,
) -> Element<'a, Message> {
    responsive(move |size| {
        start_page_content(
            patrons,
            videos,
            videos_loading,
            video_thumbs,
            discussions,
            discussions_loading,
            recents,
            thumbs,
            recent_limit,
            recent_limit_input,
            size.width,
            action_width_out.clone(),
            active,
        )
    })
    .into()
}

fn start_page_content<'a>(
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
    let outline_btn = |label: String, msg: Message| {
        button(text(label).size(14))
            .on_press(msg)
            .padding([10, 22])
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let pair = match status {
                    button::Status::Hovered => palette.background.strong,
                    _ => palette.background.weak,
                };
                start_action_shape(button::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: pair.text,
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
    };

    // Donate — the prominent call-to-action, using the theme's danger role.
    let donate_btn = {
        button(
            row![
                crate::ui::icons::themed_danger_text(crate::ui::icons::HEART, 14.0),
                text(crate::tr!("start-donate")).size(14),
            ]
            .spacing(5)
            .align_y(iced::Center),
        )
        .on_press(Message::RibbonToolClick {
            tool_id: "DONATE".to_string(),
            event: crate::modules::ModuleEvent::Command("DONATE".to_string()),
        })
        .padding([10, 22])
        .style(|theme: &Theme, status| start_action_shape(button::danger(theme, status)))
    };

    let primary_row = WrapFlow::new(vec![
        outline_btn(crate::tr!("start-new-drawing"), Message::TabNew).into(),
        outline_btn(crate::tr!("start-open-file"), Message::OpenFile).into(),
        donate_btn.into(),
    ])
    .spacing_x(12.0)
    .row_h(48.0)
    .report_natural_width(action_width_out.clone());

    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut secondary_items: Vec<Element<'a, Message>> = vec![
        outline_btn(
            crate::tr!("start-send-feedback"),
            Message::RibbonToolClick {
                tool_id: "REPORT".to_string(),
                event: crate::modules::ModuleEvent::Command("REPORT".to_string()),
            },
        )
        .into(),
        outline_btn(crate::tr!("action-options"), Message::OptionsOpen).into(),
    ];
    secondary_items.push(outline_btn(crate::tr!("action-plugins"), Message::PluginManagerOpen).into());
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
                .style(|theme: &Theme, status| start_action_shape(button::primary(theme, status)))
                .into(),
        );
    }
    let secondary_row = WrapFlow::new(secondary_items)
        .spacing_x(12.0)
        .row_h(44.0)
        .report_natural_width(action_width_out.clone());

    let sponsors = column![
        text(crate::tr!("start-sponsors")).size(15),
        mouse_area(
            container(
                iced::widget::svg(iced::widget::svg::Handle::from_memory(include_bytes!(
                    "../../../assets/sponsors/openaec-logo-dark-on-light.svg"
                )))
                .width(Fill)
                .height(iced::Length::Fixed(120.0))
                .content_fit(iced::ContentFit::Contain),
            )
            .width(Fill.max(300.0)),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::OpenUrl("https://open-aec.com/".to_string())),
    ]
    .spacing(10)
    .align_x(iced::alignment::Horizontal::Center)
    .width(Fill);

    let content = column![
        Space::new().height(iced::Length::Fixed(28.0)),
        container(headline).center_x(Fill),
        Space::new().height(iced::Length::Fixed(22.0)),
        container(primary_row).center_x(Fill),
        Space::new().height(iced::Length::Fixed(10.0)),
        container(secondary_row).center_x(Fill),
        Space::new().height(Fill),
        sponsors,
        Space::new().height(iced::Length::Fixed(52.0)),
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
        let mut list = column![text(crate::tr!("start-tutorials")).size(15)]
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
                            color: theme.palette().background.neutral.color,
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
                crate::tr!("start-loading-videos")
            } else {
                crate::tr!("start-videos-online")
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let playlist_btn = mouse_area(
            container(text(crate::tr!("start-open-playlist")).size(12))
            .padding([6, 10])
            .width(Fill)
            .center_x(Fill)
            .style(|theme: &Theme| {
                let pair = theme.palette().danger.base;
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
            let palette = theme.palette();
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
        let mut list = column![text(crate::tr!("start-discussions")).size(15)]
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
                    text(crate::tr!("start-pinned"))
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
                let palette = theme.palette();
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
                crate::tr!("start-loading-discussions")
            } else {
                crate::tr!("start-discussions-online")
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let open_btn = mouse_area(
            container(text(crate::tr!("start-open-discussions")).size(12))
                .padding([6, 10])
                .width(Fill)
                .center_x(Fill)
                .style(|theme: &Theme| {
                    let pair = theme.palette().primary.base;
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
            let palette = theme.palette();
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
            text(crate::tr!("start-supporters")).size(15),
            Space::new().height(iced::Length::Fixed(12.0)),
        ]
        .spacing(6)
        .padding(iced::Padding {
            right: 12.0,
            ..iced::Padding::ZERO
        })
        .width(Fill);
        for (name, cents) in patrons {
            // Patreon payments are normalized to USD cents while the list is
            // generated; hand-maintained entries use USD cents as well.
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
                    text(crate::tr!("start-support-on-patreon")).size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .padding([6, 10])
            .width(Fill)
            .center_x(Fill)
            .style(|theme: &Theme| {
                let pair = theme.palette().danger.base;
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
            let palette = theme.palette();
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
            let tab_btn = |label: String, section: super::StartSection| {
                let is_active = active == section;
                button(text(label).size(14))
                    .on_press(Message::StartSectionSelect(section))
                    .padding([8, 18])
                    .style(move |theme: &Theme, status| {
                        let palette = theme.palette();
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
            let tab_bar = Row::with_children(vec![
                tab_btn(crate::tr!("start-recent-files"), super::StartSection::Recent).into(),
                tab_btn(crate::tr!("start-videos"), super::StartSection::Videos).into(),
                tab_btn(crate::tr!("start-welcome"), super::StartSection::Welcome).into(),
                tab_btn(crate::tr!("start-discussions"), super::StartSection::Discussions).into(),
                tab_btn(crate::tr!("start-supporters"), super::StartSection::Supporters).into(),
            ])
            .spacing(6.0)
            .align_y(iced::Center)
            .wrap()
            .vertical_spacing(0.0);
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
            theme.palette().background.base.color
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
    let title = text(crate::tr!("start-recent-documents")).size(15);

    let body: Element<'a, Message> = if recents.is_empty() {
        container(
            text(crate::tr!("start-no-recent-files"))
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
            // Browsers intentionally do not reveal the source folder selected
            // by the user. The reusable copy lives in origin-private browser
            // storage, which is the truthful web equivalent of native's parent
            // directory line.
            #[cfg(target_arch = "wasm32")]
            let dir = if dir.is_empty() {
                crate::tr!("start-browser-storage")
            } else {
                dir
            };

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
                let palette = theme.palette();
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
                    let palette = theme.palette();
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
        let palette = theme.palette();
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
        text(crate::tr!("start-keep-recent-files")).size(11).style(start_muted_style).width(Fill),
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
        let palette = theme.palette();
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
