// Shared rendering helpers, button styles, colours, layout constants, and
// free functions used by the Ribbon view/overlay methods.

use rustc_hash::FxHashMap as HashMap;
use std::time::Duration;

use acadrust::types::{Color as AcadColor, LineWeight};
// Ribbon tooltips anchor to the right of their button so the cursor — which
// rests on the button itself — never covers the tip text. (#143)
use iced::widget::tooltip::Position as TipPos;
use iced::widget::{button, column, container, row, text, tooltip};
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
    /// Start (welcome) tab is active — tools whose command the start-tab
    /// gate refuses render dimmed and read as unusable.
    pub start_mode: bool,
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
        IconKind::Glyph(s) => text(s).size(size * 0.7).into(),
        IconKind::Svg(bytes) => icons::semantic(bytes, size),
    }
}

/// Unusable on the Start tab: dim it. Mirrors the dispatch gate — the single
/// authority is `crate::app::commands::start_allowed`.
pub(super) fn start_dimmed(state: &ToggleState, event: &ModuleEvent) -> bool {
    state.start_mode
        && !matches!(event, ModuleEvent::Command(c) if crate::app::commands::start_allowed(c))
}

/// `make_icon`, faded when `dim` without flattening multi-colour SVGs.
pub(super) fn make_icon_dim(icon: IconKind, size: f32, dim: bool) -> Element<'static, Message> {
    if !dim {
        return make_icon(icon, size);
    }
    match icon {
        IconKind::Glyph(s) => text(s)
            .size(size * 0.7)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.base.text.scale_alpha(0.42)),
            })
            .into(),
        IconKind::Svg(bytes) => icons::semantic_disabled(bytes, size),
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

pub(super) fn tool_btn_style(
    theme: &Theme,
    is_active: bool,
    status: button::Status,
) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match (is_active, status) {
        (true, _) => palette.primary.weak,
        (_, button::Status::Hovered) => palette.background.weak,
        (_, button::Status::Pressed) => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: is_active
            .then_some(Background::Color(pair.color))
            .or_else(|| matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(Background::Color(pair.color))),
        text_color: pair.text,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

pub(super) fn combo_btn_style(
    theme: &Theme,
    is_open: bool,
    status: button::Status,
    radius: f32,
) -> button::Style {
    let palette = theme.extended_palette();
    let pair = if is_open {
        palette.primary.weak
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        palette.background.weak
    } else {
        palette.background.weakest
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: radius.into(),
            width: 1.0,
            color: if is_open {
                palette.primary.base.color
            } else {
                palette.background.neutral.color
            },
        },
        ..Default::default()
    }
}

pub(super) fn popup_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        palette.background.weak
    } else {
        palette.background.base
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}

pub(super) fn popup_panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.72)),
    }
}

pub(super) fn tool_label_style(theme: &Theme, dim: bool) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: dim.then_some(
            theme.extended_palette().background.base.text.scale_alpha(0.42),
        ),
    }
}

// ── Tooltip helpers ────────────────────────────────────────────────────────

pub(super) fn make_tip(tip: String) -> Element<'static, Message> {
    text(tip).size(11).into()
}

pub(super) fn tip_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: Some(palette.background.strong.text),
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
            let dim = start_dimmed(&state, &t.event);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let tip_text = format!("{}\nCommand: {}", t.label, t.id);
            let btn = button(make_icon_dim(t.icon, SMALL_ICON, dim))
                .on_press(Message::RibbonToolClick { tool_id, event })
                .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
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
            let dim = state.start_mode
                && !items
                    .iter()
                    .any(|(cmd, _, _)| crate::app::commands::start_allowed(cmd));
            let active = !dim
                && (active_tool.as_deref() == Some(*id)
                    || items
                        .iter()
                        .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd)));
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

            let icon_btn = button(make_icon_dim(cur_icon, SMALL_ICON, dim))
                .on_press(Message::RibbonToolClick {
                    tool_id: last.to_string(),
                    event: ModuleEvent::Command(last.to_string()),
                })
                .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
                .width(Length::Fixed(SMALL_W))
                .height(ROW_H)
                .padding([4, 4]);

            let arr_tip = format!("{} options", cur_label);
            let arr_btn = button(
                container(icons::themed_arrow_down(8.0))
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(id.to_string()))
            .style(move |theme: &Theme, status| {
                tool_btn_style(theme, dd_open, status)
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
    dim: bool,
) -> Element<'a, Message> {
    let active = !dim
        && (active_tool.as_deref() == Some(id)
            || items
                .iter()
                .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd)));
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
            make_icon_dim(cur_icon, LARGE_ICON, dim),
            text(label.to_string())
                .size(10)
                .style(move |theme: &Theme| tool_label_style(theme, dim)),
        ]
        .align_x(iced::Center)
        .spacing(3),
    )
    .on_press(Message::RibbonToolClick {
        tool_id: last.to_string(),
        event: ModuleEvent::Command(last.to_string()),
    })
    .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
    .width(Length::Fixed(LARGE_W))
    .height(Fill)
    .padding(Padding {
        top: 6.0,
        right: 4.0,
        bottom: 2.0,
        left: 4.0,
    });

    let arr_btn = button(
        container(icons::themed_arrow_down(9.0))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::ToggleRibbonDropdown(id.to_string()))
    .style(move |theme: &Theme, status| {
        tool_btn_style(theme, dd_open, status)
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
            let dim = start_dimmed(&state, &t.event);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let tip_text = format!("{}\nCommand: {}", t.label, t.id);
            let btn = button(
                column![
                    make_icon_dim(t.icon, LARGE_ICON, dim),
                    text(t.label)
                        .size(10)
                        .style(move |theme: &Theme| tool_label_style(theme, dim)),
                ]
                .align_x(iced::Center)
                .spacing(3),
            )
            .on_press(Message::RibbonToolClick { tool_id, event })
            .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
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
        } => {
                let dim = state.start_mode
                    && !items
                        .iter()
                        .any(|(cmd, _, _)| crate::app::commands::start_allowed(cmd));
                render_large_dropdown(
                    *id,
                    *icon,
                    Some(*label),
                    items,
                    *default,
                    active_tool,
                    open_dd,
                    last_cmd,
                    dim,
                )
            }

        // A plain Dropdown renders large too (used by a collapsed panel whose
        // representative tool is a dropdown).
        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
        } => {
                let dim = state.start_mode
                    && !items
                        .iter()
                        .any(|(cmd, _, _)| crate::app::commands::start_allowed(cmd));
                render_large_dropdown(
                    *id, *icon, None, items, *default, active_tool, open_dd, last_cmd,
                    dim,
                )
            }

        RibbonItem::LayerComboGroup { row2, row3 } => {
            const COMBO_W: f32 = LARGE_W * 2.5;

            let info = layer_infos.iter().find(|l| l.name == active_layer);
            let lc = info.map(|l| l.color).unwrap_or(Color::WHITE);
            let lv = info.map(|l| l.visible).unwrap_or(true);
            let lf = info.map(|l| l.frozen).unwrap_or(false);
            let ll = info.map(|l| l.locked).unwrap_or(false);
            let is_open = open_dd.as_deref() == Some(LAYER_COMBO_ID);

            let vis_icon = icons::semantic(icons::layer_visible(lv), 14.0);
            let freeze_icon = icons::semantic(icons::layer_freeze(lf), 14.0);
            let lock_icon = icons::semantic(icons::layer_lock(ll), 14.0);
            let swatch = container(text(""))
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(lc)),
                    border: Border {
                        color: theme.extended_palette().background.strong.color,
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
                    container(text(active_layer).size(11))
                        .width(name_w)
                        .clip(true),
                    icons::themed_arrow_down(9.0),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(LAYER_COMBO_ID.to_string()))
            .style(move |theme: &Theme, status| {
                combo_btn_style(theme, is_open, status, 3.0)
            })
            .padding([3, 8])
            .width(Fill);

            let make_tool_row = |tools: &[ToolDef]| -> Element<Message> {
                let btns: Vec<Element<Message>> = tools
                    .iter()
                    .map(|t| {
                        let is_active = active_tool.as_deref() == Some(t.id);
                        let dim = start_dimmed(&state, &t.event);
                        let tip = t.label;
                        let event = t.event.clone();
                        let icon_el: Element<Message> = if dim {
                            make_icon_dim(t.icon, 16.0, true)
                        } else {
                            make_icon(t.icon, 16.0)
                        };
                        let msg = module_event_to_message(event);
                        tooltip(
                            button(icon_el)
                                .on_press(msg)
                                .style(move |theme: &Theme, status| {
                                    tool_btn_style(theme, is_active, status)
                                })
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
                let mp_dim = start_dimmed(&state, &match_prop.event);
                let mp_event = match_prop.event.clone();
                let mp_id = match_prop.id.to_string();
                let mp_tip = format!("{}\nCommand: {}", match_prop.label, match_prop.id);
                let mp_btn = button(
                    column![
                        make_icon_dim(match_prop.icon, LARGE_ICON, mp_dim),
                        text(match_prop.label)
                            .size(10)
                            .style(move |theme: &Theme| tool_label_style(theme, mp_dim)),
                    ]
                    .align_x(iced::Center)
                    .spacing(3),
                )
                .on_press(Message::RibbonToolClick {
                    tool_id: mp_id,
                    event: mp_event,
                })
                .style(move |theme: &Theme, status| tool_btn_style(theme, mp_active, status))
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
                        .style(move |theme: &Theme| container::Style {
                            background: Some(Background::Color(c)),
                            border: Border {
                                color: theme.extended_palette().background.strong.color,
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
                        container(text(label).size(10))
                            .width(Fill)
                            .clip(true),
                        if is_open {
                            icons::themed_arrow_up(8.0)
                        } else {
                            icons::themed_arrow_down(8.0)
                        },
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::ToggleRibbonDropdown(dd_id.to_string()))
                .style(move |theme: &Theme, status| {
                    combo_btn_style(theme, is_open, status, 2.0)
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
                    container(text(active.clone()).size(11))
                        .width(Fill)
                        .clip(true),
                    if is_open {
                        icons::themed_arrow_up(9.0)
                    } else {
                        icons::themed_arrow_down(9.0)
                    },
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(combo_id.to_string()))
            .style(move |theme: &Theme, status| {
                combo_btn_style(theme, is_open, status, 3.0)
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
                        let dim = start_dimmed(&state, &t.event);
                        let tip = t.label;
                        let event = t.event.clone();
                        let icon_el: Element<Message> = if dim {
                            make_icon_dim(t.icon, 16.0, true)
                        } else {
                            make_icon(t.icon, 16.0)
                        };
                        let msg = module_event_to_message(event);
                        tooltip(
                            button(icon_el)
                                .on_press(msg)
                                .style(move |theme: &Theme, status| {
                                    tool_btn_style(theme, is_active, status)
                                })
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
    is_start: bool,
) -> Element<'a, Message> {
    // The bundled UI SVGs are black-stroked; tint them to a light chrome grey so
    // they read on the dark top strip (raw black is invisible there).
    // On the Start tab, commands the start gate refuses render dimmed.
    let dim = is_start && !crate::app::commands::start_allowed(cmd);
    let icon = if dim {
        icons::themed_disabled(icon_bytes, 16.0)
    } else {
        icons::themed(icon_bytes, 16.0)
    };
    let btn = button(
        container(icon)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::Command(cmd.to_string()))
    .style(button::subtle)
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

    let main_btn = {
        let glyph = if dropdown_id == UNDO_HISTORY_ID {
            icons::themed_undo(15.0, active)
        } else {
            icons::themed_redo(15.0, active)
        };
        let btn = button(
            container(glyph)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| {
            top_hist_btn_style(theme, active, dd_open, status)
        })
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
            container(if active {
                icons::themed_arrow_down(8.0)
            } else {
                icons::themed_disabled_arrow_down(8.0)
            })
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| {
            top_hist_btn_style(theme, active, dd_open, status)
        })
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
    theme: &Theme,
    active: bool,
    open: bool,
    status: button::Status,
) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match (active, open, status) {
        (false, _, _) => palette.background.weakest,
        (_, true, _) => palette.primary.weak,
        (_, _, button::Status::Hovered) => palette.background.weak,
        (_, _, button::Status::Pressed) => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: (!active || open || matches!(
            status,
            button::Status::Hovered | button::Status::Pressed
        ))
        .then_some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}
