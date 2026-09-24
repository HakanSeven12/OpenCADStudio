//! PDF dialogs: Attach PDF Underlay (pages, path type, insertion, scale,
//! rotation), Underlay Layers, PDF Import Settings and Import PDF.

use std::fmt;

use iced::widget::{
    button, column, combo_box, container, image, pick_list, radio, row, scrollable, text,
    text_input, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::pdf_import::{ImportLayers, PdfImportSettings};
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, field_style};
use crate::ui::window::block_definition::{group, labeled_checkbox};

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

/// Thumbnails of every page, about 100 px wide.
// ponytail: all pages are rasterised when the dialog opens; lazy per-row
// thumbnails if files with hundreds of pages matter.
pub fn page_thumbs(path: &str) -> Vec<PageThumb> {
    use crate::scene::model::pdf_raster::{page_count, page_size_inches, rasterize_page_at_dpi};
    let count = page_count(path).unwrap_or(0);
    (1..=count)
        .map(|n| {
            let label = n.to_string();
            let image = page_size_inches(path, &label).and_then(|(w, h)| {
                let dpi = (100.0 / w.max(h).max(0.1)) as f32;
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

fn frame_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            width: 1.0,
            radius: 2.0.into(),
            color: theme.palette().background.strong.color,
        },
        ..Default::default()
    }
}

fn page_grid<'a>(
    pages: &'a [PageThumb],
    selected: &[usize],
    on_click: fn(usize) -> PdfDialogMsg,
    height: f32,
) -> Element<'a, Message> {
    let tiles = pages.iter().enumerate().map(|(index, page)| {
        let chosen = selected.contains(&index);
        let picture: Element<'a, Message> = match &page.image {
            Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
            None => Space::new().width(Fill).height(Fill).into(),
        };
        let tile = column![
            container(picture)
                .width(Length::Fixed(100.0))
                .height(Length::Fixed(60.0))
                .padding(2)
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::WHITE)),
                    border: Border {
                        width: if chosen { 2.0 } else { 1.0 },
                        radius: 1.0.into(),
                        color: if chosen {
                            theme.palette().primary.base.color
                        } else {
                            theme.palette().background.strong.color
                        },
                    },
                    ..Default::default()
                }),
            container(text(page.label.clone()).size(11))
                .width(Length::Fixed(100.0))
                .align_x(iced::Center)
                .style(move |theme: &Theme| container::Style {
                    background: chosen
                        .then(|| Background::Color(theme.palette().primary.weak.color)),
                    ..Default::default()
                }),
        ]
        .spacing(1);
        button(tile)
            .on_press(msg(on_click(index)))
            .padding(2)
            .style(|_: &Theme, _| button::Style::default())
            .into()
    });
    container(scrollable(
        iced::widget::Row::with_children(tiles.collect::<Vec<_>>())
            .spacing(8)
            .wrap()
            .vertical_spacing(8),
    ))
    .padding(6)
    .width(Fill)
    .height(Length::Fixed(height))
    .style(frame_style)
    .into()
}

fn value_field<'a>(
    label: String,
    value: &'a str,
    enabled: bool,
    label_width: f32,
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
        text(label).size(11).style(muted_style).width(Length::Fixed(label_width)),
        field
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn read_only<'a>(label: String, value: &'a str, label_width: f32) -> Element<'a, Message> {
    value_field(label, value, false, label_width, |_| Message::Noop)
}

fn footer<'a>(ok: PdfDialogMsg, help: &'static str) -> iced::widget::Row<'a, Message> {
    row![
        Space::new().width(Fill),
        button(text(t!("OK")).size(11))
            .on_press(msg(ok))
            .style(button_style(true))
            .padding([4, 16]),
        button(text(t!("Cancel")).size(11))
            .on_press(Message::CloseModal)
            .style(button_style(false))
            .padding([4, 12]),
        button(text(t!("Help")).size(11))
            .on_press(msg(PdfDialogMsg::Help(help)))
            .style(button_style(false))
            .padding([4, 12]),
    ]
    .spacing(6)
    .align_y(iced::Center)
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
    let name_combo = combo_box(
        &state.name_combo,
        "",
        (!state.name.is_empty()).then_some(&state.name),
        |chosen: String| msg(PdfDialogMsg::AttachName(chosen)),
    )
    .size(11)
    .padding([3, 6])
    .width(Fill);
    let name_section = column![
        text(t!("Name:")).size(11).style(muted_style),
        row![
            name_combo,
            button(text(t!("Browse...")).size(11))
                .on_press(msg(PdfDialogMsg::AttachBrowse))
                .style(button_style(false))
                .padding([4, 12])
                .width(Length::Fixed(96.0)),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(3);

    let pages = group(
        t!("Select one or more pages from the PDF file:").into_owned(),
        page_grid(&state.pages, &state.selected, PdfDialogMsg::AttachPage, 250.0),
        Length::Shrink,
    );

    let path_type = group(
        t!("Path type").into_owned(),
        pick_list(Some(state.path_type), PathTypeChoice::ALL, |c| c.to_string())
            .on_select(|v| msg(PdfDialogMsg::AttachPathType(v)))
            .text_size(11)
            .padding([3, 6])
            .width(Fill),
        Length::Shrink,
    );
    let insert_enabled = !state.insert_on_screen;
    let insertion = group(
        t!("Insertion point").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.insert_on_screen, |v| msg(
                PdfDialogMsg::AttachInsertOnScreen(v)
            )),
            value_field("X:".into(), &state.insert[0], insert_enabled, 16.0, |v| msg(
                PdfDialogMsg::AttachInsert(0, v)
            )),
            value_field("Y:".into(), &state.insert[1], insert_enabled, 16.0, |v| msg(
                PdfDialogMsg::AttachInsert(1, v)
            )),
            value_field("Z:".into(), &state.insert[2], insert_enabled, 16.0, |v| msg(
                PdfDialogMsg::AttachInsert(2, v)
            )),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let middle = column![path_type, insertion].spacing(6).width(Length::FillPortion(1));

    let mut scale_field = text_input("", &state.scale)
        .size(11)
        .padding([3, 6])
        .width(Fill)
        .style(field_style);
    if !state.scale_on_screen {
        scale_field = scale_field.on_input(|v| msg(PdfDialogMsg::AttachScale(v)));
    }
    let scale = group(
        t!("Scale").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.scale_on_screen, |v| msg(
                PdfDialogMsg::AttachScaleOnScreen(v)
            )),
            scale_field,
        ]
        .spacing(5),
        Length::Shrink,
    );
    let rotation = group(
        t!("Rotation").into_owned(),
        column![
            labeled_checkbox(t!("Specify On-screen"), state.rotation_on_screen, |v| msg(
                PdfDialogMsg::AttachRotationOnScreen(v)
            )),
            value_field(
                t!("Angle:").into_owned(),
                &state.rotation,
                !state.rotation_on_screen,
                44.0,
                |v| msg(PdfDialogMsg::AttachRotation(v))
            ),
        ]
        .spacing(5),
        Length::Shrink,
    );
    let right = column![scale, rotation].spacing(6).width(Length::FillPortion(1));

    let body = row![
        container(pages).width(Length::FillPortion(1)),
        middle,
        right
    ]
    .spacing(6)
    .width(Fill);

    let details: Element<'a, Message> = if state.details {
        group(
            t!("Location").into_owned(),
            column![
                read_only(t!("Found in:").into_owned(), &state.found_in, 92.0),
                read_only(t!("Saved path:").into_owned(), &state.saved_path, 92.0),
                read_only(t!("Page size:").into_owned(), &state.page_size, 92.0),
            ]
            .spacing(5),
            Length::Shrink,
        )
    } else {
        Space::new().height(0).into()
    };

    let details_label = if state.details {
        t!("Hide Details")
    } else {
        t!("Show Details")
    };
    let footer = row![button(text(details_label).size(11))
        .on_press(msg(PdfDialogMsg::AttachDetails(!state.details)))
        .style(button_style(false))
        .padding([4, 12])]
    .push(footer(PdfDialogMsg::AttachOk, "attach"))
    .align_y(iced::Center);

    column![name_section, body, details, footer]
        .spacing(6)
        .padding([8, 10])
        .width(sizing.width)
        .into()
}

// ── Underlay Layers ────────────────────────────────────────────────────────

/// One PDF underlay the dialog can show: its name, its file's layers, and
/// which of them it turns off.
#[derive(Clone)]
pub struct LayerTarget {
    pub name: String,
    pub handle: acadrust::Handle,
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
    let reference = row![
        text(t!("Reference name:")).size(11).width(Length::Fixed(110.0)),
        pick_list(target.map(|t| t.name.clone()), names, |n: &String| n.clone())
            .on_select(|n| msg(PdfDialogMsg::LayersUnderlay(n)))
            .text_size(11)
            .padding([3, 6])
            .width(Fill),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let search = text_input(t!("Search for layer").as_ref(), &state.search)
        .on_input(|s| msg(PdfDialogMsg::LayersSearch(s)))
        .size(11)
        .padding([3, 6])
        .width(Length::Fixed(220.0))
        .style(field_style);

    let header = row![
        text(t!("On")).size(11).style(muted_style).width(Length::Fixed(44.0)),
        text(t!("Name")).size(11).style(muted_style),
    ]
    .padding([4, 8]);
    let body: Element<'a, Message> = match target {
        Some(t) if !t.layers.is_empty() => {
            let needle = state.search.to_lowercase();
            let rows = t
                .layers
                .iter()
                .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
                .map(|name| {
                    let on = !t.hidden.contains(name);
                    let bulb = button(text("●").size(12).style(move |theme: &Theme| text::Style {
                        color: Some(if on {
                            theme.palette().warning.base.color
                        } else {
                            theme.palette().background.strong.color
                        }),
                    }))
                    .on_press(msg(PdfDialogMsg::LayersToggle(name.clone())))
                    .padding([0, 4])
                    .style(|_: &Theme, _| button::Style::default());
                    row![
                        container(bulb).width(Length::Fixed(44.0)),
                        text(name.clone()).size(11)
                    ]
                    .padding([2, 8])
                    .align_y(iced::Center)
                    .into()
                });
            scrollable(iced::widget::Column::with_children(rows.collect::<Vec<_>>()))
                .height(Fill)
                .into()
        }
        _ => container(text(t!("This file does not contain any layers.")).size(11))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .into(),
    };
    let list = container(column![header, body])
        .width(Fill)
        .height(Length::Fixed(300.0))
        .style(frame_style);

    column![
        text(t!("Select an underlay to view its layers.")).size(11),
        reference,
        search,
        list,
        footer(PdfDialogMsg::LayersOk, "layers"),
    ]
    .spacing(8)
    .padding([8, 10])
    .width(sizing.width)
    .into()
}

// ── PDF Import Settings ────────────────────────────────────────────────────

fn settings_groups<'a>(s: &'a PdfImportSettings) -> (Element<'a, Message>, Element<'a, Message>, Element<'a, Message>) {
    let fills: Element<'a, Message> = if s.vector {
        labeled_checkbox(t!("Solid fills"), s.fills, |v| msg(PdfDialogMsg::Fills(v)))
    } else {
        labeled_checkbox(t!("Solid fills"), s.fills, move |_| Message::Noop)
    };
    let data = group(
        t!("PDF data to import").into_owned(),
        column![
            labeled_checkbox(t!("Vector geometry"), s.vector, |v| msg(PdfDialogMsg::Vector(v))),
            container(fills).padding(iced::Padding { left: 20.0, ..Default::default() }),
            labeled_checkbox(t!("TrueType text"), s.text, |v| msg(PdfDialogMsg::Text(v))),
            labeled_checkbox(t!("Raster images"), s.raster, |v| msg(PdfDialogMsg::Raster(v))),
        ]
        .spacing(6),
        Length::Fill,
    );
    let layer_radio = |label: std::borrow::Cow<'static, str>, value: ImportLayers| {
        radio(label, value, Some(s.layers), |v| msg(PdfDialogMsg::Layers(v)))
            .size(14)
            .text_size(11)
    };
    let layers = group(
        t!("Layers").into_owned(),
        column![
            layer_radio(t!("Use PDF layers"), ImportLayers::Pdf),
            layer_radio(t!("Create object layers"), ImportLayers::Object),
            layer_radio(t!("Current layer"), ImportLayers::Current),
        ]
        .spacing(6),
        Length::Fill,
    );
    let options = group(
        t!("Import options").into_owned(),
        column![
            labeled_checkbox(t!("Import as block"), s.as_block, |v| msg(PdfDialogMsg::AsBlock(v))),
            labeled_checkbox(t!("Join line and arc segments"), s.join, |v| msg(
                PdfDialogMsg::Join(v)
            )),
            labeled_checkbox(t!("Convert solid fills to hatches"), s.hatches, |v| msg(
                PdfDialogMsg::Hatches(v)
            )),
            labeled_checkbox(t!("Apply lineweight properties"), s.lineweights, |v| msg(
                PdfDialogMsg::Lineweights(v)
            )),
            labeled_checkbox(t!("Infer linetypes from collinear dashes"), s.linetypes, |v| msg(
                PdfDialogMsg::Linetypes(v)
            )),
        ]
        .spacing(6),
        Length::Shrink,
    );
    (data, layers, options)
}

pub fn view_import_settings<'a>(
    settings: &'a PdfImportSettings,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let (data, layers, options) = settings_groups(settings);
    let side_button = |label: std::borrow::Cow<'static, str>, on: Message, accent: bool| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(on)
            .style(button_style(accent))
            .padding([4, 12])
            .width(Length::Fixed(110.0))
    };
    let left = column![
        row![data, layers].spacing(8).height(Length::Fixed(140.0)),
        options
    ]
    .spacing(8)
    .width(Fill);
    let right = column![
        side_button(t!("OK"), msg(PdfDialogMsg::SettingsOk), true),
        side_button(t!("Cancel"), Message::CloseModal, false),
        side_button(t!("Options..."), msg(PdfDialogMsg::Options), false),
        side_button(t!("Help"), msg(PdfDialogMsg::Help("settings")), false),
    ]
    .spacing(8);
    row![left, right]
        .spacing(10)
        .padding([8, 10])
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
    let file_row = row![
        text(t!("File name:")).size(11).width(Length::Fixed(70.0)),
        text_input("", &file_name)
            .size(11)
            .padding([3, 6])
            .width(Fill)
            .style(field_style),
        button(text(t!("Browse...")).size(11))
            .on_press(msg(PdfDialogMsg::ImportBrowse))
            .style(button_style(false))
            .padding([4, 12])
            .width(Length::Fixed(96.0)),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let selected = [state.selected];
    let page_group = group(
        t!("Page to import").into_owned(),
        column![
            row![
                text(t!("Page:")).size(11),
                text_input("", &state.page_text)
                    .on_input(|s| msg(PdfDialogMsg::ImportPageText(s)))
                    .size(11)
                    .padding([3, 6])
                    .width(Length::Fixed(56.0))
                    .style(field_style),
                text(crate::tf!("Total: {count}", count = state.pages.len()))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(8)
            .align_y(iced::Center),
            page_grid(&state.pages, &selected, PdfDialogMsg::ImportPage, 300.0),
            row![
                text(crate::tf!("Page size: {size}", size = state.page_size.clone()))
                    .size(11)
                    .style(muted_style),
            ],
        ]
        .spacing(6),
        Length::Shrink,
    );

    let location = group(
        t!("Location").into_owned(),
        column![
            labeled_checkbox(t!("Specify insertion point on-screen"), state.insert_on_screen, |v| {
                msg(PdfDialogMsg::ImportInsertOnScreen(v))
            }),
            row![
                text(t!("Scale:")).size(11).style(muted_style),
                text_input("", &state.scale)
                    .on_input(|s| msg(PdfDialogMsg::ImportScale(s)))
                    .size(11)
                    .padding([3, 6])
                    .width(Length::Fixed(80.0))
                    .style(field_style),
                Space::new().width(Length::Fixed(20.0)),
                text(t!("Rotation:")).size(11).style(muted_style),
                pick_list(Some(state.rotation), RotationChoice::ALL, |c| c.to_string())
                    .on_select(|v| msg(PdfDialogMsg::ImportRotation(v)))
                    .text_size(11)
                    .padding([3, 6])
                    .width(Length::Fixed(76.0)),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(6),
        Length::Shrink,
    );
    let (data, layers, options) = settings_groups(&state.settings);
    let right = column![
        location,
        row![data, layers].spacing(8).height(Length::Fixed(140.0)),
        options
    ]
    .spacing(8)
    .width(Length::FillPortion(1));
    let body = row![container(page_group).width(Length::FillPortion(1)), right].spacing(8);

    let footer = row![button(text(t!("Options...")).size(11))
        .on_press(msg(PdfDialogMsg::Options))
        .style(button_style(false))
        .padding([4, 12])]
    .push(footer(PdfDialogMsg::ImportOk, "import"))
    .align_y(iced::Center);

    column![file_row, body, footer]
        .spacing(8)
        .padding([8, 10])
        .width(sizing.width)
        .into()
}
