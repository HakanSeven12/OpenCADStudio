//! Multileader style manager.
//!
//! One window with three faces. The manager lists the styles, shows the
//! selected one, makes it current and deletes it. New… asks for a name and
//! the style to start from; Continue, like Modify…, opens the style's
//! settings in three tabs beside a preview. OK keeps them (a new style also
//! becomes current), Cancel takes them back. Everything done in the window
//! is one undo step when it closes.

use crate::app::Message;
use crate::tr;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{card, card_style};
use document::objects::{
    BlockContentConnectionType, LeaderContentType, MultiLeaderPathType, MultiLeaderStyle,
    TextAttachmentDirectionType, TextAttachmentType,
};
use iced::widget::{
    button, canvas, checkbox, column, container, mouse_area, radio, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{mouse, Background, Border, Color, Element, Fill, Length, Point, Rectangle, Size, Theme};
use std::fmt;

/// The window's natural size, the same on every face so it does not jump.
const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 600.0;
/// Width of a field's label column on the Modify tabs.
const LABEL: f32 = 150.0;
/// Width of the fields on the Modify tabs.
const FIELD: f32 = 210.0;

/// What the window is showing beyond the manager.
#[derive(Debug, Clone, Default)]
pub struct Dialog {
    /// The list shows only the styles multileaders use.
    pub in_use_only: bool,
    /// The New face: a name and the style to start from.
    pub create: Option<CreateDraft>,
    /// The Modify face of the selected style.
    pub modify: Option<Modify>,
    /// A refused new name.
    pub notice: Option<Notice>,
    /// Delete was asked for this style.
    pub delete_ask: Option<String>,
    /// A chooser opened from a list's last entry.
    pub pick: Option<Pick>,
    /// The Load Linetypes window over Select Linetype.
    pub load: Option<LoadDraft>,
}

/// The linetypes of a `.lin` source offered for loading.
#[derive(Debug, Clone)]
pub struct LoadDraft {
    /// The chosen file's name (empty: the program's own definitions).
    pub file: String,
    pub dir: Option<std::path::PathBuf>,
    pub source: String,
    /// Name, description and dash lengths, in file order.
    pub entries: Vec<(String, String, Vec<f64>)>,
    /// The names ticked for loading.
    pub picked: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateDraft {
    pub name: String,
    pub start_with: String,
    pub annotative: bool,
}

/// The Modify face's own state; the style itself is edited in place.
#[derive(Debug, Clone)]
pub struct Modify {
    pub handle: document::Handle,
    /// The style before Modify opened; `None` when Continue created it.
    pub backup: Option<MultiLeaderStyle>,
    /// The style selected in the list before Continue.
    pub previous_selected: String,
    pub tab: u8,
    pub max_points_on: bool,
    pub first_angle_on: bool,
    pub second_angle_on: bool,
    pub scale_to_layout: bool,
}

/// Why a new name was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    EmptyName,
    Exists(String),
    UnsupportedCharacters,
}

/// The choosers behind Other…, User Arrow… and User Block….
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Linetype(String),
    UserArrow(String),
    UserBlock(String),
}

#[derive(Debug, Clone)]
pub enum MlsMsg {
    Select(String),
    ListFilter(bool),
    SetCurrent,
    New,
    Modify,
    Delete,
    DeleteAnswer(bool),
    Help,
    CreateName(String),
    CreateStart(String),
    CreateAnnotative(bool),
    CreateContinue,
    CreateCancel,
    NoticeClose,
    Tab(u8),
    /// A constraint switch: `max_points`, `first_angle` or `second_angle`.
    Constraint(&'static str, bool),
    /// The scale choice: true for "scale to layout".
    ScaleToLayout(bool),
    PickChoice(String),
    PickLoad,
    /// Load Linetypes: choose another `.lin` file.
    LoadFile,
    LoadFileChosen(Option<crate::io::PickedLinetypeFile>),
    LoadToggle(String),
    LoadOk,
    LoadCancel,
    PickOk,
    PickCancel,
    DefaultText,
    TextStyles,
    Ok,
    Cancel,
}

fn msg(m: MlsMsg) -> Message {
    Message::MLeaderStyleDlg(m)
}

/// The text field each numeric value is typed into, focused when OK refuses
/// its value.
pub fn field_id(field: &str) -> iced::widget::Id {
    iced::widget::Id::new(match field {
        "arrowhead_size" => "mls-arrowhead-size",
        "break_gap" => "mls-break-gap",
        "max_points" => "mls-max-points",
        "landing_distance" => "mls-landing-distance",
        "scale_factor" => "mls-scale-factor",
        "text_height" => "mls-text-height",
        "landing_gap" => "mls-landing-gap",
        "block_scale" => "mls-block-scale",
        _ => "mls-field",
    })
}

/// The tab a value's field is on.
pub fn field_tab(field: &str) -> u8 {
    match field {
        "arrowhead_size" | "break_gap" => 0,
        "max_points" | "landing_distance" | "scale_factor" => 1,
        _ => 2,
    }
}

// ── Choice lists ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
struct Choice {
    code: String,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn choice(code: impl Into<String>, label: impl Into<String>) -> Choice {
    Choice {
        code: code.into(),
        label: label.into(),
    }
}

/// The reference's Left / Right attachment list, in its order.
const SIDE_ATTACHMENTS: [TextAttachmentType; 9] = [
    TextAttachmentType::TopOfTopLine,
    TextAttachmentType::MiddleOfTopLine,
    TextAttachmentType::BottomOfTopLineUnderlineTopLine,
    TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
    TextAttachmentType::MiddleOfText,
    TextAttachmentType::MiddleOfBottomLine,
    TextAttachmentType::BottomOfBottomLine,
    TextAttachmentType::BottomLine,
    TextAttachmentType::BottomOfTopLineUnderlineAll,
];

/// Values 5–8 are named one place off in the codec; the labels follow what
/// they draw (see `ATTACH_LABELS` in entities/multileader.rs).
fn attachment_label(value: TextAttachmentType) -> String {
    match value {
        TextAttachmentType::TopOfTopLine => tr!("mleader-style", "att-top-of-top"),
        TextAttachmentType::MiddleOfTopLine => tr!("mleader-style", "att-middle-of-top"),
        TextAttachmentType::BottomOfTopLineUnderlineTopLine => {
            tr!("mleader-style", "att-bottom-of-top")
        }
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine => {
            tr!("mleader-style", "att-underline-top")
        }
        TextAttachmentType::MiddleOfText => tr!("mleader-style", "att-middle-of-text"),
        TextAttachmentType::MiddleOfBottomLine => tr!("mleader-style", "att-middle-of-bottom"),
        TextAttachmentType::BottomOfBottomLine => tr!("mleader-style", "att-bottom-of-bottom"),
        TextAttachmentType::BottomLine => tr!("mleader-style", "att-underline-bottom"),
        TextAttachmentType::BottomOfTopLineUnderlineAll => {
            tr!("mleader-style", "att-underline-all")
        }
        TextAttachmentType::CenterOfText => tr!("mleader-style", "att-center"),
        TextAttachmentType::CenterOfTextOverline => tr!("mleader-style", "att-overline-center"),
    }
}

/// The code a picker value carries for an attachment.
pub fn attachment_code(value: TextAttachmentType) -> String {
    format!("{value:?}")
}

fn side_attachment_choices() -> Vec<Choice> {
    SIDE_ATTACHMENTS
        .iter()
        .map(|&a| choice(attachment_code(a), attachment_label(a)))
        .collect()
}

/// Top: Center or Overline and center; bottom: Center or Underline and
/// center (both store 10 for the lined one).
fn vertical_attachment_choices(top: bool) -> Vec<Choice> {
    vec![
        choice(
            attachment_code(TextAttachmentType::CenterOfText),
            tr!("mleader-style", "att-center"),
        ),
        choice(
            attachment_code(TextAttachmentType::CenterOfTextOverline),
            if top {
                tr!("mleader-style", "att-overline-center")
            } else {
                tr!("mleader-style", "att-underline-center")
            },
        ),
    ]
}

fn vertical_attachment_choice(value: TextAttachmentType, top: bool) -> Choice {
    let list = vertical_attachment_choices(top);
    if value == TextAttachmentType::CenterOfTextOverline {
        list[1].clone()
    } else {
        list[0].clone()
    }
}

/// What the arrowhead picker shows for a standard name; a user block keeps
/// its own name.
pub fn arrow_label(name: &str) -> String {
    match name {
        "Closed filled" => tr!("mleader-style", "arrow-closed-filled"),
        "Closed blank" => tr!("mleader-style", "arrow-closed-blank"),
        "Closed" => tr!("mleader-style", "arrow-closed"),
        "Dot" => tr!("mleader-style", "arrow-dot"),
        "Architectural tick" => tr!("mleader-style", "arrow-arch-tick"),
        "Oblique" => tr!("mleader-style", "arrow-oblique"),
        "Open" => tr!("mleader-style", "arrow-open"),
        "Origin indicator" => tr!("mleader-style", "arrow-origin"),
        "Origin indicator 2" => tr!("mleader-style", "arrow-origin2"),
        "Right angle" => tr!("mleader-style", "arrow-right-angle"),
        "Open 30" => tr!("mleader-style", "arrow-open30"),
        "Dot small" => tr!("mleader-style", "arrow-dot-small"),
        "Dot blank" => tr!("mleader-style", "arrow-dot-blank"),
        "Dot small blank" => tr!("mleader-style", "arrow-dot-small-blank"),
        "Box" => tr!("mleader-style", "arrow-box"),
        "Box filled" => tr!("mleader-style", "arrow-box-filled"),
        "Datum triangle" => tr!("mleader-style", "arrow-datum"),
        "Datum triangle filled" => tr!("mleader-style", "arrow-datum-filled"),
        "Integral" => tr!("mleader-style", "arrow-integral"),
        "None" => tr!("mleader-style", "arrow-none"),
        _ => name.to_string(),
    }
}

/// What the source block picker shows for a predefined block's name.
pub fn block_label(name: &str) -> String {
    match name {
        "Detail Callout" => tr!("mleader-style", "block-detail-callout"),
        "Slot" => tr!("mleader-style", "block-slot"),
        "Circle" => tr!("mleader-style", "block-circle"),
        "Box" => tr!("mleader-style", "block-box"),
        "Hexagon" => tr!("mleader-style", "block-hexagon"),
        "Triangle" => tr!("mleader-style", "block-triangle"),
        _ => name.to_string(),
    }
}

/// The value a list's last entry sends to open its chooser.
pub const OTHER: &str = "\u{1}other";

/// `ByLayer` / `ByBlock` in the user's language; other names as they are.
fn record_label(name: &str) -> String {
    match name {
        "ByLayer" | "ByBlock" => crate::t!(name).into_owned(),
        _ => name.to_string(),
    }
}

// ── View data ───────────────────────────────────────────────────────────────

/// A loaded linetype for the Select Linetype chooser: name, description and
/// its dash pattern (positive dash, negative gap, zero dot).
pub struct LinetypeRow {
    pub name: String,
    pub description: String,
    pub pattern: Vec<f64>,
}

pub struct View<'a> {
    pub dialog: &'a Dialog,
    /// Every style, with whether it is annotative.
    pub styles: Vec<(String, bool)>,
    /// The styles multileaders use.
    pub in_use: Vec<String>,
    pub selected: &'a str,
    pub current: &'a str,
    pub style: Option<&'a MultiLeaderStyle>,
    pub landing_distance: &'a str,
    pub landing_gap: &'a str,
    pub arrowhead_size: &'a str,
    pub text_height: &'a str,
    pub scale_factor: &'a str,
    pub break_gap: &'a str,
    pub max_points: &'a str,
    pub default_text: &'a str,
    pub line_color: &'a str,
    pub text_color: &'a str,
    pub block_color: &'a str,
    pub block_scale: &'a str,
    pub color_open: Option<&'static str>,
    pub line_type_name: String,
    pub linetype_opts: Vec<String>,
    pub linetypes: Vec<LinetypeRow>,
    pub arrowhead_name: String,
    pub arrow_opts: Vec<String>,
    pub text_style_name: String,
    pub textstyle_opts: Vec<String>,
    pub block_name: String,
    pub block_opts: Vec<String>,
    /// The drawing's blocks a user arrow or content block can be.
    pub user_blocks: Vec<String>,
    /// The drawing's usual text height (metric 2.5-based styles 4, imperial
    /// 0.18): the preview's yardstick for the style's sizes.
    pub unit: f64,
}

pub fn view_window<'a>(v: View<'a>, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let face: Element<'a, Message> = if v.dialog.modify.is_some() {
        modify_view(&v)
    } else {
        manager_view(&v)
    };
    let mut layers: Vec<Element<'a, Message>> = vec![
        Space::new()
            .width(Length::Fixed(WIDTH))
            .height(Length::Fixed(HEIGHT))
            .into(),
        container(face)
            .width(sizing.width)
            .height(sizing.height)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                ..Default::default()
            })
            .into(),
    ];
    let overlay: Option<(Element<'a, Message>, Message)> =
        if let Some(notice) = &v.dialog.notice {
            Some((notice_card(notice), msg(MlsMsg::NoticeClose)))
        } else if let Some(name) = &v.dialog.delete_ask {
            Some((delete_card(name), msg(MlsMsg::DeleteAnswer(false))))
        } else if let Some(draft) = &v.dialog.create {
            Some((create_card(draft, &v), msg(MlsMsg::CreateCancel)))
        } else if let Some(load) = &v.dialog.load {
            Some((load_card(load, &v), msg(MlsMsg::LoadCancel)))
        } else {
            v.dialog
                .pick
                .as_ref()
                .map(|pick| (pick_card(pick, &v), msg(MlsMsg::PickCancel)))
        };
    if let Some((panel, dismiss)) = overlay {
        layers.push(
            mouse_area(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(
                            theme.palette().background.strongest.color.scale_alpha(0.55),
                        )),
                        ..Default::default()
                    }),
            )
            .on_press(dismiss)
            .into(),
        );
        layers.push(
            container(panel)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .into(),
        );
    }
    stack(layers).into()
}

/// The window's title for its current face.
pub fn title(dialog: &Dialog, selected: &str) -> String {
    if dialog.create.is_some() {
        tr!("mleader-style", "create-title")
    } else if dialog.modify.is_some() {
        tr!("mleader-style", "modify-title", name = selected.to_string())
    } else {
        tr!("modal", "multileader-style-manager")
    }
}

/// A text field that reads as off while it takes no input.
fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = field_style(theme, status);
    if matches!(status, text_input::Status::Disabled) {
        style.value = style.value.scale_alpha(0.45);
        style.background = Background::Color(theme.palette().background.weak.color);
    }
    style
}

/// The preview's frame: a darker panel than the window.
fn preview_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn help_button<'a>() -> Element<'a, Message> {
    button(text("?").size(12))
        .on_press(msg(MlsMsg::Help))
        .style(button_style(false))
        .padding([5, 11])
        .into()
}

fn side_button<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    crate::ui::style::form::dialog_button_styled_opt(label, message, button::secondary)
        .width(Fill)
        .into()
}

fn label<'a>(value: String) -> Element<'a, Message> {
    text(value)
        .size(11)
        .style(muted_style)
        .width(Length::Fixed(LABEL))
        .into()
}

// ── Manager ─────────────────────────────────────────────────────────────────

fn manager_view<'a>(v: &View<'a>) -> Element<'a, Message> {
    let listed: Vec<&(String, bool)> = v
        .styles
        .iter()
        .filter(|(name, _)| {
            !v.dialog.in_use_only || v.in_use.iter().any(|used| used.eq_ignore_ascii_case(name))
        })
        .collect();
    let rows: Vec<Element<'a, Message>> = listed
        .iter()
        .map(|(name, annotative)| {
            let selected = name == v.selected;
            let mut line = row![].spacing(6).align_y(iced::Center);
            line = line.push(if *annotative {
                crate::ui::icons::tinted(ANNOTATIVE, 12.0, move |theme: &Theme| {
                    if selected {
                        theme.palette().primary.strong.text
                    } else {
                        theme.palette().primary.base.color
                    }
                })
            } else {
                Space::new().width(12).into()
            });
            line = line.push(text(name.clone()).size(11));
            let cell = container(line)
                .padding([4, 8])
                .width(Fill)
                .style(move |theme: &Theme| {
                    let pair = theme.palette().primary.strong;
                    container::Style {
                        background: selected.then_some(Background::Color(pair.color)),
                        text_color: selected.then_some(pair.text),
                        ..Default::default()
                    }
                });
            mouse_area(cell)
                .on_press(msg(MlsMsg::Select(name.clone())))
                .into()
        })
        .collect();
    let list = container(scrollable(column(rows).spacing(1)).height(Fill))
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
        .padding(2)
        .width(Fill)
        .height(Fill);
    let filters = vec![
        choice("all", tr!("mleader-style", "all-styles")),
        choice("used", tr!("mleader-style", "styles-in-use")),
    ];
    let filter_selected = filters[usize::from(v.dialog.in_use_only)].clone();
    let filter = crate::ui::dropdown_list::pick_list(Some(filter_selected), filters, |c| {
        c.to_string()
    })
    .on_select(|c: Choice| msg(MlsMsg::ListFilter(c.code == "used")))
    .text_size(11)
    .width(Fill);
    let left = column![
        text(tr!("mleader-style", "styles")).size(11).style(muted_style),
        list,
        text(tr!("mleader-style", "list")).size(11).style(muted_style),
        filter,
    ]
    .spacing(6)
    .width(Length::Fixed(230.0))
    .height(Fill);

    let in_use = v.in_use.iter().any(|u| u.eq_ignore_ascii_case(v.selected));
    let can_delete = v.style.is_some()
        && !v.selected.eq_ignore_ascii_case("Standard")
        && !v.selected.eq_ignore_ascii_case(v.current)
        && !in_use;
    let actions = column![
        side_button(
            tr!("mleader-style", "set-current"),
            v.style.is_some().then(|| msg(MlsMsg::SetCurrent))
        ),
        side_button(tr!("mleader-style", "new"), Some(msg(MlsMsg::New))),
        side_button(
            tr!("mleader-style", "modify"),
            v.style.is_some().then(|| msg(MlsMsg::Modify))
        ),
        side_button(
            tr!("mleader-style", "delete"),
            can_delete.then(|| msg(MlsMsg::Delete))
        ),
    ]
    .spacing(8)
    .width(Length::Fixed(132.0));

    let preview = column![
        text(tr!(
            "mleader-style",
            "preview-of",
            name = v.selected.to_string()
        ))
        .size(11)
        .style(muted_style),
        preview_well(v, Length::Fill),
    ]
    .spacing(6)
    .width(Fill)
    .height(Fill);

    let body = column![
        container(
            text(tr!(
                "mleader-style",
                "current",
                name = v.current.to_string()
            ))
            .size(12)
        )
        .padding([8, 12])
        .width(Fill)
        .style(card_style),
        row![left, preview, actions].spacing(12).height(Fill),
    ]
    .spacing(10)
    .padding([12, 14])
    .height(Fill);

    let footer = row![
        help_button(),
        Space::new().width(Fill),
        dialog_button(tr!("mleader-style", "close"), Message::CloseModal, true),
    ]
    .spacing(6)
    .align_y(iced::Center);

    column![
        container(body).height(Fill),
        crate::ui::style::form::hdivider(Fill),
        container(footer).padding([8, 14]),
    ]
    .height(Fill)
    .into()
}

// ── Cards over the manager ──────────────────────────────────────────────────

fn panel<'a>(content: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    container(content)
        .padding([16, 18])
        .width(Length::Fixed(width))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                width: 1.0,
                radius: 8.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

fn create_card<'a>(draft: &'a CreateDraft, v: &View<'a>) -> Element<'a, Message> {
    let starts: Vec<String> = v.styles.iter().map(|(name, _)| name.clone()).collect();
    let name = text_input("", &draft.name)
        .id(iced::widget::Id::new("mls-new-name"))
        .on_input(|value| msg(MlsMsg::CreateName(value)))
        .on_submit(msg(MlsMsg::CreateContinue))
        .size(12)
        .padding([5, 8])
        .style(input_style)
        .width(Fill);
    let start = crate::ui::dropdown_list::pick_list(
        Some(draft.start_with.clone()),
        starts,
        |value| value.to_string(),
    )
    .on_select(|value| msg(MlsMsg::CreateStart(value)))
    .text_size(12)
    .width(Fill);
    let annotative = row![
        checkbox(draft.annotative)
            .on_toggle(|on| msg(MlsMsg::CreateAnnotative(on)))
            .size(14),
        crate::ui::icons::tinted(ANNOTATIVE, 12.0, |theme: &Theme| {
            theme.palette().primary.base.color
        }),
        text(tr!("mleader-style", "annotative")).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let body = column![
        text(tr!("mleader-style", "create-title"))
            .size(13)
            .style(crate::ui::window::pdf_dialogs::accent_text),
        column![
            text(tr!("mleader-style", "new-name")).size(11).style(muted_style),
            name
        ]
        .spacing(4),
        column![
            text(tr!("mleader-style", "start-with")).size(11).style(muted_style),
            start
        ]
        .spacing(4),
        annotative,
        Space::new().height(4),
        row![
            help_button(),
            Space::new().width(Fill),
            dialog_button(tr!("mleader-style", "cancel"), msg(MlsMsg::CreateCancel), false),
            dialog_button(
                tr!("mleader-style", "continue"),
                msg(MlsMsg::CreateContinue),
                true
            ),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(10);
    panel(body.into(), 420.0)
}

const WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");
const ANNOTATIVE: &[u8] = include_bytes!("../../../assets/icons/ui/annotative.svg");

fn notice_card<'a>(notice: &'a Notice) -> Element<'a, Message> {
    let (title, detail): (String, Option<String>) = match notice {
        Notice::EmptyName => (tr!("mleader-style", "name-empty"), None),
        Notice::Exists(name) => (
            tr!("mleader-style", "name-exists", name = name.clone()),
            Some(tr!("mleader-style", "name-exists-detail")),
        ),
        Notice::UnsupportedCharacters => (
            tr!("mleader-style", "name-chars"),
            Some(format!(
                "{}\n\\ < > / ? \" : ; * | , = `",
                tr!("mleader-style", "name-chars-detail")
            )),
        ),
    };
    let mut lines = column![text(title).size(13)].spacing(6);
    if let Some(detail) = detail {
        lines = lines.push(text(detail).size(11).style(muted_style));
    }
    let body = column![
        row![crate::ui::icons::semantic(WARN, 20.0), lines]
            .spacing(12)
            .align_y(iced::Alignment::Start),
        row![
            Space::new().width(Fill),
            dialog_button(tr!("mleader-style", "close"), msg(MlsMsg::NoticeClose), true),
        ],
    ]
    .spacing(14);
    panel(body.into(), 420.0)
}

fn delete_card<'a>(name: &'a str) -> Element<'a, Message> {
    let body = column![
        text(tr!("mleader-style", "delete-title"))
            .size(13)
            .style(crate::ui::window::pdf_dialogs::accent_text),
        row![
            crate::ui::icons::semantic(WARN, 20.0),
            text(tr!("mleader-style", "delete-ask", name = name.to_string())).size(12),
        ]
        .spacing(12)
        .align_y(iced::Center),
        row![
            Space::new().width(Fill),
            dialog_button(tr!("mleader-style", "yes"), msg(MlsMsg::DeleteAnswer(true)), false),
            dialog_button(tr!("mleader-style", "no"), msg(MlsMsg::DeleteAnswer(false)), true),
        ]
        .spacing(6),
    ]
    .spacing(14);
    panel(body.into(), 420.0)
}

fn pick_card<'a>(pick: &'a Pick, v: &View<'a>) -> Element<'a, Message> {
    let footer = |load: bool| {
        let mut left = row![].spacing(6).align_y(iced::Center);
        if load {
            left = left.push(dialog_button(
                tr!("mleader-style", "load"),
                msg(MlsMsg::PickLoad),
                false,
            ));
        }
        row![
            left,
            Space::new().width(Fill),
            dialog_button(tr!("mleader-style", "cancel"), msg(MlsMsg::PickCancel), false),
            dialog_button(tr!("mleader-style", "ok"), msg(MlsMsg::PickOk), true),
        ]
        .spacing(6)
        .align_y(iced::Center)
    };
    match pick {
        Pick::Linetype(selected) => {
            let header = row![
                text(tr!("mleader-style", "linetype"))
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(150.0)),
                text(tr!("mleader-style", "appearance"))
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(120.0)),
                text(tr!("mleader-style", "description"))
                    .size(10)
                    .style(muted_style),
            ]
            .spacing(8)
            .padding([0, 8]);
            let rows: Vec<Element<'a, Message>> = v
                .linetypes
                .iter()
                .map(|lt| {
                    let on = lt.name.eq_ignore_ascii_case(selected);
                    let cell = container(
                        row![
                            text(record_label(&lt.name))
                                .size(11)
                                .width(Length::Fixed(150.0)),
                            canvas(PatternSample {
                                pattern: lt.pattern.clone(),
                            })
                            .width(Length::Fixed(120.0))
                            .height(Length::Fixed(14.0)),
                            text(lt.description.clone())
                                .size(11)
                                .wrapping(iced::advanced::text::Wrapping::None),
                        ]
                        .spacing(8)
                        .align_y(iced::Center),
                    )
                    .padding([4, 8])
                    .width(Fill)
                    .style(move |theme: &Theme| {
                        let pair = theme.palette().primary.strong;
                        container::Style {
                            background: on.then_some(Background::Color(pair.color)),
                            text_color: on.then_some(pair.text),
                            ..Default::default()
                        }
                    });
                    mouse_area(cell)
                        .on_press(msg(MlsMsg::PickChoice(lt.name.clone())))
                        .into()
                })
                .collect();
            let list = container(scrollable(column(rows).spacing(1)).height(Length::Fixed(220.0)))
                .padding(2)
                .width(Fill)
                .style(crate::ui::window::pdf_dialogs::well_style);
            let body = column![
                text(tr!("mleader-style", "linetype-title"))
                    .size(13)
                    .style(crate::ui::window::pdf_dialogs::accent_text),
                text(tr!("mleader-style", "loaded-linetypes"))
                    .size(11)
                    .style(muted_style),
                header,
                list,
                footer(true),
            ]
            .spacing(8);
            panel(body.into(), 560.0)
        }
        Pick::UserArrow(selected) | Pick::UserBlock(selected) => {
            let title = if matches!(pick, Pick::UserArrow(_)) {
                tr!("mleader-style", "arrow-title")
            } else {
                tr!("mleader-style", "block-title")
            };
            let chooser: Element<'a, Message> = if v.user_blocks.is_empty() {
                text(tr!("mleader-style", "no-blocks"))
                    .size(11)
                    .style(muted_style)
                    .into()
            } else {
                crate::ui::dropdown_list::pick_list(
                    (!selected.is_empty()).then(|| selected.clone()),
                    v.user_blocks.clone(),
                    |value| value.to_string(),
                )
                .on_select(|value| msg(MlsMsg::PickChoice(value)))
                .text_size(12)
                .width(Fill)
                .into()
            };
            let body = column![
                text(title)
                    .size(13)
                    .style(crate::ui::window::pdf_dialogs::accent_text),
                text(tr!("mleader-style", "from-blocks"))
                    .size(11)
                    .style(muted_style),
                chooser,
                footer(false),
            ]
            .spacing(10);
            panel(body.into(), 420.0)
        }
    }
}

fn load_card<'a>(load: &'a LoadDraft, v: &View<'a>) -> Element<'a, Message> {
    let file = if load.file.is_empty() {
        tr!("mleader-style", "program-linetypes")
    } else {
        load.file.clone()
    };
    let rows: Vec<Element<'a, Message>> = load
        .entries
        .iter()
        .map(|(name, description, pattern)| {
            let loaded = v
                .linetype_opts
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name));
            let on = loaded || load.picked.iter().any(|p| p.eq_ignore_ascii_case(name));
            let caption = if loaded {
                tr!("mleader-style", "already-loaded", name = name.clone())
            } else {
                name.clone()
            };
            let cell = container(
                row![
                    crate::ui::icons::themed_check_cell(on),
                    text(caption).size(11).width(Length::Fixed(170.0)),
                    canvas(PatternSample {
                        pattern: pattern.clone(),
                    })
                    .width(Length::Fixed(110.0))
                    .height(Length::Fixed(14.0)),
                    text(description.clone())
                        .size(11)
                        .wrapping(iced::advanced::text::Wrapping::None),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .padding([3, 8])
            .width(Fill);
            let area = mouse_area(cell);
            if loaded {
                area.into()
            } else {
                area.on_press(msg(MlsMsg::LoadToggle(name.clone()))).into()
            }
        })
        .collect();
    let list = container(scrollable(column(rows).spacing(1)).height(Length::Fixed(260.0)))
        .padding(2)
        .width(Fill)
        .style(preview_style);
    let body = column![
        text(tr!("mleader-style", "load-title"))
            .size(13)
            .style(crate::ui::window::pdf_dialogs::accent_text),
        row![
            text(tr!("mleader-style", "file")).size(11).style(muted_style),
            text(file).size(11).width(Fill),
            dialog_button(tr!("mleader-style", "choose-file"), msg(MlsMsg::LoadFile), false),
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(tr!("mleader-style", "available-linetypes"))
            .size(11)
            .style(muted_style),
        list,
        row![
            Space::new().width(Fill),
            dialog_button(tr!("mleader-style", "cancel"), msg(MlsMsg::LoadCancel), false),
            crate::ui::style::form::dialog_button_styled_opt(
                tr!("mleader-style", "ok"),
                (!load.picked.is_empty()).then(|| msg(MlsMsg::LoadOk)),
                button::primary,
            ),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(8);
    panel(body.into(), 600.0)
}

/// A lineweight drawn as a line that thick (thin for the By… values).
struct WeightSample {
    width: f32,
}

impl canvas::Program<Message> for WeightSample {
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
        let y = bounds.height * 0.5;
        frame.stroke(
            &canvas::Path::line(Point::new(2.0, y), Point::new(bounds.width - 2.0, y)),
            canvas::Stroke::default()
                .with_color(theme.palette().background.base.text)
                .with_width(self.width),
        );
        vec![frame.into_geometry()]
    }
}

fn weight_sample<'a>(weight: document::types::LineWeight) -> Element<'a, Message> {
    // Hundredths of a millimetre at 96 px per inch, at least a pixel.
    let width = match weight {
        document::types::LineWeight::Value(v) => (f32::from(v) / 100.0 * 96.0 / 25.4).max(1.0),
        _ => 1.0,
    }
    .min(8.0);
    canvas(WeightSample { width })
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(12.0))
        .into()
}

fn pattern_sample<'a>(pattern: Vec<f64>) -> Element<'a, Message> {
    canvas(PatternSample { pattern })
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(12.0))
        .into()
}

/// A list whose entries show a line sample beside their names (linetypes,
/// lineweights), opened and closed like the colour lists.
fn sample_selector<'a>(
    caption: String,
    current: (Element<'a, Message>, String),
    entries: Vec<(Element<'a, Message>, String, Message)>,
    field: &'static str,
    open: bool,
    enabled: bool,
) -> Element<'a, Message> {
    use crate::ui::dropdown_list::{DropdownList, DropdownRow};
    let (sample, name) = current;
    let control: Element<'a, Message> = if !enabled {
        container(
            row![sample, text(name).size(11).style(muted_style)]
                .spacing(6)
                .align_y(iced::Center),
        )
        .padding([4, 8])
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        let head = button(
            row![
                sample,
                text(name).size(11),
                Space::new().width(Fill),
                crate::ui::icons::themed_arrow_toggle(open, 9.0),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .on_press(Message::MLeaderColorMore(field))
        .style(button_style(false))
        .padding([3, 8])
        .width(Fill);
        if open {
            let rows: Vec<DropdownRow<'a, Message>> = entries
                .into_iter()
                .map(|(sample, label, message)| {
                    let element: Element<'a, Message> = button(
                        row![sample, text(label.clone()).size(11)]
                            .spacing(6)
                            .align_y(iced::Center),
                    )
                    .on_press(message.clone())
                    .style(crate::ui::color_select::list_row_style)
                    .padding([2, 4])
                    .width(Fill)
                    .into();
                    DropdownRow::item(element, label, message)
                })
                .collect();
            let popup = container(DropdownList::with_options(
                rows,
                Length::Shrink,
                Length::Fill,
                1.0,
                0.0,
            ))
            .style(crate::ui::color_select::popup_panel_style)
            .padding(2);
            crate::ui::drop_down_fit::drop_down_fit(
                iced_aw::DropDown::new(head, popup, true)
                    .alignment(iced_aw::drop_down::Alignment::Bottom)
                    .offset(2.0)
                    .on_dismiss(Message::MLeaderColorMore(field)),
            )
        } else {
            head.into()
        }
    };
    row![label(caption), container(control).width(Length::Fixed(FIELD))]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// A linetype's dash pattern drawn across its cell.
struct PatternSample {
    pattern: Vec<f64>,
}

impl canvas::Program<Message> for PatternSample {
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
        let y = bounds.height * 0.5;
        let total: f64 = self.pattern.iter().map(|v| v.abs()).sum();
        let stroke = canvas::Stroke::default().with_color(ink).with_width(1.2);
        if self.pattern.is_empty() || total <= 1e-9 {
            frame.stroke(
                &canvas::Path::line(Point::new(2.0, y), Point::new(bounds.width - 2.0, y)),
                stroke,
            );
            return vec![frame.into_geometry()];
        }
        // Two repeats of the pattern across the cell.
        let scale = f64::from(bounds.width - 4.0) / (total * 2.0);
        let mut x = 2.0f64;
        'draw: for _ in 0..2 {
            for &element in &self.pattern {
                let length = element.abs() * scale;
                let end = (x + length).min(f64::from(bounds.width) - 2.0);
                if element > 0.0 {
                    frame.stroke(
                        &canvas::Path::line(Point::new(x as f32, y), Point::new(end as f32, y)),
                        stroke,
                    );
                } else if element == 0.0 {
                    frame.fill(&canvas::Path::circle(Point::new(x as f32, y), 1.0), ink);
                }
                x = end;
                if x >= f64::from(bounds.width) - 2.0 {
                    break 'draw;
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

// ── Modify ──────────────────────────────────────────────────────────────────

fn number_field<'a>(
    caption: String,
    value: &'a str,
    field: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .id(field_id(field))
        .size(11)
        .padding([4, 8])
        .width(Length::Fixed(FIELD))
        .style(input_style);
    if enabled {
        input = input.on_input(move |value| Message::MLeaderStyleEdit { field, value });
    }
    row![label(caption), input]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn choice_row<'a>(
    caption: String,
    options: Vec<Choice>,
    selected: Choice,
    enabled: bool,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let mut list = crate::ui::dropdown_list::pick_list(Some(selected), options, |c| c.to_string())
        .text_size(11)
        .width(Length::Fixed(FIELD));
    if enabled {
        list = list.on_select(move |c: Choice| on_select(c.code));
    }
    row![label(caption), list]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn color_row<'a>(
    caption: String,
    value: &'a str,
    field: &'static str,
    open: bool,
    enabled: bool,
) -> Element<'a, Message> {
    let current = crate::ui::color_select::aci_string_to_color(value);
    let control: Element<'a, Message> = if enabled {
        crate::ui::color_select::color_selector(
            current,
            open,
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
                standard_only: true,
                ..Default::default()
            },
            move |color| Message::MLeaderStyleEdit {
                field,
                value: crate::ui::color_select::color_to_aci_string(color),
            },
            Message::MLeaderColorMore(field),
            Message::OpenColorWindow(crate::app::ColorPickTarget::MLeader(field), current),
        )
    } else {
        crate::ui::read_only::field(
            &crate::ui::color_select::color_display_name(current),
            11.0,
            Length::Fill,
        )
    };
    row![label(caption), container(control).width(Length::Fixed(FIELD))]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn toggle<'a>(
    caption: String,
    value: bool,
    on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
) -> Element<'a, Message> {
    let mut check = checkbox(value).size(14);
    // The checkbox draws its own disabled look; the caption follows it.
    let enabled = on_toggle.is_some();
    if let Some(on_toggle) = on_toggle {
        check = check.on_toggle(on_toggle);
    }
    let caption = text(caption).size(11).style(move |theme: &Theme| {
        iced::widget::text::Style {
            color: (!enabled)
                .then(|| crate::ui::style::readable::muted_text(theme).scale_alpha(0.7)),
        }
    });
    row![check, caption]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

fn style_toggle<'a>(caption: String, value: bool, field: &'static str, enabled: bool) -> Element<'a, Message> {
    toggle(
        caption,
        value,
        enabled.then(|| {
            Box::new(move |_| Message::MLeaderStyleToggle(field)) as Box<dyn Fn(bool) -> Message>
        }),
    )
}

/// A radio choice, inert (shown muted) while `enabled` is false.
fn radio_choice<'a>(
    caption: String,
    value: bool,
    selected: bool,
    enabled: bool,
    on: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    let r = radio(caption, value, Some(selected), move |v| {
        if enabled {
            on(v)
        } else {
            Message::Noop
        }
    })
    .size(14)
    .spacing(6)
    .text_size(11);
    if enabled {
        r.into()
    } else {
        r.style(|theme: &Theme, status| {
            let mut style = radio::default(theme, status);
            style.text_color = Some(crate::ui::style::readable::muted_text(theme).scale_alpha(0.7));
            style.dot_color = style.dot_color.scale_alpha(0.45);
            style.border_color = style.border_color.scale_alpha(0.45);
            style
        })
        .into()
    }
}

fn format_tab<'a>(v: &View<'a>, style: &'a MultiLeaderStyle) -> Element<'a, Message> {
    let leader = style.path_type != MultiLeaderPathType::Invisible;
    let types = vec![
        choice("StraightLineSegments", tr!("mleader-style", "straight")),
        choice("Spline", tr!("mleader-style", "spline")),
        choice("Invisible", tr!("mleader-style", "none")),
    ];
    let type_code = format!("{:?}", style.path_type);
    let type_selected = types
        .iter()
        .find(|c| c.code == type_code)
        .cloned()
        .unwrap_or_else(|| types[0].clone());
    let pattern_of = |name: &str| -> Vec<f64> {
        v.linetypes
            .iter()
            .find(|row| row.name.eq_ignore_ascii_case(name))
            .map(|row| row.pattern.clone())
            .unwrap_or_default()
    };
    let mut linetype_entries: Vec<(Element<'a, Message>, String, Message)> = v
        .linetype_opts
        .iter()
        .map(|name| {
            (
                pattern_sample(pattern_of(name)),
                record_label(name),
                Message::MLeaderStyleSetHandle {
                    field: "line_type_handle",
                    value: name.clone(),
                },
            )
        })
        .collect();
    linetype_entries.push((
        Space::new().width(48).into(),
        tr!("mleader-style", "other"),
        Message::MLeaderStyleSetHandle {
            field: "line_type_handle",
            value: OTHER.to_string(),
        },
    ));
    let linetype = sample_selector(
        tr!("mleader-style", "linetype"),
        (
            pattern_sample(pattern_of(&v.line_type_name)),
            record_label(&v.line_type_name),
        ),
        linetype_entries,
        "line_type",
        v.color_open == Some("line_type"),
        leader,
    );
    let lineweight = sample_selector(
        tr!("mleader-style", "lineweight"),
        (
            weight_sample(style.line_weight),
            crate::ui::properties::LwItem(style.line_weight).to_string(),
        ),
        crate::ui::properties::lw_options()
            .into_iter()
            .map(|item| {
                (
                    weight_sample(item.0),
                    item.to_string(),
                    Message::MLeaderStyleLineWeightChanged(item.0),
                )
            })
            .collect(),
        "line_weight",
        v.color_open == Some("line_weight"),
        leader,
    );
    let mut arrows: Vec<Choice> = v
        .arrow_opts
        .iter()
        .map(|name| choice(name.clone(), arrow_label(name)))
        .collect();
    arrows.push(choice(OTHER, tr!("mleader-style", "user-arrow")));
    let arrow = choice(v.arrowhead_name.clone(), arrow_label(&v.arrowhead_name));
    column![
        card(
            tr!("mleader-style", "general"),
            column![
                choice_row(
                    tr!("mleader-style", "type"),
                    types,
                    type_selected,
                    true,
                    |code| Message::MLeaderStyleSetEnum {
                        field: "path_type",
                        value: code
                    }
                ),
                color_row(
                    tr!("mleader-style", "color"),
                    v.line_color,
                    "line_color",
                    v.color_open == Some("line_color"),
                    leader
                ),
                linetype,
                lineweight,
            ]
            .spacing(8)
        ),
        card(
            tr!("mleader-style", "arrowhead"),
            column![
                choice_row(
                    tr!("mleader-style", "symbol"),
                    arrows,
                    arrow,
                    leader,
                    |code| Message::MLeaderStyleSetHandle {
                        field: "arrowhead_handle",
                        value: code
                    }
                ),
                // A None arrowhead keeps its size; only no leader turns it off.
                number_field(
                    tr!("mleader-style", "size"),
                    v.arrowhead_size,
                    "arrowhead_size",
                    leader
                ),
            ]
            .spacing(8)
        ),
        card(
            tr!("mleader-style", "leader-break"),
            number_field(
                tr!("mleader-style", "break-size"),
                v.break_gap,
                "break_gap",
                leader
            ),
        ),
    ]
    .spacing(10)
    .into()
}

const ANGLES: [&str; 6] = ["0", "15", "30", "45", "60", "90"];

fn angle_text(radians: f64) -> String {
    trim_number(radians.to_degrees())
}

fn structure_tab<'a>(
    v: &View<'a>,
    style: &'a MultiLeaderStyle,
    m: &'a Modify,
) -> Element<'a, Message> {
    let leader = style.path_type != MultiLeaderPathType::Invisible;
    let angle_row = |caption: String,
                     on: bool,
                     key: &'static str,
                     value: f64,
                     field: &'static str|
     -> Element<'a, Message> {
        let mut list = crate::ui::dropdown_list::pick_list(
            Some(angle_text(value)),
            ANGLES.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            |value| value.to_string(),
        )
        .text_size(11)
        .width(Length::Fixed(90.0));
        if on {
            list = list.on_select(move |value| Message::MLeaderStyleSetEnum { field, value });
        }
        row![
            container(toggle(
                caption,
                on,
                Some(Box::new(move |checked| msg(MlsMsg::Constraint(key, checked)))
                    as Box<dyn Fn(bool) -> Message>)
            ))
            .width(Length::Fixed(LABEL + 80.0)),
            list,
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };
    let mut max_input = text_input("", v.max_points)
        .id(field_id("max_points"))
        .size(11)
        .padding([4, 8])
        .width(Length::Fixed(90.0))
        .style(input_style);
    if m.max_points_on {
        max_input = max_input.on_input(|value| Message::MLeaderStyleEdit {
            field: "max_points",
            value,
        });
    }
    let constraints = column![
        row![
            container(toggle(
                tr!("mleader-style", "max-points"),
                m.max_points_on,
                Some(Box::new(|checked| msg(MlsMsg::Constraint("max_points", checked)))
                    as Box<dyn Fn(bool) -> Message>)
            ))
            .width(Length::Fixed(LABEL + 80.0)),
            max_input,
        ]
        .spacing(8)
        .align_y(iced::Center),
        angle_row(
            tr!("mleader-style", "first-angle"),
            m.first_angle_on,
            "first_angle",
            style.first_segment_angle,
            "first_seg_angle"
        ),
        angle_row(
            tr!("mleader-style", "second-angle"),
            m.second_angle_on,
            "second_angle",
            style.second_segment_angle,
            "second_seg_angle"
        ),
    ]
    .spacing(8);
    let distance_on = leader && style.enable_landing && style.enable_dogleg;
    let mut distance = text_input("", v.landing_distance)
        .id(field_id("landing_distance"))
        .size(11)
        .padding([4, 8])
        .width(Length::Fixed(110.0))
        .style(input_style);
    if distance_on {
        distance = distance.on_input(|value| Message::MLeaderStyleEdit {
            field: "landing_distance",
            value,
        });
    }
    let landing = column![
        style_toggle(
            tr!("mleader-style", "auto-landing"),
            style.enable_landing,
            "enable_landing",
            leader
        ),
        style_toggle(
            tr!("mleader-style", "landing-distance"),
            style.enable_dogleg,
            "enable_dogleg",
            leader && style.enable_landing
        ),
        row![Space::new().width(22), distance],
    ]
    .spacing(8);
    let annotative = style.is_annotative;
    let shown_scale: &str = if m.scale_to_layout { "0" } else { v.scale_factor };
    let mut scale_input = text_input("", shown_scale)
        .id(field_id("scale_factor"))
        .size(11)
        .padding([4, 8])
        .width(Length::Fixed(110.0))
        .style(input_style);
    if !annotative && !m.scale_to_layout {
        scale_input = scale_input.on_input(|value| Message::MLeaderStyleEdit {
            field: "scale_factor",
            value,
        });
    }
    let scale = column![
        row![
            style_toggle(
                tr!("mleader-style", "annotative"),
                annotative,
                "annotative",
                true
            ),
            crate::ui::icons::tinted(ANNOTATIVE, 12.0, |theme: &Theme| {
                theme.palette().primary.base.color
            }),
        ]
        .spacing(6)
        .align_y(iced::Center),
        row![
            Space::new().width(22),
            radio_choice(
                tr!("mleader-style", "scale-layout"),
                true,
                m.scale_to_layout,
                !annotative,
                |v| msg(MlsMsg::ScaleToLayout(v))
            ),
        ],
        row![
            Space::new().width(22),
            container(radio_choice(
                tr!("mleader-style", "specify-scale"),
                false,
                m.scale_to_layout,
                !annotative,
                |v| msg(MlsMsg::ScaleToLayout(v))
            ))
            .width(Length::Fixed(LABEL + 58.0)),
            scale_input,
        ]
        .align_y(iced::Center),
    ]
    .spacing(8);
    column![
        card(tr!("mleader-style", "constraints"), constraints),
        card(tr!("mleader-style", "landing-settings"), landing),
        card(tr!("mleader-style", "scale"), scale),
    ]
    .spacing(10)
    .into()
}

fn content_tab<'a>(v: &View<'a>, style: &'a MultiLeaderStyle) -> Element<'a, Message> {
    let types = vec![
        choice("MText", tr!("mleader-style", "mtext")),
        choice("Block", tr!("mleader-style", "block")),
        choice("None", tr!("mleader-style", "none")),
    ];
    let type_code = match style.content_type {
        LeaderContentType::Block => "Block",
        LeaderContentType::None => "None",
        _ => "MText",
    };
    let type_selected = types
        .iter()
        .find(|c| c.code == type_code)
        .cloned()
        .unwrap_or_else(|| types[0].clone());
    let type_row = choice_row(
        tr!("mleader-style", "multileader-type"),
        types,
        type_selected,
        true,
        |code| Message::MLeaderStyleSetEnum {
            field: "content_type",
            value: code,
        },
    );
    let mut body = column![type_row].spacing(10);
    match style.content_type {
        LeaderContentType::Block => body = body.push(block_options(v, style)),
        LeaderContentType::None => {}
        _ => {
            body = body.push(text_options(v, style));
            body = body.push(leader_connection(v, style));
        }
    }
    body.into()
}

fn dots_button<'a>(message: Message) -> Element<'a, Message> {
    button(text("…").size(11))
        .on_press(message)
        .style(button_style(false))
        .padding([3, 10])
        .into()
}

fn text_options<'a>(v: &View<'a>, style: &'a MultiLeaderStyle) -> Element<'a, Message> {
    let shown = if v.default_text.is_empty() {
        tr!("mleader-style", "default-text-sample")
    } else {
        v.default_text.replace("\\P", " ").replace('\n', " ")
    };
    let default_text = row![
        label(tr!("mleader-style", "default-text")),
        crate::ui::read_only::field(&shown, 11.0, Length::Fixed(FIELD - 44.0)),
        dots_button(msg(MlsMsg::DefaultText)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let text_style = row![
        label(tr!("mleader-style", "text-style")),
        crate::ui::dropdown_list::pick_list(
            Some(v.text_style_name.clone()),
            v.textstyle_opts.clone(),
            |value| value.to_string(),
        )
        .on_select(|value| Message::MLeaderStyleSetHandle {
            field: "text_style_handle",
            value,
        })
        .text_size(11)
        .width(Length::Fixed(FIELD - 44.0)),
        dots_button(msg(MlsMsg::TextStyles)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let angles = vec![
        choice("Optimized", tr!("mleader-style", "angle-right-reading")),
        choice("ParallelToLastLeaderLine", tr!("mleader-style", "angle-as-inserted")),
        choice("Horizontal", tr!("mleader-style", "angle-horizontal")),
    ];
    let angle_code = format!("{:?}", style.text_angle_type);
    let angle_selected = angles
        .iter()
        .find(|c| c.code == angle_code)
        .cloned()
        .unwrap_or_else(|| angles[2].clone());
    card(
        tr!("mleader-style", "text-options"),
        column![
            default_text,
            text_style,
            choice_row(
                tr!("mleader-style", "text-angle"),
                angles,
                angle_selected,
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "text_angle_type",
                    value: code
                }
            ),
            color_row(
                tr!("mleader-style", "text-color"),
                v.text_color,
                "text_color",
                v.color_open == Some("text_color"),
                true
            ),
            number_field(
                tr!("mleader-style", "text-height"),
                v.text_height,
                "text_height",
                true
            ),
            row![
                container(style_toggle(
                    tr!("mleader-style", "always-left"),
                    style.text_always_left,
                    "text_always_left",
                    true
                ))
                .width(Length::Fixed(LABEL + 60.0)),
                style_toggle(
                    tr!("mleader-style", "frame-text"),
                    style.text_frame,
                    "text_frame",
                    true
                ),
            ]
            .align_y(iced::Center),
        ]
        .spacing(8),
    )
}

fn leader_connection<'a>(v: &View<'a>, style: &'a MultiLeaderStyle) -> Element<'a, Message> {
    let vertical = style.text_attachment_direction == TextAttachmentDirectionType::Vertical;
    let direction = |value: bool, caption: String| {
        radio(caption, value, Some(vertical), |value| {
            Message::MLeaderStyleSetEnum {
                field: "text_attachment_direction",
                value: if value { "Vertical" } else { "Horizontal" }.to_string(),
            }
        })
        .size(14)
        .spacing(6)
        .text_size(11)
    };
    let mut body = column![
        direction(false, tr!("mleader-style", "horizontal")),
        direction(true, tr!("mleader-style", "vertical")),
    ]
    .spacing(8);
    let indented = |e: Element<'a, Message>| row![Space::new().width(22), e];
    if vertical {
        body = body
            .push(indented(choice_row(
                tr!("mleader-style", "top-attachment"),
                vertical_attachment_choices(true),
                vertical_attachment_choice(style.text_top_attachment, true),
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "text_top_attachment",
                    value: code,
                },
            )))
            .push(indented(choice_row(
                tr!("mleader-style", "bottom-attachment"),
                vertical_attachment_choices(false),
                vertical_attachment_choice(style.text_bottom_attachment, false),
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "text_bottom_attachment",
                    value: code,
                },
            )))
            .push(indented(number_field(
                tr!("mleader-style", "landing-gap"),
                v.landing_gap,
                "landing_gap",
                true,
            )));
    } else {
        let side = |value: TextAttachmentType| choice(attachment_code(value), attachment_label(value));
        // Framed text always meets the leader: Extend is off with it.
        body = body
            .push(indented(choice_row(
                tr!("mleader-style", "left-attachment"),
                side_attachment_choices(),
                side(style.text_left_attachment),
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "text_left_attachment",
                    value: code,
                },
            )))
            .push(indented(choice_row(
                tr!("mleader-style", "right-attachment"),
                side_attachment_choices(),
                side(style.text_right_attachment),
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "text_right_attachment",
                    value: code,
                },
            )))
            .push(indented(number_field(
                tr!("mleader-style", "landing-gap"),
                v.landing_gap,
                "landing_gap",
                true,
            )))
            .push(indented(style_toggle(
                tr!("mleader-style", "extend-leader"),
                style.unknown_flag_298,
                "extend_leader",
                !style.text_frame,
            )));
    }
    card(tr!("mleader-style", "leader-connection"), body)
}

fn block_options<'a>(v: &View<'a>, style: &'a MultiLeaderStyle) -> Element<'a, Message> {
    let mut blocks: Vec<Choice> = v
        .block_opts
        .iter()
        .map(|name| choice(name.clone(), block_label(name)))
        .collect();
    blocks.push(choice(OTHER, tr!("mleader-style", "user-block")));
    let block = choice(v.block_name.clone(), block_label(&v.block_name));
    let attachments = vec![
        choice("BlockExtents", tr!("mleader-style", "center-extents")),
        choice("BasePoint", tr!("mleader-style", "insertion-point")),
    ];
    let attachment = if style.block_content_connection == BlockContentConnectionType::BasePoint {
        attachments[1].clone()
    } else {
        attachments[0].clone()
    };
    card(
        tr!("mleader-style", "block-options"),
        column![
            choice_row(
                tr!("mleader-style", "source-block"),
                blocks,
                block,
                true,
                |code| Message::MLeaderStyleSetHandle {
                    field: "block_content_handle",
                    value: code
                }
            ),
            choice_row(
                tr!("mleader-style", "attachment"),
                attachments,
                attachment,
                true,
                |code| Message::MLeaderStyleSetEnum {
                    field: "block_content_connection",
                    value: code
                }
            ),
            color_row(
                tr!("mleader-style", "color"),
                v.block_color,
                "block_color",
                v.color_open == Some("block_color"),
                true
            ),
            number_field(
                tr!("mleader-style", "scale"),
                v.block_scale,
                "block_scale",
                true
            ),
        ]
        .spacing(8),
    )
}

fn modify_view<'a>(v: &View<'a>) -> Element<'a, Message> {
    let (Some(style), Some(m)) = (v.style, v.dialog.modify.as_ref()) else {
        return Space::new().into();
    };
    let tabs = [
        tr!("mleader-style", "tab-format"),
        tr!("mleader-style", "tab-structure"),
        tr!("mleader-style", "tab-content"),
    ];
    let tab_bar = container(
        iced::widget::Row::with_children(
            tabs.into_iter()
                .enumerate()
                .map(|(index, caption)| {
                    let on = m.tab == index as u8;
                    button(text(caption).size(11).width(Fill).align_x(iced::Center))
                        .on_press(msg(MlsMsg::Tab(index as u8)))
                        .style(button_style(on))
                        .padding([5, 6])
                        .width(Fill)
                        .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::well_style);
    let page: Element<'a, Message> = match m.tab {
        0 => format_tab(v, style),
        1 => structure_tab(v, style, m),
        _ => content_tab(v, style),
    };
    let left = column![
        tab_bar,
        scrollable(container(page).padding([2, 2]).width(Fill))
            .height(Fill)
            .width(Fill),
    ]
    .spacing(10)
    .width(Length::Fixed(500.0))
    .height(Fill);
    let right = column![
        text(tr!(
            "mleader-style",
            "preview-of",
            name = v.selected.to_string()
        ))
        .size(11)
        .style(muted_style),
        preview_well(v, Length::Fixed(240.0)),
    ]
    .spacing(6)
    .width(Fill);
    let body = row![left, right].spacing(14).padding([12, 14]).height(Fill);
    let footer = row![
        help_button(),
        Space::new().width(Fill),
        dialog_button(tr!("mleader-style", "cancel"), msg(MlsMsg::Cancel), false),
        dialog_button(tr!("mleader-style", "ok"), msg(MlsMsg::Ok), true),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![
        container(body).height(Fill),
        crate::ui::style::form::hdivider(Fill),
        container(footer).padding([8, 14]),
    ]
    .height(Fill)
    .into()
}

// ── Preview ─────────────────────────────────────────────────────────────────

fn preview_well<'a>(v: &View<'a>, height: Length) -> Element<'a, Message> {
    let content: Element<'a, Message> = match v.style {
        Some(style) => canvas(LeaderPreview::new(v, style))
            .width(Fill)
            .height(Fill)
            .into(),
        None => Space::new().into(),
    };
    container(content)
        .width(Fill)
        .height(height)
        .style(preview_style)
        .into()
}

fn aci_color(value: &str, fallback: Color) -> Color {
    crate::ui::color_select::aci_string_to_color(value)
        .rgb()
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(fallback)
}

/// A number as the dialog shows it: no trailing zeros.
pub fn trim_number(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

struct LeaderPreview {
    leader: bool,
    spline: bool,
    content: LeaderContentType,
    line_color: String,
    line_weight: i16,
    dashed: bool,
    arrow: String,
    landing: bool,
    dogleg: bool,
    text: String,
    text_color: String,
    frame: bool,
    vertical: bool,
    block: String,
    block_color: String,
    /// Pixels per drawing unit: the drawing's usual text height draws 15 px
    /// high, times the style's overall scale.
    px_per_unit: f32,
    arrow_size: f32,
    landing_distance: f32,
    landing_gap: f32,
    text_height: f32,
    block_scale: f32,
}

/// A typed value, or the style's while the field holds no number.
fn typed(value: &str, stored: f64) -> f32 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(stored) as f32
}

impl LeaderPreview {
    fn new(v: &View<'_>, style: &MultiLeaderStyle) -> Self {
        let lt = v.line_type_name.to_ascii_lowercase();
        Self {
            leader: style.path_type != MultiLeaderPathType::Invisible,
            spline: style.path_type == MultiLeaderPathType::Spline,
            content: style.content_type,
            line_color: v.line_color.to_string(),
            line_weight: style.line_weight.value(),
            dashed: !(lt.is_empty() || lt == "bylayer" || lt == "byblock" || lt.contains("continuous")),
            arrow: v.arrowhead_name.to_ascii_uppercase(),
            landing: style.enable_landing,
            dogleg: style.enable_dogleg,
            text: if v.default_text.trim().is_empty() {
                tr!("mleader-style", "default-text-sample")
            } else {
                v.default_text.replace("\\P", " ").replace('\n', " ")
            },
            text_color: v.text_color.to_string(),
            frame: style.text_frame,
            vertical: style.text_attachment_direction == TextAttachmentDirectionType::Vertical,
            block: v.block_name.clone(),
            block_color: v.block_color.to_string(),
            px_per_unit: {
                let scale = if style.is_annotative || v.dialog.modify.as_ref().is_some_and(|m| m.scale_to_layout) {
                    1.0
                } else {
                    typed(v.scale_factor, style.scale_factor)
                };
                let scale = if scale > 0.0 { scale } else { 1.0 };
                15.0 / v.unit.max(1e-9) as f32 * scale
            },
            arrow_size: typed(v.arrowhead_size, style.arrowhead_size),
            landing_distance: typed(v.landing_distance, style.landing_distance),
            landing_gap: typed(v.landing_gap, style.landing_gap),
            text_height: typed(v.text_height, style.text_height),
            block_scale: typed(v.block_scale, style.block_content_scale_x),
        }
    }
}

impl canvas::Program<Message> for LeaderPreview {
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
        let width = (f32::from(self.line_weight.max(0)) / 35.0).clamp(1.0, 4.0);
        let head = Point::new(18.0, bounds.height - 22.0);
        let elbow = Point::new(bounds.width * 0.42, bounds.height * 0.32);
        let px = |value: f32| value * self.px_per_unit;
        let tail = if self.landing && self.dogleg {
            Point::new(
                elbow.x + px(self.landing_distance).clamp(0.0, bounds.width * 0.5),
                elbow.y,
            )
        } else {
            elbow
        };
        let stroke = canvas::Stroke {
            style: canvas::Style::Solid(line_color),
            width,
            line_dash: if self.dashed {
                canvas::LineDash {
                    segments: &[7.0, 4.0],
                    offset: 0,
                }
            } else {
                canvas::LineDash::default()
            },
            ..Default::default()
        };
        if self.leader {
            let path = canvas::Path::new(|b| {
                b.move_to(head);
                if self.spline {
                    let control = Point::new(head.x + (elbow.x - head.x) * 0.15, elbow.y);
                    b.quadratic_curve_to(control, elbow);
                } else {
                    b.line_to(elbow);
                }
                b.line_to(tail);
            });
            frame.stroke(&path, stroke);
            // The arrowhead, pointing back along the first segment.
            let dir = {
                let dx = elbow.x - head.x;
                let dy = elbow.y - head.y;
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                (dx / len, dy / len)
            };
            let size = px(self.arrow_size).clamp(1.0, bounds.height * 0.5);
            let along = |d: f32, side: f32| {
                Point::new(
                    head.x + dir.0 * d - dir.1 * side,
                    head.y + dir.1 * d + dir.0 * side,
                )
            };
            let thin = canvas::Stroke::default()
                .with_color(line_color)
                .with_width(width);
            let code = self.arrow.as_str();
            if code.contains("NONE") {
            } else if code.contains("DOT") {
                let filled = !code.contains("BLANK");
                let r = if code.contains("SMALL") { 2.0 } else { 4.0 };
                if filled {
                    frame.fill(&canvas::Path::circle(head, r), line_color);
                } else {
                    frame.stroke(&canvas::Path::circle(head, r), thin);
                }
            } else if code.contains("TICK") || code.contains("OBLIQUE") {
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(head.x - 5.0, head.y + 5.0),
                        Point::new(head.x + 5.0, head.y - 5.0),
                    ),
                    thin,
                );
            } else if code.contains("BOX") {
                let square = canvas::Path::rectangle(
                    Point::new(head.x - 4.0, head.y - 4.0),
                    Size::new(8.0, 8.0),
                );
                if code.contains("FILLED") {
                    frame.fill(&square, line_color);
                } else {
                    frame.stroke(&square, thin);
                }
            } else if code.contains("ORIGIN") || code.contains("INTEGRAL") {
                frame.stroke(&canvas::Path::circle(head, 4.0), thin);
            } else {
                let half = if code.contains("30") { 0.27 } else { 0.32 };
                let tri = canvas::Path::new(|b| {
                    b.move_to(head);
                    b.line_to(along(size, size * half));
                    b.line_to(along(size, -size * half));
                    b.close();
                });
                if code.contains("OPEN") || code.contains("RIGHT") {
                    let open = canvas::Path::new(|b| {
                        b.move_to(along(size, size * half));
                        b.line_to(head);
                        b.line_to(along(size, -size * half));
                    });
                    frame.stroke(&open, thin);
                } else if code.contains("BLANK") || code == "CLOSED" || code.contains("DATUM TRIANGLE") && !code.contains("FILLED") {
                    frame.stroke(&tri, thin);
                } else {
                    frame.fill(&tri, line_color);
                }
            }
        }
        let origin = Point::new(tail.x + px(self.landing_gap).clamp(0.0, bounds.width * 0.5), tail.y);
        match self.content {
            LeaderContentType::Block => {
                let color = aci_color(&self.block_color, ink);
                let s = canvas::Stroke::default().with_color(color).with_width(1.4);
                // The predefined blocks are a unit across their centre.
                let r = px(self.block_scale).clamp(1.0, bounds.height * 0.45);
                let c = Point::new(origin.x + r, origin.y);
                let shape = match self.block.as_str() {
                    "Box" => canvas::Path::rectangle(
                        Point::new(c.x - r, c.y - r),
                        Size::new(2.0 * r, 2.0 * r),
                    ),
                    "Hexagon" | "Triangle" => {
                        let (n, start) = if self.block == "Hexagon" { (6, 0.0) } else { (3, 90.0f32) };
                        canvas::Path::new(|b| {
                            for i in 0..n {
                                let a = (start + 360.0 * i as f32 / n as f32).to_radians();
                                let p = Point::new(c.x + r * a.cos(), c.y - r * a.sin());
                                if i == 0 {
                                    b.move_to(p);
                                } else {
                                    b.line_to(p);
                                }
                            }
                            b.close();
                        })
                    }
                    "Slot" => canvas::Path::rounded_rectangle(
                        Point::new(c.x - r, c.y - r * 0.5),
                        Size::new(2.0 * r, r),
                        (r * 0.5).into(),
                    ),
                    _ => canvas::Path::circle(c, r),
                };
                frame.stroke(&shape, s);
                if self.block == "Detail Callout" {
                    frame.stroke(
                        &canvas::Path::line(Point::new(c.x - r, c.y), Point::new(c.x + r, c.y)),
                        s,
                    );
                }
            }
            LeaderContentType::None => {}
            _ => {
                let size = px(self.text_height).clamp(1.0, bounds.height * 0.6);
                let color = aci_color(&self.text_color, ink);
                let width = self.text.chars().count() as f32 * size * 0.55;
                let (x, y) = if self.vertical {
                    (origin.x - width * 0.5, origin.y - size)
                } else {
                    (origin.x, origin.y)
                };
                if self.frame {
                    frame.stroke(
                        &canvas::Path::rectangle(
                            Point::new(x - 4.0, y - size * 0.75),
                            Size::new(width + 8.0, size * 1.5),
                        ),
                        canvas::Stroke::default().with_color(color).with_width(1.0),
                    );
                }
                frame.fill_text(canvas::Text {
                    content: self.text.clone(),
                    position: Point::new(x, y),
                    color,
                    size: iced::Pixels(size),
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}
