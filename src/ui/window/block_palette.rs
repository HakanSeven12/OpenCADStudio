//! The docked Blocks palette: Current Drawing, Recent, Favorites and
//! Libraries tabs over one block list, a wildcard filter, six views, and the
//! insertion options. A click on a block starts placing it with the options;
//! a block dragged onto the drawing goes in where it is dropped.

use std::path::PathBuf;
use std::rc::Rc;

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
/// Padding inside a card of the icon views.
const GRID_CARD_PAD: f32 = 6.0;
/// Space between a card's thumbnail and its label.
const GRID_LABEL_GAP: f32 = 4.0;
/// Space between the list's rows.
const LIST_SPACING: f32 = 4.0;
/// Height of a row's content in Small Icons, Details and List (and of the
/// Details header): the 16 px thumbnail, with the one-line name centred.
const LIST_ROW_H: f32 = 16.0;
/// Padding inside a card of Small Icons, Details and List.
const LIST_CARD_PAD: f32 = 3.0;
/// Rows built beyond each end of the visible ones, so a fast scroll doesn't
/// show an empty row before the list catches up.
const SPARE_ROWS: usize = 2;
/// How tall the list is taken to be for its previews before it reports its
/// viewport (it does on the first frame it is drawn).
const FIRST_SCREEN_H: f32 = 1200.0;
/// Cards in a row of the ribbon's block gallery.
const GALLERY_PER_ROW: usize = 4;
/// Height of a thumbnail in the ribbon's block gallery.
const GALLERY_THUMB_H: f32 = 48.0;
/// Padding inside a gallery card.
const GALLERY_CARD_PAD: f32 = 3.0;
/// Space between a gallery card's thumbnail and its label.
const GALLERY_LABEL_GAP: f32 = 2.0;
/// Space between the gallery's rows and cards.
const GALLERY_SPACING: f32 = 4.0;
/// The gallery's block list, so its scroll position can be reset.
const GALLERY_SCROLL_ID: &str = "block_gallery_scroll";

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

    /// How the list is laid out: cards per row, the height of a row, and
    /// the height above the first row (the Details header and its gap).
    /// Every row is exactly this tall, so only the rows in view are built.
    fn rows(self) -> (usize, f32, f32) {
        let list_row_h = LIST_ROW_H + 2.0 * LIST_CARD_PAD;
        match (self.grid(), self) {
            (Some((per_row, size)), _) => (per_row, grid_row_h(size), 0.0),
            (None, ViewMode::Small) => (2, list_row_h, 0.0),
            (None, ViewMode::Details) => (1, list_row_h, LIST_ROW_H + LIST_SPACING),
            (None, _) => (1, list_row_h, 0.0),
        }
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
    /// Web: the picked drawing arrived already parsed from the picker's
    /// bytes — a name-only path has nothing behind it to read later.
    #[cfg(target_arch = "wasm32")]
    FilePickedDoc(Result<(String, codec::Document), String>),
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
    /// The list scrolled to `offset` (from its top) in a viewport `height`
    /// tall.
    Scrolled {
        offset: f32,
        height: f32,
    },
    /// The ribbon gallery's list scrolled to this offset from its top.
    GalleryScrolled(f32),
    /// Thumbnails read in the background, by drawing path (`None` when the
    /// drawing has none).
    Thumbnails(Vec<(PathBuf, Option<image::Handle>)>),
}

/// What [`BlockPalette::shown_previews_changed`] compares.
struct PreviewsShownFor {
    scroll: (f32, f32),
    view: ViewMode,
    tab: Tab,
    search: String,
    recent: Vec<PaletteBlockRef>,
    favorites: Vec<PaletteBlockRef>,
}

/// Which block definitions a change touched, as far as it is known.
#[derive(Debug, Default, Clone, PartialEq)]
pub enum ChangeHint {
    /// Nothing is known: a refresh rebuilds every preview.
    #[default]
    Unknown,
    /// Only these blocks (upper-cased) of the drawing with this tab id
    /// changed, and the palette was up to date before.
    Blocks(u64, std::collections::HashSet<String>),
    /// A change was made while the palette was already out of date, so
    /// nothing else is known after all.
    Spoiled,
}

/// One block of the current drawing with its cached preview.
pub struct BlockEntry {
    pub name: String,
    pub preview: Rc<BlockPreview>,
    pub annotative: bool,
    pub dynamic: bool,
}

/// A block's preview wires, with its thumbnail's tessellated geometry kept
/// across frames: it is only drawn again when the thumbnail's size changes,
/// and a refresh of the palette makes new previews. Shared between the
/// Current Drawing list, the previews kept for Recent / Favorites and the
/// ribbon gallery.
///
/// The gallery draws its smaller thumbnail through a cache of its own: a
/// cache holds one size, so sharing it would draw both thumbnails again on
/// every frame, and both would show whichever was uploaded last.
///
/// The wires are worked out only once the palette or the ribbon gallery is
/// about to show the block ([`BlockPalette::shown_preview_names`],
/// [`BlockPalette::gallery_shown`]); until then it draws as an empty
/// thumbnail.
pub struct BlockPreview {
    wires: std::cell::OnceCell<Vec<WireModel>>,
    cache: canvas::Cache,
    gallery_cache: canvas::Cache,
}

impl BlockPreview {
    /// A preview whose wires are not worked out yet.
    pub fn pending() -> Self {
        Self {
            wires: std::cell::OnceCell::new(),
            cache: canvas::Cache::new(),
            gallery_cache: canvas::Cache::new(),
        }
    }

    /// The wires, once worked out.
    pub fn wires(&self) -> Option<&[WireModel]> {
        self.wires.get().map(Vec::as_slice)
    }

    /// Set the wires of a pending preview (a built one keeps its own).
    pub fn fill(&self, wires: Vec<WireModel>) {
        if self.wires.set(wires).is_ok() {
            // Whatever was drawn while pending is not the block.
            self.cache.clear();
            self.gallery_cache.clear();
        }
    }

    /// Set the wires of a pending preview, working them out only then.
    pub fn fill_with(&self, wires: impl FnOnce() -> Vec<WireModel>) {
        if self.wires.get().is_none() {
            self.fill(wires());
        }
    }
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
    /// Current drawing blocks, rebuilt on refresh (set with
    /// [`BlockPalette::set_blocks`], which keeps `block_index` in step).
    pub blocks: Vec<BlockEntry>,
    /// Position in `blocks` of each upper-cased block name (the first, when
    /// two differ only in case), so a lookup doesn't scan the list.
    block_index: std::collections::HashMap<String, usize>,
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
    /// Where the list is scrolled to and how tall its viewport is (0 until
    /// the list first reports it), so the icon views only build the rows
    /// that can be seen.
    pub scroll: (f32, f32),
    /// What the message being handled is known to have changed, so the
    /// refresh after it can keep the previews of the other blocks. Cleared
    /// once the message is handled.
    pub change_hint: ChangeHint,
    /// Drawings whose thumbnails are wanted but not asked for yet; they are
    /// read in the background once the message is handled.
    pub thumb_requests: Vec<PathBuf>,
    /// What the shown previews were last worked out for; `None` once the
    /// blocks are replaced.
    previews_shown_for: Option<PreviewsShownFor>,
    /// How far the ribbon gallery's list is scrolled (0 when it opens).
    pub gallery_offset: f32,
    /// The gallery offset its previews were last worked out for; `None`
    /// once the blocks are replaced or the gallery opens again.
    gallery_previews_for: Option<f32>,
    /// Blocks brought in from drawings this session (upper name → file),
    /// so the Recent list remembers the drawing.
    pub file_sources: std::collections::HashMap<String, String>,
    /// Previews of blocks seen this session, keyed `source|NAME`, for
    /// recent / favorite blocks of other drawings.
    pub ref_wires: std::collections::HashMap<String, Rc<BlockPreview>>,
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
        let pass =
            |name: &str| pattern.is_empty() || crate::io::xref_model::wildcard_match(name, pattern);
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
                .map(|e| {
                    if e.folder {
                        Item::Folder(e.path.clone())
                    } else {
                        Item::File(e.path.clone())
                    }
                })
                .collect(),
        }
    }

    /// The blocks whose previews the list shows with the current scroll
    /// position: the rows in view (or the first screenful, before the list
    /// reported its viewport), none in List, which has no thumbnails.
    /// Blocks of the drawing and recent / favorite blocks that may be in it.
    pub fn shown_preview_names(&self) -> Vec<String> {
        if self.view == ViewMode::List {
            return Vec::new();
        }
        let items = self.items();
        let (per_row, row_h, lead) = self.view.rows();
        let (offset, height) = self.scroll;
        let height = if height > 0.0 { height } else { FIRST_SCREEN_H };
        let rows = items.len().div_ceil(per_row);
        let range = visible_rows(rows, row_h + LIST_SPACING, offset - lead, height);
        let end = (range.end * per_row).min(items.len());
        let shown = &items[(range.start * per_row).min(end)..end];
        shown
            .iter()
            .filter_map(|item| match item {
                Item::Current(name) => Some(name.clone()),
                Item::Ref(r) if !r.drawing => Some(r.name.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether what decides the shown previews (scroll position, view, tab,
    /// filter, Recent / Favorites, the blocks themselves) changed since the
    /// last call, which remembers it. Checked after every message, so it
    /// copies nothing while nothing changed.
    pub fn shown_previews_changed(&mut self) -> bool {
        let same = self.previews_shown_for.as_ref().is_some_and(|last| {
            last.scroll == self.scroll
                && last.view == self.view
                && last.tab == self.tab
                && last.search == self.search
                && last.recent == self.recent
                && last.favorites == self.favorites
        });
        if !same {
            self.previews_shown_for = Some(PreviewsShownFor {
                scroll: self.scroll,
                view: self.view,
                tab: self.tab,
                search: self.search.clone(),
                recent: self.recent.clone(),
                favorites: self.favorites.clone(),
            });
        }
        !same
    }

    /// Replace the Current Drawing blocks.
    pub fn set_blocks(&mut self, blocks: Vec<BlockEntry>) {
        let mut index = std::collections::HashMap::with_capacity(blocks.len());
        for (i, b) in blocks.iter().enumerate() {
            index.entry(b.name.to_ascii_uppercase()).or_insert(i);
        }
        self.blocks = blocks;
        self.block_index = index;
        // New previews, all pending.
        self.previews_shown_for = None;
        self.gallery_previews_for = None;
    }

    /// The ribbon gallery opens: at the top of its list, with its previews
    /// still to be worked out.
    pub fn open_gallery(&mut self) -> iced::Task<Message> {
        self.gallery_offset = 0.0;
        self.gallery_previews_for = None;
        iced::widget::operation::snap_to(
            iced::widget::Id::new(GALLERY_SCROLL_ID),
            scrollable::RelativeOffset::START,
        )
    }

    /// The rows of the ribbon gallery in view, plus spare ones, and their
    /// blocks: what it builds and whose previews it shows. Its height is set
    /// by the number of blocks, so it is known before it is drawn.
    pub fn gallery_shown(&self) -> (std::ops::Range<usize>, &[BlockEntry]) {
        let n = self.blocks.len();
        let rows = visible_rows(
            n.div_ceil(GALLERY_PER_ROW),
            gallery_pitch(),
            self.gallery_offset,
            gallery_list_h(n),
        );
        let end = (rows.end * GALLERY_PER_ROW).min(n);
        let start = (rows.start * GALLERY_PER_ROW).min(end);
        (rows, &self.blocks[start..end])
    }

    /// Whether the gallery's scroll position or blocks changed since the
    /// last call, which remembers it.
    pub fn gallery_previews_changed(&mut self) -> bool {
        let changed = self.gallery_previews_for != Some(self.gallery_offset);
        self.gallery_previews_for = Some(self.gallery_offset);
        changed
    }

    /// The Current Drawing block named `name`, ignoring ASCII case.
    pub(crate) fn current_entry(&self, name: &str) -> Option<&BlockEntry> {
        self.block_index
            .get(&name.to_ascii_uppercase())
            .and_then(|&i| self.blocks.get(i))
            .filter(|b| b.name.eq_ignore_ascii_case(name))
    }
}

pub fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Canvas program that draws a block's wires fit-to-box inside its bounds,
/// through `cache` when the preview has one.
struct BlockPreviewCanvas<'a> {
    wires: &'a [WireModel],
    cache: Option<&'a canvas::Cache>,
}

impl<'a> BlockPreviewCanvas<'a> {
    /// The canvas of a palette preview, drawn through its cache (empty
    /// while the preview is pending).
    fn cached(preview: &'a BlockPreview) -> Self {
        Self {
            wires: preview.wires().unwrap_or_default(),
            cache: Some(&preview.cache),
        }
    }

    /// The canvas of a ribbon gallery preview, drawn through the gallery's
    /// own cache.
    fn gallery(preview: &'a BlockPreview) -> Self {
        Self {
            wires: preview.wires().unwrap_or_default(),
            cache: Some(&preview.gallery_cache),
        }
    }
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
        let size = bounds.size();
        match self.cache {
            Some(cache) => vec![cache.draw(renderer, size, |frame| {
                draw_preview(frame, self.wires, size)
            })],
            None => {
                let mut frame = Frame::new(renderer, size);
                draw_preview(&mut frame, self.wires, size);
                vec![frame.into_geometry()]
            }
        }
    }
}

/// Draw `wires` fit-to-box into `frame`, `bounds` in size.
fn draw_preview(frame: &mut Frame, wires: &[WireModel], bounds: iced::Size) {
    let pad = 4.0;
    let inner_w = (bounds.width - 2.0 * pad).max(1.0);
    let inner_h = (bounds.height - 2.0 * pad).max(1.0);
    let (mut minx, mut miny, mut maxx, mut maxy) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    let mut any = false;
    for w in wires {
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
        let rect = Path::rectangle(
            iced::Point::new(pad, pad),
            iced::Size::new(inner_w, inner_h),
        );
        frame.fill(
            &rect,
            Color {
                r: 0.10,
                g: 0.10,
                b: 0.10,
                a: 1.0,
            },
        );
        return;
    }
    let span_x = (maxx - minx).max(1e-6);
    let span_y = (maxy - miny).max(1e-6);
    let scale = (inner_w / span_x).min(inner_h / span_y);
    let ox = (bounds.width - span_x * scale) * 0.5;
    let oy = (bounds.height - span_y * scale) * 0.5;
    let map = |x: f32, y: f32| iced::Point::new(ox + (x - minx) * scale, oy + (maxy - y) * scale);
    for w in wires {
        let col = Color {
            r: w.color[0],
            g: w.color[1],
            b: w.color[2],
            a: 0.55,
        };
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
    for w in wires {
        let col = Color {
            r: w.color[0],
            g: w.color[1],
            b: w.color[2],
            a: 1.0,
        };
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
                flush(frame, &mut run);
            }
        }
        flush(frame, &mut run);
    }
}

/// A block preview (the palette's canvas) for other dialogs.
pub(crate) fn preview_canvas<'a>(wires: &'a [WireModel], height: f32) -> Element<'a, Message> {
    canvas(BlockPreviewCanvas { wires, cache: None })
        .width(Fill)
        .height(Length::Fixed(height))
        .into()
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
    let preview = |name: &str| palette.current_entry(name).map(|e| &*e.preview);
    let file_thumb = |path: &str| palette.file_thumbs.get(path).cloned().flatten();
    match item {
        Item::Current(name) => match preview(name) {
            Some(p) => canvas(BlockPreviewCanvas::cached(p))
                .width(Fill)
                .height(Length::Fixed(size))
                .into(),
            None => blank(),
        },
        Item::Ref(r) => {
            if !r.drawing {
                let seen =
                    palette
                        .ref_wires
                        .get(&format!("{}|{}", r.source, r.name.to_ascii_uppercase()));
                // A kept preview not built yet falls back to this drawing's.
                if let Some(p) = seen
                    .map(|p| &**p)
                    .filter(|p| p.wires().is_some())
                    .or_else(|| preview(&r.name))
                {
                    return canvas(BlockPreviewCanvas::cached(p))
                        .width(Fill)
                        .height(Length::Fixed(size))
                        .into();
                }
            }
            match file_thumb(&r.source) {
                Some(h) if r.drawing => image(h).width(Fill).height(Length::Fixed(size)).into(),
                _ => blank(),
            }
        }
        Item::File(path) => {
            match palette
                .library_entries
                .iter()
                .find(|e| &e.path == path)
                .and_then(|e| e.thumb.clone())
            {
                Some(h) => image(h).width(Fill).height(Length::Fixed(size)).into(),
                None => blank(),
            }
        }
        Item::Folder(_) => container(crate::ui::icons::themed_secondary(
            crate::ui::icons::FOLDER_OPEN,
            size * 0.6,
        ))
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
            b.on_press(pm(BlockPaletteMsg::Menu(item.clone(), action)))
                .into()
        } else {
            b.into()
        }
    };
    let favorite = if tab == Tab::Favorites {
        entry(
            crate::t!("Remove from Favorites").into_owned(),
            MenuAction::Unfavorite,
            true,
        )
    } else {
        entry(
            crate::t!("Add to Favorites").into_owned(),
            MenuAction::Favorite,
            !matches!(item, Item::Folder(_)),
        )
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
            entry(
                crate::t!("Insert").into_owned(),
                MenuAction::Insert,
                !matches!(item, Item::Folder(_))
            ),
            entry(
                crate::t!("Redefine").into_owned(),
                MenuAction::Redefine,
                external
            ),
            favorite,
            container(Space::new())
                .height(1)
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.neutral.color)),
                    ..Default::default()
                }),
            entry(
                crate::t!("Edit Block").into_owned(),
                MenuAction::Edit,
                in_drawing
            ),
            entry(
                remove_label.into_owned(),
                MenuAction::Remove,
                remove_enabled
            ),
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
    let placing =
        matches!(&item, Item::Current(n) if palette.placing.as_deref() == Some(n.as_str()));
    let in_drawing = palette.current_entry(&name).is_some()
        && !matches!(&item, Item::Ref(r) if r.drawing)
        && !matches!(item, Item::File(_) | Item::Folder(_));
    let external =
        matches!(&item, Item::Ref(r) if !r.source.is_empty()) || matches!(item, Item::File(_));
    let body: Element<'a, Message> = match palette.view.grid() {
        Some((_, size)) => column![
            thumb(palette, &item, size),
            text(crate::ui::text_util::elide(&name, 14))
                .size(11)
                .width(Fill)
                .height(Length::Fixed(LABEL_LINE_H))
                .center(),
        ]
        .spacing(GRID_LABEL_GAP)
        .into(),
        // One line per name, cut with "…" where it runs out of room, so every
        // row is exactly `LIST_ROW_H` tall.
        None => match palette.view {
            ViewMode::Small => row![
                container(thumb(palette, &item, 16.0)).width(Length::Fixed(20.0)),
                one_line(text(crate::ui::text_util::elide(&name, 16)).size(11)),
            ]
            .spacing(4)
            .height(Length::Fixed(LIST_ROW_H))
            .align_y(iced::Center)
            .into(),
            ViewMode::Details => {
                let (ann, dynamic) = palette
                    .current_entry(&name)
                    .map_or((false, false), |e| (e.annotative, e.dynamic));
                let yes_no =
                    |v: bool| if v { crate::t!("Yes") } else { crate::t!("No") }.into_owned();
                row![
                    container(thumb(palette, &item, 16.0)).width(Length::Fixed(40.0)),
                    one_line(text(name.clone()).size(11)),
                    text(format!("{} / {}", yes_no(ann), yes_no(dynamic)))
                        .size(11)
                        .width(Length::Fixed(140.0)),
                ]
                .spacing(4)
                .height(Length::Fixed(LIST_ROW_H))
                .align_y(iced::Center)
                .into()
            }
            _ => one_line(text(name.clone()).size(11))
                .height(Length::Fixed(LIST_ROW_H))
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        },
    };
    let card = container(body)
        .padding(if palette.view.grid().is_some() {
            GRID_CARD_PAD
        } else {
            LIST_CARD_PAD
        })
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
    iced_aw::ContextMenu::new(area, move || {
        item_menu(tab, item.clone(), in_drawing, external)
    })
    .into()
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
        let mut input = text_input("", value)
            .size(11)
            .padding([3, 6])
            .width(Length::Fixed(48.0))
            .style(field_style);
        if enabled {
            input = input.on_input(move |v| pm(BlockPaletteMsg::Option(f(v))));
        }
        input.into()
    };
    let header = mouse_area(
        row![
            text(crate::t!("Options").to_uppercase())
                .size(10)
                .style(accent_text),
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
        let scale_kinds = vec![
            crate::t!("Scale").into_owned(),
            crate::t!("Uniform Scale").into_owned(),
        ];
        let current_kind = scale_kinds[o.uniform as usize].clone();
        let uniform_label = scale_kinds[1].clone();
        let kind = pick_list(Some(current_kind), scale_kinds, |v: &String| v.clone())
            .on_select(move |v: String| {
                pm(BlockPaletteMsg::Option(OptionMsg::Uniform(
                    v == uniform_label,
                )))
            })
            .text_size(11)
            .padding([3, 6]);
        let fields_on = !o.scale;
        let mut scale_row = row![
            checkbox(o.scale)
                .size(14)
                .on_toggle(|v| pm(BlockPaletteMsg::Option(OptionMsg::Scale(v)))),
            kind,
        ]
        .spacing(6)
        .align_y(iced::Center);
        // The scale values sit on their own line under the switch.
        let mut xyz_row = row![Space::new().width(Length::Fixed(20.0))]
            .spacing(6)
            .align_y(iced::Center);
        if o.scale {
            scale_row = scale_row.push(
                text(crate::t!("specified on screen"))
                    .size(11)
                    .style(muted_style),
            );
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
        let mut rot_row = row![
            check(
                crate::t!("Rotation").into_owned(),
                o.rotation,
                OptionMsg::Rotation
            ),
            Space::new().width(Fill)
        ]
        .spacing(6)
        .align_y(iced::Center);
        if o.rotation {
            rot_row = rot_row.push(
                text(crate::t!("specified on screen"))
                    .size(11)
                    .style(muted_style),
            );
        } else {
            rot_row = rot_row
                .push(num(&palette.option_text[3], OptionMsg::Angle, true))
                .push(text(crate::t!("Angle")).size(11));
        }
        content = content
            .push(check(
                crate::t!("Insertion Point").into_owned(),
                o.insertion_point,
                OptionMsg::InsertionPoint,
            ))
            .push(scale_row);
        if !o.scale {
            content = content.push(xyz_row);
        }
        content = content
            .push(rot_row)
            .push(check(
                crate::t!("Auto-Placement").into_owned(),
                o.auto_placement,
                OptionMsg::AutoPlacement,
            ))
            .push(check(
                crate::t!("Repeat Placement").into_owned(),
                o.repeat,
                OptionMsg::Repeat,
            ))
            .push(check(
                crate::t!("Explode").into_owned(),
                o.explode,
                OptionMsg::Explode,
            ));
    }
    container(content)
        .padding([8, 10])
        .width(Fill)
        .style(card_style)
        .into()
}

/// Build the docked panel element from the palette state.
pub fn view(
    palette: &BlockPalette,
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'_, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::BlockPalette,
        crate::t!("Blocks").into_owned(),
        chrome,
    );

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
    let view_pick = pick_list(
        Some(palette.view.label()),
        ViewMode::ALL.map(|v| v.label()).to_vec(),
        |v: &String| v.clone(),
    )
    .on_select(|label: String| {
        pm(BlockPaletteMsg::View(
            ViewMode::ALL
                .into_iter()
                .find(|v| v.label() == label)
                .unwrap_or_default(),
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
            button(crate::ui::icons::themed(
                crate::ui::icons::FOLDER_OPEN,
                14.0
            ))
            .on_press(pm(BlockPaletteMsg::LibraryBrowse))
            .style(button_style(false))
            .padding([3, 8]),
        ]
        .spacing(4)
        .align_y(iced::Center);
        let in_sub = palette.library_dir.as_ref().is_some_and(|d| {
            palette
                .libraries
                .first()
                .is_some_and(|root| d != &PathBuf::from(root))
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
            top = top.push(
                text(format!("{} {shown}", crate::t!("Path:")))
                    .size(10)
                    .style(accent_text),
            );
        }
    }
    top = top.push(
        text(palette.tab.heading().to_uppercase())
            .size(10)
            .style(accent_text),
    );

    let items = palette.items();
    let body: Element<'_, Message> = if items.is_empty() {
        let msg = match palette.tab {
            Tab::Current if palette.cached_names.is_empty() => {
                crate::t!("No blocks in this drawing")
            }
            Tab::Libraries if palette.libraries.is_empty() => {
                crate::t!("Choose a library folder with …")
            }
            _ => crate::t!("No matches"),
        };
        container(text(msg).size(12).style(muted_style))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        let mut col = column![].spacing(LIST_SPACING);
        let (per_row, row_h, lead) = palette.view.rows();
        if palette.view == ViewMode::Details {
            col = col.push(
                row![
                    text(crate::t!("Icon"))
                        .size(11)
                        .style(muted_style)
                        .width(Length::Fixed(40.0)),
                    text(crate::t!("Name"))
                        .size(11)
                        .style(muted_style)
                        .width(Fill),
                    text(crate::t!("Annotative / Dynamic"))
                        .size(11)
                        .style(muted_style)
                        .width(Length::Fixed(140.0)),
                ]
                .spacing(4)
                .height(Length::Fixed(LIST_ROW_H)),
            );
        }
        // Every row is equally tall, so only the rows in view (and a few
        // spare) are built; spaces stand in for the others, keeping the
        // list's height and scroll position.
        let rows: Vec<&[Item]> = items.chunks(per_row).collect();
        let pitch = row_h + LIST_SPACING;
        let (offset, height) = palette.scroll;
        let shown = visible_rows(rows.len(), pitch, offset - lead, height);
        if shown.start > 0 {
            col = col.push(Space::new().height(shown.start as f32 * pitch - LIST_SPACING));
        }
        for chunk in &rows[shown.clone()] {
            let mut r = row![].spacing(6).width(Fill);
            for item in chunk.iter() {
                r = r.push(item_view(palette, item.clone()));
            }
            for _ in chunk.len()..per_row {
                r = r.push(Space::new().width(Fill));
            }
            col = col.push(r);
        }
        let after = rows.len() - shown.end;
        if after > 0 {
            col = col.push(Space::new().height(after as f32 * pitch - LIST_SPACING));
        }
        scrollable(container(col).padding(iced::Padding {
            top: 0.0,
            right: 8.0,
            bottom: 6.0,
            left: 0.0,
        }))
        .on_scroll(|viewport| {
            pm(BlockPaletteMsg::Scrolled {
                offset: viewport.absolute_offset().y,
                height: viewport.bounds().height,
            })
        })
        .width(Fill)
        .height(Fill)
        .into()
    };

    crate::ui::dock::frame(
        title_bar,
        column![top, body, options_card(palette)].spacing(6),
        width,
    )
}

/// `name` on one line the width of what is left, cut with "…" when it
/// does not fit.
fn one_line(name: iced::widget::Text<'_>) -> iced::widget::Text<'_> {
    name.width(Fill)
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::advanced::text::Ellipsis::End)
}

/// Height of a row of cards in an icon view with `size` tall thumbnails.
fn grid_row_h(size: f32) -> f32 {
    2.0 * GRID_CARD_PAD + size + GRID_LABEL_GAP + LABEL_LINE_H
}

/// The rows of a `rows` long list, `pitch` apart, to build for a viewport
/// `height` tall scrolled `offset` down from the first row (negative while
/// something above the rows is in view): those in view plus [`SPARE_ROWS`]
/// either side. All of them while the viewport's height is not known yet.
fn visible_rows(rows: usize, pitch: f32, offset: f32, height: f32) -> std::ops::Range<usize> {
    if height <= 0.0 || pitch <= 0.0 {
        return 0..rows;
    }
    // The list scrolls no further than its end, whatever was reported for
    // a longer list before.
    let offset = offset.clamp(0.0, (rows as f32 * pitch - height).max(0.0));
    let first = ((offset / pitch).floor() as usize).saturating_sub(SPARE_ROWS);
    let end = ((offset + height) / pitch).ceil() as usize + SPARE_ROWS;
    first.min(rows)..end.min(rows)
}

/// Width of the ribbon's block gallery.
pub const GALLERY_W: f32 = 300.0;

/// Distance between the tops of two rows of the ribbon gallery.
fn gallery_pitch() -> f32 {
    2.0 * GALLERY_CARD_PAD + GALLERY_THUMB_H + GALLERY_LABEL_GAP + LABEL_LINE_H + GALLERY_SPACING
}

/// Height of the ribbon gallery's list of `blocks` blocks.
fn gallery_list_h(blocks: usize) -> f32 {
    (blocks.div_ceil(GALLERY_PER_ROW) as f32 * 79.0).clamp(36.0, 260.0)
}

/// The ribbon Insert gallery: the drawing's blocks (a click inserts one),
/// then the ways into the palette's other tabs.
pub fn gallery(palette: &BlockPalette) -> Element<'_, Message> {
    let ribbon_command = |cmd: String| Message::RibbonToolClick {
        tool_id: "INSERT".to_string(),
        event: crate::modules::ModuleEvent::Command(cmd),
    };
    // Like the palette's lists, only the rows in view (and a few spare) are
    // built; spaces stand in for the others.
    let mut grid = column![].spacing(GALLERY_SPACING);
    let (shown, shown_blocks) = palette.gallery_shown();
    let pitch = gallery_pitch();
    if shown.start > 0 {
        grid = grid.push(Space::new().height(shown.start as f32 * pitch - GALLERY_SPACING));
    }
    for chunk in shown_blocks.chunks(GALLERY_PER_ROW) {
        let mut r = row![].spacing(GALLERY_SPACING).width(Fill);
        for block in chunk.iter() {
            let card = column![
                canvas(BlockPreviewCanvas::gallery(&block.preview))
                    .width(Fill)
                    .height(Length::Fixed(GALLERY_THUMB_H)),
                text(crate::ui::text_util::elide(&block.name, 10))
                    .size(10)
                    .width(Fill)
                    .height(Length::Fixed(LABEL_LINE_H))
                    .center(),
            ]
            .spacing(GALLERY_LABEL_GAP);
            r = r.push(
                button(card)
                    .on_press(ribbon_command(format!("_BLOCKINSERT {}", block.name)))
                    .style(crate::ui::ribbon::widgets::popup_row_style)
                    .padding(GALLERY_CARD_PAD)
                    .width(Fill),
            );
        }
        for _ in chunk.len()..GALLERY_PER_ROW {
            r = r.push(Space::new().width(Fill));
        }
        grid = grid.push(r);
    }
    let after = palette.blocks.len().div_ceil(GALLERY_PER_ROW) - shown.end;
    if after > 0 {
        grid = grid.push(Space::new().height(after as f32 * pitch - GALLERY_SPACING));
    }
    let blocks: Element<'_, Message> = if palette.blocks.is_empty() {
        container(
            text(crate::t!("No blocks in this drawing"))
                .size(11)
                .style(muted_style),
        )
        .padding(10)
        .center_x(Fill)
        .into()
    } else {
        scrollable(container(grid).padding(iced::Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 0.0,
        }))
        .id(iced::widget::Id::new(GALLERY_SCROLL_ID))
        .on_scroll(|viewport| {
            pm(BlockPaletteMsg::GalleryScrolled(
                viewport.absolute_offset().y,
            ))
        })
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
            text(crate::t!("Current Drawing Blocks").to_uppercase())
                .size(10)
                .style(accent_text),
            container(blocks).height(Length::Fixed(gallery_list_h(palette.blocks.len()))),
            container(Space::new())
                .height(1)
                .width(Fill)
                .style(|theme: &Theme| container::Style {
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
pub(crate) fn block_card_colors(
    theme: &Theme,
    is_placing: bool,
    status: button::Status,
) -> (Color, Color) {
    let palette = theme.palette();
    if is_placing {
        (palette.primary.base.color, palette.primary.base.text)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => (
                palette.background.strong.color,
                palette.background.strong.text,
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> BlockEntry {
        BlockEntry {
            name: name.to_string(),
            preview: Rc::new(BlockPreview::pending()),
            annotative: false,
            dynamic: false,
        }
    }

    #[test]
    fn current_entry_ignores_case_and_takes_the_first_match() {
        let mut palette = BlockPalette::default();
        palette.set_blocks(vec![entry("Door"), entry("DOOR"), entry("Window")]);
        let door = palette.current_entry("door").unwrap();
        assert!(std::ptr::eq(door, &palette.blocks[0]));
        assert_eq!(palette.current_entry("WINDOW").unwrap().name, "Window");
        assert!(palette.current_entry("Wall").is_none());
        // A new list replaces the index with it.
        palette.set_blocks(vec![entry("Wall")]);
        assert!(palette.current_entry("door").is_none());
        assert_eq!(palette.current_entry("wall").unwrap().name, "Wall");
    }

    #[test]
    fn visible_rows_covers_the_viewport_with_spare_rows() {
        // 100 rows, 50 px apart, a 200 px viewport.
        assert_eq!(visible_rows(100, 50.0, 0.0, 200.0), 0..6);
        assert_eq!(visible_rows(100, 50.0, 1000.0, 200.0), 18..26);
        // Scrolled past the end (a longer list before): clamped to the end.
        assert_eq!(visible_rows(100, 50.0, 1e6, 200.0), 94..100);
        assert_eq!(visible_rows(3, 50.0, 1e6, 200.0), 0..3);
        // The viewport is not known yet: everything.
        assert_eq!(visible_rows(100, 50.0, 0.0, 0.0), 0..100);
        assert_eq!(visible_rows(0, 50.0, 0.0, 200.0), 0..0);
    }

    /// Centre of text `label` in `palette`'s view laid out very tall, so
    /// the whole list fits on screen.
    fn label_at(palette: &BlockPalette, label: &str) -> Option<iced::Point> {
        locate(view(palette, 300.0, Default::default()), label, true)
    }

    /// Centre of text `label` in `element` laid out very tall: where it is
    /// in view (`visible`), or wherever it is laid out.
    fn locate(element: Element<'_, Message>, label: &str, visible: bool) -> Option<iced::Point> {
        use iced_test::core::widget;
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        use iced_test::Selector;
        let _one = crate::headless_ui_lock();
        let mut renderer = iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                None,
            ),
        )
        .expect("headless renderer");
        let size = iced::Size::new(300.0, 100_000.0);
        let mut ui = UserInterface::build(
            element,
            size,
            Cache::default(),
            &mut renderer,
        );
        let mut find = Selector::find(label);
        ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
        match widget::Operation::finish(&find) {
            widget::operation::Outcome::Some(Some(target)) if visible => {
                target.visible_bounds().map(|b| b.center())
            }
            widget::operation::Outcome::Some(Some(target)) => Some(target.bounds().center()),
            _ => None,
        }
    }

    #[test]
    fn every_view_only_builds_the_rows_in_view_at_their_real_place() {
        for mode in ViewMode::ALL {
            let (per_row, row_h, lead) = mode.rows();
            let pitch = row_h + LIST_SPACING;
            // Row 1 starts with a name far too long for the list: it stays
            // on one line, so its row is as tall as any other.
            let long = "L".repeat(200);
            let mut palette = BlockPalette {
                view: mode,
                ..Default::default()
            };
            palette.set_blocks(
                (0..300)
                    .map(|i| {
                        if i == per_row {
                            entry(&long)
                        } else {
                            entry(&format!("B{i:03}"))
                        }
                    })
                    .collect(),
            );
            let label = |row: usize| format!("B{:03}", row * per_row);
            // Everything built: the rows really are `pitch` apart.
            palette.scroll = (0.0, 0.0);
            let first = label_at(&palette, &label(0)).unwrap();
            let third = label_at(&palette, &label(2)).unwrap();
            assert!((third.y - first.y - 2.0 * pitch).abs() < 0.01, "{mode:?}");
            let far = label_at(&palette, &label(30)).unwrap();
            // Scrolled to row 30 of a 3-row viewport: rows far above are
            // not built, and the ones in view sit where they always did.
            palette.scroll = (lead + 30.0 * pitch, 3.0 * pitch);
            assert!(label_at(&palette, &label(0)).is_none(), "{mode:?}");
            assert!(label_at(&palette, &label(40)).is_none(), "{mode:?}");
            assert_eq!(label_at(&palette, &label(30)), Some(far), "{mode:?}");
            // Only the previews in view are worked out (List has none).
            let shown = palette.shown_preview_names();
            assert_eq!(
                shown.contains(&label(30)),
                mode != ViewMode::List,
                "{mode:?}"
            );
            assert!(!shown.contains(&label(0)), "{mode:?}");
        }
    }

    #[test]
    fn gallery_only_builds_the_rows_in_view_at_their_real_place() {
        let mut palette = BlockPalette::default();
        palette.set_blocks((0..300).map(|i| entry(&format!("B{i:03}"))).collect());
        let pitch = gallery_pitch();
        let label = |row: usize| format!("B{:03}", row * GALLERY_PER_ROW);
        let at = |palette: &BlockPalette, row: usize| locate(gallery(palette), &label(row), false);
        // At the top: the first rows, `pitch` apart, and none far below.
        let first = at(&palette, 0).unwrap();
        let third = at(&palette, 2).unwrap();
        assert!((third.y - first.y - 2.0 * pitch).abs() < 0.01);
        assert!(at(&palette, 30).is_none());
        // Scrolled to row 30: rows far above are not built, and row 30 sits
        // where it would with every row built.
        palette.gallery_offset = 30.0 * pitch;
        assert!(at(&palette, 0).is_none());
        assert!(at(&palette, 60).is_none());
        let row30 = at(&palette, 30).unwrap();
        assert!((row30.y - first.y - 30.0 * pitch).abs() < 0.01);
        // Only the previews of those rows are worked out.
        let (_, blocks) = palette.gallery_shown();
        let shown: Vec<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
        assert!(shown.contains(&label(30).as_str()));
        assert!(!shown.contains(&label(0).as_str()));
    }

    #[test]
    fn gallery_previews_are_worked_out_again_when_needed() {
        let mut palette = BlockPalette::default();
        palette.set_blocks(vec![entry("A")]);
        assert!(palette.gallery_previews_changed());
        assert!(!palette.gallery_previews_changed());
        palette.gallery_offset = 10.0;
        assert!(palette.gallery_previews_changed());
        // New blocks have new, pending previews.
        palette.set_blocks(vec![entry("A")]);
        assert!(palette.gallery_previews_changed());
        // Opening the gallery starts at the top, its previews to work out.
        let _ = palette.open_gallery();
        assert_eq!(palette.gallery_offset, 0.0);
        assert!(palette.gallery_previews_changed());
    }

    #[test]
    fn gallery_and_palette_thumbnails_keep_their_own_drawing() {
        let _one = crate::headless_ui_lock();
        let renderer = iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                None,
            ),
        )
        .expect("headless renderer");
        let preview = BlockPreview::pending();
        preview.fill(Vec::new());
        let draw = |canvas: BlockPreviewCanvas<'_>, w: f32, h: f32| {
            let _ = canvas.draw(
                &(),
                &renderer,
                &Theme::Dark,
                iced::Rectangle::with_size(iced::Size::new(w, h)),
                iced::mouse::Cursor::Unavailable,
            );
        };
        // Both sizes drawn in one frame: each cache keeps its own size, so
        // neither is drawn again (or shows the other's thumbnail) next frame.
        draw(BlockPreviewCanvas::cached(&preview), 101.0, 87.0);
        draw(BlockPreviewCanvas::gallery(&preview), 63.0, GALLERY_THUMB_H);
        let palette = format!("{:?}", preview.cache);
        let gallery = format!("{:?}", preview.gallery_cache);
        assert!(palette.contains("width: 101.0"), "{palette}");
        assert!(!palette.contains("width: 63.0"), "{palette}");
        assert!(gallery.contains("width: 63.0"), "{gallery}");
    }
}
