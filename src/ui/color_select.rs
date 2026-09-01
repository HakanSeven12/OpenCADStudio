//! Shared colour selector: a dropdown-style button that opens a list of named
//! colours (each shown with its swatch) plus the full ACI palette. Used by the
//! properties panel and every style editor so colour selection looks and
//! behaves the same everywhere.

use crate::app::Message;
use crate::ui::properties::acad_color_display;
use acadrust::types::Color as AcadColor;
use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Theme};
use crate::t;

/// Which "logical" entries the colour list offers besides the standard ACI
/// colours.
#[derive(Clone, Copy, Default)]
pub struct ColorExtras {
    pub by_layer: bool,
    pub by_block: bool,
    pub none: bool,
    pub background: bool,
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

/// Convert an Iced colour chosen by `iced_aw::ColorPicker` into a DWG true
/// colour. ACI-only destinations map it to their closest indexed colour later.
pub fn iced_to_acad_color(color: Color) -> AcadColor {
    let [r, g, b, _] = color.into_rgba8();
    AcadColor::Rgb { r, g, b }
}

/// Return the closest CAD Color Index for an RGB colour.
pub fn nearest_aci(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 7;
    let mut best_distance = u32::MAX;

    for index in 1..=255 {
        let Some((ar, ag, ab)) = acadrust::types::aci_table::aci_to_rgb(index) else {
            continue;
        };
        let dr = i32::from(r) - i32::from(ar);
        let dg = i32::from(g) - i32::from(ag);
        let db = i32::from(b) - i32::from(ab);
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }

    best
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
/// ByBlock, otherwise the "R,G,B" values (0-255) so unnamed palette colours
/// read meaningfully.
pub fn color_display_name(c: AcadColor) -> String {
    let (_, label) = acad_color_display(c);
    if label == "Index" || label == "Custom" {
        match c {
            AcadColor::Index(i) => {
                let (r, g, b) =
                    acadrust::types::aci_table::aci_to_rgb(i).unwrap_or((128, 128, 128));
                format!("{r},{g},{b}")
            }
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
    color_selector_with_name(
        current,
        None,
        open,
        extras,
        on_select,
        on_toggle,
        on_more,
    )
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
    let on_dismiss = on_toggle.clone();

    // Closed button: current swatch + name + caret.
    let head = button(
        row![
            swatch(cur_bg),
            text(cur_name).size(11),
            iced::widget::Space::new().width(Length::Fill),
            crate::ui::icons::themed_arrow_toggle(open, 9.0),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(on_toggle)
    .padding([3, 6])
    .width(Length::Fill);

    if !open {
        return head.into();
    }

    let popup = container(color_list(extras, on_select, on_more))
        .style(|theme: &Theme| {
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
        })
        .padding(2);

    // `DropDown` keeps the popup outside the surrounding form layout and
    // handles viewport placement, Escape, and outside-click dismissal.
    // By omitting `.width(...)`, DropDown defaults to the exact pixel width
    // of the underlay (`head`), matching the value column width and aligning flush.
    iced_aw::DropDown::new(head, popup, true)
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(on_dismiss)
        .into()
}

fn list_row_style(theme: &Theme, status: button::Status) -> button::Style {
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
    let mut dd = iced_aw::DropDown::new(base, popup, true)
        .height(popup_height)
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(on_dismiss);
    if let Some(w) = popup_width {
        dd = dd.width(w);
    }
    dd.into()
}

/// Full CAD indexed-colour page used by the standalone Select Color dialog.
///
/// This only edits the pending colour. The caller commits it with
/// `Message::ColorWindowPick`, so choosing a swatch does not immediately close
/// the dialog.
pub fn index_color_page<'a>(
    current: AcadColor,
    recent_colors: &'a [AcadColor],
) -> Element<'a, Message> {
    let swatch_button = |color: AcadColor, size: f32| -> Element<'a, Message> {
        let (bg, _) = acad_color_display(color);
        let selected = current == color;

        button(
            text("")
                .width(Length::Fixed(size))
                .height(Length::Fixed(size)),
        )
        .on_press(Message::ColorPickerColorChanged(color))
        .style(move |theme: &Theme, status| {
            let hovered = matches!(
                status,
                button::Status::Hovered | button::Status::Pressed
            );

            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    color: if selected || hovered {
                        theme.palette().primary.base.color
                    } else {
                        theme.palette().background.neutral.color
                    },
                    width: if selected {
                        2.5
                    } else if hovered {
                        1.5
                    } else {
                        1.0
                    },
                    radius: 1.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(0)
        .into()
    };

    // The first standard indexed CAD colours.
    let mut standard = row![].spacing(4);
    for idx in 1u8..=9 {
        standard = standard.push(swatch_button(AcadColor::Index(idx), 22.0));
    }

    // ACI 10-249.
    //
    // Each group of ten indices belongs together. Arrange the 24 colour
    // families horizontally and their ten shade/intensity variants vertically.
    // This makes the palette read as a colour spectrum instead of a raw numeric
    // sequence.
    let mut palette_rows = column![].spacing(2);

    for variant in 0u8..10 {
        let mut palette_row = row![].spacing(2);

        for family in 1u8..=24 {
            let idx = family * 10 + variant;
            palette_row = palette_row.push(swatch_button(AcadColor::Index(idx), 16.0));
        }

        palette_rows = palette_rows.push(palette_row);
    }

    // ACI 250-255 are the grayscale entries and are shown separately.
    let mut grayscale = row![].spacing(4);

    for idx in 250u8..=255 {
        grayscale =
            grayscale.push(swatch_button(AcadColor::Index(idx), 22.0));
    }

    let recent: Element<'a, Message> = if recent_colors.is_empty() {
        text("No recent colors yet")
            .size(10)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(
                    theme
                        .palette()
                        .background
                        .base
                        .text
                        .scale_alpha(0.55),
                ),
            })
            .into()
    } else {
        let mut recent_row = row![].spacing(4);

        for &color in recent_colors.iter().take(12) {
            recent_row = recent_row.push(swatch_button(color, 22.0));
        }

        recent_row.into()
    };

    let selected_text = match current {
        AcadColor::Index(i) => {
            let (r, g, b) =
                acadrust::types::aci_table::aci_to_rgb(i).unwrap_or((128, 128, 128));
            format!("ACI {i}    RGB {r}, {g}, {b}")
        }
        AcadColor::Rgb { r, g, b } => {
            format!("True Color    RGB {r}, {g}, {b}")
        }
        AcadColor::ByLayer => "ByLayer".to_string(),
        AcadColor::ByBlock => "ByBlock".to_string(),
        AcadColor::None => "None".to_string(),
    };

    let tabs = row![
        button(text("Index Color").size(12))
            .style(button::primary)
            .padding([5, 14]),
        button(text("True Color").size(12))
            .on_press(Message::ColorPickerTabChanged(
                crate::app::ColorPickerTab::TrueColor
            ))
            .style(button::secondary)
            .padding([5, 14]),
    ]
    .spacing(4);

    let actions = container(
        row![
            button(text("Cancel").size(11))
                .on_press(Message::CloseColorPicker)
                .style(button::secondary)
                .padding([5, 14]),
            button(text("OK").size(11))
                .on_press(Message::ColorWindowPick(current))
                .style(button::primary)
                .padding([5, 18]),
        ]
        .spacing(8),
    )
    .width(Length::Fill)
    .align_x(iced::alignment::Horizontal::Right);

    container(
        column![
            tabs,
            text("Standard colors").size(11),
            standard,
            text("CAD Color Index (ACI) 10–249").size(11),

            text("Color family  →    ·    Shade / intensity  ↓")
                .size(9)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
                }),

            container(palette_rows)
                .padding(6)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        theme.palette().background.weakest.color
                    )),
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),

            text("Grayscale 250–255").size(11),

            grayscale,

            text("Recently used").size(11),

            recent,

            text(selected_text).size(11),

            actions,
        ]
        .spacing(9),
    )
    .padding(10)
    .width(Length::Fixed(470.0))
    .into()
}
