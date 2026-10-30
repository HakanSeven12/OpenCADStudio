//! The Select Color panel: state, update logic and view.
//!
//! The panel edits a pending colour and only reports it back on OK, so every
//! caller keeps applying colours through the same `ColorWindowPick` path.
//! Opened for an interface colour (any colour in Options) it shows only the
//! colour editor: no tabs, no index colours.

use super::book::ColorBook;
use super::document::{self, DocColor, DocSort, DocView};
use super::editor::{rgb_color, ColorEditor, EditorMsg};
use super::eyedropper::{Eyedropper, EyedropperMsg};
use super::math;
use crate::app::{ArrowKey, ColorPickTarget, Message};
use crate::t;
use crate::ui::properties::acad_color_display;
use codec::types::Color as AcadColor;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, svg, text, text_input, Space,
};
use iced::{Background, Border, Color, Element, Length, Theme};
use std::sync::Arc;

const PANEL_WIDTH: f32 = 400.0;
/// Scope for Tab / Shift+Tab, so focus cycles through the panel's own
/// fields instead of the dialogs and command line underneath.
const PANEL_ID: &str = "select-color-panel";
const BODY_HEIGHT: f32 = 368.0;
/// Swatches per row in the Document tab.
const DOC_COLUMNS: usize = 8;
/// Search matches shown at once in the Color books tab.
const PAGE_BARS: usize = 7;
/// Height of the search boxes, dropdowns and icon buttons.
const CONTROL_HEIGHT: f32 = 28.0;
/// Width of the page strip, the RGB values and the gap between them.
const SIDE_WIDTH: f32 = 26.0 + 10.0 + 104.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorPickerTab {
    #[default]
    Document,
    Index,
    TrueColor,
    Books,
}

#[derive(Debug, Clone)]
pub enum ColorPickerMsg {
    Tab(ColorPickerTab),
    /// A swatch was clicked.
    Pick(AcadColor),
    /// Set the new colour back to the current one.
    Revert,
    Editor(EditorMsg),
    /// Replace the true colour with its nearest index colour.
    UseNearest,
    Arrow(ArrowKey),
    Eyedropper(EyedropperMsg),
    /// A colour of the Document tab, by its place in the list.
    PickDocument(usize),
    DocSort(DocSort),
    DocView(DocView),
    /// Filter the Document tab by name.
    DocQuery(String),
    /// Show another loaded colour book.
    Book(usize),
    BookPage(usize),
    /// Scrolling over the page: one page per notch.
    BookScroll(f32),
    BookQuery(String),
    /// A colour of the shown book, by its index in the book.
    PickBook(usize),
    LoadBook,
    Ok,
    Cancel,
}

/// What the caller has to do after an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The colour, and its `BOOK$NAME` when it comes from a colour book.
    Commit(AcadColor, Option<String>),
    Cancel,
    /// Capture the window for the eyedropper.
    Screenshot,
    /// Ask for an `.acb` file.
    LoadBook,
}

pub struct ColorPicker {
    pub target: ColorPickTarget,
    /// The colour shown as "Current". For a mixed selection this is the colour
    /// of the most recently drawn object.
    current: AcadColor,
    varies: bool,
    pending: AcadColor,
    /// `BOOK$NAME` of the current and the new colour, for colour-book colours.
    current_name: Option<String>,
    pending_name: Option<String>,
    tab: ColorPickerTab,
    editor: ColorEditor,
    eyedropper: Option<Eyedropper>,
    doc: Vec<DocColor>,
    doc_sort: DocSort,
    doc_view: DocView,
    doc_query: String,
    /// This drawing's colours picked this session, newest first.
    recent: Vec<AcadColor>,
    books: Arc<Vec<ColorBook>>,
    book: usize,
    book_page: usize,
    book_query: String,
    /// Why the last colour book did not load.
    status: Option<String>,
    /// What ByLayer looks like here: the layer's colour.
    by_layer: Option<AcadColor>,
}

impl ColorPicker {
    pub fn new(target: ColorPickTarget, current: AcadColor, varies: bool) -> Self {
        Self {
            target,
            current,
            varies,
            pending: current,
            current_name: None,
            pending_name: None,
            tab: ColorPickerTab::Document,
            editor: ColorEditor::new(math::color_rgb(current).unwrap_or((255, 255, 255))),
            eyedropper: None,
            doc: Vec::new(),
            doc_sort: DocSort::default(),
            doc_view: DocView::default(),
            doc_query: String::new(),
            recent: Vec::new(),
            books: Arc::default(),
            book: 0,
            book_page: 0,
            book_query: String::new(),
            status: None,
            by_layer: None,
        }
    }

    /// Fill the Document and Color books tabs. `current_name` is the
    /// `BOOK$NAME` of the current colour; the Color books tab shows its page.
    /// `by_layer` is the colour of the layer the colour is for, so ByLayer
    /// shows that colour instead of gray.
    pub fn with_drawing(
        mut self,
        mut doc: Vec<DocColor>,
        recent: &[AcadColor],
        books: Arc<Vec<ColorBook>>,
        current_name: Option<String>,
        by_layer: Option<AcadColor>,
    ) -> Self {
        self.by_layer = by_layer;
        document::sort(&mut doc, self.doc_sort, recent);
        self.doc = doc;
        self.recent = recent.to_vec();
        self.books = books;
        if let Some(name) = current_name.filter(|name| !name.is_empty()) {
            self.show_identity(&name);
            self.current_name = Some(name.clone());
            self.pending_name = Some(name);
        }
        self
    }

    /// The loaded books changed; show book `index`.
    pub fn show_book(&mut self, books: Arc<Vec<ColorBook>>, index: usize) {
        self.books = books;
        self.book = index;
        self.book_page = 0;
        self.book_query.clear();
        self.status = None;
        self.tab = ColorPickerTab::Books;
    }

    pub fn set_status(&mut self, status: String) {
        self.status = Some(status);
    }

    /// Turn to the page holding `BOOK$NAME`, if a loaded book has it.
    fn show_identity(&mut self, identity: &str) {
        let Some((book_name, name)) = identity.split_once('$') else {
            return;
        };
        for (i, book) in self.books.iter().enumerate() {
            if let Some(color) = book.find(name).filter(|_| book.name == book_name) {
                self.book = i;
                self.book_page = book.page_of(color);
                return;
            }
        }
    }

    /// The Document tab's colours that match the search, by index in `doc`.
    fn doc_visible(&self) -> Vec<usize> {
        let query = self.doc_query.trim().to_lowercase();
        (0..self.doc.len())
            .filter(|&i| query.is_empty() || contains_ignore_case(&self.doc[i].label(), &query))
            .collect()
    }

    /// The shown book's colours on screen, by index in the book: the page,
    /// or the first matches while searching.
    fn book_visible(&self) -> Vec<usize> {
        let Some(book) = self.books.get(self.book) else {
            return Vec::new();
        };
        let query = self.book_query.trim().to_lowercase();
        if query.is_empty() {
            let (start, colors) = book.page(self.book_page);
            return (start..start + colors.len()).collect();
        }
        (0..book.colors.len())
            .filter(|&i| contains_ignore_case(&book.colors[i].name, &query))
            .take(PAGE_BARS)
            .collect()
    }

    /// The colour of the shown book that is the new colour, if any.
    fn book_selection(&self) -> Option<usize> {
        let (book_name, name) = self.pending_name.as_deref()?.split_once('$')?;
        let book = self
            .books
            .get(self.book)
            .filter(|book| book.name == book_name)?;
        book.find(name)
    }

    pub fn pending(&self) -> AcadColor {
        self.pending
    }

    pub fn tab(&self) -> ColorPickerTab {
        self.tab
    }

    /// Interface colours (Options) only get the colour editor.
    fn editor_only(&self) -> bool {
        matches!(self.target, ColorPickTarget::Interface(_))
    }

    fn set_pending(&mut self, color: AcadColor) {
        self.pending = color;
        self.pending_name = None;
        if let Some(rgb) = math::color_rgb(color) {
            self.editor.set_rgb(rgb);
        }
    }

    fn rgb(&self) -> (u8, u8, u8) {
        self.editor.rgb()
    }

    pub fn update(&mut self, msg: ColorPickerMsg) -> Option<Outcome> {
        // While picking from the screen only the eyedropper listens; Esc
        // backs out of it rather than closing the panel.
        if let Some(eye) = self.eyedropper.as_mut() {
            match msg {
                ColorPickerMsg::Eyedropper(EyedropperMsg::Move(p)) => {
                    return eye.track(p).then_some(Outcome::Screenshot);
                }
                ColorPickerMsg::Eyedropper(EyedropperMsg::Shot(shot)) => eye.set_shot(shot),
                ColorPickerMsg::Eyedropper(EyedropperMsg::Pick) => {
                    if let Some((r, g, b)) = eye.picked() {
                        self.eyedropper = None;
                        self.set_pending(AcadColor::Rgb { r, g, b });
                    }
                }
                // Esc leaves the eyedropper but keeps the panel open.
                ColorPickerMsg::Cancel => self.eyedropper = None,
                _ => {}
            }
            return None;
        }
        match msg {
            ColorPickerMsg::Tab(tab) => self.tab = tab,
            ColorPickerMsg::Pick(color) => self.set_pending(color),
            ColorPickerMsg::Revert => {
                self.set_pending(self.current);
                self.pending_name = self.current_name.clone();
            }
            ColorPickerMsg::Editor(editor) => {
                if self.editor.update(editor) {
                    let (r, g, b) = self.editor.rgb();
                    self.pending = AcadColor::Rgb { r, g, b };
                    self.pending_name = None;
                }
            }
            ColorPickerMsg::UseNearest => {
                let (r, g, b) = self.rgb();
                self.set_pending(AcadColor::Index(math::nearest_aci(r, g, b)));
            }
            ColorPickerMsg::Arrow(direction) => match (self.tab, self.pending) {
                (ColorPickerTab::Index, AcadColor::Index(index)) => {
                    if let Some(next) = step_index(index, direction) {
                        self.set_pending(AcadColor::Index(next));
                    }
                }
                // Only over the colours on screen, so a search is respected.
                (ColorPickerTab::Document, _) => {
                    let visible = self.doc_visible();
                    let at = visible.iter().position(|&i| {
                        self.doc[i].color == self.pending && self.doc[i].name == self.pending_name
                    });
                    if let Some(next) = step_list(at, visible.len(), DOC_COLUMNS, direction) {
                        self.update(ColorPickerMsg::PickDocument(visible[next]));
                    }
                }
                (ColorPickerTab::Books, _) => {
                    // Without a search the arrows walk the whole book, page
                    // after page.
                    let visible: Vec<usize> = if self.book_query.trim().is_empty() {
                        (0..self
                            .books
                            .get(self.book)
                            .map_or(0, |book| book.colors.len()))
                            .collect()
                    } else {
                        self.book_visible()
                    };
                    let selected = self.book_selection();
                    let at = visible.iter().position(|&i| Some(i) == selected);
                    if let Some(next) = step_list(at, visible.len(), 1, direction) {
                        self.pick_book(visible[next]);
                    }
                }
                _ => {}
            },
            ColorPickerMsg::PickDocument(i) => {
                if let Some(entry) = self.doc.get(i) {
                    let (color, name) = (entry.color, entry.name.clone());
                    self.set_pending(color);
                    self.pending_name = name;
                }
            }
            ColorPickerMsg::DocView(view) => self.doc_view = view,
            ColorPickerMsg::DocQuery(query) => self.doc_query = query,
            ColorPickerMsg::DocSort(by) => {
                self.doc_sort = by;
                document::sort(&mut self.doc, by, &self.recent);
            }
            ColorPickerMsg::Book(i) => {
                if i < self.books.len() {
                    self.book = i;
                    self.book_page = 0;
                    self.book_query.clear();
                }
            }
            ColorPickerMsg::BookPage(page) => {
                if let Some(book) = self.books.get(self.book) {
                    self.book_page = page.min(book.page_count().saturating_sub(1));
                    self.book_query.clear();
                }
            }
            ColorPickerMsg::BookScroll(delta) => {
                let page = if delta < 0.0 {
                    self.book_page + 1
                } else {
                    self.book_page.saturating_sub(1)
                };
                self.update(ColorPickerMsg::BookPage(page));
            }
            ColorPickerMsg::BookQuery(query) => self.book_query = query,
            ColorPickerMsg::PickBook(i) => self.pick_book(i),
            ColorPickerMsg::LoadBook => return Some(Outcome::LoadBook),
            ColorPickerMsg::Eyedropper(EyedropperMsg::Start) => {
                self.eyedropper = Some(Eyedropper::default());
            }
            ColorPickerMsg::Eyedropper(_) => {}
            ColorPickerMsg::Ok => {
                // Apply a value still being typed, as if Enter had been pressed.
                self.update(ColorPickerMsg::Editor(EditorMsg::HexSubmit));
                return Some(Outcome::Commit(self.pending, self.pending_name.clone()));
            }
            ColorPickerMsg::Cancel => return Some(Outcome::Cancel),
        }
        None
    }

    /// How a colour looks; ByLayer takes the layer's colour when known.
    fn shown(&self, color: AcadColor) -> Color {
        match (color, self.by_layer) {
            (AcadColor::ByLayer, Some(layer)) => acad_color_display(layer).0,
            _ => acad_color_display(color).0,
        }
    }

    fn pick_book(&mut self, index: usize) {
        let books = Arc::clone(&self.books);
        let Some(book) = books.get(self.book) else {
            return;
        };
        if let Some(color) = book.colors.get(index) {
            let (r, g, b) = color.rgb;
            self.set_pending(AcadColor::Rgb { r, g, b });
            self.pending_name = Some(book.identity(index));
            self.book_page = book.page_of(index);
        }
    }

    /// The full-window eyedropper layer, while it is active.
    pub fn eyedropper_view(&self) -> Option<Element<'_, Message>> {
        self.eyedropper.as_ref().map(Eyedropper::view)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let editor_only = self.editor_only();
        let body = match self.tab {
            _ if editor_only => self.true_color_tab(),
            ColorPickerTab::Document => self.document_tab(),
            ColorPickerTab::Index => self.index_tab(),
            ColorPickerTab::TrueColor => self.true_color_tab(),
            ColorPickerTab::Books => self.books_tab(),
        };

        // The interface picker shrinks to the editor; the CAD panel keeps one
        // height across its tabs, with the info line at the bottom.
        let mut body = column![body].spacing(8);
        if !editor_only {
            // The other tabs fill the height themselves.
            if self.tab == ColorPickerTab::Index {
                body = body.push(Space::new().height(Length::Fill));
            }
            let fill = self.shown(self.pending);
            let info = match (&self.status, &self.pending_name) {
                (Some(status), _) => status.clone(),
                (None, Some(name)) => {
                    let (r, g, b) = math::color_rgb(self.pending).unwrap_or_default();
                    format!("{} · RGB {r}, {g}, {b}", name.replacen('$', " - ", 1))
                }
                (None, None) => math::describe(self.pending),
            };
            let error = self.status.is_some();
            body = body.push(divider()).push(
                row![
                    container(Space::new())
                        .width(12)
                        .height(12)
                        .style(move |_theme: &Theme| swatch_style(fill)),
                    text(info)
                        .size(11)
                        .font(iced::Font::MONOSPACE)
                        .style(move |theme: &Theme| text::Style {
                            color: error.then(|| theme.palette().danger.base.color),
                        }),
                ]
                .spacing(8)
                .align_y(iced::Center),
            );
        }
        let body = container(body)
            .padding(12)
            .width(Length::Fill)
            .height(if editor_only {
                Length::Shrink
            } else {
                Length::Fixed(BODY_HEIGHT)
            })
            .style(panel_style);

        let actions = row![
            Space::new().width(Length::Fill),
            button(text(t!("Cancel")).size(12))
                .on_press(picker_msg(ColorPickerMsg::Cancel))
                .style(button::secondary)
                .padding([6, 16]),
            button(text(t!("OK")).size(12))
                .on_press(picker_msg(ColorPickerMsg::Ok))
                .style(button::primary)
                .padding([6, 22]),
        ]
        .spacing(8);

        let mut panel = column![].spacing(12).width(PANEL_WIDTH);
        if !editor_only {
            panel = panel.push(self.tabs());
        }
        container(panel.push(self.preview_row()).push(body).push(actions))
            .id(PANEL_ID)
            .into()
    }

    fn tabs(&self) -> Element<'_, Message> {
        let tab = |label: String, tab: ColorPickerTab| {
            let active = self.tab == tab;
            button(container(text(label).size(12)).center_x(Length::Fill))
                .on_press(picker_msg(ColorPickerMsg::Tab(tab)))
                .width(Length::Fill)
                .padding([5, 4])
                .style(move |theme: &Theme, status| segment_style(theme, active, status))
        };
        container(
            row![
                tab(t!("Document").into_owned(), ColorPickerTab::Document),
                tab(t!("Index Color").into_owned(), ColorPickerTab::Index),
                tab(t!("True Color").into_owned(), ColorPickerTab::TrueColor),
                tab(t!("Color Books").into_owned(), ColorPickerTab::Books),
            ]
            .spacing(2),
        )
        .padding(2)
        .style(panel_style)
        .into()
    }

    /// Current and New side by side across the full width, the divider
    /// under the middle of the tab bar. Clicking Current reverts.
    fn preview_row(&self) -> Element<'_, Message> {
        let half = |label: String, value: String, fill: Color| {
            let ink = ink(fill);
            column![
                text(label).size(11).color(ink.scale_alpha(0.85)),
                Space::new().height(Length::Fill),
                text(value).size(11).font(iced::Font::MONOSPACE).color(ink),
            ]
            .padding([8, 10])
            .width(Length::Fill)
            .height(Length::Fill)
        };
        let current_bg = self.shown(self.current);
        let new_bg = self.shown(self.pending);
        // A colour-book colour shows its own name, without the book.
        let name = |color: AcadColor, identity: &Option<String>| match identity {
            Some(identity) => identity
                .split_once('$')
                .map_or(identity.as_str(), |(_, name)| name)
                .to_string(),
            None => math::short_name(color),
        };
        let current_value = if self.varies {
            "*VARIES*".to_string()
        } else {
            name(self.current, &self.current_name)
        };
        let current = button(half(t!("Current").into_owned(), current_value, current_bg))
            .on_press(picker_msg(ColorPickerMsg::Revert))
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(current_bg.into()),
                border: Border {
                    radius: iced::border::radius(7.0).right(0.0),
                    ..Border::default()
                },
                ..Default::default()
            });
        let divider = container(Space::new())
            .width(1)
            .height(Length::Fill)
            .style(|_theme: &Theme| filled(Color::from_rgba(0.0, 0.0, 0.0, 0.35)));
        let new = container(half(
            t!("New").into_owned(),
            name(self.pending, &self.pending_name),
            new_bg,
        ))
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                radius: iced::border::radius(7.0).left(0.0),
                ..Border::default()
            },
            ..filled(new_bg)
        });
        container(row![current, divider, new])
            .width(Length::Fill)
            .height(60)
            .padding(1)
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .clip(true)
            .into()
    }

    fn index_tab(&self) -> Element<'_, Message> {
        let section = |label: String| text(label).size(11);
        let swatches = |range: std::ops::RangeInclusive<u8>| {
            range.fold(row![].spacing(5), |line, index| {
                line.push(self.index_swatch(index, 30.0, 26.0))
            })
        };
        // ACI 10–249 come in groups of ten: 24 hue families across, their ten
        // shades down.
        let grid = (0u8..10).fold(column![].spacing(2), |grid, shade| {
            grid.push((1u8..=24).fold(row![].spacing(2), |line, family| {
                line.push(self.index_swatch(family * 10 + shade, 13.0, 13.0))
            }))
        });
        column![
            column![section(t!("Standard colors").into_owned()), swatches(1..=9)].spacing(6),
            column![
                section(t!("CAD Color Index (ACI) 10–249").into_owned()),
                grid
            ]
            .spacing(6),
            column![
                section(t!("Grayscale 250–255").into_owned()),
                swatches(250..=255)
            ]
            .spacing(6),
        ]
        .spacing(12)
        .into()
    }

    /// One index colour of the Index tab.
    fn index_swatch(&self, index: u8, width: f32, height: f32) -> Element<'_, Message> {
        let color = AcadColor::Index(index);
        swatch(
            fill_of(color),
            self.pending == color,
            ColorPickerMsg::Pick(color),
            Length::Fixed(width),
            Length::Fixed(height),
        )
    }

    fn document_tab(&self) -> Element<'_, Message> {
        // Search on the left half; sorting and the view buttons on the right.
        let header = row![
            container(search_box(&self.doc_query, |query| {
                ColorPickerMsg::DocQuery(query)
            }))
            .width(Length::FillPortion(1)),
            row![
                pick_list(Some(self.doc_sort), &DocSort::ALL[..], DocSort::to_string)
                    .on_select(|by| picker_msg(ColorPickerMsg::DocSort(by)))
                    .text_size(12)
                    .padding([6, 8])
                    .width(Length::Fill),
                row![
                    self.view_button(crate::ui::icons::GRID, DocView::Grid),
                    self.view_button(crate::ui::icons::LIST, DocView::List),
                ]
                .spacing(2),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .width(Length::FillPortion(1)),
        ]
        .spacing(8)
        .align_y(iced::Center);
        if self.doc.is_empty() {
            return column![
                header,
                empty_note(t!("No colors in this drawing yet").into_owned())
            ]
            .spacing(10)
            .into();
        }
        // Indices stay those of `doc`, which the pick messages refer to.
        let shown: Vec<(usize, &DocColor)> = self
            .doc_visible()
            .into_iter()
            .map(|i| (i, &self.doc[i]))
            .collect();
        if shown.is_empty() {
            return column![header, empty_note(t!("No colors match").into_owned())]
                .spacing(10)
                .into();
        }
        let list: Element<'_, Message> = if self.doc_view == DocView::List {
            shown
                .iter()
                .fold(column![].spacing(2), |list, &(i, entry)| {
                    let fill = fill_of(entry.color);
                    let selected = self.pending == entry.color && self.pending_name == entry.name;
                    list.push(
                        color_bar(
                            entry.label(),
                            fill,
                            selected,
                            ColorPickerMsg::PickDocument(i),
                        )
                        .height(26),
                    )
                })
                .into()
        } else {
            self.doc_grid(&shown)
        };
        column![
            header,
            scrollable(list).height(Length::Fill).spacing(4).direction(
                scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                )
            ),
        ]
        .spacing(10)
        .into()
    }

    /// Grid or list toggle for the Document tab, styled like the app's other
    /// toggles: light accent when active, its icon in that accent's text
    /// colour.
    fn view_button(&self, icon: &'static [u8], view: DocView) -> Element<'_, Message> {
        let active = self.doc_view == view;
        button(
            container(
                svg(crate::ui::icons::themed_handle(icon))
                    .width(18)
                    .height(18)
                    .style(move |theme: &Theme, _status| svg::Style {
                        color: Some(if active {
                            theme.palette().primary.weak.text
                        } else {
                            theme.palette().background.base.text
                        }),
                    }),
            )
            .center(Length::Fill),
        )
        .on_press(picker_msg(ColorPickerMsg::DocView(view)))
        .width(CONTROL_HEIGHT)
        .height(CONTROL_HEIGHT)
        .padding(0)
        .style(move |theme: &Theme, status| toggle_style(theme, active, status))
        .into()
    }

    fn doc_grid<'a>(&self, shown: &[(usize, &'a DocColor)]) -> Element<'a, Message> {
        shown
            .chunks(DOC_COLUMNS)
            .fold(column![].spacing(4), |grid, chunk| {
                let line = chunk.iter().fold(row![].spacing(4), |line, &(i, entry)| {
                    line.push(self.doc_swatch(i, entry))
                });
                let line = (chunk.len()..DOC_COLUMNS)
                    .fold(line, |line, _| line.push(Space::new().width(Length::Fill)));
                grid.push(line)
            })
            .into()
    }

    fn doc_swatch<'a>(&self, i: usize, entry: &'a DocColor) -> Element<'a, Message> {
        swatch(
            fill_of(entry.color),
            self.pending == entry.color && self.pending_name == entry.name,
            ColorPickerMsg::PickDocument(i),
            Length::Fill,
            Length::Fixed(30.0),
        )
    }

    fn books_tab(&self) -> Element<'_, Message> {
        let load_button = |content: Element<'static, Message>| {
            button(content)
                .on_press(picker_msg(ColorPickerMsg::LoadBook))
                .style(tab_button_style)
        };
        let Some(book) = self.books.get(self.book) else {
            return container(
                column![
                    text(t!("No color books loaded")).size(12).style(muted_text),
                    load_button(text(t!("Load color book (.acb)")).size(12).into())
                        .padding([6, 14]),
                ]
                .spacing(10)
                .align_x(iced::Center),
            )
            .center(Length::Fill)
            .into();
        };
        // The book list and its load button over the colour bars; the
        // search box over the page strip and the RGB values, to the end.
        let books = Arc::clone(&self.books);
        let top = row![
            row![
                pick_list(
                    Some(self.book),
                    (0..self.books.len()).collect::<Vec<_>>(),
                    move |&i| books[i].name.clone(),
                )
                .on_select(|i| picker_msg(ColorPickerMsg::Book(i)))
                .text_size(12)
                .padding([6, 8])
                .width(Length::Fill),
                load_button(
                    container(
                        tab_icon(crate::ui::icons::themed_handle(
                            crate::ui::icons::FOLDER_OPEN
                        ))
                        .width(16)
                        .height(16)
                    )
                    .center(Length::Fill)
                    .into()
                )
                .width(CONTROL_HEIGHT)
                .height(CONTROL_HEIGHT)
                .padding(0),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .width(Length::Fill),
            container(search_box(&self.book_query, |query| {
                ColorPickerMsg::BookQuery(query)
            }))
            .width(SIDE_WIDTH),
        ]
        .spacing(10)
        .align_y(iced::Center);

        // The page's colours, or the first matches while searching.
        let selected = self.book_selection();
        let searching = !self.book_query.trim().is_empty();
        let visible = self.book_visible();
        let bars: Element<'_, Message> = if visible.is_empty() {
            empty_note(t!("No colors match").into_owned())
        } else {
            let bars = visible
                .iter()
                .fold(column![].spacing(2).height(Length::Fill), |bars, &i| {
                    bars.push(book_bar(i, &book.colors[i], selected))
                });
            mouse_area(bars)
                .on_scroll(|delta| {
                    let (iced::mouse::ScrollDelta::Lines { y, .. }
                    | iced::mouse::ScrollDelta::Pixels { y, .. }) = delta;
                    picker_msg(ColorPickerMsg::BookScroll(y))
                })
                .into()
        };

        // Every page as one band of its middle colour; the shown page framed.
        let strip = (0..book.page_count()).fold(column![], |strip, page| {
            let (_, colors) = book.page(page);
            let fill = rgb_color(colors[colors.len() / 2].rgb);
            let shown = !searching && page == self.book_page;
            strip.push(
                button(Space::new())
                    .on_press(picker_msg(ColorPickerMsg::BookPage(page)))
                    .padding(0)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(fill.into()),
                        border: Border {
                            color: Color::WHITE,
                            width: if shown { 2.0 } else { 0.0 },
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }),
            )
        });
        let arrow = |path: &'static str, page: usize| {
            button(
                svg(svg::Handle::from_memory(
                    format!(
                        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 7 4'>\
                     <path d='{path}' fill='black'/></svg>"
                    )
                    .into_bytes(),
                ))
                .width(9)
                .height(5)
                .style(|theme: &Theme, _status| svg::Style {
                    color: Some(theme.palette().background.strong.text),
                }),
            )
            .on_press(picker_msg(ColorPickerMsg::BookPage(page)))
            .width(Length::Fill)
            .height(18)
            .padding([6, 8])
            .style(tab_button_style)
        };
        let strip = column![
            arrow("M0 4L3.5 0L7 4z", self.book_page.saturating_sub(1)),
            container(strip)
                .height(Length::Fill)
                .padding(1)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.strong.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
            arrow("M0 0L7 0L3.5 4z", self.book_page + 1),
        ]
        .spacing(3)
        .width(26);

        // RGB equivalent of the selected colour.
        let chosen = selected.map(|i| &book.colors[i]);
        let channel = |name: std::borrow::Cow<'static, str>, value: Option<u8>| {
            row![
                text(name).size(12).style(muted_text),
                Space::new().width(Length::Fill),
                text(value.map_or("—".to_string(), |v| v.to_string()))
                    .size(12)
                    .font(iced::Font::MONOSPACE),
            ]
        };
        let equivalent = column![
            text(t!("RGB equivalent")).size(11),
            channel(t!("Red"), chosen.map(|c| c.rgb.0)),
            channel(t!("Green"), chosen.map(|c| c.rgb.1)),
            channel(t!("Blue"), chosen.map(|c| c.rgb.2)),
            divider(),
            text(format!("{} / {}", self.book_page + 1, book.page_count()))
                .size(11)
                .style(muted_text),
            text(chosen.map_or(String::new(), |c| c.name.clone())).size(12),
        ]
        .spacing(6)
        .width(104);

        column![
            top,
            row![bars, strip, equivalent]
                .spacing(10)
                .height(Length::Fill)
        ]
        .spacing(12)
        .into()
    }

    fn true_color_tab(&self) -> Element<'_, Message> {
        // The eyedropper reads the window back from the GPU, which the web
        // build cannot do.
        let pipette: Option<Element<'_, Message>> = if cfg!(target_arch = "wasm32") {
            None
        } else {
            Some(
                // Just the icon, without a button frame.
                button(
                    // Gray at rest; the accent colour while the pointer is on it.
                    iced::widget::svg(crate::ui::icons::themed_handle(
                        crate::ui::icons::EYEDROPPER,
                    ))
                    .width(24)
                    .height(24)
                    .style(|theme: &Theme, status| iced::widget::svg::Style {
                        color: Some(match status {
                            iced::widget::svg::Status::Hovered => {
                                theme.palette().primary.base.color
                            }
                            iced::widget::svg::Status::Idle => {
                                theme.palette().background.base.text.scale_alpha(0.5)
                            }
                        }),
                    }),
                )
                .on_press(picker_msg(ColorPickerMsg::Eyedropper(EyedropperMsg::Start)))
                .padding(0)
                .style(|_theme: &Theme, _status| button::Style::default())
                .into(),
            )
        };
        let nearest = (!self.editor_only()).then(|| self.nearest_button());
        self.editor.view(pipette, nearest, !self.editor_only())
    }

    fn nearest_button(&self) -> Element<'_, Message> {
        let (r, g, b) = self.rgb();
        let nearest = math::nearest_aci(r, g, b);
        let nearest_rgb = math::aci_rgb(nearest).unwrap_or((128, 128, 128));
        let exact = nearest_rgb == (r, g, b) && !matches!(self.pending, AcadColor::Index(_));
        let fill = rgb_color(nearest_rgb);
        row![
            text(if exact {
                t!("Exact index match")
            } else {
                t!("Nearest index")
            })
            .size(11)
            .style(move |theme: &Theme| text::Style {
                color: Some(if exact {
                    theme.palette().success.base.color
                } else {
                    theme.palette().background.base.text.scale_alpha(0.65)
                }),
            }),
            button(
                row![
                    container(Space::new())
                        .width(16)
                        .height(16)
                        .style(move |_theme: &Theme| swatch_style(fill)),
                    text(format!("ACI {nearest}"))
                        .size(12)
                        .font(iced::Font::MONOSPACE),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(picker_msg(ColorPickerMsg::UseNearest))
            .padding([4, 8])
            .style(move |theme: &Theme, status| toggle_style(theme, exact, status)),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    }
}

/// Move through the index palette with the arrow keys: the 24×10 block moves
/// by family (left/right) and shade (up/down); the standard and gray rows
/// move along their row.
fn step_index(index: u8, direction: ArrowKey) -> Option<u8> {
    let along_row = |first: u8, last: u8| match direction {
        ArrowKey::Left if index > first => Some(index - 1),
        ArrowKey::Right if index < last => Some(index + 1),
        _ => None,
    };
    match index {
        1..=9 => along_row(1, 9),
        250..=255 => along_row(250, 255),
        10..=249 => {
            let (family, shade) = (index / 10, index % 10);
            match direction {
                ArrowKey::Left if family > 1 => Some(index - 10),
                ArrowKey::Right if family < 24 => Some(index + 10),
                ArrowKey::Up if shade > 0 => Some(index - 1),
                ArrowKey::Down if shade < 9 => Some(index + 1),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Arrow keys over a list laid out `columns` wide. Nothing selected yet
/// starts at the first entry.
fn step_list(at: Option<usize>, len: usize, columns: usize, direction: ArrowKey) -> Option<usize> {
    let Some(at) = at else {
        return (len > 0).then_some(0);
    };
    let next = match direction {
        ArrowKey::Left => at.checked_sub(1),
        ArrowKey::Right => Some(at + 1),
        ArrowKey::Up => at.checked_sub(columns),
        ArrowKey::Down => Some(at + columns),
    };
    next.filter(|&next| next < len)
}

/// Move keyboard focus to the next (or previous) field inside the panel.
pub fn focus_step<T>(forward: bool) -> iced::Task<T> {
    use iced::advanced::widget::operation::{focusable, scope};
    let id = iced::widget::Id::new(PANEL_ID);
    iced_runtime::task::effect(if forward {
        iced_runtime::Action::widget(scope(id, focusable::focus_next()))
    } else {
        iced_runtime::Action::widget(scope(id, focusable::focus_previous()))
    })
}

fn picker_msg(msg: ColorPickerMsg) -> Message {
    Message::ColorPicker(msg)
}

/// Whether `haystack` contains `needle`, ignoring case; `needle` is already
/// lower case. ASCII needles, the usual case, compare without allocating.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        true
    } else if needle.is_ascii() {
        haystack
            .as_bytes()
            .windows(needle.len().max(1))
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    } else {
        haystack.to_lowercase().contains(needle)
    }
}

/// Text that reads on top of `fill`.
fn ink(fill: Color) -> Color {
    let [r, g, b, _] = fill.into_rgba8();
    if math::ink_is_dark(r, g, b) {
        Color::from_rgb8(0x11, 0x12, 0x14)
    } else {
        Color::WHITE
    }
}

/// One colour of a colour book: a full-width bar with its name.
fn book_bar(
    i: usize,
    color: &super::book::BookColor,
    selected: Option<usize>,
) -> Element<'_, Message> {
    color_bar(
        &color.name,
        rgb_color(color.rgb),
        selected == Some(i),
        ColorPickerMsg::PickBook(i),
    )
    .height(Length::Fill)
    .into()
}

/// A full-width colour bar with a name: the Color books pages and the
/// Document tab's list.
fn color_bar<'a>(
    name: impl text::IntoFragment<'a>,
    fill: Color,
    selected: bool,
    on_press: ColorPickerMsg,
) -> button::Button<'a, Message> {
    button(
        container(
            text(name)
                .size(12)
                .font(iced::Font::MONOSPACE)
                .color(ink(fill)),
        )
        .center(Length::Fill),
    )
    .on_press(picker_msg(on_press))
    .padding(0)
    .width(Length::Fill)
    .style(move |theme: &Theme, status| swatch_button_style(theme, status, fill, selected))
}

/// A search field with a magnifier and a gray "Search" placeholder.
fn search_box<'a>(
    value: &str,
    on_input: impl Fn(String) -> ColorPickerMsg + 'a,
) -> Element<'a, Message> {
    iced::widget::stack![
        text_input(&t!("Search"), value)
            .on_input(move |query| picker_msg(on_input(query)))
            .size(12)
            .padding(iced::Padding {
                top: 6.0,
                right: 8.0,
                bottom: 6.0,
                left: 28.0,
            })
            .width(Length::Fill),
        container(
            svg(crate::ui::icons::themed_handle(crate::ui::icons::SEARCH))
                .width(14)
                .height(14)
                .style(|theme: &Theme, _status| svg::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.5)),
                }),
        )
        .padding([0, 8])
        .height(Length::Fill)
        .center_y(Length::Fill),
    ]
    .width(Length::Fill)
    .into()
}

/// A small button in the colours of the active tab: the load button and
/// the page arrows.
fn tab_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = segment_style(theme, true, status);
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.background = Some(theme.palette().background.stronger.color.into());
    }
    style
}

/// An icon in the active tab's text colour, for `tab_button_style`.
fn tab_icon<'a>(handle: svg::Handle) -> svg::Svg<'a> {
    svg(handle).style(|theme: &Theme, _status| svg::Style {
        color: Some(theme.palette().background.strong.text),
    })
}

/// A clickable colour swatch, ringed when selected or hovered.
fn swatch<'a>(
    fill: Color,
    selected: bool,
    on_press: ColorPickerMsg,
    width: Length,
    height: Length,
) -> Element<'a, Message> {
    button(Space::new())
        .on_press(picker_msg(on_press))
        .padding(0)
        .width(width)
        .height(height)
        .style(move |theme: &Theme, status| swatch_button_style(theme, status, fill, selected))
        .into()
}

/// How a concrete colour looks; gray for one without RGB.
fn fill_of(color: AcadColor) -> Color {
    math::color_rgb(color).map_or(Color::from_rgb8(128, 128, 128), rgb_color)
}

/// A 1 px separator line across the panel.
fn divider<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(|theme: &Theme| filled(theme.palette().background.weak.color))
        .into()
}

/// A short gray note in the middle of the free space.
fn empty_note<'a>(note: String) -> Element<'a, Message> {
    container(text(note).size(12).style(muted_text))
        .center(Length::Fill)
        .into()
}

/// A clickable colour: ringed in the accent colour when selected or hovered.
fn swatch_button_style(
    theme: &Theme,
    status: button::Status,
    fill: Color,
    selected: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(fill.into()),
        border: Border {
            color: if selected || hovered {
                theme.palette().primary.base.color
            } else {
                Color::from_rgba(0.0, 0.0, 0.0, 0.35)
            },
            width: if selected {
                2.5
            } else if hovered {
                1.5
            } else {
                1.0
            },
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

fn filled(color: Color) -> container::Style {
    container::Style {
        background: Some(color.into()),
        ..Default::default()
    }
}

pub(super) fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    }
}

fn swatch_style(fill: Color) -> container::Style {
    container::Style {
        background: Some(fill.into()),
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(palette.background.weakest.color.into()),
        border: Border {
            color: palette.background.weak.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn segment_style(theme: &Theme, active: bool, status: button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: if active {
            Some(palette.background.strong.color.into())
        } else if matches!(status, button::Status::Hovered) {
            Some(palette.background.weak.color.into())
        } else {
            None
        },
        text_color: if active {
            palette.background.strong.text
        } else {
            palette.background.base.text
        },
        border: Border {
            radius: 5.0.into(),
            ..Border::default()
        },
        ..Default::default()
    }
}

pub(super) fn toggle_style(theme: &Theme, active: bool, status: button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: Some(Background::Color(if active {
            palette.primary.weak.color
        } else if matches!(status, button::Status::Hovered) {
            palette.background.weak.color
        } else {
            palette.background.base.color
        })),
        text_color: if active {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        },
        border: Border {
            color: if active {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::editor::EditorMode;
    use super::*;

    fn picker(current: AcadColor) -> ColorPicker {
        ColorPicker::new(ColorPickTarget::Ribbon, current, false)
    }

    fn edit(p: &mut ColorPicker, msg: EditorMsg) {
        p.update(ColorPickerMsg::Editor(msg));
    }

    #[test]
    fn opens_on_the_document_tab() {
        assert_eq!(picker(AcadColor::Index(30)).tab(), ColorPickerTab::Document);
        assert_eq!(
            picker(AcadColor::Rgb { r: 1, g: 2, b: 3 }).tab(),
            ColorPickerTab::Document
        );
    }

    #[test]
    fn revert_restores_the_current_colour() {
        let mut p = picker(AcadColor::Index(30));
        p.update(ColorPickerMsg::Pick(AcadColor::Index(140)));
        assert_eq!(p.pending(), AcadColor::Index(140));
        p.update(ColorPickerMsg::Revert);
        assert_eq!(p.pending(), AcadColor::Index(30));
    }

    #[test]
    fn ok_commits_and_cancel_does_not() {
        let mut p = picker(AcadColor::Index(30));
        p.update(ColorPickerMsg::Pick(AcadColor::Index(5)));
        assert_eq!(
            p.update(ColorPickerMsg::Ok),
            Some(Outcome::Commit(AcadColor::Index(5), None))
        );
        assert_eq!(p.update(ColorPickerMsg::Cancel), Some(Outcome::Cancel));
    }

    #[test]
    fn ok_applies_a_hex_value_still_being_typed() {
        let mut p = picker(AcadColor::Index(30));
        edit(&mut p, EditorMsg::HexInput("#3bf".into()));
        assert_eq!(
            p.update(ColorPickerMsg::Ok),
            Some(Outcome::Commit(
                AcadColor::Rgb {
                    r: 0x33,
                    g: 0xBB,
                    b: 0xFF
                },
                None
            ))
        );
    }

    #[test]
    fn the_editor_produces_true_colours() {
        let mut p = picker(AcadColor::Index(1));
        edit(&mut p, EditorMsg::Hue(120.0));
        assert_eq!(p.pending(), AcadColor::Rgb { r: 0, g: 255, b: 0 });
        edit(&mut p, EditorMsg::Mode(EditorMode::Rgb));
        assert_eq!(p.pending(), AcadColor::Rgb { r: 0, g: 255, b: 0 });
    }

    #[test]
    fn nearest_turns_a_true_colour_into_an_index() {
        let mut p = picker(AcadColor::Rgb { r: 250, g: 4, b: 4 });
        p.update(ColorPickerMsg::UseNearest);
        assert_eq!(p.pending(), AcadColor::Index(1));
    }

    #[test]
    fn arrow_keys_walk_the_index_grid() {
        assert_eq!(step_index(140, ArrowKey::Right), Some(150));
        assert_eq!(step_index(140, ArrowKey::Left), Some(130));
        assert_eq!(step_index(140, ArrowKey::Down), Some(141));
        assert_eq!(step_index(140, ArrowKey::Up), None);
        assert_eq!(step_index(249, ArrowKey::Right), None);
        assert_eq!(step_index(5, ArrowKey::Right), Some(6));
        assert_eq!(step_index(255, ArrowKey::Right), None);

        let mut p = picker(AcadColor::Index(140));
        p.update(ColorPickerMsg::Tab(ColorPickerTab::Index));
        p.update(ColorPickerMsg::Arrow(ArrowKey::Right));
        assert_eq!(p.pending(), AcadColor::Index(150));
    }

    #[test]
    fn eyedropper_picks_a_pixel_and_esc_only_leaves_it() {
        let mut p = picker(AcadColor::Index(30));
        p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Start));
        assert_eq!(
            p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Move(
                iced::Point::ORIGIN
            ))),
            Some(Outcome::Screenshot)
        );
        // Esc and Enter while picking never close the panel.
        assert_eq!(p.update(ColorPickerMsg::Ok), None);
        assert_eq!(p.update(ColorPickerMsg::Cancel), None);
        assert!(p.eyedropper_view().is_none());

        p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Start));
        p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Move(
            iced::Point::ORIGIN,
        )));
        let shot = iced::window::Screenshot::new(vec![10, 20, 30, 255], iced::Size::new(1, 1), 1.0);
        p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Shot(shot)));
        p.update(ColorPickerMsg::Eyedropper(EyedropperMsg::Pick));
        assert!(p.eyedropper_view().is_none());
        assert_eq!(
            p.pending(),
            AcadColor::Rgb {
                r: 10,
                g: 20,
                b: 30
            }
        );
    }

    fn ral() -> Arc<Vec<ColorBook>> {
        let entry = |name: &str, (r, g, b): (u8, u8, u8)| {
            format!(
                "<colorEntry><colorName>{name}</colorName><RGB8><red>{r}</red>\
                 <green>{g}</green><blue>{b}</blue></RGB8></colorEntry>"
            )
        };
        let text = format!(
            "<colorBook><bookName>RAL CLASSIC</bookName><colorPage>{}{}</colorPage>\
             <colorPage>{}</colorPage></colorBook>",
            entry("RAL 1000", (190, 189, 127)),
            entry("RAL 3020", (204, 6, 5)),
            entry("RAL 5015", (34, 113, 179)),
        );
        Arc::new(vec![super::super::book::parse(&text).unwrap()])
    }

    #[test]
    fn book_colours_keep_their_name_until_edited() {
        let mut p = picker(AcadColor::Index(30)).with_drawing(Vec::new(), &[], ral(), None, None);
        p.update(ColorPickerMsg::PickBook(2));
        assert_eq!(p.book_page, 1, "turns to the colour's page");
        assert_eq!(
            p.update(ColorPickerMsg::Ok),
            Some(Outcome::Commit(
                AcadColor::Rgb {
                    r: 34,
                    g: 113,
                    b: 179
                },
                Some("RAL CLASSIC$RAL 5015".into())
            ))
        );
        p.update(ColorPickerMsg::Tab(ColorPickerTab::Books));
        p.update(ColorPickerMsg::Arrow(ArrowKey::Up));
        assert_eq!(p.pending_name.as_deref(), Some("RAL CLASSIC$RAL 3020"));
        edit(&mut p, EditorMsg::Hue(120.0));
        assert_eq!(
            p.pending_name, None,
            "an edited colour is no longer the book's"
        );
    }

    #[test]
    fn opens_a_book_colour_on_its_page() {
        let p = picker(AcadColor::Rgb {
            r: 34,
            g: 113,
            b: 179,
        })
        .with_drawing(
            Vec::new(),
            &[],
            ral(),
            Some("RAL CLASSIC$RAL 5015".into()),
            None,
        );
        assert_eq!(p.book_selection(), Some(2));
        assert_eq!(p.book_page, 1);
    }

    #[test]
    fn document_colours_pick_and_walk() {
        // A new drawing's layer 0 is white.
        let doc = document::collect(&codec::Document::new());
        assert!(doc.iter().any(|c| c.color == AcadColor::Index(7)));
        let mut p = picker(AcadColor::Index(30)).with_drawing(doc, &[], Arc::default(), None, None);
        p.update(ColorPickerMsg::Tab(ColorPickerTab::Document));
        p.update(ColorPickerMsg::Arrow(ArrowKey::Right));
        assert_eq!(
            p.pending(),
            p.doc[0].color,
            "the first key press starts at the top"
        );

        assert_eq!(step_list(None, 3, 8, ArrowKey::Down), Some(0));
        assert_eq!(step_list(Some(1), 20, 8, ArrowKey::Down), Some(9));
        assert_eq!(step_list(Some(1), 5, 8, ArrowKey::Down), None);
        assert_eq!(step_list(Some(0), 5, 8, ArrowKey::Left), None);
    }

    #[test]
    fn loading_a_book_is_left_to_the_app() {
        let mut p = picker(AcadColor::Index(30));
        assert_eq!(p.update(ColorPickerMsg::LoadBook), Some(Outcome::LoadBook));
        p.show_book(ral(), 0);
        assert_eq!(p.tab(), ColorPickerTab::Books);
    }

    // ── Headless UI tests ───────────────────────────────────────────────
    //
    // These run the real widgets through iced's simulator. Setting
    // OCS_COLOR_PICKER_SNAPSHOTS to a folder also writes a PNG per view.

    fn simulator(picker: &ColorPicker) -> iced_test::simulator::Simulator<'_, Message> {
        iced_test::simulator::Simulator::with_size(
            iced::Settings {
                fonts: vec![std::borrow::Cow::Borrowed(iced_aw::ICED_AW_FONT_BYTES)],
                ..iced::Settings::default()
            },
            (PANEL_WIDTH + 24.0, 560.0),
            container(picker.view()).padding(12),
        )
    }

    fn published(ui: iced_test::simulator::Simulator<'_, Message>) -> Vec<ColorPickerMsg> {
        ui.into_messages()
            .filter_map(|message| match message {
                Message::ColorPicker(msg) => Some(msg),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn clicking_a_tab_switches_it() {
        let p = picker(AcadColor::Index(30));
        let mut ui = simulator(&p);
        ui.click(t!("True Color").as_ref()).expect("True Color tab");
        assert!(published(ui)
            .iter()
            .any(|msg| matches!(msg, ColorPickerMsg::Tab(ColorPickerTab::TrueColor))));
    }

    #[test]
    fn dragging_a_slider_publishes_values() {
        let mut p = picker(AcadColor::Rgb {
            r: 57,
            g: 191,
            b: 252,
        });
        p.update(ColorPickerMsg::Tab(ColorPickerTab::TrueColor));
        edit(&mut p, EditorMsg::Mode(EditorMode::Rgb));
        let mut ui = simulator(&p);
        let label = ui.find("R").expect("R label");
        let bounds = label.visible_bounds().expect("R visible");
        ui.point_at(iced::Point::new(bounds.x + 40.0, bounds.center_y()));
        ui.simulate(iced_test::simulator::click());
        assert!(published(ui)
            .iter()
            .any(|msg| matches!(msg, ColorPickerMsg::Editor(EditorMsg::Red(v)) if *v < 57)));
    }

    #[test]
    fn interface_colours_show_only_the_editor() {
        let p = ColorPicker::new(
            ColorPickTarget::Interface(crate::app::UiColor::ModelBackground),
            AcadColor::Rgb {
                r: 30,
                g: 30,
                b: 30,
            },
            false,
        );
        let mut ui = simulator(&p);
        assert!(ui.find(t!("Index Color").as_ref()).is_err());
        assert!(ui.find(t!("Nearest index").as_ref()).is_err());
        assert!(ui.find("HSB").is_ok());
    }

    #[test]
    fn every_view_renders() {
        let folder = std::env::var_os("OCS_COLOR_PICKER_SNAPSHOTS").map(std::path::PathBuf::from);
        let blue = AcadColor::Rgb {
            r: 57,
            g: 191,
            b: 252,
        };
        let views: [(
            &str,
            ColorPickTarget,
            AcadColor,
            ColorPickerTab,
            EditorMode,
            bool,
        ); 10] = [
            (
                "document",
                ColorPickTarget::Properties,
                AcadColor::Index(7),
                ColorPickerTab::Document,
                EditorMode::Hsb,
                false,
            ),
            (
                "books",
                ColorPickTarget::Properties,
                AcadColor::Index(30),
                ColorPickerTab::Books,
                EditorMode::Hsb,
                false,
            ),
            (
                "no-books",
                ColorPickTarget::Properties,
                AcadColor::Index(30),
                ColorPickerTab::Books,
                EditorMode::Hsb,
                false,
            ),
            (
                "index",
                ColorPickTarget::Properties,
                AcadColor::Index(140),
                ColorPickerTab::Index,
                EditorMode::Hsb,
                false,
            ),
            (
                "varies",
                ColorPickTarget::Properties,
                AcadColor::Index(30),
                ColorPickerTab::Index,
                EditorMode::Hsb,
                true,
            ),
            (
                "hsb",
                ColorPickTarget::Properties,
                blue,
                ColorPickerTab::TrueColor,
                EditorMode::Hsb,
                false,
            ),
            (
                "rgb",
                ColorPickTarget::Properties,
                blue,
                ColorPickerTab::TrueColor,
                EditorMode::Rgb,
                false,
            ),
            (
                "cmyk",
                ColorPickTarget::Properties,
                blue,
                ColorPickerTab::TrueColor,
                EditorMode::Cmyk,
                false,
            ),
            (
                "exact",
                ColorPickTarget::Properties,
                AcadColor::Rgb { r: 255, g: 0, b: 0 },
                ColorPickerTab::TrueColor,
                EditorMode::Hsb,
                false,
            ),
            (
                "interface",
                ColorPickTarget::Interface(crate::app::UiColor::ModelBackground),
                AcadColor::Rgb {
                    r: 33,
                    g: 40,
                    b: 48,
                },
                ColorPickerTab::TrueColor,
                EditorMode::Hsb,
                false,
            ),
        ];
        for theme in [
            crate::ui::style::fusion_theme::fusion_black(),
            crate::ui::style::fusion_theme::fusion_white(),
        ] {
            for (name, target, color, tab, mode, varies) in views.clone() {
                let books = if name == "no-books" {
                    Arc::default()
                } else {
                    ral()
                };
                let mut p = ColorPicker::new(target, color, varies).with_drawing(
                    document::collect(&codec::Document::new()),
                    &[],
                    books,
                    None,
                    None,
                );
                p.update(ColorPickerMsg::Tab(tab));
                if name == "books" {
                    p.update(ColorPickerMsg::PickBook(1));
                }
                edit(&mut p, EditorMsg::Mode(mode));
                let mut ui = simulator(&p);
                let snapshot = ui.snapshot(&theme).expect("snapshot");
                if let Some(folder) = &folder {
                    let shade = if theme.palette().background.base.color.r < 0.5 {
                        "dark"
                    } else {
                        "light"
                    };
                    let path = folder.join(format!("{shade}-{name}.png"));
                    let _ = std::fs::remove_file(&path);
                    snapshot.matches_image(&path).expect("write snapshot");
                }
            }
        }
    }
}
