//! The Point Group Properties window CREATEPOINTGROUP opens: the group's
//! name, styles, the groups and rules it takes its points from, a custom
//! query, what it overrides in its points, its point list and summary. OK
//! (or Apply) writes the group.

use iced::widget::{Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::civil::point_group_query::{Attr, Joint, Op, Point, Query, Row, Rules, Term};
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{card, well_style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgTab {
    Information,
    PointGroups,
    RawDesc,
    Include,
    Exclude,
    QueryBuilder,
    Overrides,
    PointList,
    Summary,
}

impl PgTab {
    pub const ALL: [PgTab; 9] = [
        PgTab::Information,
        PgTab::PointGroups,
        PgTab::RawDesc,
        PgTab::Include,
        PgTab::Exclude,
        PgTab::QueryBuilder,
        PgTab::Overrides,
        PgTab::PointList,
        PgTab::Summary,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PgTab::Information => "Information",
            PgTab::PointGroups => "Point Groups",
            PgTab::RawDesc => "Raw Desc Matching",
            PgTab::Include => "Include",
            PgTab::Exclude => "Exclude",
            PgTab::QueryBuilder => "Query Builder",
            PgTab::Overrides => "Overrides",
            PgTab::PointList => "Point List",
            PgTab::Summary => "Summary",
        }
    }

    /// The tab a script names: its label in lower case, spaces as `_`.
    pub fn of(name: &str) -> Option<PgTab> {
        PgTab::ALL.into_iter().find(|t| t.label().to_lowercase().replace(' ', "_") == name)
    }
}

/// Another point group of the drawing, listed on Point Groups.
#[derive(Debug, Clone)]
pub struct GroupRow {
    pub name: String,
    pub description: String,
    pub numbers: Vec<u32>,
    pub picked: bool,
}

/// The window's state.
#[derive(Debug, Clone)]
pub struct PointGroupWindow {
    pub tab: PgTab,
    pub name: String,
    pub description: String,
    pub point_styles: Vec<(String, u64)>,
    pub label_styles: Vec<(String, u64)>,
    pub point_style: usize,
    pub label_style: usize,
    /// A style was picked: the group gets the styles picked (otherwise
    /// the drawing's defaults the codec gives it).
    pub styles_picked: bool,
    pub groups: Vec<GroupRow>,
    pub include: Rules,
    pub include_all: bool,
    pub exclude: Rules,
    pub case_sensitive: bool,
    pub modify_query: bool,
    pub rows: Vec<Row>,
    pub row: Option<usize>,
    /// Overrides: raw description, elevation (each with its value), point
    /// style, point label style.
    pub override_raw: (bool, String),
    pub override_elevation: (bool, String),
    pub override_style: bool,
    pub override_label: bool,
    pub points: Vec<Point>,
    pub list: PointListView,
    pub error: Option<String>,
    /// The group Apply wrote; OK and a later Apply rewrite it.
    pub written: Option<document::Handle>,
}

impl PointGroupWindow {
    /// The query the window describes; the Include terms take the points of
    /// the groups picked on Point Groups.
    pub fn query(&self) -> Result<Query, (Attr, crate::modules::civil::point_group_query::Bad)> {
        use crate::modules::civil::point_group_query::{Value, rules_terms};
        let mut include = Vec::new();
        for g in self.groups.iter().filter(|g| g.picked) {
            include.extend(g.numbers.iter().map(|n| Term::Compare(Attr::Number, Op::Eq, Value::Number(*n as f64))));
        }
        include.extend(rules_terms(&self.include)?);
        let exclude = rules_terms(&self.exclude)?;
        Ok(Query {
            include_all: self.include_all,
            include,
            exclude,
            custom: self.modify_query.then(|| self.rows.clone()),
            case_sensitive: self.case_sensitive,
        })
    }
}

#[derive(Debug, Clone)]
pub enum PointGroupMsg {
    Tab(PgTab),
    Name(String),
    Description(String),
    PointStyle(String),
    LabelStyle(String),
    Group(usize, bool),
    /// Include (false) or Exclude (true) rule `index` turned on or off.
    Rule(bool, usize, bool),
    RuleText(bool, usize, String),
    /// A script's rule: on, with this text.
    SetRule(bool, usize, String),
    IncludeAll(bool),
    /// "Selection Set in Drawing >>" of Include or Exclude.
    Pick(bool),
    CaseSensitive(bool),
    ModifyQuery(bool),
    InsertRow,
    DeleteRow,
    SelectRow(usize),
    RowJoint(usize, Joint),
    RowOpen(usize, bool),
    RowAttr(usize, Attr),
    RowOp(usize, Op),
    RowValue(usize, String),
    RowClose(usize, bool),
    Reset,
    Override(usize, bool),
    OverrideText(usize, String),
    ListFilter(String),
    ListSort(ListCol),
    ListSwitch(ListSwitch),
    ListPick(u32),
    /// Select the point (or, with none, the listed points) in the drawing.
    ListShow(Option<u32>),
    DismissError,
    Apply,
    Ok,
    Help,
}

fn msg(m: PointGroupMsg) -> Message {
    Message::PointGroup(m)
}

fn small(s: impl Into<String>) -> iced::widget::Text<'static> {
    text(s.into()).size(11)
}

fn field<'a>(value: &'a str, enabled: bool, on: impl Fn(String) -> Message + 'a) -> Element<'a, Message> {
    let mut input = text_input("", value).size(11).padding([4, 8]).width(Fill).style(field_style);
    if enabled {
        input = input.on_input(on);
    }
    input.into()
}

/// One "With … matching" row: its switch and its text.
fn rule_row<'a>(label: String, rule: &'a crate::modules::civil::point_group_query::Rule, enabled: bool, exclude: bool, index: usize) -> Element<'a, Message> {
    let mut check = checkbox(rule.on).label(label).size(14).text_size(11).width(Length::Fixed(210.0));
    if enabled {
        check = check.on_toggle(move |v| msg(PointGroupMsg::Rule(exclude, index, v)));
    }
    row![check, field(&rule.text, enabled && rule.on, move |s| msg(PointGroupMsg::RuleText(exclude, index, s)))]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn rules_view<'a>(state: &'a PointGroupWindow, exclude: bool) -> Element<'a, Message> {
    let rules = if exclude { &state.exclude } else { &state.include };
    let enabled = !state.modify_query && (exclude || !state.include_all);
    let labels = [
        t!("With numbers matching:"),
        t!("With elevations matching:"),
        t!("With names matching:"),
        t!("With raw descriptions matching:"),
        t!("With full descriptions matching:"),
    ];
    let mut col = column![].spacing(8);
    for (index, label) in labels.into_iter().enumerate() {
        col = col.push(rule_row(label.into_owned(), &rules.0[index], enabled, exclude, index));
        if index == 0 {
            col = col.push(
                row![
                    Space::new().width(Length::Fixed(218.0)),
                    button(small(t!("Selection Set in Drawing >>")))
                        .on_press_maybe((enabled && rules.0[0].on).then(|| msg(PointGroupMsg::Pick(exclude))))
                        .style(button_style(false))
                        .padding([4, 10]),
                ]
                .spacing(8),
            );
        }
    }
    if !exclude {
        let mut all = checkbox(state.include_all).label(t!("Include all points")).size(14).text_size(11);
        if !state.modify_query {
            all = all.on_toggle(|v| msg(PointGroupMsg::IncludeAll(v)));
        }
        col = col.push(Space::new().height(8)).push(all);
    }
    card(if exclude { t!("Exclude points") } else { t!("Include points") }.into_owned(), col).into()
}

fn style_pick<'a>(label: String, styles: &'a [(String, u64)], current: usize, on: fn(String) -> PointGroupMsg) -> Element<'a, Message> {
    let names: Vec<String> = styles.iter().map(|s| s.0.clone()).collect();
    row![
        small(label).style(muted_style).width(Length::Fixed(110.0)),
        pick_list(styles.get(current).map(|s| s.0.clone()), names, |n: &String| n.clone())
            .on_select(move |n| msg(on(n)))
            .text_size(11)
            .padding([4, 8])
            .width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn query_builder<'a>(state: &'a PointGroupWindow) -> Element<'a, Message> {
    let on = state.modify_query;
    let mut head = column![
        checkbox(state.case_sensitive).label(t!("Use case-sensitive matching")).size(14).text_size(11).on_toggle(|v| msg(PointGroupMsg::CaseSensitive(v))),
        checkbox(state.modify_query).label(t!("Modify query")).size(14).text_size(11).on_toggle(|v| msg(PointGroupMsg::ModifyQuery(v))),
    ]
    .spacing(6);
    let header = row![
        small(t!("Set Operator")).style(muted_style).width(Length::Fixed(90.0)),
        small("(").style(muted_style).width(Length::Fixed(28.0)),
        small(t!("Property")).style(muted_style).width(Length::Fixed(150.0)),
        small(t!("Operator")).style(muted_style).width(Length::Fixed(70.0)),
        small(t!("Value")).style(muted_style).width(Fill),
        small(")").style(muted_style).width(Length::Fixed(28.0)),
    ]
    .spacing(6);
    let mut list = column![header].spacing(4);
    for (k, r) in state.rows.iter().enumerate() {
        let joints: Vec<String> = Joint::ALL.iter().map(|j| j.label().to_string()).collect();
        let attrs: Vec<String> = Attr::ALL.iter().map(|a| t!(a.label()).into_owned()).collect();
        let ops: Vec<String> = Op::ALL.iter().map(|o| o.label().to_string()).collect();
        let selected = state.row == Some(k);
        let line = row![
            pick_list(Some(r.joint.label().to_string()), joints, |s: &String| s.clone())
                .on_select(move |s| msg(PointGroupMsg::RowJoint(k, Joint::ALL.into_iter().find(|j| j.label() == s).unwrap_or(Joint::None))))
                .text_size(11)
                .padding([3, 6])
                .width(Length::Fixed(90.0)),
            checkbox(r.open).size(13).on_toggle(move |v| msg(PointGroupMsg::RowOpen(k, v))).width(Length::Fixed(28.0)),
            pick_list(Some(t!(r.attr.label()).into_owned()), attrs, |s: &String| s.clone())
                .on_select(move |s| msg(PointGroupMsg::RowAttr(k, Attr::ALL.into_iter().find(|a| t!(a.label()) == s.as_str()).unwrap_or(Attr::Name))))
                .text_size(11)
                .padding([3, 6])
                .width(Length::Fixed(150.0)),
            pick_list(Some(r.op.label().to_string()), ops, |s: &String| s.clone())
                .on_select(move |s| msg(PointGroupMsg::RowOp(k, Op::ALL.into_iter().find(|o| o.label() == s).unwrap_or(Op::Eq))))
                .text_size(11)
                .padding([3, 6])
                .width(Length::Fixed(70.0)),
            field(&r.value, true, move |s| msg(PointGroupMsg::RowValue(k, s))),
            checkbox(r.close).size(13).on_toggle(move |v| msg(PointGroupMsg::RowClose(k, v))).width(Length::Fixed(28.0)),
        ]
        .spacing(6)
        .align_y(iced::Center);
        list = list.push(
            button(line)
                .on_press(msg(PointGroupMsg::SelectRow(k)))
                .style(button_style(selected))
                .padding([2, 4])
                .width(Fill),
        );
    }
    let body: Element<'a, Message> = if on {
        column![
            scrollable(list).height(Length::Fixed(200.0)),
            row![
                button(small(t!("Insert Row"))).on_press(msg(PointGroupMsg::InsertRow)).style(button_style(false)).padding([4, 10]),
                button(small(t!("Delete Row")))
                    .on_press_maybe(state.row.map(|_| msg(PointGroupMsg::DeleteRow)))
                    .style(button_style(false))
                    .padding([4, 10]),
                Space::new().width(Fill),
                button(small(t!("Reset"))).on_press(msg(PointGroupMsg::Reset)).style(button_style(false)).padding([4, 10]),
            ]
            .spacing(6),
        ]
        .spacing(8)
        .into()
    } else {
        // Off: the rows the Include and Exclude tabs make, read only.
        let statement = state.query().map(|q| q.statement()).unwrap_or_default();
        container(small(statement)).padding(8).width(Fill).height(Length::Fixed(200.0)).style(well_style).into()
    };
    head = head.push(body);
    card(t!("Query Builder").into_owned(), head).into()
}

fn overrides<'a>(state: &'a PointGroupWindow) -> Element<'a, Message> {
    let style = state.point_styles.get(state.point_style).map(|s| s.0.clone()).unwrap_or_default();
    let label = state.label_styles.get(state.label_style).map(|s| s.0.clone()).unwrap_or_default();
    let line = |index: usize, name: String, on: bool, value: Element<'a, Message>| -> Element<'a, Message> {
        row![
            checkbox(on).label(name).size(14).text_size(11).on_toggle(move |v| msg(PointGroupMsg::Override(index, v))).width(Length::Fixed(200.0)),
            value,
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };
    card(
        t!("Overrides").into_owned(),
        column![
            line(0, t!("Raw Description").into_owned(), state.override_raw.0, field(&state.override_raw.1, state.override_raw.0, |s| msg(PointGroupMsg::OverrideText(0, s)))),
            line(1, t!("Point Elevation").into_owned(), state.override_elevation.0, field(&state.override_elevation.1, state.override_elevation.0, |s| msg(PointGroupMsg::OverrideText(1, s)))),
            line(2, t!("Style").into_owned(), state.override_style, small(style).into()),
            line(3, t!("Point Label Style").into_owned(), state.override_label, small(label).into()),
        ]
        .spacing(8),
    )
    .into()
}

/// A column of the Point List.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListCol {
    Number,
    Easting,
    Northing,
    Elevation,
    Name,
    Raw,
    Full,
    GridEasting,
    GridNorthing,
}

impl ListCol {
    fn title(self) -> &'static str {
        match self {
            ListCol::Number => "Point Number",
            ListCol::Easting => "Easting",
            ListCol::Northing => "Northing",
            ListCol::Elevation => "Point Elevation",
            ListCol::Name => "Name",
            ListCol::Raw => "Raw Description",
            ListCol::Full => "Full Description",
            ListCol::GridEasting => "Grid Easting",
            ListCol::GridNorthing => "Grid Northing",
        }
    }

    /// The column a script names: its title in lower case, spaces as `_`.
    pub fn of(name: &str) -> Option<ListCol> {
        use ListCol::*;
        [Number, Easting, Northing, Elevation, Name, Raw, Full, GridEasting, GridNorthing]
            .into_iter()
            .find(|c| c.title().to_lowercase().replace(' ', "_") == name)
    }

    fn width(self) -> f32 {
        match self {
            ListCol::Number => 100.0,
            ListCol::Name => 70.0,
            ListCol::Raw | ListCol::Full => 100.0,
            _ => 90.0,
        }
    }

    fn numeric(self) -> bool {
        !matches!(self, ListCol::Name | ListCol::Raw | ListCol::Full)
    }

    /// The cell of point `p`. Without description keys the full
    /// description is the raw one; without a coordinate system grid
    /// coordinates are the drawing's.
    pub fn cell(self, p: &Point) -> String {
        match self {
            ListCol::Number => p.number.to_string(),
            ListCol::Easting | ListCol::GridEasting => format!("{:.4}", p.position[0]),
            ListCol::Northing | ListCol::GridNorthing => format!("{:.4}", p.position[1]),
            ListCol::Elevation => {
                if p.has_elevation() {
                    format!("{:.3}", p.position[2])
                } else {
                    String::new()
                }
            }
            ListCol::Name => p.name.clone().unwrap_or_default(),
            ListCol::Raw | ListCol::Full => p.raw.clone().unwrap_or_default(),
        }
    }

    fn key(self, p: &Point) -> f64 {
        match self {
            ListCol::Number => p.number as f64,
            ListCol::Easting | ListCol::GridEasting => p.position[0],
            ListCol::Northing | ListCol::GridNorthing => p.position[1],
            _ => p.position[2],
        }
    }
}

/// How the Point List shows the group's points.
#[derive(Debug, Clone)]
pub struct PointListView {
    pub filter: String,
    pub sort: ListCol,
    pub descending: bool,
    pub grid: bool,
    pub full: bool,
    pub name: bool,
    pub raw: bool,
    /// The column switches are open under the toolbar.
    pub columns_open: bool,
    pub picked: Option<u32>,
}

impl Default for PointListView {
    fn default() -> Self {
        PointListView {
            filter: String::new(),
            sort: ListCol::Number,
            descending: false,
            grid: false,
            full: true,
            name: true,
            raw: true,
            columns_open: false,
            picked: None,
        }
    }
}

impl PointListView {
    pub fn columns(&self) -> Vec<ListCol> {
        let mut c = vec![ListCol::Number, ListCol::Easting, ListCol::Northing, ListCol::Elevation];
        let optional = [
            (self.name, ListCol::Name),
            (self.raw, ListCol::Raw),
            (self.full, ListCol::Full),
            (self.grid, ListCol::GridEasting),
            (self.grid, ListCol::GridNorthing),
        ];
        c.extend(optional.into_iter().filter(|o| o.0).map(|o| o.1));
        c
    }

    /// The listed points: the group's, filtered by number, name or raw
    /// description and sorted by the sort column.
    pub fn rows<'p>(&self, points: &'p [Point], members: &[u32]) -> Vec<&'p Point> {
        let needle = self.filter.trim().to_lowercase();
        let mut rows: Vec<&Point> = points
            .iter()
            .filter(|p| members.contains(&p.number))
            .filter(|p| {
                needle.is_empty() || [ListCol::Number, ListCol::Name, ListCol::Raw].iter().any(|c| c.cell(p).to_lowercase().contains(&needle))
            })
            .collect();
        let col = self.sort;
        if col.numeric() {
            rows.sort_by(|a, b| col.key(a).total_cmp(&col.key(b)));
        } else {
            rows.sort_by_key(|p| col.cell(p).to_lowercase());
        }
        if self.descending {
            rows.reverse();
        }
        rows
    }
}

/// A switch of the Point List toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSwitch {
    Grid,
    Full,
    Name,
    Raw,
    Columns,
}

fn point_list<'a>(state: &'a PointGroupWindow) -> Element<'a, Message> {
    use crate::ui::kit::{column as list_column, column_header, list_row, number_cell, search_field, text_cell};
    use crate::ui::window::pdf_dialogs::chip;
    let view = &state.list;
    let members = state.query().ok().and_then(|q| q.members(&state.points).ok()).unwrap_or_default();
    let rows = view.rows(&state.points, &members);
    let cols = view.columns();
    let switch = |s: ListSwitch| msg(PointGroupMsg::ListSwitch(s));
    let tools = row![
        container(search_field(t!("Search number, name or description").as_ref(), &view.filter, |s| msg(PointGroupMsg::ListFilter(s))))
            .width(Length::Fixed(230.0)),
        small(t!("%{n} points", n = rows.len())).style(muted_style),
        Space::new().width(Fill),
        chip(t!("Grid coordinates").into_owned(), view.grid, switch(ListSwitch::Grid)),
        chip(t!("Full Description").into_owned(), view.full, switch(ListSwitch::Full)),
        chip(t!("Columns").into_owned(), view.columns_open, switch(ListSwitch::Columns)),
        button(text(t!("Show in drawing")).size(11)).on_press(msg(PointGroupMsg::ListShow(None))).style(button_style(false)).padding([4, 10]),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let mut body = column![tools].spacing(6);
    if view.columns_open {
        body = body.push(
            row![
                small(t!("Columns:")).style(muted_style),
                chip(t!("Name").into_owned(), view.name, switch(ListSwitch::Name)),
                chip(t!("Raw Description").into_owned(), view.raw, switch(ListSwitch::Raw)),
            ]
            .spacing(6)
            .align_y(iced::Center),
        );
    }
    let header = column_header(
        cols.iter()
            .map(|c| {
                let mut h = list_column(t!(c.title()).into_owned(), Length::Fixed(c.width())).on_press(msg(PointGroupMsg::ListSort(*c)));
                if c.numeric() {
                    h = h.end();
                }
                if *c == view.sort {
                    h = h.sorted(view.descending);
                }
                h
            })
            .collect(),
    );
    let mut list = column![];
    for p in &rows {
        let cells = cols
            .iter()
            .map(|c| {
                let v = c.cell(p);
                let cell = if c.numeric() { number_cell(v) } else { text_cell(v, false) };
                (cell, Length::Fixed(c.width()))
            })
            .collect();
        list = list.push(
            list_row(cells)
                .selected(view.picked == Some(p.number))
                .on_press(msg(PointGroupMsg::ListPick(p.number)))
                .on_double_click(msg(PointGroupMsg::ListShow(Some(p.number))))
                .view(),
        );
    }
    let table = container(column![header, scrollable(list).height(Fill)]).height(Fill).width(Fill).style(well_style);
    let note = small(t!("Click a title to sort; double-click a row to select its point in the drawing.")).style(muted_style);
    card(t!("Point List").into_owned(), body.push(table).push(note)).into()
}

fn summary<'a>(state: &'a PointGroupWindow) -> Element<'a, Message> {
    let query = state.query().ok();
    let count = query.as_ref().and_then(|q| q.members(&state.points).ok()).map_or(0, |m| m.len());
    let style = state.point_styles.get(state.point_style).map(|s| s.0.clone()).unwrap_or_default();
    let label = state.label_styles.get(state.label_style).map(|s| s.0.clone()).unwrap_or_default();
    let yes = |b: bool| if b { t!("True") } else { t!("False") }.into_owned();
    let pairs = [
        (t!("Name").into_owned(), state.name.clone()),
        (t!("Description").into_owned(), state.description.clone()),
        (t!("Number of points").into_owned(), count.to_string()),
        (t!("Point Label Style").into_owned(), label),
        (t!("Style").into_owned(), style),
        (t!("Point Label Style Overridden").into_owned(), yes(state.override_label)),
        (t!("Point Style Overridden").into_owned(), yes(state.override_style)),
        (t!("Raw Description Overridden").into_owned(), yes(state.override_raw.0)),
        (t!("Elevation Overridden").into_owned(), yes(state.override_elevation.0)),
    ];
    let mut col = column![].spacing(4);
    for (k, v) in pairs {
        col = col.push(row![small(k).style(muted_style).width(Length::Fixed(220.0)), small(v)].spacing(8));
    }
    let statement = query.map(|q| q.statement()).unwrap_or_default();
    column![
        card(t!("Summary").into_owned(), col),
        card(t!("Query Statement:").into_owned(), container(small(statement)).padding(8).width(Fill).height(Length::Fixed(70.0)).style(well_style)),
    ]
    .spacing(10)
    .into()
}

pub fn view<'a>(state: &'a PointGroupWindow, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let tabs = crate::ui::kit::tabs(PgTab::ALL.iter().map(|t| (t!(t.label()).into_owned(), msg(PointGroupMsg::Tab(*t)))).collect(), PgTab::ALL.iter().position(|t| *t == state.tab).unwrap_or(0));
    let body: Element<'a, Message> = match state.tab {
        PgTab::Information => column![
            card(
                t!("Information").into_owned(),
                column![
                    row![small(t!("Name:")).style(muted_style).width(Length::Fixed(110.0)), field(&state.name, true, |s| msg(PointGroupMsg::Name(s)))].spacing(8).align_y(iced::Center),
                    row![small(t!("Description:")).style(muted_style).width(Length::Fixed(110.0)), field(&state.description, true, |s| msg(PointGroupMsg::Description(s)))].spacing(8).align_y(iced::Center),
                ]
                .spacing(8),
            ),
            card(
                t!("Default styles").into_owned(),
                column![
                    style_pick(t!("Point style:").into_owned(), &state.point_styles, state.point_style, PointGroupMsg::PointStyle),
                    style_pick(t!("Point label style:").into_owned(), &state.label_styles, state.label_style, PointGroupMsg::LabelStyle),
                ]
                .spacing(8),
            ),
        ]
        .spacing(10)
        .into(),
        PgTab::PointGroups => {
            let mut list = column![].spacing(4);
            for (k, g) in state.groups.iter().enumerate() {
                let mut c = checkbox(g.picked).label(g.name.clone()).size(14).text_size(11).width(Length::Fixed(260.0));
                if !state.modify_query {
                    c = c.on_toggle(move |v| msg(PointGroupMsg::Group(k, v)));
                }
                list = list.push(row![c, small(g.description.clone()).style(muted_style)].spacing(8));
            }
            card(t!("Point Groups").into_owned(), scrollable(list).height(Length::Fixed(310.0))).into()
        }
        PgTab::RawDesc => card(
            t!("Raw Desc Matching").into_owned(),
            column![
                row![small(t!("Code")).style(muted_style).width(Length::Fixed(260.0)), small(t!("Format")).style(muted_style)].spacing(8),
                Space::new().height(Length::Fixed(300.0)),
            ]
            .spacing(6),
        )
        .into(),
        PgTab::Include => rules_view(state, false),
        PgTab::Exclude => rules_view(state, true),
        PgTab::QueryBuilder => query_builder(state),
        PgTab::Overrides => overrides(state),
        PgTab::PointList => point_list(state),
        PgTab::Summary => summary(state),
    };
    // Every tab takes the same room, so the window keeps its size.
    let body = container(body).height(Length::Fixed(370.0));
    let mut col = column![tabs, body]
    .spacing(10);
    if let Some(error) = &state.error {
        col = col.push(
            container(
                row![
                    small(error.clone()),
                    Space::new().width(Fill),
                    button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 10.0)).on_press(msg(PointGroupMsg::DismissError)).style(button_style(false)).padding([1, 6]),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .padding([6, 10])
            .width(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(iced::Background::Color(theme.palette().danger.base.color.scale_alpha(0.18))),
                border: iced::Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }),
        );
    }
    col = col.push(
        row![
            button(text("?").size(12)).on_press(msg(PointGroupMsg::Help)).style(button_style(false)).padding([5, 11]),
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button(t!("Apply"), msg(PointGroupMsg::Apply), false),
            dialog_button(t!("OK"), msg(PointGroupMsg::Ok), true),
        ]
        .spacing(6)
        .align_y(iced::Center),
    );
    col.padding([10, 12]).width(sizing.width).into()
}
