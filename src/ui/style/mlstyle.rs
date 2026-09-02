//! Multiline style manager.

use crate::app::Message;
use crate::t;
use iced::widget::{button, canvas, checkbox, column, container, row, text, text_input};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Theme};
use crate::ui::style::common::muted_style;
use std::borrow::Cow;

pub struct MlStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub style: Option<&'a acadrust::objects::MLineStyle>,
    pub current: String,
    pub tab: u8,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub in_use: bool,
    pub description: &'a str,
    pub start_angle: &'a str,
    pub end_angle: &'a str,
    pub fill_color: &'a str,
    pub elements: &'a [[String; 3]],
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn input_row<'a>(
    label: Cow<'static, str>,
    placeholder: &'static str,
    value: &'a str,
    field: &'static str,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(165),
        text_input(placeholder, value)
            .on_input(move |value| Message::MlStyleEdit { field, value })
            .size(11)
            .width(190),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn toggle<'a>(label: Cow<'static, str>, value: bool, field: &'static str) -> Element<'a, Message> {
    checkbox(value)
        .label(label)
        .on_toggle(move |_| Message::MlStyleToggle(field))
        .size(14)
        .text_size(11)
        .into()
}

fn aci_color(value: &str, fallback: Color) -> Color {
    crate::ui::color_select::aci_string_to_color(value)
        .rgb()
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(fallback)
}

struct MLinePreviewCanvas {
    description: String,
    start_angle: String,
    end_angle: String,
    fill_color: String,
    fill: bool,
    joints: bool,
    start_square: bool,
    start_inner: bool,
    start_round: bool,
    end_square: bool,
    end_inner: bool,
    end_round: bool,
    elements: Vec<[String; 3]>,
}

fn element_points(offset: f32, bounds: Rectangle) -> [Point; 3] {
    let y = bounds.height * 0.48 - offset * 26.0;
    [
        Point::new(44.0, y),
        Point::new(bounds.width * 0.52, y - 16.0),
        Point::new(bounds.width - 44.0, y + 5.0),
    ]
}

fn draw_arc(frame: &mut canvas::Frame, center: Point, radius: f32, start_side: bool, color: Color) {
    let path = canvas::Path::new(|builder| {
        for step in 0..=18 {
            let t = step as f32 / 18.0;
            let angle = if start_side {
                std::f32::consts::FRAC_PI_2 + t * std::f32::consts::PI
            } else {
                -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::PI
            };
            let point = Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin());
            if step == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
    });
    frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(1.1));
}

impl canvas::Program<Message> for MLinePreviewCanvas {
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
        let mut parsed = self
            .elements
            .iter()
            .map(|element| {
                (
                    element[0].trim().parse::<f32>().unwrap_or(0.0),
                    element[1].clone(),
                    element[2].clone(),
                )
            })
            .collect::<Vec<_>>();
        parsed.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let (Some(top), Some(bottom)) = (parsed.first(), parsed.last()) {
            if self.fill {
                let upper = element_points(top.0, bounds);
                let lower = element_points(bottom.0, bounds);
                let fill = canvas::Path::new(|builder| {
                    builder.move_to(upper[0]);
                    builder.line_to(upper[1]);
                    builder.line_to(upper[2]);
                    builder.line_to(lower[2]);
                    builder.line_to(lower[1]);
                    builder.line_to(lower[0]);
                    builder.close();
                });
                frame.fill(&fill, aci_color(&self.fill_color, theme.palette().background.weak.color));
            }
        }
        for (offset, color, linetype) in &parsed {
            let points = element_points(*offset, bounds);
            let dashed = !linetype.trim().is_empty()
                && !linetype.eq_ignore_ascii_case("ByLayer")
                && !linetype.to_ascii_lowercase().contains("continuous");
            let stroke = canvas::Stroke {
                style: canvas::Style::Solid(aci_color(color, ink)),
                width: 1.4,
                line_dash: if dashed {
                    canvas::LineDash { segments: &[6.0, 4.0], offset: 0 }
                } else {
                    canvas::LineDash::default()
                },
                ..Default::default()
            };
            frame.stroke(
                &canvas::Path::new(|builder| {
                    builder.move_to(points[0]);
                    builder.line_to(points[1]);
                    builder.line_to(points[2]);
                }),
                stroke,
            );
        }
        if let (Some(top), Some(bottom)) = (parsed.first(), parsed.last()) {
            let top_points = element_points(top.0, bounds);
            let bottom_points = element_points(bottom.0, bounds);
            let start_top = top_points[0];
            let start_bottom = bottom_points[0];
            let end_top = top_points[2];
            let end_bottom = bottom_points[2];
            let start_angle = self.start_angle.trim().parse::<f32>().unwrap_or(90.0).to_radians();
            let end_angle = self.end_angle.trim().parse::<f32>().unwrap_or(90.0).to_radians();
            let cap = |a: Point, b: Point, angle: f32| {
                let middle = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
                let half = ((a.y - b.y).abs() * 0.5).max(2.0);
                let direction = Point::new(angle.cos() * half, angle.sin() * half);
                (Point::new(middle.x - direction.x, middle.y - direction.y), Point::new(middle.x + direction.x, middle.y + direction.y))
            };
            if self.start_square {
                let (a, b) = cap(start_top, start_bottom, start_angle);
                frame.stroke(&canvas::Path::line(a, b), canvas::Stroke::default().with_color(ink));
            }
            if self.end_square {
                let (a, b) = cap(end_top, end_bottom, end_angle);
                frame.stroke(&canvas::Path::line(a, b), canvas::Stroke::default().with_color(ink));
            }
            let start_center = Point::new((start_top.x + start_bottom.x) * 0.5, (start_top.y + start_bottom.y) * 0.5);
            let end_center = Point::new((end_top.x + end_bottom.x) * 0.5, (end_top.y + end_bottom.y) * 0.5);
            let outer_radius = ((start_top.y - start_bottom.y).abs() * 0.5).max(3.0);
            if self.start_round {
                draw_arc(&mut frame, start_center, outer_radius, true, ink);
            }
            if self.end_round {
                draw_arc(&mut frame, end_center, outer_radius, false, ink);
            }
            if self.start_inner && parsed.len() > 2 {
                draw_arc(&mut frame, start_center, outer_radius * 0.55, true, ink);
            }
            if self.end_inner && parsed.len() > 2 {
                draw_arc(&mut frame, end_center, outer_radius * 0.55, false, ink);
            }
            if self.joints {
                let joint_top = top_points[1];
                let joint_bottom = bottom_points[1];
                frame.stroke(&canvas::Path::line(joint_top, joint_bottom), canvas::Stroke::default().with_color(ink).with_width(1.0));
            }
        }
        frame.fill_text(canvas::Text {
            content: format!("{} · {} {}", self.description, t!("Elements"), self.elements.len()),
            position: Point::new(10.0, bounds.height - 9.0),
            color: ink.scale_alpha(0.62),
            size: iced::Pixels(10.0),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

pub fn view_window<'a>(
    v: MlStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let style = v.style;
    let content: Element<'a, Message> = match (v.tab, style) {
        (_, None) => text(t!("Select a style to view details.")).size(11).into(),
        (0, Some(_)) => {
            let mut items = column![
                row![
                    text(t!("Offset")).size(10).style(muted_style).width(110),
                    text(t!("Color")).size(10).style(muted_style).width(100),
                    text(t!("Line type")).size(10).style(muted_style).width(170),
                    button(text(t!("Add")).size(10)).on_press(Message::MlStyleElementAdd).padding([4, 12]),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(6);
            for (index, element) in v.elements.iter().enumerate() {
                items = items.push(
                    row![
                        text_input("0.0", &element[0])
                            .on_input(move |value| Message::MlStyleElementEdit { index, field: "offset", value })
                            .size(11).width(110),
                        text_input("256", &element[1])
                            .on_input(move |value| Message::MlStyleElementEdit { index, field: "color", value })
                            .size(11).width(100),
                        text_input("ByLayer", &element[2])
                            .on_input(move |value| Message::MlStyleElementEdit { index, field: "linetype", value })
                            .size(11).width(170),
                        button(text(t!("Delete")).size(10))
                            .on_press(Message::MlStyleElementDelete(index))
                            .padding([4, 10]),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                );
            }
            items.into()
        }
        (_, Some(style)) => column![
            input_row(t!("Description"), "", v.description, "description"),
            text(t!("Fill and joints")).size(11).style(primary_style),
            row![
                toggle(t!("Fill enabled"), style.flags.fill_on, "fill"),
                toggle(t!("Display joints"), style.flags.display_joints, "joints"),
            ].spacing(18),
            input_row(t!("Fill color"), "256", v.fill_color, "fill_color"),
            text(t!("Start caps")).size(11).style(primary_style),
            row![
                toggle(t!("Line"), style.flags.start_square_cap, "start_square"),
                toggle(t!("Inner arcs"), style.flags.start_inner_arcs_cap, "start_inner"),
                toggle(t!("Outer arc"), style.flags.start_round_cap, "start_round"),
            ].spacing(18),
            input_row(t!("Start angle"), "90", v.start_angle, "start_angle"),
            text(t!("End caps")).size(11).style(primary_style),
            row![
                toggle(t!("Line"), style.flags.end_square_cap, "end_square"),
                toggle(t!("Inner arcs"), style.flags.end_inner_arcs_cap, "end_inner"),
                toggle(t!("Outer arc"), style.flags.end_round_cap, "end_round"),
            ].spacing(18),
            input_row(t!("End angle"), "90", v.end_angle, "end_angle"),
        ].spacing(9).into(),
    };
    let preview: Element<'a, Message> = if let Some(style) = style {
        canvas(MLinePreviewCanvas {
            description: v.description.to_string(),
            start_angle: v.start_angle.to_string(),
            end_angle: v.end_angle.to_string(),
            fill_color: v.fill_color.to_string(),
            fill: style.flags.fill_on,
            joints: style.flags.display_joints,
            start_square: style.flags.start_square_cap,
            start_inner: style.flags.start_inner_arcs_cap,
            start_round: style.flags.start_round_cap,
            end_square: style.flags.end_square_cap,
            end_inner: style.flags.end_inner_arcs_cap,
            end_round: style.flags.end_round_cap,
            elements: v.elements.to_vec(),
        })
        .width(Length::Fill)
        .height(150)
        .into()
    } else {
        container(text(t!("No preview"))).height(150).into()
    };
    let status = if v.selected.eq_ignore_ascii_case(&v.current) {
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
            preview,
            comparison: Some(crate::ui::style::style_manager::EditorComparison {
                selected: v.compare_name,
                options: v.compare_opts,
                summary,
                on_select: Message::MlStyleDialogCompare,
            }),
            tabs: vec![
                crate::ui::style::style_manager::EditorTab {
                    label: t!("Elements"),
                    active: v.tab == 0,
                    on_press: Message::MlStyleDialogTab(0),
                },
                crate::ui::style::style_manager::EditorTab {
                    label: t!("Caps and Fill"),
                    active: v.tab == 1,
                    on_press: Message::MlStyleDialogTab(1),
                },
            ],
            content,
        },
    );
    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: crate::app::StyleKind::MLine,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current.as_str()),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: Message::MlStyleDialogNew,
        on_copy: Message::MlStyleDialogCopy,
        on_delete: Message::MlStyleDialogDelete,
        on_select: Message::MlStyleDialogSelect,
        on_set_current: Message::MlStyleDialogSetCurrent,
        on_apply: Message::MlStyleApply,
        read_only: false,
        editor,
    })
}
