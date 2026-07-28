//! Multileader Style Manager window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Background, Border, Element, Fill, Theme};

fn btn_s(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
        let pair = match (accent, st) {
            (true, button::Status::Hovered | button::Status::Pressed) => palette.primary.strong,
            (false, button::Status::Hovered | button::Status::Pressed) => {
                palette.background.strong
            }
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
        }
    }
}

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

/// View-model borrowed from the app for one render of the dialog.
pub struct MLeaderStyleView<'a> {
    pub styles: Vec<String>,
    pub selected: &'a str,
    pub style: Option<&'a acadrust::objects::MultiLeaderStyle>,
    pub current: String,
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
    // Handle dropdown option lists + currently-selected record names.
    pub block_opts: Vec<String>,
    pub lt_opts: Vec<String>,
    pub textstyle_opts: Vec<String>,
    pub line_type_name: String,
    pub arrowhead_name: String,
    pub text_style_name: String,
    pub block_content_name: String,
    /// Name of the style being renamed inline (double-clicked), if any.
    pub rename_active: Option<&'a str>,
    /// Edit buffer for the inline rename text input.
    pub rename_buf: &'a str,
    /// Colour field whose expanded palette is currently open.
    pub color_open: Option<&'static str>,
}

fn section<'a>(label: &'static str) -> Element<'a, Message> {
    text(label).size(11).style(primary_style).into()
}

fn num_row<'a>(
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    field: &'static str,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(150),
        text_input(placeholder, value)
            .on_input(move |v| Message::MLeaderStyleEdit { field, value: v })
            .size(11)
            .width(110),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Shared colour selector row. Reuses MLeaderStyleEdit by sending the chosen
/// colour as an ACI string; `open` shows the expanded palette.
fn color_row<'a>(
    label: &'static str,
    value: &'a str,
    field: &'static str,
    open: bool,
) -> Element<'a, Message> {
    let cur = crate::ui::color_select::aci_string_to_color(value);
    let selector = crate::ui::color_select::color_selector(
        cur,
        open,
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: true,
        },
        move |c| Message::MLeaderStyleEdit {
            field,
            value: crate::ui::color_select::color_to_aci_string(c),
        },
        Message::MLeaderColorMore(field),
        Message::OpenColorWindow(crate::app::ColorPickTarget::MLeader(field)),
    );
    row![text(label).size(11).style(muted_style).width(150), selector]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn enum_row<'a>(
    label: &'static str,
    options: Vec<String>,
    selected: String,
    field: &'static str,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(150),
        pick_list(options, Some(selected), move |value| {
            Message::MLeaderStyleSetEnum { field, value }
        })
        .text_size(11)
        .width(190),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn lineweight_row<'a>(selected: acadrust::types::LineWeight) -> Element<'a, Message> {
    let selected = crate::ui::properties::LwItem(selected);
    row![
        text("Line weight:").size(11).style(muted_style).width(150),
        pick_list(
            crate::ui::properties::lw_options(),
            Some(selected),
            |item| Message::MLeaderStyleLineWeightChanged(item.0)
        )
        .text_size(11)
        .width(190),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// The 11 horizontal text-attachment variants (debug names).
const ATTACH_OPTS: [&str; 11] = [
    "TopOfTopLine",
    "MiddleOfTopLine",
    "MiddleOfText",
    "MiddleOfBottomLine",
    "BottomOfBottomLine",
    "BottomLine",
    "BottomOfTopLineUnderlineBottomLine",
    "BottomOfTopLineUnderlineTopLine",
    "BottomOfTopLineUnderlineAll",
    "CenterOfText",
    "CenterOfTextOverline",
];

fn opts(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Dropdown for an Option<Handle> field (linetype / arrowhead / text style /
/// block content). Options are record names; "None" clears the handle.
fn handle_row<'a>(
    label: &'static str,
    options: Vec<String>,
    selected: String,
    field: &'static str,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(150),
        pick_list(options, Some(selected), move |value| {
            Message::MLeaderStyleSetHandle { field, value }
        })
        .text_size(11)
        .width(190),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn chk<'a>(label: &'static str, val: bool, field: &'static str) -> Element<'a, Message> {
    checkbox(val)
        .label(label)
        .on_toggle(move |_| Message::MLeaderStyleToggle(field))
        .size(14)
        .text_size(11)
        .into()
}

pub fn view_window<'a>(v: MLeaderStyleView<'a>) -> Element<'a, Message> {
    // ── Right: Details panel ──────────────────────────────────────────────
    let details: Element<'_, Message> = if let Some(s) = v.style {
        scrollable(
            column![
                row![
                    text("Name:").size(11).style(muted_style).width(150),
                    text(s.name.clone()).size(11),
                ]
                .spacing(8),
                num_row("Description:", "", v.description, "description"),
                // Leader Format
                section("Leader Format"),
                enum_row(
                    "Path type:",
                    opts(&["Invisible", "StraightLineSegments", "Spline"]),
                    format!("{:?}", s.path_type),
                    "path_type"
                ),
                color_row("Line color (ACI):", v.line_color, "line_color", v.color_open == Some("line_color")),
                lineweight_row(s.line_weight),
                handle_row(
                    "Line type:",
                    v.lt_opts.clone(),
                    v.line_type_name.clone(),
                    "line_type_handle"
                ),
                handle_row(
                    "Arrowhead block:",
                    v.block_opts.clone(),
                    v.arrowhead_name.clone(),
                    "arrowhead_handle"
                ),
                num_row(
                    "Arrowhead size:",
                    "0.18",
                    v.arrowhead_size,
                    "arrowhead_size"
                ),
                num_row("Break gap size:", "0.125", v.break_gap, "break_gap"),
                // Leader Structure
                section("Leader Structure"),
                chk("Enable landing", s.enable_landing, "enable_landing"),
                chk("Enable dogleg", s.enable_dogleg, "enable_dogleg"),
                num_row(
                    "Landing distance:",
                    "8.0",
                    v.landing_distance,
                    "landing_distance"
                ),
                num_row("Landing gap:", "0.09", v.landing_gap, "landing_gap"),
                num_row("Max leader points:", "2", v.max_points, "max_points"),
                num_row(
                    "First seg. angle:",
                    "0",
                    v.first_seg_angle,
                    "first_seg_angle"
                ),
                num_row(
                    "Second seg. angle:",
                    "0",
                    v.second_seg_angle,
                    "second_seg_angle"
                ),
                num_row("Scale factor:", "1.0", v.scale_factor, "scale_factor"),
                num_row("Align space:", "4.0", v.align_space, "align_space"),
                enum_row(
                    "Leader draw order:",
                    opts(&["LeaderHeadFirst", "LeaderTailFirst"]),
                    format!("{:?}", s.leader_draw_order),
                    "leader_draw_order"
                ),
                enum_row(
                    "Multileader draw order:",
                    opts(&["ContentFirst", "LeaderFirst"]),
                    format!("{:?}", s.multileader_draw_order),
                    "multileader_draw_order"
                ),
                chk("Annotative", s.is_annotative, "annotative"),
                // Content
                section("Content"),
                enum_row(
                    "Content type:",
                    opts(&["None", "Block", "MText", "Tolerance"]),
                    format!("{:?}", s.content_type),
                    "content_type"
                ),
                num_row("Default text:", "", v.default_text, "default_text"),
                handle_row(
                    "Text style:",
                    v.textstyle_opts.clone(),
                    v.text_style_name.clone(),
                    "text_style_handle"
                ),
                num_row("Text height:", "0.18", v.text_height, "text_height"),
                color_row("Text color (ACI):", v.text_color, "text_color", v.color_open == Some("text_color")),
                enum_row(
                    "Text angle:",
                    opts(&["ParallelToLastLeaderLine", "Horizontal", "Optimized"]),
                    format!("{:?}", s.text_angle_type),
                    "text_angle_type"
                ),
                enum_row(
                    "Text alignment:",
                    opts(&["Left", "Center", "Right"]),
                    format!("{:?}", s.text_alignment),
                    "text_alignment"
                ),
                enum_row(
                    "Left attachment:",
                    opts(&ATTACH_OPTS),
                    format!("{:?}", s.text_left_attachment),
                    "text_left_attachment"
                ),
                enum_row(
                    "Right attachment:",
                    opts(&ATTACH_OPTS),
                    format!("{:?}", s.text_right_attachment),
                    "text_right_attachment"
                ),
                enum_row(
                    "Top attachment:",
                    opts(&ATTACH_OPTS),
                    format!("{:?}", s.text_top_attachment),
                    "text_top_attachment"
                ),
                enum_row(
                    "Bottom attachment:",
                    opts(&ATTACH_OPTS),
                    format!("{:?}", s.text_bottom_attachment),
                    "text_bottom_attachment"
                ),
                enum_row(
                    "Attachment direction:",
                    opts(&["Horizontal", "Vertical"]),
                    format!("{:?}", s.text_attachment_direction),
                    "text_attachment_direction"
                ),
                chk("Text frame", s.text_frame, "text_frame"),
                chk("Text always left", s.text_always_left, "text_always_left"),
                // Block Content
                section("Block Content"),
                handle_row(
                    "Block:",
                    v.block_opts.clone(),
                    v.block_content_name.clone(),
                    "block_content_handle"
                ),
                color_row("Block color (ACI):", v.block_color, "block_color", v.color_open == Some("block_color")),
                enum_row(
                    "Block connection:",
                    opts(&["BlockExtents", "BasePoint"]),
                    format!("{:?}", s.block_content_connection),
                    "block_content_connection"
                ),
                num_row("Block rotation:", "0", v.block_rotation, "block_rotation"),
                num_row("Block scale X:", "1.0", v.block_scale_x, "block_scale_x"),
                num_row("Block scale Y:", "1.0", v.block_scale_y, "block_scale_y"),
                num_row("Block scale Z:", "1.0", v.block_scale_z, "block_scale_z"),
                chk(
                    "Enable block scale",
                    s.enable_block_scale,
                    "enable_block_scale"
                ),
                chk(
                    "Enable block rotation",
                    s.enable_block_rotation,
                    "enable_block_rotation"
                ),
                button(text("Apply").size(11))
                    .on_press(Message::MLeaderStyleApply)
                    .style(btn_s(true))
                    .padding([4, 14]),
            ]
            .spacing(6)
            .padding([12, 12]),
        )
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        container(text("Select a style to view details.").size(11).style(muted_style))
            .padding([12, 12])
            .into()
    };

    let right_panel = container(details).width(Fill).height(Fill);

    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
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
        editor: right_panel.into(),
    })
}
