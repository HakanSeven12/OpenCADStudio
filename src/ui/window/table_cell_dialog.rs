//! Picked table cell windows: Cell Borders (line properties and the edges of
//! the picked range they go on) and Cell Contents (the contents of a cell,
//! their order and how they are laid out).

use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip};

/// Which edges of the picked range the border settings go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSet {
    All,
    Outside,
    Inside,
    None,
    Top,
    Bottom,
    Left,
    Right,
    InsideHorizontal,
    InsideVertical,
}

#[derive(Debug, Clone)]
pub enum TableCellDialogMsg {
    Lineweight(String),
    Linetype(String),
    Color(String),
    Double(bool),
    Spacing(String),
    Edges(BorderSet),
    Pick(usize),
    Up,
    Down,
    Delete,
    Layout(u32),
    ContentSpacing(String),
    Help,
    Ok,
}

fn msg(m: TableCellDialogMsg) -> Message {
    Message::TableCellDialog(m)
}

pub struct CellBordersState {
    pub lineweights: Vec<String>,
    pub lineweight: String,
    pub linetypes: Vec<String>,
    pub linetype: String,
    pub colors: Vec<String>,
    pub color: String,
    pub double: bool,
    pub spacing: String,
    /// The chosen edge sets (the last one alone when it is All or None).
    pub sets: Vec<BorderSet>,
    /// Picked rows × columns, for the sample grid.
    pub rows: usize,
    pub columns: usize,
}

pub struct CellContentsState {
    /// Each content as listed: "Text: …" / "Block: …".
    pub items: Vec<String>,
    /// The cell's content indices, in their new order.
    pub order: Vec<usize>,
    pub selected: Option<usize>,
    /// Content layout bit: 1 flow, 2 stacked horizontally, 4 vertically.
    pub layout: u32,
    pub spacing: String,
}

/// Whether an edge of the sample grid gets the settings.
fn edge_chosen(sets: &[BorderSet], outside: bool, horizontal: bool, side: BorderSet) -> bool {
    sets.iter().any(|set| match set {
        BorderSet::All => true,
        BorderSet::Outside => outside,
        BorderSet::Inside => !outside,
        BorderSet::InsideHorizontal => !outside && horizontal,
        BorderSet::InsideVertical => !outside && !horizontal,
        BorderSet::None => false,
        edge => outside && *edge == side,
    })
}

/// A 2×2 sample of the picked cells: the edges the settings go on drawn
/// in the accent colour, the others faint.
fn sample<'a>(state: &CellBordersState) -> Element<'a, Message> {
    let rows = state.rows.clamp(1, 2);
    let columns = state.columns.clamp(1, 2);
    let mut grid = column![].spacing(0);
    for r in 0..rows {
        let mut line = row![].spacing(0);
        for c in 0..columns {
            let top = edge_chosen(&state.sets, r == 0, true, BorderSet::Top);
            let bottom = edge_chosen(&state.sets, r + 1 == rows, true, BorderSet::Bottom);
            let left = edge_chosen(&state.sets, c == 0, false, BorderSet::Left);
            let right = edge_chosen(&state.sets, c + 1 == columns, false, BorderSet::Right);
            let none = state.sets.contains(&BorderSet::None);
            let cell = container(
                column![
                    edge_bar(top && !none, true),
                    row![
                        edge_bar(left && !none, false),
                        Space::new().width(Fill).height(Fill),
                        edge_bar(right && !none, false),
                    ]
                    .height(Fill),
                    edge_bar(bottom && !none, true),
                ],
            )
            .width(Length::Fixed(54.0))
            .height(Length::Fixed(30.0));
            line = line.push(cell);
        }
        grid = grid.push(line);
    }
    container(grid).center_x(Fill).padding(6).into()
}

fn edge_bar<'a>(on: bool, horizontal: bool) -> Element<'a, Message> {
    let bar = container(Space::new()).style(move |theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(if on {
            theme.palette().primary.base.color
        } else {
            theme.palette().background.strong.color
        })),
        ..Default::default()
    });
    if horizontal {
        bar.width(Fill).height(Length::Fixed(if on { 3.0 } else { 1.0 }))
    } else {
        bar.height(Fill).width(Length::Fixed(if on { 3.0 } else { 1.0 }))
    }
    .into()
}

fn labelled<'a>(label: String, input: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(label).size(11).width(Length::Fixed(110.0)),
        input.into()
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn list<'a>(
    options: &'a [String],
    selected: &'a str,
    on: fn(String) -> TableCellDialogMsg,
) -> Element<'a, Message> {
    pick_list(Some(selected.to_string()), options.to_vec(), |value| value.clone())
        .on_select(move |value| msg(on(value)))
        .text_size(11)
        .padding([3, 6])
        .width(Fill)
        .into()
}

fn footer<'a>() -> Element<'a, Message> {
    row![
        button(text("?").size(12))
            .on_press(msg(TableCellDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
        dialog_button_styled_opt(
            crate::t!("OK"),
            Some(msg(TableCellDialogMsg::Ok)),
            button::primary
        ),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub fn view_borders<'a>(
    state: &'a CellBordersState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let line = card(
        crate::tr!("table-cells", "line"),
        column![
            labelled(
                crate::tr!("table-cells", "lineweight"),
                list(&state.lineweights, &state.lineweight, M::Lineweight)
            ),
            labelled(
                crate::tr!("table-cells", "linetype"),
                list(&state.linetypes, &state.linetype, M::Linetype)
            ),
            labelled(
                crate::tr!("table-cells", "color"),
                list(&state.colors, &state.color, M::Color)
            ),
            row![
                chip(
                    crate::tr!("table-cells", "double"),
                    state.double,
                    msg(M::Double(!state.double))
                ),
                text(crate::tr!("table-cells", "spacing")).size(11),
                {
                    let mut input = text_input("", &state.spacing)
                        .size(11)
                        .padding([4, 8])
                        .width(Length::Fixed(90.0))
                        .style(field_style);
                    if state.double {
                        input = input.on_input(|v| msg(M::Spacing(v)));
                    }
                    input
                },
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    let set = |kind: BorderSet, label: String| -> Element<'a, Message> {
        let on = state.sets.contains(&kind);
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(M::Edges(kind)))
            .style(button_style(on))
            .padding([5, 6])
            .width(Fill)
            .into()
    };
    let edges = card(
        crate::tr!("table-cells", "apply-to"),
        row![
            column![
                row![
                    set(BorderSet::All, crate::tr!("table-cells", "all")),
                    set(BorderSet::Outside, crate::tr!("table-cells", "outside")),
                    set(BorderSet::Inside, crate::tr!("table-cells", "inside")),
                    set(BorderSet::None, crate::tr!("table-cells", "none")),
                ]
                .spacing(4),
                row![
                    set(BorderSet::Top, crate::tr!("table-cells", "top")),
                    set(BorderSet::Bottom, crate::tr!("table-cells", "bottom")),
                    set(BorderSet::Left, crate::tr!("table-cells", "left")),
                    set(BorderSet::Right, crate::tr!("table-cells", "right")),
                ]
                .spacing(4),
                row![
                    set(BorderSet::InsideHorizontal, crate::tr!("table-cells", "inside-horizontal")),
                    set(BorderSet::InsideVertical, crate::tr!("table-cells", "inside-vertical")),
                ]
                .spacing(4),
            ]
            .spacing(4)
            .width(Fill),
            container(sample(state)).width(Length::Fixed(130.0)),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );
    column![line, edges, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

pub fn view_contents<'a>(
    state: &'a CellContentsState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let mut items = column![].spacing(2);
    if state.order.is_empty() {
        items = items.push(text(crate::tr!("table-cells", "empty")).size(11).style(muted_style));
    }
    for (position, &index) in state.order.iter().enumerate() {
        let label = state.items.get(index).cloned().unwrap_or_default();
        items = items.push(
            button(text(format!("{}.  {label}", position + 1)).size(11))
                .on_press(msg(M::Pick(position)))
                .style(button_style(state.selected == Some(position)))
                .padding([4, 8])
                .width(Fill),
        );
    }
    let picked = state.selected.is_some();
    let tool = |label: String, message: M, enabled: bool| -> Element<'a, Message> {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press_maybe(enabled.then(|| msg(message)))
            .style(button_style(false))
            .padding([5, 8])
            .width(Fill)
            .into()
    };
    let contents = card(
        crate::tr!("table-cells", "contents"),
        row![
            container(items).width(Fill),
            column![
                tool(crate::tr!("table-cells", "move-up"), M::Up, picked && state.selected != Some(0)),
                tool(
                    crate::tr!("table-cells", "move-down"),
                    M::Down,
                    picked && state.selected.is_some_and(|p| p + 1 < state.order.len())
                ),
                tool(crate::tr!("table-cells", "delete"), M::Delete, picked),
            ]
            .spacing(4)
            .width(Length::Fixed(110.0)),
        ]
        .spacing(10),
    );
    let layout_button = |bit: u32, label: String| -> Element<'a, Message> {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(M::Layout(bit)))
            .style(button_style(state.layout == bit))
            .padding([5, 6])
            .width(Fill)
            .into()
    };
    let layout = card(
        crate::tr!("table-cells", "layout"),
        column![
            row![
                layout_button(1, crate::tr!("table-cells", "flow")),
                layout_button(2, crate::tr!("table-cells", "stack-horizontal")),
                layout_button(4, crate::tr!("table-cells", "stack-vertical")),
            ]
            .spacing(4),
            labelled(crate::tr!("table-cells", "content-spacing"), {
                let mut input = text_input("", &state.spacing)
                    .size(11)
                    .padding([4, 8])
                    .width(Length::Fixed(90.0))
                    .style(field_style);
                if state.layout != 1 {
                    input = input.on_input(|v| msg(M::ContentSpacing(v)));
                }
                input
            }),
        ]
        .spacing(8),
    );
    column![contents, layout, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
