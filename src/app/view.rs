use super::{H7CAD, Message};
use super::document::DocumentTab;
use super::history::history_dropdown_labels;
use super::helpers::grid_plane_from_camera;
use crate::scene::{VIEWCUBE_DRAW_PX, VIEWCUBE_PAD};
use crate::scene::grip::{grips_to_screen, grips_to_screen_paper};
use crate::scene::paper_canvas::PaperCanvas;
use crate::scene::viewport_pane::{PaperViewportPane, ViewportPane};
use crate::ui::overlay;
use iced::widget::{button, canvas, column, container, mouse_area, row, shader, stack, text, Row, Space};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Subscription, Task, Theme};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;

impl H7CAD {
    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        // ── Floating panel windows ─────────────────────────────────────────
        if Some(window_id) == self.layer_window {
            let tab = &self.tabs[self.active_tab];
            return tab.layers.view_window();
        }
        if Some(window_id) == self.page_setup_window {
            return crate::ui::page_setup::view_window(
                &self.page_setup_w, &self.page_setup_h,
                &self.page_setup_plot_area, self.page_setup_center,
                &self.page_setup_offset_x, &self.page_setup_offset_y,
                &self.page_setup_rotation, &self.page_setup_scale,
            );
        }
        if Some(window_id) == self.textstyle_window {
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab.scene.document.text_styles
                .iter().map(|s| s.name.clone()).collect();
            return crate::ui::textstyle::view_window(styles, &self.textstyle_selected,
                &self.textstyle_font, &self.textstyle_width, &self.textstyle_oblique);
        }
        if Some(window_id) == self.tablestyle_window {
            use acadrust::objects::ObjectType;
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab.scene.document.objects.values()
                .filter_map(|o| if let ObjectType::TableStyle(s) = o { Some(s.name.clone()) } else { None })
                .collect();
            let selected_style = tab.scene.document.objects.values()
                .find_map(|o| if let ObjectType::TableStyle(s) = o {
                    if s.name == self.tablestyle_selected { Some(s) } else { None }
                } else { None });
            return crate::ui::tablestyle::view_window(styles, &self.tablestyle_selected, selected_style);
        }
        if Some(window_id) == self.mlstyle_window {
            use acadrust::objects::ObjectType;
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab.scene.document.objects.values()
                .filter_map(|o| if let ObjectType::MLineStyle(s) = o { Some(s.name.clone()) } else { None })
                .collect();
            let selected_style = tab.scene.document.objects.values()
                .find_map(|o| if let ObjectType::MLineStyle(s) = o {
                    if s.name == self.mlstyle_selected { Some(s) } else { None }
                } else { None });
            return crate::ui::mlstyle::view_window(styles, &self.mlstyle_selected, selected_style,
                tab.scene.document.header.multiline_style.clone());
        }
        if Some(window_id) == self.layout_manager_window {
            let i = self.active_tab;
            let layouts = self.tabs[i].scene.layout_names();
            let current = self.tabs[i].scene.current_layout.clone();
            return crate::ui::layout_manager::view_window(layouts, &self.layout_manager_selected,
                &self.layout_manager_rename_buf, current);
        }
        if Some(window_id) == self.plotstyle_window {
            return crate::ui::plotstyle::view_window(
                self.active_plot_style.as_ref(), self.plotstyle_panel_aci,
                &self.ps_color_buf, &self.ps_lineweight_buf, &self.ps_screening_buf,
            );
        }
        if Some(window_id) == self.dimstyle_window {
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab.scene.document.dim_styles
                .iter().map(|s| s.name.clone()).collect();
            return crate::ui::dimstyle::view_window(
                styles, &self.dimstyle_selected, self.dimstyle_tab,
                crate::ui::dimstyle::DimStyleValues {
                    dimdle: &self.ds_dimdle,   dimdli: &self.ds_dimdli,  dimgap: &self.ds_dimgap,
                    dimexe: &self.ds_dimexe,   dimexo: &self.ds_dimexo,
                    dimsd1: self.ds_dimsd1,    dimsd2: self.ds_dimsd2,
                    dimse1: self.ds_dimse1,    dimse2: self.ds_dimse2,
                    dimasz: &self.ds_dimasz,   dimcen: &self.ds_dimcen,  dimtsz: &self.ds_dimtsz,
                    dimtxt: &self.ds_dimtxt,   dimtxsty: &self.ds_dimtxsty, dimtad: &self.ds_dimtad,
                    dimtih: self.ds_dimtih,    dimtoh: self.ds_dimtoh,
                    dimscale: &self.ds_dimscale, dimlfac: &self.ds_dimlfac,
                    dimlunit: &self.ds_dimlunit, dimdec: &self.ds_dimdec, dimpost: &self.ds_dimpost,
                    dimtol: self.ds_dimtol,    dimlim: self.ds_dimlim,
                    dimtp: &self.ds_dimtp,     dimtm: &self.ds_dimtm,
                    dimtdec: &self.ds_dimtdec, dimtfac: &self.ds_dimtfac,
                },
            );
        }
        if Some(window_id) == self.shortcuts_window {
            return crate::ui::shortcuts::view_window(&self.shortcut_overrides);
        }
        if Some(window_id) == self.about_window {
            return crate::ui::about::view_window();
        }
        if Some(window_id) == self.unsaved_dialog_window {
            let tab_name = match &self.pending_close {
                Some(super::PendingClose::Tab(idx)) => self
                    .tabs.get(*idx)
                    .map(|t| t.tab_display_name())
                    .unwrap_or_default(),
                Some(super::PendingClose::Quit) => self
                    .tabs.iter().find(|t| t.dirty)
                    .map(|t| t.tab_display_name())
                    .unwrap_or_default(),
                None => String::new(),
            };
            return unsaved_changes_dialog_window(&tab_name);
        }

        let i = self.active_tab;
        let tab = &self.tabs[i];
        let is_paper = tab.scene.current_layout != "Model";
        let viewport_3d: Element<'_, Message> = if is_paper {
            paper_canvas_view(tab)
        } else {
            shader(ViewportPane::model(&tab.scene, self.show_viewcube))
                .width(Fill)
                .height(Fill)
                .into()
        };

        let selection_overlay = {
            let sel = tab.scene.selection.borrow().clone();
            let snap_info = tab.snap_result.map(|s| (s.screen, s.snap_type));

            let grips: Vec<overlay::GripMarker> =
                if tab.active_cmd.is_none() && !tab.selected_grips.is_empty() {
                    let (vw, vh) = tab.scene.selection.borrow().vp_size;
                    let bounds = iced::Rectangle {
                        x: 0.0, y: 0.0, width: vw, height: vh,
                    };
                    let sel_h = tab.selected_handle;
                    let screen_grips = if is_paper {
                        let cam = tab.scene.camera.borrow();
                        let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                        let half_h = cam.ortho_size();
                        let half_w = half_h * aspect;
                        let tx = cam.target.x;
                        let ty = cam.target.y;
                        drop(cam);
                        grips_to_screen_paper(&tab.selected_grips, tx, ty, half_w, half_h, bounds)
                    } else {
                        let vp_mat = tab.scene.camera.borrow().view_proj(bounds);
                        grips_to_screen(&tab.selected_grips, vp_mat, bounds)
                    };
                    screen_grips
                        .into_iter()
                        .filter(|(_, screen, _, _)| {
                            screen.x.is_finite()
                                && screen.y.is_finite()
                                && screen.x >= -bounds.width
                                && screen.x <= bounds.width * 2.0
                                && screen.y >= -bounds.height
                                && screen.y <= bounds.height * 2.0
                        })
                        .map(|(grip_id, screen, _is_midpoint, shape)| {
                            let is_hot = tab
                                .active_grip
                                .as_ref()
                                .map_or(false, |g| Some(g.handle) == sel_h && g.grip_id == grip_id);
                            overlay::GripMarker { pos: screen, shape, is_hot }
                        })
                        .collect()
                } else {
                    vec![]
                };

            let (vw, vh) = tab.scene.selection.borrow().vp_size;
            let vp_bounds = iced::Rectangle { x: 0.0, y: 0.0, width: vw, height: vh };

            let grid = if self.show_grid {
                let cam = tab.scene.camera.borrow();
                let plane = grid_plane_from_camera(cam.pitch, cam.yaw);
                Some(overlay::GridParams {
                    view_proj: cam.view_proj(vp_bounds),
                    bounds: vp_bounds,
                    plane,
                })
            } else {
                None
            };

            let ucs_icon = if self.show_ucs_icon && !is_paper {
                let cam = tab.scene.camera.borrow();
                Some(overlay::UcsIconParams {
                    view_proj: cam.view_proj(vp_bounds),
                    bounds: vp_bounds,
                })
            } else {
                None
            };

            // OST tracking points → screen positions.
            let ost_points: Vec<overlay::OstTrackPoint> = if self.snapper.otrack_enabled {
                let vp_mat = tab.scene.camera.borrow().view_proj(vp_bounds);
                self.snapper.tracking_points.iter().map(|&wp| {
                    let ndc = vp_mat.project_point3(wp);
                    overlay::OstTrackPoint {
                        screen: iced::Point::new(
                            (ndc.x + 1.0) * 0.5 * vp_bounds.width,
                            (1.0 - ndc.y) * 0.5 * vp_bounds.height,
                        ),
                    }
                }).collect()
            } else {
                vec![]
            };

            overlay::selection_overlay(sel, snap_info, grips, grid, ucs_icon, ost_points, tab.last_cursor_screen, !is_paper && self.show_viewcube)
        };

        let info = container(overlay::info_bar(
            if is_paper { &tab.scene.current_layout } else { "Custom View" },
            &tab.visual_style,
        ))
        .padding([4, 6]);

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
            Color { r: 0.22, g: 0.24, b: 0.28, a: 1.0 }
        } else {
            tab.bg_color
                .map(|[r, g, b, a]| Color { r, g, b, a })
                .unwrap_or(Color { r: 0.11, g: 0.11, b: 0.11, a: 1.0 })
        };

        // Dynamic input overlay — shown when a command is active and DYN is on.
        let dyn_input_overlay: Option<Element<'_, Message>> =
            if self.dyn_input && tab.active_cmd.is_some() {
                let w = tab.last_cursor_world;
                let label = if let Some(base) = self.last_point {
                    // Show relative distance + angle when we have a base point.
                    let dx = (w.x - base.x) as f64;
                    let dy = (w.z - base.z) as f64;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let ang = dy.atan2(dx).to_degrees();
                    format!("d={:.3}  <{:.1}°", dist, ang)
                } else {
                    format!("X:{:.3}  Y:{:.3}", w.x, w.z)
                };
                Some(overlay::dynamic_input_overlay(tab.last_cursor_screen, label))
            } else {
                None
            };

        let mut viewport_stack = stack![
            container(viewport_3d)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(bg_color)),
                    ..Default::default()
                })
                .width(Fill)
                .height(Fill),
            container(info).width(Fill).height(Fill),
            selection_overlay,
            viewport_mouse,
        ]
        .width(Fill)
        .height(Fill);

        if self.show_navbar {
            let nav = container(overlay::nav_toolbar())
                .align_right(Fill)
                .align_top(Fill)
                .padding(iced::Padding { top: 148.0, right: 8.0, bottom: 0.0, left: 0.0 });
            viewport_stack = viewport_stack.push(nav);
        }

        if self.show_viewcube && !is_paper {
            let cube_click: Element<'_, Message> = container(
                mouse_area(container(
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(VIEWCUBE_HIT_SIZE))
                        .height(iced::Length::Fixed(VIEWCUBE_HIT_SIZE)),
                ))
                .on_move(Message::CursorMoved)
                .on_press(Message::ViewportClick),
            )
            .align_right(Fill)
            .align_top(Fill)
            .padding(iced::Padding { top: VIEWCUBE_PAD, right: VIEWCUBE_PAD, bottom: 0.0, left: 0.0 })
            .width(Fill)
            .height(Fill)
            .into();
            viewport_stack = viewport_stack.push(cube_click);
        }

        if let Some(dyn_ol) = dyn_input_overlay {
            viewport_stack = viewport_stack.push(dyn_ol);
        }

        let properties_el: Element<'_, Message> = if self.show_properties {
            tab.properties.view()
        } else {
            Space::new().into()
        };

        let center_stack = iced::widget::stack![
            row![properties_el, viewport_stack]
                .width(Fill)
                .height(Fill),
        ]
        .width(Fill)
        .height(Fill);

        let main_ui = container({
            let mut col = column![self.ribbon.view(
                is_paper,
                self.tabs[self.active_tab].history.undo_stack.len(),
                self.tabs[self.active_tab].history.redo_stack.len(),
            )];
            if self.show_file_tabs {
                col = col.push(doc_tab_bar(&self.tabs, self.active_tab));
            }
            col.push(center_stack)
               .push(self.command_line.view())
               .push(self.status_bar.view(
                    &self.snapper,
                    self.snap_popup_open,
                    self.ortho_mode,
                    self.polar_mode,
                    self.polar_increment_deg,
                    self.show_grid,
                    self.dyn_input,
                    self.snapper.otrack_enabled,
                    tab.scene.layout_names(),
                    tab.scene.current_layout.clone(),
                    self.layout_rename_state.as_ref(),
                    tab.scene.first_viewport_scale(),
                    tab.scene.viewport_count(),
                    tab.scene.active_viewport.is_some(),
                    self.show_layout_tabs,
               ))
               .width(Fill)
               .height(Fill)
        })
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color { r: 0.11, g: 0.11, b: 0.11, a: 1.0 })),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill);

        let snap_layer: Element<'_, Message> = if self.snap_popup_open {
            crate::ui::snap_popup::snap_popup_overlay(&self.snapper, 4.0)
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

        let layout_ctx_layer: Element<'_, Message> =
            if let Some(name) = &self.layout_context_menu {
                layout_context_menu_overlay(name)
            } else {
                iced::widget::Space::new().width(0).height(0).into()
            };

        // ── Viewport right-click context menu ─────────────────────────────
        let viewport_ctx_layer: Element<'_, Message> = {
            let ctx_pos = tab.scene.selection.borrow().context_menu;
            if let Some(p) = ctx_pos {
                let has_cmd = tab.active_cmd.is_some();
                let has_selection = !tab.scene.selected.is_empty();
                let last_cmds: Vec<String> = self.command_line.cmd_recall
                    .iter().rev().take(3).cloned().collect();
                viewport_context_menu_overlay(p, has_cmd, has_selection, last_cmds)
            } else {
                iced::widget::Space::new().width(0).height(0).into()
            }
        };

        stack![main_ui, self.app_menu.view(), snap_layer, dropdown_layer, layout_ctx_layer, viewport_ctx_layer].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        use iced::event;
        iced::Subscription::batch([
            window::frames().map(Message::Tick),
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
                    iced::Event::Keyboard(keyboard::Event::KeyPressed {
                        key, modifiers, ..
                    }) => {
                        let ctrl = modifiers.control();
                        let shift = modifiers.shift();
                        match key {
                            keyboard::Key::Named(keyboard::key::Named::Enter)
                            | keyboard::Key::Named(keyboard::key::Named::Space)
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
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                                Some(Message::CommandHistoryPrev)
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                                Some(Message::CommandHistoryNext)
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
                            keyboard::Key::Character(c) if ctrl => match c.as_str() {
                                "n" => Some(Message::ClearScene),
                                "o" => Some(Message::OpenFile),
                                "s" if !shift => Some(Message::SaveFile),
                                "s" if shift => Some(Message::SaveAs),
                                "z" if !shift => Some(Message::Undo),
                                "z" if shift => Some(Message::Redo),
                                "y" => Some(Message::Redo),
                                "c" => Some(Message::Command("COPYCLIP".to_string())),
                                "x" => Some(Message::Command("CUTCLIP".to_string())),
                                "v" => Some(Message::Command("PASTECLIP".to_string())),
                                _ => None,
                            },
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

    pub(super) fn blur_cmd_input(&self) -> Task<Message> {
        let op = iced::advanced::widget::operation::focusable::unfocus::<Message>();
        iced::advanced::widget::operate(op)
    }
}

// ── Paper canvas ──────────────────────────────────────────────────────────
//
// PSPACE: single full-canvas PaperSheet widget — renders paper entities plus
//   model content of all viewports via CPU projection.
//
// MSPACE (active viewport): PaperSheet widget (excludes the active viewport
//   from its CPU projection) + a PaperViewportPane widget overlaid at the
//   active viewport's screen-space position.  PaperViewportPane uses a
//   distinct pipeline type (PaperViewportPipeline) so Iced's per-type storage
//   keeps the two prepare() calls from overwriting each other.

fn paper_canvas_view<'a>(tab: &'a super::document::DocumentTab) -> Element<'a, Message> {
    let scene = &tab.scene;

    // 2-D canvas for the paper sheet — paper entities, viewport borders, and
    // inactive viewport projections are rendered as vector paths.  This lets
    // users select/edit paper-space entities directly without entering MSPACE.
    let paper_sheet = canvas(PaperCanvas::new(scene))
        .width(Fill)
        .height(Fill);

    if let Some(vp_handle) = scene.active_viewport {
        let (canvas_w, canvas_h) = scene.selection.borrow().vp_size;
        if let Some(rect) = scene.viewport_screen_rect(vp_handle, (canvas_w, canvas_h)) {
            // Clamp to canvas bounds so Space widgets never get negative size.
            let x = rect.x.max(0.0).min(canvas_w);
            let y = rect.y.max(0.0).min(canvas_h);
            let w = rect.width.clamp(1.0, canvas_w - x);
            let h = rect.height.clamp(1.0, canvas_h - y);

            let vp_widget = shader(PaperViewportPane::new(scene, vp_handle))
                .width(iced::Length::Fixed(w))
                .height(iced::Length::Fixed(h));

            let positioned = column![
                Space::new().height(iced::Length::Fixed(y)),
                row![
                    Space::new().width(iced::Length::Fixed(x)),
                    vp_widget,
                ],
            ]
            .width(Fill)
            .height(Fill);

            // Blue border drawn on top of the 3-D overlay so the viewport
            // boundary is always visible even when the shader fills the area.
            const VP_BORDER: Color = Color { r: 0.18, g: 0.52, b: 0.95, a: 1.0 };
            let border_frame = container(
                Space::new()
                    .width(iced::Length::Fixed(w))
                    .height(iced::Length::Fixed(h)),
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
                row![
                    Space::new().width(iced::Length::Fixed(x)),
                    border_frame,
                ],
            ]
            .width(Fill)
            .height(Fill);

            return stack![paper_sheet, positioned, border_layer]
                .width(Fill)
                .height(Fill)
                .into();
        }
    }

    paper_sheet.into()
}

// ── Document tab bar ───────────────────────────────────────────────────────

pub(super) fn doc_tab_bar<'a>(tabs: &'a [DocumentTab], active_tab: usize) -> Element<'a, Message> {
    const BAR_BG: Color = Color { r: 0.13, g: 0.13, b: 0.13, a: 1.0 };
    const TAB_ACTIVE: Color = Color { r: 0.22, g: 0.22, b: 0.22, a: 1.0 };
    const TAB_HOVER: Color = Color { r: 0.18, g: 0.18, b: 0.18, a: 1.0 };
    const TAB_INACTIVE: Color = Color { r: 0.13, g: 0.13, b: 0.13, a: 1.0 };
    const ACCENT: Color = Color { r: 0.20, g: 0.55, b: 0.90, a: 1.0 };
    const TEXT_ACTIVE: Color = Color::WHITE;
    const TEXT_INACTIVE: Color = Color { r: 0.60, g: 0.60, b: 0.60, a: 1.0 };
    const CLOSE_HOVER: Color = Color { r: 0.70, g: 0.22, b: 0.22, a: 1.0 };
    const BORDER_COLOR: Color = Color { r: 0.25, g: 0.25, b: 0.25, a: 1.0 };

    let mut bar = Row::new().spacing(0).align_y(iced::Center);

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = idx == active_tab;
        let name = tab.tab_display_name();
        let label = if tab.dirty { format!("● {}", name) } else { name };

        let title_btn = button(text(label).size(12))
            .on_press(Message::TabSwitch(idx))
            .padding([5, 12])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match (is_active, status) {
                    (true, _) => TAB_ACTIVE,
                    (false, button::Status::Hovered) => TAB_HOVER,
                    _ => TAB_INACTIVE,
                })),
                text_color: if is_active { TEXT_ACTIVE } else { TEXT_INACTIVE },
                border: Border {
                    color: if is_active { ACCENT } else { Color::TRANSPARENT },
                    width: if is_active { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: false,
            });

        let close_btn = button(text("×").size(11).color(Color { r: 0.55, g: 0.55, b: 0.55, a: 1.0 }))
            .on_press(Message::TabClose(idx))
            .padding([3, 5])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => CLOSE_HOVER,
                    _ => if is_active { TAB_ACTIVE } else { TAB_INACTIVE },
                })),
                border: Border { radius: 3.0.into(), ..Default::default() },
                ..Default::default()
            });

        bar = bar.push(
            container(row![title_btn, close_btn].spacing(0).align_y(iced::Center))
                .style(move |_: &Theme| container::Style {
                    border: Border {
                        color: if is_active { BORDER_COLOR } else { Color::TRANSPARENT },
                        width: if is_active { 1.0 } else { 0.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        );
    }

    let new_btn = button(text("+").size(14).color(Color { r: 0.65, g: 0.65, b: 0.65, a: 1.0 }))
        .on_press(Message::TabNew)
        .padding([4, 10])
        .style(|_: &Theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => TAB_HOVER,
                _ => Color::TRANSPARENT,
            })),
            border: Border { radius: 0.0.into(), ..Default::default() },
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

// ── Viewport right-click context menu ──────────────────────────────────────

fn viewport_context_menu_overlay(
    pos: iced::Point,
    has_cmd: bool,
    has_selection: bool,
    last_cmds: Vec<String>,
) -> Element<'static, Message> {
    const MENU_BG: Color = Color { r: 0.17, g: 0.17, b: 0.17, a: 1.0 };
    const MENU_BORDER: Color = Color { r: 0.35, g: 0.35, b: 0.35, a: 1.0 };
    const ITEM_HOVER: Color = Color { r: 0.25, g: 0.45, b: 0.70, a: 1.0 };
    const TEXT_COL: Color = Color { r: 0.88, g: 0.88, b: 0.88, a: 1.0 };
    const SEP_COL: Color = Color { r: 0.30, g: 0.30, b: 0.30, a: 1.0 };

    let item = |label: String, msg: Message| -> Element<'static, Message> {
        button(text(label).size(12).color(TEXT_COL))
            .on_press(msg)
            .style(|_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => ITEM_HOVER,
                    _ => Color::TRANSPARENT,
                })),
                text_color: TEXT_COL,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: false,
            })
            .padding([4, 12])
            .width(Fill)
            .into()
    };

    let sep = || -> Element<'static, Message> {
        container(iced::widget::Space::new().width(Fill).height(1))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(SEP_COL)),
                ..Default::default()
            })
            .width(Fill)
            .height(1)
            .padding([0, 6])
            .into()
    };

    let mut items: Vec<Element<'static, Message>> = Vec::new();

    if has_cmd {
        items.push(item("Cancel".to_string(), Message::CommandEscape));
        items.push(item("Enter".to_string(), Message::CommandFinalize));
    } else {
        if !last_cmds.is_empty() {
            let last = last_cmds[0].clone();
            items.push(item(
                format!("Repeat {last}"),
                Message::Command(last.to_uppercase()),
            ));
            if last_cmds.len() > 1 {
                for cmd in last_cmds.iter().skip(1) {
                    let c = cmd.clone();
                    items.push(item(c.clone(), Message::Command(c.to_uppercase())));
                }
            }
            items.push(sep());
        }
        if has_selection {
            items.push(item("Delete".to_string(), Message::DeleteSelected));
            items.push(item("Move".to_string(), Message::Command("MOVE".to_string())));
            items.push(item("Copy".to_string(), Message::Command("COPY".to_string())));
            items.push(sep());
        }
        items.push(item("Select All".to_string(), Message::Command("SELECTALL".to_string())));
        items.push(item("Zoom Extents".to_string(), Message::Command("ZOOM".to_string())));
    }

    let menu_col = column(items).spacing(0).width(180);

    let menu = container(menu_col)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(MENU_BG)),
            border: Border { color: MENU_BORDER, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        })
        .padding([4, 0]);

    // Click-catcher to close the menu when clicking outside.
    let catcher = mouse_area(
        container(Space::new()).width(Fill).height(Fill),
    )
    .on_press(Message::ViewportContextMenuClose)
    .on_right_press(Message::ViewportContextMenuClose);

    // Position using top/left spacing.
    let positioned = column![
        Space::new().height(pos.y),
        row![Space::new().width(pos.x), menu],
    ]
    .width(Fill)
    .height(Fill);

    stack![catcher, positioned].into()
}

/// A small right-click context menu rendered above the status bar.
/// The `name` is the layout tab that was right-clicked.
fn layout_context_menu_overlay(name: &str) -> Element<'_, Message> {
    const MENU_BG: Color = Color { r: 0.17, g: 0.17, b: 0.17, a: 1.0 };
    const MENU_BORDER: Color = Color { r: 0.35, g: 0.35, b: 0.35, a: 1.0 };
    const ITEM_HOVER: Color = Color { r: 0.25, g: 0.45, b: 0.70, a: 1.0 };
    const TEXT_COLOR: Color = Color { r: 0.88, g: 0.88, b: 0.88, a: 1.0 };

    let item = |label: &'static str, msg: Message| {
        button(text(label).size(12).color(TEXT_COLOR))
            .on_press(msg)
            .style(|_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => ITEM_HOVER,
                    _ => Color::TRANSPARENT,
                })),
                text_color: TEXT_COLOR,
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: false,
            })
            .padding([4, 12])
            .width(Fill)
    };

    let rename_name = name.to_string();
    let delete_name = name.to_string();

    let menu = container(
        column![
            item("Rename", Message::LayoutRenameStart(rename_name)),
            item("Delete", Message::LayoutDelete(delete_name)),
        ]
        .spacing(0)
        .width(160),
    )
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(MENU_BG)),
        border: Border {
            color: MENU_BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .padding([4, 0]);

    // Click-catcher fills the whole screen to close the menu when clicking outside.
    let catcher = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::LayoutContextMenuClose)
    .on_right_press(Message::LayoutContextMenuClose);

    // Position the menu above the status bar at the left.
    let positioned = container(menu)
        .align_bottom(Fill)
        .align_left(Fill)
        .padding(iced::Padding { top: 0.0, right: 0.0, bottom: 30.0, left: 8.0 });

    stack![catcher, positioned].into()
}

/// Content for the floating "Unsaved Changes" OS window.
fn unsaved_changes_dialog_window(name: &str) -> Element<'static, Message> {
    const BG:         Color = Color { r: 0.18, g: 0.18, b: 0.20, a: 1.0 };
    const BORDER_COL: Color = Color { r: 0.38, g: 0.38, b: 0.42, a: 1.0 };
    const TEXT_COL:   Color = Color { r: 0.90, g: 0.90, b: 0.90, a: 1.0 };
    const BTN_SAVE:   Color = Color { r: 0.20, g: 0.46, b: 0.80, a: 1.0 };
    const BTN_HOVER:  Color = Color { r: 0.26, g: 0.55, b: 0.92, a: 1.0 };
    const BTN_DISC:   Color = Color { r: 0.28, g: 0.28, b: 0.30, a: 1.0 };
    const BTN_DHOV:   Color = Color { r: 0.36, g: 0.36, b: 0.40, a: 1.0 };

    let body_text = format!("Do you want to save changes to \"{}\"?", name);

    let btn = |label: &'static str, msg: Message, base: Color, hov: Color| {
        button(text(label).size(13).color(TEXT_COL))
            .on_press(msg)
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => hov,
                    _ => base,
                })),
                text_color: TEXT_COL,
                border: Border { color: BORDER_COL, width: 1.0, radius: 4.0.into() },
                shadow: iced::Shadow::default(),
                snap: false,
            })
            .padding([6, 18])
    };

    container(
        column![
            text(body_text).size(13).color(TEXT_COL),
            iced::widget::Space::new().height(20),
            row![
                btn("Save",    Message::UnsavedDialogSave,    BTN_SAVE, BTN_HOVER),
                iced::widget::Space::new().width(8),
                btn("Discard", Message::UnsavedDialogDiscard, BTN_DISC, BTN_DHOV),
                iced::widget::Space::new().width(8),
                btn("Cancel",  Message::UnsavedDialogCancel,  BTN_DISC, BTN_DHOV),
            ],
        ]
        .spacing(0),
    )
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(BG)),
        ..Default::default()
    })
    .center(Fill)
    .padding([24, 28])
    .into()
}

