//! Plot Style Table Editor window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Theme};
use crate::t;
use std::borrow::Cow;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
struct PlotLineweightItem {
    index: u8,
    label: String,
}

impl fmt::Display for PlotLineweightItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn plot_lineweight_options() -> Vec<PlotLineweightItem> {
    let mut options = vec![PlotLineweightItem {
        index: 255,
        label: t!("Use object lineweight").into_owned(),
    }];

    options.extend(
        crate::io::plot_style::LW_TABLE
            .iter()
            .enumerate()
            .map(|(index, lw)| PlotLineweightItem {
                index: index as u8,
                label: format!("{lw:.2} mm"),
            }),
    );

    options
}

fn btn_s(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.palette();
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
    let palette = theme.palette();
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
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn hdivider<'a>(width: iced::Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

fn vsep<'a>(height: iced::Length) -> Element<'a, Message> {
    container(Space::new().width(1).height(height))
        .width(1)
        .height(height)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    document: &'a acadrust::CadDocument,
    table: Option<&'a crate::io::plot_style::PlotStyleTable>,
    selected_aci: u8,
    color_buf: &'a str,
    lw_buf: &'a str,
    screen_buf: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let show_layer_usage = !table.is_some_and(|table| table.is_stb);

let mut layer_usage = vec![Vec::<String>::new(); 256];

    if show_layer_usage {
        for layer in document.layers.iter() {
            if let acadrust::types::Color::Index(aci) = &layer.color {
                let index = *aci as usize;

                if index > 0 && index < layer_usage.len() {
                    layer_usage[index].push(layer.name.clone());
                }
            }
        }

        for layers in &mut layer_usage {
            layers.sort_by_key(|name| name.to_ascii_lowercase());
        }
    }
    let table_name = table
        .map(|t| t.name.clone())
        .unwrap_or_else(|| t!("(no table loaded)").into_owned());

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            button(text(t!("Load CTB/STB")).size(11))
                .on_press(Message::PlotStyleLoad)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text(t!("Save")).size(11))
                .on_press(Message::PlotStylePanelSaveDirect)
                .style(btn_s(true))
                .padding([4, 14]),
            button(text(t!("Save As…")).size(11))
                .on_press(Message::PlotStylePanelSave)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text(t!("Clear Table")).size(11))
                .on_press(Message::PlotStyleClear)
                .style(btn_s(false))
                .padding([4, 10]),
            Space::new().width(sizing.width),
            text(table_name).size(10).style(muted_style),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.weak.color
        )),
        ..Default::default()
    })
    .width(sizing.width)
    .padding([5, 8]);

    // ── Left: ACI list ────────────────────────────────────────────────────
    let aci_items: Vec<Element<'_, Message>> = (1u8..=255)
        .map(|aci| {
            let is_sel = aci == selected_aci;
            let usage_count = layer_usage
                .get(aci as usize)
                .map(Vec::len)
                .unwrap_or(0);

            let usage_label = if show_layer_usage && usage_count > 0 {
                format!("● {usage_count}")
            } else {
                String::new()
            };
            let (aci_color, _) = crate::ui::properties::acad_color_display(
                acadrust::types::Color::Index(aci),
            );
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
                format!("{aci:>3}  {}", t!("(default)"))
            };
            button(
                row![
                    crate::ui::color_select::swatch(aci_color),

                    text(label)
                        .size(10)
                        .font(iced::Font::MONOSPACE)
                        .width(iced::Length::Fill),

                    text(usage_label)
                        .size(10)
                        .font(iced::Font::MONOSPACE)
                        .width(42),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(Message::PlotStylePanelSelectAci(aci))
                .style(move |theme: &Theme, st| {
                    let palette = theme.palette();
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
                .width(sizing.width)
                .into()
        })
        .collect();

    let aci_list = container(
        column![
            row![
                text(t!("ACI Color Index"))
                    .size(10)
                    .style(muted_style)
                    .width(iced::Length::Fill),

                text(t!("Layers"))
                    .size(10)
                    .style(muted_style)
                    .width(42),
            ]
            .align_y(iced::Center),
            container(scrollable(column(aci_items).spacing(1)).height(sizing.height))
                .style(|theme: &Theme| {
                    let palette = theme.palette();
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
                .width(sizing.width)
                .height(sizing.height)
                .padding(2),
        ]
        .spacing(4)
        .height(sizing.height),
    )
    .width(280)
    .height(sizing.height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: Edit panel ─────────────────────────────────────────────────
    let selected_layers = layer_usage
        .get(selected_aci as usize)
        .cloned()
        .unwrap_or_default();

    let selected_layers_text = if selected_layers.is_empty() {
        t!("(none)").into_owned()
    } else {
        selected_layers.join(", ")
    };
    let entry = table.and_then(|t| t.aci_entries.get(selected_aci as usize));
    let cur_color = entry
        .and_then(|e| e.color.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}")))
        .unwrap_or_else(|| t!("(none)").into());
    let cur_lw = entry
        .map(|e| {
            if e.lineweight == 255 {
                t!("object").into()
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

    let lbl = |s: Cow<'static, str>| text(s).size(11).style(muted_style);
    let lw_items = plot_lineweight_options();

    let current_lw_index = lw_buf
        .trim()
        .parse::<u8>()
        .unwrap_or(255);

    let current_lw = lw_items
        .iter()
        .find(|item| item.index == current_lw_index)
        .cloned()
        .unwrap_or_else(|| lw_items[0].clone());

    let edit_panel = container(
        column![
            row![
                text(t!("ACI:"))
                    .size(11)
                    .style(muted_style)
                    .width(100),
                text(format!("{selected_aci}")).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
            if show_layer_usage {
                container(
                    column![
                        text(format!(
                            "{}: {}",
                            t!("Layers"),
                            selected_layers.len()
                        ))
                        .size(10)
                        .style(muted_style),

                        text(selected_layers_text)
                            .size(10)
                            .width(iced::Length::Fill),
                    ]
                    .spacing(3),
                )
                .padding([5, 8])
                .width(iced::Length::Fill)
            } else {
                container(Space::new().height(0))
            },

            // ── Plot color ───────────────────────────────────────────────────
            lbl(t!("Plot color:")),
            {
                let selected_color = color_buf
                    .strip_prefix('#')
                    .filter(|hex| hex.len() == 6)
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .map(|rgb| acadrust::types::Color::Rgb {
                        r: ((rgb >> 16) & 0xFF) as u8,
                        g: ((rgb >> 8) & 0xFF) as u8,
                        b: (rgb & 0xFF) as u8,
                    });

                let display_color = selected_color
                    .unwrap_or(acadrust::types::Color::Index(selected_aci));

                let (swatch_color, _) =
                    crate::ui::properties::acad_color_display(display_color);

                let display_text = if color_buf.is_empty() {
                    t!("Use object color").into_owned()
                } else {
                    crate::ui::color_select::color_display_name(display_color)
                };

                row![
                    container(
                        row![
                            crate::ui::color_select::swatch(swatch_color),
                            text(display_text).size(11),
                        ]
                        .spacing(6)
                        .align_y(iced::Center)
                    )
                    .padding([4, 8])
                    .width(iced::Length::Fill),

                    button(text(t!("Choose color…")).size(11))
                        .on_press(Message::OpenColorWindow(
                            crate::app::ColorPickTarget::PlotStyle,
                            display_color,
                        ))
                        .style(btn_s(false))
                        .padding([4, 10]),

                    button(text(t!("Reset")).size(11))
                        .on_press(Message::PlotStylePanelColorBuf(String::new()))
                        .style(btn_s(false))
                        .padding([4, 10]),
                ]
                .spacing(6)
                .align_y(iced::Center)
            },

            // ── Lineweight ───────────────────────────────────────────────────
            lbl(t!("Lineweight:")),
                iced::widget::pick_list(
                    Some(current_lw),
                    lw_items,
                    |item| item.to_string(),
                )
                .on_select(|item: PlotLineweightItem| {
                    Message::PlotStylePanelLwSet(item.index)
                })
                .text_size(11)
                .padding([3, 5])
                .width(iced::Length::Fill),

            // ── Screening ────────────────────────────────────────────────────
            lbl(t!("Screening (0-100):")),
            text_input("100", screen_buf)
                .on_input(Message::PlotStylePanelScreenBuf)
                .style(field_style)
                .size(11)
                .padding([4, 8]),

            Space::new().height(8),

            // ── Current values ───────────────────────────────────────────────
            text(t!("Current values:"))
                .size(10)
                .style(muted_style),

            text(t!(
                "  Color: %{cur_color}",
                cur_color = cur_color
            ))
            .size(10),

            text(t!(
                "  Lineweight: %{cur_lw}",
                cur_lw = cur_lw
            ))
            .size(10),

            text(t!(
                "  Screening: %{cur_scr}",
                cur_scr = cur_scr
            ))
            .size(10),

            Space::new().height(12),

            hdivider(iced::Length::Fill),

            Space::new().height(8),

            if show_layer_usage {
                container(
                    column![
                        row![
                            text(format!("Layers using ACI {selected_aci}"))
                                .size(11),

                            Space::new().width(iced::Length::Fill),

                            text(format!(
                                "{} {}",
                                selected_layers.len(),
                                if selected_layers.len() == 1 {
                                    "layer"
                                } else {
                                    "layers"
                                }
                            ))
                            .size(10)
                            .style(muted_style),
                        ]
                        .align_y(iced::Center),

                        if selected_layers.is_empty() {
                            column![
                                text("No layers use this ACI in the current drawing.")
                                    .size(10)
                                    .style(muted_style)
                            ]
                        } else {
                            column(
                                selected_layers
                                    .iter()
                                    .map(|name| {
                                        text(format!("• {name}"))
                                            .size(10)
                                            .into()
                                    })
                                    .collect::<Vec<Element<'_, Message>>>()
                            )
                            .spacing(4)
                        },
                    ]
                    .spacing(7),
                )
                .padding([8, 4])
                .width(iced::Length::Fill)
            } else {
                container(Space::new().height(0))
            },

            Space::new().height(sizing.height),

        ]
        .spacing(8)
        .height(sizing.height),
    )
    .width(sizing.width)
    .height(sizing.height)
    .padding([12, 12]);

    let body = row![aci_list, vsep(sizing.height), edit_panel].height(sizing.height);

    container(column![toolbar, hdivider(sizing.width), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color
            )),
            ..Default::default()
        })
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
