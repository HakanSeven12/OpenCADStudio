//! Layer State Manager — native DWG/DXF named layer-state UI.

use crate::app::{LayerStateLayerFlag, LayerStateProperty, Message};
use crate::ui::properties::{lw_options, LwItem};
use acadrust::{LayerState, LayerStateMask};
use iced::widget::{
    button, checkbox, column, container, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};
use crate::t;
use std::borrow::Cow;
use std::fmt;

#[derive(Clone, PartialEq, Debug)]
struct TransparencyItem(Option<acadrust::types::Transparency>);

impl fmt::Display for TransparencyItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => write!(f, "{}", t!("Not set")),
            Some(value) => write!(f, "{}%", (value.as_percent() * 100.0).round() as u8),
        }
    }
}

fn transparency_options(current: Option<acadrust::types::Transparency>) -> Vec<TransparencyItem> {
    let mut options = vec![TransparencyItem(None)];
    for percent in (0..=90).step_by(10) {
        options.push(TransparencyItem(Some(
            acadrust::types::Transparency::from_percent(percent as f64 / 100.0),
        )));
    }
    let current = TransparencyItem(current);
    if !options.contains(&current) {
        options.insert(1, current);
    }
    options
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(
            theme
                .palette()
                .background
                .base
                .text
                .scale_alpha(0.65),
        ),
    }
}

fn button_style(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if accent {
            button::primary(theme, status)
        } else {
            button::secondary(theme, status)
        }
    }
}

fn list_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

fn divider<'a>(width: Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color,
            )),
            ..Default::default()
        })
        .into()
}

fn mask_summary(mask: LayerStateMask) -> String {
    let properties = [
        (LayerStateMask::ON, "On/Off"),
        (LayerStateMask::FROZEN, "Freeze"),
        (LayerStateMask::LOCKED, "Lock"),
        (LayerStateMask::PLOT, "Plot"),
        (LayerStateMask::NEW_VIEWPORT, "New VP Freeze"),
        (LayerStateMask::COLOR, "Color"),
        (LayerStateMask::LINE_TYPE, "Linetype"),
        (LayerStateMask::LINE_WEIGHT, "Lineweight"),
        (LayerStateMask::PLOT_STYLE, "Plot style"),
        (LayerStateMask::TRANSPARENCY, "Transparency"),
    ];
    properties
        .into_iter()
        .filter_map(|(flag, label)| mask.contains(flag).then(|| t!(label)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn view_window<'a>(
    states: Vec<LayerState>,
    selected: Option<&'a str>,
    name: &'a str,
    description: &'a str,
    filter: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let selected_state = selected.and_then(|selected| {
        states
            .iter()
            .find(|state| state.name.eq_ignore_ascii_case(selected))
    });
    let query = filter.trim().to_lowercase();
    let rows: Vec<Element<'_, Message>> = states
        .iter()
        .filter(|state| {
            query.is_empty()
                || state.name.to_lowercase().contains(&query)
                || state.description.to_lowercase().contains(&query)
        })
        .map(|state| {
            let is_selected =
                selected.is_some_and(|selected| state.name.eq_ignore_ascii_case(selected));
            let subtitle = if state.description.is_empty() {
                t!("%{n} layers", n = state.layers.len()).to_string()
            } else {
                state.description.clone()
            };
            button(
                column![
                    text(state.name.clone()).size(12),
                    text(subtitle).size(10).style(muted),
                ]
                .spacing(2),
            )
            .on_press(Message::LayerStateManagerSelect(state.name.clone()))
            .style(list_style(is_selected))
            .padding([6, 9])
            .width(sizing.width)
            .into()
        })
        .collect();

    let empty: Element<'_, Message> = container(
        column![
            text(if states.is_empty() {
                t!("No layer states in this drawing")
            } else {
                t!("No matching layer states")
            })
            .size(11)
            .style(muted),
            text(t!("Choose New to capture the current layer settings."))
                .size(10)
                .style(muted),
        ]
        .spacing(4),
    )
    .padding(14)
    .into();

    let state_list: Element<'_, Message> = if rows.is_empty() {
        empty
    } else {
        scrollable(column(rows).spacing(2))
            .height(sizing.height)
            .into()
    };

    let left = container(
        column![
            text_input(t!("Search layer states…").as_ref(), filter)
                .on_input(Message::LayerStateManagerFilter)
                .size(11)
                .padding([5, 8]),
            container(state_list)
                .width(sizing.width)
                .height(sizing.height)
                .padding(3)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(8)
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

    let details = if let Some(state) = selected_state {
        column![
            text(t!("Saved state details")).size(13),
            row![
                text(t!("Layers")).size(10).style(muted).width(92),
                text(state.layers.len().to_string()).size(11),
            ]
            .spacing(8),
            row![
                text(t!("Current layer")).size(10).style(muted).width(92),
                text(if state.current_layer.is_empty() {
                    "—".to_string()
                } else {
                    state.current_layer.clone()
                })
                .size(11),
            ]
            .spacing(8),
            text(t!("Restored properties")).size(10).style(muted),
            text(mask_summary(state.mask)).size(11),
        ]
        .spacing(7)
    } else {
        column![
            text(t!("New layer state")).size(13),
            text(t!("Save captures the current settings of every layer in the drawing."))
                .size(11)
                .style(muted),
        ]
        .spacing(7)
    };

    let restore = if selected_state.is_some() {
        button(text(t!("Restore")).size(11))
            .on_press(Message::LayerStateManagerRestore)
            .style(button_style(true))
    } else {
        button(text(t!("Restore")).size(11)).style(button_style(true))
    };
    let delete = if selected_state.is_some() {
        button(text(t!("Delete")).size(11))
            .on_press(Message::LayerStateManagerDelete)
            .style(button::danger)
    } else {
        button(text(t!("Delete")).size(11)).style(button::danger)
    };
    let edit = if selected_state.is_some() {
        button(text(t!("Edit")).size(11))
            .on_press(Message::LayerStateManagerEdit)
            .style(button_style(false))
    } else {
        button(text(t!("Edit")).size(11)).style(button_style(false))
    };

    let right = container(
        column![
            row![
                button(text(t!("New")).size(11))
                    .on_press(Message::LayerStateManagerNew)
                    .style(button_style(false))
                    .padding([5, 12]),
                edit.padding([5, 12]),
                Space::new().width(sizing.width),
                restore.padding([5, 12]),
                delete.padding([5, 12]),
            ]
            .spacing(6)
            .align_y(iced::Center),
            divider(sizing.width),
            text(t!("Name")).size(10).style(muted),
            text_input(t!("Layer state name").as_ref(), name)
                .on_input(Message::LayerStateManagerName)
                .on_submit(Message::LayerStateManagerSave)
                .size(11)
                .padding([5, 8]),
            text(t!("Description")).size(10).style(muted),
            text_input(t!("Optional description").as_ref(), description)
                .on_input(Message::LayerStateManagerDescription)
                .size(11)
                .padding([5, 8]),
            Space::new().height(8),
            details,
            Space::new().height(sizing.height),
            text(t!("Layer states are stored inside the drawing and remain available after reopening it."))
                .size(10)
                .style(muted),
            button(text(if selected_state.is_some() {
                t!("Update from Drawing")
            } else {
                t!("Save New State")
            })
            .size(11))
            .on_press(Message::LayerStateManagerSave)
            .style(button_style(true))
            .padding([6, 14]),
        ]
        .spacing(7)
        .height(sizing.height),
    )
    .width(sizing.width)
    .height(sizing.height)
    .padding([12, 12]);

    container(row![left, right].height(sizing.height))
        .width(sizing.width)
        .height(sizing.height)
        .into()
}

fn mask_for(property: LayerStateProperty) -> LayerStateMask {
    match property {
        LayerStateProperty::On => LayerStateMask::ON,
        LayerStateProperty::Frozen => LayerStateMask::FROZEN,
        LayerStateProperty::Locked => LayerStateMask::LOCKED,
        LayerStateProperty::Plot => LayerStateMask::PLOT,
        LayerStateProperty::NewViewport => LayerStateMask::NEW_VIEWPORT,
        LayerStateProperty::Color => LayerStateMask::COLOR,
        LayerStateProperty::LineType => LayerStateMask::LINE_TYPE,
        LayerStateProperty::LineWeight => LayerStateMask::LINE_WEIGHT,
        LayerStateProperty::PlotStyle => LayerStateMask::PLOT_STYLE,
        LayerStateProperty::Transparency => LayerStateMask::TRANSPARENCY,
    }
}

fn mask_button<'a>(
    state: &LayerState,
    label: Cow<'static, str>,
    property: LayerStateProperty,
) -> Element<'a, Message> {
    let enabled = state.mask.contains(mask_for(property));
    button(
        row![
            text(if enabled { "✓" } else { " " }).size(10),
            text(label).size(10),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(Message::LayerStateEditorMaskToggle(property))
    .style(list_style(enabled))
    .padding([4, 7])
    .into()
}

fn bool_cell<'a>(
    value: bool,
    index: usize,
    flag: LayerStateLayerFlag,
    width: f32,
) -> Element<'a, Message> {
    container(
        checkbox(value)
            .on_toggle(move |_| Message::LayerStateEditorLayerFlagToggle(index, flag))
            .size(14),
    )
    .center_x(Length::Fixed(width))
    .width(Length::Fixed(width))
    .into()
}

fn editor_header<'a>() -> Element<'a, Message> {
    container(
        row![
            text(t!("Layer")).size(10).style(muted).width(Length::Fixed(170.0)),
            text(t!("On")).size(10).style(muted).width(Length::Fixed(44.0)),
            text(t!("Freeze")).size(10).style(muted).width(Length::Fixed(54.0)),
            text(t!("Lock")).size(10).style(muted).width(Length::Fixed(44.0)),
            text(t!("Plot")).size(10).style(muted).width(Length::Fixed(44.0)),
            text(t!("New VP")).size(10).style(muted).width(Length::Fixed(54.0)),
            text(t!("Color")).size(10).style(muted).width(Length::Fixed(135.0)),
            text(t!("Linetype")).size(10).style(muted).width(Length::Fixed(150.0)),
            text(t!("Lineweight")).size(10).style(muted).width(Length::Fixed(115.0)),
            text(t!("Plot style")).size(10).style(muted).width(Length::Fixed(135.0)),
            text(t!("Transparency")).size(10).style(muted).width(Length::Fixed(105.0)),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .padding([5, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.weak.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .width(Fill)
    .into()
}

fn editor_layer_row<'a>(
    index: usize,
    layer: &'a acadrust::LayerStateLayer,
    color_open: bool,
    linetypes: Vec<String>,
) -> Element<'a, Message> {
    let current_linetype = Some(layer.line_type.clone());
    let current_lineweight = Some(LwItem(layer.line_weight));
    let color = crate::ui::color_select::color_selector(
        layer.color,
        color_open,
        crate::ui::color_select::ColorExtras {
            by_layer: false,
            by_block: false,
            ..Default::default()
        },
        move |color| Message::LayerStateEditorLayerColor(index, color),
        Message::LayerStateEditorLayerColorToggle(index),
        Message::OpenColorWindow(
            crate::app::ColorPickTarget::LayerState(index),
            layer.color,
        ),
    );

    container(
        row![
            text(layer.layer_name.as_str())
                .size(11)
                .width(Length::Fixed(170.0)),
            bool_cell(!layer.off, index, LayerStateLayerFlag::On, 44.0),
            bool_cell(layer.frozen, index, LayerStateLayerFlag::Frozen, 54.0),
            bool_cell(layer.locked, index, LayerStateLayerFlag::Locked, 44.0),
            bool_cell(layer.plottable, index, LayerStateLayerFlag::Plot, 44.0),
            bool_cell(
                layer.new_viewport_frozen,
                index,
                LayerStateLayerFlag::NewViewport,
                54.0
            ),
            container(color).width(Length::Fixed(135.0)),
            iced::widget::pick_list(
                current_linetype,
                linetypes,
                |value| value.to_string(),
            )
            .on_select(move |value| Message::LayerStateEditorLayerLinetype(index, value))
            .text_size(11)
            .padding([3, 5])
            .width(Length::Fixed(150.0)),
            iced::widget::pick_list(
                current_lineweight,
                lw_options(),
                |value| value.to_string(),
            )
            .on_select(move |item: LwItem| {
                Message::LayerStateEditorLayerLineweight(index, item.0)
            })
            .text_size(11)
            .padding([3, 5])
            .width(Length::Fixed(115.0)),
            text_input(t!("Default").as_ref(), &layer.plot_style)
                .on_input(move |value| Message::LayerStateEditorLayerPlotStyle(index, value))
                .size(11)
                .padding([3, 5])
                .width(Length::Fixed(135.0)),
            iced::widget::pick_list(
                Some(TransparencyItem(layer.transparency)),
                transparency_options(layer.transparency),
                |value| value.to_string(),
            )
            .on_select(move |item| Message::LayerStateEditorLayerTransparency(index, item.0))
            .text_size(11)
            .padding([3, 5])
            .width(Length::Fixed(105.0)),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .padding([3, 8])
    .style(move |theme: &Theme| container::Style {
        background: (index % 2 == 1).then_some(Background::Color(
            theme.palette().background.weak.color,
        )),
        ..Default::default()
    })
    .width(Fill)
    .into()
}

pub fn view_editor<'a>(
    state: &'a LayerState,
    filter: &'a str,
    color_open: Option<usize>,
    linetypes: Vec<String>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let properties = [
        (t!("On / Off"), LayerStateProperty::On),
        (t!("Freeze"), LayerStateProperty::Frozen),
        (t!("Lock"), LayerStateProperty::Locked),
        (t!("Plot"), LayerStateProperty::Plot),
        (t!("New VP"), LayerStateProperty::NewViewport),
        (t!("Color"), LayerStateProperty::Color),
        (t!("Linetype"), LayerStateProperty::LineType),
        (t!("Lineweight"), LayerStateProperty::LineWeight),
        (t!("Plot style"), LayerStateProperty::PlotStyle),
        (t!("Transparency"), LayerStateProperty::Transparency),
    ];
    let mask_controls = properties
        .into_iter()
        .fold(row![].spacing(5), |row, (label, property)| {
            row.push(mask_button(state, label, property))
        });

    let layer_names: Vec<String> = state
        .layers
        .iter()
        .map(|layer| layer.layer_name.clone())
        .collect();
    let query = filter.trim().to_lowercase();
    let rows = state
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| {
            query.is_empty() || layer.layer_name.to_lowercase().contains(&query)
        })
        .fold(column![].spacing(0), |rows, (index, layer)| {
            rows.push(editor_layer_row(
                index,
                layer,
                color_open == Some(index),
                linetypes.clone(),
            ))
        });

    container(
        column![
            row![
                text(t!("Name")).size(10).style(muted),
                text_input(t!("Layer state name").as_ref(), &state.name)
                    .on_input(Message::LayerStateEditorName)
                    .size(11)
                    .padding([3, 6])
                    .width(Length::Fixed(220.0)),
                text(t!("Description")).size(10).style(muted),
                text_input(t!("Optional description").as_ref(), &state.description)
                    .on_input(Message::LayerStateEditorDescription)
                    .size(11)
                    .padding([3, 6])
                    .width(sizing.width),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text(t!("%{n} saved layers", n = state.layers.len()))
                    .size(10)
                    .style(muted),
                Space::new().width(sizing.width),
                text(t!("Current layer")).size(10).style(muted),
                iced::widget::pick_list(
                    Some(state.current_layer.clone()),
                    layer_names,
                    |value| value.to_string(),
                )
                .on_select(Message::LayerStateEditorCurrentLayer)
                .text_size(11)
                .padding([3, 6])
                .width(Length::Fixed(180.0)),
            ]
            .spacing(8)
            .align_y(iced::Center),
            divider(sizing.width),
            text(t!("Properties restored by this state")).size(10).style(muted),
            mask_controls,
            row![
                text(t!("Saved layer values")).size(12),
                Space::new().width(sizing.width),
                text_input(t!("Search layers…").as_ref(), filter)
                    .on_input(Message::LayerStateEditorFilter)
                    .size(11)
                    .padding([4, 7])
                    .width(Length::Fixed(190.0)),
            ]
            .align_y(iced::Center),
            container(
                column![editor_header(), scrollable(rows).height(sizing.height)]
                    .spacing(0)
                    .height(sizing.height),
            )
            .height(sizing.height)
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }),
            row![
                text(t!("Changes affect the saved state only; the drawing is unchanged until Restore."))
                    .size(10)
                    .style(muted),
                Space::new().width(sizing.width),
                button(text(t!("Cancel")).size(11))
                    .on_press(Message::LayerStateEditorCancel)
                    .style(button_style(false))
                    .padding([5, 12]),
                button(text(t!("Save Changes")).size(11))
                    .on_press(Message::LayerStateEditorSave)
                    .style(button_style(true))
                    .padding([5, 12]),
            ]
            .spacing(7)
            .align_y(iced::Center),
        ]
        .spacing(8)
        .height(sizing.height),
    )
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
