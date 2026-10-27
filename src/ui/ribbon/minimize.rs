//! The minimized ribbon: the minimize button after the tabs, the row of
//! panel titles or panel buttons that replaces the tool area, and the popups
//! that open from a tab or a panel title.

use std::time::Duration;

use iced::time::Instant;
use iced::widget::{button, column, container, mouse_area, opaque, row, text, tooltip, Space};
use iced::{Background, Border, Element, Fill, Length, Padding, Theme};

use super::widgets::{
    make_icon, make_tip, popup_panel_style, tip_style, tool_btn_style, top_hist_btn_style,
    TOOL_BAR_H,
};
use super::{
    draw_panel, extras, first_tool_icon, keytips, render_group, ribbon_menu, visibility_panel,
    CollapseMode, MinimizePref, Ribbon, RibbonMinimize,
};
use crate::app::Message;
use crate::ui::icons;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::{dropdown_bounds, PosReport};

/// The dropdown that picks what the minimize button does.
pub const MINIMIZE_MENU_ID: &str = "RIBBON_MINIMIZE";
/// The tab row, which the tab popup opens under.
pub(super) const TAB_BAR_ID: &str = "ribbon_tab_bar";
/// The open popup, which the key tips' hint bar moves under.
pub(super) const POPUP_ID: &str = "ribbon_minimized_popup";
/// Prefix of the anchors of the panel titles and buttons.
const OPENER_PREFIX: &str = "minimized:";
/// How long the pointer rests on a tab or title before its popup opens.
const OPEN_DELAY: Duration = Duration::from_millis(150);
/// How long a popup stays after the pointer leaves it and its opener.
const CLOSE_DELAY: Duration = Duration::from_millis(1000);
/// Two clicks on a tab within this time are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const TITLE_ROW_H: f32 = 19.0;
const BUTTON_ROW_H: f32 = 36.0;
const BUTTON_ICON: f32 = metrics::SMALL_ICON;
const MENU_W: f32 = 210.0;

/// What a popup opens from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opener {
    Tab(usize),
    Panel(&'static str),
}

#[derive(Clone, Debug)]
pub enum MinimizeMsg {
    /// The minimize button's face: the next minimize state.
    Toggle,
    /// What the face does; choosing one minimize state also switches to it.
    SetPref(MinimizePref),
    /// The pointer rests on a tab or panel title.
    EnterOpener(Opener),
    /// The pointer left that tab or panel title.
    LeaveOpener(Opener),
    EnterPopup,
    LeavePopup,
    /// A panel title or button was clicked.
    Click(Opener),
}

/// Popup state of the minimized ribbon.
#[derive(Default)]
pub(super) struct MinState {
    /// Minimized to tabs: the tool area of the tab on show is open.
    tool_area: bool,
    /// The tab or panel title the pointer is on.
    over_opener: Option<Opener>,
    over_popup: bool,
    /// The popup the pointer rests on, and when it opens.
    opening: Option<(Opener, Instant)>,
    /// When the open popup closes.
    closing: Option<Instant>,
    /// The last tab clicked, for double clicks.
    last_tab_click: Option<(usize, Instant)>,
}

/// The minimize state the button's face moves to from `current`.
pub(super) fn next(current: RibbonMinimize, pref: MinimizePref) -> RibbonMinimize {
    use RibbonMinimize::*;
    let target = match pref {
        MinimizePref::Cycle => {
            return match current {
                Off => Tabs,
                Tabs => PanelTitles,
                PanelTitles => PanelButtons,
                PanelButtons => Off,
            };
        }
        MinimizePref::Tabs => Tabs,
        MinimizePref::PanelTitles => PanelTitles,
        MinimizePref::PanelButtons => PanelButtons,
    };
    if current == Off {
        target
    } else {
        Off
    }
}

impl Ribbon {
    pub fn minimize(&self) -> RibbonMinimize {
        self.layout.minimize
    }

    fn set_minimize(&mut self, minimize: RibbonMinimize) {
        self.close_popup();
        self.close_dropdown();
        self.layout.minimize = minimize;
    }

    /// Whether a popup of the minimized ribbon shows.
    fn popup_open(&self) -> bool {
        match self.layout.minimize {
            RibbonMinimize::Off => false,
            RibbonMinimize::Tabs => self.min.tool_area,
            RibbonMinimize::PanelTitles | RibbonMinimize::PanelButtons => {
                self.collapsed_open.is_some()
            }
        }
    }

    /// Close the minimized ribbon's popup; whether one was open.
    pub fn close_popup(&mut self) -> bool {
        let open = self.popup_open();
        self.min.tool_area = false;
        self.min.opening = None;
        self.min.closing = None;
        self.min.over_popup = false;
        if self.layout.minimize != RibbonMinimize::Off {
            self.collapsed_open = None;
        }
        open
    }

    /// Whether a popup or slide-out is due to open or close, so the app
    /// keeps time.
    pub fn wants_tick(&self) -> bool {
        self.min.opening.is_some()
            || self.min.closing.is_some()
            || self.slide.closing.is_some()
            || self.search_flashing()
    }

    fn open_popup(&mut self, opener: Opener) {
        self.min.opening = None;
        self.min.closing = None;
        match opener {
            Opener::Tab(index) => {
                if self.layout.minimize != RibbonMinimize::Tabs {
                    return;
                }
                if index != self.active {
                    self.close_dropdown();
                    self.select(index);
                }
                self.min.tool_area = true;
            }
            Opener::Panel(title) => {
                if self.collapsed_open.as_deref() != Some(title) {
                    self.close_dropdown();
                    self.collapsed_open = Some(title.to_string());
                }
            }
        }
    }

    /// Start the close delay once the pointer is off the popup and its opener.
    fn arm_close(&mut self, now: Instant) {
        if self.popup_open() && self.min.over_opener.is_none() && !self.min.over_popup {
            self.min.closing = Some(now + CLOSE_DELAY);
        }
    }

    /// Apply a minimize message. Returns whether the layout changed and
    /// should be saved.
    pub fn on_minimize(&mut self, msg: MinimizeMsg, now: Instant) -> bool {
        match msg {
            MinimizeMsg::Toggle => {
                self.set_minimize(next(self.layout.minimize, self.layout.minimize_pref));
                return true;
            }
            MinimizeMsg::SetPref(pref) => {
                self.layout.minimize_pref = pref;
                self.close_dropdown();
                let mode = match pref {
                    MinimizePref::Cycle => None,
                    MinimizePref::Tabs => Some(RibbonMinimize::Tabs),
                    MinimizePref::PanelTitles => Some(RibbonMinimize::PanelTitles),
                    MinimizePref::PanelButtons => Some(RibbonMinimize::PanelButtons),
                };
                if let Some(mode) = mode {
                    self.set_minimize(mode);
                }
                return true;
            }
            MinimizeMsg::EnterOpener(opener) => {
                self.min.over_opener = Some(opener);
                self.min.closing = None;
                let showing = match opener {
                    Opener::Tab(index) => self.min.tool_area && index == self.active,
                    Opener::Panel(title) => self.collapsed_open.as_deref() == Some(title),
                };
                self.min.opening = (!showing).then_some((opener, now + OPEN_DELAY));
            }
            // Moving onto a neighbour can report entering it before leaving
            // this one: only what this opener started is undone.
            MinimizeMsg::LeaveOpener(opener) => {
                if self.min.over_opener == Some(opener) {
                    self.min.over_opener = None;
                }
                if self
                    .min
                    .opening
                    .is_some_and(|(pending, _)| pending == opener)
                {
                    self.min.opening = None;
                }
                self.arm_close(now);
            }
            MinimizeMsg::EnterPopup => {
                self.min.over_popup = true;
                self.min.closing = None;
            }
            MinimizeMsg::LeavePopup => {
                self.min.over_popup = false;
                self.arm_close(now);
            }
            MinimizeMsg::Click(opener) => {
                if matches!(opener, Opener::Panel(title) if self.collapsed_open.as_deref() == Some(title))
                {
                    self.close_popup();
                } else {
                    self.open_popup(opener);
                }
            }
        }
        false
    }

    /// Open or close what is due: a popup of the minimized ribbon, or an
    /// unpinned slide-out the pointer left.
    pub fn tick(&mut self, now: Instant) {
        if let Some((opener, at)) = self.min.opening {
            if now >= at {
                self.open_popup(opener);
            }
        }
        if let Some(at) = self.min.closing {
            if now >= at {
                // An open dropdown, menu or key tips keep the popup.
                if self.open_dropdown.is_some() || self.menu_open() || self.keytips.is_some() {
                    self.min.closing = Some(now + CLOSE_DELAY);
                } else {
                    self.close_popup();
                }
            }
        }
        self.slide_out_tick(now);
        self.search_tick(now);
    }

    /// A click on tab `index`: shows the tab, opens its tool area when the
    /// ribbon is minimized to tabs, and on a double click does what the
    /// minimize button does. Returns whether the layout changed.
    pub fn click_tab(&mut self, index: usize, now: Instant) -> bool {
        let double = self.min.last_tab_click.is_some_and(|(tab, at)| {
            tab == index && now.saturating_duration_since(at) <= DOUBLE_CLICK
        });
        if double {
            self.min.last_tab_click = None;
            self.select(index);
            self.set_minimize(next(self.layout.minimize, self.layout.minimize_pref));
            return true;
        }
        self.min.last_tab_click = Some((index, now));
        if self.layout.minimize == RibbonMinimize::Tabs {
            if self.min.tool_area && index == self.active {
                self.close_popup();
            } else {
                self.open_popup(Opener::Tab(index));
            }
        } else {
            self.select(index);
        }
        false
    }

    /// A tab chosen by its key tip: its tool area opens when the ribbon is
    /// minimized to tabs, so the codes of its buttons can show.
    pub(super) fn keytip_tab(&mut self, index: usize) {
        self.select(index);
        if self.layout.minimize == RibbonMinimize::Tabs {
            self.open_popup(Opener::Tab(index));
        }
    }

    /// The minimize split button after the last tab.
    pub(super) fn minimize_button(&self) -> Element<'_, Message> {
        let minimized = self.layout.minimize != RibbonMinimize::Off;
        let to_full = next(self.layout.minimize, self.layout.minimize_pref) == RibbonMinimize::Off;
        let tip = if to_full {
            crate::tr!("ribbon-menu", "show-full")
        } else if self.layout.minimize_pref == MinimizePref::Cycle {
            crate::tr!("ribbon-menu", "minimize-cycle")
        } else {
            crate::tr!("ribbon-menu", "minimize")
        };
        let chevron = if minimized {
            icons::themed_arrow_down(10.0)
        } else {
            icons::themed_arrow_up(10.0)
        };
        let menu_open = self.open_dropdown.as_deref() == Some(MINIMIZE_MENU_ID);
        let face = tooltip(
            button(chevron)
                .on_press(Message::RibbonMinimize(MinimizeMsg::Toggle))
                .style(|theme: &Theme, status| top_hist_btn_style(theme, false, false, status))
                .height(22)
                .padding([2, 5]),
            make_tip(tip),
            tooltip::Position::Bottom,
        )
        .delay(Duration::from_millis(400))
        .style(tip_style);
        let arrow = button(icons::themed_arrow_down(7.0))
            .on_press(Message::ToggleRibbonDropdown(MINIMIZE_MENU_ID.to_string()))
            .style(move |theme: &Theme, status| top_hist_btn_style(theme, true, menu_open, status))
            .height(22)
            .padding([2, 3]);
        PosReport::new(MINIMIZE_MENU_ID, row![face, arrow].align_y(iced::Center)).into()
    }

    /// The list of what the minimize button does, under its arrow.
    pub(super) fn minimize_menu(&self, win: (f32, f32)) -> Element<'_, Message> {
        let choices = [
            (
                MinimizePref::Tabs,
                crate::tr!("ribbon-menu", "minimize-to-tabs"),
            ),
            (
                MinimizePref::PanelTitles,
                crate::tr!("ribbon-menu", "minimize-to-panel-titles"),
            ),
            (
                MinimizePref::PanelButtons,
                crate::tr!("ribbon-menu", "minimize-to-panel-buttons"),
            ),
            (
                MinimizePref::Cycle,
                crate::tr!("ribbon-menu", "cycle-through-all"),
            ),
        ];
        let current = self.layout.minimize_pref;
        let rows: Vec<Element<'_, Message>> = choices
            .into_iter()
            .map(|(pref, label)| {
                let choose = Message::RibbonMinimize(MinimizeMsg::SetPref(pref));
                ribbon_menu::check_row(label, pref == current, Some(choose)).into()
            })
            .collect();
        // The same menu as the ribbon's right-click menu.
        let panel = ribbon_menu::panel_of_width(rows, MENU_W);
        self.place_dropdown(MINIMIZE_MENU_ID, panel, MENU_W, win.0)
    }

    /// The row that stands for the tool area when the ribbon is minimized
    /// to panel titles or panel buttons: one opener per panel.
    pub(super) fn minimized_row(&self, is_start: bool) -> Element<'_, Message> {
        let buttons = self.layout.minimize == RibbonMinimize::PanelButtons;
        let height = if buttons { BUTTON_ROW_H } else { TITLE_ROW_H };
        let openers = self.groups_for(self.active).map(|group| {
            let title = group.title;
            let open = self.collapsed_open.as_deref() == Some(title);
            let label = text(crate::t!(title)).size(if buttons { 12.0 } else { 11.5 });
            let content: Element<'_, Message> = match first_tool_icon(group).filter(|_| buttons) {
                Some(icon) => row![make_icon(icon, BUTTON_ICON), label]
                    .spacing(6)
                    .align_y(iced::Center)
                    .into(),
                None => label.into(),
            };
            let mut opener = button(container(content).center_y(Fill))
                .style(move |theme: &Theme, status| tool_btn_style(theme, open, status))
                .height(height)
                .padding([0, 9]);
            // The Start tab has no drawing for the tools to work on.
            if !is_start {
                opener = opener.on_press(Message::RibbonMinimize(MinimizeMsg::Click(
                    Opener::Panel(title),
                )));
            }
            let opener: Element<'_, Message> = if is_start {
                opener.into()
            } else {
                mouse_area(opener)
                    .on_enter(Message::RibbonMinimize(MinimizeMsg::EnterOpener(
                        Opener::Panel(title),
                    )))
                    .on_exit(Message::RibbonMinimize(MinimizeMsg::LeaveOpener(
                        Opener::Panel(title),
                    )))
                    .into()
            };
            keytips::anchor_with(
                || keytips::collapsed_anchor(title),
                PosReport::owned(format!("{OPENER_PREFIX}{title}"), opener),
            )
        });
        container(row(openers).spacing(1).align_y(iced::Center))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::panel(theme))),
                border: Border {
                    color: chrome::page(theme),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .padding([0, 2])
            .height(height + 2.0)
            .width(Fill)
            .into()
    }

    /// The open popup of the minimized ribbon, over the drawing: the tool
    /// area of the tab on show, or one panel under its title.
    pub fn minimized_popup(
        &self,
        is_paper: bool,
        is_start: bool,
        show_block_palette: bool,
        visibility: Option<crate::ui::window::block_visibility::RibbonContext>,
        win: (f32, f32),
    ) -> Option<Element<'_, Message>> {
        if !self.popup_open() {
            return None;
        }
        let (content, left, top) = if self.layout.minimize == RibbonMinimize::Tabs {
            let top = dropdown_bounds(TAB_BAR_ID).map_or(self.tab_bar_height(), |b| b.y + b.height);
            // A popup is never squeezed: its panels show at full size.
            let area = self.tool_area(
                is_paper,
                is_start,
                show_block_palette,
                visibility,
                CollapseMode::Full,
            );
            (area, 0.0, top)
        } else {
            let title = self.collapsed_open.as_deref()?;
            let anchor = dropdown_bounds(&format!("{OPENER_PREFIX}{title}"))?;
            let panel = self.panel_popup(title, show_block_palette, visibility)?;
            (panel, anchor.x, anchor.y + anchor.height)
        };
        let content = PosReport::new(
            POPUP_ID,
            opaque(
                mouse_area(content)
                    .on_enter(Message::RibbonMinimize(MinimizeMsg::EnterPopup))
                    .on_exit(Message::RibbonMinimize(MinimizeMsg::LeavePopup)),
            ),
        );
        let width = if self.layout.minimize == RibbonMinimize::Tabs {
            Length::Fill
        } else {
            Length::Shrink
        };
        let placed = container(container(content).width(width))
            .padding(Padding {
                top,
                left: left.clamp(0.0, (win.0 - 40.0).max(0.0)),
                ..Padding::ZERO
            })
            .width(Fill)
            .height(Fill);
        Some(placed.into())
    }

    /// Panel `title` of the tab on show, with a footer holding its slide-out
    /// opener and its dialog launcher.
    fn panel_popup(
        &self,
        title: &str,
        show_block_palette: bool,
        visibility: Option<crate::ui::window::block_visibility::RibbonContext>,
    ) -> Option<Element<'_, Message>> {
        let module = self.modules.get(self.active)?;
        let group = self
            .groups_for(self.active)
            .find(|group| group.title == title)?;
        let key = extras::panel_key(module.id(), group.title);
        let body = if module.id() == "block_editor" && group.title == "Visibility" {
            visibility_panel::panel(visibility).full()
        } else {
            let style_ctx = self.style_context();
            render_group(
                false,
                group,
                &key,
                false,
                self.slide_look(&key),
                Vec::new(),
                self.lit_tool(),
                &self.open_dropdown,
                &self.last_cmd,
                self.toggle_state(show_block_palette),
                &self.layer_infos,
                &self.active_layer,
                self.active_color,
                &self.active_linetype,
                self.active_lineweight,
                &style_ctx,
            )
        };
        let mut footer = row![].spacing(6).align_y(iced::Center);
        let extras = draw_panel::panel_for_group(&key);
        if let Some(extras) = extras {
            let open = self
                .open_dropdown
                .as_deref()
                .and_then(draw_panel::parent_panel)
                == Some(extras.id);
            let more = button(
                row![
                    text(crate::tr!("ribbon-menu", "more-tools")).size(11),
                    if open {
                        icons::themed_arrow_up(8.0)
                    } else {
                        icons::themed_arrow_down(8.0)
                    },
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(extras.id.to_string()))
            .style(move |theme: &Theme, status| tool_btn_style(theme, open, status))
            .padding([1, 6]);
            footer = footer.push(keytips::anchor(extras.id, PosReport::new(extras.id, more)));
        }
        footer = footer.push(Space::new().width(Fill));
        if let Some(launcher) = extras.and_then(draw_panel::launcher) {
            footer = footer.push(launcher);
        }
        let popup = column![
            container(body).height(TOOL_BAR_H),
            container(footer).padding([2, 4]).width(Fill),
        ];
        Some(container(popup).style(popup_panel_style).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_cycles_or_switches_with_the_preference() {
        use RibbonMinimize::*;
        let cycle = MinimizePref::Cycle;
        assert_eq!(next(Off, cycle), Tabs);
        assert_eq!(next(Tabs, cycle), PanelTitles);
        assert_eq!(next(PanelTitles, cycle), PanelButtons);
        assert_eq!(next(PanelButtons, cycle), Off);
        assert_eq!(next(Off, MinimizePref::PanelButtons), PanelButtons);
        assert_eq!(next(PanelButtons, MinimizePref::PanelButtons), Off);
        // From another state the preference still leads back to full.
        assert_eq!(next(Tabs, MinimizePref::PanelTitles), Off);
    }

    fn tab(ribbon: &Ribbon, id: &str) -> usize {
        ribbon
            .modules
            .iter()
            .position(|module| module.id() == id)
            .unwrap()
    }

    #[test]
    fn a_hovered_tab_opens_after_the_delay_and_closes_after_leaving() {
        let mut ribbon = Ribbon::default();
        ribbon.on_minimize(MinimizeMsg::Toggle, Instant::now());
        assert_eq!(ribbon.minimize(), RibbonMinimize::Tabs);
        let insert = tab(&ribbon, "insert");
        let t0 = Instant::now();
        ribbon.on_minimize(MinimizeMsg::EnterOpener(Opener::Tab(insert)), t0);
        ribbon.tick(t0 + Duration::from_millis(100));
        assert!(!ribbon.popup_open());
        ribbon.tick(t0 + Duration::from_millis(160));
        assert!(ribbon.popup_open());
        assert_eq!(ribbon.active, insert);

        // Leaving the tab for the popup keeps it.
        let t1 = t0 + Duration::from_millis(200);
        ribbon.on_minimize(MinimizeMsg::LeaveOpener(Opener::Tab(insert)), t1);
        ribbon.on_minimize(MinimizeMsg::EnterPopup, t1);
        ribbon.tick(t1 + Duration::from_secs(2));
        assert!(ribbon.popup_open());

        // Leaving both closes it a second later, unless a dropdown is open.
        let t2 = t1 + Duration::from_secs(3);
        ribbon.on_minimize(MinimizeMsg::LeavePopup, t2);
        ribbon.open_dropdown = Some("CIRCLE".into());
        ribbon.tick(t2 + Duration::from_millis(1100));
        assert!(ribbon.popup_open());
        ribbon.open_dropdown = None;
        ribbon.tick(t2 + Duration::from_millis(2200));
        assert!(!ribbon.popup_open());
        assert!(!ribbon.wants_tick());
    }

    #[test]
    fn moving_left_onto_a_neighbour_opens_it() {
        let mut ribbon = Ribbon::default();
        ribbon.on_minimize(MinimizeMsg::Toggle, Instant::now());
        let draw = tab(&ribbon, "draw");
        let insert = tab(&ribbon, "insert");
        let t0 = Instant::now();
        ribbon.on_minimize(MinimizeMsg::EnterOpener(Opener::Tab(insert)), t0);
        ribbon.tick(t0 + Duration::from_millis(200));
        assert_eq!(ribbon.active, insert);
        // The tab on the left reports the pointer before the right one lets
        // go of it.
        let t1 = t0 + Duration::from_millis(300);
        ribbon.on_minimize(MinimizeMsg::EnterOpener(Opener::Tab(draw)), t1);
        ribbon.on_minimize(MinimizeMsg::LeaveOpener(Opener::Tab(insert)), t1);
        ribbon.tick(t1 + Duration::from_millis(200));
        assert!(ribbon.popup_open());
        assert_eq!(ribbon.active, draw);
    }

    #[test]
    fn choosing_a_minimize_state_switches_to_it() {
        let mut ribbon = Ribbon::default();
        let now = Instant::now();
        for (pref, mode) in [
            (MinimizePref::PanelButtons, RibbonMinimize::PanelButtons),
            (MinimizePref::Tabs, RibbonMinimize::Tabs),
            (MinimizePref::PanelTitles, RibbonMinimize::PanelTitles),
        ] {
            assert!(ribbon.on_minimize(MinimizeMsg::SetPref(pref), now));
            assert_eq!(ribbon.minimize(), mode);
            assert_eq!(ribbon.layout.minimize_pref, pref);
        }
        // Cycling only changes what the button does next.
        ribbon.on_minimize(MinimizeMsg::SetPref(MinimizePref::Cycle), now);
        assert_eq!(ribbon.minimize(), RibbonMinimize::PanelTitles);
    }

    #[test]
    fn a_double_click_on_a_tab_does_what_the_button_does() {
        let mut ribbon = Ribbon::default();
        let draw = tab(&ribbon, "draw");
        let t0 = Instant::now();
        assert!(!ribbon.click_tab(draw, t0));
        assert!(ribbon.click_tab(draw, t0 + Duration::from_millis(200)));
        assert_eq!(ribbon.minimize(), RibbonMinimize::Tabs);
        // Slow clicks are two clicks.
        let t1 = t0 + Duration::from_secs(5);
        ribbon.click_tab(draw, t1);
        assert!(!ribbon.click_tab(draw, t1 + Duration::from_secs(1)));
        assert_eq!(ribbon.minimize(), RibbonMinimize::Tabs);
    }

    #[test]
    fn a_panel_title_click_toggles_its_popup() {
        let mut ribbon = Ribbon::default();
        ribbon.layout.minimize_pref = MinimizePref::PanelTitles;
        ribbon.on_minimize(MinimizeMsg::Toggle, Instant::now());
        assert_eq!(ribbon.minimize(), RibbonMinimize::PanelTitles);
        let now = Instant::now();
        ribbon.on_minimize(MinimizeMsg::Click(Opener::Panel("Draw")), now);
        assert_eq!(ribbon.collapsed_open.as_deref(), Some("Draw"));
        ribbon.on_minimize(MinimizeMsg::Click(Opener::Panel("Draw")), now);
        assert!(!ribbon.popup_open());
        // Back to full, no popup is left open.
        ribbon.on_minimize(MinimizeMsg::Click(Opener::Panel("Modify")), now);
        ribbon.on_minimize(MinimizeMsg::Toggle, now);
        assert_eq!(ribbon.minimize(), RibbonMinimize::Off);
        assert!(ribbon.collapsed_open.is_none());
    }
}
