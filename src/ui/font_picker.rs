//! The shared font picker of the text style manager and the MTEXT toolbar:
//! one searchable list of the drawing's fonts, the bundled stroke fonts, the
//! SHX files in the font folders and the system's TrueType families (#2413).
//!
//! The head is the search field: typing filters the list below it.

use crate::app::Message;
use crate::t;
use crate::ui::style::common::{muted_style, readable_accent};
use codec::Document;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Font, Length, Theme};
use std::borrow::Cow;
use std::path::Path;

/// The built-in stroke fonts: the name shown, and the SHX file the style
/// stores, so another program finds the font it names. Each file resolves to
/// the bundled LFF font of the shown name (`scene::text::lff`).
pub const STROKE_FONTS: &[(&str, &str)] = &[
    ("Standard", "txt.shx"),
    ("ISO", "isocp.shx"),
    ("Simplex", "simplex.shx"),
    ("RomanS", "romans.shx"),
    ("RomanD", "romand.shx"),
    ("RomanC", "romanc.shx"),
    ("RomanT", "romant.shx"),
    ("ItalicC", "italicc.shx"),
    ("ItalicT", "italict.shx"),
    ("ScriptS", "scripts.shx"),
    ("ScriptC", "scriptc.shx"),
    ("GothGBT", "gothice.shx"),
    ("GothGRT", "gothicg.shx"),
    ("GothITT", "gothici.shx"),
    ("GreekC", "greekc.shx"),
    ("ISO3098", "isoct.shx"),
];

/// What a pick puts into the style or the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontChoice {
    /// MTEXT only: no font of its own, the text style's font.
    StyleDefault,
    /// A stroke font, by its SHX file (`simplex.shx`).
    Stroke(String),
    /// A TrueType font, by family (`Arial`).
    TrueType(String),
}

impl FontChoice {
    /// The choice a stored name stands for: an SHX file or a bundled stroke
    /// font name is a stroke font, anything else a TrueType family.
    pub fn of_name(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            FontChoice::StyleDefault
        } else if crate::scene::text::lff::is_shx_name(name)
            || crate::scene::text::lff::is_builtin(name)
        {
            FontChoice::Stroke(name.to_string())
        } else {
            FontChoice::TrueType(name.to_string())
        }
    }

    /// The name shown: a bundled stroke font's own name, else the stored one.
    pub fn label(&self) -> Cow<'static, str> {
        match self {
            FontChoice::StyleDefault => t!("[Style default]"),
            FontChoice::Stroke(file) => STROKE_FONTS
                .iter()
                .find(|(label, value)| names_stroke_font(file, label, value))
                .map_or_else(
                    || Cow::Owned(file.clone()),
                    |(label, _)| Cow::Borrowed(*label),
                ),
            FontChoice::TrueType(family) => Cow::Owned(family.clone()),
        }
    }

    /// Whether `self` and `other` name the same font, whichever way the
    /// name is spelled.
    pub fn same_font(&self, other: &FontChoice) -> bool {
        match (self, other) {
            (FontChoice::StyleDefault, FontChoice::StyleDefault) => true,
            (FontChoice::Stroke(a), FontChoice::Stroke(b)) => {
                stem(a) == stem(b) || self.label() == other.label()
            }
            (FontChoice::TrueType(a), FontChoice::TrueType(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

/// The SHX file a text style stores for a stroke font: the file itself, a
/// bundled font's file by its shown name, or the name with `.shx` added.
pub fn stroke_file(name: &str) -> String {
    let name = name.trim();
    if crate::scene::text::lff::is_shx_name(name) {
        return name.to_string();
    }
    STROKE_FONTS
        .iter()
        .find(|(label, file)| names_stroke_font(name, label, file))
        .map_or_else(|| format!("{name}.shx"), |(_, file)| file.to_string())
}

fn stem(name: &str) -> String {
    let name = crate::io::font_repo::font_key(name);
    name.strip_suffix(".shx").unwrap_or(&name).to_string()
}

/// True when the stored font names this stroke font: by file, with or
/// without the `.shx` extension, or by the shown name older versions stored.
fn names_stroke_font(stored: &str, label: &str, file: &str) -> bool {
    stem(stored) == stem(file) || stored.trim().eq_ignore_ascii_case(label)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Drawing,
    Stroke,
    Folders,
    TrueType,
}

impl Section {
    fn title(self) -> Cow<'static, str> {
        match self {
            Section::Drawing => t!("In this drawing"),
            Section::Stroke => t!("Stroke fonts (SHX)"),
            Section::Folders => t!("Font folders"),
            Section::TrueType => t!("TrueType fonts"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontEntry {
    pub choice: FontChoice,
    pub section: Section,
    /// The drawing names the font, but nothing here draws it: the font
    /// drawn in its place.
    pub missing: Option<String>,
}

/// Every font the picker lists, in section order. `style_default` puts the
/// MTEXT "[Style default]" entry first.
pub fn catalogue(doc: &Document, style_default: bool) -> Vec<FontEntry> {
    let entry = |choice, section| FontEntry {
        choice,
        section,
        missing: None,
    };
    let mut entries = Vec::new();
    if style_default {
        entries.push(entry(FontChoice::StyleDefault, Section::Drawing));
    }
    for (choice, missing) in drawing_fonts(doc) {
        entries.push(FontEntry {
            choice,
            section: Section::Drawing,
            missing,
        });
    }
    entries.extend(
        STROKE_FONTS
            .iter()
            .map(|(_, file)| entry(FontChoice::Stroke(file.to_string()), Section::Stroke)),
    );
    entries.extend(
        crate::io::font_repo::folder_shx_fonts()
            .into_iter()
            .map(|file| entry(FontChoice::Stroke(file), Section::Folders)),
    );
    // Web: the fetched Noto set plus the office/CAD family names it stands
    // in for (Arial → Noto Sans, …); desktop: installed fonts.
    entries.extend(
        crate::scene::text::sysfont::picker_families()
            .into_iter()
            .map(|family| entry(FontChoice::TrueType(family), Section::TrueType)),
    );
    entries
}

/// The font `style` draws with, as the picker names it; `None` when the
/// style names none.
pub fn style_font(style: &codec::tables::TextStyle) -> Option<FontChoice> {
    let family = style.true_type_font.trim();
    if !family.is_empty() {
        return Some(FontChoice::TrueType(family.to_string()));
    }
    if style.font_file.trim().is_empty() {
        return None;
    }
    Some(
        match FontChoice::of_name(&crate::io::font_repo::font_key(&style.font_file)) {
            // A bare stroke name (`txt`) is read as its SHX file.
            FontChoice::Stroke(name) => FontChoice::Stroke(stroke_file(&name)),
            other => other,
        },
    )
}

/// The fonts the drawing's text styles use, each once, and for a missing
/// one the font drawn instead.
fn drawing_fonts(doc: &Document) -> Vec<(FontChoice, Option<String>)> {
    use crate::scene::text::sysfont;
    let base = doc
        .source_path
        .as_deref()
        .map(Path::new)
        .and_then(|p| p.parent());
    let mut fonts: Vec<(FontChoice, Option<String>)> = Vec::new();
    // A shape file entry (linetype symbols) is not a text font.
    for style in doc.text_styles.iter().filter(|style| !style.is_shape_file) {
        let Some(font) = style_font(style) else {
            continue;
        };
        let missing = match &font {
            FontChoice::TrueType(family) => {
                !sysfont::has_family(family)
                    && sysfont::family_for_reference(&style.font_file).is_none()
            }
            _ => crate::io::font_repo::shx_file_missing(&style.font_file, base),
        };
        if !fonts.iter().any(|(known, _)| known.same_font(&font)) {
            let drawn_with = missing.then(|| drawn_with(&font));
            fonts.push((font, drawn_with));
        }
    }
    fonts.sort_by_key(|(font, _)| font.label().to_lowercase());
    fonts
}

/// The font the drawing draws in place of a missing `choice`, by the name the
/// picker lists it under: a bundled stroke font of its name (a missing
/// `ISOCPEUR` draws with the ISO strokes), else Standard.
pub fn drawn_with(choice: &FontChoice) -> String {
    use crate::scene::text::font_face::Face;
    use crate::scene::text::lff;
    let name = match choice {
        FontChoice::StyleDefault => return String::new(),
        FontChoice::Stroke(name) | FontChoice::TrueType(name) => name,
    };
    match Face::resolve(name) {
        Face::Lff(font) => STROKE_FONTS
            .iter()
            .find(|(label, _)| std::ptr::eq(lff::get_font(label), font))
            .map_or_else(|| font.name.clone(), |(label, _)| label.to_string()),
        Face::Ttf { family, .. } => family,
        Face::Shx { path, .. } => crate::io::font_repo::font_key(&path),
        Face::WithBig { .. } => name.clone(),
    }
}

#[derive(Debug, Clone)]
pub enum FontPickerEvent {
    Toggle,
    Search(String),
    Pick(FontChoice),
}

/// The open state, search text and listed fonts of one picker.
#[derive(Debug, Default)]
pub struct FontPickerState {
    pub open: bool,
    pub query: String,
    entries: Vec<FontEntry>,
}

impl FontPickerState {
    /// List the fonts again, from `doc` and the folders as they are now.
    pub fn refresh(&mut self, doc: &Document, style_default: bool) {
        self.entries = catalogue(doc, style_default);
    }

    /// Follow `event`; a pick closes the list and is returned. Opening lists
    /// the fonts again, so a font downloaded meanwhile shows up.
    pub fn update(
        &mut self,
        event: FontPickerEvent,
        doc: &Document,
        style_default: bool,
    ) -> Option<FontChoice> {
        match event {
            FontPickerEvent::Toggle => {
                self.open = !self.open;
                self.query.clear();
                if self.open {
                    self.refresh(doc, style_default);
                }
                None
            }
            FontPickerEvent::Search(query) => {
                if !self.open {
                    self.refresh(doc, style_default);
                }
                self.open = true;
                self.query = query;
                None
            }
            FontPickerEvent::Pick(choice) => {
                self.open = false;
                self.query.clear();
                Some(choice)
            }
        }
    }

    /// The listed fonts the search text matches, by shown name or file.
    pub fn matches(&self) -> Vec<&FontEntry> {
        let query = self.query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.choice.label().to_lowercase().contains(&query)
                    || matches!(&entry.choice, FontChoice::Stroke(file) if file.to_lowercase().contains(&query))
            })
            .collect()
    }

    /// The search text as a font of its own, when no listed font has that
    /// name: an SHX file not in the folders, or a family not installed.
    fn typed_choice(&self) -> Option<FontChoice> {
        let typed = FontChoice::of_name(&self.query);
        (typed != FontChoice::StyleDefault
            && !self
                .entries
                .iter()
                .any(|entry| entry.choice.same_font(&typed)))
        .then_some(typed)
    }

    /// The font drawn in place of `choice`, when the drawing names it but
    /// nothing here has it.
    fn drawn_instead(&self, choice: &FontChoice) -> Option<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.choice.same_font(choice))
            .find_map(|entry| entry.missing.as_deref())
    }
}

/// What a picker shows and where its events go.
pub struct FontPicker<'a> {
    pub state: &'a FontPickerState,
    pub selected: FontChoice,
    /// The file the style stores, shown beside the name when it differs.
    pub file: String,
    pub width: Length,
    pub read_only: bool,
    pub on_event: fn(FontPickerEvent) -> Message,
}

const ROW_H: f32 = 22.0;
/// The narrowest list: a name and the missing note or the file side by side.
/// A wider field gets a list of its own width.
const POPUP_MIN_W: f32 = 260.0;

const BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::DEFAULT
};

/// What a list row is drawn on: the popup, or the highlight of the font in
/// use.
fn row_background(theme: &Theme, active: bool) -> Color {
    let palette = theme.palette();
    if active {
        palette.primary.weak.color
    } else {
        palette.background.weak.color
    }
}

/// Secondary text on a row (the file name, a section title), still easy to
/// read.
fn secondary_text(theme: &Theme, active: bool) -> Color {
    let palette = theme.palette();
    let background = row_background(theme, active);
    let text = if active {
        palette.primary.weak.text
    } else {
        palette.background.base.text
    };
    readable_accent(
        crate::ui::style::chrome::mix(background, text, 0.6),
        background,
        text,
    )
}

/// The kind tag: SHX grey, TT green, a missing font's "!" orange. The
/// theme's colour tints the tag and, made readable, writes its letters.
fn kind_badge<'a>(choice: &FontChoice, missing: bool) -> Element<'a, Message> {
    let (badge, accent): (&str, fn(&Theme) -> Color) = match choice {
        _ if missing => ("!", |theme| theme.palette().warning.base.color),
        FontChoice::StyleDefault => return Space::new().width(26).into(),
        FontChoice::Stroke(_) => ("SHX", |theme| theme.palette().background.base.text),
        FontChoice::TrueType(_) => ("TT", |theme| theme.palette().success.base.color),
    };
    container(text(badge).size(8).font(BOLD))
        .center_x(26)
        .padding([1, 0])
        .style(move |theme: &Theme| {
            let base = theme.palette().background.base;
            let fill = crate::ui::style::chrome::mix(base.color, accent(theme), 0.3);
            container::Style {
                background: Some(Background::Color(fill)),
                text_color: Some(readable_accent(accent(theme), fill, base.text)),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

fn entry_row<'a>(
    entry: &FontEntry,
    active: bool,
    on_event: fn(FontPickerEvent) -> Message,
) -> Element<'a, Message> {
    let mut line = row![
        kind_badge(&entry.choice, entry.missing.is_some()),
        text(entry.choice.label().into_owned()).size(11),
        Space::new().width(Length::Fill),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if let Some(font) = &entry.missing {
        line = line.push(
            text(crate::tf!("missing · drawn with {font}"))
                .size(10)
                .style(move |theme: &Theme| text::Style {
                    color: Some(readable_accent(
                        theme.palette().warning.base.color,
                        row_background(theme, active),
                        theme.palette().background.base.text,
                    )),
                }),
        );
    } else if let FontChoice::Stroke(file) = &entry.choice {
        if *file != entry.choice.label() {
            line = line.push(text(file.clone()).size(10).style(move |theme: &Theme| {
                text::Style {
                    color: Some(secondary_text(theme, active)),
                }
            }));
        }
    }
    button(line)
        .on_press(on_event(FontPickerEvent::Pick(entry.choice.clone())))
        .style(move |theme: &Theme, status| {
            let style = crate::ui::color_select::list_row_style(theme, status);
            if active {
                let palette = theme.palette();
                button::Style {
                    background: Some(Background::Color(palette.primary.weak.color)),
                    text_color: palette.primary.weak.text,
                    ..style
                }
            } else {
                style
            }
        })
        .padding([2, 6])
        .height(ROW_H)
        .width(Length::Fill)
        .into()
}

pub fn view<'a>(picker: FontPicker<'a>) -> Element<'a, Message> {
    let FontPicker {
        state,
        selected,
        file,
        width,
        read_only,
        on_event,
    } = picker;
    let label = selected.label().into_owned();
    let missing = state.drawn_instead(&selected).is_some();
    let shown_file = match &selected {
        FontChoice::StyleDefault => "",
        _ if file.trim().eq_ignore_ascii_case(&label) => "",
        _ => file.trim(),
    }
    .to_string();
    if read_only {
        return crate::ui::read_only::field(&label, 11.0, width);
    }
    let input = text_input(&label, if state.open { &state.query } else { &label })
        .on_input(move |query| on_event(FontPickerEvent::Search(query)))
        .size(11)
        .style(|theme: &Theme, status| text_input::Style {
            // The bordered head draws the frame; the field stays flat inside.
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            ..crate::ui::style::form::field_style(theme, status)
        })
        .padding([3, 2])
        .width(Length::Fill);
    let caret = button(if state.open {
        crate::ui::icons::themed_arrow_up(9.0)
    } else {
        crate::ui::icons::themed_arrow_down(9.0)
    })
    .on_press(on_event(FontPickerEvent::Toggle))
    .style(button::text)
    .padding([3, 4]);
    let mut head = row![kind_badge(&selected, missing), input]
        .spacing(4)
        .align_y(iced::Center);
    if !shown_file.is_empty() {
        head = head.push(text(shown_file).size(10).style(muted_style));
    }
    let head = container(head.push(caret).align_y(iced::Center))
        .padding([0, 6])
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
        .width(width);

    let mut list = column![].spacing(1);
    if let Some(typed) = state.typed_choice() {
        let name = typed.label().into_owned();
        list = list.push(
            button(text(crate::tf!("Use “{name}”")).size(11))
                .on_press(on_event(FontPickerEvent::Pick(typed)))
                .style(crate::ui::color_select::list_row_style)
                .padding([2, 6])
                .height(ROW_H)
                .width(Length::Fill),
        );
    }
    let mut section = None;
    for entry in state.matches() {
        if section != Some(entry.section) && entry.choice != FontChoice::StyleDefault {
            section = Some(entry.section);
            list = list.push(
                container(
                    text(entry.section.title().to_uppercase())
                        .size(9)
                        .font(BOLD)
                        .style(|theme: &Theme| text::Style {
                            color: Some(secondary_text(theme, false)),
                        }),
                )
                .padding(iced::Padding::from([4, 6]).top(7)),
            );
        }
        list = list.push(entry_row(
            entry,
            entry.choice.same_font(&selected),
            on_event,
        ));
    }
    // The right padding keeps the file names clear of the scroll bar.
    let list = list.padding(iced::Padding {
        right: 12.0,
        ..Default::default()
    });
    let popup = container(scrollable(list).height(Length::Fixed(300.0)))
        .style(container::bordered_box)
        .padding(2);
    // The list opens centred under the field: as wide as the field, it lines
    // up with it and stays in the field's column.
    let popup_width = match width {
        Length::Fixed(field) if field < POPUP_MIN_W => Some(Length::Fixed(POPUP_MIN_W)),
        _ => None,
    };
    crate::ui::color_select::editable_drop_down_below(
        head.into(),
        popup.into(),
        popup_width,
        Length::Shrink,
        on_event(FontPickerEvent::Toggle),
        state.open,
    )
}

/// Regular, bold, italic or both: the face of a TrueType family a style
/// draws with (its bold and italic flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FaceStyle {
    pub fn of(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => FaceStyle::Regular,
            (true, false) => FaceStyle::Bold,
            (false, true) => FaceStyle::Italic,
            (true, true) => FaceStyle::BoldItalic,
        }
    }

    pub fn bold(self) -> bool {
        matches!(self, FaceStyle::Bold | FaceStyle::BoldItalic)
    }

    pub fn italic(self) -> bool {
        matches!(self, FaceStyle::Italic | FaceStyle::BoldItalic)
    }

    /// The faces `family` has installed, plus `current` so a stored face
    /// this machine lacks stays shown.
    pub fn offered(family: &str, current: FaceStyle) -> Vec<FaceStyle> {
        use FaceStyle::*;
        [Regular, Bold, Italic, BoldItalic]
            .into_iter()
            .filter(|face| {
                *face == Regular
                    || *face == current
                    || crate::scene::text::sysfont::styled_family(
                        family,
                        face.bold(),
                        face.italic(),
                    )
                    .is_some_and(|name| {
                        let (_, bold, italic) = crate::scene::text::sysfont::split_style(&name);
                        (bold, italic) == (face.bold(), face.italic())
                    })
            })
            .collect()
    }
}

impl std::fmt::Display for FaceStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            FaceStyle::Regular => t!("Regular"),
            FaceStyle::Bold => t!("Bold"),
            FaceStyle::Italic => t!("Italic"),
            FaceStyle::BoldItalic => t!("Bold Italic"),
        })
    }
}
