//! PDF Text Recognition Settings (PDFSHXTEXT's SEttings): a preview of the
//! chosen font with a sample text on top; the SHX fonts to compare as a
//! numbered list of switches (dragged or moved with the arrows of the chosen
//! row, added from a file, removed); the layer the text goes on; the
//! recognition threshold and "use best matching font".

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text, text_input, toggler, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::insert::pdf_shx_text::ShxTextSettings;
use crate::scene::model::wire_model::WireModel;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card_style, well_style};

/// One edit in the PDF Text Recognition Settings dialog.
#[derive(Debug, Clone)]
pub enum ShxDialogMsg {
    Select(usize),
    Toggle(usize, bool),
    Up,
    Down,
    Remove,
    Add,
    Added(Option<std::path::PathBuf>),
    /// The drag handle of a row was pressed.
    DragStart(usize),
    /// The mouse was released over a row.
    DragDrop(usize),
    Threshold(u8),
    ThresholdText(String),
    Step(i8),
    CurrentLayer(bool),
    BestFont(bool),
    Sample(String),
    Defaults,
    Help,
    Apply,
}

fn msg(m: ShxDialogMsg) -> Message {
    Message::PdfShxDialog(m)
}

pub struct ShxSettingsState {
    pub settings: ShxTextSettings,
    pub selected: usize,
    pub threshold_text: String,
    pub sample: String,
    pub dragging: Option<usize>,
    /// The folder of the drawing, where a font may sit next to it.
    pub drawing: Option<String>,
    pub preview: Vec<WireModel>,
}

impl ShxSettingsState {
    pub fn new(settings: ShxTextSettings, drawing: Option<String>, sample: String) -> Self {
        let mut state = Self {
            threshold_text: settings.threshold.to_string(),
            settings,
            selected: 0,
            sample,
            dragging: None,
            drawing,
            preview: Vec::new(),
        };
        state.refresh_preview();
        state
    }

    /// The sample text in the chosen font.
    pub fn refresh_preview(&mut self) {
        self.preview.clear();
        let Some(font) = self.settings.fonts.get(self.selected) else {
            return;
        };
        // A font that is not available has nothing to show.
        let Some(spec) =
            crate::modules::insert::pdf_shx_text::resolve_font(&font.name, self.drawing.as_deref())
        else {
            return;
        };
        let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
            [0.0, 0.0],
            9.0,
            0.0,
            1.0,
            0.0,
            &spec,
            &self.sample,
        );
        self.preview = strokes
            .into_iter()
            .filter(|s| s.len() >= 2)
            .map(|s| {
                WireModel::solid(
                    "pdfshx_preview".into(),
                    s.into_iter().map(|p| [p[0], p[1], 0.0]).collect(),
                    [0.86, 0.86, 0.86, 1.0],
                    false,
                )
            })
            .collect();
    }

    pub fn set_threshold(&mut self, value: u8) {
        self.settings.threshold = value.clamp(1, 100);
        self.threshold_text = self.settings.threshold.to_string();
    }

    /// Moves the font at `from` to `to`, keeping it chosen.
    pub fn move_font(&mut self, from: usize, to: usize) {
        let n = self.settings.fonts.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let font = self.settings.fonts.remove(from);
        self.settings.fonts.insert(to, font);
        self.selected = to;
    }
}

fn section<'a>(
    title: String,
    aside: Option<String>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut head =
        row![text(title.to_uppercase()).size(10).style(accent_text)].align_y(iced::Center);
    if let Some(aside) = aside {
        head = head
            .push(Space::new().width(Fill))
            .push(text(aside).size(10).style(muted_style));
    }
    container(column![head, content.into()].spacing(8))
        .padding([10, 12])
        .width(Fill)
        .style(card_style)
        .into()
}

fn small_button<'a>(
    label: &'static str,
    message: ShxDialogMsg,
    danger: bool,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(11)
            .align_x(iced::Center)
            .style(move |theme: &Theme| text::Style {
                color: Some(if danger {
                    iced::Color::from_rgb8(0xff, 0x5a, 0x4f)
                } else {
                    theme.palette().background.base.text
                }),
            }),
    )
    .on_press(msg(message))
    .style(button_style(false))
    .padding([2, 8])
    .into()
}

fn font_row<'a>(state: &'a ShxSettingsState, index: usize) -> Element<'a, Message> {
    let font = &state.settings.fonts[index];
    let chosen = index == state.selected;
    let grip = mouse_area(container(text("⠿").size(12).style(muted_style)).padding([0, 2]))
        .on_press(msg(ShxDialogMsg::DragStart(index)))
        .interaction(iced::mouse::Interaction::Grab);
    let switch = toggler(font.enabled)
        .size(16.0)
        .on_toggle(move |on| msg(ShxDialogMsg::Toggle(index, on)));
    let enabled = font.enabled;
    let name = text(font.label()).size(12).style(move |theme: &Theme| {
        if enabled {
            text::Style::default()
        } else {
            muted_style(theme)
        }
    });
    let mut line = row![
        text(format!("{}", index + 1))
            .size(10)
            .style(muted_style)
            .width(Length::Fixed(16.0))
            .align_x(iced::alignment::Horizontal::Right),
        grip,
        switch,
        button(name.width(Fill))
            .on_press(msg(ShxDialogMsg::Select(index)))
            .style(button::text)
            .padding([2, 4])
            .width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if chosen {
        line = line
            .push(small_button("↑", ShxDialogMsg::Up, false))
            .push(small_button("↓", ShxDialogMsg::Down, false))
            .push(small_button("✕", ShxDialogMsg::Remove, true));
    }
    let body = container(line)
        .padding([3, 6])
        .width(Fill)
        .style(move |theme: &Theme| {
            if chosen {
                container::Style {
                    background: Some(Background::Color(
                        theme.palette().primary.base.color.scale_alpha(0.13),
                    )),
                    border: Border {
                        width: 1.0,
                        radius: 5.0.into(),
                        color: theme.palette().primary.base.color.scale_alpha(0.45),
                    },
                    ..Default::default()
                }
            } else {
                container::Style::default()
            }
        });
    mouse_area(body)
        .on_release(msg(ShxDialogMsg::DragDrop(index)))
        .into()
}

pub fn view<'a>(
    state: &'a ShxSettingsState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let s = &state.settings;
    let chosen_label = s
        .fonts
        .get(state.selected)
        .map(|f| f.label())
        .unwrap_or_default();
    let preview = container(crate::ui::window::block_palette::preview_canvas(
        &state.preview,
        86.0,
    ))
    .padding([6, 10])
    .width(Fill)
    .style(well_style);
    let sample = text_input("", &state.sample)
        .on_input(|v| msg(ShxDialogMsg::Sample(v)))
        .size(12)
        .padding([5, 8])
        .width(Length::Fixed(270.0))
        .style(field_style);
    let preview_card = container(
        column![
            row![
                text(
                    format!("{} · {}", crate::tr!("pdf-shx", "preview"), chosen_label)
                        .to_uppercase()
                )
                .size(10)
                .style(accent_text),
                Space::new().width(Fill),
                sample
            ]
            .align_y(iced::Center),
            preview
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style);

    let enabled = s.fonts.iter().filter(|f| f.enabled).count();
    let mut list = column![].spacing(2);
    for index in 0..s.fonts.len() {
        list = list.push(font_row(state, index));
    }
    let add = button(text(crate::tr!("pdf-shx", "add-font")).size(11))
        .on_press(msg(ShxDialogMsg::Add))
        .padding([5, 12])
        .style(|theme: &Theme, status| {
            let mut style = button_style(false)(theme, status);
            style.border = Border {
                width: 1.0,
                radius: 14.0.into(),
                color: theme.palette().primary.base.color,
            };
            style
        });
    let fonts_card = section(
        crate::tr!("pdf-shx", "fonts"),
        Some(crate::tr!(
            "pdf-shx",
            "enabled-count",
            on = enabled.to_string(),
            all = s.fonts.len().to_string()
        )),
        column![
            text(crate::tr!("pdf-shx", "fonts-hint"))
                .size(10)
                .style(muted_style),
            scrollable(list).height(Length::Fixed(250.0)),
            add,
        ]
        .spacing(8),
    );

    let segment = |current: bool, label: String| {
        button(text(label).size(12).width(Fill).align_x(iced::Center))
            .on_press(msg(ShxDialogMsg::CurrentLayer(current)))
            .style(button_style(s.current_layer == current))
            .padding([6, 8])
            .width(Fill)
    };
    let layer_card = section(
        crate::tr!("pdf-shx", "layer"),
        None,
        container(
            row![
                segment(true, crate::tr!("pdf-shx", "current-layer")),
                segment(false, crate::tr!("pdf-shx", "geometry-layer")),
            ]
            .spacing(2),
        )
        .padding(2)
        .width(Fill)
        .style(well_style),
    );

    let slide = iced::widget::slider(1..=100u8, s.threshold, |v| msg(ShxDialogMsg::Threshold(v)))
        .height(16.0)
        .width(Fill);
    let step = |dir: i8| {
        button(text(if dir < 0 { "\u{2212}" } else { "+" }).size(12))
            .on_press(msg(ShxDialogMsg::Step(dir)))
            .style(button_style(false))
            .padding([2, 8])
    };
    let value = text_input("", &state.threshold_text)
        .on_input(|v| msg(ShxDialogMsg::ThresholdText(v)))
        .size(11)
        .padding([3, 4])
        .width(Length::Fixed(36.0))
        .align_x(iced::alignment::Horizontal::Right)
        .style(field_style);
    let stepper = container(
        row![step(-1), value, text("%").size(11), step(1)]
            .spacing(4)
            .align_y(iced::Center),
    )
    .padding(2)
    .style(well_style);
    let recognition_card = section(
        crate::tr!("pdf-shx", "recognition"),
        None,
        column![
            text(crate::tr!("pdf-shx", "threshold"))
                .size(11)
                .style(muted_style),
            row![slide, stepper].spacing(10).align_y(iced::Center),
            row![
                toggler(s.best_font)
                    .size(16.0)
                    .on_toggle(|on| msg(ShxDialogMsg::BestFont(on))),
                text(crate::tr!("pdf-shx", "best-font"))
                    .size(12)
                    .width(Fill),
            ]
            .spacing(10)
            .align_y(iced::Center),
            text(crate::tr!("pdf-shx", "hint"))
                .size(10)
                .style(muted_style),
        ]
        .spacing(10),
    );

    let defaults = button(
        text(crate::tr!("pdf-shx", "defaults"))
            .size(11)
            .style(accent_text),
    )
    .on_press(msg(ShxDialogMsg::Defaults))
    .style(button::text)
    .padding([5, 4]);
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(ShxDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        defaults,
        Space::new().width(Fill),
        dialog_button(crate::t!("Cancel"), Message::CloseModal, false),
        dialog_button(
            std::borrow::Cow::Owned(crate::tr!("pdf-shx", "apply")),
            msg(ShxDialogMsg::Apply),
            true
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);

    column![
        preview_card,
        row![
            container(fonts_card).width(Length::FillPortion(11)),
            column![layer_card, recognition_card]
                .spacing(10)
                .width(Length::FillPortion(7)),
        ]
        .spacing(10),
        footer
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
