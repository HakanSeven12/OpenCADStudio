//! Block Definition dialog window — create or redefine a block definition.
//! Prompted when the user runs the `BLOCK` or `BMAKE` command.

use std::fmt;

use codec::Handle;
use iced::widget::{
    button, checkbox, column, combo_box, container, mouse_area, pick_list, row, stack, text,
    text_editor, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::draw::units;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style, form_radio};

static ICON_PICK_POINT: &[u8] = include_bytes!("../../../assets/icons/blocks/pick_point.svg");
static ICON_SELECT_OBJECTS: &[u8] =
    include_bytes!("../../../assets/icons/blocks/select_objects.svg");
static ICON_QUICK_SELECT: &[u8] = include_bytes!("../../../assets/icons/blocks/quick_select.svg");
static ICON_WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockObjectMode {
    Retain,
    Convert,
    Delete,
}

impl Default for BlockObjectMode {
    fn default() -> Self {
        BlockObjectMode::Convert
    }
}

/// Dropdown entry for the Block Unit picker.
#[derive(Clone, PartialEq, Eq)]
pub struct UnitChoice {
    pub code: i16,
    pub label: &'static str,
}

impl fmt::Display for UnitChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = crate::i18n::translate(self.label);
        f.write_str(name.as_ref())
    }
}

/// The Block Definition dialog's working copy.
pub struct BlockDefinitionState {
    pub name: String,
    pub existing_names: Vec<String>,
    pub name_combo: combo_box::State<String>,
    pub base_point_specify_onscreen: bool,
    pub base_point_x: String,
    pub base_point_y: String,
    pub base_point_z: String,
    pub objects_specify_onscreen: bool,
    pub selected_handles: Vec<Handle>,
    pub object_mode: BlockObjectMode,
    pub annotative: bool,
    pub match_orientation: bool,
    pub scale_uniformly: bool,
    pub allow_exploding: bool,
    pub unit: i16,
    pub description: String,
    pub description_content: text_editor::Content,
    pub hyperlink_url: String,
    pub hyperlink_desc: String,
    pub error_message: Option<String>,
    pub confirm_redefine: Option<String>,
    pub desc_placeholder: String,
}

impl Clone for BlockDefinitionState {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            existing_names: self.existing_names.clone(),
            name_combo: self.name_combo.clone(),
            base_point_specify_onscreen: self.base_point_specify_onscreen,
            base_point_x: self.base_point_x.clone(),
            base_point_y: self.base_point_y.clone(),
            base_point_z: self.base_point_z.clone(),
            objects_specify_onscreen: self.objects_specify_onscreen,
            selected_handles: self.selected_handles.clone(),
            object_mode: self.object_mode,
            annotative: self.annotative,
            match_orientation: self.match_orientation,
            scale_uniformly: self.scale_uniformly,
            allow_exploding: self.allow_exploding,
            unit: self.unit,
            description: self.description.clone(),
            description_content: text_editor::Content::with_text(&self.description),
            hyperlink_url: self.hyperlink_url.clone(),
            hyperlink_desc: self.hyperlink_desc.clone(),
            error_message: self.error_message.clone(),
            confirm_redefine: self.confirm_redefine.clone(),
            desc_placeholder: self.desc_placeholder.clone(),
        }
    }
}

impl BlockDefinitionState {
    pub fn new(existing_names: Vec<String>, pre_selected: Vec<Handle>, default_unit: i16) -> Self {
        Self {
            name: String::new(),
            name_combo: combo_box::State::new(existing_names.clone()),
            existing_names,
            base_point_specify_onscreen: false,
            base_point_x: "0.0000".to_string(),
            base_point_y: "0.0000".to_string(),
            base_point_z: "0.0000".to_string(),
            objects_specify_onscreen: false,
            selected_handles: pre_selected,
            object_mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: default_unit,
            description: String::new(),
            description_content: text_editor::Content::new(),
            hyperlink_url: String::new(),
            hyperlink_desc: String::new(),
            error_message: None,
            confirm_redefine: None,
            desc_placeholder: crate::t!("Enter block description...").into_owned(),
        }
    }

    pub fn parse_base_point(&self) -> glam::DVec3 {
        let x = self.base_point_x.trim().parse::<f64>().unwrap_or(0.0);
        let y = self.base_point_y.trim().parse::<f64>().unwrap_or(0.0);
        let z = self.base_point_z.trim().parse::<f64>().unwrap_or(0.0);
        glam::DVec3::new(x, y, z)
    }

    pub fn is_existing_name(&self) -> bool {
        let trimmed = self.name.trim();
        !trimmed.is_empty()
            && self
                .existing_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    }

    pub fn invalid_name_char(&self) -> Option<char> {
        self.name.chars().find(|&c| {
            matches!(
                c,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '=' | '`'
            )
        })
    }
}

pub(crate) fn group<'a>(
    title: impl Into<String>,
    body: impl Into<Element<'a, Message>>,
    height: impl Into<Length>,
) -> Element<'a, Message> {
    container(
        column![
            text(title.into())
                .size(11)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().background.strongest.text.scale_alpha(0.85)),
                }),
            body.into(),
        ]
        .spacing(4),
    )
    .padding([5, 8])
    .width(Fill)
    .height(height)
    .style(|theme: &Theme| container::Style {
        border: Border {
            width: 1.0,
            radius: 4.0.into(),
            color: theme.palette().background.strong.color,
        },
        ..Default::default()
    })
    .into()
}

pub(crate) fn labeled_checkbox<'a>(
    label: impl Into<String>,
    is_checked: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        checkbox(is_checked).on_toggle(on_toggle).size(14),
        text(label.into()).size(11),
    ]
    .spacing(5)
    .align_y(iced::Center)
    .into()
}

pub fn view_window<'a>(
    state: &'a BlockDefinitionState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // ── Name Row ─────────────────────────────────────────────────────────────
    let name_combo = combo_box(
        &state.name_combo,
        t!("Block name").as_ref(),
        if state.name.is_empty() {
            None
        } else {
            Some(&state.name)
        },
        |chosen: String| Message::BlockDefNameSelect(chosen),
    )
    .on_input(Message::BlockDefName)
    .size(11)
    .padding([3, 6])
    .width(Fill);

    let name_hint: Option<Element<'a, Message>> = if let Some(invalid_char) =
        state.invalid_name_char()
    {
        Some(
            text(crate::tf!("Invalid character '{invalid_char}' in block name (\\ / : * ? \" < > | = ` are not allowed)"))
                .size(10)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().danger.base.color),
                })
                .into(),
        )
    } else if state.is_existing_name() {
        Some(
            text(crate::tf!(
                "Block \"{}\" already exists — will redefine upon OK",
                state.name.trim()
            ))
            .size(10)
            .style(|theme: &Theme| text::Style {
                color: Some(theme.palette().primary.base.color),
            })
            .into(),
        )
    } else {
        None
    };

    let mut name_column = column![text(t!("Name:")).size(11).style(muted_style), name_combo,]
        .spacing(3)
        .width(Fill);

    if let Some(hint) = name_hint {
        name_column = name_column.push(hint);
    }

    let name_section = name_column;

    // ── Group: Base Point ─────────────────────────────────────────────────────
    let base_onscreen = labeled_checkbox(
        t!("Specify On-screen"),
        state.base_point_specify_onscreen,
        Message::BlockDefBaseOnScreen,
    );

    let pick_pt_icon = crate::ui::icons::semantic(ICON_PICK_POINT, 15.0);

    let mut pick_pt_btn = button(
        row![pick_pt_icon, text(t!("Pick point")).size(11)]
            .spacing(5)
            .align_y(iced::Center),
    )
    .padding([3, 8]);

    if !state.base_point_specify_onscreen {
        pick_pt_btn = pick_pt_btn
            .on_press(Message::BlockDefPickPoint)
            .style(button_style(false));
    }

    let coord_row = |axis: &'static str, val: &'a str, on_input: fn(String) -> Message| {
        let is_enabled = !state.base_point_specify_onscreen;
        let input: Element<'a, Message> = if is_enabled {
            text_input("", val)
                .on_input(on_input)
                .on_submit(Message::BlockDefApply)
                .style(field_style)
                .size(11)
                .padding([2, 5])
                .width(Fill)
                .into()
        } else {
            crate::ui::read_only::field(val, 11.0, Fill)
        };
        row![
            text(axis)
                .size(11)
                .style(muted_style)
                .width(Length::Fixed(16.0)),
            input,
        ]
        .spacing(4)
        .align_y(iced::Center)
    };

    let base_point_group = group(
        t!("Base point").into_owned(),
        column![
            base_onscreen,
            Space::new().height(1),
            pick_pt_btn,
            Space::new().height(2),
            coord_row("X:", &state.base_point_x, Message::BlockDefBaseX),
            coord_row("Y:", &state.base_point_y, Message::BlockDefBaseY),
            coord_row("Z:", &state.base_point_z, Message::BlockDefBaseZ),
        ]
        .spacing(3),
        Length::Fixed(152.0),
    );

    // ── Group: Objects ────────────────────────────────────────────────────────
    let objects_onscreen = labeled_checkbox(
        t!("Specify On-screen"),
        state.objects_specify_onscreen,
        Message::BlockDefObjectsOnScreen,
    );

    let sel_obj_icon = crate::ui::icons::semantic(ICON_SELECT_OBJECTS, 15.0);

    let mut sel_obj_btn = button(
        row![sel_obj_icon, text(t!("Select objects")).size(11)]
            .spacing(5)
            .align_y(iced::Center),
    )
    .padding([3, 8]);

    if !state.objects_specify_onscreen {
        sel_obj_btn = sel_obj_btn
            .on_press(Message::BlockDefSelectObjects)
            .style(button_style(false));
    }

    let qsel_icon = crate::ui::icons::semantic(ICON_QUICK_SELECT, 15.0);
    let mut qsel_btn = button(qsel_icon).padding([3, 5]).style(button_style(false));

    if !state.objects_specify_onscreen {
        qsel_btn = qsel_btn.on_press(Message::BlockDefQuickSelect);
    }

    let btns_row = row![sel_obj_btn, qsel_btn].spacing(5).align_y(iced::Center);

    let radios = column![
        form_radio(
            t!("Retain"),
            BlockObjectMode::Retain,
            Some(state.object_mode),
            Message::BlockDefObjectMode,
        ),
        form_radio(
            t!("Convert to block"),
            BlockObjectMode::Convert,
            Some(state.object_mode),
            Message::BlockDefObjectMode,
        ),
        form_radio(
            t!("Delete"),
            BlockObjectMode::Delete,
            Some(state.object_mode),
            Message::BlockDefObjectMode,
        ),
    ]
    .spacing(2);

    let count = state.selected_handles.len();
    let status_row: Element<'a, Message> = if count == 0 {
        row![
            crate::ui::icons::semantic(ICON_WARN, 13.0),
            text(t!("No objects selected"))
                .size(10)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().background.strongest.text.scale_alpha(0.75)),
                }),
        ]
        .spacing(5)
        .align_y(iced::Center)
        .into()
    } else {
        row![text(crate::tf!("{count} objects selected"))
            .size(10)
            .style(|theme: &Theme| text::Style {
                color: Some(theme.palette().primary.base.color),
            }),]
        .spacing(5)
        .align_y(iced::Center)
        .into()
    };

    let objects_group = group(
        t!("Objects").into_owned(),
        column![
            objects_onscreen,
            Space::new().height(1),
            btns_row,
            Space::new().height(2),
            radios,
            Space::new().height(2),
            status_row,
        ]
        .spacing(2),
        Length::Fixed(152.0),
    );

    // ── Group: Behavior ───────────────────────────────────────────────────────
    let annotative_cb = labeled_checkbox(
        t!("Annotative"),
        state.annotative,
        Message::BlockDefAnnotative,
    );

    let match_orient_cb: Element<'a, Message> = if state.annotative {
        row![
            Space::new().width(16),
            checkbox(state.match_orientation)
                .on_toggle(Message::BlockDefMatchOrientation)
                .size(13),
            column![
                text(t!("Match block orientation")).size(10),
                text(t!("to layout")).size(10),
            ]
            .spacing(1),
        ]
        .spacing(5)
        .align_y(iced::Center)
        .into()
    } else {
        row![
            Space::new().width(16),
            checkbox(false).size(13),
            column![
                text(t!("Match block orientation"))
                    .size(10)
                    .style(muted_style),
                text(t!("to layout")).size(10).style(muted_style),
            ]
            .spacing(1),
        ]
        .spacing(5)
        .align_y(iced::Center)
        .into()
    };

    let scale_uniform_cb = labeled_checkbox(
        t!("Scale uniformly"),
        state.scale_uniformly,
        Message::BlockDefScaleUniformly,
    );

    let allow_explode_cb = labeled_checkbox(
        t!("Allow exploding"),
        state.allow_exploding,
        Message::BlockDefAllowExploding,
    );

    let behavior_group = group(
        t!("Behavior").into_owned(),
        column![
            annotative_cb,
            Space::new().height(2),
            match_orient_cb,
            Space::new().height(4),
            scale_uniform_cb,
            Space::new().height(4),
            allow_explode_cb,
        ]
        .spacing(3),
        Length::Fixed(152.0),
    );

    // ── Group: Settings (Column 1 Bottom) ─────────────────────────────────────
    let unit_options: Vec<UnitChoice> = units::all()
        .map(|(code, label)| UnitChoice { code, label })
        .collect();

    let selected_unit = unit_options
        .iter()
        .find(|choice| choice.code == state.unit)
        .cloned();

    let unit_picker = pick_list(selected_unit, unit_options, |choice| choice.to_string())
        .on_select(|choice| Message::BlockDefUnit(choice.code))
        .text_size(11)
        .padding([2, 5])
        .width(Fill);

    let hyperlink_btn = button(
        text(if state.hyperlink_url.is_empty() {
            t!("Hyperlink...")
        } else {
            t!("Hyperlink*")
        })
        .size(11),
    )
    .on_press(Message::BlockDefHyperlink)
    .style(button_style(false))
    .padding([3, 14]);

    let settings_group = group(
        t!("Settings").into_owned(),
        column![
            text(t!("Block unit:")).size(10).style(muted_style),
            unit_picker,
            Space::new().height(6),
            row![
                Space::new().width(Fill),
                hyperlink_btn,
                Space::new().width(Fill)
            ],
        ]
        .spacing(3),
        Length::Fixed(102.0),
    );

    // ── Group: Description (Column 2 Bottom) ──────────────────────────────────
    let desc_editor = text_editor(&state.description_content)
        .on_action(Message::BlockDefDescriptionAction)
        .placeholder(&state.desc_placeholder)
        .size(11)
        .padding([4, 6])
        .height(Fill)
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            let border_color = match status {
                text_editor::Status::Focused { .. } => palette.primary.base.color,
                _ => palette.background.neutral.color,
            };
            text_editor::Style {
                background: Background::Color(palette.background.base.color),
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                placeholder: palette.background.base.text.scale_alpha(0.48),
                value: palette.background.base.text,
                selection: palette.primary.base.color.scale_alpha(0.5),
            }
        });

    let description_group = group(
        t!("Description").into_owned(),
        desc_editor,
        Length::Fixed(102.0),
    );

    // ── Layout Assembly: Left Column & Right Column ───────────────────────────
    let left_column = column![base_point_group, settings_group,]
        .spacing(6)
        .width(Length::FillPortion(1));

    let top_right_row = row![objects_group, behavior_group,].spacing(6).width(Fill);

    let right_column = column![top_right_row, description_group,]
        .spacing(6)
        .width(Length::FillPortion(2));

    let main_body = row![left_column, right_column].spacing(6).width(Fill);

    // ── Footer Bar ────────────────────────────────────────────────────────────
    let actions = row![
        Space::new().width(Fill),
        dialog_button(t!("OK"), Message::BlockDefApply, true),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("Help"), Message::BlockDefHelp, false),
    ]
    .spacing(8)
    .align_y(iced::Center);

    // ── Error Banner ──────────────────────────────────────────────────────────
    let mut main_column = column![name_section, main_body, actions,]
        .spacing(6)
        .padding([8, 10])
        .width(sizing.width);

    if let Some(err) = &state.error_message {
        let err_banner = container(
            row![
                crate::ui::icons::semantic(ICON_WARN, 16.0),
                text(err.clone())
                    .size(11)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.palette().background.strongest.text),
                    }),
                Space::new().width(Fill),
                button(text("×").size(12))
                    .on_press(Message::BlockDefDismissError)
                    .style(button_style(false))
                    .padding([1, 6]),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([6, 10])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().danger.base.color.scale_alpha(0.18),
            )),
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().danger.base.color,
            },
            ..Default::default()
        });

        main_column = column![err_banner, main_column]
            .spacing(8)
            .width(sizing.width)
            .padding(12);
    }

    let main_content = main_column.into();

    // ── Confirm Redefinition Guard ────────────────────────────────────────────
    if let Some(name) = &state.confirm_redefine {
        let question = crate::tf!("Block \"{name}\" already exists.\nDo you want to redefine it?");
        let shield = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        theme.palette().background.strongest.color.scale_alpha(0.55),
                    )),
                    ..Default::default()
                }),
        )
        .on_press(Message::BlockDefConfirmRedefine(false));

        let panel = container(
            column![
                text(question)
                    .size(13)
                    .align_x(iced::alignment::Horizontal::Center),
                Space::new().height(16),
                row![
                    dialog_button(t!("Yes"), Message::BlockDefConfirmRedefine(true), false),
                    // Default / safe focus on "No"
                    dialog_button(t!("No"), Message::BlockDefConfirmRedefine(false), true),
                ]
                .spacing(12)
                .align_y(iced::Center),
            ]
            .spacing(0)
            .align_x(iced::alignment::Horizontal::Center),
        )
        .padding([20, 24])
        .style(container::rounded_box);

        return stack![
            main_content,
            shield,
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Fill)
                .center_y(Fill),
        ]
        .into();
    }

    main_content
}
