//! Table Style Manager window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Column,
};
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

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

pub fn view_window<'a>(
    styles: Vec<String>,
    selected: &'a str,
    current: &'a str,
    selected_style: Option<&'a acadrust::objects::TableStyle>,
    hmargin_buf: &'a str,
    vmargin_buf: &'a str,
    description_buf: &'a str,
    cell_textstyle: &'a [String; 3],
    cell_height: &'a [String; 3],
    cell_textcolor: &'a [String; 3],
    cell_fillcolor: &'a [String; 3],
    cell_datatype: &'a [String; 3],
    cell_unittype: &'a [String; 3],
    cell_format: &'a [String; 3],
    border_lw: &'a [[String; 6]; 3],
    border_color: &'a [[String; 6]; 3],
    border_spacing: &'a [[String; 6]; 3],
    rename_active: Option<&'a str>,
    rename_buf: &'a str,
    color_open: Option<(u8, &'static str)>,
) -> Element<'a, Message> {
    // ── Right: Details panel ──────────────────────────────────────────────
    let info_row = |label: &'static str, val: String| -> Element<'_, Message> {
        row![
            text(label).size(11).style(muted_style).width(160),
            text(val).size(11),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    let cell_editor = |row_label: &'static str,
                       row: u8,
                       rs: &acadrust::objects::RowCellStyle|
     -> Element<'a, Message> {
        let r = row as usize;
        let cell_in = |label: &'static str,
                       placeholder: &'static str,
                       value: &'a str,
                       field: &'static str|
         -> Element<'a, Message> {
            row![
                text(label).size(11).style(muted_style).width(150),
                text_input(placeholder, value)
                    .on_input(move |v| Message::TableStyleCellEdit {
                        row,
                        field,
                        value: v
                    })
                    .size(11)
                    .width(100),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into()
        };
        // Shared colour selector — sends the chosen colour through the same
        // cell edit as an ACI string.
        let cell_color = |label: &'static str, value: &'a str, field: &'static str| -> Element<'a, Message> {
            let cur = crate::ui::color_select::aci_string_to_color(value);
            let open = color_open == Some((row, field));
            let selector = crate::ui::color_select::color_selector(
                cur,
                open,
                crate::ui::color_select::ColorExtras {
                    by_layer: true,
                    by_block: true,
                },
                move |c| Message::TableStyleCellEdit {
                    row,
                    field,
                    value: crate::ui::color_select::color_to_aci_string(c),
                },
                Message::TableColorMore(row, field),
                Message::OpenColorWindow(crate::app::ColorPickTarget::Table(row, field)),
            );
            row![text(label).size(11).style(muted_style).width(150), selector]
                .spacing(8)
                .align_y(iced::Center)
                .into()
        };
        let mut col = Column::new()
            .spacing(3)
            .push(text(row_label).size(11).style(primary_style))
            .push(cell_in(
                "  Text style:",
                "Standard",
                &cell_textstyle[r],
                "textstyle",
            ))
            .push(cell_in("  Text height:", "0.18", &cell_height[r], "height"))
            .push(cell_color("  Text color:", &cell_textcolor[r], "textcolor"))
            .push(cell_color("  Fill color:", &cell_fillcolor[r], "fillcolor"))
            .push(
                row![
                    text("  Alignment:").size(11).style(muted_style).width(150),
                    pick_list(
                        [
                            "TopLeft",
                            "TopCenter",
                            "TopRight",
                            "MiddleLeft",
                            "MiddleCenter",
                            "MiddleRight",
                            "BottomLeft",
                            "BottomCenter",
                            "BottomRight",
                        ]
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                        Some(format!("{:?}", rs.alignment)),
                        move |value| Message::TableStyleCellSetAlign { row, value },
                    )
                    .text_size(11)
                    .width(140),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .push(
                checkbox(rs.fill_enabled)
                    .label("  Background fill enabled")
                    .on_toggle(move |_| Message::TableStyleCellToggleFill(row))
                    .size(14)
                    .text_size(11),
            )
            .push(cell_in("  Data type:", "0", &cell_datatype[r], "datatype"))
            .push(cell_in("  Unit type:", "0", &cell_unittype[r], "unittype"))
            .push(cell_in("  Format string:", "", &cell_format[r], "format"))
            .push(
                text("  Borders  (type / weight / color / spacing / hidden)")
                    .size(10)
                    .style(muted_style),
            );

        let borders: [(&'static str, &acadrust::objects::TableCellBorder); 6] = [
            ("L", &rs.left_border),
            ("R", &rs.right_border),
            ("T", &rs.top_border),
            ("B", &rs.bottom_border),
            ("H", &rs.horizontal_inside_border),
            ("V", &rs.vertical_inside_border),
        ];
        for (b, (bname, bd)) in borders.into_iter().enumerate() {
            let bu = b as u8;
            col = col.push(
                row![
                    text(format!("   {bname}")).size(11).style(muted_style).width(28),
                    pick_list(
                        ["Single", "Double"]
                            .iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>(),
                        Some(format!("{:?}", bd.border_type)),
                        move |value| Message::TableStyleBorderSetType {
                            cell: row,
                            border: bu,
                            value
                        },
                    )
                    .text_size(10)
                    .width(74),
                    text_input("wt", &border_lw[r][b])
                        .on_input(move |v| Message::TableStyleBorderEdit {
                            cell: row,
                            border: bu,
                            field: "lw",
                            value: v
                        })
                        .size(10)
                        .width(46),
                    text_input("clr", &border_color[r][b])
                        .on_input(move |v| Message::TableStyleBorderEdit {
                            cell: row,
                            border: bu,
                            field: "color",
                            value: v
                        })
                        .size(10)
                        .width(46),
                    text_input("gap", &border_spacing[r][b])
                        .on_input(move |v| Message::TableStyleBorderEdit {
                            cell: row,
                            border: bu,
                            field: "spacing",
                            value: v
                        })
                        .size(10)
                        .width(46),
                    checkbox(bd.is_invisible)
                        .on_toggle(move |_| Message::TableStyleBorderToggleInvisible {
                            cell: row,
                            border: bu
                        })
                        .size(13),
                ]
                .spacing(5)
                .align_y(iced::Center),
            );
        }

        col.push(
            button(text("Apply cell").size(11))
                .on_press(Message::TableStyleCellApply(row))
                .style(btn_s(true))
                .padding([4, 12]),
        )
        .into()
    };

    let details: Element<'_, Message> = if let Some(s) = selected_style {
        scrollable(
            column![
                info_row("Name:", s.name.clone()),
                row![
                    text("Description:").size(11).style(muted_style).width(160),
                    text_input("", description_buf)
                        .on_input(|v| Message::TableStyleEdit {
                            field: "description",
                            value: v
                        })
                        .size(11)
                        .width(160),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    text("Flow direction:").size(11).style(muted_style).width(160),
                    pick_list(
                        ["Down", "Up"]
                            .iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>(),
                        Some(format!("{:?}", s.flow_direction)),
                        Message::TableStyleSetFlow,
                    )
                    .text_size(11)
                    .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                checkbox(s.annotative)
                    .label("Annotative")
                    .on_toggle(|_| Message::TableStyleToggleAnnotative)
                    .size(14)
                    .text_size(11),
                row![
                    text("H Margin:").size(11).style(muted_style).width(160),
                    text_input("1.5", hmargin_buf)
                        .on_input(|v| Message::TableStyleEdit {
                            field: "hmargin",
                            value: v
                        })
                        .size(11)
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    text("V Margin:").size(11).style(muted_style).width(160),
                    text_input("1.5", vmargin_buf)
                        .on_input(|v| Message::TableStyleEdit {
                            field: "vmargin",
                            value: v
                        })
                        .size(11)
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                checkbox(s.title_suppressed)
                    .label("Title row suppressed")
                    .on_toggle(|_| Message::TableStyleToggle("title_sup"))
                    .size(14)
                    .text_size(11),
                checkbox(s.header_suppressed)
                    .label("Header row suppressed")
                    .on_toggle(|_| Message::TableStyleToggle("header_sup"))
                    .size(14)
                    .text_size(11),
                button(text("Apply margins").size(11))
                    .on_press(Message::TableStyleApply)
                    .style(btn_s(true))
                    .padding([4, 12]),
                cell_editor("Data Row:", 0, &s.data_row_style),
                cell_editor("Header Row:", 1, &s.header_row_style),
                cell_editor("Title Row:", 2, &s.title_row_style),
            ]
            .spacing(6)
            .padding([12, 12]),
        )
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        container(text("Select a style to view details.").size(11).style(muted_style))
            .padding([12, 12])
            .into()
    };

    let right_panel = container(details).width(Fill).height(Fill);

    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        kind: crate::app::StyleKind::Table,
        styles: &styles,
        selected,
        current: Some(current),
        rename_active,
        rename_buf,
        on_new: Message::TableStyleDialogNew,
        on_copy: Message::TableStyleDialogCopy,
        on_delete: Message::TableStyleDialogDelete,
        on_select: Message::TableStyleDialogSelect,
        on_set_current: Message::TableStyleDialogSetCurrent,
        on_apply: Message::TableStyleApply,
        editor: right_panel.into(),
    })
}
