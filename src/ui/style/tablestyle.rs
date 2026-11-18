//! Table style manager.
//!
//! General page: description, table direction, annotative and the starting
//! table. The other pages edit one named cell style at a time (title,
//! header, data or a user cell style): its properties, text and edges.

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::textstyle::NameNotice;
use crate::{t, tr};
use document::objects::{NamedTableCellStyle, TableCellStyleData, TableStyle, TABLE_GRID_EDGES};
use iced::widget::{
    button, canvas, checkbox, column, container, pick_list, row, text, text_input, Space,
};
use iced::{mouse, Background, Border, Color, Element, Length, Point, Rectangle, Size, Theme};
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

fn choice(code: impl Into<String>, label: Cow<'static, str>) -> EnumChoice {
    EnumChoice {
        code: code.into(),
        label,
    }
}

fn translated(values: &[(&str, &str)]) -> Vec<EnumChoice> {
    values
        .iter()
        .map(|(code, label)| choice(*code, crate::i18n::translate(label)))
        .collect()
}

/// A choice list; the current value is shown even when it is not one of
/// the offered ones.
fn choice_list<'a>(
    current: &str,
    mut options: Vec<EnumChoice>,
    width: impl Into<Length>,
    size: u32,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    if !options.iter().any(|option| option.code == current) {
        options.push(choice(current, Cow::Owned(current.to_string())));
    }
    let selected = options
        .iter()
        .find(|option| option.code == current)
        .cloned();
    pick_list(selected, options, |option| option.to_string())
        .on_select(move |option: EnumChoice| on_select(option.code))
        .text_size(size as f32)
        .width(width)
        .into()
}

pub struct TableStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub current: &'a str,
    pub style: Option<&'a TableStyle>,
    pub tab: u8,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub in_use: bool,
    /// The list shows only the styles in use.
    pub in_use_only: bool,
    pub description: &'a str,
    /// The edited cell style and its text buffers.
    pub cell: &'a str,
    pub cell_height: &'a str,
    pub cell_angle: &'a str,
    pub cell_hmargin: &'a str,
    pub cell_vmargin: &'a str,
    pub text_styles: Vec<(String, document::Handle)>,
    pub linetypes: Vec<(String, document::Handle)>,
    /// The Borders page's line properties.
    pub border: &'a BorderDraft,
    pub cards: &'a TsCards,
    /// Select Linetype rows and the drawing's linetype names (Load).
    pub linetype_rows: Vec<crate::ui::style::mleaderstyle::LinetypeRow>,
    pub linetype_names: Vec<String>,
    /// The style's starting table.
    pub template: Option<&'a document::entities::Table>,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    pub color_open: Option<&'static str>,
    /// The cell style's data format with a sample ("General · 845.3479").
    pub format_sample: String,
    /// A table of the style uses the cell style Manage Cell Styles shows.
    pub manage_in_use: bool,
}

/// The name a cell style shows: the three row styles by their role.
pub fn cell_style_label(name: &str) -> Cow<'static, str> {
    match name {
        document::objects::TITLE_CELL_STYLE => t!("Title"),
        document::objects::HEADER_CELL_STYLE => t!("Header"),
        document::objects::DATA_CELL_STYLE => t!("Data"),
        other => Cow::Owned(other.to_string()),
    }
}

const ALIGNMENTS: [(&str, &str); 9] = [
    ("1", "Top left"),
    ("2", "Top center"),
    ("3", "Top right"),
    ("4", "Middle left"),
    ("5", "Middle center"),
    ("6", "Middle right"),
    ("7", "Bottom left"),
    ("8", "Bottom center"),
    ("9", "Bottom right"),
];

const LINE_WEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

fn line_weight_choices() -> Vec<EnumChoice> {
    let mut list = vec![choice("-1", t!("ByLayer")), choice("-2", t!("ByBlock"))];
    list.extend(LINE_WEIGHTS.iter().map(|w| {
        choice(
            w.to_string(),
            Cow::Owned(format!("{:.2} mm", *w as f64 / 100.0)),
        )
    }));
    list
}

/// A colour as a cell style field value: an ACI number (0 ByBlock, 256
/// ByLayer, 257 None) or `r,g,b` for a true colour.
pub fn color_code(color: document::types::Color) -> String {
    match color {
        document::types::Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
        other => crate::ui::color_select::color_to_aci_string(other),
    }
}

/// Reads [`color_code`].
pub fn parse_color_code(value: &str) -> Option<document::types::Color> {
    use document::types::Color as C;
    let value = value.trim();
    if let Some((r, rest)) = value.split_once(',') {
        let (g, b) = rest.split_once(',')?;
        return Some(C::Rgb {
            r: r.trim().parse().ok()?,
            g: g.trim().parse().ok()?,
            b: b.trim().parse().ok()?,
        });
    }
    match value.parse::<i16>().ok()? {
        0 => Some(C::ByBlock),
        256 => Some(C::ByLayer),
        257 => Some(C::None),
        index @ 1..=255 => Some(C::Index(index as u8)),
        _ => None,
    }
}

// ── Preview ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct PreviewEdge {
    hidden: bool,
    color: Option<Color>,
    width: f32,
    double: bool,
    dashed: bool,
}

#[derive(Clone)]
struct PreviewCell {
    fill: Option<Color>,
    text_color: Option<Color>,
    alignment: i32,
    height: f32,
    angle: f32,
    /// Top, inside horizontal, bottom, left, inside vertical, right.
    edges: [PreviewEdge; 6],
}

fn rgb(color: &document::types::Color) -> Option<Color> {
    use document::types::Color as C;
    match color {
        C::ByBlock | C::ByLayer | C::None => None,
        other => other.rgb().map(|(r, g, b)| Color::from_rgb8(r, g, b)),
    }
}

fn preview_cell(style: &TableCellStyleData) -> PreviewCell {
    let format = &style.content_format;
    let edges = std::array::from_fn(|slot| {
        let edge = TABLE_GRID_EDGES[slot];
        let grid = style.grid(edge);
        PreviewEdge {
            hidden: style.edge_hidden(edge),
            color: grid.and_then(|grid| rgb(&grid.border.color)),
            width: grid
                .map(|grid| grid.border.line_weight.value())
                .filter(|weight| *weight > 0)
                .map_or(1.0, |weight| (weight as f32 / 35.0).clamp(1.0, 4.0)),
            double: grid.is_some_and(|grid| {
                grid.border.border_type == document::objects::TableBorderType::Double
            }),
            dashed: grid.is_some_and(|grid| !grid.line_type.is_null()),
        }
    });
    PreviewCell {
        fill: rgb(&style.background_color),
        text_color: rgb(&format.content_color),
        alignment: format.cell_alignment,
        height: format.text_height as f32,
        angle: format.rotation as f32,
        edges,
    }
}

/// What the preview draws: a table, each row in its cell style.
enum PreviewLayout {
    Table {
        /// Column widths, row heights (drawing units) and each row's cell
        /// style; cells: text per row/column, merged ranges.
        widths: Vec<f32>,
        heights: Vec<f32>,
        rows: Vec<PreviewCell>,
        texts: Vec<Vec<String>>,
        merged: Vec<(usize, usize, usize, usize)>,
        flow_up: bool,
    },
}

struct TablePreviewCanvas {
    layout: PreviewLayout,
}

fn stroke_edge(frame: &mut canvas::Frame, a: Point, b: Point, edge: &PreviewEdge, ink: Color) {
    if edge.hidden {
        return;
    }
    let color = edge.color.unwrap_or(ink);
    let mut stroke = canvas::Stroke::default()
        .with_color(color)
        .with_width(edge.width);
    if edge.dashed {
        stroke.line_dash = canvas::LineDash {
            segments: &[6.0, 3.0],
            offset: 0,
        };
    }
    if edge.double {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length = (dx * dx + dy * dy).sqrt().max(1.0);
        let (nx, ny) = (-dy / length * 1.5, dx / length * 1.5);
        for sign in [-1.0f32, 1.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(a.x + nx * sign, a.y + ny * sign),
                    Point::new(b.x + nx * sign, b.y + ny * sign),
                ),
                stroke,
            );
        }
    } else {
        frame.stroke(&canvas::Path::line(a, b), stroke);
    }
}

fn draw_cell_text(
    frame: &mut canvas::Frame,
    cell: &PreviewCell,
    content: String,
    rect: Rectangle,
    size: f32,
    ink: Color,
) {
    if content.is_empty() {
        return;
    }
    let column = (cell.alignment - 1).rem_euclid(3);
    let line = ((cell.alignment - 1) / 3).clamp(0, 2);
    let pad = 4.0;
    let x = match column {
        0 => rect.x + pad,
        2 => rect.x + rect.width - pad,
        _ => rect.x + rect.width / 2.0,
    };
    let y = match line {
        0 => rect.y + pad + size / 2.0,
        2 => rect.y + rect.height - pad - size / 2.0,
        _ => rect.y + rect.height / 2.0,
    };
    let align_x = match column {
        0 => iced::advanced::text::Alignment::Left,
        2 => iced::advanced::text::Alignment::Right,
        _ => iced::advanced::text::Alignment::Center,
    };
    frame.with_save(|frame| {
        frame.translate(iced::Vector::new(x, y));
        frame.rotate(-cell.angle);
        frame.fill_text(canvas::Text {
            content,
            position: crate::ui::style::text::snap_point(Point::ORIGIN),
            color: cell.text_color.unwrap_or(ink),
            size: iced::Pixels(size),
            align_x,
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
    });
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
        match &self.layout {
            PreviewLayout::Table {
                widths,
                heights,
                rows,
                texts,
                merged,
                flow_up,
            } => {
                let total_w: f32 = widths.iter().sum::<f32>().max(1e-6);
                let total_h: f32 = heights.iter().sum::<f32>().max(1e-6);
                let scale = ((bounds.width - 24.0) / total_w).min((bounds.height - 16.0) / total_h);
                let left = (bounds.width - total_w * scale) / 2.0;
                let top = (bounds.height - total_h * scale) / 2.0;
                let mut xs = vec![left];
                for w in widths {
                    xs.push(xs.last().copied().unwrap_or(left) + w * scale);
                }
                let mut ys = vec![top];
                let order: Vec<usize> = if *flow_up {
                    (0..heights.len()).rev().collect()
                } else {
                    (0..heights.len()).collect()
                };
                let mut row_y = vec![0.0f32; heights.len()];
                let mut y = top;
                for &r in &order {
                    row_y[r] = y;
                    y += heights[r] * scale;
                    ys.push(y);
                }
                let owner = |r: usize, c: usize| {
                    merged
                        .iter()
                        .find(|(t, l, b, rt)| r >= *t && r <= *b && c >= *l && c <= *rt)
                        .copied()
                };
                for (r, cell) in rows.iter().enumerate() {
                    for c in 0..widths.len() {
                        let (t, l, b, rt) = owner(r, c).unwrap_or((r, c, r, c));
                        if (t, l) != (r, c) {
                            continue;
                        }
                        let x0 = xs[l];
                        let x1 = xs[(rt + 1).min(widths.len())];
                        let (y0, y1) = if *flow_up {
                            (row_y[b], row_y[t] + heights[t] * scale)
                        } else {
                            (row_y[t], row_y[b] + heights[b] * scale)
                        };
                        let rect = Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0));
                        if let Some(fill) = cell.fill {
                            frame.fill_rectangle(rect.position(), rect.size(), fill);
                        }
                        let size = (cell.height * scale).clamp(6.0, 16.0);
                        let content = texts
                            .get(r)
                            .and_then(|row| row.get(c))
                            .cloned()
                            .unwrap_or_default();
                        draw_cell_text(&mut frame, cell, content, rect, size, ink);
                        let e = &cell.edges;
                        stroke_edge(
                            &mut frame,
                            Point::new(x0, y0),
                            Point::new(x1, y0),
                            &e[0],
                            ink,
                        );
                        stroke_edge(
                            &mut frame,
                            Point::new(x0, y1),
                            Point::new(x1, y1),
                            &e[2],
                            ink,
                        );
                        stroke_edge(
                            &mut frame,
                            Point::new(x0, y0),
                            Point::new(x0, y1),
                            &e[3],
                            ink,
                        );
                        stroke_edge(
                            &mut frame,
                            Point::new(x1, y0),
                            Point::new(x1, y1),
                            &e[5],
                            ink,
                        );
                    }
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The general page's preview: the starting table, or a title, a header and
/// three data rows.
fn table_preview(v: &TableStyleView<'_>, style: &TableStyle) -> PreviewLayout {
    let by_id = |id: i32, fallback: &str| {
        style
            .cell_style_by_id(id)
            .or_else(|| style.cell_style(fallback))
            .map(|cell| preview_cell(&cell.cell_style))
            .unwrap_or_else(|| preview_cell(&TableCellStyleData::default()))
    };
    let flow_up = style.flow_direction == document::objects::TableFlowDirection::Up;
    if let Some(table) = v.template {
        let rows = table
            .rows
            .iter()
            .map(|row| by_id(row.style_id, document::objects::DATA_CELL_STYLE))
            .collect();
        return PreviewLayout::Table {
            widths: table.columns.iter().map(|c| c.width as f32).collect(),
            heights: table.rows.iter().map(|r| r.height as f32).collect(),
            rows,
            texts: table
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(|cell| cell.text_value().to_string())
                        .collect()
                })
                .collect(),
            merged: table
                .merged_ranges
                .iter()
                .map(|r| (r.top_row, r.left_col, r.bottom_row, r.right_col))
                .collect(),
            flow_up,
        };
    }
    let title = style.cell_style(document::objects::TITLE_CELL_STYLE);
    let header = style.cell_style(document::objects::HEADER_CELL_STYLE);
    let data = style.cell_style(document::objects::DATA_CELL_STYLE);
    let height = |cell: Option<&NamedTableCellStyle>| {
        cell.map_or(1.0, |cell| {
            let format = &cell.cell_style.content_format;
            let margin = cell
                .cell_style
                .margins()
                .map_or(style.vertical_margin, |(_, v)| v);
            (format.text_height * 4.0 / 3.0 + 2.0 * margin) as f32
        })
    };
    let preview = |cell: Option<&NamedTableCellStyle>| {
        cell.map(|cell| preview_cell(&cell.cell_style))
            .unwrap_or_else(|| preview_cell(&TableCellStyleData::default()))
    };
    let mut rows = vec![preview(title), preview(header)];
    let mut heights = vec![height(title), height(header)];
    let mut texts = vec![
        vec![t!("Title").into_owned(), String::new(), String::new()],
        vec![t!("Header").into_owned(); 3],
    ];
    for _ in 0..3 {
        rows.push(preview(data));
        heights.push(height(data));
        texts.push(vec![t!("Data").into_owned(); 3]);
    }
    let column = (heights[2] * 4.0).max(1.0);
    PreviewLayout::Table {
        widths: vec![column; 3],
        heights,
        rows,
        texts,
        merged: vec![(0, 0, 0, 2)],
        flow_up,
    }
}

// ── Pages ──────────────────────────────────────────────────────────────────

fn cell_color<'a>(
    v: &TableStyleView<'a>,
    field: &'static str,
    color: &document::types::Color,
    none: bool,
) -> Element<'a, Message> {
    let current = *color;
    let selector = crate::ui::color_select::color_selector(
        current,
        v.color_open == Some(field),
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: true,
            none,
            ..Default::default()
        },
        move |color| Message::TableStyleCellEdit {
            field,
            value: crate::ui::color_select::color_to_aci_string(color),
        },
        Message::TableColorMore(field),
        Message::OpenColorWindow(crate::app::ColorPickTarget::Table(field), current),
    );
    iced::widget::container(selector).width(Length::Fill).into()
}

fn cells(message: TsCellMsg) -> Message {
    Message::TableStyleCells(message)
}

/// The style's cell styles in list order: by their shown name.
pub fn sorted_cell_styles(style: &TableStyle) -> Vec<&NamedTableCellStyle> {
    let mut list: Vec<&NamedTableCellStyle> = style.cell_styles().collect();
    list.sort_by_key(|cell| cell_style_label(&cell.name).to_lowercase());
    list
}

/// A small "…" button beside a field.
fn more_button<'a>(on_press: Message) -> Element<'a, Message> {
    button(text("…").size(11))
        .on_press(on_press)
        .style(button::secondary)
        .padding([3, 8])
        .into()
}

// ── Page cards ─────────────────────────────────────────────────────────────

const GRID_LABEL_W: f32 = 104.0;

/// A caption and its control, the controls lined up in a card.
fn grid_row<'a>(
    name: Cow<'static, str>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        text(name).size(11).width(GRID_LABEL_W),
        container(control.into()).width(Length::Fill)
    ]
    .spacing(10)
    .align_y(iced::Center)
    .into()
}

/// One choice out of a few, as joined buttons.
fn segmented<'a>(options: Vec<(Cow<'static, str>, bool, Message)>) -> Element<'a, Message> {
    let buttons: Vec<Element<'a, Message>> = options
        .into_iter()
        .map(|(label, on, message)| {
            button(text(label).size(11))
                .on_press(message)
                .style(crate::ui::style::form::button_style(on))
                .padding([4, 12])
                .into()
        })
        .collect();
    container(row(buttons).spacing(2))
        .padding(2)
        .style(crate::ui::window::pdf_dialogs::well_style)
        .into()
}

/// The 3×3 alignment indicator: a click picks that alignment.
fn alignment_grid<'a>(current: i32) -> Element<'a, Message> {
    let dot = |code: i32| -> Element<'a, Message> {
        let on = code == current;
        button(Space::new().width(9).height(9))
            .on_press(Message::TableStyleCellEdit {
                field: "align",
                value: code.to_string(),
            })
            .padding(0)
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let color = if on {
                    palette.primary.base.color
                } else if matches!(status, button::Status::Hovered) {
                    palette.background.strong.color
                } else {
                    palette.background.neutral.color
                };
                button::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
    };
    column((0..3).map(|line| {
        row((1..=3).map(|column| dot(line * 3 + column)))
            .spacing(2)
            .into()
    }))
    .spacing(2)
    .into()
}

/// A cell style's swatch: its fill inside its top edge colour.
fn cell_swatch<'a>(data: &TableCellStyleData) -> Element<'a, Message> {
    let fill = rgb(&data.background_color);
    let edge = data
        .grid(TABLE_GRID_EDGES[0])
        .and_then(|grid| rgb(&grid.border.color));
    container(Space::new().width(16).height(12))
        .style(move |theme: &Theme| container::Style {
            background: fill.map(Background::Color),
            border: Border {
                color: edge.unwrap_or(theme.palette().background.base.text),
                width: 1.5,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn card_heading<'a>(title: String) -> Element<'a, Message> {
    text(title.to_uppercase())
        .size(10)
        .style(crate::ui::window::pdf_dialogs::accent_text)
        .into()
}

/// The cell style column: the list with its swatches, New / Rename /
/// Delete / Manage, and the table preview.
fn cell_column<'a>(v: &TableStyleView<'a>, style: &'a TableStyle) -> Element<'a, Message> {
    use crate::ui::window::pdf_dialogs::{card_style, well_style};
    use iced::widget::mouse_area;
    let rows: Vec<Element<'a, Message>> = sorted_cell_styles(style)
        .into_iter()
        .map(|cell| {
            let on = cell.name.eq_ignore_ascii_case(v.cell);
            let content = row![
                cell_swatch(&cell.cell_style),
                text(cell_style_label(&cell.name)).size(11)
            ]
            .spacing(8)
            .align_y(iced::Center);
            mouse_area(
                container(content)
                    .padding([6, 8])
                    .width(Length::Fill)
                    .style(move |theme: &Theme| {
                        let pair = theme.palette().primary.strong;
                        container::Style {
                            background: on.then_some(Background::Color(pair.color)),
                            text_color: on.then_some(pair.text),
                            border: Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
            )
            .on_press(Message::TableStyleCellSelect(cell.name.clone()))
            .on_double_click(cells(TsCellMsg::QuickRename))
            .into()
        })
        .collect();
    let user = !document::objects::is_row_cell_style(v.cell);
    let plus = button(crate::ui::icons::tinted(
        crate::ui::icons::PLUS,
        12.0,
        |theme: &Theme| theme.palette().primary.base.color,
    ))
    .on_press(cells(TsCellMsg::CreateOpen))
    .style(button::text)
    .padding([2, 4]);
    let trash = button(crate::ui::icons::tinted_faded(
        crate::ui::icons::TRASH,
        13.0,
        if user { 1.0 } else { 0.42 },
        |theme: &Theme| theme.palette().background.base.text,
    ))
    .on_press_maybe(user.then(|| cells(TsCellMsg::QuickDelete)))
    .style(crate::ui::style::form::button_style(false))
    .padding([5, 8]);
    let rename = button(
        text(tr!("table-style-cells", "rename"))
            .size(11)
            .width(Length::Fill)
            .align_x(iced::Center),
    )
    .on_press_maybe(user.then(|| cells(TsCellMsg::QuickRename)))
    .style(crate::ui::style::form::button_style(false))
    .padding([5, 8])
    .width(Length::Fill);
    let manage = button(
        text(tr!("table-style-cells", "manage-button"))
            .size(11)
            .width(Length::Fill)
            .align_x(iced::Center),
    )
    .on_press(cells(TsCellMsg::ManageOpen))
    .style(crate::ui::style::form::button_style(false))
    .padding([5, 8])
    .width(Length::Fill);
    let preview = container(
        canvas(TablePreviewCanvas {
            layout: table_preview(v, style),
        })
        .width(Length::Fill)
        .height(110),
    )
    .style(well_style);
    container(
        column![
            row![
                card_heading(tr!("table-style-cells", "cell-styles")),
                Space::new().width(Length::Fill),
                plus,
            ]
            .align_y(iced::Center),
            container(column(rows).spacing(2))
                .padding(4)
                .width(Length::Fill)
                .style(well_style),
            row![rename, trash].spacing(6),
            manage,
            Space::new().height(6),
            card_heading(tr!("table-style-cells", "preview-short")),
            preview,
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fixed(200.0))
    .style(card_style)
    .into()
}

/// Fill and alignment: fill colour, alignment with its 3×3 indicator and
/// the cell type.
fn fill_card<'a>(v: &TableStyleView<'a>, cell: &NamedTableCellStyle) -> Element<'a, Message> {
    let data = &cell.cell_style;
    let alignment = data.content_format.cell_alignment;
    let data_type = cell.style_type == 2;
    crate::ui::window::pdf_dialogs::card(
        tr!("table-style-cells", "fill-alignment"),
        column![
            grid_row(
                t!("Fill color"),
                cell_color(v, "fillcolor", &data.background_color, true)
            ),
            grid_row(
                t!("Alignment"),
                row![
                    choice_list(
                        &alignment.to_string(),
                        translated(&ALIGNMENTS),
                        Length::Fill,
                        11,
                        |value| Message::TableStyleCellEdit {
                            field: "align",
                            value
                        }
                    ),
                    alignment_grid(alignment),
                ]
                .spacing(8)
                .align_y(iced::Center)
            ),
            grid_row(
                t!("Type"),
                segmented(vec![
                    (
                        t!("Data"),
                        data_type,
                        Message::TableStyleCellEdit {
                            field: "class",
                            value: "2".into()
                        }
                    ),
                    (
                        t!("Label"),
                        !data_type,
                        Message::TableStyleCellEdit {
                            field: "class",
                            value: "1".into()
                        }
                    ),
                ])
            ),
        ]
        .spacing(8),
    )
}

/// The data format with its sample and Change…, and the cell margins.
fn format_card<'a>(v: &TableStyleView<'a>) -> Element<'a, Message> {
    let margin = |field: &'static str, value: &'a str| -> Element<'a, Message> {
        text_input("1.5", value)
            .on_input(move |value| Message::TableStyleCellEdit { field, value })
            .style(crate::ui::style::form::field_style)
            .size(11)
            .width(80)
            .into()
    };
    crate::ui::window::pdf_dialogs::card(
        t!("Format").into_owned(),
        column![
            row![
                crate::ui::read_only::field(&v.format_sample, 11.0, Length::Fill),
                button(text(tr!("table-style-cells", "change")).size(11))
                    .on_press(cells(TsCellMsg::FormatOpen))
                    .style(crate::ui::style::form::button_style(false))
                    .padding([4, 10]),
            ]
            .spacing(8)
            .align_y(iced::Center),
            Space::new().height(4),
            card_heading(t!("Margins").into_owned()),
            row![
                text(t!("Horizontal")).size(11),
                margin("hmargin", v.cell_hmargin),
                text(t!("Vertical")).size(11),
                margin("vmargin", v.cell_vmargin),
            ]
            .spacing(10)
            .align_y(iced::Center),
        ]
        .spacing(8),
    )
}

/// Text style, colour, height and angle in two columns.
fn text_card<'a>(v: &TableStyleView<'a>, cell: &NamedTableCellStyle) -> Element<'a, Message> {
    let format = &cell.cell_style.content_format;
    let styles: Vec<EnumChoice> = v
        .text_styles
        .iter()
        .map(|(name, handle)| choice(format!("{:X}", handle.value()), Cow::Owned(name.clone())))
        .collect();
    let input = |field: &'static str, value: &'a str, placeholder: &'static str| {
        text_input(placeholder, value)
            .on_input(move |value| Message::TableStyleCellEdit { field, value })
            .style(crate::ui::style::form::field_style)
            .size(11)
            .width(Length::Fill)
    };
    crate::ui::window::pdf_dialogs::card(
        t!("Text").into_owned(),
        row![
            column![
                grid_row(
                    t!("Text style"),
                    row![
                        choice_list(
                            &format!("{:X}", format.text_style.value()),
                            styles,
                            Length::Fill,
                            11,
                            |value| Message::TableStyleCellEdit {
                                field: "textstyle",
                                value
                            }
                        ),
                        more_button(cells(TsCellMsg::TextStyles)),
                    ]
                    .spacing(6)
                    .align_y(iced::Center)
                ),
                grid_row(
                    t!("Color"),
                    cell_color(v, "textcolor", &format.content_color, false)
                ),
            ]
            .spacing(8)
            .width(Length::Fill),
            column![
                grid_row(t!("Height"), input("height", v.cell_height, "4.5")),
                grid_row(t!("Angle"), input("angle", v.cell_angle, "0")),
            ]
            .spacing(8)
            .width(Length::Fill),
        ]
        .spacing(14),
    )
}

/// The line the edge tiles apply: lineweight and linetype with their
/// samples, colour, double line and its spacing.
fn line_card<'a>(v: &TableStyleView<'a>) -> Element<'a, Message> {
    use crate::ui::style::mleaderstyle::{pattern_sample, sample_list, weight_sample};
    let border = v.border;
    let weight =
        |code: &str| document::types::LineWeight::from_value(code.parse::<i16>().unwrap_or(-2));
    let weights = line_weight_choices();
    let current_weight = border.line_weight.to_string();
    let weight_label = weights
        .iter()
        .find(|choice| choice.code == current_weight)
        .map(|choice| choice.label.to_string())
        .unwrap_or_else(|| current_weight.clone());
    let lineweight = sample_list(
        (weight_sample(weight(&current_weight)), weight_label),
        weights
            .iter()
            .map(|choice| {
                (
                    weight_sample(weight(&choice.code)),
                    choice.label.to_string(),
                    cells(TsCellMsg::BorderLineweight(choice.code.clone())),
                )
            })
            .collect(),
        Message::TableColorMore("edgeweight"),
        v.color_open == Some("edgeweight"),
        true,
    );
    let pattern_of = |name: &str| -> Vec<f64> {
        v.linetype_rows
            .iter()
            .find(|row| row.name.eq_ignore_ascii_case(name))
            .map(|row| row.pattern.clone())
            .unwrap_or_default()
    };
    let mut linetypes: Vec<(String, String)> = vec![("0".into(), t!("ByBlock").into_owned())];
    linetypes.extend(
        v.linetypes
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("ByBlock"))
            .map(|(name, handle)| (format!("{:X}", handle.value()), name.clone())),
    );
    let current_name = linetypes
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(&border.linetype))
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| border.linetype.clone());
    let mut entries: Vec<(Element<'a, Message>, String, Message)> = linetypes
        .iter()
        .map(|(code, name)| {
            (
                pattern_sample(pattern_of(name)),
                name.clone(),
                cells(TsCellMsg::BorderLinetype(code.clone())),
            )
        })
        .collect();
    entries.push((
        Space::new().width(48).into(),
        tr!("table-style-cells", "other-linetype"),
        cells(TsCellMsg::BorderLinetype(OTHER_LINETYPE.to_string())),
    ));
    let linetype = sample_list(
        (pattern_sample(pattern_of(&current_name)), current_name),
        entries,
        Message::TableColorMore("edgelinetype"),
        v.color_open == Some("edgelinetype"),
        true,
    );
    let color = border.color;
    let color_field = crate::ui::color_select::color_selector(
        color,
        v.color_open == Some("edgecolor"),
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: true,
            standard_only: true,
            ..Default::default()
        },
        |color| cells(TsCellMsg::BorderColor(color)),
        Message::TableColorMore("edgecolor"),
        Message::OpenColorWindow(crate::app::ColorPickTarget::Table("edgecolor"), color),
    );
    let mut spacing = text_input("", &border.spacing)
        .style(crate::ui::style::form::field_style)
        .size(11)
        .width(90);
    if border.double {
        spacing = spacing.on_input(|value| cells(TsCellMsg::BorderSpacing(value)));
    }
    crate::ui::window::pdf_dialogs::card(
        tr!("table-style-cells", "line-card"),
        column![
            grid_row(tr!("table-cells", "lineweight").into(), lineweight),
            grid_row(tr!("table-cells", "linetype").into(), linetype),
            grid_row(tr!("table-cells", "color").into(), color_field),
            row![
                checkbox(border.double)
                    .label(tr!("table-cells", "double"))
                    .on_toggle(|on| cells(TsCellMsg::BorderDouble(on)))
                    .size(14)
                    .text_size(11),
                Space::new().width(Length::Fill),
                text(tr!("table-cells", "spacing")).size(11),
                spacing,
            ]
            .spacing(10)
            .align_y(iced::Center),
        ]
        .spacing(8),
    )
}

/// A border tile's diagram: a 2×2 cell block with the edges it applies to
/// drawn solid (top 1, inside horizontal 2, bottom 4, left 8, inside
/// vertical 16, right 32) and the others dotted.
struct BorderTile {
    mask: i32,
}

impl canvas::Program<Message> for BorderTile {
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
        let palette = theme.palette();
        let (x0, y0) = (2.0f32, 2.0f32);
        let (x2, y2) = (bounds.width - 2.0, bounds.height - 2.0);
        let (x1, y1) = ((x0 + x2) / 2.0, (y0 + y2) / 2.0);
        let lines = [
            (1, Point::new(x0, y0), Point::new(x2, y0)),
            (2, Point::new(x0, y1), Point::new(x2, y1)),
            (4, Point::new(x0, y2), Point::new(x2, y2)),
            (8, Point::new(x0, y0), Point::new(x0, y2)),
            (16, Point::new(x1, y0), Point::new(x1, y2)),
            (32, Point::new(x2, y0), Point::new(x2, y2)),
        ];
        for (bit, a, b) in lines {
            let stroke = if self.mask & bit != 0 {
                canvas::Stroke::default()
                    .with_color(palette.primary.base.color)
                    .with_width(2.0)
            } else {
                canvas::Stroke {
                    line_dash: canvas::LineDash {
                        segments: &[2.0, 2.0],
                        offset: 0,
                    },
                    ..canvas::Stroke::default()
                        .with_color(palette.background.base.text.scale_alpha(0.45))
                        .with_width(1.0)
                }
            };
            frame.stroke(&canvas::Path::line(a, b), stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// The tiles in the order they show, as indexes of [`BORDER_BUTTONS`],
/// with the edges each one's diagram marks.
const TILE_ORDER: [(usize, i32); 8] = [
    (0, 63),
    (1, 1 | 4 | 8 | 32),
    (2, 2 | 16),
    (5, 1),
    (3, 4),
    (4, 8),
    (6, 32),
    (7, 0),
];

/// The edges to apply the line to: a tile per edge set, the last one
/// applied highlighted.
fn edges_card<'a>(v: &TableStyleView<'a>) -> Element<'a, Message> {
    let tiles: Vec<Element<'a, Message>> = TILE_ORDER
        .iter()
        .map(|&(index, mask)| {
            let on = v.border.last == Some(index as u8);
            let label = if index == 7 {
                t!("None").into_owned()
            } else {
                border_label(index)
            };
            button(
                column![
                    canvas(BorderTile { mask }).width(24).height(24),
                    text(label).size(10).align_x(iced::Center),
                ]
                .spacing(4)
                .align_x(iced::Center),
            )
            .on_press(cells(TsCellMsg::BorderApply(index as u8)))
            .padding([6, 4])
            .width(Length::Fixed(58.0))
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(if hovered {
                        palette.background.strong.color
                    } else {
                        palette.background.base.color
                    })),
                    text_color: palette.background.base.text,
                    border: Border {
                        color: if on {
                            palette.primary.base.color
                        } else {
                            palette.background.neutral.color
                        },
                        width: if on { 2.0 } else { 1.0 },
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
        })
        .collect();
    crate::ui::window::pdf_dialogs::card(
        tr!("table-style-cells", "apply-edges"),
        column![
            row(tiles).spacing(6).wrap().vertical_spacing(6),
            text(tr!("table-style-cells", "edges-hint"))
                .size(10)
                .style(muted_style),
        ]
        .spacing(8),
    )
}

/// The table style's own settings: description, title and header rows,
/// annotative, the starting table and the comparison.
fn table_card<'a>(v: &TableStyleView<'a>, style: &TableStyle) -> Element<'a, Message> {
    let template_summary = v.template.map_or_else(
        || t!("None").into_owned(),
        |table| {
            t!(
                "%{rows} rows × %{columns} columns",
                rows = table.rows.len(),
                columns = table.columns.len()
            )
            .into_owned()
        },
    );
    let mut remove = button(text(t!("Remove")).size(11))
        .style(crate::ui::style::form::button_style(false))
        .padding([4, 10]);
    if v.template.is_some() {
        remove = remove.on_press(Message::TableStyleTemplateRemove);
    }
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
    crate::ui::window::pdf_dialogs::card(
        tr!("table-style-cells", "table-card"),
        column![
            text(status).size(10).style(muted_style),
            grid_row(
                t!("Description"),
                text_input("", v.description)
                    .on_input(|value| Message::TableStyleEdit {
                        field: "description",
                        value,
                    })
                    .style(crate::ui::style::form::field_style)
                    .size(11)
                    .width(Length::Fill)
            ),
            row![
                checkbox(style.title_suppressed)
                    .label(t!("Suppress title row"))
                    .on_toggle(|_| Message::TableStyleToggle("title_sup"))
                    .size(14)
                    .text_size(11),
                checkbox(style.header_suppressed)
                    .label(t!("Suppress header row"))
                    .on_toggle(|_| Message::TableStyleToggle("header_sup"))
                    .size(14)
                    .text_size(11),
                checkbox(style.annotative)
                    .label(t!("Annotative"))
                    .on_toggle(|_| Message::TableStyleToggleAnnotative)
                    .size(14)
                    .text_size(11),
            ]
            .spacing(18)
            .wrap(),
            grid_row(
                t!("Starting table"),
                row![
                    text(template_summary).size(11).width(Length::Fill),
                    button(text(t!("Select table…")).size(11))
                        .on_press(Message::TableStyleTemplatePick)
                        .style(crate::ui::style::form::button_style(false))
                        .padding([4, 10]),
                    remove,
                ]
                .spacing(6)
                .align_y(iced::Center)
            ),
            crate::ui::style::style_manager::comparison_row(Some(
                crate::ui::style::style_manager::EditorComparison {
                    selected: v.compare_name.clone(),
                    options: v.compare_opts.clone(),
                    summary,
                    on_select: Message::TableStyleDialogCompare,
                }
            )),
        ]
        .spacing(8),
    )
}

/// The single page: the cell style column beside the editor's cards.
fn page<'a>(v: &TableStyleView<'a>, style: &'a TableStyle) -> Element<'a, Message> {
    use crate::ui::window::pdf_dialogs::{accent_text, card_style};
    let Some(cell) = style.cell_style(v.cell) else {
        return text(t!("Select a style to view details.")).size(11).into();
    };
    let up = style.flow_direction == document::objects::TableFlowDirection::Up;
    let header = container(
        row![
            text(cell_style_label(&cell.name)).size(15).style(accent_text),
            text(format!(
                "{} · {}",
                tr!("table-style-cells", "cell-style-of"),
                v.selected
            ))
            .size(11)
            .style(muted_style),
            Space::new().width(Length::Fill),
            segmented(vec![
                (t!("Down"), !up, Message::TableStyleSetFlow("Down".into())),
                (t!("Up"), up, Message::TableStyleSetFlow("Up".into())),
            ]),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .style(card_style);
    let merge = container(
        checkbox(cell.cell_style.merge_all())
            .label(t!("Merge cells on row/column creation"))
            .on_toggle(|on| Message::TableStyleCellEdit {
                field: "merge",
                value: if on { "1" } else { "0" }.to_string(),
            })
            .size(14)
            .text_size(11),
    )
    .padding([10, 12])
    .width(Length::Fill)
    .style(card_style);
    let cards = column![
        header,
        row![fill_card(v, cell), format_card(v)].spacing(12),
        merge,
        text_card(v, cell),
        row![line_card(v), edges_card(v)].spacing(12),
        table_card(v, style),
    ]
    .spacing(12)
    .width(Length::Fill);
    iced::widget::scrollable(
        container(row![cell_column(v, style), cards].spacing(12))
            .padding(iced::Padding::ZERO.right(12).bottom(4)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The edges each border button applies the line properties to (top 1,
/// inside horizontal 2, bottom 4, left 8, inside vertical 16, right 32),
/// in button order; the last one hides every edge.
pub const BORDER_BUTTONS: [(&str, i32); 8] = [
    ("all", 63),
    ("outside", 1 | 4 | 8 | 32),
    ("inside", 2 | 16),
    ("bottom", 4 | 2),
    ("left", 8 | 16),
    ("top", 1 | 2),
    ("right", 32 | 16),
    ("none", 63),
];

fn border_label(index: usize) -> String {
    match index {
        0 => tr!("table-cells", "all"),
        1 => tr!("table-cells", "outside"),
        2 => tr!("table-cells", "inside"),
        3 => tr!("table-cells", "bottom"),
        4 => tr!("table-cells", "left"),
        5 => tr!("table-cells", "top"),
        6 => tr!("table-cells", "right"),
        _ => tr!("table-cells", "none"),
    }
}

/// The linetype list's code that opens Select Linetype.
pub const OTHER_LINETYPE: &str = "other";

// ── Cell style cards ───────────────────────────────────────────────────────

/// New Cell Style: the name being typed and the cell style it copies.
#[derive(Debug, Clone)]
pub struct CreateCell {
    pub name: String,
    pub start: String,
    pub notice: Option<NameNotice>,
    /// Opened from Manage Cell Styles, which it returns to.
    pub from_manage: bool,
}

/// Manage Cell Styles: the cell styles as they were (Cancel restores
/// them), the selected one, an inline rename and the delete question.
#[derive(Debug, Clone)]
pub struct ManageCells {
    pub before: Vec<(i32, NamedTableCellStyle)>,
    pub selected: String,
    pub rename: Option<String>,
    pub notice: Option<NameNotice>,
    pub delete_ask: bool,
    /// Delete refused: a table of the style uses the cell style.
    pub in_use: bool,
}

/// The cards over the table style window.
#[derive(Default)]
pub struct TsCards {
    pub create: Option<CreateCell>,
    pub manage: Option<ManageCells>,
    /// Select Linetype for the border properties, with the picked name.
    pub linetype: Option<String>,
    pub load: Option<crate::ui::style::mleaderstyle::LoadDraft>,
    pub reload_ask: Option<crate::ui::style::mleaderstyle::ReloadAsk>,
}

/// The line properties the Borders page applies with its buttons. They
/// start over whenever another cell style is edited.
#[derive(Debug, Clone)]
pub struct BorderDraft {
    pub line_weight: i16,
    /// Linetype handle (hex), `0` for ByBlock.
    pub linetype: String,
    pub color: document::types::Color,
    pub double: bool,
    pub spacing: String,
    /// The edge tile applied last, highlighted (an index of
    /// [`BORDER_BUTTONS`]).
    pub last: Option<u8>,
}

impl BorderDraft {
    pub fn new(spacing: f64) -> Self {
        Self {
            line_weight: -2,
            linetype: "0".into(),
            color: document::types::Color::ByBlock,
            double: false,
            spacing: crate::ui::style::mleaderstyle::trim_number(spacing),
            last: None,
        }
    }
}

impl Default for BorderDraft {
    fn default() -> Self {
        Self::new(1.125)
    }
}

#[derive(Debug, Clone)]
pub enum TsCellMsg {
    CreateOpen,
    CreateName(String),
    CreateStart(String),
    CreateOk,
    CreateCancel,
    NoticeClose,
    ManageOpen,
    ManageSelect(String),
    ManageNew,
    RenameStart,
    RenameText(String),
    RenameCommit,
    DeleteAsk,
    DeleteAnswer(bool),
    ManageOk,
    ManageCancel,
    /// "?" in Manage Cell Styles.
    ManageHelp,
    /// Rename / Delete beside the cell style list: Manage Cell Styles
    /// opens on the edited cell style with its name ready or the
    /// question asked.
    QuickRename,
    QuickDelete,
    /// Format "…": the Cell Data Format window for the cell style.
    FormatOpen,
    /// Text style "…": the Text Style manager.
    TextStyles,
    BorderLineweight(String),
    BorderLinetype(String),
    BorderColor(document::types::Color),
    BorderDouble(bool),
    BorderSpacing(String),
    BorderApply(u8),
    LinetypePick(String),
    LinetypeOk,
    LinetypeCancel,
    LinetypeLoad,
    LoadFile,
    LoadFileChosen(Option<crate::io::PickedLinetypeFile>),
    LoadToggle(String),
    LoadOk,
    LoadCancel,
    ReloadAnswer(crate::ui::style::mleaderstyle::Reload),
}

fn card_title<'a>(label: String) -> Element<'a, Message> {
    text(label)
        .size(13)
        .style(crate::ui::window::pdf_dialogs::accent_text)
        .into()
}

fn notice_card<'a>(notice: &NameNotice) -> Element<'a, Message> {
    use crate::ui::style::form::dialog_button;
    let body = column![
        crate::ui::style::textstyle::name_notice_lines(notice),
        row![
            Space::new().width(Length::Fill),
            dialog_button(tr!("text-style", "close"), cells(TsCellMsg::NoticeClose), true),
        ],
    ]
    .spacing(14);
    crate::ui::style::mleaderstyle::panel(body.into(), 420.0)
}

fn create_card<'a>(draft: &CreateCell, style: &TableStyle) -> Element<'a, Message> {
    use crate::ui::style::form::dialog_button;
    let starts: Vec<EnumChoice> = sorted_cell_styles(style)
        .into_iter()
        .map(|cell| choice(cell.name.clone(), cell_style_label(&cell.name)))
        .collect();
    let body = column![
        card_title(tr!("table-style-cells", "new-title")),
        column![
            text(tr!("table-style-cells", "new-style-name"))
                .size(11)
                .style(muted_style),
            text_input("", &draft.name)
                .id(iced::widget::Id::new(CREATE_NAME_ID))
                .on_input(|value| cells(TsCellMsg::CreateName(value)))
                .on_submit(cells(TsCellMsg::CreateOk))
                .size(12)
                .padding([5, 8])
                .style(crate::ui::style::form::field_style)
                .width(Length::Fill),
        ]
        .spacing(4),
        column![
            text(tr!("table-style-cells", "start-with"))
                .size(11)
                .style(muted_style),
            choice_list(&draft.start, starts, 384.0, 12, |value| cells(
                TsCellMsg::CreateStart(value)
            )),
        ]
        .spacing(4),
        row![
            Space::new().width(Length::Fill),
            dialog_button(tr!("table-style-cells", "cancel"), cells(TsCellMsg::CreateCancel), false),
            dialog_button(tr!("table-style-cells", "continue"), cells(TsCellMsg::CreateOk), true),
        ]
        .spacing(6),
    ]
    .spacing(12);
    crate::ui::style::mleaderstyle::panel(body.into(), 420.0)
}

pub const CREATE_NAME_ID: &str = "ts-cell-new-name";
pub const RENAME_ID: &str = "ts-cell-rename";

fn manage_card<'a>(manage: &ManageCells, style: &TableStyle, in_use: bool) -> Element<'a, Message> {
    use crate::ui::style::form::{button_style, dialog_button};
    use crate::ui::window::pdf_dialogs::{card_style, well_style};
    if manage.delete_ask || manage.in_use {
        let (question, buttons): (String, Element<'a, Message>) = if manage.in_use {
            (
                tr!("table-style-cells", "cell-style-in-use"),
                row![
                    Space::new().width(Length::Fill),
                    dialog_button(tr!("text-style", "close"), cells(TsCellMsg::DeleteAnswer(false)), true),
                ]
                .into(),
            )
        } else {
            (
                tr!("table-style-cells", "delete-ask"),
                row![
                    Space::new().width(Length::Fill),
                    dialog_button(tr!("table-style-cells", "cancel"), cells(TsCellMsg::DeleteAnswer(false)), false),
                    dialog_button(tr!("table-style-cells", "ok"), cells(TsCellMsg::DeleteAnswer(true)), true),
                ]
                .spacing(6)
                .into(),
            )
        };
        let body = column![text(question).size(13), buttons].spacing(14);
        return crate::ui::style::mleaderstyle::panel(body.into(), 420.0);
    }
    // The cell styles as one segmented row, then New as a dashed chip.
    let mut styles: Vec<Element<'a, Message>> = sorted_cell_styles(style)
        .into_iter()
        .map(|cell| {
            let on = cell.name.eq_ignore_ascii_case(&manage.selected);
            button(text(cell_style_label(&cell.name)).size(11))
                .on_press(cells(TsCellMsg::ManageSelect(cell.name.clone())))
                .style(button_style(on))
                .padding([5, 12])
                .into()
        })
        .collect();
    let segments = container(row(std::mem::take(&mut styles)).spacing(2).wrap())
        .padding(2)
        .style(well_style);
    let new_chip = button(
        row![
            crate::ui::icons::tinted(crate::ui::icons::PLUS, 11.0, |theme: &Theme| {
                theme.palette().primary.base.color
            }),
            text(tr!("table-style-cells", "new-cell-style")).size(11),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(cells(TsCellMsg::ManageNew))
    .padding([5, 12])
    .style(|theme: &Theme, status| {
        let palette = theme.palette();
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(palette.background.strong.color)),
            text_color: palette.primary.base.color,
            border: Border {
                color: palette.primary.base.color.scale_alpha(0.6),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    });
    let user = !document::objects::is_row_cell_style(&manage.selected);
    let label = style
        .cell_style(&manage.selected)
        .map(|cell| cell_style_label(&cell.name).into_owned())
        .unwrap_or_default();
    let mut name = text_input("", manage.rename.as_deref().unwrap_or(&label))
        .id(iced::widget::Id::new(RENAME_ID))
        .size(12)
        .padding([5, 8])
        .style(crate::ui::style::form::field_style)
        .width(Length::Fill);
    if user {
        name = name
            .on_input(|value| cells(TsCellMsg::RenameText(value)))
            .on_submit(cells(TsCellMsg::RenameCommit));
    }
    let usage = if in_use {
        tr!("table-style-cells", "used-some")
    } else {
        tr!("table-style-cells", "used-none")
    };
    let action = |icon: Option<&'static [u8]>, caption: String, on: Option<Message>| -> Element<'a, Message> {
        let enabled = on.is_some();
        let mut content = row![].spacing(6).align_y(iced::Center);
        if let Some(icon) = icon {
            content = content.push(crate::ui::icons::tinted_faded(
                icon,
                12.0,
                if enabled { 1.0 } else { 0.42 },
                |theme: &Theme| theme.palette().background.base.text,
            ));
        }
        button(content.push(text(caption).size(11)))
        .on_press_maybe(on)
        .style(button_style(false))
        .padding([5, 10])
        .into()
    };
    let selected_cell = style.cell_style(&manage.selected);
    let preview = container(
        canvas(TablePreviewCanvas {
            layout: rows_preview(style, selected_cell, &label),
        })
        .width(Length::Fixed(300.0))
        .height(120),
    )
    .style(well_style);
    let details = column![
        text(tr!("table-style-cells", "selected-style").to_uppercase())
            .size(10)
            .style(crate::ui::window::pdf_dialogs::accent_text),
        grid_row(t!("Name"), name),
        grid_row(
            tr!("table-style-cells", "usage").into(),
            text(usage).size(11).style(muted_style)
        ),
        row![
            action(
                None,
                tr!("table-style-cells", "rename"),
                user.then(|| cells(TsCellMsg::RenameStart))
            ),
            action(
                Some(crate::ui::icons::TRASH),
                tr!("table-style-cells", "delete"),
                user.then(|| cells(TsCellMsg::DeleteAsk))
            ),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .width(Length::Fill);
    let selected_card = container(row![details, preview].spacing(16))
        .padding([12, 14])
        .width(Length::Fill)
        .style(card_style);
    let body = column![
        text(tr!("table-style-cells", "manage-title"))
            .size(13)
            .style(crate::ui::window::pdf_dialogs::accent_text),
        row![segments, new_chip].spacing(10).align_y(iced::Center),
        selected_card,
        row![
            button(text("?").size(12))
                .on_press(cells(TsCellMsg::ManageHelp))
                .style(button_style(false))
                .padding([5, 11]),
            Space::new().width(Length::Fill),
            dialog_button(tr!("table-style-cells", "cancel"), cells(TsCellMsg::ManageCancel), false),
            dialog_button(tr!("table-style-cells", "ok"), cells(TsCellMsg::ManageOk), true),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(12);
    crate::ui::style::mleaderstyle::panel(body.into(), 780.0)
}

/// The Manage Cell Styles preview: the title and header rows, then two
/// rows of the selected cell style named after it.
fn rows_preview(
    style: &TableStyle,
    selected: Option<&NamedTableCellStyle>,
    label: &str,
) -> PreviewLayout {
    let preview = |cell: Option<&NamedTableCellStyle>| {
        cell.map(|cell| preview_cell(&cell.cell_style))
            .unwrap_or_else(|| preview_cell(&TableCellStyleData::default()))
    };
    let title = style.cell_style(document::objects::TITLE_CELL_STYLE);
    let header = style.cell_style(document::objects::HEADER_CELL_STYLE);
    let height = |cell: Option<&NamedTableCellStyle>| {
        cell.map_or(1.0, |cell| {
            let margin = cell
                .cell_style
                .margins()
                .map_or(style.vertical_margin, |(_, v)| v);
            (cell.cell_style.content_format.text_height * 4.0 / 3.0 + 2.0 * margin) as f32
        })
    };
    let row_height = height(selected);
    PreviewLayout::Table {
        widths: vec![(row_height * 4.0).max(1.0); 3],
        heights: vec![height(title), height(header), row_height, row_height],
        rows: vec![
            preview(title),
            preview(header),
            preview(selected),
            preview(selected),
        ],
        texts: vec![
            vec![t!("Title").into_owned(), String::new(), String::new()],
            vec![t!("Header").into_owned(); 3],
            vec![label.to_string(); 3],
            vec![label.to_string(); 3],
        ],
        merged: vec![(0, 0, 0, 2)],
        flow_up: false,
    }
}

/// The card on top, if any, and what a click beside it does.
fn card_view<'a>(v: &TableStyleView<'a>) -> Option<(Element<'a, Message>, Message)> {
    use crate::ui::style::mleaderstyle::Reload;
    let cards = v.cards;
    if let Some(ask) = &cards.reload_ask {
        return Some((
            crate::ui::style::mleaderstyle::reload_card(ask, |answer| {
                cells(TsCellMsg::ReloadAnswer(answer))
            }),
            cells(TsCellMsg::ReloadAnswer(Reload::Cancel)),
        ));
    }
    if let Some(load) = &cards.load {
        return Some((
            crate::ui::style::mleaderstyle::load_linetypes_card(
                load,
                &v.linetype_names,
                cells(TsCellMsg::LoadFile),
                |name| cells(TsCellMsg::LoadToggle(name)),
                cells(TsCellMsg::LoadOk),
                cells(TsCellMsg::LoadCancel),
            ),
            cells(TsCellMsg::LoadCancel),
        ));
    }
    if let Some(selected) = &cards.linetype {
        return Some((
            crate::ui::style::mleaderstyle::linetype_card(
                &v.linetype_rows,
                selected,
                |name| cells(TsCellMsg::LinetypePick(name)),
                cells(TsCellMsg::LinetypeLoad),
                cells(TsCellMsg::LinetypeCancel),
                cells(TsCellMsg::LinetypeOk),
            ),
            cells(TsCellMsg::LinetypeCancel),
        ));
    }
    let style = v.style?;
    if let Some(create) = &cards.create {
        if let Some(notice) = &create.notice {
            return Some((notice_card(notice), cells(TsCellMsg::NoticeClose)));
        }
        return Some((create_card(create, style), cells(TsCellMsg::CreateCancel)));
    }
    if let Some(manage) = &cards.manage {
        if let Some(notice) = &manage.notice {
            return Some((notice_card(notice), cells(TsCellMsg::NoticeClose)));
        }
        return Some((
            manage_card(manage, style, v.manage_in_use),
            cells(TsCellMsg::ManageCancel),
        ));
    }
    None
}

/// The window with its card over it, dimmed as the text style window's.
fn with_card<'a>(window: Element<'a, Message>, v: &TableStyleView<'a>) -> Element<'a, Message> {
    use iced::widget::{container, mouse_area, stack};
    let Some((panel, dismiss)) = card_view(v) else {
        return window;
    };
    stack([
        window,
        mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|theme: &Theme| iced::widget::container::Style {
                    background: Some(iced::Background::Color(
                        theme.palette().background.strongest.color.scale_alpha(0.55),
                    )),
                    ..Default::default()
                }),
        )
        .on_press(dismiss)
        .into(),
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    ])
    .into()
}

pub fn view_window<'a>(
    v: TableStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let editor: Element<'a, Message> = match v.style {
        Some(style) => page(&v, style),
        None => text(t!("Select a style to view details.")).size(11).into(),
    };
    let window = crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
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
        on_import: None,
        on_select: Message::TableStyleDialogSelect,
        on_set_current: Message::TableStyleDialogSetCurrent,
        on_apply: Message::TableStyleApply,
        read_only: false,
        locked_keeps_actions: false,
        on_list_scroll: None,
        in_use_filter: Some((v.in_use_only, Message::TableStyleListInUse)),
        apply_blocked: None,
        rows: None,
        compact: false,
        apply_pending: true,
        delete_in_use: false,
        on_context: None,
        context_menu: None,
        list: None,
        extra_actions: Vec::new(),
        list_title: Some(Cow::Owned(tr!("table-style-cells", "table-styles"))),
        editor,
    });
    with_card(window, &v)
}
