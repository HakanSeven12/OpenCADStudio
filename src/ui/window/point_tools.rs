//! The windows of Create COGO Points from Corridor, Create Blocks from COGO
//! Points and Replace Legacy Point Blocks: what each converts and how a
//! point number already in use is settled.

use iced::widget::{Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{card, chip};

/// Create COGO Points from Corridor: the corridor's baselines and point
/// codes.
#[derive(Debug, Clone)]
pub struct CorridorPointsWindow {
    pub corridor: document::Handle,
    /// Baseline names and their station ranges.
    pub baselines: Vec<(String, f64, f64)>,
    pub baseline: usize,
    /// The user specified range is on (otherwise the entire corridor).
    pub user_range: bool,
    pub start: String,
    pub end: String,
    pub group_name: String,
    /// Codes and whether each is picked.
    pub codes: Vec<(String, bool)>,
    pub error: Option<String>,
}

/// The spatial filter of Create Blocks from COGO Points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spatial {
    None,
    CurrentDisplay,
    Window,
}

/// Create Blocks from COGO Points.
#[derive(Debug, Clone)]
pub struct PointBlocksWindow {
    pub spatial: Spatial,
    /// The window "Define window" picked: two corners.
    pub window: Option<([f64; 2], [f64; 2])>,
    /// The current display when the command started: two corners.
    pub display: Option<([f64; 2], [f64; 2])>,
    /// Point group names and whether each is selected.
    pub groups: Vec<(String, bool)>,
    /// The drawing's blocks that hold a POINT attribute definition.
    pub blocks: Vec<String>,
    pub use_existing: bool,
    pub existing: usize,
    pub new_name: String,
    pub layers: Vec<String>,
    pub layer: String,
    pub error: Option<String>,
}

/// How a point number already in use is settled (the reference's order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    AddOffset,
    Merge,
    Overwrite,
    SequenceFrom,
    UseNext,
}

impl Resolution {
    pub const ALL: [Resolution; 5] = [Resolution::AddOffset, Resolution::Merge, Resolution::Overwrite, Resolution::SequenceFrom, Resolution::UseNext];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::AddOffset => "Add an offset",
            Resolution::Merge => "Merge",
            Resolution::Overwrite => "Overwrite",
            Resolution::SequenceFrom => "Sequence from",
            Resolution::UseNext => "Use next point number",
        }
    }
}

/// Duplicate Point Number: asked for a legacy point whose number is taken.
#[derive(Debug, Clone)]
pub struct DuplicatePointWindow {
    pub number: u32,
    pub resolution: Resolution,
    pub offset: String,
    pub sequence: String,
    pub apply_all: bool,
}

#[derive(Debug, Clone)]
pub enum PointToolMsg {
    // Create COGO Points from Corridor
    UserRange(bool),
    Baseline(String),
    Start(String),
    End(String),
    GroupName(String),
    Code(usize, bool),
    CorridorOk,
    // Create Blocks from COGO Points
    Spatial(Spatial),
    PickWindow,
    BlockGroup(usize, bool),
    UseExisting(bool),
    Existing(String),
    NewName(String),
    Layer(String),
    BlocksOk,
    // Point Blocks with Duplicate Point Numbers Found
    BlockSkip,
    BlockOverwrite,
    // Duplicate Point Number
    Resolution(String),
    Offset(String),
    Sequence(String),
    ApplyAll(bool),
    DuplicateOk,
    DismissError,
    Help(&'static str),
}

fn msg(m: PointToolMsg) -> Message {
    Message::PointTool(m)
}

fn small(s: impl Into<String>) -> iced::widget::Text<'static> {
    text(s.into()).size(11)
}

fn field<'a>(value: &'a str, enabled: bool, on: impl Fn(String) -> Message + 'a) -> Element<'a, Message> {
    let mut input = text_input("", value).size(11).padding([4, 8]).width(Fill).style(field_style);
    if enabled {
        input = input.on_input(on);
    }
    input.into()
}

fn labelled<'a>(label: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![small(label).style(muted_style).width(Length::Fixed(130.0)), content.into()].spacing(8).align_y(iced::Center).into()
}

fn error_band<'a>(error: &Option<String>) -> Option<Element<'a, Message>> {
    let error = error.clone()?;
    Some(
        container(
            row![
                small(error),
                Space::new().width(Fill),
                button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 10.0)).on_press(msg(PointToolMsg::DismissError)).style(button_style(false)).padding([1, 6]),
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

fn footer<'a>(help: &'static str, ok: Message) -> Element<'a, Message> {
    row![
        button(text("?").size(12)).on_press(msg(PointToolMsg::Help(help))).style(button_style(false)).padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("OK"), ok, true),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub fn view_corridor<'a>(state: &'a CorridorPointsWindow, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let names: Vec<String> = state.baselines.iter().map(|b| b.0.clone()).collect();
    let on = state.user_range;
    let range = card(
        t!("Station range").into_owned(),
        column![
            row![
                chip(t!("For entire corridor range").into_owned(), !on, msg(PointToolMsg::UserRange(false))),
                chip(t!("For user specified range").into_owned(), on, msg(PointToolMsg::UserRange(true))),
            ]
            .spacing(6),
            labelled(
                t!("Select baseline:").into_owned(),
                pick_list(state.baselines.get(state.baseline).map(|b| b.0.clone()), names, |s: &String| s.clone())
                    .on_select(|s| msg(PointToolMsg::Baseline(s)))
                    .text_size(11)
                    .padding([4, 8])
                    .width(Fill),
            ),
            labelled(t!("Baseline start:").into_owned(), field(&state.start, on, |s| msg(PointToolMsg::Start(s)))),
            labelled(t!("Baseline end:").into_owned(), field(&state.end, on, |s| msg(PointToolMsg::End(s)))),
        ]
        .spacing(8),
    );
    let mut codes = column![].spacing(4);
    for (k, (code, picked)) in state.codes.iter().enumerate() {
        codes = codes.push(checkbox(*picked).label(code.clone()).size(14).text_size(11).on_toggle(move |v| msg(PointToolMsg::Code(k, v))));
    }
    let mut col = column![
        range,
        card(t!("New point group name:").into_owned(), field(&state.group_name, true, |s| msg(PointToolMsg::GroupName(s)))),
        card(t!("Select point codes to create:").into_owned(), scrollable(codes).height(Length::Fixed(170.0))),
    ]
    .spacing(10);
    if let Some(band) = error_band(&state.error) {
        col = col.push(band);
    }
    col.push(footer("Creates a COGO point at each corridor station for the point codes picked, described by its code, and a point group holding them.", msg(PointToolMsg::CorridorOk)))
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

pub fn view_blocks<'a>(state: &'a PointBlocksWindow, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let spatial = card(
        t!("Spatial filter").into_owned(),
        row![
            chip(t!("None").into_owned(), state.spatial == Spatial::None, msg(PointToolMsg::Spatial(Spatial::None))),
            chip(t!("Current display").into_owned(), state.spatial == Spatial::CurrentDisplay, msg(PointToolMsg::Spatial(Spatial::CurrentDisplay))),
            chip(t!("Define window").into_owned(), state.spatial == Spatial::Window, msg(PointToolMsg::Spatial(Spatial::Window))),
            Space::new().width(Fill),
            button(small(t!("Pick window")))
                .on_press_maybe((state.spatial == Spatial::Window).then(|| msg(PointToolMsg::PickWindow)))
                .style(button_style(false))
                .padding([4, 10]),
        ]
        .spacing(6)
        .align_y(iced::Center),
    );
    let mut groups = column![].spacing(4);
    for (k, (name, on)) in state.groups.iter().enumerate() {
        groups = groups.push(checkbox(*on).label(name.clone()).size(14).text_size(11).on_toggle(move |v| msg(PointToolMsg::BlockGroup(k, v))));
    }
    let output = card(
        t!("Block output").into_owned(),
        column![
            row![
                chip(t!("Use existing block").into_owned(), state.use_existing, msg(PointToolMsg::UseExisting(true))),
                pick_list(state.blocks.get(state.existing).cloned(), state.blocks.clone(), |s: &String| s.clone())
                    .on_select(|s| msg(PointToolMsg::Existing(s)))
                    .text_size(11)
                    .padding([4, 8])
                    .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                chip(t!("Make new block").into_owned(), !state.use_existing, msg(PointToolMsg::UseExisting(false))),
                field(&state.new_name, !state.use_existing, |s| msg(PointToolMsg::NewName(s))),
            ]
            .spacing(8)
            .align_y(iced::Center),
            labelled(
                t!("Block layer").into_owned(),
                pick_list(Some(state.layer.clone()), state.layers.clone(), |s: &String| s.clone())
                    .on_select(|s| msg(PointToolMsg::Layer(s)))
                    .text_size(11)
                    .padding([4, 8])
                    .width(Fill),
            ),
        ]
        .spacing(8),
    );
    let mut col = column![spatial, card(t!("Selected point groups:").into_owned(), scrollable(groups).height(Length::Fixed(120.0))), output].spacing(10);
    if let Some(band) = error_band(&state.error) {
        col = col.push(band);
    }
    col.push(footer("Inserts a block at each COGO point of the groups picked (and inside the filter) with the point's number, elevation and description in its attributes.", msg(PointToolMsg::BlocksOk)))
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

/// Point Blocks with Duplicate Point Numbers Found.
pub fn view_block_duplicate<'a>(number: u32, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    column![
        small(crate::tf!("While converting COGO Points, a block with the same Point Number ({}) was found. You can choose one of the following options:", number).into_owned()),
        small(t!("Skip - keeps the existing block and ignores the new one").into_owned()),
        small(t!("Overwrite - replaces the existing block with the new one").into_owned()),
        small(t!("Cancel - stop the conversion").into_owned()),
        row![
            Space::new().width(Fill),
            dialog_button(t!("Skip"), msg(PointToolMsg::BlockSkip), true),
            dialog_button(t!("Overwrite"), msg(PointToolMsg::BlockOverwrite), false),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
        ]
        .spacing(6),
    ]
    .spacing(8)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

pub fn view_duplicate<'a>(state: &'a DuplicatePointWindow, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let labels: Vec<String> = Resolution::ALL.iter().map(|r| t!(r.label()).into_owned()).collect();
    let r = state.resolution;
    column![
        small(crate::tf!("Point number {} is already in use. Choose how to resolve the conflict.", state.number).into_owned()),
        card(
            t!("Duplicate point number resolution").into_owned(),
            column![
                labelled(
                    t!("Resolution:").into_owned(),
                    pick_list(Some(t!(r.label()).into_owned()), labels, |s: &String| s.clone())
                        .on_select(|s| msg(PointToolMsg::Resolution(s)))
                        .text_size(11)
                        .padding([4, 8])
                        .width(Fill),
                ),
                labelled(t!("Add an offset from:").into_owned(), field(&state.offset, r == Resolution::AddOffset, |s| msg(PointToolMsg::Offset(s)))),
                labelled(t!("Sequence from:").into_owned(), field(&state.sequence, r == Resolution::SequenceFrom, |s| msg(PointToolMsg::Sequence(s)))),
            ]
            .spacing(8),
        ),
        checkbox(state.apply_all).label(t!("Apply to all duplicate point numbers")).size(14).text_size(11).on_toggle(|v| msg(PointToolMsg::ApplyAll(v))),
        footer("Settles a point number already in use: add an offset to it, merge into or overwrite the existing point, number from a value, or take the next free number.", msg(PointToolMsg::DuplicateOk)),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
