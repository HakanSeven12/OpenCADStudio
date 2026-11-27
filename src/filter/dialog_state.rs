//! FILTER's dialog state (#2611): the filter being edited with its scope,
//! the live result and the lists the editor offers. The dialog's behaviour
//! and view are in `app::filter_dialog`.

use super::{Filter, QSelectPropertyChoice, QSelectResult, QSelectSample, QSelectScope};

/// A row of the editor: one at the root, or one inside a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterRow {
    Root(usize),
    Group(usize, usize),
}

/// A line of the live result, clicked to show only those objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterBucket {
    Type(String),
    Layer(String),
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
    pub sample: Option<QSelectSample>,
    /// Layer mode: what makes each found layer match, beside it.
    pub layer_reasons: Vec<String>,
    /// Layer mode: every layer, for From layer.
    pub all_layers: Vec<String>,
}
