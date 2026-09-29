//! Graphic Attributes palette: apply or remove an associative object fill.

use crate::app::Message;
use crate::ui::dock::{DockMsg, PanelId};
use acadrust::{CadDocument, EntityType, Handle};
use iced::widget::{
    button, column, combo_box, container, image, mouse_area, row, slider, text, text_input,
    tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Length, Padding, Theme};

pub(crate) struct SolidFillColorInfo {
    pub color: acadrust::types::Color,
    pub display: iced::Color,
    pub varies: bool,
}

struct TransparencyInfo {
    value: u8,
    varies: bool,
    mode: PropertyMode,
}

#[derive(Clone, Copy)]
enum PropertyMode {
    ByLayer,
    ByBlock,
    Custom,
    Varies,
}

impl PropertyMode {
    fn button_label(self) -> &'static str {
        match self {
            Self::ByLayer => "L",
            Self::ByBlock => "B",
            Self::Custom => "C",
            Self::Varies => "V",
        }
    }
}

struct LinetypeInfo {
    value: String,
    varies: bool,
}

struct LineweightInfo {
    value: acadrust::types::LineWeight,
    varies: bool,
}

struct LinetypeScaleInfo {
    value: f64,
    varies: bool,
}

const PALETTE_CONTROL_ICON_SIZE: f32 = 20.0;
const COMPACT_BUTTON_HEIGHT: f32 = 22.0;
const TRANSPARENCY_ICON: &[u8] =
    include_bytes!("../../../assets/icons/attributes/transparency.svg");
const LINEWEIGHT_ICON: &[u8] = include_bytes!("../../../assets/icons/attributes/lineweight.svg");

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

pub(crate) fn line_handles(document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
    let mut handles = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();
    for handle in selected {
        match document.get_entity(*handle) {
            Some(EntityType::Hatch(hatch)) if hatch.is_associative => {
                for boundary in hatch.paths.iter().flat_map(|path| &path.boundary_handles) {
                    if seen.insert(*boundary)
                        && document
                            .get_entity(*boundary)
                            .is_some_and(|entity| !matches!(entity, EntityType::Hatch(_)))
                    {
                        handles.push(*boundary);
                    }
                }
            }
            Some(entity) if !matches!(entity, EntityType::Hatch(_)) && seen.insert(*handle) => {
                handles.push(*handle);
            }
            _ => {}
        }
    }
    handles
}

fn top_handle(
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<Handle> {
    handles.iter().copied().max_by(|left, right| {
        let left = draw_depth.get(&left.value()).map_or(0.0, |depth| depth[0]);
        let right = draw_depth.get(&right.value()).map_or(0.0, |depth| depth[0]);
        left.total_cmp(&right)
    })
}

fn transparency_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<TransparencyInfo> {
    let top = top_handle(handles, draw_depth)?;
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
    Some(TransparencyInfo {
        value: (effective.as_percent() * 100.0).round().clamp(0.0, 90.0) as u8,
        varies: handles.iter().any(|handle| {
            document
                .get_entity(*handle)
                .is_some_and(|entity| entity.common().transparency != stored)
        }),
        mode: if stored.is_by_layer() {
            PropertyMode::ByLayer
        } else if stored.is_by_block() {
            PropertyMode::ByBlock
        } else {
            PropertyMode::Custom
        },
    })
}

fn color_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<SolidFillColorInfo> {
    let top = top_handle(handles, draw_depth)?;
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

fn linetype_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<LinetypeInfo> {
    let top = top_handle(handles, draw_depth)?;
    let value = document.get_entity(top)?.common().linetype.clone();
    let varies = handles.iter().any(|handle| {
        document
            .get_entity(*handle)
            .is_some_and(|entity| entity.common().linetype != value)
    });
    Some(LinetypeInfo {
        value: if value.is_empty() {
            "ByLayer".to_string()
        } else {
            value
        },
        varies,
    })
}

fn lineweight_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<LineweightInfo> {
    let top = top_handle(handles, draw_depth)?;
    let value = document.get_entity(top)?.common().line_weight;
    let varies = handles.iter().any(|handle| {
        document
            .get_entity(*handle)
            .is_some_and(|entity| entity.common().line_weight != value)
    });
    Some(LineweightInfo { value, varies })
}

fn effective_lineweight(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    stored: acadrust::types::LineWeight,
) -> acadrust::types::LineWeight {
    if !matches!(stored, acadrust::types::LineWeight::ByLayer) {
        return stored;
    }
    let layer_name = top_handle(handles, draw_depth)
        .and_then(|handle| document.get_entity(handle))
        .map(|entity| entity.common().layer.as_str())
        .unwrap_or_else(|| {
            if document.header.current_layer_name.is_empty() {
                "0"
            } else {
                document.header.current_layer_name.as_str()
            }
        });
    document
        .layers
        .get(layer_name)
        .map_or(acadrust::types::LineWeight::Default, |layer| {
            layer.line_weight
        })
}

fn linetype_scale_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<LinetypeScaleInfo> {
    let top = top_handle(handles, draw_depth)?;
    let value = document.get_entity(top)?.common().linetype_scale;
    let varies = handles.iter().any(|handle| {
        document
            .get_entity(*handle)
            .is_some_and(|entity| (entity.common().linetype_scale - value).abs() > f64::EPSILON)
    });
    Some(LinetypeScaleInfo { value, varies })
}

fn effective_linetype_is_continuous(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> bool {
    let (name, layer) = top_handle(handles, draw_depth)
        .and_then(|handle| document.get_entity(handle))
        .map(|entity| {
            (
                entity.common().linetype.as_str(),
                entity.common().layer.as_str(),
            )
        })
        .unwrap_or_else(|| {
            (
                current_linetype_name(document),
                document.header.current_layer_name.as_str(),
            )
        });
    let name = if name.is_empty() || name.eq_ignore_ascii_case("ByLayer") {
        document
            .layers
            .get(if layer.is_empty() { "0" } else { layer })
            .map_or("Continuous", |layer| layer.line_type.as_str())
    } else if name.eq_ignore_ascii_case("ByBlock") {
        "Continuous"
    } else {
        name
    };
    name.eq_ignore_ascii_case("Continuous") || name.eq_ignore_ascii_case("Solid")
}

fn current_linetype_name(document: &CadDocument) -> &str {
    if !document.header.current_linetype_name.is_empty() {
        document.header.current_linetype_name.as_str()
    } else if !document.header.current_linetype_handle.is_null() {
        document
            .line_types
            .iter()
            .find(|line_type| line_type.handle == document.header.current_linetype_handle)
            .map_or("ByLayer", |line_type| line_type.name.as_str())
    } else {
        "ByLayer"
    }
}

fn linetype_preview_art(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    logical_name: &str,
) -> String {
    let layer_name = top_handle(handles, draw_depth)
        .and_then(|handle| document.get_entity(handle))
        .map(|entity| entity.common().layer.as_str())
        .unwrap_or_else(|| {
            if document.header.current_layer_name.is_empty() {
                "0"
            } else {
                document.header.current_layer_name.as_str()
            }
        });
    let effective_name = if logical_name.is_empty()
        || logical_name.eq_ignore_ascii_case("ByLayer")
        || logical_name.eq_ignore_ascii_case("ByBlock")
    {
        document
            .layers
            .get(layer_name)
            .map_or("Continuous", |layer| layer.line_type.as_str())
    } else {
        logical_name
    };
    if effective_name.eq_ignore_ascii_case("Continuous")
        || effective_name.eq_ignore_ascii_case("Solid")
    {
        return "_".repeat(80);
    }
    let art = document
        .line_types
        .iter()
        .find(|line_type| line_type.name.eq_ignore_ascii_case(effective_name))
        .map(|line_type| {
            if line_type.elements.is_empty() {
                "____________".to_string()
            } else {
                crate::io::linetypes::extract_pattern(&line_type.description)
            }
        })
        .unwrap_or_default();
    let pattern_start = art.split_whitespace().find_map(|part| {
        (part
            .chars()
            .filter(|character| matches!(character, '_' | '.' | '-' | '/' | '\\' | '|' | '~'))
            .count()
            >= 2)
        .then(|| art.find(part))
        .flatten()
    });
    let pattern = pattern_start.map_or(art.as_str(), |start| &art[start..]);
    if pattern.is_empty() {
        return String::new();
    }
    let mut full_width = pattern.to_string();
    while full_width.chars().count() < 80 {
        full_width.push(' ');
        full_width.push_str(pattern);
    }
    full_width
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
            let right = draw_depth.get(&right.value()).map_or(0.0, |depth| depth[0]);
            left.total_cmp(&right)
        })
        .map(|(_, hatch)| hatch)
}

fn gradients_vary(document: &CadDocument, selected: &[Handle]) -> bool {
    let mut gradients = fill_handles(document, selected)
        .into_iter()
        .filter_map(|handle| match document.get_entity(handle) {
            Some(EntityType::Hatch(hatch)) if hatch.gradient_color.enabled => {
                Some(&hatch.gradient_color)
            }
            _ => None,
        });
    let Some(first) = gradients.next() else {
        return false;
    };
    gradients.any(|gradient| gradient != first)
}

fn popup_alignment(side: crate::app::config::DockSide) -> iced_aw::drop_down::Alignment {
    match side {
        crate::app::config::DockSide::Left => iced_aw::drop_down::Alignment::BottomEnd,
        crate::app::config::DockSide::Right => iced_aw::drop_down::Alignment::BottomStart,
    }
}

fn compact_button_content(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(crate::ui::ROW_H * 0.42))
        .width(12)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .into()
}

fn mode_button_content(mode: PropertyMode) -> Element<'static, Message> {
    compact_button_content(mode.button_label())
}

fn by_mode_menu<'a>(
    side: crate::app::config::DockSide,
    mode: PropertyMode,
    open: bool,
    toggle: Message,
    close: Message,
    by_layer: Message,
    by_block: Message,
) -> Element<'a, Message> {
    let more = button(mode_button_content(mode))
        .on_press(toggle)
        .style(move |theme: &Theme, status| {
            if open {
                button::primary(theme, status)
            } else {
                button::secondary(theme, status)
            }
        })
        .height(COMPACT_BUTTON_HEIGHT)
        .padding([1, 3]);
    if !open {
        return more.into();
    }
    let choice = |label, message| {
        button(text(label).size(11))
            .on_press(message)
            .style(crate::ui::color_select::list_row_style)
            .width(Fill)
            .padding([2, 4])
    };
    let popup = container(column![
        choice("ByLayer", by_layer),
        choice("ByBlock", by_block),
    ])
    .style(crate::ui::color_select::popup_panel_style)
    .padding(2);
    iced_aw::DropDown::new(more, popup, true)
        .width(Length::Fixed(110.0))
        .height(Length::Shrink)
        .alignment(popup_alignment(side))
        .offset(2.0)
        .on_dismiss(close)
        .into()
}

fn color_control<'a>(
    info: SolidFillColorInfo,
    side: crate::app::config::DockSide,
    menu_open: bool,
    target: crate::app::ColorPickTarget,
    toggle: Message,
    close: Message,
    changed: fn(acadrust::types::Color) -> Message,
) -> Element<'a, Message> {
    let label = if info.varies {
        Some("*VARIES*")
    } else {
        match info.color {
            acadrust::types::Color::ByLayer => None,
            acadrust::types::Color::ByBlock => Some("ByBlock"),
            _ => None,
        }
    }
    .map(|label| {
        container(text(label).size(10))
            .width(Fill)
            .align_x(iced::Left)
            .align_y(iced::Center)
            .padding([0, 6])
    });
    let color = button(label.unwrap_or_else(|| container(Space::new()).width(Fill).height(Fill)))
        .on_press(Message::OpenColorWindow(target.clone(), info.color))
        .width(Fill)
        .height(COMPACT_BUTTON_HEIGHT)
        .padding(0)
        .style(move |theme: &Theme, status| button::Style {
            background: Some(Background::Color(info.display)),
            text_color: if info.display.r * 0.299 + info.display.g * 0.587 + info.display.b * 0.114
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
    let mode = if info.varies {
        PropertyMode::Varies
    } else {
        match info.color {
            acadrust::types::Color::ByLayer => PropertyMode::ByLayer,
            acadrust::types::Color::ByBlock => PropertyMode::ByBlock,
            _ => PropertyMode::Custom,
        }
    };
    let more = button(mode_button_content(mode))
        .on_press(toggle)
        .style(move |theme: &Theme, status| {
            if menu_open {
                button::primary(theme, status)
            } else {
                button::secondary(theme, status)
            }
        })
        .height(COMPACT_BUTTON_HEIGHT)
        .padding([1, 3]);
    let more: Element<'a, Message> = if menu_open {
        let choice = |label, color| {
            button(text(label).size(11))
                .on_press(changed(color))
                .style(crate::ui::color_select::list_row_style)
                .width(Fill)
                .padding([2, 4])
        };
        let popup = container(column![
            choice("ByLayer", acadrust::types::Color::ByLayer),
            choice("ByBlock", acadrust::types::Color::ByBlock),
            button(text("Custom...").size(11))
                .on_press(Message::OpenColorWindow(target, info.color))
                .style(crate::ui::color_select::list_row_style)
                .width(Fill)
                .padding([2, 4])
        ])
        .style(crate::ui::color_select::popup_panel_style)
        .padding(2);
        iced_aw::DropDown::new(more, popup, true)
            .width(Length::Fixed(110.0))
            .height(Length::Shrink)
            .alignment(popup_alignment(side))
            .offset(2.0)
            .on_dismiss(close)
            .into()
    } else {
        more.into()
    };
    row![color, more].spacing(4).align_y(iced::Center).into()
}

fn transparency_control<'a>(
    info: TransparencyInfo,
    side: crate::app::config::DockSide,
    menu_open: bool,
    toggle: Message,
    close: Message,
    changed: fn(u8) -> Message,
    by_layer: Message,
    by_block: Message,
) -> Element<'a, Message> {
    let value_label = if info.varies {
        "*VARIES*".to_string()
    } else {
        format!("{}%", info.value)
    };
    let control = row![
        crate::ui::icons::semantic(TRANSPARENCY_ICON, PALETTE_CONTROL_ICON_SIZE),
        text(value_label).size(10).width(38),
        slider(0..=90, i32::from(info.value), move |value| changed(
            value as u8
        ))
        .width(Fill),
    ]
    .spacing(5)
    .align_y(iced::Center)
    .height(crate::ui::ROW_H);
    let mode = if info.varies {
        PropertyMode::Varies
    } else {
        info.mode
    };
    let more = button(mode_button_content(mode))
        .on_press(toggle)
        .style(move |theme: &Theme, status| {
            if menu_open {
                button::primary(theme, status)
            } else {
                button::secondary(theme, status)
            }
        })
        .height(crate::ui::ROW_H)
        .padding([1, 3]);
    let more: Element<'a, Message> = if menu_open {
        let row_button = |label, message| {
            button(text(label).size(11))
                .on_press(message)
                .style(crate::ui::color_select::list_row_style)
                .width(Fill)
                .padding([2, 4])
        };
        let popup = container(column![
            row_button("ByLayer", by_layer),
            row_button("ByBlock", by_block),
            row_button("Custom", changed(info.value)),
        ])
        .style(crate::ui::color_select::popup_panel_style)
        .padding(2);
        iced_aw::DropDown::new(more, popup, true)
            .width(Length::Fixed(110.0))
            .height(Length::Shrink)
            .alignment(popup_alignment(side))
            .offset(2.0)
            .on_dismiss(close)
            .into()
    } else {
        more.into()
    };
    row![control, more].spacing(4).align_y(iced::Center).into()
}

pub fn view<'a>(
    document: &'a CadDocument,
    properties: &'a crate::ui::properties::PropertiesPanel,
    selected: &[Handle],
    draw_depth: &rustc_hash::FxHashMap<u64, [f32; 2]>,
    width: f32,
    auto_collapse: bool,
    side: crate::app::config::DockSide,
    menu_open: bool,
    line_color_menu_open: bool,
    line_linetype_menu_open: bool,
    line_lineweight_menu_open: bool,
    line_transparency_menu_open: bool,
    solid_color_menu_open: bool,
    transparency_menu_open: bool,
    hatch_editor_open: bool,
    hatch_pattern_search: &'a str,
    hatch_pattern_focus: usize,
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
    let line_handles = line_handles(document, selected);
    let fill_handles = fill_handles(document, selected);
    let mut body = column![text(crate::t!("Line")).size(11)].spacing(6);
    let no_selection = selected.is_empty();
    let line_color = color_info(document, &line_handles, draw_depth).or_else(|| {
        no_selection.then(|| {
            let color = document.header.current_entity_color;
            let display = if color == acadrust::types::Color::ByLayer {
                document
                    .layers
                    .get(if document.header.current_layer_name.is_empty() {
                        "0"
                } else {
                        &document.header.current_layer_name
                    })
                    .map_or_else(
                        || crate::ui::properties::acad_color_display(color).0,
                        |layer| crate::ui::properties::acad_color_display(layer.color).0,
                    )
                    } else {
                crate::ui::properties::acad_color_display(color).0
            };
            SolidFillColorInfo {
                color,
                display,
                varies: false,
            }
        })
            });
    let linetype = linetype_info(document, &line_handles, draw_depth).or_else(|| {
        no_selection.then(|| LinetypeInfo {
            value: current_linetype_name(document).to_string(),
            varies: false,
        })
    });
    let continuous_linetype = linetype.as_ref().is_some_and(|info| {
        !info.varies && effective_linetype_is_continuous(document, &line_handles, draw_depth)
    });
    let linetype_scale = linetype_scale_info(document, &line_handles, draw_depth).or_else(|| {
        no_selection.then(|| LinetypeScaleInfo {
            value: document.header.current_entity_linetype_scale,
            varies: false,
        })
    });
    if let Some(info) = linetype.as_ref() {
        let selected = (!info.varies).then(|| crate::ui::properties::LinetypeItem {
            // An empty name makes only the preview art visible in the closed
            // Graphic Attributes control. Menu items retain their full names.
            name: String::new(),
            art: linetype_preview_art(document, &line_handles, draw_depth, &info.value),
        });
        let combo = combo_box(
            &properties.linetype_combo,
            "*VARIES*",
            selected.as_ref(),
            |item: crate::ui::properties::LinetypeItem| Message::LineLinetypeChanged(item.name),
        )
        .size(crate::ui::ROW_H * 0.42)
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 6.0,
            right: 6.0,
        })
        .input_style(crate::ui::properties::combo_input_style)
        .width(Fill);
        let more = by_mode_menu(
            side,
            if info.varies {
                PropertyMode::Varies
            } else if info.value.eq_ignore_ascii_case("ByLayer") {
                PropertyMode::ByLayer
            } else if info.value.eq_ignore_ascii_case("ByBlock") {
                PropertyMode::ByBlock
                    } else {
                PropertyMode::Custom
            },
            line_linetype_menu_open,
            Message::ToggleLineLinetypeMenu,
            Message::CloseLineLinetypeMenu,
            Message::LineLinetypeChanged("ByLayer".to_string()),
            Message::LineLinetypeChanged("ByBlock".to_string()),
        );
        body = body.push(
            row![crate::ui::wide_menu::wide_menu(combo, 220.0), more]
                .spacing(4)
                .align_y(iced::Center),
        );
                    }
    let lineweight = lineweight_info(document, &line_handles, draw_depth).or_else(|| {
        no_selection.then(|| LineweightInfo {
            value: acadrust::types::LineWeight::from_value(document.header.current_line_weight),
            varies: false,
                })
    });
    if let Some(color) = line_color {
        body = body.push(color_control(
            color,
            side,
            line_color_menu_open,
            crate::app::ColorPickTarget::GraphicAttributesLine,
            Message::ToggleLineColorDropdown,
            Message::CloseLineColorDropdown,
            Message::LineColorChanged,
        ));
    }
    if let (Some(scale), Some(lineweight)) = (linetype_scale, lineweight) {
        let scale_text = if scale.varies {
            String::new()
        } else {
            format!("{:.2}", scale.value)
                };
        let mut scale_input = text_input("*VARIES*", &scale_text)
            .size(crate::ui::ROW_H * 0.42)
            .padding([3, 5])
            .style(move |theme: &Theme, status| {
                let mut style = crate::ui::properties::combo_input_style(theme, status);
                if continuous_linetype {
                    let disabled = theme.palette().background.base.text.scale_alpha(0.42);
                    style.value = disabled;
                    style.placeholder = disabled;
                }
                style
            })
            .on_submit(Message::Noop)
            .width(Fill);
        if !continuous_linetype {
            scale_input = scale_input.on_input(Message::LineLinetypeScaleInput);
        }
        let step = |up: bool| {
            let icon: Element<'_, Message> = if up {
                crate::ui::icons::themed_arrow_up(7.0)
            } else {
                crate::ui::icons::themed_arrow_down(7.0)
            };
            let button = button(icon)
                .style(button::subtle)
                .height(10)
                .padding([0, 3]);
            if continuous_linetype {
                button
            } else {
                let delta = if up { 0.1 } else { -0.1 };
                button.on_press(Message::LineLinetypeScaleChanged(
                    (scale.value + delta).clamp(0.01, 1000.0),
                        ))
                    }
        };
        let scale_icon: Element<'_, Message> = if continuous_linetype {
            crate::ui::icons::themed_disabled(
                GraphicAttribute::Solid.icon(),
                PALETTE_CONTROL_ICON_SIZE,
            )
        } else {
            crate::ui::icons::semantic(
                GraphicAttribute::Solid.icon(),
                PALETTE_CONTROL_ICON_SIZE,
            )
        };
        let spinner_divider = container(Space::new())
            .width(Fill)
            .height(1)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            });
        let spinner = container(column![step(true), spinner_divider, step(false)].spacing(0))
            .height(COMPACT_BUTTON_HEIGHT)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
        let scale_field = row![scale_input, spinner]
            .spacing(0)
            .align_y(iced::Center)
            .width(Fill);
        let scale = row![scale_icon, scale_field]
        .spacing(3)
        .align_y(iced::Center)
        .width(Length::FillPortion(1));
        let selected = (!lineweight.varies).then_some(crate::ui::properties::LwItem(
            effective_lineweight(document, &line_handles, draw_depth, lineweight.value),
        ));
        let lineweight_mode = if lineweight.varies {
            PropertyMode::Varies
        } else {
            match lineweight.value {
                acadrust::types::LineWeight::ByLayer => PropertyMode::ByLayer,
                acadrust::types::LineWeight::ByBlock => PropertyMode::ByBlock,
                _ => PropertyMode::Custom,
                    }
                };
        let lineweight_picker = combo_box(
            &properties.lineweight_combo,
            "*VARIES*",
            selected.as_ref(),
            |item: crate::ui::properties::LwItem| Message::LineLineweightChanged(item.0),
        )
        .size(crate::ui::ROW_H * 0.42)
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 6.0,
            right: 6.0,
        })
        .input_style(crate::ui::properties::combo_input_style)
        .width(Fill);
        let more = by_mode_menu(
            side,
            lineweight_mode,
            line_lineweight_menu_open,
            Message::ToggleLineLineweightMenu,
            Message::CloseLineLineweightMenu,
            Message::LineLineweightChanged(acadrust::types::LineWeight::ByLayer),
            Message::LineLineweightChanged(acadrust::types::LineWeight::ByBlock),
        );
        let lineweight = row![
            crate::ui::icons::semantic(LINEWEIGHT_ICON, PALETTE_CONTROL_ICON_SIZE),
            lineweight_picker,
            more,
        ]
        .spacing(3)
        .align_y(iced::Center)
        .width(Length::FillPortion(1));
        body = body.push(
            row![scale, lineweight]
                .spacing(12)
                .align_y(iced::Center)
                .height(crate::ui::ROW_H),
        );
    }
    let line_transparency = transparency_info(document, &line_handles, draw_depth).or_else(|| {
        no_selection.then(|| {
            let stored = document.current_entity_transparency();
            let effective = if stored.is_by_layer() {
                document
                    .layers
                    .get(if document.header.current_layer_name.is_empty() {
                        "0"
            } else {
                        &document.header.current_layer_name
                    })
                    .map_or(stored, |layer| layer.transparency)
            } else if stored.is_by_block() {
                acadrust::types::Transparency::from_percent(0.0)
            } else {
                stored
            };
            TransparencyInfo {
                value: (effective.as_percent() * 100.0).round().clamp(0.0, 90.0) as u8,
                varies: false,
                mode: if stored.is_by_layer() {
                    PropertyMode::ByLayer
                } else if stored.is_by_block() {
                    PropertyMode::ByBlock
                } else {
                    PropertyMode::Custom
                },
        }
        })
    });
    if let Some(info) = line_transparency {
        body = body.push(transparency_control(
            info,
            side,
            line_transparency_menu_open,
            Message::ToggleLineTransparencyDropdown,
            Message::CloseLineTransparencyDropdown,
            Message::LineTransparencyChanged,
            Message::LineTransparencyByLayer,
            Message::LineTransparencyByBlock,
        ));
    }
    body = body
        .push(Space::new().height(2))
        .push(text(crate::t!("Fill")).size(11))
        .push(picker);
    if !matches!(current, GraphicAttribute::None | GraphicAttribute::Gradient) {
        if let Some(info) = color_info(document, &fill_handles, draw_depth) {
            body = body.push(color_control(
                info,
                side,
                solid_color_menu_open,
                crate::app::ColorPickTarget::GraphicAttributesSolid,
                Message::ToggleSolidFillColorDropdown,
                Message::CloseSolidFillColorDropdown,
                Message::SolidFillColorChanged,
            ));
        }
    }
    if current == GraphicAttribute::Gradient {
        if let Some(hatch) =
            top_fill_hatch(document, selected, draw_depth, GraphicAttribute::Gradient)
        {
            let toggle = if gradient_editor.is_some() {
                Message::GradientCancel
            } else {
                Message::GradientEditorOpen
            };
            let preview_image = image(crate::ui::window::gradient_editor::compact_preview(hatch))
                .width(Fill)
                .height(Fill)
                .content_fit(iced::ContentFit::Fill);
            let preview_content: Element<'_, Message> = if gradients_vary(document, selected) {
                iced::widget::stack![
                    preview_image,
                    container(text("*VARIES*").size(10))
                        .width(Fill)
                        .height(Fill)
                        .align_x(iced::Left)
                        .align_y(iced::Center)
                        .padding([0, 6]),
                ]
                .into()
            } else {
                preview_image.into()
            };
            let preview = button(preview_content)
            .on_press(toggle.clone())
            .width(Fill)
            .height(crate::ui::ROW_H)
            .padding(0)
            .style(move |theme: &Theme, status| button::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                border: Border {
                    color: if gradient_editor.is_some()
                        || matches!(status, button::Status::Hovered | button::Status::Pressed)
                    {
                        theme.palette().primary.base.color
                    } else {
                        theme.palette().background.neutral.color
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
            let edit = button(compact_button_content("⋮"))
                .on_press(toggle)
                .style(move |theme: &Theme, status| {
                    if gradient_editor.is_some() {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    }
                })
                .height(COMPACT_BUTTON_HEIGHT)
                .padding([1, 3]);
            let edit = tooltip(
                edit,
                text("Edit gradient").size(10),
                tooltip::Position::Bottom,
            );
            let edit: Element<'_, Message> = if let Some(editor) = gradient_editor {
                let alignment = match side {
                    crate::app::config::DockSide::Left => iced_aw::drop_down::Alignment::BottomEnd,
                    crate::app::config::DockSide::Right => {
                        iced_aw::drop_down::Alignment::BottomStart
                    }
                };
                iced_aw::DropDown::new(
                    edit,
                    crate::ui::window::gradient_editor::view(editor, gradient_color_picker_open),
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
        let hatch = top_fill_hatch(document, selected, draw_depth, GraphicAttribute::Hatch);
        let current_pattern = hatch.map_or("", |hatch| hatch.pattern.name.as_str());
        let preview_content: Element<'_, Message> = hatch
        .and_then(|hatch| {
            crate::scene::Scene::hatch_model_from_dxf(hatch, [1.0; 4])
                .map(|model| model.pattern)
        })
        .map(crate::ui::properties::compact_hatch_pattern_preview)
        .unwrap_or_else(|| {
            container(Space::new())
                .width(Fill)
                .height(crate::ui::ROW_H)
                    .into()
            });
        let preview = button(preview_content)
            .on_press(if hatch_editor_open {
                Message::HatchEditorClose
            } else {
                Message::HatchEditorOpen
            })
            .width(Fill)
            .height(COMPACT_BUTTON_HEIGHT)
            .padding(0)
            .style(move |theme: &Theme, status| button::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                    color: if hatch_editor_open
                        || matches!(status, button::Status::Hovered | button::Status::Pressed)
                    {
                        theme.palette().primary.base.color
                    } else {
                        theme.palette().background.neutral.color
                    },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
        });
        let edit = button(compact_button_content("⋮"))
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
            .padding([1, 3]);
        let edit = tooltip(edit, text("Edit hatch").size(10), tooltip::Position::Bottom);
        let edit: Element<'_, Message> = if hatch_editor_open {
            let popup = crate::ui::properties::hatch_pattern_picker_content(
                hatch_pattern_search,
                hatch_pattern_focus,
                current_pattern,
                "graphic-hatch-pattern-search",
                Message::GraphicHatchPatternSearchChanged,
                Message::GraphicHatchPatternConfirm,
                Message::GraphicHatchPatternFocus,
                Message::GraphicHatchPatternChanged,
            );
            let popup = container(popup)
                .padding(1)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().primary.base.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            iced_aw::DropDown::new(edit, popup, true)
                .width(Length::Fixed(352.0))
                .height(Length::Fixed(724.0))
                .alignment(popup_alignment(side))
                .offset(4.0)
                .on_dismiss(Message::HatchEditorClose)
                .into()
        } else {
            edit.into()
        };
        body = body.push(row![preview, edit].spacing(4).align_y(iced::Center));
    }
    if current != GraphicAttribute::None {
        if let Some(info) = transparency_info(document, &fill_handles, draw_depth) {
            body = body.push(transparency_control(
                info,
                side,
                transparency_menu_open,
                Message::ToggleFillTransparencyDropdown,
                Message::CloseFillTransparencyDropdown,
                Message::FillTransparencyChanged,
                Message::FillTransparencyByLayer,
                Message::FillTransparencyByBlock,
            ));
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
