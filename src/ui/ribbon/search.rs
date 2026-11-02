//! The command search box: finds a command among the quick-access buttons,
//! the application menu, the menu bar and the ribbon, and runs it the way its
//! button or menu item would.

use std::time::Duration;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::time::Instant;
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, stack, text, text_input,
    tooltip, Space,
};
use iced::{
    Background, Border, Color, Element, Event, Fill, Font, Length, Padding, Rectangle, Renderer,
    Size, Theme, Vector,
};

use super::widgets::{make_icon, make_tip, tip_style};
use super::{app_menu, draw_panel, extras, Ribbon, QUICK_ACCESS};
use crate::app::Message;
use crate::modules::{IconKind, ModuleEvent, RibbonItem, ToolDef};
use crate::ui::icons;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::{recent_bounds, PosReport};

/// The search field, in the box or in the narrow window's overlay.
pub const SEARCH_INPUT_ID: &str = "ribbon_search_input";
/// The box, or the magnifier button that stands in for it.
const BOX_ID: &str = "ribbon_search_box";
const RESULTS_ID: &str = "ribbon_search_results";
const RESULTS_SCROLL_ID: &str = "ribbon_search_scroll";
/// The full box, its padding and the gap between its parts.
const BOX_W: f32 = 190.0;
const BOX_H: f32 = 20.0;
const BOX_PAD_LEFT: f32 = 6.0;
/// The last button meets the box's border, so its hover fills to the edge.
const BOX_PAD_RIGHT: f32 = 1.0;
const BOX_GAP: f32 = 5.0;
/// The clear, collapse and keep-expanded buttons in the box.
const BOX_BUTTON: f32 = 18.0;
/// The magnifier button that stands for the box.
const MAGNIFIER_W: f32 = 24.0;
const MAGNIFIER_H: f32 = 20.0;
/// Size of the typed text and the placeholder.
const INPUT_SIZE: f32 = 11.5;
/// The field over the magnifier button on a narrow window, and how far above
/// the button it stands.
const FIELD_W: f32 = 300.0;
const FIELD_H: f32 = 24.0;
const FIELD_ABOVE: f32 = 2.0;
/// How long a temporary box stays after a press elsewhere, so a press on its
/// own buttons or results still lands.
const BLUR_DELAY: Duration = Duration::from_millis(200);

// The search box's icons on the 24-unit grid.
const MAGNIFIER: &str = "M4 11a7 7 0 1 0 14 0a7 7 0 1 0-14 0 M20 20l-4-4";
const COLLAPSE: &str = "M7 6l6 6-6 6 M13 6l6 6-6 6";
const KEEP: &str = "M17 6l-6 6 6 6 M11 6l-6 6 6 6";
const CLEAR: &str = "M5.5 5.5l13 13 M18.5 5.5l-13 13";
const RESULTS_W: f32 = 440.0;
const ROW_H: f32 = 26.0;
const HEADER_H: f32 = 20.0;
/// Below this window width the box shrinks to its magnifier button.
pub(super) const NARROW: f32 = 1080.0;
const BEST: usize = 5;
const PER_GROUP: usize = 8;
/// A picked ribbon button blinks: on and off `FLASHES` times this long.
const FLASH: Duration = Duration::from_millis(110);
const FLASHES: u32 = 6;

/// Where a result comes from; also the result groups in display order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Best,
    QuickAccess,
    AppMenu,
    Ribbon,
    MenuBar,
}

impl Group {
    fn title(self) -> String {
        match self {
            Group::Best => crate::tr!("ribbon-menu", "best-matches"),
            Group::QuickAccess => crate::tr!("ribbon-menu", "quick-access-toolbar"),
            Group::AppMenu => crate::tr!("ribbon-menu", "application-menu"),
            Group::Ribbon => crate::tr!("ribbon-menu", "ribbon"),
            Group::MenuBar => crate::tr!("ribbon-menu", "menu-bar"),
        }
    }
}

/// One thing the search can find.
#[derive(Clone)]
struct Hit {
    group: Group,
    label: String,
    path: String,
    /// The command it runs, if it runs one.
    command: Option<String>,
    icon: Option<IconKind>,
    run: Message,
    /// A ribbon button: its tab and the id its highlight is keyed on.
    place: Option<(usize, &'static str)>,
}

impl Hit {
    /// How well it matches the lower-case `query`: 0 exactly, 1 by its
    /// start, 2 by the start of a word of its label, 3 anywhere in it.
    fn rank(&self, query: &str) -> Option<u8> {
        // "Layer..." matches "layer" exactly.
        let label = self.label.trim_end_matches('.').to_lowercase();
        let command = self.command.as_deref().map(str::to_lowercase);
        let command = command.as_deref();
        if label == query || command == Some(query) {
            Some(0)
        } else if label.starts_with(query) || command.is_some_and(|c| c.starts_with(query)) {
            Some(1)
        } else if label
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word.starts_with(query))
        {
            Some(2)
        } else if label.contains(query) {
            Some(3)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub enum SearchMsg {
    Input(String),
    Clear,
    /// ↓ (`true`) or ↑ through the results.
    Move(bool),
    Submit,
    Pick(usize),
    /// Shrink the box to its magnifier button (`true`), or keep it expanded.
    Collapse(bool),
    /// Open the box for now, from the magnifier button.
    OpenField,
    /// A press inside the box (`true`) or anywhere else.
    Press(bool),
    Scrolled(f32),
}

#[derive(Default)]
pub(super) struct SearchState {
    query: String,
    hits: Vec<Hit>,
    /// The result groups on show: indices into `hits`.
    shown: Vec<(Group, Vec<usize>)>,
    /// The highlighted row, counted over every group.
    selected: usize,
    /// The results were put away by a press elsewhere; typing brings them back.
    hidden: bool,
    /// The box is open for now: from the magnifier button while collapsed,
    /// or as the field over it on a narrow window.
    temp_open: bool,
    /// When a temporary box closes after a press elsewhere.
    closing: Option<Instant>,
    scroll: f32,
    /// The ribbon button blinking after a pick, and when it started.
    flash: Option<(&'static str, Instant)>,
    /// What the ribbon lights while the button blinks.
    lit: Option<String>,
}

/// What the app does after a search message.
#[derive(Default)]
pub struct SearchOutcome {
    /// The layout changed and should be saved.
    pub save: bool,
    /// The picked result's message.
    pub run: Option<Message>,
    /// Focus the search field.
    pub focus: bool,
    /// Scroll the results to this offset.
    pub scroll_to: Option<f32>,
}

impl Ribbon {
    /// Every command the search can find, grouped as the results show them.
    fn search_index(&self) -> Vec<Hit> {
        let mut hits = Vec::new();
        let quick_access = crate::tr!("ribbon-menu", "quick-access-toolbar");
        for &(icon, label, command) in &QUICK_ACCESS {
            hits.push(Hit {
                group: Group::QuickAccess,
                label: crate::t!(label).into_owned(),
                path: quick_access.clone(),
                command: Some(command.to_string()),
                icon: Some(IconKind::Svg(icon)),
                run: Message::Command(command.to_string()),
                place: None,
            });
        }
        for (label, command, run) in [
            ("Undo", "UNDO", Message::Undo),
            ("Redo", "REDO", Message::Redo),
        ] {
            hits.push(Hit {
                group: Group::QuickAccess,
                label: crate::t!(label).into_owned(),
                path: quick_access.clone(),
                command: Some(command.to_string()),
                icon: crate::modules::registry::command_icon(command).map(IconKind::Svg),
                run,
                place: None,
            });
        }
        let app_menu = crate::tr!("ribbon-menu", "application-menu");
        for (icon, label, _, command) in app_menu::commands() {
            hits.push(Hit {
                group: Group::AppMenu,
                label,
                path: app_menu.clone(),
                command: Some(command.to_string()),
                icon: Some(IconKind::Svg(icon)),
                run: Message::Command(command.to_string()),
                place: None,
            });
        }
        let menu_bar = crate::tr!("ribbon-menu", "menu-bar");
        for entry in crate::ui::menu_bar::search_entries() {
            hits.push(Hit {
                group: Group::MenuBar,
                label: entry.label,
                path: format!("{menu_bar}: {}", entry.path),
                command: entry.line.split_whitespace().next().map(str::to_string),
                icon: entry.icon.map(IconKind::Svg),
                run: Message::MenuCommand(entry.line.to_string()),
                place: None,
            });
        }
        // The tab on show first, so a button on two tabs is found where the
        // user already is.
        let ribbon_tab = crate::tr!("ribbon-menu", "ribbon-tab");
        let mut seen = std::collections::HashSet::new();
        let tabs = std::iter::once(self.active)
            .chain((0..self.modules.len()).filter(|i| *i != self.active));
        for index in tabs {
            let module = &self.modules[index];
            if module.id() == "layout"
                || (module.id() == "block_editor" && self.block_editing.is_none())
                || self.tab_hidden(index)
            {
                continue;
            }
            let tab = crate::i18n::ribbon_module_title(module.id(), module.title());
            for group in self.groups_for(index) {
                let path = format!("{ribbon_tab}: {tab} › {}", crate::t!(group.title));
                // Label, command line, icon, message and highlight id of each
                // button of the panel.
                let mut found: Vec<(
                    String,
                    Option<&str>,
                    Option<IconKind>,
                    Message,
                    &'static str,
                )> = Vec::new();
                for item in &group.tools {
                    match item {
                        RibbonItem::Tool(tool)
                        | RibbonItem::LabeledTool(tool)
                        | RibbonItem::LargeTool(tool)
                        | RibbonItem::PropertiesGroup { match_prop: tool } => {
                            found.push(tool_found(tool));
                        }
                        RibbonItem::ToolGrid { columns }
                        | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                            found.extend(columns.iter().flatten().map(tool_found));
                        }
                        RibbonItem::LayerComboGroup { row2, row3 } => {
                            found.extend(row2.iter().chain(row3).map(tool_found));
                        }
                        RibbonItem::Dropdown { id, items, .. }
                        | RibbonItem::LabeledDropdown { id, items, .. }
                        | RibbonItem::LargeDropdown { id, items, .. } => {
                            found.extend(items.iter().map(|&(command, label, ref icon)| {
                                (
                                    crate::t!(label).into_owned(),
                                    Some(command),
                                    Some(icon.clone()),
                                    Message::DropdownSelectItem {
                                        dropdown_id: id,
                                        cmd: command,
                                    },
                                    command,
                                )
                            }));
                        }
                    }
                }
                let key = extras::panel_key(module.id(), group.title);
                if let Some(panel) = draw_panel::panel_for_group(&key) {
                    for more in panel.tools() {
                        let command = more.command();
                        let label = crate::t!(more.tool.label).into_owned();
                        found.push((
                            label.clone(),
                            Some(command),
                            Some(more.tool.icon.clone()),
                            Message::DropdownSelectItem {
                                dropdown_id: panel.id,
                                cmd: command,
                            },
                            command,
                        ));
                        for &(option, option_label) in more.options {
                            found.push((
                                format!("{label} › {}", crate::t!(option_label)),
                                Some(option),
                                Some(more.tool.icon.clone()),
                                Message::DropdownSelectItem {
                                    dropdown_id: panel.id,
                                    cmd: option,
                                },
                                option,
                            ));
                        }
                    }
                    if let Some(dialog) = panel.dialog {
                        found.push((
                            crate::t!(panel.dialog_label).into_owned(),
                            Some(dialog),
                            crate::modules::registry::command_icon(dialog).map(IconKind::Svg),
                            draw_panel::launcher_message(dialog),
                            dialog,
                        ));
                    }
                }
                for (label, command, icon, run, id) in found {
                    let label = label.replace('\n', " ");
                    if label.is_empty()
                        || !seen.insert((label.clone(), command.map(str::to_string)))
                    {
                        continue;
                    }
                    hits.push(Hit {
                        group: Group::Ribbon,
                        label,
                        path: path.clone(),
                        command: command
                            .and_then(|line| line.split_whitespace().next())
                            .map(str::to_string),
                        icon,
                        run,
                        place: Some((index, id)),
                    });
                }
            }
        }
        hits
    }

    /// Match the query against a fresh index.
    fn run_search(&mut self) {
        self.search.selected = 0;
        self.search.scroll = 0.0;
        self.search.shown.clear();
        let query = self.search.query.trim().to_lowercase();
        if query.is_empty() {
            self.search.hits.clear();
            return;
        }
        let hits = self.search_index();
        let ranked: Vec<(usize, u8)> = hits
            .iter()
            .enumerate()
            .filter_map(|(i, hit)| hit.rank(&query).map(|rank| (i, rank)))
            .collect();
        // Best matches: what starts with the query, a ribbon button before
        // the same command elsewhere, as that one shows where it lives.
        let preference = |group: Group| match group {
            Group::Ribbon => 0,
            Group::QuickAccess => 1,
            Group::AppMenu => 2,
            _ => 3,
        };
        let mut best: Vec<(usize, u8)> = ranked.iter().copied().filter(|&(_, r)| r <= 1).collect();
        best.sort_by_key(|&(i, rank)| (rank, preference(hits[i].group)));
        let mut commands = std::collections::HashSet::new();
        let best: Vec<usize> = best
            .into_iter()
            .map(|(i, _)| i)
            .filter(|&i| {
                let hit = &hits[i];
                commands.insert(hit.command.clone().unwrap_or_else(|| hit.label.clone()))
            })
            .take(BEST)
            .collect();
        if !best.is_empty() {
            self.search.shown.push((Group::Best, best.clone()));
        }
        for group in [
            Group::QuickAccess,
            Group::AppMenu,
            Group::Ribbon,
            Group::MenuBar,
        ] {
            let mut members: Vec<(usize, u8)> = ranked
                .iter()
                .copied()
                .filter(|(i, _)| hits[*i].group == group && !best.contains(i))
                .collect();
            members.sort_by_key(|&(_, rank)| rank);
            let members: Vec<usize> = members
                .into_iter()
                .map(|(i, _)| i)
                .take(PER_GROUP)
                .collect();
            if !members.is_empty() {
                self.search.shown.push((group, members));
            }
        }
        self.search.hits = hits;
    }

    /// The results in display order.
    fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.search
            .shown
            .iter()
            .flat_map(|(_, members)| members.iter().copied())
    }

    /// Whether the results are on show.
    pub fn search_results_open(&self) -> bool {
        !self.search.query.trim().is_empty() && !self.search.hidden
    }

    /// Empty the box and close its results.
    fn clear_query(&mut self) {
        self.search.query.clear();
        self.search.hits.clear();
        self.search.shown.clear();
        self.search.hidden = false;
    }

    /// Empty the box and close its results and a temporary box.
    fn clear_search(&mut self) {
        self.clear_query();
        self.search.temp_open = false;
        self.search.closing = None;
    }

    /// Escape: clear the text, or with none close a temporary box; whether
    /// there was something to do.
    pub fn escape_search(&mut self) -> bool {
        if !self.search.query.is_empty() {
            self.clear_query();
            true
        } else if self.search.temp_open {
            self.clear_search();
            true
        } else {
            false
        }
    }

    /// Whether the button `id` on tab `tab` shows on the ribbon: the ribbon
    /// is not minimized and the panel holding it is not collapsed.
    fn button_shows(&self, tab: usize, id: &str) -> bool {
        if self.layout.minimize != super::RibbonMinimize::Off {
            return false;
        }
        let Some(module) = self.modules.get(tab) else {
            return false;
        };
        !self.groups_for(tab).any(|group| {
            super::locate::group_holds(&extras::panel_key(module.id(), group.title), group, id)
                && recent_bounds(&super::minimize::opener_id(group.title)).is_some()
        })
    }

    /// The window changed width: turning narrow or wide closes a temporary
    /// box and empties the search, whose results would otherwise hang from
    /// a box that went; the kept-open preference stays as it is.
    pub(super) fn search_resized(&mut self, was_narrow: bool) {
        if self.narrow() != was_narrow {
            self.clear_search();
        }
    }

    /// Apply a search message.
    pub fn on_search(&mut self, msg: SearchMsg, now: Instant) -> SearchOutcome {
        let mut out = SearchOutcome::default();
        match msg {
            SearchMsg::Input(query) => {
                self.search.query = query;
                self.search.hidden = false;
                self.run_search();
                out.scroll_to = Some(0.0);
            }
            SearchMsg::Clear => {
                self.clear_query();
                out.focus = true;
            }
            SearchMsg::Move(down) => {
                let count = self.rows().count();
                if count == 0 || !self.search_results_open() {
                    return out;
                }
                self.search.selected = if down {
                    (self.search.selected + 1) % count
                } else {
                    (self.search.selected + count - 1) % count
                };
                out.scroll_to = self.scroll_for_selected();
            }
            SearchMsg::Submit => {
                let picked = self.rows().nth(self.search.selected);
                if let Some(hit) = picked {
                    return self.on_search(SearchMsg::Pick(hit), now);
                }
            }
            SearchMsg::Pick(index) => {
                let Some(hit) = self.search.hits.get(index).cloned() else {
                    return out;
                };
                self.clear_search();
                if let Some((tab, id)) = hit.place {
                    self.select(tab);
                    // The button blinks only where it shows: not on a
                    // minimized ribbon or in a collapsed panel.
                    if self.button_shows(tab, id) {
                        self.search.flash = Some((id, now));
                        self.search.lit = Some(id.to_string());
                    }
                }
                out.run = Some(hit.run);
            }
            SearchMsg::Collapse(collapsed) => {
                out.save = self.layout.search_expanded == collapsed;
                self.layout.search_expanded = !collapsed;
                if collapsed {
                    self.clear_search();
                } else {
                    // Kept open: the text stays and the field keeps focus.
                    self.search.temp_open = false;
                    self.search.closing = None;
                    out.focus = true;
                }
            }
            SearchMsg::OpenField => {
                self.search.temp_open = true;
                self.search.closing = None;
                self.search.hidden = false;
                out.focus = true;
            }
            SearchMsg::Press(inside) => {
                if inside {
                    self.search.hidden = false;
                    self.search.closing = None;
                } else {
                    self.search.hidden = true;
                    // A temporary box closes once the press is done with.
                    if self.search.temp_open {
                        self.search.closing = Some(now + BLUR_DELAY);
                    }
                }
            }
            SearchMsg::Scrolled(offset) => self.search.scroll = offset,
        }
        out
    }

    /// The scroll offset that brings the selected row into view, if it is
    /// out of it.
    fn scroll_for_selected(&self) -> Option<f32> {
        let mut y = 0.0;
        let mut row = 0;
        for (_, members) in &self.search.shown {
            y += HEADER_H;
            for _ in members {
                if row == self.search.selected {
                    let view = self.results_h();
                    return if y < self.search.scroll {
                        Some(y - HEADER_H.min(y))
                    } else if y + ROW_H > self.search.scroll + view {
                        Some(y + ROW_H - view)
                    } else {
                        None
                    };
                }
                y += ROW_H;
                row += 1;
            }
        }
        None
    }

    fn content_h(&self) -> f32 {
        self.search
            .shown
            .iter()
            .map(|(_, members)| HEADER_H + members.len() as f32 * ROW_H)
            .sum::<f32>()
            .max(ROW_H)
    }

    /// The height of the results list: all of it, or what fits the window.
    fn results_h(&self) -> f32 {
        let top = self.search_anchor().map_or(0.0, |b| b.y + b.height + 2.0);
        self.content_h()
            .min((self.win_h - top - 40.0).max(ROW_H * 4.0))
    }

    /// The ribbon button blinking after a pick: blink on, then off, until done.
    pub(super) fn search_tick(&mut self, now: Instant) {
        if self.search.closing.is_some_and(|at| now >= at) {
            self.clear_search();
        }
        let Some((id, at)) = self.search.flash else {
            return;
        };
        let phase = (now.saturating_duration_since(at).as_millis() / FLASH.as_millis()) as u32;
        if phase >= FLASHES {
            self.search.flash = None;
            self.search.lit = None;
        } else {
            self.search.lit = (phase % 2 == 0).then(|| id.to_string());
        }
    }

    /// Whether a button blinks or a temporary box is due to close.
    pub(super) fn search_flashing(&self) -> bool {
        self.search.flash.is_some() || self.search.closing.is_some()
    }

    /// The button the ribbon lights: the running command's, or the picked
    /// one while it blinks.
    pub(super) fn lit_tool(&self) -> &Option<String> {
        if self.search.flash.is_some() {
            &self.search.lit
        } else {
            &self.active_tool
        }
    }

    /// A window too narrow for the box: only its magnifier button shows.
    fn narrow(&self) -> bool {
        self.win_w < NARROW
    }

    /// Whether the full box shows in the tab row: kept open, or open for now.
    fn shows_box(&self) -> bool {
        !self.narrow() && (self.layout.search_expanded || self.search.temp_open)
    }

    /// The search box, or its magnifier button while it is collapsed or the
    /// window is narrow.
    pub fn search_box(&self) -> Element<'_, Message> {
        if !self.shows_box() {
            let narrow = self.narrow();
            let open = self.search.temp_open;
            let magnifier = button(
                container(icons::line_icon(MAGNIFIER, metrics::SMALL_ICON))
                    .center_x(Fill)
                    .center_y(Fill),
            )
            .on_press(Message::RibbonSearch(SearchMsg::OpenField))
            .width(MAGNIFIER_W)
            .height(MAGNIFIER_H)
            .padding(0)
            .style(move |theme: &Theme, status| {
                let lit =
                    open || matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: lit.then(|| Background::Color(chrome::hover(theme))),
                    text_color: chrome::title_text(theme),
                    border: Border {
                        color: if lit {
                            chrome::hover_border(theme)
                        } else {
                            Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }
            });
            // A right-click keeps the box expanded on a wide window.
            let (magnifier, tip): (Element<'_, Message>, _) = if narrow {
                (
                    magnifier.into(),
                    crate::tr!("ribbon-menu", "search-commands"),
                )
            } else {
                (
                    mouse_area(magnifier)
                        .on_right_press(Message::RibbonSearch(SearchMsg::Collapse(false)))
                        .into(),
                    crate::tr!("ribbon-menu", "search-commands-keep"),
                )
            };
            let magnifier = tooltip(magnifier, make_tip(tip), tooltip::Position::Bottom)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style);
            return PosReport::new(BOX_ID, magnifier).into();
        }
        let end = if self.layout.search_expanded {
            End::Collapse
        } else {
            End::Keep
        };
        let field = self.search_field(BOX_W, BOX_H, end);
        PosReport::new(BOX_ID, self.watch_presses(field)).into()
    }

    /// Put the results away on a press elsewhere, and bring them back on a
    /// press in the field.
    fn watch_presses<'a>(&self, field: Element<'a, Message>) -> Element<'a, Message> {
        let active = !self.search.query.is_empty() || self.search.temp_open;
        PressWatch {
            child: field,
            on_inside: (active && self.search.hidden)
                .then_some(Message::RibbonSearch(SearchMsg::Press(true))),
            on_outside: (active && !self.search.hidden)
                .then_some(Message::RibbonSearch(SearchMsg::Press(false))),
        }
        .into()
    }

    /// The search field: magnifier, input with its placeholder, clear button
    /// while there is text, and at the end the button `end` names. The field
    /// over the magnifier button on a narrow window has an accent border.
    fn search_field(&self, width: f32, height: f32, end: End) -> Element<'_, Message> {
        let query = &self.search.query;
        let input = text_input("", query)
            .id(SEARCH_INPUT_ID)
            .on_input(|query| Message::RibbonSearch(SearchMsg::Input(query)))
            .on_submit(Message::RibbonSearch(SearchMsg::Submit))
            .size(INPUT_SIZE)
            .padding(0)
            .width(Fill)
            .style(|theme: &Theme, _| text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: chrome::muted(theme),
                placeholder: chrome::muted(theme),
                value: chrome::strong(theme),
                selection: chrome::accent(theme).scale_alpha(0.4),
            });
        // The placeholder is italic, which the input's own is not. The stack
        // stays when there is a query, so the input keeps its focus.
        let placeholder = if query.is_empty() {
            crate::tr!("ribbon-menu", "search-placeholder")
        } else {
            String::new()
        };
        let input: Element<'_, Message> = stack![
            container(
                text(placeholder)
                    .size(INPUT_SIZE)
                    .font(Font {
                        style: iced::font::Style::Italic,
                        ..Font::DEFAULT
                    })
                    .style(|theme: &Theme| text::Style {
                        color: Some(chrome::muted(theme)),
                    }),
            )
            .width(Fill)
            .center_y(Fill),
            container(input).center_y(Fill),
        ]
        .into();
        let magnifier =
            container(icons::line_icon(MAGNIFIER, metrics::STRIP_ICON)).style(|theme: &Theme| {
                container::Style {
                    text_color: Some(chrome::title_text(theme)),
                    ..Default::default()
                }
            });
        let mut content = row![magnifier, input]
            .spacing(BOX_GAP)
            .align_y(iced::Center);
        if !query.is_empty() {
            content = content.push(box_button(
                icons::line_icon(CLEAR, metrics::SMALL_ICON),
                SearchMsg::Clear,
                chrome::danger,
                chrome::danger,
            ));
        }
        let end_button = match end {
            End::Collapse => Some((COLLAPSE, true, crate::tr!("ribbon-menu", "collapse-search"))),
            End::Keep => Some((KEEP, false, crate::tr!("ribbon-menu", "keep-search"))),
            End::None => None,
        };
        if let Some((path, collapse, tip)) = end_button {
            content = content.push(
                tooltip(
                    box_button(
                        icons::line_icon(path, metrics::STRIP_ICON),
                        SearchMsg::Collapse(collapse),
                        chrome::muted,
                        chrome::strong,
                    ),
                    make_tip(tip),
                    tooltip::Position::Bottom,
                )
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style),
            );
        }
        let overlay = end == End::None;
        container(content)
            .width(width)
            .height(height)
            .padding(Padding {
                top: 0.0,
                right: BOX_PAD_RIGHT,
                bottom: 0.0,
                left: BOX_PAD_LEFT,
            })
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::page(theme))),
                border: Border {
                    color: if overlay {
                        chrome::accent(theme)
                    } else {
                        chrome::border(theme)
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Where the results hang from: the box, or the field over the
    /// magnifier button.
    fn search_anchor(&self) -> Option<Rectangle> {
        let b = recent_bounds(BOX_ID)?;
        Some(if self.narrow() {
            Rectangle {
                x: b.x + b.width - FIELD_W,
                y: b.y - FIELD_ABOVE,
                width: FIELD_W,
                height: FIELD_H,
            }
        } else {
            b
        })
    }

    /// The search layer over the window: the field over the magnifier button
    /// when it is open, and the results under the box or field.
    pub fn search_layer(&self) -> Option<Element<'_, Message>> {
        let field = self.narrow() && self.search.temp_open;
        if !field && !self.search_results_open() {
            return None;
        }
        let anchor = self.search_anchor()?;
        // The results' right edge meets the box's.
        let results_open = self.search_results_open();
        let x = if results_open {
            (anchor.x + anchor.width - RESULTS_W).max(4.0)
        } else {
            anchor.x
        };
        let mut layer = column![];
        if field {
            let field = self.watch_presses(self.search_field(FIELD_W, FIELD_H, End::None));
            layer = layer.push(container(field).padding(Padding {
                left: anchor.x - x,
                ..Padding::ZERO
            }));
        }
        if results_open {
            let results = PosReport::new(RESULTS_ID, self.search_results());
            layer = layer.push(container(results).padding(Padding {
                top: 2.0,
                ..Padding::ZERO
            }));
        }
        let top = if field {
            anchor.y
        } else {
            anchor.y + anchor.height
        };
        Some(
            container(layer)
                .padding(Padding {
                    top,
                    left: x,
                    ..Padding::ZERO
                })
                .width(Fill)
                .height(Fill)
                .into(),
        )
    }

    /// The results: each group's title, then its rows (icon, label, path).
    fn search_results(&self) -> Element<'_, Message> {
        let mut list = column![];
        let mut row_index = 0;
        for (group, members) in &self.search.shown {
            list = list.push(
                container(
                    text(group.title())
                        .size(10.5)
                        .style(|theme: &Theme| text::Style {
                            color: Some(chrome::muted(theme)),
                        }),
                )
                .height(HEADER_H)
                .padding([0, 10])
                .center_y(HEADER_H),
            );
            for &hit_index in members {
                list = list.push(self.result_row(hit_index, row_index == self.search.selected));
                row_index += 1;
            }
        }
        if self.search.shown.is_empty() {
            list = list.push(
                container(
                    text(crate::tr!("ribbon-menu", "no-matches"))
                        .size(12)
                        .style(|theme: &Theme| text::Style {
                            color: Some(chrome::muted(theme)),
                        }),
                )
                .height(ROW_H)
                .padding([0, 10])
                .center_y(ROW_H),
            );
        }
        let list = scrollable(list.padding([2, 0]))
            .id(RESULTS_SCROLL_ID)
            .on_scroll(|viewport| {
                Message::RibbonSearch(SearchMsg::Scrolled(viewport.absolute_offset().y))
            })
            .height(Length::Fixed(self.results_h() + 4.0));
        opaque(
            container(list)
                .width(RESULTS_W)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::panel(theme))),
                    border: Border {
                        color: chrome::border(theme),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        )
    }

    fn result_row(&self, index: usize, selected: bool) -> Element<'_, Message> {
        let hit = &self.search.hits[index];
        let icon: Element<'_, Message> = match &hit.icon {
            Some(icon) => make_icon(icon.clone(), metrics::SMALL_ICON),
            None => Space::new()
                .width(metrics::SMALL_ICON)
                .height(metrics::SMALL_ICON)
                .into(),
        };
        button(
            row![
                container(icon).width(16).center_x(16),
                text(hit.label.as_str())
                    .size(12)
                    .wrapping(text::Wrapping::None),
                container(
                    text(hit.path.as_str())
                        .size(11)
                        .wrapping(text::Wrapping::None)
                        .font(Font {
                            style: iced::font::Style::Italic,
                            ..Font::DEFAULT
                        })
                        .style(|theme: &Theme| text::Style {
                            color: Some(chrome::muted(theme)),
                        }),
                )
                .width(Fill)
                .clip(true),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .on_press(Message::RibbonSearch(SearchMsg::Pick(index)))
        .width(Fill)
        .height(ROW_H)
        .padding([0, 10])
        .style(move |theme: &Theme, status| {
            let lit =
                selected || matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: lit.then(|| Background::Color(chrome::hover(theme))),
                text_color: if lit {
                    chrome::strong(theme)
                } else {
                    chrome::text(theme)
                },
                ..Default::default()
            }
        })
        .into()
    }
}

/// A ribbon tool as the search finds it.
fn tool_found(
    tool: &ToolDef,
) -> (
    String,
    Option<&str>,
    Option<IconKind>,
    Message,
    &'static str,
) {
    let command = match &tool.event {
        ModuleEvent::Command(command) => Some(command.as_str()),
        _ => None,
    };
    (
        crate::t!(tool.label).into_owned(),
        command,
        Some(tool.icon.clone()),
        Message::RibbonToolClick {
            tool_id: tool.id.to_string(),
            event: tool.event.clone(),
        },
        tool.id,
    )
}

/// The search scroll's id, for the app's scroll operations.
pub fn results_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new(RESULTS_SCROLL_ID)
}

/// The button at the end of the search field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    /// » : collapse the kept-open box to its magnifier button.
    Collapse,
    /// « : keep the temporarily open box expanded.
    Keep,
    /// None: the field over the magnifier button on a narrow window.
    None,
}

/// A button in the search field, with its icon in `colour`, or `hover`
/// under the pointer, on a `panel` background. It sends `msg`; the app then
/// gives the field its focus back.
fn box_button<'a>(
    icon: Element<'a, Message>,
    msg: SearchMsg,
    colour: fn(&Theme) -> Color,
    hover: fn(&Theme) -> Color,
) -> Element<'a, Message> {
    button(container(icon).center_x(Fill).center_y(Fill))
        .on_press(Message::RibbonSearch(msg))
        .width(BOX_BUTTON)
        .height(BOX_BUTTON)
        .padding(0)
        .style(move |theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hovered.then(|| Background::Color(chrome::panel(theme))),
                text_color: if hovered { hover(theme) } else { colour(theme) },
                border: Border {
                    radius: 2.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// Reports a mouse press inside or outside its child without taking it, so
/// the press still does what it does there. A press on the results counts as
/// neither: their rows handle it.
struct PressWatch<'a> {
    child: Element<'a, Message>,
    on_inside: Option<Message>,
    on_outside: Option<Message>,
}

impl<'a> Widget<Message, Theme, Renderer> for PressWatch<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            let on_results = cursor
                .position()
                .zip(recent_bounds(RESULTS_ID))
                .is_some_and(|(at, results)| results.contains(at));
            let message = if cursor.is_over(layout.bounds()) {
                self.on_inside.clone()
            } else if on_results {
                None
            } else {
                self.on_outside.clone()
            };
            if let Some(message) = message {
                shell.publish(message);
            }
        }
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<PressWatch<'a>> for Element<'a, Message> {
    fn from(watch: PressWatch<'a>) -> Self {
        Element::new(watch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(label: &str, command: Option<&str>) -> Hit {
        Hit {
            group: Group::Ribbon,
            label: label.to_string(),
            path: String::new(),
            command: command.map(str::to_string),
            icon: None,
            run: Message::Undo,
            place: None,
        }
    }

    #[test]
    fn a_result_ranks_by_how_it_matches() {
        assert_eq!(hit("Line", Some("LINE")).rank("line"), Some(0));
        assert_eq!(hit("Linetype", Some("LINETYPE")).rank("line"), Some(1));
        assert_eq!(
            hit("Construction Line", Some("XLINE")).rank("line"),
            Some(2)
        );
        assert_eq!(hit("Polyline", Some("PLINE")).rank("line"), Some(3));
        assert_eq!(hit("Circle", Some("CIRCLE")).rank("line"), None);
        // The command counts too.
        assert_eq!(hit("Construction Line", Some("XLINE")).rank("xl"), Some(1));
    }

    #[test]
    fn a_search_groups_its_results_and_puts_the_best_first() {
        let mut ribbon = Ribbon::new();
        let _ = ribbon.on_search(SearchMsg::Input("line".into()), Instant::now());
        let groups: Vec<Group> = ribbon.search.shown.iter().map(|(g, _)| *g).collect();
        assert_eq!(groups.first(), Some(&Group::Best));
        assert!(groups.contains(&Group::Ribbon));
        let (_, best) = &ribbon.search.shown[0];
        assert!(best.len() <= BEST);
        let first = &ribbon.search.hits[best[0]];
        assert_eq!(first.command.as_deref(), Some("LINE"));
        // The ribbon's Line button, which a pick flashes on its tab.
        assert_eq!(first.group, Group::Ribbon);
        assert!(first.path.contains(" › "));
        for (_, members) in &ribbon.search.shown[1..] {
            assert!(members.len() <= PER_GROUP);
            assert!(members.iter().all(|i| !best.contains(i)));
        }
    }

    #[test]
    fn picking_a_ribbon_result_shows_its_tab_and_blinks_its_button() {
        let mut ribbon = Ribbon::new();
        let now = Instant::now();
        let _ = ribbon.on_search(SearchMsg::Input("attach".into()), now);
        // A button on another tab than the one on show.
        let index = ribbon
            .rows()
            .find(|&i| {
                ribbon.search.hits[i]
                    .place
                    .is_some_and(|(tab, _)| tab != ribbon.active)
            })
            .expect("a ribbon result on another tab");
        let (tab, id) = ribbon.search.hits[index].place.expect("a ribbon place");
        let out = ribbon.on_search(SearchMsg::Pick(index), now);
        assert!(out.run.is_some());
        assert_eq!(ribbon.active, tab);
        assert!(ribbon.search.query.is_empty());
        assert_eq!(ribbon.lit_tool().as_deref(), Some(id));
        ribbon.search_tick(now + FLASH);
        assert_eq!(ribbon.lit_tool().as_deref(), None);
        ribbon.search_tick(now + FLASH * FLASHES);
        assert!(!ribbon.search_flashing());
    }

    #[test]
    fn arrows_wrap_and_escape_clears() {
        let mut ribbon = Ribbon::new();
        let now = Instant::now();
        let _ = ribbon.on_search(SearchMsg::Input("line".into()), now);
        let count = ribbon.rows().count();
        assert!(count > 1);
        let _ = ribbon.on_search(SearchMsg::Move(false), now);
        assert_eq!(ribbon.search.selected, count - 1);
        let _ = ribbon.on_search(SearchMsg::Move(true), now);
        assert_eq!(ribbon.search.selected, 0);
        assert!(ribbon.escape_search());
        assert!(!ribbon.search_results_open());
        assert!(!ribbon.escape_search());
    }

    #[test]
    fn the_box_collapses_opens_for_now_and_can_be_kept_expanded() {
        let mut ribbon = Ribbon::new();
        ribbon.set_window_size((1600.0, 900.0));
        let now = Instant::now();
        // Kept open by default; » collapses it and asks to save.
        assert!(ribbon.shows_box());
        assert!(ribbon.on_search(SearchMsg::Collapse(true), now).save);
        assert!(!ribbon.shows_box());

        // The magnifier opens it for now; a press elsewhere closes it a
        // moment later, a press inside in the meantime keeps it.
        let out = ribbon.on_search(SearchMsg::OpenField, now);
        assert!(out.focus && ribbon.shows_box());
        let _ = ribbon.on_search(SearchMsg::Press(false), now);
        let _ = ribbon.on_search(SearchMsg::Press(true), now);
        ribbon.search_tick(now + BLUR_DELAY);
        assert!(ribbon.shows_box());
        let _ = ribbon.on_search(SearchMsg::Press(false), now);
        assert!(ribbon.wants_tick());
        ribbon.search_tick(now + BLUR_DELAY);
        assert!(!ribbon.shows_box());

        // Escape clears the text first, then closes the temporary box.
        let _ = ribbon.on_search(SearchMsg::OpenField, now);
        let _ = ribbon.on_search(SearchMsg::Input("line".into()), now);
        assert!(ribbon.escape_search());
        assert!(ribbon.shows_box());
        assert!(ribbon.escape_search());
        assert!(!ribbon.shows_box());

        // « keeps it open, with its text.
        let _ = ribbon.on_search(SearchMsg::OpenField, now);
        let _ = ribbon.on_search(SearchMsg::Input("line".into()), now);
        let out = ribbon.on_search(SearchMsg::Collapse(false), now);
        assert!(out.save && out.focus);
        assert!(ribbon.layout.search_expanded);
        assert_eq!(ribbon.search.query, "line");
        let _ = ribbon.on_search(SearchMsg::Press(false), now);
        ribbon.search_tick(now + BLUR_DELAY);
        assert!(ribbon.shows_box());
    }

    #[test]
    fn a_narrow_window_shows_the_magnifier_whatever_the_preference() {
        let mut ribbon = Ribbon::new();
        ribbon.set_window_size((1600.0, 900.0));
        let now = Instant::now();
        let _ = ribbon.on_search(SearchMsg::Collapse(true), now);
        let _ = ribbon.on_search(SearchMsg::OpenField, now);
        // Turning narrow closes the temporary box; the preference stays.
        ribbon.set_window_size((900.0, 900.0));
        assert!(!ribbon.search.temp_open);
        let _ = ribbon.on_search(SearchMsg::Collapse(false), now);
        ribbon.set_window_size((900.0, 900.0));
        assert!(!ribbon.shows_box());
        ribbon.set_window_size((1600.0, 900.0));
        assert!(ribbon.shows_box());
    }
}
