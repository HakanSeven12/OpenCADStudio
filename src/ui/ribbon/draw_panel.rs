//! Additional drawing tools, reached from the Draw panel's title.

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, tooltip};
use iced::{Element, Fill, Length, Theme};

use super::widgets::{make_icon, make_tip, muted_text_style, popup_panel_style, popup_row_style, tip_style, tool_btn_style};
use super::{dropdown_backdrop, position_ribbon_dropdown, Ribbon};
use crate::app::Message;
use crate::modules::IconKind;
use crate::t;
use crate::ui::{icons, wrap_bar::PosReport};

const PANEL_ID: &str = "draw_extension";
const CELL: f32 = 36.0;
const GAP: f32 = 3.0;

struct Tool {
    command: &'static str,
    label: &'static str,
    icon: &'static [u8],
    options: &'static [(&'static str, &'static str)],
}

include!(concat!(env!("OUT_DIR"), "/draw_panel_tools.rs"));

pub(super) fn owns_dropdown(id: &str) -> bool {
    id == PANEL_ID || TOOLS.iter().any(|tool| tool.command == id && !tool.options.is_empty())
}

pub(super) fn group_title<'a>(title: &'static str, open: &Option<String>) -> Element<'a, Message> {
    if title != "Draw" || TOOLS.is_empty() {
        return container(text(t!(title)).size(9).style(muted_text_style)).padding([1, 4]).into();
    }
    let expanded = open.as_deref().is_some_and(owns_dropdown);
    let arrow = if expanded { icons::themed_arrow_up(7.0) } else { icons::themed_arrow_down(7.0) };
    PosReport::new(PANEL_ID, button(row![text(t!(title)).size(9), arrow].spacing(4).align_y(iced::Center))
        .on_press(Message::ToggleRibbonDropdown(PANEL_ID.to_string()))
        .style(move |theme: &Theme, status| tool_btn_style(theme, expanded, status))
        .padding([1, 4])).into()
}

fn tool_button(tool: &Tool, active: bool) -> Element<'static, Message> {
    let face = button(make_icon(IconKind::Svg(tool.icon), 23.0))
        .on_press(Message::DropdownSelectItem { dropdown_id: PANEL_ID, cmd: tool.command })
        .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
        .width(if tool.options.is_empty() { CELL } else { CELL - 11.0 })
        .height(CELL)
        .padding(3);
    let tip = format!("{}\n{} {}", t!(tool.label), t!("Command:"), tool.command);
    let face: Element<'static, Message> = tooltip(face, make_tip(tip), tooltip::Position::Bottom)
        .delay(Duration::from_millis(400)).style(tip_style).into();
    if !tool.options.is_empty() {
        row![face, button(icons::themed_arrow_down(6.0))
            .on_press(Message::ToggleRibbonDropdown(tool.command.to_string()))
            .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
            .width(11).height(CELL).padding(2)].into()
    } else {
        face
    }
}

pub(super) fn overlay<'a>(ribbon: &Ribbon, id: &str, win: (f32, f32)) -> Element<'a, Message> {
    let width = (7.0 * (CELL + GAP) - GAP + 12.0).min((win.0 - 8.0).max(CELL + 12.0));
    let (_, x, anchor_top) = ribbon.dd_anchor(PANEL_ID, width, win.0);
    let anchor = crate::ui::wrap_bar::dropdown_bounds(PANEL_ID);
    let left = anchor.map_or(x, |b| b.x).clamp(0.0, (win.0 - width).max(0.0));
    let top = anchor_top.min((win.1 - 80.0).max(0.0));
    let available_height = (win.1 - top - 4.0).max(1.0);
    let contents: Element<'static, Message> = if let Some(tool) = TOOLS.iter().find(|tool| tool.command == id && !tool.options.is_empty()) {
        column(tool.options.iter().map(|&(cmd, label)| {
            button(text(t!(label)).size(11))
                .on_press(Message::DropdownSelectItem { dropdown_id: tool.command, cmd })
                .style(popup_row_style).width(Fill).padding([6, 10]).into()
        }).collect::<Vec<Element<'static, Message>>>()).into()
    } else {
        let cols = (((width - 12.0 + GAP) / (CELL + GAP)).floor() as usize).clamp(1, 7);
        column(TOOLS.chunks(cols).map(|tools| {
            row(tools.iter().map(|tool| tool_button(tool, ribbon.active_tool.as_deref() == Some(tool.command)))
                .collect::<Vec<_>>()).spacing(GAP).into()
        }).collect::<Vec<Element<'static, Message>>>()).spacing(GAP).padding(6).into()
    };
    let panel = container(scrollable(contents).height(Length::Shrink))
        .max_height(available_height).width(width).style(popup_panel_style);
    dropdown_backdrop(position_ribbon_dropdown(panel.into(), false, left, top))
}
