//! Named, property-based layer filters (LAYERFILTER).
//!
//! A filter is a name wildcard plus property predicates, with optional nested
//! child filters. The drawing keeps only the layer table; the filters are a
//! session view built by this module. They are used two ways:
//!
//! * `LAYERFILTER LIST <name>` narrows the layer listing to the match.
//! * `-LAYER <option> FILTER:<name>` (or `LAYERFILTER APPLY`) runs an
//!   ordinary layer operation on every match at once.

use iced::Task;

use super::{Message, OpenCADStudio};
use crate::io::xref_model::wildcard_match;

/// One property a layer must satisfy to be in a filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum LayerPredicate {
    Color(u16),
    Linetype(String),
    Off(bool),
    Frozen(bool),
    Locked(bool),
    Plottable(bool),
}

#[derive(Clone, Debug, Default)]
pub(in crate::app) struct LayerFilter {
    pub name: String,
    /// Layer-name wildcard, empty to match every name.
    pub pattern: String,
    pub predicates: Vec<LayerPredicate>,
    /// Nested filters: when present, a layer must match one of them too.
    pub children: Vec<LayerFilter>,
    /// Conditions made in the Layer manager's editor (FILTER's condition
    /// editor over layer properties); used instead of the pattern and the
    /// predicates when present.
    pub conditions: Option<crate::filter::Filter>,
}

/// A layer as the shared filter engine reads it (#2610, #2611): the same
/// comparisons and name patterns as Quick Select and FILTER.
pub(in crate::app) struct LayerProperties;

impl crate::filter::PropertySource for LayerProperties {
    type Item = document::tables::Layer;

    fn type_name<'a>(&'a self, _: &'a document::tables::Layer) -> &'a str {
        "Layer"
    }

    fn value(&self, layer: &document::tables::Layer, property: &str) -> Option<String> {
        let flag = |on: bool| if on { "1" } else { "0" }.to_string();
        Some(match property {
            "name" => layer.name.clone(),
            "description" => layer.description.clone(),
            "color" => layer.color.index()?.to_string(),
            "linetype" => layer.line_type.clone(),
            "on" => flag(!layer.is_off()),
            "frozen" => flag(layer.is_frozen()),
            "locked" => flag(layer.is_locked()),
            "plot" => flag(layer.is_plottable),
            "xref" => flag(layer.name.contains('|') || layer.flags.xref_dependent),
            _ => return None,
        })
    }
}

impl LayerFilter {
    /// The name pattern and the predicates as one all-filter.
    pub(in crate::app) fn to_filter(&self) -> crate::filter::Filter {
        use crate::filter::{Filter, Group, Item, Match};
        // Conditions from the editor (or read back from the drawing), with a
        // name pattern or predicates LAYERFILTER SET added since (#2610).
        if let Some(conditions) = &self.conditions {
            let mut extra = self.legacy_filter();
            if extra.items.is_empty() {
                return conditions.clone();
            }
            if conditions.root == Match::All {
                let mut items = conditions.items.clone();
                items.append(&mut extra.items);
                return Filter {
                    root: Match::All,
                    items,
                };
            }
            if conditions.items.iter().all(|item| matches!(item, Item::Condition(_))) {
                let group = Group {
                    mode: Match::Any,
                    conditions: conditions
                        .items
                        .iter()
                        .filter_map(|item| match item {
                            Item::Condition(c) => Some(c.clone()),
                            Item::Group(_) => None,
                        })
                        .collect(),
                };
                extra.items.push(Item::Group(group));
                return extra;
            }
            return conditions.clone();
        }
        self.legacy_filter()
    }

    /// The name pattern and the predicates alone, as one all-filter.
    fn legacy_filter(&self) -> crate::filter::Filter {
        use crate::filter::{Condition, Filter, Item, Match, Op};
        let condition = |property: &str, value: String| {
            Item::Condition(Condition {
                property: Some(property.to_string()),
                op: Op::Eq,
                value,
                ..Default::default()
            })
        };
        let flag = |on: bool| if on { "1" } else { "0" }.to_string();
        let mut items = Vec::new();
        if !self.pattern.is_empty() {
            items.push(condition("name", self.pattern.clone()));
        }
        for predicate in &self.predicates {
            items.push(match predicate {
                LayerPredicate::Color(index) => condition("color", index.to_string()),
                LayerPredicate::Linetype(name) => condition("linetype", name.clone()),
                LayerPredicate::Off(off) => condition("on", flag(!off)),
                LayerPredicate::Frozen(frozen) => condition("frozen", flag(*frozen)),
                LayerPredicate::Locked(locked) => condition("locked", flag(*locked)),
                LayerPredicate::Plottable(plot) => condition("plot", flag(*plot)),
            });
        }
        Filter {
            root: Match::All,
            items,
        }
    }

    /// The layer passes the name pattern and every predicate, and, when the
    /// filter nests others, one of them too.
    pub(in crate::app) fn matches(&self, layer: &document::tables::Layer) -> bool {
        if !self.to_filter().matches(&LayerProperties, layer) {
            return false;
        }
        self.children.is_empty() || self.children.iter().any(|child| child.matches(layer))
    }
}

/// The filter named `name` anywhere in the tree, nested ones included.
pub(in crate::app) fn find_filter_mut<'a>(
    list: &'a mut [LayerFilter],
    name: &str,
) -> Option<&'a mut LayerFilter> {
    for filter in list {
        if filter.name.eq_ignore_ascii_case(name) {
            return Some(filter);
        }
        if let Some(found) = find_filter_mut(&mut filter.children, name) {
            return Some(found);
        }
    }
    None
}

/// Take the filter named `name` (and the filters under it) out of the tree.
pub(in crate::app) fn remove_filter(list: &mut Vec<LayerFilter>, name: &str) {
    list.retain(|filter| !filter.name.eq_ignore_ascii_case(name));
    for filter in list {
        remove_filter(&mut filter.children, name);
    }
}

impl OpenCADStudio {
    /// The drawing's layer filters as the session works with them: read from
    /// the drawing, so undo, another drawing and a file just opened are
    /// right (#2610).
    pub(in crate::app) fn load_layer_filters(&mut self, i: usize) {
        let stored = super::layer_filter_store::stored_layer_filters(&self.tabs[i].scene.document);
        fn build(
            parent: Option<&str>,
            stored: &[super::layer_filter_store::StoredLayerFilter],
            depth: usize,
        ) -> Vec<LayerFilter> {
            if depth > 16 {
                return Vec::new();
            }
            stored
                .iter()
                .filter(|f| match (&f.parent, parent) {
                    (None, None) => true,
                    (Some(p), Some(q)) => p.eq_ignore_ascii_case(q),
                    _ => false,
                })
                .map(|f| LayerFilter {
                    name: f.name.clone(),
                    pattern: String::new(),
                    predicates: Vec::new(),
                    children: build(Some(&f.name), stored, depth + 1),
                    conditions: Some(f.filter.clone()),
                })
                .collect()
        }
        let mut filters = build(None, &stored, 0);
        // A filter whose parent is gone stays, at the top.
        for f in &stored {
            let placed = |list: &[LayerFilter]| {
                fn any(list: &[LayerFilter], name: &str) -> bool {
                    list.iter()
                        .any(|f| f.name.eq_ignore_ascii_case(name) || any(&f.children, name))
                }
                any(list, &f.name)
            };
            if !placed(&filters) {
                filters.push(LayerFilter {
                    name: f.name.clone(),
                    conditions: Some(f.filter.clone()),
                    ..Default::default()
                });
            }
        }
        self.layer_filters = filters;
    }

    /// Write the session's layer filters into the drawing, one undo step
    /// labelled `label`; nothing when they are already there.
    pub(in crate::app) fn persist_layer_filters(&mut self, i: usize, label: &str) {
        fn flatten(
            list: &[LayerFilter],
            parent: Option<&str>,
            out: &mut Vec<super::layer_filter_store::StoredLayerFilter>,
        ) {
            for f in list {
                out.push(super::layer_filter_store::StoredLayerFilter {
                    name: f.name.clone(),
                    parent: parent.map(str::to_owned),
                    filter: f.to_filter(),
                });
                flatten(&f.children, Some(&f.name), out);
            }
        }
        let mut wanted = Vec::new();
        flatten(&self.layer_filters, None, &mut wanted);
        let document = &self.tabs[i].scene.document;
        if super::layer_filter_store::stored_layer_filters(document) == wanted {
            return;
        }
        self.push_undo_snapshot(i, label);
        super::layer_filter_store::write_layer_filters(&mut self.tabs[i].scene.document, &wanted);
        self.tabs[i].dirty = true;
    }

    pub(in crate::app) fn dispatch_layer_filter(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap_or("");
        if !verb.eq_ignore_ascii_case("LAYERFILTER") && !verb.eq_ignore_ascii_case("-LAYERFILTER")
        {
            return None;
        }
        let rest = parts.next().unwrap_or("").trim();
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let sub = tokens
            .first()
            .map(|token| token.to_ascii_uppercase())
            .unwrap_or_default();
        self.load_layer_filters(i);
        match sub.as_str() {
            "" => self.list_layer_filters(i),
            "NEW" | "CREATE" => self.new_layer_filter(i, &tokens[1..]),
            "SET" | "ADD" => self.set_layer_filter_predicate(i, &tokens[1..]),
            "NEST" | "GROUP" => self.nest_layer_filter(i, &tokens[1..]),
            "LIST" | "SHOW" => self.list_layer_filter_members(i, &tokens[1..]),
            "REMOVE" | "DELETE" => self.remove_layer_filter(i, &tokens[1..]),
            "APPLY" => self.apply_layer_filter(i, &tokens[1..]),
            other => {
                self.command_line.push_error(
                    crate::tf!(
                        "LAYERFILTER: unknown option '{}'. Use NEW, SET, NEST, LIST, REMOVE or APPLY.",
                        other
                    )
                    .as_ref(),
                );
            }
        }
        self.persist_layer_filters(i, "LAYERFILTER");
        Some(Task::none())
    }

    fn matching_layers(&self, i: usize, filter: &LayerFilter) -> Vec<String> {
        self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .filter(|layer| filter.matches(layer))
            .map(|layer| layer.name.clone())
            .collect()
    }

    fn find_layer_filter(&self, name: &str) -> Option<usize> {
        self.layer_filters
            .iter()
            .position(|filter| filter.name.eq_ignore_ascii_case(name))
    }

    fn list_layer_filters(&mut self, i: usize) {
        if self.layer_filters.is_empty() {
            self.command_line
                .push_output(crate::t!("No layer filters.").as_ref());
            return;
        }
        let lines: Vec<String> = self
            .layer_filters
            .iter()
            .map(|filter| {
                format!(
                    "  {} <{}> ({} layer(s))",
                    filter.name,
                    filter.pattern,
                    self.matching_layers(i, filter).len()
                )
            })
            .collect();
        for line in lines {
            self.command_line.push_output(&line);
        }
    }

    fn new_layer_filter(&mut self, i: usize, args: &[&str]) {
        let Some(name) = args.first() else {
            self.command_line
                .push_error(crate::t!("LAYERFILTER: NEW needs a filter name.").as_ref());
            return;
        };
        let pattern = args.get(1).copied().unwrap_or("*").to_string();
        let filter = LayerFilter {
            name: (*name).to_string(),
            pattern,
            predicates: Vec::new(),
            children: Vec::new(),
            conditions: None,
        };
        let matches = self.matching_layers(i, &filter).len();
        match self.find_layer_filter(name) {
            Some(index) => self.layer_filters[index] = filter,
            None => self.layer_filters.push(filter),
        }
        self.command_line.push_output(
            crate::tf!("LAYERFILTER: '{}' saved ({} layer(s) match).", name, matches).as_ref(),
        );
    }

    fn set_layer_filter_predicate(&mut self, i: usize, args: &[&str]) {
        let (Some(name), Some(property)) = (args.first(), args.get(1)) else {
            self.command_line.push_error(
                crate::t!(
                    "LAYERFILTER: SET needs <filter> <property> [value] (COLOR, LINETYPE, ON, OFF, FROZEN, THAWED, LOCKED, UNLOCKED, PLOT, NOPLOT)."
                )
                .as_ref(),
            );
            return;
        };
        let Some(index) = self.find_layer_filter(name) else {
            self.command_line
                .push_error(crate::tf!("LAYERFILTER: '{}' not found.", name).as_ref());
            return;
        };
        let value = args.get(2).copied().unwrap_or("");
        let predicate = match property.to_ascii_uppercase().as_str() {
            "COLOR" => match value.trim().parse::<u16>() {
                Ok(index) => LayerPredicate::Color(index),
                Err(_) => {
                    self.command_line.push_error(
                        crate::t!("LAYERFILTER: COLOR needs an index from 0 to 255.").as_ref(),
                    );
                    return;
                }
            },
            "LINETYPE" | "LTYPE" if !value.is_empty() => {
                LayerPredicate::Linetype(value.to_string())
            }
            "ON" => LayerPredicate::Off(false),
            "OFF" => LayerPredicate::Off(true),
            "FROZEN" => LayerPredicate::Frozen(true),
            "THAWED" => LayerPredicate::Frozen(false),
            "LOCKED" => LayerPredicate::Locked(true),
            "UNLOCKED" => LayerPredicate::Locked(false),
            "PLOT" => LayerPredicate::Plottable(true),
            "NOPLOT" => LayerPredicate::Plottable(false),
            _ => {
                self.command_line.push_error(
                    crate::tf!(
                        "LAYERFILTER: '{}' is not a filter property (COLOR, LINETYPE, ON, OFF, FROZEN, THAWED, LOCKED, UNLOCKED, PLOT, NOPLOT).",
                        property
                    )
                    .as_ref(),
                );
                return;
            }
        };
        self.layer_filters[index].predicates.push(predicate);
        let matches = self.matching_layers(i, &self.layer_filters[index]).len();
        let count = self.layer_filters[index].predicates.len();
        self.command_line.push_output(
            crate::tf!(
                "LAYERFILTER: '{}' has {} predicate(s), {} layer(s) match.",
                name,
                count,
                matches
            )
            .as_ref(),
        );
    }

    fn nest_layer_filter(&mut self, i: usize, args: &[&str]) {
        let (Some(parent), Some(child)) = (args.first(), args.get(1)) else {
            self.command_line.push_error(
                crate::t!("LAYERFILTER: NEST needs a parent and a child filter.").as_ref(),
            );
            return;
        };
        let Some(child_filter) = self
            .find_layer_filter(child)
            .map(|index| self.layer_filters[index].clone())
        else {
            self.command_line
                .push_error(crate::tf!("LAYERFILTER: '{}' not found.", child).as_ref());
            return;
        };
        let Some(parent_index) = self.find_layer_filter(parent) else {
            self.command_line
                .push_error(crate::tf!("LAYERFILTER: '{}' not found.", parent).as_ref());
            return;
        };
        self.layer_filters[parent_index].children.push(child_filter);
        let matches = self.matching_layers(i, &self.layer_filters[parent_index]).len();
        self.command_line.push_output(
            crate::tf!(
                "LAYERFILTER: '{}' nests '{}', {} layer(s) match.",
                parent,
                child,
                matches
            )
            .as_ref(),
        );
    }

    fn list_layer_filter_members(&mut self, i: usize, args: &[&str]) {
        let Some(name) = args.first() else {
            self.list_layer_filters(i);
            return;
        };
        let Some(filter) = self
            .find_layer_filter(name)
            .map(|index| self.layer_filters[index].clone())
        else {
            self.command_line
                .push_error(crate::tf!("LAYERFILTER: '{}' not found.", name).as_ref());
            return;
        };
        let names = self.matching_layers(i, &filter);
        if names.is_empty() {
            self.command_line.push_output(
                crate::tf!("LAYERFILTER: '{}' matches no layers.", name).as_ref(),
            );
            return;
        }
        self.command_line
            .push_output(crate::tf!("Layers in '{}':", name).as_ref());
        for layer in names {
            self.command_line.push_output(&format!("  {layer}"));
        }
    }

    fn remove_layer_filter(&mut self, _i: usize, args: &[&str]) {
        let Some(name) = args.first() else {
            self.command_line
                .push_error(crate::t!("LAYERFILTER: REMOVE needs a filter name.").as_ref());
            return;
        };
        match self.find_layer_filter(name) {
            Some(index) => {
                self.layer_filters.remove(index);
                self.command_line.push_output(
                    crate::tf!("LAYERFILTER: '{}' removed.", name).as_ref(),
                );
            }
            None => self.command_line.push_error(
                crate::tf!("LAYERFILTER: '{}' not found.", name).as_ref(),
            ),
        }
    }

    fn apply_layer_filter(&mut self, i: usize, args: &[&str]) {
        let (Some(name), Some(operation)) = (args.first(), args.get(1)) else {
            self.command_line.push_error(
                crate::t!(
                    "LAYERFILTER: APPLY needs <filter> <OFF|ON|FREEZE|THAW|LOCK|UNLOCK|PLOT|NOPLOT>."
                )
                .as_ref(),
            );
            return;
        };
        let Some(filter) = self
            .find_layer_filter(name)
            .map(|index| self.layer_filters[index].clone())
        else {
            self.command_line
                .push_error(crate::tf!("LAYERFILTER: '{}' not found.", name).as_ref());
            return;
        };
        let names = self.matching_layers(i, &filter);
        if names.is_empty() {
            self.command_line.push_output(
                crate::tf!("LAYERFILTER: '{}' matches no layers.", name).as_ref(),
            );
            return;
        }
        let operation = operation.to_ascii_uppercase();
        let label = format!("LAYERFILTER {operation}");
        match operation.as_str() {
            "OFF" => self.edit_layers(i, &label, &names, |layer| layer.turn_off()),
            "ON" => self.edit_layers(i, &label, &names, |layer| layer.turn_on()),
            "FREEZE" => self.edit_layers(i, &label, &names, |layer| layer.freeze()),
            "THAW" => self.edit_layers(i, &label, &names, |layer| layer.thaw()),
            "LOCK" => self.edit_layers(i, &label, &names, |layer| layer.lock()),
            "UNLOCK" => self.edit_layers(i, &label, &names, |layer| layer.unlock()),
            "PLOT" => self.edit_layers(i, &label, &names, |layer| layer.is_plottable = true),
            "NOPLOT" => self.edit_layers(i, &label, &names, |layer| layer.is_plottable = false),
            _ => {
                self.command_line.push_error(
                    crate::tf!(
                        "LAYERFILTER: '{}' is not a layer operation (OFF, ON, FREEZE, THAW, LOCK, UNLOCK, PLOT, NOPLOT).",
                        operation
                    )
                    .as_ref(),
                );
                return;
            }
        }
        self.command_line.push_output(
            crate::tf!(
                "LAYERFILTER: {} layer(s) in '{}' set to {}.",
                names.len(),
                name,
                operation
            )
            .as_ref(),
        );
    }
}

/// Expand a `-LAYER` name list that may contain `FILTER:<name>` entries.
pub(in crate::app) fn expand_layer_list<'a>(
    layers: impl IntoIterator<Item = &'a document::tables::Layer>,
    filters: &[LayerFilter],
    list: &str,
) -> Vec<String> {
    layers
        .into_iter()
        .filter(|layer| {
            list.split(',')
                .map(str::trim)
                .filter(|pattern| !pattern.is_empty())
                .any(|pattern| {
                    if let Some(name) = pattern
                        .strip_prefix("FILTER:")
                        .or_else(|| pattern.strip_prefix("filter:"))
                    {
                        filters
                            .iter()
                            .find(|filter| filter.name.eq_ignore_ascii_case(name))
                            .is_some_and(|filter| filter.matches(layer))
                    } else {
                        wildcard_match(&layer.name, pattern)
                    }
                })
        })
        .map(|layer| layer.name.clone())
        .collect()
}
