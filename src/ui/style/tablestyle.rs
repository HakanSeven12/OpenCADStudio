//! Table style manager.

use crate::app::Message;
use crate::t;
use iced::widget::{canvas, checkbox, column, row, text, text_input, Column};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Size, Theme};
use crate::ui::style::common::muted_style;
use std::borrow::Cow;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumChoice {
    code: String,
    label: Cow<'static, str>,
}

impl fmt::Display for EnumChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label.as_ref())
    }
}

fn choices(values: &[(&str, &str)]) -> Vec<EnumChoice> {
    values
        .iter()
        .map(|(code, label)| EnumChoice {
            code: (*code).to_string(),
            label: crate::i18n::translate(label),
        })
        .collect()
}

pub struct TableStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub current: &'a str,
    pub style: Option<&'a acadrust::objects::TableStyle>,
    pub tab: u8,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub in_use: bool,
    pub hmargin: &'a str,
    pub vmargin: &'a str,
    pub description: &'a str,
    pub cell_textstyle: &'a [String; 3],
    pub cell_height: &'a [String; 3],
    pub cell_textcolor: &'a [String; 3],
    pub cell_fillcolor: &'a [String; 3],
    pub cell_datatype: &'a [String; 3],
    pub cell_unittype: &'a [String; 3],
    pub cell_format: &'a [String; 3],
    pub border_lw: &'a [[String; 6]; 3],
    pub border_color: &'a [[String; 6]; 3],
    pub border_spacing: &'a [[String; 6]; 3],
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    pub color_open: Option<(u8, &'static str)>,
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

#[derive(Clone)]
struct PreviewBorder {
    color: String,
    weight: String,
    spacing: String,
    double: bool,
    hidden: bool,
}

#[derive(Clone)]
struct PreviewRow {
    label: String,
    text_style: String,
    text_height: String,
    text_color: String,
    fill_color: String,
    fill: bool,
    alignment: String,
    data_type: String,
    unit_type: String,
    format: String,
    borders: [PreviewBorder; 6],
}

struct TablePreviewCanvas {
    rows: [PreviewRow; 3],
    active_row: usize,
    hmargin: String,
    vmargin: String,
    description: String,
    flow_up: bool,
    title_suppressed: bool,
    header_suppressed: bool,
    annotative: bool,
}

fn aci_color(value: &str, fallback: Color) -> Color {
    crate::ui::color_select::aci_string_to_color(value)
        .rgb()
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(fallback)
}

fn border_width(value: &str) -> f32 {
    value
        .trim()
        .parse::<f32>()
        .map(|value| (value.abs() / 35.0).clamp(0.7, 4.5))
        .unwrap_or(1.0)
}

fn draw_line(
    frame: &mut canvas::Frame,
    a: Point,
    b: Point,
    border: &PreviewBorder,
    fallback: Color,
) {
    if border.hidden {
        return;
    }
    let stroke = canvas::Stroke::default()
        .with_color(aci_color(&border.color, fallback))
        .with_width(border_width(&border.weight));
    frame.stroke(&canvas::Path::line(a, b), stroke.clone());
    if border.double {
        let spacing = border
            .spacing
            .trim()
            .parse::<f32>()
            .unwrap_or(1.0)
            .abs()
            .clamp(1.0, 5.0);
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length = (dx * dx + dy * dy).sqrt().max(1.0);
        let offset = Point::new(-dy / length * spacing, dx / length * spacing);
        frame.stroke(
            &canvas::Path::line(
                Point::new(a.x + offset.x, a.y + offset.y),
                Point::new(b.x + offset.x, b.y + offset.y),
            ),
            stroke,
        );
    }
}

impl canvas::Program<Message> for TablePreviewCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let ink = theme.palette().background.base.text;
        let mut order = vec![2usize, 1, 0];
        if self.title_suppressed {
            order.retain(|row| *row != 2);
        }
        if self.header_suppressed {
            order.retain(|row| *row != 1);
        }
        if self.flow_up {
            order.reverse();
        }
        let hmargin = self.hmargin.trim().parse::<f32>().unwrap_or(1.5).abs();
        let vmargin = self.vmargin.trim().parse::<f32>().unwrap_or(1.5).abs();
        let left = 18.0 + (hmargin * 1.2).clamp(0.0, 18.0);
        let right = bounds.width - left;
        let top = 9.0 + (vmargin * 0.8).clamp(0.0, 12.0);
        let bottom = bounds.height - 24.0;
        let row_height = ((bottom - top) / order.len().max(1) as f32).max(18.0);
        let col_width = (right - left) / 3.0;
        for (display_row, row_index) in order.iter().copied().enumerate() {
            let row = &self.rows[row_index];
            let y0 = top + display_row as f32 * row_height;
            let y1 = y0 + row_height;
            let fallback_fill = theme.palette().background.weak.color;
            if row.fill {
                frame.fill(
                    &canvas::Path::rectangle(
                        Point::new(left, y0),
                        Size::new(right - left, row_height),
                    ),
                    aci_color(&row.fill_color, fallback_fill),
                );
            }
            let borders = &row.borders;
            draw_line(&mut frame, Point::new(left, y0), Point::new(left, y1), &borders[0], ink);
            draw_line(&mut frame, Point::new(right, y0), Point::new(right, y1), &borders[1], ink);
            draw_line(&mut frame, Point::new(left, y0), Point::new(right, y0), &borders[2], ink);
            draw_line(&mut frame, Point::new(left, y1), Point::new(right, y1), &borders[3], ink);
            for column in 1..3 {
                let x = left + column as f32 * col_width;
                draw_line(&mut frame, Point::new(x, y0), Point::new(x, y1), &borders[5], ink);
            }
            if display_row + 1 < order.len() {
                draw_line(&mut frame, Point::new(left, y1), Point::new(right, y1), &borders[4], ink);
            }
            for column in 0..3 {
                let label = if row.format.trim().is_empty() {
                    format!("{} {}", row.label, column + 1)
                } else {
                    row.format.replace("{}", &(column + 1).to_string())
                };
                let horizontal = if row.alignment.ends_with("Left") {
                    iced::advanced::text::Alignment::Left
                } else if row.alignment.ends_with("Right") {
                    iced::advanced::text::Alignment::Right
                } else {
                    iced::advanced::text::Alignment::Center
                };
                let x = match horizontal {
                    iced::advanced::text::Alignment::Left => left + column as f32 * col_width + 5.0,
                    iced::advanced::text::Alignment::Right => left + (column + 1) as f32 * col_width - 5.0,
                    iced::advanced::text::Alignment::Center => left + (column as f32 + 0.5) * col_width,
                    _ => left + (column as f32 + 0.5) * col_width,
                };
                let y = if row.alignment.starts_with("Top") {
                    y0 + 7.0
                } else if row.alignment.starts_with("Bottom") {
                    y1 - 7.0
                } else {
                    (y0 + y1) * 0.5
                };
                frame.fill_text(canvas::Text {
                    content: label,
                    position: Point::new(x, y),
                    color: aci_color(&row.text_color, ink),
                    size: iced::Pixels(
                        row.text_height
                            .trim()
                            .parse::<f32>()
                            .map(|height| (height * 34.0).clamp(7.0, 16.0))
                            .unwrap_or(10.0),
                    ),
                    align_x: horizontal,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
        let active = &self.rows[self.active_row.min(2)];
        frame.fill_text(canvas::Text {
            content: format!(
                "{} · {} · {} {} / {} {}{}{}",
                active.text_style,
                if self.annotative { t!("Annotative") } else { t!("Drawing units") },
                t!("Type"), active.data_type,
                t!("Unit"), active.unit_type,
                if self.description.trim().is_empty() { "" } else { " · " },
                self.description,
            ),
            position: Point::new(10.0, bounds.height - 9.0),
            color: ink.scale_alpha(0.65),
            size: iced::Pixels(10.0),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

fn input_row<'a>(
    label: Cow<'static, str>,
    placeholder: &'static str,
    value: &'a str,
    message: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(155),
        text_input(placeholder, value)
            .on_input(message)
            .size(11)
            .width(170),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn cell_editor<'a>(v: &TableStyleView<'a>, row_index: u8) -> Element<'a, Message> {
    let Some(style) = v.style else {
        return text(t!("Select a style to view details.")).into();
    };
    let row_style = match row_index {
        0 => &style.data_row_style,
        1 => &style.header_row_style,
        _ => &style.title_row_style,
    };
    let index = row_index as usize;
    let cell_input = |label, placeholder, value: &'a str, field| {
        input_row(label, placeholder, value, move |value| Message::TableStyleCellEdit {
            row: row_index,
            field,
            value,
        })
    };
    let cell_color = |label: Cow<'static, str>, value: &'a str, field: &'static str| {
        let current = crate::ui::color_select::aci_string_to_color(value);
        let selector = crate::ui::color_select::color_selector(
            current,
            v.color_open == Some((row_index, field)),
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
                ..Default::default()
            },
            move |color| Message::TableStyleCellEdit {
                row: row_index,
                field,
                value: crate::ui::color_select::color_to_aci_string(color),
            },
            Message::TableColorMore(row_index, field),
            Message::OpenColorWindow(crate::app::ColorPickTarget::Table(row_index, field), current),
        );
        row![text(label).size(11).style(muted_style).width(155), selector]
            .spacing(8)
            .align_y(iced::Center)
    };
    let selected_alignment = format!("{:?}", row_style.alignment);
    let mut content = Column::new()
        .spacing(7)
        .push(cell_input(t!("Text style"), "Standard", &v.cell_textstyle[index], "textstyle"))
        .push(cell_input(t!("Text height"), "0.18", &v.cell_height[index], "height"))
        .push(cell_color(t!("Text color"), &v.cell_textcolor[index], "textcolor"))
        .push(cell_color(t!("Fill color"), &v.cell_fillcolor[index], "fillcolor"))
        .push(
            row![
                text(t!("Alignment")).size(11).style(muted_style).width(155),
                iced::widget::pick_list(
                    Some(EnumChoice { code: selected_alignment.clone(), label: crate::i18n::translate(&selected_alignment) }),
                    choices(&[
                        ("TopLeft", "Top left"), ("TopCenter", "Top center"), ("TopRight", "Top right"),
                        ("MiddleLeft", "Middle left"), ("MiddleCenter", "Middle center"), ("MiddleRight", "Middle right"),
                        ("BottomLeft", "Bottom left"), ("BottomCenter", "Bottom center"), ("BottomRight", "Bottom right"),
                    ]),
                    |value| value.to_string(),
                )
                .on_select(move |choice| Message::TableStyleCellSetAlign { row: row_index, value: choice.code })
                .text_size(11)
                .width(170),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .push(
            checkbox(row_style.fill_enabled)
                .label(t!("Background fill enabled"))
                .on_toggle(move |_| Message::TableStyleCellToggleFill(row_index))
                .size(14)
                .text_size(11),
        )
        .push(cell_input(t!("Data type"), "0", &v.cell_datatype[index], "datatype"))
        .push(cell_input(t!("Unit type"), "0", &v.cell_unittype[index], "unittype"))
        .push(cell_input(t!("Format string"), "", &v.cell_format[index], "format"))
        .push(text(t!("Borders")).size(11).style(primary_style));
    let borders = [
        (t!("Left"), &row_style.left_border),
        (t!("Right"), &row_style.right_border),
        (t!("Top"), &row_style.top_border),
        (t!("Bottom"), &row_style.bottom_border),
        (t!("Inside horizontal"), &row_style.horizontal_inside_border),
        (t!("Inside vertical"), &row_style.vertical_inside_border),
    ];
    for (border_index, (label, border)) in borders.into_iter().enumerate() {
        let border_index = border_index as u8;
        let border_type = format!("{:?}", border.border_type);
        content = content.push(
            row![
                text(label).size(10).style(muted_style).width(96),
                iced::widget::pick_list(
                    Some(EnumChoice { code: border_type.clone(), label: crate::i18n::translate(&border_type) }),
                    choices(&[("Single", "Single"), ("Double", "Double")]),
                    |value| value.to_string(),
                )
                .on_select(move |choice| Message::TableStyleBorderSetType { cell: row_index, border: border_index, value: choice.code })
                .text_size(10)
                .width(82),
                text_input(t!("Weight").as_ref(), &v.border_lw[index][border_index as usize])
                    .on_input(move |value| Message::TableStyleBorderEdit { cell: row_index, border: border_index, field: "lw", value })
                    .size(10).width(68),
                text_input(t!("Color").as_ref(), &v.border_color[index][border_index as usize])
                    .on_input(move |value| Message::TableStyleBorderEdit { cell: row_index, border: border_index, field: "color", value })
                    .size(10).width(62),
                text_input(t!("Spacing").as_ref(), &v.border_spacing[index][border_index as usize])
                    .on_input(move |value| Message::TableStyleBorderEdit { cell: row_index, border: border_index, field: "spacing", value })
                    .size(10).width(68),
                checkbox(border.is_invisible)
                    .label(t!("Hidden"))
                    .on_toggle(move |_| Message::TableStyleBorderToggleInvisible { cell: row_index, border: border_index })
                    .size(13).text_size(10),
            ]
            .spacing(5)
            .align_y(iced::Center),
        );
    }
    content.into()
}

fn preview_rows(v: &TableStyleView<'_>) -> Option<[PreviewRow; 3]> {
    let style = v.style?;
    let source = [&style.data_row_style, &style.header_row_style, &style.title_row_style];
    Some(std::array::from_fn(|row| {
        let borders = [
            &source[row].left_border,
            &source[row].right_border,
            &source[row].top_border,
            &source[row].bottom_border,
            &source[row].horizontal_inside_border,
            &source[row].vertical_inside_border,
        ];
        PreviewRow {
            label: [t!("Data"), t!("Header"), t!("Title")][row].to_string(),
            text_style: v.cell_textstyle[row].clone(),
            text_height: v.cell_height[row].clone(),
            text_color: v.cell_textcolor[row].clone(),
            fill_color: v.cell_fillcolor[row].clone(),
            fill: source[row].fill_enabled,
            alignment: format!("{:?}", source[row].alignment),
            data_type: v.cell_datatype[row].clone(),
            unit_type: v.cell_unittype[row].clone(),
            format: v.cell_format[row].clone(),
            borders: std::array::from_fn(|border| PreviewBorder {
                color: v.border_color[row][border].clone(),
                weight: v.border_lw[row][border].clone(),
                spacing: v.border_spacing[row][border].clone(),
                double: format!("{:?}", borders[border].border_type) == "Double",
                hidden: borders[border].is_invisible,
            }),
        }
    }))
}

pub fn view_window<'a>(
    v: TableStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let style = v.style;
    let content: Element<'a, Message> = match (v.tab, style) {
        (_, None) => text(t!("Select a style to view details.")).size(11).into(),
        (0, Some(style)) => column![
            input_row(t!("Description"), "", v.description, |value| Message::TableStyleEdit { field: "description", value }),
            row![
                text(t!("Flow direction")).size(11).style(muted_style).width(155),
                iced::widget::pick_list(
                    Some(EnumChoice { code: format!("{:?}", style.flow_direction), label: crate::i18n::translate(&format!("{:?}", style.flow_direction)) }),
                    choices(&[("Down", "Top to bottom"), ("Up", "Bottom to top")]),
                    |value| value.to_string(),
                )
                .on_select(|choice| Message::TableStyleSetFlow(choice.code))
                .text_size(11).width(170),
            ]
            .spacing(8).align_y(iced::Center),
            input_row(t!("Horizontal margin"), "1.5", v.hmargin, |value| Message::TableStyleEdit { field: "hmargin", value }),
            input_row(t!("Vertical margin"), "1.5", v.vmargin, |value| Message::TableStyleEdit { field: "vmargin", value }),
            checkbox(style.title_suppressed).label(t!("Suppress title row")).on_toggle(|_| Message::TableStyleToggle("title_sup")).size(14).text_size(11),
            checkbox(style.header_suppressed).label(t!("Suppress header row")).on_toggle(|_| Message::TableStyleToggle("header_sup")).size(14).text_size(11),
            checkbox(style.annotative).label(t!("Annotative")).on_toggle(|_| Message::TableStyleToggleAnnotative).size(14).text_size(11),
        ]
        .spacing(9)
        .into(),
        (tab, Some(_)) => cell_editor(&v, tab.saturating_sub(1).min(2)),
    };
    let rows = preview_rows(&v).unwrap_or_else(|| {
        std::array::from_fn(|row| PreviewRow {
            label: ["Data", "Header", "Title"][row].to_string(),
            text_style: String::new(),
            text_height: String::new(),
            text_color: String::new(),
            fill_color: String::new(),
            fill: false,
            alignment: "MiddleCenter".to_string(),
            data_type: String::new(),
            unit_type: String::new(),
            format: String::new(),
            borders: std::array::from_fn(|_| PreviewBorder {
                color: String::new(), weight: String::new(), spacing: String::new(), double: false, hidden: false,
            }),
        })
    });
    let preview = canvas(TablePreviewCanvas {
        rows,
        active_row: v.tab.saturating_sub(1).min(2) as usize,
        hmargin: v.hmargin.to_string(),
        vmargin: v.vmargin.to_string(),
        description: v.description.to_string(),
        flow_up: style.is_some_and(|style| format!("{:?}", style.flow_direction) == "Up"),
        title_suppressed: style.is_some_and(|style| style.title_suppressed),
        header_suppressed: style.is_some_and(|style| style.header_suppressed),
        annotative: style.is_some_and(|style| style.annotative),
    })
    .width(Length::Fill)
    .height(150);
    let status = if v.selected.eq_ignore_ascii_case(v.current) {
        t!("Current style")
    } else if v.in_use {
        t!("Used in drawing")
    } else {
        t!("Available style")
    };
    let summary = if v.comparison_sections.is_empty() {
        t!("No differences").into_owned()
    } else {
        format!("{}: {}", t!("Different"), v.comparison_sections.join(", "))
    };
    let editor = crate::ui::style::style_manager::editor_shell(
        crate::ui::style::style_manager::EditorShell {
            sizing,
            selected: v.selected.to_string(),
            status,
            preview: preview.into(),
            comparison: Some(crate::ui::style::style_manager::EditorComparison {
                selected: v.compare_name,
                options: v.compare_opts,
                summary,
                on_select: Message::TableStyleDialogCompare,
            }),
            tabs: [t!("General"), t!("Data Row"), t!("Header Row"), t!("Title Row")]
                .into_iter()
                .enumerate()
                .map(|(tab, label)| crate::ui::style::style_manager::EditorTab {
                    label,
                    active: v.tab == tab as u8,
                    on_press: Message::TableStyleDialogTab(tab as u8),
                })
                .collect(),
            content,
        },
    );
    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: crate::app::StyleKind::Table,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: Message::TableStyleDialogNew,
        on_copy: Message::TableStyleDialogCopy,
        on_delete: Message::TableStyleDialogDelete,
        on_select: Message::TableStyleDialogSelect,
        on_set_current: Message::TableStyleDialogSetCurrent,
        on_apply: Message::TableStyleApply,
        read_only: false,
        editor,
    })
}
