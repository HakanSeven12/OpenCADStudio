//! Edit Reference dialog (REFEDIT): the nesting level to check out, shown as
//! a chain of chips beside a preview of the chosen definition, how the
//! working set is filled, whether attribute definitions come along, and the
//! session settings (lock what is outside the working set, give clashing
//! named objects of an external reference unique names).

use iced::widget::{button, column, container, row, text, toggler, Space};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::scene::model::wire_model::WireModel;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button};
use crate::ui::window::pdf_dialogs::{card, well_style};

/// One edit in the Edit Reference dialog.
#[derive(Debug, Clone)]
pub enum RefEditDialogMsg {
    /// The nesting level picked in the chain (0 = the selected reference).
    Level(usize),
    /// Pick the working set's objects in the drawing (else all of them).
    Nested(bool),
    Attributes(bool),
    Lock(bool),
    Unique(bool),
    Help,
    Ok,
}

fn msg(m: RefEditDialogMsg) -> Message {
    Message::RefEditDialog(m)
}

/// One level of the nesting chain as the dialog shows it.
pub struct RefEditLevel {
    /// The reference's handle: the picked object for level 0, the
    /// definition-owned reference below it.
    pub insert: codec::Handle,
    pub name: String,
    /// The definition's preview.
    pub wires: Vec<WireModel>,
    /// The definition holds attribute definitions (the attribute switch is
    /// only offered then).
    pub has_attdefs: bool,
    /// The referenced file of an external reference.
    pub xref_path: Option<String>,
}

pub struct RefEditDialogState {
    pub top: codec::Handle,
    /// The picked chain, outermost first.
    pub chain: Vec<crate::modules::draw::modify::refedit::ChainLevel>,
    pub levels: Vec<RefEditLevel>,
    pub level: usize,
    pub nested: bool,
    pub attributes: bool,
    pub lock: bool,
    pub unique: bool,
}

impl RefEditDialogState {
    /// The attribute switch counts only where the chosen definition has
    /// attribute definitions.
    pub fn attributes_enabled(&self) -> bool {
        self.levels.get(self.level).is_some_and(|l| l.has_attdefs)
    }
}

fn label<'a>(value: String, enabled: bool) -> Element<'a, Message> {
    text(value)
        .size(12)
        .style(move |theme: &Theme| {
            if enabled {
                text::Style::default()
            } else {
                text::Style {
                    color: Some(theme.palette().background.strong.color),
                }
            }
        })
        .width(Fill)
        .into()
}

/// A labelled switch on one row; inactive while `on_toggle` is None.
fn switch_row<'a>(
    name: String,
    value: bool,
    on_toggle: Option<fn(bool) -> RefEditDialogMsg>,
) -> Element<'a, Message> {
    let enabled = on_toggle.is_some();
    let mut switch = toggler(value).size(18.0);
    if let Some(on) = on_toggle {
        switch = switch.on_toggle(move |v| msg(on(v)));
    }
    row![label(name, enabled), switch]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// A level of the chain as a pill; the chosen one framed in the accent colour.
fn level_chip<'a>(name: &str, index: usize, chosen: bool) -> Element<'a, Message> {
    button(text(name.to_string()).size(12))
        .on_press(msg(RefEditDialogMsg::Level(index)))
        .padding([5, 12])
        .style(move |theme: &Theme, status| {
            let mut style = button_style(false)(theme, status);
            style.border = Border {
                width: if chosen { 1.5 } else { 1.0 },
                radius: 14.0.into(),
                color: if chosen {
                    theme.palette().primary.base.color
                } else {
                    theme.palette().background.strong.color
                },
            };
            if chosen {
                style.text_color = theme.palette().primary.base.color;
            }
            style
        })
        .into()
}

pub fn view<'a>(
    state: &'a RefEditDialogState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let current = state.levels.get(state.level);
    let preview: Element<'a, Message> = container(match current {
        Some(level) => crate::ui::window::block_palette::preview_canvas(&level.wires, 104.0),
        None => Space::new().into(),
    })
    .width(Length::Fixed(150.0))
    .height(Length::Fixed(104.0))
    .padding(4)
    .style(well_style)
    .into();
    let mut chips = row![].spacing(6).align_y(iced::Center);
    for (index, level) in state.levels.iter().enumerate() {
        if index > 0 {
            chips = chips.push(text("›").size(13).style(muted_style));
        }
        chips = chips.push(level_chip(&level.name, index, index == state.level));
    }
    let mut about = column![text(crate::tr!("ref-edit", "dialog-chain-hint"))
        .size(11)
        .style(muted_style)]
    .spacing(4);
    if let Some(path) = current.and_then(|l| l.xref_path.as_deref()) {
        // A long path keeps its end, where the file name is.
        const MAX: usize = 64;
        let count = path.chars().count();
        let shown = if count > MAX {
            format!(
                "…{}",
                path.chars().skip(count - (MAX - 1)).collect::<String>()
            )
        } else {
            path.to_string()
        };
        about = about.push(
            text(format!(
                "{} {}",
                crate::tr!("ref-edit", "dialog-path"),
                shown
            ))
            .size(11)
            .style(muted_style),
        );
    }
    let reference = card(
        crate::tr!("ref-edit", "dialog-reference"),
        row![
            preview,
            column![
                iced::widget::scrollable(chips).direction(
                    iced::widget::scrollable::Direction::Horizontal(
                        iced::widget::scrollable::Scrollbar::new()
                            .width(4)
                            .scroller_width(4)
                    )
                ),
                about
            ]
            .spacing(8)
            .width(Fill)
        ]
        .spacing(12),
    );

    let segment = |nested: bool, name: String| {
        button(text(name).size(12).width(Fill).align_x(iced::Center))
            .on_press(msg(RefEditDialogMsg::Nested(nested)))
            .style(button_style(state.nested == nested))
            .padding([6, 8])
            .width(Fill)
    };
    let method = container(
        row![
            segment(false, crate::tr!("ref-edit", "dialog-all-objects")),
            segment(true, crate::tr!("ref-edit", "dialog-pick-objects")),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style);
    let attributes_enabled = state.attributes_enabled();
    let working_set = card(
        crate::tr!("ref-edit", "dialog-working-set"),
        column![
            method,
            switch_row(
                crate::tr!("ref-edit", "dialog-attributes"),
                state.attributes && attributes_enabled,
                attributes_enabled.then_some(RefEditDialogMsg::Attributes as fn(bool) -> _),
            ),
        ]
        .spacing(10),
    );
    let session = card(
        crate::tr!("ref-edit", "dialog-session"),
        column![
            switch_row(
                crate::tr!("ref-edit", "dialog-lock"),
                state.lock,
                Some(RefEditDialogMsg::Lock),
            ),
            switch_row(
                crate::tr!("ref-edit", "dialog-unique"),
                state.unique,
                Some(RefEditDialogMsg::Unique),
            ),
        ]
        .spacing(10),
    );
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(RefEditDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(crate::t!("Cancel"), Message::CloseModal, false),
        dialog_button(
            std::borrow::Cow::Owned(crate::tr!("ref-edit", "dialog-start")),
            msg(RefEditDialogMsg::Ok),
            true
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![reference, working_set, session, footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
