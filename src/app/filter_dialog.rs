//! FILTER: the advanced selection filter (#2611). All / any of its rows,
//! groups one level deep with their own all / any, NOT per row, the filter
//! read back as a sentence and a live result by type and layer. It runs on
//! the same engine as Quick Select, whose "Advanced…" opens it with the
//! Quick Select conditions as groups.

use super::{Message, OpenCADStudio, QSelectPropertyChoice, QSelectResult, QSelectScope};
use crate::filter::{Condition, Filter, Group, Item, Match, Op};
use document::Handle;
use iced::Task;
use std::borrow::Cow;

/// A row of the editor: one at the root, or one inside a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterRow {
    Root(usize),
    Group(usize, usize),
}

/// A change to one row.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterRowEdit {
    Not(bool),
    Type(Option<String>),
    Property(Option<String>),
    Op(Op),
    Value(String),
    Remove,
}

/// A line of the live result, clicked to show only those objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterBucket {
    Type(String),
    Layer(String),
}

#[derive(Clone, Debug)]
pub enum FilterMsg {
    /// FILTER, or Quick Select's "Advanced…" with its filter.
    Open(Option<Box<FilterSeed>>),
    Close,
    Root(Match),
    GroupMode(usize, Match),
    GroupRemove(usize),
    /// A new row at the root, or in group `n`.
    AddCondition(Option<usize>),
    AddGroup,
    Edit(FilterRow, FilterRowEdit),
    Scope(QSelectScope),
    Result(QSelectResult),
    Invert(bool),
    Show(bool),
    Focus(Option<FilterBucket>),
    Apply,
    /// Load a saved filter from the drawing.
    Saved(String),
    /// Save over the loaded saved filter.
    Save,
    /// Start (`Some("")`), type (`Some(name)`) or drop (`None`) a new name.
    SaveAs(Option<String>),
    SaveAsCommit,
    /// Delete the loaded saved filter from the drawing.
    DeleteSaved,
    /// Show the result's layers in the Layer manager.
    ShowLayers,
    /// Layer mode: the name the layer filter is saved under.
    LayerName(String),
    /// Layer mode: keep the filter as a LAYERFILTER filter.
    SaveLayerFilter,
    /// Layer mode: the filter to sit under ("" for the top).
    LayerParent(String),
    /// Hide the dialog and pick an object to take rows from.
    PickObject,
    /// Layer mode: rows from this layer's properties, to tick.
    FromLayer(String),
    /// Window…: pick the objects to search in.
    Window,
    /// Write the drawing's saved filters to a file / read them from one.
    Export,
    Import,
    #[cfg(not(target_arch = "wasm32"))]
    ExportTo(std::path::PathBuf),
    #[cfg(not(target_arch = "wasm32"))]
    ImportFrom(std::path::PathBuf),
    /// Layer mode: FILTER for the objects on the layers found.
    ObjectsOnLayers,
    /// Tick or untick a picked object's property.
    SampleToggle(usize),
    /// The ticked properties become rows.
    SampleAdd,
    SampleCancel,
}

/// What Quick Select hands over to FILTER.
#[derive(Clone, Debug)]
pub struct FilterSeed {
    pub filter: Filter,
    pub scope: QSelectScope,
    pub result: QSelectResult,
    pub invert: bool,
    pub show: bool,
}

#[derive(Clone, Debug)]
pub struct FilterDialogState {
    pub filter: Filter,
    pub scope: QSelectScope,
    pub result: QSelectResult,
    pub invert: bool,
    pub show: bool,
    pub candidate_count: usize,
    pub live_count: usize,
    pub by_type: Vec<(String, usize)>,
    pub by_layer: Vec<(String, usize)>,
    /// Only these of the matches are highlighted.
    pub focus: Option<FilterBucket>,
    /// Object types in the scope with their counts.
    pub types: Vec<(String, usize)>,
    /// Properties offered per type (`None` = any type), filled as needed.
    pub properties: Vec<(Option<String>, Vec<QSelectPropertyChoice>)>,
    /// The drawing's saved filters, and the one loaded.
    pub saved: Vec<String>,
    pub saved_name: Option<String>,
    /// A new name being typed for Save as.
    pub save_as: Option<String>,
    /// Editing a layer filter for the Layer manager (#2610): the name it is
    /// saved under. Rows read layer properties, and there is no type.
    pub layer_filter: Option<String>,
    /// The layer filter's name when the editor opened, so a renamed filter
    /// replaces the one it was.
    pub layer_filter_from: Option<String>,
    /// Opened with 'FILTER inside this command's "Select objects:".
    pub for_command: Option<String>,
    /// Layer mode: the filter this one sits under (`None`: at the top), and
    /// the filters it may go under.
    pub layer_filter_parent: Option<String>,
    pub layer_filter_parents: Vec<String>,
    /// A picked object's properties, each ticked to become a row.
    pub sample: Option<super::QSelectSample>,
    /// Layer mode: what makes each found layer match, beside it.
    pub layer_reasons: Vec<String>,
    /// Layer mode: every layer, for From layer.
    pub all_layers: Vec<String>,
}

impl FilterDialogState {
    pub fn properties_for(&self, type_name: Option<&str>) -> &[QSelectPropertyChoice] {
        self.properties
            .iter()
            .find(|(t, _)| t.as_deref() == type_name)
            .map(|(_, list)| list.as_slice())
            .unwrap_or(&[])
    }

    pub fn condition(&self, row: FilterRow) -> Option<&Condition> {
        match row {
            FilterRow::Root(i) => match self.filter.items.get(i)? {
                Item::Condition(c) => Some(c),
                Item::Group(_) => None,
            },
            FilterRow::Group(g, i) => match self.filter.items.get(g)? {
                Item::Group(group) => group.conditions.get(i),
                Item::Condition(_) => None,
            },
        }
    }

    fn condition_mut(&mut self, row: FilterRow) -> Option<&mut Condition> {
        match row {
            FilterRow::Root(i) => match self.filter.items.get_mut(i)? {
                Item::Condition(c) => Some(c),
                Item::Group(_) => None,
            },
            FilterRow::Group(g, i) => match self.filter.items.get_mut(g)? {
                Item::Group(group) => group.conditions.get_mut(i),
                Item::Condition(_) => None,
            },
        }
    }

    /// Why Select can't run yet, if it can't: a number property compared
    /// with something that isn't a number.
    pub fn problem(&self) -> Option<String> {
        let conditions = self.filter.items.iter().flat_map(|item| match item {
            Item::Condition(c) => std::slice::from_ref(c).iter(),
            Item::Group(group) => group.conditions.iter(),
        });
        for condition in conditions {
            let Some(property) = condition.property.as_deref() else {
                continue;
            };
            if condition.op == Op::Any {
                continue;
            }
            let number = self
                .properties_for(condition.type_filter.as_deref())
                .iter()
                .find(|p| p.field == property)
                .is_some_and(|p| matches!(p.editor, super::QSelectValueEditor::Number));
            if number
                && !crate::filter::compare::is_pattern(&condition.value)
                && crate::entities::common::parse_f64(&condition.value).is_none()
            {
                return Some(crate::t!("Enter a valid number.").into_owned());
            }
        }
        None
    }
}

/// A new row: over objects, any type; over layers there is no type, so it
/// starts on the name (#2610 mockup).
fn new_row(state: &FilterDialogState) -> Condition {
    if state.layer_filter.is_some() {
        // "Name = *": every layer until the row is narrowed.
        Condition {
            property: Some("name".into()),
            value: "*".into(),
            ..Default::default()
        }
    } else {
        Condition::default()
    }
}

/// Live result lines shown per kind.
const RESULT_LINES: usize = 6;

/// The types a filter's rows name, each once.
fn sample_type_of(filter: &Filter) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    let mut note = |c: &Condition| {
        if let Some(t) = &c.type_filter {
            if !types.contains(t) {
                types.push(t.clone());
            }
        }
    };
    for item in &filter.items {
        match item {
            Item::Condition(c) => note(c),
            Item::Group(g) => g.conditions.iter().for_each(&mut note),
        }
    }
    types
}

/// Rows from a picked object go into `filter` (#2611): into an empty filter
/// as its rows; beside rows of the same type (or none) as more rows; next
/// to rows of another type as a group of their own, the rows already there
/// becoming a group too and the filter matching any of them.
/// Returns false, leaving the filter as it is, when the rows cannot join it
/// without making it match nothing.
pub fn add_sample(filter: &mut Filter, rows: Vec<Condition>) -> bool {
    if rows.is_empty() {
        return true;
    }
    let blank = |c: &Condition| c.property.is_none() && c.type_filter.is_none() && !c.negate;
    let empty = filter.items.iter().all(|item| match item {
        Item::Condition(c) => blank(c),
        Item::Group(g) => g.conditions.iter().all(blank),
    });
    if empty {
        filter.root = Match::All;
        filter.items = rows.into_iter().map(Item::Condition).collect();
        return true;
    }
    let sample_type = rows.iter().find_map(|c| c.type_filter.clone());
    let has_groups = filter
        .items
        .iter()
        .any(|item| matches!(item, Item::Group(_)));
    let types = sample_type_of(filter);
    let same_type = types.iter().all(|t| Some(t) == sample_type.as_ref());
    if filter.root == Match::All && !has_groups && same_type {
        for row in rows {
            if !filter
                .items
                .iter()
                .any(|item| matches!(item, Item::Condition(c) if *c == row))
            {
                filter.items.push(Item::Condition(row));
            }
        }
        return true;
    }
    // Groups do not nest: under "all of" with groups already there, another
    // type's group would have to match both types at once.
    if filter.root == Match::All && has_groups && !same_type {
        return false;
    }
    if filter.root == Match::All && !has_groups {
        let before: Vec<Condition> = filter
            .items
            .drain(..)
            .filter_map(|item| match item {
                Item::Condition(c) if !blank(&c) => Some(c),
                _ => None,
            })
            .collect();
        filter.items.push(Item::Group(Group {
            mode: Match::All,
            conditions: before,
        }));
        filter.root = Match::Any;
    }
    filter.items.push(Item::Group(Group {
        mode: Match::All,
        conditions: rows,
    }));
    true
}

/// Window…: Quick Select or FILTER steps aside while the objects to search
/// in are picked (a window, a crossing, picks); Enter brings the dialog
/// back with them as the selection and "Selection" as the scope (#2611
/// mockup: "Searching only the N selected objects").
struct ScopeWindowCommand {
    qselect: Option<Box<super::QSelectState>>,
    filter: Option<Box<FilterDialogState>>,
    tab_id: u64,
    picked: bool,
}

impl ScopeWindowCommand {
    fn finish(&mut self) -> crate::command::CmdResult {
        use crate::command::CmdResult;
        let picked = self.picked;
        if let Some(mut state) = self.qselect.take() {
            if picked {
                state.scope = QSelectScope::CurrentSelection;
            }
            return CmdResult::QSelectSample {
                state,
                tab_id: self.tab_id,
                handle: None,
            };
        }
        if let Some(mut state) = self.filter.take() {
            if picked {
                state.scope = QSelectScope::CurrentSelection;
            }
            return CmdResult::FilterSample {
                state,
                tab_id: self.tab_id,
                handle: None,
            };
        }
        CmdResult::Cancel
    }
}

impl crate::command::CadCommand for ScopeWindowCommand {
    fn name(&self) -> &'static str {
        if self.qselect.is_some() {
            "QSELECT"
        } else {
            "FILTER"
        }
    }
    fn prompt(&self) -> String {
        format!("{}  {}", self.name(), crate::t!("Select objects:"))
    }
    fn is_selection_gathering(&self) -> bool {
        true
    }
    // Windows and picks add up in the selection until Enter, as at any
    // "Select objects:" prompt.
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> crate::command::CmdResult {
        self.picked |= !handles.is_empty();
        crate::command::CmdResult::NeedPoint
    }
    fn on_point(&mut self, _point: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        self.finish()
    }
    fn on_escape(&mut self) -> crate::command::CmdResult {
        self.picked = false;
        self.finish()
    }
}

/// Export / Import: the file to write the saved filters to, or read from.
fn pick_filters_file(save: bool) -> Task<Message> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Task::perform(
            crate::io::pick_selection_filters_path(save),
            move |path| match path {
                Some(path) if save => Message::FilterDialog(FilterMsg::ExportTo(path)),
                Some(path) => Message::FilterDialog(FilterMsg::ImportFrom(path)),
                None => Message::Noop,
            },
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = save;
        Task::none()
    }
}

/// The pick behind FILTER's From object: the dialog waits in the command
/// and comes back with the object, or without one on Enter or Esc.
struct FilterSampleCommand {
    state: Option<Box<FilterDialogState>>,
    tab_id: u64,
}

impl FilterSampleCommand {
    fn finish(&mut self, handle: Option<Handle>) -> crate::command::CmdResult {
        match self.state.take() {
            Some(state) => crate::command::CmdResult::FilterSample {
                state,
                tab_id: self.tab_id,
                handle,
            },
            None => crate::command::CmdResult::Cancel,
        }
    }
}

impl crate::command::CadCommand for FilterSampleCommand {
    fn name(&self) -> &'static str {
        "FILTER"
    }
    fn prompt(&self) -> String {
        format!("FILTER  {}", crate::t!("Select object:"))
    }
    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }
    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }
    fn on_entity_pick(&mut self, handle: Handle, _point: glam::DVec3) -> crate::command::CmdResult {
        if handle.is_null() {
            crate::command::CmdResult::NeedPoint
        } else {
            self.finish(Some(handle))
        }
    }
    fn on_point(&mut self, _point: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        self.finish(None)
    }
    fn on_escape(&mut self) -> crate::command::CmdResult {
        self.finish(None)
    }
}

impl OpenCADStudio {
    pub(super) fn on_filter_message(&mut self, msg: FilterMsg) -> Task<Message> {
        match msg {
            FilterMsg::PickObject => return self.filter_pick_object(),
            FilterMsg::Window => return self.scope_window_pick(false),
            FilterMsg::Export => return pick_filters_file(true),
            FilterMsg::Import => return pick_filters_file(false),
            #[cfg(not(target_arch = "wasm32"))]
            FilterMsg::ExportTo(path) => {
                self.export_saved_filters(&path);
                return Task::none();
            }
            #[cfg(not(target_arch = "wasm32"))]
            FilterMsg::ImportFrom(path) => {
                self.import_saved_filters(&path);
                return Task::none();
            }
            FilterMsg::FromLayer(name) => {
                self.filter_sample_layer(&name);
                return Task::none();
            }
            FilterMsg::ObjectsOnLayers => {
                let names: Vec<String> = self
                    .filter_dialog
                    .as_ref()
                    .map(|state| state.by_layer.iter().map(|(n, _)| n.clone()).collect())
                    .unwrap_or_default();
                self.filter_dialog = None;
                self.filter_open_for_layers(names.join(","));
                return Task::none();
            }
            FilterMsg::SampleToggle(index) => {
                if let Some(row) = self
                    .filter_dialog
                    .as_mut()
                    .and_then(|state| state.sample.as_mut())
                    .and_then(|sample| sample.rows.get_mut(index))
                {
                    row.1 = !row.1;
                }
                return Task::none();
            }
            FilterMsg::SampleCancel => {
                if let Some(state) = self.filter_dialog.as_mut() {
                    state.sample = None;
                }
                return Task::none();
            }
            FilterMsg::SampleAdd => {
                let types_now = if let Some(state) = self.filter_dialog.as_mut() {
                    if let Some(sample) = state.sample.take() {
                        let ticked: Vec<Condition> = sample
                            .rows
                            .iter()
                            .filter(|(_, ticked)| *ticked)
                            .map(|(condition, _)| Condition::from(condition))
                            .collect();
                        if !add_sample(&mut state.filter, ticked) {
                            self.command_line.push_error(
                                crate::t!(
                                    "An object of another type cannot join a filter that needs all of its groups. Set the filter to \"any of these\" first."
                                )
                                .as_ref(),
                            );
                        }
                    }
                    Some(sample_type_of(&state.filter))
                } else {
                    None
                };
                for t in types_now.into_iter().flatten() {
                    self.filter_load_properties(Some(t));
                }
                self.filter_refresh_scope();
                self.filter_refresh_live();
                return Task::none();
            }
            FilterMsg::Open(seed) => {
                self.filter_open(seed.map(|seed| *seed));
                return Task::none();
            }
            FilterMsg::Close => {
                let layers = self
                    .filter_dialog
                    .take()
                    .is_some_and(|state| state.layer_filter.is_some());
                self.reset_modal_geometry();
                self.filter_refresh_live();
                if layers {
                    return Task::none();
                }
                if self.transparent_select.is_some() {
                    return self.finish_transparent_select(None);
                }
                self.return_to_block_dialog(false);
                return Task::none();
            }
            FilterMsg::Apply => return self.filter_apply(),
            FilterMsg::SaveLayerFilter => {
                self.save_layer_filter();
                return Task::none();
            }
            FilterMsg::ShowLayers => {
                self.filter_show_layers();
                // The dialog is gone: a parked command or block dialog comes
                // back as it does on Close.
                if self.transparent_select.is_some() {
                    return self.finish_transparent_select(None);
                }
                self.return_to_block_dialog(false);
                return Task::none();
            }
            FilterMsg::Saved(name) => {
                let i = self.active_tab;
                if let Some((saved, filter)) =
                    super::saved_filters::find_saved_filter(&self.tabs[i].scene.document, &name)
                {
                    if let Some(state) = self.filter_dialog.as_mut() {
                        state.filter = filter;
                        state.saved_name = Some(saved);
                        state.save_as = None;
                    }
                }
                self.filter_refresh_scope();
                self.filter_refresh_live();
                return Task::none();
            }
            FilterMsg::Save => {
                let target = self
                    .filter_dialog
                    .as_ref()
                    .and_then(|state| state.saved_name.clone());
                match target {
                    Some(name) => self.filter_save(name),
                    None => {
                        if let Some(state) = self.filter_dialog.as_mut() {
                            state.save_as = Some(String::new());
                        }
                    }
                }
                return Task::none();
            }
            FilterMsg::SaveAsCommit => {
                let name = self
                    .filter_dialog
                    .as_ref()
                    .and_then(|state| state.save_as.as_deref())
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
                if let Some(name) = name {
                    self.filter_save(name);
                }
                return Task::none();
            }
            FilterMsg::DeleteSaved => {
                let i = self.active_tab;
                let Some(name) = self
                    .filter_dialog
                    .as_ref()
                    .and_then(|state| state.saved_name.clone())
                else {
                    return Task::none();
                };
                self.edit_saved_filters(i, "FILTER DELETE", |document| {
                    super::saved_filters::delete_saved_filter(document, &name);
                });
                let saved = self.saved_filter_names(i);
                if let Some(state) = self.filter_dialog.as_mut() {
                    state.saved = saved;
                    state.saved_name = None;
                }
                return Task::none();
            }
            _ => {}
        }
        let Some(state) = self.filter_dialog.as_mut() else {
            return Task::none();
        };
        let mut types_changed = None;
        match msg {
            FilterMsg::Root(mode) => state.filter.root = mode,
            FilterMsg::GroupMode(g, mode) => {
                if let Some(Item::Group(group)) = state.filter.items.get_mut(g) {
                    group.mode = mode;
                }
            }
            FilterMsg::GroupRemove(g) => {
                if g < state.filter.items.len() {
                    state.filter.items.remove(g);
                }
            }
            FilterMsg::AddCondition(None) => {
                let row = new_row(state);
                state.filter.items.push(Item::Condition(row));
            }
            FilterMsg::AddCondition(Some(g)) => {
                let row = new_row(state);
                if let Some(Item::Group(group)) = state.filter.items.get_mut(g) {
                    group.conditions.push(row);
                }
            }
            FilterMsg::AddGroup => {
                let row = new_row(state);
                state.filter.items.push(Item::Group(Group {
                    mode: Match::Any,
                    conditions: vec![row],
                }))
            }
            FilterMsg::Edit(row, FilterRowEdit::Remove) => match row {
                FilterRow::Root(i) if i < state.filter.items.len() => {
                    state.filter.items.remove(i);
                }
                FilterRow::Group(g, i) => {
                    if let Some(Item::Group(group)) = state.filter.items.get_mut(g) {
                        if i < group.conditions.len() {
                            group.conditions.remove(i);
                        }
                        if group.conditions.is_empty() {
                            state.filter.items.remove(g);
                        }
                    }
                }
                FilterRow::Root(_) => {}
            },
            FilterMsg::Edit(row, edit) => {
                let Some(condition) = state.condition_mut(row) else {
                    return Task::none();
                };
                match edit {
                    FilterRowEdit::Not(on) => condition.negate = on,
                    FilterRowEdit::Type(t) => {
                        condition.type_filter = t.clone();
                        condition.property = None;
                        condition.op = Op::Eq;
                        condition.value.clear();
                        types_changed = Some(t);
                    }
                    FilterRowEdit::Property(p) => {
                        condition.property = p;
                        condition.op = Op::Eq;
                        condition.value.clear();
                    }
                    FilterRowEdit::Op(op) => condition.op = op,
                    FilterRowEdit::Value(v) => condition.value = v,
                    FilterRowEdit::Remove => {}
                }
            }
            FilterMsg::Scope(scope) => {
                state.scope = scope;
                state.properties.clear();
            }
            // Neither changes what matches: nothing to count again.
            FilterMsg::Result(result) => {
                state.result = result;
                return Task::none();
            }
            FilterMsg::Invert(on) => state.invert = on,
            FilterMsg::Show(on) => state.show = on,
            FilterMsg::Focus(bucket) => {
                state.focus = if state.focus == bucket { None } else { bucket };
            }
            FilterMsg::SaveAs(name) => {
                state.save_as = name;
                return Task::none();
            }
            FilterMsg::LayerName(name) => {
                state.layer_filter = Some(name);
                return Task::none();
            }
            FilterMsg::LayerParent(parent) => {
                state.layer_filter_parent = (!parent.is_empty()).then_some(parent);
                return Task::none();
            }
            FilterMsg::Open(_)
            | FilterMsg::Close
            | FilterMsg::Apply
            | FilterMsg::Saved(_)
            | FilterMsg::Save
            | FilterMsg::SaveAsCommit
            | FilterMsg::DeleteSaved
            | FilterMsg::SaveLayerFilter
            | FilterMsg::LayerParent(_)
            | FilterMsg::ShowLayers
            | FilterMsg::PickObject
            | FilterMsg::FromLayer(_)
            | FilterMsg::Window
            | FilterMsg::Export
            | FilterMsg::Import
            | FilterMsg::ObjectsOnLayers
            | FilterMsg::SampleToggle(_)
            | FilterMsg::SampleAdd
            | FilterMsg::SampleCancel => {}
            // Files only on the desktop.
            #[cfg(not(target_arch = "wasm32"))]
            FilterMsg::ExportTo(_) | FilterMsg::ImportFrom(_) => {}
        }
        if let Some(t) = types_changed {
            self.filter_load_properties(t);
        }
        self.filter_refresh_scope();
        self.filter_refresh_live();
        Task::none()
    }

    /// Window…: the open Quick Select (`qselect`) or FILTER steps aside while
    /// the objects to search in are picked.
    pub(in crate::app) fn scope_window_pick(&mut self, qselect: bool) -> Task<Message> {
        let i = self.active_tab;
        let (qselect, filter) = if qselect {
            match self.qselect.take() {
                Some(state) => (Some(Box::new(state)), None),
                None => return Task::none(),
            }
        } else {
            match self.filter_dialog.take() {
                Some(state) if state.layer_filter.is_none() => (None, Some(Box::new(state))),
                Some(state) => {
                    self.filter_dialog = Some(state);
                    return Task::none();
                }
                None => return Task::none(),
            }
        };
        self.tabs[i].scene.set_filter_preview_highlights(&[]);
        // The objects picked now are the ones searched.
        self.tabs[i].scene.deselect_all();
        let command = ScopeWindowCommand {
            qselect,
            filter,
            tab_id: self.tabs[i].id,
            picked: false,
        };
        use crate::command::CadCommand;
        self.active_modal = None;
        self.command_line.input.clear();
        self.command_line.set_step_options(Vec::new());
        self.command_line.push_info(&command.prompt());
        self.tabs[i].scene.selection.borrow_mut().menu.close();
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// Export: the drawing's saved filters into a file, to bring into
    /// another drawing with Import.
    #[cfg(not(target_arch = "wasm32"))]
    fn export_saved_filters(&mut self, path: &std::path::Path) {
        let i = self.active_tab;
        let filters: Vec<serde_json::Value> =
            super::saved_filters::saved_filters(&self.tabs[i].scene.document)
                .into_iter()
                .map(|(name, filter)| serde_json::json!({ "name": name, "filter": filter }))
                .collect();
        let text = serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "filters": filters }))
            .unwrap_or_default();
        match std::fs::write(path, text) {
            Ok(()) => self
                .command_line
                .push_output(crate::tf!("{} saved filter(s) exported.", filters.len()).as_ref()),
            Err(why) => self
                .command_line
                .push_error(crate::tf!("Export saved filters: {}.", why).as_ref()),
        }
    }

    /// Import: saved filters from a file into the drawing, one undo step; a
    /// filter of the same name is replaced.
    #[cfg(not(target_arch = "wasm32"))]
    fn import_saved_filters(&mut self, path: &std::path::Path) {
        let i = self.active_tab;
        let parsed: Option<Vec<(String, Filter)>> = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| {
                value.get("filters")?.as_array().map(|list| {
                    list.iter()
                        .filter_map(|item| {
                            // The names Save as accepts: trimmed, not empty.
                            let name = item.get("name")?.as_str()?.trim().to_string();
                            if name.is_empty() {
                                return None;
                            }
                            let filter =
                                serde_json::from_value(item.get("filter")?.clone()).ok()?;
                            Some((name, filter))
                        })
                        .collect()
                })
            });
        let Some(filters) = parsed.filter(|f| !f.is_empty()) else {
            self.command_line
                .push_error(crate::t!("No saved filters found in that file.").as_ref());
            return;
        };
        let count = filters.len();
        self.edit_saved_filters(i, "FILTER IMPORT", |document| {
            for (name, filter) in &filters {
                super::saved_filters::store_saved_filter(document, name, filter);
            }
        });
        let names = self.saved_filter_names(i);
        if let Some(state) = self.filter_dialog.as_mut() {
            state.saved = names;
        }
        self.command_line
            .push_output(crate::tf!("{} saved filter(s) imported.", count).as_ref());
    }

    /// From object: the dialog steps aside while an object is picked.
    fn filter_pick_object(&mut self) -> Task<Message> {
        if self
            .filter_dialog
            .as_ref()
            .is_none_or(|state| state.layer_filter.is_some())
        {
            return Task::none();
        }
        let state = self.filter_dialog.take().expect("checked open dialog");
        let i = self.active_tab;
        self.tabs[i].scene.set_filter_preview_highlights(&[]);
        let command = FilterSampleCommand {
            state: Some(Box::new(state)),
            tab_id: self.tabs[i].id,
        };
        use crate::command::CadCommand;
        self.command_line.input.clear();
        self.command_line.set_step_options(Vec::new());
        self.command_line.push_info(&command.prompt());
        self.tabs[i].scene.selection.borrow_mut().menu.close();
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// The pick ended: the dialog is back, with the object's properties to
    /// tick when one was picked.
    pub(in crate::app) fn finish_filter_sample(
        &mut self,
        mut state: FilterDialogState,
        tab_id: u64,
        handle: Option<Handle>,
    ) {
        let i = self.active_tab;
        if self.tabs[i].id != tab_id {
            return;
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        if let Some(handle) = handle {
            state.sample = Some(self.sample_from_entity(i, handle));
        }
        self.filter_dialog = Some(state);
        self.filter_refresh_scope();
        self.filter_refresh_live();
    }

    fn filter_open(&mut self, seed: Option<FilterSeed>) {
        let i = self.active_tab;
        self.tabs[i].scene.selection.borrow_mut().menu.close();
        let seed = seed.unwrap_or(FilterSeed {
            filter: Filter {
                root: Match::All,
                items: vec![Item::Condition(Condition::default())],
            },
            scope: QSelectScope::CurrentSpace,
            result: QSelectResult::New,
            invert: false,
            show: false,
        });
        self.filter_dialog = Some(FilterDialogState {
            filter: seed.filter,
            scope: seed.scope,
            result: seed.result,
            invert: seed.invert,
            show: seed.show,
            candidate_count: 0,
            live_count: 0,
            by_type: Vec::new(),
            by_layer: Vec::new(),
            focus: None,
            types: Vec::new(),
            properties: Vec::new(),
            saved: self.saved_filter_names(i),
            saved_name: None,
            save_as: None,
            layer_filter: None,
            layer_filter_from: None,
            for_command: self.transparent_select.clone(),
            layer_filter_parent: None,
            layer_filter_parents: Vec::new(),
            sample: None,
            layer_reasons: Vec::new(),
            all_layers: Vec::new(),
        });
        self.reset_modal_geometry();
        self.filter_refresh_scope();
        self.filter_refresh_live();
    }

    /// FILTER over the objects with `layers` (a comma list) as its first row:
    /// the Layer manager's "Filter objects..." (#2610).
    pub(super) fn filter_open_for_layers(&mut self, layers: String) {
        self.filter_open(Some(FilterSeed {
            filter: Filter {
                root: Match::All,
                items: vec![Item::Condition(Condition {
                    property: Some("layer".into()),
                    op: Op::Eq,
                    value: layers,
                    ..Default::default()
                })],
            },
            scope: QSelectScope::CurrentSpace,
            result: QSelectResult::New,
            invert: false,
            show: false,
        }));
    }

    /// The layers of every object the filter finds, as the Layer manager's
    /// "Layers of the filter result" node.
    fn filter_show_layers(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.filter_dialog.take() else {
            return;
        };
        self.reset_modal_geometry();
        self.tabs[i].scene.set_filter_preview_highlights(&[]);
        let found = self.tabs[i]
            .scene
            .filter_matches(state.scope, &state.filter, state.invert);
        let mut layers: Vec<String> = found
            .iter()
            .filter_map(|h| self.tabs[i].scene.document.get_entity(*h))
            .map(|e| e.common().layer.clone())
            .collect();
        layers.sort_by_key(|name| name.to_lowercase());
        layers.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let panel = &mut self.tabs[i].layers;
        panel.found_layers = layers;
        panel.view = crate::ui::window::layers::TreeNode::Found;
        panel.invert = false;
        // The way LAYERS opens it: a pallet that lives in a window of its own
        // gets that window. Setting `show_layers` alone left it "open" with
        // no window, and LAYERS then took it as open already (#2610).
        if !self.show_layers {
            self.pallet_as_window(crate::ui::dock::PanelId::Layers);
        }
        self.open_layers_pallet();
    }

    /// FILTER over the layers, for a Layer manager filter named `name`.
    pub(super) fn filter_open_layers(&mut self, name: String, filter: Filter) {
        let parents_source = self.layer_filters.clone();
        self.filter_open(Some(FilterSeed {
            filter,
            scope: QSelectScope::CurrentSpace,
            result: QSelectResult::New,
            invert: false,
            show: false,
        }));
        if let Some(state) = self.filter_dialog.as_mut() {
            state.layer_filter_from = Some(name.clone()).filter(|name| !name.is_empty());
            state.layer_filter = Some(name);
            // Layers, not the objects a parked command asks for.
            state.for_command = None;
            // Where it sits, and where it may go: not under itself or a filter
            // under it.
            fn find_parent(
                list: &[super::layer_filters::LayerFilter],
                parent: Option<&str>,
                name: &str,
            ) -> Option<Option<String>> {
                for f in list {
                    if f.name.eq_ignore_ascii_case(name) {
                        return Some(parent.map(str::to_owned));
                    }
                    if let Some(found) = find_parent(&f.children, Some(&f.name), name) {
                        return Some(found);
                    }
                }
                None
            }
            fn names(
                list: &[super::layer_filters::LayerFilter],
                skip: &str,
                out: &mut Vec<String>,
            ) {
                for f in list {
                    if !skip.is_empty() && f.name.eq_ignore_ascii_case(skip) {
                        continue;
                    }
                    out.push(f.name.clone());
                    names(&f.children, skip, out);
                }
            }
            let current = state.layer_filter.clone().unwrap_or_default();
            state.layer_filter_parent = find_parent(&parents_source, None, &current).flatten();
            let mut options = Vec::new();
            names(&parents_source, &current, &mut options);
            state.layer_filter_parents = options;
            // Layers have no type: a blank row reads the name, any name
            // until a value is typed, so it still finds every layer.
            for item in &mut state.filter.items {
                if let Item::Condition(c) = item {
                    if c.property.is_none() && c.type_filter.is_none() {
                        c.property = Some("name".into());
                        if c.value.is_empty() {
                            c.op = Op::Any;
                        }
                    }
                }
            }
        }
        self.filter_refresh_scope();
        self.filter_refresh_live();
    }

    /// The layer properties the editor offers, for the layer mode.
    fn layer_filter_properties(&self) -> Vec<QSelectPropertyChoice> {
        use super::QSelectValueEditor as Editor;
        let i = self.active_tab;
        let mut linetypes = vec!["Continuous".to_string()];
        linetypes.extend(
            self.tabs[i]
                .scene
                .document
                .line_types
                .iter()
                .map(|lt| lt.name.clone())
                .filter(|name| {
                    // A layer's linetype is never ByLayer / ByBlock.
                    !["Continuous", "ByLayer", "ByBlock"]
                        .iter()
                        .any(|n| name.eq_ignore_ascii_case(n))
                }),
        );
        let yes_no = || Editor::Choice(vec!["1".into(), "0".into()]);
        let choice = |field: &str, label: Cow<'static, str>, editor| QSelectPropertyChoice {
            field: field.into(),
            label: label.into_owned(),
            editor,
        };
        vec![
            choice("name", crate::t!("Name"), Editor::Text),
            choice("description", crate::t!("Description"), Editor::Text),
            choice("color", crate::t!("Color"), Editor::Number),
            choice("linetype", crate::t!("Linetype"), Editor::Choice(linetypes)),
            choice("on", crate::t!("On"), yes_no()),
            choice("frozen", crate::t!("Freeze"), yes_no()),
            choice("locked", crate::t!("Lock"), yes_no()),
            choice("plot", crate::t!("Plot"), yes_no()),
            choice("xref", crate::t!("Xref-dependent"), yes_no()),
        ]
    }

    /// From layer: the layer's properties as rows to tick, its name and
    /// colour ticked (#2610 spec LM-22).
    fn filter_sample_layer(&mut self, name: &str) {
        use crate::filter::PropertySource;
        let i = self.active_tab;
        let Some(layer) = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return;
        };
        let source = super::layer_filters::LayerProperties;
        let rows = self
            .layer_filter_properties()
            .into_iter()
            .filter_map(|property| {
                let value = source.value(&layer, &property.field)?;
                let ticked = matches!(property.field.as_str(), "name" | "color");
                Some((
                    super::QSelectCondition {
                        type_filter: None,
                        property: Some(property),
                        operator: super::QSelectOp::Eq,
                        value,
                    },
                    ticked,
                ))
            })
            .collect();
        if let Some(state) = self.filter_dialog.as_mut() {
            state.sample = Some(super::QSelectSample {
                type_name: layer.name.clone(),
                rows,
            });
        }
    }

    /// Keep the edited filter as a LAYERFILTER filter and show it in the
    /// Layer manager's tree.
    fn save_layer_filter(&mut self) {
        let i = self.active_tab;
        self.load_layer_filters(i);
        let Some(state) = self.filter_dialog.as_ref() else {
            return;
        };
        let Some(name) = state
            .layer_filter
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
        else {
            return;
        };
        let conditions = Some(state.filter.clone());
        let from = state.layer_filter_from.clone();
        let matches = state.live_count;
        let parent = state.layer_filter_parent.clone();
        fn take(
            list: &mut Vec<super::layer_filters::LayerFilter>,
            name: &str,
        ) -> Option<super::layer_filters::LayerFilter> {
            if let Some(at) = list.iter().position(|f| f.name.eq_ignore_ascii_case(name)) {
                return Some(list.remove(at));
            }
            list.iter_mut().find_map(|f| take(&mut f.children, name))
        }
        fn under<'a>(
            list: &'a mut Vec<super::layer_filters::LayerFilter>,
            name: &str,
        ) -> Option<&'a mut Vec<super::layer_filters::LayerFilter>> {
            for f in list.iter_mut() {
                if f.name.eq_ignore_ascii_case(name) {
                    return Some(&mut f.children);
                }
                if let Some(found) = under(&mut f.children, name) {
                    return Some(found);
                }
            }
            None
        }
        // A renamed filter replaces the one it was, keeping its nested
        // filters; another filter of the new name gives way.
        let mut filter = from
            .as_deref()
            .and_then(|from| take(&mut self.layer_filters, from))
            .unwrap_or_default();
        if let Some(other) = take(&mut self.layer_filters, &name) {
            if filter.name.is_empty() {
                filter = other;
            }
        }
        filter.name = name.clone();
        filter.conditions = conditions;
        filter.pattern.clear();
        filter.predicates.clear();
        match parent
            .as_deref()
            .and_then(|p| under(&mut self.layer_filters, p))
        {
            Some(children) => children.push(filter),
            None => self.layer_filters.push(filter),
        }
        self.persist_layer_filters(i, "LAYERFILTER");
        self.filter_dialog = None;
        self.reset_modal_geometry();
        self.command_line.push_output(
            crate::tf!(
                "LAYERFILTER: '{}' saved ({} layer(s) match).",
                name,
                matches
            )
            .as_ref(),
        );
        self.refresh_layer_counts();
        self.tabs[i].layers.view = crate::ui::window::layers::TreeNode::Filter(name);
    }

    /// The scope's types and counts, and the properties of every type a row
    /// uses.
    fn filter_refresh_scope(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.filter_dialog.as_ref() else {
            return;
        };
        if state.layer_filter.is_some() {
            let properties = self.layer_filter_properties();
            let count = self.tabs[i].scene.document.layers.iter().count();
            if let Some(state) = self.filter_dialog.as_mut() {
                state.types.clear();
                state.properties = vec![(None, properties)];
                state.candidate_count = count;
            }
            return;
        }
        let scope = state.scope;
        let mut wanted: Vec<Option<String>> = vec![None];
        for item in &state.filter.items {
            let conditions = match item {
                Item::Condition(c) => std::slice::from_ref(c),
                Item::Group(group) => group.conditions.as_slice(),
            };
            for c in conditions {
                if !wanted.contains(&c.type_filter) {
                    wanted.push(c.type_filter.clone());
                }
            }
        }
        let missing: Vec<Option<String>> = wanted
            .into_iter()
            .filter(|t| !state.properties.iter().any(|(have, _)| have == t))
            .collect();
        let types = self.tabs[i].scene.qselect_type_counts(scope);
        let count = self.tabs[i].scene.qselect_candidate_count(scope);
        if let Some(state) = self.filter_dialog.as_mut() {
            state.types = types;
            state.candidate_count = count;
        }
        for t in missing {
            self.filter_load_properties(t);
        }
    }

    fn filter_load_properties(&mut self, type_name: Option<String>) {
        if self
            .filter_dialog
            .as_ref()
            .is_some_and(|state| state.layer_filter.is_some())
        {
            return;
        }
        let i = self.active_tab;
        let Some(scope) = self.filter_dialog.as_ref().map(|s| s.scope) else {
            return;
        };
        let list = self.tabs[i]
            .scene
            .qselect_properties(type_name.as_deref(), scope)
            .into_iter()
            .filter(|p| p.field != "handle")
            .collect();
        if let Some(state) = self.filter_dialog.as_mut() {
            state.properties.retain(|(t, _)| *t != type_name);
            state.properties.push((type_name, list));
        }
    }

    /// Count what the filter finds, by type and by layer, and highlight it
    /// (or only the focused line of it) when Show is on.
    pub(super) fn filter_refresh_live(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.filter_dialog.as_ref() else {
            self.tabs[i].scene.set_filter_preview_highlights(&[]);
            return;
        };
        if state.layer_filter.is_some() {
            // The layers the filter takes, with their object counts.
            let source = super::layer_filters::LayerProperties;
            let counts = &self.tabs[i].layers.counts;
            let layers: Vec<(String, usize)> = self.tabs[i]
                .scene
                .document
                .layers
                .iter()
                .filter(|layer| !layer.name.starts_with('*'))
                .filter(|layer| state.filter.matches(&source, *layer) != state.invert)
                .map(|layer| {
                    let objects = counts
                        .get(&layer.name.to_lowercase())
                        .map_or(0, |(n, _)| *n);
                    (layer.name.clone(), objects)
                })
                .collect();
            let all_layers: Vec<String> = self.tabs[i]
                .scene
                .document
                .layers
                .iter()
                .filter(|layer| !layer.name.starts_with('*'))
                .map(|layer| layer.name.clone())
                .collect();
            // Why each one is in: the first condition that is not the name,
            // as "Plot: No" (mockup), else its object count.
            let reason_field = state.filter.items.iter().find_map(|item| match item {
                Item::Condition(c) => c.property.clone().filter(|p| p != "name"),
                Item::Group(g) => g
                    .conditions
                    .iter()
                    .find_map(|c| c.property.clone().filter(|p| p != "name")),
            });
            let label = reason_field.as_ref().and_then(|field| {
                state
                    .properties_for(None)
                    .iter()
                    .find(|p| &p.field == field)
                    .map(|p| p.label.clone())
            });
            let reasons: Vec<String> = layers
                .iter()
                .map(|(name, objects)| {
                    let layer = self.tabs[i].scene.document.layers.get(name);
                    match (&reason_field, &label, layer) {
                        (Some(field), Some(label), Some(layer)) => {
                            use crate::filter::PropertySource;
                            let value = source.value(layer, field).unwrap_or_default();
                            let value = match value.as_str() {
                                "1" if field != "color" => crate::t!("Yes").into_owned(),
                                "0" if field != "color" => crate::t!("No").into_owned(),
                                _ => value,
                            };
                            format!("{label}: {value}")
                        }
                        _ => crate::tf!("{} objects", objects).into_owned(),
                    }
                })
                .collect();
            self.tabs[i].scene.set_filter_preview_highlights(&[]);
            if let Some(state) = self.filter_dialog.as_mut() {
                state.live_count = layers.len();
                state.by_type.clear();
                state.by_layer = layers;
                state.layer_reasons = reasons;
                state.all_layers = all_layers;
            }
            return;
        }
        let matches = self.tabs[i]
            .scene
            .filter_matches(state.scope, &state.filter, state.invert);
        let scene = &self.tabs[i].scene;
        let mut by_type = std::collections::BTreeMap::<String, usize>::new();
        let mut by_layer = std::collections::BTreeMap::<String, usize>::new();
        let mut focused: Vec<Handle> = Vec::new();
        for h in &matches {
            let Some(entity) = scene.document.get_entity(*h) else {
                continue;
            };
            let type_name = crate::entities::traits::entity_type_name(entity).to_string();
            let layer = entity.common().layer.clone();
            let in_focus = match &state.focus {
                None => true,
                Some(FilterBucket::Type(t)) => *t == type_name,
                Some(FilterBucket::Layer(l)) => l.eq_ignore_ascii_case(&layer),
            };
            if in_focus {
                focused.push(*h);
            }
            *by_type.entry(type_name).or_default() += 1;
            *by_layer.entry(layer).or_default() += 1;
        }
        let top = |counts: std::collections::BTreeMap<String, usize>| {
            let mut lines: Vec<(String, usize)> = counts.into_iter().collect();
            lines.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            lines.truncate(RESULT_LINES);
            lines
        };
        let show = state.show;
        let highlight = if show { focused } else { Vec::new() };
        self.tabs[i].scene.set_filter_preview_highlights(&highlight);
        if let Some(state) = self.filter_dialog.as_mut() {
            state.live_count = matches.len();
            state.by_type = top(by_type);
            state.by_layer = top(by_layer);
        }
    }

    /// Save the dialog's filter in the drawing as `name` (one undo step) and
    /// say where it went, without a popup.
    fn filter_save(&mut self, name: String) {
        let i = self.active_tab;
        let Some(filter) = self
            .filter_dialog
            .as_ref()
            .map(|state| state.filter.clone())
        else {
            return;
        };
        self.edit_saved_filters(i, "FILTER SAVE", |document| {
            super::saved_filters::store_saved_filter(document, &name, &filter);
        });
        self.command_line
            .push_info(crate::tf!("Filter \"{name}\" saved in this drawing.").as_ref());
        let saved = self.saved_filter_names(i);
        if let Some(state) = self.filter_dialog.as_mut() {
            state.saved = saved;
            state.saved_name = Some(name);
            state.save_as = None;
        }
    }

    fn filter_apply(&mut self) -> Task<Message> {
        if self
            .filter_dialog
            .as_ref()
            .is_none_or(|state| state.problem().is_some())
        {
            return Task::none();
        }
        let Some(state) = self.filter_dialog.take() else {
            return Task::none();
        };
        let i = self.active_tab;
        self.tabs[i].scene.set_filter_preview_highlights(&[]);
        self.reset_modal_geometry();
        let found = self.tabs[i]
            .scene
            .filter_matches(state.scope, &state.filter, state.invert);
        let selected = self.tabs[i].scene.apply_found(found, state.result);
        if self.transparent_select.is_some() {
            return self.finish_transparent_select(Some(selected));
        }
        self.command_line
            .push_output(crate::tf!("FILTER: {} object(s) selected.", selected).as_ref());
        self.refresh_properties();
        if self.return_to_block_dialog(true) {
            return Task::none();
        }
        self.unfocus_widgets()
    }

    /// The Block Definition or Write Block dialog Quick Select or FILTER
    /// was opened from comes back; with `take_selection` it takes the
    /// selection they made. False when neither waits.
    pub(super) fn return_to_block_dialog(&mut self, take_selection: bool) -> bool {
        let i = self.active_tab;
        let selected = || -> Vec<Handle> {
            self.tabs[i]
                .scene
                .selected_entities()
                .into_iter()
                .map(|(h, _)| h)
                .collect()
        };
        if self.block_definition.is_some() {
            if take_selection {
                let handles = selected();
                if let Some(block_def) = self.block_definition.as_mut() {
                    block_def.selected_handles = handles;
                    block_def.error_message = None;
                }
                self.refresh_block_definition_preview();
            }
            self.active_modal = Some(super::ModalKind::BlockDefinition);
            return true;
        }
        if self.wblock.is_some() {
            if take_selection {
                let handles = selected();
                if let Some(wblock) = self.wblock.as_mut() {
                    wblock.selected_handles = handles;
                    wblock.error_message = None;
                }
            }
            self.active_modal = Some(super::ModalKind::WriteBlock);
            return true;
        }
        false
    }

    /// 'FILTER / 'QSELECT: open the dialog for the command asking for
    /// objects, parked until the dialog closes. Anywhere else it says why
    /// not.
    pub(super) fn start_transparent_select(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if self.tabs[i].suspended_cmd.is_some() {
            self.command_line.push_error(
                crate::tf!("'{} cannot run while another command is waiting.", cmd).as_ref(),
            );
            return Task::none();
        }
        let asking = self.tabs[i]
            .active_cmd
            .as_ref()
            .filter(|running| running.is_selection_gathering())
            .map(|running| {
                // The name the user knows is the one its prompt starts with
                // ("ERASE  Select objects:"); a shared picker's own name is
                // only SELECT.
                running
                    .prompt()
                    .split_whitespace()
                    .next()
                    .filter(|word| word.chars().all(|c| c.is_ascii_uppercase() || c == '-'))
                    .map_or_else(|| running.name().to_string(), str::to_owned)
            });
        let Some(asking) = asking else {
            self.command_line.push_error(
                crate::tf!("'{} works only while a command asks for objects.", cmd).as_ref(),
            );
            return Task::none();
        };
        self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
        self.tabs[i].transparent_resume = true;
        self.tabs[i].scene.clear_preview_wire();
        self.transparent_select = Some(asking);
        if cmd == "FILTER" {
            self.filter_open(None);
            Task::none()
        } else {
            self.on_qselect_open()
        }
    }

    /// The dialog closed: the parked command comes back and, after Select,
    /// takes the selection it now has; after Cancel it goes on as it was.
    pub(super) fn finish_transparent_select(&mut self, found: Option<usize>) -> Task<Message> {
        let i = self.active_tab;
        self.transparent_select = None;
        self.resume_transparent_parent(i);
        let Some(found) = found else {
            return Task::none();
        };
        self.command_line
            .push_output(crate::tf!("{} found", found).as_ref());
        let selected: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        self.feed_command(crate::command::StepInput::SelectionComplete(selected))
    }

    /// Quick Select's "Advanced…": FILTER with the same conditions (one
    /// group per object type, as Quick Select reads them), scope and result.
    pub(super) fn qselect_advanced(&mut self) -> Task<Message> {
        let Some(state) = self.qselect.take() else {
            return Task::none();
        };
        self.tabs[self.active_tab]
            .scene
            .set_filter_preview_highlights(&[]);
        self.qselect_settings = Some((&state).into());
        let conditions = state.effective_conditions().unwrap_or_default();
        let mut filter = crate::filter::qselect::filter_from_conditions(&conditions);
        if filter.items.is_empty() {
            filter.items.push(Item::Condition(Condition::default()));
        }
        self.filter_open(Some(FilterSeed {
            filter,
            scope: state.scope,
            result: state.result,
            invert: matches!(state.mode, super::QSelectMode::Exclude),
            show: state.show,
        }));
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::QSelectMode;
    use document::entities::{Circle, Line};
    use document::types::Vector3;

    fn app_with_objects() -> (OpenCADStudio, Vec<Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        for name in ["FAL-KULSO", "AJTO"] {
            let _ = app.tabs[i]
                .scene
                .document
                .layers
                .add(document::tables::Layer::new(name));
        }
        let mut handles = Vec::new();
        for (layer, circle) in [("FAL-KULSO", false), ("FAL-KULSO", true), ("AJTO", false)] {
            let mut entity = if circle {
                document::EntityType::Circle(Circle::new())
            } else {
                document::EntityType::Line(Line::from_points(
                    Vector3::ZERO,
                    Vector3::new(1.0, 0.0, 0.0),
                ))
            };
            entity.common_mut().layer = layer.into();
            handles.push(app.tabs[i].scene.add_entity(entity));
        }
        (app, handles)
    }

    fn filter(app: &mut OpenCADStudio, msg: FilterMsg) {
        let _ = app.update(Message::FilterDialog(msg));
    }

    /// #2611: rows, a group, NOT, the live result and Select.
    #[test]
    fn filter_dialog_builds_a_filter_and_selects() {
        let (mut app, handles) = app_with_objects();
        let i = app.active_tab;
        filter(&mut app, FilterMsg::Open(None));
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.live_count, 3, "an empty row matches everything");
        let row = FilterRow::Root(0);
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Property(Some("layer".into()))),
        );
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Value("FAL*".into())),
        );
        assert_eq!(app.filter_dialog.as_ref().unwrap().live_count, 2);
        // ... and (Circle) negated: the line on FAL-KULSO only.
        filter(&mut app, FilterMsg::AddGroup);
        let in_group = FilterRow::Group(1, 0);
        filter(
            &mut app,
            FilterMsg::Edit(in_group, FilterRowEdit::Type(Some("Circle".into()))),
        );
        filter(
            &mut app,
            FilterMsg::Edit(in_group, FilterRowEdit::Not(true)),
        );
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.live_count, 1);
        assert_eq!(state.by_type, [("Line".to_string(), 1)]);
        assert!(state.problem().is_none());
        filter(&mut app, FilterMsg::Show(true));
        assert_eq!(app.tabs[i].scene.hover_highlight_handles().len(), 1);
        filter(&mut app, FilterMsg::Apply);
        assert!(app.filter_dialog.is_none());
        assert!(app.tabs[i].scene.hover_highlight_handles().is_empty());
        assert_eq!(app.tabs[i].scene.selected.len(), 1);
        assert!(app.tabs[i].scene.selected.contains(&handles[0]));
    }

    /// #2611: From object hides the dialog for a pick; the object's ticked
    /// properties become rows, and a second type goes into its own group.
    #[test]
    fn from_object_rows_and_a_second_type_as_a_group() {
        let (mut app, handles) = app_with_objects();
        let i = app.active_tab;
        filter(&mut app, FilterMsg::Open(None));
        let pick = |app: &mut OpenCADStudio, handle| {
            filter(app, FilterMsg::PickObject);
            assert!(app.filter_dialog.is_none(), "the dialog steps aside");
            let result = app.tabs[i]
                .active_cmd
                .as_mut()
                .expect("picking")
                .on_entity_pick(handle, glam::DVec3::ZERO);
            let _ = app.apply_cmd_result(result);
            assert!(app.tabs[i].active_cmd.is_none());
        };
        // The line on FAL-KULSO: type, layer, colour, linetype ticked.
        pick(&mut app, handles[0]);
        let sample = app.filter_dialog.as_ref().unwrap().sample.as_ref().unwrap();
        assert_eq!(sample.type_name, "Line");
        let ticked = sample.rows.iter().filter(|(_, t)| *t).count();
        assert_eq!(ticked, 1 + crate::app::SAMPLE_TICKED_FIELDS.len());
        filter(&mut app, FilterMsg::SampleAdd);
        let state = app.filter_dialog.as_ref().unwrap();
        assert!(state.sample.is_none());
        assert_eq!(state.filter.root, Match::All);
        assert_eq!(state.filter.items.len(), ticked);
        assert_eq!(state.live_count, 1, "the line on FAL-KULSO");
        // The circle: its own group, the filter now any of the two.
        pick(&mut app, handles[1]);
        filter(&mut app, FilterMsg::SampleAdd);
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.filter.root, Match::Any);
        assert_eq!(state.filter.items.len(), 2);
        assert!(state
            .filter
            .items
            .iter()
            .all(|item| matches!(item, Item::Group(_))));
        assert_eq!(state.live_count, 2);
        // Esc during a pick brings the dialog back as it was.
        filter(&mut app, FilterMsg::PickObject);
        let result = app.tabs[i].active_cmd.as_mut().unwrap().on_escape();
        let _ = app.apply_cmd_result(result);
        let state = app.filter_dialog.as_ref().unwrap();
        assert!(state.sample.is_none());
        assert_eq!(state.live_count, 2);
    }

    #[test]
    fn a_sample_of_the_same_type_adds_rows_without_repeats() {
        let row = |field: &str, value: &str| Condition {
            type_filter: Some("Line".into()),
            property: Some(field.into()),
            value: value.into(),
            ..Default::default()
        };
        let mut filter = Filter {
            root: Match::All,
            items: vec![Item::Condition(row("layer", "A"))],
        };
        add_sample(&mut filter, vec![row("layer", "A"), row("color", "1")]);
        assert_eq!(filter.root, Match::All);
        assert_eq!(filter.items.len(), 2);
        // An untyped row of the user's stays beside a typed sample.
        let mut filter = Filter {
            root: Match::All,
            items: vec![Item::Condition(Condition {
                property: Some("layer".into()),
                value: "A*".into(),
                ..Default::default()
            })],
        };
        add_sample(&mut filter, vec![row("color", "1")]);
        assert_eq!(filter.items.len(), 2);
        assert_eq!(filter.root, Match::All);
    }

    /// #2610 (F2): the layer filter editor works on layer properties only:
    /// rows start on the name, the linetype list has no ByLayer, From layer
    /// ticks the name and the colour, each layer found says why, and its
    /// layers open in FILTER for their objects.
    #[test]
    fn layer_filter_editor_as_in_the_mockup() {
        let (mut app, _) = app_with_objects();
        app.filter_open_layers(
            String::new(),
            Filter {
                root: Match::All,
                items: vec![Item::Condition(Condition::default())],
            },
        );
        let state = app.filter_dialog.as_ref().unwrap();
        let first = state.condition(FilterRow::Root(0)).unwrap();
        assert_eq!(
            first.property.as_deref(),
            Some("name"),
            "no type row over layers"
        );
        let linetypes = state
            .properties_for(None)
            .iter()
            .find_map(|p| match &p.editor {
                crate::app::QSelectValueEditor::Choice(o) if p.field == "linetype" => {
                    Some(o.clone())
                }
                _ => None,
            })
            .unwrap();
        assert!(!linetypes.iter().any(|l| l.eq_ignore_ascii_case("ByLayer")));
        filter(&mut app, FilterMsg::AddCondition(None));
        let added = app
            .filter_dialog
            .as_ref()
            .unwrap()
            .condition(FilterRow::Root(1))
            .cloned();
        assert_eq!(added.unwrap().property.as_deref(), Some("name"));
        filter(
            &mut app,
            FilterMsg::Edit(FilterRow::Root(1), FilterRowEdit::Remove),
        );
        // From layer: name and colour ticked.
        filter(&mut app, FilterMsg::FromLayer("FAL-KULSO".into()));
        let sample = app.filter_dialog.as_ref().unwrap().sample.as_ref().unwrap();
        let ticked: Vec<&str> = sample
            .rows
            .iter()
            .filter(|(_, t)| *t)
            .filter_map(|(c, _)| c.property.as_ref().map(|p| p.field.as_str()))
            .collect();
        assert_eq!(ticked, ["name", "color"]);
        filter(&mut app, FilterMsg::SampleCancel);
        // Plot = Yes: each layer found says so.
        filter(
            &mut app,
            FilterMsg::Edit(
                FilterRow::Root(0),
                FilterRowEdit::Property(Some("plot".into())),
            ),
        );
        filter(
            &mut app,
            FilterMsg::Edit(FilterRow::Root(0), FilterRowEdit::Value("1".into())),
        );
        let state = app.filter_dialog.as_ref().unwrap();
        assert!(state.live_count >= 2);
        assert!(state.layer_reasons.iter().all(|r| r.contains(':')));
        // Their objects in FILTER.
        filter(&mut app, FilterMsg::ObjectsOnLayers);
        let state = app.filter_dialog.as_ref().unwrap();
        assert!(state.layer_filter.is_none());
        assert_eq!(state.live_count, 3, "every object sits on a plotted layer");
    }

    #[test]
    fn counts_keep_their_thousands_apart() {
        use crate::ui::live_result::fmt_count;
        assert_eq!(fmt_count(312), "312");
        assert_eq!(fmt_count(3997), "3\u{2009}997");
        assert_eq!(fmt_count(1234567), "1\u{2009}234\u{2009}567");
    }

    /// #2611 (J5): Window… steps aside for a pick; the objects picked become
    /// the selection searched; Esc brings the dialog back as it was.
    #[test]
    fn window_scope_in_quick_select_and_filter() {
        let (mut app, handles) = app_with_objects();
        let i = app.active_tab;
        let pick = |app: &mut OpenCADStudio, picked: &[Handle]| {
            for h in picked {
                app.tabs[i].scene.select_entity(*h, false);
            }
            let command = app.tabs[i].active_cmd.as_mut().expect("picking");
            // Picks add up until Enter.
            let more = command.on_selection_complete(picked.to_vec());
            assert!(matches!(more, crate::command::CmdResult::NeedPoint));
            let result = command.on_enter();
            let _ = app.apply_cmd_result(result);
        };
        let _ = app.update(Message::QSelectOpen);
        let _ = app.update(Message::QSelectWindow);
        assert!(app.qselect.is_none(), "Quick Select steps aside");
        pick(&mut app, &handles[..2]);
        let state = app.qselect.as_ref().expect("back");
        assert_eq!(state.scope, QSelectScope::CurrentSelection);
        let _ = app.update(Message::QSelectClose);
        // FILTER: Esc leaves the scope alone.
        app.tabs[i].scene.deselect_all();
        filter(&mut app, FilterMsg::Open(None));
        filter(&mut app, FilterMsg::Window);
        assert!(app.filter_dialog.is_none());
        let result = app.tabs[i].active_cmd.as_mut().unwrap().on_escape();
        let _ = app.apply_cmd_result(result);
        assert_eq!(
            app.filter_dialog.as_ref().unwrap().scope,
            QSelectScope::CurrentSpace
        );
        filter(&mut app, FilterMsg::Window);
        pick(&mut app, &handles[2..]);
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.scope, QSelectScope::CurrentSelection);
        assert_eq!(state.candidate_count, 1);
    }

    /// #2611 (J5): saved filters go to a file and come back into a drawing.
    #[test]
    fn saved_filters_export_and_import() {
        let (mut app, _) = app_with_objects();
        let i = app.active_tab;
        filter(&mut app, FilterMsg::Open(None));
        filter(
            &mut app,
            FilterMsg::Edit(
                FilterRow::Root(0),
                FilterRowEdit::Type(Some("Circle".into())),
            ),
        );
        filter(&mut app, FilterMsg::SaveAs(Some("Circles".into())));
        filter(&mut app, FilterMsg::SaveAsCommit);
        let path =
            std::env::temp_dir().join(format!("ocs-filters-{}.ocsfilter", std::process::id()));
        filter(&mut app, FilterMsg::ExportTo(path.clone()));
        filter(&mut app, FilterMsg::DeleteSaved);
        assert!(app.saved_filter_names(i).is_empty());
        filter(&mut app, FilterMsg::ImportFrom(path.clone()));
        assert_eq!(app.saved_filter_names(i), ["Circles"]);
        let _ = std::fs::remove_file(path);
    }

    /// Quick Select's "Advanced…" carries its conditions over: two types
    /// become two groups under "any", and the count is the same.
    #[test]
    fn advanced_carries_quick_select_over() {
        let (mut app, _) = app_with_objects();
        let _ = app.update(Message::QSelectOpen);
        let _ = app.update(Message::QSelectSetScope(QSelectScope::CurrentSpace));
        let _ = app.update(Message::QSelectSetType(Some("Line".into())));
        let _ = app.update(Message::QSelectAddCondition);
        let _ = app.update(Message::QSelectSetType(Some("Circle".into())));
        let _ = app.update(Message::QSelectAddCondition);
        let _ = app.update(Message::QSelectSetMode(QSelectMode::Include));
        let quick = app.qselect.as_ref().unwrap().live_count;
        // Quick Select's live line counts each type (#2611 mockup).
        let by_type = &app.qselect.as_ref().unwrap().live_by_type;
        assert_eq!(by_type.len(), 2);
        assert_eq!(by_type.iter().map(|(_, n)| n).sum::<usize>(), quick);
        let _ = app.update(Message::QSelectAdvanced);
        assert!(app.qselect.is_none());
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.filter.root, Match::Any);
        assert_eq!(state.filter.items.len(), 2);
        assert_eq!(state.live_count, quick);
        assert_eq!(state.live_count, 3);
    }

    /// #2611: a filter saved from the dialog lives in the drawing, loads
    /// back, runs as `FILTER <name>` and shows in the right-click menu.
    #[test]
    fn saved_filters_from_the_dialog() {
        let (mut app, _) = app_with_objects();
        let i = app.active_tab;
        filter(&mut app, FilterMsg::Open(None));
        let row = FilterRow::Root(0);
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Type(Some("Circle".into()))),
        );
        // Save with nothing loaded asks for a name first.
        filter(&mut app, FilterMsg::Save);
        assert_eq!(
            app.filter_dialog.as_ref().unwrap().save_as.as_deref(),
            Some("")
        );
        filter(&mut app, FilterMsg::SaveAs(Some("Circles".into())));
        filter(&mut app, FilterMsg::SaveAsCommit);
        let state = app.filter_dialog.as_ref().unwrap();
        assert_eq!(state.saved, ["Circles"]);
        assert_eq!(state.saved_name.as_deref(), Some("Circles"));
        filter(&mut app, FilterMsg::Close);

        // Back in a fresh dialog it loads; from the command line it applies.
        filter(&mut app, FilterMsg::Open(None));
        filter(&mut app, FilterMsg::Saved("circles".into()));
        assert_eq!(app.filter_dialog.as_ref().unwrap().live_count, 1);
        filter(&mut app, FilterMsg::Close);
        let _ = app.update(Message::Command("FILTER Circles".into()));
        assert_eq!(app.tabs[i].scene.selected.len(), 1);
        let menu = app.context_menu_context();
        let crate::ui::popup::context_menu::MenuContext::Idle { saved_filters, .. } = menu else {
            panic!("an idle right-click menu");
        };
        assert_eq!(saved_filters, ["Circles"]);

        filter(&mut app, FilterMsg::Open(None));
        filter(&mut app, FilterMsg::Saved("Circles".into()));
        filter(&mut app, FilterMsg::DeleteSaved);
        assert!(app.saved_filter_names(i).is_empty());
    }

    /// #2611: 'FILTER inside ERASE's "Select objects:" works for ERASE, hands
    /// it what it finds, and Cancel leaves ERASE as it was.
    #[test]
    fn transparent_filter_works_for_the_running_command() {
        let (mut app, handles) = app_with_objects();
        let i = app.active_tab;
        app.command_line.last_error = None;
        let _ = app.run_command_line("'FILTER");
        assert!(app.filter_dialog.is_none());
        assert!(
            app.command_line.last_error.is_some(),
            "no command asks for objects"
        );

        let _ = app.run_command_line("ERASE");
        assert!(app.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.is_selection_gathering()));
        let _ = app.run_command_line("'FILTER");
        let state = app.filter_dialog.as_ref().expect("FILTER opens for ERASE");
        assert_eq!(state.for_command.as_deref(), Some("ERASE"));
        assert!(app.tabs[i].active_cmd.is_none(), "ERASE waits");
        // Cancel: ERASE goes on with nothing picked.
        filter(&mut app, FilterMsg::Close);
        assert!(app.tabs[i].active_cmd.is_some());
        assert!(app.tabs[i].scene.selected.is_empty());

        let _ = app.run_command_line("'FILTER");
        let row = FilterRow::Root(0);
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Type(Some("Circle".into()))),
        );
        filter(&mut app, FilterMsg::Apply);
        assert!(app.filter_dialog.is_none());
        assert!(app.tabs[i].active_cmd.is_some(), "ERASE is back");
        let _ = app.update(Message::CommandFinalize);
        let doc = &app.tabs[i].scene.document;
        assert!(doc.get_entity(handles[1]).is_none(), "the circle is erased");
        assert!(doc.get_entity(handles[0]).is_some() && doc.get_entity(handles[2]).is_some());
    }

    #[test]
    fn a_number_property_needs_a_number() {
        let (mut app, _) = app_with_objects();
        filter(&mut app, FilterMsg::Open(None));
        let row = FilterRow::Root(0);
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Type(Some("Circle".into()))),
        );
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Property(Some("radius".into()))),
        );
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Value("big".into())),
        );
        assert!(app.filter_dialog.as_ref().unwrap().problem().is_some());
        filter(
            &mut app,
            FilterMsg::Edit(row, FilterRowEdit::Value("1".into())),
        );
        assert!(app.filter_dialog.as_ref().unwrap().problem().is_none());
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

mod view {
    use super::*;
    use crate::t;
    use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt};
    use iced::widget::{
        button, checkbox, column, container, row, scrollable, text, text_input, Space,
    };
    use iced::{Background, Border, Element, Fill, Length, Theme};
    use std::borrow::Cow;

    const ANY_TYPE: &str = "(Any type)";
    const TYPE_ONLY: &str = "(type only)";

    fn type_label(name: &str) -> String {
        if name == "Insert" {
            t!("Block Reference").into_owned()
        } else {
            t!(name).into_owned()
        }
    }

    fn muted<'a>(s: impl Into<Cow<'static, str>>) -> iced::widget::Text<'a> {
        text(s.into())
            .size(11)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.palette().background.base.text.scale_alpha(0.7)),
            })
    }

    fn panel_style(theme: &Theme) -> container::Style {
        let palette = theme.palette();
        container::Style {
            background: Some(Background::Color(palette.background.base.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }

    /// A group's frame: a border, the dialog's own background (mockup).
    fn frame_style(theme: &Theme) -> container::Style {
        container::Style {
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }

    /// The NOT toggle's width, shared with the column header.
    const NOT_W: f32 = 48.0;

    /// NOT: on, an accent border and text; off, a quiet button (mockup).
    fn not_style(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |theme: &Theme, status| {
            let palette = theme.palette();
            button::Style {
                background: Some(Background::Color(
                    if matches!(status, button::Status::Hovered) {
                        palette.background.strong.color
                    } else {
                        palette.background.weak.color
                    },
                )),
                text_color: if on {
                    palette.primary.base.color
                } else {
                    palette.background.base.text.scale_alpha(0.7)
                },
                border: Border {
                    color: if on {
                        palette.primary.base.color
                    } else {
                        palette.background.neutral.color
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        }
    }

    fn msg(m: FilterMsg) -> Message {
        Message::FilterDialog(m)
    }

    /// "all of these" / "any of these".
    fn match_picker<'a>(mode: Match, on: impl Fn(Match) -> Message + 'a) -> Element<'a, Message> {
        let label = |m: &Match| match m {
            Match::All => t!("all of these").into_owned(),
            Match::Any => t!("any of these").into_owned(),
        };
        crate::ui::dropdown_list::pick_list(Some(mode), vec![Match::All, Match::Any], label)
            .on_select(on)
            .text_size(12)
            .width(Length::Fixed(130.0))
            .into()
    }

    fn op_label(op: &Op) -> String {
        use super::super::QSelectOp;
        let qs = match op {
            Op::Any => QSelectOp::Any,
            Op::Eq => QSelectOp::Eq,
            Op::Ne => QSelectOp::Neq,
            Op::Gt => QSelectOp::Gt,
            Op::Ge => QSelectOp::Ge,
            Op::Lt => QSelectOp::Lt,
            Op::Le => QSelectOp::Le,
        };
        qs.to_string()
    }

    fn add_button<'a>(caption: Cow<'static, str>, message: Message) -> Element<'a, Message> {
        button(
            row![
                crate::ui::icons::themed(crate::ui::icons::PLUS, 11.0),
                text(caption).size(12)
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(message)
        .style(button::secondary)
        .padding([4, 10])
        .into()
    }

    fn condition_row<'a>(
        state: &'a FilterDialogState,
        row_id: FilterRow,
        condition: &'a Condition,
    ) -> Element<'a, Message> {
        let edit = move |e: FilterRowEdit| msg(FilterMsg::Edit(row_id, e));
        let not = button(text(t!("Not")).size(11).align_x(iced::Center))
            .on_press(edit(FilterRowEdit::Not(!condition.negate)))
            .style(not_style(condition.negate))
            .padding([4, 6])
            .width(Length::Fixed(NOT_W));

        let mut type_options = vec![ANY_TYPE.to_string()];
        type_options.extend(state.types.iter().map(|(t, _)| t.clone()));
        if let Some(t) = &condition.type_filter {
            if !type_options.contains(t) {
                type_options.push(t.clone());
            }
        }
        let counts: std::collections::HashMap<String, usize> =
            state.types.iter().cloned().collect();
        let type_picker = crate::ui::dropdown_list::pick_list(
            Some(
                condition
                    .type_filter
                    .clone()
                    .unwrap_or_else(|| ANY_TYPE.to_string()),
            ),
            type_options,
            move |name: &String| match counts.get(name) {
                Some(n) => format!("{}  ({n})", type_label(name)),
                None if name == ANY_TYPE => t!(ANY_TYPE).into_owned(),
                None => type_label(name),
            },
        )
        .on_select(move |picked: String| {
            edit(FilterRowEdit::Type((picked != ANY_TYPE).then_some(picked)))
        })
        .text_size(12)
        .width(Length::Fixed(150.0));

        let properties = state.properties_for(condition.type_filter.as_deref());
        let mut property_options = if state.layer_filter.is_some() {
            Vec::new()
        } else {
            vec![(String::new(), t!(TYPE_ONLY).into_owned())]
        };
        property_options.extend(
            properties
                .iter()
                .map(|p| (p.field.clone(), p.label.clone())),
        );
        let ids: Vec<String> = property_options.iter().map(|(id, _)| id.clone()).collect();
        let labels: std::collections::HashMap<String, String> =
            property_options.into_iter().collect();
        let property_picker = crate::ui::dropdown_list::pick_list(
            Some(condition.property.clone().unwrap_or_default()),
            ids,
            move |id: &String| labels.get(id).cloned().unwrap_or_else(|| id.clone()),
        )
        .on_select(move |id: String| edit(FilterRowEdit::Property((!id.is_empty()).then_some(id))))
        .text_size(12)
        .width(Length::Fixed(150.0));

        let number = condition.property.as_deref().is_some_and(|id| {
            properties.iter().any(|p| {
                p.field == id && matches!(p.editor, super::super::QSelectValueEditor::Number)
            })
        });
        let mut ops = vec![Op::Eq, Op::Ne];
        if number {
            ops.extend([Op::Gt, Op::Ge, Op::Lt, Op::Le]);
        }
        ops.push(Op::Any);
        let has_property = condition.property.is_some();
        let op_cell: Element<'a, Message> = if has_property {
            crate::ui::dropdown_list::pick_list(Some(condition.op), ops, op_label)
                .on_select(move |op| edit(FilterRowEdit::Op(op)))
                .text_size(12)
                .width(Length::Fixed(140.0))
                .into()
        } else {
            Space::new().width(Length::Fixed(140.0)).into()
        };
        let choices = condition.property.as_deref().and_then(|id| {
            properties.iter().find_map(|p| match &p.editor {
                super::super::QSelectValueEditor::Choice(options) if p.field == id => {
                    Some(options.clone())
                }
                _ => None,
            })
        });
        let value_cell: Element<'a, Message> = if !has_property || condition.op == Op::Any {
            Space::new().width(Fill).into()
        } else if let Some(options) = choices {
            let selected = (!condition.value.is_empty()).then(|| condition.value.clone());
            crate::ui::dropdown_list::pick_list(selected, options, |value: &String| {
                match value.as_str() {
                    "1" => t!("Yes").into_owned(),
                    "0" => t!("No").into_owned(),
                    other => other.to_string(),
                }
            })
            .on_select(move |v| edit(FilterRowEdit::Value(v)))
            .text_size(12)
            .width(Fill)
            .into()
        } else if has_property {
            text_input("", &condition.value)
                .on_input(move |v| edit(FilterRowEdit::Value(v)))
                .size(12)
                .width(Fill)
                .into()
        } else {
            Space::new().width(Fill).into()
        };
        let remove = button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 12.0))
            .on_press(edit(FilterRowEdit::Remove))
            .style(button::text)
            .padding([4, 6]);
        let mut cells = row![not].spacing(6).align_y(iced::Alignment::Center);
        if state.layer_filter.is_none() {
            cells = cells.push(type_picker);
        }
        cells
            .push(property_picker)
            .push(op_cell)
            .push(value_cell)
            .push(remove)
            .into()
    }

    pub(in crate::app) fn content<'a>(
        state: &'a FilterDialogState,
        sizing: crate::ui::modal::ModalSizing,
    ) -> Element<'a, Message> {
        // ── Search in ─────────────────────────────────────────────────────
        let mut scope_row = row![text(t!("Search in")).size(12)]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        for scope in [QSelectScope::CurrentSpace, QSelectScope::CurrentSelection] {
            let caption = match scope {
                QSelectScope::CurrentSelection => t!("Selection"),
                _ => Cow::Owned(scope.to_string()),
            };
            scope_row = scope_row.push(
                button(text(caption).size(12))
                    .on_press(msg(FilterMsg::Scope(scope)))
                    .style(button_style(state.scope == scope))
                    .padding([4, 10]),
            );
        }
        // Window…: pick the objects to search in (#2611 mockup).
        let scope_row = scope_row.push(
            button(text(t!("Window…")).size(12))
                .on_press(msg(FilterMsg::Window))
                .style(button_style(false))
                .padding([4, 10]),
        );

        // ── Saved filters (kept in the drawing) ────────────────────────────
        let icon_button = |icon: &'static [u8], tip: Cow<'static, str>, on: Option<Message>| {
            iced::widget::tooltip(
                button(crate::ui::icons::themed(icon, 13.0))
                    .on_press_maybe(on)
                    .style(button::secondary)
                    .padding([5, 8]),
                container(text(tip).size(11))
                    .padding([3, 7])
                    .style(panel_style),
                iced::widget::tooltip::Position::Bottom,
            )
        };
        let mut saved_row = row![text(t!("Saved")).size(12)]
            .spacing(6)
            .align_y(iced::Alignment::Center);
        saved_row = saved_row.push(
            crate::ui::dropdown_list::pick_list(
                state.saved_name.clone(),
                state.saved.clone(),
                |name: &String| name.clone(),
            )
            .on_select(|name| msg(FilterMsg::Saved(name)))
            .text_size(12)
            .width(Length::Fixed(200.0)),
        );
        saved_row = saved_row
            .push(icon_button(
                crate::ui::icons::SAVE,
                t!("Save"),
                Some(msg(FilterMsg::Save)),
            ))
            .push(icon_button(
                crate::ui::icons::COPY,
                t!("Save As"),
                Some(msg(FilterMsg::SaveAs(Some(String::new())))),
            ))
            .push(icon_button(
                crate::ui::icons::TRASH,
                t!("Delete"),
                state
                    .saved_name
                    .is_some()
                    .then_some(msg(FilterMsg::DeleteSaved)),
            ));
        // Saved filters to and from a file, for another drawing (#2611).
        #[cfg(not(target_arch = "wasm32"))]
        {
            saved_row = saved_row
                .push(Space::new().width(6))
                .push(icon_button(
                    crate::ui::icons::IMPORT,
                    t!("Import"),
                    Some(msg(FilterMsg::Import)),
                ))
                .push(icon_button(
                    crate::ui::icons::FILE_EXPORT,
                    t!("Export"),
                    (!state.saved.is_empty()).then_some(msg(FilterMsg::Export)),
                ));
        }
        if let Some(typed) = &state.save_as {
            saved_row = saved_row
                .push(
                    text_input(&t!("Name"), typed)
                        .on_input(|v| msg(FilterMsg::SaveAs(Some(v))))
                        .on_submit(msg(FilterMsg::SaveAsCommit))
                        .size(12)
                        .width(Length::Fixed(180.0)),
                )
                .push(dialog_button(t!("OK"), msg(FilterMsg::SaveAsCommit), true))
                .push(dialog_button(
                    t!("Cancel"),
                    msg(FilterMsg::SaveAs(None)),
                    false,
                ));
        }
        // Where they are kept, said quietly (#2611: no popup); Search in on
        // the same bar, as in the mockup.
        let saved_row = saved_row
            .push(muted(t!("Saved in this drawing")))
            .push(Space::new().width(Fill))
            .push(scope_row);

        // ── Rows ──────────────────────────────────────────────────────────
        let layers_mode = state.layer_filter.is_some();
        let intro = if layers_mode {
            t!("Show layers that match")
        } else {
            t!("Select objects that match")
        };
        let head = |label: Cow<'static, str>, w: Length| -> Element<'a, Message> {
            muted(label).width(w).into()
        };
        let mut header = row![head(t!("NOT"), Length::Fixed(NOT_W))]
            .spacing(6)
            .align_y(iced::Alignment::Center);
        if !layers_mode {
            header = header.push(head(t!("Type"), Length::Fixed(150.0)));
        }
        let header = header
            .push(head(
                if layers_mode {
                    t!("Layer property")
                } else {
                    t!("Property")
                },
                Length::Fixed(150.0),
            ))
            .push(head(t!("Op"), Length::Fixed(140.0)))
            .push(
                row![muted(t!("Value")), crate::ui::live_result::pattern_help()]
                    .spacing(2)
                    .align_y(iced::Alignment::Center)
                    .width(Fill),
            );
        let mut rows = column![
            row![
                text(intro).size(12),
                match_picker(state.filter.root, |m| msg(FilterMsg::Root(m))),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            header,
        ]
        .spacing(6);
        for (index, item) in state.filter.items.iter().enumerate() {
            match item {
                Item::Condition(condition) => {
                    rows = rows.push(condition_row(state, FilterRow::Root(index), condition));
                }
                Item::Group(group) => {
                    let mut inner = column![row![
                        text(t!("Group: match")).size(12),
                        match_picker(group.mode, move |m| msg(FilterMsg::GroupMode(index, m))),
                        Space::new().width(Fill),
                        button(crate::ui::icons::themed(crate::ui::icons::TRASH, 12.0))
                            .on_press(msg(FilterMsg::GroupRemove(index)))
                            .style(button::text)
                            .padding([4, 6]),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center)]
                    .spacing(6);
                    for (i, condition) in group.conditions.iter().enumerate() {
                        inner =
                            inner.push(condition_row(state, FilterRow::Group(index, i), condition));
                    }
                    inner = inner.push(add_button(
                        t!("Condition"),
                        msg(FilterMsg::AddCondition(Some(index))),
                    ));
                    // A frame, no colour of its own (#2611).
                    rows = rows.push(container(inner).padding(8).width(Fill).style(frame_style));
                }
            }
        }
        let mut add_buttons = row![
            add_button(t!("Condition"), msg(FilterMsg::AddCondition(None))),
            add_button(t!("Group"), msg(FilterMsg::AddGroup)),
        ]
        .spacing(6);
        if layers_mode {
            // Rows from a layer's properties (#2610 mockup: From layer).
            let names: Vec<String> = state.all_layers.clone();
            add_buttons = add_buttons.push(
                crate::ui::dropdown_list::pick_list(None::<String>, names, |n: &String| n.clone())
                    .on_select(|n| msg(FilterMsg::FromLayer(n)))
                    .placeholder(t!("From layer").into_owned())
                    .text_size(12)
                    .width(Length::Fixed(170.0)),
            );
        }
        if !layers_mode {
            // Rows from a picked object's properties (#2611).
            add_buttons = add_buttons.push(
                button(
                    row![
                        crate::ui::icons::themed(crate::ui::icons::EYEDROPPER, 12.0),
                        text(t!("From object")).size(12)
                    ]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(msg(FilterMsg::PickObject))
                .style(button_style(false))
                .padding([4, 10]),
            );
        }

        // ── Read back ─────────────────────────────────────────────────────
        let property_label = |id: &str| -> String {
            state
                .properties
                .iter()
                .flat_map(|(_, list)| list.iter())
                .find(|p| p.field == id)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| id.to_string())
        };
        let type_label_fn = |t: &str| type_label(t);
        let sentence = crate::filter::describe::describe(
            &state.filter,
            &crate::filter::describe::Labels {
                type_label: &type_label_fn,
                property_label: &property_label,
            },
        );
        let reads = container(
            row![
                muted(t!("Reads:")),
                crate::ui::live_result::reads(
                    sentence,
                    &[
                        t!("and").into_owned(),
                        t!("or").into_owned(),
                        t!("not").into_owned(),
                    ],
                ),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Start),
        )
        .padding([6, 8])
        .width(Fill)
        .style(panel_style);

        // After From object: the picked object's properties to tick, in
        // place of the rows until they are added or dropped.
        let rows_area: Element<'a, Message> = match &state.sample {
            Some(sample) => {
                let mut ticks = column![].spacing(4);
                for (index, (condition, ticked)) in sample.rows.iter().enumerate() {
                    let label = match &condition.property {
                        None => type_label(&sample.type_name),
                        Some(property) => format!("{} = {}", property.label, condition.value),
                    };
                    ticks = ticks.push(
                        checkbox(*ticked)
                            .label(label)
                            .text_size(12)
                            .size(14)
                            .on_toggle(move |_| msg(FilterMsg::SampleToggle(index))),
                    );
                }
                column![
                    text(crate::tf!(
                        "Properties of the picked {}",
                        type_label(&sample.type_name)
                    ))
                    .size(12),
                    scrollable(ticks).height(Length::Fixed(240.0)),
                    row![
                        dialog_button(t!("Add ticked conditions"), msg(FilterMsg::SampleAdd), true),
                        dialog_button(t!("Cancel"), msg(FilterMsg::SampleCancel), false),
                    ]
                    .spacing(6),
                ]
                .spacing(8)
                .into()
            }
            // As tall as the rows (B3: the dialog fits its content), up to a
            // limit; a dialog the user made taller gives them the room.
            None => {
                let list = scrollable(rows.padding(iced::Padding {
                    right: 12.0,
                    ..Default::default()
                }));
                if sizing.height == Length::Fill {
                    list.height(Fill).into()
                } else {
                    list.height(Length::Shrink).into()
                }
            }
        };
        let editor = column![rows_area, add_buttons, reads,]
            .spacing(8)
            .width(Fill);

        // ── Live result ───────────────────────────────────────────────────
        // A line per type or layer: a click shows only those, again shows
        // all of them.
        let bucket_line =
            |label: String, count: usize, bucket: FilterBucket| -> Element<'a, Message> {
                let on = state.focus.as_ref() == Some(&bucket);
                let next = (!on).then_some(bucket);
                crate::ui::live_result::bucket(
                    label,
                    count,
                    state.live_count,
                    on,
                    msg(FilterMsg::Focus(next)),
                )
            };
        let mut live = column![
            row![
                text(t!("Live result")).size(12).width(Fill),
                crate::ui::live_result::show_toggle(
                    state.show,
                    t!("Show").into_owned(),
                    msg(FilterMsg::Show(!state.show)),
                ),
            ]
            .align_y(iced::Alignment::Center),
            crate::ui::live_result::big_count(
                state.live_count,
                if layers_mode {
                    crate::tf!(
                        "of {} layers",
                        crate::ui::live_result::fmt_count(state.candidate_count)
                    )
                } else {
                    crate::tf!(
                        "of {}",
                        crate::ui::live_result::fmt_count(state.candidate_count)
                    )
                },
            ),
            crate::ui::live_result::track(state.live_count, state.candidate_count, true),
        ]
        .push(if state.live_count == 0 {
            Element::from(muted(t!("No match")))
        } else {
            Element::from(Space::new().height(0))
        })
        .spacing(6)
        .width(Length::Fixed(240.0))
        .height(if sizing.height == Length::Fill {
            Length::Fill
        } else {
            Length::Shrink
        });
        if !layers_mode {
            live = live.push(muted(t!("By type")));
        }
        for (t, n) in &state.by_type {
            live = live.push(bucket_line(
                type_label(t),
                *n,
                FilterBucket::Type(t.clone()),
            ));
        }
        if layers_mode {
            // The layers themselves, each with why it is in (mockup).
            live = live.push(muted(t!("Matching layers")));
            let mut names = column![].spacing(2);
            for (at, (l, _)) in state.by_layer.iter().enumerate() {
                let reason = state.layer_reasons.get(at).cloned().unwrap_or_default();
                names = names.push(
                    row![
                        text(crate::ui::text_util::elide(l, 18))
                            .size(11)
                            .width(Fill)
                            .wrapping(iced::widget::text::Wrapping::None),
                        muted(reason)
                    ]
                    .spacing(6),
                );
            }
            // A long list scrolls: at its natural size the dialog keeps a
            // modest height, and a dialog made taller gives the list the room
            // (it filled the screen and could not be made smaller).
            live = live.push(scrollable(names).height(if sizing.height == Length::Fill {
                Length::Fill
            } else {
                Length::Fixed(260.0)
            }));
            live = live.push(
                text(t!(
                    "The filter stays live: a new layer that matches shows up in it on its own."
                ))
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
                }),
            );
        } else {
            live = live.push(muted(t!("By layer")));
            for (l, n) in &state.by_layer {
                live = live.push(bucket_line(l.clone(), *n, FilterBucket::Layer(l.clone())));
            }
            // The result's layers in the Layer manager (#2610).
            live = live.push(
                button(text(t!("Open in Layer manager")).size(11))
                    .on_press_maybe((state.live_count > 0).then_some(msg(FilterMsg::ShowLayers)))
                    .style(button::secondary)
                    .padding([3, 8]),
            );
            live = live.push(
                text(t!(
                    "Click a row to show only that type or layer in the drawing."
                ))
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
                }),
            );
        }
        let live = container(live)
            .padding(10)
            .height(if sizing.height == Length::Fill {
                Fill
            } else {
                Length::Shrink
            })
            .style(panel_style);

        // ── Footer ────────────────────────────────────────────────────────
        let mut result_row = row![].spacing(2);
        for (value, caption) in [
            (QSelectResult::New, t!("New selection")),
            (QSelectResult::Add, t!("Add")),
            (QSelectResult::Remove, t!("Remove")),
        ] {
            result_row = result_row.push(
                button(text(caption).size(12))
                    .on_press(msg(FilterMsg::Result(value)))
                    .style(button_style(state.result == value))
                    .padding([4, 10]),
            );
        }
        let problem = state.problem();
        let select = dialog_button_styled_opt(
            match &state.for_command {
                Some(command) => crate::tf!("Use {} in {}", state.live_count, command),
                None => crate::tf!("Select {}", state.live_count),
            },
            (problem.is_none() && state.live_count > 0).then_some(msg(FilterMsg::Apply)),
            button::primary,
        );
        let footer = row![
            result_row,
            checkbox(state.invert)
                .label(t!("Select the objects that do not match"))
                .text_size(12)
                .size(14)
                .on_toggle(|on| msg(FilterMsg::Invert(on))),
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), msg(FilterMsg::Close), false),
            select,
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);

        // Layer mode: the filter's name and Save filter instead.
        let footer: Element<'a, Message> = match &state.layer_filter {
            Some(name) => row![
                button(text(t!("Filter objects on these layers…")).size(12))
                    .on_press_maybe(
                        (state.live_count > 0).then_some(msg(FilterMsg::ObjectsOnLayers))
                    )
                    .style(button::secondary)
                    .padding([4, 10]),
                Space::new().width(Fill),
                dialog_button(t!("Cancel"), msg(FilterMsg::Close), false),
                dialog_button_styled_opt(
                    t!("Save filter"),
                    (!name.trim().is_empty() && problem.is_none())
                        .then_some(msg(FilterMsg::SaveLayerFilter)),
                    button::primary,
                ),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .into(),
            None => footer.into(),
        };
        let mut body = column![].spacing(10);
        if !layers_mode {
            body = body.push(saved_row);
            if state.scope == QSelectScope::CurrentSelection {
                body = body.push(crate::ui::live_result::selection_note(
                    state.candidate_count,
                    msg(FilterMsg::Scope(QSelectScope::CurrentSpace)),
                ));
            }
        }
        if let Some(name) = &state.layer_filter {
            // The name on top, as in the mockup; without one it can't be
            // saved, and says so.
            let mut name_row = row![
                text(t!("Name")).size(12),
                text_input("", name)
                    .on_input(|v| msg(FilterMsg::LayerName(v)))
                    .on_submit(msg(FilterMsg::SaveLayerFilter))
                    .size(12)
                    .width(Length::Fixed(260.0)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            if name.trim().is_empty() {
                name_row = name_row.push(muted(t!("A name is needed to save the filter.")));
            }
            // Under: at the top, or under another filter (#2610).
            let mut options = vec![String::new()];
            options.extend(state.layer_filter_parents.iter().cloned());
            name_row = name_row
                .push(Space::new().width(Fill))
                .push(text(t!("Under")).size(12))
                .push(
                    crate::ui::dropdown_list::pick_list(
                        Some(state.layer_filter_parent.clone().unwrap_or_default()),
                        options,
                        |o: &String| {
                            if o.is_empty() {
                                t!("Property filters").into_owned()
                            } else {
                                o.clone()
                            }
                        },
                    )
                    .on_select(|o| msg(FilterMsg::LayerParent(o)))
                    .text_size(12)
                    .width(Length::Fixed(200.0)),
                );
            body = body.push(name_row);
        }
        let body = body
            .extend([
                row![editor, live].spacing(12).into(),
                match problem {
                    Some(problem) => {
                        text(problem)
                            .size(11)
                            .style(|theme: &Theme| iced::widget::text::Style {
                                color: Some(theme.palette().danger.base.color),
                            })
                    }
                    None => text("").size(11),
                }
                .into(),
                footer,
            ])
            .width(sizing.width)
            .height(sizing.height);
        container(body)
            .padding(16)
            .width(sizing.width)
            .height(sizing.height)
            .style(|theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    }
}

impl OpenCADStudio {
    /// The FILTER dialog in the shared movable modal frame.
    pub(super) fn filter_overlay<'a>(
        &self,
        state: &'a FilterDialogState,
    ) -> iced::Element<'a, Message> {
        let measurement = view::content(state, crate::ui::modal::ModalSizing::INTRINSIC);
        let content = view::content(state, crate::ui::modal::ModalSizing::FILL);
        let content = crate::ui::modal::intrinsic(
            measurement,
            content,
            iced::Size::new(1000.0, f32::INFINITY),
            self.modal_resize,
        );
        crate::ui::modal::modal(
            iced::widget::Space::new()
                .width(iced::Fill)
                .height(iced::Fill),
            match (&state.layer_filter, &state.for_command) {
                (Some(_), _) => crate::t!("Layer Filter"),
                // Who it works for (#2611 mockup, Transparent).
                (None, Some(command)) => crate::tf!("Filter · for {}", command),
                (None, None) => crate::t!("Filter"),
            },
            content,
            Message::FilterDialog(FilterMsg::Close),
            self.modal_offset,
            crate::ui::modal::ModalOptions::STANDARD,
        )
    }
}
