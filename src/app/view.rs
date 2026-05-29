use super::document::DocumentTab;
use super::helpers::grid_plane_from_camera;
use super::history::history_dropdown_labels;
use super::document::{DynComponent, DynFieldEntry};
use super::{Message, OpenCADStudio};
use crate::scene::grip::{grips_to_screen, grips_to_screen_paper};
use crate::scene::paper_canvas::PaperCanvas;
use crate::scene::viewport_pane::ViewportPane;
use crate::scene::{VIEWCUBE_DRAW_PX, VIEWCUBE_PAD};
use crate::ui::overlay;
use iced::widget::{
    button, canvas, column, container, mouse_area, pick_list, row, shader, stack, text, text_input,
    Row, Space,
};
use iced::window;
use iced::{keyboard, Background, Border, Color, Element, Fill, Subscription, Task, Theme};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;

/// `pick_list` requires its items to implement `Display`; acadrust's
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
    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        // ── Floating panel windows ─────────────────────────────────────────
        if Some(window_id) == self.layer_window {
            let tab = &self.tabs[self.active_tab];
            return tab.layers.view_window();
        }
        if Some(window_id) == self.page_setup_window {
            return crate::ui::page_setup::view_window(
                &self.page_setup_w,
                &self.page_setup_h,
                &self.page_setup_plot_area,
                self.page_setup_center,
                &self.page_setup_offset_x,
                &self.page_setup_offset_y,
                &self.page_setup_rotation,
                &self.page_setup_scale,
            );
        }
        if Some(window_id) == self.textstyle_window {
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab
                .scene
                .document
                .text_styles
                .iter()
                .map(|s| s.name.clone())
                .collect();
            return crate::ui::textstyle::view_window(
                styles,
                &self.textstyle_selected,
                &self.textstyle_font,
                &self.textstyle_width,
                &self.textstyle_oblique,
            );
        }
        if Some(window_id) == self.tablestyle_window {
            use acadrust::objects::ObjectType;
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab
                .scene
                .document
                .objects
                .values()
                .filter_map(|o| {
                    if let ObjectType::TableStyle(s) = o {
                        Some(s.name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            let selected_style = tab.scene.document.objects.values().find_map(|o| {
                if let ObjectType::TableStyle(s) = o {
                    if s.name == self.tablestyle_selected {
                        Some(s)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
            return crate::ui::tablestyle::view_window(
                styles,
                &self.tablestyle_selected,
                selected_style,
            );
        }
        if Some(window_id) == self.mlstyle_window {
            use acadrust::objects::ObjectType;
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab
                .scene
                .document
                .objects
                .values()
                .filter_map(|o| {
                    if let ObjectType::MLineStyle(s) = o {
                        Some(s.name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            let selected_style = tab.scene.document.objects.values().find_map(|o| {
                if let ObjectType::MLineStyle(s) = o {
                    if s.name == self.mlstyle_selected {
                        Some(s)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
            return crate::ui::mlstyle::view_window(
                styles,
                &self.mlstyle_selected,
                selected_style,
                tab.scene.document.header.multiline_style.clone(),
            );
        }
        if Some(window_id) == self.layout_manager_window {
            let i = self.active_tab;
            let layouts = self.tabs[i].scene.layout_names();
            let current = self.tabs[i].scene.current_layout.clone();
            return crate::ui::layout_manager::view_window(
                layouts,
                &self.layout_manager_selected,
                &self.layout_manager_rename_buf,
                current,
            );
        }
        if Some(window_id) == self.plotstyle_window {
            return crate::ui::plotstyle::view_window(
                self.active_plot_style.as_ref(),
                self.plotstyle_panel_aci,
                &self.ps_color_buf,
                &self.ps_lineweight_buf,
                &self.ps_screening_buf,
            );
        }
        if Some(window_id) == self.dimstyle_window {
            let tab = &self.tabs[self.active_tab];
            let styles: Vec<String> = tab
                .scene
                .document
                .dim_styles
                .iter()
                .map(|s| s.name.clone())
                .collect();
            return crate::ui::dimstyle::view_window(
                styles,
                &self.dimstyle_selected,
                self.dimstyle_tab,
                crate::ui::dimstyle::DimStyleValues {
                    dimdle: &self.ds_dimdle,
                    dimdli: &self.ds_dimdli,
                    dimgap: &self.ds_dimgap,
                    dimexe: &self.ds_dimexe,
                    dimexo: &self.ds_dimexo,
                    dimsd1: self.ds_dimsd1,
                    dimsd2: self.ds_dimsd2,
                    dimse1: self.ds_dimse1,
                    dimse2: self.ds_dimse2,
                    dimasz: &self.ds_dimasz,
                    dimcen: &self.ds_dimcen,
                    dimtsz: &self.ds_dimtsz,
                    dimtxt: &self.ds_dimtxt,
                    dimtxsty: &self.ds_dimtxsty,
                    dimtad: &self.ds_dimtad,
                    dimtih: self.ds_dimtih,
                    dimtoh: self.ds_dimtoh,
                    dimscale: &self.ds_dimscale,
                    dimlfac: &self.ds_dimlfac,
                    dimlunit: &self.ds_dimlunit,
                    dimdec: &self.ds_dimdec,
                    dimpost: &self.ds_dimpost,
                    dimtol: self.ds_dimtol,
                    dimlim: self.ds_dimlim,
                    dimtp: &self.ds_dimtp,
                    dimtm: &self.ds_dimtm,
                    dimtdec: &self.ds_dimtdec,
                    dimtfac: &self.ds_dimtfac,
                },
            );
        }
        if Some(window_id) == self.shortcuts_window {
            return crate::ui::shortcuts::view_window(&self.shortcut_overrides);
        }
        if Some(window_id) == self.about_window {
            return crate::ui::about::view_window();
        }
        if Some(window_id) == self.update_notice_window {
            let latest = self
                .update_notice_version
                .as_deref()
                .unwrap_or("?");
            let body = self.update_notice_body.as_deref().unwrap_or("");
            return crate::ui::update_notice::view_window(latest, body);
        }
        if Some(window_id) == self.unsaved_dialog_window {
            let tab_name = match &self.pending_close {
                Some(super::PendingClose::Tab(idx)) => self
                    .tabs
                    .get(*idx)
                    .map(|t| t.tab_display_name())
                    .unwrap_or_default(),
                Some(super::PendingClose::Quit) => self
                    .tabs
                    .iter()
                    .find(|t| t.dirty)
                    .map(|t| t.tab_display_name())
                    .unwrap_or_default(),
                None => String::new(),
            };
            return unsaved_changes_dialog_window(&tab_name);
        }
        if Some(window_id) == self.save_dialog_window {
            return save_as_dialog_window(
                &self.save_dialog_filename,
                &self.save_dialog_folder,
                &self.save_dialog_entries,
                &self.save_dialog_format,
            );
        }

        let i = self.active_tab;
        let tab = &self.tabs[i];
        let is_paper = tab.scene.current_layout != "Model";
        // Start tab: render welcome page in place of the model/paper canvas.
        // Surrounding chrome (tab bar, status bar) stays; the welcome widget
        // returned here also flags the rest of `view` to skip drawing-only
        // overlays via `tab.is_start`.
        // Unified GPU widget for both layouts. In a paper layout the
        // PaperCanvas (2-D sheet + paper entities + viewport borders) is
        // layered underneath this shader so the area outside floating
        // viewports shows the sheet; the shader only blits inside each
        // content viewport's scissor rect.
        let viewport_3d: Element<'_, Message> = if tab.is_start {
            start_page_view()
        } else {
            shader(ViewportPane::model(
                &tab.scene,
                self.show_viewcube,
                tab.render_mode,
            ))
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
                    // Overlays project through the active tile's camera, so
                    // they must use the active tile's screen rectangle (with
                    // its canvas offset) — not the whole canvas — or they
                    // land in the wrong place in a tiled layout.
                    let bounds = tab.scene.active_model_tile_bounds(vw, vh);
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
                            overlay::GripMarker {
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
                self.snapper
                    .tracking_points
                    .iter()
                    .map(|&wp| {
                        let ndc = vp_mat.project_point3(wp);
                        overlay::OstTrackPoint {
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

            // Model-space tile dividers (none in paper / single-tile layouts).
            let tile_edges = if !is_paper {
                tab.scene.model_tile_edges()
            } else {
                vec![]
            };

            overlay::selection_overlay(
                sel,
                snap_info,
                grips,
                grid,
                ucs_icon,
                ost_points,
                tab.last_cursor_screen,
                !is_paper && self.show_viewcube,
                tile_edges,
            )
        };

        // Render-mode picker, top-left of the viewport. Drives whether
        // 3D face / mesh fills and edges are rendered for the active
        // tab. Hatch fills are deliberately *not* gated by this — the
        // document's FILLMODE still owns 2D fill state.
        let info = render_mode_picker(tab.render_mode);

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
                    r: 0.11,
                    g: 0.11,
                    b: 0.11,
                    a: 1.0,
                })
        };

        // Dynamic input overlay — editable boxes near the cursor, one per
        // quantity the active command is asking for (X/Y, or polar
        // distance+angle, or a single distance/angle). TAB moves focus
        // between boxes; typing locks a box to a fixed value while the
        // rest keep tracking the cursor. The field set is maintained in
        // `tab.dyn_fields` by `sync_dyn_fields`.
        let dyn_input_overlay: Option<Element<'_, Message>> =
            if self.dyn_input && tab.active_cmd.is_some() && !tab.dyn_fields.is_empty() {
                let w = tab.last_cursor_world;
                let base = self.last_point;
                let boxes: Vec<overlay::DynBox> = tab
                    .dyn_fields
                    .iter()
                    .enumerate()
                    .map(|(idx, f)| overlay::DynBox {
                        label: dyn_component_label(f.component),
                        value: dyn_component_value(f, w, base),
                        active: idx == tab.dyn_active,
                        locked: f.locked(),
                    })
                    .collect();
                Some(overlay::dynamic_input_overlay(tab.last_cursor_screen, boxes))
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
            // Paper layout: 2-D sheet + paper entities + viewport borders
            // underneath, unified GPU shader on top (only paints inside
            // each content viewport's scissor rect so the sheet shows
            // through outside them).
            let paper_sheet: Element<'_, Message> =
                canvas(PaperCanvas::new(&tab.scene)).width(Fill).height(Fill).into();
            stack![
                paper_sheet,
                container(viewport_3d).width(Fill).height(Fill),
                selection_overlay,
                viewport_mouse,
            ]
            .width(Fill)
            .height(Fill)
        } else {
            stack![
                container(viewport_3d)
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(bg_color)),
                        ..Default::default()
                    })
                    .width(Fill)
                    .height(Fill),
                selection_overlay,
                viewport_mouse,
            ]
            .width(Fill)
            .height(Fill)
        };

        // Model-space render-mode picker, top-left. Sits ABOVE the
        // viewport mouse_area so clicks inside its bounds reach it
        // instead of the shader behind it; `opaque` stops them bubbling
        // further. Outside the chip the Fill container is transparent so
        // viewport drawing / selection is unaffected. In a paper layout
        // the active viewport gets its own picker (below) instead.
        if !is_paper && !tab.is_start {
            // Render-mode picker + viewport-split buttons (horizontal /
            // vertical divider of the active Model tile).
            let split_btn = |glyph: &'static str, horizontal: bool| {
                button(text(glyph).size(13).color(Color {
                    r: 0.85,
                    g: 0.85,
                    b: 0.85,
                    a: 1.0,
                }))
                .on_press(Message::SplitModelViewport(horizontal))
                .padding([4, 8])
                .style(|_: &Theme, status| iced::widget::button::Style {
                    background: Some(Background::Color(match status {
                        iced::widget::button::Status::Hovered => Color {
                            r: 0.20,
                            g: 0.20,
                            b: 0.20,
                            a: 0.85,
                        },
                        _ => Color {
                            r: 0.10,
                            g: 0.10,
                            b: 0.10,
                            a: 0.75,
                        },
                    })),
                    border: Border {
                        color: Color {
                            r: 0.35,
                            g: 0.35,
                            b: 0.35,
                            a: 1.0,
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    text_color: Color {
                        r: 0.85,
                        g: 0.85,
                        b: 0.85,
                        a: 1.0,
                    },
                    ..Default::default()
                })
            };
            let bar = row![info, split_btn("▤", true), split_btn("▥", false)].spacing(4);
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
        // clicks (the paper canvas itself sits below it). Positioned with
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
            let x = rect.x.max(0.0);
            let y = rect.y.max(0.0);
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
                    .width(iced::Length::Fixed(rect.width.max(1.0)))
                    .height(iced::Length::Fixed(rect.height.max(1.0))),
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
                    iced::widget::opaque(render_mode_picker(vp_mode)),
                ],
            ]
            .width(Fill)
            .height(Fill);
            viewport_stack = viewport_stack.push(picker_layer);

            if self.show_viewcube {
                let cube_x = (rect.x + rect.width - VIEWCUBE_HIT_SIZE - VIEWCUBE_PAD).max(0.0);
                let cube_y = (rect.y + VIEWCUBE_PAD).max(0.0);
                let cube_click = column![
                    Space::new().height(iced::Length::Fixed(cube_y)),
                    row![
                        Space::new().width(iced::Length::Fixed(cube_x)),
                        mouse_area(
                            iced::widget::Space::new()
                                .width(iced::Length::Fixed(VIEWCUBE_HIT_SIZE))
                                .height(iced::Length::Fixed(VIEWCUBE_HIT_SIZE)),
                        )
                        .on_move(Message::CursorMoved)
                        .on_press(Message::ViewportClick),
                    ],
                ]
                .width(Fill)
                .height(Fill);
                viewport_stack = viewport_stack.push(cube_click);
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
            let cube_click = column![
                Space::new().height(iced::Length::Fixed(cube_y)),
                row![
                    Space::new().width(iced::Length::Fixed(cube_x)),
                    mouse_area(
                        iced::widget::Space::new()
                            .width(iced::Length::Fixed(VIEWCUBE_HIT_SIZE))
                            .height(iced::Length::Fixed(VIEWCUBE_HIT_SIZE)),
                    )
                    .on_move(Message::CursorMoved)
                    .on_press(Message::ViewportClick),
                ],
            ]
            .width(Fill)
            .height(Fill);
            viewport_stack = viewport_stack.push(cube_click);
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
                let mut col = column![]
                    .spacing(0)
                    .width(iced::Length::Fixed(row_w));
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
                viewport_stack = viewport_stack.push(position_canvas_overlay(anchor, menu_panel.into()));
            }
        }

        // Right-click context menu. Lives inside the viewport stack so
        // the cursor position (canvas-relative) anchors the menu under
        // the cursor instead of drifting into window-relative space.
        if !tab.is_start {
            let ctx_pos = tab.scene.selection.borrow().context_menu;
            if let Some(p) = ctx_pos {
                let has_cmd = tab.active_cmd.is_some();
                let has_selection = !tab.scene.selected.is_empty();
                let last_cmds: Vec<String> = self
                    .command_line
                    .cmd_recall
                    .iter()
                    .rev()
                    .take(3)
                    .cloned()
                    .collect();
                viewport_stack = viewport_stack
                    .push(viewport_context_menu_overlay(p, has_cmd, has_selection, last_cmds));
            }
        }

        // Properties / layers panels carry no useful state on the Start tab.
        // Replace the properties panel with a Recent Documents list there.
        let properties_el: Element<'_, Message> = if tab.is_start {
            recent_files_panel(&self.app_menu.recent)
        } else if self.show_properties {
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
        let dyn_capturing =
            self.dyn_input && tab.active_cmd.is_some() && !tab.dyn_fields.is_empty();
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
            let mut col = column![self.ribbon.view(
                is_paper,
                self.tabs[self.active_tab].history.undo_stack.len(),
                self.tabs[self.active_tab].history.redo_stack.len(),
            )];
            if self.show_file_tabs {
                col = col.push(doc_tab_bar(&self.tabs, self.active_tab));
            }
            col.push(center_stack)
                .push({
                    let is_model = tab.scene.current_layout == "Model";
                    let scale_pill_enabled = is_model
                        || tab.scene.active_viewport.is_some()
                        || tab.scene.has_selected_viewport();
                    self.status_bar.view(
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
                        tab.scene.annotation_scale,
                        self.scale_popup_open,
                        scale_pill_enabled,
                        tab.scene.document.header.lineweight_display,
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
            crate::ui::snap_popup::snap_popup_overlay(&self.snapper, 4.0)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let scale_layer: Element<'_, Message> = if self.scale_popup_open {
            let is_model = tab.scene.current_layout == "Model";
            crate::ui::scale_popup::scale_popup_overlay(
                is_model,
                tab.scene.annotation_scale,
                tab.scene.first_viewport_scale(),
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
            let properties = tab
                .scene
                .qselect_properties(state.type_filter.as_deref());
            qselect_overlay(state, &types, &properties)
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        let open_progress_layer: Element<'_, Message> = if let Some(p) = &self.opening {
            crate::ui::open_progress::view(p, iced::time::Instant::now())
        } else {
            iced::widget::Space::new().width(0).height(0).into()
        };

        stack![
            main_ui,
            self.app_menu.view(),
            snap_layer,
            scale_layer,
            dropdown_layer,
            layout_ctx_layer,
            qselect_layer,
            open_progress_layer,
        ]
        .into()
    }

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
        iced::Subscription::batch([
            frames,
            history_tick,
            grip_dwell,
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
                        key, modifiers, text, ..
                    }) => {
                        let ctrl = modifiers.control();
                        let shift = modifiers.shift();
                        // Any key that produces a printable glyph types it,
                        // even when its logical key resolves to navigation
                        // (NumLock-on Numpad8 / Numpad2 arrive as
                        // ArrowUp / ArrowDown but still carry text "8" /
                        // "2"). Checked before the Arrow / history arms so
                        // those numpad digits aren't swallowed as history
                        // navigation. Whitespace / control text (Space,
                        // Enter, Tab) falls through to the named handlers.
                        if !ctrl && status == Status::Ignored {
                            if let Some(t) = text.as_deref() {
                                if !t.is_empty()
                                    && t.chars().all(|c| !c.is_control() && !c.is_whitespace())
                                {
                                    return Some(Message::CommandAppendChar(t.to_string()));
                                }
                            }
                        }
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
                                "n" => Some(Message::TabNew),
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
        let name = tab.tab_display_name();
        let label = if tab.dirty {
            format!("● {}", name)
        } else {
            name
        };

        let title_btn = button(text(label).size(12))
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
            let close_btn = button(text("×").size(11).color(Color {
                r: 0.55,
                g: 0.55,
                b: 0.55,
                a: 1.0,
            }))
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
                    color: if is_active { BORDER_COLOR } else { Color::TRANSPARENT },
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
fn position_canvas_overlay<'a>(
    anchor: iced::Point,
    panel: Element<'a, Message>,
) -> Element<'a, Message> {
    let ax = anchor.x.max(0.0);
    let ay = anchor.y.max(0.0);
    column![
        Space::new().height(iced::Length::Fixed(ay)),
        row![
            Space::new().width(iced::Length::Fixed(ax)),
            iced::widget::opaque(panel),
        ],
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

// ── Viewport right-click context menu ──────────────────────────────────────

fn viewport_context_menu_overlay(
    pos: iced::Point,
    has_cmd: bool,
    has_selection: bool,
    last_cmds: Vec<String>,
) -> Element<'static, Message> {
    const MENU_BG: Color = Color {
        r: 0.17,
        g: 0.17,
        b: 0.17,
        a: 1.0,
    };
    const MENU_BORDER: Color = Color {
        r: 0.35,
        g: 0.35,
        b: 0.35,
        a: 1.0,
    };
    const ITEM_HOVER: Color = Color {
        r: 0.25,
        g: 0.45,
        b: 0.70,
        a: 1.0,
    };
    const TEXT_COL: Color = Color {
        r: 0.88,
        g: 0.88,
        b: 0.88,
        a: 1.0,
    };
    const SEP_COL: Color = Color {
        r: 0.30,
        g: 0.30,
        b: 0.30,
        a: 1.0,
    };

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
            items.push(item(
                "Move".to_string(),
                Message::Command("MOVE".to_string()),
            ));
            items.push(item(
                "Copy".to_string(),
                Message::Command("COPY".to_string()),
            ));
            items.push(sep());
            items.push(item("Select Similar".to_string(), Message::SelectSimilar));
        }
        items.push(item(
            "Select All".to_string(),
            Message::Command("SELECTALL".to_string()),
        ));
        items.push(item("Quick Select...".to_string(), Message::QSelectOpen));
        items.push(item(
            "Zoom Extents".to_string(),
            Message::Command("ZOOM".to_string()),
        ));
    }

    let menu_col = column(items).spacing(0).width(iced::Length::Fixed(180.0));

    let menu = container(menu_col)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(MENU_BG)),
            border: Border {
                color: MENU_BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .padding([4, 0])
        .width(iced::Length::Fixed(180.0));

    position_canvas_overlay(pos, menu.into())
}

/// A small right-click context menu rendered above the status bar.
/// The `name` is the layout tab that was right-clicked.
fn layout_context_menu_overlay(name: &str) -> Element<'_, Message> {
    const MENU_BG: Color = Color {
        r: 0.17,
        g: 0.17,
        b: 0.17,
        a: 1.0,
    };
    const MENU_BORDER: Color = Color {
        r: 0.35,
        g: 0.35,
        b: 0.35,
        a: 1.0,
    };
    const ITEM_HOVER: Color = Color {
        r: 0.25,
        g: 0.45,
        b: 0.70,
        a: 1.0,
    };
    const TEXT_COLOR: Color = Color {
        r: 0.88,
        g: 0.88,
        b: 0.88,
        a: 1.0,
    };

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
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 30.0,
            left: 8.0,
        });

    stack![catcher, positioned].into()
}

// ── Quick Select panel ─────────────────────────────────────────────────────

const QSELECT_ANY_TYPE: &str = "(Any type)";
const QSELECT_ANY_PROP: &str = "(Any property)";

/// Floating panel for the Quick Select feature. Single-row filter:
/// object type → property → operator → value, plus an "Append to current
/// selection" checkbox. The property dropdown is type-aware — Common
/// properties (Layer, Color, Linetype, Lineweight) are always shown;
/// picking a specific Object type adds that type's `geometry_properties`
/// fields (Start X, Length, Radius, …) so type-specific filtering works.
fn qselect_overlay<'a>(
    state: &'a crate::app::QSelectState,
    types: &[&'static str],
    properties: &[(String, String)],
) -> Element<'a, Message> {
    use iced::widget::{checkbox, pick_list};
    const BG: Color = Color { r: 0.12, g: 0.12, b: 0.12, a: 0.98 };
    const BORDER: Color = Color { r: 0.35, g: 0.35, b: 0.35, a: 1.0 };
    const TEXT: Color = Color { r: 0.88, g: 0.88, b: 0.88, a: 1.0 };
    const BTN_OK: Color = Color { r: 0.22, g: 0.42, b: 0.68, a: 1.0 };
    const BTN_OK_HOV: Color = Color { r: 0.30, g: 0.52, b: 0.80, a: 1.0 };
    const BTN_BG: Color = Color { r: 0.22, g: 0.22, b: 0.22, a: 1.0 };
    const BTN_HOV: Color = Color { r: 0.30, g: 0.30, b: 0.30, a: 1.0 };

    let mut type_options: Vec<String> = vec![QSELECT_ANY_TYPE.to_string()];
    type_options.extend(types.iter().map(|s| (*s).to_string()));

    let mut prop_options: Vec<crate::app::QSelectPropertyChoice> =
        vec![crate::app::QSelectPropertyChoice {
            field: String::new(),
            label: QSELECT_ANY_PROP.to_string(),
        }];
    prop_options.extend(properties.iter().map(|(field, label)| {
        crate::app::QSelectPropertyChoice {
            field: field.clone(),
            label: label.clone(),
        }
    }));

    let op_options: Vec<crate::app::QSelectOp> = vec![
        crate::app::QSelectOp::Eq,
        crate::app::QSelectOp::Neq,
        crate::app::QSelectOp::Gt,
        crate::app::QSelectOp::Lt,
        crate::app::QSelectOp::Any,
    ];

    let type_sel = state
        .type_filter
        .clone()
        .unwrap_or_else(|| QSELECT_ANY_TYPE.to_string());
    let prop_sel = state.property.clone().unwrap_or(crate::app::QSelectPropertyChoice {
        field: String::new(),
        label: QSELECT_ANY_PROP.to_string(),
    });

    // The value field is disabled (visually de-emphasised; we still
    // render the same widget) when no property is picked or the
    // operator is "*Any value" — both of those skip the value test.
    let value_enabled = state.property.is_some()
        && !matches!(state.operator, crate::app::QSelectOp::Any);

    let label = |s: &'static str| text(s).size(12).color(TEXT).width(iced::Length::Fixed(90.0));

    let btn = |lbl: &'static str, msg: Message, base: Color, hov: Color| {
        button(text(lbl).size(12).color(TEXT))
            .on_press(msg)
            .style(move |_: &Theme, st| button::Style {
                background: Some(Background::Color(
                    if matches!(st, button::Status::Hovered | button::Status::Pressed) {
                        hov
                    } else {
                        base
                    },
                )),
                text_color: TEXT,
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .padding([4, 14])
    };

    let mut value_input = text_input("", &state.value).size(12);
    if value_enabled {
        value_input = value_input.on_input(Message::QSelectSetValue);
    }

    let panel_body = column![
        text("Quick Select").size(14).color(TEXT),
        Space::new().height(10),
        row![
            label("Object type:"),
            pick_list(type_options, Some(type_sel), |s: String| {
                if s == QSELECT_ANY_TYPE {
                    Message::QSelectSetType(None)
                } else {
                    Message::QSelectSetType(Some(s))
                }
            })
            .width(Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
        Space::new().height(6),
        row![
            label("Property:"),
            pick_list(prop_options, Some(prop_sel), |p: crate::app::QSelectPropertyChoice| {
                if p.field.is_empty() {
                    Message::QSelectSetProperty(None)
                } else {
                    Message::QSelectSetProperty(Some(p))
                }
            })
            .width(Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
        Space::new().height(6),
        row![
            label("Operator:"),
            pick_list(op_options, Some(state.operator), Message::QSelectSetOperator)
                .width(Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
        Space::new().height(6),
        row![label("Value:"), value_input,]
            .align_y(iced::Alignment::Center)
            .spacing(8),
        Space::new().height(10),
        row![
            checkbox(state.append)
                .on_toggle(Message::QSelectSetAppend)
                .size(14),
            Space::new().width(6),
            text("Append to current selection").size(12).color(TEXT),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(14),
        row![
            Space::new().width(Fill),
            btn("Cancel", Message::QSelectClose, BTN_BG, BTN_HOV),
            Space::new().width(8),
            btn("Apply", Message::QSelectApply, BTN_OK, BTN_OK_HOV),
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(0);

    let panel = container(panel_body)
        .padding(16)
        .width(iced::Length::Fixed(400.0))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BG)),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

    // Outside-click catcher — fills the whole screen, sits below the
    // panel. The panel itself is rendered above and absorbs its own
    // clicks via standard widget event handling.
    let catcher = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::QSelectClose)
    .on_right_press(Message::QSelectClose);

    let centered = container(iced::widget::opaque(panel))
        .center(Fill);

    stack![catcher, centered].into()
}

/// Content for the floating "Unsaved Changes" OS window.
const SAVE_FORMAT_OPTIONS: &[&str] = &[
    "DWG 2018", "DWG 2013", "DWG 2010", "DWG 2007", "DWG 2004", "DWG 2000", "DWG R14", "DXF 2018",
    "DXF 2013", "DXF 2010", "DXF 2007", "DXF 2004", "DXF 2000", "DXF R14",
];

fn save_as_dialog_window<'a>(
    filename: &'a str,
    folder: &'a std::path::Path,
    entries: &'a [(String, bool, std::path::PathBuf)],
    format: &'a str,
) -> Element<'a, Message> {
    const BG: Color = Color {
        r: 0.15,
        g: 0.15,
        b: 0.17,
        a: 1.0,
    };
    const LIST_BG: Color = Color {
        r: 0.11,
        g: 0.11,
        b: 0.13,
        a: 1.0,
    };
    const BORDER: Color = Color {
        r: 0.32,
        g: 0.32,
        b: 0.36,
        a: 1.0,
    };
    const TEXT: Color = Color {
        r: 0.90,
        g: 0.90,
        b: 0.90,
        a: 1.0,
    };
    const DIM: Color = Color {
        r: 0.58,
        g: 0.58,
        b: 0.62,
        a: 1.0,
    };
    const INPUT_BG: Color = Color {
        r: 0.10,
        g: 0.10,
        b: 0.12,
        a: 1.0,
    };
    const BTN_OK: Color = Color {
        r: 0.20,
        g: 0.46,
        b: 0.80,
        a: 1.0,
    };
    const BTN_HOV: Color = Color {
        r: 0.26,
        g: 0.55,
        b: 0.92,
        a: 1.0,
    };
    const BTN_GREY: Color = Color {
        r: 0.26,
        g: 0.26,
        b: 0.29,
        a: 1.0,
    };
    const BTN_GHOV: Color = Color {
        r: 0.34,
        g: 0.34,
        b: 0.38,
        a: 1.0,
    };
    const DIR_COL: Color = Color {
        r: 0.75,
        g: 0.85,
        b: 1.00,
        a: 1.0,
    };
    const FILE_COL: Color = TEXT;
    const ROW_HOV: Color = Color {
        r: 0.22,
        g: 0.24,
        b: 0.28,
        a: 1.0,
    };

    let input_sty =
        |_: &Theme, _: iced::widget::text_input::Status| iced::widget::text_input::Style {
            background: Background::Color(INPUT_BG),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            icon: TEXT,
            placeholder: DIM,
            value: TEXT,
            selection: Color {
                r: 0.20,
                g: 0.46,
                b: 0.80,
                a: 0.45,
            },
        };

    let btn = |lbl: &'static str, msg: Message, base: Color, hov: Color| {
        button(text(lbl).size(12).color(TEXT))
            .on_press(msg)
            .style(move |_: &Theme, st| button::Style {
                background: Some(Background::Color(
                    if matches!(st, button::Status::Hovered | button::Status::Pressed) {
                        hov
                    } else {
                        base
                    },
                )),
                text_color: TEXT,
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .padding([4, 12])
    };

    // ── Path bar ─────────────────────────────────────────────────────────
    let path_str = folder.to_string_lossy().into_owned();
    let up_path = folder.parent().map(|p| p.to_path_buf());
    let path_bar = row![
        {
            let up_msg = up_path.map(Message::SaveDialogNavigate);
            let b = button(text("↑").size(14).color(TEXT))
                .style(|_: &Theme, st| button::Style {
                    background: Some(Background::Color(
                        if matches!(st, button::Status::Hovered | button::Status::Pressed) {
                            BTN_GHOV
                        } else {
                            BTN_GREY
                        },
                    )),
                    text_color: TEXT,
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .padding([3, 10]);
            if let Some(msg) = up_msg {
                b.on_press(msg)
            } else {
                b
            }
        },
        Space::new().width(8),
        container(text(path_str.clone()).size(12).color(DIM))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(INPUT_BG)),
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: 4.0.into()
                },
                ..Default::default()
            })
            .padding([4, 8])
            .width(Fill),
    ]
    .align_y(iced::Alignment::Center);

    // ── File list ─────────────────────────────────────────────────────────
    let file_list: Element<'_, Message> = {
        let rows: Vec<Element<'_, Message>> = entries
            .iter()
            .map(|(name, is_dir, path)| {
                let icon = if *is_dir { "📁" } else { "📄" };
                let color = if *is_dir { DIR_COL } else { FILE_COL };
                let p = path.clone();
                let d = *is_dir;
                mouse_area(
                    container(
                        row![
                            text(icon).size(13),
                            Space::new().width(6),
                            text(name.as_str()).size(13).color(color),
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .style(|_: &Theme| container::Style {
                        ..Default::default()
                    })
                    .padding([3, 8])
                    .width(Fill),
                )
                .on_press(Message::SaveDialogEntryClicked(p, d))
                .into()
            })
            .collect();

        container(iced::widget::scrollable(
            column(rows).spacing(1).width(Fill),
        ))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(LIST_BG)),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
    };
    let _ = ROW_HOV; // used conceptually, suppress warning

    let sel_fmt = SAVE_FORMAT_OPTIONS.iter().copied().find(|&s| s == format);
    let label = |s: &'static str| text(s).size(11).color(DIM);

    // ── Bottom controls ───────────────────────────────────────────────────
    let bottom = column![
        row![
            label("File name:").width(90),
            text_input("drawing.dwg", filename)
                .on_input(Message::SaveDialogFilenameChanged)
                .style(input_sty)
                .size(13)
                .padding([5, 8])
                .width(Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(6),
        Space::new().height(6),
        row![
            label("Format:").width(90),
            pick_list(SAVE_FORMAT_OPTIONS, sel_fmt, |s: &str| {
                Message::SaveDialogFormatChanged(s.to_string())
            })
            .width(Fill),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(6),
        Space::new().height(12),
        row![
            Space::new().width(Fill),
            btn("Save", Message::SaveDialogConfirm, BTN_OK, BTN_HOV),
            Space::new().width(8),
            btn("Cancel", Message::SaveDialogCancel, BTN_GREY, BTN_GHOV),
        ],
    ]
    .spacing(0);

    container(
        column![
            path_bar,
            Space::new().height(8),
            file_list,
            Space::new().height(10),
            bottom,
        ]
        .spacing(0),
    )
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(BG)),
        ..Default::default()
    })
    .padding([14, 16])
    .width(Fill)
    .height(Fill)
    .into()
}

fn unsaved_changes_dialog_window(name: &str) -> Element<'static, Message> {
    const BG: Color = Color {
        r: 0.18,
        g: 0.18,
        b: 0.20,
        a: 1.0,
    };
    const BORDER_COL: Color = Color {
        r: 0.38,
        g: 0.38,
        b: 0.42,
        a: 1.0,
    };
    const TEXT_COL: Color = Color {
        r: 0.90,
        g: 0.90,
        b: 0.90,
        a: 1.0,
    };
    const BTN_SAVE: Color = Color {
        r: 0.20,
        g: 0.46,
        b: 0.80,
        a: 1.0,
    };
    const BTN_HOVER: Color = Color {
        r: 0.26,
        g: 0.55,
        b: 0.92,
        a: 1.0,
    };
    const BTN_DISC: Color = Color {
        r: 0.28,
        g: 0.28,
        b: 0.30,
        a: 1.0,
    };
    const BTN_DHOV: Color = Color {
        r: 0.36,
        g: 0.36,
        b: 0.40,
        a: 1.0,
    };

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
                border: Border {
                    color: BORDER_COL,
                    width: 1.0,
                    radius: 4.0.into(),
                },
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
                btn("Save", Message::UnsavedDialogSave, BTN_SAVE, BTN_HOVER),
                iced::widget::Space::new().width(8),
                btn("Discard", Message::UnsavedDialogDiscard, BTN_DISC, BTN_DHOV),
                iced::widget::Space::new().width(8),
                btn("Cancel", Message::UnsavedDialogCancel, BTN_DISC, BTN_DHOV),
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

// ── Start / Welcome page ──────────────────────────────────────────────────
//
// Renders in place of the model-space viewport when the active tab is the
// fixed Start tab (`DocumentTab::is_start`). English-only by design — this
// is the public welcome screen and stays consistent across locales.
//
// The page picks up the application icon's red-brown (#B03020) as a tint so
// it visually belongs to OpenCADStudio without overpowering the dark workspace.

const BRAND: Color = Color { r: 0.690, g: 0.188, b: 0.125, a: 1.0 }; // #B03020
const BRAND_DARK: Color = Color { r: 0.45, g: 0.12, b: 0.08, a: 1.0 };

pub(super) fn start_page_view<'a>() -> Element<'a, Message> {
    const TEXT: Color = Color { r: 0.94, g: 0.93, b: 0.92, a: 1.0 };
    const MUTED: Color = Color { r: 0.62, g: 0.62, b: 0.62, a: 1.0 };
    const CARD_BG: Color = Color { r: 0.12, g: 0.12, b: 0.13, a: 1.0 };
    const CARD_BORDER: Color = Color { r: 0.20, g: 0.20, b: 0.22, a: 1.0 };

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
                    button::Status::Hovered => Color { r: 0.18, g: 0.18, b: 0.20, a: 1.0 },
                    _ => Color { r: 0.13, g: 0.13, b: 0.15, a: 1.0 },
                })),
                text_color: TEXT,
                border: Border {
                    color: Color { r: 0.30, g: 0.30, b: 0.33, a: 1.0 },
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
                text("♥ ").size(15).color(Color::WHITE),
                text("Donate").size(14).color(Color::WHITE),
            ]
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
                color: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.4 },
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

    let secondary_row = row![
        outline_btn(
            "Contribute",
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
        outline_btn("About", Message::AboutOpen),
    ]
    .spacing(12)
    .align_y(iced::Center);

    let card_section = |heading: &'static str, sub: &'static str, body: &'static str| {
        container(
            column![
                text(heading).size(20).color(TEXT),
                text(sub).size(13).color(MUTED),
                Space::new().height(iced::Length::Fixed(10.0)),
                text(body).size(12).color(MUTED),
            ]
            .spacing(6),
        )
        .padding(20)
        .width(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: CARD_BORDER,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
    };

    let cards = row![
        card_section(
            "Free and open source",
            "Open CAD Studio belongs to its users.",
            "Source is openly developed and distributed under a permissive licence. \
             You are free to use, study, modify, and share Open CAD Studio.",
        ),
        card_section(
            "Community driven",
            "Be part of the story.",
            "Anyone can contribute — translations, bug reports, feature ideas, code, \
             or simply spreading the word. Your involvement shapes the project.",
        ),
    ]
    .spacing(16);

    // Buttons sit on a transparent container with a large, brand-tinted
    // ambient shadow (offset = 0, big blur) — produces a soft halo behind
    // the action row, matching the Thunderbird coloured-glow look against
    // the dark page.
    let primary_glow = container(primary_row)
        .padding(iced::Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            shadow: iced::Shadow {
                color: Color { r: BRAND.r, g: BRAND.g, b: BRAND.b, a: 0.45 },
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
    const PAGE_BG: Color = Color { r: 0.08, g: 0.08, b: 0.085, a: 1.0 };
    container(content)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(PAGE_BG)),
            ..Default::default()
        })
        .padding(iced::Padding { top: 40.0, right: 60.0, bottom: 40.0, left: 60.0 })
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
    const PANEL_BG: Color = Color { r: 0.10, g: 0.10, b: 0.11, a: 1.0 };
    const PANEL_BORDER: Color = Color { r: 0.18, g: 0.18, b: 0.20, a: 1.0 };
    const ITEM_HOVER: Color = Color { r: 0.16, g: 0.16, b: 0.18, a: 1.0 };
    const TEXT: Color = Color { r: 0.92, g: 0.91, b: 0.90, a: 1.0 };
    const MUTED: Color = Color { r: 0.60, g: 0.60, b: 0.62, a: 1.0 };

    let header = container(text("Recent Documents").size(11).color(MUTED))
        .padding(iced::Padding { top: 12.0, right: 14.0, bottom: 8.0, left: 14.0 });

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
                    text(name).size(12).color(TEXT),
                    text(dir).size(10).color(MUTED),
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
            let remove_btn = button(text("×").size(12).color(MUTED))
                .on_press(Message::RecentRemove(path_for_remove))
                .padding([4, 8])
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered => Color { r: 0.45, g: 0.15, b: 0.15, a: 1.0 },
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

            col = col.push(
                row![open_btn, remove_btn]
                    .spacing(0)
                    .align_y(iced::Center),
            );
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

// ── Render-mode picker ──────────────────────────────────────────────────────

/// The 7-way visual-style `pick_list`, styled as a dark overlay chip.
/// Shared by the model-space viewport (top-left) and each active
/// paper-space viewport. Emits `SetRenderMode`, which the update loop
/// routes to the active viewport entity or the model-layout tab.
fn render_mode_picker<'a>(
    current: acadrust::entities::ViewportRenderMode,
) -> Element<'a, Message> {
    use acadrust::entities::ViewportRenderMode as M;
    let render_modes: Vec<RenderModeChoice> = vec![
        RenderModeChoice(M::Wireframe2D),
        RenderModeChoice(M::Wireframe3D),
        RenderModeChoice(M::HiddenLine),
        RenderModeChoice(M::FlatShaded),
        RenderModeChoice(M::GouraudShaded),
        RenderModeChoice(M::FlatShadedWithEdges),
        RenderModeChoice(M::GouraudShadedWithEdges),
    ];
    container(
        iced::widget::pick_list(render_modes, Some(RenderModeChoice(current)), |c| {
            Message::SetRenderMode(c.0)
        })
        .text_size(11),
    )
    .style(|_: &Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color {
            r: 0.10,
            g: 0.10,
            b: 0.10,
            a: 0.75,
        })),
        border: iced::Border {
            color: Color {
                r: 0.35,
                g: 0.35,
                b: 0.35,
                a: 1.0,
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(Color {
            r: 0.85,
            g: 0.85,
            b: 0.85,
            a: 1.0,
        }),
        ..Default::default()
    })
    .padding([4, 8])
    .into()
}

// ── Dynamic-input field formatting ─────────────────────────────────────────

/// Short prefix shown before a dynamic-input box's value.
fn dyn_component_label(c: DynComponent) -> String {
    match c {
        DynComponent::X => "X".into(),
        DynComponent::Y => "Y".into(),
        DynComponent::Z => "Z".into(),
        DynComponent::Distance => "d".into(),
        DynComponent::Angle => "<".into(),
    }
}

/// The string shown inside a dynamic-input box: the typed buffer when the
/// field is locked, otherwise the live value derived from the cursor
/// world position (and the base point for polar quantities).
fn dyn_component_value(
    f: &DynFieldEntry,
    w: glam::Vec3,
    base: Option<glam::Vec3>,
) -> String {
    if let Some(b) = &f.buffer {
        return b.clone();
    }
    let b = base.unwrap_or(glam::Vec3::ZERO);
    let dx = (w.x - b.x) as f64;
    let dy = (w.y - b.y) as f64;
    // When a base point exists (DYN-on after the first pick) the cartesian
    // fields show relative deltas — matching the typed-value convention
    // in `dyn_resolve_point` so the live preview and the committed
    // coordinate use the same frame. See #35.
    let has_base = base.is_some();
    match f.component {
        DynComponent::X if has_base => format!("{:.4}", dx),
        DynComponent::Y if has_base => format!("{:.4}", dy),
        DynComponent::Z if has_base => "0.0000".to_string(),
        DynComponent::X => format!("{:.4}", w.x),
        DynComponent::Y => format!("{:.4}", w.y),
        DynComponent::Z => format!("{:.4}", b.z),
        DynComponent::Distance => format!("{:.4}", (dx * dx + dy * dy).sqrt()),
        DynComponent::Angle => format!("{:.1}", dy.atan2(dx).to_degrees().rem_euclid(360.0)),
    }
}
