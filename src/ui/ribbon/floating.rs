//! Ribbon panels floating over the drawing (`RibbonLayout::floating`): the
//! layer that holds them and each panel's frame.
//!
//! The layer keeps its panels in a fixed order in the widget tree, so a
//! panel's widget state stays with it when it is raised, and draws them and
//! hands them the pointer by their stacking order. Away from the panels it
//! takes no pointer input, so the drawing under it works as usual.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _, Shell};
use iced::time::Instant;
use iced::widget::{button, container};
use iced::{
    Background, Border, Element, Event, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    Vector,
};

use super::draw_panel::{self, SlideLook};
use super::layout::FloatingPanel;
use super::{extras, floating_id, panel_frame, render_group, Ribbon, Strip};
use crate::app::Message;
use crate::modules::RibbonGroup;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::PosReport;

impl Ribbon {
    /// The floating panels with their tab's index and group, in the order
    /// `layout.floating` keeps them. A panel whose tab or group is gone is
    /// left out.
    pub(super) fn floating_groups(&self) -> Vec<(usize, &RibbonGroup, &FloatingPanel)> {
        self.layout
            .floating
            .iter()
            .filter_map(|panel| {
                let index = self
                    .modules
                    .iter()
                    .position(|module| module.id() == panel.module_id)?;
                let group = self
                    .ordered_groups(index)
                    .into_iter()
                    .find(|group| extras::panel_key(&panel.module_id, group.title) == panel.key)?;
                Some((index, group, panel))
            })
            .collect()
    }

    /// The layer of floating panels, over the drawing and under the ribbon's
    /// popups. The caller leaves it out on the Start tab, with Clean Screen
    /// and while the ribbon is closed.
    pub fn floating_layer(&self, show_block_palette: bool) -> Option<Element<'_, Message>> {
        let found = self.floating_groups();
        if found.is_empty() {
            return None;
        }
        let ts = self.toggle_state(show_block_palette);
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by_key(|&i| found[i].2.z);
        let keys: Vec<String> = found
            .iter()
            .map(|(_, _, panel)| panel.key.clone())
            .collect();
        let open = self
            .float_ui
            .open
            .as_ref()
            .and_then(|open| keys.iter().position(|key| key == open));
        let panels = found
            .iter()
            .map(|&(index, group, panel)| {
                let module_id = self.modules[index].id();
                Floater {
                    at: Point::new(panel.x, panel.y),
                    panel: self.floating_panel(module_id, group, panel, ts),
                    tab: self.float_tab(panel),
                }
            })
            .collect();
        Some(
            FloatLayer {
                panels,
                order,
                keys,
                open,
            }
            .into(),
        )
    }

    /// One floating panel: its tools at full size over its title strip, on
    /// `panel` with a 1 px `border` outline.
    fn floating_panel<'a>(
        &'a self,
        module_id: &str,
        group: &'a RibbonGroup,
        floating: &FloatingPanel,
        ts: super::widgets::ToggleState,
    ) -> Element<'a, Message> {
        let key = extras::panel_key(module_id, group.title);
        let style_ctx = self.style_context();
        let look = self.float_look(&key);
        let right = floating.extras_right;
        let body = render_group(
            false,
            group,
            &key,
            Strip::Bare,
            SlideLook::Closed,
            Vec::new(),
            self.lit_tool(),
            &self.open_dropdown,
            &self.last_cmd,
            ts,
            &self.layer_infos,
            &self.active_layer,
            self.active_color,
            &self.active_linetype,
            self.active_lineweight,
            &style_ctx,
        );
        // Open and unpinned, the body is lit like its extra tools.
        let body = container(body).style(move |theme: &Theme| container::Style {
            background: (look == SlideLook::Open)
                .then(|| Background::Color(chrome::open_panel(theme))),
            ..Default::default()
        });
        let body = PosReport::owned(body_id(&key), body);
        let extras = draw_panel::panel_for_group(&key);
        let (content, edge, on_click): (Element<'a, Message>, Edge, Option<Message>) =
            match (extras, look) {
                // The title at the side: a strip on the right, or the band
                // of extra tools with its own strip at the far right.
                (Some(panel), SlideLook::Open | SlideLook::Pinned) if right => {
                    let pinned = look == SlideLook::Pinned;
                    let band = self.float_band(panel, pinned);
                    let strip =
                        side_strip(group.title, Some(true), draw_panel::launcher(panel), look);
                    (
                        iced::widget::row![body, band, strip].into(),
                        Edge::Right(metrics::FLOAT_SIDE_STRIP_W),
                        Some(Message::RibbonFloat(FloatMsg::Collapse(key.clone()))),
                    )
                }
                _ if right => {
                    let launcher = extras::for_panel(&key).and_then(draw_panel::launcher);
                    let strip = side_strip(
                        group.title,
                        extras.map(|_| false),
                        launcher,
                        SlideLook::Closed,
                    );
                    (
                        iced::widget::row![body, strip].into(),
                        Edge::Right(metrics::FLOAT_SIDE_STRIP_W),
                        extras.map(|_| Message::RibbonFloat(FloatMsg::ToggleExtras(key.clone()))),
                    )
                }
                (Some(panel), SlideLook::Open | SlideLook::Pinned) => {
                    // The extra tools continue the panel downward, over its
                    // title strip; their footer is the title now.
                    let width = crate::ui::wrap_bar::recent_bounds(&body_id(&key))
                        .map_or(metrics::FLOAT_EXTRAS_MIN_W, |b| b.width);
                    let pinned = look == SlideLook::Pinned;
                    // As wide as the panel: the layer lays its panels out
                    // unbounded, where a filling width has no end.
                    let tools = container(draw_panel::extra_tools(self, panel, width))
                        .width(width)
                        .style(move |theme: &Theme| container::Style {
                            background: Some(Background::Color(if pinned {
                                chrome::panel(theme)
                            } else {
                                chrome::open_panel(theme)
                            })),
                            ..Default::default()
                        });
                    let footer = self.float_footer(&key, panel, pinned, width);
                    (
                        iced::widget::column![body, tools, footer].into(),
                        Edge::Bottom(draw_panel::FOOTER_H),
                        Some(Message::RibbonFloat(FloatMsg::Collapse(key.clone()))),
                    )
                }
                _ => {
                    let title =
                        draw_panel::group_title(group.title, &key, SlideLook::Closed, false);
                    let hovered = self.float_ui.strip_hover.as_deref() == Some(key.as_str());
                    // The centred title moves right just enough to clear
                    // the buttons the hovered strip shows at its left.
                    let title_row: Element<'a, Message> = match title_shift(
                        hovered,
                        group.title,
                        extras.is_some(),
                        crate::ui::wrap_bar::recent_bounds(&body_id(&key)).map(|b| b.width),
                    ) {
                        Some(shift) => {
                            iced::widget::row![iced::widget::Space::new().width(shift), title.row]
                                .into()
                        }
                        None => title.row,
                    };
                    let framed = panel_frame::PanelFrame::new(
                        body.into(),
                        metrics::TITLE_STRIP_H,
                        title.launcher,
                    )
                    .title(Some(title_row));
                    // Hovering the strip shows Return to ribbon and the
                    // orientation toggle at its left end.
                    let framed: Element<'a, Message> = if hovered {
                        let buttons = container(self.place_buttons(floating))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_left(Length::Fill)
                            .align_bottom(Length::Fill)
                            .padding(Padding {
                                left: metrics::FLOAT_HOVER_INSET,
                                bottom: (metrics::TITLE_STRIP_H - metrics::FLOAT_HOVER_HIT) / 2.0,
                                ..Padding::ZERO
                            });
                        iced::widget::stack![framed, buttons].into()
                    } else {
                        framed.into()
                    };
                    (
                        framed,
                        Edge::Bottom(metrics::TITLE_STRIP_H),
                        // Only a panel with extra tools opens them.
                        extras.map(|_| Message::RibbonFloat(FloatMsg::ToggleExtras(key.clone()))),
                    )
                }
            };
        // Outlined all round, extra tools included: the content sits inside
        // the 1 px line, which it would otherwise cover on every side it
        // fills. Over the tool area, where a release docks it, the line is
        // `accent`.
        let docking = self
            .float_ui
            .drag
            .as_ref()
            .is_some_and(|drag| drag.key == key && drag.over_ribbon);
        let framed = container(content)
            .padding(crate::ui::icons::hairline())
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::panel(theme))),
                border: Border {
                    color: if docking {
                        chrome::accent(theme)
                    } else {
                        chrome::border(theme)
                    },
                    width: crate::ui::icons::hairline(),
                    radius: 0.0.into(),
                },
                ..Default::default()
            });
        // The pointer on the panel or its extra tools keeps them open.
        let watched = iced::widget::mouse_area(framed)
            .on_enter(Message::RibbonFloat(FloatMsg::Hover {
                key: key.clone(),
                over: true,
            }))
            .on_exit(Message::RibbonFloat(FloatMsg::Hover {
                key: key.clone(),
                over: false,
            }));
        let grip = Grip {
            child: watched.into(),
            key: key.clone(),
            edge,
            on_click,
            threshold: metrics::FLOAT_MOVE_THRESHOLD,
        };
        PosReport::owned(floating_id(&key), grip).into()
    }

    /// The extra tools of `panel` as a band to the right of a panel whose
    /// title is at its side: three rows of buttons, the tool groups side by
    /// side with a line between them, as high as the panel.
    fn float_band<'a>(
        &self,
        panel: &'static extras::PanelExtras,
        pinned: bool,
    ) -> Element<'a, Message> {
        let active = self.lit_tool().as_deref();
        let cell = metrics::FLOAT_BAND_BUTTON;
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        let separator = || -> Element<'a, Message> {
            container(
                iced::widget::Space::new()
                    .width(crate::ui::icons::hairline())
                    .height(Length::Fill),
            )
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::border(theme))),
                ..Default::default()
            })
            .padding([metrics::FLOAT_BAND_PAD, 0.0])
            .into()
        };
        let gap = || -> Element<'a, Message> {
            iced::widget::Space::new()
                .width(metrics::FLOAT_BAND_GROUP_GAP)
                .into()
        };
        for group in panel.more.iter().filter(|group| !group.tools.is_empty()) {
            if !parts.is_empty() {
                parts.extend([gap(), separator(), gap()]);
            }
            let columns = group.tools.chunks(3).map(|chunk| {
                iced::widget::Column::with_children(chunk.iter().map(|tool| {
                    draw_panel::sized_tool_button(tool, tool.is_active(active), panel.id, cell)
                }))
                .spacing(metrics::GAP)
                .into()
            });
            parts.push(
                iced::widget::Row::with_children(columns)
                    .spacing(metrics::GAP)
                    .into(),
            );
        }
        if let Some(special) = draw_panel::extra_special(self, panel) {
            if !parts.is_empty() {
                parts.extend([gap(), separator(), gap()]);
            }
            parts.push(
                container(special)
                    .width(metrics::FLOAT_BAND_LIST_W)
                    .height(Length::Fill)
                    .clip(true)
                    .into(),
            );
        }
        container(iced::widget::Row::with_children(parts).height(Length::Fill))
            .height(metrics::CONTENT_H)
            .center_y(metrics::CONTENT_H)
            .padding([0.0, metrics::FLOAT_BAND_PAD])
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(if pinned {
                    chrome::panel(theme)
                } else {
                    chrome::open_panel(theme)
                })),
                ..Default::default()
            })
            .into()
    }

    /// Return to ribbon and the orientation toggle of floating `panel`.
    fn place_buttons<'a>(&self, panel: &FloatingPanel) -> Element<'a, Message> {
        let key = panel.key.clone();
        let (toggle, tip) = if panel.extras_right {
            (
                TO_BOTTOM_ICON,
                crate::tr!("ribbon-menu", "float-title-bottom"),
            )
        } else {
            (TO_SIDE_ICON, crate::tr!("ribbon-menu", "float-title-side"))
        };
        iced::widget::row![
            hover_button(
                RETURN_ICON,
                crate::tr!("ribbon-menu", "return-to-ribbon"),
                FloatMsg::Return(key.clone()),
            ),
            hover_button(toggle, tip, FloatMsg::ToggleOrientation(key)),
        ]
        .spacing(metrics::FLOAT_HOVER_GAP)
        .into()
    }

    /// The tab above the top-right corner of a floating `panel` whose title
    /// is at its side, shown while the pointer is on the side strip or the
    /// tab: Return to ribbon and the orientation toggle while the extra
    /// tools are closed, the pin while they are open. Empty otherwise.
    fn float_tab<'a>(&self, panel: &FloatingPanel) -> Element<'a, Message> {
        let key = panel.key.as_str();
        let hovered = self.float_ui.strip_hover.as_deref() == Some(key)
            || self.float_ui.tab_hover.as_deref() == Some(key);
        if !panel.extras_right || !hovered {
            return iced::widget::Space::new().into();
        }
        let look = self.float_look(key);
        let content: Element<'a, Message> = match look {
            SlideLook::Closed => self.place_buttons(panel),
            SlideLook::Open | SlideLook::Pinned => {
                let pinned = look == SlideLook::Pinned;
                let pin = button(crate::ui::icons::chrome(
                    if pinned {
                        crate::ui::icons::PIN
                    } else {
                        crate::ui::icons::PIN_ACTIVE
                    },
                    metrics::PIN_ICON,
                ))
                .on_press(Message::RibbonFloat(FloatMsg::TogglePin(key.to_string())))
                .style(hover_button_style)
                .width(metrics::FLOAT_HOVER_HIT)
                .height(metrics::FLOAT_HOVER_HIT)
                .padding((metrics::FLOAT_HOVER_HIT - metrics::PIN_ICON) / 2.0);
                pin.into()
            }
        };
        // Sized exactly: the layer lays panels out unbounded, where a
        // filling length has no end.
        let buttons = if look == SlideLook::Closed { 2.0 } else { 1.0 };
        let inner_w = buttons * metrics::FLOAT_HOVER_HIT
            + (buttons - 1.0) * metrics::FLOAT_HOVER_GAP
            + 2.0 * metrics::FLOAT_TAB_PAD;
        let inner_h = metrics::FLOAT_HOVER_HIT + 2.0 * metrics::FLOAT_TAB_PAD;
        let hair = crate::ui::icons::hairline();
        let line = |width: f32, height: f32| -> Element<'a, Message> {
            container(iced::widget::Space::new().width(width).height(height))
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::border(theme))),
                    ..Default::default()
                })
                .into()
        };
        // `title_bar`, outlined in `border` on three sides: it opens onto
        // the panel's edge.
        let body = container(content)
            .padding(metrics::FLOAT_TAB_PAD)
            .width(inner_w)
            .height(inner_h)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::title_bar(theme))),
                ..Default::default()
            });
        let tab = iced::widget::row![
            line(hair, inner_h + hair),
            iced::widget::column![line(inner_w, hair), body],
            line(hair, inner_h + hair),
        ];
        iced::widget::mouse_area(tab)
            .on_enter(Message::RibbonFloat(FloatMsg::TabHover {
                key: key.to_string(),
                over: true,
            }))
            .on_exit(Message::RibbonFloat(FloatMsg::TabHover {
                key: key.to_string(),
                over: false,
            }))
            .into()
    }

    /// How the extra tools of floating panel `key` show.
    pub(super) fn float_look(&self, key: &str) -> SlideLook {
        if draw_panel::panel_for_group(key).is_none() {
            SlideLook::Closed
        } else if self
            .layout
            .floating
            .iter()
            .any(|panel| panel.key == key && panel.pinned)
        {
            SlideLook::Pinned
        } else if self.float_ui.open.as_deref() == Some(key) {
            SlideLook::Open
        } else {
            SlideLook::Closed
        }
    }

    /// The footer of a floating panel's extra tools: the pin (while the
    /// pointer is on the footer), the title with ▴, and the dialog launcher.
    /// A click on the title closes and unpins them; a drag on it moves the
    /// panel (the panel's grip does both).
    fn float_footer<'a>(
        &self,
        key: &str,
        panel: &'static extras::PanelExtras,
        pinned: bool,
        width: f32,
    ) -> Element<'a, Message> {
        let side = metrics::FLOAT_SIDE_STRIP_W;
        let hovered = self.float_ui.footer.as_deref() == Some(key);
        let pin: Element<'a, Message> = if hovered {
            iced::widget::button(crate::ui::icons::chrome(
                if pinned {
                    crate::ui::icons::PIN
                } else {
                    crate::ui::icons::PIN_ACTIVE
                },
                metrics::PIN_ICON,
            ))
            .on_press(Message::RibbonFloat(FloatMsg::TogglePin(key.to_string())))
            .style(|theme: &Theme, status| super::widgets::tool_btn_style(theme, false, status))
            .width(side)
            .padding(metrics::FLOAT_FOOTER_PIN_PAD)
            .into()
        } else {
            iced::widget::Space::new().width(side).into()
        };
        let title = iced::widget::row![
            iced::widget::text(crate::t!(panel.title()))
                .size(metrics::FLOAT_TITLE_SIZE)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: Some(if pinned {
                        chrome::title_text(theme)
                    } else {
                        chrome::strong(theme)
                    }),
                }),
            crate::ui::icons::dropup_arrow()
        ]
        .spacing(metrics::FLOAT_TITLE_GAP)
        .align_y(iced::Center);
        let launcher = draw_panel::launcher(panel)
            .unwrap_or_else(|| iced::widget::Space::new().width(side).into());
        let bar = container(
            iced::widget::row![
                pin,
                iced::widget::Space::new().width(Length::Fill),
                title,
                iced::widget::Space::new().width(Length::Fill),
                launcher
            ]
            .align_y(iced::Center)
            .padding(metrics::FLOAT_FOOTER_PAD),
        )
        .center_y(draw_panel::FOOTER_H)
        .width(width)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(if pinned {
                chrome::title_bar(theme)
            } else {
                chrome::hover(theme)
            })),
            text_color: Some(if pinned {
                chrome::title_text(theme)
            } else {
                chrome::strong(theme)
            }),
            ..Default::default()
        });
        iced::widget::mouse_area(bar)
            .on_enter(Message::RibbonFloat(FloatMsg::Footer(Some(
                key.to_string(),
            ))))
            .on_exit(Message::RibbonFloat(FloatMsg::Footer(None)))
            .into()
    }

    /// Close the unpinned extra tools the pointer left a while ago.
    pub(super) fn float_tick(&mut self, now: Instant) {
        if self.float_ui.closing.is_some_and(|at| now >= at) {
            self.float_ui.closing = None;
            self.float_ui.open = None;
        }
    }

    /// Whether unpinned extra tools are due to close.
    pub(super) fn float_closing(&self) -> bool {
        self.float_ui.closing.is_some()
    }

    /// Apply a floating panel message. Returns whether the layout changed
    /// and should be saved.
    pub fn on_float(&mut self, msg: FloatMsg, now: Instant) -> bool {
        match msg {
            FloatMsg::ToggleExtras(key) => {
                if self.float_ui.open.as_deref() == Some(key.as_str()) {
                    self.float_ui.open = None;
                } else if draw_panel::panel_for_group(&key).is_some() {
                    self.float_ui.open = Some(key);
                }
                self.float_ui.closing = None;
                false
            }
            FloatMsg::Collapse(key) => {
                if self.float_ui.open.as_deref() == Some(key.as_str()) {
                    self.float_ui.open = None;
                }
                self.float_ui.footer = None;
                self.float_ui.closing = None;
                self.set_float_pinned(&key, false)
            }
            FloatMsg::TogglePin(key) => {
                let pinned = self.float_look(&key) == SlideLook::Pinned;
                // A pinned panel's tools show on their own; unpinned, they
                // stay open under the pointer.
                self.float_ui.open = pinned.then(|| key.clone());
                self.float_ui.closing = None;
                self.set_float_pinned(&key, !pinned)
            }
            FloatMsg::Hover { key, over } => {
                if self.float_ui.open.as_deref() == Some(key.as_str()) {
                    self.float_ui.closing =
                        (!over).then(|| now + std::time::Duration::from_millis(EXTRAS_CLOSE_MS));
                }
                false
            }
            FloatMsg::Footer(key) => {
                self.float_ui.footer = key;
                false
            }
            FloatMsg::Outside => {
                self.float_ui.open = None;
                self.float_ui.closing = None;
                false
            }
            FloatMsg::StripHover { key, over } => {
                hover_flag(&mut self.float_ui.strip_hover, key, over);
                false
            }
            FloatMsg::TabHover { key, over } => {
                hover_flag(&mut self.float_ui.tab_hover, key, over);
                false
            }
            FloatMsg::Return(key) => self.dock_floating(&key),
            FloatMsg::ToggleOrientation(key) => {
                self.float_ui.strip_hover = None;
                self.float_ui.tab_hover = None;
                match self
                    .layout
                    .floating
                    .iter_mut()
                    .find(|panel| panel.key == key)
                {
                    Some(panel) => {
                        panel.extras_right ^= true;
                        true
                    }
                    None => false,
                }
            }
            FloatMsg::Raise(key) => {
                let top = self.top_float_z();
                // On top already only when no other panel is as high: a
                // hand-edited layout may give two the same order.
                let below = |panel: &FloatingPanel| {
                    self.layout
                        .floating
                        .iter()
                        .any(|other| other.key != panel.key && other.z >= panel.z)
                };
                let raise = self
                    .layout
                    .floating
                    .iter()
                    .find(|p| p.key == key)
                    .is_some_and(below);
                if raise {
                    if let Some(panel) = self.layout.floating.iter_mut().find(|p| p.key == key) {
                        panel.z = top + 1;
                    }
                    // Saved when the press lets go, not on every press.
                    self.float_ui.unsaved = true;
                }
                false
            }
            FloatMsg::Settle => std::mem::take(&mut self.float_ui.unsaved),
            FloatMsg::Press { key, at } => {
                self.float_ui.drag = self
                    .layout
                    .floating
                    .iter()
                    .find(|panel| panel.key == key)
                    .map(|panel| FloatDrag {
                        key,
                        grab: at - Point::new(panel.x, panel.y),
                        over_ribbon: false,
                        at,
                    });
                false
            }
            FloatMsg::Move { at, free } => {
                let over_ribbon = crate::ui::wrap_bar::recent_bounds(super::TOOL_AREA_ID)
                    .is_some_and(|area| area.contains(at));
                let Some(drag) = self.float_ui.drag.as_mut() else {
                    return false;
                };
                drag.over_ribbon = over_ribbon;
                drag.at = at;
                let drag = drag.clone();
                let corner = at - drag.grab;
                // Alt moves it freely; otherwise its edges snap to the other
                // floating panels, the window and the bottom of the ribbon.
                let corner = if free {
                    corner
                } else {
                    let size = Self::floating_size(&drag.key).unwrap_or_default();
                    let others: Vec<Rectangle> = self
                        .layout
                        .floating
                        .iter()
                        .filter(|panel| panel.key != drag.key)
                        .filter_map(|panel| {
                            crate::ui::wrap_bar::recent_bounds(&floating_id(&panel.key))
                        })
                        .collect();
                    let window = Rectangle::new(Point::ORIGIN, Size::new(self.win_w, self.win_h));
                    let ribbon_bottom = crate::ui::wrap_bar::recent_bounds(super::RIBBON_ID)
                        .map(|ribbon| ribbon.y + ribbon.height);
                    snap(
                        Rectangle::new(corner, Size::new(size.0, size.1)),
                        &others,
                        window,
                        ribbon_bottom,
                        metrics::FLOAT_SNAP,
                    )
                };
                if let Some(panel) = self.layout.floating.iter_mut().find(|p| p.key == drag.key) {
                    panel.x = corner.x;
                    panel.y = corner.y;
                }
                false
            }
            FloatMsg::Release { moved } => {
                let changed = match self.float_ui.drag.take() {
                    // Dropped on the tool area: back into the ribbon.
                    Some(drag) if moved && drag.over_ribbon => {
                        self.dock_floating_at(&drag.key, drag.at.x)
                    }
                    Some(_) => moved,
                    None => false,
                };
                changed | std::mem::take(&mut self.float_ui.unsaved)
            }
        }
    }

    /// Put floating panel `key` back in its tab's ribbon row, at its old
    /// place there; whether it was floating.
    pub(super) fn dock_floating(&mut self, key: &str) -> bool {
        let before = self.layout.floating.len();
        self.layout.floating.retain(|panel| panel.key != key);
        if self.float_ui.open.as_deref() == Some(key) {
            self.float_ui.open = None;
            self.float_ui.closing = None;
        }
        for hover in [&mut self.float_ui.strip_hover, &mut self.float_ui.tab_hover] {
            if hover.as_deref() == Some(key) {
                *hover = None;
            }
        }
        self.layout.floating.len() != before
    }

    /// Dock floating panel `key` dropped on the tool area at `x`: on the tab
    /// on show it goes in where it was dropped (the place a reorder would
    /// give it); a panel of another tab goes back to its old place there.
    fn dock_floating_at(&mut self, key: &str, x: f32) -> bool {
        let Some(panel) = self.layout.floating.iter().find(|panel| panel.key == key) else {
            return false;
        };
        let module_id = panel.module_id.clone();
        let index = self
            .modules
            .iter()
            .position(|module| module.id() == module_id);
        let title = key
            .split_once('/')
            .map_or(key, |(_, title)| title)
            .to_string();
        let row: Vec<String> = match index {
            Some(index) if index == self.active => self
                .groups_for(index)
                .map(|group| group.title.to_string())
                .collect(),
            _ => return self.dock_floating(key),
        };
        // Before every panel whose middle is right of the drop.
        let slot = row
            .iter()
            .filter(|other| {
                crate::ui::wrap_bar::recent_bounds(&super::row_panel_id(&module_id, other))
                    .is_some_and(|b| b.center_x() < x)
            })
            .count();
        self.dock_floating(key);
        let mut titles = row;
        titles.insert(slot.min(titles.len()), title);
        self.reorder_panels(&module_id, &titles);
        true
    }

    /// Pin or unpin the extra tools of floating panel `key`; whether that
    /// changed anything.
    fn set_float_pinned(&mut self, key: &str, pinned: bool) -> bool {
        match self
            .layout
            .floating
            .iter_mut()
            .find(|panel| panel.key == key)
        {
            Some(panel) if panel.pinned != pinned => {
                panel.pinned = pinned;
                true
            }
            _ => false,
        }
    }
}

/// A floating panel message.
#[derive(Clone, Debug)]
pub enum FloatMsg {
    /// A press on the panel with this key: it comes to the front.
    Raise(String),
    /// A press on its title strip, at this window point.
    Press { key: String, at: Point },
    /// The pointer moved far enough from the press: the panel follows it.
    /// `free` (Alt held) moves it without snapping.
    Move { at: Point, free: bool },
    /// The press let go; `moved` once it had moved the panel.
    Release { moved: bool },
    /// A press over a panel let go: save a raise it made.
    Settle,
    /// A click on the title strip: open or close the extra tools.
    ToggleExtras(String),
    /// A click on the extra tools' footer title: close and unpin them.
    Collapse(String),
    /// The footer's pin.
    TogglePin(String),
    /// The pointer entered or left the panel (with its extra tools).
    Hover { key: String, over: bool },
    /// The pointer is on the footer of this panel's extra tools.
    Footer(Option<String>),
    /// A press off the panel whose extra tools are open and unpinned.
    Outside,
    /// Put the title at the side and open the extra tools to the right, or
    /// back.
    ToggleOrientation(String),
    /// Return to ribbon: the panel goes back to its tab, in its old place.
    Return(String),
    /// The pointer entered or left the panel's title strip (or footer).
    StripHover { key: String, over: bool },
    /// The pointer entered or left the tab above the panel's corner.
    TabHover { key: String, over: bool },
}

/// What the floating panels' pointer and extra tools are doing; not saved.
#[derive(Default)]
pub(super) struct FloatUi {
    /// A panel being moved by its title strip.
    pub drag: Option<FloatDrag>,
    /// The panel whose extra tools are open and not pinned.
    pub open: Option<String>,
    /// The panel whose footer the pointer is on: its pin shows.
    pub footer: Option<String>,
    /// When the open extra tools close, the pointer having left them.
    pub closing: Option<Instant>,
    /// The panel whose title strip (or footer) the pointer is on.
    pub strip_hover: Option<String>,
    /// The panel whose corner tab the pointer is on.
    pub tab_hover: Option<String>,
    /// A panel was raised since the layout was last saved.
    pub unsaved: bool,
}

/// Where a floating panel moved to `moving` lands: each axis snaps on its
/// own to the nearest edge within `threshold` px. A panel snaps side by
/// side to another (their borders overlapping 1 px) or level with it
/// (top/top, bottom/bottom, left/left, right/right) when the two are near
/// across that axis; and to the edges of the `window` and the bottom of
/// the ribbon. `others` are the other panels' full extents, extra tools
/// included, as `moving`'s is.
pub(super) fn snap(
    moving: Rectangle,
    others: &[Rectangle],
    window: Rectangle,
    ribbon_bottom: Option<f32>,
    threshold: f32,
) -> Point {
    let hair = crate::ui::icons::hairline();
    let (w, h) = (moving.width, moving.height);
    // Whether `a..a_len` and `b..b_len` come within `threshold`.
    let near = |a: f32, a_len: f32, b: f32, b_len: f32| {
        a <= b + b_len + threshold && b <= a + a_len + threshold
    };
    let mut xs: Vec<f32> = vec![window.x, window.x + window.width - w];
    let mut ys: Vec<f32> = vec![window.y, window.y + window.height - h];
    ys.extend(ribbon_bottom);
    for other in others {
        if near(moving.y, h, other.y, other.height) {
            xs.extend([
                other.x - w + hair,
                other.x + other.width - hair,
                other.x,
                other.x + other.width - w,
            ]);
        }
        if near(moving.x, w, other.x, other.width) {
            ys.extend([
                other.y - h + hair,
                other.y + other.height - hair,
                other.y,
                other.y + other.height - h,
            ]);
        }
    }
    let nearest = |at: f32, candidates: &[f32]| {
        candidates
            .iter()
            .copied()
            .filter(|c| (c - at).abs() <= threshold)
            .min_by(|a, b| (a - at).abs().total_cmp(&(b - at).abs()))
            .unwrap_or(at)
    };
    Point::new(nearest(moving.x, &xs), nearest(moving.y, &ys))
}

/// The room to put before the centred title of a panel `width` wide (its
/// strip `hovered`) so it clears the hover buttons at the strip's left
/// end; `None` when it already does.
fn title_shift(hovered: bool, title: &str, chevron: bool, width: Option<f32>) -> Option<f32> {
    if !hovered {
        return None;
    }
    let width = width?;
    let buttons_end = metrics::FLOAT_HOVER_INSET
        + 2.0 * metrics::FLOAT_HOVER_HIT
        + metrics::FLOAT_HOVER_GAP
        + metrics::FLOAT_HOVER_GAP;
    let text = crate::ui::style::text_metrics::width(
        &crate::t!(title),
        super::widgets::PANEL_TITLE_SIZE,
        false,
    ) + if chevron {
        metrics::DROPDOWN_ARROW_W + metrics::FLOAT_TITLE_GAP
    } else {
        0.0
    };
    let start = (width - text) / 2.0;
    // Centred with a spacer before it, the title starts half the spacer
    // further right.
    (start < buttons_end).then_some(2.0 * (buttons_end - start))
}

/// Set `flag` to `key` while the pointer is `over` it; leaving clears it
/// only if it still names `key`.
fn hover_flag(flag: &mut Option<String>, key: String, over: bool) {
    if over {
        *flag = Some(key);
    } else if flag.as_deref() == Some(key.as_str()) {
        *flag = None;
    }
}

/// Return to ribbon: a ribbon bar with an arrow up into it.
const RETURN_ICON: &str = "M4 4h16v5H4z M12 21v-8 M8.5 16.5L12 13l3.5 3.5";
/// The orientation toggle, drawn as the state it switches to: title at
/// the side with tools to the right, or title at the bottom with tools
/// below.
const TO_SIDE_ICON: &str = "M4 5h9v14H4z M16 12h5 M18.5 9.5L21 12l-2.5 2.5";
const TO_BOTTOM_ICON: &str = "M5 4h14v9H5z M12 16v5 M9.5 18.5L12 21l2.5-2.5";

/// A hover-only button: a 14 px icon in a 16 px square, `muted`, `strong`
/// on `hover` under the pointer.
fn hover_button<'a>(icon: &'static str, tip: String, msg: FloatMsg) -> Element<'a, Message> {
    let btn = button(
        container(crate::ui::icons::line_icon(icon, metrics::FLOAT_HOVER_ICON))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(Message::RibbonFloat(msg))
    .style(hover_button_style)
    .width(metrics::FLOAT_HOVER_HIT)
    .height(metrics::FLOAT_HOVER_HIT)
    .padding(0);
    iced::widget::tooltip(
        btn,
        super::widgets::make_tip(tip),
        iced::widget::tooltip::Position::Top,
    )
    .delay(std::time::Duration::from_millis(400))
    .style(super::widgets::tip_style)
    .into()
}

fn hover_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: hovered.then(|| Background::Color(chrome::hover(theme))),
        text_color: if hovered {
            chrome::strong(theme)
        } else {
            chrome::muted(theme)
        },
        border: Border {
            radius: super::widgets::BUTTON_RADIUS.into(),
            ..Border::default()
        },
        ..Default::default()
    }
}

/// How long unpinned extra tools stay after the pointer leaves the panel.
const EXTRAS_CLOSE_MS: u64 = 450;

/// The id a floating panel's tools report their bounds under: the extra
/// tools are as wide.
fn body_id(key: &str) -> String {
    format!("floatbody:{key}")
}

/// A floating panel being moved by its title strip.
#[derive(Clone, Debug)]
pub(super) struct FloatDrag {
    pub key: String,
    /// The press point within the panel.
    pub grab: Vector,
    /// The pointer is over the ribbon's tool area: a release docks it.
    pub over_ribbon: bool,
    /// Where the pointer last was.
    pub at: Point,
}

/// Where on a panel its title strip is: along the bottom or the right, this
/// many px.
#[derive(Clone, Copy, Debug)]
enum Edge {
    Bottom(f32),
    Right(f32),
}

impl Edge {
    fn strip(self, bounds: Rectangle) -> Rectangle {
        match self {
            Edge::Bottom(h) => Rectangle {
                y: bounds.y + bounds.height - h,
                height: h,
                ..bounds
            },
            Edge::Right(w) => Rectangle {
                x: bounds.x + bounds.width - w,
                width: w,
                ..bounds
            },
        }
    }
}

/// A floating panel's title strip, `edge` of `child`: a press there that no
/// button inside took moves the panel once the pointer has gone
/// `threshold` px, and without that movement is a click (`on_click`).
/// The pointer stays the plain arrow.
struct Grip<'a> {
    child: Element<'a, Message>,
    key: String,
    edge: Edge,
    on_click: Option<Message>,
    threshold: f32,
}

#[derive(Default)]
struct GripState {
    press: Option<Point>,
    moving: bool,
    alt: bool,
    /// The pointer is on the strip.
    over: bool,
}

impl<'a> Widget<Message, Theme, Renderer> for Grip<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<GripState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(GripState::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let moving = tree.state.downcast_ref::<GripState>().moving;
        // While the panel moves, what is on it takes no pointer input.
        let child_cursor = if moving {
            mouse::Cursor::Unavailable
        } else {
            cursor
        };
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            child_cursor,
            renderer,
            shell,
            viewport,
        );
        let state = tree.state.downcast_mut::<GripState>();
        if let Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) = event {
            let over = cursor
                .position_over(self.edge.strip(layout.bounds()))
                .is_some();
            if over != state.over {
                state.over = over;
                shell.publish(Message::RibbonFloat(FloatMsg::StripHover {
                    key: self.key.clone(),
                    over,
                }));
            }
        }
        match event {
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.alt = modifiers.alt();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if shell.is_event_captured() {
                    return;
                }
                let Some(at) = cursor.position_over(self.edge.strip(layout.bounds())) else {
                    return;
                };
                state.press = Some(at);
                state.moving = false;
                shell.publish(Message::RibbonFloat(FloatMsg::Press {
                    key: self.key.clone(),
                    at,
                }));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let Some(press) = state.press else {
                    return;
                };
                if !state.moving && position.distance(press) >= self.threshold {
                    state.moving = true;
                }
                if state.moving {
                    shell.publish(Message::RibbonFloat(FloatMsg::Move {
                        at: *position,
                        free: state.alt,
                    }));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.press.take().is_none() {
                    return;
                }
                let moved = std::mem::take(&mut state.moving);
                shell.publish(Message::RibbonFloat(FloatMsg::Release { moved }));
                if !moved
                    && cursor
                        .position_over(self.edge.strip(layout.bounds()))
                        .is_some()
                {
                    if let Some(message) = self.on_click.clone() {
                        shell.publish(message);
                    }
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Grip<'a>> for Element<'a, Message> {
    fn from(grip: Grip<'a>) -> Self {
        Element::new(grip)
    }
}

/// The title strip at the side of a floating panel, as high as its tools:
/// the title written downward, cut off with "…" when it is too long (the
/// tooltip names it in full), with ▸ under it for a panel with extra tools
/// (◂ while they are open: `extras` is whether they are), and the dialog
/// launcher at the bottom. Coloured as the title strip or footer of `look`.
fn side_strip<'a>(
    title: &'static str,
    extras: Option<bool>,
    launcher: Option<Element<'a, Message>>,
    look: SlideLook,
) -> Element<'a, Message> {
    let label = crate::t!(title).into_owned();
    let vertical = iced::widget::canvas(VerticalTitle {
        label: label.clone(),
        arrow: extras,
        strong: look == SlideLook::Open,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let vertical = iced::widget::tooltip(
        vertical,
        super::widgets::make_tip(label),
        iced::widget::tooltip::Position::Right,
    )
    .delay(std::time::Duration::from_millis(400))
    .style(super::widgets::tip_style);
    let mut strip = iced::widget::column![vertical].align_x(iced::Center);
    if let Some(launcher) = launcher {
        strip = strip.push(launcher);
    }
    let strip = container(strip)
        .width(metrics::FLOAT_SIDE_STRIP_W - crate::ui::icons::hairline())
        .height(metrics::CONTENT_H)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(match look {
                SlideLook::Open => chrome::hover(theme),
                _ => chrome::title_bar(theme),
            })),
            ..Default::default()
        });
    // A line of `page` between the tools and the strip, as over the title
    // strip at the bottom.
    let line = container(
        iced::widget::Space::new()
            .width(crate::ui::icons::hairline())
            .height(metrics::CONTENT_H),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::page(theme))),
        ..Default::default()
    });
    iced::widget::row![line, strip].into()
}

/// A floating panel's title written downward in its side strip, with an
/// arrow under it when the panel has extra tools: ▸ to open them, ◂ while
/// they are open (`arrow`).
struct VerticalTitle {
    label: String,
    arrow: Option<bool>,
    /// In `strong` rather than `title_text`: the extra tools are open and
    /// not pinned.
    strong: bool,
}

impl VerticalTitle {
    /// `label` cut off with "…" so it is at most `room` px long.
    fn fitted(label: &str, room: f32) -> String {
        let size = metrics::FLOAT_TITLE_SIZE;
        let width = |text: &str| crate::ui::style::text_metrics::width(text, size, false);
        if width(label) <= room {
            return label.to_string();
        }
        let mut chars: Vec<char> = label.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let cut = format!("{}…", chars.iter().collect::<String>().trim_end());
            if width(&cut) <= room {
                return cut;
            }
        }
        "…".to_string()
    }
}

impl iced::widget::canvas::Program<Message> for VerticalTitle {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Text};
        let mut frame = Frame::new(renderer, bounds.size());
        let color = if self.strong {
            chrome::strong(theme)
        } else {
            chrome::title_text(theme)
        };
        let arrow_room = if self.arrow.is_some() {
            metrics::FLOAT_SIDE_ARROW + 2.0 * metrics::FLOAT_SIDE_ARROW_GAP
        } else {
            0.0
        };
        let room = (bounds.height - 2.0 * metrics::FLOAT_SIDE_TEXT_PAD - arrow_room).max(0.0);
        let label = Self::fitted(&self.label, room);
        let size = metrics::FLOAT_TITLE_SIZE;
        let length = crate::ui::style::text_metrics::width(&label, size, false);
        let top = metrics::FLOAT_SIDE_TEXT_PAD;
        let center_x = bounds.width / 2.0;
        frame.with_save(|frame| {
            frame.translate(Vector::new(center_x, top + length / 2.0));
            frame.rotate(std::f32::consts::FRAC_PI_2);
            frame.fill_text(Text {
                content: label,
                position: Point::ORIGIN,
                color,
                size: size.into(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        });
        if let Some(open) = self.arrow {
            // A small solid triangle under the title.
            let a = metrics::FLOAT_SIDE_ARROW;
            let y = top + length + metrics::FLOAT_SIDE_ARROW_GAP;
            let (tip, back) = if open {
                (center_x - a / 3.0, center_x + a / 3.0)
            } else {
                (center_x + a / 3.0, center_x - a / 3.0)
            };
            let triangle = Path::new(|path| {
                path.move_to(Point::new(back, y));
                path.line_to(Point::new(tip, y + a / 2.0));
                path.line_to(Point::new(back, y + a));
                path.close();
            });
            frame.fill(&triangle, color);
        }
        vec![frame.into_geometry()]
    }
}

/// One floating panel on the layer: its top-left corner, the panel, and
/// the tab that stands above its top-right corner (empty while hidden).
pub(super) struct Floater<'a> {
    pub at: Point,
    pub panel: Element<'a, Message>,
    pub tab: Element<'a, Message>,
}

/// The floating panels, each at its top-left corner. `order` lists them
/// back to front; the widget tree keeps them in `panels`' order, each as
/// its panel and its tab.
pub(super) struct FloatLayer<'a> {
    panels: Vec<Floater<'a>>,
    order: Vec<usize>,
    /// The panels' keys, in `panels`' order.
    keys: Vec<String>,
    /// The panel whose extra tools are open and not pinned: a press off it
    /// closes them.
    open: Option<usize>,
}

impl<'a> FloatLayer<'a> {
    /// The front-most panel whose panel or tab is under the cursor.
    fn hit(&self, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
        let at = cursor.position()?;
        let bounds: Vec<Rectangle> = layout.children().map(|child| child.bounds()).collect();
        self.order.iter().rev().copied().find(|&i| {
            [2 * i, 2 * i + 1]
                .iter()
                .any(|&j| bounds.get(j).is_some_and(|b| b.contains(at)))
        })
    }

    fn parts_mut(&mut self) -> Vec<&mut Element<'a, Message>> {
        self.panels
            .iter_mut()
            .flat_map(|floater| [&mut floater.panel, &mut floater.tab])
            .collect()
    }

    fn part(&self, j: usize) -> &Element<'a, Message> {
        let floater = &self.panels[j / 2];
        if j.is_multiple_of(2) {
            &floater.panel
        } else {
            &floater.tab
        }
    }
}

/// The layer's tree state: the panels its children were built for.
#[derive(Default)]
struct LayerState {
    keys: Vec<String>,
}

impl<'a> Widget<Message, Theme, Renderer> for FloatLayer<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<LayerState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(LayerState {
            keys: self.keys.clone(),
        })
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        // The children follow the panels by position: when a panel comes or
        // goes, the ones after it would take over its hover and press
        // state, so the children start afresh.
        let state = tree.state.downcast_mut::<LayerState>();
        if state.keys != self.keys {
            state.keys.clone_from(&self.keys);
            tree.children.clear();
        }
        let mut parts = self.parts_mut();
        tree.diff_children_custom(
            &mut parts,
            |tree, part| tree.diff(part.as_widget_mut()),
            |part| widget::Tree::new(part.as_widget()),
        );
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let natural = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY));
        let mut nodes = Vec::with_capacity(2 * self.panels.len());
        for (floater, trees) in self.panels.iter_mut().zip(tree.children.chunks_mut(2)) {
            let panel = floater
                .panel
                .as_widget_mut()
                .layout(&mut trees[0], renderer, &natural)
                .move_to(floater.at);
            let tab = floater
                .tab
                .as_widget_mut()
                .layout(&mut trees[1], renderer, &natural);
            // The tab stands on the panel's top edge at its right end.
            let tab_at = Point::new(
                floater.at.x + panel.size().width - tab.size().width,
                floater.at.y - tab.size().height,
            );
            nodes.push(panel);
            nodes.push(tab.move_to(tab_at));
        }
        layout::Node::with_children(limits.max(), nodes)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let hit = self.hit(layout, cursor);
        // A release over a panel saves a raise its press made.
        if hit.is_some() && matches!(event, Event::Mouse(mouse::Event::ButtonReleased(_))) {
            shell.publish(Message::RibbonFloat(FloatMsg::Settle));
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            // Any press on a panel brings it to the front (the ribbon knows
            // whether it already is).
            if let Some(i) = hit {
                shell.publish(Message::RibbonFloat(FloatMsg::Raise(self.keys[i].clone())));
            }
            // A press anywhere else closes unpinned extra tools.
            if self.open.is_some() && hit != self.open {
                shell.publish(Message::RibbonFloat(FloatMsg::Outside));
            }
        }
        let layouts: Vec<Layout<'_>> = layout.children().collect();
        // Front to back; a panel under another sees no pointer there.
        for slot in (0..self.order.len()).rev() {
            let i = self.order[slot];
            let panel_cursor = match hit {
                Some(top) if top != i => mouse::Cursor::Unavailable,
                _ => cursor,
            };
            // The tab first: it lies over the panel under it.
            for j in [2 * i + 1, 2 * i] {
                let floater = &mut self.panels[i];
                let part = if j.is_multiple_of(2) {
                    &mut floater.panel
                } else {
                    &mut floater.tab
                };
                part.as_widget_mut().update(
                    &mut tree.children[j],
                    event,
                    layouts[j],
                    panel_cursor,
                    renderer,
                    shell,
                    viewport,
                );
                if shell.is_event_captured() {
                    return;
                }
            }
        }
        // A panel is opaque: what is under it gets no presses or wheel. A
        // release still goes through, so a drag that began elsewhere (a
        // ribbon panel, a pan of the drawing) ends where it is let go.
        if hit.is_some()
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
            )
        {
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(i) = self.hit(layout, cursor) else {
            // Off the panels the drawing under the layer has the pointer.
            return mouse::Interaction::None;
        };
        let layouts: Vec<Layout<'_>> = layout.children().collect();
        let interaction = [2 * i + 1, 2 * i]
            .into_iter()
            .map(|j| {
                self.part(j).as_widget().mouse_interaction(
                    &tree.children[j],
                    layouts[j],
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .find(|interaction| *interaction != mouse::Interaction::None);
        interaction.unwrap_or(mouse::Interaction::Idle)
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        for ((part, tree), layout) in self
            .parts_mut()
            .into_iter()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            part.as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let hit = self.hit(layout, cursor);
        let layouts: Vec<Layout<'_>> = layout.children().collect();
        for &i in &self.order {
            let panel_cursor = match hit {
                Some(top) if top != i => mouse::Cursor::Unavailable,
                _ => cursor,
            };
            // Each panel on its own layer, so a panel on top covers the
            // text of the ones under it.
            let bounds = layouts[2 * i].bounds().union(&layouts[2 * i + 1].bounds());
            renderer.with_layer(bounds, |renderer| {
                for j in [2 * i, 2 * i + 1] {
                    self.part(j).as_widget().draw(
                        &tree.children[j],
                        renderer,
                        theme,
                        style,
                        layouts[j],
                        panel_cursor,
                        viewport,
                    );
                }
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // The panels' tooltips.
        let overlays: Vec<_> = self
            .panels
            .iter_mut()
            .flat_map(|floater| [&mut floater.panel, &mut floater.tab])
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((part, tree), layout)| {
                part.as_widget_mut()
                    .overlay(tree, layout, renderer, viewport, translation)
            })
            .collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
}

impl<'a> From<FloatLayer<'a>> for Element<'a, Message> {
    fn from(layer: FloatLayer<'a>) -> Self {
        Element::new(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::super::layout::{FloatingPanel, RibbonLayout};
    use super::Ribbon;

    fn floating(key: &str, z: u32) -> FloatingPanel {
        let (module_id, _) = key.split_once('/').unwrap();
        FloatingPanel {
            key: key.into(),
            module_id: module_id.into(),
            z,
            ..FloatingPanel::default()
        }
    }

    #[test]
    fn the_layer_shows_the_floating_panels_that_still_exist() {
        let mut ribbon = Ribbon::default();
        assert!(ribbon.floating_layer(false).is_none());
        let layout = RibbonLayout {
            floating: vec![
                floating("draw/Modify", 2),
                floating("draw/Gone", 5),
                floating("draw/Draw", 1),
            ],
            ..RibbonLayout::default()
        };
        ribbon.set_layout(layout);
        let found: Vec<_> = ribbon
            .floating_groups()
            .iter()
            .map(|(_, group, panel)| (group.title, panel.z))
            .collect();
        assert_eq!(found, [("Modify", 2), ("Draw", 1)]);
        assert!(ribbon.floating_layer(false).is_some());
    }

    impl Ribbon {
        fn on_float_now(&mut self, msg: super::FloatMsg) -> bool {
            self.on_float(msg, iced::time::Instant::now())
        }
    }

    #[test]
    fn a_press_raises_a_panel_and_its_strip_moves_it() {
        use super::FloatMsg;
        use iced::Point;
        let mut ribbon = Ribbon::default();
        let mut draw = floating("draw/Draw", 1);
        draw.x = 100.0;
        draw.y = 200.0;
        ribbon.set_layout(RibbonLayout {
            floating: vec![draw, floating("draw/Modify", 2)],
            ..RibbonLayout::default()
        });
        // Raising the one on top changes nothing; the other goes above it,
        // saved when the press lets go rather than at once.
        ribbon.on_float_now(FloatMsg::Raise("draw/Modify".into()));
        assert!(!ribbon.float_ui.unsaved);
        assert!(!ribbon.on_float_now(FloatMsg::Raise("draw/Draw".into())));
        assert_eq!(ribbon.layout.floating[0].z, 3);
        assert!(ribbon.on_float_now(FloatMsg::Release { moved: false }));
        assert!(!ribbon.float_ui.unsaved);

        // The press point keeps its place on the panel while it moves.
        ribbon.on_float_now(FloatMsg::Press {
            key: "draw/Draw".into(),
            at: Point::new(150.0, 290.0),
        });
        assert!(!ribbon.on_float_now(FloatMsg::Move {
            at: Point::new(250.0, 390.0),
            free: true,
        }));
        let panel = &ribbon.layout.floating[0];
        assert_eq!((panel.x, panel.y), (200.0, 300.0));
        // Saved on release once it moved; a click is not a move.
        assert!(ribbon.on_float_now(FloatMsg::Release { moved: true }));
        ribbon.on_float_now(FloatMsg::Press {
            key: "draw/Draw".into(),
            at: Point::new(250.0, 390.0),
        });
        assert!(!ribbon.on_float_now(FloatMsg::Release { moved: false }));
        assert!(ribbon.float_ui.drag.is_none());
    }

    #[test]
    fn extra_tools_open_close_after_the_pointer_leaves_and_stay_when_pinned() {
        use super::super::draw_panel::SlideLook;
        use super::FloatMsg;
        use std::time::Duration;
        let mut ribbon = Ribbon::default();
        ribbon.set_layout(RibbonLayout {
            floating: vec![floating("draw/Draw", 1), floating("draw/Groups", 2)],
            ..RibbonLayout::default()
        });
        let t0 = iced::time::Instant::now();
        // A panel without extra tools has none to open.
        ribbon.on_float(FloatMsg::ToggleExtras("draw/Groups".into()), t0);
        assert_eq!(ribbon.float_look("draw/Groups"), SlideLook::Closed);

        ribbon.on_float(FloatMsg::ToggleExtras("draw/Draw".into()), t0);
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Open);
        // Leaving closes them 450 ms later, not sooner.
        ribbon.on_float(
            FloatMsg::Hover {
                key: "draw/Draw".into(),
                over: false,
            },
            t0,
        );
        assert!(ribbon.wants_tick());
        ribbon.tick(t0 + Duration::from_millis(300));
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Open);
        ribbon.tick(t0 + Duration::from_millis(460));
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Closed);

        // A press elsewhere closes them too.
        ribbon.on_float(FloatMsg::ToggleExtras("draw/Draw".into()), t0);
        ribbon.on_float(FloatMsg::Outside, t0);
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Closed);

        // Pinned they stay, and the pin is saved; the footer title unpins.
        ribbon.on_float(FloatMsg::ToggleExtras("draw/Draw".into()), t0);
        assert!(ribbon.on_float(FloatMsg::TogglePin("draw/Draw".into()), t0));
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Pinned);
        ribbon.on_float(FloatMsg::Outside, t0);
        ribbon.tick(t0 + Duration::from_secs(5));
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Pinned);
        assert!(ribbon.on_float(FloatMsg::Collapse("draw/Draw".into()), t0));
        assert_eq!(ribbon.float_look("draw/Draw"), SlideLook::Closed);
        assert!(!ribbon.layout.floating[0].pinned);
    }

    #[test]
    fn a_side_title_is_cut_off_to_fit_and_the_orientation_toggles() {
        use super::{FloatMsg, VerticalTitle};
        assert_eq!(VerticalTitle::fitted("Draw", 60.0), "Draw");
        let cut = VerticalTitle::fitted("Block Definition", 50.0);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(
            crate::ui::style::text_metrics::width(
                &cut,
                crate::ui::style::ribbon_metrics::FLOAT_TITLE_SIZE,
                false
            ) <= 50.0
        );
        assert_eq!(VerticalTitle::fitted("Block Definition", 1.0), "…");

        let mut ribbon = Ribbon::default();
        ribbon.set_layout(RibbonLayout {
            floating: vec![floating("draw/Draw", 1)],
            ..RibbonLayout::default()
        });
        let now = iced::time::Instant::now();
        assert!(ribbon.on_float(FloatMsg::ToggleOrientation("draw/Draw".into()), now));
        assert!(ribbon.layout.floating[0].extras_right);
        // It builds in both orientations, its extra tools open or not.
        for _ in 0..2 {
            assert!(ribbon.floating_layer(false).is_some());
            ribbon.on_float(FloatMsg::ToggleExtras("draw/Draw".into()), now);
            assert!(ribbon.floating_layer(false).is_some());
            ribbon.on_float(FloatMsg::ToggleOrientation("draw/Draw".into()), now);
        }
    }

    #[test]
    fn the_hover_buttons_follow_the_strip_and_the_tab() {
        use super::{hover_flag, title_shift, FloatMsg};
        let mut flag = None;
        hover_flag(&mut flag, "a".into(), true);
        // Leaving another panel keeps this one's.
        hover_flag(&mut flag, "b".into(), false);
        assert_eq!(flag.as_deref(), Some("a"));
        hover_flag(&mut flag, "a".into(), false);
        assert!(flag.is_none());

        // A narrow panel's title moves clear of the buttons; a wide one's
        // stays centred, and nothing moves off hover.
        assert!(title_shift(true, "Modify", true, Some(110.0)).is_some());
        assert!(title_shift(true, "Draw", true, Some(400.0)).is_none());
        assert!(title_shift(false, "Modify", true, Some(110.0)).is_none());

        let mut ribbon = Ribbon::default();
        let mut side = floating("draw/Draw", 1);
        side.extras_right = true;
        ribbon.set_layout(RibbonLayout {
            floating: vec![side, floating("draw/Modify", 2)],
            ..RibbonLayout::default()
        });
        let now = iced::time::Instant::now();
        ribbon.on_float(
            FloatMsg::StripHover {
                key: "draw/Draw".into(),
                over: true,
            },
            now,
        );
        assert!(ribbon.floating_layer(false).is_some());
        // Return to ribbon puts the panel back and forgets its hover.
        assert!(ribbon.on_float(FloatMsg::Return("draw/Draw".into()), now));
        assert!(ribbon.float_ui.strip_hover.is_none());
        assert_eq!(ribbon.layout.floating.len(), 1);
        let draw = ribbon
            .modules
            .iter()
            .position(|module| module.id() == "draw")
            .unwrap();
        assert_eq!(
            ribbon.groups_for(draw).next().map(|g| g.title),
            Some("Draw")
        );
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> iced::Rectangle {
        iced::Rectangle::new(iced::Point::new(x, y), iced::Size::new(w, h))
    }

    #[test]
    fn two_panels_snap_side_by_side_and_level() {
        use super::snap;
        let window = rect(0.0, 0.0, 1400.0, 900.0);
        let modify = rect(500.0, 300.0, 120.0, 100.0);
        // 5 px left of Modify and 3 px lower: it closes the gap, borders
        // overlapping, and lines up with Modify's top.
        let draw = rect(500.0 - 200.0 - 5.0, 303.0, 200.0, 100.0);
        let at = snap(draw, &[modify], window, None, 8.0);
        assert_eq!((at.x, at.y), (500.0 - 200.0 + 1.0, 300.0));
        // Under Modify, left edges level.
        let below = rect(506.0, 405.0, 200.0, 100.0);
        let at = snap(below, &[modify], window, None, 8.0);
        assert_eq!((at.x, at.y), (500.0, 399.0));
        // Too far: no snap.
        let far = rect(200.0, 600.0, 200.0, 100.0);
        let at = snap(far, &[modify], window, None, 8.0);
        assert_eq!((at.x, at.y), (200.0, 600.0));
    }

    #[test]
    fn a_panel_snaps_to_another_with_its_pinned_extra_tools() {
        use super::snap;
        let window = rect(0.0, 0.0, 1400.0, 900.0);
        // Modify with its pinned tools below is 180 high, not 100: Draw
        // dropped beside its tools' lower part snaps to their bottom.
        let modify_with_tools = rect(500.0, 300.0, 120.0, 180.0);
        let draw = rect(295.0, 385.0, 200.0, 100.0);
        let at = snap(draw, &[modify_with_tools], window, None, 8.0);
        assert_eq!((at.x, at.y), (500.0 - 200.0 + 1.0, 380.0));
        // And a panel under it snaps under the tools, not under the panel.
        let under = rect(503.0, 486.0, 150.0, 80.0);
        let at = snap(under, &[modify_with_tools], window, None, 8.0);
        assert_eq!((at.x, at.y), (500.0, 479.0));
    }

    #[test]
    fn a_panel_snaps_to_the_window_and_the_ribbon() {
        use super::snap;
        let window = rect(0.0, 0.0, 1400.0, 900.0);
        let at = snap(
            rect(6.0, 893.0 - 100.0, 200.0, 100.0),
            &[],
            window,
            None,
            8.0,
        );
        assert_eq!((at.x, at.y), (0.0, 800.0));
        let at = snap(rect(1195.0, 400.0, 200.0, 100.0), &[], window, None, 8.0);
        assert_eq!(at.x, 1200.0);
        // Under the ribbon, whose bottom is at 154.
        let at = snap(
            rect(300.0, 160.0, 200.0, 100.0),
            &[],
            window,
            Some(154.0),
            8.0,
        );
        assert_eq!(at.y, 154.0);
    }

    #[test]
    fn a_drop_on_the_tool_area_docks_on_its_own_tab_or_goes_back_to_its_slot() {
        use super::FloatMsg;
        use iced::Point;
        let mut ribbon = Ribbon::default();
        let index = |ribbon: &Ribbon, id: &str| {
            ribbon
                .modules
                .iter()
                .position(|module| module.id() == id)
                .unwrap()
        };
        let draw = index(&ribbon, "draw");
        let insert = index(&ribbon, "insert");
        ribbon.select(draw);
        assert!(ribbon.float_panel("draw", "Modify", 400.0, 300.0));
        assert!(ribbon.float_panel("insert", "Block Definition", 600.0, 300.0));

        // On its own tab, dropped left of every panel: it goes first.
        let drop = |ribbon: &mut Ribbon, key: &str| {
            ribbon.float_ui.drag = Some(super::FloatDrag {
                key: key.into(),
                grab: iced::Vector::ZERO,
                over_ribbon: true,
                at: Point::new(-100.0, 50.0),
            });
            ribbon.on_float_now(FloatMsg::Release { moved: true })
        };
        assert!(drop(&mut ribbon, "draw/Modify"));
        assert_eq!(
            ribbon.groups_for(draw).next().map(|g| g.title),
            Some("Modify")
        );

        // Another tab's panel goes back to its old place there.
        let before: Vec<_> = ribbon
            .ordered_groups(insert)
            .iter()
            .map(|g| g.title)
            .collect();
        assert!(drop(&mut ribbon, "insert/Block Definition"));
        assert!(ribbon.layout.floating.is_empty());
        let after: Vec<_> = ribbon.groups_for(insert).map(|g| g.title).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn a_raise_is_saved_when_the_press_lets_go_even_off_the_title_strip() {
        use super::FloatMsg;
        let mut ribbon = Ribbon::default();
        // Two at the same order, as a hand-edited layout may have them.
        ribbon.set_layout(RibbonLayout {
            floating: vec![floating("draw/Draw", 2), floating("draw/Modify", 2)],
            ..RibbonLayout::default()
        });
        assert!(!ribbon.on_float_now(FloatMsg::Raise("draw/Draw".into())));
        assert_eq!(ribbon.layout.floating[0].z, 3);
        // A click on a button sends no grip release; the layer settles it.
        assert!(ribbon.on_float_now(FloatMsg::Settle));
        assert!(!ribbon.on_float_now(FloatMsg::Settle));
        // On top alone, a press changes nothing.
        ribbon.on_float_now(FloatMsg::Raise("draw/Draw".into()));
        assert!(!ribbon.on_float_now(FloatMsg::Settle));
    }

    #[test]
    fn a_floating_panels_dropdown_opens_from_any_tab() {
        let mut ribbon = Ribbon::default();
        ribbon.set_layout(RibbonLayout {
            floating: vec![floating("draw/Draw", 1)],
            ..RibbonLayout::default()
        });
        let insert = ribbon
            .modules
            .iter()
            .position(|module| module.id() == "insert")
            .unwrap();
        ribbon.select(insert);
        ribbon.toggle_dropdown("CIRCLE");
        let list = ribbon.dropdown_overlay(&[], &[], (1400.0, 900.0), false, &[], &[], &[]);
        assert!(list.is_some());
    }
}
