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

/// Live result lines shown per kind.
const RESULT_LINES: usize = 6;

impl OpenCADStudio {
    pub(super) fn on_filter_message(&mut self, msg: FilterMsg) -> Task<Message> {
        match msg {
            FilterMsg::Open(seed) => {
                self.filter_open(seed.map(|seed| *seed));
                return Task::none();
            }
            FilterMsg::Close => {
                self.filter_dialog = None;
                self.reset_modal_geometry();
                self.filter_refresh_live();
                return Task::none();
            }
            FilterMsg::Apply => return self.filter_apply(),
            FilterMsg::SaveLayerFilter => {
                self.save_layer_filter();
                return Task::none();
            }
            FilterMsg::ShowLayers => {
                self.filter_show_layers();
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
                state
                    .filter
                    .items
                    .push(Item::Condition(Condition::default()));
            }
            FilterMsg::AddCondition(Some(g)) => {
                if let Some(Item::Group(group)) = state.filter.items.get_mut(g) {
                    group.conditions.push(Condition::default());
                }
            }
            FilterMsg::AddGroup => state.filter.items.push(Item::Group(Group {
                mode: Match::Any,
                conditions: vec![Condition::default()],
            })),
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
            FilterMsg::Result(result) => state.result = result,
            FilterMsg::Invert(on) => state.invert = on,
            FilterMsg::Show(on) => state.show = on,
            FilterMsg::Focus(bucket) => {
                state.focus = if state.focus == bucket { None } else { bucket };
            }
            FilterMsg::SaveAs(name) => state.save_as = name,
            FilterMsg::LayerName(name) => state.layer_filter = Some(name),
            FilterMsg::Open(_)
            | FilterMsg::Close
            | FilterMsg::Apply
            | FilterMsg::Saved(_)
            | FilterMsg::Save
            | FilterMsg::SaveAsCommit
            | FilterMsg::DeleteSaved
            | FilterMsg::SaveLayerFilter
            | FilterMsg::ShowLayers => {}
        }
        if let Some(t) = types_changed {
            self.filter_load_properties(t);
        }
        self.filter_refresh_scope();
        self.filter_refresh_live();
        Task::none()
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
        self.show_layers = true;
        self.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    }

    /// FILTER over the layers, for a Layer manager filter named `name`.
    pub(super) fn filter_open_layers(&mut self, name: String, filter: Filter) {
        self.filter_open(Some(FilterSeed {
            filter,
            scope: QSelectScope::CurrentSpace,
            result: QSelectResult::New,
            invert: false,
            show: false,
        }));
        if let Some(state) = self.filter_dialog.as_mut() {
            state.layer_filter = Some(name);
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
                .filter(|name| !name.eq_ignore_ascii_case("Continuous")),
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
        ]
    }

    /// Keep the edited filter as a LAYERFILTER filter and show it in the
    /// Layer manager's tree.
    fn save_layer_filter(&mut self) {
        let i = self.active_tab;
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
        let filter = super::layer_filters::LayerFilter {
            name: name.clone(),
            conditions: Some(state.filter.clone()),
            ..Default::default()
        };
        let matches = state.live_count;
        match self
            .layer_filters
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(&name))
        {
            Some(at) => self.layer_filters[at] = filter,
            None => self.layer_filters.push(filter),
        }
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
            self.tabs[i].scene.set_filter_preview_highlights(&[]);
            if let Some(state) = self.filter_dialog.as_mut() {
                state.live_count = layers.len();
                state.by_type.clear();
                state.by_layer = layers;
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
        self.command_line
            .push_output(crate::tf!("FILTER: {} object(s) selected.", selected).as_ref());
        self.refresh_properties();
        self.unfocus_widgets()
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
            .style(button_style(condition.negate))
            .padding([4, 6])
            .width(Length::Fixed(44.0));

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
        let mut property_options = vec![(String::new(), t!(TYPE_ONLY).into_owned())];
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
            scope_row = scope_row.push(
                button(text(scope.to_string()).size(12))
                    .on_press(msg(FilterMsg::Scope(scope)))
                    .style(button_style(state.scope == scope))
                    .padding([4, 10]),
            );
        }
        let scope_row = scope_row
            .push(Space::new().width(Fill))
            .push(muted(crate::tf!(
                "{} candidate object(s)",
                state.candidate_count
            )));

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
        // Where they are kept, said quietly (#2611: no popup).
        let saved_row = saved_row
            .push(Space::new().width(Fill))
            .push(muted(t!("Saved in this drawing")));

        // ── Rows ──────────────────────────────────────────────────────────
        let layers_mode = state.layer_filter.is_some();
        let intro = if layers_mode {
            t!("Layers that match")
        } else {
            t!("Select objects that match")
        };
        let mut rows = column![row![
            text(intro).size(12),
            match_picker(state.filter.root, |m| msg(FilterMsg::Root(m))),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)]
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
                    rows = rows.push(container(inner).padding(8).width(Fill).style(panel_style));
                }
            }
        }
        let add_buttons = row![
            add_button(t!("Condition"), msg(FilterMsg::AddCondition(None))),
            add_button(t!("Group"), msg(FilterMsg::AddGroup)),
        ]
        .spacing(6);

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
            row![muted(t!("Reads:")), text(sentence).size(12).width(Fill)]
                .spacing(6)
                .align_y(iced::Alignment::Start),
        )
        .padding([6, 8])
        .width(Fill)
        .style(panel_style);

        let editor = column![
            scrollable(rows.padding(iced::Padding {
                right: 12.0,
                ..Default::default()
            }))
            .height(Length::Fixed(300.0)),
            add_buttons,
            reads,
        ]
        .spacing(8)
        .width(Fill);

        // ── Live result ───────────────────────────────────────────────────
        let show_icon = if state.show {
            crate::ui::icons::EYE
        } else {
            crate::ui::icons::EYE_OFF
        };
        let bucket_line =
            |label: String, count: usize, bucket: FilterBucket| -> Element<'a, Message> {
                let on = state.focus.as_ref() == Some(&bucket);
                button(
                    row![
                        text(label).size(11).width(Fill),
                        text(count.to_string()).size(11)
                    ]
                    .spacing(6),
                )
                .on_press(msg(FilterMsg::Focus(Some(bucket))))
                .style(button_style(on))
                .padding([2, 6])
                .width(Fill)
                .into()
            };
        let mut live = column![
            row![
                text(t!("Live result")).size(12).width(Fill),
                button(
                    row![
                        crate::ui::icons::themed(show_icon, 12.0),
                        text(t!("Show")).size(11)
                    ]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(msg(FilterMsg::Show(!state.show)))
                .style(button_style(state.show))
                .padding([3, 8]),
            ]
            .align_y(iced::Alignment::Center),
            text(if layers_mode {
                crate::tf!(
                    "{} of {} layers shown",
                    state.live_count,
                    state.candidate_count
                )
            } else {
                crate::tf!(
                    "{} of {} objects match",
                    state.live_count,
                    state.candidate_count
                )
            })
            .size(13),
        ]
        .spacing(6)
        .width(Length::Fixed(240.0));
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
            // The layers themselves, with their object counts.
            let mut names = column![].spacing(2);
            for (l, n) in &state.by_layer {
                names = names.push(
                    row![text(l.clone()).size(11).width(Fill), muted(n.to_string())].spacing(6),
                );
            }
            live = live.push(scrollable(names).height(Length::Fixed(260.0)));
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
        }
        let live = container(live).padding(10).style(panel_style);

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
            crate::tf!("Select {}", state.live_count),
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
                text(t!("Filter name")).size(12),
                text_input("", name)
                    .on_input(|v| msg(FilterMsg::LayerName(v)))
                    .on_submit(msg(FilterMsg::SaveLayerFilter))
                    .size(12)
                    .width(Length::Fixed(220.0)),
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
            body = body.push(saved_row).push(scope_row);
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
            iced::Size::new(1100.0, f32::INFINITY),
            self.modal_resize,
        );
        crate::ui::modal::modal(
            iced::widget::Space::new()
                .width(iced::Fill)
                .height(iced::Fill),
            crate::t!("Filter"),
            content,
            Message::FilterDialog(FilterMsg::Close),
            self.modal_offset,
            crate::ui::modal::ModalOptions::STANDARD,
        )
    }
}
