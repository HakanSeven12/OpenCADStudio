//! Write Block (WBLOCK) dialog window — write a block, selected entities, or entire drawing to an external DWG/DXF file.

use std::fmt;

use acadrust::Handle;
use iced::widget::{
    button, column, combo_box, container, pick_list, row, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::draw::units;
use crate::t;
use crate::ui::style::form::{button_style, dialog_button, field_style, form_radio};

static ICON_PICK_POINT: &[u8] = include_bytes!("../../../assets/icons/blocks/pick_point.svg");
static ICON_SELECT_OBJECTS: &[u8] =
    include_bytes!("../../../assets/icons/blocks/select_objects.svg");
static ICON_QUICK_SELECT: &[u8] = include_bytes!("../../../assets/icons/blocks/quick_select.svg");
static ICON_WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WblockSourceMode {
    Block,
    EntireDrawing,
    Objects,
}

impl Default for WblockSourceMode {
    fn default() -> Self {
        WblockSourceMode::Objects
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WblockObjectMode {
    Retain,
    Convert,
    Delete,
}

impl Default for WblockObjectMode {
    fn default() -> Self {
        WblockObjectMode::Retain
    }
}

/// Dropdown entry for the Unit picker.
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

/// The Write Block dialog's state.
pub struct WblockState {
    pub source_mode: WblockSourceMode,
    pub block_name: String,
    pub existing_blocks: Vec<String>,
    pub block_combo: combo_box::State<String>,

    pub base_point_x: String,
    pub base_point_y: String,
    pub base_point_z: String,

    pub selected_handles: Vec<Handle>,
    pub object_mode: WblockObjectMode,

    pub file_path: String,
    pub unit: i16,

    pub error_message: Option<String>,
}

impl Clone for WblockState {
    fn clone(&self) -> Self {
        Self {
            source_mode: self.source_mode,
            block_name: self.block_name.clone(),
            existing_blocks: self.existing_blocks.clone(),
            block_combo: self.block_combo.clone(),
            base_point_x: self.base_point_x.clone(),
            base_point_y: self.base_point_y.clone(),
            base_point_z: self.base_point_z.clone(),
            selected_handles: self.selected_handles.clone(),
            object_mode: self.object_mode,
            file_path: self.file_path.clone(),
            unit: self.unit,
            error_message: self.error_message.clone(),
        }
    }
}

impl WblockState {
    pub fn new(
        existing_blocks: Vec<String>,
        pre_selected: Vec<Handle>,
        default_unit: i16,
        default_path: String,
    ) -> Self {
        let default_block = existing_blocks.first().cloned().unwrap_or_default();
        Self {
            source_mode: WblockSourceMode::Objects,
            block_name: default_block,
            block_combo: combo_box::State::new(existing_blocks.clone()),
            existing_blocks,
            base_point_x: "0.00".to_string(),
            base_point_y: "0.00".to_string(),
            base_point_z: "0.00".to_string(),
            selected_handles: pre_selected,
            object_mode: WblockObjectMode::Retain,
            file_path: default_path,
            unit: default_unit,
            error_message: None,
        }
    }

    pub fn parse_base_point(&self) -> glam::DVec3 {
        let x = self.base_point_x.trim().parse::<f64>().unwrap_or(0.0);
        let y = self.base_point_y.trim().parse::<f64>().unwrap_or(0.0);
        let z = self.base_point_z.trim().parse::<f64>().unwrap_or(0.0);
        glam::DVec3::new(x, y, z)
    }
}

fn group<'a>(
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

pub fn view_window<'a>(
    state: &'a WblockState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let is_objects = state.source_mode == WblockSourceMode::Objects;
    let is_block = state.source_mode == WblockSourceMode::Block;

    // ── Source: Block Radio & Dropdown ─────────────────────────────────────────
    let block_radio = form_radio(
        t!("Block:"),
        WblockSourceMode::Block,
        Some(state.source_mode),
        Message::WblockSourceMode,
    );

    let block_dropdown: Element<'a, Message> = if is_block {
        combo_box(
            &state.block_combo,
            "",
            if state.block_name.is_empty() {
                None
            } else {
                Some(&state.block_name)
            },
            |chosen: String| Message::WblockBlockSelect(chosen),
        )
        .on_input(Message::WblockBlockName)
        .size(11)
        .padding([2, 5])
        .width(Fill)
        .into()
    } else {
        crate::ui::read_only::field(&state.block_name, 11.0, Fill)
    };

    let block_row = row![block_radio, block_dropdown]
        .spacing(8)
        .width(Fill)
        .align_y(iced::Center);

    let entire_drawing_radio = form_radio(
        t!("Entire drawing"),
        WblockSourceMode::EntireDrawing,
        Some(state.source_mode),
        Message::WblockSourceMode,
    );

    let objects_radio = form_radio(
        t!("Objects"),
        WblockSourceMode::Objects,
        Some(state.source_mode),
        Message::WblockSourceMode,
    );

    // ── Source: Base Point Sub-group ──────────────────────────────────────────
    let pick_pt_icon = crate::ui::icons::semantic(ICON_PICK_POINT, 14.0);
    let mut pick_pt_btn = button(
        row![pick_pt_icon, text(t!("Pick point")).size(11)]
            .spacing(6)
            .align_y(iced::Center),
    )
    .padding([3, 10]);

    if is_objects {
        pick_pt_btn = pick_pt_btn
            .on_press(Message::WblockPickPoint)
            .style(button_style(false));
    } else {
        pick_pt_btn = pick_pt_btn.style(button_style(false));
    }

    let coord_row = |axis: &'static str, val: &'a str, on_input: fn(String) -> Message| {
        let input: Element<'a, Message> = if is_objects {
            text_input("", val)
                .on_input(on_input)
                .on_submit(Message::WblockApply)
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
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().background.strongest.text.scale_alpha(0.85)),
                })
                .width(Length::Fixed(16.0)),
            input,
        ]
        .spacing(4)
        .align_y(iced::Center)
    };

    let base_point_group = group(
        t!("Base point").into_owned(),
        column![
            pick_pt_btn,
            Space::new().height(3),
            coord_row("X:", &state.base_point_x, Message::WblockBaseX),
            coord_row("Y:", &state.base_point_y, Message::WblockBaseY),
            coord_row("Z:", &state.base_point_z, Message::WblockBaseZ),
        ]
        .spacing(3),
        Length::Fixed(142.0),
    );

    // ── Source: Objects Sub-group ─────────────────────────────────────────────
    let sel_obj_icon = crate::ui::icons::semantic(ICON_SELECT_OBJECTS, 14.0);
    let mut sel_obj_btn = button(
        row![sel_obj_icon, text(t!("Select objects")).size(11)]
            .spacing(6)
            .align_y(iced::Center),
    )
    .padding([3, 8]);

    let qsel_icon = crate::ui::icons::semantic(ICON_QUICK_SELECT, 14.0);
    let mut qsel_btn = button(qsel_icon).padding([3, 6]);

    if is_objects {
        sel_obj_btn = sel_obj_btn
            .on_press(Message::WblockSelectObjects)
            .style(button_style(false));
        qsel_btn = qsel_btn
            .on_press(Message::WblockQuickSelect)
            .style(button_style(false));
    } else {
        sel_obj_btn = sel_obj_btn.style(button_style(false));
        qsel_btn = qsel_btn.style(button_style(false));
    }

    let obj_btns_row = row![sel_obj_btn, qsel_btn]
        .spacing(5)
        .align_y(iced::Center);

    let object_radios = column![
        form_radio(
            t!("Retain"),
            WblockObjectMode::Retain,
            if is_objects { Some(state.object_mode) } else { None },
            Message::WblockObjectMode,
        ),
        form_radio(
            t!("Convert to block"),
            WblockObjectMode::Convert,
            if is_objects { Some(state.object_mode) } else { None },
            Message::WblockObjectMode,
        ),
        form_radio(
            t!("Delete from drawing"),
            WblockObjectMode::Delete,
            if is_objects { Some(state.object_mode) } else { None },
            Message::WblockObjectMode,
        ),
    ]
    .spacing(3);

    let count = state.selected_handles.len();
    let status_row: Element<'a, Message> = if !is_objects {
        Space::new().height(14).into()
    } else if count == 0 {
        row![
            crate::ui::icons::semantic(ICON_WARN, 13.0),
            text(t!("No objects selected"))
                .size(10)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().warning.base.color),
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

    let objects_subgroup = group(
        t!("Objects").into_owned(),
        column![obj_btns_row, Space::new().height(2), object_radios, Space::new().height(2), status_row,]
            .spacing(2),
        Length::Fixed(142.0),
    );

    let sub_panels = row![base_point_group, objects_subgroup]
        .spacing(8)
        .width(Fill);

    let source_group = group(
        t!("Source").into_owned(),
        column![
            block_row,
            entire_drawing_radio,
            objects_radio,
            Space::new().height(4),
            sub_panels,
        ]
        .spacing(4),
        Length::Shrink,
    );

    // ── Destination Group ─────────────────────────────────────────────────────
    let file_input = text_input("", &state.file_path)
        .on_input(Message::WblockFilePath)
        .on_submit(Message::WblockApply)
        .style(field_style)
        .size(11)
        .padding([3, 6])
        .width(Fill);

    let browse_btn = button(
        container(text("...").size(11))
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fixed(28.0))
    .on_press(Message::WblockBrowsePath)
    .style(button::secondary)
    .padding([3, 0]);

    let path_row = row![file_input, browse_btn].spacing(5).align_y(iced::Center);

    let unit_options: Vec<UnitChoice> = units::all()
        .map(|(code, label)| UnitChoice { code, label })
        .collect();

    let selected_unit = unit_options
        .iter()
        .find(|choice| choice.code == state.unit)
        .cloned();

    let unit_picker = pick_list(selected_unit, unit_options, |choice| choice.to_string())
        .on_select(|choice| Message::WblockUnit(choice.code))
        .text_size(11)
        .padding([2, 5])
        .width(Fill);

    let units_row = row![
        text(t!("Insert units:"))
            .size(11)
            .style(|theme: &Theme| text::Style {
                color: Some(theme.palette().background.strongest.text.scale_alpha(0.85)),
            }),
        Space::new().width(8),
        unit_picker,
        Space::new().width(Length::Fixed(33.0)),
    ]
    .spacing(4)
    .align_y(iced::Center);

    let destination_group = group(
        t!("Destination").into_owned(),
        column![
            text(t!("File name and path:"))
                .size(11)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().background.strongest.text.scale_alpha(0.85)),
                }),
            path_row,
            Space::new().height(4),
            units_row,
        ]
        .spacing(3),
        Length::Shrink,
    );

    // ── Footer Bar ────────────────────────────────────────────────────────────
    let actions = row![
        Space::new().width(Fill),
        dialog_button(t!("OK"), Message::WblockApply, true),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("Help"), Message::WblockHelp, false),
    ]
    .spacing(8)
    .align_y(iced::Center);

    // ── Error Banner ──────────────────────────────────────────────────────────
    let mut main_column = column![source_group, destination_group, actions,]
        .spacing(8)
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
                    .on_press(Message::WblockDismissError)
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
            .spacing(6)
            .width(sizing.width)
            .padding(10);
    }

    main_column.into()
}
