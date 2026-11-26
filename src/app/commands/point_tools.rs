//! The tools under the Create Points groups: Create Point Group (the Point
//! Group Properties window), Create COGO Points from Corridor, Create Blocks
//! from COGO Points, Convert CAD Points and Replace Legacy Point Blocks.

use std::collections::HashMap;

use document::classes::civil as c3d;
use document::objects::ObjectType;
use document::Handle;
use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::civil::point_group_query::{self as pq, Bad, Point};
use crate::ui::window::point_group::{GroupRow, PgTab, PointGroupMsg, PointGroupWindow};
use crate::ui::window::point_tools::{CorridorPointsWindow, DuplicatePointWindow, PointBlocksWindow, PointToolMsg, Resolution, Spatial};

/// The windows the tools open and what they keep between runs.
#[derive(Debug, Default)]
pub struct PointTools {
    pub point_group: Option<PointGroupWindow>,
    pub corridor: Option<CorridorPointsWindow>,
    pub blocks: Option<PointBlocksWindow>,
    /// The block conversion waiting on its duplicate question.
    pub block_job: Option<BlockJob>,
    /// The legacy block replacement waiting on its duplicate question.
    pub legacy_job: Option<LegacyJob>,
    pub duplicate: Option<DuplicatePointWindow>,
    /// The last "Point Group - (n)" each drawing (tab id) offered: a window
    /// that was cancelled still takes its number (measured: (4) cancelled,
    /// then (5)).
    group_names: HashMap<u64, usize>,
}

/// The selection the Point Group Properties window steps aside for.
pub const PICK_INCLUDE: &str = "_POINTGROUP_PICKED_INCLUDE";
pub const PICK_EXCLUDE: &str = "_POINTGROUP_PICKED_EXCLUDE";

fn arch(o: &ObjectType) -> Option<&document::classes::arch::ArchObject> {
    match o {
        ObjectType::Arch(a) => Some(a),
        _ => None,
    }
}

/// A style's name: its first text, "Standard" for the unnamed default the
/// codec writes (the reference shows it so).
fn style_name(a: &document::classes::arch::ArchObject) -> String {
    a.decode().strings.into_iter().next().filter(|s| !s.is_empty()).unwrap_or_else(|| "Standard".into())
}

/// The drawing's point groups in the group node's order: name,
/// description, members, and the point and label style handles.
pub fn point_groups(doc: &document::Document) -> Vec<(String, String, Vec<u32>, u64, u64)> {
    let order: Vec<u64> = doc
        .objects
        .values()
        .filter_map(arch)
        .find(|o| o.dxf_name == "AECC_POINT_GROUP_NODE")
        .map(|o| o.decode().handles)
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let handles = order.into_iter().chain(doc.objects.keys().map(|h| h.value()).collect::<Vec<_>>());
    for h in handles {
        if seen.contains(&h) {
            continue;
        }
        let Some(g) = doc.objects.get(&Handle::new(h)).and_then(arch).filter(|o| o.dxf_name == "AECC_POINT_PG") else { continue };
        seen.push(h);
        let d = g.decode();
        let all = d.strings.iter().any(|s| s == "AeccImpAllPointsPG");
        let name = if all { "_All Points".to_string() } else { d.strings.first().cloned().unwrap_or_default() };
        let description = if all { String::new() } else { d.strings.get(1).cloned().unwrap_or_default() };
        out.push((name, description, g.point_group_numbers(), d.handles.get(1).copied().unwrap_or(0), d.handles.first().copied().unwrap_or(0)));
    }
    out
}

impl OpenCADStudio {
    /// CREATEPOINTGROUP: the Point Group Properties window of a new group.
    pub(in crate::app) fn open_point_group_window(&mut self) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let points: Vec<Point> = c3d::cogo_points(doc).iter().map(Point::of).collect();
        let groups = point_groups(doc);
        let styles = |class: &str, want: &dyn Fn(&document::classes::arch::ArchObject) -> bool| -> Vec<(String, u64)> {
            let mut v: Vec<(String, u64)> = doc
                .objects
                .iter()
                .filter_map(|(h, o)| arch(o).filter(|a| a.dxf_name == class && want(a)).map(|a| (style_name(a), h.value())))
                .collect();
            v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            v
        };
        let point_styles = styles("AECC_POINT_STYLE", &|_| true);
        let label_styles = styles("AECC_LABEL_COLLECTOR_STYLE", &|a| a.decode().strings.iter().any(|s| s == "AeccDbCogoPoint"));
        // The new group starts with _All Points' styles, the drawing's
        // defaults.
        let all = groups.iter().find(|g| g.0 == "_All Points");
        let index = |list: &[(String, u64)], h: Option<u64>| h.and_then(|h| list.iter().position(|s| s.1 == h)).unwrap_or(0);
        let point_style = index(&point_styles, all.map(|g| g.3));
        let label_style = index(&label_styles, all.map(|g| g.4));
        let name = self.next_group_name(i, groups.len());
        let groups = groups.into_iter().map(|(name, description, numbers, ..)| GroupRow { name, description, numbers, picked: false }).collect();
        self.point_tools.point_group = Some(PointGroupWindow {
            tab: PgTab::Information,
            name,
            description: String::new(),
            point_styles,
            label_styles,
            point_style,
            label_style,
            styles_picked: false,
            groups,
            include: Default::default(),
            include_all: false,
            exclude: Default::default(),
            case_sensitive: false,
            modify_query: false,
            rows: Vec::new(),
            row: None,
            override_raw: (false, String::new()),
            override_elevation: (false, "0".into()),
            override_style: false,
            override_label: false,
            points,
            list: Default::default(),
            error: None,
            written: None,
        });
        self.active_modal = Some(crate::app::ModalKind::PointGroup);
    }

    /// A rule that does not read: "Point list is not valid!" in the window;
    /// an elevation the reference reports as a type mismatch on the command
    /// line with the query.
    fn point_group_refusal(&mut self, bad: Bad) {
        let Some(state) = self.point_tools.point_group.as_mut() else { return };
        match bad {
            Bad::Numbers => state.error = Some(crate::t!("Point list is not valid!").into_owned()),
            Bad::Mismatch => {
                let statement = state.query_text();
                self.command_line.push_error(&format!("{} {}", crate::t!("Type mismatch."), statement));
            }
        }
    }

    pub(in crate::app) fn on_point_group(&mut self, message: PointGroupMsg) -> Task<Message> {
        use crate::modules::civil::point_group_query::Rules;
        let Some(state) = self.point_tools.point_group.as_mut() else { return Task::none() };
        state.error = None;
        match message {
            PointGroupMsg::Tab(tab) => {
                // Include and Exclude are read as they are left.
                let leaving = match state.tab {
                    PgTab::Include => Some(&state.include),
                    PgTab::Exclude => Some(&state.exclude),
                    _ => None,
                };
                if let Some(Err((_, bad))) = leaving.map(pq::rules_terms) {
                    self.point_group_refusal(bad);
                    return Task::none();
                }
                state.tab = tab;
            }
            PointGroupMsg::Name(s) => state.name = s,
            PointGroupMsg::Description(s) => state.description = s,
            PointGroupMsg::PointStyle(s) => {
                state.point_style = state.point_styles.iter().position(|x| x.0 == s).unwrap_or(state.point_style);
                state.styles_picked = true;
            }
            PointGroupMsg::LabelStyle(s) => {
                state.label_style = state.label_styles.iter().position(|x| x.0 == s).unwrap_or(state.label_style);
                state.styles_picked = true;
            }
            PointGroupMsg::Group(k, on) => {
                if let Some(g) = state.groups.get_mut(k) {
                    g.picked = on;
                }
            }
            PointGroupMsg::Rule(exclude, k, on) => {
                let rules = if exclude { &mut state.exclude } else { &mut state.include };
                let rule = &mut rules.0[k.min(4)];
                rule.on = on;
                if !on {
                    rule.text.clear();
                }
            }
            PointGroupMsg::SetRule(exclude, k, s) => {
                let rules = if exclude { &mut state.exclude } else { &mut state.include };
                rules.0[k.min(4)] = pq::Rule { on: true, text: s };
            }
            PointGroupMsg::RuleText(exclude, k, s) => {
                let rules = if exclude { &mut state.exclude } else { &mut state.include };
                rules.0[k.min(4)].text = s;
            }
            // Include all points turns the other Include rules off.
            PointGroupMsg::IncludeAll(on) => {
                state.include_all = on;
                if on {
                    state.include = Rules::default();
                }
            }
            PointGroupMsg::Pick(exclude) => {
                self.active_modal = None;
                use crate::command::CadCommand;
                use crate::modules::draw::select::SelectObjectsCommand;
                let cmd = SelectObjectsCommand::plain("CREATEPOINTGROUP", if exclude { PICK_EXCLUDE } else { PICK_INCLUDE });
                self.command_line.push_info(&cmd.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(cmd));
            }
            PointGroupMsg::CaseSensitive(on) => state.case_sensitive = on,
            // Turned on, the rows start from what the rules select.
            PointGroupMsg::ModifyQuery(on) => {
                if on {
                    match state.query() {
                        Ok(q) => state.rows = pq::rows_of(&q.include, &q.exclude),
                        Err((_, bad)) => {
                            self.point_group_refusal(bad);
                            return Task::none();
                        }
                    }
                } else {
                    state.rows.clear();
                }
                state.modify_query = on;
                state.row = None;
            }
            PointGroupMsg::InsertRow => {
                let at = state.row.map_or(state.rows.len(), |r| r + 1);
                state.rows.insert(at, pq::Row::default());
                state.row = Some(at);
            }
            PointGroupMsg::DeleteRow => {
                if let Some(r) = state.row.take().filter(|r| *r < state.rows.len()) {
                    state.rows.remove(r);
                }
            }
            PointGroupMsg::SelectRow(k) => state.row = Some(k),
            PointGroupMsg::RowJoint(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.joint = v;
                }
            }
            PointGroupMsg::RowOpen(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.open = v;
                }
            }
            PointGroupMsg::RowAttr(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.attr = v;
                }
            }
            PointGroupMsg::RowOp(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.op = v;
                }
            }
            PointGroupMsg::RowValue(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.value = v;
                }
            }
            PointGroupMsg::RowClose(k, v) => {
                if let Some(r) = state.rows.get_mut(k) {
                    r.close = v;
                }
            }
            // Reset goes back to the rows the rules make.
            PointGroupMsg::Reset => {
                let mut copy = state.clone();
                copy.modify_query = false;
                if let Ok(q) = copy.query() {
                    state.rows = pq::rows_of(&q.include, &q.exclude);
                }
            }
            PointGroupMsg::Override(k, on) => match k {
                0 => state.override_raw.0 = on,
                1 => state.override_elevation.0 = on,
                2 => state.override_style = on,
                _ => state.override_label = on,
            },
            PointGroupMsg::OverrideText(k, s) => {
                if k == 0 {
                    state.override_raw.1 = s;
                } else {
                    state.override_elevation.1 = s;
                }
            }
            PointGroupMsg::ListFilter(s) => state.list.filter = s,
            PointGroupMsg::ListSort(col) => {
                let list = &mut state.list;
                list.descending = list.sort == col && !list.descending;
                list.sort = col;
            }
            PointGroupMsg::ListSwitch(s) => {
                use crate::ui::window::point_group::ListSwitch;
                let list = &mut state.list;
                let flag = match s {
                    ListSwitch::Grid => &mut list.grid,
                    ListSwitch::Full => &mut list.full,
                    ListSwitch::Name => &mut list.name,
                    ListSwitch::Raw => &mut list.raw,
                    ListSwitch::Columns => &mut list.columns_open,
                };
                *flag = !*flag;
            }
            PointGroupMsg::ListPick(n) => state.list.picked = Some(n),
            PointGroupMsg::ListShow(n) => {
                // The row double-clicked, else the row picked, else every
                // point listed.
                let numbers: Vec<u32> = match n.or(state.list.picked) {
                    Some(n) => vec![n],
                    None => {
                        let members = state.query().ok().and_then(|q| q.members(&state.points).ok()).unwrap_or_default();
                        state.list.rows(&state.points, &members).iter().map(|p| p.number).collect()
                    }
                };
                let i = self.active_tab;
                let handles: Vec<Handle> = numbers.iter().filter_map(|n| cogo_point_entity(&self.tabs[i].scene.document, *n)).collect();
                let scene = &mut self.tabs[i].scene;
                scene.deselect_all();
                for h in &handles {
                    scene.select_entity(*h, false);
                }
                // A lone point has no extent to fit; the view stays where it is.
                if handles.len() > 1 {
                    scene.zoom_to_entities(&handles);
                }
                self.refresh_properties();
                self.refresh_selected_grips();
            }
            PointGroupMsg::DismissError => {}
            PointGroupMsg::Help => self.command_line.push_info(
                crate::t!("Defines a point group: the points it takes from other groups, rules and a query, the styles it draws them with and what it overrides in them.").as_ref(),
            ),
            PointGroupMsg::Apply => {
                self.write_point_group();
            }
            PointGroupMsg::Ok => {
                if self.write_point_group() {
                    self.point_tools.point_group = None;
                    self.close_active_modal();
                }
            }
        }
        Task::none()
    }

    /// The selection "Selection Set in Drawing >>" stepped aside for: the
    /// numbers of the COGO points picked fill the rule.
    pub(in crate::app) fn point_group_picked(&mut self, exclude: bool) {
        let i = self.active_tab;
        let numbers: Vec<u32> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .filter_map(|(_, e)| match e {
                document::EntityType::Extended(x) => match &x.data {
                    document::entities::ExtendedEntityData::Arch(a) => a.cogo_point_number(),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.deselect_all();
        self.refresh_properties();
        self.refresh_selected_grips();
        if let Some(state) = self.point_tools.point_group.as_mut() {
            if !numbers.is_empty() {
                let rules = if exclude { &mut state.exclude } else { &mut state.include };
                rules.0[0].on = true;
                rules.0[0].text = pq::number_list(&numbers);
            }
            self.active_modal = Some(crate::app::ModalKind::PointGroup);
        }
    }

    /// Writes the window's group (a new one, or the one Apply wrote) as one
    /// undo step; false when the window refused it.
    fn write_point_group(&mut self) -> bool {
        let i = self.active_tab;
        let Some(state) = self.point_tools.point_group.as_ref() else { return false };
        let name = pq::clean_name(state.name.trim());
        let refuse = |s: &mut Self, error: String| {
            if let Some(state) = s.point_tools.point_group.as_mut() {
                state.error = Some(error);
            }
            false
        };
        let duplicate = || crate::tf!("The name you entered ({}) is a duplicate, or pre-defined, or contains one or more of these invalid characters: <>/\\:;=|,*?'\"", name).into_owned();
        if name.is_empty() {
            return refuse(self, crate::t!("Invalid name! Name must have at least one character.").into_owned());
        }
        let doc = &self.tabs[i].scene.document;
        let own = state.written.and_then(|h| doc.objects.get(&h)).and_then(arch).and_then(|a| a.decode().strings.first().cloned());
        if point_groups(doc).iter().any(|g| g.0 == name && Some(&g.0) != own.as_ref()) || name == "_All Points" {
            return refuse(self, duplicate());
        }
        let query = match state.query().map_err(|(_, b)| b).and_then(|q| q.to_codec()) {
            Ok(q) => q,
            Err(bad) => {
                self.point_group_refusal(bad);
                return false;
            }
        };
        let elevation = match state.override_elevation.0 {
            true => match state.override_elevation.1.trim().parse::<f64>() {
                Ok(v) if v.is_finite() => Some(v),
                _ => {
                    self.point_group_refusal(Bad::Mismatch);
                    return false;
                }
            },
            false => None,
        };
        let style = state.point_styles.get(state.point_style).map(|s| s.1);
        let label = state.label_styles.get(state.label_style).map(|s| s.1);
        let overrides = c3d::PointGroupOverrides {
            point_style: style.filter(|_| state.override_style),
            label_style: label.filter(|_| state.override_label),
            elevation,
            raw_description: state.override_raw.0.then(|| state.override_raw.1.clone()),
            locked: false,
            classification: c3d::PointGroupClassification::None,
        };
        let any_override = overrides.point_style.is_some() || overrides.label_style.is_some() || elevation.is_some() || overrides.raw_description.is_some();
        let description = state.description.clone();
        let written = state.written;
        let styles_picked = state.styles_picked;
        self.push_undo_snapshot(i, "CREATEPOINTGROUP");
        let doc = &mut self.tabs[i].scene.document;
        let result = (|| -> Result<Handle, document::error::DxfError> {
            let h = match written {
                Some(h) => {
                    c3d::set_point_group_query(doc, h, &name, Some(&description), &query)?;
                    h
                }
                None => c3d::add_point_group_query(doc, &name, Some(&description), &query)?,
            };
            if let (true, Some(style), Some(label)) = (styles_picked, style, label) {
                c3d::set_point_group_styles(doc, h, style, label)?;
            }
            if any_override || written.is_some() {
                c3d::set_point_group_overrides(doc, h, &overrides)?;
            }
            Ok(h)
        })();
        match result {
            Ok(h) => {
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                if let Some(state) = self.point_tools.point_group.as_mut() {
                    state.written = Some(h);
                    state.name = name;
                }
                true
            }
            Err(e) => {
                let message = match e {
                    document::error::DxfError::Parse(m) if m.contains("already used") => duplicate(),
                    document::error::DxfError::Parse(m) => m,
                    e => e.to_string(),
                };
                refuse(self, message)
            }
        }
    }
}

impl PointGroupWindow {
    /// The query statement, or the rules' text when they do not read.
    pub fn query_text(&self) -> String {
        match self.query() {
            Ok(q) => q.statement(),
            Err(_) => {
                let mut copy = self.clone();
                for rules in [&mut copy.include, &mut copy.exclude] {
                    for (attr, rule) in pq::Rules::ORDER.iter().zip(rules.0.iter_mut()) {
                        if pq::rule_terms(*attr, &rule.text).is_err() {
                            rule.on = false;
                        }
                    }
                }
                let bad: Vec<String> = [&self.include, &self.exclude]
                    .iter()
                    .flat_map(|r| pq::Rules::ORDER.iter().zip(r.0.iter()).filter(|(a, rule)| rule.on && pq::rule_terms(**a, &rule.text).is_err()).map(|(a, rule)| format!("( {} = {} )", a.label(), rule.text)))
                    .collect();
                bad.join(" ")
            }
        }
    }
}

/// The layer a tool's COGO points go on: the point creation layer of the
/// drawing's settings, else the current layer (as CREATEPOINTMANUAL).
fn point_layer(doc: &document::Document, current: String) -> String {
    c3d::point_creation_layer(doc).filter(|l| doc.layers.get(l).is_some()).unwrap_or(current)
}

fn arch_entity(e: &document::EntityType) -> Option<&document::classes::arch::ArchEntityData> {
    match e {
        document::EntityType::Extended(x) => match &x.data {
            document::entities::ExtendedEntityData::Arch(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

/// The COGO point entity with number `n`.
fn cogo_point_entity(doc: &document::Document, n: u32) -> Option<Handle> {
    doc.entities().find(|e| arch_entity(e).and_then(|a| a.cogo_point_number()) == Some(n)).map(|e| e.common().handle)
}

/// An attribute of an insert by tag (case ignored).
fn attribute<'a>(insert: &'a document::entities::Insert, tag: &str) -> Option<&'a str> {
    insert.attributes.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)).map(|a| a.value.as_str())
}

/// A legacy point block: an insert with a POINT attribute.
pub fn is_legacy_point(e: &document::EntityType) -> bool {
    matches!(e, document::EntityType::Insert(i) if attribute(i, "POINT").is_some())
}

/// The private command the legacy point selection hands its blocks to.
pub const LEGACY_GATHERED: &str = "_REPLACELEGACY_GATHERED";

/// One legacy point block waiting to become a COGO point.
#[derive(Debug, Clone)]
pub struct Legacy {
    pub insert: Handle,
    pub number: u32,
    pub position: [f64; 3],
    pub description: Option<String>,
}

/// Replace Legacy Point Blocks under way.
#[derive(Debug, Default)]
pub struct LegacyJob {
    pub queue: std::collections::VecDeque<Legacy>,
    /// "Apply to all": the resolution, its offset or sequence start, and
    /// the next sequence number.
    pub all: Option<(Resolution, u32, u32)>,
}

/// Create Blocks from COGO Points under way.
#[derive(Debug)]
pub struct BlockJob {
    pub block: String,
    pub layer: String,
    pub queue: std::collections::VecDeque<c3d::CogoPoint>,
    /// The point asked about and the block it would replace.
    pub pending: Option<(c3d::CogoPoint, Handle)>,
    pub converted: usize,
}

impl OpenCADStudio {
    /// The next "Point Group - (n)" of the drawing in tab `i` holding
    /// `groups` point groups (the counter runs on through cancelled
    /// windows: measured).
    fn next_group_name(&mut self, i: usize, groups: usize) -> String {
        let id = self.tabs[i].id;
        let last = self.point_tools.group_names.entry(id).or_insert(0);
        *last = (*last + 1).max(groups);
        format!("Point Group - ({})", *last)
    }

    // ── Create COGO Points from Corridor ──────────────────────────────

    /// CREATEPOINTSFROMCORRIDOR: the window of the drawing's corridor; with
    /// several, the one picked first (measured: one corridor opens the
    /// window at once).
    pub(in crate::app) fn start_corridor_points(&mut self) {
        let i = self.active_tab;
        let corridors: Vec<Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| arch_entity(e).is_some_and(|a| a.dxf_name == "AECC_CORRIDOR"))
            .map(|e| e.common().handle)
            .collect();
        match corridors.as_slice() {
            [] => self.command_line.push_error(&crate::t!("There are no corridors in the current drawing.")),
            [one] => self.open_corridor_points(*one),
            _ => {
                use crate::command::CadCommand;
                let c = crate::modules::civil::point_pick::CorridorPickCommand::default();
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
        }
    }

    pub(in crate::app) fn open_corridor_points(&mut self, corridor: Handle) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        let doc = &self.tabs[i].scene.document;
        let Some(data) = doc.get_entity(corridor).and_then(arch_entity) else { return };
        let codes: Vec<(String, bool)> = data.corridor_point_codes().into_iter().map(|c| (c, true)).collect();
        let lines = data.corridor_baselines();
        let definition = data.corridor_definition();
        // A built corridor's definition leaves the baselines' alignments
        // unread; its references hold them in baseline order.
        // ponytail: a target alignment of an earlier baseline would take a
        // later baseline's place here; read the baselines' references once
        // the codec maps them for built corridors.
        let referenced: Vec<u64> = data
            .typed_handles()
            .iter()
            .map(|r| r.0)
            .filter(|h| doc.get_entity(Handle::new(*h)).and_then(arch_entity).is_some_and(|a| a.dxf_name == "AECC_ALIGNMENT"))
            .fold(Vec::new(), |mut v, h| {
                if !v.contains(&h) {
                    v.push(h);
                }
                v
            });
        let baselines = lines
            .iter()
            .enumerate()
            .map(|(k, groups)| {
                let stations = groups.iter().flatten().flatten().map(|(s, _)| *s);
                let (lo, hi) = stations.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(s), b.max(s)));
                let alignment = definition.as_ref().and_then(|d| d.baselines.get(k)).map(|b| b.alignment).filter(|h| *h != 0).or_else(|| referenced.get(k).copied());
                let name = alignment
                    .and_then(|h| doc.get_entity(Handle::new(h)))
                    .and_then(arch_entity)
                    .and_then(|a| a.alignment_name())
                    .unwrap_or_else(|| format!("Baseline ({})", k + 1));
                (name, if lo.is_finite() { lo } else { 0.0 }, if hi.is_finite() { hi } else { 0.0 }, alignment.unwrap_or(0))
            })
            .collect::<Vec<_>>();
        // The stations each baseline gives each code a point at; two runs
        // meeting at a station share its point (as OK takes them).
        let stations = lines
            .iter()
            .map(|groups| {
                (0..codes.len())
                    .map(|c| {
                        let mut v: Vec<f64> = Vec::new();
                        for (s, _) in groups.get(c).into_iter().flatten().flatten() {
                            if v.last().is_none_or(|l| (l - s).abs() > 1e-9) {
                                v.push(*s);
                            }
                        }
                        v
                    })
                    .collect()
            })
            .collect();
        let groups = point_groups(doc).len();
        let group_name = self.next_group_name(i, groups);
        let (start, end) = baselines.first().map_or((0.0, 0.0), |b| (b.1, b.2));
        use crate::modules::civil::point_alignment::format_station;
        self.point_tools.corridor = Some(CorridorPointsWindow {
            corridor,
            baselines,
            stations,
            baseline: 0,
            user_range: false,
            start: format_station(start),
            end: format_station(end),
            group_name,
            codes,
            error: None,
        });
        self.active_modal = Some(crate::app::ModalKind::CorridorPoints);
    }

    /// The pick a range button stepped aside for: the baseline whose
    /// alignment was picked, or the station of the point picked on the
    /// baseline; then the window comes back.
    pub(in crate::app) fn corridor_range_picked(&mut self, args: &str) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        let doc = &self.tabs[i].scene.document;
        let Some(w) = self.point_tools.corridor.as_mut() else { return };
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts[..] {
            [hex] => {
                let picked = u64::from_str_radix(hex, 16).ok();
                match w.baselines.iter().position(|b| Some(b.3) == picked) {
                    Some(k) => {
                        w.baseline = k;
                        w.start = crate::modules::civil::point_alignment::format_station(w.baselines[k].1);
                        w.end = crate::modules::civil::point_alignment::format_station(w.baselines[k].2);
                    }
                    None => w.error = Some(crate::t!("The alignment picked is not a baseline of this corridor.").into_owned()),
                }
            }
            [which, x, y] => {
                let alignment = doc.get_entity(Handle::new(w.baselines.get(w.baseline).map_or(0, |b| b.3)));
                if let (Some(e), Ok(x), Ok(y)) = (alignment, x.parse::<f64>(), y.parse::<f64>()) {
                    if let Some(s) = crate::modules::civil::point_alignment::station_on(e, [x, y]) {
                        let text = crate::modules::civil::point_alignment::format_station(s);
                        if which.eq_ignore_ascii_case("end") {
                            w.end = text;
                        } else {
                            w.start = text;
                        }
                    }
                }
            }
            _ => {}
        }
        self.active_modal = Some(crate::app::ModalKind::CorridorPoints);
    }

    /// OK: a COGO point at every corridor station of each code picked, in
    /// code order (measured: all of the first code's points, then the
    /// next's), described by the code; then the point group holding them.
    fn corridor_points_ok(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.point_tools.corridor.clone() else { return };
        let fail = |s: &mut Self, e: String| {
            if let Some(w) = s.point_tools.corridor.as_mut() {
                w.error = Some(e);
            }
        };
        let (start, end) = match state.range() {
            Some(r) => r,
            None if state.user_range => return fail(self, crate::t!("Type mismatch.").into_owned()),
            None => (f64::NEG_INFINITY, f64::INFINITY),
        };
        let name = pq::clean_name(state.group_name.trim());
        let doc = &self.tabs[i].scene.document;
        if !name.is_empty() && point_groups(doc).iter().any(|g| g.0 == name) {
            let e = crate::tf!("The name you entered ({}) is a duplicate, or pre-defined, or contains one or more of these invalid characters: <>/\\:;=|,*?'\"", name).into_owned();
            return fail(self, e);
        }
        let Some(data) = doc.get_entity(state.corridor).and_then(arch_entity) else { return };
        let lines = data.corridor_baselines();
        let mut points: Vec<(document::types::Vector3, String)> = Vec::new();
        for (c, (code, on)) in state.codes.iter().enumerate() {
            if !on {
                continue;
            }
            for (b, groups) in lines.iter().enumerate() {
                if state.user_range && b != state.baseline {
                    continue;
                }
                for run in groups.get(c).into_iter().flatten() {
                    for (station, p) in run {
                        // Two runs meeting at a station share its point.
                        let repeated = points.last().is_some_and(|(q, c2)| c2 == code && (q.x - p.x).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9 && (q.z - p.z).abs() < 1e-9);
                        if (start..=end).contains(station) && !repeated {
                            points.push((*p, code.clone()));
                        }
                    }
                }
            }
        }
        self.point_tools.corridor = None;
        self.close_active_modal();
        if points.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "CREATEPOINTSFROMCORRIDOR");
        let layer = point_layer(&self.tabs[i].scene.document, self.tabs[i].active_layer.clone());
        let doc = &mut self.tabs[i].scene.document;
        let mut numbers = Vec::new();
        let mut changes = Vec::new();
        let mut failed = None;
        for (p, code) in &points {
            match c3d::create_cogo_point(doc, *p, Some(code), Some(&layer)) {
                Ok((h, n)) => {
                    numbers.push(n);
                    changes.push((h, crate::scene::ChangeKind::Added));
                }
                Err(e) => {
                    failed = Some(e.to_string());
                    break;
                }
            }
        }
        if !name.is_empty() && !numbers.is_empty() {
            if let Err(e) = c3d::add_point_group(doc, &name, None, &numbers) {
                failed = Some(e.to_string());
            }
        }
        if let Some(e) = failed {
            self.command_line.push_error(&e);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.bump_entities(&changes);
        self.command_line.push_output(&crate::t!("COGO points created."));
    }

    // ── Create Blocks from COGO Points ────────────────────────────────

    /// CREATEBLOCKSFROMPOINTS: the window, _All Points picked and a new
    /// POINT block on layer 0 offered (measured).
    pub(in crate::app) fn open_point_blocks(&mut self) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let groups = point_groups(doc).into_iter().map(|g| (g.0.clone(), g.0 == "_All Points")).collect();
        let mut blocks: Vec<String> = doc
            .block_records
            .iter()
            .filter(|r| !r.name.starts_with('*'))
            .filter(|r| doc.entities_in_block(&r.name).any(|e| matches!(e, document::EntityType::AttributeDefinition(d) if d.tag.eq_ignore_ascii_case("POINT"))))
            .map(|r| r.name.clone())
            .collect();
        blocks.sort();
        let mut layers: Vec<String> = doc.layers.iter().map(|l| l.name.clone()).collect();
        layers.sort();
        self.point_tools.blocks = Some(PointBlocksWindow {
            spatial: Spatial::None,
            window: None,
            display: None,
            groups,
            blocks,
            use_existing: false,
            existing: 0,
            new_name: "POINT".into(),
            layers,
            layer: "0".into(),
            error: None,
        });
        self.active_modal = Some(crate::app::ModalKind::PointBlocks);
    }

    /// The window "Define window" picked (or none: unchanged).
    pub(in crate::app) fn point_blocks_window(&mut self, corners: &str) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        let v: Vec<f64> = corners.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if let Some(state) = self.point_tools.blocks.as_mut() {
            if let [a, b, c, d] = v[..] {
                state.window = Some(([a.min(c), b.min(d)], [a.max(c), b.max(d)]));
            }
            self.active_modal = Some(crate::app::ModalKind::PointBlocks);
        }
    }

    fn point_blocks_ok(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.point_tools.blocks.clone() else { return };
        let fail = |s: &mut Self, e: String| {
            if let Some(w) = s.point_tools.blocks.as_mut() {
                w.error = Some(e);
            }
        };
        let block = if state.use_existing {
            match state.blocks.get(state.existing) {
                Some(b) => b.clone(),
                None => return fail(self, crate::t!("Pick a block.").into_owned()),
            }
        } else {
            match crate::scene::block_service::validate_block_name(&state.new_name) {
                Ok(name) => name,
                Err(e) => return fail(self, e.to_string()),
            }
        };
        let window = match (state.spatial, state.window) {
            (Spatial::Window, None) => return fail(self, crate::t!("Pick the window first.").into_owned()),
            (_, w) => w,
        };
        let doc = &self.tabs[i].scene.document;
        let all = state.groups.iter().any(|(n, on)| *on && n == "_All Points");
        let picked: Vec<u32> = point_groups(doc).into_iter().filter(|g| state.groups.iter().any(|(n, on)| *on && *n == g.0)).flat_map(|g| g.2).collect();
        let mut points: Vec<c3d::CogoPoint> = c3d::cogo_points(doc).into_iter().filter(|p| all || picked.contains(&p.number)).collect();
        points.retain(|p| match (state.spatial, window) {
            (Spatial::Window, Some((lo, hi))) => (lo[0]..=hi[0]).contains(&p.position.x) && (lo[1]..=hi[1]).contains(&p.position.y),
            (Spatial::CurrentDisplay, _) => self.typed_point_in_view(i, glam::DVec3::new(p.position.x, p.position.y, 0.0)),
            _ => true,
        });
        points.sort_by_key(|p| p.number);
        self.point_tools.blocks = None;
        self.close_active_modal();
        self.push_undo_snapshot(i, "CREATEBLOCKSFROMPOINTS");
        self.ensure_point_block(i, &block);
        let layer = if self.tabs[i].scene.document.layers.get(&state.layer).is_some() { state.layer.clone() } else { "0".to_string() };
        self.point_tools.block_job = Some(BlockJob { block, layer, queue: points.into(), pending: None, converted: 0 });
        self.run_block_job();
    }

    /// The POINT block the reference writes, when the drawing has no block
    /// of that name: a point at the base, ELEV, POINT and DESC attribute
    /// definitions (height 1, on layers ELEV, PNTS and DESC of colour 7).
    fn ensure_point_block(&mut self, i: usize, name: &str) {
        let scene = &mut self.tabs[i].scene;
        if scene.document.block_records.get(name).is_some() {
            return;
        }
        for layer in ["ELEV", "PNTS", "DESC"] {
            if scene.document.layers.get(layer).is_none() {
                let _ = scene.document.layers.add(document::tables::Layer::new(layer));
            }
        }
        let mut entities = vec![document::EntityType::Point(document::entities::Point::new())];
        for (tag, layer, y) in [("ELEV", "ELEV", -0.5), ("POINT", "PNTS", 1.0), ("DESC", "DESC", -2.0)] {
            let mut d = document::entities::AttributeDefinition::new(tag.into(), String::new(), String::new());
            d.insertion_point = document::types::Vector3::new(1.5, y, 0.0);
            d.height = 1.0;
            d.common.layer = layer.into();
            entities.push(document::EntityType::AttributeDefinition(d));
        }
        scene.define_block_raw(name, document::types::Vector3::ZERO, entities);
    }

    /// Converts points until one meets a block holding its number, which
    /// the question settles.
    fn run_block_job(&mut self) {
        let i = self.active_tab;
        loop {
            let Some(job) = self.point_tools.block_job.as_mut() else { return };
            let Some(p) = job.queue.pop_front() else { break };
            let number = p.number.to_string();
            let existing = self.tabs[i]
                .scene
                .document
                .entities()
                .find(|e| matches!(e, document::EntityType::Insert(ins) if attribute(ins, "POINT") == Some(number.as_str())))
                .map(|e| e.common().handle);
            match existing {
                Some(h) => {
                    if let Some(job) = self.point_tools.block_job.as_mut() {
                        job.pending = Some((p, h));
                    }
                    self.active_modal = Some(crate::app::ModalKind::BlockDuplicate);
                    return;
                }
                None => self.insert_point_block(i, &p),
            }
        }
        self.finish_block_job();
    }

    fn insert_point_block(&mut self, i: usize, p: &c3d::CogoPoint) {
        let Some(job) = self.point_tools.block_job.as_mut() else { return };
        let (block, layer) = (job.block.clone(), job.layer.clone());
        job.converted += 1;
        let scene = &mut self.tabs[i].scene;
        // A point without elevation sits at 0 (measured: ELEV 0.000).
        let z = if crate::entities::civil::has_elevation(p.position.z) { p.position.z } else { 0.0 };
        let mut insert = document::entities::Insert::new(block.clone(), document::types::Vector3::new(p.position.x, p.position.y, z));
        insert.common.layer = layer;
        let transform = crate::scene::render_graph::insert_transform_with_base(&insert, document::types::Vector3::ZERO);
        for e in scene.document.entities_in_block(&block) {
            let document::EntityType::AttributeDefinition(def) = e else { continue };
            let value = match def.tag.to_uppercase().as_str() {
                "ELEV" => format!("{z:.3}"),
                "POINT" => p.number.to_string(),
                "DESC" => p.description.clone().unwrap_or_default(),
                _ => def.default_value.clone(),
            };
            let mut a = document::entities::AttributeEntity::from_definition(&def, Some(value));
            document::entities::Entity::apply_transform(&mut a, &transform);
            insert.attributes.push(a);
        }
        if let Ok(h) = scene.document.add_entity(document::EntityType::Insert(insert)) {
            scene.bump_entities(&[(h, crate::scene::ChangeKind::Added)]);
        }
        self.tabs[i].dirty = true;
    }

    pub(in crate::app) fn finish_block_job(&mut self) {
        if let Some(job) = self.point_tools.block_job.take() {
            self.command_line.push_output(&crate::tf!("Converted {} COGO points to Block {}.", job.converted, job.block));
        }
    }

    // ── Replace Legacy Point Blocks ───────────────────────────────────

    pub(in crate::app) fn start_legacy_points(&mut self) {
        use crate::command::CadCommand;
        let c = crate::modules::draw::select::SelectObjectsCommand::filtered("REPLACELEGACYPOINTBLOCKS", LEGACY_GATHERED, "Select legacy points:", is_legacy_point);
        self.command_line.push_info(&c.prompt());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(c));
    }

    /// The legacy blocks picked, in creation order, each to its COGO point.
    pub(in crate::app) fn legacy_points_gathered(&mut self) {
        let i = self.active_tab;
        let mut queue: Vec<Legacy> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .filter_map(|(h, e)| {
                let document::EntityType::Insert(ins) = e else { return None };
                let number = attribute(&ins, "POINT")?.trim().parse::<u32>().ok().filter(|n| *n > 0)?;
                let z = attribute(&ins, "ELEV").and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(ins.insert_point.z);
                let description = attribute(&ins, "DESC").map(str::to_string).filter(|d| !d.is_empty());
                Some(Legacy { insert: h, number, position: [ins.insert_point.x, ins.insert_point.y, z], description })
            })
            .collect();
        self.tabs[i].scene.deselect_all();
        self.refresh_properties();
        self.refresh_selected_grips();
        self.tabs[i].active_cmd = None;
        if queue.is_empty() {
            return;
        }
        queue.sort_by_key(|l| l.insert.value());
        self.push_undo_snapshot(i, "REPLACELEGACYPOINTBLOCKS");
        self.point_tools.legacy_job = Some(LegacyJob { queue: queue.into(), all: None });
        self.run_legacy_job();
    }

    /// Replaces blocks until one's number is taken and no answer applies to
    /// all; that one asks Duplicate Point Number.
    fn run_legacy_job(&mut self) {
        let i = self.active_tab;
        loop {
            let Some(job) = self.point_tools.legacy_job.as_mut() else { return };
            let Some(item) = job.queue.front().cloned() else { break };
            let all = job.all;
            let taken = cogo_point_entity(&self.tabs[i].scene.document, item.number).is_some();
            if taken && all.is_none() {
                self.point_tools.duplicate = Some(DuplicatePointWindow {
                    number: item.number,
                    resolution: Resolution::UseNext,
                    offset: "1".into(),
                    sequence: "100".into(),
                    apply_all: true,
                });
                self.active_modal = Some(crate::app::ModalKind::DuplicatePoint);
                return;
            }
            if let Some(job) = self.point_tools.legacy_job.as_mut() {
                job.queue.pop_front();
            }
            match (taken, all) {
                (true, Some((r, value, _))) => self.settle_legacy(i, &item, r, value),
                _ => self.write_legacy(i, &item, Some(item.number)),
            }
        }
        self.point_tools.legacy_job = None;
    }

    /// A legacy block's COGO point, numbered `number` (`None`: the next free
    /// number); the block goes.
    fn write_legacy(&mut self, i: usize, item: &Legacy, number: Option<u32>) {
        let layer = point_layer(&self.tabs[i].scene.document, self.tabs[i].active_layer.clone());
        let scene = &mut self.tabs[i].scene;
        let at = document::types::Vector3::new(item.position[0], item.position[1], item.position[2]);
        match c3d::add_cogo_point(&mut scene.document, number, at, item.description.as_deref(), None, Some(&layer)) {
            Ok(h) => {
                scene.bump_entities(&[(h, crate::scene::ChangeKind::Added)]);
                scene.erase_entities(&[item.insert]);
            }
            Err(e) => self.command_line.push_error(&e.to_string()),
        }
        self.tabs[i].dirty = true;
    }

    /// A taken number settled as `r` (`value`: the offset or the sequence's
    /// next number).
    fn settle_legacy(&mut self, i: usize, item: &Legacy, r: Resolution, value: u32) {
        let doc = &self.tabs[i].scene.document;
        let used: Vec<u32> = c3d::cogo_points(doc).iter().map(|p| p.number).collect();
        match r {
            Resolution::UseNext => self.write_legacy(i, item, None),
            Resolution::AddOffset => {
                let step = value.max(1);
                let mut n = item.number.saturating_add(step);
                while used.contains(&n) {
                    n = n.saturating_add(step);
                }
                self.write_legacy(i, item, Some(n));
            }
            Resolution::SequenceFrom => {
                let mut n = value.max(1);
                while used.contains(&n) {
                    n += 1;
                }
                if let Some((_, v, _)) = self.point_tools.legacy_job.as_mut().and_then(|j| j.all.as_mut()) {
                    *v = n + 1;
                }
                self.write_legacy(i, item, Some(n));
            }
            Resolution::Overwrite | Resolution::Merge => {
                let Some(h) = cogo_point_entity(doc, item.number) else { return self.write_legacy(i, item, None) };
                let old = c3d::cogo_points(doc).into_iter().find(|p| p.number == item.number);
                let mut at = document::types::Vector3::new(item.position[0], item.position[1], item.position[2]);
                let mut description = item.description.clone();
                // Merge fills in only what the existing point lacks.
                if let (Resolution::Merge, Some(old)) = (r, old) {
                    let z = if crate::entities::civil::has_elevation(old.position.z) { old.position.z } else { at.z };
                    at = document::types::Vector3::new(old.position.x, old.position.y, z);
                    description = old.description.clone().filter(|d| !d.is_empty()).or(description);
                }
                let scene = &mut self.tabs[i].scene;
                match c3d::set_cogo_point(&mut scene.document, h, item.number, at, description.as_deref()) {
                    Ok(()) => {
                        scene.bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
                        scene.erase_entities(&[item.insert]);
                    }
                    Err(e) => self.command_line.push_error(&e.to_string()),
                }
                self.tabs[i].dirty = true;
            }
        }
    }

    pub(in crate::app) fn on_point_tool(&mut self, message: PointToolMsg) -> Task<Message> {
        let i = self.active_tab;
        if let Some(w) = self.point_tools.corridor.as_mut() {
            w.error = None;
        }
        if let Some(w) = self.point_tools.blocks.as_mut() {
            w.error = None;
        }
        let corridor = self.point_tools.corridor.as_mut();
        match message {
            PointToolMsg::UserRange(on) => {
                if let Some(w) = corridor {
                    w.user_range = on;
                }
            }
            PointToolMsg::Baseline(name) => {
                if let Some(w) = corridor {
                    if let Some(k) = w.baselines.iter().position(|b| b.0 == name) {
                        w.baseline = k;
                        w.start = crate::modules::civil::point_alignment::format_station(w.baselines[k].1);
                        w.end = crate::modules::civil::point_alignment::format_station(w.baselines[k].2);
                    }
                }
            }
            PointToolMsg::Start(s) => {
                if let Some(w) = corridor {
                    w.start = s;
                }
            }
            PointToolMsg::End(s) => {
                if let Some(w) = corridor {
                    w.end = s;
                }
            }
            PointToolMsg::GroupName(s) => {
                if let Some(w) = corridor {
                    w.group_name = s;
                }
            }
            PointToolMsg::Code(k, on) => {
                if let Some(c) = corridor.and_then(|w| w.codes.get_mut(k)) {
                    c.1 = on;
                }
            }
            PointToolMsg::CorridorOk => self.corridor_points_ok(),
            PointToolMsg::PickBaseline | PointToolMsg::PickStation(_) => {
                use crate::command::CadCommand;
                let station = match message {
                    PointToolMsg::PickStation(end) => Some(end),
                    _ => None,
                };
                self.active_modal = None;
                let c = crate::modules::civil::point_pick::CorridorRangePickCommand::new(station);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            PointToolMsg::Spatial(s) => {
                if let Some(w) = self.point_tools.blocks.as_mut() {
                    w.spatial = s;
                }
            }
            PointToolMsg::PickWindow => {
                use crate::command::CadCommand;
                self.active_modal = None;
                let c = crate::modules::civil::point_pick::WindowPickCommand::default();
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            PointToolMsg::BlockGroup(k, on) => {
                if let Some(g) = self.point_tools.blocks.as_mut().and_then(|w| w.groups.get_mut(k)) {
                    g.1 = on;
                }
            }
            PointToolMsg::UseExisting(on) => {
                if let Some(w) = self.point_tools.blocks.as_mut() {
                    // With no block to use, only a new one can be made.
                    w.use_existing = on && !w.blocks.is_empty();
                }
            }
            PointToolMsg::Existing(name) => {
                if let Some(w) = self.point_tools.blocks.as_mut() {
                    w.existing = w.blocks.iter().position(|b| *b == name).unwrap_or(w.existing);
                }
            }
            PointToolMsg::NewName(s) => {
                if let Some(w) = self.point_tools.blocks.as_mut() {
                    w.new_name = s;
                }
            }
            PointToolMsg::Layer(s) => {
                if let Some(w) = self.point_tools.blocks.as_mut() {
                    w.layer = s;
                }
            }
            PointToolMsg::BlocksOk => self.point_blocks_ok(),
            PointToolMsg::BlockSkip | PointToolMsg::BlockOverwrite => {
                self.active_modal = None;
                let overwrite = matches!(message, PointToolMsg::BlockOverwrite);
                if let Some((p, old)) = self.point_tools.block_job.as_mut().and_then(|j| j.pending.take()) {
                    if overwrite {
                        self.tabs[i].scene.erase_entities(&[old]);
                        self.insert_point_block(i, &p);
                    }
                }
                self.run_block_job();
            }
            PointToolMsg::Resolution(label) => {
                if let Some(w) = self.point_tools.duplicate.as_mut() {
                    w.resolution = Resolution::ALL.into_iter().find(|r| crate::t!(r.label()) == label.as_str()).unwrap_or(w.resolution);
                }
            }
            PointToolMsg::Offset(s) => {
                if let Some(w) = self.point_tools.duplicate.as_mut() {
                    w.offset = s;
                }
            }
            PointToolMsg::Sequence(s) => {
                if let Some(w) = self.point_tools.duplicate.as_mut() {
                    w.sequence = s;
                }
            }
            PointToolMsg::ApplyAll(on) => {
                if let Some(w) = self.point_tools.duplicate.as_mut() {
                    w.apply_all = on;
                }
            }
            PointToolMsg::DuplicateOk => {
                let Some(w) = self.point_tools.duplicate.take() else { return Task::none() };
                self.active_modal = None;
                let value = match w.resolution {
                    Resolution::AddOffset => w.offset.trim().parse::<u32>().unwrap_or(1),
                    Resolution::SequenceFrom => w.sequence.trim().parse::<u32>().unwrap_or(100),
                    _ => 0,
                };
                let item = self.point_tools.legacy_job.as_mut().and_then(|j| j.queue.pop_front());
                if let (true, Some(job)) = (w.apply_all, self.point_tools.legacy_job.as_mut()) {
                    job.all = Some((w.resolution, value, value));
                }
                if let Some(item) = item {
                    self.settle_legacy(i, &item, w.resolution, value);
                }
                self.run_legacy_job();
            }
            PointToolMsg::DismissError => {}
            PointToolMsg::Help(text) => self.command_line.push_info(crate::t!(text).as_ref()),
        }
        Task::none()
    }
}
