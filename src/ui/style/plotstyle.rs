//! Plot Style Table Editor window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Theme};

fn btn_s(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
        let pair = match (accent, st) {
            (true, button::Status::Hovered | button::Status::Pressed) => palette.primary.strong,
            (false, button::Status::Hovered | button::Status::Pressed) => {
                palette.background.strong
            }
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
        }
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    table: Option<&'a crate::io::plot_style::PlotStyleTable>,
    selected_aci: u8,
    color_buf: &'a str,
    lw_buf: &'a str,
    screen_buf: &'a str,
) -> Element<'a, Message> {
    let table_name = table
        .map(|t| t.name.as_str())
        .unwrap_or("(no table loaded)");

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            button(text("Load CTB/STB").size(11))
                .on_press(Message::PlotStyleLoad)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text("Save As…").size(11))
                .on_press(Message::PlotStylePanelSave)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text("Clear Table").size(11))
                .on_press(Message::PlotStyleClear)
                .style(btn_s(false))
                .padding([4, 10]),
            Space::new().width(Fill),
            text(table_name).size(10).style(muted_style),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weak.color
        )),
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 8]);

    // ── Left: ACI list ────────────────────────────────────────────────────
    let aci_items: Vec<Element<'_, Message>> = (1u8..=255)
        .map(|aci| {
            let is_sel = aci == selected_aci;
            let has_override = table
                .and_then(|t| t.aci_entries.get(aci as usize))
                .map(|e| e.color.is_some() || e.lineweight != 255 || e.screening != 100)
                .unwrap_or(false);
            let lw_str = table
                .and_then(|t| t.aci_entries.get(aci as usize))
                .and_then(|e| {
                    if e.lineweight != 255 {
                        crate::io::plot_style::LW_TABLE
                            .get(e.lineweight as usize)
                            .map(|lw| format!("{lw:.2}mm"))
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let color_str = table
                .and_then(|t| t.aci_entries.get(aci as usize))
                .and_then(|e| e.color.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}")))
                .unwrap_or_default();
            let label = if has_override {
                format!("{aci:>3}  {color_str:<9} {lw_str}")
            } else {
                format!("{aci:>3}  (default)")
            };
            button(text(label).size(10).font(iced::Font::MONOSPACE))
                .on_press(Message::PlotStylePanelSelectAci(aci))
                .style(move |theme: &Theme, st| {
                    let palette = theme.extended_palette();
                    let pair = match (is_sel, st) {
                        (true, _) => Some(palette.primary.strong),
                        (false, button::Status::Hovered | button::Status::Pressed) => {
                            Some(palette.background.strong)
                        }
                        _ => None,
                    };
                    button::Style {
                    background: pair.map(|p| Background::Color(p.color)),
                    text_color: pair
                        .map(|p| p.text)
                        .unwrap_or(palette.background.base.text),
                    ..Default::default()
                    }
                })
                .padding([2, 8])
                .width(Fill)
                .into()
        })
        .collect();

    let aci_list = container(
        column![
            text("ACI Color Index").size(10).style(muted_style),
            container(scrollable(column(aci_items).spacing(1)).height(Fill))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(280)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: Edit panel ─────────────────────────────────────────────────
    let entry = table.and_then(|t| t.aci_entries.get(selected_aci as usize));
    let cur_color = entry
        .and_then(|e| e.color.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}")))
        .unwrap_or_else(|| "(none)".into());
    let cur_lw = entry
        .map(|e| {
            if e.lineweight == 255 {
                "object".into()
            } else {
                crate::io::plot_style::LW_TABLE
                    .get(e.lineweight as usize)
                    .map(|lw| format!("{lw:.2}mm (idx {})", e.lineweight))
                    .unwrap_or_else(|| format!("idx {}", e.lineweight))
            }
        })
        .unwrap_or_else(|| "—".into());
    let cur_scr = entry
        .map(|e| format!("{}%", e.screening))
        .unwrap_or_else(|| "—".into());

    let lbl = |s: &'static str| text(s).size(11).style(muted_style);

    let edit_panel = container(
        column![
            row![
                text("ACI:").size(11).style(muted_style).width(100),
                text(format!("{selected_aci}")).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
            lbl("Color override (#RRGGBB):"),
            text_input("#RRGGBB or blank", color_buf)
                .on_input(Message::PlotStylePanelColorBuf)
                .style(field_style)
                .size(11)
                .padding([4, 8]),
            lbl("Lineweight index (0-24, 255=obj):"),
            text_input("255", lw_buf)
                .on_input(Message::PlotStylePanelLwBuf)
                .style(field_style)
                .size(11)
                .padding([4, 8]),
            lbl("Screening (0-100):"),
            text_input("100", screen_buf)
                .on_input(Message::PlotStylePanelScreenBuf)
                .style(field_style)
                .size(11)
                .padding([4, 8]),
            Space::new().height(8),
            text("Current values:").size(10).style(muted_style),
            text(format!("  Color: {cur_color}")).size(10),
            text(format!("  Lineweight: {cur_lw}")).size(10),
            text(format!("  Screening: {cur_scr}")).size(10),
            Space::new().height(Fill),
            button(text("Apply to ACI").size(11))
                .on_press(Message::PlotStylePanelApply)
                .style(btn_s(true))
                .padding([5, 10]),
        ]
        .spacing(8)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding([12, 12]);

    let body = row![aci_list, vsep(), edit_panel].height(Fill);

    container(column![toolbar, hdivider(), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
