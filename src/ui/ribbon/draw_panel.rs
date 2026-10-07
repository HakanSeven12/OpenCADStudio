//! Additional drawing tools, reached from the Draw panel's title.

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, tooltip};
use iced::{Element, Fill, Theme};

use super::widgets::{
    make_icon, make_tip, muted_text_style, popup_panel_style, popup_row_style, tip_style,
    tool_btn_style, LARGE_DROPDOWN_ARROW_SIZE,
};
use super::{dropdown_backdrop, position_ribbon_dropdown, Ribbon};
use crate::app::Message;
use crate::modules::IconKind;
use crate::t;
use crate::ui::{icons, wrap_bar::PosReport};

pub(super) const PANEL_ID: &str = "draw_extension";
const TITLE_ID: &str = "draw_extension_title";
const SCALE: f32 = 0.7;
const CELL: f32 = 36.0 * SCALE;
const GAP: f32 = 3.0 * SCALE;
const OPTION_HEIGHT: f32 = 30.0 * SCALE;
const GROUP_TITLE_ARROW_SIZE: f32 = LARGE_DROPDOWN_ARROW_SIZE * 1.5;

pub(super) struct Tool {
    pub command: &'static str,
    pub label: &'static str,
    pub icon: &'static [u8],
    pub options: &'static [(&'static str, &'static str)],
}

const TOOLS: &[Tool] = &[
    Tool {
        command: "SPLINE",
        label: "Spline Fit",
        icon: include_bytes!("../../../assets/icons/spline.svg"),
        options: &[],
    },
    Tool {
        command: "SPLINECV",
        label: "Spline CV",
        icon: include_bytes!("../../../assets/icons/spline_cv.svg"),
        options: &[],
    },
    Tool {
        command: "XLINE",
        label: "Construction Line",
        icon: include_bytes!("../../../assets/icons/xline.svg"),
        options: &[],
    },
    Tool {
        command: "RAY",
        label: "Ray",
        icon: include_bytes!("../../../assets/icons/ray.svg"),
        options: &[],
    },
    Tool {
        command: "DIVIDE",
        label: "Divide",
        icon: include_bytes!("../../../assets/icons/divide.svg"),
        options: &[],
    },
    Tool {
        command: "MEASURE",
        label: "Measure",
        icon: include_bytes!("../../../assets/icons/measure.svg"),
        options: &[],
    },
    Tool {
        command: "REGION",
        label: "Region",
        icon: include_bytes!("../../../assets/icons/region.svg"),
        options: &[],
    },
    Tool {
        command: "BOUNDARY",
        label: "Boundary",
        icon: include_bytes!("../../../assets/icons/boundary.svg"),
        options: &[],
    },
    Tool {
        command: "HELIX",
        label: "Helix",
        icon: include_bytes!("../../../assets/icons/helix.svg"),
        options: &[],
    },
    Tool {
        command: "DONUT",
        label: "Donut",
        icon: include_bytes!("../../../assets/icons/donut.svg"),
        options: &[],
    },
    Tool {
        command: "MULTIPOINT",
        label: "Multiple Points",
        icon: include_bytes!("../../../assets/icons/multipoint.svg"),
        options: &[
            ("POINT", "Single Point"),
            ("MULTIPOINT", "Multiple Points"),
            ("DDPTYPE", "Point Style"),
        ],
    },
];

#[derive(Clone, Copy)]
struct Panel {
    id: &'static str,
    /// A command the owning ribbon group must hold, for titles more than one
    /// tab uses ("Block" on Home and Insert).
    anchor: Option<&'static str>,
    title_id: &'static str,
    title: &'static str,
    tools: &'static [Tool],
}

/// The Reference panel's slide-out: Edit Reference, then the xref fading
/// switch and amount (drawn by `reference_overlay`).
const REFERENCE_TOOLS: &[Tool] = &[Tool {
    command: "REFEDIT",
    label: "Edit Reference",
    icon: include_bytes!("../../../assets/icons/edit_block.svg"),
    options: &[],
}];
pub(super) const REFERENCE_PANEL_ID: &str = "reference_extension";

const fn icon_tool(command: &'static str, label: &'static str, icon: &'static [u8]) -> Tool {
    Tool { command, label, icon, options: &[] }
}

/// Home > Block slide-out.
const HOME_BLOCK_TOOLS: &[Tool] = &[
    icon_tool("ATTDEF", "Define Attributes", include_bytes!("../../../assets/icons/attdef.svg")),
    icon_tool("ATTMAN", "Manage Attributes", include_bytes!("../../../assets/icons/attman.svg")),
    icon_tool("ATTSYNC", "Synchronize", include_bytes!("../../../assets/icons/attsync.svg")),
    icon_tool("BASE", "Set Base Point", include_bytes!("../../../assets/icons/base_point.svg")),
];

/// Insert > Block Definition slide-out.
const BLOCK_DEFINITION_TOOLS: &[Tool] = &[
    icon_tool("BASE", "Set Base Point", include_bytes!("../../../assets/icons/base_point.svg")),
    icon_tool("ATTSYNC", "Synchronize", include_bytes!("../../../assets/icons/attsync.svg")),
];

const PANELS: &[Panel] = &[
    Panel {
        id: PANEL_ID,
        anchor: None,
        title_id: TITLE_ID,
        title: "Draw",
        tools: TOOLS,
    },
    Panel {
        id: "modify_extension",
        anchor: None,
        title_id: "modify_extension_title",
        title: "Modify",
        tools: super::modify_panel::TOOLS,
    },
    Panel {
        id: REFERENCE_PANEL_ID,
        anchor: None,
        title_id: "reference_extension_title",
        title: "Reference",
        tools: REFERENCE_TOOLS,
    },
    Panel {
        id: "home_block_extension",
        anchor: Some("BEDIT"),
        title_id: "home_block_extension_title",
        title: "Block",
        tools: HOME_BLOCK_TOOLS,
    },
    Panel {
        id: "block_definition_extension",
        anchor: None,
        title_id: "block_definition_extension_title",
        title: "Block Definition",
        tools: BLOCK_DEFINITION_TOOLS,
    },
];

/// Command, label and icon of every tool in the expanded Draw and Modify
/// panels.
pub(crate) fn tools() -> impl Iterator<Item = (&'static str, &'static str, &'static [u8])> {
    PANELS
        .iter()
        .flat_map(|panel| panel.tools)
        .map(|tool| (tool.command, tool.label, tool.icon))
}

fn panel_for_dropdown(id: &str) -> Option<Panel> {
    PANELS.iter().copied().find(|panel| {
        panel.id == id
            || panel
                .tools
                .iter()
                .any(|tool| tool.command == id && !tool.options.is_empty())
    })
}

pub(super) fn owns_dropdown(id: &str) -> bool {
    panel_for_dropdown(id).is_some()
}

pub(super) fn parent_panel(id: &str) -> Option<&'static str> {
    panel_for_dropdown(id).map(|panel| panel.id)
}

/// The slide-out under the title of the ribbon group `title`, whose items
/// are `group_ids`.
fn panel_for_group(title: &str, group_ids: &[&'static str]) -> Option<Panel> {
    PANELS.iter().copied().find(|panel| {
        panel.title == title
            && !panel.tools.is_empty()
            && panel.anchor.is_none_or(|anchor| group_ids.contains(&anchor))
    })
}

/// Every command the slide-out of the group `title` offers: each tool and
/// each of its submenu options.
pub(super) fn slide_out_commands(
    title: &str,
    group_ids: &[&'static str],
) -> impl Iterator<Item = &'static str> {
    panel_for_group(title, group_ids)
        .into_iter()
        .flat_map(|panel| panel.tools)
        .flat_map(|tool| {
            std::iter::once(tool.command).chain(tool.options.iter().map(|(cmd, _)| *cmd))
        })
}

/// A slide-out tool is lit while it or one of its submenu options runs.
fn tool_is_active(tool: &Tool, active_tool: Option<&str>) -> bool {
    active_tool.is_some_and(|active| {
        tool.command == active || tool.options.iter().any(|(cmd, _)| *cmd == active)
    })
}

pub(super) fn group_title<'a>(
    title: &'static str,
    group_ids: &[&'static str],
    open: &Option<String>,
    active_tool: Option<&str>,
) -> Element<'a, Message> {
    let Some(panel) = panel_for_group(title, group_ids) else {
        return container(text(t!(title)).size(9).style(muted_text_style))
            .padding([1, 4])
            .into();
    };
    let expanded = open.as_deref().and_then(parent_panel) == Some(panel.id);
    // A command running from the closed slide-out lights its title, the way a
    // dropdown lights while one of its items runs.
    let lit = expanded || panel.tools.iter().any(|tool| tool_is_active(tool, active_tool));
    let arrow = if expanded {
        icons::themed_arrow_up(GROUP_TITLE_ARROW_SIZE)
    } else {
        icons::themed_arrow_down(GROUP_TITLE_ARROW_SIZE)
    };
    let title_button: Element<'a, Message> = PosReport::new(
        panel.id,
        button(
            row![
                PosReport::new(panel.title_id, text(t!(title)).size(9)),
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
    if panel.id != REFERENCE_PANEL_ID {
        return title_button;
    }
    // The corner arrow opens the External References palette.
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
        .on_press(Message::RibbonToolClick {
            tool_id: "EXTERNALREFERENCES".to_string(),
            event: crate::modules::ModuleEvent::Command("EXTERNALREFERENCES".to_string()),
        })
        .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
        .padding([0, 3]),
        make_tip(format!("{}\n{} EXTERNALREFERENCES", t!("External References"), t!("Command:"))),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(400))
    .style(tip_style);
    // Shrink-sized: a Fill here would make the panel claim the whole row.
    row![title_button, launcher]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

fn tool_button(tool: &Tool, active: bool, panel_id: &'static str) -> Element<'static, Message> {
    let face = button(make_icon(IconKind::Svg(tool.icon), 23.0 * SCALE))
        .on_press(Message::DropdownSelectItem {
            dropdown_id: panel_id,
            cmd: tool.command,
        })
        .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
        .width(if tool.options.is_empty() {
            CELL
        } else {
            CELL - 11.0 * SCALE
        })
        .height(CELL)
        .padding(3.0 * SCALE);
    let tip = format!("{}\n{} {}", t!(tool.label), t!("Command:"), tool.command);
    let face: Element<'static, Message> = tooltip(face, make_tip(tip), tooltip::Position::Bottom)
        .delay(Duration::from_millis(400))
        .style(tip_style)
        .into();
    if !tool.options.is_empty() {
        row![
            face,
            button(icons::themed_arrow_down(6.0 * SCALE))
                .on_press(Message::ToggleRibbonDropdown(tool.command.to_string()))
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

/// The Reference slide-out: Edit Reference, and xref fading on/off with its
/// amount (XDWGFADECTL; the switch keeps the amount as a negative value).
fn reference_overlay<'a>(ribbon: &Ribbon, panel: Panel, win: (f32, f32)) -> Element<'a, Message> {
    let width: f32 = 260.0_f32.min((win.0 - 8.0).max(120.0));
    let (_, x, anchor_top) = ribbon.dd_anchor(panel.id, width, win.0);
    let left = x.clamp(0.0, (win.0 - width).max(0.0));
    let top = anchor_top.min((win.1 - 80.0).max(0.0));
    let tool = &panel.tools[0];
    let edit = button(
        row![
            make_icon(IconKind::Svg(tool.icon), 16.0),
            text(t!(tool.label)).size(11)
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::DropdownSelectItem {
        dropdown_id: panel.id,
        cmd: tool.command,
    })
    .style(popup_row_style)
    .width(Fill)
    .padding([3, 6]);
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
    let fading = row![
        tooltip(
            switch,
            make_tip(format!("{}\n{} XDWGFADECTL", t!("Xref fading"), t!("Command:"))),
            tooltip::Position::Bottom
        )
        .delay(Duration::from_millis(400))
        .style(tip_style),
        text(t!("Xref fading")).size(11),
        iced::widget::slider(0..=90u8, amount, Message::XrefFadeSlide)
            .on_release(Message::XrefFadeCommit)
            .width(Fill),
        text(amount.to_string()).size(11).width(22),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let body = container(column![edit, fading].spacing(4).padding(6))
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
    if panel.id == REFERENCE_PANEL_ID {
        return reference_overlay(ribbon, panel, win);
    }
    let tools = panel.tools;
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
        .find(|tool| tool.command == id && !tool.options.is_empty());
    let content_height = option_tool.map_or_else(
        || tools.len().div_ceil(cols) as f32 * (CELL + GAP) - GAP + 12.0,
        |tool| tool.options.len() as f32 * OPTION_HEIGHT,
    );
    let contents: Element<'static, Message> = if let Some(tool) = option_tool {
        column(
            tool.options
                .iter()
                .map(|&(cmd, label)| {
                    button(text(t!(label)).size(11))
                        .on_press(Message::DropdownSelectItem {
                            dropdown_id: tool.command,
                            cmd,
                        })
                        .style(popup_row_style)
                        .width(Fill)
                        .height(OPTION_HEIGHT)
                        .padding([3, 7])
                        .into()
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
                                tool_is_active(tool, ribbon.active_tool.as_deref()),
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
    use super::{parent_panel, PANEL_ID, TOOLS};

    #[test]
    fn extension_submenus_resolve_to_their_parent_panel() {
        assert_eq!(parent_panel("MULTIPOINT"), Some(PANEL_ID));
        assert_eq!(parent_panel("DRAWORDER_FRONT"), Some("modify_extension"));
        assert_eq!(parent_panel("unknown"), None);
    }

    #[test]
    fn split_tools_follow_plain_tools() {
        let first_split = TOOLS
            .iter()
            .position(|tool| !tool.options.is_empty())
            .unwrap_or(TOOLS.len());
        assert!(TOOLS[..first_split]
            .iter()
            .all(|tool| tool.options.is_empty()));
        assert!(TOOLS[first_split..]
            .iter()
            .all(|tool| !tool.options.is_empty()));
    }
}
