//! Lists with columns (Layers, Xref Manager, Count, Parameters Manager,
//! Tool Palettes): a header of sortable, resizable column titles over
//! 18 px rows of cells. Header and rows take the same column widths, so
//! they line up.

use super::tree::row_style;
use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{button, container, mouse_area, row, text as label, Space};
use iced::{Background, Element, Length, Padding, Theme};

/// A column of a [`column_header`]: its title and width, and optionally
/// its sort state, the message a click on it sends and a resize handle.
pub struct Column {
    title: String,
    width: Length,
    sort: Option<bool>,
    on_press: Option<Message>,
    on_resize: Option<Message>,
    end: bool,
}

/// A column titled `title`, `width` wide (the same width its cells get).
pub fn column(title: impl Into<String>, width: Length) -> Column {
    Column {
        title: title.into(),
        width,
        sort: None,
        on_press: None,
        on_resize: None,
        end: false,
    }
}

impl Column {
    /// The list is sorted by this column, descending or not: an arrow shows.
    pub fn sorted(mut self, descending: bool) -> Self {
        self.sort = Some(descending);
        self
    }

    /// A click on the title sends `message` (usually: sort by it).
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// A drag handle on the column's right edge; pressing it sends
    /// `message` (the caller follows the drag).
    pub fn on_resize(mut self, message: Message) -> Self {
        self.on_resize = Some(message);
        self
    }

    /// Align the title to the column's end (numbers).
    pub fn end(mut self) -> Self {
        self.end = true;
        self
    }
}

/// Width of a column's resize handle.
const RESIZE_W: f32 = 4.0;

/// The line at a column header's right end: 1 px of `surface`, and
/// with `on_resize` a 4 px grip over it that sends it when
/// pressed, to size the column by dragging. For headers laid out by hand
/// (the Layer Manager's, Layer Walk's) as well as [`column_header`].
pub fn column_divider<'a>(on_resize: Option<Message>) -> Element<'a, Message> {
    let line =
        container(Space::new().width(km::line()).height(Length::Fill)).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::surface(theme))),
                ..Default::default()
            }
        });
    match on_resize {
        Some(m) => mouse_area(
            container(line)
                .width(Length::Fixed(RESIZE_W))
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right),
        )
        .on_press(m)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into(),
        None => line.into(),
    }
}

/// The header row: column titles on `surface_raised`, 11 px semibold, a sort
/// arrow on the sorted column, 1 px palette-body lines between columns.
pub fn column_header<'a>(columns: Vec<Column>) -> Element<'a, Message> {
    let count = columns.len();
    let cells = columns.into_iter().enumerate().map(|(i, c)| {
        let mut title = row![super::one_line(label(c.title))
            .size(text::UI)
            .line_height(text::line())
            .font(super::SEMIBOLD)]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center);
        if let Some(descending) = c.sort {
            title = title.push(crate::ui::icons::themed_arrow_toggle(
                !descending,
                km::CARET,
            ));
        }
        let title = container(title)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
            .align_x(if c.end {
                iced::alignment::Horizontal::Right
            } else {
                iced::alignment::Horizontal::Left
            })
            .align_y(iced::Center);
        let title: Element<'a, Message> = match c.on_press {
            Some(m) => button(title)
                .on_press(m)
                .padding(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|theme: &Theme, status| button::Style {
                    background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                        .then(|| Background::Color(chrome::hover(theme))),
                    text_color: chrome::text(theme),
                    ..Default::default()
                })
                .into(),
            None => title.into(),
        };
        let mut cell = row![title].height(Length::Fill);
        // The line between columns, and the resize handle over it.
        if i + 1 < count || c.on_resize.is_some() {
            cell = cell.push(column_divider(c.on_resize));
        }
        container(cell).width(c.width).height(Length::Fill).into()
    });
    container(row(cells).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(km::ROW_H))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface_raised(theme))),
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        })
        .into()
}

/// A row of cells (each with the width of its column), 18 px.
pub struct ListRow<'a> {
    cells: Vec<(Element<'a, Message>, Length)>,
    selected: bool,
    on_press: Option<Message>,
    on_double_click: Option<Message>,
    on_right_press: Option<Message>,
}

/// A row of `cells`, each `(content, column width)`.
pub fn list_row<'a>(cells: Vec<(Element<'a, Message>, Length)>) -> ListRow<'a> {
    ListRow {
        cells,
        selected: false,
        on_press: None,
        on_double_click: None,
        on_right_press: None,
    }
}

impl<'a> ListRow<'a> {
    /// Paint the row as selected (`row_selected`).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Send `message` when the row is pressed.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Send `message` on a double-click (rename, open).
    pub fn on_double_click(mut self, message: Message) -> Self {
        self.on_double_click = Some(message);
        self
    }

    /// Send `message` on a right-click (its menu).
    pub fn on_right_press(mut self, message: Message) -> Self {
        self.on_right_press = Some(message);
        self
    }

    /// The row.
    pub fn view(self) -> Element<'a, Message> {
        let cells = self.cells.into_iter().map(|(content, width)| {
            container(content)
                .width(width)
                .height(Length::Fill)
                .align_y(iced::Center)
                .clip(true)
                .into()
        });
        let interactive = self.on_press.is_some();
        let selected = self.selected;
        let content = container(row(cells).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(km::ROW_H));
        let lit = |hovered: bool| {
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(km::ROW_H))
                .style(row_style(selected, hovered && interactive))
        };
        let stacked = iced::widget::stack![iced::widget::hover(lit(false), lit(true)), content];
        let mut area = mouse_area(stacked);
        if let Some(m) = self.on_press {
            area = area
                .on_press(m)
                .interaction(iced::mouse::Interaction::Pointer);
        }
        if let Some(m) = self.on_double_click {
            area = area.on_double_click(m);
        }
        if let Some(m) = self.on_right_press {
            area = area.on_right_press(m);
        }
        area.into()
    }
}

/// A text cell: 11 px, padded, muted when it only states something.
pub fn text_cell<'a>(value: impl Into<String>, muted: bool) -> Element<'a, Message> {
    container(super::value_text(value, muted))
        .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
        .into()
}

/// A text cell aligned to its column's end (a count, a size).
pub fn number_cell<'a>(value: impl Into<String>) -> Element<'a, Message> {
    container(super::value_text(value, false))
        .width(Length::Fill)
        .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
}

/// A toggle cell (on / freeze / lock / plot): `icon` in a square button the
/// height of a row, flat until hovered.
pub fn toggle_cell<'a>(
    icon: impl Into<Element<'a, Message>>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(icon).center(Length::Fill))
        .on_press_maybe(message)
        .width(Length::Fixed(km::ROW_H))
        .height(Length::Fixed(km::ROW_H))
        .padding(0)
        .style(super::flat_button_style(false))
        .into()
}
