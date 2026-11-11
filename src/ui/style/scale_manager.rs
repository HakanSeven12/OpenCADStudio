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

/// What the list buttons may do for the selected scale, and the line that
/// describes it.
#[derive(Clone)]
pub struct ListState {
    pub can_up: bool,
    pub can_down: bool,
    pub can_delete: bool,
    pub units: String,
    /// Reset asks which default list to restore.
    pub reset_asking: bool,
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
/// order. The editor buffers hold the paper / drawing units of the selected
/// scale.
#[allow(clippy::too_many_arguments)]
pub fn view_window<'a, 'b>(
    list: ListState,
    scales: &'b [(String, String)],
    selected: &'b str,
    current: &'b str,
    rename_active: Option<&'b str>,
    rename_buf: &'a str,
    paper_buf: &'a str,
    drawing_buf: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // ── Toolbar: Add / Delete / Move Up / Move Down | Reset ──────────────
    let toolbar = container(
        row![
            tb_button(t!("Add"), Message::ScaleManagerNew, false),
            tb_button_maybe(
                t!("Delete"),
                list.can_delete.then_some(Message::ScaleManagerDelete),
                false
            ),
            tb_button_maybe(
                t!("Move Up"),
                list.can_up.then_some(Message::ScaleManagerMove(-1)),
                false
            ),
            tb_button_maybe(
                t!("Move Down"),
                list.can_down.then_some(Message::ScaleManagerMove(1)),
                false
            ),
            Space::new().width(sizing.width),
            tb_button(t!("Reset"), Message::ScaleManagerReset, false),
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
    let rows: Vec<Element<'_, Message>> =
        scales
            .iter()
            .map(|(name, ratio)| {
                // The row being renamed shows an inline text field; a single click
                // selects, a double click starts renaming.
                if rename_active == Some(name.as_str()) {
                    return text_input("", rename_buf)
                        .id(rename_input_id())
                        .on_input(Message::ScaleRenameEdit)
                        .on_submit(Message::ScaleRenameCommit)
                        .size(11)
                        .padding([4, 8])
                        .width(sizing.width)
                        .into();
                }
                let is_sel = name.as_str() == selected;
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
                mouse_area(cell)
                    .on_press(Message::ScaleManagerSelect(name.clone()))
                    .on_double_click(Message::ScaleRenameStart(name.clone()))
                    .into()
            })
            .collect();

    let list_panel = container(
        column![
            text(t!("Scales")).size(10).style(muted_text_style),
            container(scrollable(column(rows).spacing(1)).height(sizing.height))
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
    .width(250)
    .height(sizing.height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: editor (name + paper:drawing) ──────────────────────────────
    let field = |label: Cow<'static, str>,
                 ph: Cow<'static, str>,
                 value: &str,
                 on: fn(String) -> Message| {
        row![
            text(label).size(11).style(muted_text_style).width(96),
            text_input(ph.as_ref(), value)
                .on_input(on)
                .style(|theme, status| field_style_with_radius(theme, status, 4.0))
                .size(12)
                .padding([5, 8])
                .width(sizing.width),
        ]
        .align_y(iced::Center)
        .spacing(6)
    };

    let editor = container(
        column![
            text(Cow::Borrowed("Scale")).size(10).style(muted_text_style),
            field(t!("Paper units"), Cow::Borrowed("1"), paper_buf, Message::ScaleManagerPaperBuf),
            field(t!("Drawing units"), Cow::Borrowed("50"), drawing_buf, Message::ScaleManagerDrawingBuf),
            Space::new().height(6),
            text(list.units.clone()).size(11),
            Space::new().height(6),
            text(t!("Double-click a scale to rename it; edit its paper : drawing ratio here. Add puts a new scale at the end of the list. Nothing is kept unless you click OK."))
                .size(10)
                .style(muted_text_style),
        ]
        .spacing(8),
    )
    .width(sizing.width)
    .height(sizing.height)
    .padding(12);

    let body = row![list_panel, vsep(sizing.height), editor].height(sizing.height);
    // Reset asks which default list to restore before it purges anything.
    let footer: Element<'a, Message> = if list.reset_asking {
        row![
            text(t!(
                "Restore which default list? Unreferenced scales are purged."
            ))
            .size(11),
            Space::new().width(sizing.width),
            tb_button(t!("Metric"), Message::ScaleManagerResetChoice(0), false),
            tb_button(t!("Imperial"), Message::ScaleManagerResetChoice(1), false),
            tb_button(
                t!("Metric and Imperial"),
                Message::ScaleManagerResetChoice(2),
                false
            ),
            tb_button(t!("Cancel"), Message::ScaleManagerResetChoice(3), false),
        ]
        .spacing(4)
        .align_y(iced::Center)
        .into()
    } else {
        row![
            Space::new().width(sizing.width),
            tb_button(t!("Cancel"), Message::CloseModal, false),
            tb_button(t!("OK"), Message::ScaleManagerOk, true),
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
