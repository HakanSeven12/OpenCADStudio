//! PDF dialogs: Attach PDF Underlay (pages, path type, insertion, scale,
//! rotation), Underlay Layers, PDF Import Settings and Import PDF, built
//! from cards, segmented choices and toggle chips; the point cloud Attach
//! and Color Map dialogs share them.

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
    /// DGN conversion units: sub (true) or master.
    AttachSubUnits(bool),
    AttachPathType(PathTypeChoice),
    AttachInsertOnScreen(bool),
    AttachInsert(usize, String),
    AttachScaleOnScreen(bool),
    AttachScale(String),
    AttachRotationOnScreen(bool),
    AttachRotation(String),
    AttachDetails(bool),
    AttachOk,
    // Attach Point Cloud
    CloudBrowse,
    CloudPathType(PathTypeChoice),
    CloudInsertOnScreen(bool),
    CloudInsert(usize, String),
    CloudScaleOnScreen(bool),
    CloudScale(String),
    CloudRotationOnScreen(bool),
    CloudRotation(String),
    CloudLock(bool),
    CloudZoom(bool),
    CloudOk,
    // Point Cloud Color Map
    /// Elevation tab (else intensity).
    MapTab(bool),
    MapScheme(String),
    MapCount(usize),
    MapEven,
    MapReverse,
    MapGradient(bool),
    MapNew,
    MapDelete,
    MapRename,
    MapNameInput(String),
    MapNameOk,
    MapNameCancel,
    MapMax(String),
    MapMin(String),
    MapInterval(String),
    MapExtents(bool),
    MapOutOfRange(OutOfRange),
    MapCurrent(bool),
    MapApply,
    MapOk,
    // Extract Section Lines from Point Cloud
    SecPerimeter(bool),
    SecMaxPoints(String),
    SecLayer(String),
    SecColor(LineColor),
    SecPolylines(bool),
    SecWidth(String),
    SecMinLength(String),
    SecConnect(String),
    SecAngle(String),
    /// Measure the connect tolerance (else the minimum length) on screen.
    SecPick(bool),
    SecPreview(bool),
    SecCreate,
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
    // View background
    BgKind(BgKind),
    /// Open the colour window for a slot: 0 solid, 1 top, 2 middle, 3 bottom.
    BgPickColor(u8),
    BgColor(u8, [u8; 3]),
    BgThree(bool),
    BgRotation(String),
    BgImage(String),
    BgBrowse,
    BgPosition(BgPosition),
    BgAspect(bool),
    /// An image background's offset (pixels, x and y) and scale, as typed.
    BgOffset([String; 2]),
    BgScale([String; 2]),
    /// A spinner step on offset x / y (0, 1) or scale x / y (2, 3).
    BgStep(u8, i8),
    /// Offset 0, scale 1.
    BgReset,
    BgApply,
    // Image Adjust: rows 0 brightness, 1 contrast, 2 fade.
    /// A row's value from its bar.
    IaValue(u8, u8),
    /// A row's box, as typed.
    IaText(u8, String),
    /// One step down (-1) or up (+1).
    IaStep(u8, i8),
    /// Brightness 50, contrast 50, fade 0.
    IaReset,
    IaApply,
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
                Some(image::Handle::from_rgba(
                    page.width,
                    page.height,
                    page.pixels.to_vec(),
                ))
            });
            PageThumb { label, image }
        })
        .collect()
}

/// Thumbnails of a file's pages (PDF), sheets (DWF) or models (DGN).
pub fn item_thumbs(kind: codec::entities::UnderlayType, path: &str) -> Vec<PageThumb> {
    use crate::scene::model::underlay_vector;
    if kind == codec::entities::UnderlayType::Pdf {
        return page_thumbs(path);
    }
    underlay_vector::item_names(kind, path)
        .unwrap_or_default()
        .into_iter()
        .map(|label| {
            let backdrop = underlay_vector::Backdrop {
                max_channel: 0,
                light: false,
            };
            let image = underlay_vector::display_raster(kind, path, &label, &[], 220.0, backdrop)
                .map(|page| {
                    image::Handle::from_rgba(page.width, page.height, page.pixels.to_vec())
                });
            PageThumb { label, image }
        })
        .collect()
}

/// Ctrl adds or removes a page, Shift takes the range from the last click,
/// a plain click selects that page alone.
pub fn click_page(
    selected: &mut Vec<usize>,
    anchor: &mut usize,
    page: usize,
    ctrl: bool,
    shift: bool,
) {
    if shift {
        let (a, b) = if *anchor <= page {
            (*anchor, page)
        } else {
            (page, *anchor)
        };
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

pub(crate) fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

pub(crate) fn card_style(theme: &Theme) -> container::Style {
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

pub(crate) fn well_style(theme: &Theme) -> container::Style {
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
pub(crate) fn card<'a>(title: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![
            text(title.to_uppercase()).size(10).style(accent_text),
            content.into()
        ]
        .spacing(8),
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
    // The tick is an SVG: the web build's font has no check-mark glyph.
    let mut content = row![].spacing(4).align_y(iced::Center);
    if on {
        content = content.push(
            iced::widget::svg(crate::ui::icons::themed_handle(crate::ui::icons::CHECK))
                .width(11.0)
                .height(11.0)
                .style(|theme: &Theme, _| iced::widget::svg::Style {
                    color: Some(theme.palette().primary.base.text),
                }),
        );
    }
    button(content.push(text(label).size(11)))
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
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(label_width)),
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
        format!(
            "…{}",
            value.chars().skip(count - (MAX - 1)).collect::<String>()
        )
    } else {
        value.to_string()
    };
    row![
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(96.0)),
        text(shown)
            .size(11)
            .width(Fill)
            .wrapping(iced::advanced::text::Wrapping::None),
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
        let tick: Element<'a, Message> = if chosen {
            crate::ui::icons::themed_primary(crate::ui::icons::CHECK, 11.0)
        } else {
            Space::new().width(0.0).into()
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
            container(
                row![
                    tick,
                    text(page.label.clone())
                        .size(11)
                        .style(move |theme: &Theme| text::Style {
                            color: Some(if chosen {
                                theme.palette().primary.base.color
                            } else {
                                theme.palette().background.base.text.scale_alpha(0.68)
                            }),
                        })
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
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
    /// PDF pages, DWF sheets or DGN models.
    pub kind: codec::entities::UnderlayType,
    /// DGN: sub units chosen (the scale then turns them into master units).
    pub sub_units: bool,
    /// DGN: sub units per master unit of the chosen model.
    pub sub_per_master: f64,
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
            kind: codec::entities::UnderlayType::Pdf,
            sub_units: false,
            sub_per_master: 1.0,
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
    use codec::entities::UnderlayType;
    let count_text = match state.kind {
        UnderlayType::Pdf => crate::tf!("{count} pages", count = state.pages.len()),
        UnderlayType::Dwf => crate::tf!("{count} sheets", count = state.pages.len()),
        UnderlayType::Dgn => crate::tf!("{count} models", count = state.pages.len()),
    };
    let count = container(text(count_text).size(11).style(accent_text))
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
    let (items_title, chosen_text, size_label) = match state.kind {
        UnderlayType::Pdf => (
            t!("Pages"),
            crate::tf!("Selected pages: {pages}", pages = chosen),
            t!("Page size:"),
        ),
        UnderlayType::Dwf => (
            t!("Sheets"),
            crate::tf!("Selected sheet: {sheet}", sheet = chosen),
            t!("Sheet size:"),
        ),
        UnderlayType::Dgn => (
            t!("Models"),
            crate::tf!("Selected model: {model}", model = chosen),
            t!("Model size:"),
        ),
    };
    let pages = card(
        items_title.into_owned(),
        column![
            container(page_tiles(
                &state.pages,
                &state.selected,
                PdfDialogMsg::AttachPage
            ))
            .padding(6)
            .height(Length::Fixed(226.0))
            .width(Fill)
            .style(well_style),
            text(chosen_text).size(11).style(muted_style),
        ]
        .spacing(6),
    );

    // Placement: insertion point, scale and rotation, each with an
    // "On screen" chip that leaves the value to the command line.
    let on_screen = t!("On screen").into_owned();
    let insert_enabled = !state.insert_on_screen;
    let xyz = row![
        field("X".into(), &state.insert[0], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::AttachInsert(0, v)
        )),
        field("Y".into(), &state.insert[1], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::AttachInsert(1, v)
        )),
        field("Z".into(), &state.insert[2], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::AttachInsert(2, v)
        )),
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
            field(
                String::new(),
                &state.scale,
                !state.scale_on_screen,
                0.0,
                |v| msg(PdfDialogMsg::AttachScale(v))
            ),
            heading(
                t!("Rotation"),
                state.rotation_on_screen,
                msg(PdfDialogMsg::AttachRotationOnScreen(
                    !state.rotation_on_screen
                ))
            ),
            field(
                String::new(),
                &state.rotation,
                !state.rotation_on_screen,
                0.0,
                |v| msg(PdfDialogMsg::AttachRotation(v))
            ),
        ]
        .spacing(6),
    );

    let path = card(
        t!("Path type").into_owned(),
        segmented(
            PathTypeChoice::ALL
                .iter()
                .map(|c| (*c, c.to_string()))
                .collect(),
            state.path_type,
            PdfDialogMsg::AttachPathType,
        ),
    );

    let details = card(
        t!("File details").into_owned(),
        column![
            info_line(t!("Found in:").into_owned(), &state.found_in),
            info_line(t!("Saved path:").into_owned(), &state.saved_path),
            info_line(size_label.into_owned(), &state.page_size),
        ]
        .spacing(4),
    );

    let mut right = column![placement, path]
        .spacing(8)
        .width(Length::FillPortion(2));
    // A DGN model is attached in its master or its sub units.
    if state.kind == UnderlayType::Dgn {
        right = right.push(card(
            t!("Conversion units").into_owned(),
            column![
                segmented(
                    vec![
                        (false, t!("Master units").into_owned()),
                        (true, t!("Sub units").into_owned())
                    ],
                    state.sub_units,
                    PdfDialogMsg::AttachSubUnits,
                ),
                text(crate::tf!(
                    "Sub units make the default scale {scale}",
                    scale = 1.0 / state.sub_per_master.max(1e-12)
                ))
                .size(11)
                .style(muted_style),
            ]
            .spacing(6),
        ));
    }
    let body = row![
        column![pages, details]
            .spacing(8)
            .width(Length::FillPortion(3)),
        right,
    ]
    .spacing(10);

    column![
        file,
        body,
        footer("attach", None, t!("Attach"), PdfDialogMsg::AttachOk)
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

// ── Attach Point Cloud ─────────────────────────────────────────────────────

/// The Attach Point Cloud dialog: the scan or project chosen, what it
/// holds, and how it is placed.
pub struct PointCloudAttachState {
    /// The file read (absolute).
    pub path: String,
    pub name: String,
    pub preview: Option<image::Handle>,
    /// "1 scan · 365281 points".
    pub summary: String,
    /// Which data the scans carry (colour, intensity, normals; then
    /// classification and segmentation).
    pub data: String,
    pub data_more: String,
    /// Width × length × height.
    pub size: String,
    pub unit: String,
    pub found_in: String,
    pub saved_path: String,
    pub path_type: PathTypeChoice,
    pub insert_on_screen: bool,
    pub insert: [String; 3],
    pub scale_on_screen: bool,
    pub scale: String,
    pub rotation_on_screen: bool,
    pub rotation: String,
    pub lock: bool,
    pub zoom: bool,
}

/// A switch that cannot be used here: dimmed, not pressable.
fn off_chip<'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(11).style(muted_style))
        .padding([4, 10])
        .into()
}

pub fn view_point_cloud_attach<'a>(
    state: &'a PointCloudAttachState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let file = row![
        text_input("", &state.name)
            .size(12)
            .padding([5, 8])
            .width(Fill)
            .style(field_style),
        container(text(state.summary.clone()).size(11).style(accent_text))
            .padding([4, 10])
            .style(well_style),
        button(text(t!("Browse...")).size(11))
            .on_press(msg(PdfDialogMsg::CloudBrowse))
            .style(button_style(false))
            .padding([5, 12]),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let picture: Element<'a, Message> = match &state.preview {
        Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let preview = card(
        t!("Preview").into_owned(),
        container(picture)
            .padding(6)
            .height(Length::Fixed(226.0))
            .width(Fill)
            .style(well_style),
    );

    let details = card(
        t!("File details").into_owned(),
        column![
            info_line(t!("Found in:").into_owned(), &state.found_in),
            info_line(t!("Saved path:").into_owned(), &state.saved_path),
            info_line(t!("Data:").into_owned(), &state.data),
            info_line(String::new(), &state.data_more),
            info_line(t!("Size:").into_owned(), &state.size),
            info_line(t!("Unit:").into_owned(), &state.unit),
        ]
        .spacing(4),
    );

    let on_screen = t!("On screen").into_owned();
    let heading = |label: std::borrow::Cow<'static, str>, on: bool, message: Message| {
        row![
            text(label).size(11).width(Fill),
            chip(on_screen.clone(), on, message)
        ]
        .align_y(iced::Center)
    };
    let insert_enabled = !state.insert_on_screen;
    let xyz = row![
        field("X".into(), &state.insert[0], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::CloudInsert(0, v)
        )),
        field("Y".into(), &state.insert[1], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::CloudInsert(1, v)
        )),
        field("Z".into(), &state.insert[2], insert_enabled, 12.0, |v| msg(
            PdfDialogMsg::CloudInsert(2, v)
        )),
    ]
    .spacing(8);
    let placement = card(
        t!("Placement").into_owned(),
        column![
            heading(
                t!("Insertion point"),
                state.insert_on_screen,
                msg(PdfDialogMsg::CloudInsertOnScreen(!state.insert_on_screen))
            ),
            xyz,
            heading(
                t!("Scale"),
                state.scale_on_screen,
                msg(PdfDialogMsg::CloudScaleOnScreen(!state.scale_on_screen))
            ),
            field(
                String::new(),
                &state.scale,
                !state.scale_on_screen,
                0.0,
                |v| msg(PdfDialogMsg::CloudScale(v))
            ),
            heading(
                t!("Rotation"),
                state.rotation_on_screen,
                msg(PdfDialogMsg::CloudRotationOnScreen(
                    !state.rotation_on_screen
                ))
            ),
            field(
                String::new(),
                &state.rotation,
                !state.rotation_on_screen,
                0.0,
                |v| msg(PdfDialogMsg::CloudRotation(v))
            ),
        ]
        .spacing(6),
    );
    let path = card(
        t!("Path type").into_owned(),
        segmented(
            PathTypeChoice::ALL
                .iter()
                .map(|c| (*c, c.to_string()))
                .collect(),
            state.path_type,
            PdfDialogMsg::CloudPathType,
        ),
    );
    let options = card(
        t!("Options").into_owned(),
        column![
            off_chip(t!("Use geographic location").into_owned()),
            chip(
                t!("Lock point cloud").into_owned(),
                state.lock,
                msg(PdfDialogMsg::CloudLock(!state.lock))
            ),
            chip(
                t!("Zoom to point cloud").into_owned(),
                state.zoom,
                msg(PdfDialogMsg::CloudZoom(!state.zoom))
            ),
        ]
        .spacing(6),
    );

    let body = row![
        column![preview, details]
            .spacing(8)
            .width(Length::FillPortion(3)),
        column![placement, path, options]
            .spacing(8)
            .width(Length::FillPortion(2)),
    ]
    .spacing(10);
    column![
        file,
        body,
        footer("pointcloud", None, t!("Attach"), PdfDialogMsg::CloudOk)
    ]
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
    let has_underlays = !names.is_empty();
    let underlay = pick_list(target.map(|t| t.name.clone()), names, |n: &String| {
        n.clone()
    });
    // No underlays: the list stays empty and disabled.
    let underlay = if has_underlays {
        underlay.on_select(|n| msg(PdfDialogMsg::LayersUnderlay(n)))
    } else {
        underlay
    }
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
    let summary = text(crate::tf!(
        "{count} layers, {hidden} hidden",
        count = total,
        hidden = hidden
    ))
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
                        row![text(name.clone()).size(12).width(Fill), switch].align_y(iced::Center),
                    )
                    .padding([5, 10])
                    .style(well_style)
                    .into()
                });
            scrollable(iced::widget::Column::with_children(rows.collect::<Vec<_>>()).spacing(4))
                .height(Fill)
                .into()
        }
        _ => container(
            text(t!("This file does not contain any layers"))
                .size(11)
                .style(muted_style),
        )
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
            column![
                search,
                summary,
                container(body).height(Length::Fixed(250.0))
            ]
            .spacing(6)
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
fn settings_cards<'a>(
    s: &'a PdfImportSettings,
) -> (
    Element<'a, Message>,
    Element<'a, Message>,
    Element<'a, Message>,
) {
    let fills = if s.vector {
        chip(
            t!("Solid fills").into_owned(),
            s.fills,
            msg(PdfDialogMsg::Fills(!s.fills)),
        )
    } else {
        chip(t!("Solid fills").into_owned(), s.fills, Message::Noop)
    };
    let content = card(
        t!("PDF data to import").into_owned(),
        row![
            chip(
                t!("Vector geometry").into_owned(),
                s.vector,
                msg(PdfDialogMsg::Vector(!s.vector))
            ),
            fills,
            chip(
                t!("TrueType text").into_owned(),
                s.text,
                msg(PdfDialogMsg::Text(!s.text))
            ),
            chip(
                t!("Raster images").into_owned(),
                s.raster,
                msg(PdfDialogMsg::Raster(!s.raster))
            ),
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
                (
                    ImportLayers::Object,
                    t!("Create object layers").into_owned(),
                ),
                (ImportLayers::Current, t!("Current layer").into_owned()),
            ],
            s.layers,
            PdfDialogMsg::Layers,
        ),
    );
    let options = card(
        t!("Import options").into_owned(),
        row![
            chip(
                t!("Import as block").into_owned(),
                s.as_block,
                msg(PdfDialogMsg::AsBlock(!s.as_block))
            ),
            chip(
                t!("Join line and arc segments").into_owned(),
                s.join,
                msg(PdfDialogMsg::Join(!s.join))
            ),
            chip(
                t!("Convert solid fills to hatches").into_owned(),
                s.hatches,
                msg(PdfDialogMsg::Hatches(!s.hatches))
            ),
            chip(
                t!("Apply lineweight properties").into_owned(),
                s.lineweights,
                msg(PdfDialogMsg::Lineweights(!s.lineweights))
            ),
            chip(
                t!("Infer linetypes from collinear dashes").into_owned(),
                s.linetypes,
                msg(PdfDialogMsg::Linetypes(!s.linetypes))
            ),
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
        footer(
            "settings",
            Some(more.into()),
            t!("Save"),
            PdfDialogMsg::SettingsOk
        ),
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
    let preview: Element<'a, Message> = match state
        .pages
        .get(state.selected)
        .and_then(|p| p.image.clone())
    {
        Some(handle) => image(handle).width(Fill).height(Fill).into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let step = |icon: Element<'a, Message>, to: Option<usize>| {
        button(icon)
            .on_press_maybe(to.map(|p| msg(PdfDialogMsg::ImportPage(p))))
            .style(button_style(false))
            .padding([2, 12])
    };
    let navigator = row![
        step(
            crate::ui::icons::themed_arrow_left(12.0),
            state.selected.checked_sub(1)
        ),
        Space::new().width(Fill),
        text_input("", &state.page_text)
            .on_input(|s| msg(PdfDialogMsg::ImportPageText(s)))
            .size(11)
            .padding([3, 6])
            .width(Length::Fixed(48.0))
            .style(field_style),
        text(format!("/ {count}")).size(11).style(muted_style),
        Space::new().width(Fill),
        step(
            crate::ui::icons::themed_arrow_right(12.0),
            (state.selected + 1 < count).then_some(state.selected + 1),
        ),
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
                    border: Border {
                        width: 0.0,
                        radius: 6.0.into(),
                        color: iced::Color::TRANSPARENT
                    },
                    ..Default::default()
                }),
            navigator,
            row![
                text(crate::tf!(
                    "Page size: {size}",
                    size = state.page_size.clone()
                ))
                .size(11)
                .style(muted_style),
                Space::new().width(Fill),
                text(crate::tf!(
                    "PDF scale: {scale}:1",
                    scale = state.scale.trim()
                ))
                .size(11)
                .style(muted_style),
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
            field(t!("Scale").into_owned(), &state.scale, true, 60.0, |s| msg(
                PdfDialogMsg::ImportScale(s)
            )),
            row![
                text(t!("Rotation"))
                    .size(11)
                    .style(muted_style)
                    .width(Length::Fixed(60.0)),
                segmented(
                    RotationChoice::ALL
                        .iter()
                        .map(|r| (*r, format!("{}\u{b0}", r.0)))
                        .collect(),
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
    column![
        file,
        body,
        footer(
            "import",
            Some(more.into()),
            t!("Import"),
            PdfDialogMsg::ImportOk
        )
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

// ── Point Cloud Color Map ──────────────────────────────────────────────────

/// A colour scheme as the dialog edits it: identifier, name and colours
/// (high end first).
#[derive(Debug, Clone, PartialEq)]
pub struct MapRamp {
    pub id: String,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

/// What a scheme does with values outside its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfRange(pub i16);

impl OutOfRange {
    pub const ALL: [OutOfRange; 3] = [OutOfRange(0), OutOfRange(1), OutOfRange(2)];
}

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.0 {
            0 => t!("Use min/max colors"),
            2 => t!("Hide points"),
            _ => t!("Use RGB scan colors"),
        };
        f.write_str(label.as_ref())
    }
}

/// One tab's settings: its scheme and range.
#[derive(Debug, Clone)]
pub struct MapSlot {
    pub scheme: String,
    pub gradient: bool,
    pub max: String,
    pub min: String,
    pub out_of_range: OutOfRange,
}

/// The Point Cloud Color Map dialog: the drawing's colour schemes and, for
/// a chosen cloud, its intensity and elevation colouring.
pub struct PointCloudColorMapState {
    /// The cloud edited; `None` edits the drawing's default schemes.
    pub cloud: Option<codec::Handle>,
    /// Elevation tab (else intensity).
    pub elevation_tab: bool,
    pub ramps: Vec<MapRamp>,
    pub intensity: MapSlot,
    pub elevation: MapSlot,
    /// Elevation over the cloud's own height (else the fixed range).
    pub extents: bool,
    /// The cloud's height range.
    pub height: (f64, f64),
    pub make_current: bool,
    /// The height of one band (elevation), as shown and typed.
    pub interval: String,
    /// A name being typed for a new scheme (`true`) or a rename.
    pub naming: Option<(bool, String)>,
}

impl PointCloudColorMapState {
    pub fn slot(&self) -> &MapSlot {
        if self.elevation_tab {
            &self.elevation
        } else {
            &self.intensity
        }
    }

    pub fn slot_mut(&mut self) -> &mut MapSlot {
        if self.elevation_tab {
            &mut self.elevation
        } else {
            &mut self.intensity
        }
    }

    pub fn ramp(&self) -> Option<&MapRamp> {
        let id = &self.slot().scheme;
        self.ramps
            .iter()
            .find(|r| r.id.eq_ignore_ascii_case(id))
            .or(self.ramps.first())
    }

    pub fn ramp_mut(&mut self) -> Option<&mut MapRamp> {
        let id = self.slot().scheme.clone();
        let at = self
            .ramps
            .iter()
            .position(|r| r.id.eq_ignore_ascii_case(&id))
            .unwrap_or(0);
        self.ramps.get_mut(at)
    }

    /// The range the ramp spans on the current tab.
    pub fn range(&self) -> Option<(f64, f64)> {
        if self.elevation_tab && self.extents {
            return Some(self.height);
        }
        let slot = self.slot();
        Some((slot.min.trim().parse().ok()?, slot.max.trim().parse().ok()?))
    }
}

/// `t` (0 at the high end) along colours, blended; channels are cut to
/// whole values, as the reference stores resampled schemes.
pub fn blend(colors: &[[u8; 3]], t: f64) -> [u8; 3] {
    if colors.len() < 2 {
        return colors.first().copied().unwrap_or([255; 3]);
    }
    let x = t.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
    let k = (x as usize).min(colors.len() - 2);
    let f = x - k as f64;
    let (a, b) = (colors[k], colors[k + 1]);
    [0, 1, 2].map(|c| (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * f) as u8)
}

/// A number as the dialog labels it: at most two decimals.
pub fn short(value: f64) -> String {
    let value = if value.abs() < 0.005 { 0.0 } else { value };
    let text = format!("{:.2}", value);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn swatch<'a>(color: [u8; 3], width: f32, height: f32) -> Element<'a, Message> {
    let color = iced::Color::from_rgb8(color[0], color[1], color[2]);
    container(Space::new().width(width).height(height))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                width: 1.0,
                radius: 3.0.into(),
                color: iced::Color::from_rgb8(0x77, 0x77, 0x77),
            },
            ..Default::default()
        })
        .into()
}

/// The ramp as a bar, high end on top, blended or in bands, with the values
/// where the bands meet (blended: the ends and the inner band centres).
fn ramp_bar<'a>(
    colors: &[[u8; 3]],
    gradient: bool,
    range: Option<(f64, f64)>,
    percent: bool,
) -> Element<'a, Message> {
    const HEIGHT: f32 = 360.0;
    let n = colors.len().max(1);
    let strips: Vec<Element<'a, Message>> = if gradient {
        (0..72)
            .map(|k| {
                let c = blend(colors, (k as f64 + 0.5) / 72.0);
                let color = iced::Color::from_rgb8(c[0], c[1], c[2]);
                container(Space::new().width(Fill).height(Fill))
                    .height(Length::FillPortion(1))
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(color)),
                        ..Default::default()
                    })
                    .into()
            })
            .collect()
    } else {
        colors
            .iter()
            .map(|c| {
                let color = iced::Color::from_rgb8(c[0], c[1], c[2]);
                container(Space::new().width(Fill).height(Fill))
                    .height(Length::FillPortion(1))
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(color)),
                        ..Default::default()
                    })
                    .into()
            })
            .collect()
    };
    let bar = container(iced::widget::Column::with_children(strips))
        .width(Length::Fixed(56.0))
        .height(Length::Fixed(HEIGHT));
    // Label positions from the top (0) to the bottom (1).
    let marks: Vec<f64> = if gradient {
        let mut marks = vec![0.0];
        marks.extend((1..n.saturating_sub(1)).map(|k| (k as f64 + 0.5) / n as f64));
        marks.push(1.0);
        marks
    } else {
        (0..=n).map(|k| k as f64 / n as f64).collect()
    };
    let mut labels = column![];
    if let Some((low, high)) = range {
        // Each label is centred on its mark, kept inside the bar.
        let mut used = 0.0;
        for mark in marks {
            let value = high - (high - low) * mark;
            let label = if percent {
                format!("{}%", value.round())
            } else {
                short(value)
            };
            let top = (mark * f64::from(HEIGHT) - 7.0).clamp(0.0, f64::from(HEIGHT) - 14.0);
            labels = labels
                .push(Space::new().height((top - used).max(0.0) as f32))
                .push(text(label).size(10).height(14.0));
            used = top.max(used) + 14.0;
        }
    }
    row![bar, labels.width(Length::Fixed(44.0))]
        .spacing(4)
        .into()
}

pub fn view_point_cloud_color_map<'a>(
    state: &'a PointCloudColorMapState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let tab = |label: std::borrow::Cow<'static, str>,
               on: bool,
               message: Option<Message>|
     -> Element<'a, Message> {
        let mut b = button(text(label).size(11).width(Fill).align_x(iced::Center))
            .style(button_style(on))
            .padding([5, 6])
            .width(Length::Fixed(118.0));
        if let Some(message) = message {
            b = b.on_press(message);
        }
        b.into()
    };
    let tabs = container(
        row![
            tab(
                t!("Intensity"),
                !state.elevation_tab,
                Some(msg(PdfDialogMsg::MapTab(false)))
            ),
            tab(
                t!("Elevation"),
                state.elevation_tab,
                Some(msg(PdfDialogMsg::MapTab(true)))
            ),
            tab(t!("Classification"), false, None),
        ]
        .spacing(2),
    )
    .padding(2)
    .style(well_style);

    let slot = state.slot();
    let ramp = state.ramp();
    let colors: Vec<[u8; 3]> = ramp.map(|r| r.colors.clone()).unwrap_or_default();
    let ramp_card = card(
        t!("Color ramp").into_owned(),
        ramp_bar(
            &colors,
            slot.gradient,
            state.cloud.and(state.range()),
            !state.elevation_tab,
        ),
    );

    // Listed by name, as the reference lists them.
    let mut names: Vec<String> = state.ramps.iter().map(|r| r.name.clone()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    let scheme = pick_list(ramp.map(|r| r.name.clone()), names, |n: &String| n.clone())
        .on_select(|n| msg(PdfDialogMsg::MapScheme(n)))
        .text_size(12)
        .padding([5, 8])
        .width(Fill);
    let small = |label: std::borrow::Cow<'static, str>, message: PdfDialogMsg| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(message))
            .style(button_style(false))
            .padding([5, 8])
            .width(Length::Fixed(118.0))
    };
    let counts: Vec<usize> = (2..=25).collect();
    let count = pick_list(Some(colors.len().max(2)), counts, |n: &usize| n.to_string())
        .on_select(|n| msg(PdfDialogMsg::MapCount(n)))
        .text_size(12)
        .padding([4, 8])
        .width(Length::Fixed(70.0));
    // Blend: a strip from the first colour to the last.
    let ends = [
        colors.first().copied().unwrap_or([255; 3]),
        colors.last().copied().unwrap_or([255; 3]),
    ];
    let blend_strip = row((0..6).map(|k| swatch(blend(&ends, k as f64 / 5.0), 3.0, 14.0)));
    let even = button(blend_strip)
        .on_press(msg(PdfDialogMsg::MapEven))
        .style(button_style(false))
        .padding([5, 6]);
    let reverse = button(
        iced::widget::svg(crate::ui::icons::themed_handle(include_bytes!(
            "../../../assets/icons/modify_reverse.svg"
        )))
        .width(16.0)
        .height(16.0),
    )
    .on_press(msg(PdfDialogMsg::MapReverse))
    .style(button_style(false))
    .padding([4, 6]);
    let mut scheme_rows = column![
        row![scheme, small(t!("New"), PdfDialogMsg::MapNew)]
            .spacing(8)
            .align_y(iced::Center),
        row![
            text(t!("Number of colors"))
                .size(11)
                .width(Length::Fixed(110.0)),
            count,
            even,
            reverse,
            Space::new().width(Fill),
            small(t!("Delete"), PdfDialogMsg::MapDelete)
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            chip(
                t!("Display as gradient").into_owned(),
                slot.gradient,
                msg(PdfDialogMsg::MapGradient(!slot.gradient))
            ),
            Space::new().width(Fill),
            small(t!("Rename"), PdfDialogMsg::MapRename)
        ]
        .align_y(iced::Center),
    ]
    .spacing(8);
    if let Some((_, name)) = &state.naming {
        scheme_rows = scheme_rows.push(
            row![
                text(t!("Scheme name"))
                    .size(11)
                    .style(muted_style)
                    .width(Length::Fixed(110.0)),
                text_input("", name)
                    .on_input(|v| msg(PdfDialogMsg::MapNameInput(v)))
                    .on_submit(msg(PdfDialogMsg::MapNameOk))
                    .size(11)
                    .padding([4, 8])
                    .width(Fill)
                    .style(field_style),
                dialog_button(t!("Cancel"), msg(PdfDialogMsg::MapNameCancel), false),
                dialog_button(t!("OK"), msg(PdfDialogMsg::MapNameOk), true),
            ]
            .spacing(6)
            .align_y(iced::Center),
        );
    }
    let scheme_card = card(t!("Color scheme").into_owned(), scheme_rows);

    let has_cloud = state.cloud.is_some();
    let top = colors.first().copied().unwrap_or([255; 3]);
    let bottom = colors.last().copied().unwrap_or([255; 3]);
    let editable = has_cloud && !(state.elevation_tab && state.extents);
    let with_swatch = |field: Element<'a, Message>, color: [u8; 3]| -> Element<'a, Message> {
        row![field, swatch(color, 22.0, 22.0)]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    };
    let out_of_range = {
        let list = pick_list(
            Some(slot.out_of_range),
            OutOfRange::ALL.to_vec(),
            |o: &OutOfRange| o.to_string(),
        )
        .text_size(12)
        .padding([4, 8])
        .width(Fill);
        let list = if editable {
            list.on_select(|o| msg(PdfDialogMsg::MapOutOfRange(o)))
        } else {
            list
        };
        row![
            text(t!("Out of range points"))
                .size(11)
                .style(muted_style)
                .width(Length::Fixed(150.0)),
            list
        ]
        .spacing(6)
        .align_y(iced::Center)
    };
    let range_rows = if state.elevation_tab {
        let (low, high) = state.height;
        let interval = state.interval.as_str();
        column![
            chip(
                format!(
                    "{} ({} ~ {})",
                    t!("Apply to extents of point cloud"),
                    short(low),
                    short(high)
                ),
                state.extents,
                msg(PdfDialogMsg::MapExtents(!state.extents))
            ),
            field(
                t!("Interval height").into_owned(),
                interval,
                editable,
                150.0,
                |v| msg(PdfDialogMsg::MapInterval(v))
            ),
            with_swatch(
                field(
                    t!("Maximum elevation").into_owned(),
                    &slot.max,
                    editable,
                    150.0,
                    |v| msg(PdfDialogMsg::MapMax(v))
                ),
                top
            ),
            with_swatch(
                field(
                    t!("Minimum elevation").into_owned(),
                    &slot.min,
                    editable,
                    150.0,
                    |v| msg(PdfDialogMsg::MapMin(v))
                ),
                bottom
            ),
            out_of_range,
        ]
        .spacing(8)
    } else {
        column![
            with_swatch(
                field(
                    t!("Maximum intensity").into_owned(),
                    &slot.max,
                    editable,
                    150.0,
                    |v| msg(PdfDialogMsg::MapMax(v))
                ),
                top
            ),
            with_swatch(
                field(
                    t!("Minimum intensity").into_owned(),
                    &slot.min,
                    editable,
                    150.0,
                    |v| msg(PdfDialogMsg::MapMin(v))
                ),
                bottom
            ),
            out_of_range,
        ]
        .spacing(8)
    };
    let range_card = card(t!("Range of colorized points").into_owned(), range_rows);

    let body = row![
        container(ramp_card).width(Length::Fixed(150.0)),
        column![scheme_card, range_card].spacing(8).width(Fill),
    ]
    .spacing(10);

    let mut left = row![button(text("?").size(12))
        .on_press(msg(PdfDialogMsg::Help("pointcloudcolormap")))
        .style(button_style(false))
        .padding([5, 11])]
    .spacing(8)
    .align_y(iced::Center);
    if has_cloud {
        left = left.push(chip(
            t!("Make stylization current").into_owned(),
            state.make_current,
            msg(PdfDialogMsg::MapCurrent(!state.make_current)),
        ));
    }
    let footer = row![
        left,
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("Apply"), msg(PdfDialogMsg::MapApply), false),
        dialog_button(t!("OK"), msg(PdfDialogMsg::MapOk), true),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![tabs, body, footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

// ── Extract Section Lines from Point Cloud ─────────────────────────────────

/// A colour the extracted lines can take (ACI; 256 ByLayer, 0 ByBlock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineColor(pub i16);

impl LineColor {
    pub const ALL: [LineColor; 9] = [
        LineColor(256),
        LineColor(0),
        LineColor(1),
        LineColor(2),
        LineColor(3),
        LineColor(4),
        LineColor(5),
        LineColor(6),
        LineColor(7),
    ];
}

impl fmt::Display for LineColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.0 {
            256 => t!("ByLayer"),
            0 => t!("ByBlock"),
            1 => t!("Red"),
            2 => t!("Yellow"),
            3 => t!("Green"),
            4 => t!("Cyan"),
            5 => t!("Blue"),
            6 => t!("Magenta"),
            _ => t!("White"),
        };
        f.write_str(label.as_ref())
    }
}

/// The dialog's fields; numbers as typed.
pub struct PcSectionState {
    pub perimeter: bool,
    pub max_points: String,
    /// "Use Current" first, then the drawing's layers.
    pub layers: Vec<String>,
    pub layer: String,
    pub color: LineColor,
    pub polylines: bool,
    pub width: String,
    pub min_length: String,
    pub connect: String,
    pub angle: String,
    pub preview: bool,
}

pub fn view_pc_section<'a>(
    state: &'a PcSectionState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let extract = card(
        t!("Extract").into_owned(),
        segmented(
            vec![
                (false, t!("Entire cross section").into_owned()),
                (true, t!("Perimeter only").into_owned()),
            ],
            state.perimeter,
            PdfDialogMsg::SecPerimeter,
        ),
    );
    let points = state
        .max_points
        .trim()
        .parse::<f32>()
        .unwrap_or(18_000.0)
        .clamp(1_000.0, 200_000.0);
    // ponytail: the time shown scales with the points (18000 ≈ 1 minute, as
    // the reference shows it); the real rate depends on the machine.
    let minutes = (points / 18_000.0).ceil().max(1.0) as u32;
    let max_points = card(
        t!("Maximum points to process").into_owned(),
        column![
            row![
                iced::widget::slider(1_000.0..=200_000.0, points, |v| msg(
                    PdfDialogMsg::SecMaxPoints(((v / 1_000.0).round() * 1_000.0).to_string())
                ))
                .step(1_000.0)
                .width(Fill),
                text_input("", &state.max_points)
                    .on_input(|v| msg(PdfDialogMsg::SecMaxPoints(v)))
                    .size(11)
                    .padding([4, 8])
                    .width(Length::Fixed(90.0))
                    .style(field_style),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text(t!("Faster")).size(10).style(muted_style),
                Space::new().width(Fill),
                text(t!("More accurate")).size(10).style(muted_style),
                Space::new().width(Fill),
                text(crate::tf!(
                    "Estimated time: {count} minutes",
                    count = minutes
                ))
                .size(10)
                .style(muted_style),
            ],
        ]
        .spacing(6),
    );
    let layer = pick_list(
        Some(state.layer.clone()),
        state.layers.clone(),
        |n: &String| n.clone(),
    )
    .on_select(|n| msg(PdfDialogMsg::SecLayer(n)))
    .text_size(12)
    .padding([4, 8])
    .width(Fill);
    let color = pick_list(
        Some(state.color),
        LineColor::ALL.to_vec(),
        |c: &LineColor| c.to_string(),
    )
    .on_select(|c| msg(PdfDialogMsg::SecColor(c)))
    .text_size(12)
    .padding([4, 8])
    .width(Fill);
    let labelled = |label: std::borrow::Cow<'static, str>,
                    control: Element<'a, Message>|
     -> Element<'a, Message> {
        row![
            text(label)
                .size(11)
                .style(muted_style)
                .width(Length::Fixed(70.0)),
            control
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    };
    let output = card(
        t!("Output geometry").into_owned(),
        column![
            labelled(t!("Layer"), layer.into()),
            labelled(t!("Color"), color.into()),
            segmented(
                vec![
                    (false, t!("Lines").into_owned()),
                    (true, t!("2D Polylines").into_owned())
                ],
                state.polylines,
                PdfDialogMsg::SecPolylines,
            ),
            field(
                t!("Polyline width").into_owned(),
                &state.width,
                state.polylines,
                150.0,
                |v| msg(PdfDialogMsg::SecWidth(v))
            ),
        ]
        .spacing(8),
    );
    let pick = |connect: bool| {
        button(text("⌖").size(12))
            .on_press(msg(PdfDialogMsg::SecPick(connect)))
            .style(button_style(false))
            .padding([4, 8])
    };
    let tolerances = card(
        t!("Extraction tolerances").into_owned(),
        column![
            row![
                field(
                    t!("Minimum line length").into_owned(),
                    &state.min_length,
                    true,
                    170.0,
                    |v| msg(PdfDialogMsg::SecMinLength(v))
                ),
                pick(false)
            ]
            .spacing(6)
            .align_y(iced::Center),
            row![
                field(
                    t!("Connect lines tolerance").into_owned(),
                    &state.connect,
                    true,
                    170.0,
                    |v| msg(PdfDialogMsg::SecConnect(v))
                ),
                pick(true)
            ]
            .spacing(6)
            .align_y(iced::Center),
            row![
                field(
                    t!("Collinear angle tolerance").into_owned(),
                    &state.angle,
                    true,
                    170.0,
                    |v| msg(PdfDialogMsg::SecAngle(v))
                ),
                text("0–10°").size(10).style(muted_style)
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    let body = column![
        row![extract, max_points].spacing(10),
        row![output, tolerances].spacing(10),
    ]
    .spacing(10);
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(PdfDialogMsg::Help("pcsection")))
            .style(button_style(false))
            .padding([5, 11]),
        chip(
            t!("Preview result").into_owned(),
            state.preview,
            msg(PdfDialogMsg::SecPreview(!state.preview))
        ),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button(t!("Create"), msg(PdfDialogMsg::SecCreate), true),
    ]
    .spacing(8)
    .align_y(iced::Center);
    column![body, footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

// ── View background (-VIEW sEttings Background) ─────────────────────────

/// What a named view shows behind the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgKind {
    None,
    Solid,
    Gradient,
    Image,
}

/// Where an image background sits: centred, stretched over the view, or
/// repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgPosition {
    Center,
    Stretch,
    Tile,
}

impl BgPosition {
    pub const ALL: [BgPosition; 3] = [BgPosition::Center, BgPosition::Stretch, BgPosition::Tile];
}

impl fmt::Display for BgPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&match self {
            BgPosition::Center => crate::tr!("view-background", "center"),
            BgPosition::Stretch => crate::tr!("view-background", "stretch"),
            BgPosition::Tile => crate::tr!("view-background", "tile"),
        })
    }
}

/// A background image as the preview draws it.
pub struct BgPreview {
    pub size: [u32; 2],
    /// Unmirrored, mirrored in x, in y, in both.
    pub handles: [image::Handle; 4],
}

/// Loads a background image for the preview, at most 512 pixels a side.
pub fn bg_preview(path: &str) -> Option<BgPreview> {
    let decoded = ::image::open(path.trim()).ok()?;
    let size = [decoded.width(), decoded.height()];
    let small = if size[0].max(size[1]) > 512 { decoded.thumbnail(512, 512) } else { decoded }.to_rgba8();
    let handle = |pixels: ::image::RgbaImage| image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw());
    let flip_x = ::image::imageops::flip_horizontal(&small);
    let flip_y = ::image::imageops::flip_vertical(&small);
    let flip_xy = ::image::imageops::flip_vertical(&flip_x);
    Some(BgPreview { size, handles: [handle(small), handle(flip_x), handle(flip_y), handle(flip_xy)] })
}

/// Image offsets the reference's Adjust Image sliders reach, in pixels.
pub const BG_OFFSET_LIMIT: f64 = 2000.0;
/// Image scales the reference's sliders reach (slider value / 10).
pub const BG_SCALE_LIMIT: f64 = 10.0;

/// One spinner step on an image background field: a pixel for an offset, a
/// tenth for a scale, which skips 0 (the reference ignores a zero scale).
pub fn bg_step(field: u8, value: f64, dir: i8) -> f64 {
    let dir = f64::from(dir.signum());
    if field < 2 {
        return (value.round() + dir).clamp(-BG_OFFSET_LIMIT, BG_OFFSET_LIMIT);
    }
    let mut next = ((value * 10.0).round() + dir) / 10.0;
    if next.abs() < 0.05 {
        next += dir / 10.0;
    }
    next.clamp(-BG_SCALE_LIMIT, BG_SCALE_LIMIT)
}

/// The Background dialog's fields for one named view.
pub struct ViewBackgroundState {
    pub view: String,
    pub kind: BgKind,
    pub solid: [u8; 3],
    pub top: [u8; 3],
    pub middle: [u8; 3],
    pub bottom: [u8; 3],
    pub three: bool,
    pub rotation: String,
    pub image: String,
    pub position: BgPosition,
    pub aspect: bool,
    /// The image's offset (pixels) and scale as typed, and the scale the
    /// view's image had (kept for a zero scale).
    pub offset: [String; 2],
    pub scale: [String; 2],
    pub scale_base: [f64; 2],
    /// The chosen image for the preview: its size in pixels and a small copy
    /// as it is and mirrored (x, y, both), for negative scales.
    pub preview: Option<BgPreview>,
    /// The drawing viewport's size, which the preview stands for.
    pub viewport: [f32; 2],
    /// The drawing area colour, shown for no background.
    pub canvas: [u8; 3],
}

pub fn view_background<'a>(
    state: &'a ViewBackgroundState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let kind = segmented(
        vec![
            (BgKind::None, crate::tr!("view-background", "none")),
            (BgKind::Solid, crate::tr!("view-background", "solid")),
            (BgKind::Gradient, crate::tr!("view-background", "gradient")),
            (BgKind::Image, crate::tr!("view-background", "image")),
        ],
        state.kind,
        PdfDialogMsg::BgKind,
    );
    let rgb = |c: [u8; 3]| iced::Color::from_rgb8(c[0], c[1], c[2]);
    let fill = |background: Background| {
        move |_: &Theme| container::Style {
            background: Some(background),
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: iced::Color::from_rgb8(0x4a, 0x4a, 0x4a),
            },
            ..Default::default()
        }
    };
    let note = |label: String| text(label).size(11).style(muted_style);
    let preview: Element<'a, Message> = match state.kind {
        BgKind::None => container(note(crate::tr!("view-background", "canvas")))
            .center(Fill)
            .style(fill(Background::Color(rgb(state.canvas))))
            .into(),
        BgKind::Solid => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(fill(Background::Color(rgb(state.solid))))
            .into(),
        BgKind::Gradient => {
            // Bottom colour at the start, top at the end, turned
            // counter-clockwise by the rotation.
            let angle = state.rotation.trim().parse::<f32>().unwrap_or(0.0);
            let mut linear = iced::gradient::Linear::new(iced::Radians(-angle.to_radians()))
                .add_stop(0.0, rgb(state.bottom));
            if state.three {
                linear = linear.add_stop(0.5, rgb(state.middle));
            }
            linear = linear.add_stop(1.0, rgb(state.top));
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(fill(Background::Gradient(iced::Gradient::Linear(linear))))
                .into()
        }
        BgKind::Image => {
            if std::path::Path::new(state.image.trim()).is_file() {
                let canvas = Background::Color(rgb(state.canvas));
                if state.position == BgPosition::Stretch {
                    // Filling the view; keeping the aspect ratio it covers
                    // the view and the overflow is cut off.
                    let fit = if state.aspect { iced::ContentFit::Cover } else { iced::ContentFit::Fill };
                    let handle = state.preview.as_ref().map_or_else(
                        || image::Handle::from_path(state.image.trim()),
                        |preview| preview.handles[0].clone(),
                    );
                    container(image(handle).content_fit(fit).width(Fill).height(Fill))
                        .center(Fill)
                        .clip(true)
                        .style(fill(canvas))
                        .into()
                } else {
                    // The image at its size times its scale, moved by its
                    // offset, in a view of the drawing viewport's shape.
                    let parse = |v: &String, d: f64| v.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(d);
                    let offset = [parse(&state.offset[0], 0.0), parse(&state.offset[1], 0.0)];
                    let scale = [0, 1].map(|k| {
                        let v = parse(&state.scale[k], state.scale_base[k]);
                        if v.abs() < 1e-9 { state.scale_base[k] } else { v }
                    });
                    let [iw, ih] = state.preview.as_ref().map_or([1, 1], |preview| preview.size);
                    // A negative scale mirrors the image.
                    let mirrored = usize::from(scale[0] < 0.0) | usize::from(scale[1] < 0.0) << 1;
                    let handle = state.preview.as_ref().map_or_else(
                        || image::Handle::from_path(state.image.trim()),
                        |preview| preview.handles[mirrored].clone(),
                    );
                    let viewport = state.viewport;
                    // The image's pixels and offsets are device pixels; the
                    // viewport is measured in logical ones.
                    let device = crate::scene::view::device_scale().max(0.1);
                    let tile = state.position == BgPosition::Tile;
                    container(iced::widget::responsive(move |size| {
                        let factor = size.height / viewport[1].max(1.0) / device;
                        let w = (iw as f32 * scale[0].abs() as f32 * factor).max(1.0);
                        let h = (ih as f32 * scale[1].abs() as f32 * factor).max(1.0);
                        let x = size.width / 2.0 + offset[0] as f32 * factor - w / 2.0;
                        let y = size.height / 2.0 + offset[1] as f32 * factor - h / 2.0;
                        let (cols, rows) = if tile {
                            ((size.width / w).ceil() as i32 + 1, (size.height / h).ceil() as i32 + 1)
                        } else {
                            (0, 0)
                        };
                        let mut layers = iced::widget::Stack::new()
                            .width(Fill)
                            .height(Fill)
                            .push(container(Space::new()).width(Fill).height(Fill));
                        let x0 = if tile { x - (x / w).ceil() * w } else { x };
                        let y0 = if tile { y - (y / h).ceil() * h } else { y };
                        for row_index in 0..=rows.min(60) {
                            for col_index in 0..=cols.min(60) {
                                let at = iced::Point::new(x0 + col_index as f32 * w, y0 + row_index as f32 * h);
                                layers = layers.push(
                                    iced::widget::pin(
                                        image(handle.clone())
                                            .content_fit(iced::ContentFit::Fill)
                                            .width(Length::Fixed(w))
                                            .height(Length::Fixed(h)),
                                    )
                                    .position(at),
                                );
                            }
                        }
                        Element::<Message>::from(layers)
                    }))
                    .width(Fill)
                    .height(Fill)
                    .clip(true)
                    .style(fill(canvas))
                    .into()
                }
            } else {
                container(note(crate::tr!("view-background", "no-image")))
                    .center(Fill)
                    .style(fill(Background::Color(iced::Color::from_rgb8(0x1a, 0x1a, 0x1a))))
                    .into()
            }
        }
    };
    let preview = card(
        crate::tr!("view-background", "preview"),
        container(preview).width(Fill).height(Length::Fixed(150.0)),
    );
    let label = |value: String, enabled: bool| {
        text(value)
            .size(11)
            .style(move |theme: &Theme| {
                if enabled {
                    muted_style(theme)
                } else {
                    text::Style {
                        color: Some(theme.palette().background.strong.color),
                    }
                }
            })
            .width(Length::Fixed(92.0))
    };
    let color_row = |name: String, slot: u8, color: [u8; 3], enabled: bool| -> Element<'a, Message> {
        let mut pick = button(text(crate::tr!("view-background", "select")).size(11))
            .style(button_style(false))
            .padding([4, 10]);
        if enabled {
            pick = pick.on_press(msg(PdfDialogMsg::BgPickColor(slot)));
        }
        row![
            label(name, enabled),
            swatch(color, 64.0, 22.0),
            text(format!("{},{},{}", color[0], color[1], color[2]))
                .size(11)
                .style(muted_style),
            Space::new().width(Fill),
            pick,
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };
    let options: Element<'a, Message> = match state.kind {
        BgKind::None => card(
            crate::tr!("view-background", "info"),
            note(crate::tr!("view-background", "none-info")),
        ),
        BgKind::Solid => card(
            crate::tr!("view-background", "solid-color"),
            color_row(crate::tr!("view-background", "color"), 0, state.solid, true),
        ),
        BgKind::Gradient => card(
            crate::tr!("view-background", "gradient"),
            column![
                row![
                    chip(
                        crate::tr!("view-background", "three-colors"),
                        state.three,
                        msg(PdfDialogMsg::BgThree(!state.three))
                    ),
                    Space::new().width(Fill),
                    text(crate::tr!("view-background", "rotation"))
                        .size(11)
                        .style(muted_style),
                    text_input("", &state.rotation)
                        .on_input(|v| msg(PdfDialogMsg::BgRotation(v)))
                        .size(11)
                        .padding([4, 8])
                        .width(Length::Fixed(64.0))
                        .style(field_style),
                ]
                .spacing(8)
                .align_y(iced::Center),
                color_row(crate::tr!("view-background", "top"), 1, state.top, true),
                color_row(crate::tr!("view-background", "middle"), 2, state.middle, state.three),
                color_row(crate::tr!("view-background", "bottom"), 3, state.bottom, true),
            ]
            .spacing(8),
        ),
        BgKind::Image => card(
            crate::tr!("view-background", "image"),
            column![
                row![
                    text_input("", &state.image)
                        .on_input(|v| msg(PdfDialogMsg::BgImage(v)))
                        .size(11)
                        .padding([4, 8])
                        .width(Fill)
                        .style(field_style),
                    button(text(crate::tr!("view-background", "browse")).size(11))
                        .on_press(msg(PdfDialogMsg::BgBrowse))
                        .style(button_style(false))
                        .padding([4, 10]),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    label(crate::tr!("view-background", "position"), true),
                    pick_list(Some(state.position), BgPosition::ALL.to_vec(), |p: &BgPosition| {
                        p.to_string()
                    })
                    .on_select(|p| msg(PdfDialogMsg::BgPosition(p)))
                    .text_size(12)
                    .padding([4, 8])
                    .width(Fill),
                ]
                .spacing(8)
                .align_y(iced::Center),
                chip(
                    crate::tr!("view-background", "aspect"),
                    state.aspect,
                    msg(PdfDialogMsg::BgAspect(!state.aspect))
                ),
            ]
            .spacing(8),
        ),
    };
    let adjust = (state.kind == BgKind::Image).then(|| adjust_card(state));
    let mut body = column![kind, preview, options].spacing(10);
    if let Some(adjust) = adjust {
        body = body.push(adjust);
    }
    column![
        body,
        footer(
            "background",
            None,
            crate::tr!("view-background", "apply").into(),
            PdfDialogMsg::BgApply
        ),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

/// The image's offset and scale (the reference's Adjust Image): pixel offsets
/// and scales with spinners and a reset; inactive while the image is
/// stretched over the view, where the reference ignores them.
fn adjust_card<'a>(state: &'a ViewBackgroundState) -> Element<'a, Message> {
    let enabled = state.position != BgPosition::Stretch;
    let dim = move |theme: &Theme| {
        if enabled {
            muted_style(theme)
        } else {
            text::Style { color: Some(theme.palette().background.strong.color) }
        }
    };
    let spin = |field: u8| -> Element<'a, Message> {
        let value = if field < 2 { &state.offset[field as usize] } else { &state.scale[field as usize - 2] };
        let mut input = text_input("", value)
            .size(11)
            .padding([4, 8])
            .width(Length::Fixed(64.0))
            .style(move |theme: &Theme, status| {
                let mut style = field_style(theme, status);
                if !enabled {
                    style.value = theme.palette().background.strong.color;
                }
                style
            });
        if enabled {
            input = input.on_input(move |v| {
                if field < 2 {
                    let mut pair = state.offset.clone();
                    pair[field as usize] = v;
                    msg(PdfDialogMsg::BgOffset(pair))
                } else {
                    let mut pair = state.scale.clone();
                    pair[field as usize - 2] = v;
                    msg(PdfDialogMsg::BgScale(pair))
                }
            });
        }
        let arrow = |up: bool| {
            let mut b = button(crate::ui::icons::themed_arrow_toggle(up, 7.0))
                .style(button_style(false))
                .padding([1, 4]);
            if enabled {
                b = b.on_press(msg(PdfDialogMsg::BgStep(field, if up { 1 } else { -1 })));
            }
            b
        };
        row![input, column![arrow(true), arrow(false)].spacing(1)].spacing(2).align_y(iced::Center).into()
    };
    let axis = |name: &'static str| text(name).size(11).style(dim);
    let line = |label: String, a: u8, b: u8, unit: bool| -> Element<'a, Message> {
        let mut r = row![
            text(label).size(11).style(dim).width(Length::Fixed(72.0)),
            axis("X"),
            spin(a),
            axis("Y"),
            spin(b),
        ]
        .spacing(6)
        .align_y(iced::Center);
        if unit {
            r = r.push(text(crate::tr!("view-background", "pixels")).size(11).style(dim));
        }
        r.into()
    };
    let mut reset = button(text(crate::tr!("view-background", "reset")).size(11))
        .style(button_style(false))
        .padding([3, 10]);
    if enabled {
        reset = reset.on_press(msg(PdfDialogMsg::BgReset));
    }
    container(
        column![
            row![
                text(crate::tr!("view-background", "adjust").to_uppercase()).size(10).style(move |theme: &Theme| {
                    if enabled { accent_text(theme) } else { dim(theme) }
                }),
                Space::new().width(Fill),
                reset,
            ]
            .align_y(iced::Center),
            line(crate::tr!("view-background", "offset"), 0, 1, true),
            line(crate::tr!("view-background", "scale"), 2, 3, false),
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style)
    .into()
}

// ── Image Adjust (IMAGEADJUST) ──────────────────────────────────────────

/// The Image Adjust dialog: brightness, contrast and fade (0–100) of the
/// chosen raster images, with a before / after preview of the first.
pub struct ImageAdjustState {
    pub handles: Vec<codec::Handle>,
    /// The first image's file name, shown over the preview.
    pub name: String,
    /// Brightness, contrast, fade.
    pub values: [u8; 3],
    /// The boxes as shown; a box may be empty while it is typed in.
    pub texts: [String; 3],
    /// The first image, at most 512 pixels a side, as RGBA.
    pub source: Option<(u32, u32, Vec<u8>)>,
    /// The drawing area colour the image fades into.
    pub canvas: [u8; 3],
    /// Left half as it is, right half adjusted, over the drawing area colour.
    pub preview: Option<image::Handle>,
}

/// Brightness 50, contrast 50, fade 0.
pub const IMAGE_ADJUST_DEFAULTS: [u8; 3] = [50, 50, 0];

impl ImageAdjustState {
    pub fn new(handles: Vec<codec::Handle>, path: &str, values: [u8; 3], canvas: [u8; 3]) -> Self {
        let name = std::path::Path::new(path.trim())
            .file_name()
            .map_or_else(|| path.trim().to_string(), |n| n.to_string_lossy().into_owned());
        let source = crate::scene::model::image_model::resolve_image(path).and_then(|decoded| {
            let full = ::image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.pixels.to_vec())?;
            let small = if full.width().max(full.height()) > 512 {
                ::image::imageops::thumbnail(&full, 512, 512)
            } else {
                full
            };
            Some((small.width(), small.height(), small.into_raw()))
        });
        let mut state = Self {
            handles,
            name,
            values: [0; 3],
            texts: Default::default(),
            source,
            canvas,
            preview: None,
        };
        state.set_all(values);
        state
    }

    pub fn set(&mut self, row: usize, value: u8) {
        let mut values = self.values;
        values[row] = value.min(100);
        self.set_all(values);
    }

    pub fn set_all(&mut self, values: [u8; 3]) {
        self.values = values.map(|v| v.min(100));
        self.texts = self.values.map(|v| v.to_string());
        self.preview = self.source.as_ref().map(|(w, h, pixels)| {
            // As the drawing shows it: brightness then contrast per channel,
            // fade as the image's opacity over the drawing area.
            let lut = crate::scene::model::image_model::brightness_contrast_lut(self.values[0], self.values[1]);
            let opacity = 1.0 - f32::from(self.values[2]) / 100.0;
            let mut out = Vec::with_capacity(pixels.len());
            for (k, px) in pixels.chunks_exact(4).enumerate() {
                let after = k as u32 % w >= w / 2;
                let alpha = f32::from(px[3]) / 255.0 * if after { opacity } else { 1.0 };
                for c in 0..3 {
                    let value = if after { lut[px[c] as usize] } else { px[c] };
                    let bg = f32::from(self.canvas[c]);
                    out.push((bg + (f32::from(value) - bg) * alpha).round() as u8);
                }
                out.push(255);
            }
            image::Handle::from_rgba(*w, *h, out)
        });
    }
}

pub fn image_adjust<'a>(
    state: &'a ImageAdjustState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let badge = |label: String, accent: bool| {
        container(text(label).size(10))
            .padding([2, 8])
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(if accent {
                    theme.palette().primary.base.color.scale_alpha(0.2)
                } else {
                    iced::Color::from_rgba8(0x1e, 0x1e, 0x1e, 0.85)
                })),
                border: Border {
                    width: 1.0,
                    radius: 10.0.into(),
                    color: if accent {
                        theme.palette().primary.base.color
                    } else {
                        iced::Color::from_rgb8(0x4a, 0x4a, 0x4a)
                    },
                },
                text_color: Some(iced::Color::WHITE),
                ..Default::default()
            })
    };
    let canvas = iced::Color::from_rgb8(state.canvas[0], state.canvas[1], state.canvas[2]);
    let picture: Element<'a, Message> = match &state.preview {
        Some(handle) => image(handle.clone())
            .content_fit(iced::ContentFit::Cover)
            .width(Fill)
            .height(Fill)
            .into(),
        None => container(text(crate::tr!("image-adjust", "no-preview")).size(11).style(muted_style))
            .center(Fill)
            .into(),
    };
    let divider = container(
        container(Space::new().width(2.0).height(Fill)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().primary.base.color)),
            ..Default::default()
        }),
    )
    .center_x(Fill)
    .height(Fill);
    let badges = row![
        badge(crate::tr!("image-adjust", "before"), false),
        Space::new().width(Fill),
        badge(crate::tr!("image-adjust", "after"), true),
    ]
    .padding(8);
    let mut layers = iced::widget::Stack::new().width(Fill).height(Fill).push(picture);
    if state.preview.is_some() {
        layers = layers.push(divider).push(badges);
    }
    let preview = card(
        format!("{} · {}", crate::tr!("image-adjust", "preview"), state.name),
        container(layers)
            .width(Fill)
            .height(Length::Fixed(150.0))
            .clip(true)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(canvas)),
                border: Border { radius: 5.0.into(), ..Default::default() },
                ..Default::default()
            }),
    );
    let ramp = |row: u8| -> Background {
        let (from, to) = match row {
            // Contrast: grey, the flat image its zero gives.
            1 => (iced::Color::from_rgb8(0x77, 0x77, 0x77), iced::Color::from_rgb8(0x77, 0x77, 0x77)),
            _ => (iced::Color::from_rgb8(0x22, 0x22, 0x22), iced::Color::from_rgb8(0xee, 0xee, 0xee)),
        };
        Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, from)
                .add_stop(1.0, to),
        ))
    };
    let line = |row: u8, label: String| -> Element<'a, Message> {
        let k = row as usize;
        let bar = container(container(Space::new()).width(Fill).height(8.0).style(move |_: &Theme| {
            container::Style {
                background: Some(ramp(row)),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }
        }))
        .center_y(16.0)
        .width(Fill);
        let slide = iced::widget::slider(0..=100u8, state.values[k], move |v| msg(PdfDialogMsg::IaValue(row, v)))
            .height(16.0)
            .style(|theme: &Theme, _| iced::widget::slider::Style {
                rail: iced::widget::slider::Rail {
                    backgrounds: (iced::Color::TRANSPARENT.into(), iced::Color::TRANSPARENT.into()),
                    width: 0.0,
                    border: Border::default(),
                },
                handle: iced::widget::slider::Handle {
                    shape: iced::widget::slider::HandleShape::Rectangle { width: 4, border_radius: 2.0.into() },
                    background: theme.palette().primary.base.color.into(),
                    border_width: 0.0,
                    border_color: iced::Color::TRANSPARENT,
                },
            });
        let step = |dir: i8| {
            button(text(if dir < 0 { "\u{2212}" } else { "+" }).size(12))
                .on_press(msg(PdfDialogMsg::IaStep(row, dir)))
                .style(button_style(false))
                .padding([2, 8])
        };
        let value = text_input("", &state.texts[k])
            .on_input(move |v| msg(PdfDialogMsg::IaText(row, v)))
            .size(11)
            .padding([3, 4])
            .width(Length::Fixed(40.0))
            .align_x(iced::alignment::Horizontal::Right)
            .style(field_style);
        row![
            text(label).size(11).width(Length::Fixed(72.0)),
            iced::widget::Stack::new().width(Fill).push(bar).push(slide),
            container(row![step(-1), value, step(1)].spacing(2).align_y(iced::Center))
                .padding(2)
                .style(well_style),
        ]
        .spacing(10)
        .align_y(iced::Center)
        .into()
    };
    let adjust = card(
        crate::tr!("image-adjust", "adjust"),
        column![
            line(0, crate::tr!("image-adjust", "brightness")),
            line(1, crate::tr!("image-adjust", "contrast")),
            line(2, crate::tr!("image-adjust", "fade")),
            text(crate::tr!("image-adjust", "hint")).size(10).style(muted_style),
        ]
        .spacing(10),
    );
    let reset = button(text(crate::tr!("image-adjust", "reset")).size(11).style(accent_text))
        .on_press(msg(PdfDialogMsg::IaReset))
        .style(button::text)
        .padding([5, 4]);
    column![
        preview,
        adjust,
        footer("imageadjust", Some(reset.into()), crate::tr!("image-adjust", "apply").into(), PdfDialogMsg::IaApply),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
