use crate::app::Message;
use crate::modules::draw::modify::overkill::Settings;
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};
use iced::widget::{button, checkbox, column, row, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink};

/// Properties that can be left out of the comparison, by ignore bit.
const PROPERTIES: [(&str, u16); 9] = [
    ("Color", 1),
    ("Layer", 2),
    ("Linetype", 4),
    ("Linetype scale", 8),
    ("Lineweight", 16),
    ("Thickness", 32),
    ("Transparency", 64),
    ("Plot style", 128),
    ("Material", 256),
];

#[derive(Clone, Debug)]
pub(crate) struct State {
    pub settings: Settings,
    pub tolerance: String,
}

impl State {
    pub(crate) fn new(settings: Settings) -> Self {
        Self {
            settings,
            tolerance: settings.tolerance.to_string(),
        }
    }

    /// The settings to apply, or `None` while the tolerance is not a valid
    /// non-negative number.
    pub(crate) fn parsed(&self) -> Option<Settings> {
        let tolerance = self.tolerance.trim().parse::<f64>().ok()?;
        (tolerance.is_finite() && tolerance >= 0.0).then_some(Settings {
            tolerance,
            ..self.settings
        })
    }
}

pub(crate) fn view_window(
    state: &State,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let s = state.settings;
    let field_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let toggle = |label: &'static str, on: bool, set: fn(&mut Settings, bool)| {
        checkbox(on).label(t!(label)).on_toggle(move |value| {
            let mut next = s;
            set(&mut next, value);
            Message::OverkillSettingsChanged(next)
        })
    };
    let property = |(label, bit): (&'static str, u16)| {
        checkbox(s.ignore & bit != 0)
            .label(t!(label))
            .on_toggle(move |value| {
                let mut next = s;
                next.ignore = if value {
                    s.ignore | bit
                } else {
                    s.ignore & !bit
                };
                Message::OverkillSettingsChanged(next)
            })
            .width(150)
            .into()
    };
    let mut properties = column![].spacing(6);
    for pair in PROPERTIES.chunks(2) {
        properties = properties.push(row(pair.iter().copied().map(property)).spacing(8));
    }
    let all = || Settings { ignore: 511, ..s };
    let none = || Settings { ignore: 0, ..s };

    column![
        text(t!("Object Comparison Settings")).size(13),
        row![
            text(t!("Tolerance")).size(12).width(90),
            text_input("0.000001", &state.tolerance)
                .on_input(Message::OverkillToleranceChanged)
                .padding([6, 8])
                .size(13)
                .width(field_width),
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(t!("Ignore object property")).size(12),
        properties,
        row![
            button(text(t!("Select All")).size(12))
                .on_press(Message::OverkillSettingsChanged(all())),
            button(text(t!("Clear All")).size(12))
                .on_press(Message::OverkillSettingsChanged(none())),
        ]
        .spacing(8),
        text(t!("Options")).size(13),
        toggle("Optimize segments within polylines", s.optimize, |s, v| s
            .optimize =
            v),
        toggle(
            "Combine co-linear objects that partially overlap",
            s.overlap,
            |s, v| s.overlap = v
        ),
        toggle(
            "Combine co-linear objects when aligned end to end",
            s.end_to_end,
            |s, v| s.end_to_end = v
        ),
        toggle("Maintain associative objects", s.associativity, |s, v| {
            s.associativity = v
        }),
        row![
            Space::new().width(field_width),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button_styled_opt(
                t!("OK"),
                state.parsed().is_some().then_some(Message::OverkillOk),
                button::primary,
            ),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
