//! VISIBILITYSTATES: authoring the visibility states of a BricsCAD
//! parametric block in model space of the block's own drawing, stored the way
//! BricsCAD stores them. (A block definition stores them the same way, but
//! the anonymous copies its references show would keep the states they had,
//! so the command offers model space only.)
//!
//! BricsCAD keeps, per owner (model space or block definition):
//!
//! - per visibility parameter a configuration feature with a table: one row
//!   per state, one column (`look_n`) per group of objects the parameter
//!   rules, 1 where the group shows in that state;
//! - per group of objects that show and hide together a suppression feature
//!   whose variable is `1 - ` the product of the group's columns, one per
//!   parameter that rules the group, in parameter order;
//! - the variables of all of them in the owner's associative network, and a
//!   dependency and a `BDMOBJECT` per object.
//!
//! An action on objects changes, for each object, the pattern of states in
//! which the parameter shows it:
//!
//! - Add and Visible on an object the parameter does not rule: only in the
//!   state; Invisible: in every state but that one; Add on one it rules
//!   changes nothing;
//! - Visible and Invisible on one it rules: set or clear the state;
//! - Remove: the parameter no longer rules it.
//!
//! Objects whose patterns now agree with a group's join it. When all members
//! of a group change alike the group changes in place; otherwise the changed
//! ones leave it. A group left with no member, or ruled by no parameter, goes
//! with its columns. A new state starts with every group hidden. After every
//! action the owner shows its current states again. All of this is what
//! BricsCAD V26 does for the same commands.

use std::collections::{BTreeMap, BTreeSet};

use document::objects::{
    AssocAction, AssocActionDependency, AssocDependency, AssocEvalValue, AssocEvalVariant,
    AssocNetwork, AssocValueDependency, AssocVariable, AssocVariableDependency, AssociativeData,
    AssociativeObject, Dictionary, ObjectType,
};
use document::{Document, DxfVersion, EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};

use super::record::{Body, Properties, Record, Ref, Value};
use super::switch::{apply_states, set_variable, store, unknown, SUPPRESS_LAYER};
use super::{database_entry, features, record as read_record};
use crate::scene::Scene;

/// What VISIBILITYSTATES does to the picked objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Add,
    Remove,
    Visible,
    Invisible,
    /// Make the state current.
    Select,
    /// Add the state, with every object the parameter rules hidden, and make
    /// it current (the palette's "Add state"; the command makes a state with
    /// its first action).
    NewState,
    /// Move the state one place up or down in the parameter's list, the
    /// order Properties lists the states in.
    MoveUp,
    MoveDown,
}

const UNSUPPORTED: &str = "The visibility states here cannot be edited.";
const OLD_FORMAT: &str = "Visibility states need a drawing in DWG 2010 format or later.";
const NO_PARAMETER: &str = "No visibility parameter of that name.";
const NO_STATE: &str = "No visibility state of that name.";
const STATE_EXISTS: &str = "That visibility state exists already.";
const CANNOT_MOVE: &str = "The visibility state cannot move further.";
const BAD_NAME: &str = "Enter a name of letters, digits and underscores.";
const NAME_TAKEN: &str = "That name is used by another variable.";
const NO_OBJECTS: &str = "Select objects of the space being edited.";

type Result<T> = std::result::Result<T, &'static str>;

/// The classes of BricsCAD's objects, as BricsCAD V26 registers them.
const CLASSES: &[(&str, &str, &str)] = &[
    (
        "BDMDBFEATUREMANAGER",
        "BdmDbFeatureManager",
        "BricsCAD|Description: features data",
    ),
    (
        "BDMDBFEATUREDATA",
        "BdmDbFeatureData",
        "BricsCAD|Description: features data",
    ),
    (
        "BDMDBCONFIGURATIONTABLE",
        "BdmDbConfigurationTable",
        "BricsCAD|Description: configurations data",
    ),
    (
        "BDMDBCONFIGVARREACTOR",
        "BdmDbConfigVarReactor",
        "BricsCAD|Description: features reactor",
    ),
    (
        "BDMDBPERSIDREACTOR",
        "BdmDbPersIdReactor",
        "BricsCAD|Description: features reactor",
    ),
    (
        "BdmDbPersIdManager",
        "BdmDbPersIdManager",
        "BricsCAD|Description: model data",
    ),
    (
        "BDMDBCONSTRAINTMANAGER",
        "BdmDbConstraintManager",
        "BricsCAD|Description: 3D constraints data",
    ),
    (
        "BDMOBJECT",
        "BdmDbObject",
        "BdmModule|Description: Driving Dimension",
    ),
    (
        "BDMDBOBJECTDEPENDENCY",
        "BdmDbObjectDependency",
        "BricsCAD|Description: 3D constraints data",
    ),
    (
        "BDMDBEXPRESSIONREACTOR",
        "BdmDbExpressionReactor",
        "BricsCAD|Description: features reactor",
    ),
];

/// The names BricsCAD gives suppressions and objects, and the description of
/// a column, in the language of its interface. A drawing that has some keeps
/// its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Naming {
    pub(crate) suppression: &'static str,
    pub(crate) description: &'static str,
    pub(crate) object: &'static str,
}

impl Naming {
    pub(crate) const ENGLISH: Self = Self {
        suppression: "Suppress_",
        description: "controlling visibility of entities in ",
        object: "Entity_",
    };
    pub(crate) const GERMAN: Self = Self {
        suppression: "Unterdrücken_",
        description: "Steuerung der Sichtbarkeit von Objekten in ",
        object: "Objekt_",
    };

    /// The names for the interface language `tag` (`de-DE`, …).
    pub(crate) fn for_language(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("de") {
            Self::GERMAN
        } else {
            Self::ENGLISH
        }
    }
}

/// The visibility parameters of an owner, with their states, for the prompts.
/// A visibility parameter of an owner as the palette shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParameterView {
    pub(crate) name: String,
    pub(crate) states: Vec<String>,
    /// The state shown, by index into `states`.
    pub(crate) current: usize,
    /// Each object the parameter rules, in drawing order, with whether it
    /// shows in each state.
    pub(crate) objects: Vec<(Handle, Vec<bool>)>,
}

/// The visibility parameters of `owner` with the objects each rules; `None`
/// when they cannot be edited here.
pub(crate) fn parameter_views(document: &Document, owner: Handle) -> Option<Vec<ParameterView>> {
    let setup = Setup::read(document, owner).ok()?;
    let order: FxHashMap<Handle, usize> = owner_members(document, owner)
        .into_iter()
        .enumerate()
        .map(|(k, handle)| (handle, k))
        .collect();
    Some(
        setup
            .parameters
            .iter()
            .enumerate()
            .map(|(p, parameter)| {
                let mut objects: Vec<(Handle, Vec<bool>)> = setup
                    .groups
                    .iter()
                    .enumerate()
                    .filter_map(|(g, group)| {
                        let pattern = setup.signature(g).remove(&p)?;
                        Some(
                            group
                                .members
                                .iter()
                                .map(move |member| (member.entity, pattern.clone())),
                        )
                    })
                    .flatten()
                    .collect();
                objects.sort_by_key(|(entity, _)| order.get(entity).copied().unwrap_or(usize::MAX));
                ParameterView {
                    name: parameter.name.clone(),
                    states: parameter.states.clone(),
                    current: parameter
                        .current
                        .min(parameter.states.len().saturating_sub(1)),
                    objects,
                }
            })
            .collect(),
    )
}

/// The layer each member of `owner` is on when no state hides it.
pub(crate) fn shown_layers(document: &Document, owner: Handle) -> FxHashMap<Handle, String> {
    original_layers(document, owner)
}

pub(crate) fn parameter_states(document: &Document, owner: Handle) -> Vec<(String, Vec<String>)> {
    Setup::read(document, owner)
        .map(|setup| {
            setup
                .parameters
                .into_iter()
                .map(|parameter| (parameter.name, parameter.states))
                .collect()
        })
        .unwrap_or_default()
}

impl Scene {
    /// VISIBILITYSTATES on `owner` (model space or a block definition): apply
    /// `action` in `state` of `parameter` to `objects`, making the parameter
    /// and the state when they are new. Names match regardless of case. On an
    /// error the document may be part-way changed; callers work on a copy or
    /// take an undo snapshot first.
    pub(crate) fn edit_bricscad_visibility(
        &mut self,
        owner: Handle,
        parameter: &str,
        state: &str,
        action: Action,
        objects: &[Handle],
    ) -> Result<()> {
        let language = crate::i18n::loader()
            .current_languages()
            .into_iter()
            .next()
            .map(|language| language.to_string())
            .unwrap_or_default();
        let naming = Naming::for_language(&language);
        self.edit_bricscad_visibility_named(owner, parameter, state, action, objects, naming)
    }

    /// [`Self::edit_bricscad_visibility`] with the names for a drawing that has
    /// none yet.
    pub(crate) fn edit_bricscad_visibility_named(
        &mut self,
        owner: Handle,
        parameter: &str,
        state: &str,
        action: Action,
        objects: &[Handle],
        naming: Naming,
    ) -> Result<()> {
        if self.document.version < DxfVersion::AC1024 {
            return Err(OLD_FORMAT);
        }
        // What the editor holds goes into the drawing first, so the reload at
        // the end loses nothing.
        self.sync_native_parametric_graph();
        let document = &self.document;
        let layers = original_layers(document, owner);
        let mut setup = Setup::read(document, owner)?;
        setup.learn_names(document, naming);
        let parameter = parameter.trim();
        let state = state.trim();
        if state.is_empty() {
            return Err(NO_STATE);
        }
        let p = match setup
            .parameters
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(parameter))
        {
            Some(p) => p,
            None if !matches!(action, Action::Add | Action::Visible | Action::Invisible) => {
                return Err(NO_PARAMETER)
            }
            None => {
                if !valid_name(parameter) {
                    return Err(BAD_NAME);
                }
                if setup
                    .variable_names(document)
                    .contains(&parameter.to_lowercase())
                {
                    return Err(NAME_TAKEN);
                }
                setup.parameters.push(Parameter::new(parameter));
                setup.parameters.len() - 1
            }
        };
        let s = match setup.parameters[p]
            .states
            .iter()
            .position(|s| s.eq_ignore_ascii_case(state))
        {
            Some(_) if action == Action::NewState => return Err(STATE_EXISTS),
            Some(s) => s,
            None if matches!(
                action,
                Action::Select | Action::Remove | Action::MoveUp | Action::MoveDown
            ) =>
            {
                return Err(NO_STATE)
            }
            None => {
                if !valid_name(state) {
                    return Err(BAD_NAME);
                }
                setup.parameters[p].add_state(state)
            }
        };
        if matches!(action, Action::MoveUp | Action::MoveDown) {
            let parameter = &mut setup.parameters[p];
            let to = if action == Action::MoveUp {
                s.checked_sub(1)
            } else {
                (s + 1 < parameter.states.len()).then_some(s + 1)
            };
            parameter.swap_states(s, to.ok_or(CANNOT_MOVE)?);
        } else if matches!(action, Action::Select | Action::NewState) {
            setup.parameters[p].current = s;
        } else {
            // BricsCAD works through a selection from its last object.
            let mut seen = FxHashSet::default();
            let objects: Vec<Handle> = objects
                .iter()
                .rev()
                .copied()
                .filter(|handle| {
                    seen.insert(*handle)
                        && document
                            .get_entity(*handle)
                            .is_some_and(|entity| entity.common().owner_handle == owner)
                })
                .collect();
            if objects.is_empty() {
                return Err(NO_OBJECTS);
            }
            setup.act(p, s, action, &objects);
        }
        if setup.parameters[p].states.is_empty()
            || setup.parameters[p].columns.is_empty() && setup.parameters[p].feature.is_none()
        {
            // A new parameter that came to rule nothing is not made.
            return Err(NO_OBJECTS);
        }
        let states: BTreeMap<String, String> = setup
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    parameter.states[parameter.current.min(parameter.states.len() - 1)].clone(),
                )
            })
            .collect();
        let parameter_name = setup.parameters[p].name.clone();
        setup.write(&mut self.document)?;
        let members = owner_members(&self.document, owner);
        apply_states(
            &mut self.document,
            owner,
            &members,
            &layers,
            &states,
            &BTreeSet::new(),
            true,
        )?;
        let chosen = matches!(action, Action::Select | Action::NewState).then_some(parameter_name);
        number_keys(&mut self.document, owner, chosen.as_deref())?;
        self.load_named_parameters_from_document();
        self.load_parametric_constraints_from_document();
        Ok(())
    }
}

/// Give each enabled key the number of the state it shows: BricsCAD shows the
/// state of that number again when it next evaluates the owner. A disabled
/// key is left alone; BricsCAD reads its state by name, and its variable may
/// hold an expression of its own (openCirt's templates have keys showing the
/// second state over a variable `0`). The key of `chosen`, the parameter a
/// state was picked for, is set the way BricsCAD sets it when it shows a
/// state: enabled, holding the state's number, its variable that number with
/// no expression.
fn number_keys(document: &mut Document, owner: Handle, chosen: Option<&str>) -> Result<()> {
    for feature in features(document, owner) {
        let Some(properties) = super::properties(document, feature) else {
            continue;
        };
        if properties.text("Feature_type") != Some("BdmConfigurationFeature")
            || properties.text("TableType") != Some("VisibilityStates")
        {
            continue;
        }
        let states: Vec<String> = properties
            .get("ConfigTable")
            .and_then(Value::object)
            .and_then(|table| super::properties(document, table))
            .and_then(|table| {
                table
                    .get("KeyValues")?
                    .items()?
                    .iter()
                    .map(|state| state.text().map(str::to_string))
                    .collect()
            })
            .ok_or(UNSUPPORTED)?;
        let is_chosen = chosen.is_some_and(|name| {
            properties
                .text("Name")
                .is_some_and(|own| own.eq_ignore_ascii_case(name))
        });
        let reactors: Vec<Handle> = properties
            .get("Reactors")
            .and_then(Value::items)
            .map(|items| items.iter().filter_map(Value::object).collect())
            .unwrap_or_default();
        for reactor in reactors {
            let mut record = read_record(document, reactor).ok_or(UNSUPPORTED)?;
            let properties = record.body.properties_mut();
            if properties.get("IsKeyVar") != Some(&Value::Flag(true)) {
                continue;
            }
            if !is_chosen && properties.get("IsEnabled") == Some(&Value::Flag(false)) {
                continue;
            }
            let Some(row) = properties
                .text("String")
                .and_then(|state| states.iter().position(|name| name == state))
            else {
                continue;
            };
            let variable = properties
                .get("VarId")
                .and_then(Value::object)
                .ok_or(UNSUPPORTED)?;
            properties.set("Value", Value::Real(row as f64));
            if is_chosen {
                properties.set("IsEnabled", Value::Flag(true));
            }
            store(document, &record);
            let real = AssocEvalVariant {
                code: 40,
                value: AssocEvalValue::Real(row as f64),
            };
            if is_chosen {
                set_variable(document, variable, Some(String::new()), real)?;
            } else {
                let value = match variable_value(document, variable).value {
                    AssocEvalValue::Long(_) => AssocEvalVariant {
                        code: 90,
                        value: AssocEvalValue::Long(row as i32),
                    },
                    _ => real,
                };
                set_variable(document, variable, None, value)?;
            }
        }
    }
    Ok(())
}

/// Every member of `owner`, in drawing order.
fn owner_members(document: &Document, owner: Handle) -> Vec<Handle> {
    document
        .entities()
        .filter(|entity| {
            entity.common().owner_handle == owner
                && !matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_))
        })
        .map(|entity| entity.common().handle)
        .collect()
}

/// Each member's layer as it is when no state hides it: a member on
/// `*SUPPRESS` takes the layer its suppression keeps for it.
fn original_layers(document: &Document, owner: Handle) -> FxHashMap<Handle, String> {
    let mut kept: FxHashMap<Handle, Handle> = FxHashMap::default();
    for feature in features(document, owner) {
        let Some(properties) = super::properties(document, feature) else {
            continue;
        };
        if let Some(entries) = properties.get("aEntLayerKey").and_then(Value::entries) {
            for (entity, layer) in entries {
                if let (Some(entity), Some(layer)) = (entity.object(), layer.object()) {
                    kept.insert(entity, layer);
                }
            }
        }
    }
    let layer_name = |handle: Handle| {
        document
            .layers
            .iter()
            .find(|layer| layer.handle == handle)
            .map(|layer| layer.name.clone())
    };
    owner_members(document, owner)
        .into_iter()
        .filter_map(|member| {
            let layer = &document.get_entity(member)?.common().layer;
            let layer = if layer == SUPPRESS_LAYER {
                kept.get(&member)
                    .and_then(|handle| layer_name(*handle))
                    .unwrap_or_else(|| "0".to_string())
            } else {
                layer.clone()
            };
            Some((member, layer))
        })
        .collect()
}

/// Letters (any script), digits and underscores, not starting with a digit.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// One column of a parameter's table: a group's pattern under it.
#[derive(Debug, Clone)]
struct Column {
    name: String,
    values: Vec<bool>,
    reactor: Option<Handle>,
    variable: Option<Handle>,
}

#[derive(Debug, Clone)]
struct Parameter {
    name: String,
    states: Vec<String>,
    current: usize,
    columns: Vec<Column>,
    feature: Option<Handle>,
    table: Option<Handle>,
    key_reactor: Option<Handle>,
    key_variable: Option<Handle>,
}

impl Parameter {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            states: Vec::new(),
            current: 0,
            columns: Vec::new(),
            feature: None,
            table: None,
            key_reactor: None,
            key_variable: None,
        }
    }

    /// A new state, in which every group is hidden.
    /// Swap two states with their rows; the shown state stays shown.
    fn swap_states(&mut self, a: usize, b: usize) {
        self.states.swap(a, b);
        for column in &mut self.columns {
            column.values.swap(a, b);
        }
        if self.current == a {
            self.current = b;
        } else if self.current == b {
            self.current = a;
        }
    }

    fn add_state(&mut self, name: &str) -> usize {
        self.states.push(name.to_string());
        for column in &mut self.columns {
            column.values.push(false);
        }
        self.states.len() - 1
    }
}

#[derive(Debug, Clone)]
struct Member {
    entity: Handle,
    dependency: Option<Handle>,
}

#[derive(Debug, Clone)]
struct Group {
    name: String,
    /// The columns the suppression multiplies: parameter index and column
    /// name, in parameter order.
    columns: Vec<(usize, String)>,
    members: Vec<Member>,
    feature: Option<Handle>,
    persid: Option<Handle>,
    expression: Option<Handle>,
    variable: Option<Handle>,
    /// The name of the group's variable; BricsCAD's own drawings have groups
    /// whose variable is named after another group (`Unterdrücken_1` with
    /// the variable `Unterdrücken_7`).
    variable_name: String,
    columns_changed: bool,
}

/// Per parameter, the states a group shows in.
type Signature = BTreeMap<usize, Vec<bool>>;

/// One owner's visibility parameters and groups, read from its objects and
/// written back.
struct Setup {
    owner: Handle,
    parameters: Vec<Parameter>,
    groups: Vec<Group>,
    /// Columns, groups and dependencies that go, by their objects.
    erased_columns: Vec<Column>,
    erased_groups: Vec<Group>,
    released: Vec<Handle>,
    suppress_prefix: Option<String>,
    description_prefix: Option<String>,
    object_prefix: Option<String>,
    /// The lowercased names of the network's variables that are no
    /// parameter's, column's or group's.
    foreign: FxHashSet<String>,
}

impl Setup {
    fn read(document: &Document, owner: Handle) -> Result<Self> {
        let mut setup = Self {
            owner,
            parameters: Vec::new(),
            groups: Vec::new(),
            erased_columns: Vec::new(),
            erased_groups: Vec::new(),
            released: Vec::new(),
            suppress_prefix: None,
            description_prefix: None,
            object_prefix: None,
            foreign: FxHashSet::default(),
        };
        let variable = |handle: Handle| match document.objects.get(&handle) {
            Some(ObjectType::Associative(AssociativeObject {
                data: AssociativeData::Variable(variable),
                ..
            })) => Some(variable),
            _ => None,
        };
        let handles = |properties: &Properties, name: &str| -> Vec<Handle> {
            properties
                .get(name)
                .and_then(Value::items)
                .map(|items| items.iter().filter_map(Value::object).collect())
                .unwrap_or_default()
        };
        let mut suppressions = Vec::new();
        for feature in features(document, owner) {
            let properties = super::properties(document, feature).ok_or(UNSUPPORTED)?;
            match properties.text("Feature_type") {
                Some("BdmConfigurationFeature") => {
                    if properties.text("TableType") != Some("VisibilityStates") {
                        return Err(UNSUPPORTED);
                    }
                    let table_handle = properties
                        .get("ConfigTable")
                        .and_then(Value::object)
                        .ok_or(UNSUPPORTED)?;
                    let table = super::properties(document, table_handle).ok_or(UNSUPPORTED)?;
                    let texts = |key: &str| -> Option<Vec<String>> {
                        table
                            .get(key)?
                            .items()?
                            .iter()
                            .map(|item| item.text().map(str::to_string))
                            .collect()
                    };
                    let states = texts("KeyValues").ok_or(UNSUPPORTED)?;
                    let names = texts("ColNames").ok_or(UNSUPPORTED)?;
                    let values: Vec<f64> = table
                        .get("DblValues")
                        .and_then(Value::items)
                        .and_then(|items| {
                            items
                                .iter()
                                .map(|item| match item {
                                    Value::Real(value) => Some(*value),
                                    _ => None,
                                })
                                .collect()
                        })
                        .ok_or(UNSUPPORTED)?;
                    if values.len() != states.len() * names.len() {
                        return Err(UNSUPPORTED);
                    }
                    let mut reactors: FxHashMap<String, (Handle, Handle)> = FxHashMap::default();
                    let mut key = None;
                    for reactor in handles(&properties, "Reactors") {
                        let reactor_properties =
                            super::properties(document, reactor).ok_or(UNSUPPORTED)?;
                        let variable = reactor_properties
                            .get("VarId")
                            .and_then(Value::object)
                            .ok_or(UNSUPPORTED)?;
                        if reactor_properties.get("IsKeyVar") == Some(&Value::Flag(true)) {
                            key = Some((reactor, variable));
                        } else {
                            let name = reactor_properties
                                .text("VarName")
                                .ok_or(UNSUPPORTED)?
                                .to_string();
                            reactors.insert(name, (reactor, variable));
                        }
                    }
                    let (key_reactor, key_variable) = key.ok_or(UNSUPPORTED)?;
                    let columns = names
                        .iter()
                        .enumerate()
                        .map(|(c, name)| {
                            let (reactor, variable) = reactors.get(name).ok_or(UNSUPPORTED)?;
                            if setup.description_prefix.is_none() {
                                setup.description_prefix = description_prefix(document, *variable);
                            }
                            Ok(Column {
                                name: name.clone(),
                                values: (0..states.len())
                                    .map(|row| values[row * names.len() + c] != 0.0)
                                    .collect(),
                                reactor: Some(*reactor),
                                variable: Some(*variable),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let name = table
                        .text("KeyName")
                        .or_else(|| properties.text("Name"))
                        .ok_or(UNSUPPORTED)?
                        .to_string();
                    let current = properties
                        .text("LastKeyValue")
                        .and_then(|shown| states.iter().position(|state| state == shown))
                        .unwrap_or(0);
                    setup.parameters.push(Parameter {
                        name,
                        states,
                        current,
                        columns,
                        feature: Some(feature),
                        table: Some(table_handle),
                        key_reactor: Some(key_reactor),
                        key_variable: Some(key_variable),
                    });
                }
                Some("BdmSuppressFeature") => suppressions.push((feature, properties)),
                // Model space of a drawing also holds the entries of block
                // references; they are not this owner's parameters.
                _ => {}
            }
        }
        for (feature, properties) in suppressions {
            let name = properties.text("Name").ok_or(UNSUPPORTED)?.to_string();
            let mut persid = None;
            let mut expression = None;
            let mut members = Vec::new();
            let mut suppress_variable = None;
            for reactor in handles(&properties, "Reactors") {
                let reactor_properties = super::properties(document, reactor).ok_or(UNSUPPORTED)?;
                if reactor_properties.get("pers_ids").is_some() {
                    persid = Some(reactor);
                    for dependency in handles(&reactor_properties, "pers_ids") {
                        match read_record(document, dependency).map(|record| record.body) {
                            Some(Body::Dependency { entity, .. }) => members.push(Member {
                                entity: entity.handle,
                                dependency: Some(dependency),
                            }),
                            _ => return Err(UNSUPPORTED),
                        }
                    }
                } else if let Some(variable) = reactor_properties.get("var").and_then(Value::object)
                {
                    expression = Some(reactor);
                    suppress_variable = Some(variable);
                }
            }
            let variable_handle = suppress_variable.ok_or(UNSUPPORTED)?;
            let expression_text = &variable(variable_handle).ok_or(UNSUPPORTED)?.expression;
            let factors: Vec<String> = expression_text
                .split_once('-')
                .filter(|(one, _)| one.trim() == "1")
                .map(|(_, product)| product.split('*').map(|f| f.trim().to_string()).collect())
                .ok_or(UNSUPPORTED)?;
            let mut columns = Vec::new();
            for factor in factors {
                let p = setup
                    .parameters
                    .iter()
                    .position(|p| p.columns.iter().any(|c| c.name == factor))
                    .ok_or(UNSUPPORTED)?;
                columns.push((p, factor));
            }
            if setup.suppress_prefix.is_none() {
                setup.suppress_prefix = name
                    .rsplit_once('_')
                    .filter(|(_, n)| n.parse::<u32>().is_ok())
                    .map(|(prefix, _)| format!("{prefix}_"));
            }
            setup.groups.push(Group {
                name,
                columns,
                members,
                feature: Some(feature),
                persid: Some(persid.ok_or(UNSUPPORTED)?),
                expression: Some(expression.ok_or(UNSUPPORTED)?),
                variable: Some(variable_handle),
                variable_name: variable(variable_handle).ok_or(UNSUPPORTED)?.name.clone(),
                columns_changed: false,
            });
        }
        // Each column belongs to one group.
        let mut used = FxHashSet::default();
        for group in &setup.groups {
            for (_, column) in &group.columns {
                if !used.insert(column.clone()) {
                    return Err(UNSUPPORTED);
                }
            }
        }
        if let Some(network) = owner_network(document, owner) {
            let managed = setup.taken();
            setup.foreign = document
                .objects
                .values()
                .filter_map(|object| match object {
                    ObjectType::Associative(AssociativeObject {
                        data: AssociativeData::Variable(variable),
                        ..
                    }) if variable.action.owning_network == network => {
                        Some(variable.name.to_lowercase())
                    }
                    _ => None,
                })
                .filter(|name| !managed.contains(name))
                .collect();
        }
        Ok(setup)
    }

    /// Fill in the names the owner does not show: from the rest of the
    /// drawing, else `naming`.
    fn learn_names(&mut self, document: &Document, naming: Naming) {
        if self.object_prefix.is_none() {
            self.object_prefix = document.objects.iter().find_map(|(handle, object)| {
                matches!(object, ObjectType::Unknown { type_name, .. } if type_name == "BDMOBJECT")
                    .then(|| read_record(document, *handle))
                    .flatten()
                    .and_then(|record| {
                        let name = record.body.properties().text("NAME")?.to_string();
                        let (prefix, number) = name.rsplit_once('_')?;
                        number.parse::<u32>().ok()?;
                        Some(format!("{prefix}_"))
                    })
            });
        }
        if self.suppress_prefix.is_none() || self.description_prefix.is_none() {
            for (handle, object) in &document.objects {
                if !matches!(object, ObjectType::Unknown { type_name, .. } if type_name == "BDMDBFEATUREDATA")
                {
                    continue;
                }
                let Some(properties) = super::properties(document, *handle) else {
                    continue;
                };
                if properties.text("Feature_type") != Some("BdmSuppressFeature") {
                    continue;
                }
                let Some(name) = properties.text("Name") else {
                    continue;
                };
                if self.suppress_prefix.is_none() {
                    self.suppress_prefix = name
                        .rsplit_once('_')
                        .filter(|(_, n)| n.parse::<u32>().is_ok())
                        .map(|(prefix, _)| format!("{prefix}_"));
                }
                if self.description_prefix.is_none() {
                    self.description_prefix =
                        document.objects.values().find_map(|object| match object {
                            ObjectType::Associative(AssociativeObject {
                                data: AssociativeData::Variable(variable),
                                ..
                            }) => variable
                                .description
                                .strip_suffix(name)
                                .filter(|prefix| !prefix.is_empty())
                                .map(str::to_string),
                            _ => None,
                        });
                }
                if self.suppress_prefix.is_some() && self.description_prefix.is_some() {
                    break;
                }
            }
        }
        self.suppress_prefix
            .get_or_insert_with(|| naming.suppression.to_string());
        self.description_prefix
            .get_or_insert_with(|| naming.description.to_string());
        self.object_prefix
            .get_or_insert_with(|| naming.object.to_string());
    }

    /// The lowercased names of the owner's variables.
    /// The lowercased names a new column or group may not take: every
    /// variable of the network but those this edit erases.
    fn taken(&self) -> FxHashSet<String> {
        self.parameters
            .iter()
            .flat_map(|p| {
                std::iter::once(p.name.to_lowercase())
                    .chain(p.columns.iter().map(|c| c.name.to_lowercase()))
            })
            .chain(
                self.groups
                    .iter()
                    .flat_map(|g| [g.name.to_lowercase(), g.variable_name.to_lowercase()]),
            )
            .chain(self.foreign.iter().cloned())
            .collect()
    }

    fn variable_names(&self, document: &Document) -> FxHashSet<String> {
        let mut names: FxHashSet<String> = self
            .parameters
            .iter()
            .flat_map(|p| {
                std::iter::once(p.name.to_lowercase())
                    .chain(p.columns.iter().map(|c| c.name.to_lowercase()))
            })
            .chain(self.groups.iter().map(|g| g.name.to_lowercase()))
            .collect();
        if let Some(network) = owner_network(document, self.owner) {
            for object in document.objects.values() {
                if let ObjectType::Associative(AssociativeObject {
                    data: AssociativeData::Variable(variable),
                    ..
                }) = object
                {
                    if variable.action.owning_network == network {
                        names.insert(variable.name.to_lowercase());
                    }
                }
            }
        }
        names
    }

    fn signature(&self, group: usize) -> Signature {
        self.groups[group]
            .columns
            .iter()
            .map(|(p, name)| {
                let values = self.parameters[*p]
                    .columns
                    .iter()
                    .find(|column| &column.name == name)
                    .map(|column| column.values.clone())
                    .unwrap_or_default();
                (*p, values)
            })
            .collect()
    }

    fn group_of(&self, entity: Handle) -> Option<usize> {
        self.groups
            .iter()
            .position(|group| group.members.iter().any(|member| member.entity == entity))
    }

    /// Apply `action` in state `s` of parameter `p` to `objects`.
    fn act(&mut self, p: usize, s: usize, action: Action, objects: &[Handle]) {
        let states = self.parameters[p].states.len();
        let mut changes: Vec<(Option<usize>, Signature, Handle)> = Vec::new();
        for &entity in objects {
            let group = self.group_of(entity);
            let old = group.map(|g| self.signature(g)).unwrap_or_default();
            let mut new = old.clone();
            match (old.get(&p), action) {
                (Some(_), Action::Add)
                | (None, Action::Remove)
                | (_, Action::Select | Action::NewState | Action::MoveUp | Action::MoveDown) => {}
                (Some(_), Action::Remove) => {
                    new.remove(&p);
                }
                (Some(pattern), Action::Visible | Action::Invisible) => {
                    let mut pattern = pattern.clone();
                    pattern[s] = action == Action::Visible;
                    new.insert(p, pattern);
                }
                (None, Action::Add | Action::Visible) => {
                    new.insert(p, (0..states).map(|k| k == s).collect());
                }
                (None, Action::Invisible) => {
                    new.insert(p, (0..states).map(|k| k != s).collect());
                }
            }
            if new != old {
                changes.push((group, new, entity));
            }
        }
        // Objects of one group that change alike change together.
        let mut batches: Vec<(Option<usize>, Signature, Vec<Handle>)> = Vec::new();
        for (group, signature, entity) in changes {
            match batches
                .iter_mut()
                .find(|(g, sig, _)| *g == group && *sig == signature)
            {
                Some((_, _, entities)) => entities.push(entity),
                None => batches.push((group, signature, vec![entity])),
            }
        }
        for (group, signature, entities) in batches {
            let whole = group.is_some_and(|g| {
                let members: BTreeSet<Handle> =
                    self.groups[g].members.iter().map(|m| m.entity).collect();
                members == entities.iter().copied().collect::<BTreeSet<Handle>>()
            });
            match group {
                Some(g) if whole => self.reshape(g, &signature),
                _ => {
                    let mut moved: Vec<Member> = Vec::new();
                    if let Some(g) = group {
                        let members = std::mem::take(&mut self.groups[g].members);
                        let (leaving, staying): (Vec<_>, Vec<_>) = members
                            .into_iter()
                            .partition(|member| entities.contains(&member.entity));
                        self.groups[g].members = staying;
                        moved = leaving;
                    }
                    for entity in &entities {
                        if !moved.iter().any(|member| member.entity == *entity) {
                            moved.push(Member {
                                entity: *entity,
                                dependency: None,
                            });
                        }
                    }
                    if signature.is_empty() {
                        self.released
                            .extend(moved.iter().filter_map(|member| member.dependency));
                        continue;
                    }
                    let target = (0..self.groups.len()).find(|&h| {
                        !self.groups[h].members.is_empty() && self.signature(h) == signature
                    });
                    match target {
                        Some(h) => self.groups[h].members.extend(moved),
                        None => self.add_group(&signature, moved),
                    }
                }
            }
        }
        // Groups left with no member go.
        let mut g = 0;
        while g < self.groups.len() {
            if self.groups[g].members.is_empty() {
                self.erase_group(g);
            } else {
                g += 1;
            }
        }
    }

    /// Change group `g` to `signature` in place.
    fn reshape(&mut self, g: usize, signature: &Signature) {
        if signature.is_empty() {
            let members = std::mem::take(&mut self.groups[g].members);
            self.released
                .extend(members.into_iter().filter_map(|member| member.dependency));
            return;
        }
        for (&p, pattern) in signature {
            let existing = self.groups[g]
                .columns
                .iter()
                .find(|(q, _)| *q == p)
                .map(|(_, name)| name.clone());
            match existing {
                Some(name) => {
                    if let Some(column) = self.parameters[p]
                        .columns
                        .iter_mut()
                        .find(|c| c.name == name)
                    {
                        column.values = pattern.clone();
                    }
                }
                None => {
                    let name = self.add_column(p, pattern.clone());
                    let group = &mut self.groups[g];
                    group.columns.push((p, name));
                    group.columns.sort_by_key(|(q, _)| *q);
                    group.columns_changed = true;
                }
            }
        }
        let dropped: Vec<(usize, String)> = self.groups[g]
            .columns
            .iter()
            .filter(|(p, _)| !signature.contains_key(p))
            .cloned()
            .collect();
        for (p, name) in dropped {
            self.erase_column(p, &name);
            self.groups[g].columns.retain(|(_, column)| *column != name);
            self.groups[g].columns_changed = true;
        }
    }

    fn add_group(&mut self, signature: &Signature, members: Vec<Member>) {
        let prefix = self.suppress_prefix.clone().unwrap_or_default();
        let taken = self.taken();
        let mut number = self
            .groups
            .iter()
            .filter_map(|group| group.name.strip_prefix(&prefix)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        // BricsCAD finds variables by name: a second variable of the same
        // name gets the two mixed up.
        while taken.contains(&format!("{prefix}{number}").to_lowercase()) {
            number += 1;
        }
        let name = format!("{prefix}{number}");
        let columns = signature
            .iter()
            .map(|(&p, pattern)| (p, self.add_column(p, pattern.clone())))
            .collect();
        self.groups.push(Group {
            variable_name: name.clone(),
            name,
            columns,
            members,
            feature: None,
            persid: None,
            expression: None,
            variable: None,
            columns_changed: true,
        });
    }

    /// A new column of parameter `p`, named with the lowest free number.
    fn add_column(&mut self, p: usize, values: Vec<bool>) -> String {
        let taken = self.taken();
        let number = (1..)
            .find(|n| !taken.contains(&format!("look_{n}")))
            .unwrap_or(1);
        let name = format!("look_{number}");
        self.parameters[p].columns.push(Column {
            name: name.clone(),
            values,
            reactor: None,
            variable: None,
        });
        name
    }

    fn erase_column(&mut self, p: usize, name: &str) {
        if let Some(index) = self.parameters[p]
            .columns
            .iter()
            .position(|c| c.name == name)
        {
            let column = self.parameters[p].columns.remove(index);
            self.erased_columns.push(column);
        }
    }

    fn erase_group(&mut self, g: usize) {
        let group = self.groups.remove(g);
        for (p, name) in &group.columns {
            self.erase_column(*p, name);
        }
        self.released
            .extend(group.members.iter().filter_map(|member| member.dependency));
        self.erased_groups.push(group);
    }

    /// Write the setup back into the owner's objects.
    fn write(mut self, document: &mut Document) -> Result<()> {
        let version = document.version;
        let types = Types::ensure(document);
        let database = ensure_database(document, self.owner, &types, version)?;
        let network = ensure_network(document, self.owner)?;
        let description_prefix = self.description_prefix.clone().unwrap_or_default();
        let object_prefix = self.object_prefix.clone().unwrap_or_default();
        let mut new_features = Vec::new();

        // Columns and groups that go: their variables leave the network
        // after the suppressions that read them are rewritten below.
        let mut erased_variables: Vec<Handle> = Vec::new();
        let mut erased_objects: Vec<Handle> = Vec::new();
        for column in &self.erased_columns {
            erased_variables.extend(column.variable);
            erased_objects.extend(column.reactor);
        }
        for group in &self.erased_groups {
            erased_variables.extend(group.variable);
            erased_objects.extend(group.feature);
            erased_objects.extend(group.persid);
            erased_objects.extend(group.expression);
        }

        // Group names, for the columns' descriptions.
        let mut column_group: FxHashMap<String, String> = FxHashMap::default();
        for group in &self.groups {
            for (_, column) in &group.columns {
                column_group.insert(column.clone(), group.name.clone());
            }
        }

        for parameter in &mut self.parameters {
            let current = parameter
                .current
                .min(parameter.states.len().saturating_sub(1));
            let key_variable = match parameter.key_variable {
                Some(variable) => variable,
                None => new_variable(
                    document,
                    network,
                    &parameter.name,
                    "",
                    "",
                    AssocEvalVariant {
                        code: 40,
                        value: AssocEvalValue::Real(0.0),
                    },
                ),
            };
            parameter.key_variable = Some(key_variable);
            let feature = parameter
                .feature
                .unwrap_or_else(|| document.allocate_handle());
            let table = parameter
                .table
                .unwrap_or_else(|| document.allocate_handle());
            if parameter.key_reactor.is_none() {
                let handle = document.allocate_handle();
                let record = Record::new(
                    types.config_reactor,
                    handle,
                    Ref::new(feature, 4),
                    Body::Properties(variable_reactor(
                        true,
                        &parameter.states[current],
                        0.0,
                        key_variable,
                        &parameter.name,
                    )),
                    version,
                );
                document
                    .objects
                    .insert(handle, unknown("BDMDBCONFIGVARREACTOR", &record));
                add_reactor(document, key_variable, handle);
                parameter.key_reactor = Some(handle);
            }
            for column in &mut parameter.columns {
                let value = column.values.get(current).copied().unwrap_or(false);
                let variable = match column.variable {
                    Some(variable) => variable,
                    None => {
                        let group = column_group.get(&column.name).cloned().unwrap_or_default();
                        new_variable(
                            document,
                            network,
                            &column.name,
                            "",
                            &format!("{description_prefix}{group}"),
                            AssocEvalVariant {
                                code: 40,
                                value: AssocEvalValue::Real(f64::from(u8::from(value))),
                            },
                        )
                    }
                };
                column.variable = Some(variable);
                if column.reactor.is_none() {
                    let handle = document.allocate_handle();
                    let record = Record::new(
                        types.config_reactor,
                        handle,
                        Ref::new(feature, 4),
                        Body::Properties(variable_reactor(
                            false,
                            "",
                            f64::from(u8::from(value)),
                            variable,
                            &column.name,
                        )),
                        version,
                    );
                    document
                        .objects
                        .insert(handle, unknown("BDMDBCONFIGVARREACTOR", &record));
                    add_reactor(document, variable, handle);
                    column.reactor = Some(handle);
                }
            }
            let table_record = Record::new(
                types.table,
                table,
                Ref::new(feature, 4),
                Body::Properties(table_properties(parameter)),
                version,
            );
            document
                .objects
                .insert(table, unknown("BDMDBCONFIGURATIONTABLE", &table_record));
            let mut reactors = vec![parameter.key_reactor.unwrap_or_default()];
            reactors.extend(parameter.columns.iter().filter_map(|column| column.reactor));
            let reactors = Value::List {
                element: 8,
                items: reactors
                    .into_iter()
                    .map(|reactor| Value::Object {
                        kind: 3,
                        target: Ref::new(reactor, 3),
                    })
                    .collect(),
            };
            match parameter.feature {
                Some(existing) => {
                    let mut record = read_record(document, existing).ok_or(UNSUPPORTED)?;
                    let properties = record.body.properties_mut();
                    properties.set("Reactors", reactors);
                    properties.set(
                        "LastKeyValue",
                        Value::Text(parameter.states[current].clone()),
                    );
                    store(document, &record);
                }
                None => {
                    let record = Record::new(
                        types.feature,
                        feature,
                        Ref::new(database.features, 4),
                        Body::Properties(Properties(vec![
                            ("_ver".into(), Value::Byte(1)),
                            ("CloningType".into(), Value::Flag(true)),
                            (
                                "ConfigTable".into(),
                                Value::Object {
                                    kind: 3,
                                    target: Ref::new(table, 3),
                                },
                            ),
                            (
                                "Feature_type".into(),
                                Value::Text("BdmConfigurationFeature".into()),
                            ),
                            ("FreeConfig".into(), Value::Text("Custom".into())),
                            (
                                "LastKeyValue".into(),
                                Value::Text(parameter.states[current].clone()),
                            ),
                            ("Name".into(), Value::Text(parameter.name.clone())),
                            ("Reactors".into(), reactors),
                            ("Status".into(), Value::Integer(1)),
                            ("TableType".into(), Value::Text("VisibilityStates".into())),
                        ])),
                        version,
                    );
                    document
                        .objects
                        .insert(feature, unknown("BDMDBFEATUREDATA", &record));
                    new_features.push(feature);
                    parameter.feature = Some(feature);
                    parameter.table = Some(table);
                }
            }
        }

        // The objects every member needs.
        let entity_objects = entity_objects(document, self.owner, database.constraints);
        let mut new_objects: Vec<Handle> = Vec::new();
        let mut new_dependencies: Vec<Handle> = Vec::new();
        let mut next_object_number = entity_objects
            .values()
            .filter_map(|object| {
                read_record(document, *object)?
                    .body
                    .properties()
                    .text("NAME")?
                    .strip_prefix(object_prefix.as_str())?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            + 1;
        let mut entity_objects = entity_objects;

        let variables_by_name: FxHashMap<String, Handle> = self
            .parameters
            .iter()
            .flat_map(|p| p.columns.iter())
            .filter_map(|column| Some((column.name.clone(), column.variable?)))
            .collect();
        for group in &mut self.groups {
            let expression = format!(
                "1 - {}",
                group
                    .columns
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" * ")
            );
            let reads: Vec<Handle> = group
                .columns
                .iter()
                .filter_map(|(_, name)| variables_by_name.get(name).copied())
                .collect();
            let variable = match group.variable {
                Some(variable) => {
                    if group.columns_changed {
                        set_expression(document, variable, &expression, &reads);
                    }
                    variable
                }
                None => {
                    let variable = new_variable(
                        document,
                        network,
                        &group.name,
                        &expression,
                        "",
                        AssocEvalVariant {
                            code: 90,
                            value: AssocEvalValue::Long(0),
                        },
                    );
                    set_expression(document, variable, &expression, &reads);
                    variable
                }
            };
            group.variable = Some(variable);
            let feature = group.feature.unwrap_or_else(|| document.allocate_handle());
            let persid = group.persid.unwrap_or_else(|| document.allocate_handle());
            let expression_reactor = group
                .expression
                .unwrap_or_else(|| document.allocate_handle());
            if group.feature.is_none() {
                let record = Record::new(
                    types.feature,
                    feature,
                    Ref::new(database.features, 4),
                    Body::Properties(Properties(vec![
                        ("_ver".into(), Value::Byte(1)),
                        (
                            "Feature_type".into(),
                            Value::Text("BdmSuppressFeature".into()),
                        ),
                        ("Name".into(), Value::Text(group.name.clone())),
                        (
                            "Reactors".into(),
                            Value::List {
                                element: 8,
                                items: [persid, expression_reactor]
                                    .into_iter()
                                    .map(|reactor| Value::Object {
                                        kind: 3,
                                        target: Ref::new(reactor, 3),
                                    })
                                    .collect(),
                            },
                        ),
                        ("Status".into(), Value::Integer(0)),
                        (
                            "aEntLayerKey".into(),
                            Value::Map {
                                key: 8,
                                value: 8,
                                entries: Vec::new(),
                            },
                        ),
                    ])),
                    version,
                );
                document
                    .objects
                    .insert(feature, unknown("BDMDBFEATUREDATA", &record));
                new_features.push(feature);
                let record = Record::new(
                    types.expression_reactor,
                    expression_reactor,
                    Ref::new(feature, 4),
                    Body::Properties(Properties(vec![
                        ("_ver".into(), Value::Byte(1)),
                        ("deleteOnErase".into(), Value::Flag(true)),
                        ("expr".into(), Value::Text("1".into())),
                        ("prop".into(), Value::Text("suppress".into())),
                        ("react".into(), Value::Flag(false)),
                        ("strType".into(), Value::Flag(false)),
                        ("value".into(), Value::Real(0.0)),
                        (
                            "var".into(),
                            Value::Object {
                                kind: 0,
                                target: Ref::new(variable, 4),
                            },
                        ),
                    ])),
                    version,
                );
                document.objects.insert(
                    expression_reactor,
                    unknown("BDMDBEXPRESSIONREACTOR", &record),
                );
                add_reactor(document, variable, expression_reactor);
                group.feature = Some(feature);
                group.expression = Some(expression_reactor);
            }
            // Members: a dependency each, on this group's reactor.
            let mut dependencies = Vec::with_capacity(group.members.len());
            for member in &mut group.members {
                let dependency = match member.dependency {
                    Some(dependency) => {
                        let mut record = read_record(document, dependency).ok_or(UNSUPPORTED)?;
                        if record.reactors.iter().map(|r| r.handle).ne([persid]) {
                            record.reactors = vec![Ref::new(persid, 4)];
                            store(document, &record);
                        }
                        dependency
                    }
                    None => {
                        let handle = document.allocate_handle();
                        let mut record = Record::new(
                            types.dependency,
                            handle,
                            Ref::new(database.dependencies, 4),
                            Body::Dependency {
                                properties: Properties::default(),
                                tail: 0,
                                entity: Ref::new(member.entity, 5),
                            },
                            version,
                        );
                        record.reactors = vec![Ref::new(persid, 4)];
                        document
                            .objects
                            .insert(handle, unknown("BDMDBOBJECTDEPENDENCY", &record));
                        new_dependencies.push(handle);
                        // The member's object lists the dependency.
                        match entity_objects.get(&member.entity) {
                            Some(&object) => {
                                let mut object_record =
                                    read_record(document, object).ok_or(UNSUPPORTED)?;
                                let Some(Value::List { items, .. }) = object_record
                                    .body
                                    .properties_mut()
                                    .get_mut("PersIdDependencies")
                                else {
                                    return Err(UNSUPPORTED);
                                };
                                items.push(Value::Object {
                                    kind: 0,
                                    target: Ref::new(handle, 4),
                                });
                                store(document, &object_record);
                            }
                            None => {
                                let object = document.allocate_handle();
                                let record = Record::new(
                                    types.object,
                                    object,
                                    Ref::new(database.constraints, 4),
                                    Body::Object {
                                        lead: 5,
                                        lead_count: 0,
                                        entity: Ref::new(member.entity, 4),
                                        properties: Properties(vec![
                                            (
                                                "NAME".into(),
                                                Value::Text(format!(
                                                    "{object_prefix}{next_object_number}"
                                                )),
                                            ),
                                            (
                                                "PersIdDependencies".into(),
                                                Value::List {
                                                    element: 8,
                                                    items: vec![Value::Object {
                                                        kind: 0,
                                                        target: Ref::new(handle, 4),
                                                    }],
                                                },
                                            ),
                                        ]),
                                        tail: 0,
                                    },
                                    version,
                                );
                                next_object_number += 1;
                                document
                                    .objects
                                    .insert(object, unknown("BDMOBJECT", &record));
                                new_objects.push(object);
                                entity_objects.insert(member.entity, object);
                                if let Some(entity) = document.get_entity_mut(member.entity) {
                                    entity.common_mut().reactors.push(object);
                                }
                            }
                        }
                        handle
                    }
                };
                member.dependency = Some(dependency);
                dependencies.push(dependency);
            }
            let persid_record = Record::new(
                types.persid_reactor,
                persid,
                Ref::new(feature, 4),
                Body::Properties(Properties(vec![
                    ("_ver".into(), Value::Byte(1)),
                    ("mask".into(), Value::Integer(1)),
                    (
                        "pers_ids".into(),
                        Value::List {
                            element: 8,
                            items: dependencies
                                .iter()
                                .map(|dependency| Value::Object {
                                    kind: 0,
                                    target: Ref::new(*dependency, 4),
                                })
                                .collect(),
                        },
                    ),
                    ("tag".into(), Value::Integer(0)),
                ])),
                version,
            );
            match group.persid {
                Some(existing) => {
                    let mut record = read_record(document, existing).ok_or(UNSUPPORTED)?;
                    record.body = persid_record.body;
                    store(document, &record);
                }
                None => {
                    document
                        .objects
                        .insert(persid, unknown("BDMDBPERSIDREACTOR", &persid_record));
                    group.persid = Some(persid);
                }
            }
        }

        // Dependencies of objects no group rules any more go; their objects
        // stay, as BricsCAD keeps them, with the dependency struck off.
        for &dependency in &self.released {
            for &object in entity_objects.values() {
                let Some(mut record) = read_record(document, object) else {
                    continue;
                };
                if let Some(Value::List { items, .. }) =
                    record.body.properties_mut().get_mut("PersIdDependencies")
                {
                    let before = items.len();
                    items.retain(|item| item.object() != Some(dependency));
                    if items.len() != before {
                        store(document, &record);
                    }
                }
            }
            erased_objects.push(dependency);
        }

        for variable in &erased_variables {
            erase_variable(document, network, *variable);
        }
        for object in &erased_objects {
            document.objects.remove(object);
        }
        let erased: FxHashSet<Handle> = erased_objects.iter().copied().collect();

        // The managers list what was added and drop what went.
        let mut record = read_record(document, database.features).ok_or(UNSUPPORTED)?;
        if let Some(Value::List { items, .. }) = record.body.properties_mut().get_mut("Features") {
            items.retain(|item| item.object().is_none_or(|h| !erased.contains(&h)));
            items.extend(new_features.iter().map(|feature| Value::Object {
                kind: 3,
                target: Ref::new(*feature, 3),
            }));
        }
        store(document, &record);
        let mut record = read_record(document, database.dependencies).ok_or(UNSUPPORTED)?;
        if let Some(Value::List { items, .. }) = record.body.properties_mut().get_mut("deps") {
            items.retain(|item| item.object().is_none_or(|h| !erased.contains(&h)));
            items.extend(new_dependencies.iter().map(|dependency| Value::Object {
                kind: 3,
                target: Ref::new(*dependency, 3),
            }));
        }
        store(document, &record);
        if !new_objects.is_empty() {
            let mut record = read_record(document, database.constraints).ok_or(UNSUPPORTED)?;
            let Body::ConstraintManager {
                counts, children, ..
            } = &mut record.body
            else {
                return Err(UNSUPPORTED);
            };
            let at = usize::try_from(counts[1]).map_err(|_| UNSUPPORTED)?;
            for (k, object) in new_objects.iter().enumerate() {
                children.insert((at + k).min(children.len()), Ref::new(*object, 3));
            }
            counts[1] += new_objects.len() as i32;
            store(document, &record);
        }
        Ok(())
    }
}

/// The prefix of a column variable's description, the text before the name
/// of the suppression it controls.
fn description_prefix(document: &Document, variable: Handle) -> Option<String> {
    let Some(ObjectType::Associative(AssociativeObject {
        data: AssociativeData::Variable(variable),
        ..
    })) = document.objects.get(&variable)
    else {
        return None;
    };
    let (prefix, _) = variable.description.rsplit_once(' ')?;
    Some(format!("{prefix} "))
}

fn table_properties(parameter: &Parameter) -> Properties {
    let rows = parameter.states.len();
    let mut values = Vec::with_capacity(rows * parameter.columns.len());
    for row in 0..rows {
        for column in &parameter.columns {
            values.push(column.values.get(row).copied().unwrap_or(false));
        }
    }
    let texts = |items: Vec<String>| Value::List {
        element: 2,
        items: items.into_iter().map(Value::Text).collect(),
    };
    Properties(vec![
        (
            "ColNames".into(),
            texts(parameter.columns.iter().map(|c| c.name.clone()).collect()),
        ),
        (
            "ColNum".into(),
            Value::Integer(parameter.columns.len() as i32),
        ),
        (
            "ColTypes".into(),
            Value::List {
                element: 3,
                items: parameter
                    .columns
                    .iter()
                    .map(|_| Value::Integer(0))
                    .collect(),
            },
        ),
        (
            "DblValues".into(),
            Value::List {
                element: 4,
                items: values
                    .iter()
                    .map(|v| Value::Real(f64::from(u8::from(*v))))
                    .collect(),
            },
        ),
        ("KeyName".into(), Value::Text(parameter.name.clone())),
        ("KeyType".into(), Value::Integer(1)),
        ("KeyValues".into(), texts(parameter.states.clone())),
        ("RowNum".into(), Value::Integer(rows as i32)),
        (
            "Values".into(),
            texts(
                values
                    .iter()
                    .map(|v| if *v { "1".into() } else { "0".into() })
                    .collect(),
            ),
        ),
    ])
}

fn variable_reactor(
    key: bool,
    string: &str,
    value: f64,
    variable: Handle,
    name: &str,
) -> Properties {
    Properties(vec![
        ("_ver".into(), Value::Byte(1)),
        ("DeleteVarOnErase".into(), Value::Flag(true)),
        ("IsEnabled".into(), Value::Flag(true)),
        ("IsKeyVar".into(), Value::Flag(key)),
        ("IsString".into(), Value::Flag(key)),
        ("String".into(), Value::Text(string.to_string())),
        ("Value".into(), Value::Real(value)),
        (
            "VarId".into(),
            Value::Object {
                kind: 0,
                target: Ref::new(variable, 4),
            },
        ),
        ("VarName".into(), Value::Text(name.to_string())),
    ])
}

/// The type numbers of BricsCAD's classes in this drawing.
struct Types {
    feature_manager: u16,
    feature: u16,
    table: u16,
    config_reactor: u16,
    persid_reactor: u16,
    persid_manager: u16,
    constraint_manager: u16,
    object: u16,
    dependency: u16,
    expression_reactor: u16,
}

impl Types {
    fn ensure(document: &mut Document) -> Self {
        crate::scene::dwg_native_constraints::ensure_associative_classes_registered(document);
        let mut number = |dxf: &str| -> u16 {
            let (dxf, cpp, application) = CLASSES
                .iter()
                .find(|(name, ..)| *name == dxf)
                .copied()
                .unwrap_or((dxf, dxf, ""));
            if document.classes.get_by_name(dxf).is_none() {
                document.classes.add_or_update(document::classes::DxfClass {
                    dxf_name: dxf.to_string(),
                    cpp_class_name: cpp.to_string(),
                    application_name: application.to_string(),
                    proxy_flags: document::classes::ProxyFlags(1025),
                    instance_count: 0,
                    was_zombie: false,
                    is_an_entity: false,
                    class_number: 0,
                    item_class_id: 0x1F3,
                    dwg_version: 33,
                    maintenance_version: 55,
                    unknown1: 0,
                    unknown2: 0,
                });
            }
            document
                .classes
                .get_by_name(dxf)
                .map_or(0, |class| class.class_number as u16)
        };
        Self {
            feature_manager: number("BDMDBFEATUREMANAGER"),
            feature: number("BDMDBFEATUREDATA"),
            table: number("BDMDBCONFIGURATIONTABLE"),
            config_reactor: number("BDMDBCONFIGVARREACTOR"),
            persid_reactor: number("BDMDBPERSIDREACTOR"),
            persid_manager: number("BdmDbPersIdManager"),
            constraint_manager: number("BDMDBCONSTRAINTMANAGER"),
            object: number("BDMOBJECT"),
            dependency: number("BDMDBOBJECTDEPENDENCY"),
            expression_reactor: number("BDMDBEXPRESSIONREACTOR"),
        }
    }
}

struct Database {
    features: Handle,
    dependencies: Handle,
    constraints: Handle,
}

/// The owner's `BDM_DATABASE` with its three managers, made when missing.
fn ensure_database(
    document: &mut Document,
    owner: Handle,
    types: &Types,
    version: DxfVersion,
) -> Result<Database> {
    if let (Some(features), Some(dependencies), Some(constraints)) = (
        database_entry(document, owner, "FEATURE_MANAGER"),
        database_entry(document, owner, "PERSIDMANAGER"),
        database_entry(document, owner, "BDM_DATABASE"),
    ) {
        return Ok(Database {
            features,
            dependencies,
            constraints,
        });
    }
    let extension = document.ensure_extension_dictionary(owner);
    if matches!(
        document.objects.get(&extension),
        Some(ObjectType::Dictionary(dictionary)) if dictionary.get("BDM_DATABASE").is_some()
    ) {
        // A database without its three managers is not one this writes into.
        return Err(UNSUPPORTED);
    }
    let [dictionary, features, dependencies, constraints] =
        [(); 4].map(|_| document.allocate_handle());
    let mut database = Dictionary::new();
    database.handle = dictionary;
    database.owner = extension;
    database.reactors = vec![extension];
    database.entries = vec![
        ("FEATURE_MANAGER".into(), features),
        ("PERSIDMANAGER".into(), dependencies),
        ("BDM_DATABASE".into(), constraints),
    ];
    document
        .objects
        .insert(dictionary, ObjectType::Dictionary(database));
    let managers = [
        (
            "BDMDBFEATUREMANAGER",
            types.feature_manager,
            features,
            Body::Properties(Properties(vec![
                ("_ver".into(), Value::Byte(1)),
                (
                    "ColorPolicy".into(),
                    Value::Object {
                        kind: 3,
                        target: Ref::new(Handle::NULL, 3),
                    },
                ),
                (
                    "Contexts".into(),
                    Value::List {
                        element: 8,
                        items: Vec::new(),
                    },
                ),
                (
                    "Features".into(),
                    Value::List {
                        element: 8,
                        items: Vec::new(),
                    },
                ),
            ])),
        ),
        (
            "BdmDbPersIdManager",
            types.persid_manager,
            dependencies,
            Body::Properties(Properties(vec![
                ("_ver".into(), Value::Byte(1)),
                (
                    "deps".into(),
                    Value::List {
                        element: 8,
                        items: Vec::new(),
                    },
                ),
            ])),
        ),
        (
            "BDMDBCONSTRAINTMANAGER",
            types.constraint_manager,
            constraints,
            Body::ConstraintManager {
                counts: [2, 0, 0, 0, 0],
                children: Vec::new(),
                settings: Properties(vec![(
                    "DesignIntentSettings".into(),
                    Value::Text("DISABLE_HEURISTIC_BODY_FIX".into()),
                )]),
            },
        ),
    ];
    for (class, object_type, handle, body) in managers {
        let mut record = Record::new(object_type, handle, Ref::new(dictionary, 4), body, version);
        record.reactors = vec![Ref::new(dictionary, 4)];
        document.objects.insert(handle, unknown(class, &record));
    }
    if let Some(ObjectType::Dictionary(extension)) = document.objects.get_mut(&extension) {
        extension.add_entry("BDM_DATABASE", dictionary);
    }
    Ok(Database {
        features,
        dependencies,
        constraints,
    })
}

/// The owner's associative network.
fn owner_network(document: &Document, owner: Handle) -> Option<Handle> {
    let dictionary = |handle: Handle| match document.objects.get(&handle)? {
        ObjectType::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    };
    let extension = dictionary(document.extension_dictionary_handle(owner)?)?;
    let mut network = extension.get("ACAD_ASSOCNETWORK")?;
    while let Some(inner) = dictionary(network) {
        network = inner.get("ACAD_ASSOCNETWORK")?;
    }
    matches!(
        document.objects.get(&network),
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Network(_),
            ..
        }))
    )
    .then_some(network)
}

/// The owner's associative network, made and attached to the drawing's root
/// network when missing.
fn ensure_network(document: &mut Document, owner: Handle) -> Result<Handle> {
    if let Some(network) = owner_network(document, owner) {
        return Ok(network);
    }
    let extension = document.ensure_extension_dictionary(owner);
    if matches!(
        document.objects.get(&extension),
        Some(ObjectType::Dictionary(dictionary)) if dictionary.get("ACAD_ASSOCNETWORK").is_some()
    ) {
        return Err(UNSUPPORTED);
    }
    let [dictionary, network] = [(); 2].map(|_| document.allocate_handle());
    let mut holder = Dictionary::new();
    holder.handle = dictionary;
    holder.owner = extension;
    holder.reactors = vec![extension];
    holder.entries = vec![("ACAD_ASSOCNETWORK".into(), network)];
    document
        .objects
        .insert(dictionary, ObjectType::Dictionary(holder));
    document.objects.insert(
        network,
        ObjectType::Associative(AssociativeObject {
            handle: network,
            owner: dictionary,
            reactors: vec![dictionary],
            dxf_name: "ACDBASSOCNETWORK".into(),
            cpp_class_name: "AcDbAssocNetwork".into(),
            data: AssociativeData::Network(AssocNetwork {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }),
    );
    if let Some(ObjectType::Dictionary(extension)) = document.objects.get_mut(&extension) {
        extension.add_entry("ACAD_ASSOCNETWORK", dictionary);
    }
    let mut networks = FxHashMap::default();
    networks.insert(network, network);
    Scene::register_copied_constraint_networks(document, &networks).map_err(|_| UNSUPPORTED)?;
    Ok(network)
}

/// A new variable in `network`, its last action.
fn new_variable(
    document: &mut Document,
    network: Handle,
    name: &str,
    expression: &str,
    description: &str,
    value: AssocEvalVariant,
) -> Handle {
    let handle = document.allocate_handle();
    let index = match document.objects.get_mut(&network) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Network(network),
            ..
        })) => {
            network.network_action_index += 1;
            network.actions.push(AssocActionDependency {
                is_owned: true,
                dependency: handle,
            });
            network.network_action_index
        }
        _ => 0,
    };
    document.objects.insert(
        handle,
        ObjectType::Associative(AssociativeObject {
            handle,
            owner: network,
            dxf_name: "ACDBASSOCVARIABLE".into(),
            cpp_class_name: "AcDbAssocVariable".into(),
            data: AssociativeData::Variable(AssocVariable {
                action: AssocAction {
                    class_version: 2,
                    owning_network: network,
                    action_index: index,
                    ..Default::default()
                },
                class_version: 2,
                name: name.to_string(),
                expression: expression.to_string(),
                evaluator: "AcDbCalc:1.0".into(),
                description: description.to_string(),
                value,
                ..Default::default()
            }),
            ..Default::default()
        }),
    );
    handle
}

fn add_reactor(document: &mut Document, object: Handle, reactor: Handle) {
    if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&object) {
        if !object.reactors.contains(&reactor) {
            object.reactors.push(reactor);
        }
    }
}

fn variable_value(document: &Document, variable: Handle) -> AssocEvalVariant {
    match document.objects.get(&variable) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            ..
        })) => variable.value.clone(),
        _ => AssocEvalVariant::default(),
    }
}

/// Set a variable's expression and give it a value dependency on each
/// variable it reads, numbered on from the ones it had.
fn set_expression(document: &mut Document, variable: Handle, expression: &str, reads: &[Handle]) {
    let old: Vec<Handle> = match document.objects.get(&variable) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(value),
            ..
        })) => value.dependencies.iter().map(|d| d.dependency).collect(),
        _ => return,
    };
    for dependency in old {
        let read = match document.objects.get(&dependency) {
            Some(ObjectType::Associative(AssociativeObject {
                data: AssociativeData::ValueDependency(value),
                ..
            })) => Some(value.dependency.dependent_on),
            _ => None,
        };
        if let Some(read) = read {
            if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&read) {
                object.reactors.retain(|reactor| *reactor != dependency);
            }
        }
        document.objects.remove(&dependency);
    }
    let mut next = match document.objects.get(&variable) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(value),
            ..
        })) => value.action.max_dependency_index,
        _ => 0,
    };
    let mut dependencies = Vec::new();
    for (position, &read) in reads.iter().enumerate() {
        next += 1;
        let handle = document.allocate_handle();
        let value = variable_value(document, read);
        document.objects.insert(
            handle,
            ObjectType::Associative(AssociativeObject {
                handle,
                owner: variable,
                dxf_name: "ACDBASSOCVALUEDEPENDENCY".into(),
                cpp_class_name: "AcDbAssocValueDependency".into(),
                data: AssociativeData::ValueDependency(AssocValueDependency {
                    dependency: AssocDependency {
                        class_version: 2,
                        is_read_dependency: true,
                        is_attached_to_object: true,
                        is_delegating_to_owning_action: true,
                        dependent_on: read,
                        dependency_body_id: next,
                        ..Default::default()
                    },
                    value,
                    ..Default::default()
                }),
                ..Default::default()
            }),
        );
        add_reactor(document, read, handle);
        // BricsCAD numbers the reads of a variable in its flags.
        dependencies.push(AssocVariableDependency {
            dependency: handle,
            flags: position as _,
        });
    }
    if let Some(ObjectType::Associative(AssociativeObject {
        data: AssociativeData::Variable(value),
        ..
    })) = document.objects.get_mut(&variable)
    {
        value.expression = expression.to_string();
        value.dependencies = dependencies;
        value.action.max_dependency_index = next;
    }
}

/// Take a variable out of its network with the value dependencies it owns.
fn erase_variable(document: &mut Document, network: Handle, variable: Handle) {
    set_expression(document, variable, "", &[]);
    if let Some(ObjectType::Associative(AssociativeObject {
        data: AssociativeData::Network(network),
        ..
    })) = document.objects.get_mut(&network)
    {
        network
            .actions
            .retain(|action| action.dependency != variable);
    }
    document.objects.remove(&variable);
}

/// For each entity of `owner`, its `BDMOBJECT` in the owner's constraint
/// manager.
fn entity_objects(
    document: &Document,
    owner: Handle,
    constraints: Handle,
) -> FxHashMap<Handle, Handle> {
    let mut objects = FxHashMap::default();
    let Some(Body::ConstraintManager {
        counts, children, ..
    }) = read_record(document, constraints).map(|record| record.body)
    else {
        return objects;
    };
    for child in children
        .iter()
        .take(usize::try_from(counts[1]).unwrap_or(0))
    {
        if let Some(Body::Object { entity, .. }) =
            read_record(document, child.handle).map(|record| record.body)
        {
            if document
                .get_entity(entity.handle)
                .is_some_and(|e| e.common().owner_handle == owner)
            {
                objects.insert(entity.handle, child.handle);
            }
        }
    }
    objects
}
