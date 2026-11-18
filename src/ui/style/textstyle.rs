//! Text style manager: the style's properties in three groups (Font, Size,
//! Effects), and beside them what Apply does to the texts, a preview drawn
//! as the drawing draws the style, and where the style is used (#2413). In a
//! narrow window the two columns stack, the Apply summary first.

use crate::app::text_style_users::Usage;
use crate::app::{Message, StyleKind};
use crate::t;
use crate::ui::font_picker::{FaceStyle, FontChoice, FontPickerState};
use crate::ui::style::common::muted_style;
use crate::ui::style::editor_table::{danger_input_style, danger_text_style};
use crate::ui::style::form::field_style;
use crate::ui::style::style_manager::StyleRow;
use iced::widget::{
    canvas, checkbox, column, container, mouse_area, responsive, row, scrollable, stack, text,
    text_input, Space,
};
use iced::{mouse, Background, Element, Length, Point, Rectangle, Theme};
use std::borrow::Cow;
use std::rc::Rc;

pub struct TextStyleView<'a> {
    /// The listed styles (all, or those in use) and what each row shows.
    pub styles: Vec<String>,
    pub rows: Vec<StyleRow>,
    pub in_use_only: bool,
    pub selected: &'a str,
    pub current: &'a str,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub read_only: bool,
    /// The texts that use the selected style, by kind.
    pub usage: Usage,
    pub font_buf: &'a str,
    pub width_buf: &'a str,
    pub oblique_buf: &'a str,
    pub height_buf: &'a str,
    pub bigfont_buf: &'a str,
    pub ttf_buf: &'a str,
    /// Why each number field's text cannot be applied, if it cannot.
    pub width_error: Option<Cow<'static, str>>,
    pub oblique_error: Option<Cow<'static, str>>,
    pub height_error: Option<Cow<'static, str>>,
    /// The stored value of each number field the user changed.
    pub width_was: Option<String>,
    pub oblique_was: Option<String>,
    pub height_was: Option<String>,
    pub invalid_fields: usize,
    pub use_big_font: bool,
    pub font_picker: &'a FontPickerState,
    pub bigfont_picker: &'a FontPickerState,
    /// What the last font switch turned off.
    pub turned_off: Option<String>,
    /// What Apply does to every changed style.
    pub apply_summary: ApplySummary,
    pub preview: Preview,
    pub sample: &'a str,
    /// The TrueType font style (typeface flags).
    pub bold: bool,
    pub italic: bool,
    pub backward: bool,
    pub upside_down: bool,
    pub vertical: bool,
    pub annotative: bool,
    /// "Match text orientation to layout", offered with Annotative on.
    pub orientation: bool,
    /// The window holds a change Apply would write.
    pub modified: bool,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    /// A question or notice over the window.
    pub card: Option<&'a Card>,
    /// The style whose actions (right click) show under it.
    pub context: Option<&'a str>,
}

/// What a question about unsaved changes was asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Select(String),
    SetCurrent,
    New,
    Copy,
    Delete,
    Rename(String),
    Close,
}

/// Why a name was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum NameNotice {
    Empty,
    Exists(String),
    UnsupportedCharacters,
}

/// A card over the text style window.
#[derive(Debug, Clone, PartialEq)]
pub enum Card {
    /// New: the name being typed, and a refusal over it.
    New {
        name: String,
        notice: Option<NameNotice>,
    },
    /// A refused rename.
    Name(NameNotice),
    /// Delete the named style?
    DeleteAsk(String),
    /// The selected style is used by texts and stays.
    InUse,
    /// The selected style changed: keep the changes before going on?
    Modified(Pending),
}

/// What a card's controls send.
#[derive(Debug, Clone)]
pub enum CardMsg {
    NewName(String),
    NewOk,
    NewCancel,
    NoticeClose,
    DeleteAnswer(bool),
    ModifiedAnswer(bool),
}

/// The characters a style name may not hold.
pub const UNSUPPORTED_NAME_CHARACTERS: &str = "\\ < > / ? \" : ; * | , = `";

/// Why `name` (as typed) cannot name a style; `exists` tells whether the
/// trimmed name is taken.
pub fn name_notice(name: &str, exists: impl Fn(&str) -> bool) -> Option<NameNotice> {
    let trimmed = name.trim();
    if name.is_empty() {
        Some(NameNotice::Empty)
    } else if trimmed.is_empty()
        || trimmed.chars().any(|c| {
            matches!(
                c,
                '\\' | '<' | '>' | '/' | '?' | '"' | ':' | ';' | '*' | '|' | ',' | '=' | '`'
            )
        })
    {
        Some(NameNotice::UnsupportedCharacters)
    } else if exists(trimmed) {
        Some(NameNotice::Exists(trimmed.to_string()))
    } else {
        None
    }
}

/// What Apply changes: each changed style's name with its lines, and how many
/// properties change in all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplySummary {
    pub styles: Vec<(String, Vec<String>)>,
    pub properties: usize,
}

/// What the preview draws: the draft resolved as the drawing resolves a
/// style (the SHX file on disk, its big font, the TrueType face).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub font: String,
    pub width_factor: f32,
    /// Radians.
    pub oblique: f32,
    pub upside_down: bool,
    pub vertical: bool,
    pub text: String,
    /// "simplex.shx + gbcbig.shx · Drawing height 3.5"
    pub caption: String,
}

/// The default sample: a few letters and digits, and characters a big font
/// draws when there is one.
pub fn default_sample(big_font: bool) -> &'static str {
    if big_font {
        "AaBb 0123 漢字"
    } else {
        "AaBb 0123"
    }
}

type Outline = (Vec<Vec<[f32; 2]>>, Vec<[f32; 2]>);

thread_local! {
    /// The last preview's strokes and fill triangles: a window redraw or a
    /// change elsewhere in the window does not shape the text again.
    static OUTLINE: std::cell::RefCell<Option<(Preview, Rc<Outline>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The strokes and fill triangles of `preview`, at a nominal height of 10.
fn outline(preview: &Preview) -> Rc<Outline> {
    if let Some(hit) = OUTLINE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .filter(|(key, _)| key == preview)
            .map(|(_, outline)| outline.clone())
    }) {
        return hit;
    }
    const HEIGHT: f32 = 10.0;
    let rotation = if preview.upside_down {
        std::f32::consts::PI
    } else {
        0.0
    };
    let draw = |origin: [f32; 2], text: &str| {
        crate::scene::text::lff::tessellate_text_ex(
            origin,
            HEIGHT,
            rotation,
            preview.width_factor,
            preview.oblique,
            &preview.font,
            text,
        )
    };
    let mut outline: Outline = (Vec::new(), Vec::new());
    if preview.vertical {
        // Vertical text stacks its characters downwards.
        for (index, character) in preview.text.chars().filter(|c| *c != ' ').enumerate() {
            let (mut strokes, mut fills) = draw(
                [0.0, -(index as f32) * HEIGHT * 1.25],
                &character.to_string(),
            );
            outline.0.append(&mut strokes);
            outline.1.append(&mut fills);
        }
    } else {
        outline = draw([0.0, 0.0], &preview.text);
    }
    let outline = Rc::new(outline);
    OUTLINE.with(|cache| *cache.borrow_mut() = Some((preview.clone(), outline.clone())));
    outline
}

struct PreviewCanvas {
    outline: Rc<Outline>,
}

/// The empty space the preview keeps around the text on each side.
const PREVIEW_MARGIN: f32 = 16.0;

impl canvas::Program<Message> for PreviewCanvas {
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
        let (strokes, fills) = &*self.outline;
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for &[x, y] in strokes.iter().flatten().chain(fills) {
            min = [min[0].min(x), min[1].min(y)];
            max = [max[0].max(x), max[1].max(y)];
        }
        if min[0] <= max[0] {
            let span = [(max[0] - min[0]).max(1.0), (max[1] - min[1]).max(1.0)];
            let available = [
                bounds.width - 2.0 * PREVIEW_MARGIN,
                bounds.height - 2.0 * PREVIEW_MARGIN,
            ];
            let scale = (available[0] / span[0])
                .min(available[1] / span[1])
                .clamp(0.0, 6.0);
            let middle = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
            let center = [bounds.width * 0.5, bounds.height * 0.5];
            let map = |[x, y]: [f32; 2]| {
                Point::new(
                    center[0] + (x - middle[0]) * scale,
                    center[1] - (y - middle[1]) * scale,
                )
            };
            for stroke in strokes.iter().filter(|s| s.len() >= 2) {
                let path = canvas::Path::new(|builder| {
                    builder.move_to(map(stroke[0]));
                    for &point in &stroke[1..] {
                        builder.line_to(map(point));
                    }
                });
                frame.stroke(
                    &path,
                    canvas::Stroke::default().with_color(ink).with_width(1.2),
                );
            }
            if fills.len() >= 3 {
                let path = canvas::Path::new(|builder| {
                    for triangle in fills.chunks_exact(3) {
                        builder.move_to(map(triangle[0]));
                        builder.line_to(map(triangle[1]));
                        builder.line_to(map(triangle[2]));
                        builder.close();
                    }
                });
                frame.fill(&path, ink);
            }
        }
        vec![frame.into_geometry()]
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

/// A caption and its control on one line, the controls lined up.
fn labelled_row<'a>(
    label: Cow<'static, str>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        text(label).size(11).width(LABEL_W),
        container(control.into()).width(Length::Fill)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

const LABEL_W: f32 = 80.0;

/// A line of guidance under a control.
fn hint<'a>(line: Cow<'static, str>) -> Element<'a, Message> {
    text(line)
        .size(10)
        .style(muted_style)
        .width(Length::Fill)
        .into()
}

/// A number field. With an `error` it turns red and the reason shows under
/// it; a changed value says what is stored (`was`).
fn number_row<'a>(
    label: Cow<'static, str>,
    placeholder: &'static str,
    value: &'a str,
    field: &'static str,
    read_only: bool,
    error: Option<Cow<'static, str>>,
    was: Option<String>,
) -> Element<'a, Message> {
    let control: Element<'a, Message> = if read_only {
        crate::ui::read_only::field(value, 11.0, Length::Fill)
    } else {
        text_input(placeholder, value)
            .on_input(move |value| Message::TextStyleEdit { field, value })
            .style(if error.is_some() {
                danger_input_style
            } else {
                field_style
            })
            .size(11)
            .width(Length::Fill)
            .into()
    };
    let line = labelled_row(label, control);
    let note = match (error, was) {
        (Some(error), _) => Some(
            text(error)
                .size(10)
                .style(danger_text_style)
                .width(Length::Fill),
        ),
        (None, Some(value)) => Some(
            text(crate::tf!("Modified · was {value}"))
                .size(10)
                .style(primary_style)
                .width(Length::Fill),
        ),
        (None, None) => None,
    };
    match note {
        Some(note) => column![line, row![Space::new().width(LABEL_W + 8.0), note]]
            .spacing(3)
            .into(),
        None => line,
    }
}

fn toggle<'a>(
    label: Cow<'static, str>,
    value: bool,
    field: &'static str,
    read_only: bool,
) -> Element<'a, Message> {
    let checkbox = checkbox(value).label(label).size(14).text_size(11);
    if read_only {
        checkbox.into()
    } else {
        checkbox
            .on_toggle(move |_| Message::TextStyleToggle(field))
            .into()
    }
}

/// A switch shown as a chip.
fn toggle_chip<'a>(
    label: Cow<'static, str>,
    value: bool,
    field: &'static str,
    read_only: bool,
) -> Element<'a, Message> {
    crate::ui::window::pdf_dialogs::chip_enabled(
        label.into_owned(),
        value,
        Message::TextStyleToggle(field),
        !read_only,
    )
}

/// A titled card of rows.
fn group<'a>(title: Cow<'static, str>, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    crate::ui::window::pdf_dialogs::card(title.into_owned(), column(rows).spacing(8))
}

/// A fixed badge: what differs, how many texts use the style.
fn info_chip<'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(11))
        .padding([5, 10])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: iced::Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// The setting cards: font, size and effects.
fn properties<'a>(v: &TextStyleView<'a>) -> Vec<Element<'a, Message>> {
    let ttf = !v.ttf_buf.trim().is_empty();
    let face = FaceStyle::of(v.bold, v.italic);
    let face_style: Element<'a, Message> = if ttf && !v.read_only {
        crate::ui::dropdown_list::pick_list(
            Some(face),
            FaceStyle::offered(v.ttf_buf, face),
            |face: &FaceStyle| face.to_string(),
        )
        .on_select(Message::TextStyleFaceStyle)
        .text_size(11)
        .width(Length::Fill)
        .into()
    } else if ttf {
        crate::ui::read_only::field(&face.to_string(), 11.0, Length::Fill)
    } else {
        // A stroke font has no bold or italic face.
        column![
            crate::ui::read_only::field(&face.to_string(), 11.0, Length::Fill),
            text(t!("TrueType fonts only")).size(10).style(muted_style),
        ]
        .spacing(3)
        .into()
    };
    let mut font_rows = vec![labelled_row(
        t!("Name"),
        crate::ui::font_picker::view(crate::ui::font_picker::FontPicker {
            state: v.font_picker,
            selected: if ttf {
                FontChoice::TrueType(v.ttf_buf.to_string())
            } else {
                FontChoice::Stroke(v.font_buf.to_string())
            },
            file: v.font_buf.to_string(),
            width: Length::Fill,
            read_only: v.read_only,
            on_event: Message::TextStyleFont,
        }),
    )];
    if let Some(turned_off) = &v.turned_off {
        font_rows.push(
            container(text(turned_off.clone()).size(11))
                .padding([6, 10])
                .width(Length::Fill)
                .style(note_style)
                .into(),
        );
    }
    font_rows.extend([
        labelled_row(t!("Style"), face_style),
        toggle(
            t!("Use SHX Big Font"),
            v.use_big_font,
            "use_big_font",
            v.read_only || ttf,
        ),
    ]);
    if v.use_big_font {
        font_rows.push(labelled_row(
            t!("Big Font file"),
            crate::ui::font_picker::view(crate::ui::font_picker::FontPicker {
                state: v.bigfont_picker,
                selected: FontChoice::Stroke(v.bigfont_buf.to_string()),
                file: v.bigfont_buf.to_string(),
                width: Length::Fill,
                read_only: v.read_only,
                on_event: Message::TextStyleBigFont,
            }),
        ));
    }
    let font = group(t!("Font"), font_rows);
    let size = group(
        t!("Size"),
        vec![
            number_row(
                if v.annotative {
                    t!("Paper text height")
                } else {
                    t!("Height")
                },
                "0",
                v.height_buf,
                "height",
                v.read_only,
                v.height_error.clone(),
                v.height_was.clone(),
            ),
            hint(t!("0 = the height is asked for when the text is placed.")),
            toggle(t!("Annotative"), v.annotative, "annotative", v.read_only),
            row![
                Space::new().width(18),
                toggle(
                    crate::tr!("text-style", "orientation").into(),
                    v.orientation,
                    "orientation",
                    v.read_only || !v.annotative,
                ),
            ]
            .into(),
        ],
    );
    let effects = group(
        t!("Effects"),
        vec![
            number_row(
                t!("Width"),
                "1",
                v.width_buf,
                "width",
                v.read_only,
                v.width_error.clone(),
                v.width_was.clone(),
            ),
            number_row(
                t!("Oblique"),
                "0",
                v.oblique_buf,
                "oblique",
                v.read_only,
                v.oblique_error.clone(),
                v.oblique_was.clone(),
            ),
            row![
                toggle_chip(t!("Backward"), v.backward, "backward", v.read_only),
                toggle_chip(t!("Upside down"), v.upside_down, "upside_down", v.read_only),
                toggle_chip(t!("Vertical"), v.vertical, "vertical", v.read_only || ttf),
            ]
            .spacing(6)
            .wrap()
            .vertical_spacing(6)
            .into(),
        ],
    );
    vec![font, size, effects]
}

/// What Apply does to every changed style and its texts, framed; nothing
/// when Apply changes nothing. The title, the counts and the closing note
/// stay put while the styles scroll, at most `height` tall.
fn apply_summary<'a>(v: &TextStyleView<'a>, height: f32) -> Option<Element<'a, Message>> {
    let summary = &v.apply_summary;
    if summary.styles.is_empty() {
        return None;
    }
    let (styles, properties) = (summary.styles.len(), summary.properties);
    let counts = match (styles, properties) {
        (1, 1) => t!("1 style, 1 property"),
        (1, _) => crate::tf!("1 style, {properties} properties"),
        _ => crate::tf!("{styles} styles, {properties} properties"),
    };
    let groups = summary.styles.iter().map(|(name, lines)| {
        column![text(name.clone()).size(11).style(primary_style)]
            .extend(
                lines
                    .iter()
                    .map(|line| text(line.clone()).size(11).width(Length::Fill).into()),
            )
            .spacing(3)
            .into()
    });
    let body = scrollable(
        container(column(groups).spacing(9))
            .padding(iced::Padding::default().right(12.0))
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fit.max(height));
    Some(
        container(
            column![
                row![
                    text(t!("Apply will change")).size(11).style(primary_style),
                    text(counts).size(10).style(muted_style),
                ]
                .spacing(8)
                .align_y(iced::Center),
                body,
                text(t!(
                    "Texts in external references do not change. A single Undo reverts every change."
                ))
                .size(10)
                .style(muted_style),
            ]
            .spacing(6),
        )
        .padding([9, 10])
        .width(Length::Fill)
        .style(note_style)
        .into(),
    )
}

/// A framed note in the accent colour: what Apply does, what a font switch
/// turned off.
fn note_style(theme: &Theme) -> container::Style {
    let primary = theme.palette().primary;
    container::Style {
        background: Some(primary.weak.color.scale_alpha(0.25).into()),
        border: iced::Border {
            color: primary.base.color.scale_alpha(0.6),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

/// The top card: the style's name and the comparison, the preview, the
/// sample text and what differs / how many texts use the style.
fn preview_card<'a>(v: &TextStyleView<'a>) -> Element<'a, Message> {
    use crate::ui::window::pdf_dialogs::{accent_text, well_style};
    let count = v.usage.kinds.iter().map(|(_, count)| count).sum::<usize>();
    let status = if v.read_only {
        t!("Referenced style · Read only")
    } else if v.selected.eq_ignore_ascii_case(v.current) {
        t!("Current style")
    } else if count > 0 {
        t!("Used in drawing")
    } else {
        t!("Available style")
    };
    let mut head = row![
        text(v.selected.to_uppercase()).size(11).style(accent_text),
        text(status).size(10).style(muted_style),
        Space::new().width(Length::Fill),
    ]
    .spacing(10)
    .align_y(iced::Center);
    if !v.compare_opts.is_empty() {
        head = head.push(text(t!("Compare with")).size(11)).push(
            crate::ui::dropdown_list::pick_list(
                Some(v.compare_name.clone()),
                v.compare_opts.clone(),
                |value| value.to_string(),
            )
            .on_select(Message::TextStyleDialogCompare)
            .text_size(11)
            .width(150),
        );
    }
    let preview = container(
        canvas(PreviewCanvas {
            outline: outline(&v.preview),
        })
        .width(Length::Fill)
        .height(110),
    )
    .padding([5, 10])
    .width(Length::Fill)
    .style(well_style);
    let sample = text_input(default_sample(v.use_big_font), v.sample)
        .on_input(Message::TextStyleSample)
        .style(field_style)
        .size(11)
        .width(Length::Fill);
    let difference = if v.compare_opts.is_empty() {
        None
    } else if v.comparison_sections.is_empty() {
        Some(t!("No differences").into_owned())
    } else {
        Some(format!(
            "{}: {}",
            t!("Different"),
            v.comparison_sections.join(", ")
        ))
    };
    let mut line = row![sample].spacing(8).align_y(iced::Center);
    if let Some(difference) = difference {
        line = line.push(info_chip(difference));
    }
    let line = line.push(info_chip(crate::tr!("text-style", "used-count", count = count)));
    container(column![head, preview, line].spacing(8))
        .padding([12, 14])
        .width(Length::Fill)
        .style(crate::ui::window::pdf_dialogs::card_style)
        .into()
}

/// Below this width the two columns stack and the toolbar shows its icons
/// only.
const STACK_BELOW: f32 = 920.0;

pub fn view_window<'a>(
    v: TextStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let card = v.card.cloned();
    let window: Element<'a, Message> = responsive(move |size| layout(&v, sizing, size)).into();
    let Some(card) = card else {
        return window;
    };
    let (panel, dismiss) = card_view(card);
    stack([
        window,
        mouse_area(
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

fn card_msg(message: CardMsg) -> Message {
    Message::TextStyleCard(message)
}

const WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

/// A warning icon beside a title and its detail.
fn notice_lines<'a>(title: String, detail: Option<String>) -> Element<'a, Message> {
    let mut lines = column![text(title).size(13)].spacing(6);
    if let Some(detail) = detail {
        lines = lines.push(text(detail).size(11).style(muted_style));
    }
    row![crate::ui::icons::semantic(WARN, 20.0), lines]
        .spacing(12)
        .align_y(iced::Alignment::Start)
        .into()
}

pub(crate) fn name_notice_lines<'a>(notice: &NameNotice) -> Element<'a, Message> {
    let (title, detail) = match notice {
        NameNotice::Empty => (crate::tr!("text-style", "name-empty"), None),
        NameNotice::Exists(name) => (
            crate::i18n::plain(crate::tr!("text-style", "name-exists", name = name.clone())),
            Some(crate::tr!("text-style", "name-exists-detail")),
        ),
        NameNotice::UnsupportedCharacters => (
            crate::tr!("text-style", "name-chars"),
            Some(format!(
                "{}\n{UNSUPPORTED_NAME_CHARACTERS}",
                crate::tr!("text-style", "name-chars-detail")
            )),
        ),
    };
    notice_lines(title, detail)
}

fn close_row<'a>() -> Element<'a, Message> {
    row![
        Space::new().width(Length::Fill),
        crate::ui::style::form::dialog_button(
            crate::tr!("text-style", "close"),
            card_msg(CardMsg::NoticeClose),
            true
        ),
    ]
    .into()
}

/// The card's panel and what a click beside it does.
fn card_view<'a>(card: Card) -> (Element<'a, Message>, Message) {
    use crate::ui::style::form::dialog_button;
    use crate::ui::style::mleaderstyle::panel;
    match card {
        Card::New { name, notice } => {
            if let Some(notice) = notice {
                let body = column![name_notice_lines(&notice), close_row()].spacing(14);
                return (panel(body.into(), 420.0), card_msg(CardMsg::NoticeClose));
            }
            let field = text_input("", &name)
                .id(iced::widget::Id::new(NEW_NAME_ID))
                .on_input(|value| card_msg(CardMsg::NewName(value)))
                .on_submit(card_msg(CardMsg::NewOk))
                .size(12)
                .padding([5, 8])
                .style(field_style)
                .width(Length::Fill);
            let body = column![
                text(crate::tr!("text-style", "new-title"))
                    .size(13)
                    .style(crate::ui::window::pdf_dialogs::accent_text),
                column![
                    text(crate::tr!("text-style", "style-name"))
                        .size(11)
                        .style(muted_style),
                    field
                ]
                .spacing(4),
                row![
                    Space::new().width(Length::Fill),
                    dialog_button(
                        crate::tr!("text-style", "cancel"),
                        card_msg(CardMsg::NewCancel),
                        false
                    ),
                    dialog_button(crate::tr!("text-style", "ok"), card_msg(CardMsg::NewOk), true),
                ]
                .spacing(6),
            ]
            .spacing(12);
            (panel(body.into(), 380.0), card_msg(CardMsg::NewCancel))
        }
        Card::Name(notice) => {
            let body = column![name_notice_lines(&notice), close_row()].spacing(14);
            (panel(body.into(), 420.0), card_msg(CardMsg::NoticeClose))
        }
        Card::InUse => {
            let body = column![
                notice_lines(crate::tr!("text-style", "in-use"), None),
                close_row()
            ]
            .spacing(14);
            (panel(body.into(), 380.0), card_msg(CardMsg::NoticeClose))
        }
        Card::DeleteAsk(name) => {
            let body = column![
                text(crate::tr!("text-style", "delete-title"))
                    .size(13)
                    .style(crate::ui::window::pdf_dialogs::accent_text),
                notice_lines(crate::i18n::plain(crate::tr!("text-style", "delete-ask", name = name)), None),
                row![
                    Space::new().width(Length::Fill),
                    dialog_button(
                        crate::tr!("text-style", "cancel"),
                        card_msg(CardMsg::DeleteAnswer(false)),
                        false
                    ),
                    dialog_button(
                        crate::tr!("text-style", "ok"),
                        card_msg(CardMsg::DeleteAnswer(true)),
                        true
                    ),
                ]
                .spacing(6),
            ]
            .spacing(14);
            (
                panel(body.into(), 380.0),
                card_msg(CardMsg::DeleteAnswer(false)),
            )
        }
        Card::Modified(_) => {
            let body = column![
                notice_lines(
                    crate::tr!("text-style", "modified"),
                    Some(crate::tr!("text-style", "modified-ask"))
                ),
                row![
                    Space::new().width(Length::Fill),
                    dialog_button(
                        crate::tr!("text-style", "yes"),
                        card_msg(CardMsg::ModifiedAnswer(true)),
                        true
                    ),
                    dialog_button(
                        crate::tr!("text-style", "no"),
                        card_msg(CardMsg::ModifiedAnswer(false)),
                        false
                    ),
                ]
                .spacing(6),
            ]
            .spacing(14);
            (
                panel(body.into(), 400.0),
                card_msg(CardMsg::NoticeClose),
            )
        }
    }
}

/// The New card's name field, focused and selected when it opens.
pub const NEW_NAME_ID: &str = "text-style-new-name";


/// The tallest the Apply summary's list of styles grows before it scrolls,
/// for a window `height` tall: the preview and "Where used" keep their room.
fn summary_height(height: f32) -> f32 {
    (height * 0.32).clamp(110.0, 320.0)
}

fn layout<'a>(
    v: &TextStyleView<'a>,
    sizing: crate::ui::modal::ModalSizing,
    size: iced::Size,
) -> Element<'a, Message> {
    let width = size.width;
    let narrow = width < STACK_BELOW;
    let summary_height = summary_height(size.height);
    // The top card, what Apply does, then the three setting cards side by
    // side (one under another when the window is narrow).
    let cards = properties(v);
    let mut stack = column![preview_card(v)].spacing(12);
    if let Some(summary) = apply_summary(v, summary_height) {
        stack = stack.push(summary);
    }
    let stack = if narrow {
        cards.into_iter().fold(stack, |stack, card| stack.push(card))
    } else {
        stack.push(
            row(cards.into_iter().map(|card| container(card).width(Length::Fill).into()))
                .spacing(12),
        )
    };
    let editor: Element<'a, Message> = scrollable(
        container(stack).padding(iced::Padding::ZERO.right(12)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into();
    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: StyleKind::Text,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: Message::TextStyleDialogNew,
        on_copy: Message::TextStyleDialogCopy,
        on_delete: Message::TextStyleDialogDelete,
        on_import: Some(Message::StyleImportPick(StyleKind::Text)),
        on_select: Message::TextStyleDialogSelect,
        on_set_current: Message::TextStyleDialogSetCurrent,
        on_apply: Message::TextStyleApply,
        read_only: v.read_only,
        locked_keeps_actions: false,
        on_list_scroll: None,
        in_use_filter: Some((v.in_use_only, Message::TextStyleInUseFilter)),
        apply_blocked: match v.invalid_fields {
            0 => None,
            1 => Some(t!("1 field needs fixing before Apply.")),
            count => Some(crate::tf!("{count} fields need fixing before Apply.")),
        },
        rows: Some(&v.rows),
        compact: narrow,
        apply_pending: v.modified,
        delete_in_use: true,
        on_context: Some(Message::TextStyleContext),
        context_menu: v.context.map(|name| {
            // Standard and the current style cannot be deleted.
            let deletable =
                !name.eq_ignore_ascii_case("Standard") && name != v.current && !v.read_only;
            (
                name,
                vec![
                    (
                        std::borrow::Cow::Owned(crate::tr!("text-style", "menu-set-current")),
                        Some(Message::TextStyleDialogSetCurrent),
                    ),
                    (
                        std::borrow::Cow::Owned(crate::tr!("text-style", "menu-rename")),
                        Some(Message::StyleRenameStart(
                            crate::app::StyleKind::Text,
                            name.to_string(),
                        )),
                    ),
                    (
                        std::borrow::Cow::Owned(crate::tr!("text-style", "menu-delete")),
                        deletable.then_some(Message::TextStyleDialogDelete),
                    ),
                ],
            )
        }),
        list: None,
        extra_actions: Vec::new(),
        list_title: Some(Cow::Owned(crate::tr!("text-style", "styles-title"))),
        editor,
    })
}
