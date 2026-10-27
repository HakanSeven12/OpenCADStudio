//! The slide-outs under panel titles and the panels' dialog launchers, drawn
//! from the registry in `extras`.

use std::time::Duration;

use iced::time::Instant;
use iced::widget::{
    button, column, container, mouse_area, opaque, row, stack, text, tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Length, Padding, Theme};

use super::extras::{self, Layout, MoreGroup, MoreTool, PanelExtras};
use super::widgets::{
    make_icon, make_tip, popup_panel_style, popup_row_style, tip_style, tool_btn_style,
    PANEL_TITLE_H, PANEL_TITLE_SIZE,
};
use super::{dropdown_backdrop, keytips, position_ribbon_dropdown, Ribbon, RibbonMinimize};
use crate::app::Message;
use crate::modules::IconKind;
use crate::t;
use crate::ui::icons;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::{dropdown_bounds, recent_bounds, PosReport};

const GROUP_TITLE_ARROW_SIZE: f32 = 8.0;
/// The title strip at the bottom of a panel and the line over it, which an
/// open slide-out covers.
const TITLE_STRIP: f32 = metrics::SLIDE_OUT_TOP;
/// A panel's dialog launcher, at the right end of its title strip.
const LAUNCHER_W: f32 = metrics::LAUNCHER_W;
const LAUNCHER_H: f32 = metrics::LAUNCHER_H;
/// Height of a slide-out's footer.
const FOOTER_H: f32 = 19.0;
/// A slide-out tool button, and the arrow of one with a submenu.
const CELL: f32 = 26.0;
const CELL_ICON: f32 = 16.0;
const SPLIT_W: f32 = 11.0;
const GAP: f32 = 2.0;
const PAD: f32 = 4.0;
const OPTION_HEIGHT: f32 = 22.0;
const OPTIONS_W: f32 = 190.0;
/// How long an unpinned slide-out stays after the pointer leaves it and its
/// panel, so the pointer can cross the gap between them.
const CLOSE_DELAY: Duration = Duration::from_millis(300);

/// Command, label and icon of every slide-out tool.
pub(crate) fn tools() -> impl Iterator<Item = (&'static str, &'static str, &'static [u8])> {
    extras::all()
        .flat_map(PanelExtras::tools)
        .filter_map(|tool| match tool.tool.icon {
            IconKind::Svg(icon) => Some((tool.command(), tool.tool.label, icon)),
            IconKind::Glyph(_) => None,
        })
}

pub(super) fn panel_for_dropdown(id: &str) -> Option<&'static PanelExtras> {
    extras::all().find(|panel| {
        panel.id == id
            || panel
                .tools()
                .any(|tool| tool.command() == id && !tool.options.is_empty())
    })
}

pub(super) fn owns_dropdown(id: &str) -> bool {
    panel_for_dropdown(id).is_some()
}

pub(super) fn parent_panel(id: &str) -> Option<&'static str> {
    panel_for_dropdown(id).map(|panel| panel.id)
}

/// The slide-out under the title of the panel `key`.
pub(super) fn panel_for_group(key: &str) -> Option<&'static PanelExtras> {
    extras::for_panel(key).filter(|panel| panel.tools().next().is_some())
}

/// The slide-out under the panel `key`, if it has one.
pub(super) fn slide_out_id(key: &str) -> Option<&'static str> {
    panel_for_group(key).map(|panel| panel.id)
}

/// The message the dialog launcher of a panel sends: its command, run like a
/// ribbon button's.
pub(super) fn launcher_message(command: &'static str) -> Message {
    Message::RibbonToolClick {
        tool_id: command.to_string(),
        event: crate::modules::ModuleEvent::Command(command.to_string()),
    }
}

/// What the open slide-out or tool submenu `id` offers, as (dropdown id,
/// command, opens a submenu) in display order.
pub(super) fn keytip_items(id: &str) -> Vec<(&'static str, &'static str, bool)> {
    let Some(panel) = panel_for_dropdown(id) else {
        return Vec::new();
    };
    if panel.id == id {
        return panel
            .tools()
            .map(|tool| (panel.id, tool.command(), !tool.options.is_empty()))
            .collect();
    }
    panel
        .tools()
        .filter(|tool| tool.command() == id)
        .flat_map(|tool| {
            tool.options
                .iter()
                .map(|(cmd, _)| (tool.command(), *cmd, false))
        })
        .collect()
}

/// Every command the slide-out of the panel `key` offers: each tool and each
/// of its submenu options.
pub(super) fn slide_out_commands(key: &str) -> impl Iterator<Item = &'static str> {
    panel_for_group(key)
        .into_iter()
        .flat_map(PanelExtras::tools)
        .flat_map(|tool| {
            std::iter::once(tool.command()).chain(tool.options.iter().map(|(cmd, _)| *cmd))
        })
}

/// A panel's slide-out: closed, open, or pinned open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SlideLook {
    Closed,
    Open,
    Pinned,
}

/// Anchor of the body of panel `key`, which its slide-out opens under.
pub(super) fn panel_box_id(key: &str) -> String {
    format!("panelbox:{key}")
}

#[derive(Clone, Debug)]
pub enum SlideOutMsg {
    /// The pointer entered or left the panel with this key.
    EnterPanel(&'static str),
    LeavePanel(&'static str),
    /// The pointer entered or left the open slide-out.
    OverSlideOut(bool),
    /// The pointer entered or left the submenu of a slide-out tool.
    OverSubmenu(bool),
    /// The pointer is over the footer of this panel's slide-out.
    OverFooter(Option<&'static str>),
    /// Pin or unpin the slide-out of this panel.
    TogglePin(&'static str),
    /// Close the slide-out of this panel, unpinning it.
    Collapse(&'static str),
}

/// Where the pointer is, for closing an unpinned slide-out.
#[derive(Default)]
pub(super) struct SlideState {
    /// The panel with a slide-out the pointer is over.
    over_panel: Option<&'static str>,
    over_slide: bool,
    over_submenu: bool,
    /// The slide-out whose footer the pointer is over: its pin shows.
    footer: Option<&'static str>,
    /// When the open slide-out closes.
    pub(super) closing: Option<Instant>,
}

impl SlideState {
    /// Forget the pointer over a slide-out or submenu that closed: a widget
    /// that goes sends no exit.
    pub(super) fn closed(&mut self, slide_out: bool) {
        self.over_submenu = false;
        if slide_out {
            self.over_slide = false;
            self.closing = None;
        }
    }
}

impl Ribbon {
    /// How the slide-out of panel `key` shows.
    pub(super) fn slide_look(&self, key: &str) -> SlideLook {
        let Some(panel) = panel_for_group(key) else {
            return SlideLook::Closed;
        };
        if self.layout.pinned_slideouts.contains(key) {
            SlideLook::Pinned
        } else if self.open_dropdown.as_deref().and_then(parent_panel) == Some(panel.id) {
            SlideLook::Open
        } else {
            SlideLook::Closed
        }
    }

    /// The open slide-out that is not pinned, if any.
    fn open_slide_out(&self) -> Option<&'static PanelExtras> {
        let panel = self.open_dropdown.as_deref().and_then(panel_for_dropdown)?;
        (!self.layout.pinned_slideouts.contains(panel.key)).then_some(panel)
    }

    /// Apply a slide-out message. Returns whether the pins changed and
    /// should be saved.
    pub fn on_slide_out(&mut self, msg: SlideOutMsg, now: Instant) -> bool {
        let mut changed = false;
        match msg {
            SlideOutMsg::EnterPanel(key) => self.slide.over_panel = Some(key),
            SlideOutMsg::LeavePanel(key) => {
                if self.slide.over_panel == Some(key) {
                    self.slide.over_panel = None;
                }
            }
            SlideOutMsg::OverSlideOut(over) => self.slide.over_slide = over,
            SlideOutMsg::OverSubmenu(over) => self.slide.over_submenu = over,
            SlideOutMsg::OverFooter(key) => self.slide.footer = key,
            SlideOutMsg::TogglePin(key) => {
                if !self.layout.pinned_slideouts.remove(key) {
                    // A pinned slide-out shows on its own, not as a dropdown.
                    self.layout.pinned_slideouts.insert(key.to_string());
                    if self.open_slide_out().is_none() {
                        self.close_dropdown();
                    }
                }
                self.slide.over_slide = false;
                changed = true;
            }
            SlideOutMsg::Collapse(key) => {
                changed = self.layout.pinned_slideouts.remove(key);
                if self
                    .open_dropdown
                    .as_deref()
                    .and_then(panel_for_dropdown)
                    .is_some_and(|panel| panel.key == key)
                {
                    self.close_dropdown();
                }
                self.slide.over_slide = false;
                self.slide.footer = None;
            }
        }
        // An unpinned slide-out closes once the pointer is off it and its
        // panel.
        self.slide.closing = match self.open_slide_out() {
            Some(panel)
                if self.slide.over_panel != Some(panel.key)
                    && !self.slide.over_slide
                    && !self.slide.over_submenu =>
            {
                Some(now + CLOSE_DELAY)
            }
            _ => None,
        };
        changed
    }

    pub(super) fn slide_out_tick(&mut self, now: Instant) {
        if self.slide.closing.is_some_and(|at| now >= at) {
            self.slide.closing = None;
            if self.open_slide_out().is_some() {
                self.close_dropdown();
            }
        }
    }

    /// The pinned slide-outs of the tab on show, under their panels.
    pub fn pinned_slide_outs(
        &self,
        is_start: bool,
        win: (f32, f32),
    ) -> Option<Element<'_, Message>> {
        if is_start
            || self.layout.minimize != RibbonMinimize::Off
            || self.layout.pinned_slideouts.is_empty()
        {
            return None;
        }
        let module = self.modules.get(self.active)?;
        let layers: Vec<Element<'_, Message>> = self
            .groups_for(self.active)
            .filter_map(|group| {
                let key = extras::panel_key(module.id(), group.title);
                if !self.layout.pinned_slideouts.contains(&key) {
                    return None;
                }
                let panel = panel_for_group(&key)?;
                // Only under a panel the row shows at full or compact size.
                recent_bounds(&panel_box_id(&key))?;
                let (left, top, width) = slide_out_place(self, panel)?;
                Some(placed(
                    slide_out(self, panel, width, true),
                    left,
                    top,
                    width,
                    win,
                ))
            })
            .collect();
        (!layers.is_empty()).then(|| stack(layers).width(Fill).height(Fill).into())
    }
}

/// The title row of the panel `key`: its title, or for a panel with a
/// slide-out a button that opens it; and the panel's dialog launcher, which
/// the panel frame puts at the right end of the strip.
pub(super) fn group_title<'a>(
    title: &'static str,
    key: &str,
    look: SlideLook,
    active_tool: Option<&str>,
) -> (Element<'a, Message>, Option<Element<'a, Message>>) {
    let panel = panel_for_group(key);
    let launcher = panel.and_then(launcher);
    // Room for the launcher on both sides keeps the title centred.
    let side = if launcher.is_some() {
        LAUNCHER_W + 2.0
    } else {
        4.0
    };
    let title_text = |title: &'static str| {
        text(t!(title))
            .size(PANEL_TITLE_SIZE)
            .wrapping(iced::widget::text::Wrapping::None)
            .style(|theme: &Theme| text::Style {
                color: Some(chrome::title_text(theme)),
            })
    };
    let Some(panel) = panel else {
        let label = keytips::label_anchor(|| format!("title:{title}"), title_text(title));
        let row = container(label)
            .height(PANEL_TITLE_H)
            .center_y(PANEL_TITLE_H)
            .padding([0.0, side]);
        return (row.into(), launcher);
    };
    let expanded = look != SlideLook::Closed;
    // A command running from the closed slide-out lights its title, the way a
    // dropdown lights while one of its items runs.
    let lit = expanded || panel.tools().any(|tool| tool.is_active(active_tool));
    let arrow = if expanded {
        icons::themed_arrow_up(GROUP_TITLE_ARROW_SIZE)
    } else {
        icons::themed_arrow_down(GROUP_TITLE_ARROW_SIZE)
    };
    let title_button: Element<'a, Message> = PosReport::new(
        panel.id,
        button(
            row![
                keytips::label_anchor(|| keytips::label_id(panel.id), title_text(title)),
                arrow
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(match look {
            // A pinned slide-out folds away and unpins.
            SlideLook::Pinned => Message::RibbonSlideOut(SlideOutMsg::Collapse(panel.key)),
            _ => Message::ToggleRibbonDropdown(panel.id.to_string()),
        })
        .style(move |theme: &Theme, status| tool_btn_style(theme, lit, status))
        .height(PANEL_TITLE_H - 3.0)
        .padding([0, 4]),
    )
    .into();
    let row = container(title_button)
        .height(PANEL_TITLE_H)
        .center_y(PANEL_TITLE_H)
        .padding([0.0, side]);
    (row.into(), launcher)
}

/// The corner arrow that runs the dialog command of `panel`, if it has one.
pub(super) fn launcher<'a>(panel: &PanelExtras) -> Option<Element<'a, Message>> {
    let dialog = panel.dialog?;
    // The corner arrow runs the panel's dialog command.
    let launcher = tooltip(
        // A right arrow turned to point into the corner.
        button(
            container(
                iced::widget::svg(icons::themed_handle(icons::ARROW_LONG_RIGHT))
                    .width(9)
                    .height(9)
                    .rotation(iced::Radians(std::f32::consts::FRAC_PI_4))
                    .style(|theme: &Theme, _| iced::widget::svg::Style {
                        color: Some(chrome::title_text(theme)),
                    }),
            )
            .center_x(Fill)
            .center_y(Fill),
        )
        .on_press(launcher_message(dialog))
        .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
        .width(LAUNCHER_W)
        .height(LAUNCHER_H)
        .padding(0),
        make_tip(format!(
            "{}\n{} {dialog}",
            t!(panel.dialog_label),
            t!("Command:")
        )),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(400))
    .style(tip_style);
    Some(keytips::anchor(keytips::XREF_LAUNCHER, launcher))
}

/// Where the slide-out of `panel` shows, as (left, top, width): under the
/// panel's body, over its title strip, as wide as the panel. Under the
/// minimized ribbon's panel popup it hangs below the "More tools" footer.
fn slide_out_place(ribbon: &Ribbon, panel: &PanelExtras) -> Option<(f32, f32, f32)> {
    let id = panel_box_id(panel.key);
    let body = recent_bounds(&id).or_else(|| dropdown_bounds(&id))?;
    let top = if ribbon.layout.minimize == RibbonMinimize::Off && ribbon.layout.show_panel_titles {
        body.y + body.height - TITLE_STRIP
    } else {
        dropdown_bounds(panel.id).map_or(body.y + body.height, |b| b.y + b.height)
    };
    Some((body.x, top, body.width))
}

/// `content` (`width` wide) at (`left`, `top`), kept inside the window.
fn placed<'a>(
    content: Element<'a, Message>,
    left: f32,
    top: f32,
    width: f32,
    win: (f32, f32),
) -> Element<'a, Message> {
    container(content)
        .padding(Padding {
            left: left.min(win.0 - width).max(0.0),
            top: top.max(0.0),
            ..Padding::ZERO
        })
        .width(Fill)
        .height(Fill)
        .into()
}

/// A slide-out: its tool groups, the xref fading row on Reference, and the
/// footer. Unpinned it is lighter than the panels; pinned it matches them.
fn slide_out<'a>(
    ribbon: &Ribbon,
    panel: &'static PanelExtras,
    width: f32,
    pinned: bool,
) -> Element<'a, Message> {
    let active = ribbon.lit_tool().as_deref();
    let inner = (width - 2.0 * PAD).max(CELL + SPLIT_W);
    let labelled = panel.more.len() > 1;
    let mut groups = column![].spacing(4);
    for group in &panel.more {
        if labelled {
            groups = groups.push(group_label(group));
        }
        groups = groups.push(tool_rows(&group.tools, panel.id, active, inner));
    }
    if panel.layout == Layout::Reference {
        groups = groups.push(fade_row(ribbon.xref_fade, panel.id));
    }
    let body = column![
        container(groups).padding(PAD).width(Fill),
        footer(ribbon.slide.footer == Some(panel.key), panel, pinned),
    ];
    let body = container(body)
        .width(Length::Fixed(width))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(if pinned {
                chrome::panel(theme)
            } else {
                chrome::open_panel(theme)
            })),
            border: Border {
                color: chrome::border(theme),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });
    // Opaque: the drawing's controls under it show no hover or tooltips.
    opaque(
        mouse_area(body)
            .on_enter(Message::RibbonSlideOut(SlideOutMsg::OverSlideOut(true)))
            .on_exit(Message::RibbonSlideOut(SlideOutMsg::OverSlideOut(false))),
    )
}

/// The name of a tool group, upper case, with a hairline after it.
fn group_label<'a>(group: &MoreGroup) -> Element<'a, Message> {
    let line =
        container(Space::new().width(Fill).height(1)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::border(theme))),
            ..Default::default()
        });
    row![
        text(t!(group.title).to_uppercase())
            .size(9.5)
            .style(|theme: &Theme| text::Style {
                color: Some(chrome::muted(theme)),
            }),
        line,
    ]
    .spacing(4)
    .align_y(iced::Center)
    .into()
}

/// `tools` as icon buttons in rows that wrap at `inner` px.
fn tool_rows<'a>(
    tools: &[MoreTool],
    panel_id: &'static str,
    active: Option<&str>,
    inner: f32,
) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut current: Vec<Element<'a, Message>> = Vec::new();
    let mut used = 0.0;
    for tool in tools {
        let width = if tool.options.is_empty() {
            CELL
        } else {
            CELL + SPLIT_W
        };
        let mut needed = if current.is_empty() {
            width
        } else {
            used + GAP + width
        };
        if needed > inner && !current.is_empty() {
            rows.push(row(std::mem::take(&mut current)).spacing(GAP).into());
            needed = width;
        }
        current.push(tool_button(tool, tool.is_active(active), panel_id));
        used = needed;
    }
    if !current.is_empty() {
        rows.push(row(current).spacing(GAP).into());
    }
    column(rows).spacing(GAP).into()
}

fn tool_button(tool: &MoreTool, active: bool, panel_id: &'static str) -> Element<'static, Message> {
    let command = tool.command();
    let face = button(make_icon(tool.tool.icon, CELL_ICON))
        .on_press(Message::DropdownSelectItem {
            dropdown_id: panel_id,
            cmd: command,
        })
        .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
        .width(CELL)
        .height(CELL)
        .padding((CELL - CELL_ICON) / 2.0);
    let tip = format!("{}\n{} {}", t!(tool.tool.label), t!("Command:"), command);
    let face: Element<'static, Message> = tooltip(face, make_tip(tip), tooltip::Position::Bottom)
        .delay(Duration::from_millis(400))
        .style(tip_style)
        .into();
    let face = keytips::anchor_with(|| keytips::item_anchor(panel_id, command), face);
    if tool.options.is_empty() {
        return face;
    }
    // The arrow opens the tool's submenu under itself.
    let arrow = button(icons::themed_arrow_down(6.0))
        .on_press(Message::ToggleRibbonDropdown(command.to_string()))
        .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
        .width(SPLIT_W)
        .height(CELL)
        .padding(2);
    row![face, PosReport::new(command, arrow)].into()
}

/// Xref fading on/off with its amount (XDWGFADECTL; the switch keeps the
/// amount as a negative value).
fn fade_row<'a>(fade: i32, panel_id: &'static str) -> Element<'a, Message> {
    let on = fade > 0;
    let switch = button(make_icon(
        IconKind::Svg(include_bytes!("../../../assets/icons/underlay_frames.svg")),
        16.0,
    ))
    .on_press(Message::XrefFadeToggle)
    .style(move |theme: &Theme, status| tool_btn_style(theme, on, status))
    .padding(3);
    let amount = fade.unsigned_abs().min(90) as u8;
    let switch = tooltip(
        switch,
        make_tip(format!(
            "{}\n{} XDWGFADECTL",
            t!("Xref fading"),
            t!("Command:")
        )),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(400))
    .style(tip_style);
    let switch = keytips::anchor_with(
        || keytips::item_anchor(panel_id, keytips::XREF_FADE),
        switch,
    );
    row![
        switch,
        keytips::label_anchor(
            || keytips::label_id(&keytips::item_anchor(panel_id, keytips::XREF_FADE)),
            text(t!("Xref fading")).size(11),
        ),
        iced::widget::slider(0..=90u8, amount, Message::XrefFadeSlide)
            .on_release(Message::XrefFadeCommit)
            .width(Fill),
        text(amount.to_string()).size(11).width(22),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// The footer: the pin (while the pointer is on the footer), the panel's
/// title with an up arrow that folds the slide-out away, and its dialog
/// launcher.
fn footer<'a>(hovered: bool, panel: &'static PanelExtras, pinned: bool) -> Element<'a, Message> {
    const SIDE: f32 = 20.0;
    let key = panel.key;
    let pin: Element<'a, Message> = if hovered {
        button(icons::themed(
            if pinned {
                icons::PIN
            } else {
                icons::PIN_ACTIVE
            },
            12.0,
        ))
        .on_press(Message::RibbonSlideOut(SlideOutMsg::TogglePin(key)))
        .style(|theme: &Theme, status| tool_btn_style(theme, false, status))
        .width(SIDE)
        .padding([1, 4])
        .into()
    } else {
        Space::new().width(SIDE).into()
    };
    let title = button(
        row![
            text(t!(panel.title()))
                .size(11.5)
                .style(move |theme: &Theme| text::Style {
                    color: Some(if pinned {
                        chrome::title_text(theme)
                    } else {
                        chrome::strong(theme)
                    }),
                }),
            icons::themed_arrow_up(7.0),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(Message::RibbonSlideOut(SlideOutMsg::Collapse(key)))
    .style(|theme: &Theme, status| tool_btn_style(theme, false, status))
    .padding([0, 4]);
    let launcher = launcher(panel).unwrap_or_else(|| Space::new().width(SIDE).into());
    let bar = container(
        row![
            pin,
            Space::new().width(Fill),
            title,
            Space::new().width(Fill),
            launcher
        ]
        .align_y(iced::Center)
        .padding([0, 3]),
    )
    .center_y(FOOTER_H)
    .width(Fill)
    .style(move |theme: &Theme| container::Style {
        background: Some(Background::Color(if pinned {
            chrome::title_bar(theme)
        } else {
            chrome::hover(theme)
        })),
        ..Default::default()
    });
    mouse_area(bar)
        .on_enter(Message::RibbonSlideOut(SlideOutMsg::OverFooter(Some(key))))
        .on_exit(Message::RibbonSlideOut(SlideOutMsg::OverFooter(None)))
        .into()
}

/// The submenu of the split tool `tool`, under its arrow.
fn options_list<'a>(
    ribbon: &Ribbon,
    tool: &'static MoreTool,
    win: (f32, f32),
) -> Element<'a, Message> {
    let command = tool.command();
    let rows: Vec<Element<'a, Message>> = tool
        .options
        .iter()
        .map(|&(cmd, label)| {
            let item = button(text(t!(label)).size(11))
                .on_press(Message::DropdownSelectItem {
                    dropdown_id: command,
                    cmd,
                })
                .style(popup_row_style)
                .width(Fill)
                .height(OPTION_HEIGHT)
                .padding([3, 7]);
            keytips::anchor_with(|| keytips::item_anchor(command, cmd), item)
        })
        .collect();
    let list = container(column(rows))
        .style(popup_panel_style)
        .width(OPTIONS_W);
    let list = opaque(
        mouse_area(list)
            .on_enter(Message::RibbonSlideOut(SlideOutMsg::OverSubmenu(true)))
            .on_exit(Message::RibbonSlideOut(SlideOutMsg::OverSubmenu(false))),
    );
    let (align_right, h_pad, top) = ribbon.dd_anchor(command, OPTIONS_W, win.0);
    position_ribbon_dropdown(list, align_right, h_pad, top)
}

/// The open slide-out `id`, or a submenu of one of its tools over it. A
/// click elsewhere closes it.
pub(super) fn overlay<'a>(ribbon: &Ribbon, id: &str, win: (f32, f32)) -> Element<'a, Message> {
    let panel = panel_for_dropdown(id).expect("registered ribbon extension");
    let mut layers: Vec<Element<'a, Message>> = Vec::new();
    // A pinned slide-out shows on its own layer.
    if !ribbon.layout.pinned_slideouts.contains(panel.key) {
        if let Some((left, top, width)) = slide_out_place(ribbon, panel) {
            layers.push(placed(
                slide_out(ribbon, panel, width, false),
                left,
                top,
                width,
                win,
            ));
        }
    }
    if let Some(tool) = panel
        .tools()
        .find(|tool| tool.command() == id && !tool.options.is_empty())
    {
        layers.push(options_list(ribbon, tool, win));
    }
    dropdown_backdrop(stack(layers).width(Fill).height(Fill).into())
}

#[cfg(test)]
mod tests {
    use super::extras::{DRAW_ID, MODIFY_ID};
    use super::parent_panel;

    #[test]
    fn a_pinned_slide_out_stays_and_collapsing_unpins_it() {
        use super::{Ribbon, SlideLook, SlideOutMsg};
        use iced::time::Instant;
        let mut ribbon = Ribbon::default();
        let now = Instant::now();
        ribbon.toggle_dropdown(DRAW_ID);
        assert_eq!(ribbon.slide_look("draw/Draw"), SlideLook::Open);

        // Pinning saves the pin and leaves the dropdown slot free.
        assert!(ribbon.on_slide_out(SlideOutMsg::TogglePin("draw/Draw"), now));
        assert_eq!(ribbon.slide_look("draw/Draw"), SlideLook::Pinned);
        assert!(ribbon.open_dropdown.is_none());
        // Another panel's slide-out opens beside it.
        ribbon.toggle_dropdown(MODIFY_ID);
        assert_eq!(ribbon.slide_look("draw/Modify"), SlideLook::Open);
        assert_eq!(ribbon.slide_look("draw/Draw"), SlideLook::Pinned);

        assert!(ribbon.on_slide_out(SlideOutMsg::Collapse("draw/Draw"), now));
        assert_eq!(ribbon.slide_look("draw/Draw"), SlideLook::Closed);
        // Collapsing an unpinned slide-out closes it, with nothing to save.
        assert!(!ribbon.on_slide_out(SlideOutMsg::Collapse("draw/Modify"), now));
        assert_eq!(ribbon.slide_look("draw/Modify"), SlideLook::Closed);
    }

    #[test]
    fn an_unpinned_slide_out_closes_after_the_pointer_leaves_it_and_its_panel() {
        use super::{Ribbon, SlideOutMsg};
        use iced::time::Instant;
        use std::time::Duration;
        let mut ribbon = Ribbon::default();
        let t0 = Instant::now();
        ribbon.toggle_dropdown(DRAW_ID);
        ribbon.on_slide_out(SlideOutMsg::EnterPanel("draw/Draw"), t0);
        // Crossing from the panel to the slide-out keeps it open.
        ribbon.on_slide_out(SlideOutMsg::LeavePanel("draw/Draw"), t0);
        ribbon.on_slide_out(SlideOutMsg::OverSlideOut(true), t0);
        ribbon.tick(t0 + Duration::from_secs(1));
        assert!(ribbon.open_dropdown.is_some());
        // Leaving both closes it after the delay.
        let t1 = t0 + Duration::from_secs(2);
        ribbon.on_slide_out(SlideOutMsg::OverSlideOut(false), t1);
        ribbon.tick(t1 + Duration::from_millis(100));
        assert!(ribbon.open_dropdown.is_some());
        ribbon.tick(t1 + Duration::from_millis(400));
        assert!(ribbon.open_dropdown.is_none());
        assert!(!ribbon.wants_tick());
    }

    #[test]
    fn extension_submenus_resolve_to_their_parent_panel() {
        assert_eq!(parent_panel("MULTIPOINT"), Some(DRAW_ID));
        assert_eq!(parent_panel("DRAWORDER_FRONT"), Some(MODIFY_ID));
        assert_eq!(parent_panel("unknown"), None);
    }
}
