//! Multileader style manager.

use crate::app::Message;
use crate::t;
use iced::widget::{canvas, checkbox, column, container, row, text, text_input};
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

pub struct MLeaderStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub style: Option<&'a acadrust::objects::MultiLeaderStyle>,
    pub current: String,
    pub tab: u8,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub in_use: bool,
    pub landing_distance: &'a str,
    pub landing_gap: &'a str,
    pub arrowhead_size: &'a str,
    pub text_height: &'a str,
    pub scale_factor: &'a str,
    pub break_gap: &'a str,
    pub first_seg_angle: &'a str,
    pub second_seg_angle: &'a str,
    pub max_points: &'a str,
    pub default_text: &'a str,
    pub line_color: &'a str,
    pub text_color: &'a str,
    pub description: &'a str,
    pub align_space: &'a str,
    pub block_color: &'a str,
    pub block_rotation: &'a str,
    pub block_scale_x: &'a str,
    pub block_scale_y: &'a str,
    pub block_scale_z: &'a str,
    pub block_opts: Vec<String>,
    pub arrow_opts: Vec<String>,
    pub lt_opts: Vec<String>,
    pub textstyle_opts: Vec<String>,
    pub line_type_name: String,
    pub arrowhead_name: String,
    pub text_style_name: String,
    pub block_content_name: String,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    pub color_open: Option<&'static str>,
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
            .on_input(move |value| Message::MLeaderStyleEdit { field, value })
            .size(11)
            .width(190),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn readonly_row<'a>(label: Cow<'static, str>, value: Cow<'static, str>) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(165),
        crate::ui::read_only::field(value.as_ref(), 11.0, Length::Fixed(190.0)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn color_row<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    field: &'static str,
    open: bool,
) -> Element<'a, Message> {
    let current = crate::ui::color_select::aci_string_to_color(value);
    let selector = crate::ui::color_select::color_selector(
        current,
        open,
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: true,
            ..Default::default()
        },
        move |color| Message::MLeaderStyleEdit {
            field,
            value: crate::ui::color_select::color_to_aci_string(color),
        },
        Message::MLeaderColorMore(field),
        Message::OpenColorWindow(crate::app::ColorPickTarget::MLeader(field), current),
    );
    row![text(label).size(11).style(muted_style).width(165), selector]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn enum_row<'a>(
    label: Cow<'static, str>,
    values: &[(&str, &str)],
    selected: String,
    field: &'static str,
) -> Element<'a, Message> {
    let selected_label = values
        .iter()
        .find(|(code, _)| *code == selected)
        .map(|(_, label)| crate::i18n::translate(label))
        .unwrap_or_else(|| crate::i18n::translate(&selected));
    row![
        text(label).size(11).style(muted_style).width(165),
        iced::widget::pick_list(
            Some(EnumChoice { code: selected, label: selected_label }),
            choices(values),
            |value| value.to_string(),
        )
        .on_select(move |choice| Message::MLeaderStyleSetEnum { field, value: choice.code })
        .text_size(11)
        .width(220),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn handle_row<'a>(
    label: Cow<'static, str>,
    options: Vec<String>,
    selected: String,
    field: &'static str,
) -> Element<'a, Message> {
    let display_label = |value: &str| match value {
        "None" | "ByBlock" | "ByLayer" | "Default" | "Closed filled" => {
            crate::i18n::translate(value)
        }
        _ => Cow::Owned(value.to_string()),
    };
    let selected = EnumChoice {
        code: selected.clone(),
        label: display_label(&selected),
    };
    let options: Vec<EnumChoice> = options
        .into_iter()
        .map(|code| EnumChoice {
            label: display_label(&code),
            code,
        })
        .collect();
    row![
        text(label).size(11).style(muted_style).width(165),
        iced::widget::pick_list(Some(selected), options, |value| value.to_string())
            .on_select(move |value| Message::MLeaderStyleSetHandle { field, value: value.code })
            .text_size(11)
            .width(220),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn toggle<'a>(label: Cow<'static, str>, value: bool, field: &'static str) -> Element<'a, Message> {
    checkbox(value)
        .label(label)
        .on_toggle(move |_| Message::MLeaderStyleToggle(field))
        .size(14)
        .text_size(11)
        .into()
}

const ATTACHMENTS: [(&str, &str); 11] = [
    ("TopOfTopLine", "Top of top line"),
    ("MiddleOfTopLine", "Middle of top line"),
    ("MiddleOfText", "Middle of text"),
    ("MiddleOfBottomLine", "Middle of bottom line"),
    ("BottomOfBottomLine", "Bottom of bottom line"),
    ("BottomLine", "Bottom line"),
    ("BottomOfTopLineUnderlineBottomLine", "Underline bottom line"),
    ("BottomOfTopLineUnderlineTopLine", "Underline top line"),
    ("BottomOfTopLineUnderlineAll", "Underline all text"),
    ("CenterOfText", "Center of text"),
    ("CenterOfTextOverline", "Center of text with overline"),
];

const LEADER_DRAW_ORDERS: [(&str, &str); 2] = [
    ("LeaderHeadFirst", "Arrowhead first"),
    ("LeaderTailFirst", "Content first"),
];
const MULTILEADER_DRAW_ORDERS: [(&str, &str); 2] = [
    ("ContentFirst", "Content first"),
    ("LeaderFirst", "Leader first"),
];
const BLOCK_CONNECTIONS: [(&str, &str); 2] = [
    ("BlockExtents", "Block extents"),
    ("BasePoint", "Base point"),
];

fn choice_label(values: &[(&str, &str)], selected: String) -> String {
    values
        .iter()
        .find(|(code, _)| *code == selected)
        .map(|(_, label)| crate::i18n::translate(label).into_owned())
        .unwrap_or(selected)
}

fn handle_label(value: &str) -> String {
    match value {
        "None" | "ByBlock" | "ByLayer" | "Default" | "Closed filled" => {
            crate::i18n::translate(value).into_owned()
        }
        _ => value.to_string(),
    }
}

fn aci_color(value: &str, fallback: Color) -> Color {
    crate::ui::color_select::aci_string_to_color(value)
        .rgb()
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(fallback)
}

struct LeaderPreviewCanvas {
    path_type: String,
    content_type: String,
    line_color: String,
    line_weight: i16,
    line_type: String,
    arrow_name: String,
    arrow_name_code: String,
    arrow_size: String,
    break_gap: String,
    landing: bool,
    dogleg: bool,
    landing_distance: String,
    landing_gap: String,
    max_points: String,
    first_angle: String,
    second_angle: String,
    scale: String,
    annotative: bool,
    align_space: String,
    default_text: String,
    text_style: String,
    text_height: String,
    text_color: String,
    text_angle: String,
    text_alignment: String,
    attachment_direction: String,
    left_attachment: String,
    left_attachment_code: String,
    right_attachment: String,
    top_attachment: String,
    bottom_attachment: String,
    text_frame: bool,
    text_always_left: bool,
    block_name: String,
    block_color: String,
    block_connection: String,
    block_connection_code: String,
    block_rotation: String,
    block_scale: [String; 3],
    enable_block_scale: bool,
    enable_block_rotation: bool,
    leader_draw_order: String,
    multileader_draw_order: String,
    description: String,
}

impl canvas::Program<Message> for LeaderPreviewCanvas {
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
        let line_color = aci_color(&self.line_color, ink);
        let scale = if self.annotative {
            1.0
        } else {
            self.scale.trim().parse::<f32>().unwrap_or(1.0).abs().clamp(0.35, 2.2)
        };
        let head = Point::new(34.0, bounds.height - 37.0);
        let angle1 = self.first_angle.trim().parse::<f32>().unwrap_or(28.0).to_radians();
        let angle2 = self.second_angle.trim().parse::<f32>().unwrap_or(0.0).to_radians();
        let points = self.max_points.trim().parse::<usize>().unwrap_or(2).clamp(2, 5);
        let elbow = Point::new(
            head.x + 105.0 * scale * angle1.cos().abs().max(0.25),
            head.y - 70.0 * scale * angle1.sin().abs().max(0.2),
        );
        let landing_length = self
            .landing_distance
            .trim()
            .parse::<f32>()
            .unwrap_or(8.0)
            .abs()
            .mul_add(6.0, 18.0)
            .clamp(24.0, 145.0)
            * scale;
        let tail = Point::new(elbow.x + landing_length * angle2.cos().abs(), elbow.y);
        let width = (self.line_weight.abs() as f32 / 35.0).clamp(0.8, 4.0);
        let dashed = !self.line_type.eq_ignore_ascii_case("None")
            && !self.line_type.eq_ignore_ascii_case("ByBlock")
            && !self.line_type.to_ascii_lowercase().contains("continuous");
        let stroke = canvas::Stroke {
            style: canvas::Style::Solid(line_color),
            width,
            line_dash: if dashed {
                canvas::LineDash { segments: &[7.0, 4.0], offset: 0 }
            } else {
                canvas::LineDash::default()
            },
            ..Default::default()
        };
        if self.path_type != "Invisible" {
            if self.path_type == "Spline" {
                let path = canvas::Path::new(|builder| {
                    builder.move_to(head);
                    for step in 1..=18 {
                        let t = step as f32 / 18.0;
                        let x = head.x + (elbow.x - head.x) * t;
                        let y = head.y + (elbow.y - head.y) * t - (t * std::f32::consts::PI).sin() * 15.0;
                        builder.line_to(Point::new(x, y));
                    }
                });
                frame.stroke(&path, stroke.clone());
            } else {
                let path = canvas::Path::new(|builder| {
                    builder.move_to(head);
                    for index in 1..points {
                        let t = index as f32 / (points - 1) as f32;
                        let bend = if index + 1 == points { 0.0 } else { (index as f32 * 7.0) % 14.0 - 7.0 };
                        builder.line_to(Point::new(
                            head.x + (elbow.x - head.x) * t,
                            head.y + (elbow.y - head.y) * t + bend,
                        ));
                    }
                });
                frame.stroke(&path, stroke.clone());
            }
            if self.landing && self.dogleg {
                let gap = self.break_gap.trim().parse::<f32>().unwrap_or(0.0).abs().clamp(0.0, 8.0);
                frame.stroke(&canvas::Path::line(elbow, Point::new((elbow.x + tail.x) * 0.5 - gap, elbow.y)), stroke.clone());
                frame.stroke(&canvas::Path::line(Point::new((elbow.x + tail.x) * 0.5 + gap, elbow.y), tail), stroke.clone());
            }
        }
        let arrow = self.arrow_size.trim().parse::<f32>().unwrap_or(0.18).abs().mul_add(12.0, 4.0).clamp(5.0, 18.0) * scale;
        let arrow_code = self.arrow_name_code.to_ascii_uppercase();
        if arrow_code.contains("DOT") {
            frame.fill(&canvas::Path::circle(head, arrow * 0.42), line_color);
        } else if arrow_code.contains("TICK") || arrow_code.contains("OBLIQUE") {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(head.x - arrow * 0.35, head.y + arrow * 0.65),
                    Point::new(head.x + arrow * 0.35, head.y - arrow * 0.65),
                ),
                canvas::Stroke::default().with_color(line_color).with_width(width),
            );
        } else if arrow_code.contains("BOX") {
            frame.fill(
                &canvas::Path::rectangle(
                    Point::new(head.x - arrow * 0.35, head.y - arrow * 0.35),
                    Size::new(arrow * 0.7, arrow * 0.7),
                ),
                line_color,
            );
        } else {
            let arrow_path = canvas::Path::new(|builder| {
                builder.move_to(head);
                builder.line_to(Point::new(head.x + arrow, head.y - arrow * 0.45));
                builder.line_to(Point::new(head.x + arrow, head.y + arrow * 0.45));
                builder.close();
            });
            if arrow_code.contains("OPEN") {
                let open_path = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(head.x + arrow, head.y - arrow * 0.45));
                    builder.line_to(head);
                    builder.line_to(Point::new(head.x + arrow, head.y + arrow * 0.45));
                });
                frame.stroke(
                    &open_path,
                    canvas::Stroke::default().with_color(line_color).with_width(width),
                );
            } else {
                frame.fill(&arrow_path, line_color);
            }
        }
        let landing_gap = self.landing_gap.trim().parse::<f32>().unwrap_or(0.09).abs().mul_add(8.0, 5.0).clamp(5.0, 24.0);
        let content_origin = Point::new(tail.x + landing_gap, tail.y);
        if self.content_type == "Block" {
            let scales = self.block_scale.iter().map(|value| value.trim().parse::<f32>().unwrap_or(1.0).abs()).collect::<Vec<_>>();
            let (scale_x, scale_y) = if self.enable_block_scale {
                (scales[0].clamp(0.3, 2.5), scales[1].clamp(0.3, 2.5))
            } else {
                (1.0, 1.0)
            };
            let size = Size::new(42.0 * scale_x, 30.0 * scale_y);
            let rotation = if self.enable_block_rotation {
                self.block_rotation.trim().parse::<f32>().unwrap_or(0.0).to_radians()
            } else {
                0.0
            };
            let center_x = if self.block_connection_code == "BasePoint" {
                content_origin.x
            } else {
                content_origin.x + size.width * 0.5
            };
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(
                    center_x,
                    content_origin.y,
                ));
                frame.rotate(iced::Radians(rotation));
                let top_left = Point::new(-size.width * 0.5, -size.height * 0.5);
                let rect = canvas::Path::rectangle(top_left, size);
                let color = aci_color(&self.block_color, ink);
                frame.stroke(
                    &rect,
                    canvas::Stroke::default().with_color(color).with_width(1.5),
                );
                frame.stroke(
                    &canvas::Path::line(
                        top_left,
                        Point::new(size.width * 0.5, size.height * 0.5),
                    ),
                    canvas::Stroke::default().with_color(color),
                );
            });
        } else if self.content_type != "None" {
            let content = if self.content_type == "Tolerance" {
                "⌀0.01 | A".to_string()
            } else if self.default_text.trim().is_empty() {
                t!("Leader note").into_owned()
            } else {
                self.default_text.clone()
            };
            let text_size = self.text_height.trim().parse::<f32>().unwrap_or(0.18).abs().mul_add(22.0, 8.0).clamp(9.0, 19.0) * scale;
            let box_width = (content.chars().count() as f32 * text_size * 0.55 + 14.0).clamp(60.0, bounds.width - content_origin.x - 8.0);
            let box_height = text_size + 12.0 + self.align_space.trim().parse::<f32>().unwrap_or(0.0).abs().min(8.0);
            let attachment_offset = if self.attachment_direction == "Horizontal" {
                match self.left_attachment_code.as_str() {
                    "TopOfTopLine" | "BottomOfTopLineUnderlineTopLine" => box_height * 0.5,
                    "MiddleOfTopLine" => box_height * 0.25,
                    "MiddleOfBottomLine" => -box_height * 0.25,
                    "BottomOfBottomLine" | "BottomLine"
                    | "BottomOfTopLineUnderlineBottomLine" => -box_height * 0.5,
                    _ => 0.0,
                }
            } else {
                0.0
            };
            let content_y = content_origin.y + attachment_offset;
            if self.text_frame {
                frame.stroke(
                    &canvas::Path::rectangle(Point::new(content_origin.x, content_y - box_height * 0.5), Size::new(box_width, box_height)),
                    canvas::Stroke::default().with_color(aci_color(&self.text_color, ink)).with_width(1.0),
                );
            }
            let align = if self.text_always_left || self.text_alignment == "Left" {
                iced::advanced::text::Alignment::Left
            } else if self.text_alignment == "Right" {
                iced::advanced::text::Alignment::Right
            } else {
                iced::advanced::text::Alignment::Center
            };
            let x = match align {
                iced::advanced::text::Alignment::Left => content_origin.x + 5.0,
                iced::advanced::text::Alignment::Right => content_origin.x + box_width - 5.0,
                _ => content_origin.x + box_width * 0.5,
            };
            let rotation = if self.attachment_direction == "Vertical" {
                std::f32::consts::FRAC_PI_2
            } else if self.text_angle == "ParallelToLastLeaderLine" {
                -angle2
            } else {
                0.0
            };
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(x, content_y));
                frame.rotate(iced::Radians(rotation));
                frame.fill_text(canvas::Text {
                    content,
                    position: Point::ORIGIN,
                    color: aci_color(&self.text_color, ink),
                    size: iced::Pixels(text_size),
                    align_x: align,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            });
        }
        frame.fill_text(canvas::Text {
            content: format!(
                "{} · {} · {} · {} · {} / {} · {}",
                handle_label(&self.line_type),
                self.arrow_name,
                self.text_style,
                self.block_name,
                self.leader_draw_order,
                self.multileader_draw_order,
                self.description,
            ),
            position: Point::new(10.0, bounds.height - 9.0),
            color: ink.scale_alpha(0.62),
            size: iced::Pixels(9.0),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        frame.fill_text(canvas::Text {
            content: format!(
                "{} / {} / {} / {} · {} · {}{} · z {} · {}",
                self.left_attachment,
                self.right_attachment,
                self.top_attachment,
                self.bottom_attachment,
                self.block_connection,
                if self.enable_block_rotation { &self.block_rotation } else { "0" },
                "°",
                self.block_scale[2],
                if self.annotative { t!("Annotative") } else { t!("Drawing scale") },
            ),
            position: Point::new(10.0, 9.0),
            color: ink.scale_alpha(0.55),
            size: iced::Pixels(9.0),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

pub fn view_window<'a>(
    v: MLeaderStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let style = v.style;
    let content: Element<'a, Message> = match (v.tab, style) {
        (_, None) => text(t!("Select a style to view details.")).size(11).into(),
        (0, Some(style)) => column![
            input_row(t!("Description"), "", v.description, "description"),
            enum_row(t!("Path type"), &[("Invisible", "None"), ("StraightLineSegments", "Straight"), ("Spline", "Spline")], format!("{:?}", style.path_type), "path_type"),
            color_row(t!("Line color"), v.line_color, "line_color", v.color_open == Some("line_color")),
            row![
                text(t!("Line weight")).size(11).style(muted_style).width(165),
                iced::widget::pick_list(
                    Some(crate::ui::properties::LwItem(style.line_weight)),
                    crate::ui::properties::lw_options(),
                    |value| value.to_string(),
                )
                .on_select(|item| Message::MLeaderStyleLineWeightChanged(item.0))
                .text_size(11).width(220),
            ].spacing(8).align_y(iced::Center),
            handle_row(t!("Line type"), v.lt_opts.clone(), v.line_type_name.clone(), "line_type_handle"),
            handle_row(t!("Arrowhead"), v.arrow_opts.clone(), v.arrowhead_name.clone(), "arrowhead_handle"),
            input_row(t!("Arrowhead size"), "0.18", v.arrowhead_size, "arrowhead_size"),
            input_row(t!("Break gap size"), "0.125", v.break_gap, "break_gap"),
        ].spacing(8).into(),
        (1, Some(style)) => column![
            toggle(t!("Enable landing"), style.enable_landing, "enable_landing"),
            toggle(t!("Enable dogleg"), style.enable_dogleg, "enable_dogleg"),
            input_row(t!("Landing distance"), "8.0", v.landing_distance, "landing_distance"),
            input_row(t!("Landing gap"), "0.09", v.landing_gap, "landing_gap"),
            input_row(t!("Maximum leader points"), "2", v.max_points, "max_points"),
            input_row(t!("First segment angle"), "0", v.first_seg_angle, "first_seg_angle"),
            input_row(t!("Second segment angle"), "0", v.second_seg_angle, "second_seg_angle"),
            if style.is_annotative { readonly_row(t!("Scale factor"), t!("By annotation scale")) } else { input_row(t!("Scale factor"), "1.0", v.scale_factor, "scale_factor") },
            input_row(t!("Alignment spacing"), "4.0", v.align_space, "align_space"),
            enum_row(t!("Leader draw order"), &LEADER_DRAW_ORDERS, format!("{:?}", style.leader_draw_order), "leader_draw_order"),
            enum_row(t!("Multileader draw order"), &MULTILEADER_DRAW_ORDERS, format!("{:?}", style.multileader_draw_order), "multileader_draw_order"),
            toggle(t!("Annotative"), style.is_annotative, "annotative"),
        ].spacing(8).into(),
        (2, Some(style)) => column![
            enum_row(t!("Content type"), &[("None", "None"), ("Block", "Block"), ("MText", "Text"), ("Tolerance", "Tolerance")], format!("{:?}", style.content_type), "content_type"),
            input_row(t!("Default text"), "", v.default_text, "default_text"),
            handle_row(t!("Text style"), v.textstyle_opts.clone(), v.text_style_name.clone(), "text_style_handle"),
            input_row(t!("Text height"), "0.18", v.text_height, "text_height"),
            color_row(t!("Text color"), v.text_color, "text_color", v.color_open == Some("text_color")),
            enum_row(t!("Text angle"), &[("ParallelToLastLeaderLine", "Parallel to last segment"), ("Horizontal", "Horizontal"), ("Optimized", "Optimized")], format!("{:?}", style.text_angle_type), "text_angle_type"),
            enum_row(t!("Text alignment"), &[("Left", "Left"), ("Center", "Center"), ("Right", "Right")], format!("{:?}", style.text_alignment), "text_alignment"),
            enum_row(t!("Attachment direction"), &[("Horizontal", "Horizontal"), ("Vertical", "Vertical")], format!("{:?}", style.text_attachment_direction), "text_attachment_direction"),
            enum_row(t!("Left attachment"), &ATTACHMENTS, format!("{:?}", style.text_left_attachment), "text_left_attachment"),
            enum_row(t!("Right attachment"), &ATTACHMENTS, format!("{:?}", style.text_right_attachment), "text_right_attachment"),
            enum_row(t!("Top attachment"), &ATTACHMENTS, format!("{:?}", style.text_top_attachment), "text_top_attachment"),
            enum_row(t!("Bottom attachment"), &ATTACHMENTS, format!("{:?}", style.text_bottom_attachment), "text_bottom_attachment"),
            row![toggle(t!("Text frame"), style.text_frame, "text_frame"), toggle(t!("Always left align"), style.text_always_left, "text_always_left")].spacing(18),
        ].spacing(8).into(),
        (_, Some(style)) => column![
            handle_row(t!("Block"), v.block_opts.clone(), v.block_content_name.clone(), "block_content_handle"),
            color_row(t!("Block color"), v.block_color, "block_color", v.color_open == Some("block_color")),
            enum_row(t!("Block connection"), &BLOCK_CONNECTIONS, format!("{:?}", style.block_content_connection), "block_content_connection"),
            input_row(t!("Block rotation"), "0", v.block_rotation, "block_rotation"),
            input_row(t!("Block scale X"), "1.0", v.block_scale_x, "block_scale_x"),
            input_row(t!("Block scale Y"), "1.0", v.block_scale_y, "block_scale_y"),
            input_row(t!("Block scale Z"), "1.0", v.block_scale_z, "block_scale_z"),
            toggle(t!("Enable block scale"), style.enable_block_scale, "enable_block_scale"),
            toggle(t!("Enable block rotation"), style.enable_block_rotation, "enable_block_rotation"),
        ].spacing(8).into(),
    };
    let preview: Element<'a, Message> = if let Some(style) = style {
        canvas(LeaderPreviewCanvas {
            path_type: format!("{:?}", style.path_type),
            content_type: format!("{:?}", style.content_type),
            line_color: v.line_color.to_string(),
            line_weight: style.line_weight.value(),
            line_type: v.line_type_name.clone(),
            arrow_name: handle_label(&v.arrowhead_name),
            arrow_name_code: v.arrowhead_name.clone(),
            arrow_size: v.arrowhead_size.to_string(),
            break_gap: v.break_gap.to_string(),
            landing: style.enable_landing,
            dogleg: style.enable_dogleg,
            landing_distance: v.landing_distance.to_string(),
            landing_gap: v.landing_gap.to_string(),
            max_points: v.max_points.to_string(),
            first_angle: v.first_seg_angle.to_string(),
            second_angle: v.second_seg_angle.to_string(),
            scale: v.scale_factor.to_string(),
            annotative: style.is_annotative,
            align_space: v.align_space.to_string(),
            default_text: v.default_text.to_string(),
            text_style: handle_label(&v.text_style_name),
            text_height: v.text_height.to_string(),
            text_color: v.text_color.to_string(),
            text_angle: format!("{:?}", style.text_angle_type),
            text_alignment: format!("{:?}", style.text_alignment),
            attachment_direction: format!("{:?}", style.text_attachment_direction),
            left_attachment: choice_label(&ATTACHMENTS, format!("{:?}", style.text_left_attachment)),
            left_attachment_code: format!("{:?}", style.text_left_attachment),
            right_attachment: choice_label(&ATTACHMENTS, format!("{:?}", style.text_right_attachment)),
            top_attachment: choice_label(&ATTACHMENTS, format!("{:?}", style.text_top_attachment)),
            bottom_attachment: choice_label(&ATTACHMENTS, format!("{:?}", style.text_bottom_attachment)),
            text_frame: style.text_frame,
            text_always_left: style.text_always_left,
            block_name: handle_label(&v.block_content_name),
            block_color: v.block_color.to_string(),
            block_connection: choice_label(&BLOCK_CONNECTIONS, format!("{:?}", style.block_content_connection)),
            block_connection_code: format!("{:?}", style.block_content_connection),
            block_rotation: v.block_rotation.to_string(),
            block_scale: [v.block_scale_x.to_string(), v.block_scale_y.to_string(), v.block_scale_z.to_string()],
            enable_block_scale: style.enable_block_scale,
            enable_block_rotation: style.enable_block_rotation,
            leader_draw_order: choice_label(&LEADER_DRAW_ORDERS, format!("{:?}", style.leader_draw_order)),
            multileader_draw_order: choice_label(&MULTILEADER_DRAW_ORDERS, format!("{:?}", style.multileader_draw_order)),
            description: v.description.to_string(),
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
                on_select: Message::MLeaderStyleDialogCompare,
            }),
            tabs: [t!("Leader Format"), t!("Leader Structure"), t!("Content"), t!("Block Content")]
                .into_iter()
                .enumerate()
                .map(|(tab, label)| crate::ui::style::style_manager::EditorTab {
                    label,
                    active: v.tab == tab as u8,
                    on_press: Message::MLeaderStyleDialogTab(tab as u8),
                })
                .collect(),
            content,
        },
    );
    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: crate::app::StyleKind::MLeader,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current.as_str()),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: Message::MLeaderStyleDialogNew,
        on_copy: Message::MLeaderStyleDialogCopy,
        on_delete: Message::MLeaderStyleDialogDelete,
        on_select: Message::MLeaderStyleDialogSelect,
        on_set_current: Message::MLeaderStyleDialogSetCurrent,
        on_apply: Message::MLeaderStyleApply,
        read_only: false,
        editor,
    })
}
