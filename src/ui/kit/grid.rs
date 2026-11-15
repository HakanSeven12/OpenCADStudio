//! The property grid of the palette area: rows of a name and a value, the
//! collapsible category bars above them, the scroll area with a reserved
//! gutter they sit in, and the field style of their combos and inputs.
//!
//! Properties uses it first; the other palettes (Layers, Xref Manager,
//! Layer Walk, ...) move onto it in the uniformity pass. Colours come from
//! [`chrome`], sizes from [`kit_metrics`], text from [`text`].
//!
//! A row is laid out as
//!
//! ```text
//! | indent (14) |1| name (118) |1| value (rest) |1|
//!               └───────── line under the row ─────┘
//! ```
//!
//! with the grid lines in `surface_raised` and nothing drawn in the indent.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{button, column, container, mouse_area, row, scrollable, text as label, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

/// The semibold UI font of category bars.
pub const SEMIBOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..iced::Font::DEFAULT
};

/// `text` on one line, cut with "…" where its cell ends: a cell's text
/// never wraps onto a second line over the rows below.
pub fn one_line(text: iced::widget::Text<'_>) -> iced::widget::Text<'_> {
    text.wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::advanced::text::Ellipsis::End)
}

/// Vertical padding of a text input or combo filling a grid cell: a whole
/// pixel, so the `UI_LINE` text box plus a 1 px border fills the row.
pub const FIELD_PAD_Y: f32 = ((km::ROW_H - text::UI_LINE - 2.0 * km::LINE) / 2.0).floor();
/// Padding of a field in a grid cell.
pub const FIELD_PAD: Padding = Padding {
    top: FIELD_PAD_Y,
    bottom: FIELD_PAD_Y,
    left: km::CELL_PAD_X,
    right: km::CELL_PAD_X,
};

/// A 1 px grid line, `w` × `h`.
fn line<'a>(w: Length, h: Length) -> Element<'a, Message> {
    container(Space::new().width(w).height(h))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface_raised(theme))),
            ..Default::default()
        })
        .into()
}

/// A vertical grid line one row tall.
fn vline<'a>() -> Element<'a, Message> {
    line(Length::Fixed(km::line()), Length::Fixed(km::ROW_H))
}

/// Name text: muted when the value is read-only.
pub fn name_text<'a>(name: impl Into<String>) -> Element<'a, Message> {
    one_line(label(name.into()))
        .size(text::UI)
        .line_height(text::line())
        .style(|theme: &Theme| iced::widget::text::Style {
            color: Some(chrome::text(theme)),
        })
        .into()
}

/// Value text: muted for read-only values and *VARIES*.
pub fn value_text<'a>(value: impl Into<String>, muted: bool) -> Element<'a, Message> {
    one_line(label(value.into()))
        .size(text::UI)
        .line_height(text::line())
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(if muted {
                chrome::muted(theme)
            } else {
                chrome::text(theme)
            }),
        })
        .into()
}

/// One grid row: `name` in the fixed name column, `value` filling the
/// rest. `selected` paints the name cell `row_selected`. Pressing the row
/// sends `on_select` (when given): the name cell selects it without
/// focusing the value, the value cell selects it as its field takes the
/// press (so the controls of the selected row appear).
pub fn row_view<'a>(
    name: Element<'a, Message>,
    value: Element<'a, Message>,
    selected: bool,
    on_select: Option<Message>,
) -> Element<'a, Message> {
    let name_cell = container(name)
        .width(Length::Fixed(km::NAME_COL_W))
        .height(Length::Fixed(km::ROW_H))
        .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
        .align_y(iced::Center)
        .clip(true)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(if selected {
                chrome::row_selected(theme)
            } else {
                chrome::surface(theme)
            })),
            ..Default::default()
        });
    let value_cell = container(value)
        .width(Length::Fill)
        .height(Length::Fixed(km::ROW_H))
        .align_y(iced::Center)
        .clip(true)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            ..Default::default()
        });
    let (name_cell, value_cell): (Element<'a, Message>, Element<'a, Message>) = match on_select {
        Some(message) => (
            mouse_area(name_cell).on_press(message.clone()).into(),
            super::press_first(value_cell, iced::mouse::Button::Left, message),
        ),
        None => (name_cell.into(), value_cell.into()),
    };
    column![
        row![
            Space::new().width(km::INDENT),
            vline(),
            name_cell,
            vline(),
            value_cell,
            vline(),
        ]
        .height(Length::Fixed(km::ROW_H)),
        row![
            Space::new().width(km::INDENT),
            line(Length::Fill, Length::Fixed(km::line())),
        ],
    ]
    .into()
}

/// A value cell holding plain text with the cell padding.
pub fn value_cell<'a>(value: impl Into<String>, muted: bool) -> Element<'a, Message> {
    container(value_text(value, muted))
        .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
        .width(Length::Fill)
        .into()
}

/// A category bar: `title` in 11 px semibold on `surface_raised`, the caret
/// at the right, the whole bar toggling `collapsed` on press, with a
/// `CATEGORY_GAP` of surface above it.
pub fn category_bar<'a>(
    title: impl Into<String>,
    collapsed: bool,
    on_toggle: Message,
) -> Element<'a, Message> {
    let caret = if collapsed {
        crate::ui::icons::themed_arrow_right(km::CARET)
    } else {
        crate::ui::icons::themed_arrow_down(km::CARET)
    };
    let bar = mouse_area(bar_box(title.into(), caret))
        .on_press(on_toggle)
        .interaction(iced::mouse::Interaction::Pointer);
    with_gap(bar)
}

/// A section bar: a [`category_bar`] that does not fold, with `trailing`
/// controls (small [`crate::ui::kit::bar_button`]s) at its end.
pub fn section_bar<'a>(
    title: impl Into<String>,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    with_gap(bar_box(
        title.into(),
        row(trailing).spacing(km::LINE).align_y(iced::Center),
    ))
}

/// The bar of a category or section: title and `end` on `surface_raised`.
fn bar_box<'a>(title: String, end: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        row![
            label(title)
                .size(text::UI)
                .line_height(text::line())
                .font(SEMIBOLD)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            end.into(),
        ]
        .align_y(iced::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(km::CATEGORY_H))
    .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
    .align_y(iced::Center)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::surface_raised(theme))),
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    })
    .into()
}

/// `bar` with a gap of surface above it, parting it from the rows
/// before.
fn with_gap<'a>(bar: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![Space::new().height(km::CATEGORY_GAP), bar.into()].into()
}

/// The scroll area of a grid: insets `GRID_INSET_L` on the left and
/// `GRID_INSET_R` on the right, the right one always reserved as the
/// scrollbar's gutter. The bar floats in that gutter, over nothing, so the
/// grid keeps its width whether or not the bar shows (an embedded bar
/// would take room of its own and push the grid left when it appears). A
/// `SCROLLBAR_W` rail with a `SCROLL_THUMB_W` thumb centred.
pub fn scroll_area<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let content = container(content).padding(Padding {
        left: km::GRID_INSET_L,
        right: km::GRID_INSET_R,
        ..Padding::ZERO
    });
    scrollable(content)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(km::SCROLLBAR_W)
                .scroller_width(km::SCROLL_THUMB_W)
                .margin(0),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(scroll_style)
        .into()
}

/// The overlaid scrollbar: no rail, a muted thumb.
pub fn scroll_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let thumb = match status {
        scrollable::Status::Dragged { .. } => chrome::muted(theme),
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        } => chrome::muted(theme),
        _ => chrome::border(theme),
    };
    style.vertical_rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(thumb),
            border: Border {
                radius: (km::SCROLL_THUMB_W / 2.0).into(),
                ..Default::default()
            },
        },
    };
    style.container = container::Style::default();
    style
}

/// A text input or combo in a grid cell: flat on the surface until
/// focused or hovered, then the `surface_raised` field with the accent
/// border while focused.
pub fn field_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    use iced::widget::text_input::Status;
    let (background, border) = match status {
        Status::Focused { .. } => (chrome::surface_raised(theme), chrome::accent(theme)),
        Status::Hovered => (chrome::surface_raised(theme), chrome::surface_raised(theme)),
        _ => (chrome::surface(theme), chrome::surface(theme)),
    };
    iced::widget::text_input::Style {
        background: Background::Color(background),
        border: Border {
            color: border,
            width: km::line(),
            radius: 0.0.into(),
        },
        icon: Color::TRANSPARENT,
        placeholder: chrome::muted(theme),
        value: chrome::text(theme),
        selection: chrome::accent(theme).scale_alpha(SELECTION_ALPHA),
    }
}

/// The combo field (a choice shown as a box): always the `surface_raised`
/// field.
pub fn combo_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    use iced::widget::text_input::Status;
    let mut style = field_style(theme, status);
    style.background = Background::Color(chrome::surface_raised(theme));
    if !matches!(status, Status::Focused { .. }) {
        style.border.color = chrome::surface_raised(theme);
    }
    style
}

/// A choice shown as a box (a combo head that is not a text input): the
/// `surface_raised` field.
pub fn combo_container_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome::surface_raised(theme))),
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    }
}

thread_local! {
    /// The row being built: the message that selects it and whether it is
    /// the selected row (see [`building_row`]).
    static ROW: std::cell::RefCell<(Option<Message>, bool)> =
        const { std::cell::RefCell::new((None, false)) };
}

/// Note the row about to be built: `select` selects it, `selected` says it
/// is. Row builders read it with [`row_selected`] and [`row_select`], so
/// the many value widgets need no extra parameters.
pub fn building_row(select: Option<Message>, selected: bool) {
    ROW.with(|row| *row.borrow_mut() = (select, selected));
}

/// Whether the row being built is the selected one.
pub fn row_selected() -> bool {
    ROW.with(|row| row.borrow().1)
}

/// The message selecting the row being built.
pub fn row_select() -> Option<Message> {
    ROW.with(|row| row.borrow().0.clone())
}

/// `control` (a combo caret, a pick button) on the selected row only;
/// other rows show the value alone.
pub fn on_selected_row<'a>(control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    if row_selected() {
        control.into()
    } else {
        Space::new().into()
    }
}

/// A read-only value field: selectable, never editable, muted.
pub fn read_only_style(
    theme: &Theme,
    _status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: Background::Color(chrome::surface(theme)),
        border: Border {
            color: chrome::surface(theme),
            width: km::line(),
            radius: 0.0.into(),
        },
        icon: Color::TRANSPARENT,
        placeholder: chrome::muted(theme),
        value: chrome::muted(theme),
        selection: chrome::accent(theme).scale_alpha(SELECTION_ALPHA),
    }
}

/// A read-only value as a selectable field that fills its cell.
pub fn read_only_field<'a>(value: &str) -> Element<'a, Message> {
    iced::widget::text_input("", value)
        .size(text::UI)
        .line_height(text::line())
        .style(read_only_style)
        .padding(FIELD_PAD)
        .width(Length::Fill)
        .into()
}

/// A small square button in a cell (a combo caret, the `...` of a pick):
/// flat, lit by `chrome::hover`.
/// The fill and text of a button that sits in a value cell like a field
/// (a step arrow, a Yes / No switch, a link): the field's colours, the
/// bar's under the pointer.
pub fn cell_field_pair(theme: &Theme, status: button::Status) -> iced::theme::palette::Pair {
    let fill = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        chrome::surface_raised(theme)
    } else {
        chrome::surface(theme)
    };
    iced::theme::palette::Pair::new(fill, chrome::text(theme))
}

pub fn cell_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(chrome::hover(theme))),
        text_color: if hovered {
            chrome::strong(theme)
        } else {
            chrome::text(theme)
        },
        border: Border::default(),
        ..Default::default()
    }
}

/// Opacity of the accent behind selected text.
const SELECTION_ALPHA: f32 = 0.5;

// A field's text, padding and border fit its box, checked when the crate
// compiles.
const _: () = assert!(2.0 * FIELD_PAD_Y + text::UI_LINE + 2.0 * km::LINE <= km::ROW_H);
