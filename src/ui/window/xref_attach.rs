//! Attach External Reference dialog — the options chosen after picking a
//! drawing with ATTACH / XATTACH: reference type, scale, insertion point,
//! path type and rotation, with the file's preview and block unit.

use std::fmt;

use iced::widget::{
    button, column, combo_box, container, image, pick_list, radio, row, text, text_input, Space,
};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::io::xref_model::Pathtype;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, field_style};
use crate::ui::window::block_definition::{group, labeled_checkbox};

/// One edit in the dialog.
#[derive(Debug, Clone)]
pub enum XrefAttachMsg {
    /// Name box: a reference already in the drawing, picked or typed.
    Name(String),
    Browse,
    Overlay(bool),
    ScaleOnScreen(bool),
    Scale(usize, String),
    Uniform(bool),
    InsertOnScreen(bool),
    Insert(usize, String),
    PathType(PathTypeChoice),
    RotationOnScreen(bool),
    Rotation(String),
    Details(bool),
    Apply,
    Help,
}

/// Path type choice as the dialog lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathTypeChoice(pub Pathtype);

impl PathTypeChoice {
    pub const ALL: [PathTypeChoice; 3] = [
        PathTypeChoice(Pathtype::Full),
        PathTypeChoice(Pathtype::Relative),
        PathTypeChoice(Pathtype::None),
    ];
}

impl fmt::Display for PathTypeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.0 {
            Pathtype::Full => t!("Full path"),
            Pathtype::Relative => t!("Relative path"),
            Pathtype::None => t!("No path"),
        };
        f.write_str(label.as_ref())
    }
}

/// The dialog's working copy.
#[derive(Clone)]
pub struct XrefAttachState {
    /// Absolute path of the drawing to attach.
    pub path: String,
    /// Reference name (the file name without extension).
    pub name: String,
    /// References already in the drawing: name and saved path.
    pub existing: Vec<(String, String)>,
    pub name_combo: combo_box::State<String>,
    pub overlay: bool,
    pub scale_on_screen: bool,
    pub scale: [String; 3],
    pub uniform: bool,
    pub insert_on_screen: bool,
    pub insert: [String; 3],
    pub path_type: PathTypeChoice,
    pub rotation_on_screen: bool,
    pub rotation: String,
    /// Block unit of the chosen drawing and its factor to the host's unit.
    pub unit_label: String,
    pub unit_factor: String,
    pub found_in: String,
    pub saved_path: String,
    pub details: bool,
    pub preview: Option<image::Handle>,
}

impl XrefAttachState {
    pub fn new(existing: Vec<(String, String)>) -> Self {
        let names = existing.iter().map(|(name, _)| name.clone()).collect();
        Self {
            path: String::new(),
            name: String::new(),
            existing,
            name_combo: combo_box::State::new(names),
            overlay: false,
            scale_on_screen: false,
            scale: ["1.0000".into(), "1.0000".into(), "1.0000".into()],
            uniform: false,
            insert_on_screen: true,
            insert: ["0.0000".into(), "0.0000".into(), "0.0000".into()],
            path_type: PathTypeChoice(Pathtype::Relative),
            rotation_on_screen: false,
            rotation: "0".into(),
            unit_label: String::new(),
            unit_factor: String::new(),
            found_in: String::new(),
            saved_path: String::new(),
            details: false,
            preview: None,
        }
    }

    /// Parsed X/Y/Z scale; a uniform scale takes X for all three.
    pub fn scale_values(&self) -> Option<[f64; 3]> {
        let parse = |s: &String| s.trim().parse::<f64>().ok();
        let x = parse(&self.scale[0])?;
        if self.uniform {
            return Some([x, x, x]);
        }
        Some([x, parse(&self.scale[1])?, parse(&self.scale[2])?])
    }

    pub fn insert_point(&self) -> Option<glam::DVec3> {
        let parse = |s: &String| s.trim().parse::<f64>().ok();
        Some(glam::DVec3::new(
            parse(&self.insert[0])?,
            parse(&self.insert[1])?,
            parse(&self.insert[2])?,
        ))
    }
}

fn msg(m: XrefAttachMsg) -> Message {
    Message::XrefAttach(m)
}

fn number_field<'a>(
    label: &'a str,
    value: &'a str,
    enabled: bool,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let mut field = text_input("", value)
        .size(11)
        .padding([3, 6])
        .width(Fill)
        .style(field_style);
    if enabled {
        field = field.on_input(on_input);
    }
    row![
        text(label).size(11).style(muted_style).width(Length::Fixed(16.0)),
        field
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn read_only_field<'a>(label: String, value: &'a str, label_width: f32) -> Element<'a, Message> {
    row![
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(label_width)),
        text_input("", value)
            .size(11)
            .padding([3, 6])
            .width(Fill)
            .style(field_style),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub fn view_window<'a>(
    state: &'a XrefAttachState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // ── Name ────────────────────────────────────────────────────────────
    let name_combo = combo_box(
        &state.name_combo,
        "",
        if state.name.is_empty() {
            None
        } else {
            Some(&state.name)
        },
        |chosen: String| msg(XrefAttachMsg::Name(chosen)),
    )
    .size(11)
    .padding([3, 6])
    .width(Fill);
    let name_section = column![
        text(t!("Name:")).size(11).style(muted_style),
        row![
            name_combo,
            button(text(t!("Browse...")).size(11))
                .on_press(msg(XrefAttachMsg::Browse))
                .style(button_style(false))
                .padding([4, 12])
                .width(Length::Fixed(96.0)),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(3);

    // ── Preview + reference type ────────────────────────────────────────
    let preview_body: Element<'a, Message> = match &state.preview {
        Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let preview = group(
        t!("Preview").into_owned(),
        container(preview_body)
            .width(Fill)
            .height(Length::Fixed(132.0))
            .style(|theme: &Theme| container::Style {
                background: Some(iced::Background::Color(
                    theme.palette().background.base.color,
                )),
                border: iced::Border {
                    width: 1.0,
                    radius: 2.0.into(),
                    color: theme.palette().background.strong.color,
                },
                ..Default::default()
            }),
        Length::Shrink,
    );
    let reference_type = group(
        t!("Reference Type").into_owned(),
        column![
            radio(
                t!("Attachment"),
                false,
                Some(state.overlay),
                |v| msg(XrefAttachMsg::Overlay(v))
            )
            .size(14)
            .text_size(11),
            radio(t!("Overlay"), true, Some(state.overlay), |v| msg(
                XrefAttachMsg::Overlay(v)
            ))
            .size(14)
            .text_size(11),
        ]
        .spacing(6),
        Length::Shrink,
    );
    let left = column![preview, reference_type]
        .spacing(6)
        .width(Length::FillPortion(1));

    // ── Scale + insertion point ─────────────────────────────────────────
    let scale_enabled = !state.scale_on_screen;
    let scale = group(
        t!("Scale").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.scale_on_screen, |v| msg(
                XrefAttachMsg::ScaleOnScreen(v)
            )),
            number_field("X:", &state.scale[0], scale_enabled, |v| msg(
                XrefAttachMsg::Scale(0, v)
            )),
            number_field(
                "Y:",
                &state.scale[1],
                scale_enabled && !state.uniform,
                |v| msg(XrefAttachMsg::Scale(1, v))
            ),
            number_field(
                "Z:",
                &state.scale[2],
                scale_enabled && !state.uniform,
                |v| msg(XrefAttachMsg::Scale(2, v))
            ),
            labeled_checkbox(t!("Uniform Scale"), state.uniform, |v| msg(
                XrefAttachMsg::Uniform(v)
            )),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let insert_enabled = !state.insert_on_screen;
    let insertion = group(
        t!("Insertion point").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.insert_on_screen, |v| msg(
                XrefAttachMsg::InsertOnScreen(v)
            )),
            number_field("X:", &state.insert[0], insert_enabled, |v| msg(
                XrefAttachMsg::Insert(0, v)
            )),
            number_field("Y:", &state.insert[1], insert_enabled, |v| msg(
                XrefAttachMsg::Insert(1, v)
            )),
            number_field("Z:", &state.insert[2], insert_enabled, |v| msg(
                XrefAttachMsg::Insert(2, v)
            )),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let middle = column![scale, insertion]
        .spacing(6)
        .width(Length::FillPortion(1));

    // ── Path type, rotation, block unit ─────────────────────────────────
    let path_type = group(
        t!("Path type").into_owned(),
        pick_list(Some(state.path_type), PathTypeChoice::ALL, |choice| choice.to_string())
            .on_select(|v| msg(XrefAttachMsg::PathType(v)))
        .text_size(11)
        .padding([3, 6])
        .width(Fill),
        Length::Shrink,
    );
    let mut angle = text_input("", &state.rotation)
        .size(11)
        .padding([3, 6])
        .width(Fill)
        .style(field_style);
    if !state.rotation_on_screen {
        angle = angle.on_input(|v| msg(XrefAttachMsg::Rotation(v)));
    }
    let rotation = group(
        t!("Rotation").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.rotation_on_screen, |v| msg(
                XrefAttachMsg::RotationOnScreen(v)
            )),
            row![
                text(t!("Angle:"))
                    .size(11)
                    .style(muted_style)
                    .width(Length::Fixed(44.0)),
                angle
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let block_unit = group(
        t!("Block Unit").into_owned(),
        column![
            read_only_field(t!("Unit:").into_owned(), &state.unit_label, 52.0),
            read_only_field(t!("Factor:").into_owned(), &state.unit_factor, 52.0),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let right = column![path_type, rotation, block_unit]
        .spacing(6)
        .width(Length::FillPortion(1));

    let body = row![left, middle, right].spacing(6).width(Fill);

    // ── Details ─────────────────────────────────────────────────────────
    let details: Element<'a, Message> = if state.details {
        group(
            t!("Location").into_owned(),
            column![
                read_only_field(t!("Found in:").into_owned(), &state.found_in, 92.0),
                read_only_field(t!("Saved path:").into_owned(), &state.saved_path, 92.0),
            ]
            .spacing(5),
            Length::Shrink,
        )
    } else {
        Space::new().height(0).into()
    };

    // ── Footer ──────────────────────────────────────────────────────────
    let details_label = if state.details {
        t!("Hide Details")
    } else {
        t!("Show Details")
    };
    let footer = row![
        button(text(details_label).size(11))
            .on_press(msg(XrefAttachMsg::Details(!state.details)))
            .style(button_style(false))
            .padding([4, 12]),
        Space::new().width(Fill),
        button(text(t!("OK")).size(11))
            .on_press(msg(XrefAttachMsg::Apply))
            .style(button_style(true))
            .padding([4, 16]),
        button(text(t!("Cancel")).size(11))
            .on_press(Message::CloseModal)
            .style(button_style(false))
            .padding([4, 12]),
        button(text(t!("Help")).size(11))
            .on_press(msg(XrefAttachMsg::Help))
            .style(button_style(false))
            .padding([4, 12]),
    ]
    .spacing(6)
    .align_y(iced::Center);

    column![name_section, body, details, footer]
        .spacing(6)
        .padding([8, 10])
        .width(sizing.width)
        .into()
}
