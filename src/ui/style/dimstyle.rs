//! Dimension style manager.
//!
//! One window with four faces. The manager lists the styles (the current
//! style's overrides as a child), previews and describes the selected one,
//! makes it current, and opens the others: New… asks for a name and what to
//! start from, then — like Modify… and Override… — shows the style's
//! settings in seven tabs beside a preview, kept by OK and dropped by
//! Cancel; Compare… lists every variable of one style, or the differences of
//! two. Everything done in the window is one undo step when it closes.

use crate::app::Message;
use crate::scene::convert::tessellate::{arrow_from_block_name, ArrowKind};
use crate::tr;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt};
use crate::ui::style::mleaderstyle::{
    input_style, linetype_card, load_linetypes_card, panel, pattern_sample, preview_style,
    record_label, reload_card, sample_list, weight_sample, LinetypeRow, LoadDraft, Reload,
    ReloadAsk,
};
use crate::ui::window::pdf_dialogs::{accent_text, card, card_style, well_style};
use document::tables::DimStyle;
use document::types::{Color as AcadColor, LineWeight};
use iced::widget::{
    button, canvas, checkbox, column, container, mouse_area, radio, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{mouse, Background, Border, Color, Element, Fill, Length, Point, Rectangle, Theme};
use std::fmt;

/// The window's natural size, the same on every face so it does not jump.
const WIDTH: f32 = 1060.0;
const HEIGHT: f32 = 660.0;
/// Width of the settings face's left column.
const COLUMN: f32 = 520.0;
/// Width of a field's label column on the settings tabs.
const LABEL: f32 = 160.0;
/// Width of the fields on the settings tabs.
const FIELD: f32 = 200.0;

/// The name the overrides entry goes by in the list's actions.
pub const OVERRIDES: &str = "\u{1}overrides";

/// A family member's parent style and short label (`ISO-25$0` →
/// `ISO-25`, Linear).
pub fn family_member(name: &str) -> Option<(&str, String)> {
    let at = name.rfind('$')?;
    let (parent, suffix) = name.split_at(at);
    let label = match suffix {
        "$0" => tr!("dim-style", "short-linear"),
        "$2" => tr!("dim-style", "short-angular"),
        "$3" => tr!("dim-style", "short-diameter"),
        "$4" => tr!("dim-style", "short-radius"),
        "$6" => tr!("dim-style", "short-ordinate"),
        "$7" => tr!("dim-style", "short-leaders"),
        _ => return None,
    };
    Some((parent, label))
}

/// How the window names a style: a family member by its parent and kind.
pub fn display_name(name: &str) -> String {
    match family_member(name) {
        Some((parent, label)) => format!("{parent}: {label}"),
        None => name.to_string(),
    }
}

/// The value a list's last entry sends to open its chooser.
pub const OTHER: &str = "\u{1}other";

// ── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Dialog {
    /// The list shows only the styles dimensions use.
    pub in_use_only: bool,
    /// "Don't list styles in Xrefs" is cleared.
    pub list_xrefs: bool,
    /// The current style's overrides are the selected list entry.
    pub overrides_selected: bool,
    /// The list entry whose actions are shown (right click); the
    /// overrides entry's are shown with `OVERRIDES`.
    pub context: Option<String>,
    /// Set Current on another style while the current style has
    /// overrides: the style waiting for the answer.
    pub discard_ask: Option<String>,
    /// A list entry being renamed: its name and the typed one.
    pub rename: Option<(String, String)>,
    pub create: Option<CreateDraft>,
    pub edit: Option<Edit>,
    pub compare: Option<CompareState>,
    pub notice: Option<Notice>,
    pub delete_ask: Option<String>,
    pub pick: Option<Pick>,
    pub load: Option<LoadDraft>,
    /// Load Linetypes asks about the ticked linetypes the drawing has.
    pub reload_ask: Option<ReloadAsk>,
    /// The list (colour, linetype, lineweight, fill) that is open.
    pub open_list: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CreateDraft {
    pub name: String,
    pub start_with: String,
    pub annotative: bool,
    /// 0 all dimensions, then Linear, Angular, Radius, Diameter, Ordinate,
    /// Leaders and Tolerances.
    pub use_for: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Modify,
    /// A new style; the list's selection before New…
    New(String),
    Override,
}

/// The settings face: the style is edited as a draft, kept by OK.
#[derive(Debug, Clone)]
pub struct Edit {
    pub mode: Mode,
    /// The style's name (a new style's chosen name).
    pub name: String,
    pub draft: DimStyle,
    pub tab: u8,
    /// Typed numbers and texts, by field.
    pub texts: Vec<(&'static str, String)>,
    /// DIMTALN, kept in the style's extended data.
    pub dimtaln: i16,
}

impl Edit {
    pub fn text(&self, field: &str) -> &str {
        self.texts
            .iter()
            .find(|(key, _)| *key == field)
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    }

    pub fn set_text(&mut self, field: &'static str, value: String) {
        match self.texts.iter_mut().find(|(key, _)| *key == field) {
            Some((_, slot)) => *slot = value,
            None => self.texts.push((field, value)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompareState {
    pub first: String,
    /// `None` is "<none>": every variable of the first style.
    pub second: Option<String>,
}

/// Why a name was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    EmptyName,
    Exists(String),
    UnsupportedCharacters,
    /// The style cannot be deleted (current or in use).
    Delete(String),
}

/// The choosers behind Other… and User Arrow…
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The linetype field and the highlighted linetype.
    Linetype(&'static str, String),
    /// The arrowhead (0 first, 1 second, 2 leader) and the highlighted block.
    UserArrow(u8, String),
}

#[derive(Debug, Clone)]
pub enum DsMsg {
    Select(String),
    SelectOverrides,
    /// The overrides entry's actions: Save to current style.
    SaveOverrides,
    /// Set Current on another style discards the overrides: the answer.
    DiscardAnswer(bool),
    ListFilter(bool),
    HideXrefs(bool),
    Context(Option<String>),
    RenameStart(String),
    RenameInput(String),
    RenameCommit,
    RenameCancel,
    SetCurrent,
    New,
    Modify,
    Override,
    Compare,
    Delete(String),
    DeleteAnswer(bool),
    Help,
    CreateName(String),
    CreateStart(String),
    CreateAnnotative(bool),
    CreateUse(u8),
    CreateContinue,
    CreateCancel,
    NoticeClose,
    Tab(u8),
    /// A typed number or text.
    Text(&'static str, String),
    /// A list or radio choice, by its code.
    Set(&'static str, String),
    /// A switch.
    Toggle(&'static str, bool),
    Color(&'static str, AcadColor),
    ColorMore(&'static str),
    OpenList(&'static str),
    /// Arrowhead 0 first, 1 second, 2 leader; [`OTHER`] opens User Arrow.
    Arrow(u8, String),
    /// A linetype field; [`OTHER`] opens Select Linetype.
    Linetype(&'static str, String),
    Lineweight(&'static str, LineWeight),
    PickChoice(String),
    PickOk,
    PickCancel,
    PickLoad,
    LoadFile,
    LoadFileChosen(Option<crate::io::PickedLinetypeFile>),
    LoadToggle(String),
    LoadOk,
    LoadCancel,
    /// The answer to the reload question of Load Linetypes.
    ReloadAnswer(Reload),
    TextStyles,
    Ok,
    Cancel,
    CompareFirst(String),
    CompareSecond(String),
    CompareCopy,
    CompareClose,
}

fn msg(m: DsMsg) -> Message {
    Message::DimStyleDlg(m)
}

/// The text field each value is typed into, focused when OK refuses it.
pub fn field_id(field: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("ds-{field}"))
}

/// The tab a typed value's field is on.
pub fn field_tab(field: &str) -> u8 {
    match field {
        "dimdle" | "dimdli" | "dimexe" | "dimexo" | "dimfxl" => 0,
        "dimasz" | "dimcen" | "dimbreak" | "dimjogang" | "dimjag" => 1,
        "dimtxt" | "dimtfac" | "dimgap" => 2,
        "dimscale" => 3,
        "dimrnd" | "prefix" | "suffix" | "dimlfac" | "dimmzf" | "dimmzs" => 4,
        "dimaltf" | "dimaltrnd" | "alt_prefix" | "alt_suffix" | "dimaltmzf" | "dimaltmzs" => 5,
        _ => 6,
    }
}

// ── Choices ─────────────────────────────────────────────────────────────────

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

fn choices(list: &[(&str, String)]) -> Vec<Choice> {
    list.iter().map(|(code, label)| choice(*code, label.clone())).collect()
}

fn pick(list: &[Choice], code: &str) -> Choice {
    list.iter()
        .find(|c| c.code == code)
        .cloned()
        .unwrap_or_else(|| choice(code, code))
}

/// The linear unit formats: primary (`alternate` false, DIMLUNIT 1–6) or
/// alternate (DIMALTU 1–8, with the stacked fractions).
fn unit_choices(alternate: bool) -> Vec<Choice> {
    let mut list = vec![
        ("1", tr!("dim-style", "unit-scientific")),
        ("2", tr!("dim-style", "unit-decimal")),
        ("3", tr!("dim-style", "unit-engineering")),
    ];
    if alternate {
        list.extend([
            ("4", tr!("dim-style", "unit-architectural-stacked")),
            ("5", tr!("dim-style", "unit-fractional-stacked")),
            ("6", tr!("dim-style", "unit-architectural")),
            ("7", tr!("dim-style", "unit-fractional")),
            ("8", tr!("dim-style", "unit-windows")),
        ]);
    } else {
        list.extend([
            ("4", tr!("dim-style", "unit-architectural")),
            ("5", tr!("dim-style", "unit-fractional")),
            ("6", tr!("dim-style", "unit-windows")),
        ]);
    }
    choices(&list)
}

/// How a linear format reads: 0 decimal, 1 scientific, 2 engineering,
/// 3 architectural, 4 fractional.
fn linear_kind(unit: i16, alternate: bool) -> u8 {
    match (unit, alternate) {
        (1, _) => 1,
        (3, _) => 2,
        (4, false) | (4, true) | (6, true) => 3,
        (5, false) | (5, true) | (7, true) => 4,
        _ => 0,
    }
}

/// Whether a primary unit format writes fractions (Architectural,
/// Fractional), which turns the fraction settings on.
pub fn linear_kind_is_fraction(unit: i16) -> bool {
    linear_kind(unit, false) >= 3
}

/// The precision list of a linear format: each entry shows the number it
/// writes.
pub fn linear_precisions(unit: i16, alternate: bool) -> Vec<String> {
    (0..=8)
        .map(|places: usize| {
            let decimals = if places == 0 {
                String::new()
            } else {
                format!(".{}", "0".repeat(places))
            };
            match linear_kind(unit, alternate) {
                1 => format!("0{decimals}E+01"),
                2 => format!("0'-0{decimals}\""),
                3 => match places {
                    0 => "0'-0\"".to_string(),
                    p => format!("0'-0 1/{}\"", 1u32 << p),
                },
                4 => match places {
                    0 => "0".to_string(),
                    p => format!("0 1/{}", 1u32 << p),
                },
                _ => format!("0{decimals}"),
            }
        })
        .collect()
}

/// The precision list of an angular format (DIMAUNIT 0–3).
pub fn angular_precisions(unit: i16) -> Vec<String> {
    (0..=8usize)
        .map(|places| {
            let decimals = if places == 0 {
                String::new()
            } else {
                format!(".{}", "0".repeat(places))
            };
            match unit {
                1 => match places {
                    0 => "0d".to_string(),
                    1 | 2 => "0d00'".to_string(),
                    3 | 4 => "0d00'00\"".to_string(),
                    p => format!("0d00'00.{}\"", "0".repeat(p - 4)),
                },
                2 => format!("0{decimals}g"),
                3 => format!("0{decimals}r"),
                _ => format!("0{decimals}"),
            }
        })
        .collect()
}

fn precision_choices(list: Vec<String>) -> Vec<Choice> {
    list.into_iter()
        .enumerate()
        .map(|(index, label)| choice(index.to_string(), label))
        .collect()
}

/// What the arrowhead lists show for a standard name; a user block keeps
/// its own name.
fn arrow_label(name: &str) -> String {
    crate::ui::style::mleaderstyle::arrow_label(name)
}

// ── View data ───────────────────────────────────────────────────────────────

/// A style in the manager's list.
pub struct StyleRow {
    pub name: String,
    pub annotative: bool,
    pub xref: bool,
    pub in_use: bool,
}

/// One variable of Compare: description, variable, and the value in each
/// style.
pub struct CompareRow {
    pub description: String,
    pub variable: &'static str,
    pub first: String,
    pub second: Option<String>,
}

/// The sample values the preview writes, formatted by the shown style.
#[derive(Clone, Default)]
pub struct PreviewTexts {
    pub horizontal: String,
    pub vertical: String,
    pub aligned: String,
    pub radius: String,
    pub angle: String,
}

/// The names the settings face shows for the draft's handles.
#[derive(Default)]
pub struct DraftNames {
    pub dimltype: String,
    pub ext1: String,
    pub ext2: String,
    pub first: String,
    pub second: String,
    pub leader: String,
    pub text_style: String,
}

pub struct View<'a> {
    pub dialog: &'a Dialog,
    pub styles: Vec<StyleRow>,
    pub selected: &'a str,
    pub current: &'a str,
    /// "Variable = value" for each override of the current style.
    pub overrides: Vec<String>,
    /// The style the manager's preview shows, and its arrowheads.
    pub shown: Option<DimStyle>,
    pub shown_arrows: PreviewArrows,
    pub description: String,
    pub preview: PreviewTexts,
    pub linetypes: Vec<LinetypeRow>,
    pub loaded_linetypes: Vec<String>,
    pub names: DraftNames,
    pub arrow_opts: Vec<String>,
    pub text_styles: Vec<String>,
    pub user_blocks: Vec<String>,
    pub compare_rows: Vec<CompareRow>,
}

pub fn view_window<'a>(v: View<'a>, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let face: Element<'a, Message> = if v.dialog.edit.is_some() {
        edit_view(&v)
    } else if v.dialog.compare.is_some() {
        compare_view(&v)
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
            Some((notice_card(notice), msg(DsMsg::NoticeClose)))
        } else if let Some(name) = &v.dialog.delete_ask {
            Some((delete_card(name), msg(DsMsg::DeleteAnswer(false))))
        } else if v.dialog.discard_ask.is_some() {
            Some((discard_card(), msg(DsMsg::DiscardAnswer(false))))
        } else if let Some(draft) = &v.dialog.create {
            Some((create_card(draft, &v), msg(DsMsg::CreateCancel)))
        } else if let Some(ask) = &v.dialog.reload_ask {
            Some((
                reload_card(ask, |answer| msg(DsMsg::ReloadAnswer(answer))),
                msg(DsMsg::ReloadAnswer(Reload::Cancel)),
            ))
        } else if let Some(load) = &v.dialog.load {
            Some((
                load_linetypes_card(
                    load,
                    &v.loaded_linetypes,
                    msg(DsMsg::LoadFile),
                    |name| msg(DsMsg::LoadToggle(name)),
                    msg(DsMsg::LoadOk),
                    msg(DsMsg::LoadCancel),
                ),
                msg(DsMsg::LoadCancel),
            ))
        } else {
            v.dialog
                .pick
                .as_ref()
                .map(|p| (pick_card(p, &v), msg(DsMsg::PickCancel)))
        };
    if let Some((content, dismiss)) = overlay {
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
            container(content)
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
pub fn title(dialog: &Dialog) -> String {
    match (&dialog.edit, &dialog.compare, &dialog.create) {
        (_, _, Some(_)) => tr!("dim-style", "create-title"),
        (Some(edit), _, _) => match edit.mode {
            Mode::Modify => tr!("dim-style", "modify-title", name = edit.name.clone()),
            Mode::New(_) => tr!("dim-style", "new-title", name = edit.name.clone()),
            Mode::Override => tr!("dim-style", "override-title", name = edit.name.clone()),
        },
        (_, Some(_), _) => tr!("dim-style", "compare-title"),
        _ => tr!("dim-style", "manager-title"),
    }
}

fn help_button<'a>() -> Element<'a, Message> {
    button(text("?").size(12))
        .on_press(msg(DsMsg::Help))
        .style(button_style(false))
        .padding([5, 11])
        .into()
}

fn side_button<'a>(caption: String, message: Option<Message>) -> Element<'a, Message> {
    dialog_button_styled_opt(caption, message, button::secondary)
        .width(Fill)
        .into()
}

const WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");
const ANNOTATIVE: &[u8] = include_bytes!("../../../assets/icons/ui/annotative.svg");

fn annotative_icon<'a>(on_accent: bool) -> Element<'a, Message> {
    crate::ui::icons::tinted(ANNOTATIVE, 12.0, move |theme: &Theme| {
        if on_accent {
            theme.palette().primary.strong.text
        } else {
            theme.palette().primary.base.color
        }
    })
}

// ── Manager ─────────────────────────────────────────────────────────────────

fn list_cell<'a>(
    content: Element<'a, Message>,
    selected: bool,
    indent: f32,
) -> iced::widget::Container<'a, Message> {
    container(row![Space::new().width(indent), content].align_y(iced::Center))
        .padding([4, 8])
        .width(Fill)
        .style(move |theme: &Theme| {
            let pair = theme.palette().primary.strong;
            container::Style {
                background: selected.then_some(Background::Color(pair.color)),
                text_color: selected.then_some(pair.text),
                ..Default::default()
            }
        })
}

/// An entry's caption, or the box it is renamed in.
fn entry_caption<'a>(d: &'a Dialog, key: &str, caption: String) -> Element<'a, Message> {
    match d.rename.as_ref().filter(|(old, _)| old == key) {
        Some((_, typed)) => text_input("", typed)
            .id(iced::widget::Id::new("ds-rename"))
            .on_input(|value| msg(DsMsg::RenameInput(value)))
            .on_submit(msg(DsMsg::RenameCommit))
            .size(11)
            .padding([2, 6])
            .style(input_style)
            .into(),
        None => text(caption).size(11).into(),
    }
}

/// An entry's actions (right click), under it.
fn context_actions<'a>(actions: Vec<(String, Option<Message>)>) -> Element<'a, Message> {
    let buttons: Vec<Element<'a, Message>> = actions
        .into_iter()
        .map(|(caption, message)| dialog_button_styled_opt(caption, message, button::secondary).into())
        .collect();
    container(row(buttons).spacing(4).wrap()).padding([4, 20]).into()
}

fn manager_view<'a>(v: &View<'a>) -> Element<'a, Message> {
    let d = v.dialog;
    let listed = |s: &&StyleRow| {
        (d.list_xrefs || !s.xref) && (!d.in_use_only || s.in_use || s.name == v.current)
    };
    // Family members are listed under their parent.
    let parent_of = |name: &str| -> Option<String> {
        family_member(name)
            .filter(|(parent, _)| v.styles.iter().any(|p| p.name.eq_ignore_ascii_case(parent)))
            .map(|(parent, _)| parent.to_lowercase())
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for style in v.styles.iter().filter(listed).filter(|s| parent_of(&s.name).is_none()) {
        let selected = style.name == v.selected && !d.overrides_selected;
        let line = row![
            if style.annotative {
                annotative_icon(selected)
            } else {
                Space::new().width(12).into()
            },
            entry_caption(d, &style.name, style.name.clone()),
        ]
        .spacing(6)
        .align_y(iced::Center);
        let name = style.name.clone();
        rows.push(
            mouse_area(list_cell(line.into(), selected, 0.0))
                .on_press(msg(DsMsg::Select(name.clone())))
                .on_right_press(msg(DsMsg::Context(Some(name.clone()))))
                .on_double_click(msg(DsMsg::RenameStart(name.clone())))
                .into(),
        );
        if d.context.as_deref() == Some(style.name.as_str()) {
            rows.push(context_actions(vec![
                (tr!("dim-style", "set-current"), Some(msg(DsMsg::SetCurrent))),
                (tr!("dim-style", "rename"), Some(msg(DsMsg::RenameStart(name.clone())))),
                (tr!("dim-style", "delete"), Some(msg(DsMsg::Delete(name.clone())))),
            ]));
        }
        // The current style's overrides come first under it, then its
        // family members.
        if style.name.eq_ignore_ascii_case(v.current) && !v.overrides.is_empty() {
            let on = d.overrides_selected;
            rows.push(
                mouse_area(list_cell(
                    entry_caption(d, OVERRIDES, tr!("dim-style", "style-overrides")),
                    on,
                    18.0,
                ))
                .on_press(msg(DsMsg::SelectOverrides))
                .on_right_press(msg(DsMsg::Context(Some(OVERRIDES.to_string()))))
                .on_double_click(msg(DsMsg::RenameStart(OVERRIDES.to_string())))
                .into(),
            );
            if d.context.as_deref() == Some(OVERRIDES) {
                rows.push(context_actions(vec![
                    (tr!("dim-style", "set-current"), None),
                    (
                        tr!("dim-style", "rename"),
                        Some(msg(DsMsg::RenameStart(OVERRIDES.to_string()))),
                    ),
                    (
                        tr!("dim-style", "delete"),
                        Some(msg(DsMsg::Delete(OVERRIDES.to_string()))),
                    ),
                    (tr!("dim-style", "save-to-current"), Some(msg(DsMsg::SaveOverrides))),
                ]));
            }
        }
        let key = style.name.to_lowercase();
        for child in v
            .styles
            .iter()
            .filter(listed)
            .filter(|c| parent_of(&c.name).as_deref() == Some(key.as_str()))
        {
            let selected = child.name == v.selected && !d.overrides_selected;
            let label = family_member(&child.name).map(|(_, l)| l).unwrap_or_default();
            let child_name = child.name.clone();
            rows.push(
                mouse_area(list_cell(text(label).size(11).into(), selected, 18.0))
                    .on_press(msg(DsMsg::Select(child_name.clone())))
                    .on_right_press(msg(DsMsg::Context(Some(child_name.clone()))))
                    .into(),
            );
            if d.context.as_deref() == Some(child.name.as_str()) {
                rows.push(context_actions(vec![
                    (tr!("dim-style", "set-current"), None),
                    (tr!("dim-style", "rename"), None),
                    (tr!("dim-style", "delete"), Some(msg(DsMsg::Delete(child_name)))),
                ]));
            }
        }
    }
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
        choice("all", tr!("dim-style", "all-styles")),
        choice("used", tr!("dim-style", "styles-in-use")),
    ];
    let filter_selected = filters[usize::from(d.in_use_only)].clone();
    let filter = crate::ui::dropdown_list::pick_list(Some(filter_selected), filters, |c| {
        c.to_string()
    })
    .on_select(|c: Choice| msg(DsMsg::ListFilter(c.code == "used")))
    .text_size(11)
    .width(Fill);
    // Without external-reference styles the option has nothing to hide.
    let xrefs = toggle(
        tr!("dim-style", "hide-xrefs"),
        !d.list_xrefs,
        if v.styles.iter().any(|s| s.xref) {
            Some(Box::new(|on: bool| msg(DsMsg::HideXrefs(on))))
        } else {
            None
        },
    );
    let left = column![
        text(tr!("dim-style", "styles")).size(11).style(muted_style),
        list,
        text(tr!("dim-style", "list")).size(11).style(muted_style),
        filter,
        xrefs,
    ]
    .spacing(6)
    .width(Length::Fixed(250.0))
    .height(Fill);

    let shown_name = if d.overrides_selected {
        tr!("dim-style", "style-overrides")
    } else {
        display_name(v.selected)
    };
    let description = container(
        column![
            text(tr!("dim-style", "description")).size(10).style(accent_text),
            scrollable(text(v.description.clone()).size(11)).height(Fill),
        ]
        .spacing(4),
    )
    .padding([8, 10])
    .width(Fill)
    .height(Length::Fixed(96.0))
    .style(card_style);
    let middle = column![
        text(tr!("dim-style", "preview-of", name = shown_name))
            .size(11)
            .style(muted_style),
        preview_well(v.shown.clone(), v.shown_arrows.clone(), &v.preview, Length::Fill),
        description,
    ]
    .spacing(6)
    .width(Fill)
    .height(Fill);

    let has_style = v.shown.is_some();
    // A family member is never current; Override… changes the current
    // style only.
    let member = !d.overrides_selected && family_member(v.selected).is_some();
    let current_selected = d.overrides_selected || v.selected.eq_ignore_ascii_case(v.current);
    let actions = column![
        side_button(
            tr!("dim-style", "set-current"),
            (has_style && !member).then(|| msg(DsMsg::SetCurrent))
        ),
        side_button(tr!("dim-style", "new"), Some(msg(DsMsg::New))),
        side_button(
            tr!("dim-style", "modify"),
            has_style.then(|| msg(DsMsg::Modify))
        ),
        side_button(
            tr!("dim-style", "override"),
            (has_style && current_selected).then(|| msg(DsMsg::Override))
        ),
        side_button(tr!("dim-style", "compare"), Some(msg(DsMsg::Compare))),
    ]
    .spacing(8)
    .width(Length::Fixed(132.0));

    let body = column![
        container(
            text(tr!("dim-style", "current", name = v.current.to_string())).size(12)
        )
        .padding([8, 12])
        .width(Fill)
        .style(card_style),
        row![left, middle, actions].spacing(12).height(Fill),
    ]
    .spacing(10)
    .padding([12, 14])
    .height(Fill);

    let footer = row![
        help_button(),
        Space::new().width(Fill),
        dialog_button(tr!("dim-style", "close"), Message::CloseModal, true),
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

// ── Cards over the faces ────────────────────────────────────────────────────

fn create_card<'a>(draft: &'a CreateDraft, v: &View<'a>) -> Element<'a, Message> {
    let starts: Vec<String> = v
        .styles
        .iter()
        .filter(|s| !s.xref && family_member(&s.name).is_none())
        .map(|s| s.name.clone())
        .collect();
    // A style for one kind of dimension takes its start style's name.
    let name = text_input("", &draft.name)
        .id(iced::widget::Id::new("ds-new-name"))
        .on_input_maybe((draft.use_for == 0).then_some(|value| msg(DsMsg::CreateName(value))))
        .on_submit(msg(DsMsg::CreateContinue))
        .size(12)
        .padding([5, 8])
        .style(input_style)
        .width(Fill);
    let start = crate::ui::dropdown_list::pick_list(
        Some(draft.start_with.clone()),
        starts,
        |value| value.to_string(),
    )
    .on_select(|value| msg(DsMsg::CreateStart(value)))
    .text_size(12)
    .width(Fill);
    let annotative = row![
        checkbox(draft.annotative)
            .on_toggle(|on| msg(DsMsg::CreateAnnotative(on)))
            .size(14),
        annotative_icon(false),
        text(tr!("dim-style", "annotative")).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let uses = use_for_choices();
    let use_for = crate::ui::dropdown_list::pick_list(
        Some(uses[usize::from(draft.use_for).min(uses.len() - 1)].clone()),
        uses,
        |c| c.to_string(),
    )
    .on_select(|c: Choice| msg(DsMsg::CreateUse(c.code.parse().unwrap_or(0))))
    .text_size(12)
    .width(Fill);
    let body = column![
        text(tr!("dim-style", "create-title"))
            .size(13)
            .style(accent_text),
        column![
            text(tr!("dim-style", "new-name")).size(11).style(muted_style),
            name
        ]
        .spacing(4),
        column![
            text(tr!("dim-style", "start-with")).size(11).style(muted_style),
            start
        ]
        .spacing(4),
        annotative,
        column![
            text(tr!("dim-style", "use-for")).size(11).style(muted_style),
            use_for
        ]
        .spacing(4),
        Space::new().height(4),
        row![
            help_button(),
            Space::new().width(Fill),
            dialog_button(tr!("dim-style", "cancel"), msg(DsMsg::CreateCancel), false),
            dialog_button(
                tr!("dim-style", "continue"),
                msg(DsMsg::CreateContinue),
                true
            ),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(10);
    panel(body.into(), 440.0)
}

/// The short label a family member made by Use for goes by.
pub fn use_for_label(use_for: u8) -> String {
    match use_for {
        1 => tr!("dim-style", "short-linear"),
        2 => tr!("dim-style", "short-angular"),
        3 => tr!("dim-style", "short-radius"),
        4 => tr!("dim-style", "short-diameter"),
        5 => tr!("dim-style", "short-ordinate"),
        _ => tr!("dim-style", "short-leaders"),
    }
}

fn use_for_choices() -> Vec<Choice> {
    choices(&[
        ("0", tr!("dim-style", "use-all")),
        ("1", tr!("dim-style", "use-linear")),
        ("2", tr!("dim-style", "use-angular")),
        ("3", tr!("dim-style", "use-radius")),
        ("4", tr!("dim-style", "use-diameter")),
        ("5", tr!("dim-style", "use-ordinate")),
        ("6", tr!("dim-style", "use-leaders")),
    ])
}

fn notice_card<'a>(notice: &'a Notice) -> Element<'a, Message> {
    let (title, detail): (String, Option<String>) = match notice {
        Notice::EmptyName => (tr!("dim-style", "name-empty"), None),
        Notice::Exists(name) => (
            tr!("dim-style", "name-exists", name = name.clone()),
            Some(tr!("dim-style", "name-exists-detail")),
        ),
        Notice::UnsupportedCharacters => (
            tr!("dim-style", "name-chars"),
            Some(format!(
                "{}\n\\ < > / ? \" : ; * | , = `",
                tr!("dim-style", "name-chars-detail")
            )),
        ),
        Notice::Delete(name) => (
            tr!("dim-style", "delete-refused", name = name.clone()),
            Some(tr!("dim-style", "delete-refused-detail")),
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
            dialog_button(tr!("dim-style", "close"), msg(DsMsg::NoticeClose), true),
        ],
    ]
    .spacing(14);
    panel(body.into(), 440.0)
}

/// Set Current on another style while the current one has overrides.
fn discard_card<'a>() -> Element<'a, Message> {
    let body = column![
        row![
            crate::ui::icons::semantic(WARN, 20.0),
            column![
                text(tr!("dim-style", "discard-overrides")).size(12),
                text(tr!("dim-style", "discard-overrides-ask")).size(12),
                text(tr!("dim-style", "discard-overrides-hint"))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(6),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Start),
        row![
            Space::new().width(Fill),
            dialog_button(tr!("dim-style", "cancel"), msg(DsMsg::DiscardAnswer(false)), false),
            dialog_button(tr!("dim-style", "ok"), msg(DsMsg::DiscardAnswer(true)), true),
        ]
        .spacing(6),
    ]
    .spacing(14);
    panel(body.into(), 460.0)
}

fn delete_card<'a>(name: &'a str) -> Element<'a, Message> {
    let body = column![
        text(tr!("dim-style", "delete-title"))
            .size(13)
            .style(accent_text),
        row![
            crate::ui::icons::semantic(WARN, 20.0),
            text(tr!(
                "dim-style",
                "delete-ask",
                name = if name == OVERRIDES {
                    tr!("dim-style", "style-overrides")
                } else {
                    display_name(name)
                }
            ))
            .size(12),
        ]
        .spacing(12)
        .align_y(iced::Center),
        row![
            Space::new().width(Fill),
            dialog_button(tr!("dim-style", "yes"), msg(DsMsg::DeleteAnswer(true)), false),
            dialog_button(tr!("dim-style", "no"), msg(DsMsg::DeleteAnswer(false)), true),
        ]
        .spacing(6),
    ]
    .spacing(14);
    panel(body.into(), 440.0)
}

fn pick_card<'a>(p: &'a Pick, v: &View<'a>) -> Element<'a, Message> {
    match p {
        Pick::Linetype(_, selected) => linetype_card(
            &v.linetypes,
            selected,
            |name| msg(DsMsg::PickChoice(name)),
            msg(DsMsg::PickLoad),
            msg(DsMsg::PickCancel),
            msg(DsMsg::PickOk),
        ),
        Pick::UserArrow(_, selected) => {
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
                .on_select(|value| msg(DsMsg::PickChoice(value)))
                .text_size(12)
                .width(Fill)
                .into()
            };
            let body = column![
                text(tr!("mleader-style", "arrow-title"))
                    .size(13)
                    .style(accent_text),
                text(tr!("mleader-style", "from-blocks"))
                    .size(11)
                    .style(muted_style),
                chooser,
                row![
                    Space::new().width(Fill),
                    dialog_button(tr!("dim-style", "cancel"), msg(DsMsg::PickCancel), false),
                    dialog_button(tr!("dim-style", "ok"), msg(DsMsg::PickOk), true),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(10);
            panel(body.into(), 440.0)
        }
    }
}

// ── Settings face: controls ─────────────────────────────────────────────────

fn label<'a>(value: String, enabled: bool) -> Element<'a, Message> {
    text(value)
        .size(11)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(if enabled {
                crate::ui::style::readable::muted_text(theme)
            } else {
                crate::ui::style::readable::muted_text(theme).scale_alpha(0.55)
            }),
        })
        .width(Length::Fixed(LABEL))
        .into()
}

fn field_row<'a>(
    caption: String,
    control: Element<'a, Message>,
    enabled: bool,
) -> Element<'a, Message> {
    row![
        label(caption, enabled),
        container(control).width(Length::Fixed(FIELD))
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn input<'a>(e: &'a Edit, field: &'static str, enabled: bool) -> Element<'a, Message> {
    let mut i = text_input("", e.text(field))
        .id(field_id(field))
        .size(11)
        .padding([4, 8])
        .width(Fill)
        .style(input_style);
    if enabled {
        i = i.on_input(move |value| msg(DsMsg::Text(field, value)));
    }
    i.into()
}

fn number<'a>(
    caption: String,
    e: &'a Edit,
    field: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    field_row(caption, input(e, field, enabled), enabled)
}

fn list_row<'a>(
    caption: String,
    options: Vec<Choice>,
    selected: Choice,
    field: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    let mut list = crate::ui::dropdown_list::pick_list(Some(selected), options, |c| c.to_string())
        .text_size(11)
        .width(Fill);
    if enabled {
        list = list.on_select(move |c: Choice| msg(DsMsg::Set(field, c.code)));
    }
    field_row(caption, list.into(), enabled)
}

type OnToggle<'a> = Option<Box<dyn Fn(bool) -> Message + 'a>>;

fn toggle<'a>(caption: String, value: bool, on_toggle: OnToggle<'a>) -> Element<'a, Message> {
    let enabled = on_toggle.is_some();
    let mut check = checkbox(value).size(14);
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

fn switch<'a>(
    caption: String,
    value: bool,
    field: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    toggle(
        caption,
        value,
        enabled.then(|| Box::new(move |on: bool| msg(DsMsg::Toggle(field, on))) as Box<dyn Fn(bool) -> Message>),
    )
}

/// A radio choice of `field`, inert (shown muted) while `enabled` is false.
fn option<'a>(
    caption: String,
    code: u8,
    current: u8,
    field: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    let r = radio(caption, code, Some(current), move |value: u8| {
        if enabled {
            msg(DsMsg::Set(field, value.to_string()))
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
            style.text_color =
                Some(crate::ui::style::readable::muted_text(theme).scale_alpha(0.7));
            style.dot_color = style.dot_color.scale_alpha(0.45);
            style.border_color = style.border_color.scale_alpha(0.45);
            style
        })
        .into()
    }
}

/// The colour of a style field: ACI index or true colour.
pub fn style_color(index: i16, true_color: Option<AcadColor>) -> AcadColor {
    match (index, true_color) {
        (_, Some(color)) => color,
        (0, _) => AcadColor::ByBlock,
        (256, _) => AcadColor::ByLayer,
        (i, _) if (1..=255).contains(&i) => AcadColor::Index(i as u8),
        _ => AcadColor::ByBlock,
    }
}

fn color_row<'a>(
    caption: String,
    current: AcadColor,
    field: &'static str,
    d: &'a Dialog,
) -> Element<'a, Message> {
    let control = crate::ui::color_select::color_selector(
        current,
        d.open_list == Some(field),
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: true,
            standard_only: true,
            ..Default::default()
        },
        move |color| msg(DsMsg::Color(field, color)),
        msg(DsMsg::OpenList(field)),
        msg(DsMsg::ColorMore(field)),
    );
    field_row(caption, control, true)
}

fn swatch_of<'a>(color: AcadColor) -> Element<'a, Message> {
    let (bg, _) = crate::ui::properties::acad_color_display(color);
    crate::ui::color_select::swatch(bg)
}

/// Fill color: None, Background, then the colours of a colour list.
fn fill_row<'a>(draft: &DimStyle, d: &'a Dialog) -> Element<'a, Message> {
    let field = "fill";
    let color = style_color(draft.dimtfillclr, draft.dimtfillclr_true_color);
    let current = match draft.dimtfill {
        0 => (
            Space::new().width(13).into(),
            tr!("dim-style", "fill-none"),
        ),
        1 => (
            Space::new().width(13).into(),
            tr!("dim-style", "fill-background"),
        ),
        _ => (
            swatch_of(color),
            crate::ui::color_select::color_display_name(color),
        ),
    };
    let mut entries: Vec<(Element<'a, Message>, String, Message)> = vec![
        (
            Space::new().width(13).into(),
            tr!("dim-style", "fill-none"),
            msg(DsMsg::Set(field, "none".into())),
        ),
        (
            Space::new().width(13).into(),
            tr!("dim-style", "fill-background"),
            msg(DsMsg::Set(field, "background".into())),
        ),
    ];
    let standard = [
        AcadColor::ByLayer,
        AcadColor::ByBlock,
        AcadColor::Index(1),
        AcadColor::Index(2),
        AcadColor::Index(3),
        AcadColor::Index(4),
        AcadColor::Index(5),
        AcadColor::Index(6),
        AcadColor::Index(7),
    ];
    let mut listed: Vec<AcadColor> = standard.to_vec();
    if draft.dimtfill == 2 && !listed.contains(&color) {
        listed.push(color);
    }
    for c in listed {
        entries.push((
            swatch_of(c),
            crate::ui::color_select::color_display_name(c),
            msg(DsMsg::Color(field, c)),
        ));
    }
    entries.push((
        Space::new().width(13).into(),
        tr!("dim-style", "select-color"),
        msg(DsMsg::ColorMore(field)),
    ));
    let control = sample_list(current, entries, msg(DsMsg::OpenList(field)), d.open_list == Some(field), true);
    field_row(tr!("dim-style", "fill-color"), control, true)
}

fn linetype_row<'a>(
    caption: String,
    current: &str,
    field: &'static str,
    v: &View<'a>,
) -> Element<'a, Message> {
    let pattern_of = |name: &str| -> Vec<f64> {
        v.linetypes
            .iter()
            .find(|row| row.name.eq_ignore_ascii_case(name))
            .map(|row| row.pattern.clone())
            .unwrap_or_default()
    };
    let mut entries: Vec<(Element<'a, Message>, String, Message)> = v
        .linetypes
        .iter()
        .map(|row| {
            (
                pattern_sample(row.pattern.clone()),
                record_label(&row.name),
                msg(DsMsg::Linetype(field, row.name.clone())),
            )
        })
        .collect();
    entries.push((
        Space::new().width(48).into(),
        tr!("dim-style", "other"),
        msg(DsMsg::Linetype(field, OTHER.to_string())),
    ));
    let control = sample_list(
        (pattern_sample(pattern_of(current)), record_label(current)),
        entries,
        msg(DsMsg::OpenList(field)),
        v.dialog.open_list == Some(field),
        true,
    );
    field_row(caption, control, true)
}

fn lineweight_row<'a>(
    caption: String,
    value: i16,
    field: &'static str,
    d: &'a Dialog,
) -> Element<'a, Message> {
    let weight = lineweight(value);
    let control = sample_list(
        (
            weight_sample(weight),
            crate::ui::properties::LwItem(weight).to_string(),
        ),
        crate::ui::properties::lw_options()
            .into_iter()
            .map(|item| {
                (
                    weight_sample(item.0),
                    item.to_string(),
                    msg(DsMsg::Lineweight(field, item.0)),
                )
            })
            .collect(),
        msg(DsMsg::OpenList(field)),
        d.open_list == Some(field),
        true,
    );
    field_row(caption, control, true)
}

/// A DIMLWD / DIMLWE value as a lineweight.
pub fn lineweight(value: i16) -> LineWeight {
    match value {
        -1 => LineWeight::ByLayer,
        -2 => LineWeight::ByBlock,
        -3 => LineWeight::Default,
        v => LineWeight::Value(v),
    }
}

/// A lineweight as its DIMLWD / DIMLWE value.
pub fn lineweight_value(weight: LineWeight) -> i16 {
    match weight {
        LineWeight::ByLayer => -1,
        LineWeight::ByBlock => -2,
        LineWeight::Default => -3,
        LineWeight::Value(v) => v,
    }
}

fn arrow_row<'a>(caption: String, slot: u8, current: &str, v: &View<'a>) -> Element<'a, Message> {
    let mut options: Vec<Choice> = v
        .arrow_opts
        .iter()
        .map(|name| choice(name.clone(), arrow_label(name)))
        .collect();
    if !options.iter().any(|c| c.code == current) {
        options.push(choice(current, arrow_label(current)));
    }
    options.push(choice(OTHER, tr!("dim-style", "user-arrow")));
    let list = crate::ui::dropdown_list::pick_list(
        Some(choice(current, arrow_label(current))),
        options,
        |c| c.to_string(),
    )
    .on_select(move |c: Choice| msg(DsMsg::Arrow(slot, c.code)))
    .text_size(11)
    .width(Fill);
    column![
        text(caption).size(11).style(muted_style),
        container(list).width(Length::Fixed(LABEL + FIELD + 8.0))
    ]
    .spacing(3)
    .into()
}

/// One zero-suppression switch of `var` (`lead`, `trail`, `feet`, `inch`).
fn zero_switch<'a>(
    caption: String,
    value: i16,
    field: &'static str,
    bit: u8,
    enabled: bool,
) -> Element<'a, Message> {
    let on = zero_flag(value, bit);
    switch(caption, on, field, enabled)
}

/// Whether zero-suppression switch `bit` (0 leading, 1 trailing, 2 zero
/// feet, 3 zero inches) is on in a DIMZIN-like value.
pub fn zero_flag(value: i16, bit: u8) -> bool {
    match bit {
        0 => value & 4 != 0,
        1 => value & 8 != 0,
        // 0 suppresses both, 1 shows both, 2 shows feet only, 3 inches only.
        2 => matches!(value & 3, 0 | 3),
        _ => matches!(value & 3, 0 | 2),
    }
}

/// `value` with zero-suppression switch `bit` set to `on`.
pub fn with_zero_flag(value: i16, bit: u8, on: bool) -> i16 {
    match bit {
        0 => (value & !4) | if on { 4 } else { 0 },
        1 => (value & !8) | if on { 8 } else { 0 },
        _ => {
            let mut feet = zero_flag(value, 2);
            let mut inch = zero_flag(value, 3);
            if bit == 2 {
                feet = on;
            } else {
                inch = on;
            }
            let low = match (feet, inch) {
                (true, true) => 0,
                (false, false) => 1,
                (false, true) => 2,
                (true, false) => 3,
            };
            (value & !3) | low
        }
    }
}

// ── Settings face: tabs ─────────────────────────────────────────────────────

fn group<'a>(caption: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    card(caption, content)
}

/// A settings tab: the cards of the left column and those under the
/// preview.
type Page<'a> = (Vec<Element<'a, Message>>, Vec<Element<'a, Message>>);

fn page<'a>(left: Vec<Element<'a, Message>>, right: Vec<Element<'a, Message>>) -> Page<'a> {
    (left, right)
}

/// Whether the draft's arrowheads are ticks (Oblique, Architectural tick,
/// or DIMTSZ), which turns Extend beyond ticks on.
pub fn draws_ticks(draft: &DimStyle, names: &DraftNames) -> bool {
    let tick = |name: &str| matches!(name, "Oblique" | "Architectural tick");
    draft.dimtsz > 0.0
        || if draft.dimsah {
            tick(&names.first) || tick(&names.second)
        } else {
            tick(&names.first)
        }
}

fn lines_tab<'a>(v: &View<'a>, e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let ticks = draws_ticks(d, &v.names);
    let dim_lines = column![
        color_row(
            tr!("dim-style", "color"),
            style_color(d.dimclrd, d.dimclrd_true_color),
            "dimclrd",
            v.dialog
        ),
        linetype_row(tr!("dim-style", "linetype"), &v.names.dimltype, "dimltype", v),
        lineweight_row(tr!("dim-style", "lineweight"), d.dimlwd, "dimlwd", v.dialog),
        number(tr!("dim-style", "extend-ticks"), e, "dimdle", ticks),
        number(tr!("dim-style", "baseline-spacing"), e, "dimdli", true),
        row![
            label(tr!("dim-style", "suppress"), true),
            switch(tr!("dim-style", "dim-line-1"), d.dimsd1, "dimsd1", true),
            switch(tr!("dim-style", "dim-line-2"), d.dimsd2, "dimsd2", true),
        ]
        .spacing(14)
        .align_y(iced::Center),
    ]
    .spacing(8);
    let ext_lines = column![
        color_row(
            tr!("dim-style", "color"),
            style_color(d.dimclre, d.dimclre_true_color),
            "dimclre",
            v.dialog
        ),
        linetype_row(tr!("dim-style", "linetype-ext1"), &v.names.ext1, "dimltex1", v),
        linetype_row(tr!("dim-style", "linetype-ext2"), &v.names.ext2, "dimltex2", v),
        lineweight_row(tr!("dim-style", "lineweight"), d.dimlwe, "dimlwe", v.dialog),
        row![
            label(tr!("dim-style", "suppress"), true),
            switch(tr!("dim-style", "ext-line-1"), d.dimse1, "dimse1", true),
            switch(tr!("dim-style", "ext-line-2"), d.dimse2, "dimse2", true),
        ]
        .spacing(14)
        .align_y(iced::Center),
        number(tr!("dim-style", "extend-dim-lines"), e, "dimexe", true),
        number(tr!("dim-style", "offset-origin"), e, "dimexo", true),
        switch(tr!("dim-style", "fixed-length"), d.dimfxlon, "dimfxlon", true),
        number(tr!("dim-style", "length"), e, "dimfxl", d.dimfxlon),
    ]
    .spacing(8);
    page(
        vec![
            group(tr!("dim-style", "group-dim-lines"), dim_lines),
            group(tr!("dim-style", "group-ext-lines"), ext_lines),
        ],
        vec![],
    )
}

/// Center mark choice of a DIMCEN value: 0 none, 1 mark, 2 line.
pub fn center_mode(dimcen: f64) -> u8 {
    if dimcen > 0.0 {
        1
    } else if dimcen < 0.0 {
        2
    } else {
        0
    }
}

fn symbols_tab<'a>(v: &View<'a>, e: &'a Edit, center: u8) -> Page<'a> {
    let d = &e.draft;
    let arrows = column![
        arrow_row(tr!("dim-style", "first"), 0, &v.names.first, v),
        arrow_row(tr!("dim-style", "second"), 1, &v.names.second, v),
        arrow_row(tr!("dim-style", "leader"), 2, &v.names.leader, v),
        number(tr!("dim-style", "arrow-size"), e, "dimasz", true),
    ]
    .spacing(8);
    let centers = row![
        column![
            option(tr!("dim-style", "center-none"), 0, center, "center", true),
            option(tr!("dim-style", "center-mark"), 1, center, "center", true),
            option(tr!("dim-style", "center-line"), 2, center, "center", true),
        ]
        .spacing(6)
        .width(Length::Fixed(LABEL)),
        container(input(e, "dimcen", center != 0)).width(Length::Fixed(FIELD)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let arc = column![
        option(tr!("dim-style", "arc-preceding"), 0, d.dimarcsym.clamp(0, 2) as u8, "dimarcsym", true),
        option(tr!("dim-style", "arc-above"), 1, d.dimarcsym.clamp(0, 2) as u8, "dimarcsym", true),
        option(tr!("dim-style", "arc-none"), 2, d.dimarcsym.clamp(0, 2) as u8, "dimarcsym", true),
    ]
    .spacing(6);
    let jog = row![
        label(tr!("dim-style", "jog-height"), true),
        container(input(e, "dimjag", true)).width(Length::Fixed(FIELD * 0.6)),
        text(tr!("dim-style", "times-text-height")).size(11).style(muted_style),
    ]
    .spacing(8)
    .align_y(iced::Center);
    page(
        vec![
            group(tr!("dim-style", "group-arrowheads"), arrows),
            group(tr!("dim-style", "group-center-marks"), centers),
            group(
                tr!("dim-style", "group-break"),
                number(tr!("dim-style", "break-size"), e, "dimbreak", true),
            ),
        ],
        vec![
            group(tr!("dim-style", "group-arc-symbol"), arc),
            group(
                tr!("dim-style", "group-radius-jog"),
                number(tr!("dim-style", "jog-angle"), e, "dimjogang", true),
            ),
            group(tr!("dim-style", "group-linear-jog"), jog),
        ],
    )
}

/// Text alignment of DIMTIH / DIMTOH: 0 horizontal, 1 aligned with the
/// dimension line, 2 ISO standard.
pub fn text_alignment(draft: &DimStyle) -> u8 {
    match (draft.dimtih, draft.dimtoh) {
        (true, true) | (true, false) => 0,
        (false, false) => 1,
        (false, true) => 2,
    }
}

fn text_tab<'a>(v: &View<'a>, e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let mut styles: Vec<Choice> = v.text_styles.iter().map(|s| choice(s.clone(), s.clone())).collect();
    if !styles.iter().any(|c| c.code == v.names.text_style) {
        styles.push(choice(v.names.text_style.clone(), v.names.text_style.clone()));
    }
    let style_list = crate::ui::dropdown_list::pick_list(
        Some(choice(v.names.text_style.clone(), v.names.text_style.clone())),
        styles,
        |c| c.to_string(),
    )
    .on_select(|c: Choice| msg(DsMsg::Set("dimtxsty", c.code)))
    .text_size(11)
    .width(Fill);
    let style_row = row![
        label(tr!("dim-style", "text-style"), true),
        container(
            row![
                style_list,
                button(text("…").size(11))
                    .on_press(msg(DsMsg::TextStyles))
                    .style(button_style(false))
                    .padding([3, 8]),
            ]
            .spacing(4)
            .align_y(iced::Center)
        )
        .width(Length::Fixed(FIELD)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let fraction = linear_kind(d.dimlunit, false) >= 3;
    let appearance = column![
        style_row,
        color_row(
            tr!("dim-style", "text-color"),
            style_color(d.dimclrt, d.dimclrt_true_color),
            "dimclrt",
            v.dialog
        ),
        fill_row(d, v.dialog),
        number(tr!("dim-style", "text-height"), e, "dimtxt", true),
        number(tr!("dim-style", "fraction-height"), e, "dimtfac", fraction),
        switch(tr!("dim-style", "frame"), d.dimgap < 0.0, "frame", true),
    ]
    .spacing(8);
    let vertical = choices(&[
        ("0", tr!("dim-style", "vert-centered")),
        ("1", tr!("dim-style", "vert-above")),
        ("2", tr!("dim-style", "vert-outside")),
        ("3", tr!("dim-style", "vert-jis")),
        ("4", tr!("dim-style", "vert-below")),
    ]);
    let horizontal = choices(&[
        ("0", tr!("dim-style", "hor-centered")),
        ("1", tr!("dim-style", "hor-ext1")),
        ("2", tr!("dim-style", "hor-ext2")),
        ("3", tr!("dim-style", "hor-over-ext1")),
        ("4", tr!("dim-style", "hor-over-ext2")),
    ]);
    let direction = choices(&[
        ("0", tr!("dim-style", "left-to-right")),
        ("1", tr!("dim-style", "right-to-left")),
    ]);
    let placement = column![
        list_row(
            tr!("dim-style", "vertical"),
            vertical.clone(),
            pick(&vertical, &d.dimtad.to_string()),
            "dimtad",
            true
        ),
        list_row(
            tr!("dim-style", "horizontal"),
            horizontal.clone(),
            pick(&horizontal, &d.dimjust.to_string()),
            "dimjust",
            true
        ),
        list_row(
            tr!("dim-style", "view-direction"),
            direction.clone(),
            pick(&direction, if d.dimtxtdirection { "1" } else { "0" }),
            "dimtxtdirection",
            true
        ),
        number(tr!("dim-style", "offset-dim-line"), e, "dimgap", true),
    ]
    .spacing(8);
    let align = text_alignment(d);
    let alignment = column![
        option(tr!("dim-style", "align-horizontal"), 0, align, "text_align", true),
        option(tr!("dim-style", "align-aligned"), 1, align, "text_align", true),
        option(tr!("dim-style", "align-iso"), 2, align, "text_align", true),
    ]
    .spacing(6);
    page(
        vec![
            group(tr!("dim-style", "group-text-appearance"), appearance),
            group(tr!("dim-style", "group-text-placement"), placement),
        ],
        vec![group(tr!("dim-style", "group-text-alignment"), alignment)],
    )
}

/// Fit option of DIMATFIT / DIMTIX: 0 best fit, 1 arrows, 2 text, 3 both,
/// 4 always keep text between the extension lines.
pub fn fit_option(draft: &DimStyle) -> u8 {
    if draft.dimtix {
        return 4;
    }
    match draft.dimatfit {
        1 => 1,
        2 => 2,
        0 => 3,
        _ => 0,
    }
}

fn fit_tab<'a>(e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let fit = fit_option(d);
    let options = column![
        text(tr!("dim-style", "fit-question")).size(11).style(muted_style),
        option(tr!("dim-style", "fit-best"), 0, fit, "fit", true),
        option(tr!("dim-style", "fit-arrows"), 1, fit, "fit", true),
        option(tr!("dim-style", "fit-text"), 2, fit, "fit", true),
        option(tr!("dim-style", "fit-both"), 3, fit, "fit", true),
        option(tr!("dim-style", "fit-keep"), 4, fit, "fit", true),
        switch(tr!("dim-style", "suppress-arrows"), d.dimsoxd, "dimsoxd", true),
    ]
    .spacing(6);
    let movement = d.dimtmove.clamp(0, 2) as u8;
    let placement = column![
        text(tr!("dim-style", "placement-question")).size(11).style(muted_style),
        option(tr!("dim-style", "move-beside"), 0, movement, "dimtmove", true),
        option(tr!("dim-style", "move-leader"), 1, movement, "dimtmove", true),
        option(tr!("dim-style", "move-free"), 2, movement, "dimtmove", true),
    ]
    .spacing(6);
    let annotative = d.annotative;
    let to_layout = !annotative && d.dimscale == 0.0;
    let scale_mode = u8::from(to_layout);
    let scale = column![
        row![
            switch(tr!("dim-style", "annotative"), annotative, "annotative", true),
            annotative_icon(false),
        ]
        .spacing(6)
        .align_y(iced::Center),
        option(tr!("dim-style", "scale-layout"), 1, scale_mode, "scale_mode", !annotative),
        row![
            container(option(
                tr!("dim-style", "scale-overall"),
                0,
                scale_mode,
                "scale_mode",
                !annotative
            ))
            .width(Length::Fixed(LABEL)),
            container(input(e, "dimscale", !annotative && !to_layout)).width(Length::Fixed(FIELD)),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(6);
    let tuning = column![
        switch(tr!("dim-style", "place-manually"), d.dimupt, "dimupt", true),
        switch(tr!("dim-style", "draw-dim-line"), d.dimtofl, "dimtofl", true),
    ]
    .spacing(6);
    page(
        vec![
            group(tr!("dim-style", "group-fit-options"), options),
            group(tr!("dim-style", "group-text-placement"), placement),
        ],
        vec![
            group(tr!("dim-style", "group-scale"), scale),
            group(tr!("dim-style", "group-fine-tuning"), tuning),
        ],
    )
}

/// The four zero-suppression switches of a group: `fields` are the
/// leading, trailing, zero-feet and zero-inches switch names.
#[allow(clippy::too_many_arguments)]
fn zero_group<'a>(
    e: &'a Edit,
    value: i16,
    fields: [&'static str; 4],
    decimal: bool,
    feet: bool,
    enabled: bool,
    sub_units: Option<(&'static str, &'static str)>,
) -> Element<'a, Message> {
    let lead = zero_switch(tr!("dim-style", "zero-leading"), value, fields[0], 0, enabled && decimal);
    let mut leading = column![lead].spacing(6);
    if let Some((factor, suffix)) = sub_units {
        let on = enabled && decimal && zero_flag(value, 0);
        leading = leading.push(
            column![
                text(tr!("dim-style", "sub-units-factor")).size(11).style(muted_style),
                container(input(e, factor, on)).width(Length::Fixed(110.0)),
                text(tr!("dim-style", "sub-units-suffix")).size(11).style(muted_style),
                container(input(e, suffix, on)).width(Length::Fixed(110.0)),
            ]
            .spacing(3)
            .padding(iced::Padding {
                left: 22.0,
                ..Default::default()
            }),
        );
    }
    let right = column![
        zero_switch(tr!("dim-style", "zero-trailing"), value, fields[1], 1, enabled && decimal),
        zero_switch(tr!("dim-style", "zero-feet"), value, fields[2], 2, enabled && feet),
        zero_switch(tr!("dim-style", "zero-inches"), value, fields[3], 3, enabled && feet),
    ]
    .spacing(6);
    row![
        container(leading).width(Length::Fixed(LABEL + 20.0)),
        right
    ]
    .spacing(8)
    .into()
}

fn primary_tab<'a>(e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let units = unit_choices(false);
    let kind = linear_kind(d.dimlunit, false);
    let precisions = precision_choices(linear_precisions(d.dimlunit, false));
    let fractions = choices(&[
        ("0", tr!("dim-style", "frac-horizontal")),
        ("1", tr!("dim-style", "frac-diagonal")),
        ("2", tr!("dim-style", "frac-not-stacked")),
    ]);
    let separators = choices(&[
        ("46", tr!("dim-style", "sep-period")),
        ("44", tr!("dim-style", "sep-comma")),
        ("32", tr!("dim-style", "sep-space")),
    ]);
    let linear = column![
        list_row(
            tr!("dim-style", "unit-format"),
            units.clone(),
            pick(&units, &d.dimlunit.to_string()),
            "dimlunit",
            true
        ),
        list_row(
            tr!("dim-style", "precision"),
            precisions.clone(),
            pick(&precisions, &d.dimdec.to_string()),
            "dimdec",
            true
        ),
        list_row(
            tr!("dim-style", "fraction-format"),
            fractions.clone(),
            pick(&fractions, &d.dimfrac.to_string()),
            "dimfrac",
            kind >= 3
        ),
        list_row(
            tr!("dim-style", "decimal-separator"),
            separators.clone(),
            pick(&separators, &d.dimdsep.to_string()),
            "dimdsep",
            kind == 0
        ),
        number(tr!("dim-style", "round-off"), e, "dimrnd", true),
        number(tr!("dim-style", "prefix"), e, "prefix", true),
        number(tr!("dim-style", "suffix"), e, "suffix", true),
    ]
    .spacing(8);
    let scale = column![
        number(tr!("dim-style", "scale-factor"), e, "dimlfac", true),
        switch(tr!("dim-style", "layout-only"), d.dimlfac < 0.0, "layout_only", true),
    ]
    .spacing(8);
    let zeros = zero_group(
        e,
        d.dimzin,
        ["dimzin.lead", "dimzin.trail", "dimzin.feet", "dimzin.inch"],
        kind != 3,
        matches!(kind, 2 | 3),
        true,
        Some(("dimmzf", "dimmzs")),
    );
    let angular_units = choices(&[
        ("0", tr!("dim-style", "ang-decimal")),
        ("1", tr!("dim-style", "ang-dms")),
        ("2", tr!("dim-style", "ang-grads")),
        ("3", tr!("dim-style", "ang-radians")),
    ]);
    let angular_prec = precision_choices(angular_precisions(d.dimaunit));
    let angular = column![
        list_row(
            tr!("dim-style", "units-format"),
            angular_units.clone(),
            pick(&angular_units, &d.dimaunit.to_string()),
            "dimaunit",
            true
        ),
        list_row(
            tr!("dim-style", "precision"),
            angular_prec.clone(),
            pick(&angular_prec, &d.dimadec.to_string()),
            "dimadec",
            true
        ),
        row![
            label(tr!("dim-style", "group-zero"), true),
            switch(tr!("dim-style", "zero-leading"), d.dimazin & 1 != 0, "dimazin.lead", true),
            switch(tr!("dim-style", "zero-trailing"), d.dimazin & 2 != 0, "dimazin.trail", true),
        ]
        .spacing(14)
        .align_y(iced::Center),
    ]
    .spacing(8);
    page(
        vec![
            group(tr!("dim-style", "group-linear"), linear),
            group(tr!("dim-style", "group-measurement-scale"), scale),
            group(tr!("dim-style", "group-zero"), zeros),
        ],
        vec![group(tr!("dim-style", "group-angular"), angular)],
    )
}

/// Where alternate units go: 0 after the primary value, 1 below it (a
/// `\X` in the primary text template).
pub fn alt_below(draft: &DimStyle) -> bool {
    draft.dimpost.contains("\\X")
}

fn alternate_tab<'a>(e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let on = d.dimalt;
    let units = unit_choices(true);
    let kind = linear_kind(d.dimaltu, true);
    let precisions = precision_choices(linear_precisions(d.dimaltu, true));
    let alternate = column![
        list_row(
            tr!("dim-style", "unit-format"),
            units.clone(),
            pick(&units, &d.dimaltu.to_string()),
            "dimaltu",
            on
        ),
        list_row(
            tr!("dim-style", "precision"),
            precisions.clone(),
            pick(&precisions, &d.dimaltd.to_string()),
            "dimaltd",
            on
        ),
        number(tr!("dim-style", "alt-multiplier"), e, "dimaltf", on),
        number(tr!("dim-style", "alt-round"), e, "dimaltrnd", on),
        number(tr!("dim-style", "prefix"), e, "alt_prefix", on),
        number(tr!("dim-style", "suffix"), e, "alt_suffix", on),
    ]
    .spacing(8);
    let zeros = zero_group(
        e,
        d.dimaltz,
        ["dimaltz.lead", "dimaltz.trail", "dimaltz.feet", "dimaltz.inch"],
        kind != 3,
        matches!(kind, 2 | 3),
        on,
        Some(("dimaltmzf", "dimaltmzs")),
    );
    let below = u8::from(alt_below(d));
    let placement = column![
        option(tr!("dim-style", "alt-after"), 0, below, "alt_place", on),
        option(tr!("dim-style", "alt-below"), 1, below, "alt_place", on),
    ]
    .spacing(6);
    page(
        vec![
            switch(tr!("dim-style", "display-alt"), on, "dimalt", true),
            group(tr!("dim-style", "group-alternate"), alternate),
            group(tr!("dim-style", "group-zero"), zeros),
        ],
        vec![group(tr!("dim-style", "group-placement"), placement)],
    )
}

/// Tolerance method of DIMTOL / DIMLIM / DIMTP / DIMTM / DIMGAP: 0 none,
/// 1 symmetrical, 2 deviation, 3 limits, 4 basic.
pub fn tolerance_method(draft: &DimStyle) -> u8 {
    if draft.dimlim {
        3
    } else if draft.dimtol && (draft.dimtp - draft.dimtm).abs() < 1e-12 {
        1
    } else if draft.dimtol {
        2
    } else if draft.dimgap < 0.0 {
        4
    } else {
        0
    }
}

fn tolerances_tab<'a>(e: &'a Edit) -> Page<'a> {
    let d = &e.draft;
    let method = tolerance_method(d);
    let methods = choices(&[
        ("0", tr!("dim-style", "tol-none")),
        ("1", tr!("dim-style", "tol-symmetrical")),
        ("2", tr!("dim-style", "tol-deviation")),
        ("3", tr!("dim-style", "tol-limits")),
        ("4", tr!("dim-style", "tol-basic")),
    ]);
    let values = matches!(method, 1..=3);
    let precisions = precision_choices(linear_precisions(d.dimlunit, false));
    let vertical = choices(&[
        ("0", tr!("dim-style", "tol-bottom")),
        ("1", tr!("dim-style", "tol-middle")),
        ("2", tr!("dim-style", "tol-top")),
    ]);
    let format = column![
        list_row(
            tr!("dim-style", "method"),
            methods.clone(),
            pick(&methods, &method.to_string()),
            "tol_method",
            true
        ),
        list_row(
            tr!("dim-style", "precision"),
            precisions.clone(),
            pick(&precisions, &d.dimtdec.to_string()),
            "dimtdec",
            values
        ),
        number(tr!("dim-style", "upper-value"), e, "dimtp", values),
        number(tr!("dim-style", "lower-value"), e, "dimtm", matches!(method, 2 | 3)),
        number(tr!("dim-style", "height-scaling"), e, "dimtfac", values),
        list_row(
            tr!("dim-style", "vertical-position"),
            vertical.clone(),
            pick(&vertical, &d.dimtolj.to_string()),
            "dimtolj",
            true
        ),
    ]
    .spacing(8);
    // DIMTALN 1 aligns the decimal separators, 0 the operational symbols.
    let taln = u8::from(e.dimtaln != 0);
    let stacked = matches!(method, 2 | 3);
    let alignment = column![
        option(tr!("dim-style", "align-decimal"), 1, taln, "dimtaln", stacked),
        option(tr!("dim-style", "align-symbols"), 0, taln, "dimtaln", stacked),
    ]
    .spacing(6);
    let kind = linear_kind(d.dimlunit, false);
    let zeros = zero_group(
        e,
        d.dimtzin,
        ["dimtzin.lead", "dimtzin.trail", "dimtzin.feet", "dimtzin.inch"],
        kind != 3,
        matches!(kind, 2 | 3),
        values,
        None,
    );
    let alt_on = d.dimalt && values;
    let alt_kind = linear_kind(d.dimaltu, true);
    let alt_prec = precision_choices(linear_precisions(d.dimaltu, true));
    let alt = column![
        list_row(
            tr!("dim-style", "precision"),
            alt_prec.clone(),
            pick(&alt_prec, &d.dimalttd.to_string()),
            "dimalttd",
            alt_on
        ),
        zero_group(
            e,
            d.dimalttz,
            ["dimalttz.lead", "dimalttz.trail", "dimalttz.feet", "dimalttz.inch"],
            alt_kind != 3,
            matches!(alt_kind, 2 | 3),
            alt_on,
            None,
        ),
    ]
    .spacing(8);
    page(
        vec![
            group(tr!("dim-style", "group-tol-format"), format),
            group(tr!("dim-style", "group-tol-alignment"), alignment),
            group(tr!("dim-style", "group-zero"), zeros),
        ],
        vec![group(tr!("dim-style", "group-alt-tolerance"), alt)],
    )
}

fn edit_view<'a>(v: &View<'a>) -> Element<'a, Message> {
    let Some(e) = v.dialog.edit.as_ref() else {
        return Space::new().into();
    };
    let tabs = [
        tr!("dim-style", "tab-lines"),
        tr!("dim-style", "tab-symbols"),
        tr!("dim-style", "tab-text"),
        tr!("dim-style", "tab-fit"),
        tr!("dim-style", "tab-primary"),
        tr!("dim-style", "tab-alternate"),
        tr!("dim-style", "tab-tolerances"),
    ];
    let tab_bar = container(
        iced::widget::Row::with_children(
            tabs.into_iter()
                .enumerate()
                .map(|(index, caption)| {
                    let on = e.tab == index as u8;
                    button(text(caption).size(10).width(Fill).align_x(iced::Center))
                        .on_press(msg(DsMsg::Tab(index as u8)))
                        .style(button_style(on))
                        .padding([5, 4])
                        .width(Fill)
                        .into()
                })
                .collect::<Vec<_>>(),
        )
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style);
    let (left_cards, right_cards) = match e.tab {
        0 => lines_tab(v, e),
        1 => symbols_tab(v, e, center_mode(e.draft.dimcen)),
        2 => text_tab(v, e),
        3 => fit_tab(e),
        4 => primary_tab(e),
        5 => alternate_tab(e),
        _ => tolerances_tab(e),
    };
    let left = scrollable(
        container(column(left_cards).spacing(10))
            .padding(iced::Padding {
                right: 10.0,
                ..Default::default()
            })
            .width(Fill),
    )
    .height(Fill)
    .width(Length::Fixed(COLUMN));
    let right = column![
        text(tr!("dim-style", "preview-of", name = e.name.clone()))
            .size(11)
            .style(muted_style),
        preview_well(
            Some(e.draft.clone()),
            PreviewArrows {
                first: v.names.first.clone(),
                second: v.names.second.clone(),
                leader: v.names.leader.clone(),
            },
            &v.preview,
            Length::Fixed(250.0)
        ),
        scrollable(column(right_cards).spacing(10)).height(Fill),
    ]
    .spacing(8)
    .width(Fill)
    .height(Fill);
    let body = column![tab_bar, row![left, right].spacing(14).height(Fill)]
        .spacing(10)
        .padding([12, 14])
        .height(Fill);
    let footer = row![
        help_button(),
        Space::new().width(Fill),
        dialog_button(tr!("dim-style", "cancel"), msg(DsMsg::Cancel), false),
        dialog_button(tr!("dim-style", "ok"), msg(DsMsg::Ok), true),
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

// ── Compare ─────────────────────────────────────────────────────────────────

fn compare_view<'a>(v: &View<'a>) -> Element<'a, Message> {
    let Some(c) = v.dialog.compare.as_ref() else {
        return Space::new().into();
    };
    let names: Vec<String> = v.styles.iter().map(|s| s.name.clone()).collect();
    let mut with: Vec<String> = vec![tr!("dim-style", "none-style")];
    with.extend(names.iter().cloned());
    let first = crate::ui::dropdown_list::pick_list(Some(c.first.clone()), names, |s| s.clone())
        .on_select(|s| msg(DsMsg::CompareFirst(s)))
        .text_size(11)
        .width(Length::Fixed(300.0));
    let second_shown = c.second.clone().unwrap_or_else(|| tr!("dim-style", "none-style"));
    let second = crate::ui::dropdown_list::pick_list(Some(second_shown), with, |s| s.clone())
        .on_select(|s| msg(DsMsg::CompareSecond(s)))
        .text_size(11)
        .width(Length::Fixed(300.0));
    let two = c.second.as_ref().is_some_and(|s| *s != c.first);
    let heading = if !two {
        tr!("dim-style", "compare-all", name = c.first.clone())
    } else if v.compare_rows.is_empty() {
        tr!("dim-style", "compare-identical")
    } else {
        tr!("dim-style", "compare-found", count = v.compare_rows.len())
    };
    let cell = |value: String, width: f32| {
        text(value)
            .size(11)
            .width(Length::Fixed(width))
            .wrapping(iced::advanced::text::Wrapping::None)
    };
    let mut header = row![
        cell(tr!("dim-style", "col-description"), 210.0).style(muted_style),
        cell(tr!("dim-style", "col-variable"), 140.0).style(muted_style),
        cell(c.first.clone(), 150.0).style(muted_style),
    ]
    .spacing(8)
    .padding([0, 8]);
    if two {
        header = header.push(cell(c.second.clone().unwrap_or_default(), 150.0).style(muted_style));
    }
    let rows: Vec<Element<'a, Message>> = v
        .compare_rows
        .iter()
        .map(|r| {
            let mut line = row![
                cell(r.description.clone(), 210.0),
                cell(r.variable.to_string(), 140.0),
                cell(r.first.clone(), 150.0),
            ]
            .spacing(8);
            if two {
                line = line.push(cell(r.second.clone().unwrap_or_default(), 150.0));
            }
            container(line).padding([3, 8]).into()
        })
        .collect();
    let table = container(scrollable(column(rows).spacing(1)).height(Fill))
        .padding(2)
        .width(Fill)
        .height(Fill)
        .style(preview_style);
    let body = column![
        row![
            text(tr!("dim-style", "compare-label")).size(11).style(muted_style).width(70),
            first
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(tr!("dim-style", "with-label")).size(11).style(muted_style).width(70),
            second
        ]
        .spacing(8)
        .align_y(iced::Center),
        crate::ui::style::form::hdivider(Fill),
        row![
            text(heading).size(12).width(Fill),
            button(text(tr!("dim-style", "copy")).size(11))
                .on_press(msg(DsMsg::CompareCopy))
                .style(button_style(false))
                .padding([4, 10]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        header,
        table,
    ]
    .spacing(8)
    .padding([12, 14])
    .height(Fill);
    let footer = row![
        help_button(),
        Space::new().width(Fill),
        dialog_button(tr!("dim-style", "close"), msg(DsMsg::CompareClose), true),
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

fn preview_well<'a>(
    style: Option<DimStyle>,
    arrows: PreviewArrows,
    texts: &PreviewTexts,
    height: Length,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = match style {
        Some(style) => canvas(DimPreview {
            arrows,
            style,
            texts: texts.clone(),
        })
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

/// The arrowhead names the preview draws at the first and second end and on
/// the leader.
#[derive(Clone, Default)]
pub struct PreviewArrows {
    pub first: String,
    pub second: String,
    pub leader: String,
}

struct DimPreview {
    style: DimStyle,
    arrows: PreviewArrows,
    texts: PreviewTexts,
}

fn aci_rgb(index: i16, true_color: Option<AcadColor>, fallback: Color) -> Color {
    style_color(index, true_color)
        .rgb()
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(fallback)
}

impl canvas::Program<Message> for DimPreview {
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
        let s = &self.style;
        let ink = theme.palette().background.base.text;
        let object = Color::from_rgb8(0xC8, 0x9B, 0x3C);
        let dim_color = aci_rgb(s.dimclrd, s.dimclrd_true_color, ink);
        let ext_color = aci_rgb(s.dimclre, s.dimclre_true_color, ink);
        let text_color = aci_rgb(s.dimclrt, s.dimclrt_true_color, ink);
        // The sample part is drawn to the style's text height, so the text
        // keeps its size in the well whatever the style's units.
        let k = (s.dimtxt.abs().max(1e-9) / 2.5) as f32;
        // The sample spans x -6..56, y 0..38 (nominal units).
        let (min_x, span_x, span_y) = (-6.0_f32, 62.0_f32, 38.0_f32);
        let px = (bounds.width / span_x).min(bounds.height / span_y) / k;
        let ox = (bounds.width - span_x * k * px) * 0.5 - min_x * k * px;
        let oy = (bounds.height - span_y * k * px) * 0.5;
        let at = |x: f32, y: f32| Point::new(ox + x * k * px, bounds.height - (oy + y * k * px));
        let sz = |v: f64| (v.abs() as f32) * px;
        let thin = |c: Color| canvas::Stroke::default().with_color(c).with_width(1.0);
        // The part.
        let outline = [
            (12.0, 10.0),
            (42.0, 10.0),
            (32.0, 24.0),
            (22.0, 24.0),
            (22.0, 28.0),
            (12.0, 28.0),
        ];
        frame.stroke(
            &canvas::Path::new(|p| {
                p.move_to(at(outline[0].0, outline[0].1));
                for &(x, y) in &outline[1..] {
                    p.line_to(at(x, y));
                }
                p.close();
            }),
            thin(object),
        );
        let hole = (18.0, 17.0, 3.5);
        frame.stroke(
            &canvas::Path::circle(at(hole.0, hole.1), hole.2 * k * px),
            thin(object),
        );

        let text_h = sz(s.dimtxt).max(4.0);
        let gap = sz(s.dimgap);
        let arrow = sz(s.dimasz);
        let kind_of = |name: &str| arrow_from_block_name(Some(crate::entities::arrowheads::block_name(name)), arrow);
        let first = kind_of(&self.arrows.first);
        let second = if s.dimsah { kind_of(&self.arrows.second) } else { kind_of(&self.arrows.first) };
        let tick = s.dimtsz > 0.0;
        let draw_text = |frame: &mut canvas::Frame, content: &str, at_p: Point, angle: f32| {
            let width = content.chars().count() as f32 * text_h * 0.6;
            if s.dimtfill != 0 || s.dimgap < 0.0 {
                let pad = gap.max(1.0);
                let rect = |frame: &mut canvas::Frame, fill: bool, color: Color| {
                    frame.with_save(|f| {
                        f.translate(iced::Vector::new(at_p.x, at_p.y));
                        f.rotate(angle);
                        let path = canvas::Path::rectangle(
                            Point::new(-width * 0.5 - pad, -text_h * 0.5 - pad),
                            iced::Size::new(width + pad * 2.0, text_h + pad * 2.0),
                        );
                        if fill {
                            f.fill(&path, color);
                        } else {
                            f.stroke(&path, thin(color));
                        }
                    });
                };
                if s.dimtfill == 1 {
                    rect(frame, true, theme.palette().background.weak.color);
                } else if s.dimtfill == 2 {
                    rect(frame, true, aci_rgb(s.dimtfillclr, s.dimtfillclr_true_color, ink));
                }
                if s.dimgap < 0.0 {
                    rect(frame, false, dim_color);
                }
            }
            frame.with_save(|f| {
                f.translate(iced::Vector::new(at_p.x, at_p.y));
                f.rotate(angle);
                f.fill_text(canvas::Text {
                    content: content.to_string(),
                    position: Point::ORIGIN,
                    color: text_color,
                    size: iced::Pixels(text_h * 1.25),
                    align_x: iced::advanced::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            });
            width
        };
        let draw_arrow = |frame: &mut canvas::Frame, tip: Point, dir: (f32, f32), kind: &ArrowKind| {
            // `dir` points from the tip back along the dimension line.
            let (dx, dy) = dir;
            let map = |a: f32, b: f32| Point::new(tip.x + dx * a - dy * b, tip.y + dy * a + dx * b);
            if tick {
                let t = sz(s.dimtsz);
                frame.stroke(&canvas::Path::line(map(-t * 0.7, t * 0.7), map(t * 0.7, -t * 0.7)), thin(dim_color).with_width(1.6));
                return;
            }
            match kind {
                ArrowKind::None => {}
                ArrowKind::Triangle { size, filled, size_mul } => {
                    let a = size * size_mul;
                    let path = canvas::Path::new(|p| {
                        p.move_to(map(0.0, 0.0));
                        p.line_to(map(a, a / 6.0));
                        p.line_to(map(a, -a / 6.0));
                        p.close();
                    });
                    if *filled {
                        frame.fill(&path, dim_color);
                    } else {
                        frame.stroke(&path, thin(dim_color));
                    }
                }
                ArrowKind::Open { size, half_angle } => {
                    let b = size * half_angle.tan();
                    frame.stroke(&canvas::Path::line(map(0.0, 0.0), map(*size, b)), thin(dim_color));
                    frame.stroke(&canvas::Path::line(map(0.0, 0.0), map(*size, -b)), thin(dim_color));
                }
                ArrowKind::Dot { size, filled } | ArrowKind::Box_ { size, filled } => {
                    let r = size * 0.5;
                    let path = if matches!(kind, ArrowKind::Dot { .. }) {
                        canvas::Path::circle(tip, r)
                    } else {
                        canvas::Path::new(|p| {
                            p.move_to(map(-r, -r));
                            p.line_to(map(r, -r));
                            p.line_to(map(r, r));
                            p.line_to(map(-r, r));
                            p.close();
                        })
                    };
                    if *filled {
                        frame.fill(&path, dim_color);
                    } else {
                        frame.stroke(&path, thin(dim_color));
                    }
                }
                ArrowKind::Tick { size, width } => {
                    frame.stroke(
                        &canvas::Path::line(map(-size, *size), map(*size, -size)),
                        thin(dim_color).with_width(if *width > 0.0 { 2.4 } else { 1.0 }),
                    );
                }
                ArrowKind::Origin { size, .. } => {
                    frame.stroke(&canvas::Path::circle(tip, size * 0.5), thin(dim_color));
                }
                ArrowKind::Datum { size, filled } => {
                    let path = canvas::Path::new(|p| {
                        p.move_to(map(0.0, size * 0.58));
                        p.line_to(map(0.0, -size * 0.58));
                        p.line_to(map(*size, 0.0));
                        p.close();
                    });
                    if *filled {
                        frame.fill(&path, dim_color);
                    } else {
                        frame.stroke(&path, thin(dim_color));
                    }
                }
                ArrowKind::Custom { size, .. } => {
                    frame.stroke(&canvas::Path::circle(map(size * 0.5, 0.0), size * 0.5), thin(dim_color));
                }
            }
        };
        // A linear dimension between two points of the part, its line
        // `offset` away along the unit normal `n`.
        let linear = |frame: &mut canvas::Frame, a: (f32, f32), b: (f32, f32), n: (f32, f32), offset: f32, content: &str| {
            let pa = at(a.0, a.1);
            let pb = at(b.0, b.1);
            let nn = (n.0, -n.1);
            let off = offset * k * px;
            let la = Point::new(pa.x + nn.0 * off, pa.y + nn.1 * off);
            let lb = Point::new(pb.x + nn.0 * off, pb.y + nn.1 * off);
            let exe = sz(s.dimexe);
            let exo = sz(s.dimexo);
            for (on, p, l) in [(!s.dimse1, pa, la), (!s.dimse2, pb, lb)] {
                if !on {
                    continue;
                }
                let start = if s.dimfxlon {
                    let len = sz(s.dimfxl);
                    Point::new(l.x - nn.0 * len, l.y - nn.1 * len)
                } else {
                    Point::new(p.x + nn.0 * exo, p.y + nn.1 * exo)
                };
                let end = Point::new(l.x + nn.0 * exe, l.y + nn.1 * exe);
                frame.stroke(&canvas::Path::line(start, end), thin(ext_color));
            }
            let len = ((lb.x - la.x).powi(2) + (lb.y - la.y).powi(2)).sqrt().max(1.0);
            let u = ((lb.x - la.x) / len, (lb.y - la.y) / len);
            let mid = Point::new((la.x + lb.x) * 0.5, (la.y + lb.y) * 0.5);
            let mut angle = u.1.atan2(u.0);
            if angle > std::f32::consts::FRAC_PI_2 || angle < -std::f32::consts::FRAC_PI_2 {
                angle += std::f32::consts::PI;
            }
            let horizontal_text = s.dimtih;
            let text_angle = if horizontal_text { 0.0 } else { angle };
            let width = content.chars().count() as f32 * text_h * 0.6;
            let above = s.dimtad != 0;
            let lift = if above { text_h * 0.5 + gap.max(1.0) } else { 0.0 };
            // Text above the line sits on its outer side.
            let text_at = Point::new(mid.x + nn.0 * lift, mid.y + nn.1 * lift);
            let break_half = if above { 0.0 } else { width * 0.5 + gap.max(2.0) };
            let ext_d = if tick { sz(s.dimdle) } else { 0.0 };
            let p0 = Point::new(la.x - u.0 * ext_d, la.y - u.1 * ext_d);
            let p3 = Point::new(lb.x + u.0 * ext_d, lb.y + u.1 * ext_d);
            let m1 = Point::new(mid.x - u.0 * break_half, mid.y - u.1 * break_half);
            let m2 = Point::new(mid.x + u.0 * break_half, mid.y + u.1 * break_half);
            if !s.dimsd1 {
                frame.stroke(&canvas::Path::line(p0, m1), thin(dim_color));
            }
            if !s.dimsd2 {
                frame.stroke(&canvas::Path::line(m2, p3), thin(dim_color));
            }
            draw_arrow(frame, la, u, &first);
            draw_arrow(frame, lb, (-u.0, -u.1), &second);
            draw_text(frame, content, text_at, text_angle);
        };
        // A family member's preview leaves out the dimensions it is not
        // for (their texts are empty).
        if !self.texts.horizontal.is_empty() {
            linear(&mut frame, (12.0, 28.0), (22.0, 28.0), (0.0, 1.0), 4.0, &self.texts.horizontal);
            linear(&mut frame, (12.0, 10.0), (12.0, 28.0), (-1.0, 0.0), 6.0, &self.texts.vertical);
            // The slanted edge, dimensioned on its outer side.
            let edge_len = (10.0_f32 * 10.0 + 14.0 * 14.0).sqrt();
            linear(&mut frame, (42.0, 10.0), (32.0, 24.0), (14.0 / edge_len, 10.0 / edge_len), 5.0, &self.texts.aligned);
        }
        // Radius: from the hole's centre out through its edge and past the
        // part's corner, the text at the leader's end.
        let c = at(hole.0, hole.1);
        let r = hole.2 * k * px;
        if !self.texts.radius.is_empty() {
            let dir = (-std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
            let edge = Point::new(c.x + dir.0 * r, c.y + dir.1 * r);
            let reach = r + 9.0 * k * px;
            let out = Point::new(c.x + dir.0 * reach, c.y + dir.1 * reach);
            frame.stroke(&canvas::Path::line(c, out), thin(dim_color));
            let leader = kind_of(&self.arrows.first);
            draw_arrow(&mut frame, edge, (-dir.0, -dir.1), &leader);
            let radius_width = self.texts.radius.chars().count() as f32 * text_h * 0.6;
            draw_text(
                &mut frame,
                &self.texts.radius,
                Point::new(
                    (out.x - radius_width * 0.5).max(radius_width * 0.5 + 2.0),
                    out.y + text_h * 0.9,
                ),
                0.0,
            );
        }
        // The angle between the bottom edge and the slanted edge.
        if !self.texts.angle.is_empty() {
            let v0 = at(42.0, 10.0);
            let ar = 9.0 * k * px;
            let a1 = 14.0_f32.atan2(-10.0);
            let a2 = std::f32::consts::PI;
            frame.stroke(
                &canvas::Path::new(|p| {
                    p.arc(canvas::path::Arc {
                        center: v0,
                        radius: ar,
                        start_angle: iced::Radians(-a2),
                        end_angle: iced::Radians(-a1),
                    });
                }),
                thin(dim_color),
            );
            let e1 = Point::new(v0.x + ar * a1.cos(), v0.y - ar * a1.sin());
            let e2 = Point::new(v0.x - ar, v0.y);
            // Each arrow points along the arc, away from the other end.
            draw_arrow(&mut frame, e1, (-a1.sin(), -a1.cos()), &first);
            draw_arrow(&mut frame, e2, (0.0, -1.0), &second);
            let am = (a1 + a2) * 0.5;
            let inner = ar - text_h * 1.6;
            draw_text(
                &mut frame,
                &self.texts.angle,
                Point::new(v0.x + inner * am.cos(), v0.y - inner * am.sin()),
                0.0,
            );
        }
        vec![frame.into_geometry()]
    }
}
