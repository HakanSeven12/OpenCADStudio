//! Table style manager.
//!
//! General page: description, table direction, annotative and the starting
//! table. The other pages edit one named cell style at a time (title,
//! header, data or a user cell style): its properties, text and edges.

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use codec::objects::{NamedTableCellStyle, TableCellStyleData, TableStyle, TABLE_GRID_EDGES};
use iced::widget::{button, canvas, checkbox, column, pick_list, row, text, text_input, Column};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Size, Theme};
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
    width: f32,
    size: u32,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    if !options.iter().any(|option| option.code == current) {
        options.push(choice(current, Cow::Owned(current.to_string())));
    }
    let selected = options.iter().find(|option| option.code == current).cloned();
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
    pub cell_name: &'a str,
    pub cell_height: &'a str,
    pub cell_angle: &'a str,
    pub cell_hmargin: &'a str,
    pub cell_vmargin: &'a str,
    pub cell_format: &'a str,
    pub edge_spacing: &'a [String; 6],
    pub text_styles: Vec<(String, codec::Handle)>,
    pub linetypes: Vec<(String, codec::Handle)>,
    /// The style's starting table.
    pub template: Option<&'a codec::entities::Table>,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    pub color_open: Option<&'static str>,
}

/// The name a cell style shows: the three row styles by their role.
fn cell_style_label(name: &str) -> Cow<'static, str> {
    match name {
        codec::objects::TITLE_CELL_STYLE => t!("Title"),
        codec::objects::HEADER_CELL_STYLE => t!("Header"),
        codec::objects::DATA_CELL_STYLE => t!("Data"),
        other => Cow::Owned(other.to_string()),
    }
}

/// Data types a cell style formats its values as: (data type, unit type).
const DATA_TYPES: [(&str, &str); 10] = [
    ("512/0", "General"),
    ("4/0", "Text"),
    ("1/0", "Whole number"),
    ("2/0", "Real number"),
    ("2/1", "Distance"),
    ("2/2", "Angle"),
    ("2/4", "Area"),
    ("2/8", "Volume"),
    ("8/0", "Date"),
    ("32/0", "Point"),
];

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
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
    200, 211,
];

fn line_weight_choices() -> Vec<EnumChoice> {
    let mut list = vec![
        choice("-1", t!("ByLayer")),
        choice("-2", t!("ByBlock")),
    ];
    list.extend(
        LINE_WEIGHTS
            .iter()
            .map(|w| choice(w.to_string(), Cow::Owned(format!("{:.2} mm", *w as f64 / 100.0)))),
    );
    list
}

fn edge_color_code(color: &codec::types::Color) -> String {
    use codec::types::Color as C;
    match color {
        C::ByBlock => "0".into(),
        C::ByLayer => "256".into(),
        C::Index(index) => index.to_string(),
        other => other.index().map_or_else(|| "0".into(), |index| index.to_string()),
    }
}

fn edge_color_choices() -> Vec<EnumChoice> {
    let mut list = vec![choice("0", t!("ByBlock")), choice("256", t!("ByLayer"))];
    for (index, name) in [
        (1, "Red"),
        (2, "Yellow"),
        (3, "Green"),
        (4, "Cyan"),
        (5, "Blue"),
        (6, "Magenta"),
        (7, "White"),
    ] {
        list.push(choice(index.to_string(), crate::i18n::translate(name)));
    }
    list
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

fn rgb(color: &codec::types::Color) -> Option<Color> {
    use codec::types::Color as C;
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
                grid.border.border_type == codec::objects::TableBorderType::Double
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

/// What the preview draws: the whole table (general page) or a 2×2 block
/// of the edited cell style.
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
    Cell {
        cell: PreviewCell,
        label: String,
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
                stroke.clone(),
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
            position: Point::ORIGIN,
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
            PreviewLayout::Cell { cell, label } => {
                let (w, h) = (bounds.width * 0.3, 30.0f32);
                let left = (bounds.width - 2.0 * w) / 2.0;
                let top = (bounds.height - 2.0 * h) / 2.0;
                if let Some(fill) = cell.fill {
                    frame.fill_rectangle(Point::new(left, top), Size::new(2.0 * w, 2.0 * h), fill);
                }
                for r in 0..2 {
                    for c in 0..2 {
                        let rect = Rectangle::new(
                            Point::new(left + c as f32 * w, top + r as f32 * h),
                            Size::new(w, h),
                        );
                        draw_cell_text(&mut frame, cell, label.clone(), rect, 12.0, ink);
                    }
                }
                let (x0, x1, x2) = (left, left + w, left + 2.0 * w);
                let (y0, y1, y2) = (top, top + h, top + 2.0 * h);
                let e = &cell.edges;
                stroke_edge(&mut frame, Point::new(x0, y0), Point::new(x2, y0), &e[0], ink);
                stroke_edge(&mut frame, Point::new(x0, y1), Point::new(x2, y1), &e[1], ink);
                stroke_edge(&mut frame, Point::new(x0, y2), Point::new(x2, y2), &e[2], ink);
                stroke_edge(&mut frame, Point::new(x0, y0), Point::new(x0, y2), &e[3], ink);
                stroke_edge(&mut frame, Point::new(x1, y0), Point::new(x1, y2), &e[4], ink);
                stroke_edge(&mut frame, Point::new(x2, y0), Point::new(x2, y2), &e[5], ink);
            }
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
                        stroke_edge(&mut frame, Point::new(x0, y0), Point::new(x1, y0), &e[0], ink);
                        stroke_edge(&mut frame, Point::new(x0, y1), Point::new(x1, y1), &e[2], ink);
                        stroke_edge(&mut frame, Point::new(x0, y0), Point::new(x0, y1), &e[3], ink);
                        stroke_edge(&mut frame, Point::new(x1, y0), Point::new(x1, y1), &e[5], ink);
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
    let flow_up = style.flow_direction == codec::objects::TableFlowDirection::Up;
    if let Some(table) = v.template {
        let rows = table
            .rows
            .iter()
            .map(|row| by_id(row.style_id, codec::objects::DATA_CELL_STYLE))
            .collect();
        return PreviewLayout::Table {
            widths: table.columns.iter().map(|c| c.width as f32).collect(),
            heights: table.rows.iter().map(|r| r.height as f32).collect(),
            rows,
            texts: table
                .rows
                .iter()
                .map(|row| row.cells.iter().map(|cell| cell.text_value().to_string()).collect())
                .collect(),
            merged: table
                .merged_ranges
                .iter()
                .map(|r| (r.top_row, r.left_col, r.bottom_row, r.right_col))
                .collect(),
            flow_up,
        };
    }
    let title = style.cell_style(codec::objects::TITLE_CELL_STYLE);
    let header = style.cell_style(codec::objects::HEADER_CELL_STYLE);
    let data = style.cell_style(codec::objects::DATA_CELL_STYLE);
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

fn label<'a>(value: Cow<'static, str>) -> Element<'a, Message> {
    text(value).size(11).style(muted_style).width(140).into()
}

fn field_row<'a>(name: Cow<'static, str>, control: Element<'a, Message>) -> Element<'a, Message> {
    row![label(name), control]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn cell_input<'a>(
    field: &'static str,
    value: &'a str,
    placeholder: &'static str,
) -> Element<'a, Message> {
    text_input(placeholder, value)
        .on_input(move |value| Message::TableStyleCellEdit { field, value })
        .style(crate::ui::style::form::field_style)
        .size(11)
        .width(170)
        .into()
}

fn cell_color<'a>(
    v: &TableStyleView<'a>,
    field: &'static str,
    color: &codec::types::Color,
    none: bool,
) -> Element<'a, Message> {
    let current = *color;
    crate::ui::color_select::color_selector(
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
    )
}

/// The cell style picker with its new / rename / delete actions.
fn cell_style_bar<'a>(v: &TableStyleView<'a>, style: &TableStyle) -> Element<'a, Message> {
    let options: Vec<EnumChoice> = style
        .cell_styles()
        .map(|cell| choice(cell.name.clone(), cell_style_label(&cell.name)))
        .collect();
    let builtin = codec::objects::is_row_cell_style(v.cell);
    let mut rename = button(text(t!("Rename")).size(11)).padding([3, 8]);
    let mut delete = button(text(t!("Delete")).size(11)).padding([3, 8]);
    if !builtin {
        delete = delete.on_press(Message::TableStyleCellDelete);
        if !v.cell_name.trim().is_empty() {
            rename = rename.on_press(Message::TableStyleCellRename);
        }
    }
    column![
        row![
            text(t!("Cell style")).size(11).style(primary_style).width(140),
            choice_list(v.cell, options, 170.0, 11, Message::TableStyleCellSelect),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text_input(t!("New name").as_ref(), v.cell_name)
                .on_input(|value| Message::TableStyleCellEdit {
                    field: "name",
                    value
                })
                .style(crate::ui::style::form::field_style)
                .size(11)
                .width(140),
            button(text(t!("New from this")).size(11))
                .on_press(Message::TableStyleCellNew)
                .padding([3, 8]),
            rename,
            delete,
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(6)
    .into()
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn properties_page<'a>(v: &TableStyleView<'a>, cell: &NamedTableCellStyle) -> Column<'a, Message> {
    let data = &cell.cell_style;
    let format = &data.content_format;
    let data_type = format!("{}/{}", format.value_data_type, format.value_unit_type);
    column![
        field_row(t!("Fill color"), cell_color(v, "fillcolor", &data.background_color, true)),
        field_row(
            t!("Alignment"),
            choice_list(
                &format.cell_alignment.to_string(),
                translated(&ALIGNMENTS),
                170.0,
                11,
                |value| Message::TableStyleCellEdit {
                    field: "align",
                    value
                }
            )
        ),
        field_row(
            t!("Data type"),
            choice_list(&data_type, translated(&DATA_TYPES), 170.0, 11, |value| {
                Message::TableStyleCellEdit {
                    field: "datatype",
                    value,
                }
            })
        ),
        field_row(t!("Format"), cell_input("format", v.cell_format, "%lu2%pr3")),
        field_row(
            t!("Type"),
            choice_list(
                &cell.style_type.to_string(),
                translated(&[("2", "Data"), ("1", "Label")]),
                170.0,
                11,
                |value| Message::TableStyleCellEdit {
                    field: "class",
                    value
                }
            )
        ),
        text(t!("Margins")).size(11).style(primary_style),
        field_row(t!("Horizontal"), cell_input("hmargin", v.cell_hmargin, "1.5")),
        field_row(t!("Vertical"), cell_input("vmargin", v.cell_vmargin, "1.5")),
        checkbox(data.merge_all())
            .label(t!("Merge cells on row/column creation"))
            .on_toggle(|on| Message::TableStyleCellEdit {
                field: "merge",
                value: if on { "1" } else { "0" }.to_string(),
            })
            .size(14)
            .text_size(11),
    ]
    .spacing(8)
}

fn text_page<'a>(v: &TableStyleView<'a>, cell: &NamedTableCellStyle) -> Column<'a, Message> {
    let format = &cell.cell_style.content_format;
    let styles: Vec<EnumChoice> = v
        .text_styles
        .iter()
        .map(|(name, handle)| choice(format!("{:X}", handle.value()), Cow::Owned(name.clone())))
        .collect();
    column![
        field_row(
            t!("Text style"),
            choice_list(
                &format!("{:X}", format.text_style.value()),
                styles,
                170.0,
                11,
                |value| Message::TableStyleCellEdit {
                    field: "textstyle",
                    value
                }
            )
        ),
        field_row(t!("Text height"), cell_input("height", v.cell_height, "4.5")),
        field_row(t!("Text color"), cell_color(v, "textcolor", &format.content_color, false)),
        field_row(t!("Text angle"), cell_input("angle", v.cell_angle, "0")),
    ]
    .spacing(8)
}

fn borders_page<'a>(v: &TableStyleView<'a>, cell: &NamedTableCellStyle) -> Column<'a, Message> {
    let data = &cell.cell_style;
    let mut linetypes = vec![choice("0", t!("ByBlock"))];
    linetypes.extend(
        v.linetypes
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("ByBlock"))
            .map(|(name, handle)| choice(format!("{:X}", handle.value()), Cow::Owned(name.clone()))),
    );
    let names = [
        t!("Top"),
        t!("Inside horizontal"),
        t!("Bottom"),
        t!("Left"),
        t!("Inside vertical"),
        t!("Right"),
    ];
    let mut page = Column::new().spacing(6).push(
        row![
            text(t!("Edge")).size(10).style(muted_style).width(118),
            text(t!("Lineweight")).size(10).style(muted_style).width(82),
            text(t!("Linetype")).size(10).style(muted_style).width(92),
            text(t!("Color")).size(10).style(muted_style).width(78),
            text(t!("Double")).size(10).style(muted_style).width(56),
            text(t!("Spacing")).size(10).style(muted_style).width(60),
        ]
        .spacing(5),
    );
    for (slot, name) in names.into_iter().enumerate() {
        let edge = slot as u8;
        let mask = TABLE_GRID_EDGES[slot];
        let grid = data.grid(mask);
        let weight = grid.map_or(-2, |grid| grid.border.line_weight.value());
        let linetype = grid.map_or(0, |grid| grid.line_type.value());
        let color = grid.map_or_else(|| "0".to_string(), |grid| edge_color_code(&grid.border.color));
        let double = grid.is_some_and(|grid| {
            grid.border.border_type == codec::objects::TableBorderType::Double
        });
        let edit = move |field: &'static str| {
            move |value: String| Message::TableStyleEdgeEdit { edge, field, value }
        };
        page = page.push(
            row![
                checkbox(!data.edge_hidden(mask))
                    .label(name)
                    .on_toggle(move |on| Message::TableStyleEdgeEdit {
                        edge,
                        field: "visible",
                        value: if on { "1" } else { "0" }.to_string(),
                    })
                    .size(13)
                    .text_size(10)
                    .width(118),
                choice_list(&weight.to_string(), line_weight_choices(), 82.0, 10, edit("lw")),
                choice_list(&format!("{linetype:X}"), linetypes.clone(), 92.0, 10, edit("linetype")),
                choice_list(&color, edge_color_choices(), 78.0, 10, edit("color")),
                checkbox(double)
                    .on_toggle(move |on| Message::TableStyleEdgeEdit {
                        edge,
                        field: "double",
                        value: if on { "1" } else { "0" }.to_string(),
                    })
                    .size(13)
                    .width(56),
                text_input("", &v.edge_spacing[slot])
                    .on_input(edit("spacing"))
                    .size(10)
                    .width(60),
            ]
            .spacing(5)
            .align_y(iced::Center),
        );
    }
    page
}

fn general_page<'a>(v: &TableStyleView<'a>, style: &TableStyle) -> Column<'a, Message> {
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
    let mut remove = button(text(t!("Remove")).size(11)).padding([3, 8]);
    if v.template.is_some() {
        remove = remove.on_press(Message::TableStyleTemplateRemove);
    }
    column![
        field_row(
            t!("Description"),
            text_input("", v.description)
                .on_input(|value| Message::TableStyleEdit {
                    field: "description",
                    value,
                })
                .style(crate::ui::style::form::field_style)
                .size(11)
                .width(170)
                .into()
        ),
        field_row(
            t!("Table direction"),
            choice_list(
                &format!("{:?}", style.flow_direction),
                translated(&[("Down", "Down"), ("Up", "Up")]),
                170.0,
                11,
                Message::TableStyleSetFlow
            )
        ),
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
        text(t!("Starting table")).size(11).style(primary_style),
        field_row(t!("Table"), text(template_summary).size(11).into()),
        row![
            button(text(t!("Select table…")).size(11))
                .on_press(Message::TableStyleTemplatePick)
                .padding([3, 8]),
            remove,
        ]
        .spacing(6),
    ]
    .spacing(9)
}

pub fn view_window<'a>(
    v: TableStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let style = v.style;
    let cell = style.and_then(|style| style.cell_style(v.cell));
    let content: Element<'a, Message> = match (v.tab, style, cell) {
        (_, None, _) => text(t!("Select a style to view details.")).size(11).into(),
        (0, Some(style), _) => general_page(&v, style).into(),
        (_, Some(_), None) => text(t!("Select a style to view details.")).size(11).into(),
        (tab, Some(style), Some(cell)) => {
            let page = match tab {
                1 => properties_page(&v, cell),
                2 => text_page(&v, cell),
                _ => borders_page(&v, cell),
            };
            column![cell_style_bar(&v, style), page].spacing(12).into()
        }
    };
    let layout = match (v.tab, style, cell) {
        (0, Some(style), _) => table_preview(&v, style),
        (_, Some(_), Some(cell)) => PreviewLayout::Cell {
            cell: preview_cell(&cell.cell_style),
            label: cell_style_label(&cell.name).into_owned(),
        },
        _ => PreviewLayout::Cell {
            cell: preview_cell(&TableCellStyleData::default()),
            label: String::new(),
        },
    };
    let preview = canvas(TablePreviewCanvas { layout })
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
            tabs: [
                t!("General"),
                t!("Cell Properties"),
                t!("Cell Text"),
                t!("Cell Borders"),
            ]
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
        on_import: None,
        on_select: Message::TableStyleDialogSelect,
        on_set_current: Message::TableStyleDialogSetCurrent,
        on_apply: Message::TableStyleApply,
        read_only: false,
        in_use_filter: Some((v.in_use_only, Message::TableStyleListInUse)),
        apply_blocked: None,
        rows: None,
        compact: false,
        editor,
    })
}
