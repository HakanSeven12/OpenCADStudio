//! The docked Blocks palette: Current Drawing, Recent, Favorites and
//! Libraries tabs over one block list, a wildcard filter, six views, and the
//! insertion options. A click on a block starts placing it with the options;
//! a block dragged onto the drawing goes in where it is dropped.

use std::path::PathBuf;

use crate::app::settings::{BlockInsertOptions, PaletteBlockRef};
use crate::app::Message;
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use crate::ui::dock::PanelId;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card_style, well_style};
use iced::widget::canvas::{Frame, Path, Program, Stroke};
use iced::widget::{
    button, canvas, checkbox, column, container, image, mouse_area, pick_list, row, scrollable,
    text, text_input, Space,
};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

const TOOL_H: f32 = 22.0;
/// Fixed single-line height of a card's label so every cell in a row
/// shares the same height.
const LABEL_LINE_H: f32 = 16.0;

/// The palette's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Current,
    Recent,
    Favorites,
    Libraries,
}

impl Tab {
    fn label(self) -> String {
        match self {
            Tab::Current => crate::t!("Current Drawing"),
            Tab::Recent => crate::t!("Recent"),
            Tab::Favorites => crate::t!("Favorites"),
            Tab::Libraries => crate::t!("Libraries"),
        }
        .into_owned()
    }

    fn heading(self) -> String {
        match self {
            Tab::Current => crate::t!("Current Drawing Blocks"),
            Tab::Recent => crate::t!("Recent Blocks"),
            Tab::Favorites => crate::t!("Favorite Blocks"),
            Tab::Libraries => crate::t!("Library Blocks"),
        }
        .into_owned()
    }
}

/// How the list is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    ExtraLarge,
    #[default]
    Large,
    Medium,
    Small,
    Details,
    List,
}

impl ViewMode {
    pub const ALL: [ViewMode; 6] = [
        ViewMode::ExtraLarge,
        ViewMode::Large,
        ViewMode::Medium,
        ViewMode::Small,
        ViewMode::Details,
        ViewMode::List,
    ];

    pub fn from_u8(v: u8) -> Self {
        Self::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn label(self) -> String {
        match self {
            ViewMode::ExtraLarge => crate::t!("Extra Large Icons"),
            ViewMode::Large => crate::t!("Large Icons"),
            ViewMode::Medium => crate::t!("Medium Icons"),
            ViewMode::Small => crate::t!("Small Icons"),
            ViewMode::Details => crate::t!("Details"),
            ViewMode::List => crate::t!("List"),
        }
        .into_owned()
    }

    /// Cards per row and thumbnail height for the icon views.
    fn grid(self) -> Option<(usize, f32)> {
        match self {
            ViewMode::ExtraLarge => Some((2, 120.0)),
            ViewMode::Large => Some((3, 70.0)),
            ViewMode::Medium => Some((4, 46.0)),
            _ => None,
        }
    }
}

/// One thing the list shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A block of the current drawing.
    Current(String),
    /// A recent or favorite block, possibly from another drawing.
    Ref(PaletteBlockRef),
    /// A drawing in a library folder (inserted as a block).
    File(PathBuf),
    /// A sub-folder of a library folder.
    Folder(PathBuf),
}

impl Item {
    pub fn name(&self) -> String {
        match self {
            Item::Current(name) => name.clone(),
            Item::Ref(r) => r.name.clone(),
            Item::File(p) | Item::Folder(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// A switch or value of the Options card.
#[derive(Debug, Clone)]
pub enum OptionMsg {
    InsertionPoint(bool),
    Scale(bool),
    Uniform(bool),
    X(String),
    Y(String),
    Z(String),
    Rotation(bool),
    Angle(String),
    AutoPlacement(bool),
    Repeat(bool),
    Explode(bool),
}

/// The block context menu's commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuAction {
    Insert,
    Redefine,
    Favorite,
    Unfavorite,
    Edit,
    Remove,
}

/// Every message the block palette emits. Wrapped in `Message::BlockPalette`.
#[derive(Debug, Clone)]
pub enum BlockPaletteMsg {
    Search(String),
    Tab(Tab),
    View(ViewMode),
    PickFile,
    FilePicked(Result<PathBuf, String>),
    Refresh,
    Option(OptionMsg),
    ToggleOptions,
    LibraryBrowse,
    LibraryPicked(Result<PathBuf, String>),
    LibrarySelect(String),
    LibraryUp,
    /// Pressed on an item (a click when released on it, else a drag).
    Press(Item),
    Release(Item),
    Menu(Item, MenuAction),
}

/// One block of the current drawing with its cached preview.
pub struct BlockEntry {
    pub name: String,
    pub wires: Vec<WireModel>,
    pub annotative: bool,
    pub dynamic: bool,
}

/// A library folder entry.
pub struct LibraryEntry {
    pub path: PathBuf,
    pub folder: bool,
    pub thumb: Option<image::Handle>,
}

/// Panel state held on the app.
#[derive(Default)]
pub struct BlockPalette {
    pub tab: Tab,
    pub search: String,
    pub view: ViewMode,
    /// Current drawing blocks, rebuilt on refresh.
    pub blocks: Vec<BlockEntry>,
    /// Block name currently being placed, for the highlight.
    pub placing: Option<String>,
    pub cached_names: Vec<String>,
    pub source_tab_id: Option<u64>,
    pub source_block_epoch: u64,
    pub recent: Vec<PaletteBlockRef>,
    pub favorites: Vec<PaletteBlockRef>,
    /// Library roots, last used first.
    pub libraries: Vec<String>,
    /// The folder shown (a library root or one of its sub-folders).
    pub library_dir: Option<PathBuf>,
    pub library_entries: Vec<LibraryEntry>,
    /// Thumbnails of drawings recent / favorite entries point at.
    pub file_thumbs: std::collections::HashMap<String, Option<image::Handle>>,
    pub options: BlockInsertOptions,
    /// The typed X / Y / Z / angle fields.
    pub option_text: [String; 4],
    pub options_collapsed: bool,
    /// The item a press started on (a drag until released on it).
    pub pressed: Option<Item>,
    /// Blocks brought in from drawings this session (upper name → file),
    /// so the Recent list remembers the drawing.
    pub file_sources: std::collections::HashMap<String, String>,
    /// Previews of blocks seen this session, keyed `source|NAME`, for
    /// recent / favorite blocks of other drawings.
    pub ref_wires: std::collections::HashMap<String, Vec<WireModel>>,
}

impl BlockPalette {
    pub fn set_options(&mut self, options: BlockInsertOptions) {
        self.option_text = [
            short(options.x),
            short(options.y),
            short(options.z),
            short(options.angle),
        ];
        self.options = options;
    }

    /// The items of the current tab that pass the filter.
    pub fn items(&self) -> Vec<Item> {
        let pattern = self.search.trim();
        let pass = |name: &str| {
            pattern.is_empty() || crate::io::xref_model::wildcard_match(name, pattern)
        };
        match self.tab {
            Tab::Current => self
                .blocks
                .iter()
                .filter(|b| pass(&b.name))
                .map(|b| Item::Current(b.name.clone()))
                .collect(),
            Tab::Recent => self
                .recent
                .iter()
                .filter(|r| pass(&r.name))
                .map(|r| Item::Ref(r.clone()))
                .collect(),
            Tab::Favorites => self
                .favorites
                .iter()
                .filter(|r| pass(&r.name))
                .map(|r| Item::Ref(r.clone()))
                .collect(),
            Tab::Libraries => self
                .library_entries
                .iter()
                .filter(|e| e.folder || pass(&display_name(&e.path)))
                .map(|e| if e.folder { Item::Folder(e.path.clone()) } else { Item::File(e.path.clone()) })
                .collect(),
        }
    }

    fn current_entry(&self, name: &str) -> Option<&BlockEntry> {
        self.blocks.iter().find(|b| b.name.eq_ignore_ascii_case(name))
    }
}

pub fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Canvas program that draws a block's wires fit-to-box inside its bounds.
struct BlockPreviewCanvas<'a> {
    wires: &'a [WireModel],
}

impl<'a> Program<Message> for BlockPreviewCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let pad = 4.0;
        let inner_w = (bounds.width - 2.0 * pad).max(1.0);
        let inner_h = (bounds.height - 2.0 * pad).max(1.0);
        let (mut minx, mut miny, mut maxx, mut maxy) =
            (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut any = false;
        for w in self.wires {
            for p in &w.points {
                if p[0].is_finite() && p[1].is_finite() {
                    minx = minx.min(p[0]);
                    miny = miny.min(p[1]);
                    maxx = maxx.max(p[0]);
                    maxy = maxy.max(p[1]);
                    any = true;
                }
            }
        }
        if !any {
            let rect = Path::rectangle(iced::Point::new(pad, pad), iced::Size::new(inner_w, inner_h));
            frame.fill(&rect, Color { r: 0.10, g: 0.10, b: 0.10, a: 1.0 });
            return vec![frame.into_geometry()];
        }
        let span_x = (maxx - minx).max(1e-6);
        let span_y = (maxy - miny).max(1e-6);
        let scale = (inner_w / span_x).min(inner_h / span_y);
        let ox = (bounds.width - span_x * scale) * 0.5;
        let oy = (bounds.height - span_y * scale) * 0.5;
        let map = |x: f32, y: f32| iced::Point::new(ox + (x - minx) * scale, oy + (maxy - y) * scale);
        for w in self.wires {
            let col = Color { r: w.color[0], g: w.color[1], b: w.color[2], a: 0.55 };
            for tri in w.fill_tris.chunks(3) {
                if tri.len() == 3 {
                    let path = Path::new(|p| {
                        p.move_to(map(tri[0][0], tri[0][1]));
                        p.line_to(map(tri[1][0], tri[1][1]));
                        p.line_to(map(tri[2][0], tri[2][1]));
                        p.close();
                    });
                    frame.fill(&path, col);
                }
            }
        }
        for w in self.wires {
            let col = Color { r: w.color[0], g: w.color[1], b: w.color[2], a: 1.0 };
            let width = if w.line_weight_px > 1.5 { 2.0 } else { 1.0 };
            let mut run: Vec<iced::Point> = Vec::new();
            let flush = |frame: &mut Frame, run: &mut Vec<iced::Point>| {
                if run.len() >= 2 {
                    let path = Path::new(|p| {
                        p.move_to(run[0]);
                        for &pt in &run[1..] {
                            p.line_to(pt);
                        }
                    });
                    frame.stroke(&path, Stroke::default().with_color(col).with_width(width));
                }
                run.clear();
            };
            for p in &w.points {
                if p[0].is_finite() && p[1].is_finite() {
                    run.push(map(p[0], p[1]));
                } else {
                    flush(&mut frame, &mut run);
                }
            }
            flush(&mut frame, &mut run);
        }
        vec![frame.into_geometry()]
    }
}

/// A block preview (the palette's canvas) for other dialogs.
pub(crate) fn preview_canvas<'a>(wires: &'a [WireModel], height: f32) -> Element<'a, Message> {
    canvas(BlockPreviewCanvas { wires }).width(Fill).height(Length::Fixed(height)).into()
}

fn pm(m: BlockPaletteMsg) -> Message {
    Message::BlockPalette(m)
}

/// The thumbnail of an item, `size` pixels tall.
fn thumb<'a>(palette: &'a BlockPalette, item: &Item, size: f32) -> Element<'a, Message> {
    let blank = || -> Element<'a, Message> {
        container(Space::new())
            .width(Fill)
            .height(Length::Fixed(size))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.10))),
                ..Default::default()
            })
            .into()
    };
    let wires = |name: &str| palette.current_entry(name).map(|e| &e.wires[..]);
    let file_thumb = |path: &str| palette.file_thumbs.get(path).cloned().flatten();
    match item {
        Item::Current(name) => match wires(name) {
            Some(w) => canvas(BlockPreviewCanvas { wires: w }).width(Fill).height(Length::Fixed(size)).into(),
            None => blank(),
        },
        Item::Ref(r) => {
            if !r.drawing {
                let seen = palette.ref_wires.get(&format!("{}|{}", r.source, r.name.to_ascii_uppercase()));
                if let Some(w) = seen.map(|w| &w[..]).or_else(|| wires(&r.name)) {
                    return canvas(BlockPreviewCanvas { wires: w }).width(Fill).height(Length::Fixed(size)).into();
                }
            }
            match file_thumb(&r.source) {
                Some(h) if r.drawing => image(h).width(Fill).height(Length::Fixed(size)).into(),
                _ => blank(),
            }
        }
        Item::File(path) => {
            match palette.library_entries.iter().find(|e| &e.path == path).and_then(|e| e.thumb.clone()) {
                Some(h) => image(h).width(Fill).height(Length::Fixed(size)).into(),
                None => blank(),
            }
        }
        Item::Folder(_) => container(crate::ui::icons::themed_secondary(crate::ui::icons::FOLDER_OPEN, size * 0.6))
            .center_x(Fill)
            .center_y(Length::Fixed(size))
            .into(),
    }
}

/// The context menu for an item.
fn item_menu(tab: Tab, item: Item, in_drawing: bool, external: bool) -> Element<'static, Message> {
    let entry = |label: String, action: MenuAction, enabled: bool| -> Element<'static, Message> {
        let b = button(text(label).size(12))
            .width(Fill)
            .padding([5, 12])
            .style(|theme: &Theme, status| {
                let mut s = button::text(theme, status);
                if matches!(status, button::Status::Hovered) {
                    s.background = Some(Background::Color(theme.palette().primary.base.color));
                    s.text_color = theme.palette().primary.base.text;
                }
                s
            });
        if enabled {
            b.on_press(pm(BlockPaletteMsg::Menu(item.clone(), action))).into()
        } else {
            b.into()
        }
    };
    let favorite = if tab == Tab::Favorites {
        entry(crate::t!("Remove from Favorites").into_owned(), MenuAction::Unfavorite, true)
    } else {
        entry(crate::t!("Add to Favorites").into_owned(), MenuAction::Favorite, !matches!(item, Item::Folder(_)))
    };
    let remove_label = match tab {
        Tab::Recent | Tab::Favorites => crate::t!("Remove from List"),
        _ => crate::t!("Remove Block"),
    };
    let remove_enabled = match tab {
        Tab::Current => true,
        Tab::Recent | Tab::Favorites => true,
        Tab::Libraries => false,
    };
    container(
        column![
            entry(crate::t!("Insert").into_owned(), MenuAction::Insert, !matches!(item, Item::Folder(_))),
            entry(crate::t!("Redefine").into_owned(), MenuAction::Redefine, external),
            favorite,
            container(Space::new()).height(1).width(Fill).style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            }),
            entry(crate::t!("Edit Block").into_owned(), MenuAction::Edit, in_drawing),
            entry(remove_label.into_owned(), MenuAction::Remove, remove_enabled),
        ]
        .spacing(1),
    )
    .padding(4)
    .width(Length::Fixed(200.0))
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// One item as a card / row, with press, release and the context menu.
fn item_view<'a>(palette: &'a BlockPalette, item: Item) -> Element<'a, Message> {
    let name = item.name();
    let placing = matches!(&item, Item::Current(n) if palette.placing.as_deref() == Some(n.as_str()));
    let in_drawing = palette.current_entry(&name).is_some()
        && !matches!(&item, Item::Ref(r) if r.drawing)
        && !matches!(item, Item::File(_) | Item::Folder(_));
    let external = matches!(&item, Item::Ref(r) if !r.source.is_empty()) || matches!(item, Item::File(_));
    let body: Element<'a, Message> = match palette.view.grid() {
        Some((_, size)) => column![
            thumb(palette, &item, size),
            text(crate::ui::text_util::elide(&name, 14))
                .size(11)
                .width(Fill)
                .height(Length::Fixed(LABEL_LINE_H))
                .center(),
        ]
        .spacing(4)
        .into(),
        None => match palette.view {
            ViewMode::Small => row![
                container(thumb(palette, &item, 16.0)).width(Length::Fixed(20.0)),
                text(crate::ui::text_util::elide(&name, 16)).size(11),
            ]
            .spacing(4)
            .align_y(iced::Center)
            .into(),
            ViewMode::Details => {
                let (ann, dynamic) = palette
                    .current_entry(&name)
                    .map_or((false, false), |e| (e.annotative, e.dynamic));
                let yes_no = |v: bool| if v { crate::t!("Yes") } else { crate::t!("No") }.into_owned();
                row![
                    container(thumb(palette, &item, 16.0)).width(Length::Fixed(40.0)),
                    text(name.clone()).size(11).width(Fill),
                    text(format!("{} / {}", yes_no(ann), yes_no(dynamic)))
                        .size(11)
                        .width(Length::Fixed(140.0)),
                ]
                .spacing(4)
                .align_y(iced::Center)
                .into()
            }
            _ => text(name.clone()).size(11).width(Fill).into(),
        },
    };
    let card = container(body)
        .padding(if palette.view.grid().is_some() { 6 } else { 3 })
        .width(Fill)
        .style(move |theme: &Theme| {
            let (bg, fg) = block_card_colors(theme, placing, button::Status::Active);
            container::Style {
                background: Some(Background::Color(bg)),
                text_color: Some(fg),
                border: Border {
                    color: block_card_border(theme, placing),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });
    let area = mouse_area(card)
        .on_press(pm(BlockPaletteMsg::Press(item.clone())))
        .on_release(pm(BlockPaletteMsg::Release(item.clone())))
        .interaction(iced::mouse::Interaction::Pointer);
    let tab = palette.tab;
    iced_aw::ContextMenu::new(area, move || item_menu(tab, item.clone(), in_drawing, external)).into()
}

fn icon_button<'a>(icon: IconKind, tip: &str, msg: BlockPaletteMsg) -> Element<'a, Message> {
    let icon_el: Element<'_, Message> = match icon {
        IconKind::Glyph(s) => text(s).size(15).into(),
        IconKind::Svg(bytes) => crate::ui::icons::semantic(bytes, TOOL_H),
    };
    crate::ui::dock::tool_button(icon_el, crate::t!(tip).into_owned(), pm(msg))
}

fn options_card<'a>(palette: &'a BlockPalette) -> Element<'a, Message> {
    let o = &palette.options;
    let check = |label: String, on: bool, f: fn(bool) -> OptionMsg| -> Element<'a, Message> {
        checkbox(on)
            .label(label)
            .text_size(12)
            .size(14)
            .on_toggle(move |v| pm(BlockPaletteMsg::Option(f(v))))
            .into()
    };
    let num = |value: &'a str, f: fn(String) -> OptionMsg, enabled: bool| -> Element<'a, Message> {
        let mut input = text_input("", value).size(11).padding([3, 6]).width(Length::Fixed(48.0)).style(field_style);
        if enabled {
            input = input.on_input(move |v| pm(BlockPaletteMsg::Option(f(v))));
        }
        input.into()
    };
    let header = mouse_area(
        row![
            text(crate::t!("Options").to_uppercase()).size(10).style(accent_text),
            Space::new().width(Fill),
            if palette.options_collapsed {
                crate::ui::icons::themed_arrow_right(12.0)
            } else {
                crate::ui::icons::themed_arrow_down(12.0)
            },
        ]
        .align_y(iced::Center),
    )
    .on_press(pm(BlockPaletteMsg::ToggleOptions))
    .interaction(iced::mouse::Interaction::Pointer);
    let mut content = column![header].spacing(6);
    if !palette.options_collapsed {
        let scale_kinds = vec![crate::t!("Scale").into_owned(), crate::t!("Uniform Scale").into_owned()];
        let current_kind = scale_kinds[o.uniform as usize].clone();
        let uniform_label = scale_kinds[1].clone();
        let kind = pick_list(Some(current_kind), scale_kinds, |v: &String| v.clone())
            .on_select(move |v: String| pm(BlockPaletteMsg::Option(OptionMsg::Uniform(v == uniform_label))))
        .text_size(11)
        .padding([3, 6]);
        let fields_on = !o.scale;
        let mut scale_row = row![
            checkbox(o.scale).size(14).on_toggle(|v| pm(BlockPaletteMsg::Option(OptionMsg::Scale(v)))),
            kind,
        ]
        .spacing(6)
        .align_y(iced::Center);
        // The scale values sit on their own line under the switch.
        let mut xyz_row = row![Space::new().width(Length::Fixed(20.0))].spacing(6).align_y(iced::Center);
        if o.scale {
            scale_row = scale_row.push(text(crate::t!("specified on screen")).size(11).style(muted_style));
        } else if o.uniform {
            xyz_row = xyz_row.push(num(&palette.option_text[0], OptionMsg::X, fields_on));
        } else {
            xyz_row = xyz_row
                .push(text("X").size(11))
                .push(num(&palette.option_text[0], OptionMsg::X, fields_on))
                .push(text("Y").size(11))
                .push(num(&palette.option_text[1], OptionMsg::Y, fields_on))
                .push(text("Z").size(11))
                .push(num(&palette.option_text[2], OptionMsg::Z, fields_on));
        }
        let mut rot_row = row![check(crate::t!("Rotation").into_owned(), o.rotation, OptionMsg::Rotation), Space::new().width(Fill)]
            .spacing(6)
            .align_y(iced::Center);
        if o.rotation {
            rot_row = rot_row.push(text(crate::t!("specified on screen")).size(11).style(muted_style));
        } else {
            rot_row = rot_row
                .push(num(&palette.option_text[3], OptionMsg::Angle, true))
                .push(text(crate::t!("Angle")).size(11));
        }
        content = content
            .push(check(crate::t!("Insertion Point").into_owned(), o.insertion_point, OptionMsg::InsertionPoint))
            .push(scale_row);
        if !o.scale {
            content = content.push(xyz_row);
        }
        content = content
            .push(rot_row)
            .push(check(crate::t!("Auto-Placement").into_owned(), o.auto_placement, OptionMsg::AutoPlacement))
            .push(check(crate::t!("Repeat Placement").into_owned(), o.repeat, OptionMsg::Repeat))
            .push(check(crate::t!("Explode").into_owned(), o.explode, OptionMsg::Explode));
    }
    container(content).padding([8, 10]).width(Fill).style(card_style).into()
}

/// Build the docked panel element from the palette state.
pub fn view(palette: &BlockPalette, width: f32, auto_collapse: bool) -> Element<'_, Message> {
    let title_bar = crate::ui::dock::title_bar(PanelId::BlockPalette, crate::t!("Blocks").into_owned(), auto_collapse);

    let tabs = {
        let buttons = [Tab::Current, Tab::Recent, Tab::Favorites, Tab::Libraries].map(|t| {
            button(text(t.label()).size(10).width(Fill).align_x(iced::Center))
                .on_press(pm(BlockPaletteMsg::Tab(t)))
                .style(button_style(t == palette.tab))
                .padding([5, 1])
                .width(Fill)
                .into()
        });
        container(iced::widget::Row::with_children(buttons).spacing(2))
            .padding(2)
            .width(Fill)
            .style(well_style)
    };

    let search_input = text_input(&crate::t!("Filter... (* ? wildcards)"), &palette.search)
        .on_input(|v| pm(BlockPaletteMsg::Search(v)))
        .padding([4, 8])
        .size(12)
        .style(field_style);
    let view_pick = pick_list(Some(palette.view.label()), ViewMode::ALL.map(|v| v.label()).to_vec(), |v: &String| v.clone())
        .on_select(|label: String| {
            pm(BlockPaletteMsg::View(
                ViewMode::ALL.into_iter().find(|v| v.label() == label).unwrap_or_default(),
            ))
        })
    .text_size(11)
    .width(Length::Fixed(118.0))
    .padding([3, 6]);
    let header = row![
        search_input.width(Fill),
        icon_button(
            IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
            "Insert a drawing as a block",
            BlockPaletteMsg::PickFile,
        ),
        view_pick,
    ]
    .spacing(4)
    .align_y(iced::Center);

    let mut top = column![tabs, header].spacing(6);
    if palette.tab == Tab::Libraries {
        let names: Vec<String> = palette.libraries.clone();
        let selected = palette.libraries.first().cloned();
        let mut lib_row = row![
            pick_list(selected, names, |v: &String| v.clone())
                .on_select(|v: String| pm(BlockPaletteMsg::LibrarySelect(v)))
                .placeholder(crate::t!("No library").into_owned())
                .text_size(11)
                .padding([3, 6])
                .width(Fill),
            button(crate::ui::icons::themed(crate::ui::icons::FOLDER_OPEN, 14.0))
                .on_press(pm(BlockPaletteMsg::LibraryBrowse))
                .style(button_style(false))
                .padding([3, 8]),
        ]
        .spacing(4)
        .align_y(iced::Center);
        let in_sub = palette.library_dir.as_ref().is_some_and(|d| {
            palette.libraries.first().is_some_and(|root| d != &PathBuf::from(root))
        });
        if in_sub {
            lib_row = lib_row.push(
                button(crate::ui::icons::themed_arrow_up(12.0))
                    .on_press(pm(BlockPaletteMsg::LibraryUp))
                    .style(button_style(false))
                    .padding([3, 8]),
            );
        }
        top = top.push(lib_row);
        if let (Some(root), Some(dir)) = (palette.libraries.first(), palette.library_dir.as_ref()) {
            let root = PathBuf::from(root);
            let shown = match dir.strip_prefix(root.parent().unwrap_or(&root)) {
                Ok(rel) => rel.display().to_string(),
                Err(_) => dir.display().to_string(),
            };
            top = top.push(text(format!("{} {shown}", crate::t!("Path:"))).size(10).style(accent_text));
        }
    }
    top = top.push(text(palette.tab.heading().to_uppercase()).size(10).style(accent_text));

    let items = palette.items();
    let body: Element<'_, Message> = if items.is_empty() {
        let msg = match palette.tab {
            Tab::Current if palette.cached_names.is_empty() => crate::t!("No blocks in this drawing"),
            Tab::Libraries if palette.libraries.is_empty() => crate::t!("Choose a library folder with …"),
            _ => crate::t!("No matches"),
        };
        container(text(msg).size(12).style(muted_style)).center_x(Fill).center_y(Fill).width(Fill).height(Fill).into()
    } else {
        let mut col = column![].spacing(4);
        match palette.view.grid() {
            Some((per_row, _)) => {
                for chunk in items.chunks(per_row) {
                    let mut r = row![].spacing(6).width(Fill);
                    for item in chunk {
                        r = r.push(item_view(palette, item.clone()));
                    }
                    for _ in chunk.len()..per_row {
                        r = r.push(Space::new().width(Fill));
                    }
                    col = col.push(r);
                }
            }
            None if palette.view == ViewMode::Small => {
                for chunk in items.chunks(2) {
                    let mut r = row![].spacing(6).width(Fill);
                    for item in chunk {
                        r = r.push(item_view(palette, item.clone()));
                    }
                    if chunk.len() < 2 {
                        r = r.push(Space::new().width(Fill));
                    }
                    col = col.push(r);
                }
            }
            None => {
                if palette.view == ViewMode::Details {
                    col = col.push(
                        row![
                            text(crate::t!("Icon")).size(11).style(muted_style).width(Length::Fixed(40.0)),
                            text(crate::t!("Name")).size(11).style(muted_style).width(Fill),
                            text(crate::t!("Annotative / Dynamic")).size(11).style(muted_style).width(Length::Fixed(140.0)),
                        ]
                        .spacing(4),
                    );
                }
                for item in items {
                    col = col.push(item_view(palette, item));
                }
            }
        }
        scrollable(container(col).padding(iced::Padding { top: 0.0, right: 8.0, bottom: 6.0, left: 0.0 }))
            .width(Fill)
            .height(Fill)
            .into()
    };

    crate::ui::dock::frame(column![title_bar, top, body, options_card(palette)].spacing(6), width)
}

/// Width of the ribbon's block gallery.
pub const GALLERY_W: f32 = 300.0;

/// The ribbon Insert gallery: the drawing's blocks (a click inserts one),
/// then the ways into the palette's other tabs.
pub fn gallery(palette: &BlockPalette) -> Element<'_, Message> {
    let ribbon_command = |cmd: String| Message::RibbonToolClick {
        tool_id: "INSERT".to_string(),
        event: crate::modules::ModuleEvent::Command(cmd),
    };
    let mut grid = column![].spacing(4);
    for chunk in palette.blocks.chunks(4) {
        let mut r = row![].spacing(4).width(Fill);
        for block in chunk {
            let card = column![
                canvas(BlockPreviewCanvas { wires: &block.wires }).width(Fill).height(Length::Fixed(48.0)),
                text(crate::ui::text_util::elide(&block.name, 10))
                    .size(10)
                    .width(Fill)
                    .height(Length::Fixed(LABEL_LINE_H))
                    .center(),
            ]
            .spacing(2);
            r = r.push(
                button(card)
                    .on_press(ribbon_command(format!("_BLOCKINSERT {}", block.name)))
                    .style(crate::ui::ribbon::widgets::popup_row_style)
                    .padding(3)
                    .width(Fill),
            );
        }
        for _ in chunk.len()..4 {
            r = r.push(Space::new().width(Fill));
        }
        grid = grid.push(r);
    }
    let blocks: Element<'_, Message> = if palette.blocks.is_empty() {
        container(text(crate::t!("No blocks in this drawing")).size(11).style(muted_style))
            .padding(10)
            .center_x(Fill)
            .into()
    } else {
        scrollable(container(grid).padding(iced::Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }))
            .height(Length::Shrink)
            .into()
    };
    let link = |label: std::borrow::Cow<'static, str>, tab: &str| -> Element<'_, Message> {
        button(text(label.into_owned()).size(11))
            .on_press(ribbon_command(format!("_BLOCKSPALETTE {tab}")))
            .style(crate::ui::ribbon::widgets::popup_row_style)
            .width(Fill)
            .padding([5, 10])
            .into()
    };
    container(
        column![
            text(crate::t!("Current Drawing Blocks").to_uppercase()).size(10).style(accent_text),
            container(blocks).height(Length::Fixed(((palette.blocks.len().div_ceil(4)) as f32 * 79.0).clamp(36.0, 260.0))),
            container(Space::new()).height(1).width(Fill).style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            }),
            link(crate::t!("Recent Blocks..."), "RECENT"),
            link(crate::t!("Favorite Blocks..."), "FAVORITES"),
            link(crate::t!("Blocks from Libraries..."), "LIBRARIES"),
        ]
        .spacing(6),
    )
    .padding(8)
    .width(Length::Fixed(GALLERY_W))
    .style(crate::ui::ribbon::widgets::popup_panel_style)
    .into()
}

/// Theme-aware foreground/background pair for a block card (normal,
/// hovered/pressed, or the block being placed).
pub(crate) fn block_card_colors(theme: &Theme, is_placing: bool, status: button::Status) -> (Color, Color) {
    let palette = theme.palette();
    if is_placing {
        (palette.primary.base.color, palette.primary.base.text)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => {
                (palette.background.strong.color, palette.background.strong.text)
            }
            _ => (palette.background.weak.color, palette.background.weak.text),
        }
    }
}

/// Border color for a block card in a given state.
pub(crate) fn block_card_border(theme: &Theme, is_placing: bool) -> Color {
    let palette = theme.palette();
    if is_placing {
        palette.primary.base.color
    } else {
        palette.background.neutral.color
    }
}

/// Theme-aware foreground for the palette's header icon buttons (shared with
/// the dock's tool buttons).
pub(crate) fn block_icon_button_text_color(theme: &Theme, status: button::Status) -> Color {
    let palette = theme.palette();
    match status {
        button::Status::Hovered | button::Status::Pressed => palette.background.strong.text,
        _ => palette.background.base.text,
    }
}
