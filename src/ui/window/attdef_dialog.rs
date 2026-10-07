//! Attribute Definition (ATTDEF) and Edit Attribute Definition dialogs:
//! cards with accent headings, a help button on the left and the named
//! action on the right, like the other drawing dialogs.

use iced::widget::{button, checkbox, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::modules::draw::draw::attdef::{
    Justify, AFLAG_CONSTANT, AFLAG_INVISIBLE, AFLAG_LOCK, AFLAG_MULTILINE, AFLAG_PRESET,
    AFLAG_VERIFY,
};
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card_style};

/// One edit in either attribute definition dialog.
#[derive(Debug, Clone)]
pub enum AttdefDialogMsg {
    Mode(u8, bool),
    Annotative(bool),
    Tag(String),
    Prompt(String),
    Default(String),
    Justify(JustifyChoice),
    Style(String),
    Height(String),
    Rotation(String),
    Width(String),
    OnScreen(bool),
    Coord(usize, String),
    AlignBelow(bool),
    PickHeight,
    PickRotation,
    PickWidth,
    /// The ƒ button: pick a field for the Default value.
    InsertField,
    EditInsertField,
    /// Edit the default value in the multi-line text editor.
    EditValue,
    DismissError,
    Ok,
    Help,
    EditTag(String),
    EditPrompt(String),
    EditDefault(String),
    EditOk,
}

/// A justification as the dialog list shows it (translated label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JustifyChoice(pub Justify);

impl std::fmt::Display for JustifyChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.0.label()))
    }
}

/// The ATTDEF dialog's fields.
#[derive(Debug, Clone)]
pub struct AttdefDialogState {
    pub aflags: u8,
    pub annotative: bool,
    pub tag: String,
    pub prompt: String,
    pub default: String,
    pub justify: Justify,
    pub style: String,
    pub styles: Vec<String>,
    pub height: String,
    pub rotation: String,
    pub width: String,
    pub on_screen: bool,
    pub coords: [String; 3],
    pub align_below: bool,
    /// A definition was placed before in this session.
    pub can_align_below: bool,
    /// Validation message shown above the buttons.
    pub error: Option<String>,
    /// The field the Default value shows, when one was inserted.
    pub field: Option<(String, Vec<codec::Handle>)>,
}

/// The Edit Attribute Definition dialog's fields.
#[derive(Debug, Clone)]
pub struct AttdefEditState {
    pub handle: codec::Handle,
    pub tag: String,
    pub prompt: String,
    pub default: String,
    pub constant: bool,
    pub error: Option<String>,
    pub field: Option<(String, Vec<codec::Handle>)>,
    /// The Default was retyped or a field inserted: the stored field is
    /// replaced on Apply.
    pub field_changed: bool,
}

fn msg(m: AttdefDialogMsg) -> Message {
    Message::AttdefDialog(m)
}

fn card<'a>(title: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![text(title.to_uppercase()).size(10).style(accent_text), content.into()].spacing(8))
        .padding([10, 12])
        .width(Fill)
        .style(card_style)
        .into()
}

fn labelled<'a>(label: String, field: Element<'a, Message>, enabled: bool) -> Element<'a, Message> {
    let label = text(label).size(12).width(Length::Fixed(118.0));
    let label = if enabled { label } else { label.style(muted_style) };
    row![label, field].spacing(8).align_y(iced::Center).into()
}

fn input<'a>(value: &str, enabled: bool, on: fn(String) -> AttdefDialogMsg) -> Element<'a, Message> {
    let mut field = text_input("", value).size(12).padding([5, 8]).width(Fill).style(field_style);
    if enabled {
        field = field.on_input(move |v| msg(on(v)));
    }
    field.into()
}

/// The small button beside a field that takes the value from the drawing.
fn pick<'a>(
    face: Element<'a, Message>,
    tip: String,
    message: Option<AttdefDialogMsg>,
) -> Element<'a, Message> {
    let b = button(container(face).center_x(Fill))
        .style(button_style(false))
        .padding([4, 0])
        .width(Length::Fixed(28.0));
    let b = match message {
        Some(m) => b.on_press(msg(m)),
        None => b,
    };
    iced::widget::tooltip(b, text(tip).size(11), iced::widget::tooltip::Position::Bottom).into()
}

fn mode<'a>(label: String, on: bool, bit: u8) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .text_size(12)
        .size(14)
        .on_toggle(move |v| msg(AttdefDialogMsg::Mode(bit, v)))
        .into()
}

/// The validation message band, as in the Write Block dialog.
fn error_band<'a>(error: &Option<String>) -> Option<Element<'a, Message>> {
    let error = error.clone()?;
    Some(
        container(
            row![
                crate::ui::icons::semantic(ICON_WARN, 14.0),
                text(error).size(11),
                Space::new().width(Fill),
                button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 10.0))
                    .on_press(msg(AttdefDialogMsg::DismissError))
                    .style(button_style(false))
                    .padding([1, 6]),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([6, 10])
        .width(Fill)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(theme.palette().danger.base.color.scale_alpha(0.18))),
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into(),
    )
}

/// The face of the buttons that take a value from the drawing.
fn screen_pick<'a>() -> Element<'a, Message> {
    crate::ui::icons::themed(crate::ui::icons::SNAP, 14.0)
}

static ICON_WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

fn footer<'a>(action: std::borrow::Cow<'static, str>, ok: AttdefDialogMsg) -> Element<'a, Message> {
    row![
        button(text("?").size(12))
            .on_press(msg(AttdefDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(action, msg(ok), true),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(state: &'a AttdefDialogState, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let f = state.aflags;
    let constant = f & AFLAG_CONSTANT != 0;
    let multiline = f & AFLAG_MULTILINE != 0;
    let modes = card(
        t!("Mode").into_owned(),
        column![
            mode(t!("Invisible").into_owned(), f & AFLAG_INVISIBLE != 0, AFLAG_INVISIBLE),
            mode(t!("Constant").into_owned(), constant, AFLAG_CONSTANT),
            mode(t!("Verify").into_owned(), f & AFLAG_VERIFY != 0, AFLAG_VERIFY),
            mode(t!("Preset").into_owned(), f & AFLAG_PRESET != 0, AFLAG_PRESET),
            mode(t!("Lock position").into_owned(), f & AFLAG_LOCK != 0, AFLAG_LOCK),
            mode(t!("Multiple lines").into_owned(), multiline, AFLAG_MULTILINE),
        ]
        .spacing(7),
    );
    let coords_on = !state.on_screen && !state.align_below;
    let coord = |i: usize, axis: &'static str| -> Element<'a, Message> {
        let mut field = text_input("", &state.coords[i]).size(12).padding([5, 8]).width(Fill).style(field_style);
        if coords_on {
            field = field.on_input(move |v| msg(AttdefDialogMsg::Coord(i, v)));
        }
        let label = text(axis).size(12).width(Length::Fixed(14.0));
        let label = if coords_on { label } else { label.style(muted_style) };
        row![label, field].spacing(8).align_y(iced::Center).into()
    };
    let mut on_screen = checkbox(state.on_screen)
        .label(t!("Specify on-screen").into_owned())
        .text_size(12)
        .size(14);
    if !state.align_below {
        on_screen = on_screen.on_toggle(|v| msg(AttdefDialogMsg::OnScreen(v)));
    }
    let insertion = card(
        t!("Insertion Point").into_owned(),
        column![on_screen, coord(0, "X"), coord(1, "Y"), coord(2, "Z")].spacing(7),
    );
    let attribute = card(
        t!("Attribute").into_owned(),
        column![
            labelled(t!("Tag").into_owned(), input(&state.tag, true, AttdefDialogMsg::Tag), true),
            labelled(
                t!("Prompt").into_owned(),
                input(&state.prompt, !constant, AttdefDialogMsg::Prompt),
                !constant
            ),
            labelled(
                if constant { t!("Value") } else { t!("Default") }.into_owned(),
                {
                    let mut value = row![input(&state.default, true, AttdefDialogMsg::Default)].spacing(6);
                    if multiline {
                        value = value.push(pick(
                            text("...").size(13).into(),
                            t!("Multiline editor").into_owned(),
                            Some(AttdefDialogMsg::EditValue),
                        ));
                    }
                    value.push(pick(text("fx").size(12).into(), t!("Insert field").into_owned(), Some(AttdefDialogMsg::InsertField)))
                }
                .spacing(6)
                .into(),
                true
            ),
        ]
        .spacing(8),
    );
    let text_on = !state.align_below;
    let justify: Element<'a, Message> = if text_on && !multiline {
        pick_list(
            Some(JustifyChoice(state.justify)),
            Justify::ALL.map(JustifyChoice).to_vec(),
            |c: &JustifyChoice| c.to_string(),
        )
        .on_select(|c| msg(AttdefDialogMsg::Justify(c)))
        .text_size(12)
        .padding([5, 8])
        .width(Fill)
        .into()
    } else {
        container(text(JustifyChoice(state.justify).to_string()).size(12).style(muted_style))
            .padding([5, 8])
            .width(Fill)
            .into()
    };
    let style: Element<'a, Message> = if text_on {
        pick_list(Some(state.style.clone()), state.styles.clone(), |s: &String| s.clone())
            .on_select(|s| msg(AttdefDialogMsg::Style(s)))
            .text_size(12)
            .padding([5, 8])
            .width(Fill)
            .into()
    } else {
        container(text(state.style.clone()).size(12).style(muted_style)).padding([5, 8]).width(Fill).into()
    };
    let mut annotative = checkbox(state.annotative).label(t!("Annotative").into_owned()).text_size(12).size(14);
    if text_on {
        annotative = annotative.on_toggle(|v| msg(AttdefDialogMsg::Annotative(v)));
    }
    let size_on = text_on && state.justify != Justify::Align;
    let rot_on = text_on && !state.justify.two_point();
    let settings = card(
        t!("Text Settings").into_owned(),
        column![
            labelled(t!("Justification").into_owned(), justify, text_on && !multiline),
            labelled(t!("Text style").into_owned(), style, text_on),
            row![Space::new().width(Length::Fixed(126.0)), annotative],
            labelled(
                t!("Text height").into_owned(),
                row![
                    input(&state.height, size_on, AttdefDialogMsg::Height),
                    pick(screen_pick(), t!("Specify on-screen").into_owned(), size_on.then_some(AttdefDialogMsg::PickHeight)),
                ]
                .spacing(6)
                .into(),
                size_on
            ),
            labelled(
                t!("Rotation").into_owned(),
                row![
                    input(&state.rotation, rot_on, AttdefDialogMsg::Rotation),
                    pick(screen_pick(), t!("Specify on-screen").into_owned(), rot_on.then_some(AttdefDialogMsg::PickRotation)),
                ]
                .spacing(6)
                .into(),
                rot_on
            ),
            labelled(
                t!("Boundary width").into_owned(),
                row![
                    input(&state.width, text_on && multiline, AttdefDialogMsg::Width),
                    pick(
                        screen_pick(),
                        t!("Specify on-screen").into_owned(),
                        (text_on && multiline).then_some(AttdefDialogMsg::PickWidth)
                    ),
                ]
                .spacing(6)
                .into(),
                text_on && multiline
            ),
        ]
        .spacing(8),
    );
    let mut align = checkbox(state.align_below)
        .label(t!("Align below previous attribute definition").into_owned())
        .text_size(12)
        .size(14);
    if state.can_align_below {
        align = align.on_toggle(|v| msg(AttdefDialogMsg::AlignBelow(v)));
    }
    let body = row![
        column![modes, insertion].spacing(10).width(Length::Fixed(200.0)),
        column![attribute, settings].spacing(10).width(Fill),
    ]
    .spacing(10);
    let mut content = column![body, align];
    if let Some(band) = error_band(&state.error) {
        content = content.push(band);
    }
    content
        .push(footer(t!("Define"), AttdefDialogMsg::Ok))
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

pub fn view_edit<'a>(state: &'a AttdefEditState, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let field = |label: String, value: &'a str, enabled: bool, on: fn(String) -> AttdefDialogMsg| {
        let mut input = text_input("", value).size(12).padding([5, 8]).width(Fill).style(field_style);
        if enabled {
            input = input.on_input(move |v| msg(on(v)));
        }
        let label = text(label).size(12).width(Length::Fixed(80.0));
        let label = if enabled { label } else { label.style(muted_style) };
        row![label, input].spacing(8).align_y(iced::Center)
    };
    let content = card(
        t!("Attribute Definition").into_owned(),
        column![
            field(t!("Tag").into_owned(), &state.tag, true, AttdefDialogMsg::EditTag),
            field(t!("Prompt").into_owned(), &state.prompt, !state.constant, AttdefDialogMsg::EditPrompt),
            row![
                field(
                    if state.constant { t!("Value") } else { t!("Default") }.into_owned(),
                    &state.default,
                    true,
                    AttdefDialogMsg::EditDefault
                ),
                pick(text("fx").size(12).into(), t!("Insert field").into_owned(), Some(AttdefDialogMsg::EditInsertField)),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    let mut content = column![content];
    if let Some(band) = error_band(&state.error) {
        content = content.push(band);
    }
    content
        .push(footer(t!("Apply"), AttdefDialogMsg::EditOk))
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

/// The tag messages the reference shows for the dialogs.
pub fn tag_error(tag: &str) -> Option<&'static str> {
    if tag.trim().is_empty() {
        Some("The tag cannot be empty.")
    } else if tag.trim().contains(char::is_whitespace) {
        Some("The tag may not contain spaces.")
    } else {
        None
    }
}
