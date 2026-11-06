//! Password Protected PDF window: asks for the password that opens an
//! encrypted PDF and then for the one that removes its restrictions, with a
//! step bar, a wrong-password band inside the window and a session switch
//! that offers the password to the other PDFs opened later.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::scene::model::pdf_password::Stage;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip, well_style};

#[derive(Debug, Clone)]
pub enum PdfPasswordMsg {
    Input(String),
    Show(bool),
    Remember(bool),
    Help,
    Ok,
}

fn msg(m: PdfPasswordMsg) -> Message {
    Message::PdfPassword(m)
}

/// What runs once the file is unlocked.
#[derive(Debug, Clone)]
pub enum PdfPasswordThen {
    /// The active command gets its file prompt's answer again; `attach` as
    /// in `CmdResult::PdfPassword`.
    Command { input: String, attach: bool },
    /// The Attach PDF Underlay dialog opens on the file.
    Attach,
    /// The Import PDF dialog opens on the file.
    ImportFile,
}

impl PdfPasswordThen {
    /// Attaching: document access is enough and cancelling says why.
    pub fn attach(&self) -> bool {
        matches!(self, Self::Attach | Self::Command { attach: true, .. })
    }
}

pub struct PdfPasswordState {
    pub path: String,
    pub stage: Stage,
    pub input: String,
    pub show: bool,
    pub remember: bool,
    /// The last password was wrong.
    pub wrong: bool,
    pub then: PdfPasswordThen,
}

impl PdfPasswordState {
    pub fn new(path: &str, stage: Stage, then: PdfPasswordThen) -> Self {
        Self {
            path: path.to_string(),
            stage,
            input: String::new(),
            show: false,
            remember: true,
            wrong: false,
            then,
        }
    }
}

fn icon<'a>(bytes: &'static [u8], size: f32, accent: bool) -> Element<'a, Message> {
    crate::ui::icons::tinted(bytes, size, move |theme: &Theme| {
        if accent {
            theme.palette().primary.base.color
        } else {
            theme.palette().background.base.text
        }
    })
}

/// The file the window is about: lock badge, name and the end of its path.
fn file_card<'a>(path: &str) -> Element<'a, Message> {
    let file = std::path::Path::new(path);
    let name = file
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    // A long path keeps its last folders, where the file is.
    let parts: Vec<_> = file.components().collect();
    let shown = if parts.len() > 3 {
        let tail: std::path::PathBuf = parts[parts.len() - 3..].iter().collect();
        format!("...{}{}", std::path::MAIN_SEPARATOR, tail.display())
    } else {
        path.to_string()
    };
    let badge = container(icon(crate::ui::icons::LOCK, 18.0, true))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(|theme: &Theme| container::Style {
            background: Some(iced::Background::Color(
                theme.palette().primary.base.color.scale_alpha(0.14),
            )),
            border: iced::Border {
                radius: 17.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    container(
        row![
            badge,
            column![
                text(name).size(13).font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..Default::default()
                }),
                text(shown).size(10).style(muted_style)
            ]
            .spacing(1)
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([10, 12])
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::card_style)
    .into()
}

/// The two steps; the open step ticked once it is passed.
fn steps<'a>(stage: Stage) -> Element<'a, Message> {
    let step = |label: String, on: bool, done: bool| {
        // The tick is an SVG: the web build's font has no check-mark glyph.
        let mut content = row![].spacing(4).align_y(iced::Center);
        if done {
            content = content.push(icon(crate::ui::icons::CHECK, 11.0, false));
        }
        container(content.push(text(label).size(11)))
            .padding([5, 6])
            .center_x(Fill)
            .style(move |theme: &Theme| {
                let style = button_style(on)(theme, button::Status::Active);
                container::Style {
                    background: style.background,
                    text_color: Some(style.text_color),
                    border: style.border,
                    ..Default::default()
                }
            })
    };
    let open = crate::tr!("pdf-password", "step-open");
    let first = match stage {
        Stage::Document => format!("1 · {open}"),
        Stage::Permission => open,
    };
    container(
        row![
            step(first, stage == Stage::Document, stage == Stage::Permission),
            step(
                format!("2 · {}", crate::tr!("pdf-password", "step-permission")),
                stage == Stage::Permission,
                false
            ),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style)
    .into()
}

fn wrong_band<'a>() -> Element<'a, Message> {
    container(
        row![
            crate::ui::icons::tinted(crate::ui::icons::ALERT, 14.0, |theme: &Theme| {
                theme.palette().danger.base.color
            }),
            text(crate::tr!("pdf-password", "wrong")).size(11),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([6, 10])
    .width(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(iced::Background::Color(
            theme.palette().danger.base.color.scale_alpha(0.18),
        )),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: theme.palette().danger.base.color.scale_alpha(0.55),
        },
        ..Default::default()
    })
    .into()
}

pub fn view<'a>(
    state: &'a PdfPasswordState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let (title, hint) = match state.stage {
        Stage::Document => (
            crate::tr!("pdf-password", "open-password"),
            crate::tr!("pdf-password", "open-hint"),
        ),
        Stage::Permission => (
            crate::tr!("pdf-password", "permission-password"),
            crate::tr!("pdf-password", "permission-hint"),
        ),
    };
    let mut input = text_input("", &state.input)
        .id(INPUT_ID)
        .secure(!state.show)
        .size(13)
        .padding([4, 8])
        .width(Fill)
        .style(field_style)
        .on_input(|v| msg(PdfPasswordMsg::Input(v)));
    if !state.input.is_empty() {
        input = input.on_submit(msg(PdfPasswordMsg::Ok));
    }
    let eye = button(icon(
        if state.show {
            crate::ui::icons::EYE_OFF
        } else {
            crate::ui::icons::EYE
        },
        15.0,
        false,
    ))
    .on_press(msg(PdfPasswordMsg::Show(!state.show)))
    .style(button_style(false))
    .padding([4, 6]);
    let mut body = column![row![
        text(crate::tr!("pdf-password", "password"))
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(70.0)),
        input,
        eye
    ]
    .spacing(6)
    .align_y(iced::Center)]
    .spacing(8);
    if state.wrong {
        body = body.push(wrong_band());
    }
    body = body.push(text(hint).size(11).style(muted_style));
    let password = card(title, body);
    let session = card(
        crate::tr!("pdf-password", "session"),
        column![
            row![chip(
                crate::tr!("pdf-password", "remember"),
                state.remember,
                msg(PdfPasswordMsg::Remember(!state.remember)),
            )],
            text(crate::tr!("pdf-password", "remember-hint"))
                .size(11)
                .style(muted_style),
        ]
        .spacing(8),
    );
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(PdfPasswordMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
        dialog_button_styled_opt(
            crate::tr!("pdf-password", "open"),
            (!state.input.is_empty()).then(|| msg(PdfPasswordMsg::Ok)),
            button::primary
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![file_card(&state.path), steps(state.stage), password, session, footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

/// The password field, focused when the window opens and after each step.
pub const INPUT_ID: &str = "pdf-password-input";
