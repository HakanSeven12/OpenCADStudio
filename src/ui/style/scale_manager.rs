//! Annotation-scale manager — add / delete / edit the drawing's ACAD_SCALELIST.
//!
//! Shares the style managers' frame (toolbar + list + editor + palette) so it
//! looks and behaves like Text / Dim / MLeader style managers, but scales are
//! plain `Scale` objects rather than symbol-table styles, so it drives its own
//! messages instead of the StyleKind machinery.

use crate::app::Message;
use crate::t;
use crate::ui::style::form::field_style_with_radius;
use crate::ui::style::style_manager::{
    hdivider, muted_text_style, tb_button, tb_button_maybe, vsep,
};
use iced::widget::{column, container, mouse_area, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Theme};
use std::borrow::Cow;

/// Inline-rename text-input id, so the rename-start handler can focus it.
pub fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("scale-rename-input")
}

/// What the list buttons may do for the selected scales, and the line that
/// describes them.
#[derive(Clone)]
pub struct ListState {
    pub can_up: bool,
    pub can_down: bool,
    pub can_edit: bool,
    pub can_delete: bool,
    pub units: String,
    /// Reset asks which default list to restore.
    pub reset_asking: bool,
    /// Delete asks whether to drop the selected scales not in use.
    pub delete_asking: bool,
}

/// The Edit / Add card: the scale's name and its paper : drawing units, with
/// the units line it would read as.
#[derive(Clone)]
pub struct EditCard<'a> {
    pub adding: bool,
    pub name: &'a str,
    pub paper: &'a str,
    pub drawing: &'a str,
    pub preview: String,
}

/// `1 paper Millimeter = 2 drawing Meters`: the paper unit of the drawing's
/// scale family and the drawing's own unit, plural past one.
pub fn units_line(paper: f64, drawing: f64, paper_unit: i16, drawing_unit: i16) -> String {
    let number = |value: f64| {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let one = |value: f64| (value - 1.0).abs() < 1e-12;
    format!(
        "{} {} {} = {} {} {}",
        number(paper),
        t!("paper"),
        unit_name(paper_unit, !one(paper)),
        number(drawing),
        t!("drawing"),
        unit_name(drawing_unit, !one(drawing))
    )
}

fn unit_name(code: i16, plural: bool) -> Cow<'static, str> {
    match (code, plural) {
        (1, false) => t!("Inch"),
        (1, true) => t!("Inches"),
        (2, false) => t!("Foot"),
        (2, true) => t!("Feet"),
        (3, false) => t!("Mile"),
        (3, true) => t!("Miles"),
        (4, false) => t!("Millimeter"),
        (4, true) => t!("Millimeters"),
        (5, false) => t!("Centimeter"),
        (5, true) => t!("Centimeters"),
        (6, false) => t!("Meter"),
        (6, true) => t!("Meters"),
        (7, false) => t!("Kilometer"),
        (7, true) => t!("Kilometers"),
        (_, false) => t!("unit"),
        (_, true) => t!("units"),
    }
}

/// `scales` is (name, "paper:drawing" ratio label) in the drawing's list
/// order; `selected` the picked rows (Ctrl adds, Shift extends). `edit` opens
/// the Edit / Add card in place of the details panel.
pub fn view_window<'a, 'b>(
    list: ListState,
    scales: &'b [(String, String)],
    selected: &'b [String],
    current: &'b str,
    edit: Option<EditCard<'a>>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let editing = edit.is_some();
    let idle = |msg: Message| (!editing).then_some(msg);
    // ── Toolbar: Add / Edit / Delete / Move Up / Move Down | Reset ───────
    let toolbar = container(
        row![
            tb_button_maybe(t!("Add..."), idle(Message::ScaleManagerNew), false),
            tb_button_maybe(
                t!("Edit…"),
                list.can_edit.then_some(Message::ScaleManagerEdit).filter(|_| !editing),
                false
            ),
            tb_button_maybe(
                t!("Delete"),
                list.can_delete.then_some(Message::ScaleManagerDelete).filter(|_| !editing),
                false
            ),
            tb_button_maybe(
                t!("Move Up"),
                list.can_up.then_some(Message::ScaleManagerMove(-1)).filter(|_| !editing),
                false
            ),
            tb_button_maybe(
                t!("Move Down"),
                list.can_down.then_some(Message::ScaleManagerMove(1)).filter(|_| !editing),
                false
            ),
            Space::new().width(sizing.width),
            tb_button_maybe(t!("Reset"), idle(Message::ScaleManagerReset), false),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        ..Default::default()
    })
    .width(sizing.width)
    .padding([5, 8]);

    // ── Left: scale list ──────────────────────────────────────────────────
    let rows: Vec<Element<'_, Message>> = scales
        .iter()
        .map(|(name, ratio)| {
            let is_sel = selected.iter().any(|s| s.eq_ignore_ascii_case(name));
            let is_cur = name.eq_ignore_ascii_case(current);
            let check = crate::ui::icons::themed_check_cell(is_cur);
            // The ratio keeps its whole width; the name takes the rest.
            let label = row![
                check,
                text(name.clone()).size(11).width(iced::Length::Fill),
                text(ratio.clone())
                    .size(10)
                    .style(muted_text_style)
                    .width(iced::Length::Shrink),
            ]
            .spacing(4)
            .align_y(iced::Center);
            let cell = container(label).padding([4, 8]).width(sizing.width).style(
                move |theme: &Theme| {
                    let pair = theme.palette().primary.strong;
                    container::Style {
                        background: is_sel.then_some(Background::Color(pair.color)),
                        text_color: is_sel.then_some(pair.text),
                        ..Default::default()
                    }
                },
            );
            let area = mouse_area(cell);
            if editing {
                area.into()
            } else {
                area.on_press(Message::ScaleManagerSelect(name.clone()))
                    .on_double_click(Message::ScaleRenameStart(name.clone()))
                    .into()
            }
        })
        .collect();

    let list_panel = container(
        column![
            text(t!("Scales")).size(10).style(muted_text_style),
            // The rows leave room on the right for the scrollbar, so it never
            // covers a ratio.
            container(
                scrollable(column(rows).spacing(1).padding(iced::Padding {
                    right: 12.0,
                    ..iced::Padding::ZERO
                }))
                .height(sizing.height)
            )
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
                .width(sizing.width)
                .height(sizing.height)
                .padding(2),
        ]
        .spacing(4)
        .height(sizing.height),
    )
    .width(270)
    .height(sizing.height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: the selected scale, or the Edit / Add card ────────────────
    let right: Element<'a, Message> = match edit {
        Some(card) => edit_card(card, sizing),
        None => {
            let (name, ratio) = match selected {
                [one] => scales
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(one))
                    .map(|(n, r)| (n.clone(), r.clone()))
                    .unwrap_or_default(),
                _ => (String::new(), String::new()),
            };
            container(
                column![
                    text(t!("Scale")).size(10).style(muted_text_style),
                    text(name).size(15),
                    text(ratio).size(12).style(muted_text_style),
                    Space::new().height(4),
                    text(list.units.clone()).size(11),
                    Space::new().height(6),
                    text(t!("Double-click a scale or use Edit… to change its name and its paper : drawing units. Ctrl or Shift click picks several scales. Nothing is kept unless you click OK."))
                        .size(10)
                        .style(muted_text_style),
                ]
                .spacing(8),
            )
            .width(sizing.width)
            .height(sizing.height)
            .padding(12)
            .into()
        }
    };

    let body = row![list_panel, vsep(sizing.height), right].height(sizing.height);
    let footer: Element<'a, Message> = if list.reset_asking {
        // Reset asks which default list to restore before it purges anything.
        row![
            text(t!("Restore which default list? Unreferenced scales are purged.")).size(11),
            Space::new().width(sizing.width),
            tb_button(t!("Metric"), Message::ScaleManagerResetChoice(0), false),
            tb_button(t!("Imperial"), Message::ScaleManagerResetChoice(1), false),
            tb_button(t!("Metric and Imperial"), Message::ScaleManagerResetChoice(2), false),
            tb_button(t!("Cancel"), Message::ScaleManagerResetChoice(3), false),
        ]
        .spacing(4)
        .align_y(iced::Center)
        .into()
    } else if list.delete_asking {
        row![
            text(t!("Scales in use cannot be deleted. Delete the selected scales not in use?"))
                .size(11),
            Space::new().width(sizing.width),
            tb_button(t!("Yes"), Message::ScaleManagerDeleteChoice(true), true),
            tb_button(t!("No"), Message::ScaleManagerDeleteChoice(false), false),
        ]
        .spacing(4)
        .align_y(iced::Center)
        .into()
    } else {
        row![
            Space::new().width(sizing.width),
            tb_button_maybe(t!("Cancel"), idle(Message::CloseModal), false),
            tb_button_maybe(t!("OK"), idle(Message::ScaleManagerOk), true),
        ]
        .spacing(4)
        .into()
    };

    container(column![
        toolbar,
        hdivider(sizing.width),
        body,
        hdivider(sizing.width),
        container(footer).padding([6, 8]),
    ])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    })
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

/// The Edit / Add card: an accented panel with the name, the two unit
/// fields, the units line they make and its own Cancel / Apply.
fn edit_card<'a>(card: EditCard<'a>, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let field = |label: Cow<'static, str>,
                 ph: Cow<'static, str>,
                 value: &str,
                 on: fn(String) -> Message,
                 id: Option<iced::widget::Id>| {
        let mut input = text_input(ph.as_ref(), value)
            .on_input(on)
            .on_submit(Message::ScaleRenameCommit)
            .style(|theme, status| field_style_with_radius(theme, status, 4.0))
            .size(12)
            .padding([5, 8])
            .width(sizing.width);
        if let Some(id) = id {
            input = input.id(id);
        }
        row![text(label).size(11).style(muted_text_style).width(104), input]
            .align_y(iced::Center)
            .spacing(6)
    };
    let title = if card.adding { t!("Add Scale") } else { t!("Edit Scale") };
    let ok_label = if card.adding { t!("Add") } else { t!("Apply") };
    let inner = container(
        column![
            text(title).size(13),
            field(
                t!("Name"),
                Cow::Borrowed("1:50"),
                card.name,
                Message::ScaleRenameEdit,
                Some(rename_input_id())
            ),
            field(t!("Paper units"), Cow::Borrowed("1"), card.paper, Message::ScaleManagerPaperBuf, None),
            field(
                t!("Drawing units"),
                Cow::Borrowed("50"),
                card.drawing,
                Message::ScaleManagerDrawingBuf,
                None
            ),
            text(card.preview).size(11).style(muted_text_style),
            row![
                Space::new().width(sizing.width),
                tb_button(t!("Cancel"), Message::ScaleManagerEditCancel, false),
                tb_button(ok_label, Message::ScaleRenameCommit, true),
            ]
            .spacing(4),
        ]
        .spacing(10),
    )
    .padding(12)
    .width(sizing.width)
    .style(|theme: &Theme| {
        let palette = theme.palette();
        container::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                color: palette.primary.base.color,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    });
    container(inner)
        .width(sizing.width)
        .height(sizing.height)
        .padding(12)
        .into()
}
