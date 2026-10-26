use super::document::DocumentTab;
use super::document::DynComponent;
use super::history::history_dropdown_labels;
use super::{ArrowKey, Message, OpenCADStudio};
use crate::scene::pick::grip::{grips_to_screen, grips_to_screen_paper, grips_to_screen_rte};
use crate::scene::view::viewport_pane::ViewportPane;
use crate::scene::{VIEWCUBE_PAD, VIEWCUBE_REGION_PX};
use crate::t;
use crate::ui::wrap_bar::DensitySwap;
use crate::ui::wrap_bar::WrapFlow;
use iced::widget::{
    button, column, container, mouse_area, pane_grid, responsive, row, scrollable, shader, stack,
    text, Row, Space,
};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Subscription, Task, Theme};
use iced_aw::ContextMenu;

/// Invisible event surface for accurate point picking. MouseArea emits a
/// positionless press message; capturing the cursor in the actual button
/// event avoids using the previous hover location when clicking quickly.
#[derive(Debug, Clone, Copy)]
struct ViewportPickSurface;

#[derive(Default)]
struct ViewportPointerState {
    middle_drag: bool,
    position: Option<iced::Point>,
}

impl ViewportPointerState {
    fn event_cursor(
        &mut self,
        event: &iced::mouse::Event,
        fallback: iced::mouse::Cursor,
    ) -> iced::mouse::Cursor {
        // iced passes the final window cursor to every event in a batch.
        // Preserve event order: a press must see the preceding CursorMoved,
        // never the cursor from a later move in that same batch.
        match event {
            iced::mouse::Event::CursorMoved { position } => self.position = Some(*position),
            iced::mouse::Event::CursorLeft => self.position = None,
            _ => {}
        }
        // An overlay can deliberately hide/levitate the base cursor. Retain
        // that ownership even though we know a historical pointer position.
        if !matches!(fallback, iced::mouse::Cursor::Available(_)) {
            return fallback;
        }
        self.position
            .map(iced::mouse::Cursor::Available)
            .unwrap_or(fallback)
    }
}

/// Pane-local point of a lone finger driving the cursor. Touch events stay
/// uncaptured: the gesture handler listens to them as well.
fn touch_cursor_move(event: &iced::Event, bounds: iced::Rectangle) -> Option<iced::Point> {
    use iced::touch::Event as Touch;
    match event {
        iced::Event::Touch(
            Touch::FingerPressed { position, .. } | Touch::FingerMoved { position, .. },
        ) if bounds.contains(*position) => Some(iced::Point::new(
            position.x - bounds.x,
            position.y - bounds.y,
        )),
        _ => None,
    }
}

impl iced::widget::canvas::Program<Message> for ViewportPickSurface {
    type State = ViewportPointerState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<Message>> {
        use iced::mouse::{Button, Event as Mouse};
        use iced::widget::canvas::Action;
        if let Some(local) = touch_cursor_move(event, bounds) {
            return Some(Action::publish(Message::ViewportMove(local)));
        }
        let iced::Event::Mouse(event) = event else {
            return None;
        };
        let cursor = state.event_cursor(event, cursor);
        match event {
            Mouse::ButtonPressed(Button::Left) => cursor.position_in(bounds).map(|position| {
                Action::publish(Message::ViewportLeftPressAt(position)).and_capture()
            }),
            Mouse::ButtonPressed(Button::Middle) => {
                state.middle_drag = cursor.is_over(bounds);
                None
            }
            // Motion comes from the event, not the final batch cursor, so the
            // MouseArea around this canvas reports no moves of its own. Left
            // uncaptured so it still tracks hover for `on_exit`. A middle
            // drag keeps following the pointer past the viewport edge.
            Mouse::CursorMoved { position } if cursor.is_over(bounds) || state.middle_drag => {
                Some(Action::publish(Message::ViewportMove(iced::Point::new(
                    position.x - bounds.x,
                    position.y - bounds.y,
                ))))
            }
            Mouse::ButtonReleased(Button::Middle) if state.middle_drag => {
                state.middle_drag = false;
                (!cursor.is_over(bounds)).then(|| Action::publish(Message::ViewportMiddleRelease))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &iced::Renderer,
        _theme: &iced::Theme,
        _bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        Vec::new()
    }
}

/// Input layer of one Model pane: keeps a pan or orbit that began in the pane
/// following the pointer past its edge, and hears the release there.
struct PaneDragSurface(usize);

impl iced::widget::canvas::Program<Message> for PaneDragSurface {
    type State = ViewportPointerState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<Message>> {
        use iced::mouse::{Button, Event as Mouse};
        use iced::widget::canvas::Action;
        if let Some(local) = touch_cursor_move(event, bounds) {
            return Some(Action::publish(Message::PaneMove(self.0, local)));
        }
        let iced::Event::Mouse(event) = event else {
            return None;
        };
        let cursor = state.event_cursor(event, cursor);
        match event {
            Mouse::ButtonPressed(Button::Left) => cursor.position_in(bounds).map(|position| {
                Action::publish(Message::PanePressAt(self.0, position)).and_capture()
            }),
            Mouse::ButtonPressed(Button::Middle) => {
                state.middle_drag = cursor.is_over(bounds);
                None
            }
            Mouse::CursorMoved { position } if cursor.is_over(bounds) || state.middle_drag => {
                let local = iced::Point::new(position.x - bounds.x, position.y - bounds.y);
                Some(Action::publish(Message::PaneMove(self.0, local)))
            }
            Mouse::ButtonReleased(Button::Middle) if state.middle_drag => {
                state.middle_drag = false;
                (!cursor.is_over(bounds))
                    .then(|| Action::publish(Message::PaneMiddleRelease(self.0)))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &iced::Renderer,
        _theme: &iced::Theme,
        _bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        Vec::new()
    }
}

use std::sync::Arc;

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

pub(in crate::app) const VIEWPORT_CAPTURE_BOUNDS_ID: &str = "viewport-capture-bounds";

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_REGION_PX;

/// Desktop UI-font changes while running. Linux listens for the Settings
/// portal's `SettingChanged` (no portal: restart to apply). Windows exposes no
/// change notification through iced, but the metrics read is a cheap system
/// call, so it is polled. macOS has no user-adjustable UI font, and the web
/// build follows the font it loads for the UI language.
fn ui_font_changes() -> Subscription<Message> {
    #[cfg(target_os = "linux")]
    return Subscription::run(crate::sys::ui_font::change_signals)
        .map(|()| Message::UiFontSettingsChanged);
    #[cfg(target_os = "windows")]
    return iced::time::every(std::time::Duration::from_secs(2))
        .map(|_| Message::UiFontSettingsChanged);
    #[allow(unreachable_code)]
    Subscription::none()
}

/// Background used by drafting overlays in model or paper space.
fn crosshair_background(tab: &DocumentTab, is_paper: bool) -> [f32; 4] {
    if tab.active_block_edit_session().is_some() {
        return [0.62, 0.62, 0.62, 1.0];
    }
    if !is_paper {
        return tab.scene.bg_color;
    }
    tab.scene.paper_bg_color
}

/// Which navigation tool the viewport cursor should advertise. The three flags
/// are mutually exclusive — every command entry point clears all of them before
/// arming one (`crate::app::commands`) — so the order here only settles a state
/// that cannot occur.
pub(in crate::app) fn nav_cursor(tab: &DocumentTab) -> crate::ui::overlay::NavCursor {
    use crate::ui::overlay::NavCursor;
    if tab.pan_mode {
        NavCursor::Pan
    } else if tab.orbit_mode {
        NavCursor::Orbit
    } else if tab.zoom_dynamic_mode {
        NavCursor::Zoom
    } else {
        NavCursor::None
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

fn shortcut_capture_key_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    // Ignore bare modifier presses so holding Ctrl+Shift before the final
    // key doesn't end the capture early.
    if let keyboard::Key::Named(named) = &key {
        if matches!(
            named,
            keyboard::key::Named::Control
                | keyboard::key::Named::Shift
                | keyboard::key::Named::Alt
                | keyboard::key::Named::Super
                | keyboard::key::Named::Meta
        ) {
            return None;
        }
    }
    // Esc backs out of capture (and abandons an unfinished draft row) —
    // binding the ESCAPE key itself is still possible via SHORTCUTS SET.
    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
        return Some(Message::ShortcutCaptureCancel);
    }
    shortcut_key_name(&key, modifiers).map(Message::ShortcutCaptureKey)
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
                "Enter" | "Space" | "Escape" | "Delete" | "Backspace" | "Tab" | "Home" | "End"
                | "Insert" => name.to_uppercase(),
                _ if name.starts_with('F') && name[1..].chars().all(|ch| ch.is_ascii_digit()) => {
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
pub(super) struct RenderModeChoice(pub codec::entities::ViewportRenderMode);

impl std::fmt::Display for RenderModeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::t!(crate::modules::view::visual_style::label_for(self.0)).as_ref())
    }
}

/// A named style (Realistic, …) the active viewport draws as, by name.
fn named_visual_style(
    scene: &crate::scene::Scene,
    mode: codec::entities::ViewportRenderMode,
) -> Option<String> {
    match scene.document.objects.get(&scene.active_visual_style()) {
        Some(codec::objects::ObjectType::VisualStyle(style)) => {
            match crate::modules::view::visual_style::mode_for_style_name(&style.description) {
                Some((named_mode, true)) if named_mode == mode => {
                    Some(crate::t!(style.description.as_str()).into_owned())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

impl OpenCADStudio {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        // ── Floating panel windows ─────────────────────────────────────────
        // All dialogs are in-canvas modals now (Plan B); view_main stacks the
        // active one. `window_id` is unused — there is only the main window.
        let _ = window_id;
        // Pan latency on a large drawing is ~120 ms while `update`, `prepare`
        // and `encode` together account for ~1 ms of it, the GPU sits at 0-8%,
        // and encoding no scene at all changes nothing. Widget-tree
        // construction is the largest span between the input handler returning
        // and `prepare` running, and it was never measured.
        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let element = self.view_main();
        if let Some(started) = started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 1.0 {
                crate::perf_record!("[perf] view {:>7.1}ms", elapsed_ms);
            }
        }
        crate::perf::note_view_finish();
        element
    }

    /// The primary window: viewport, ribbon, tab bar, status bar. Split out of
    /// `view` so the single-window web build can render it directly, bypassing
    /// the multi-window id dispatch above (the web build has no extra windows).
    pub fn view_main(&self) -> Element<'_, Message> {
        // Section marks for `view-detail`. A closure over a RefCell rather
        // than one binding per section: two of these sections sit inside a
        // nested block and would otherwise be out of scope at the report.
        let view_t0 = crate::perf::enabled().then(iced::time::Instant::now);
        let view_marks: std::cell::RefCell<Vec<(&'static str, f64)>> =
            std::cell::RefCell::new(Vec::with_capacity(8));
        let mark = |name: &'static str| {
            if let Some(t0) = view_t0 {
                view_marks
                    .borrow_mut()
                    .push((name, t0.elapsed().as_secs_f64() * 1000.0));
            }
        };

        let i = self.active_tab;
        let tab = &self.tabs[i];
        // Dynamic input is built before the status bar and properties panel,
        // so seed its shared formatter here rather than relying on either of
        // those later views to have done it for this drawing/thread.
        crate::entities::common::set_unit_context(
            crate::entities::common::UnitContext::from_header(&tab.scene.document.header),
        );
        let thumbnail_capture_clean = self.thumbnail_capture_clean;
        let theme_text = self.active_theme.palette().background.base.text;
        let viewcube_text_color = [theme_text.r, theme_text.g, theme_text.b, theme_text.a];
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
                    let (cw, ch) = tab.scene.selection.borrow().view.vp_size;
                    tab.scene.viewport_screen_rect(h, (cw, ch))
                })
                .map(|r| r.width)
                .unwrap_or(f32::INFINITY)
        } else {
            let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
            tab.scene.active_model_tile_bounds(vw, vh).width
        };
        let viewcube_visible = self.show_viewcube
            && !thumbnail_capture_clean
            && !tab.is_start
            && viewcube_has_room(render_bar_w, active_vp_w);
        // Start tab: render welcome page in place of the viewport.
        // Surrounding chrome (tab bar, status bar) stays; the welcome widget
        // returned here also flags the rest of `view` to skip drawing-only
        // overlays via `tab.is_start`.
        // Unified GPU widget for both layouts. A paper layout renders through
        // the same shader as model space: a full-canvas top-locked "sheet"
        // viewport draws the layout's own geometry (white sheet + entities +
        // borders) and the floating content viewports blit on top.
        mark("viewport_3d");
        let viewport_3d: Element<'_, Message> = if self.layout_settling && !tab.is_start {
            // Omit the shader and drafting overlays for the notice frame.
            let in_block_editor = tab.active_block_edit_session().is_some();
            let bg = if in_block_editor {
                [0.62, 0.62, 0.62, 1.0]
            } else {
                crosshair_background(tab, is_paper)
            };
            let lum = 0.299 * bg[0] + 0.587 * bg[1] + 0.114 * bg[2];
            let ink = if lum > 0.5 {
                Color::from_rgba(0.0, 0.0, 0.0, 0.55)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.55)
            };
            container(
                text(crate::tr!("viewport", "preparing-layout"))
                    .size(15)
                    .color(ink),
            )
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .style(move |_| container::Style {
                background: Some(Color::from_rgba(bg[0], bg[1], bg[2], bg[3]).into()),
                ..container::Style::default()
            })
            .into()
        } else if tab.is_start {
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
                self.recent_view,
                self.start_action_w.clone(),
                self.start_section,
            )
        } else if is_paper {
            shader(ViewportPane::model(
                &tab.scene,
                viewcube_visible,
                !thumbnail_capture_clean,
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
            let show_interaction = !thumbnail_capture_clean;
            let render_mode = viewport_render_mode;
            let size_probe: Element<'_, Message> = responsive(move |size| {
                {
                    let mut sel = scene.selection.borrow_mut();
                    sel.view.vp_size = (size.width, size.height);
                }
                scene.sync_tiles_from_panes(size.width, size.height);
                Space::new().width(Fill).height(Fill)
            })
            .into();
            let shaders =
                pane_grid::PaneGrid::new(&scene.model_panes, move |_pane, &idx, _maximized| {
                    pane_grid::Content::new(
                        shader(ViewportPane::for_pane(
                            scene,
                            show_viewcube,
                            show_interaction,
                            render_mode,
                            idx,
                            viewcube_text_color,
                        ))
                        .width(Fill)
                        .height(Fill),
                    )
                })
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
        mark("model_input");
        let model_input_layer: Option<Element<'_, Message>> =
            if is_paper || tab.is_start || self.layout_settling {
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

        mark("grid");
        let grid_overlay = if self.layout_settling || tab.is_start {
            Space::new().into()
        } else {
            let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
            let model_basis = {
                let (o, ux, uy, uz) = tab.ucs_xform().axes();
                let (ux, uy, uz) = super::helpers::drafting_axes(
                    ux,
                    uy,
                    uz,
                    self.isometric_drafting,
                    self.iso_plane,
                    self.snap_angle_deg,
                );
                (o, (ux.as_vec3(), uy.as_vec3(), uz.as_vec3()))
            };
            // The first paper frame spends seconds in this block and `grid`
            // alone cannot say which part. Reported only when it costs
            // something, so ordinary frames stay quiet.
            let t_grid = crate::perf::enabled().then(iced::time::Instant::now);
            let views = tab.scene.grid_views(vw, vh);
            let views_ms = crate::perf::elapsed_ms(t_grid);
            let view_count = views.len();
            let t_params = crate::perf::enabled().then(iced::time::Instant::now);
            let grid: Vec<crate::ui::overlay::GridParams> = views
                .into_iter()
                .map(|(bounds, cam, handle)| {
                    let (origin, mut axes): (glam::DVec3, _) = if is_paper {
                        match tab.ucs_from_viewport(handle) {
                            Some(u) => {
                                let (o, ux, uy, uz) = super::helpers::UcsXform::from_ucs(&u).axes();
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
                    if is_paper {
                        let (ux, uy, uz) = super::helpers::drafting_axes(
                            axes.0.as_dvec3(),
                            axes.1.as_dvec3(),
                            axes.2.as_dvec3(),
                            self.isometric_drafting,
                            self.iso_plane,
                            self.snap_angle_deg,
                        );
                        axes = (ux.as_vec3(), uy.as_vec3(), uz.as_vec3());
                    }
                    // BUG FIX: the display step used to ignore GRIDUNIT entirely
                    // (hardcoded 1.0 base). It now resizes from the DSettings
                    // grid spacing, and honors "Display grid beyond Limits".
                    let (step_x, step_y) = crate::ui::overlay::compute_grid_steps(
                        self.grid_spacing_x,
                        self.grid_spacing_y,
                        cam.distance,
                        cam.fov_y,
                        bounds,
                        self.grid_adaptive,
                    );
                    let limits = if self.grid_beyond_limits {
                        None
                    } else {
                        tab.scene.grid_limits_for_viewport(handle)
                    };
                    crate::ui::overlay::GridParams {
                        view_rot: cam.view_proj_rte(bounds),
                        eye: cam.eye(),
                        bounds,
                        step_x,
                        step_y,
                        major_every: self.grid_major_every,
                        origin,
                        axes,
                        limits,
                    }
                })
                .collect();
            let params_ms = crate::perf::elapsed_ms(t_params);
            let t_bg = crate::perf::enabled().then(iced::time::Instant::now);
            let in_block_editor = tab.active_block_edit_session().is_some();
            let bg = if in_block_editor {
                [0.62, 0.62, 0.62, 1.0]
            } else {
                crosshair_background(tab, is_paper)
            };
            let bg_ms = crate::perf::elapsed_ms(t_bg);
            if views_ms + params_ms + bg_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] grid-detail views={views_ms:.1}ms params={params_ms:.1}ms \
bg={bg_ms:.1}ms n={view_count}"
                );
            }
            let bg_lum = 0.299 * bg[0] + 0.587 * bg[1] + 0.114 * bg[2];
            let grid_style = crate::ui::overlay::GridStyle {
                opacity: if in_block_editor { 38 } else { self.model_space.grid_opacity },
                bg_luminance: bg_lum,
                color: if in_block_editor { [0.27, 0.48, 0.91] } else { [0.80, 0.80, 0.80] },
                axes: self.grid_axes,
            };
            crate::ui::overlay::grid_overlay(grid, grid_style)
        };

        mark("selection_overlay");
        let selection_overlay = if self.layout_settling || tab.is_start {
            Space::new().into()
        } else {
            // Hold a single `Ref<'_, SelectionState>` for the whole overlay
            // block. The `vp_size` and `last_move_pos` reads below become
            // field accesses on `sel_ref` (no redundant `borrow()`s), and
            // the final widget call takes `Arc::clone(&selection)` — atomic
            // bump per frame instead of a deep `SelectionState` clone.
            // `sel_ref` is a borrowed view for field reads; the widget owns
            // the Arc clone. No `selection` mutation occurs inside this block.
            let sel_ref = tab.scene.selection.borrow();
            let snap_info = tab.snap_result.map(|s| (s.screen, s.snap_type));
            let snap_ext_base = tab.snap_result.and_then(|s| s.extension_base);
            let snap_ext_base2 = tab.snap_result.and_then(|s| s.extension_base2);

            let show_grips = tab.active_cmd.is_none()
                || tab
                    .active_cmd
                    .as_ref()
                    .is_some_and(|cmd| cmd.name() == "STRETCH");

            let grips: Vec<crate::ui::overlay::GripMarker> = if show_grips
                && !tab.selected_grips.is_empty()
            {
                let (vw, vh) = sel_ref.view.vp_size;
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
                // Mark the Properties panel's current point grip hot.
                let current_vertex_grip: Option<usize> = tab
                    .properties
                    .prop_vertex_indicator_active
                    .then(|| sel_h)
                    .flatten()
                    .and_then(|h| {
                        let indexed = match tab.scene.document.get_entity(h) {
                            Some(codec::EntityType::LwPolyline(_))
                            | Some(codec::EntityType::Polyline2D(_))
                            | Some(codec::EntityType::Polyline3D(_))
                            | Some(codec::EntityType::Spline(_))
                            | Some(codec::EntityType::Face3D(_))
                            | Some(codec::EntityType::PolygonMesh(_)) => true,
                            _ => false,
                        };
                        indexed.then_some(tab.properties.prop_vertex)
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
                                || (Some(handle) == sel_h && Some(grip_id) == current_vertex_grip)
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
                            is_plugin: false,
                            is_authoring: crate::app::block_authoring_edit::is_editor_grip(grip_id),
                            is_dynamic: crate::app::block_actions::is_test_grip(grip_id)
                                || grip_id == crate::app::visibility::VIS_GRIP_ID,
                        }
                    })
                    .collect()
            } else {
                vec![]
            };
            // API v11: plugin-declared custom grips render in the same overlay
            // layer with a distinct accent (always-visible display state of a
            // live declaring plugin; hover highlighting through the same
            // pipeline; the dragged grip is hot). Culling is the per-tab
            // uniform grid's view-rect query, not a scan of every grip.
            #[cfg(not(target_arch = "wasm32"))]
            let grips = {
                let mut grips = grips;
                if !tab.plugin_grips.is_empty() {
                    for (candidate, screen) in self.plugin_grips_in_view(i) {
                        grips.push(crate::ui::overlay::GripMarker {
                            pos: screen,
                            shape: candidate.shape,
                            is_hot: tab.active_plugin_grip.as_ref().is_some_and(|active| {
                                active.plugin == candidate.plugin
                                    && active.grip_id == candidate.grip_id
                            }),
                            is_hovered: self.plugin_grip_hover.as_ref().is_some_and(|hover| {
                                hover.plugin == candidate.plugin
                                    && hover.grip_id == candidate.grip_id
                            }),
                            dir: None,
                            is_plugin: true,
                            is_authoring: false,
                            is_dynamic: false,
                        });
                    }
                }
                grips
            };
            let control_polygon = tab.selected_handle.and_then(|handle| {
                let spline = match tab.scene.document.get_entity(handle) {
                    Some(codec::EntityType::Spline(spline))
                        if crate::entities::spline::shows_control_vertices(spline) =>
                    {
                        spline
                    }
                    _ => return None,
                };
                if tab
                    .selected_grip_handles
                    .iter()
                    .any(|owner| *owner != handle)
                {
                    return None;
                }
                let points: Vec<_> = grips
                    .iter()
                    .filter(|grip| grip.shape == crate::scene::model::object::GripShape::Circle)
                    .map(|grip| grip.pos)
                    .collect();
                (points.len() >= 2)
                    .then_some((points, spline.flags.closed || spline.flags.periodic))
            });
            let grip_clip = if grips.is_empty() {
                None
            } else {
                let (vw, vh) = sel_ref.view.vp_size;
                Some(
                    tab.scene
                        .viewport_edit_frame((vw, vh))
                        .map(|(_, bounds)| bounds)
                        .unwrap_or_else(|| tab.scene.active_model_tile_bounds(vw, vh)),
                )
            };

            let (vw, vh) = sel_ref.view.vp_size;
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
            let ucs_icons: Vec<crate::ui::overlay::UcsIconParams> = if !self.show_ucs_icon
                || tab.active_block_edit_session().is_some()
            {
                vec![]
            } else if let Some((vp_cam, full)) = tab.scene.viewport_edit_frame((vw, vh)) {
                let (_, ux, uy, uz) = tab.ucs_xform().axes();
                let origin_screen = self
                    .ucs_icon_at_origin
                    .then(|| {
                        vp_cam
                            .project(tab.ucs_origin_world(), full)
                            .map(|p| iced::Point::new(full.x + p.x, full.y + p.y))
                    })
                    .flatten();
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
                        let cam = if i == active {
                            live.clone()
                        } else {
                            t.camera.clone()
                        };
                        let origin_screen = self
                            .ucs_icon_at_origin
                            .then(|| {
                                cam.project(origin_w, b)
                                    .map(|p| iced::Point::new(b.x + p.x, b.y + p.y))
                            })
                            .flatten();
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
            let drafting_alignment_active = self.snapper.alignment_active()
                || (tab.active_grip.is_some() && (self.polar_mode || self.ortho_mode));

            let otrack_proj: Option<(glam::Mat4, glam::DVec3, iced::Rectangle)> =
                if drafting_alignment_active {
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
            // Project a *ray* using a screen-space baseline long enough to
            // survive f32 pixel rounding. Projecting base + one world unit can
            // produce a subpixel delta at low zoom: normalizing that delta in
            // the overlay magnifies the rounding error into a visibly wandering
            // guide, even when the acquired world-space ray is fixed.
            let ost_project_ray = |origin: glam::DVec3,
                                   dir: glam::DVec3,
                                   view_rot: glam::Mat4,
                                   eye: glam::DVec3,
                                   ob: iced::Rectangle|
             -> Option<(iced::Point, iced::Point)> {
                let b = ost_project(origin, view_rot, eye, ob);
                if !b.x.is_finite() || !b.y.is_finite() {
                    return None;
                }
                let unit = dir.try_normalize()?;
                let mut distance = 1.0_f64;
                for _ in 0..24 {
                    let tip = ost_project(origin + unit * distance, view_rot, eye, ob);
                    let length = (tip.x - b.x).hypot(tip.y - b.y);
                    if length.is_finite() && length >= 64.0 {
                        return Some((b, tip));
                    }
                    distance *= 2.0;
                }
                None
            };
            // Draw the Extension start at the actual acquired *world* point,
            // projected through the same camera/rect as the entity and guides.
            // The snap engine's screen-space base is only pane-local and can
            // differ from the final canvas coordinates used by the overlay.
            // Keep that older value as a fallback for snaps without origin.
            let snap_ext_base = tab
                .snap_result
                .filter(|hit| hit.snap_type == crate::snap::SnapType::Extension)
                .and_then(|hit| hit.extension_origin)
                .and_then(|origin| {
                    let (rot, eye, rect) = otrack_proj?;
                    let pixel = ost_project(origin, rot, eye, rect);
                    (pixel.x.is_finite() && pixel.y.is_finite()).then_some(pixel)
                })
                .or(snap_ext_base);
            // OSNAP Extension is also active with F11 / OTRACK turned off.
            // Give its marker the actual acquired geometric direction instead
            // of reconstructing a new ray from the moving snap cursor.
            let snap_ext_dir: Option<iced::Point> = tab
                .snap_result
                .filter(|hit| hit.snap_type == crate::snap::SnapType::Extension)
                .and_then(|hit| hit.extension_origin.zip(hit.extension_dir))
                .and_then(|(origin, dir)| {
                    let (rot, eye, rect) = otrack_proj?;
                    let (base, tip) = ost_project_ray(origin, dir, rot, eye, rect)?;
                    Some(iced::Point::new(tip.x - base.x, tip.y - base.y))
                });
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
            // An intersection lock is the meeting of two tracking vectors, so
            // both are drawn — one guide alone hides what the point is (#1313).
            let otrack_lines: Vec<(iced::Point, iced::Point)> = match otrack_proj {
                Some((view_rot, eye, ob)) => {
                    self.otrack_active
                        .into_iter()
                        .chain(self.otrack_cross)
                        .filter_map(|(base, dir)| {
                            // Draw the acquired geometrical ray, not the line
                            // from its origin to the moving cursor. The latter
                            // rotates slightly as the pointer moves away from
                            // the exact ray, making the Extension guide appear
                            // to slide across its acquired endpoint.
                            ost_project_ray(base, dir, view_rot, eye, ob)
                        })
                        .collect()
                }
                None => vec![],
            };
            // The acquired Parallel-snap reference, marked on its line (#277).
            let parallel_ref_marker: Option<iced::Point> =
                match (otrack_proj, self.snapper.parallel_ref) {
                    (Some((view_rot, eye, ob)), Some((_, pt))) => {
                        let s = ost_project(pt, view_rot, eye, ob);
                        (s.x.is_finite() && s.y.is_finite()).then_some(s)
                    }
                    _ => None,
                };

            // Model-space pane dividers (none in paper / single-pane layouts).
            let dividers = if !is_paper {
                let (vw, vh) = sel_ref.view.vp_size;
                tab.scene.model_pane_dividers(vw, vh)
            } else {
                vec![]
            };

            // Pane move (drag-to-swap) visuals: source pane rect + the drop
            // target pane under the cursor.
            let (pane_move_rect, pane_drop_rect) = match self.pane_move_from {
                Some(from) if !is_paper => {
                    let (vw, vh) = sel_ref.view.vp_size;
                    let cursor = sel_ref.input.last_move_pos;
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
            // Keep the hollow pickbox at rest. Use the standard LINE/RECTANG
            // dot crosshair only after the first window/crossing corner.
            // No OSNAP or selection behavior changes.
            let selecting_window = tab.active_cmd.is_none()
                && tab.scene.selection.borrow().gesture.box_anchor.is_some();
            let point_cursor = selecting_window
                || tab
                    .active_cmd
                    .as_ref()
                    .is_some_and(|cmd| !cmd.needs_entity_pick() && !cmd.is_selection_gathering());
            let constraint_cursor_badge = if is_paper {
                None
            } else {
                tab.scene.hover_highlight.and_then(|handle| {
                    tab.scene
                        .parametric_constraint_set(tab.current_parametric_scope())
                        .and_then(|set| {
                            set.constraints_touching(handle)
                                .find(|constraint| {
                                    constraint.kind
                                        == crate::scene::parametric_constraints::ConstraintKind::Concentric
                                })
                                .or_else(|| set.constraints_touching(handle).next())
                        })
                        .map(|constraint| constraint.kind.glyph_symbol().to_string())
                })
            };
            let constraint_glyphs: std::sync::Arc<
                [crate::scene::parametric_constraints::GlyphEntry],
            > = if is_paper {
                std::sync::Arc::from([])
            } else {
                let scope = tab.current_parametric_scope();
                tab.scene.cached_glyph_placements(
                    scope,
                    sel_ref.view.vp_size,
                    self.show_constraint_values,
                    self.constraint_bar_display,
                    self.constraint_bar_mode,
                )
            };
            // Selection stays OUTSIDE the cache (applied post-hoc): a selection
            // change must not invalidate the placement memo.
            let constraint_glyph_selected: std::sync::Arc<[bool]> = constraint_glyphs
                .iter()
                .map(|entry| tab.scene.selected_constraint == Some(entry.id))
                .collect();
            let authoring_visibility = tab.active_block_edit_session().and_then(|session| {
                let parameter = crate::scene::block_visibility::parameter(
                    &tab.scene.document,
                    session.br_handle,
                )?;
                let native_point = crate::scene::block_grips::config::locations(&tab.scene.document, session.br_handle, parameter.handle).ok()
                    .and_then(|grips| grips.into_iter().next()).map(|grip| grip.point).unwrap_or(parameter.def_point);
                let location = glam::DVec3::new(native_point.x, native_point.y, native_point.z);
                let point = tab.scene.camera.borrow().project(location, vp_bounds)?;
                let point = iced::Point::new(vp_bounds.x + point.x, vp_bounds.y + point.y);
                (point.x.is_finite() && point.y.is_finite()).then(|| {
                    (
                        point,
                        parameter.name.clone(),
                        vp_bounds,
                        tab.selected_authoring == Some(parameter.handle),
                    )
                })
            });
            let authoring_points =
                tab.active_block_edit_session()
                    .map(|session| {
                        crate::scene::block_point_parameter::parameters(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .into_iter()
                        .filter_map(|(handle, parameter)| {
                            let project = |point: codec::types::Vector3| {
                                let point = tab.scene.camera.borrow().project(
                                    glam::DVec3::new(point.x, point.y, point.z),
                                    vp_bounds,
                                )?;
                                (point.x.is_finite() && point.y.is_finite()).then_some(
                                    iced::Point::new(vp_bounds.x + point.x, vp_bounds.y + point.y),
                                )
                            };
                            Some((
                                project(parameter.parameter.definition_point)?,
                                project(parameter.definition_label_point)?,
                                parameter.position_name.clone(),
                                vp_bounds,
                                tab.selected_authoring == Some(handle),
                            ))
                        })
                        .collect()
                    })
                    .unwrap_or_default();
            let authoring_dimensions = tab
                .active_block_edit_session()
                .map(|session| {
                    let action_handles: std::collections::HashSet<_> =
                        crate::scene::block_actions::actions(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .into_iter()
                        .map(|(handle, _, _, _)| handle)
                        .collect();
                    crate::scene::block_distance_parameter::parameters(
                        &tab.scene.document,
                        session.br_handle,
                    )
                    .iter()
                        .map(|p| {
                        (
                                p.handle,
                            crate::scene::block_distance_parameter::graphic(p),
                            p.native.definition_base_point,
                        )
                    })
                    .chain(crate::scene::block_point_parameter::parameters(&tab.scene.document, session.br_handle).into_iter().map(|(handle, p)| {
                        (handle, crate::scene::block_distance_parameter::Graphic::default(), p.parameter.definition_point)
                    }))
                    .chain(
                        crate::scene::block_orientation_parameter::parameters(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .iter()
                        .map(|p| {
                            (
                                p.handle,
                                crate::scene::block_orientation_parameter::graphic(p),
                                crate::scene::block_orientation_parameter::marker_tail(p),
                            )
                        }),
                    )
                    .chain(
                        crate::scene::block_basepoint::parameter(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .map(|(handle, p)| (handle, p.parameter.definition_point))
                        .or_else(|| {
                            let name = tab
                                .scene
                                .document
                                .block_records
                                .iter()
                                .find(|record| record.handle == session.br_handle)?
                                .name
                                .as_str();
                            Some((
                                session.br_handle,
                                crate::scene::render_graph::block_base_point(
                                    &tab.scene.document,
                                    name,
                                ),
                            ))
                        })
                        .map(|(handle, point)| {
                            let mut g = crate::scene::block_distance_parameter::Graphic::default();
                            g.base_grips.push(point);
                            g.labels.push((point, "Basepoint".into()));
                            (handle, g, point)
                        }),
                    )
                    .chain(
                        crate::scene::block_lookup::parameters(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .into_iter()
                        .map(|(handle, p)| {
                            let mut g = crate::scene::block_distance_parameter::Graphic::default();
                            g.lookup_grips.push(p.parameter.definition_point);
                            g.labels
                                .push((p.parameter.definition_point, p.lookup_name.clone()));
                            (handle, g, p.parameter.definition_point)
                        }),
                    )
                    .chain(crate::scene::block_authoring::edit::items(&tab.scene.document, session.br_handle).into_iter().filter_map(|item| {
                        let native = tab.scene.document.objects.get(&item.handle)?;
                        if matches!(native, codec::objects::ObjectType::DynamicBlock(object) if matches!(object.data, codec::objects::DynamicBlockData::PropertiesTable(_))) {
                            Some((item.handle, item.graphic, item.grips.first()?.1))
                        } else { None }
                    }))
                    .chain(
                        crate::scene::block_actions::actions(
                            &tab.scene.document,
                            session.br_handle,
                        )
                        .into_iter()
                        .map(|(handle, action, _, _)| {
                            let mut g = crate::scene::block_distance_parameter::Graphic::default();
                            let frame = crate::scene::block_actions::frame_outline(
                                &tab.scene.document,
                                handle,
                            );
                            g.lines.extend(
                                frame
                                    .iter()
                                    .zip(frame.iter().cycle().skip(1))
                                    .take(frame.len())
                                    .map(|(a, b)| [*a, *b]),
                            );
                            if !self.block_editor_preferences.action_bars {
                                g.grips.push((action.display_location, false));
                                g.labels.push((action.display_location, action.element.name.clone()));
                            } else if tab.selected_authoring!=Some(handle) {
                                g.lines.clear();
                            }
                            (handle, g, action.display_location)
                        }),
                    )
                    .filter_map(|(handle, mut graphic, marker_tail)| {
                        let native_grips = crate::scene::block_grips::config::apply_graphic(&tab.scene.document, session.br_handle, handle, &mut graphic);
                        let project = |p: codec::types::Vector3| {
                            let p = tab
                                .scene
                                .camera
                                .borrow()
                                .project(glam::DVec3::new(p.x, p.y, p.z), vp_bounds)?;
                            (p.x.is_finite() && p.y.is_finite())
                                .then_some(iced::Point::new(vp_bounds.x + p.x, vp_bounds.y + p.y))
                        };
                        let lines = graphic
                            .lines
                            .into_iter()
                            .map(|[a, b]| Some([project(a)?, project(b)?]))
                            .collect::<Option<Vec<_>>>()?;
                        let labels = graphic
                            .labels
                            .into_iter()
                            .map(|(point, label)| Some((project(point)?, label)))
                            .collect::<Option<Vec<_>>>()?;
                        let grips = graphic
                            .grips
                            .into_iter()
                            .map(|(point, oriented)| {
                                Some((
                                    project(point)?,
                                    if oriented {
                                        let tail = native_grips.as_ref().and_then(|grips| grips.iter().find(|grip| grip.point == point))
                                            .and_then(|grip| grip.direction.map(|direction| point - direction)).unwrap_or(marker_tail);
                                        Some(project(tail)?)
                                    } else {
                                        None
                                    },
                                ))
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(crate::ui::overlay::AuthoringDimension {
                            selected: tab.selected_authoring == Some(handle),
                            is_action: action_handles.contains(&handle),
                            lines,
                            base_grips: graphic
                                .base_grips
                                .into_iter()
                                .map(project)
                                .collect::<Option<Vec<_>>>()?,
                            lookup_grips: graphic
                                .lookup_grips
                                .into_iter()
                                .map(project)
                                .collect::<Option<Vec<_>>>()?,
                            round_grips: graphic
                                .round_grips
                                .into_iter()
                                .map(project)
                                .collect::<Option<Vec<_>>>()?,
                            dimensions: graphic
                                .dimensions
                                .into_iter()
                                .map(|[a, b]| Some([project(a)?, project(b)?]))
                                .collect::<Option<Vec<_>>>()?,
                            labels,
                            grips,
                            clip: vp_bounds,
                        })
                    })
                    .collect()
                })
                .unwrap_or_default();
            crate::ui::overlay::selection_overlay(
                std::sync::Arc::clone(&tab.scene.selection),
                snap_info,
                snap_ext_base,
                snap_ext_base2,
                snap_ext_dir,
                grips,
                control_polygon,
                grip_clip,
                ucs_icons,
                ost_points,
                otrack_lines,
                parallel_ref_marker,
                // ViewCube hover region matches the drawn cube — gone when hidden.
                !is_paper && viewcube_visible,
                dividers,
                pane_move_rect,
                pane_drop_rect,
                nav_cursor(tab),
                self.ribbon.open_dropdown.is_some(),
                hover_locked,
                crosshair_background(tab, is_paper),
                crate::ui::overlay::CrosshairOptions {
                    size_percent: self.cursor_size,
                    pick_box: self.pick_box,
                    cursor_type: self.cursor_type,
                    point_mode: point_cursor,
                    color: self.crosshair_color,
                    isometric: self.isometric_drafting,
                    iso_plane: self.iso_plane,
                    snap_angle_deg: self.snap_angle_deg,
                    pick_pending: self.pending_pick_label().is_some(),
                    matchprop_source_pick: tab
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.name() == "MATCHPROP" && c.needs_entity_pick()),
                    matchprop_pickbox: tab
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.name() == "MATCHPROP"),
                    // A command asking for objects shows the pickbox alone, so
                    // picking reads apart from both idle and point input (#1661).
                    hide_arms: tab
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.needs_entity_pick() || c.is_selection_gathering()),
                    // An infinite-line preview runs along the arms; keep them
                    // short so the preview stays visible past them (#1437)
                    // without hiding the cursor itself.
                    short_arms: tab
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| matches!(c.name(), "XLINE" | "RAY")),
                    snap_color: self.snap_marker_color,
                    snap_size: self.model_space.snap_marker_size as f32,
                },
                // MATCHPROP paints properties onto destinations; it gets the
                // paintbrush cursor instead of the crosshair.
                tab.active_cmd
                    .as_ref()
                    .is_some_and(|c| c.name() == "MATCHPROP"),
                crate::ui::overlay::SelectionVisualOptions {
                    area: self.model_space.selection_area,
                    opacity: self.model_space.selection_opacity,
                    window_color: self.model_space.selection_window_color,
                    crossing_color: self.model_space.selection_crossing_color,
                    highlight_color: self.model_space.selection_highlight_color,
                    grip_size: self.model_space.grip_size as f32,
                    grip_color: self.model_space.grip_color,
                    grip_hot: self.model_space.grip_hot,
                    grip_hover: self.model_space.grip_hover,
                    grip_dynamic: self.block_editor_preferences.dynamic_grip_color,
                },
                constraint_glyphs,
                constraint_glyph_selected,
                self.constraint_glyph_tooltip
                    .map(|kind| crate::t!(kind.label()).into_owned()),
                constraint_cursor_badge,
                authoring_visibility,
                authoring_points,
                authoring_dimensions,
                crate::ui::overlay::authoring::AuthoringVisualOptions::new(
                    &self.block_editor_preferences,
                    &tab.scene.document,
                    crosshair_background(tab, is_paper),
                ),
                self.block_feedback(vp_bounds),
            )
        };

        mark("viewport_mouse");
        let viewport_mouse = mouse_area(container(
            iced::widget::canvas(ViewportPickSurface)
                .width(Fill)
                .height(Fill),
        ))
        // Moves come from the canvas (mouse and touch) in event order.
        // Left mouse presses are captured by the canvas with their exact
        // event-time position. MouseArea::on_press has no coordinate, so
        // queued cursor moves could otherwise make quick clicks use a stale
        // snap position. Touch presses still use this fallback.
        .on_press(Message::ViewportLeftPress)
        .on_release(Message::ViewportLeftRelease)
        .on_right_press(Message::ViewportRightPress)
        .on_right_release(Message::ViewportRightRelease)
        .on_middle_press(Message::ViewportMiddlePress)
        .on_middle_release(Message::ViewportMiddleRelease)
        .on_scroll(Message::ViewportScroll)
        .on_exit(Message::ViewportExit);

        let desk_bg = self.model_space.resolve_desk_bg();
        let desk_color = Color {
            r: desk_bg[0],
            g: desk_bg[1],
            b: desk_bg[2],
            a: desk_bg[3],
        };
        let bg_color = if tab.active_block_edit_session().is_some() {
            Color {
                r: 0.62,
                g: 0.62,
                b: 0.62,
                a: 1.0,
            }
        } else if is_paper {
            desk_color
        } else {
            let c = tab.scene.bg_color;
            Color {
                r: c[0],
                g: c[1],
                b: c[2],
                a: c[3],
            }
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
        let dyn_input_overlay: Option<Element<'_, Message>> = if self.dyn_input
            && (tab.active_cmd.is_some() || tab.active_grip.is_some())
            && (!tab.dyn_fields.is_empty() || dyn_picks_object)
        {
            let w = tab.last_cursor_world;
            let base = tab.dyn_anchor.or(self.last_point);
            // A single distance field on an acquired tracking ray is measured
            // from the acquired OSNAP/OTRACK point, not from the command's
            // previous point (or from WCS zero before the first pick).
            // Keep typed fields and ordinary polar distance inputs unchanged.
            let tracking_distance = if tab.dyn_fields.len() == 1
                && tab.dyn_fields[0].component == DynComponent::Distance
            {
                self.active_distance_ray(self.active_tab)
                    .and_then(|(origin, direction)| {
                        let axis = direction.try_normalize()?;
                        Some((w - origin).dot(axis).abs())
                    })
            } else {
                None
            };
            let label_screen = tab
                .active_cmd
                .as_ref()
                .and_then(|command| command.dyn_label_point(w))
                .and_then(|world| {
                    let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
                    let (camera, bounds) =
                        tab.scene.viewport_edit_frame((vw, vh)).unwrap_or_else(|| {
                            (
                                tab.scene.camera.borrow().clone(),
                                tab.scene.active_model_tile_bounds(vw, vh),
                            )
                        });
                    camera.project(world, bounds).and_then(|point| {
                        (point.x.is_finite() && point.y.is_finite())
                            .then(|| iced::Point::new(bounds.x + point.x, bounds.y + point.y))
                    })
                });
            // A command may drive a typed scalar by mouse (e.g. a
            // perpendicular distance to a picked object); show that live
            // value in the box until the user types over it.
            let live = tab
                .active_cmd
                .as_ref()
                .and_then(|c| c.dyn_live_value(w))
                .or_else(|| {
                    let grip = tab.active_grip.as_ref()?;
                    let action = match grip.mode {
                        crate::scene::pick::grip::GripEditMode::Lengthen => {
                            crate::scene::model::object::GripMenuAction::Lengthen
                        }
                        crate::scene::pick::grip::GripEditMode::Radius => {
                            crate::scene::model::object::GripMenuAction::Radius
                        }
                        crate::scene::pick::grip::GripEditMode::ArcLength => {
                            crate::scene::model::object::GripMenuAction::ArcLength
                        }
                        crate::scene::pick::grip::GripEditMode::RectangleWidth => {
                            crate::scene::model::object::GripMenuAction::RectangleWidth
                        }
                        crate::scene::pick::grip::GripEditMode::RectangleHeight => {
                            crate::scene::model::object::GripMenuAction::RectangleHeight
                        }
                        crate::scene::pick::grip::GripEditMode::MoveParallel => {
                            crate::scene::model::object::GripMenuAction::MoveParallel
                        }
                        _ => return None,
                    };
                    let original = self
                        .grip_originals
                        .iter()
                        .find(|(handle, _)| *handle == grip.handle)
                        .map(|(_, entity)| entity)?;
                    crate::scene::view::dispatch::grip_menu_point_value(
                        original,
                        grip.grip_id,
                        action,
                        w,
                    )
                });
            let rectangle_values = tab
                .active_grip
                .as_ref()
                .and_then(|grip| grip.rectangle_frame)
                .map(|(opposite, width_axis, height_axis)| {
                    let delta = w - opposite;
                    (delta.dot(width_axis).abs(), delta.dot(height_axis).abs())
                });
            let rectangle_label_screens = tab
                .active_grip
                .as_ref()
                .and_then(|grip| grip.rectangle_frame)
                .and_then(|(opposite, width_axis, height_axis)| {
                    let delta = w - opposite;
                    let width = delta.dot(width_axis);
                    let height = delta.dot(height_axis);
                    let width_center = opposite + width_axis * (width * 0.5);
                    let height_center =
                        opposite + width_axis * width + height_axis * (height * 0.5);
                    let rectangle_center =
                        opposite + width_axis * (width * 0.5) + height_axis * (height * 0.5);
                    let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
                    let (camera, bounds) =
                        tab.scene.viewport_edit_frame((vw, vh)).unwrap_or_else(|| {
                            (
                                tab.scene.camera.borrow().clone(),
                                tab.scene.active_model_tile_bounds(vw, vh),
                            )
                        });
                    let project = |point| {
                        camera.project(point, bounds).map(|screen| {
                            iced::Point::new(bounds.x + screen.x, bounds.y + screen.y)
                        })
                    };
                    let center = project(rectangle_center)?;
                    let offset_from_center = |side: iced::Point, pixels: f32| {
                        let dx = side.x - center.x;
                        let dy = side.y - center.y;
                        let length = dx.hypot(dy);
                        if length > 1.0e-3 {
                            iced::Point::new(
                                side.x + dx / length * pixels,
                                side.y + dy / length * pixels,
                            )
                        } else {
                            side
                        }
                    };
                    Some((
                        offset_from_center(project(width_center)?, 14.0),
                        offset_from_center(project(height_center)?, 18.0),
                    ))
                });
            let boxes: Vec<crate::ui::overlay::DynBox> = tab
                .dyn_fields
                .iter()
                .enumerate()
                .map(|(idx, f)| {
                    let value = match (&f.buffer, live) {
                        (Some(b), _) => b.clone(),
                        (None, _) if rectangle_values.is_some() => {
                            let (width, height) = rectangle_values.unwrap();
                            match f.role {
                                crate::command::DynRole::Width => {
                                    crate::entities::common::format_length(width)
                                }
                                crate::command::DynRole::Height => {
                                    crate::entities::common::format_length(height)
                                }
                                _ => String::new(),
                            }
                        }
                        // An angle step with a command-supplied live value
                        // (ARC span / direction) shows it in degrees.
                        (None, Some(lv)) if f.component == DynComponent::Angle => {
                            crate::entities::common::format_angle(lv.to_radians())
                        }
                        (None, Some(lv))
                            if matches!(
                                f.component,
                                DynComponent::Scalar | DynComponent::Distance
                            ) =>
                        {
                            if f.component == DynComponent::Distance {
                                crate::entities::common::format_length(lv)
                            } else {
                                format!("{lv:.4}")
                            }
                        }
                        // The displayed live distance must use the same
                        // acquired tracking origin as the guide and typed
                        // distance placement, not the last picked point.
                        (None, _)
                            if f.component == DynComponent::Distance
                                && tracking_distance.is_some() =>
                        {
                            crate::entities::common::format_length(
                                tracking_distance.unwrap() * f.role.value_scale() as f64,
                            )
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
                        center: rectangle_label_screens.and_then(|(width, height)| match f.role {
                            crate::command::DynRole::Width => Some(width),
                            crate::command::DynRole::Height => Some(height),
                            _ => None,
                        }),
                    }
                })
                .collect();
            let prompt = tab
                .active_cmd
                .as_ref()
                .map(|c| c.prompt())
                .unwrap_or_default();

            let tracking_hint = match self.otrack_kind {
                Some(crate::snap::TrackingKind::Perpendicular) => {
                    Some(crate::tr!("common", "perpendicular"))
                }
                Some(crate::snap::TrackingKind::Extension) => {
                    Some(crate::tr!("common", "extension"))
                }
                Some(crate::snap::TrackingKind::PlusZ) => Some("+Z".to_string()),
                Some(crate::snap::TrackingKind::MinusZ) => Some("-Z".to_string()),
                _ => None,
            };

            Some(crate::ui::overlay::dynamic_input_overlay(
                tab.last_cursor_screen,
                tab.last_point_screen,
                tab.dyn_ref_screen,
                label_screen,
                tab.dyn_guide,
                boxes,
                prompt,
                tracking_hint,
            ))
        } else {
            None
        };

        mark("viewport_stack");
        let mut viewport_stack = if tab.is_start || self.layout_settling {
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
        } else if thumbnail_capture_clean {
            let capture_bg = if is_paper { desk_color } else { bg_color };
            stack![
                container(Space::new())
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(capture_bg)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                viewport_3d,
            ]
            .width(Fill)
            .height(Fill)
        } else if is_paper {
            // Keep the grid above the opaque GPU sheet and below interaction UI.
            stack![
                container(Space::new())
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(desk_color)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                viewport_3d,
                grid_overlay,
                selection_overlay,
                viewport_mouse,
            ]
            .width(Fill)
            .height(Fill)
        } else {
            stack![
                container(Space::new())
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(bg_color)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                viewport_3d,
                grid_overlay,
                selection_overlay,
            ]
            .width(Fill)
            .height(Fill)
        };

        if !thumbnail_capture_clean && !self.layout_settling {
            if let Some(pivot) = self.spacemouse_pivot_overlay() {
                viewport_stack = viewport_stack.push(pivot);
            }
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
                let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
                let rect = tab.scene.active_model_tile_bounds(vw, vh);
                // Unified control chip: split buttons + render-mode picker +
                // grid / grid-snap toggles, for the active Model tile.
                let style_name = named_visual_style(&tab.scene, tab.render_mode);
                let bar = viewport_controls(
                    tab.render_mode,
                    self.show_grid,
                    self.snapper.grid_snap(),
                    true,
                    tab.scene.model_tiles.borrow().len(),
                    self.render_mode_menu_open,
                    self.render_mode_preview,
                    style_name,
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
            mark("active_vp_rect");
            let active_vp_rect: Option<(codec::Handle, iced::Rectangle)> =
                if is_paper && !tab.is_start {
                    tab.scene.active_viewport.and_then(|h| {
                        let (cw, ch) = tab.scene.selection.borrow().view.vp_size;
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
                let (cw, ch) = tab.scene.selection.borrow().view.vp_size;
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
                let border_layer = iced::widget::pin(border_frame).position(iced::Point::new(x, y));
                viewport_stack = viewport_stack.push(border_layer);

                let vp_mode = tab
                    .scene
                    .active_viewport_render_mode()
                    .unwrap_or(codec::entities::ViewportRenderMode::Wireframe2D);
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
                    named_visual_style(&tab.scene, vp_mode),
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
                    // Epoch-memoised (#34): Arc clone per frame, rebuilt only
                    // after a UCS-table bump.
                    let ucs_names = tab.scene.cached_ucs_names();
                    let picker = iced::widget::pin(iced::widget::opaque(viewcube_ucs_picker(
                        ucs_current,
                        ucs_names,
                    )))
                    .position(iced::Point::new(
                        cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                        cube_y + VIEWCUBE_HIT_SIZE + 10.0,
                    ));
                    viewport_stack = viewport_stack.push(picker);
                }
            }

            if viewcube_visible && !is_paper {
                // Place the ViewCube hit area in the active model tile's top-right
                // corner so it tracks the active panel in a tiled layout. The hit
                // test in update.rs already maps clicks through the active tile.
                let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
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
                // Epoch-memoised (#34): Arc clone per frame, rebuilt only
                // after a UCS-table bump.
                let ucs_names = tab.scene.cached_ucs_names();
                let picker = iced::widget::pin(iced::widget::opaque(viewcube_ucs_picker(
                    ucs_current,
                    ucs_names,
                )))
                .position(iced::Point::new(
                    cube_x + VIEWCUBE_HIT_SIZE * 0.5 - UCS_PICKER_W * 0.5,
                    cube_y + VIEWCUBE_HIT_SIZE + 10.0,
                ));
                viewport_stack = viewport_stack.push(picker);
            }

            if let Some(dyn_ol) = dyn_input_overlay {
                if !tab.is_start {
                    viewport_stack = viewport_stack.push(dyn_ol);
                }
            }

            let (vw,vh)=tab.scene.selection.borrow().view.vp_size;
            let bar_bounds=tab.scene.active_model_tile_bounds(vw,vh);
            let bar_color=crate::ui::overlay::authoring::AuthoringVisualOptions::new(&self.block_editor_preferences,&tab.scene.document,crosshair_background(tab,is_paper)).action;
            for (group,point,offset,width) in self.action_bar_layout(bar_bounds) {
                let bar=crate::ui::window::block_action_bars::view(group,tab.selected_authoring,offset,width,tab.active_cmd.is_none(),bar_color);
                viewport_stack=viewport_stack.push(position_canvas_overlay(point,bar));
            }
            if let Some(popup) = self.grip_popup.as_ref() {
                if !tab.is_start {
                    let labels: Vec<String> = popup
                        .items
                        .iter()
                        .map(|item| {
                            let (mark, raw) = item
                                .label
                                .strip_prefix("✓ ")
                                .map_or(("", item.label), |raw| ("✓ ", raw));
                            format!("{mark}{}", crate::i18n::translate(raw))
                        })
                        .collect();
                    let max_len = labels
                        .iter()
                        .map(|label| label.chars().count())
                        .max()
                        .unwrap_or(8) as f32;
                    let row_w = max_len * 7.0 + 24.0;
                    let mut col = column![].spacing(0).width(iced::Length::Fixed(row_w));
                    for (idx, label) in labels.into_iter().enumerate() {
                        let is_sel = idx == popup.selected;
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
                    let menu_panel = container(col).padding(2).style(|theme: &Theme| {
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
                            row![container(mark).width(16), text(name).size(12),]
                                .spacing(2)
                                .align_y(iced::Center),
                        )
                        .on_press(Message::VisibilityPick(idx))
                        .padding([3, 10])
                        .width(Fill)
                        .style(move |theme: &Theme, status| {
                            let palette = theme.palette();
                            iced::widget::button::Style {
                                background: matches!(status, iced::widget::button::Status::Hovered)
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
                    let panel =
                        container(iced::widget::scrollable(col).height(iced::Length::Shrink))
                            .height(iced::Length::Fit.max(360.0))
                            .padding(2)
                            .style(|theme: &Theme| {
                                let palette = theme.palette();
                                container::Style {
                                    background: Some(Background::Color(
                                        palette.background.weak.color,
                                    )),
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
            if let Some(popup) = self.table_popup.as_ref() {
                if !tab.is_start {
                    let mut col = column![text(popup.header.clone()).size(12)].spacing(2);
                    for (idx, name) in popup.rows.iter().enumerate() {
                        let mark: Element<'_, Message> = if popup.current == Some(idx) {
                            crate::ui::icons::themed_check_cell(true)
                        } else {
                            Space::new().width(11).into()
                        };
                        col = col.push(
                            button(
                                row![container(mark).width(16), text(name).size(12)]
                                    .spacing(2)
                                    .align_y(iced::Center),
                            )
                            .on_press(Message::TableRowPick(idx))
                            .padding([3, 10])
                            .width(Fill),
                        );
                    }
                    let panel =
                        container(iced::widget::scrollable(col).height(iced::Length::Shrink))
                            .padding(2);
                    let anchor = iced::Point::new(popup.anchor.x + 12.0, popup.anchor.y + 12.0);
                    viewport_stack =
                        viewport_stack.push(position_canvas_overlay(anchor, panel.into()));
                }
            }

            // Selection actions: with only PDF underlays or only xrefs
            // selected, their tools take the right edge (over the paper-space
            // tools, which come back when the selection changes).
            // ponytail: one right-edge toolbar at a time; stack them if both
            // are ever needed together.
            let selection_tools = if tab.is_start {
                None
            } else if self.ribbon.xref_context() {
                crate::ui::side_toolbar::view(&crate::ui::ribbon::xref_tools())
            } else if let Some(shown) = self.ribbon.point_cloud_context() {
                crate::ui::side_toolbar::view_with_active(
                    &crate::ui::ribbon::point_cloud_tools(),
                    &move |id| id == "_PCCROPSHOW" && shown,
                )
            } else if let Some(ctx) = self.ribbon.underlay_context() {
                let ctx = ctx.clone();
                crate::ui::side_toolbar::view_with_active(
                    &crate::ui::ribbon::pdf_underlay_tools(ctx.kind),
                    &move |id| match id {
                        "_PDFULMONO" => ctx.monochrome,
                        "_PDFULSHOW" => ctx.shown,
                        "_PDFULSNAP" => ctx.snap,
                        _ => false,
                    },
                )
            } else {
                None
            };
            if let Some(tb) = selection_tools {
                viewport_stack = viewport_stack.push(tb);
            } else if is_paper && !tab.is_start {
                if let Some(tb) =
                    crate::ui::side_toolbar::view(&crate::modules::layout::paper_space_tools())
                {
                    viewport_stack = viewport_stack.push(tb);
                }
            }

            // In-place block edit (REFEDIT): right-edge toolbar with Save / Discard
            // so the edit can be finished by clicking. (#136)
            if let (Some(session), false) = (tab.refedit_session.as_ref(), tab.is_start) {
                use crate::modules::draw::modify::refedit::{refedit_set_tools, refedit_tools};
                let set_tools = refedit_set_tools();
                let close_tools = refedit_tools();
                let header = crate::ui::side_toolbar::refedit_header(
                    &session.block_name,
                    refedit_working_set_count(session, &tab.scene),
                );
                if let Some(tb) = crate::ui::side_toolbar::view_sections(
                    Some(header),
                    &[&set_tools, &close_tools],
                    &|_| false,
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

            // Count mode: a blue frame round the drawing and the count toolbar.
            if let (Some(mode), false) = (tab.count.as_ref(), tab.is_start) {
                viewport_stack = viewport_stack.push(crate::ui::window::count_palette::border());
                viewport_stack = viewport_stack.push(crate::ui::window::count_palette::toolbar(
                    mode,
                    &tab.scene.document,
                ));
            }

            // Reserve the overlaid command line when placing cursor-anchored panels.
            mark("chrome");
            let command_line_inset = if self.command_line.history_open {
                self.command_line.history_height.clamp(
                    crate::ui::command_line::HISTORY_HEIGHT_MIN,
                    crate::ui::command_line::history_max_height(self.win_size.1),
                ) + 72.0
            } else {
                34.0 + self.command_line.overlay_lines_height()
            };
            if let Some((cursor, tip)) = self.hovered_dynamic_grip_tip() {
                viewport_stack = viewport_stack.push(overlay::grip_tip_overlay(cursor, command_line_inset, tip));
            }
            if !tab.is_start {
                if let Some(readout) = self.measurement_readout(self.active_tab) {
                    viewport_stack = viewport_stack.push(overlay::measurement_overlay(
                        readout.anchor,
                        command_line_inset,
                        readout.text,
                    ));
                }
            }
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
                    t!("No samples yet").into_owned()
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
                    text(t!("PERF"))
                        .size(12)
                        .style(|theme: &Theme| iced::widget::text::Style {
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
                        text(summary)
                            .size(11)
                            .style(|theme: &Theme| iced::widget::text::Style {
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
                    let items: Vec<crate::ui::popup::cycle_popup::CycleCandidate> = cands
                        .iter()
                        .filter_map(|&h| {
                            tab.scene.document.get_entity(h).map(|e| {
                                let style = crate::scene::view::render::render_style_for_viewport(
                                    &tab.scene.document,
                                    e,
                                    None,
                                );
                                crate::ui::popup::cycle_popup::CycleCandidate {
                                    handle: h,
                                    type_name: crate::entities::traits::entity_type_name(e)
                                        .to_string(),
                                    layer: e.common().layer.clone(),
                                    color: style.0,
                                }
                            })
                        })
                        .collect();
                    if !items.is_empty() {
                        viewport_stack = viewport_stack.push(
                            crate::ui::popup::cycle_popup::cycle_popup_overlay(*pt, items),
                        );
                    }
                }
            }

            // Right-click context menu. Lives inside the viewport stack so
            // the cursor position (canvas-relative) anchors the menu under
            // the cursor instead of drifting into window-relative space.
            if !tab.is_start {
                let (ctx_pos, highlighted) = {
                    let sel = tab.scene.selection.borrow();
                    (sel.menu.open_at, sel.menu.ui.highlighted)
                };
                if let Some(p) = ctx_pos {
                    let menu = self.current_context_menu();
                    viewport_stack = viewport_stack.push(viewport_context_menu_overlay(
                        p,
                        command_line_inset,
                        &menu,
                        highlighted,
                    ));
                }
            }

            // In-place MText editor (toolbar + text area), anchored at the
            // insertion-point click.
            if !tab.is_start {
                let canvas = tab.scene.selection.borrow().view.vp_size;
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
        }

        if self.show_node_graph && !tab.is_start {
            let sections = self.graph_all_sections(self.active_tab);
            viewport_stack = viewport_stack.push(tab.graph.view(sections));
        }

        // Docked pallets: each edge's icon strip and shown group (see
        // `app::dock`).
        let left_edge = self.build_edge_stack(crate::app::config::DockSide::Left, tab);
        let right_edge = self.build_edge_stack(crate::app::config::DockSide::Right, tab);

        // Command-line sits as a bottom-centre overlay on top of the
        // viewport stack rather than as a separate row in the main
        // column — frees up vertical space when no command is active
        // and keeps the input close to where the cursor is drawing.
        // Autocomplete shows only when no command or grip-menu action is
        // collecting its own input. A pending Lengthen / Radius / Arc Length
        // value is numeric input, not the prefix of a new command (for example,
        // `3` must not open the 3DALIGN / 3DARRAY suggestions).
        let allow_autocomplete = self.grip_pending.is_none()
            && (tab.active_cmd.is_none() || self.command_line.has_contextual_suggestions());
        // Dynamic input captures keystrokes when its fields are showing,
        // so the command-line field must release focus / its on_input.
        // The MText preview also captures keystrokes (typing edits it), so the
        // command line must likewise release its on_input there.
        let interactive_value_grip = tab.active_grip.as_ref().is_some_and(|grip| {
            matches!(
                grip.mode,
                crate::scene::pick::grip::GripEditMode::Lengthen
                    | crate::scene::pick::grip::GripEditMode::Radius
                    | crate::scene::pick::grip::GripEditMode::ArcLength
                    | crate::scene::pick::grip::GripEditMode::RectangleWidth
                    | crate::scene::pick::grip::GripEditMode::RectangleHeight
                    | crate::scene::pick::grip::GripEditMode::MoveParallel
            )
        });
        let dyn_capturing = (self.dyn_input
            && (tab.active_cmd.is_some() || tab.active_grip.is_some())
            && !tab.dyn_fields.is_empty()
            && !interactive_value_grip)
            || self.mtext_editor.as_ref().is_some_and(|e| e.show_preview)
            || self.text_inline.is_some();
        // The workspace row is: left edge stack, viewport, right edge stack.
        let mut parts: Vec<Element<'_, Message>> = Vec::new();
        if let Some(e) = left_edge {
            parts.push(e);
        }
        parts.push(
            crate::ui::wrap_bar::PosReport::new(VIEWPORT_CAPTURE_BOUNDS_ID, viewport_stack).into(),
        );
        if let Some(e) = right_edge {
            parts.push(e);
        }
        let workspace: Element<'_, Message> = row(parts).width(Fill).height(Fill).into();

        let workspace = self.dock_overlay(workspace, tab);
        let command_line = self.command_line.view(
            allow_autocomplete,
            dyn_capturing,
            &self.history_content,
            self.win_size.1,
            self.control.enabled,
            self.control_busy(),
            self.pending_pick_label().is_some(),
            self.show_node_graph,
        );
        let center_stack: Element<'_, Message> = if thumbnail_capture_clean {
            workspace
        } else if tab.is_start {
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
            // menu bar and status bar stay so commands remain reachable and
            // the mode can be toggled back off.
            let mut col = column![];
            if self.show_menu_bar {
                col = col.push(crate::ui::menu_bar::view());
            }
            if !self.clean_screen && !self.ribbon_closed {
                col = col.push(self.ribbon.view(
                    is_paper,
                    self.tabs[self.active_tab].is_start,
                    self.tabs[self.active_tab].history.undo_stack.len(),
                    self.tabs[self.active_tab].history.redo_stack.len(),
                    self.show_block_palette,
                    self.block_visibility_context(),
                ));
            }
            // Split the `chrome` bucket so live `view-detail` traces show
            // ribbon vs status-bar construction separately (both gated on
            // PERF; zero cost otherwise).
            mark("ribbon");
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
                    // Cached Arcs are moved (refcount bump only), never deep-cloned:
                    // `StatusBar::view` / `StatusMenuData` take ownership of the
                    // `Arc`s and borrow them internally. `current_layout` is the
                    // one true borrow — it anchors to `tab`, which outlives the
                    // call — because a call-site slice over an `Arc` temporary
                    // cannot outlive this block (E0515).
                    let layout_names = tab.scene.cached_layout_names();
                    let scale_list = tab.scene.cached_scale_picker_list();
                    let selection_types = tab.scene.entity_type_names_in_layout();
                    let current_scale_name = tab.scene.displayed_annotation_scale_name();
                    let block_tabs: Vec<String> = tab
                        .block_edits
                        .iter()
                        .map(|session| session.block_name.clone())
                        .collect();
                    let active_block = tab
                        .active_block_edit_session()
                        .map(|session| session.block_name.clone());
                    // The coordinate readout formats through the same helper
                    // the properties panel uses, so it has to see the drawing's
                    // linear-unit settings. Properties happens to set this on
                    // every refresh; the status bar redraws on its own schedule
                    // and cannot rely on that having happened.
                    crate::entities::common::set_unit_context(
                        crate::entities::common::UnitContext::from_header(
                            &tab.scene.document.header,
                        ),
                    );
                    let status_menu_data = crate::ui::statusbar::StatusMenuData {
                        layout_names: Arc::clone(&layout_names),
                        polar_custom_input: &self.polar_custom_input,
                        scale_is_model: is_model,
                        current_scale_name,
                        scale_list,
                        has_selection: !tab.scene.selected.is_empty(),
                        selection_types,
                        selection_filter: &tab.scene.selection_filter,
                        tooltip_hidden: self.status_menu_tooltip_hidden,
                    };
                    self.status_bar.view(
                        &self.snapper,
                        crate::ui::statusbar::StatusBarLayoutState {
                            layouts: layout_names,
                            block_tabs,
                            current_layout: &tab.scene.current_layout,
                            active_block,
                            is_start: tab.is_start,
                            rename_state: self.layout_rename_state.as_ref(),
                            viewport_scale: tab.scene.first_viewport_scale(),
                            viewport_count: tab.scene.viewport_count(),
                            in_mspace: tab.scene.active_viewport.is_some(),
                            show_layout_tabs: self.show_layout_tabs,
                            annotation_scale: tab.scene.annotation_scale,
                            scale_pill_enabled,
                            annotation_all_visible: tab.scene.annotation_all_visible(),
                            annotation_auto_add: self.annotation_auto_scale > 0,
                            viewport_scale_synced: tab.scene.viewport_annotation_scale_synced(),
                            viewport_locked: tab.scene.viewports_display_locked(),
                            lineweight_display: tab.scene.document.header.lineweight_display,
                            constraint_status: tab.active_block_edit_session().map(|_| tab.scene.block_constraint_status_mode),
                        },
                        crate::ui::statusbar::StatusBarModes {
                            ortho_mode: self.ortho_mode,
                            polar_mode: self.polar_mode,
                            polar_increment_deg: self.polar_increment_deg,
                            dyn_input: self.dyn_input,
                            otrack: self.snapper.otrack_enabled,
                            dynamic_ucs: self.dynamic_ucs,
                            isometric_drafting: self.isometric_drafting,
                            iso_plane: self.iso_plane,
                            clean_screen: self.clean_screen,
                            linear_format: tab.scene.document.header.linear_unit_format,
                            isolation_active: tab.scene.is_isolation_active(),
                            transparency_display: tab.scene.transparency_display,
                            quick_properties: self.quick_properties,
                            selection_filter_active: tab.scene.selection_filter_active(),
                            selection_cycling: self.selection_cycling,
                        },
                        crate::ui::statusbar::StatusBarReadout {
                            cursor_world: cursor_coord,
                            coords_mode,
                            last_point: last_coord,
                            picking,
                        },
                        &self.statusbar_config,
                        status_menu_data,
                        tab.scene
                            .parametric_constraint_set(tab.current_parametric_scope())
                            .and_then(|s| s.dof),
                        tab.scene
                            .parametric_constraint_set(tab.current_parametric_scope())
                            .map(|s| s.conflicts.len())
                            .unwrap_or(0),
                        &self.gpu_status,
                        (self.spacemouse.visible()
                            || self.spacemouse_preferences.mode
                                != crate::input::spacemouse::NavigationMode::Auto)
                            .then(|| {
                                crate::ui::statusbar::spacemouse::view(
                                    self.spacemouse_preferences,
                                    self.spacemouse.status(),
                                    self.spacemouse_paused,
                                    self.spacemouse_label(),
                                    self.spacemouse_sheet(),
                                )
                            }),
                    )
                })
                .width(Fill)
                .height(Fill)
        })
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill);
        mark("statusbar");

        // History labels are built only when their dropdown is actually open;
        // otherwise an empty slice short-circuits the overlay gate
        // (`dropdown_overlay` returns `None` for empty labels anyway).
        let open_dropdown = self.ribbon.open_dropdown.as_deref();
        let undo_labels: Vec<String> = if open_dropdown == Some(crate::ui::ribbon::UNDO_HISTORY_ID)
        {
            history_dropdown_labels(&self.tabs[self.active_tab].history.undo_stack)
        } else {
            Vec::new()
        };
        let redo_labels: Vec<String> = if open_dropdown == Some(crate::ui::ribbon::REDO_HISTORY_ID)
        {
            history_dropdown_labels(&self.tabs[self.active_tab].history.redo_stack)
        } else {
            Vec::new()
        };
        let gallery_open = open_dropdown == Some(crate::modules::insert::insert_block::GALLERY_ID)
            && !self.tabs[self.active_tab].is_start;
        let dropdown_layer: Element<'_, Message> = if gallery_open {
            self.ribbon.place_dropdown(
                crate::modules::insert::insert_block::GALLERY_ID,
                crate::ui::window::block_palette::gallery(&self.block_palette),
                crate::ui::window::block_palette::GALLERY_W,
                self.win_size.0,
            )
        } else {
            self.ribbon
                .dropdown_overlay(
                    &undo_labels,
                    &redo_labels,
                    self.win_size,
                    self.tabs[self.active_tab].is_start,
                    &self.tabs[self.active_tab].recent_colors,
                )
                .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into())
        };

        let snap_override_layer: Element<'_, Message> = if let Some(pos) = self.snap_override_popup
        {
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

        // A minimized ribbon's popup lies over the drawing, under dropdowns.
        let ribbon_popup_layer: Element<'_, Message> = if self.clean_screen || self.ribbon_closed {
            None
        } else {
            self.ribbon.minimized_popup(
                is_paper,
                self.tabs[self.active_tab].is_start,
                self.show_block_palette,
                self.block_visibility_context(),
                self.win_size,
            )
        }
        .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into());

        let ribbon_menu_layer: Element<'_, Message> = self
            .ribbon
            .menu_overlay(self.win_size, self.show_menu_bar)
            .unwrap_or_else(|| iced::widget::Space::new().width(0).height(0).into());

        // Key tips: the hint bar under the dropdowns, the codes above all.
        let (keytip_bar, keytip_layer): (Element<'_, Message>, Element<'_, Message>) =
            self.ribbon.keytip_layers().unwrap_or_else(|| {
                (
                    iced::widget::Space::new().width(0).height(0).into(),
                    iced::widget::Space::new().width(0).height(0).into(),
                )
            });

        let composed = stack![
            main_ui,
            ribbon_popup_layer,
            keytip_bar,
            dropdown_layer,
            ribbon_menu_layer,
            qselect_layer,
            snap_override_layer,
            open_progress_layer,
            keytip_layer,
        ];

        // If the Plot Style editor was launched from PLOT, keep the Plot dialog
        // rendered underneath as its blocked parent.
        let modal_underlay: Element<'_, Message> = if self.active_modal
            == Some(super::ModalKind::Plotstyle)
        {
            if let Some((plot_offset, plot_resize)) = self.plotstyle_parent_plot_geometry.as_ref() {
                let plot_content = self.plot_modal_content(*plot_resize);

                crate::ui::modal::modal(
                    composed,
                    crate::tr!("modal", "plot"),
                    plot_content,
                    Message::CloseModal,
                    *plot_offset,
                    crate::ui::modal::ModalOptions::STANDARD,
                )
            } else {
                composed.into()
            }
        } else {
            composed.into()
        };

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
                    modal_underlay,
                    self.modal_title(),
                    content,
                    Message::CloseModal,
                    self.modal_offset,
                    modal_options,
                )
            }
            None => modal_underlay,
        };
        // Shared CAD colour picker. Indexed ACI colours use OpenCADStudio's own
        // dialog; True Color keeps the existing iced_aw gradient picker.
        // Which part of the widget tree the frame went into. Reported as a
        // breakdown rather than one number, because a first switch to a paper
        // layout spends seconds here and `view` alone cannot say where.
        if let Some(t0) = view_t0 {
            mark("end");
            let total = t0.elapsed().as_secs_f64() * 1000.0;
            if total >= 1.0 {
                let marks = view_marks.borrow();
                // A mark opens a section; the following mark closes it, so a
                // section's cost is the gap to its successor. The first mark
                // is preceded by the preamble, reported as `head`.
                let mut detail = String::new();
                if let Some((_, first)) = marks.first() {
                    let _ =
                        std::fmt::Write::write_fmt(&mut detail, format_args!(" head={first:.1}"));
                }
                for pair in marks.windows(2) {
                    let _ = std::fmt::Write::write_fmt(
                        &mut detail,
                        format_args!(" {}={:.1}", pair[0].0, pair[1].1 - pair[0].1),
                    );
                }
                crate::perf_record!("[perf] view-detail total={total:.1}ms{detail}");
            }
        }

        if let Some(picker) = self.color_picker.as_ref() {
            // While the eyedropper is picking, the panel steps aside so the
            // whole window can be sampled.
            if let Some(eyedropper) = picker.eyedropper_view() {
                return iced::widget::stack![base, eyedropper].into();
            }
            crate::ui::modal::modal(
                base,
                t!("Select Color"),
                picker.view(),
                Message::CloseColorPicker,
                self.modal_offset,
                crate::ui::modal::ModalOptions::NOTICE,
            )
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
        // recently-pushed history entry that is expiring, re-render every frame so the
        // entry disappears at the moment its visible window expires.
        // The subscription auto-stops once no entry is expiring.
        let history_tick = if self.command_line.has_expiring_history() {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // While the cursor sits over a grip, request animation frames
        // so the multi-functional popup opens even when the user keeps
        // the mouse perfectly still — `ViewportMove` alone would never
        // fire again. Auto-stops once the hover clears or the popup is
        // already open.
        // Resume scene construction after the notice redraw.
        let layout_settle = if self.layout_settling {
            window::frames().map(|_| Message::LayoutSettled)
        } else {
            Subscription::none()
        };
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
        let snap_dwell = if self
            .snap_dwell
            .as_ref()
            .is_some_and(|dwell| !dwell.snap_resolved || dwell.preview_dirty)
        {
            window::frames().map(|_| Message::SnapDwellTick)
        } else {
            Subscription::none()
        };
        // Interaction LOD: while the view is (or just was) navigating, keep
        // requesting frames a little past the settle point. Panning itself is
        // driven by input events, but once the cursor stops no event would fire
        // the one full-quality frame that re-renders hatches — this tick does,
        // then the scene-render cache holds it and the subscription auto-stops.
        // Underlay rasters follow the zoom once it settles; tick until then.
        let nav_settle = if self.tabs[self.active_tab].scene.is_settling()
            || self.tabs[self.active_tab].scene.underlay_resolution_stale()
        {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // Graphics verdict: a device appears inside the first frame and a
        // dropped scene shows up as draws without one, but neither arrives
        // as a message. Tick until the verdict is in — one frame on a
        // working GPU, three on none — then stop. The Start page draws no
        // viewport, so there is nothing to learn there.
        let gpu_probe = if matches!(self.gpu_status, crate::scene::pipeline::GpuStatus::Unknown)
            && !self.tabs[self.active_tab].is_start
        {
            window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        // A compositor that delivers no frame (an occluded or throttled
        // window) must not hold the save: a timer goes ahead without it.
        let thumbnail_capture = if self.thumbnail_capture_clean {
            Subscription::batch([
                window::frames().map(|_| Message::ThumbnailCaptureFrame),
                iced::time::every(std::time::Duration::from_millis(300))
                    .map(|_| Message::ThumbnailCaptureFrame),
            ])
        } else {
            Subscription::none()
        };
        // C11: park a `capture` request until the window draws another frame;
        // a timer fallback prevents a request from hanging when no redraw is
        // pending.
        let capture_frame = if self.control.capture_frame_pending {
            Subscription::batch([
                window::frames().map(|_| Message::ControlCaptureFrame { redrew: true }),
                iced::time::every(std::time::Duration::from_millis(50))
                    .map(|_| Message::ControlCaptureFrame { redrew: false }),
            ])
        } else {
            Subscription::none()
        };
        // Expire the QuickCalc status message ("Copied 870").
        // Open or close a minimized ribbon's popup after its delay.
        let ribbon_minimize = if self.ribbon.wants_tick() {
            iced::time::every(std::time::Duration::from_millis(50))
                .map(|_| Message::RibbonMinimize(crate::ui::ribbon::MinimizeMsg::Tick))
        } else {
            Subscription::none()
        };
        let quick_calc_flash = if self.quick_calc.flash.is_some() {
            iced::time::every(crate::ui::window::quick_calc::QuickCalcPalette::flash_interval())
                .map(|_| {
                    Message::QuickCalc(crate::ui::window::quick_calc::QuickCalcMsg::FlashExpired)
                })
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
        // Periodic autosave (SAVETIME): a `.sv$` recovery copy beside the file
        // on native, an OPFS snapshot on web. SAVETIME is the interval in
        // minutes; 0 disables it. The web build checks the deadline from the
        // `PollWebFonts` tick instead: a long `time::every` there stalls every
        // other timer for its whole period (#1775).
        let autosave = if self.savetime_min > 0 && cfg!(not(target_arch = "wasm32")) {
            iced::time::every(std::time::Duration::from_secs(
                self.savetime_min as u64 * 60,
            ))
            .map(|_| Message::AutoSave)
        } else {
            Subscription::none()
        };
        // Drawings handed over by a second launch (single instance). Inert in a
        // process that lost the port election, so it costs nothing there.
        #[cfg(not(target_arch = "wasm32"))]
        let single_instance = crate::io::single_instance::subscribe().map(Message::OpenExternal);
        #[cfg(target_arch = "wasm32")]
        let single_instance = Subscription::none();
        // Drain plugin-to-host requests on a timer when any plugin is loaded.
        // This lets long-lived plugin sessions (e.g. the Python REPL) mutate the
        // host document without requiring a user-generated message.
        #[cfg(not(target_arch = "wasm32"))]
        let plugin_drain = if crate::plugin::external::with_manager(|mgr| !mgr.ids().is_empty()) {
            iced::time::every(std::time::Duration::from_millis(100))
                .map(|_| Message::DrainPluginRequests)
        } else {
            Subscription::none()
        };
        #[cfg(target_arch = "wasm32")]
        let plugin_drain = Subscription::none();
        // While a sheet set is open: it reloads when its .dst changes on disk,
        // and its sheets' .dwl lock files follow the open tabs.
        #[cfg(not(target_arch = "wasm32"))]
        let sheet_set_poll = if self.sheet_set.sets.is_empty() {
            Subscription::none()
        } else {
            iced::time::every(std::time::Duration::from_secs(1))
                .map(|_| Message::SheetSet(crate::ui::window::sheet_set::SheetSetMsg::Poll))
        };
        #[cfg(target_arch = "wasm32")]
        let sheet_set_poll = Subscription::none();
        let hatch_pattern_keys = if self.tabs[self.active_tab]
            .properties
            .hatch_pattern_picker_open
        {
            event::listen_with(hatch_pattern_key_event)
        } else {
            Subscription::none()
        };
        // While a shortcut-editor Key cell is armed for capture, a dedicated
        // listener records the pressed combination and swallows every other
        // key so captured shortcuts do not run in the drawing.
        let keyboard_events = if self.shortcut_capture_row.is_some() {
            event::listen_with(shortcut_capture_key_event)
        } else {
            event::listen_with(|ev, status, win_id| {
                use iced::event::Status;
                match ev {
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_))
                        if crate::ui::ribbon::keytips_active() =>
                    {
                        crate::ui::ribbon::keytip_pointer_pressed();
                        Some(Message::KeyTip(crate::ui::ribbon::KeyTipInput::Hide))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )) => {
                        crate::ui::ribbon::keytip_pointer_pressed();
                        Some(Message::PropPointerPressed)
                    }
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
                    iced::Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                        crate::ui::ribbon::keytip_key_released(&key)
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
                        repeat,
                        ..
                    }) => {
                        if let Some(keytip) =
                            crate::ui::ribbon::keytip_key_pressed(&key, modifiers, repeat)
                        {
                            return keytip;
                        }
                        #[cfg(target_arch = "wasm32")]
                        let accel = modifiers.command();
                        // AltGr reaches Windows apps as Ctrl+Alt, and a macOS
                        // layout types some characters with Option: when such a
                        // chord produces a printable glyph (`@`, `<`, `{` on many
                        // European layouts) it is typing, not a shortcut. (#1236)
                        let types_glyph = text.as_deref().is_some_and(|value| {
                            !value.is_empty()
                                && value
                                    .chars()
                                    .all(|ch| !ch.is_control() && !ch.is_whitespace())
                        });
                        let altgr = modifiers.control() && modifiers.alt();
                        let mac_option = cfg!(target_os = "macos")
                            && modifiers.alt()
                            && !modifiers.control()
                            && !modifiers.logo();
                        let shortcut_modifier =
                            (modifiers.control() || modifiers.alt() || modifiers.logo())
                                && !(types_glyph && (altgr || mac_option));
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
                                extend_selection: modifiers.shift(),
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
                        if status == Status::Ignored
                            || crate::app::shortcuts::is_global_key(&shortcut)
                        {
                            return Some(Message::ShortcutPressed(shortcut));
                        }
                        // A text field captured the key. SPACE rebound away from
                        // COMMANDSPACE still fires from the command line, but
                        // only there: other fields keep typing spaces. This
                        // closure is a plain fn, so the binding is read from a
                        // flag and the focus is asked for in update. (#1710)
                        (shortcut == "SPACE"
                            && crate::app::shortcuts::SPACE_REBOUND_OFF_COMMANDSPACE
                                .load(std::sync::atomic::Ordering::Relaxed))
                        .then_some(Message::CommandLineSpaceProbe)
                    }
                    _ => None,
                }
            })
        };
        #[cfg(not(target_arch = "wasm32"))]
        let control = iced::Subscription::batch([
            super::control::subscribe().map(Message::ControlRequest),
            // Loopback REST channel when launched with files + --http.
            super::control::http_bridge::subscribe().map(Message::ControlRequest),
        ]);
        #[cfg(target_arch = "wasm32")]
        let control = iced::time::every(std::time::Duration::from_millis(50))
            .map(|_| Message::PollWebControl);
        iced::Subscription::batch([
            if self.spacemouse.moving() && self.spacemouse_focused && !self.spacemouse_paused {
                window::frames().map(Message::SpaceMouseFrame)
            } else {
                Subscription::none()
            },
            self.spacemouse
                .subscription()
                .map(|_| Message::SpaceMouseWake),
            crate::input::trackpad::subscription().map(Message::TrackpadPinch),
            event::listen_with(|event, _, id| match event {
                iced::Event::Touch(touch) => Some(Message::Touch(touch)),
                iced::Event::Window(window::Event::Focused) => {
                    Some(Message::SpaceMouseFocus(id, true))
                }
                iced::Event::Window(window::Event::Unfocused) => {
                    Some(Message::SpaceMouseFocus(id, false))
                }
                _ => None,
            }),
            control,
            frames,
            history_tick,
            layout_settle,
            grip_dwell,
            hover_dwell,
            snap_dwell,
            nav_settle,
            gpu_probe,
            thumbnail_capture,
            capture_frame,
            caret_blink,
            quick_calc_flash,
            ribbon_minimize,
            web_fonts,
            autosave,
            plugin_drain,
            sheet_set_poll,
            single_instance,
            hatch_pattern_keys,
            keyboard_events,
            ui_font_changes(),
        ])
    }

    pub(super) fn focus_cmd_input(&self) -> Task<Message> {
        iced::widget::operation::focus(iced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID))
    }

    pub(super) fn unfocus_widgets(&self) -> Task<Message> {
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
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

    let item = |label: &'static str, msg: Option<Message>| -> Element<'static, Message> {
        let enabled = msg.is_some();

        let content = container(text(t!(label)).size(12))
            .padding([4, 12])
            .width(Fill)
            .style(move |theme: &Theme| {
                let palette = theme.palette();

                container::Style {
                    text_color: Some(if enabled {
                        palette.background.base.text
                    } else {
                        palette.background.base.text.scale_alpha(0.42)
                    }),
                    ..Default::default()
                }
            });

        if let Some(msg) = msg {
            mouse_area(content)
                .on_press(msg)
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        } else {
            content.into()
        }
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
            crate::ui::wrap_bar::ReorderTab::document(idx, drag_targets.clone(), title_btn).into()
        };

        // Start tab is fixed — no close button. Every other tab gets a close.
        let row_inner: Row<'_, Message> = if tab.is_start {
            row![title_btn].spacing(0).align_y(iced::Center)
        } else {
            let close_btn = button(text("×").size(12))
                .on_press(Message::TabClose(tab.id))
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
        .padding([4, 8])
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
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
                left: 2.0,
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
        background: Some(Background::Color(theme.palette().background.base.color)),
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
    mouse_area(container(
        iced::widget::canvas(PaneDragSurface(idx))
            .width(Fill)
            .height(Fill),
    ))
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
    thumbs: &'a std::collections::HashMap<std::path::PathBuf, Option<iced::widget::image::Handle>>,
    recent_limit: usize,
    recent_limit_input: &'a str,
    recent_view: super::RecentView,
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
            recent_view,
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
    thumbs: &'a std::collections::HashMap<std::path::PathBuf, Option<iced::widget::image::Handle>>,
    recent_limit: usize,
    recent_limit_input: &'a str,
    recent_view: super::RecentView,
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
                text(crate::tr!("start", "donate")).size(14),
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
        outline_btn(crate::tr!("start", "new-drawing"), Message::TabNew).into(),
        // NEW asks which template to start from (.dwt, .dwg or .dxf).
        outline_btn(
            crate::tr!("start", "new-from-template"),
            Message::RibbonToolClick {
                tool_id: "NEW".to_string(),
                event: crate::modules::ModuleEvent::Command("NEW".to_string()),
            },
        )
        .into(),
        outline_btn(crate::tr!("start", "open-file"), Message::OpenFile).into(),
        donate_btn.into(),
    ])
    .spacing_x(12.0)
    .row_h(48.0)
    .report_natural_width(action_width_out.clone());

    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut secondary_items: Vec<Element<'a, Message>> = vec![
        outline_btn(
            crate::tr!("start", "send-feedback"),
            Message::RibbonToolClick {
                tool_id: "REPORT".to_string(),
                event: crate::modules::ModuleEvent::Command("REPORT".to_string()),
            },
        )
        .into(),
        outline_btn(crate::tr!("action", "options"), Message::OptionsOpen).into(),
    ];
    secondary_items
        .push(outline_btn(crate::tr!("action", "plugins"), Message::PluginManagerOpen).into());
    // The web build is already in the browser, so only the desktop offers a
    // link to the web version.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Filled with the active theme's primary colour.
        secondary_items.push(
            button(text(crate::t!("OCS Web")).size(14))
                .on_press(Message::RibbonToolClick {
                    tool_id: "WEBVERSION".to_string(),
                    event: crate::modules::ModuleEvent::Command("WEBVERSION".to_string()),
                })
                .padding([10, 22])
                .style(|theme: &Theme, status| start_action_shape(button::primary(theme, status)))
                .into(),
        );
    }
    #[cfg(target_arch = "wasm32")]
    secondary_items.push(
        button(text(crate::t!("OCS Desktop")).size(14))
            .on_press(Message::OpenUrl(
                "https://github.com/HakanSeven12/OpenCADStudio/releases/latest".to_string(),
            ))
            .padding([10, 22])
            .style(|theme: &Theme, status| start_action_shape(button::primary(theme, status)))
            .into(),
    );
    let secondary_row = WrapFlow::new(secondary_items)
        .spacing_x(12.0)
        .row_h(44.0)
        .report_natural_width(action_width_out.clone());

    let reddit_btn = button(
        row![
            iced::widget::svg(iced::widget::svg::Handle::from_memory(include_bytes!(
                "../../../assets/icons/reddit.svg"
            )))
            .width(20)
            .height(20),
            text("r/OpenCADStudio").size(14),
        ]
        .spacing(7)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenUrl(
        "https://www.reddit.com/r/OpenCADStudio/".to_string(),
    ))
    .padding([10, 22])
    .style(|theme: &Theme, status| {
        let palette = theme.palette();
        let pair = match status {
            button::Status::Hovered => palette.background.strong,
            _ => palette.background.weak,
        };
        start_action_shape(button::Style {
            background: Some(Background::Color(pair.color)),
            text_color: pair.text,
            border: Border {
                color: Color::from_rgb8(255, 69, 0),
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
    });

    let sponsors = column![
        text(crate::tr!("start", "sponsors")).size(15),
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
        Space::new().height(iced::Length::Fixed(10.0)),
        container(reddit_btn).center_x(Fill),
        Space::new().height(iced::Length::Fixed(20.0)),
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
    let measured_action_w =
        f32::from_bits(action_width_out.load(std::sync::atomic::Ordering::Relaxed));
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
        recent_view,
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
        let thumb_h = (panel_w - VIDEO_PANEL_PADDING * 2.0 - VIDEO_SCROLL_GUTTER) * 9.0 / 16.0;
        let mut list = column![text(crate::tr!("start", "tutorials")).size(15)]
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
                crate::tr!("start", "loading-videos")
            } else {
                crate::tr!("start", "videos-online")
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let playlist_btn = mouse_area(
            container(text(crate::tr!("start", "open-playlist")).size(12))
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
        let mut list = column![text(crate::tr!("start", "discussions")).size(15)]
            .spacing(8)
            .width(Fill);
        for discussion in discussions {
            let mut meta = iced::widget::row![text(format!("#{}", discussion.number))
                .size(10)
                .style(start_muted_style),]
            .spacing(6)
            .align_y(iced::Center);
            if discussion.pinned {
                meta = meta.push(
                    text(crate::tr!("start", "pinned"))
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
            let card =
                container(column![text(discussion.title.clone()).size(12), meta,].spacing(4))
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
                crate::tr!("start", "loading-discussions")
            } else {
                crate::tr!("start", "discussions-online")
            };
            list = list.push(text(note).size(12).style(start_muted_style));
        }
        let open_btn = mouse_area(
            container(text(crate::tr!("start", "open-discussions")).size(12))
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
            StartLayout::AllPanels | StartLayout::WithoutVideos => iced::Length::Fixed(panel_w),
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
            text(crate::tr!("start", "supporters")).size(15),
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
                    text(crate::tr!("start", "support-on-patreon")).size(12),
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
            | StartLayout::WithoutVideosAndDiscussions => iced::Length::Fixed(panel_w),
            StartLayout::RecentAndWelcome | StartLayout::Compact => iced::Length::Fill,
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
        StartLayout::AllPanels => {
            iced::widget::row![recent, videos_panel, welcome, discussions_panel, supporters,]
                .spacing(16)
                .height(Fill)
                .into()
        }
        StartLayout::WithoutVideos => {
            iced::widget::row![recent, welcome, discussions_panel, supporters]
                .spacing(16)
                .height(Fill)
                .into()
        }
        StartLayout::WithoutVideosAndDiscussions => iced::widget::row![recent, welcome, supporters]
            .spacing(16)
            .height(Fill)
            .into(),
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
                            (false, button::Status::Hovered) => Some(palette.background.strong),
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
                tab_btn(
                    crate::tr!("start", "recent-files"),
                    super::StartSection::Recent,
                )
                .into(),
                tab_btn(crate::tr!("start", "videos"), super::StartSection::Videos).into(),
                tab_btn(crate::tr!("start", "welcome"), super::StartSection::Welcome).into(),
                tab_btn(
                    crate::tr!("start", "discussions"),
                    super::StartSection::Discussions,
                )
                .into(),
                tab_btn(
                    crate::tr!("start", "supporters"),
                    super::StartSection::Supporters,
                )
                .into(),
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
            background: Some(Background::Color(theme.palette().background.base.color)),
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
/// Recent files as tiles: small thumbnails wrapping across the panel, or one
/// large preview per row. A tile opens its file; the list view keeps the
/// remove buttons.
fn recent_tiles<'a>(
    recents: &'a [std::path::PathBuf],
    thumbs: &'a std::collections::HashMap<std::path::PathBuf, Option<iced::widget::image::Handle>>,
    large: bool,
) -> iced::widget::Column<'a, Message> {
    let (thumb_w, thumb_h, name_chars) = if large {
        (Fill, iced::Length::Fixed(180.0), 40)
    } else {
        (iced::Length::Fixed(72.0), iced::Length::Fixed(54.0), 11)
    };
    let tile = |path: &'a std::path::PathBuf| -> Element<'a, Message> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let picture: Element<'a, Message> = match thumbs.get(path).and_then(|o| o.as_ref()) {
            Some(h) => iced::widget::image(h.clone())
                .content_fit(iced::ContentFit::Contain)
                .width(Fill)
                .height(Fill)
                .into(),
            None => crate::ui::icons::themed_secondary(crate::ui::icons::DOC, 28.0),
        };
        let picture = container(picture)
            .width(thumb_w)
            .height(thumb_h)
            .center_x(thumb_w)
            .center_y(thumb_h)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
        let mut label = column![text(crate::ui::text_util::elide(&name, name_chars)).size(12)];
        if large {
            let dir = path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            label = label.push(
                text(crate::ui::text_util::elide(&dir, 52))
                    .size(10)
                    .style(start_muted_style),
            );
        }
        button(column![picture, label.spacing(2)].spacing(4))
            .on_press(Message::OpenRecent(path.clone()))
            .padding(6)
            .width(if large { Fill } else { iced::Length::Shrink })
            .style(|theme: &Theme, status| {
                let palette = theme.palette();
                button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then_some(Background::Color(palette.background.strong.color)),
                    text_color: palette.background.base.text,
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
    };
    if large {
        column(recents.iter().map(tile)).spacing(6)
    } else {
        column![iced::widget::Row::with_children(recents.iter().map(tile))
            .spacing(4)
            .wrap()]
        .width(Fill)
    }
}

pub(super) fn recent_files_panel<'a>(
    recents: &'a [std::path::PathBuf],
    thumbs: &'a std::collections::HashMap<std::path::PathBuf, Option<iced::widget::image::Handle>>,
    limit: usize,
    limit_input: &'a str,
    view: super::RecentView,
    width: iced::Length,
) -> Element<'a, Message> {
    use super::RecentView;
    // Title mirrors the Supporters rail: size 15 in the bright text colour,
    // followed by a 12px gap before the content. The view-mode toggles sit
    // on the title's right.
    let mode_button = |icon: &'static [u8], mode: RecentView, tip: String| {
        let active = view == mode;
        iced::widget::tooltip(
            button(crate::ui::icons::themed(icon, 13.0))
                .on_press(Message::SetRecentView(mode))
                .padding([3, 5])
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    let background = if active {
                        Some(palette.primary.weak.color)
                    } else {
                        matches!(status, button::Status::Hovered)
                            .then_some(palette.background.strong.color)
                    };
                    button::Style {
                        background: background.map(Background::Color),
                        text_color: palette.background.base.text,
                        border: Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            container(text(tip).size(11))
                .padding([2, 6])
                .style(container::rounded_box),
            iced::widget::tooltip::Position::Bottom,
        )
    };
    let title = row![
        text(crate::tr!("start", "recent-documents"))
            .size(15)
            .width(Fill),
        mode_button(
            crate::ui::icons::MENU,
            RecentView::List,
            crate::tr!("start", "view-list"),
        ),
        mode_button(
            crate::ui::icons::GRID,
            RecentView::Icons,
            crate::tr!("start", "view-icons"),
        ),
        mode_button(
            crate::ui::icons::PREVIEW,
            RecentView::Preview,
            crate::tr!("start", "view-preview"),
        ),
    ]
    .spacing(2)
    .align_y(iced::Center);

    let body: Element<'a, Message> = if recents.is_empty() {
        container(
            text(crate::tr!("start", "no-recent-files"))
                .size(12)
                .style(start_muted_style),
        )
        .height(Fill)
        .into()
    } else if view != RecentView::List {
        iced::widget::scrollable(
            recent_tiles(recents, thumbs, view == RecentView::Preview).padding(iced::Padding {
                right: 12.0,
                ..iced::Padding::ZERO
            }),
        )
        .width(Fill)
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
                crate::tr!("start", "browser-storage")
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
                        text(crate::ui::text_util::elide(&name, 28)).size(12),
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
                    background: matches!(status, button::Status::Hovered)
                        .then_some(Background::Color(palette.background.strong.color)),
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
            background: matches!(status, button::Status::Hovered)
                .then_some(Background::Color(palette.background.strong.color)),
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
        text(crate::tr!("start", "keep-recent-files"))
            .size(11)
            .style(start_muted_style)
            .width(Fill),
        button(crate::ui::icons::themed(crate::ui::icons::MINUS, 11.0))
            .on_press(Message::SetRecentLimit(shown.saturating_sub(STEP)))
            .padding([3, 6])
            .style(step_style),
        count_box,
        button(crate::ui::icons::themed(crate::ui::icons::PLUS, 11.0))
            .on_press(Message::SetRecentLimit(shown.saturating_add(STEP)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_drawing_batched_pointer_events_keep_the_position_before_each_press() {
        use iced::mouse::{Button, Cursor, Event as Mouse};
        use iced::widget::canvas::Program;
        let bounds = iced::Rectangle {
            x: 100.0,
            y: 50.0,
            width: 800.0,
            height: 600.0,
        };
        // iced supplies this final position for EVERY event in the batch.
        let final_cursor = Cursor::Available(iced::Point::new(800.0, 500.0));
        let mut viewport = ViewportPointerState::default();
        let mut pane = ViewportPointerState::default();
        for position in [
            iced::Point::new(150.0, 100.0),
            iced::Point::new(250.0, 200.0),
        ] {
            let event = iced::Event::Mouse(Mouse::CursorMoved { position });
            let expected = iced::Point::new(position.x - bounds.x, position.y - bounds.y);
            let (motion, _, _) = ViewportPickSurface
                .update(&mut viewport, &event, bounds, final_cursor)
                .unwrap()
                .into_inner();
            assert!(matches!(motion, Some(Message::ViewportMove(p)) if p == expected));
            let (motion, _, _) = PaneDragSurface(2)
                .update(&mut pane, &event, bounds, final_cursor)
                .unwrap()
                .into_inner();
            assert!(matches!(motion, Some(Message::PaneMove(2, p)) if p == expected));
            let press = iced::Event::Mouse(Mouse::ButtonPressed(Button::Left));
            let (pick, _, status) = ViewportPickSurface
                .update(&mut viewport, &press, bounds, final_cursor)
                .unwrap()
                .into_inner();
            assert!(matches!(pick, Some(Message::ViewportLeftPressAt(p)) if p == expected));
            assert_eq!(status, iced::event::Status::Captured);
            let (pick, _, status) = PaneDragSurface(2)
                .update(&mut pane, &press, bounds, final_cursor)
                .unwrap()
                .into_inner();
            assert!(matches!(pick, Some(Message::PanePressAt(2, p)) if p == expected));
            assert_eq!(status, iced::event::Status::Captured);
        }
        // Menus above the viewport must still suppress underlying picks.
        let press = iced::Event::Mouse(Mouse::ButtonPressed(Button::Left));
        for hidden in [
            Cursor::Unavailable,
            Cursor::Levitating(iced::Point::new(250.0, 200.0)),
        ] {
            assert!(ViewportPickSurface
                .update(&mut viewport, &press, bounds, hidden)
                .is_none());
            assert!(PaneDragSurface(2)
                .update(&mut pane, &press, bounds, hidden)
                .is_none());
        }
    }

    #[test]
    fn test_crosshair_background_paper_space() {
        let tab = crate::app::document::DocumentTab::new_drawing(1);
        let bg = crosshair_background(&tab, true);
        assert_eq!(bg, tab.scene.paper_bg_color);
        assert!(crate::ui::style::common::canvas_is_light(bg));
    }

    /// The [+] step on the keep-recent-files box is built eagerly from an
    /// uncapped user string (`RecentLimitInput` keeps every ASCII digit):
    /// `18446744073709551615` parses to `usize::MAX`, and the plain
    /// `shown + STEP` overflowed while the *widget was constructed* —
    /// i.e. during `view()`, before any click. The [−] button next to it
    /// already used `saturating_sub`.
    #[test]
    fn recent_limit_plus_step_survives_an_overflowing_input() {
        let _ = start_page_content(
            &[],
            &[],
            false,
            &std::collections::HashMap::new(),
            &[],
            false,
            &[],
            &std::collections::HashMap::new(),
            50,
            "18446744073709551615",
            super::super::RecentView::default(),
            800.0,
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            super::super::StartSection::default(),
        );
    }
}

/// The objects in a REFEDIT session's working set: the checked-out copies and
/// objects added with REFSET still in it, and everything created in the
/// space since.
// ponytail: walks the document per frame while a session is open; keep a
// running count on the session if large drawings feel it.
fn refedit_working_set_count(
    session: &crate::modules::draw::modify::refedit::RefEditSession,
    scene: &crate::scene::Scene,
) -> usize {
    let space = scene.current_layout_block_handle_pub();
    scene
        .document
        .entities()
        .filter(|entity| {
            entity.common().owner_handle == space && session.in_working_set(entity.common().handle)
        })
        .count()
}
