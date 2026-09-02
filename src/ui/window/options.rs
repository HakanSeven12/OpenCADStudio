use crate::app::config::UiThemeConfig;
use crate::app::settings::CursorType;
use crate::app::Message;
use iced::widget::{
    button, column, container, row, scrollable, slider, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Theme};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionsTab {
    #[default]
    General,
    Display,
    Selection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Labelled<T> {
    value: T,
    label: String,
}

impl<T> fmt::Display for Labelled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn view_window<'a>(
    default_save_format: &'a str,
    file_assoc_enabled: bool,
    ui_theme: &'a UiThemeConfig,
    theme_color_inputs: &'a [String; 6],
    language: crate::i18n::Language,
    active_tab: OptionsTab,
    cursor_size: i32,
    pick_box: i32,
    cursor_type: CursorType,
    crosshair_color: Option<[u8; 3]>,
    crosshair_color_input: &'a str,
    lineweight_display_scale: i32,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let selected_format = crate::io::SAVE_FORMAT_OPTIONS
        .iter()
        .copied()
        .find(|candidate| *candidate == default_save_format);

    let theme_options = Theme::ALL
        .iter()
        .map(ToString::to_string)
        .chain(std::iter::once("Custom".to_string()))
        .map(|value| Labelled {
            label: match value.as_str() {
                "Light" => crate::t!("Light").into_owned(),
                "Dark" => crate::t!("Dark").into_owned(),
                "Custom" => crate::t!("Custom").into_owned(),
                _ => value.clone(),
            },
            value,
        })
        .collect::<Vec<_>>();
    let selected_theme = theme_options
        .iter()
        .find(|choice| choice.value == ui_theme.name)
        .cloned();

    let language_options = crate::i18n::Language::ALL
        .into_iter()
        .map(|value| Labelled {
            label: value.label(),
            value,
        })
        .collect::<Vec<_>>();
    let selected_language = language_options
        .iter()
        .find(|choice| choice.value == language)
        .cloned();

    let cursor_options = CursorType::ALL
        .into_iter()
        .map(|value: CursorType| Labelled {
            label: crate::t!(value.label()).into_owned(),
            value,
        })
        .collect::<Vec<_>>();
    let selected_cursor = cursor_options
        .iter()
        .find(|choice| choice.value == cursor_type)
        .cloned();

    let palette = ui_theme.palette.to_iced();
    let colors = [
        (crate::tr!("options", "color-background"), palette.background),
        (crate::tr!("options", "color-text"), palette.text),
        (crate::tr!("options", "color-primary"), palette.primary),
        (crate::tr!("options", "color-success"), palette.success),
        (crate::tr!("options", "color-warning"), palette.warning),
        (crate::tr!("options", "color-danger"), palette.danger),
    ];

    let mut color_controls = column![].spacing(8);
    for (index, (label, color)) in colors.into_iter().enumerate() {
        let swatch = container(Space::new())
            .width(28)
            .height(22)
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    color: theme.palette().background.strong.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });
        color_controls = color_controls.push(
            row![
                text(label).size(12).width(110),
                swatch,
                text_input("#RRGGBB", theme_color_inputs[index].as_str())
                    .on_input(move |value| Message::OptionsThemeColorChanged(index, value))
                    .width(130),
            ]
            .spacing(10)
            .align_y(iced::Center),
        );
    }

    let close = button(text(crate::tr!("action", "close")).size(12))
        .on_press(Message::CloseModal)
        .padding([6, 18])
        .style(button::secondary);

    let general = column![
        text(crate::tr!("options", "language-section")).size(15),
        Space::new().height(10),
        row![
            text(crate::tr!("options", "language-label")).size(12).width(150),
            iced::widget::pick_list(
                selected_language,
                language_options,
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::LanguageChanged(choice.value))
            .width(sizing.width),
        ]
        .spacing(12)
        .align_y(iced::Center),
        Space::new().height(22),
        text(crate::tr!("options", "open-save-section")).size(15),
        Space::new().height(10),
        row![
            text(crate::tr!("options", "default-save-format-label")).size(12).width(150),
            iced::widget::pick_list(
                selected_format,
                crate::io::SAVE_FORMAT_OPTIONS,
                |value| value.to_string(),
            )
            .on_select(|format: &str| Message::DefaultSaveFormatChanged(format.to_string()))
            .width(sizing.width),
        ]
        .spacing(12)
        .align_y(iced::Center),
        Space::new().height(8),
        text(crate::tr!("options", "default-save-format-help"))
        .size(11)
        .width(sizing.width),
        Space::new().height(14),
        row![
            iced::widget::checkbox(file_assoc_enabled)
                .on_toggle(Message::FileAssocChanged)
                .size(15),
            text(crate::t!("Open .dwg and .dxf files with Open CAD Studio"))
                .size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!(
            "Also installs the application and file-type icons the desktop shows."
        ))
        .size(11)
        .width(sizing.width),
    ]
    .spacing(0)
    .width(sizing.width);

    let crosshair_rgb = crosshair_color.unwrap_or([255, 255, 255]);
    let crosshair_swatch = container(Space::new())
        .width(28)
        .height(22)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb8(
                crosshair_rgb[0],
                crosshair_rgb[1],
                crosshair_rgb[2],
            ))),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    let display = column![
        text(crate::tr!("options", "theme-section")).size(15),
        Space::new().height(10),
        row![
            text(crate::tr!("options", "theme-label")).size(12).width(150),
            iced::widget::pick_list(
                selected_theme,
                theme_options,
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::OptionsThemeChanged(choice.value))
            .width(sizing.width),
        ]
        .spacing(12)
        .align_y(iced::Center),
        Space::new().height(8),
        text(crate::tr!("options", "theme-help"))
        .size(11)
        .width(sizing.width),
        Space::new().height(12),
        color_controls,
        Space::new().height(24),
        text(crate::t!("Crosshair")).size(15),
        Space::new().height(10),
        row![
            text(crate::t!("Crosshair size")).size(12).width(140),
            slider(1..=100, cursor_size.clamp(1, 100), Message::CursorSizeChanged)
                .step(1)
                .width(Fill),
            text(format!("{}%", cursor_size.clamp(1, 100)))
                .size(11)
                .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Cursor type")).size(12).width(140),
            iced::widget::pick_list(
                selected_cursor,
                cursor_options,
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::CursorTypeChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Crosshair color")).size(12).width(140),
            crosshair_swatch,
            text_input(crate::t!("#RRGGBB or blank").as_ref(), crosshair_color_input)
                .on_input(Message::CrosshairColorChanged)
                .width(150),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!("Leave the color blank to keep automatic viewport contrast."))
            .size(11)
            .width(sizing.width),
        Space::new().height(24),
        text(crate::t!("Lineweight")).size(15),
        Space::new().height(10),
        row![
            text(crate::t!("Model display scale")).size(12).width(140),
            slider(
                25..=200,
                lineweight_display_scale.clamp(25, 200),
                Message::LineweightDisplayScaleChanged,
            )
            .step(1)
            .width(Fill),
            text(format!("{}%", lineweight_display_scale.clamp(25, 200)))
                .size(11)
                .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!("Changes the on-screen width in Model without affecting plotted output."))
            .size(11)
            .width(sizing.width),
    ]
    .spacing(0)
    .width(sizing.width);

    let selection = column![
        text(crate::t!("Selection")).size(15),
        Space::new().height(10),
        row![
            text(crate::t!("Pick box size")).size(12).width(140),
            slider(0..=50, pick_box.clamp(0, 50), Message::PickBoxChanged)
                .step(1)
                .width(Fill),
            text(pick_box.clamp(0, 50).to_string()).size(11).width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(8),
        text(crate::t!(
            "Controls both the visible selection box and the click aperture."
        ))
        .size(11)
        .width(sizing.width),
    ]
    .spacing(0)
    .width(sizing.width);

    let content: Element<'a, Message> = match active_tab {
        OptionsTab::General => general.into(),
        OptionsTab::Display => display.into(),
        OptionsTab::Selection => selection.into(),
    };

    let tab_button = |label, tab| {
        let selected = active_tab == tab;
        button(text(label).size(12))
            .on_press(Message::OptionsTabChanged(tab))
            .padding([6, 14])
            .style(if selected { button::primary } else { button::secondary })
    };
    let tabs = row![
        tab_button(crate::t!("General"), OptionsTab::General),
        tab_button(crate::t!("Display"), OptionsTab::Display),
        tab_button(crate::t!("Selection"), OptionsTab::Selection),
    ]
    .spacing(6);

    let body = column![
        tabs,
        Space::new().height(12),
        // Keep the scrollbar in its own lane instead of floating over the
        // controls at the trailing edge of the Options content.
        scrollable(content).spacing(8).height(sizing.height),
        Space::new().height(12),
        row![Space::new().width(sizing.width), close],
    ]
    .width(sizing.width)
    .height(sizing.height);

    container(body)
        .style(container::rounded_box)
        .padding([16, 18])
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
