//! PDF dialogs: Attach PDF Underlay (pages, path type, insertion, scale,
//! rotation), Underlay Layers, PDF Import Settings and Import PDF, built
//! from cards, segmented choices and toggle chips.

use std::fmt;

use iced::widget::{
    button, column, combo_box, container, image, pick_list, row, scrollable, text, text_input,
    Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::pdf_import::{ImportLayers, PdfImportSettings};
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};

/// One edit in any of the PDF dialogs.
#[derive(Debug, Clone)]
pub enum PdfDialogMsg {
    // Attach
    AttachName(String),
    AttachBrowse,
    AttachPage(usize),
    AttachPathType(PathTypeChoice),
    AttachInsertOnScreen(bool),
    AttachInsert(usize, String),
    AttachScaleOnScreen(bool),
    AttachScale(String),
    AttachRotationOnScreen(bool),
    AttachRotation(String),
    AttachDetails(bool),
    AttachOk,
    // Underlay Layers
    LayersUnderlay(String),
    LayersSearch(String),
    LayersToggle(String),
    LayersOk,
    // Import settings (the settings dialog and the Import PDF dialog)
    Vector(bool),
    Fills(bool),
    Text(bool),
    Raster(bool),
    Layers(ImportLayers),
    AsBlock(bool),
    Join(bool),
    Hatches(bool),
    Lineweights(bool),
    Linetypes(bool),
    SettingsOk,
    // Import PDF
    ImportBrowse,
    ImportPage(usize),
    ImportPageText(String),
    ImportInsertOnScreen(bool),
    ImportScale(String),
    ImportRotation(RotationChoice),
    ImportOk,
    Options,
    Help(&'static str),
}

fn msg(m: PdfDialogMsg) -> Message {
    Message::PdfDialog(m)
}

/// Path type as the Attach dialog lists it.
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

/// Rotation of an imported page: the four right angles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationChoice(pub u16);

impl RotationChoice {
    pub const ALL: [RotationChoice; 4] = [
        RotationChoice(0),
        RotationChoice(90),
        RotationChoice(180),
        RotationChoice(270),
    ];
}

impl fmt::Display for RotationChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A page of the chosen file and its thumbnail.
#[derive(Clone)]
pub struct PageThumb {
    pub label: String,
    pub image: Option<image::Handle>,
}

/// Thumbnails of every page, about 220 px wide (sharp at preview size).
// ponytail: all pages are rasterised when the dialog opens; lazy per-row
// thumbnails if files with hundreds of pages matter.
pub fn page_thumbs(path: &str) -> Vec<PageThumb> {
    use crate::scene::model::pdf_raster::{page_count, page_size_inches, rasterize_page_at_dpi};
    let count = page_count(path).unwrap_or(0);
    (1..=count)
        .map(|n| {
            let label = n.to_string();
            let image = page_size_inches(path, &label).and_then(|(w, h)| {
                let dpi = (220.0 / w.max(h).max(0.1)) as f32;
                let page = rasterize_page_at_dpi(path, &label, dpi)?;
                Some(image::Handle::from_rgba(page.width, page.height, page.pixels.to_vec()))
            });
            PageThumb { label, image }
        })
        .collect()
}

/// Ctrl adds or removes a page, Shift takes the range from the last click,
/// a plain click selects that page alone.
pub fn click_page(selected: &mut Vec<usize>, anchor: &mut usize, page: usize, ctrl: bool, shift: bool) {
    if shift {
        let (a, b) = if *anchor <= page { (*anchor, page) } else { (page, *anchor) };
        *selected = (a..=b).collect();
    } else if ctrl {
        match selected.iter().position(|p| *p == page) {
            Some(at) if selected.len() > 1 => {
                selected.remove(at);
            }
            Some(_) => {}
            None => {
                selected.push(page);
                selected.sort_unstable();
            }
        }
        *anchor = page;
    } else {
        *selected = vec![page];
        *anchor = page;
    }
}

// ── Shared look ────────────────────────────────────────────────────────────
//
// The PDF dialogs are built from rounded cards on a slightly raised surface,
// accent-coloured card titles, segmented choices and toggle chips, with the
// help button on the left of the footer and the named action on the right.

fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn card_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border {
            width: 0.0,
            radius: 8.0.into(),
            color: iced::Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

fn well_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            width: 0.0,
            radius: 6.0.into(),
            color: iced::Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

/// A titled card.
fn card<'a>(title: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![text(title.to_uppercase()).size(10).style(accent_text), content.into()].spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style)
    .into()
}

/// One choice out of a few, as joined buttons.
fn segmented<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, String)>,
    current: T,
    on: fn(T) -> PdfDialogMsg,
) -> Element<'a, Message> {
    let buttons = options.into_iter().map(|(value, label)| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(on(value)))
            .style(button_style(value == current))
            .padding([5, 6])
            .width(Fill)
            .into()
    });
    container(iced::widget::Row::with_children(buttons.collect::<Vec<_>>()).spacing(2))
        .padding(2)
        .width(Fill)
        .style(well_style)
        .into()
}

/// A switch shown as a chip: filled with a tick while on.
fn chip<'a>(label: String, on: bool, message: Message) -> Element<'a, Message> {
    let label = if on { format!("\u{2713} {label}") } else { label };
    button(text(label).size(11))
        .on_press(message)
        .style(button_style(on))
        .padding([4, 10])
        .into()
}

/// A labelled field on one row; read-only while `enabled` is false.
fn field<'a>(
    label: String,
    value: &'a str,
    enabled: bool,
    label_width: f32,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .size(11)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    if enabled {
        input = input.on_input(on_input);
    }
    row![
        text(label).size(11).style(muted_style).width(Length::Fixed(label_width)),
        input
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// A label and a value on one line; a long value keeps its end.
fn info_line<'a>(label: String, value: &'a str) -> Element<'a, Message> {
    const MAX: usize = 70;
    let count = value.chars().count();
    let shown = if count > MAX {
        format!("…{}", value.chars().skip(count - (MAX - 1)).collect::<String>())
    } else {
        value.to_string()
    };
    row![
        text(label).size(11).style(muted_style).width(Length::Fixed(96.0)),
        text(shown).size(11).width(Fill).wrapping(iced::advanced::text::Wrapping::None),
    ]
    .spacing(6)
    .into()
}

/// Help on the left, extra buttons, then Cancel and the named action.
fn footer<'a>(
    help: &'static str,
    extra: Option<Element<'a, Message>>,
    action: std::borrow::Cow<'static, str>,
    ok: PdfDialogMsg,
) -> Element<'a, Message> {
    let mut left = row![button(text("?").size(12))
        .on_press(msg(PdfDialogMsg::Help(help)))
        .style(button_style(false))
        .padding([5, 11])]
    .spacing(6)
    .align_y(iced::Center);
    if let Some(extra) = extra {
        left = left.push(extra);
    }
    row![
        left,
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(action, msg(ok), true),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// Page thumbnails as cards; the chosen ones framed in the accent colour
/// with a ticked page number.
fn page_tiles<'a>(
    pages: &'a [PageThumb],
    selected: &[usize],
    on_click: fn(usize) -> PdfDialogMsg,
) -> Element<'a, Message> {
    let tiles = pages.iter().enumerate().map(|(index, page)| {
        let chosen = selected.contains(&index);
        let picture: Element<'a, Message> = match &page.image {
            Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
            None => Space::new().width(Fill).height(Fill).into(),
        };
        let number = if chosen {
            format!("\u{2713} {}", page.label)
        } else {
            page.label.clone()
        };
        let tile = column![
            container(picture)
                .width(Length::Fixed(132.0))
                .height(Length::Fixed(86.0))
                .padding(4)
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::WHITE)),
                    border: Border {
                        width: if chosen { 2.5 } else { 0.0 },
                        radius: 6.0.into(),
                        color: theme.palette().primary.base.color,
                    },
                    ..Default::default()
                }),
            container(text(number).size(11).style(move |theme: &Theme| text::Style {
                color: Some(if chosen {
                    theme.palette().primary.base.color
                } else {
                    theme.palette().background.base.text.scale_alpha(0.68)
                }),
            }))
            .width(Length::Fixed(132.0))
            .align_x(iced::Center),
        ]
        .spacing(4);
        button(tile)
            .on_press(msg(on_click(index)))
            .padding(4)
            .style(|_: &Theme, _| button::Style::default())
            .into()
    });
    scrollable(
        iced::widget::Row::with_children(tiles.collect::<Vec<_>>())
            .spacing(6)
            .wrap()
            .vertical_spacing(6),
    )
    .height(Fill)
    .into()
}

// ── Attach PDF Underlay ────────────────────────────────────────────────────

/// Choices the Attach dialog keeps for its next opening.
#[derive(Clone)]
pub struct AttachMemory {
    pub path_type: PathTypeChoice,
    pub insert_on_screen: bool,
    pub scale_on_screen: bool,
    pub scale: String,
    pub rotation_on_screen: bool,
    pub rotation: String,
    pub details: bool,
}

impl Default for AttachMemory {
    fn default() -> Self {
        Self {
            path_type: PathTypeChoice(Pathtype::Relative),
            insert_on_screen: true,
            scale_on_screen: false,
            scale: "1.0000".into(),
            rotation_on_screen: false,
            rotation: "0".into(),
            details: false,
        }
    }
}

pub static ATTACH_MEMORY: std::sync::Mutex<Option<AttachMemory>> = std::sync::Mutex::new(None);

pub struct PdfAttachState {
    /// The file read (absolute).
    pub path: String,
    pub name: String,
    /// PDF files already attached: name and read path.
    pub existing: Vec<(String, String)>,
    pub name_combo: combo_box::State<String>,
    pub pages: Vec<PageThumb>,
    pub selected: Vec<usize>,
    pub anchor: usize,
    pub path_type: PathTypeChoice,
    pub insert_on_screen: bool,
    pub insert: [String; 3],
    pub scale_on_screen: bool,
    pub scale: String,
    pub rotation_on_screen: bool,
    pub rotation: String,
    pub found_in: String,
    pub saved_path: String,
    pub page_size: String,
    pub details: bool,
}

impl PdfAttachState {
    pub fn new(existing: Vec<(String, String)>) -> Self {
        let memory = ATTACH_MEMORY
            .lock()
            .ok()
            .and_then(|m| m.clone())
            .unwrap_or_default();
        let names = existing.iter().map(|(name, _)| name.clone()).collect();
        Self {
            path: String::new(),
            name: String::new(),
            existing,
            name_combo: combo_box::State::new(names),
            pages: Vec::new(),
            selected: vec![0],
            anchor: 0,
            path_type: memory.path_type,
            insert_on_screen: memory.insert_on_screen,
            insert: ["0.0000".into(), "0.0000".into(), "0.0000".into()],
            scale_on_screen: memory.scale_on_screen,
            scale: memory.scale,
            rotation_on_screen: memory.rotation_on_screen,
            rotation: memory.rotation,
            found_in: String::new(),
            saved_path: String::new(),
            page_size: String::new(),
            details: memory.details,
        }
    }

    pub fn remember(&self) {
        if let Ok(mut m) = ATTACH_MEMORY.lock() {
            *m = Some(AttachMemory {
                path_type: self.path_type,
                insert_on_screen: self.insert_on_screen,
                scale_on_screen: self.scale_on_screen,
                scale: self.scale.clone(),
                rotation_on_screen: self.rotation_on_screen,
                rotation: self.rotation.clone(),
                details: self.details,
            });
        }
    }

    pub fn page_labels(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter_map(|i| self.pages.get(*i).map(|p| p.label.clone()))
            .collect()
    }
}

pub fn view_attach<'a>(
    state: &'a PdfAttachState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // File: the attached PDFs to pick from, Browse, and the page count.
    let name_combo = combo_box(
        &state.name_combo,
        "",
        (!state.name.is_empty()).then_some(&state.name),
        |chosen: String| msg(PdfDialogMsg::AttachName(chosen)),
    )
    .size(12)
    .padding([5, 8])
    .width(Fill);
    let count = container(
        text(crate::tf!("{count} pages", count = state.pages.len())).size(11).style(accent_text),
    )
    .padding([4, 10])
    .style(well_style);
    let file = row![
        name_combo,
        count,
        button(text(t!("Browse...")).size(11))
            .on_press(msg(PdfDialogMsg::AttachBrowse))
            .style(button_style(false))
            .padding([5, 12]),
    ]
    .spacing(8)
    .align_y(iced::Center);

    // Pages: the thumbnails, then which ones are chosen.
    let chosen = state.page_labels().join(", ");
    let pages = card(
        t!("Pages").into_owned(),
        column![
            container(page_tiles(&state.pages, &state.selected, PdfDialogMsg::AttachPage))
                .padding(6)
                .height(Length::Fixed(226.0))
                .width(Fill)
                .style(well_style),
            text(crate::tf!("Selected pages: {pages}", pages = chosen)).size(11).style(muted_style),
        ]
        .spacing(6),
    );

    // Placement: insertion point, scale and rotation, each with an
    // "On screen" chip that leaves the value to the command line.
    let on_screen = t!("On screen").into_owned();
    let insert_enabled = !state.insert_on_screen;
    let xyz = row![
        field("X".into(), &state.insert[0], insert_enabled, 12.0, |v| msg(PdfDialogMsg::AttachInsert(0, v))),
        field("Y".into(), &state.insert[1], insert_enabled, 12.0, |v| msg(PdfDialogMsg::AttachInsert(1, v))),
        field("Z".into(), &state.insert[2], insert_enabled, 12.0, |v| msg(PdfDialogMsg::AttachInsert(2, v))),
    ]
    .spacing(8);
    let heading = |label: std::borrow::Cow<'static, str>, on: bool, message: Message| {
        row![
            text(label).size(11).width(Fill),
            chip(on_screen.clone(), on, message)
        ]
        .align_y(iced::Center)
    };
    let placement = card(
        t!("Placement").into_owned(),
        column![
            heading(
                t!("Insertion point"),
                state.insert_on_screen,
                msg(PdfDialogMsg::AttachInsertOnScreen(!state.insert_on_screen))
            ),
            xyz,
            heading(
                t!("Scale"),
                state.scale_on_screen,
                msg(PdfDialogMsg::AttachScaleOnScreen(!state.scale_on_screen))
            ),
            field(String::new(), &state.scale, !state.scale_on_screen, 0.0, |v| msg(PdfDialogMsg::AttachScale(v))),
            heading(
                t!("Rotation"),
                state.rotation_on_screen,
                msg(PdfDialogMsg::AttachRotationOnScreen(!state.rotation_on_screen))
            ),
            field(String::new(), &state.rotation, !state.rotation_on_screen, 0.0, |v| msg(PdfDialogMsg::AttachRotation(v))),
        ]
        .spacing(6),
    );

    let path = card(
        t!("Path type").into_owned(),
        segmented(
            PathTypeChoice::ALL.iter().map(|c| (*c, c.to_string())).collect(),
            state.path_type,
            PdfDialogMsg::AttachPathType,
        ),
    );

    let details = card(
        t!("File details").into_owned(),
        column![
            info_line(t!("Found in:").into_owned(), &state.found_in),
            info_line(t!("Saved path:").into_owned(), &state.saved_path),
            info_line(t!("Page size:").into_owned(), &state.page_size),
        ]
        .spacing(4),
    );

    let body = row![
        column![pages, details].spacing(8).width(Length::FillPortion(3)),
        column![placement, path].spacing(8).width(Length::FillPortion(2)),
    ]
    .spacing(10);

    column![file, body, footer("attach", None, t!("Attach"), PdfDialogMsg::AttachOk)]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

// ── Underlay Layers ────────────────────────────────────────────────────────

/// One PDF underlay the dialog can show: its name, its file's layers, and
/// which of them it turns off.
#[derive(Clone)]
pub struct LayerTarget {
    pub name: String,
    pub handle: codec::Handle,
    pub layers: Vec<String>,
    pub hidden: Vec<String>,
}

pub struct UnderlayLayersState {
    pub targets: Vec<LayerTarget>,
    pub current: usize,
    pub search: String,
}

pub fn view_layers<'a>(
    state: &'a UnderlayLayersState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let names: Vec<String> = state.targets.iter().map(|t| t.name.clone()).collect();
    let target = state.targets.get(state.current);
    let underlay = pick_list(target.map(|t| t.name.clone()), names, |n: &String| n.clone())
        .on_select(|n| msg(PdfDialogMsg::LayersUnderlay(n)))
        .text_size(12)
        .padding([5, 8])
        .width(Fill);
    let search = text_input(t!("Search for layer").as_ref(), &state.search)
        .on_input(|s| msg(PdfDialogMsg::LayersSearch(s)))
        .size(11)
        .padding([5, 8])
        .width(Fill)
        .style(field_style);

    let (total, hidden) = target.map_or((0, 0), |t| (t.layers.len(), t.hidden.len()));
    let summary = text(crate::tf!("{count} layers, {hidden} hidden", count = total, hidden = hidden))
        .size(11)
        .style(muted_style);

    // Each layer: its name and an On / Off switch.
    let body: Element<'a, Message> = match target {
        Some(t) if !t.layers.is_empty() => {
            let needle = state.search.to_lowercase();
            let rows = t
                .layers
                .iter()
                .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
                .map(|name| {
                    let on = !t.hidden.contains(name);
                    let switch = button(
                        text(if on { t!("On") } else { t!("Off") })
                            .size(11)
                            .width(Fill)
                            .align_x(iced::Center),
                    )
                    .on_press(msg(PdfDialogMsg::LayersToggle(name.clone())))
                    .style(button_style(on))
                    .padding([3, 6])
                    .width(Length::Fixed(64.0));
                    container(
                        row![text(name.clone()).size(12).width(Fill), switch]
                            .align_y(iced::Center),
                    )
                    .padding([5, 10])
                    .style(well_style)
                    .into()
                });
            scrollable(iced::widget::Column::with_children(rows.collect::<Vec<_>>()).spacing(4))
                .height(Fill)
                .into()
        }
        _ => container(text(t!("This file does not contain any layers.")).size(11).style(muted_style))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .into(),
    };

    column![
        card(t!("Underlay").into_owned(), underlay),
        card(
            t!("Layers").into_owned(),
            column![search, summary, container(body).height(Length::Fixed(250.0))].spacing(6)
        ),
        footer("layers", None, t!("Apply"), PdfDialogMsg::LayersOk),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

// ── PDF Import Settings ────────────────────────────────────────────────────

/// Content as chips, layers as a segmented choice, options as chips.
fn settings_cards<'a>(s: &'a PdfImportSettings) -> (Element<'a, Message>, Element<'a, Message>, Element<'a, Message>) {
    let fills = if s.vector {
        chip(t!("Solid fills").into_owned(), s.fills, msg(PdfDialogMsg::Fills(!s.fills)))
    } else {
        chip(t!("Solid fills").into_owned(), s.fills, Message::Noop)
    };
    let content = card(
        t!("PDF data to import").into_owned(),
        row![
            chip(t!("Vector geometry").into_owned(), s.vector, msg(PdfDialogMsg::Vector(!s.vector))),
            fills,
            chip(t!("TrueType text").into_owned(), s.text, msg(PdfDialogMsg::Text(!s.text))),
            chip(t!("Raster images").into_owned(), s.raster, msg(PdfDialogMsg::Raster(!s.raster))),
        ]
        .spacing(6)
        .wrap()
        .vertical_spacing(6),
    );
    let layers = card(
        t!("Layers").into_owned(),
        segmented(
            vec![
                (ImportLayers::Pdf, t!("Use PDF layers").into_owned()),
                (ImportLayers::Object, t!("Create object layers").into_owned()),
                (ImportLayers::Current, t!("Current layer").into_owned()),
            ],
            s.layers,
            PdfDialogMsg::Layers,
        ),
    );
    let options = card(
        t!("Import options").into_owned(),
        row![
            chip(t!("Import as block").into_owned(), s.as_block, msg(PdfDialogMsg::AsBlock(!s.as_block))),
            chip(t!("Join line and arc segments").into_owned(), s.join, msg(PdfDialogMsg::Join(!s.join))),
            chip(t!("Convert solid fills to hatches").into_owned(), s.hatches, msg(PdfDialogMsg::Hatches(!s.hatches))),
            chip(t!("Apply lineweight properties").into_owned(), s.lineweights, msg(PdfDialogMsg::Lineweights(!s.lineweights))),
            chip(t!("Infer linetypes from collinear dashes").into_owned(), s.linetypes, msg(PdfDialogMsg::Linetypes(!s.linetypes))),
        ]
        .spacing(6)
        .wrap()
        .vertical_spacing(6),
    );
    (content, layers, options)
}

pub fn view_import_settings<'a>(
    settings: &'a PdfImportSettings,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let (content, layers, options) = settings_cards(settings);
    let more = button(text(t!("Options...")).size(11))
        .on_press(msg(PdfDialogMsg::Options))
        .style(button_style(false))
        .padding([5, 12]);
    column![
        content,
        layers,
        options,
        footer("settings", Some(more.into()), t!("Save"), PdfDialogMsg::SettingsOk),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

// ── Import PDF (File) ──────────────────────────────────────────────────────

pub struct PdfImportFileState {
    pub path: String,
    pub pages: Vec<PageThumb>,
    pub selected: usize,
    pub page_text: String,
    pub insert_on_screen: bool,
    pub scale: String,
    pub rotation: RotationChoice,
    pub page_size: String,
    pub settings: PdfImportSettings,
}

pub fn view_import_file<'a>(
    state: &'a PdfImportFileState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let file_name = std::path::Path::new(&state.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = row![
        container(text(file_name).size(12))
            .padding([6, 10])
            .width(Fill)
            .style(well_style),
        button(text(t!("Browse...")).size(11))
            .on_press(msg(PdfDialogMsg::ImportBrowse))
            .style(button_style(false))
            .padding([5, 12]),
    ]
    .spacing(8)
    .align_y(iced::Center);

    // One page at a time: a large preview with previous / next.
    let count = state.pages.len();
    let preview: Element<'a, Message> = match state.pages.get(state.selected).and_then(|p| p.image.clone()) {
        Some(handle) => image(handle).width(Fill).height(Fill).into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let step = |label: &'static str, to: Option<usize>| {
        button(text(label).size(14))
            .on_press_maybe(to.map(|p| msg(PdfDialogMsg::ImportPage(p))))
            .style(button_style(false))
            .padding([2, 12])
    };
    let navigator = row![
        step("\u{2039}", state.selected.checked_sub(1)),
        Space::new().width(Fill),
        text_input("", &state.page_text)
            .on_input(|s| msg(PdfDialogMsg::ImportPageText(s)))
            .size(11)
            .padding([3, 6])
            .width(Length::Fixed(48.0))
            .style(field_style),
        text(format!("/ {count}")).size(11).style(muted_style),
        Space::new().width(Fill),
        step("\u{203a}", (state.selected + 1 < count).then_some(state.selected + 1)),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let page = card(
        t!("Pages").into_owned(),
        column![
            container(preview)
                .padding(8)
                .height(Length::Fixed(300.0))
                .width(Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::WHITE)),
                    border: Border { width: 0.0, radius: 6.0.into(), color: iced::Color::TRANSPARENT },
                    ..Default::default()
                }),
            navigator,
            row![
                text(crate::tf!("Page size: {size}", size = state.page_size.clone())).size(11).style(muted_style),
                Space::new().width(Fill),
                text(crate::tf!("PDF scale: {scale}:1", scale = state.scale.trim())).size(11).style(muted_style),
            ],
        ]
        .spacing(8),
    );

    let placement = card(
        t!("Placement").into_owned(),
        column![
            row![
                text(t!("Insertion point")).size(11).width(Fill),
                chip(
                    t!("On screen").into_owned(),
                    state.insert_on_screen,
                    msg(PdfDialogMsg::ImportInsertOnScreen(!state.insert_on_screen))
                ),
            ]
            .align_y(iced::Center),
            field(t!("Scale").into_owned(), &state.scale, true, 60.0, |s| msg(PdfDialogMsg::ImportScale(s))),
            row![
                text(t!("Rotation")).size(11).style(muted_style).width(Length::Fixed(60.0)),
                segmented(
                    RotationChoice::ALL.iter().map(|r| (*r, format!("{}\u{b0}", r.0))).collect(),
                    state.rotation,
                    PdfDialogMsg::ImportRotation,
                ),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    let (content, layers, options) = settings_cards(&state.settings);
    let right = scrollable(column![placement, content, layers, options].spacing(8)).height(Fill);
    let body = row![
        container(page).width(Length::FillPortion(1)),
        container(right).width(Length::FillPortion(1)),
    ]
    .spacing(10)
    .height(Length::Fixed(420.0));

    let more = button(text(t!("Options...")).size(11))
        .on_press(msg(PdfDialogMsg::Options))
        .style(button_style(false))
        .padding([5, 12]);
    column![file, body, footer("import", Some(more.into()), t!("Import"), PdfDialogMsg::ImportOk)]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
