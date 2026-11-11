//! Annotation Object Scale window — the annotation scales the selected
//! annotative objects carry a representation for.
//!
//! The list shows every scale of the selected objects, or only the scales
//! they all share. Add opens the drawing's other scales to pick from; Delete
//! takes the selected scale off the objects. Nothing changes the drawing until
//! OK, which applies the edits as one undo step; Cancel drops them.

use crate::app::Message;
use crate::t;
use crate::ui::style::style_manager::{hdivider, muted_text_style, tb_button, tb_button_maybe};
use document::types::Handle;
use iced::widget::{column, container, mouse_area, radio, row, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Theme};

/// The dialog's staged state: nothing reaches the drawing before OK.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// List only the scales every edited object has.
    pub common_only: bool,
    /// The scale picked in the list.
    pub selected: Option<String>,
    /// Each edited object's scale names, as edited so far.
    pub staged: Vec<(Handle, Vec<String>)>,
    /// The Add list while it is open: the scales ticked in it.
    pub adding: Option<Vec<String>>,
}

impl State {
    /// The scales the list shows, in the order the objects carry them.
    pub fn listed(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (_, scales) in &self.staged {
            for name in scales {
                if !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
                    names.push(name.clone());
                }
            }
        }
        if self.common_only {
            names.retain(|name| {
                self.staged
                    .iter()
                    .all(|(_, scales)| scales.iter().any(|scale| scale.eq_ignore_ascii_case(name)))
            });
        }
        names
    }

    /// Delete is offered for a listed scale no object holds as its only one.
    pub fn can_delete(&self) -> bool {
        let Some(selected) = self.selected.as_deref() else {
            return false;
        };
        self.listed()
            .iter()
            .any(|name| name.eq_ignore_ascii_case(selected))
            && self.staged.iter().all(|(_, scales)| {
                !scales
                    .iter()
                    .any(|scale| scale.eq_ignore_ascii_case(selected))
                    || scales.len() > 1
            })
    }
}

/// `scales` is every drawing scale with its paper and drawing units; `label`
/// names the edited objects.
pub fn view_window(
    state: &State,
    label: &str,
    scales: &[(String, f64, f64)],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    if let Some(ticked) = &state.adding {
        return add_view(state, ticked, scales, sizing);
    }
    let listed = state.listed();
    let selected = state.selected.clone();
    let rows: Vec<Element<'static, Message>> = listed
        .iter()
        .map(|name| {
            let is_sel = selected
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(name));
            list_row(
                name.clone(),
                is_sel,
                Message::AnnoObjectScaleSelect(name.clone()),
            )
        })
        .collect();
    // The units line describes the selected scale, else the first listed.
    let shown = selected
        .clone()
        .filter(|name| {
            listed
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(name))
        })
        .or_else(|| listed.first().cloned());
    let units = shown
        .and_then(|name| {
            scales
                .iter()
                .find(|(scale, _, _)| scale.eq_ignore_ascii_case(&name))
                .map(|(_, paper, drawing)| units_line(*paper, *drawing))
        })
        .unwrap_or_default();

    let buttons = column![
        tb_button(t!("Add..."), Message::AnnoObjectScaleAdd, false),
        tb_button_maybe(
            t!("Delete"),
            state.can_delete().then_some(Message::AnnoObjectScaleDelete),
            false
        ),
    ]
    .spacing(6)
    .width(Length::Fixed(96.0));

    let common = state.common_only;
    let mode = column![
        radio(
            t!("List all scales of the selected objects").into_owned(),
            false,
            Some(common),
            Message::AnnoObjectScaleCommon,
        )
        .size(14)
        .text_size(11),
        radio(
            t!("List only the scales all selected objects share").into_owned(),
            true,
            Some(common),
            Message::AnnoObjectScaleCommon,
        )
        .size(14)
        .text_size(11),
    ]
    .spacing(6);

    let body = column![
        row![
            text(t!("Object: %{object_label}", object_label = label)).size(11),
            Space::new().width(Length::Fill),
        ],
        row![list_box(rows, sizing), buttons].spacing(10),
        text(units).size(11).style(muted_text_style),
        mode,
    ]
    .spacing(10)
    .padding(14);

    frame(
        body.into(),
        row![
            Space::new().width(Length::Fill),
            tb_button(t!("Cancel"), Message::CloseModal, false),
            tb_button(t!("OK"), Message::AnnoObjectScaleOk, true),
        ]
        .spacing(6)
        .into(),
        sizing,
    )
}

/// The Add list: the drawing's scales the objects do not all have yet.
fn add_view(
    state: &State,
    ticked: &[String],
    scales: &[(String, f64, f64)],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    // The drawing's scales not in the list as it shows now.
    let listed = state.listed();
    let rows: Vec<Element<'static, Message>> = scales
        .iter()
        .filter(|(name, _, _)| !listed.iter().any(|have| have.eq_ignore_ascii_case(name)))
        .map(|(name, paper, drawing)| {
            let on = ticked.iter().any(|t| t.eq_ignore_ascii_case(name));
            let line = row![
                crate::ui::icons::themed_check_cell(on),
                text(name.clone()).size(11).width(Length::Fill),
                text(ratio_label(*paper, *drawing))
                    .size(10)
                    .style(muted_text_style)
                    .width(Length::Shrink),
            ]
            .spacing(6)
            .align_y(iced::Center);
            mouse_area(container(line).padding([4, 8]).width(Length::Fill))
                .on_press(Message::AnnoObjectScaleAddToggle(name.clone()))
                .into()
        })
        .collect();
    let body = column![
        text(t!("Scales to add to the objects"))
            .size(11)
            .style(muted_text_style),
        list_box(rows, sizing),
    ]
    .spacing(8)
    .padding(14);
    frame(
        body.into(),
        row![
            Space::new().width(Length::Fill),
            tb_button(t!("Back"), Message::AnnoObjectScaleAddCancel, false),
            tb_button_maybe(
                t!("Add"),
                (!ticked.is_empty()).then_some(Message::AnnoObjectScaleAddOk),
                true
            ),
        ]
        .spacing(6)
        .into(),
        sizing,
    )
}

fn list_row(name: String, selected: bool, on_press: Message) -> Element<'static, Message> {
    let cell = container(text(name).size(11))
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |theme: &Theme| {
            let pair = theme.palette().primary.strong;
            container::Style {
                background: selected.then_some(Background::Color(pair.color)),
                text_color: selected.then_some(pair.text),
                ..Default::default()
            }
        });
    mouse_area(cell).on_press(on_press).into()
}

fn list_box(
    rows: Vec<Element<'static, Message>>,
    _sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    container(scrollable(column(rows).spacing(1)).height(Length::Fill))
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
        .width(Length::Fill)
        .height(Length::Fixed(190.0))
        .padding(2)
        .into()
}

fn frame(
    body: Element<'static, Message>,
    footer: Element<'static, Message>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    container(column![
        container(body).height(Length::Fill),
        hdivider(sizing.width),
        container(footer).padding([8, 14]),
    ])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    })
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

/// `1 paper unit = 2 drawing units`, singular for one.
pub fn units_line(paper: f64, drawing: f64) -> String {
    let side =
        |value: f64, one: std::borrow::Cow<'static, str>, many: std::borrow::Cow<'static, str>| {
            let number = number(value);
            let word = if (value - 1.0).abs() < 1e-12 {
                one
            } else {
                many
            };
            format!("{number} {word}")
        };
    format!(
        "{} = {}",
        side(paper, t!("paper unit"), t!("paper units")),
        side(drawing, t!("drawing unit"), t!("drawing units"))
    )
}

fn ratio_label(paper: f64, drawing: f64) -> String {
    format!("{}:{}", number(paper), number(drawing))
}

fn number(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}
