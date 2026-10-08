//! Drawing Units — how this drawing writes lengths and angles, and what one of
//! its units measures.
//!
//! Three separate settings that the word "units" runs together, kept in one
//! place because changing one usually means looking at the others. Length and
//! angle formats decide how numbers are *written*; the insertion unit decides
//! what they *count*, and only matters when content arrives from elsewhere.
//! Nothing here moves geometry — DWGUNITS does that.
//!
//! The sample updates as the fields change, so a format is chosen by seeing it
//! rather than by knowing what its name implies. (#668)

use std::fmt;

use iced::widget::{checkbox, column, container, row, text, text_input, Space};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::draw::units;
use crate::t;
use crate::ui::style::form::dialog_button;

/// The dialog's working copy. Nothing reaches the drawing until OK, so a format
/// can be tried against the sample and abandoned.
#[derive(Clone, PartialEq)]
pub struct State {
    /// LUNITS / LUPREC.
    pub linear_format: i16,
    pub linear_precision: i16,
    /// AUNITS / AUPREC.
    pub angular_format: i16,
    pub angular_precision: i16,
    /// ANGDIR — positive angles run clockwise.
    pub clockwise: bool,
    /// ANGBASE — the direction angle zero points, in degrees. Held as text so
    /// a half-typed number does not snap back while it is being typed.
    pub base_angle: String,
    /// INSUNITS.
    pub insertion_units: i16,
}

/// A dropdown entry that shows a translated name but hands back a raw code.
#[derive(Clone, PartialEq, Eq)]
pub struct Choice {
    pub code: i16,
    label: &'static str,
    sample: &'static str,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = crate::i18n::translate(self.label);
        if self.sample.is_empty() {
            f.write_str(name.as_ref())
        } else {
            write!(f, "{name}  ·  {}", self.sample)
        }
    }
}

/// Which field a dialog message is for.
#[derive(Clone, Debug)]
pub enum Field {
    LinearFormat(i16),
    LinearPrecision(i16),
    AngularFormat(i16),
    AngularPrecision(i16),
    Clockwise(bool),
    BaseAngle(String),
    InsertionUnits(i16),
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

fn group<'a>(title: String, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title).size(11).style(muted), body].spacing(6))
        .padding(8)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

fn drop_row<'a>(
    label: String,
    options: Vec<Choice>,
    selected: Option<Choice>,
    ctor: fn(i16) -> Field,
) -> Element<'a, Message> {
    let picker = iced::widget::pick_list(selected, options, |choice| choice.to_string())
        .on_select(move |choice| Message::DrawingUnitsField(ctor(choice.code)))
        .text_size(12)
        .padding([3, 6])
        .width(Fill);
    row![text(label).size(11).style(muted).width(78), picker]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// Precision is 0–8 places everywhere it applies; fractional and architectural
/// read the same number as a denominator power, which is why it is offered as
/// a plain count rather than as decimals.
fn precision_row<'a>(
    current: i16,
    fractional: bool,
    ctor: fn(i16) -> Field,
) -> Element<'a, Message> {
    let options: Vec<Choice> = (0..=8)
        .map(|n| Choice {
            code: n,
            label: PRECISION_LABELS[n as usize],
            sample: if fractional {
                FRACTION_SAMPLES[n as usize]
            } else {
                ""
            },
        })
        .collect();
    let selected = options
        .iter()
        .find(|choice| choice.code == current.clamp(0, 8))
        .cloned();
    drop_row(t!("Precision").into_owned(), options, selected, ctor)
}

const PRECISION_LABELS: [&str; 9] = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];
const FRACTION_SAMPLES: [&str; 9] = [
    "1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/128", "1/256",
];

pub fn view_window<'a>(
    state: &State,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let linear_options: Vec<Choice> = units::linear_formats()
        .map(|(code, label, sample)| Choice {
            code,
            label,
            sample,
        })
        .collect();
    let linear_selected = linear_options
        .iter()
        .find(|choice| choice.code == state.linear_format)
        .cloned();
    // Architectural and fractional read precision as a fraction denominator,
    // so the list shows fractions instead of decimal places for those two.
    let fractional = matches!(state.linear_format, 4 | 5);

    let angular_options: Vec<Choice> = units::angular_formats()
        .map(|(code, label, sample)| Choice {
            code,
            label,
            sample,
        })
        .collect();
    let angular_selected = angular_options
        .iter()
        .find(|choice| choice.code == state.angular_format)
        .cloned();

    let insertion_options: Vec<Choice> = units::all()
        .map(|(code, label)| Choice {
            code,
            label,
            sample: "",
        })
        .collect();
    let insertion_selected = insertion_options
        .iter()
        .find(|choice| choice.code == state.insertion_units)
        .cloned();

    let length = group(
        t!("Length").into_owned(),
        column![
            drop_row(
                t!("Type").into_owned(),
                linear_options,
                linear_selected,
                Field::LinearFormat
            ),
            precision_row(state.linear_precision, fractional, Field::LinearPrecision),
        ]
        .spacing(6)
        .into(),
    );

    let angle = group(
        t!("Angle").into_owned(),
        column![
            drop_row(
                t!("Type").into_owned(),
                angular_options,
                angular_selected,
                Field::AngularFormat
            ),
            precision_row(state.angular_precision, false, Field::AngularPrecision),
            row![
                text(t!("Zero at")).size(11).style(muted).width(78),
                text_input("0", &state.base_angle)
                    .on_input(|value| Message::DrawingUnitsField(Field::BaseAngle(value)))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(80.0)),
                text(t!("degrees")).size(11).style(muted),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                Space::new().width(Length::Fixed(78.0)),
                checkbox(state.clockwise)
                    .on_toggle(|on| Message::DrawingUnitsField(Field::Clockwise(on)))
                    .size(14),
                text(t!("Positive angles run clockwise")).size(11),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(6)
        .into(),
    );

    // What the insertion unit is for, said once here, because the setting is
    // invisible until content arrives from another drawing and is otherwise
    // easy to mistake for the one above it.
    let insertion = group(
        t!("Insertion scale").into_owned(),
        column![
            drop_row(
                t!("Unit").into_owned(),
                insertion_options,
                insertion_selected,
                Field::InsertionUnits
            ),
            text(t!(
                "Scales blocks and drawings inserted from elsewhere. Unitless inserts them unscaled."
            ))
            .size(10)
            .style(muted),
        ]
        .spacing(6)
        .into(),
    );

    let sample = group(
        t!("Sample output").into_owned(),
        column![
            text(sample_length(state)).size(12),
            text(sample_angle(state)).size(12),
        ]
        .spacing(3)
        .into(),
    );

    let actions = row![
        Space::new().width(Fill),
        dialog_button(t!("OK"), Message::DrawingUnitsApply, true),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![row![length, angle].spacing(8), insertion, sample, actions,]
        .spacing(8)
        .padding(10)
        .width(sizing.width)
        .into()
}

/// The sample is produced by the same formatters the drawing uses, so what it
/// shows is what the readout will show — not a second implementation that can
/// drift from the first.
fn sample_length(state: &State) -> String {
    with_context(state, || {
        crate::entities::common::format_length(1.5)
            + "   "
            + &crate::entities::common::format_length(33.5)
    })
}

fn sample_angle(state: &State) -> String {
    with_context(state, || {
        crate::entities::common::format_angle(std::f64::consts::FRAC_PI_4)
    })
}

/// Format under the dialog's pending settings, then put back whatever the rest
/// of the frame is drawing with. Without the restore the sample would leak its
/// settings into every widget drawn after it.
fn with_context(state: &State, body: impl FnOnce() -> String) -> String {
    use crate::entities::common::{set_unit_context, unit_context, UnitContext};
    let previous = unit_context();
    set_unit_context(UnitContext {
        lunits: state.linear_format,
        luprec: state.linear_precision,
        aunits: state.angular_format,
        auprec: state.angular_precision,
        angbase: state
            .base_angle
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0)
            .to_radians(),
        angdir_cw: state.clockwise,
    });
    let out = body();
    set_unit_context(previous);
    out
}
