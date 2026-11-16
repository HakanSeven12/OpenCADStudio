//! Rows of a tree or a plain list: indent, fold caret, icon, label and a
//! trailing control (a switch, a count), 18 px high like the grid's rows.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{button, container, mouse_area, row, text as label, Space};
use iced::{Background, Element, Length, Padding, Theme};

/// One row of a tree (or, at depth 0 without a caret, of a plain list).
/// Built with its setters, drawn with [`TreeRow::view`].
pub struct TreeRow<'a> {
    label: String,
    depth: u16,
    fold: Option<(bool, Message)>,
    caret_room: bool,
    icon: Option<Element<'a, Message>>,
    trailing: Option<Element<'a, Message>>,
    selected: bool,
    muted: bool,
    on_press: Option<Message>,
    on_double_click: Option<Message>,
    on_right_press: Option<Message>,
}

/// A row named `label`.
pub fn tree_row<'a>(label: impl Into<String>) -> TreeRow<'a> {
    TreeRow {
        label: label.into(),
        depth: 0,
        fold: None,
        caret_room: false,
        icon: None,
        trailing: None,
        selected: false,
        muted: false,
        on_press: None,
        on_double_click: None,
        on_right_press: None,
    }
}

impl<'a> TreeRow<'a> {
    /// Indent the row `depth` levels (14 px each).
    pub fn depth(mut self, depth: u16) -> Self {
        self.depth = depth;
        self
    }

    /// A fold caret, `open` or closed, sending `toggle` when pressed.
    pub fn fold(mut self, open: bool, toggle: Message) -> Self {
        self.fold = Some((open, toggle));
        self
    }

    /// Keep the caret's room without a caret, so leaves line up with their
    /// foldable siblings.
    pub fn caret_room(mut self) -> Self {
        self.caret_room = true;
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: impl Into<Element<'a, Message>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// A control at the row's end (a switch, a count, a button).
    pub fn trailing(mut self, control: impl Into<Element<'a, Message>>) -> Self {
        self.trailing = Some(control.into());
        self
    }

    /// Paint the row as the selected one.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Show the label muted (a row that states rather than does).
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Send `message` when the row is pressed.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Send `message` on a double-click.
    pub fn on_double_click(mut self, message: Message) -> Self {
        self.on_double_click = Some(message);
        self
    }

    /// Send `message` on a right-click.
    pub fn on_right_press(mut self, message: Message) -> Self {
        self.on_right_press = Some(message);
        self
    }

    /// The row.
    pub fn view(self) -> Element<'a, Message> {
        let mut cells = row![Space::new().width(f32::from(self.depth) * km::INDENT)]
            .spacing(0)
            .align_y(iced::Center)
            .height(Length::Fill);
        if let Some((open, toggle)) = self.fold {
            let caret = if open {
                crate::ui::icons::themed_arrow_down(km::CARET)
            } else {
                crate::ui::icons::themed_arrow_right(km::CARET)
            };
            cells = cells.push(
                button(container(caret).center(Length::Fill))
                    .on_press(toggle)
                    .width(Length::Fixed(km::INDENT))
                    .height(Length::Fixed(km::ROW_H))
                    .padding(0)
                    .style(|_: &Theme, _| button::Style::default()),
            );
        } else if self.caret_room {
            cells = cells.push(Space::new().width(km::INDENT));
        }
        if let Some(icon) = self.icon {
            cells = cells.push(container(icon).padding(Padding::ZERO.right(km::TOOL_GAP)));
        }
        let muted = self.muted;
        cells = cells.push(
            super::one_line(label(self.label))
                .size(text::UI)
                .line_height(text::line())
                .width(Length::Fill)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: Some(if muted {
                        chrome::muted(theme)
                    } else {
                        chrome::text(theme)
                    }),
                }),
        );
        if let Some(trailing) = self.trailing {
            cells = cells.push(trailing);
        }
        let selected = self.selected;
        let interactive = self.on_press.is_some();
        let body = |hovered: bool| row_style(selected, hovered && interactive);
        let row_box = container(cells)
            .width(Length::Fill)
            .height(Length::Fixed(km::ROW_H))
            .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
            .style(body(false));
        // Hover lights the whole row: the same row drawn lit under the
        // pointer would need the cells twice, so the hover is a layer of
        // its own under the content.
        let hover_layer = iced::widget::hover(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(km::ROW_H)),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(km::ROW_H))
                .style(body(true)),
        );
        let stacked = iced::widget::stack![hover_layer, row_box];
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

/// The background of a list or tree row: `row_selected` when selected,
/// `hover` while hovered, nothing otherwise.
pub(super) fn row_style(selected: bool, hovered: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| container::Style {
        background: if selected {
            Some(Background::Color(chrome::row_selected(theme)))
        } else if hovered {
            Some(Background::Color(chrome::hover(theme)))
        } else {
            None
        },
        ..Default::default()
    }
}
