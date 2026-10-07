//! The Count palette (COUNTLIST) and count mode (COUNT): the drawing's
//! blocks with how many references of each are counted, and — once a block
//! is chosen — its counted references coloured in the drawing with a
//! floating toolbar above it. Counting itself is `codec::count`.

use crate::app::Message;
use crate::ui::dock::PanelId;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, field_style};
use codec::count::{BlockInstance, CountKey};
use codec::{CadDocument, Handle};
use iced::widget::{button, checkbox, column, container, mouse_area, row, scrollable, text, text_input, tooltip, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

const BLOCK_ICON: &[u8] = include_bytes!("../../../assets/icons/blocks/block.svg");
const AREA_ICON: &[u8] = include_bytes!("../../../assets/icons/shapes/rect.svg");
const SELECT_ICON: &[u8] = include_bytes!("../../../assets/icons/blocks/select_objects.svg");
const TABLE_ICON: &[u8] = include_bytes!("../../../assets/icons/table.svg");
const WARNING_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");
const CHECK_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/check.svg");

/// The name of the layer count areas are drawn on.
pub const AREA_LAYER: &str = "0-CountArea";

/// What a count counts.
#[derive(Debug, Clone, PartialEq)]
pub enum CountTarget {
    /// The references of a block. `reference` is the instance the match
    /// options compare with; `picked` when it came from a COUNT selection.
    Block {
        name: String,
        reference: Option<Handle>,
        matching: [bool; 3],
        picked: bool,
    },
    /// Picked objects (several, or one that is not a reference): the copies
    /// of the group are counted.
    Group(Vec<Handle>),
}

/// Count mode of a drawing.
#[derive(Debug, Clone, Default)]
pub struct CountMode {
    /// The count area (a closed WCS polygon); `None` = all of model space.
    pub area: Option<Vec<[f64; 2]>>,
    /// The closed polyline marking the area, and whether count mode drew it
    /// (then COUNTCLOSE erases it unless a field refers to it).
    pub boundary: Option<(Handle, bool)>,
    pub target: Option<CountTarget>,
    /// The counted reference ← / → last showed.
    pub cursor: Option<usize>,
    /// The scene geometry epoch the colouring was computed at.
    pub epoch: u64,
    /// The result and count at `epoch` (recomputed when the drawing changes).
    pub cache: Option<(CountResult, usize)>,
}

/// A count mode's result: the counted references and the overlapping
/// duplicates (errors).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CountResult {
    pub counted: Vec<Handle>,
    pub errors: Vec<Handle>,
    /// The counted references a duplicate overlaps (drawn in the error colour too).
    pub overlapped: Vec<Handle>,
}

impl CountMode {
    pub fn instances(&self, doc: &CadDocument) -> Vec<BlockInstance> {
        codec::count::block_instances(doc, self.area.as_deref())
    }

    /// The key the match options build from the reference instance.
    pub fn key(&self, instances: &[BlockInstance]) -> CountKey {
        match &self.target {
            Some(CountTarget::Block { reference: Some(r), matching, .. }) => instances
                .iter()
                .find(|i| i.handle == *r)
                .map(|i| CountKey::of(i, matching[0], matching[1], matching[2]))
                .unwrap_or_default(),
            _ => CountKey::default(),
        }
    }

    /// The cached result, or one worked out from the drawing when there is none.
    pub fn result(&self, doc: &CadDocument) -> std::borrow::Cow<'_, CountResult> {
        match &self.cache {
            Some((result, _)) => std::borrow::Cow::Borrowed(result),
            None => std::borrow::Cow::Owned(self.compute(doc).0),
        }
    }

    /// What the count shows: the counted references, or the copies of a group.
    pub fn count(&self, doc: &CadDocument) -> usize {
        match &self.cache {
            Some((_, n)) => *n,
            None => self.compute(doc).1,
        }
    }

    /// The result and the count, worked out from the drawing.
    pub fn compute(&self, doc: &CadDocument) -> (CountResult, usize) {
        if let Some(CountTarget::Group(handles)) = &self.target {
            let groups = codec::count::group_matches(doc, handles, self.area.as_deref());
            let result = CountResult { counted: groups.concat(), errors: Vec::new(), overlapped: Vec::new() };
            return (result, groups.len());
        }
        let result = self.block_result(doc);
        let n = result.counted.len();
        (result, n)
    }

    fn block_result(&self, doc: &CadDocument) -> CountResult {
        match &self.target {
            None | Some(CountTarget::Group(_)) => CountResult::default(),
            Some(CountTarget::Block { name, .. }) => {
                let instances = self.instances(doc);
                let key = self.key(&instances);
                let mine = |i: &&BlockInstance| i.name.eq_ignore_ascii_case(name) && key.matches(i);
                CountResult {
                    counted: instances.iter().filter(mine).filter(|i| i.duplicate_of.is_none()).map(|i| i.handle).collect(),
                    errors: instances.iter().filter(mine).filter(|i| i.duplicate_of.is_some()).map(|i| i.handle).collect(),
                    overlapped: instances.iter().filter(mine).filter_map(|i| i.duplicate_of).collect(),
                }
            }
        }
    }

    /// The name the toolbar and the command line show for the target.
    pub fn target_name(&self) -> Option<String> {
        match &self.target {
            Some(CountTarget::Block { name, .. }) => Some(name.clone()),
            Some(CountTarget::Group(_)) => Some("group".into()),
            None => None,
        }
    }
}

/// Palette state that is not in the drawing, and the profile variables
/// COUNTCOLOR, COUNTERRORCOLOR and COUNTSERVICE.
#[derive(Debug)]
pub struct CountPalette {
    pub show: bool,
    pub search: String,
    /// Sorted by the Count column (else by Name), and descending.
    pub by_count: bool,
    pub descending: bool,
    /// Expanded rows: block name (upper case) → expand by layer, scale, mirror state.
    pub expanded: Vec<(String, [bool; 3])>,
    /// Create Table selection mode: the checked block names (upper case).
    pub table: Option<Vec<String>>,
    pub details_closed: bool,
    pub errors_closed: bool,
    pub color: i16,
    pub error_color: i16,
    pub service: bool,
    /// The Invalid Area dialog's "always" box while it is open.
    pub invalid_always: bool,
    /// What a lost count boundary does without asking: 0 ask, 1 undo, 2 go
    /// on with all of model space (kept with the user settings).
    pub invalid_choice: u8,
    /// The list's references for (document tab id, geometry epoch, area).
    pub list_cache: std::cell::RefCell<Option<(u64, u64, Option<Vec<[f64; 2]>>, std::rc::Rc<Vec<BlockInstance>>)>>,
}

impl Default for CountPalette {
    fn default() -> Self {
        Self {
            show: false,
            search: String::new(),
            by_count: false,
            descending: false,
            expanded: Vec::new(),
            table: None,
            details_closed: false,
            errors_closed: false,
            color: 3,
            error_color: 1,
            service: true,
            invalid_always: false,
            invalid_choice: 0,
            list_cache: Default::default(),
        }
    }
}

impl CountPalette {
    pub fn expansion(&self, name: &str) -> Option<[bool; 3]> {
        let key = name.to_ascii_uppercase();
        self.expanded.iter().find(|(n, _)| *n == key).map(|(_, e)| *e)
    }
}

/// What a row's context menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Review,
    Field,
    /// Expand by layer (0), scale (1) or mirror state (2).
    Expand(usize),
}

#[derive(Debug, Clone)]
pub enum CountMsg {
    Search(String),
    /// Sort by the Count column (`true`) or the Name column.
    Sort(bool),
    /// A row clicked: count mode on its block (and key, for an expanded row).
    Open(String, CountKey),
    Menu(String, CountKey, RowAction),
    CreateTable,
    TableCheck(String, bool),
    TableAll(bool),
    TableCancel,
    TableInsert,
    Back,
    Match(usize, bool),
    ToggleDetails,
    ToggleErrors,
    /// Show an overlapping duplicate.
    ShowError(Handle),
    Area,
    Prev,
    Next,
    Select,
    Field,
    Close,
    /// The Invalid Area dialog: undo the boundary change (also its ✕ and
    /// Cancel), or go on counting all of model space; `InvalidAlways` is its
    /// "always perform my current choice" box.
    InvalidUndo,
    InvalidContinue,
    InvalidAlways(bool),
}

fn msg(m: CountMsg) -> Message {
    Message::Count(m)
}

/// One block's row: counted references and duplicates.
pub struct Row {
    pub name: String,
    pub count: usize,
    pub errors: usize,
}

/// The rows of the list: blocks whose name contains the search text
/// (case-insensitively), sorted by the chosen column.
pub fn rows(instances: &[BlockInstance], palette: &CountPalette) -> Vec<Row> {
    let query = palette.search.trim().to_lowercase();
    let mut rows: Vec<Row> = codec::count::block_counts(instances)
        .into_iter()
        .filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query))
        .map(|c| Row { count: c.counted.len(), errors: c.duplicates.len(), name: c.name })
        .collect();
    if palette.by_count {
        rows.sort_by(|a, b| a.count.cmp(&b.count).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    }
    if palette.descending {
        rows.reverse();
    }
    rows
}

fn small_icon<'a>(bytes: &'static [u8], size: f32) -> Element<'a, Message> {
    crate::ui::icons::semantic(bytes, size)
}

fn row_menu(name: String, key: CountKey, expandable: bool) -> Element<'static, Message> {
    let entry = |label: String, action: RowAction| -> Element<'static, Message> {
        button(text(label).size(12))
            .width(Fill)
            .padding([5, 12])
            .on_press(msg(CountMsg::Menu(name.clone(), key.clone(), action)))
            .style(|theme: &Theme, status| {
                let mut s = button::text(theme, status);
                if matches!(status, button::Status::Hovered) {
                    s.background = Some(Background::Color(theme.palette().primary.base.color));
                    s.text_color = theme.palette().primary.base.text;
                }
                s
            })
            .into()
    };
    let mut items = column![
        entry(crate::t!("Review Count Details").into_owned(), RowAction::Review),
        entry(crate::t!("Insert Count Field").into_owned(), RowAction::Field),
    ]
    .spacing(1);
    if expandable {
        items = items
            .push(container(Space::new()).height(1).width(Fill).style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            }))
            .push(container(text(crate::t!("EXPAND BY")).size(10).style(muted_style)).padding([3, 12]))
            .push(entry(crate::t!("Layer").into_owned(), RowAction::Expand(0)))
            .push(entry(crate::t!("Scale").into_owned(), RowAction::Expand(1)))
            .push(entry(crate::t!("Mirror State").into_owned(), RowAction::Expand(2)));
    }
    container(items)
        .padding(4)
        .width(Length::Fixed(200.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border { color: theme.palette().background.neutral.color, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        })
        .into()
}

fn list_row<'a>(
    palette: &CountPalette,
    name: &str,
    key: CountKey,
    label: String,
    count: Option<usize>,
    errors: bool,
    depth: u16,
    expanded: Option<bool>,
) -> Element<'a, Message> {
    let lead: Element<'a, Message> = if let Some(checked) = palette.table.as_ref().filter(|_| depth == 0) {
        let on = checked.contains(&name.to_ascii_uppercase());
        let n = name.to_string();
        checkbox(on).size(14).on_toggle(move |v| msg(CountMsg::TableCheck(n.clone(), v))).into()
    } else if expanded == Some(true) {
        container(crate::ui::icons::themed(crate::ui::icons::MINUS, 12.0)).width(Length::Fixed(14.0)).into()
    } else {
        Space::new().width(Length::Fixed(14.0)).into()
    };
    let mut cells = row![Space::new().width(Length::Fixed(f32::from(depth) * 18.0)), lead];
    if depth == 0 {
        cells = cells.push(small_icon(BLOCK_ICON, 14.0));
    }
    cells = cells.push(text(label).size(12).width(Fill));
    if errors {
        cells = cells.push(crate::ui::icons::themed_warning(WARNING_ICON, 12.0));
    }
    // An expanded row shows its counts on the rows below it.
    cells = cells.push(match count {
        Some(n) => Element::from(text(n.to_string()).size(12)),
        None => crate::ui::icons::themed(crate::ui::icons::MORE, 12.0),
    });
    let body = container(cells.spacing(6).align_y(iced::Center)).width(Fill).padding([4, 6]);
    let area = mouse_area(body)
        .on_press(msg(CountMsg::Open(name.to_string(), key.clone())))
        .interaction(iced::mouse::Interaction::Pointer);
    let (n, expandable) = (name.to_string(), depth == 0);
    iced_aw::ContextMenu::new(area, move || row_menu(n.clone(), key.clone(), expandable)).into()
}

fn header_button<'a>(label: String, active: bool, descending: bool, m: CountMsg) -> Element<'a, Message> {
    let mut content = row![text(label).size(11)].spacing(3).align_y(iced::Center);
    if active {
        content = content.push(crate::ui::icons::themed_arrow_toggle(!descending, 8.0));
    }
    button(content)
        .on_press(msg(m))
        .style(button::text)
        .padding([2, 4])
        .into()
}

/// The list: search, area button, Name / Count columns, rows, Create Table.
fn list_view<'a>(palette: &'a CountPalette, instances: &[BlockInstance]) -> Element<'a, Message> {
    let search = text_input(&crate::t!("Search"), &palette.search)
        .on_input(|v| msg(CountMsg::Search(v)))
        .padding([4, 8])
        .size(12)
        .style(field_style);
    let top = row![
        search.width(Fill),
        crate::ui::dock::tool_button(
            small_icon(AREA_ICON, crate::ui::dock::TOOL_H),
            crate::t!("Count in a specified area").into_owned(),
            msg(CountMsg::Area),
        ),
    ]
    .spacing(4)
    .align_y(iced::Center);
    let header = row![
        header_button(crate::t!("Name").into_owned(), !palette.by_count, palette.descending, CountMsg::Sort(false)),
        Space::new().width(Fill),
        header_button(crate::t!("Count").into_owned(), palette.by_count, palette.descending, CountMsg::Sort(true)),
    ]
    .align_y(iced::Center);
    let mut list = column![].spacing(1);
    for r in rows(instances, palette) {
        let expansion = palette.expansion(&r.name).filter(|e| e.iter().any(|v| *v));
        let count = expansion.is_none().then_some(r.count);
        list = list.push(list_row(
            palette,
            &r.name,
            CountKey::default(),
            r.name.clone(),
            count,
            r.errors > 0,
            0,
            Some(expansion.is_some()),
        ));
        if let Some([l, s, m]) = expansion {
            for (key, n) in codec::count::expanded_counts(instances, &r.name, l, s, m) {
                let label = key.label();
                list = list.push(list_row(palette, &r.name, key, label, Some(n), false, 1, None));
            }
        }
    }
    let footer: Element<'a, Message> = match &palette.table {
        None => row![
            Space::new().width(Fill),
            button(text(crate::t!("Create Table")).size(12))
                .on_press(msg(CountMsg::CreateTable))
                .style(button_style(false))
                .padding([5, 12]),
        ]
        .into(),
        Some(checked) => {
            let all = rows(instances, palette);
            let every = !all.is_empty() && all.iter().all(|r| checked.contains(&r.name.to_ascii_uppercase()));
            let mut insert = button(text(crate::t!("Insert")).size(12)).style(button_style(true)).padding([5, 12]);
            if !checked.is_empty() {
                insert = insert.on_press(msg(CountMsg::TableInsert));
            }
            row![
                checkbox(every).size(14).on_toggle(|v| msg(CountMsg::TableAll(v))),
                Space::new().width(Fill),
                button(text(crate::t!("Cancel")).size(12))
                    .on_press(msg(CountMsg::TableCancel))
                    .style(button_style(false))
                    .padding([5, 12]),
                insert,
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into()
        }
    };
    column![
        top,
        container(column![header, scrollable(list).height(Fill)].spacing(2))
            .padding(4)
            .height(Fill)
            .style(card),
        footer,
    ]
    .spacing(6)
    .into()
}

fn card(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border { radius: 4.0.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Count mode in the palette: back to the list, the counted block with the
/// match options, and the error report.
fn mode_view<'a>(palette: &'a CountPalette, mode: &'a CountMode, doc: &CadDocument) -> Element<'a, Message> {
    let back = button(row![crate::ui::icons::themed_arrow_left(10.0), text(crate::t!("Back to List")).size(12)].spacing(4).align_y(iced::Center))
        .on_press(msg(CountMsg::Back))
        .style(button::text)
        .padding([2, 2]);
    let result = mode.result(doc);
    let mut body = column![back].spacing(8);
    if let Some(name) = mode.target_name() {
        let mut details = column![button(
            row![
                crate::ui::icons::themed_success(CHECK_ICON, 12.0),
                text(match mode.target {
                    Some(CountTarget::Group(_)) => crate::tf!("Geometries: {}", mode.count(doc)).into_owned(),
                    _ => format!("{name}: {}", mode.count(doc)),
                })
                .size(12)
                .width(Fill),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .on_press(msg(CountMsg::ToggleDetails))
        .style(button::text)
        .padding(0)]
        .spacing(6);
        if let (false, Some(CountTarget::Block { matching, .. })) = (palette.details_closed, &mode.target) {
            for (k, label) in ["Match Layer", "Match Scale", "Match Mirror State"].iter().enumerate() {
                details = details.push(
                    checkbox(matching[k])
                        .label(crate::t!(*label).into_owned())
                        .text_size(12)
                        .size(14)
                        .on_toggle(move |v| msg(CountMsg::Match(k, v))),
                );
            }
        }
        body = body.push(container(details).padding(8).width(Fill).style(card));
    }
    if !result.errors.is_empty() {
        let mut report = column![button(
            row![
                crate::ui::icons::themed_warning(WARNING_ICON, 12.0),
                text(crate::tf!("Count Error Report: {}", result.errors.len())).size(12).width(Fill),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .on_press(msg(CountMsg::ToggleErrors))
        .style(button::text)
        .padding(0)]
        .spacing(4);
        if !palette.errors_closed {
            for h in &result.errors {
                report = report.push(
                    button(text(crate::t!("Overlapping object")).size(12))
                        .on_press(msg(CountMsg::ShowError(*h)))
                        .style(button::text)
                        .padding([2, 18]),
                );
            }
        }
        body = body.push(container(report).padding(8).width(Fill).style(card));
    }
    body.into()
}

pub fn view<'a>(
    palette: &'a CountPalette,
    mode: Option<&'a CountMode>,
    doc: &'a CadDocument,
    doc_id: u64,
    epoch: u64,
    width: f32,
    auto_collapse: bool,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(PanelId::Count, crate::t!("Count").into_owned(), auto_collapse);
    let body = match mode {
        Some(mode) if mode.target.is_some() => mode_view(palette, mode, doc),
        _ => {
            let area = mode.and_then(|m| m.area.clone());
            let key = (doc_id, epoch);
            let mut cache = palette.list_cache.borrow_mut();
            let instances = match cache.as_ref() {
                Some((d, e, a, list)) if (*d, *e) == key && *a == area => list.clone(),
                _ => {
                    let list = std::rc::Rc::new(codec::count::block_instances(doc, area.as_deref()));
                    *cache = Some((key.0, key.1, area, list.clone()));
                    list
                }
            };
            list_view(palette, &instances)
        }
    };
    crate::ui::dock::frame(column![title_bar, body].spacing(6), width)
}

fn bar_button<'a>(icon: Element<'a, Message>, tip: String, m: Option<CountMsg>) -> Element<'a, Message> {
    let mut b = button(icon).style(button::text).padding([3, 5]);
    if let Some(m) = m {
        b = b.on_press(msg(m));
    }
    tooltip(b, text(tip).size(10), tooltip::Position::Bottom).gap(4).into()
}

/// The floating toolbar over the drawing in count mode: the count, the
/// status, previous / next reference, area, select, field and close.
pub fn toolbar<'a>(mode: &CountMode, doc: &CadDocument) -> Element<'a, Message> {
    let has_target = mode.target.is_some();
    let result = mode.result(doc);
    let count = if has_target { mode.count(doc).to_string() } else { "-".to_string() };
    let status: Element<'a, Message> = if !result.errors.is_empty() {
        crate::ui::icons::themed_warning(WARNING_ICON, 14.0)
    } else if has_target {
        crate::ui::icons::themed_primary(crate::ui::icons::INFO, 14.0)
    } else {
        crate::ui::icons::themed_disabled(WARNING_ICON, 14.0)
    };
    let on = |m: CountMsg| has_target.then_some(m);
    let icon = |bytes: &'static [u8]| -> Element<'a, Message> {
        if has_target {
            crate::ui::icons::themed_success(bytes, 16.0)
        } else {
            crate::ui::icons::themed_disabled(bytes, 16.0)
        }
    };
    let bar = row![
        text(crate::tf!("Count: {}", count)).size(13),
        status,
        bar_button(crate::ui::icons::themed_arrow_left(12.0), crate::t!("Previous").into_owned(), on(CountMsg::Prev)),
        bar_button(crate::ui::icons::themed_arrow_right(12.0), crate::t!("Next").into_owned(), on(CountMsg::Next)),
        bar_button(crate::ui::icons::themed_success(AREA_ICON, 16.0), crate::t!("Count in a specified area").into_owned(), Some(CountMsg::Area)),
        bar_button(crate::ui::icons::themed_success(SELECT_ICON, 16.0), crate::t!("Select objects").into_owned(), Some(CountMsg::Select)),
        bar_button(icon(TABLE_ICON), crate::t!("Insert Count Field").into_owned(), on(CountMsg::Field)),
        bar_button(crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0), crate::t!("Close").into_owned(), Some(CountMsg::Close)),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let bar = container(bar).padding([4, 10]).style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border { color: theme.palette().background.neutral.color, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    });
    container(iced::widget::opaque(bar)).width(Fill).align_x(iced::Center).padding([10, 0]).into()
}

/// The blue frame around the drawing area in count mode.
pub fn border<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(Fill))
        .width(Fill)
        .height(Fill)
        .style(|_: &Theme| container::Style {
            border: Border { color: Color::from_rgb(0.18, 0.52, 0.95), width: 2.0, radius: 0.0.into() },
            ..Default::default()
        })
        .into()
}

/// A colour index as the display colour (`COUNTCOLOR` / `COUNTERRORCOLOR`).
pub fn aci_rgba(index: i16) -> [f32; 4] {
    crate::scene::convert::tess_util::aci_to_rgba(&codec::types::Color::from_index(index))
}


/// The Invalid Area dialog, shown when the count area's boundary is gone.
pub fn invalid_area_view<'a>(palette: &CountPalette, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let action = |title: String, detail: String, m: CountMsg| -> Element<'a, Message> {
        button(
            row![
                crate::ui::icons::themed_primary(crate::ui::icons::ARROW_LONG_RIGHT, 16.0),
                column![text(title).size(13), text(detail).size(11).style(muted_style)].spacing(2),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .on_press(msg(m))
        .width(Fill)
        .padding([10, 12])
        .style(button_style(false))
        .into()
    };
    column![
        row![
            crate::ui::icons::themed_warning(WARNING_ICON, 20.0),
            text(crate::t!("The changes to count boundary have resulted to an invalid count area. What do you want to do?"))
                .size(14)
                .width(Fill),
        ]
        .spacing(12)
        .align_y(iced::Center),
        action(
            crate::t!("Undo the changes to the count boundary").into_owned(),
            crate::t!("The last change is undone; the boundary and the count area come back.").into_owned(),
            CountMsg::InvalidUndo,
        ),
        action(
            crate::t!("Continue and count the entire model space").into_owned(),
            crate::t!("The count area is dropped; counting goes on in all of model space.").into_owned(),
            CountMsg::InvalidContinue,
        ),
        row![
            checkbox(palette.invalid_always)
                .label(crate::t!("Always perform my current choice").into_owned())
                .text_size(12)
                .size(14)
                .on_toggle(|v| msg(CountMsg::InvalidAlways(v))),
            Space::new().width(Fill),
            crate::ui::style::form::dialog_button(crate::t!("Cancel"), msg(CountMsg::InvalidUndo), false),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}
