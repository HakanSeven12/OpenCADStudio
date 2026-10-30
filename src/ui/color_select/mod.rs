//! Shared colour selector: a dropdown-style button that opens a list of named
//! colours (each shown with its swatch) plus the full ACI palette. Used by the
//! properties panel and every style editor so colour selection looks and
//! behaves the same everywhere.

pub mod book;
pub mod document;
pub mod editor;
pub mod eyedropper;
pub mod math;
pub mod picker;
pub mod widgets;

pub use math::nearest_aci;
pub use picker::{ColorPicker, ColorPickerMsg, Outcome};

use crate::app::Message;
use crate::t;
use crate::ui::properties::acad_color_display;
use crate::ui::ROW_H;
use codec::types::Color as AcadColor;
use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Theme};

/// Which "logical" entries the colour list offers besides the standard ACI
/// colours.
#[derive(Clone, Copy, Default)]
pub struct ColorExtras {
    pub by_layer: bool,
    pub by_block: bool,
    pub none: bool,
    pub background: bool,
}

thread_local! {
    static RECENT: std::cell::RefCell<Vec<AcadColor>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The active drawing's recently picked colours, newest first. The view
/// seeds it so every colour list offers them.
pub fn set_recent_colors(colors: &[AcadColor]) {
    RECENT.with(|recent| {
        let mut recent = recent.borrow_mut();
        if recent.as_slice() != colors {
            *recent = colors.to_vec();
        }
    });
}

/// Encode a colour as the ACI integer string the style editors store
/// (ByBlock=0, ByLayer=256, indexed 1-255). True colours are mapped to the
/// closest ACI entry because these fields cannot store RGB values.
pub fn color_to_aci_string(c: AcadColor) -> String {
    match c {
        AcadColor::ByBlock => "0".to_string(),
        AcadColor::ByLayer => "256".to_string(),
        AcadColor::None => "257".to_string(),
        AcadColor::Index(i) => i.to_string(),
        AcadColor::Rgb { r, g, b } => nearest_aci(r, g, b).to_string(),
    }
}

/// Decode an ACI integer string back into an `AcadColor`.
pub fn aci_string_to_color(s: &str) -> AcadColor {
    match s.trim().parse::<i16>().unwrap_or(256) {
        0 => AcadColor::ByBlock,
        256 => AcadColor::ByLayer,
        257 => AcadColor::None,
        n if (1..=255).contains(&n) => AcadColor::Index(n as u8),
        _ => AcadColor::ByLayer,
    }
}

/// Display name for a colour: the standard name for ACI 1-9 / ByLayer /
/// ByBlock, the index for any other palette colour and "R,G,B" for a true
/// colour, so the two kinds read apart (#1748).
pub fn color_display_name(c: AcadColor) -> String {
    let (_, label) = acad_color_display(c);
    if label == "Index" || label == "Custom" {
        match c {
            AcadColor::Index(i) => i.to_string(),
            AcadColor::Rgb { r, g, b } => format!("{r},{g},{b}"),
            _ => label.to_string(),
        }
    } else {
        t!(label).into_owned()
    }
}

/// A small colour square.
pub fn swatch<'a>(bg: Color) -> Element<'a, Message> {
    container(text("").width(13).height(13))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .width(13)
        .height(13)
        .into()
}

/// Build a colour selector.
///
/// * `current` — the currently selected colour (shown on the button).
/// * `open` — whether the colour list / palette is expanded.
/// * `extras` — whether ByLayer / ByBlock appear in the list.
/// * `on_select` — called with the chosen colour.
/// * `on_toggle` — opens / closes the list.
pub fn color_selector<'a>(
    current: AcadColor,
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    color_selector_with_name(current, None, open, extras, on_select, on_toggle, on_more)
}

pub fn color_selector_with_name<'a>(
    current: AcadColor,
    display_name: Option<&'a str>,
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    let (cur_bg, _) = acad_color_display(current);
    let cur_name = display_name
        .map(str::to_string)
        .unwrap_or_else(|| color_display_name(current));

    color_selector_with_indicator(
        swatch(cur_bg),
        cur_name,
        open,
        extras,
        on_select,
        on_toggle,
        on_more,
    )
}

/// Build the shared colour selector for a mixed selection.
pub fn color_selector_varies<'a>(
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    let indicator = container(text(crate::t!("?")).size(10))
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.strong.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                text_color: Some(palette.background.strong.text),
                ..Default::default()
            }
        })
        .width(13)
        .height(13)
        .align_x(iced::Center)
        .align_y(iced::Center);

    color_selector_with_indicator(
        indicator.into(),
        "*VARIES*".to_string(),
        open,
        extras,
        on_select,
        on_toggle,
        on_more,
    )
}

fn color_selector_with_indicator<'a>(
    indicator: Element<'a, Message>,
    name: String,
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    let on_dismiss = on_toggle.clone();

    // Closed button: current swatch + name + caret.
    let head = button(
        row![
            indicator,
            text(name).size(11),
            iced::widget::Space::new().width(Length::Fill),
            crate::ui::icons::themed_arrow_toggle(open, 9.0),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(on_toggle)
    .padding([3, 6])
    .height(ROW_H)
    .width(Length::Fill);

    if !open {
        return head.into();
    }

    let popup = container(color_list(extras, on_select, on_more))
        .style(popup_panel_style)
        .padding(2);

    // `DropDown` keeps the popup outside the surrounding form layout and
    // handles viewport placement, Escape, and outside-click dismissal.
    // By omitting `.width(...)`, DropDown defaults to the exact pixel width
    // of the underlay (`head`), matching the value column width and aligning flush.
    // Inside a scrollable (Layer Manager rows) the list needs the real
    // visible area to flip above a field near the bottom edge.
    crate::ui::drop_down_fit::drop_down_fit(
        iced_aw::DropDown::new(head, popup, true)
            .alignment(iced_aw::drop_down::Alignment::Bottom)
            .offset(2.0)
            .on_dismiss(on_dismiss),
    )
}

pub(crate) fn popup_panel_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

pub(crate) fn list_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let hovered = matches!(status, button::Status::Hovered);
    let text_color = if hovered {
        palette.background.strong.text
    } else {
        palette.background.base.text
    };
    button::Style {
        background: hovered.then_some(Background::Color(palette.background.strong.color)),
        text_color,
        ..Default::default()
    }
}

/// The colour list shown inside a picker popup: named ACI colours (with
/// swatches) plus a "Select Color..." entry that opens the full palette window.
/// Shared by `color_selector` and the ribbon's colour overlay.
pub fn color_list<'a>(
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_more: Message,
) -> Element<'a, Message> {
    let named_row = |color: AcadColor| -> Element<'a, Message> {
        let (bg, name) = acad_color_display(color);
        button(
            row![swatch(bg), text(t!(name)).size(11)]
                .spacing(5)
                .align_y(iced::Center),
        )
        .on_press(on_select(color))
        .style(list_row_style)
        .padding([2, 4])
        .width(Length::Fill)
        .into()
    };

    let logical_row = |color: AcadColor, label: &'static str| -> Element<'a, Message> {
        let (bg, _) = acad_color_display(color);
        button(
            row![swatch(bg), text(t!(label)).size(11)]
                .spacing(5)
                .align_y(iced::Center),
        )
        .on_press(on_select(color))
        .style(list_row_style)
        .padding([2, 4])
        .width(Length::Fill)
        .into()
    };

    let mut list = column![].spacing(1);
    if extras.none {
        list = list.push(named_row(AcadColor::None));
    }
    if extras.background {
        list = list.push(logical_row(AcadColor::ByBlock, "Background"));
    }
    if extras.by_layer {
        list = list.push(named_row(AcadColor::ByLayer));
    }
    if extras.by_block {
        list = list.push(named_row(AcadColor::ByBlock));
    }
    for i in 1u8..=9 {
        list = list.push(named_row(AcadColor::Index(i)));
    }
    // Colours picked recently beyond the nine standard ones.
    let recent = RECENT.with(|recent| recent.borrow().clone());
    for color in recent
        .into_iter()
        .filter(|color| !matches!(color, AcadColor::Index(1..=9)))
    {
        let (bg, _) = acad_color_display(color);
        list = list.push(
            button(
                row![swatch(bg), text(color_display_name(color)).size(11)]
                    .spacing(5)
                    .align_y(iced::Center),
            )
            .on_press(on_select(color))
            .style(list_row_style)
            .padding([2, 4])
            .width(Length::Fill),
        );
    }
    list = list.push(
        button(text(t!("Select Color...")).size(11))
            .on_press(on_more)
            .style(list_row_style)
            .padding([2, 4])
            .width(Length::Fill),
    );
    list.into()
}

/// Render `base` inline with `popup` in an `iced_aw` dropdown.
pub fn drop_down_below<'a>(
    base: Element<'a, Message>,
    popup: Element<'a, Message>,
    popup_width: Option<Length>,
    popup_height: Length,
    on_dismiss: Message,
) -> Element<'a, Message> {
    editable_drop_down_below(base, popup, popup_width, popup_height, on_dismiss, true)
}

/// Keep the same widget tree while toggling an editable dropdown, preserving
/// its text input's focus, cursor and selection.
pub fn editable_drop_down_below<'a>(
    base: Element<'a, Message>,
    popup: Element<'a, Message>,
    popup_width: Option<Length>,
    popup_height: Length,
    on_dismiss: Message,
    expanded: bool,
) -> Element<'a, Message> {
    let mut dd = iced_aw::DropDown::new(base, popup, expanded)
        .height(popup_height)
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(on_dismiss);
    if let Some(w) = popup_width {
        dd = dd.width(w);
    }
    crate::ui::drop_down_fit::drop_down_fit(dd)
}
