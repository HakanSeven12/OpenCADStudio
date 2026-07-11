// Shared rendering helpers, button styles, colours, layout constants, and
// free functions used by the Ribbon view/overlay methods.

use rustc_hash::FxHashMap as HashMap;
use std::time::Duration;

use acadrust::types::{Color as AcadColor, LineWeight};
// Ribbon tooltips anchor to the right of their button so the cursor — which
// rests on the button itself — never covers the tip text. (#143)
use iced::widget::tooltip::Position as TipPos;
use iced::widget::{button, column, container, row, svg, text, tooltip};
use iced::{Background, Border, Color, Element, Fill, Length, Padding, Theme};

use crate::app::Message;
use crate::modules::{IconKind, ModuleEvent, RibbonItem, StyleKey, ToolDef};
use crate::ui::wrap_bar::PosReport;
use crate::ui::icons;
use crate::ui::properties::{acad_color_display, LwItem};

use super::LayerInfo;

/// Live on/off state of every ribbon toggle button. Passed as a single value
/// through the render path so adding a new toggle only touches `is_active_tool`
/// plus the `Ribbon::toggle_state` builder — not every render-function signature
/// and call site.
#[derive(Clone, Copy)]
pub(super) struct ToggleState {
    pub wireframe: bool,
    pub ortho_mode: bool,
    pub show_viewcube: bool,
    pub show_ucs_icon: bool,
    pub show_properties: bool,
    pub show_file_tabs: bool,
    pub show_layout_tabs: bool,
}

// ── Layout constants (single source of truth: ROW_H from ui::mod) ─────────

use crate::ui::ROW_H;

/// Icon size inside a 3-row (large) button.
pub(super) const LARGE_ICON: f32 = ROW_H * 1.5;
/// Icon size inside a 1-row (small) button.
pub(super) const SMALL_ICON: f32 = ROW_H * 0.7;
/// Width of a 3-row (large) button.
pub(super) const LARGE_W: f32 = ROW_H * 2.2;
/// Width of a 1-row (small) button.
pub(super) const SMALL_W: f32 = ROW_H;
/// Width of the ▾ strip on a small dropdown.
pub(super) const ARROW_W: f32 = ROW_H * 0.4;
/// Height of the ▾ strip at the bottom of a large dropdown.
pub(super) const LARGE_ARR: f32 = ROW_H * 0.55;
/// Total ribbon tool-area height = 3 × ROW_H + 6 px v-padding + 12 px group-label.
pub(super) const TOOL_BAR_H: f32 = 3.0 * ROW_H + 18.0;
/// Height of a collapsed panel button's large representative face (big icon +
/// its label). A collapsed button is this face plus the title opener, so it is
/// shorter than a full 3-row panel — the ribbon height follows it down.
pub(super) const COLLAPSED_FACE_H: f32 = LARGE_ICON + 20.0;

// ── Tab-bar constants ──────────────────────────────────────────────────────

pub(super) const TOP_ARR_W: f32 = 12.0;
pub(super) const TOP_HIST_W: f32 = 28.0;
pub(super) const TOP_HIST_GAP: f32 = 4.0;

// ── Dropdown / combo ID constants ─────────────────────────────────────────

pub(super) const UNDO_HISTORY_ID: &str = "UNDO_HISTORY";
pub(super) const REDO_HISTORY_ID: &str = "REDO_HISTORY";
pub(super) const LAYER_COMBO_ID: &str = "LAYER_COMBO";
/// Dropdown id for the tab-bar panel-density selector.
pub(super) const COLLAPSE_MODE_ID: &str = "COLLAPSE_MODE";
pub(super) const PROP_COLOR_ID: &str = "PROP_COLOR";
pub(super) const PROP_LINETYPE_ID: &str = "PROP_LINETYPE";
pub(super) const PROP_LW_ID: &str = "PROP_LW";

// ── Colours ────────────────────────────────────────────────────────────────

/// Light chrome grey for the quick-access file-command icons on the top strip.
pub(super) const QA_ICON_COLOR: Color = Color {
    r: 0.82,
    g: 0.83,
    b: 0.85,
    a: 1.0,
};
pub(super) const TOPBAR_BG: Color = Color {
    r: 0.17,
    g: 0.17,
    b: 0.17,
    a: 1.0,
};
pub(super) const RIBBON_BG: Color = Color {
    r: 0.22,
    g: 0.22,
    b: 0.22,
    a: 1.0,
};
pub(super) const BORDER_DARK: Color = Color {
    r: 0.12,
    g: 0.12,
    b: 0.12,
    a: 1.0,
};
pub(super) const ACCENT_BLUE: Color = Color {
    r: 0.20,
    g: 0.55,
    b: 0.90,
    a: 1.0,
};
pub(super) const ACCENT_GOLD: Color = Color {
    r: 0.90,
    g: 0.65,
    b: 0.10,
    a: 1.0,
};
pub(super) const LABEL_COLOR: Color = Color {
    r: 0.82,
    g: 0.82,
    b: 0.82,
    a: 1.0,
};
pub(super) const GROUP_LABEL: Color = Color {
    r: 0.50,
    g: 0.50,
    b: 0.50,
    a: 1.0,
};
pub(super) const TOOL_HOVER: Color = Color {
    r: 0.32,
    g: 0.32,
    b: 0.32,
    a: 1.0,
};
pub(super) const TOOL_ACTIVE: Color = Color {
    r: 0.18,
    g: 0.42,
    b: 0.70,
    a: 1.0,
};
pub(super) const ARROW_COLOR: Color = Color {
    r: 0.65,
    g: 0.65,
    b: 0.65,
    a: 1.0,
};
pub(super) const PANEL_BG: Color = Color {
    r: 0.16,
    g: 0.16,
    b: 0.16,
    a: 0.98,
};
pub(super) const PANEL_BORDER: Color = Color {
    r: 0.32,
    g: 0.32,
    b: 0.32,
    a: 1.0,
};
pub(super) const ROW_HOVER: Color = Color {
    r: 0.24,
    g: 0.24,
    b: 0.24,
    a: 1.0,
};
pub(super) const CHECK_COLOR: Color = Color {
    r: 0.20,
    g: 0.75,
    b: 0.35,
    a: 1.0,
};
pub(super) const ICON_COLOR: Color = Color {
    r: 0.25,
    g: 0.75,
    b: 0.45,
    a: 1.0,
};
pub(super) const LABEL_ON: Color = Color {
    r: 0.92,
    g: 0.92,
    b: 0.92,
    a: 1.0,
};
pub(super) const LABEL_OFF: Color = Color {
    r: 0.72,
    g: 0.72,
    b: 0.72,
    a: 1.0,
};

// ── Combo / dropdown colors ───────────────────────────────────────────────

pub(super) const COMBO_BG: Color = Color {
    r: 0.18,
    g: 0.18,
    b: 0.18,
    a: 1.0,
};
pub(super) const COMBO_HOVER_BG: Color = Color {
    r: 0.26,
    g: 0.26,
    b: 0.26,
    a: 1.0,
};
pub(super) const COMBO_OPEN_BG: Color = Color {
    r: 0.14,
    g: 0.14,
    b: 0.14,
    a: 1.0,
};
pub(super) const COMBO_BORDER: Color = Color {
    r: 0.35,
    g: 0.35,
    b: 0.35,
    a: 1.0,
};
pub(super) const COMBO_ACTIVE_BORDER: Color = Color {
    r: 0.45,
    g: 0.65,
    b: 0.90,
    a: 1.0,
};
pub(super) const COMBO_ARROW: Color = Color {
    r: 0.70,
    g: 0.70,
    b: 0.70,
    a: 1.0,
};
pub(super) const SWATCH_BORDER: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.5,
};
pub(super) const TIP_BG: Color = Color {
    r: 0.13,
    g: 0.13,
    b: 0.13,
    a: 0.97,
};
pub(super) const HIST_INACTIVE_BG: Color = Color {
    r: 0.20,
    g: 0.20,
    b: 0.20,
    a: 1.0,
};

// ── Style context (passed from Ribbon to render_large) ────────────────────

pub(super) struct StyleContext {
    pub text_style_names: Vec<String>,
    pub active_text_style: String,
    pub dim_style_names: Vec<String>,
    pub active_dim_style: String,
    pub mleader_style_names: Vec<String>,
    pub active_mleader_style: String,
    pub table_style_names: Vec<String>,
    pub active_table_style: String,
}

impl StyleContext {
    pub(super) fn names_for(&self, key: StyleKey) -> &[String] {
        match key {
            StyleKey::TextStyle => &self.text_style_names,
            StyleKey::DimStyle => &self.dim_style_names,
            StyleKey::MLeaderStyle => &self.mleader_style_names,
            StyleKey::TableStyle => &self.table_style_names,
        }
    }
    pub(super) fn active_for(&self, key: StyleKey) -> &str {
        match key {
            StyleKey::TextStyle => &self.active_text_style,
            StyleKey::DimStyle => &self.active_dim_style,
            StyleKey::MLeaderStyle => &self.active_mleader_style,
            StyleKey::TableStyle => &self.active_table_style,
        }
    }
}

// ── Layout helpers ─────────────────────────────────────────────────────────

/// Flush up-to-3 small items as a vertical column into the group row.
pub(super) fn flush_small_col<'a>(
    buf: &mut Vec<Element<'a, Message>>,
    out: &mut Vec<Element<'a, Message>>,
) {
    if buf.is_empty() {
        return;
    }
    let col = column(std::mem::take(buf)).spacing(1);
    out.push(col.into());
}

pub(super) fn make_icon(icon: IconKind, size: f32) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(s).size(size * 0.7).color(Color::WHITE).into(),
        IconKind::Svg(bytes) => {
            let handle = svg::Handle::from_memory(bytes);
            svg(handle).width(size).height(size).into()
        }
    }
}

pub(super) fn is_active_tool(
    id: &str,
    active_tool: &Option<String>,
    state: &ToggleState,
) -> bool {
    match id {
        "WIREFRAME" => state.wireframe,
        "SOLID" => !state.wireframe,
        "ORTHO" => state.ortho_mode,
        "PERSP" => !state.ortho_mode,
        "NAVVCUBE" => state.show_viewcube,
        "UCSICON" => state.show_ucs_icon,
        "PROPERTIES" => state.show_properties,
        "FILETAB" => state.show_file_tabs,
        "LAYOUTTAB" => state.show_layout_tabs,
        id => active_tool.as_deref() == Some(id),
    }
}

// ── Button style ───────────────────────────────────────────────────────────

pub(super) fn tool_btn_style(is_active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(match (is_active, status) {
            (true, _) => TOOL_ACTIVE,
            (_, button::Status::Hovered) => TOOL_HOVER,
            (_, button::Status::Pressed) => TOOL_ACTIVE,
            _ => Color::TRANSPARENT,
        })),
        text_color: Color::WHITE,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

// ── Tooltip helpers ────────────────────────────────────────────────────────

pub(super) fn make_tip(tip: String) -> Element<'static, Message> {
    text(tip).size(11).color(Color::WHITE).into()
}

pub(super) fn tip_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(TIP_BG)),
        border: Border {
            color: COMBO_BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

// ── Small item renderer ────────────────────────────────────────────────────

/// Render a 1-row small button (Tool or Dropdown).
pub(super) fn render_small<'a>(
    item: &RibbonItem,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: ToggleState,
) -> Element<'a, Message> {
    match item {
        // Large variants render small too, so the ribbon can shrink a panel of
        // large buttons to icon-only columns when the width is tight.
        RibbonItem::Tool(t) | RibbonItem::LargeTool(t) => {
            let active = is_active_tool(t.id, active_tool, &state);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let tip_text = format!("{}\nCommand: {}", t.label, t.id);
            let btn = button(make_icon(t.icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick { tool_id, event })
                .style(move |_: &Theme, status| tool_btn_style(active, status))
                .width(Length::Fixed(SMALL_W))
                .height(ROW_H)
                .padding([4, 4]);
            tooltip(btn, make_tip(tip_text), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style)
                .into()
        }

        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
            ..
        }
        | RibbonItem::LargeDropdown {
            id,
            icon,
            items,
            default,
            ..
        } => {
            let active = active_tool.as_deref() == Some(*id)
                || items
                    .iter()
                    .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd));
            let dd_open = open_dd.as_deref() == Some(*id);
            let last = last_cmd.get(id).copied().unwrap_or(*default);
            let cur_icon = last_cmd
                .get(id)
                .copied()
                .and_then(|cmd| {
                    items
                        .iter()
                        .find(|(c, _, _)| *c == cmd)
                        .map(|(_, _, ik)| *ik)
                })
                .or_else(|| items.first().map(|(_, _, ik)| *ik))
                .unwrap_or(*icon);

            let cur_label = last_cmd
                .get(id)
                .copied()
                .and_then(|cmd| {
                    items
                        .iter()
                        .find(|(c, _, _)| *c == cmd)
                        .map(|(_, lbl, _)| *lbl)
                })
                .or_else(|| items.first().map(|(_, lbl, _)| *lbl))
                .unwrap_or(*id);
            let tip_text = format!("{}\nCommand: {}", cur_label, last);

            let icon_btn = button(make_icon(cur_icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick {
                    tool_id: last.to_string(),
                    event: ModuleEvent::Command(last.to_string()),
                })
                .style(move |_: &Theme, status| tool_btn_style(active, status))
                .width(Length::Fixed(SMALL_W))
                .height(ROW_H)
                .padding([4, 4]);

            let arr_tip = format!("{} options", cur_label);
            let arr_btn = button(
                container(icons::arrow_down(8.0, ARROW_COLOR))
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(id.to_string()))
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => TOOL_HOVER,
                    _ if dd_open => TOOL_ACTIVE,
                    _ => Color::TRANSPARENT,
                })),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .width(Length::Fixed(ARROW_W))
            .height(ROW_H)
            .padding(0);

            let icon_with_tip = tooltip(icon_btn, make_tip(tip_text), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style);
            let arr_with_tip = tooltip(arr_btn, make_tip(arr_tip), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style);

            PosReport::new(
                *id,
                row![icon_with_tip, arr_with_tip].spacing(0).height(ROW_H),
            )
            .into()
        }

        _ => text("").into(),
    }
}

// ── Large item renderer ────────────────────────────────────────────────────

/// A large dropdown button: the current icon on top, its ▾ directly beneath the
/// icon, then the label at the bottom. Shared by `LargeDropdown` / `Dropdown` in
/// the full ribbon and by a collapsed panel whose representative is a dropdown.
/// `explicit_label` overrides the derived (current-item) label when given.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_large_dropdown<'a>(
    id: &'static str,
    icon: IconKind,
    explicit_label: Option<&str>,
    items: &[(&'static str, &'static str, IconKind)],
    default: &'static str,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
) -> Element<'a, Message> {
    let active = active_tool.as_deref() == Some(id)
        || items
            .iter()
            .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd));
    let dd_open = open_dd.as_deref() == Some(id);
    let last = last_cmd.get(id).copied().unwrap_or(default);
    let cur_icon = last_cmd
        .get(id)
        .copied()
        .and_then(|cmd| items.iter().find(|(c, _, _)| *c == cmd).map(|(_, _, ik)| *ik))
        .or_else(|| items.first().map(|(_, _, ik)| *ik))
        .unwrap_or(icon);
    let cur_label = last_cmd
        .get(id)
        .copied()
        .and_then(|cmd| items.iter().find(|(c, _, _)| *c == cmd).map(|(_, lbl, _)| *lbl))
        .or_else(|| items.first().map(|(_, lbl, _)| *lbl))
        .unwrap_or(id);
    let label = explicit_label.unwrap_or(cur_label);
    let tip_text = format!("{}\nCommand: {}", cur_label, last);
    let arr_tip = format!("{} options", label);

    // Icon on top with the label beneath it, then the ▾ strip at the very bottom.
    let top_btn = button(
        column![
            make_icon(cur_icon, LARGE_ICON),
            text(label.to_string()).size(10).color(LABEL_COLOR),
        ]
        .align_x(iced::Center)
        .spacing(3),
    )
    .on_press(Message::RibbonToolClick {
        tool_id: last.to_string(),
        event: ModuleEvent::Command(last.to_string()),
    })
    .style(move |_: &Theme, status| tool_btn_style(active, status))
    .width(Length::Fixed(LARGE_W))
    .height(Fill)
    .padding(Padding {
        top: 6.0,
        right: 4.0,
        bottom: 2.0,
        left: 4.0,
    });

    let arr_btn = button(
        container(icons::arrow_down(9.0, ARROW_COLOR))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::ToggleRibbonDropdown(id.to_string()))
    .style(move |_: &Theme, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => TOOL_HOVER,
            _ if dd_open => TOOL_ACTIVE,
            _ => Color::TRANSPARENT,
        })),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .width(Length::Fixed(LARGE_W))
    .height(LARGE_ARR)
    .padding(0);

    let top_with_tip = tooltip(top_btn, make_tip(tip_text), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);
    let arr_with_tip = tooltip(arr_btn, make_tip(arr_tip), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);

    PosReport::new(
        id,
        column![top_with_tip, arr_with_tip]
            .spacing(0)
            .width(Length::Fixed(LARGE_W))
            .height(Fill),
    )
    .into()
}

/// Render a full-height large button (LargeTool, LargeDropdown, LayerCombo, StyleCombo).
pub(super) fn render_large<'a>(
    item: &RibbonItem,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: ToggleState,
    layer_infos: &'a [LayerInfo],
    active_layer: &'a str,
    active_color: AcadColor,
    active_linetype: &'a str,
    active_lineweight: LineWeight,
    style_ctx: &StyleContext,
    // When compact, the Properties panel's Match button shrinks to a small icon.
    compact: bool,
) -> Element<'a, Message> {
    match item {
        // A plain Tool renders large too, so a collapsed panel can show its
        // representative tool as a big icon.
        RibbonItem::LargeTool(t) | RibbonItem::Tool(t) => {
            let active = is_active_tool(t.id, active_tool, &state);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let tip_text = format!("{}\nCommand: {}", t.label, t.id);
            let btn = button(
                column![
                    make_icon(t.icon, LARGE_ICON),
                    text(t.label).size(10).color(LABEL_COLOR),
                ]
                .align_x(iced::Center)
                .spacing(3),
            )
            .on_press(Message::RibbonToolClick { tool_id, event })
            .style(move |_: &Theme, status| tool_btn_style(active, status))
            .width(Length::Fixed(LARGE_W))
            .height(Fill)
            .padding(Padding {
                top: 6.0,
                right: 4.0,
                bottom: 4.0,
                left: 4.0,
            });
            tooltip(btn, make_tip(tip_text), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style)
                .into()
        }

        RibbonItem::LargeDropdown {
            id,
            label,
            icon,
            items,
            default,
        } => render_large_dropdown(
            *id,
            *icon,
            Some(*label),
            items,
            *default,
            active_tool,
            open_dd,
            last_cmd,
        ),

        // A plain Dropdown renders large too (used by a collapsed panel whose
        // representative tool is a dropdown).
        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
        } => render_large_dropdown(
            *id, *icon, None, items, *default, active_tool, open_dd, last_cmd,
        ),

        RibbonItem::LayerComboGroup { row2, row3 } => {
            const COMBO_W: f32 = LARGE_W * 2.5;

            let info = layer_infos.iter().find(|l| l.name == active_layer);
            let lc = info.map(|l| l.color).unwrap_or(Color::WHITE);
            let lv = info.map(|l| l.visible).unwrap_or(true);
            let lf = info.map(|l| l.frozen).unwrap_or(false);
            let ll = info.map(|l| l.locked).unwrap_or(false);
            let is_open = open_dd.as_deref() == Some(LAYER_COMBO_ID);

            let vis_icon = icons::raw(icons::layer_visible(lv), 14.0);
            let freeze_icon = icons::raw(icons::layer_freeze(lf), 14.0);
            let lock_icon = icons::raw(icons::layer_lock(ll), 14.0);
            let swatch = container(text(""))
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(lc)),
                    border: Border {
                        color: SWATCH_BORDER,
                        width: 1.0,
                        radius: 1.0.into(),
                    },
                    ..Default::default()
                })
                .width(12)
                .height(12);

            const ICONS_USED: f32 = 14.0 + 14.0 + 14.0 + 12.0 + 10.0 + 5.0 * 4.0 + 16.0 + 16.0;
            let name_w = (COMBO_W - ICONS_USED).max(40.0);

            let combo_btn = button(
                row![
                    vis_icon,
                    freeze_icon,
                    lock_icon,
                    swatch,
                    container(text(active_layer).size(11).color(Color::WHITE))
                        .width(name_w)
                        .clip(true),
                    icons::arrow_down(9.0, COMBO_ARROW),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(LAYER_COMBO_ID.to_string()))
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match (is_open, status) {
                    (true, _) => COMBO_OPEN_BG,
                    (_, button::Status::Hovered) => COMBO_HOVER_BG,
                    _ => COMBO_BG,
                })),
                border: Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: COMBO_BORDER,
                },
                ..Default::default()
            })
            .padding([3, 8])
            .width(Fill);

            let make_tool_row = |tools: &[ToolDef]| -> Element<Message> {
                let btns: Vec<Element<Message>> = tools
                    .iter()
                    .map(|t| {
                        let is_active = active_tool.as_deref() == Some(t.id);
                        let tip = t.label;
                        let event = t.event.clone();
                        let icon_el: Element<Message> = match t.icon {
                            IconKind::Glyph(g) => text(g).size(13).color(Color::WHITE).into(),
                            IconKind::Svg(bytes) => {
                                iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes))
                                    .width(16)
                                    .height(16)
                                    .into()
                            }
                        };
                        let msg = module_event_to_message(event);
                        tooltip(
                            button(icon_el)
                                .on_press(msg)
                                .style(move |_: &Theme, status| tool_btn_style(is_active, status))
                                .padding([2, 5]),
                            make_tip(tip.to_string()),
                            TipPos::Right,
                        )
                        .gap(4.0)
                        .delay(Duration::from_millis(400))
                        .style(tip_style)
                        .into()
                    })
                    .collect();
                row(btns).spacing(2).align_y(iced::Center).into()
            };

            let tools_row2 = make_tool_row(row2);
            let tools_row3 = make_tool_row(row3);

            container(
                column![
                    PosReport::new(LAYER_COMBO_ID, combo_btn),
                    tools_row2,
                    tools_row3
                ]
                .spacing(3)
                .align_x(iced::Left),
            )
            .width(Length::Fixed(COMBO_W))
            .height(Fill)
            .align_y(iced::Center)
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 4.0,
                right: 4.0,
            })
            .into()
        }

        RibbonItem::PropertiesGroup { match_prop } => {
            // The Match Properties button is a large icon normally, but shrinks
            // to a small icon when the panel is compacted.
            let mp_el: Element<'a, Message> = if compact {
                render_small(
                    &RibbonItem::Tool(match_prop.clone()),
                    active_tool,
                    open_dd,
                    last_cmd,
                    state,
                )
            } else {
                let mp_active = is_active_tool(match_prop.id, active_tool, &state);
                let mp_event = match_prop.event.clone();
                let mp_id = match_prop.id.to_string();
                let mp_tip = format!("{}\nCommand: {}", match_prop.label, match_prop.id);
                let mp_btn = button(
                    column![
                        make_icon(match_prop.icon, LARGE_ICON),
                        text(match_prop.label).size(10).color(LABEL_COLOR),
                    ]
                    .align_x(iced::Center)
                    .spacing(3),
                )
                .on_press(Message::RibbonToolClick {
                    tool_id: mp_id,
                    event: mp_event,
                })
                .style(move |_: &Theme, status| tool_btn_style(mp_active, status))
                .width(Length::Fixed(LARGE_W))
                .height(Fill)
                .padding(Padding {
                    top: 6.0,
                    right: 4.0,
                    bottom: 4.0,
                    left: 4.0,
                });
                tooltip(mp_btn, make_tip(mp_tip), TipPos::Right)
                    .gap(6.0)
                    .delay(Duration::from_millis(400))
                    .style(tip_style)
                    .into()
            };

            const PROP_W: f32 = 130.0;

            let prop_row = |label: String, dd_id: &'static str, swatch: Option<Color>| {
                let is_open = open_dd.as_deref() == Some(dd_id);
                let swatch_el: Element<'a, Message> = if let Some(c) = swatch {
                    container(text(""))
                        .style(move |_: &Theme| container::Style {
                            background: Some(Background::Color(c)),
                            border: Border {
                                color: SWATCH_BORDER,
                                width: 1.0,
                                radius: 1.0.into(),
                            },
                            ..Default::default()
                        })
                        .width(12)
                        .height(12)
                        .into()
                } else {
                    iced::widget::Space::new().width(0).into()
                };
                button(
                    row![
                        swatch_el,
                        container(text(label).size(10).color(Color::WHITE))
                            .width(Fill)
                            .clip(true),
                        icons::arrow_toggle(
                            is_open,
                            8.0,
                            Color {
                                r: 0.6,
                                g: 0.6,
                                b: 0.6,
                                a: 1.0,
                            },
                        ),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::ToggleRibbonDropdown(dd_id.to_string()))
                .style(move |_: &Theme, status| button::Style {
                    background: Some(Background::Color(match (is_open, status) {
                        (true, _) => COMBO_OPEN_BG,
                        (_, button::Status::Hovered) => COMBO_HOVER_BG,
                        _ => COMBO_BG,
                    })),
                    border: Border {
                        radius: 2.0.into(),
                        width: 1.0,
                        color: if is_open {
                            COMBO_ACTIVE_BORDER
                        } else {
                            COMBO_BORDER
                        },
                    },
                    ..Default::default()
                })
                .padding([3, 8])
                .width(Length::Fixed(PROP_W))
            };

            let (color_swatch, _) = acad_color_display(active_color);
            let color_row = prop_row(
                crate::ui::color_select::color_display_name(active_color),
                PROP_COLOR_ID,
                Some(color_swatch),
            );
            let lt_row = prop_row(active_linetype.to_string(), PROP_LINETYPE_ID, None);
            let lw_row = prop_row(LwItem(active_lineweight).to_string(), PROP_LW_ID, None);

            let combos = container(
                column![
                    PosReport::new(PROP_COLOR_ID, color_row),
                    PosReport::new(PROP_LINETYPE_ID, lt_row),
                    PosReport::new(PROP_LW_ID, lw_row),
                ]
                .spacing(2)
                .align_x(iced::Left),
            )
            .height(Fill)
            .align_y(iced::Center)
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 0.0,
                right: 4.0,
            });

            row![mp_el, combos]
                .spacing(4)
                .align_y(iced::Center)
                .height(Fill)
                .into()
        }

        RibbonItem::StyleComboGroup {
            style_key,
            combo_id,
            rows,
            ..
        } => {
            const STYLE_COMBO_W: f32 = LARGE_W * 2.3;
            let active: String = style_ctx.active_for(*style_key).to_string();
            let is_open = open_dd.as_deref() == Some(*combo_id);

            // ── combo button ──
            let combo_btn = button(
                row![
                    container(text(active.clone()).size(11).color(Color::WHITE))
                        .width(Fill)
                        .clip(true),
                    icons::arrow_toggle(is_open, 9.0, COMBO_ARROW),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(combo_id.to_string()))
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(match (is_open, status) {
                    (true, _) => COMBO_OPEN_BG,
                    (_, button::Status::Hovered) => COMBO_HOVER_BG,
                    _ => COMBO_BG,
                })),
                border: Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: if is_open {
                        COMBO_ACTIVE_BORDER
                    } else {
                        COMBO_BORDER
                    },
                },
                ..Default::default()
            })
            .padding([3, 8])
            .width(Fill);

            // The open style list renders as a floating overlay
            // (`Ribbon::style_combo_overlay`) so it isn't clipped by the fixed
            // ribbon-row height — matching the Draw-tab dropdowns. (#153)
            let items_panel: Element<Message> =
                iced::widget::Space::new().width(0).height(0).into();

            // ── tool rows below combo ──
            let make_tool_row = |tools: &[ToolDef]| -> Element<Message> {
                let btns: Vec<Element<Message>> = tools
                    .iter()
                    .map(|t| {
                        let is_active = active_tool.as_deref() == Some(t.id);
                        let tip = t.label;
                        let event = t.event.clone();
                        let icon_el: Element<Message> = match t.icon {
                            IconKind::Glyph(g) => text(g).size(13).color(Color::WHITE).into(),
                            IconKind::Svg(bytes) => {
                                iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes))
                                    .width(16)
                                    .height(16)
                                    .into()
                            }
                        };
                        let msg = module_event_to_message(event);
                        tooltip(
                            button(icon_el)
                                .on_press(msg)
                                .style(move |_: &Theme, status| tool_btn_style(is_active, status))
                                .padding([2, 5]),
                            make_tip(tip.to_string()),
                            TipPos::Right,
                        )
                        .gap(4.0)
                        .delay(Duration::from_millis(400))
                        .style(tip_style)
                        .into()
                    })
                    .collect();
                row(btns).spacing(2).align_y(iced::Center).into()
            };

            let mut col_items: Vec<Element<Message>> =
                vec![container(row![PosReport::new(*combo_id, combo_btn), items_panel].spacing(0))
                    .width(Fill)
                    .into()];
            for row_tools in rows {
                col_items.push(make_tool_row(row_tools));
            }

            container(column(col_items).spacing(3).align_x(iced::Left))
                .width(Length::Fixed(STYLE_COMBO_W))
                .height(Fill)
                .align_y(iced::Center)
                .padding(Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 4.0,
                    right: 4.0,
                })
                .into()
        }
    }
}

// ── Message helpers ────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn module_event_to_message(event: ModuleEvent) -> Message {
    match event {
        ModuleEvent::Command(cmd) => Message::Command(cmd),
        ModuleEvent::OpenFileDialog => Message::OpenFile,
        ModuleEvent::ClearModels => Message::ClearScene,
        ModuleEvent::SetWireframe(w) => Message::SetWireframe(w),
        ModuleEvent::ToggleLayers => Message::ToggleLayers,
        // Needs the tool context + async picker — route through the normal
        // ribbon-click handler rather than a direct 1:1 message.
        e @ ModuleEvent::PluginFileDialog { .. } => Message::RibbonToolClick {
            tool_id: String::new(),
            event: e,
        },
    }
}

// ── History control ────────────────────────────────────────────────────────

/// Quick-access chrome button (New / Open / Save / Save As / Print) in the top
/// strip: an SVG icon that dispatches a command string, with a hover tooltip.
pub(super) fn quick_access_btn<'a>(
    icon_bytes: &'static [u8],
    label: &'static str,
    cmd: &'static str,
) -> Element<'a, Message> {
    // The bundled UI SVGs are black-stroked; tint them to a light chrome grey so
    // they read on the dark top strip (raw black is invisible there).
    let icon = icons::tinted(icon_bytes, 16.0, QA_ICON_COLOR);
    let btn = button(
        container(icon)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::Command(cmd.to_string()))
    .style(|_: &Theme, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => Color {
                r: 0.30,
                g: 0.30,
                b: 0.30,
                a: 1.0,
            },
            _ => Color::TRANSPARENT,
        })),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .width(Length::Fixed(TOP_HIST_W))
    .height(24)
    .padding([2, 0]);
    tooltip(btn, make_tip(label.to_string()), TipPos::Bottom)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
        .into()
}

pub(super) fn render_history_control<'a>(
    label: &'static str,
    dropdown_id: &'static str,
    count: usize,
    open_dropdown: &Option<String>,
) -> Element<'a, Message> {
    let dd_open = open_dropdown.as_deref() == Some(dropdown_id);
    let active = count > 0;
    let icon_color = if active { Color::WHITE } else { LABEL_OFF };

    let main_btn = {
        let glyph = if dropdown_id == UNDO_HISTORY_ID {
            icons::undo(15.0, icon_color)
        } else {
            icons::redo(15.0, icon_color)
        };
        let btn = button(
            container(glyph)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .style(move |_: &Theme, status| top_hist_btn_style(active, dd_open, status))
        .width(Length::Fixed(TOP_HIST_W))
        .height(24)
        .padding([2, 0]);
        let btn = if active {
            if dropdown_id == UNDO_HISTORY_ID {
                btn.on_press(Message::Undo)
            } else {
                btn.on_press(Message::Redo)
            }
        } else {
            btn
        };
        tooltip(
            btn,
            make_tip(format!("{label}\n{count} steps available")),
            TipPos::Right,
        )
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
    };

    let arrow_btn = {
        let btn = button(
            container(icons::arrow_down(
                8.0,
                if active { ARROW_COLOR } else { LABEL_OFF },
            ))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
        )
        .style(move |_: &Theme, status| top_hist_btn_style(active, dd_open, status))
        .width(Length::Fixed(TOP_ARR_W))
        .height(24)
        .padding(0);
        let btn = if active {
            btn.on_press(Message::ToggleRibbonDropdown(dropdown_id.to_string()))
        } else {
            btn
        };
        tooltip(
            btn,
            make_tip(format!("Choose {label} history")),
            TipPos::Right,
        )
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
    };

    PosReport::new(dropdown_id, row![main_btn, arrow_btn].spacing(0)).into()
}

pub(super) fn top_hist_btn_style(
    active: bool,
    open: bool,
    status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(match (active, open, status) {
            (false, _, _) => HIST_INACTIVE_BG,
            (_, true, _) => TOOL_ACTIVE,
            (_, _, button::Status::Hovered) => TOOL_HOVER,
            (_, _, button::Status::Pressed) => TOOL_ACTIVE,
            _ => Color::TRANSPARENT,
        })),
        text_color: Color::WHITE,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}
