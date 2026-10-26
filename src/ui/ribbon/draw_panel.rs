//! The slide-outs under panel titles and the panels' dialog launchers, drawn
//! from the registry in `extras`.

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, tooltip};
use iced::{Element, Fill, Theme};

use super::extras::{self, Layout, MoreTool, PanelExtras};
use super::widgets::{
    make_icon, make_tip, muted_text_style, popup_panel_style, popup_row_style, tip_style,
    tool_btn_style, LARGE_DROPDOWN_ARROW_SIZE,
};
use super::{dropdown_backdrop, keytips, position_ribbon_dropdown, Ribbon};
use crate::app::Message;
use crate::modules::IconKind;
use crate::t;
use crate::ui::{icons, wrap_bar::PosReport};

const SCALE: f32 = 0.7;
const CELL: f32 = 36.0 * SCALE;
const GAP: f32 = 3.0 * SCALE;
const OPTION_HEIGHT: f32 = 30.0 * SCALE;
const GROUP_TITLE_ARROW_SIZE: f32 = LARGE_DROPDOWN_ARROW_SIZE * 1.5;

/// Command, label and icon of every slide-out tool.
pub(crate) fn tools() -> impl Iterator<Item = (&'static str, &'static str, &'static [u8])> {
    extras::all()
        .flat_map(PanelExtras::tools)
        .filter_map(|tool| match tool.tool.icon {
            IconKind::Svg(icon) => Some((tool.command(), tool.tool.label, icon)),
            IconKind::Glyph(_) => None,
        })
}

fn panel_for_dropdown(id: &str) -> Option<&'static PanelExtras> {
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

pub(super) fn group_title<'a>(
    title: &'static str,
    key: &str,
    open: &Option<String>,
    active_tool: Option<&str>,
) -> Element<'a, Message> {
    let Some(panel) = panel_for_group(key) else {
        let label = keytips::label_anchor(
            || format!("title:{title}"),
            text(t!(title)).size(9).style(muted_text_style),
        );
        return container(label).padding([1, 4]).into();
    };
    let expanded = open.as_deref().and_then(parent_panel) == Some(panel.id);
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
                keytips::label_anchor(
                    || keytips::label_id(panel.id),
                    PosReport::new(panel.title_id, text(t!(title)).size(9)),
                ),
                arrow
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(Message::ToggleRibbonDropdown(panel.id.to_string()))
        .style(move |theme: &Theme, status| tool_btn_style(theme, lit, status))
        .padding([1, 4]),
    )
    .into();
    let Some(launcher) = launcher(panel) else {
        return title_button;
    };
    row![title_button, launcher]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

/// The corner arrow that runs the dialog command of `panel`, if it has one.
pub(super) fn launcher<'a>(panel: &PanelExtras) -> Option<Element<'a, Message>> {
    let dialog = panel.dialog?;
    // The corner arrow runs the panel's dialog command.
    let launcher = tooltip(
        // A right arrow turned to point into the corner.
        button(
            iced::widget::svg(icons::themed_handle(icons::ARROW_LONG_RIGHT))
                .width(10)
                .height(10)
                .rotation(iced::Radians(std::f32::consts::FRAC_PI_4))
                .style(|theme: &Theme, _| iced::widget::svg::Style {
                    color: Some(theme.palette().background.base.text),
                }),
        )
        .on_press(launcher_message(dialog))
        .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
        .padding([0, 3]),
        make_tip(format!(
            "{}\n{} {dialog}",
            t!(panel.dialog_label),
            t!("Command:")
        )),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(400))
    .style(tip_style);
    // Shrink-sized: a Fill here would make the panel claim the whole row.
    Some(keytips::anchor(keytips::XREF_LAUNCHER, launcher))
}

fn tool_button(tool: &MoreTool, active: bool, panel_id: &'static str) -> Element<'static, Message> {
    let command = tool.command();
    let face = button(make_icon(tool.tool.icon, 23.0 * SCALE))
        .on_press(Message::DropdownSelectItem {
            dropdown_id: panel_id,
            cmd: command,
        })
        .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
        .width(if tool.options.is_empty() {
            CELL
        } else {
            CELL - 11.0 * SCALE
        })
        .height(CELL)
        .padding(3.0 * SCALE);
    let tip = format!("{}\n{} {}", t!(tool.tool.label), t!("Command:"), command);
    let face: Element<'static, Message> = tooltip(face, make_tip(tip), tooltip::Position::Bottom)
        .delay(Duration::from_millis(400))
        .style(tip_style)
        .into();
    let face = keytips::anchor_with(|| keytips::item_anchor(panel_id, command), face);
    if !tool.options.is_empty() {
        row![
            face,
            button(icons::themed_arrow_down(6.0 * SCALE))
                .on_press(Message::ToggleRibbonDropdown(command.to_string()))
                .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
                .width(11.0 * SCALE)
                .height(CELL)
                .padding(2.0 * SCALE)
        ]
        .into()
    } else {
        face
    }
}

/// A slide-out tool as one row: its icon, then its name.
fn labeled_row(panel_id: &'static str, tool: &MoreTool) -> Element<'static, Message> {
    let command = tool.command();
    let row = button(
        row![
            make_icon(tool.tool.icon, 16.0),
            keytips::label_anchor(
                move || keytips::label_id(&keytips::item_anchor(panel_id, command)),
                text(t!(tool.tool.label)).size(11),
            ),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::DropdownSelectItem {
        dropdown_id: panel_id,
        cmd: command,
    })
    .style(popup_row_style)
    .width(Fill)
    .padding([3, 6]);
    keytips::anchor_with(move || keytips::item_anchor(panel_id, command), row)
}

/// The Reference slide-out: Edit Reference, and xref fading on/off with its
/// amount (XDWGFADECTL; the switch keeps the amount as a negative value).
fn reference_overlay<'a>(
    ribbon: &Ribbon,
    panel: &PanelExtras,
    win: (f32, f32),
) -> Element<'a, Message> {
    let width: f32 = 260.0_f32.min((win.0 - 8.0).max(120.0));
    let (_, x, anchor_top) = ribbon.dd_anchor(panel.id, width, win.0);
    let left = x.clamp(0.0, (win.0 - width).max(0.0));
    let top = anchor_top.min((win.1 - 80.0).max(0.0));
    let fade = ribbon.xref_fade;
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
        || keytips::item_anchor(panel.id, keytips::XREF_FADE),
        switch,
    );
    let fading = row![
        switch,
        keytips::label_anchor(
            || keytips::label_id(&keytips::item_anchor(panel.id, keytips::XREF_FADE)),
            text(t!("Xref fading")).size(11),
        ),
        iced::widget::slider(0..=90u8, amount, Message::XrefFadeSlide)
            .on_release(Message::XrefFadeCommit)
            .width(Fill),
        text(amount.to_string()).size(11).width(22),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let rows = panel.tools().map(|tool| labeled_row(panel.id, tool));
    let body = container(column(rows).push(fading).spacing(4).padding(6))
        .width(width)
        .style(|theme: &Theme| {
            let mut style = popup_panel_style(theme);
            style.border.color = theme.palette().primary.base.color;
            style
        });
    dropdown_backdrop(position_ribbon_dropdown(body.into(), false, left, top))
}

/// A slide-out listing each tool as icon and label, one per row.
fn labeled_overlay<'a>(
    ribbon: &Ribbon,
    panel: &PanelExtras,
    win: (f32, f32),
) -> Element<'a, Message> {
    let width: f32 = 200.0_f32.min((win.0 - 8.0).max(120.0));
    let (_, x, anchor_top) = ribbon.dd_anchor(panel.id, width, win.0);
    let left = x.clamp(0.0, (win.0 - width).max(0.0));
    let top = anchor_top.min((win.1 - 80.0).max(0.0));
    let rows = panel.tools().map(|tool| {
        let active = tool.is_active(ribbon.active_tool.as_deref());
        let command = tool.command();
        let item = button(
            row![
                make_icon(tool.tool.icon, 16.0),
                keytips::label_anchor(
                    || keytips::label_id(&keytips::item_anchor(panel.id, command)),
                    text(t!(tool.tool.label)).size(11),
                ),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .on_press(Message::DropdownSelectItem {
            dropdown_id: panel.id,
            cmd: command,
        })
        .style(move |theme: &Theme, status| {
            if active {
                tool_btn_style(theme, true, status)
            } else {
                popup_row_style(theme, status)
            }
        })
        .width(Fill)
        .padding([3, 6]);
        keytips::anchor_with(|| keytips::item_anchor(panel.id, command), item)
    });
    let body = container(column(rows.collect::<Vec<_>>()).spacing(2).padding(6))
        .width(width)
        .style(|theme: &Theme| {
            let mut style = popup_panel_style(theme);
            style.border.color = theme.palette().primary.base.color;
            style
        });
    dropdown_backdrop(position_ribbon_dropdown(body.into(), false, left, top))
}

pub(super) fn overlay<'a>(ribbon: &Ribbon, id: &str, win: (f32, f32)) -> Element<'a, Message> {
    let panel = panel_for_dropdown(id).expect("registered ribbon extension");
    match panel.layout {
        Layout::Reference => return reference_overlay(ribbon, panel, win),
        Layout::List => return labeled_overlay(ribbon, panel, win),
        Layout::Grid => {}
    }
    let tools: Vec<&MoreTool> = panel.tools().collect();
    let width = (7.0 * (CELL + GAP) - GAP + 12.0).min((win.0 - 8.0).max(CELL + 12.0));
    let (_, x, anchor_top) = ribbon.dd_anchor(panel.id, width, win.0);
    let anchor = crate::ui::wrap_bar::dropdown_bounds(panel.title_id);
    let left = anchor
        .map_or(x, |b| b.x + (b.width - width) / 2.0)
        .clamp(0.0, (win.0 - width).max(0.0));
    let top = anchor_top.min((win.1 - 80.0).max(0.0));
    let available_height = (win.1 - top - 4.0).max(1.0);
    let cols = (((width - 12.0 + GAP) / (CELL + GAP)).floor() as usize).clamp(1, 7);
    let option_tool = tools
        .iter()
        .find(|tool| tool.command() == id && !tool.options.is_empty());
    let content_height = option_tool.map_or_else(
        || tools.len().div_ceil(cols) as f32 * (CELL + GAP) - GAP + 12.0,
        |tool| tool.options.len() as f32 * OPTION_HEIGHT,
    );
    let contents: Element<'static, Message> = if let Some(tool) = option_tool {
        column(
            tool.options
                .iter()
                .map(|&(cmd, label)| {
                    let item = button(text(t!(label)).size(11))
                        .on_press(Message::DropdownSelectItem {
                            dropdown_id: tool.command(),
                            cmd,
                        })
                        .style(popup_row_style)
                        .width(Fill)
                        .height(OPTION_HEIGHT)
                        .padding([3, 7]);
                    keytips::anchor_with(|| keytips::item_anchor(tool.command(), cmd), item)
                })
                .collect::<Vec<Element<'static, Message>>>(),
        )
        .into()
    } else {
        column(
            tools
                .chunks(cols)
                .map(|tools| {
                    row(tools
                        .iter()
                        .map(|tool| {
                            tool_button(
                                tool,
                                tool.is_active(ribbon.active_tool.as_deref()),
                                panel.id,
                            )
                        })
                        .collect::<Vec<_>>())
                    .spacing(GAP)
                    .into()
                })
                .collect::<Vec<Element<'static, Message>>>(),
        )
        .spacing(GAP)
        .padding(6)
        .into()
    };
    let panel = container(scrollable(contents).height(content_height.min(available_height)))
        .width(width)
        .style(|theme: &Theme| {
            let mut style = popup_panel_style(theme);
            style.border.color = theme.palette().primary.base.color;
            style
        });
    dropdown_backdrop(position_ribbon_dropdown(panel.into(), false, left, top))
}

#[cfg(test)]
mod tests {
    use super::extras::{DRAW_ID, MODIFY_ID};
    use super::parent_panel;

    #[test]
    fn extension_submenus_resolve_to_their_parent_panel() {
        assert_eq!(parent_panel("MULTIPOINT"), Some(DRAW_ID));
        assert_eq!(parent_panel("DRAWORDER_FRONT"), Some(MODIFY_ID));
        assert_eq!(parent_panel("unknown"), None);
    }
}
