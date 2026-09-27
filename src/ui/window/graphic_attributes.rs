//! Graphic Attributes palette: apply or remove an associative object fill.

use crate::app::Message;
use crate::ui::dock::{DockMsg, PanelId};
use acadrust::{CadDocument, EntityType, Handle};
use iced::widget::{
    button, column, container, image, mouse_area, row, slider, text, tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

pub(crate) struct SolidFillColorInfo {
    pub color: acadrust::types::Color,
    pub display: iced::Color,
    pub varies: bool,
}

struct FillTransparencyInfo {
    value: u8,
    varies: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicAttribute {
    Varies,
    None,
    Solid,
    Hatch,
    Gradient,
}

impl GraphicAttribute {
    pub const ALL: [Self; 4] = [Self::None, Self::Solid, Self::Hatch, Self::Gradient];

    fn icon(self) -> &'static [u8] {
        match self {
            Self::Varies | Self::None => {
                include_bytes!("../../../assets/icons/hatch/hatch_none.svg")
            }
            Self::Solid => include_bytes!("../../../assets/icons/hatch/hatch_solid.svg"),
            Self::Hatch => include_bytes!("../../../assets/icons/hatch/hatch_lines.svg"),
            Self::Gradient => include_bytes!("../../../assets/icons/hatch/hatch_gradient.svg"),
        }
    }
}

impl std::fmt::Display for GraphicAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Varies => "*VARIES*",
            Self::None => "None",
            Self::Solid => "Solid",
            Self::Hatch => "Hatch",
            Self::Gradient => "Gradient",
        })
    }
}

pub(crate) fn current(document: &CadDocument, selected: &[Handle]) -> GraphicAttribute {
    // Scan the document once, rather than once per selected object on every frame.
    let mut fills: rustc_hash::FxHashMap<Handle, Option<GraphicAttribute>> =
        selected.iter().map(|handle| (*handle, None)).collect();
    for entity in document.entities() {
        let EntityType::Hatch(hatch) = entity else {
            continue;
        };
        let value = if hatch.gradient_color.enabled {
            GraphicAttribute::Gradient
        } else if hatch.is_solid {
            GraphicAttribute::Solid
        } else {
            GraphicAttribute::Hatch
        };
        if let Some(fill) = fills.get_mut(&entity.common().handle) {
            fill.get_or_insert(value);
        }
        if !hatch.is_associative {
            continue;
        }
        for handle in hatch.paths.iter().flat_map(|path| &path.boundary_handles) {
            if let Some(fill) = fills.get_mut(handle) {
                fill.get_or_insert(value);
            }
        }
    }
    let Some((&first, rest)) = selected.split_first() else {
        return GraphicAttribute::None;
    };
    let first = fills[&first].unwrap_or(GraphicAttribute::None);
    if rest
        .iter()
        .any(|handle| fills[handle].unwrap_or(GraphicAttribute::None) != first)
    {
        GraphicAttribute::Varies
    } else {
        first
    }
}

pub(crate) fn fill_handles(document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
    let selected: rustc_hash::FxHashSet<_> = selected.iter().copied().collect();
    document
        .entities()
        .filter_map(|entity| {
            let EntityType::Hatch(hatch) = entity else {
                return None;
            };
            let directly_selected = selected.contains(&entity.common().handle);
            let selected_boundary = hatch.is_associative
                && hatch.paths.iter().any(|path| {
                    path.boundary_handles
                        .iter()
                        .any(|handle| selected.contains(handle))
                });
            (directly_selected || selected_boundary).then_some(entity.common().handle)
        })
        .collect()
}

pub(crate) fn solid_fill_handles(document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
    fill_handles(document, selected)
        .into_iter()
        .filter(|handle| {
            matches!(
                document.get_entity(*handle),
                Some(EntityType::Hatch(hatch))
                    if hatch.is_solid && !hatch.gradient_color.enabled
            )
        })
        .collect()
}

fn fill_transparency_info(
    document: &CadDocument,
    selected: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<FillTransparencyInfo> {
    let handles = fill_handles(document, selected);
    let top = handles.iter().copied().max_by(|left, right| {
        let left = draw_depth.get(&left.value()).map_or(0.0, |depth| depth[0]);
        let right = draw_depth
            .get(&right.value())
            .map_or(0.0, |depth| depth[0]);
        left.total_cmp(&right)
    })?;
    let top_entity = document.get_entity(top)?;
    let stored = top_entity.common().transparency;
    let effective = if stored.is_by_layer() {
        document
            .layers
            .get(&top_entity.common().layer)
            .map_or(stored, |layer| layer.transparency)
    } else if stored.is_by_block() {
        acadrust::types::Transparency::from_percent(0.0)
    } else {
        stored
    };
    Some(FillTransparencyInfo {
        value: (effective.as_percent() * 100.0).round().clamp(0.0, 90.0) as u8,
        varies: handles.iter().any(|handle| {
            document
                .get_entity(*handle)
                .is_some_and(|entity| entity.common().transparency != stored)
        }),
    })
}

fn solid_fill_color_info(
    document: &CadDocument,
    selected: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<SolidFillColorInfo> {
    let handles = solid_fill_handles(document, selected);
    let top = handles.iter().copied().max_by(|left, right| {
        let left = draw_depth.get(&left.value()).map_or(0.0, |depth| depth[0]);
        let right = draw_depth
            .get(&right.value())
            .map_or(0.0, |depth| depth[0]);
        left.total_cmp(&right)
    })?;
    let top_entity = document.get_entity(top)?;
    let color = top_entity.common().color;
    let varies = handles.iter().any(|handle| {
        document
            .get_entity(*handle)
            .is_some_and(|entity| entity.common().color != color)
    });
    let rgba = crate::scene::view::render::render_style_for_common_viewport(
        document,
        top_entity.common(),
        None,
    )
    .0;
    Some(SolidFillColorInfo {
        color,
        display: iced::Color::from_rgba(rgba[0], rgba[1], rgba[2], 1.0),
        varies,
    })
}

fn top_fill_hatch<'a>(
    document: &'a CadDocument,
    selected: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    expected: GraphicAttribute,
) -> Option<&'a acadrust::entities::Hatch> {
    let selected: rustc_hash::FxHashSet<_> = selected.iter().copied().collect();
    document
        .entities()
        .filter_map(|entity| {
            let EntityType::Hatch(hatch) = entity else {
                return None;
            };
            let value = if hatch.gradient_color.enabled {
                GraphicAttribute::Gradient
            } else if hatch.is_solid {
                GraphicAttribute::Solid
            } else {
                GraphicAttribute::Hatch
            };
            if value != expected {
                return None;
            }
            let directly_selected = selected.contains(&entity.common().handle);
            let selected_boundary = hatch.is_associative
                && hatch.paths.iter().any(|path| {
                    path.boundary_handles
                        .iter()
                        .any(|handle| selected.contains(handle))
                });
            (directly_selected || selected_boundary).then_some((entity.common().handle, hatch))
        })
        .max_by(|(left, _), (right, _)| {
            let left = draw_depth.get(&left.value()).map_or(0.0, |depth| depth[0]);
            let right = draw_depth
                .get(&right.value())
                .map_or(0.0, |depth| depth[0]);
            left.total_cmp(&right)
        })
        .map(|(_, hatch)| hatch)
}

pub fn view<'a>(
    document: &'a CadDocument,
    selected: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    width: f32,
    auto_collapse: bool,
    side: crate::app::config::DockSide,
    menu_open: bool,
    solid_color_menu_open: bool,
    transparency_menu_open: bool,
    hatch_editor_open: bool,
    gradient_editor: Option<&'a crate::ui::window::gradient_editor::GradientEditorState>,
    gradient_color_picker_open: bool,
) -> Element<'a, Message> {
    let pin_icon = if auto_collapse {
        crate::ui::icons::themed_primary_weak_text(crate::ui::icons::PIN, 12.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::PIN, 12.0)
    };
    let pin = button(pin_icon)
        .on_press(Message::Dock(DockMsg::AutoCollapseToggle(
            PanelId::GraphicAttributes,
        )))
        .style(button::subtle)
        .padding([3, 5]);
    let pin = tooltip(
        pin,
        text(crate::t!("Auto")).size(10),
        tooltip::Position::Bottom,
    )
    .gap(4);
    let close = button(crate::ui::icons::themed_secondary(
        crate::ui::icons::CLOSE,
        12.0,
    ))
    .on_press(Message::Dock(DockMsg::Close(PanelId::GraphicAttributes)))
    .style(button::subtle)
    .padding([3, 5]);
    let close = tooltip(
        close,
        text(crate::t!("Close")).size(10),
        tooltip::Position::Bottom,
    )
    .gap(4);
    let header = mouse_area(
        container(
            row![
                text(crate::t!("Graphic Attributes")).size(12),
                Space::new().width(Fill),
                pin,
                close
            ]
            .spacing(3)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Fill)
        .padding([3, 6]),
    )
    .on_press(Message::Dock(DockMsg::DockGrab(PanelId::GraphicAttributes)))
    .interaction(iced::mouse::Interaction::Grab);

    let current = current(document, selected);
    let picker = button(
        row![
            crate::ui::icons::semantic(current.icon(), 14.0),
            text(current.to_string()).size(11),
            Space::new().width(Fill),
            crate::ui::icons::themed_arrow_toggle(menu_open, 9.0),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(Message::ToggleGraphicAttributeDropdown)
    .style(move |theme: &Theme, _| {
        let palette = theme.palette();
        button::Style {
            background: Some(Background::Color(palette.background.base.color)),
            text_color: palette.background.base.text,
            border: Border {
                color: if menu_open {
                    palette.primary.base.color
                } else {
                    palette.background.neutral.color
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        }
    })
    .height(crate::ui::ROW_H)
    .width(Fill)
    .padding([3, 6]);
    let picker: Element<'_, Message> = if menu_open {
        let mut rows = column![].spacing(0);
        for value in GraphicAttribute::ALL {
            rows = rows.push(
                button(
                    row![
                        crate::ui::icons::semantic(value.icon(), 14.0),
                        text(value.to_string()).size(11),
                    ]
                    .spacing(5)
                    .align_y(iced::Center),
                )
                .on_press(Message::GraphicAttributeChanged(value))
                .style(crate::ui::color_select::list_row_style)
                .width(Fill)
                .padding([2, 4]),
            );
        }
        let popup = container(rows)
            .style(crate::ui::color_select::popup_panel_style)
            .padding(2);
        crate::ui::color_select::drop_down_below(
            picker.into(),
            popup.into(),
            None,
            Length::Shrink,
            Message::CloseGraphicAttributeDropdown,
        )
    } else {
        picker.into()
    };
    let mut body = column![text(crate::t!("Fill")).size(11), picker].spacing(6);
    if current == GraphicAttribute::Solid {
        if let Some(info) = solid_fill_color_info(document, selected, draw_depth) {
            let label = info.varies.then(|| {
                container(text("* VARIES *").size(10))
                    .width(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center)
            });
            let color = button(label.unwrap_or_else(|| {
                container(Space::new()).width(Fill).height(Fill)
            }))
            .on_press(Message::OpenColorWindow(
                crate::app::ColorPickTarget::GraphicAttributesSolid,
                info.color,
            ))
            .width(Fill)
            .height(crate::ui::ROW_H)
            .padding(0)
            .style(move |theme: &Theme, status| button::Style {
                background: Some(Background::Color(info.display)),
                text_color: if info.display.r * 0.299
                    + info.display.g * 0.587
                    + info.display.b * 0.114
                    > 0.55
                {
                    iced::Color::BLACK
                } else {
                    iced::Color::WHITE
                },
                border: Border {
                    color: if matches!(status, button::Status::Hovered) {
                        theme.palette().primary.base.color
                    } else {
                        theme.palette().background.neutral.color
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
            let more = button(text("...").size(crate::ui::ROW_H * 0.42))
                .on_press(Message::ToggleSolidFillColorDropdown)
                .style(move |theme: &Theme, status| {
                    if solid_color_menu_open {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    }
                })
                .height(crate::ui::ROW_H)
                .padding([2, 7]);
            let more: Element<'_, Message> = if solid_color_menu_open {
                let choice = |label, color| {
                    button(text(label).size(11))
                        .on_press(Message::SolidFillColorChanged(color))
                        .style(crate::ui::color_select::list_row_style)
                        .width(Fill)
                        .padding([2, 4])
                };
                let popup = container(column![
                    choice("ByLayer", acadrust::types::Color::ByLayer),
                    choice("ByBlock", acadrust::types::Color::ByBlock),
                    button(text("Custom...").size(11))
                        .on_press(Message::OpenColorWindow(
                            crate::app::ColorPickTarget::GraphicAttributesSolid,
                            info.color,
                        ))
                        .style(crate::ui::color_select::list_row_style)
                        .width(Fill)
                        .padding([2, 4])
                ])
                .style(crate::ui::color_select::popup_panel_style)
                .padding(2);
                let alignment = match side {
                    crate::app::config::DockSide::Left => {
                        iced_aw::drop_down::Alignment::BottomEnd
                    }
                    crate::app::config::DockSide::Right => {
                        iced_aw::drop_down::Alignment::BottomStart
                    }
                };
                iced_aw::DropDown::new(more, popup, true)
                    .width(Length::Fixed(110.0))
                    .height(Length::Shrink)
                    .alignment(alignment)
                    .offset(2.0)
                    .on_dismiss(Message::CloseSolidFillColorDropdown)
                    .into()
            } else {
                more.into()
            };
            body = body.push(row![color, more].spacing(4).align_y(iced::Center));
        }
    } else if current == GraphicAttribute::Gradient {
        if let Some(hatch) = top_fill_hatch(
            document,
            selected,
            draw_depth,
            GraphicAttribute::Gradient,
        ) {
            let preview = container(
                image(crate::ui::window::gradient_editor::compact_preview(hatch))
                    .width(Fill)
                    .height(crate::ui::ROW_H),
            )
            .width(Fill)
            .height(crate::ui::ROW_H)
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
            let edit = button(text("...").size(crate::ui::ROW_H * 0.42))
                .on_press(if gradient_editor.is_some() {
                    Message::GradientCancel
                } else {
                    Message::GradientEditorOpen
                })
                .style(move |theme: &Theme, status| {
                    if gradient_editor.is_some() {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    }
                })
                .height(crate::ui::ROW_H)
                .padding([2, 7]);
            let edit = tooltip(
                edit,
                text("Edit gradient").size(10),
                tooltip::Position::Bottom,
            );
            let edit: Element<'_, Message> = if let Some(editor) = gradient_editor {
                let alignment = match side {
                    crate::app::config::DockSide::Left => {
                        iced_aw::drop_down::Alignment::BottomEnd
                    }
                    crate::app::config::DockSide::Right => {
                        iced_aw::drop_down::Alignment::BottomStart
                    }
                };
                iced_aw::DropDown::new(
                    edit,
                    crate::ui::window::gradient_editor::view(
                        editor,
                        gradient_color_picker_open,
                    ),
                    true,
                )
                .width(Length::Fixed(390.0))
                .alignment(alignment)
                .offset(4.0)
                .into()
            } else {
                edit.into()
            };
            body = body.push(row![preview, edit].spacing(4).align_y(iced::Center));
        }
    } else if current == GraphicAttribute::Hatch {
        let preview: Element<'_, Message> = top_fill_hatch(
            document,
            selected,
            draw_depth,
            GraphicAttribute::Hatch,
        )
        .and_then(|hatch| {
            crate::scene::Scene::hatch_model_from_dxf(hatch, [1.0; 4])
                .map(|model| model.pattern)
        })
        .map(crate::ui::properties::compact_hatch_pattern_preview)
        .unwrap_or_else(|| {
            container(Space::new())
                .width(Fill)
                .height(crate::ui::ROW_H)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        });
        let edit = button(text("...").size(crate::ui::ROW_H * 0.42))
            .on_press(if hatch_editor_open {
                Message::HatchEditorClose
            } else {
                Message::HatchEditorOpen
            })
            .style(move |theme: &Theme, status| {
                if hatch_editor_open {
                    button::primary(theme, status)
                } else {
                    button::secondary(theme, status)
                }
            })
            .height(crate::ui::ROW_H)
            .padding([2, 7]);
        let edit = tooltip(edit, text("Edit hatch").size(10), tooltip::Position::Bottom);
        let edit: Element<'_, Message> = if hatch_editor_open {
            let alignment = match side {
                crate::app::config::DockSide::Left => iced_aw::drop_down::Alignment::BottomEnd,
                crate::app::config::DockSide::Right => iced_aw::drop_down::Alignment::BottomStart,
            };
            iced_aw::DropDown::new(edit, crate::ui::window::hatch_editor::view(), true)
                .width(Length::Fixed(390.0))
                .alignment(alignment)
                .offset(4.0)
                .into()
        } else {
            edit.into()
        };
        body = body.push(row![preview, edit].spacing(4).align_y(iced::Center));
    }
    if current != GraphicAttribute::None {
        if let Some(info) = fill_transparency_info(document, selected, draw_depth) {
            let value_label = if info.varies {
                "*VARIES*".to_string()
            } else {
                format!("{}%", info.value)
            };
            let control = row![
                text(crate::t!("Transparency")).size(11).width(75),
                slider(0..=90, i32::from(info.value), |value| {
                    Message::FillTransparencyChanged(value as u8)
                })
                .width(Fill),
                text(value_label).size(10).width(45),
            ]
            .spacing(5)
            .align_y(iced::Center)
            .height(crate::ui::ROW_H);
            let more = button(text("...").size(crate::ui::ROW_H * 0.42))
                .on_press(Message::ToggleFillTransparencyDropdown)
                .style(move |theme: &Theme, status| {
                    if transparency_menu_open {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    }
                })
                .height(crate::ui::ROW_H)
                .padding([2, 7]);
            let more: Element<'_, Message> = if transparency_menu_open {
                let row_button = |label, message| {
                    button(text(label).size(11))
                        .on_press(message)
                        .style(crate::ui::color_select::list_row_style)
                        .width(Fill)
                        .padding([2, 4])
                };
                let popup = container(column![
                    row_button("ByLayer", Message::FillTransparencyByLayer),
                    row_button("ByBlock", Message::FillTransparencyByBlock),
                    row_button(
                        "Custom",
                        Message::FillTransparencyChanged(info.value),
                    ),
                ])
                .style(crate::ui::color_select::popup_panel_style)
                .padding(2);
                let alignment = match side {
                    crate::app::config::DockSide::Left => {
                        iced_aw::drop_down::Alignment::BottomEnd
                    }
                    crate::app::config::DockSide::Right => {
                        iced_aw::drop_down::Alignment::BottomStart
                    }
                };
                iced_aw::DropDown::new(more, popup, true)
                    .width(Length::Fixed(110.0))
                    .height(Length::Shrink)
                    .alignment(alignment)
                    .offset(2.0)
                    .on_dismiss(Message::CloseFillTransparencyDropdown)
                    .into()
            } else {
                more.into()
            };
            body = body.push(row![control, more].spacing(4).align_y(iced::Center));
        }
    }
    let body = body.padding(8);

    container(column![header, body])
        .width(Length::Fixed(width))
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .into()
}
