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
    Drawing,
}

/// The Selection-card settings that live on `UserSettings` rather than on the
/// model-space theme.
///
/// Gathered into one value instead of four more positional parameters: the
/// function already takes seventeen, and `pick_add` / `pick_drag_rect` are two
/// adjacent `bool`s that would swap silently at the call site.
#[derive(Clone, Copy)]
pub struct SelectionPrefs {
    /// PICKBOX, 0..=50.
    pub pick_box: i32,
    /// PICKADD: true = a click adds to the selection.
    pub pick_add: bool,
    /// PICKDRAG: true = press-drag draws a rectangle instead of a lasso.
    pub pick_drag_rect: bool,
    /// GRIPOBJLIMIT, 0..=32767; 0 = no limit.
    pub grip_object_limit: i32,
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

#[allow(clippy::too_many_arguments)]
pub fn view_window<'a>(
    default_save_format: &'a str,
    file_assoc_enabled: bool,
    ui_theme: &'a UiThemeConfig,
    theme_color_inputs: &'a [String; 6],
    language: crate::i18n::Language,
    active_tab: OptionsTab,
    cursor_size: i32,
    selection: SelectionPrefs,
    double_click_block_refedit: bool,
    double_click_block_attedit: bool,
    cursor_type: CursorType,
    crosshair_color: Option<[u8; 3]>,
    crosshair_color_input: &'a str,
    lineweight_display_scale: i32,
    model_space: &'a crate::app::config::ModelSpaceThemeConfig,
    model_bg_input: &'a str,
    paper_bg_input: &'a str,
    desk_bg_input: &'a str,
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

    let model_bg_rgb = model_space.custom_bg.unwrap_or(crate::app::config::CLASSIC_CAD_DARK_BG);
    let model_bg_swatch = container(Space::new())
        .width(28)
        .height(22)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb8(
                model_bg_rgb[0],
                model_bg_rgb[1],
                model_bg_rgb[2],
            ))),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    let paper_bg_rgb = model_space.custom_paper_bg.unwrap_or(crate::app::config::DEFAULT_PAPER_BG);
    let paper_bg_swatch = container(Space::new())
        .width(28)
        .height(22)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb8(
                paper_bg_rgb[0],
                paper_bg_rgb[1],
                paper_bg_rgb[2],
            ))),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    let desk_bg_rgb = model_space.custom_desk_bg.unwrap_or(crate::app::config::DEFAULT_DESK_BG);
    let desk_bg_swatch = container(Space::new())
        .width(28)
        .height(22)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb8(
                desk_bg_rgb[0],
                desk_bg_rgb[1],
                desk_bg_rgb[2],
            ))),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    let mode_options = crate::app::config::ModelSpaceMode::ALL
        .into_iter()
        .map(|value| Labelled {
            label: crate::t!(value.label()).into_owned(),
            value,
        })
        .collect::<Vec<_>>();
    let selected_mode = mode_options
        .iter()
        .find(|choice| choice.value == model_space.mode)
        .cloned();

    let color_choice_options = [
        (0u8, "0: Theme Default"),
        (1, "1: Red"),
        (2, "2: Yellow"),
        (3, "3: Green"),
        (4, "4: Cyan"),
        (5, "5: Blue"),
        (6, "6: Magenta"),
        (7, "7: White/Black"),
    ]
    .into_iter()
    .map(|(value, label)| Labelled {
        value,
        label: crate::t!(label).into_owned(),
    })
    .collect::<Vec<_>>();

    let selected_window_color = color_choice_options
        .iter()
        .find(|c| c.value == model_space.selection_window_color)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.selection_window_color,
            label: format!("ACI {}", model_space.selection_window_color),
        });

    let selected_crossing_color = color_choice_options
        .iter()
        .find(|c| c.value == model_space.selection_crossing_color)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.selection_crossing_color,
            label: format!("ACI {}", model_space.selection_crossing_color),
        });

    let selected_highlight_color = color_choice_options
        .iter()
        .find(|c| c.value == model_space.selection_highlight_color)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.selection_highlight_color,
            label: format!("ACI {}", model_space.selection_highlight_color),
        });

    let selected_grip_color = color_choice_options
        .iter()
        .find(|c| c.value == model_space.grip_color)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.grip_color,
            label: format!("ACI {}", model_space.grip_color),
        });

    let selected_grip_hot = color_choice_options
        .iter()
        .find(|c| c.value == model_space.grip_hot)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.grip_hot,
            label: format!("ACI {}", model_space.grip_hot),
        });

    let selected_grip_hover = color_choice_options
        .iter()
        .find(|c| c.value == model_space.grip_hover)
        .cloned()
        .unwrap_or_else(|| Labelled {
            value: model_space.grip_hover,
            label: format!("ACI {}", model_space.grip_hover),
        });

    let mut display = column![
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
        row![
            text(crate::t!("Model Space Appearance")).size(15),
            Space::new().width(Fill),
            button(text(crate::t!("Restore Defaults")).size(11))
                .on_press(Message::RestoreModelSpaceDisplayDefaults)
                .padding([4, 10])
                .style(button::secondary),
        ]
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Canvas mode")).size(12).width(140),
            iced::widget::pick_list(
                selected_mode,
                mode_options,
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::ModelSpaceModeChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
    ];

    if model_space.mode == crate::app::config::ModelSpaceMode::Custom {
        display = display.push(Space::new().height(10)).push(
            row![
                text(crate::t!("Model background")).size(12).width(140),
                model_bg_swatch,
                text_input("#RRGGBB", model_bg_input)
                    .on_input(Message::ModelSpaceBgChanged)
                    .width(150),
            ]
            .spacing(10)
            .align_y(iced::Center),
        );
    }

    display = display.push(Space::new().height(10)).push(
        row![
            text(crate::t!("Paper background")).size(12).width(140),
            paper_bg_swatch,
            text_input("#RRGGBB", paper_bg_input)
                .on_input(Message::PaperSpaceBgChanged)
                .width(150),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );

    display = display.push(Space::new().height(10)).push(
        row![
            text(crate::t!("Desk surround")).size(12).width(140),
            desk_bg_swatch,
            text_input("#RRGGBB", desk_bg_input)
                .on_input(Message::DeskSpaceBgChanged)
                .width(150),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );

    display = display
        .push(Space::new().height(10))
        .push(
            row![
                text(crate::t!("Grid opacity")).size(12).width(140),
                slider(5..=100, model_space.grid_opacity.clamp(5, 100), Message::GridOpacityChanged)
                    .step(1)
                    .width(Fill),
                text(format!("{}%", model_space.grid_opacity.clamp(5, 100)))
                    .size(11)
                    .width(44),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .push(Space::new().height(6))
        .push(
            text(match model_space.mode {
                crate::app::config::ModelSpaceMode::MatchTheme => {
                    crate::t!("Canvas background, grid, and default line colors automatically adapt to the active theme.")
                }
                crate::app::config::ModelSpaceMode::ClassicDark => {
                    crate::t!("Model space canvas remains locked to classic CAD dark charcoal (#212830).")
                }
                crate::app::config::ModelSpaceMode::Custom => {
                    crate::t!("Custom background colors set above are applied to Model and Paper space.")
                }
            })
            .size(11)
            .width(sizing.width),
        )
        .push(Space::new().height(24))
        .push(text(crate::t!("Crosshair")).size(15))
        .push(Space::new().height(10))
        .push(
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
        )
        .push(Space::new().height(10))
        .push(
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
        )
        .push(Space::new().height(10))
        .push(
            row![
                text(crate::t!("Crosshair color")).size(12).width(140),
                crosshair_swatch,
                text_input(crate::t!("#RRGGBB or blank").as_ref(), crosshair_color_input)
                    .on_input(Message::CrosshairColorChanged)
                    .width(150),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .push(Space::new().height(6))
        .push(
            text(crate::t!("Leave the color blank to keep automatic viewport contrast."))
                .size(11)
                .width(sizing.width),
        )
        .push(Space::new().height(24))
        .push(text(crate::t!("Lineweight")).size(15))
        .push(Space::new().height(10))
        .push(
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
        )
        .push(Space::new().height(6))
        .push(
            text(crate::t!("Changes the on-screen width in Model without affecting plotted output."))
                .size(11)
                .width(sizing.width),
        );

    let display_element = display.spacing(0).width(sizing.width);

    let selection = column![
        text(crate::t!("Selection")).size(15),
        Space::new().height(10),
        row![
            text(crate::t!("Pick box size")).size(12).width(140),
            slider(0..=50, selection.pick_box.clamp(0, 50), Message::PickBoxChanged)
                .step(1)
                .width(Fill),
            text(selection.pick_box.clamp(0, 50).to_string())
                .size(11)
                .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(8),
        text(crate::t!(
            "Controls both the visible selection box and the click aperture."
        ))
        .size(11)
        .width(sizing.width),
        Space::new().height(24),
        text(crate::t!("Selection Modes")).size(15),
        Space::new().height(10),
        row![
            // Checked is the inverse of PICKADD: plain clicks replace and Shift adds.
            iced::widget::checkbox(!selection.pick_add)
                .on_toggle(Message::ShiftToAddToggled)
                .size(15),
            text(crate::t!("Use Shift to add to selection (PICKADD)")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            iced::widget::checkbox(selection.pick_drag_rect)
                .on_toggle(Message::PickDragRectToggled)
                .size(15),
            text(crate::t!(
                "Press and drag draws a rectangle instead of a lasso (PICKDRAG)"
            ))
            .size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(24),
        row![
            text(crate::t!("Visual Effect Settings")).size(15),
            Space::new().width(Fill),
            button(text(crate::t!("Restore Defaults")).size(11))
                .on_press(Message::RestoreSelectionVisualDefaults)
                .padding([4, 10])
                .style(button::secondary),
        ]
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            iced::widget::checkbox(model_space.selection_area)
                .on_toggle(Message::SelectionAreaToggled)
                .size(15),
            text(crate::t!("Indicate selection area with transparent fill (SELECTIONAREA)"))
                .size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Selection area opacity")).size(12).width(140),
            slider(
                0..=100,
                model_space.selection_opacity.clamp(0, 100),
                Message::SelectionOpacityChanged,
            )
            .step(1)
            .width(Fill),
            text(format!("{}%", model_space.selection_opacity.clamp(0, 100)))
                .size(11)
                .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!(
            "Transparency of the window and crossing selection areas (SELECTIONAREAOPACITY)."
        ))
        .size(11)
        .width(sizing.width),
        Space::new().height(10),
        row![
            text(crate::t!("Window selection color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_window_color),
                color_choice_options.clone(),
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::SelectionWindowColorChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Crossing selection color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_crossing_color),
                color_choice_options.clone(),
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::SelectionCrossingColorChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            iced::widget::checkbox(model_space.selection_effect)
                .on_toggle(Message::SelectionEffectToggled)
                .size(15),
            text(crate::t!("Show selection effect (SELECTIONEFFECT)")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Selection highlight color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_highlight_color),
                color_choice_options.clone(),
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::SelectionHighlightColorChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(24),
        text(crate::t!("Preview")).size(15),
        Space::new().height(10),
        row![
            // SELECTIONPREVIEW is a bitmask; bit 1 is the idle rollover and
            // bit 2 the one that runs while a command is gathering objects.
            iced::widget::checkbox(model_space.selection_preview & 1 != 0)
                .on_toggle(Message::SelectionPreviewIdleToggled)
                .size(15),
            text(crate::t!("Preview selection when no command is active")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            iced::widget::checkbox(model_space.selection_preview & 2 != 0)
                .on_toggle(Message::SelectionPreviewCommandToggled)
                .size(15),
            text(crate::t!("Preview selection during a command")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!(
            "Highlights the object under the cursor before it is picked (SELECTIONPREVIEW)."
        ))
        .size(11)
        .width(sizing.width),
        Space::new().height(24),
        text(crate::t!("Grip Settings")).size(15),
        Space::new().height(10),
        row![
            text(crate::t!("Grip size")).size(12).width(140),
            slider(
                1..=25,
                model_space.grip_size.clamp(1, 25),
                Message::GripSizeChanged,
            )
            .step(1)
            .width(Fill),
            text(format!("{} px", model_space.grip_size.clamp(1, 25)))
                .size(11)
                .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Object limit for grips")).size(12).width(140),
            // Values above the slider range remain available through SETVAR.
            slider(
                0..=1000,
                selection.grip_object_limit.clamp(0, 1000),
                Message::GripObjectLimitChanged,
            )
            .step(1)
            .width(Fill),
            text(if selection.grip_object_limit == 0 {
                crate::t!("Unlimited").into_owned()
            } else {
                selection.grip_object_limit.to_string()
            })
            .size(11)
            .width(44),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(6),
        text(crate::t!(
            "Past this many selected objects no grips are drawn; 0 removes the limit (GRIPOBJLIMIT)."
        ))
        .size(11)
        .width(sizing.width),
        Space::new().height(10),
        row![
            text(crate::t!("Unselected grip color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_grip_color),
                color_choice_options.clone(),
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::GripColorChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Selected/hot grip color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_grip_hot),
                color_choice_options.clone(),
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::GripHotChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        Space::new().height(10),
        row![
            text(crate::t!("Hover grip color")).size(12).width(140),
            iced::widget::pick_list(
                Some(selected_grip_hover),
                color_choice_options,
                |choice| choice.label.clone(),
            )
            .on_select(|choice| Message::GripHoverChanged(choice.value))
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
    ]
    .spacing(0)
    .width(sizing.width);

    let drawing = column![
        text(crate::t!("Block Edit")).size(15),
        Space::new().height(10),
        row![
            iced::widget::checkbox(double_click_block_refedit)
                .on_toggle(Message::DoubleClickBlockRefeditChanged)
                .size(15),
            text(crate::t!("Double-click to edit block in-place (REFEDIT)")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
        Space::new().height(18),
        row![
            iced::widget::checkbox(double_click_block_attedit)
                .on_toggle(Message::DoubleClickBlockAtteditChanged)
                .size(15),
            text(crate::t!("Double-click attributed blocks to edit attributes (ATTEDIT)")).size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(0)
    .width(sizing.width);

    let content: Element<'a, Message> = match active_tab {
        OptionsTab::General => general.into(),
        OptionsTab::Display => display_element.into(),
        OptionsTab::Selection => selection.into(),
        OptionsTab::Drawing => drawing.into(),
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
        tab_button(crate::t!("Drawing"), OptionsTab::Drawing),
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

    let intrinsic = sizing.width == crate::ui::modal::ModalSizing::INTRINSIC.width;
    container(body)
        .style(container::rounded_box)
        .padding([16, 18])
        .width(if intrinsic { iced::Length::Fixed(540.0) } else { sizing.width })
        .height(if intrinsic { iced::Length::Fixed(560.0) } else { sizing.height })
        .into()
}
