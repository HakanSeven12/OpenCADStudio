//! The windows of Create COGO Points from Corridor, Create Blocks from COGO
//! Points and Replace Legacy Point Blocks: what each converts and how a
//! point number already in use is settled.

use iced::widget::{Space, button, canvas, checkbox, column, container, pick_list, row, scrollable, text, text_input};
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
    /// Baseline names, their station ranges and alignments.
    pub baselines: Vec<(String, f64, f64, u64)>,
    /// The stations each baseline gives each code a point at.
    pub stations: Vec<Vec<Vec<f64>>>,
    pub baseline: usize,
    /// The drawing's unit mark after a station (`'` or `m`).
    pub mark: &'static str,
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
    /// Pick the baseline's alignment in the drawing.
    PickBaseline,
    /// Pick the start (false) or end (true) station in the drawing.
    PickStation(bool),
    /// The range bar's start (false) or end (true) handle let go at a station.
    RangeDrag(bool, f64),
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

static PICK_ICON: &[u8] = include_bytes!("../../../assets/icons/blocks/select_objects.svg");

/// A station as typed: `0+60.00`, `0+60.00'`, `0+60.00m` or `60`.
pub fn parse_station(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_suffix(|c| c == '\'' || c == 'm').unwrap_or(s);
    s.trim().replace('+', "").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl CorridorPointsWindow {
    /// A station as the window writes it: with the drawing's unit mark.
    pub fn station_text(&self, s: f64) -> String {
        format!("{}{}", crate::modules::civil::point_alignment::format_station(s), self.mark)
    }

    /// The station range the points are taken from: the typed one, or
    /// none when the whole corridor is taken (or a station does not read).
    pub fn range(&self) -> Option<(f64, f64)> {
        if !self.user_range {
            return None;
        }
        let (a, b) = (parse_station(&self.start)?, parse_station(&self.end)?);
        Some((a.min(b), a.max(b)))
    }

    /// The station a range bar handle (`end` or start) dragged to `t` along
    /// the bar (0 its start, 1 its end) is let go at, as the bar drops it;
    /// none when the range is not the user's (the bar has no handles).
    pub fn drag_handle(&self, end: bool, t: f64) -> Option<f64> {
        let (lo, hi) = self.baselines.get(self.baseline).map_or((0.0, 0.0), |b| (b.1, b.2));
        let bar = RangeBar { lo, hi, taken: self.range().unwrap_or((lo, hi)), enabled: self.user_range };
        bar.enabled.then(|| bar.dropped(end, bar.station_along(t)))
    }

    /// The points code `c` gives: the user range's baseline within the
    /// range, otherwise every baseline.
    pub fn count(&self, c: usize) -> usize {
        let range = self.range();
        self.stations
            .iter()
            .enumerate()
            .filter(|(b, _)| !self.user_range || *b == self.baseline)
            .filter_map(|(_, codes)| codes.get(c))
            .map(|s| s.iter().filter(|s| range.is_none_or(|(a, b)| (a - 1e-9..=b + 1e-9).contains(*s))).count())
            .sum()
    }
}

fn pick_button<'a>(m: Option<Message>) -> Element<'a, Message> {
    button(container(crate::ui::icons::semantic(PICK_ICON, 15.0)).center(Length::Fixed(22.0)))
        .on_press_maybe(m)
        .style(button_style(false))
        .padding(2)
        .into()
}

/// A card with a numbered step badge and an optional count next to its title.
fn step<'a>(n: u8, title: String, count: Option<String>, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    use crate::ui::window::pdf_dialogs::{accent_text, card_style};
    let badge = container(text(n.to_string()).size(11).style(|t: &iced::Theme| iced::widget::text::Style { color: Some(t.palette().primary.base.text) }))
        .center(Length::Fixed(20.0))
        .style(|t: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(t.palette().primary.base.color)),
            border: iced::Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        });
    let mut head = row![badge, text(title.to_uppercase()).size(10).style(accent_text)].spacing(8).align_y(iced::Center);
    if let Some(c) = count {
        head = head.push(small(c).style(muted_style));
    }
    container(column![head, content.into()].spacing(10)).padding([10, 12]).width(Fill).style(card_style).into()
}

/// The baseline's station range as a bar, the stations taken filled; with a
/// specified range its start and end handles drag along it.
struct RangeBar {
    lo: f64,
    hi: f64,
    taken: (f64, f64),
    enabled: bool,
}

/// The handle being dragged (false start, true end) and where it is.
#[derive(Default)]
struct RangeDragState {
    drag: Option<(bool, f64)>,
}

const HANDLE: f32 = 6.0;

impl RangeBar {
    fn x_of(&self, s: f64, width: f32) -> f32 {
        let span = (self.hi - self.lo).max(1e-9);
        HANDLE + ((s.clamp(self.lo, self.hi) - self.lo) / span) as f32 * (width - 2.0 * HANDLE)
    }

    fn station_at(&self, x: f32, width: f32) -> f64 {
        self.station_along(f64::from((x - HANDLE) / (width - 2.0 * HANDLE).max(1.0)))
    }

    /// The station `t` (0 at the start, 1 at the end) along the bar, to 0.01.
    fn station_along(&self, t: f64) -> f64 {
        ((self.lo + t.clamp(0.0, 1.0) * (self.hi - self.lo)) * 100.0).round() / 100.0
    }

    /// Where a handle let go at `s` lands: it does not pass the other.
    fn dropped(&self, end: bool, s: f64) -> f64 {
        if end { s.max(self.taken.0) } else { s.min(self.taken.1) }
    }

    /// The taken range with a dragged handle where it is.
    fn shown(&self, state: &RangeDragState) -> (f64, f64) {
        match state.drag {
            Some((false, s)) => (s, self.taken.1),
            Some((true, s)) => (self.taken.0, s),
            None => self.taken,
        }
    }
}

impl canvas::Program<Message> for RangeBar {
    type State = RangeDragState;

    fn update(&self, state: &mut RangeDragState, event: &iced::Event, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Option<canvas::Action<Message>> {
        use iced::mouse::{Button, Event};
        if !self.enabled {
            state.drag = None;
            return None;
        }
        let x = |p: iced::Point| p.x - bounds.x;
        match event {
            iced::Event::Mouse(Event::ButtonPressed(Button::Left)) => {
                let p = cursor.position_over(bounds)?;
                // The handle nearer the press; an end handle still on its
                // start takes a press to the right of it.
                let (a, b) = (self.x_of(self.taken.0, bounds.width), self.x_of(self.taken.1, bounds.width));
                let end = (x(p) - b).abs() < (x(p) - a).abs() || (a == b && x(p) > a);
                state.drag = Some((end, self.station_at(x(p), bounds.width)));
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(Event::CursorMoved { position }) => {
                let (end, _) = state.drag?;
                state.drag = Some((end, self.dropped(end, self.station_at(x(*position), bounds.width))));
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(Event::ButtonReleased(Button::Left)) => {
                let (end, s) = state.drag.take()?;
                Some(canvas::Action::publish(msg(PointToolMsg::RangeDrag(end, self.dropped(end, s)))).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &RangeDragState, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> iced::mouse::Interaction {
        if state.drag.is_some() {
            iced::mouse::Interaction::Grabbing
        } else if self.enabled && cursor.is_over(bounds) {
            iced::mouse::Interaction::Grab
        } else {
            Default::default()
        }
    }

    fn draw(&self, state: &RangeDragState, renderer: &iced::Renderer, theme: &iced::Theme, bounds: iced::Rectangle, _cursor: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, y) = (bounds.width, bounds.height / 2.0);
        let palette = theme.palette();
        let bar = |frame: &mut canvas::Frame, a: f32, b: f32, color: iced::Color| {
            frame.fill(&canvas::Path::rounded_rectangle(iced::Point::new(a, y - 3.0), iced::Size::new((b - a).max(0.0), 6.0), 3.0.into()), color);
        };
        bar(&mut frame, HANDLE, w - HANDLE, palette.background.base.color);
        let (s, e) = self.shown(state);
        let (a, b) = (self.x_of(s, w), self.x_of(e, w));
        bar(&mut frame, a, b, palette.primary.base.color);
        if self.enabled {
            for x in [a, b] {
                frame.fill(&canvas::Path::circle(iced::Point::new(x, y), HANDLE), palette.primary.base.color);
                frame.fill(&canvas::Path::circle(iced::Point::new(x, y), HANDLE - 2.0), palette.background.base.text);
            }
        }
        vec![frame.into_geometry()]
    }
}

fn range_bar<'a>(state: &CorridorPointsWindow, lo: f64, hi: f64, taken: (f64, f64), enabled: bool) -> Element<'a, Message> {
    let bar = canvas(RangeBar { lo, hi, taken, enabled }).width(Fill).height(Length::Fixed(2.0 * HANDLE + 2.0));
    let stations = row![
        small(state.station_text(lo)).style(muted_style),
        Space::new().width(Fill),
        small(state.station_text(hi)).style(muted_style),
    ];
    column![bar, stations].spacing(4).into()
}

pub fn view_corridor<'a>(state: &'a CorridorPointsWindow, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    use crate::ui::kit::{column as list_column, column_header, list_row, number_cell, text_cell};
    let names: Vec<String> = state.baselines.iter().map(|b| b.0.clone()).collect();
    let on = state.user_range;
    let seg = |label: String, value: bool| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(PointToolMsg::UserRange(value)))
            .style(button_style(on == value))
            .padding([5, 10])
            .width(Fill)
    };
    let segmented = container(row![seg(t!("Entire corridor").into_owned(), false), seg(t!("Specified range").into_owned(), true)].spacing(2))
        .padding(2)
        .width(Length::Fixed(250.0))
        .style(crate::ui::window::pdf_dialogs::well_style);
    let baseline = pick_list(state.baselines.get(state.baseline).map(|b| b.0.clone()), names, |s: &String| s.clone())
        .on_select(|s| msg(PointToolMsg::Baseline(s)))
        .text_size(11)
        .padding([4, 8])
        .width(Length::Fixed(140.0));
    let (lo, hi) = state.baselines.get(state.baseline).map_or((0.0, 0.0), |b| (b.1, b.2));
    let taken = state.range().unwrap_or((lo, hi));
    let picks = |m: PointToolMsg| on.then(|| msg(m));
    let range = step(
        1,
        t!("Range").into_owned(),
        None,
        column![
            row![
                segmented,
                Space::new().width(Fill),
                small(t!("Baseline")).style(muted_style),
                baseline,
                pick_button(picks(PointToolMsg::PickBaseline)),
            ]
            .spacing(8)
            .align_y(iced::Center),
            range_bar(state, lo, hi, taken, on),
            row![
                small(t!("Start station")).style(muted_style),
                container(field(&state.start, on, |s| msg(PointToolMsg::Start(s)))).width(Length::Fixed(130.0)),
                pick_button(picks(PointToolMsg::PickStation(false))),
                Space::new().width(Fill),
                small(t!("End station")).style(muted_style),
                container(field(&state.end, on, |s| msg(PointToolMsg::End(s)))).width(Length::Fixed(130.0)),
                pick_button(picks(PointToolMsg::PickStation(true))),
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(10),
    );
    let picked = state.codes.iter().filter(|c| c.1).count();
    let header = column_header(vec![
        list_column(t!("Code").into_owned(), Length::Fill),
        list_column(t!("Points").into_owned(), Length::Fixed(90.0)).end(),
        list_column(String::new(), Length::Fixed(40.0)),
    ]);
    let mut rows = column![];
    for (k, (code, on)) in state.codes.iter().enumerate() {
        let tick = container(checkbox(*on).size(14).on_toggle(move |v| msg(PointToolMsg::Code(k, v)))).center_x(Fill);
        rows = rows.push(
            list_row(vec![
                (text_cell(code.clone(), false), Length::Fill),
                (number_cell(state.count(k).to_string()), Length::Fixed(90.0)),
                (tick.into(), Length::Fixed(40.0)),
            ])
            .on_press(msg(PointToolMsg::Code(k, !on)))
            .view(),
        );
    }
    let codes = step(
        2,
        t!("Point codes").into_owned(),
        Some(t!("%{n} picked", n = picked).into_owned()),
        container(column![header, scrollable(rows).height(Length::Fixed(130.0))]).width(Fill).style(crate::ui::window::pdf_dialogs::well_style),
    );
    let total: usize = (0..state.codes.len()).filter(|k| state.codes[*k].1).map(|k| state.count(k)).sum();
    let group = step(
        3,
        t!("Point group").into_owned(),
        None,
        row![field(&state.group_name, true, |s| msg(PointToolMsg::GroupName(s))), small(t!("%{n} points will be added", n = total)).style(muted_style)]
            .spacing(10)
            .align_y(iced::Center),
    );
    let mut col = column![range, codes, group].spacing(10);
    if let Some(band) = error_band(&state.error) {
        col = col.push(band);
    }
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(PointToolMsg::Help(
                "Creates a COGO point at each corridor station for the point codes picked, described by its code, and a point group holding them."
            )))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("Create"), msg(PointToolMsg::CorridorOk), true),
    ]
    .spacing(6)
    .align_y(iced::Center);
    col.push(footer).padding([10, 12]).width(sizing.width).into()
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
