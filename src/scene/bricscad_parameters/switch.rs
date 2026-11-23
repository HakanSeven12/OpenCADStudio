//! Switching the visibility state a BricsCAD parametric block reference
//! shows, the way BricsCAD does it, so that BricsCAD keeps the block's
//! parameters.
//!
//! A reference shows the block itself or an anonymous copy of it, and
//! BricsCAD reads the states a reference shows from the parameter objects of
//! that copy. When a state is changed in BricsCAD, it starts from the states
//! the reference's entry in model space lists: a `BmAssemblyParameters`
//! feature, linked to the reference through a `BDMOBJECT` and a dependency,
//! that lists every parameter the reference was ever set to with the state it
//! shows. BricsCAD shares a copy between references that show the same
//! states.
//!
//! Switching here:
//! 1. works out the states the reference is to show and the parameters its
//!    entry lists;
//! 2. points the reference at the block when those are the block's own states,
//!    at a copy of the block that shows them when there is one, or at a new
//!    copy: the block's members and all its parameter objects cloned under new
//!    handles, with the states set as BricsCAD sets them;
//! 3. moves the reference's attributes on or off `*SUPPRESS` with their
//!    definitions in the block it now shows;
//! 4. writes the reference's entry in model space, creating it, and model
//!    space's own parameter objects when there are none.

use std::collections::{BTreeMap, BTreeSet};

use document::objects::{
    AssocAction, AssocDependency, AssocEvalValue, AssocEvalVariant, AssociativeData,
    AssociativeObject, ObjectType,
};
use document::{Document, EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};

use super::record::{self, Body, Properties, Record, Ref, Value};
use super::{
    database_entry, features, original_block, record as read_record, shown_block,
    visibility_parameters,
};
use crate::scene::Scene;

/// The layer BricsCAD moves the members a state hides to.
pub(super) const SUPPRESS_LAYER: &str = "*SUPPRESS";

const UNSUPPORTED: &str = "This BricsCAD parametric block cannot be switched here.";
const NOT_IN_MODEL_SPACE: &str = "Only a BricsCAD block reference in model space can be switched.";
const NO_SUCH_STATE: &str = "The block has no such visibility state.";

type Result<T> = std::result::Result<T, &'static str>;

/// Whether [`Scene::set_bricscad_visibility`] can switch `insert`: a
/// reference in model space to a BricsCAD parametric block, or to a copy of
/// one, whose parameter objects read.
pub(crate) fn can_switch(document: &Document, insert: Handle) -> bool {
    document.get_entity(insert).is_some_and(|entity| {
        entity.common().owner_handle == document.header.model_space_block_handle
    }) && super::master_block(document, insert).is_some()
}

impl Scene {
    /// Show `state` of the visibility parameter `parameter` on the reference
    /// `insert`, as BricsCAD does. `Ok(false)` when the reference shows it
    /// already. On an error the document is left as it was only when the
    /// caller works on a copy of it; the property and automation paths do.
    pub(crate) fn set_bricscad_visibility(
        &mut self,
        insert: Handle,
        parameter: &str,
        state: &str,
    ) -> Result<bool> {
        let document = &self.document;
        if !can_switch(document, insert) {
            return Err(NOT_IN_MODEL_SPACE);
        }
        let shown = shown_block(document, insert).ok_or(UNSUPPORTED)?;
        let master = original_block(document, shown).unwrap_or(shown);
        let master_parameters = visibility_parameters(document, master);
        let shown_parameters = visibility_parameters(document, shown);
        let names: Vec<&str> = master_parameters.iter().map(|p| p.name.as_str()).collect();
        if names.is_empty()
            || shown_parameters
                .iter()
                .map(|p| p.name.as_str())
                .ne(names.iter().copied())
        {
            return Err(UNSUPPORTED);
        }
        let index = names
            .iter()
            .position(|name| *name == parameter)
            .ok_or(NO_SUCH_STATE)?;
        if !master_parameters[index].states.iter().any(|s| s == state) {
            return Err(NO_SUCH_STATE);
        }
        let shown_state = |parameters: &[super::VisibilityParameter]| -> Result<Vec<String>> {
            parameters
                .iter()
                .map(|p| {
                    p.current
                        .and_then(|current| p.states.get(current).cloned())
                        .ok_or(UNSUPPORTED)
                })
                .collect()
        };
        let defaults = shown_state(&master_parameters)?;
        let current = shown_state(&shown_parameters)?;
        if current[index] == state {
            return Ok(false);
        }
        let mut target = current.clone();
        target[index] = state.to_string();

        // The entry lists what it listed, what differs from the block, and
        // the parameter set now, each in the state it shows.
        let entry = find_entry(document, insert);
        let mut listed: BTreeSet<String> = entry
            .as_ref()
            .and_then(|(_, record)| record.body.properties().get("expr.const"))
            .and_then(Value::entries)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|(key, _)| key.text().map(str::to_string))
                    .filter(|key| names.contains(&key.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        for (k, name) in names.iter().enumerate() {
            if current[k] != defaults[k] {
                listed.insert(name.to_string());
            }
        }
        listed.insert(parameter.to_string());
        let overrides: Vec<(String, String)> = listed
            .iter()
            .map(|name| {
                let k = names.iter().position(|n| n == name).unwrap_or(index);
                (name.clone(), target[k].clone())
            })
            .collect();
        let names: Vec<String> = names.iter().map(|name| name.to_string()).collect();

        let block = if target == defaults {
            master
        } else if let Some(copy) = find_copy(&self.document, master, &target) {
            copy
        } else {
            let states: BTreeMap<String, String> =
                names.iter().cloned().zip(target.iter().cloned()).collect();
            self.clone_parametric_copy(master, &states, &listed)?
        };
        if block != shown {
            let name = self
                .document
                .block_records
                .iter()
                .find(|record| record.handle == block)
                .map(|record| record.name.clone())
                .ok_or(UNSUPPORTED)?;
            self.repoint_representation(insert, shown, block, &name);
            follow_attribute_definitions(&mut self.document, insert, block);
        }
        write_entry(&mut self.document, insert, master, overrides)?;
        Ok(true)
    }

    /// A new anonymous copy of the BricsCAD parametric block `master` showing
    /// `states`, with the parameters in `listed` set on it: the members and
    /// every object under the block's extension dictionary cloned under new
    /// handles, then the states applied.
    pub(super) fn clone_parametric_copy(
        &mut self,
        master: Handle,
        states: &BTreeMap<String, String>,
        listed: &BTreeSet<String>,
    ) -> Result<Handle> {
        let document = &self.document;
        let master_dictionary = document
            .extension_dictionary_handle(master)
            .ok_or(UNSUPPORTED)?;
        let objects = owned_tree(document, master_dictionary)?;
        let records: FxHashMap<Handle, Record> = objects
            .iter()
            .filter(|handle| {
                matches!(
                    document.objects.get(handle),
                    Some(ObjectType::Unknown { .. })
                )
            })
            .map(|handle| Some((*handle, read_record(document, *handle)?)))
            .collect::<Option<_>>()
            .ok_or(UNSUPPORTED)?;
        let tag = block_tag(document, master);
        let master_record = document
            .block_records
            .iter()
            .find(|record| record.handle == master)
            .ok_or(UNSUPPORTED)?;
        let master_name = master_record.name.clone();
        let master_members = crate::scene::block_authoring::members(document, master);
        let entities: Vec<EntityType> = master_members
            .iter()
            .map(|handle| document.get_entity(*handle).cloned())
            .collect::<Option<_>>()
            .ok_or(UNSUPPORTED)?;
        let layers: Vec<String> = entities
            .iter()
            .map(|entity| entity.common().layer.clone())
            .collect();
        let base = crate::scene::render_graph::block_base_point(document, &master_name);
        let name = next_anonymous_name(document);

        self.define_block_raw_unbumped(&name, base, entities);
        let record = self
            .document
            .block_records
            .get_mut(&name)
            .ok_or(UNSUPPORTED)?;
        record.flags.anonymous = true;
        let copy = record.handle;
        let copy_members = crate::scene::block_authoring::members(&self.document, copy);
        if copy_members.len() != master_members.len() {
            return Err(UNSUPPORTED);
        }
        self.own_copied_xdicts(master_members.clone(), copy_members.clone());

        let document = &mut self.document;
        // A block BricsCAD never copied has no tag yet; BricsCAD gives it one
        // when it makes the first copy.
        let tag = match tag {
            Some(tag) => tag,
            None => give_block_tag(document, master),
        };
        let copy_dictionary = document.ensure_extension_dictionary(copy);
        let mut cloned: FxHashMap<Handle, Handle> = FxHashMap::default();
        for &handle in &objects {
            let new = if handle == master_dictionary {
                copy_dictionary
            } else {
                document.allocate_handle()
            };
            cloned.insert(handle, new);
        }
        let members: FxHashMap<Handle, Handle> = master_members
            .iter()
            .copied()
            .zip(copy_members.iter().copied())
            .collect();
        let remap = |handle: Handle| {
            if handle == master {
                copy
            } else {
                cloned
                    .get(&handle)
                    .or_else(|| members.get(&handle))
                    .copied()
                    .unwrap_or(handle)
            }
        };
        let mut clones = Vec::with_capacity(objects.len());
        for &handle in &objects {
            let new = remap(handle);
            let clone = match document.objects.get(&handle).ok_or(UNSUPPORTED)? {
                ObjectType::Dictionary(dictionary) => {
                    let mut dictionary = dictionary.clone();
                    dictionary.handle = new;
                    dictionary.owner = remap(dictionary.owner);
                    for (_, entry) in &mut dictionary.entries {
                        *entry = remap(*entry);
                    }
                    for reactor in &mut dictionary.reactors {
                        *reactor = remap(*reactor);
                    }
                    dictionary.xdictionary_handle = dictionary.xdictionary_handle.map(remap);
                    ObjectType::Dictionary(dictionary)
                }
                ObjectType::Associative(object) => {
                    let mut object = object.clone();
                    remap_associative(&mut object, &remap)?;
                    ObjectType::Associative(object)
                }
                // The block's draw order, member by member.
                ObjectType::SortEntitiesTable(table) => {
                    let mut order = document::objects::SortEntitiesTable::for_block(copy);
                    order.handle = new;
                    order.owner_handle = remap(table.owner_handle);
                    for entry in table.entries() {
                        order.add_entry(remap(entry.entity_handle), remap(entry.sort_handle));
                    }
                    ObjectType::SortEntitiesTable(order)
                }
                ObjectType::Unknown { type_name, .. } => {
                    let mut record = records[&handle].clone();
                    record.handle = new;
                    record.refs_mut(|reference| reference.handle = remap(reference.handle));
                    unknown(type_name, &record)
                }
                _ => return Err(UNSUPPORTED),
            };
            clones.push((new, clone));
        }
        for (handle, object) in clones {
            document.objects.insert(handle, object);
        }
        // The copies' reactors name the clones of the master members'
        // parameter objects; anything else they named stays with the master.
        for (original, copy_member) in master_members.iter().zip(&copy_members) {
            let reactors: Vec<Handle> = document
                .get_entity(*original)
                .map(|entity| {
                    entity
                        .common()
                        .reactors
                        .iter()
                        .filter_map(|reactor| cloned.get(reactor).copied())
                        .collect()
                })
                .unwrap_or_default();
            if let Some(entity) = document.get_entity_mut(*copy_member) {
                entity.common_mut().reactors = reactors;
            }
        }

        // The copy names the block it was made from, member by member.
        let manager = database_entry(document, copy, "PERSIDMANAGER").ok_or(UNSUPPORTED)?;
        let mut record = read_record(document, manager).ok_or(UNSUPPORTED)?;
        let properties = record.body.properties_mut();
        properties
            .0
            .retain(|(name, _)| !name.starts_with("original."));
        properties.0.push((
            "original.block".into(),
            Value::Object {
                kind: 1,
                target: Ref::new(master, 5),
            },
        ));
        properties.0.push((
            "original.map".into(),
            Value::Map {
                key: 7,
                value: 8,
                entries: master_members
                    .iter()
                    .zip(&copy_members)
                    .map(|(original, member)| {
                        (
                            Value::Raw(handle_bytes(*original)),
                            Value::Object {
                                kind: 0,
                                target: Ref::new(*member, 4),
                            },
                        )
                    })
                    .collect(),
            },
        ));
        properties.0.push(("original.tag".into(), Value::Text(tag)));
        store(document, &record);

        let layers: FxHashMap<Handle, String> = copy_members.iter().copied().zip(layers).collect();
        apply_states(
            document,
            copy,
            &copy_members,
            &layers,
            states,
            listed,
            false,
        )?;
        Ok(copy)
    }
}

/// Every object owned, directly or further down, by `root`, root first.
fn owned_tree(document: &Document, root: Handle) -> Result<Vec<Handle>> {
    let mut children: FxHashMap<Handle, Vec<Handle>> = FxHashMap::default();
    for &handle in document.objects.keys() {
        if let Some(owner) = document.object_owner(handle) {
            children.entry(owner).or_default().push(handle);
        }
    }
    let mut seen = FxHashSet::default();
    let mut order = Vec::new();
    let mut stack = vec![root];
    while let Some(handle) = stack.pop() {
        if handle.is_null() || !seen.insert(handle) {
            continue;
        }
        match document.objects.get(&handle) {
            Some(
                ObjectType::Dictionary(_)
                | ObjectType::Unknown { .. }
                | ObjectType::SortEntitiesTable(_),
            ) => {}
            Some(ObjectType::Associative(AssociativeObject {
                data:
                    AssociativeData::Network(_)
                    | AssociativeData::Variable(_)
                    | AssociativeData::ValueDependency(_),
                ..
            })) => {}
            _ => return Err(UNSUPPORTED),
        }
        order.push(handle);
        if let Some(owned) = children.get(&handle) {
            let mut owned = owned.clone();
            owned.sort_unstable_by_key(|handle| std::cmp::Reverse(*handle));
            stack.extend(owned);
        }
    }
    Ok(order)
}

/// Point every handle of an associative network, variable or value dependency
/// through `remap`.
fn remap_associative(
    object: &mut AssociativeObject,
    remap: &impl Fn(Handle) -> Handle,
) -> Result<()> {
    object.handle = remap(object.handle);
    object.owner = remap(object.owner);
    for reactor in &mut object.reactors {
        *reactor = remap(*reactor);
    }
    object.xdictionary_handle = object.xdictionary_handle.map(remap);
    let action = |action: &mut AssocAction| {
        action.owning_network = remap(action.owning_network);
        action.action_body = remap(action.action_body);
        for dependency in &mut action.dependencies {
            dependency.dependency = remap(dependency.dependency);
        }
        for parameter in &mut action.owned_parameters {
            *parameter = remap(*parameter);
        }
        for value in &mut action.values {
            value.controlled_object_dependency = remap(value.controlled_object_dependency);
            for variable in &mut value.variables {
                variable.handle = remap(variable.handle);
                if let AssocEvalValue::Handle(handle) = &mut variable.value.value {
                    *handle = remap(*handle);
                }
            }
        }
    };
    let dependency = |dependency: &mut AssocDependency| {
        dependency.dependent_on = remap(dependency.dependent_on);
        dependency.read_dependency = remap(dependency.read_dependency);
        dependency.node = remap(dependency.node);
        dependency.dependency_body = remap(dependency.dependency_body);
    };
    match &mut object.data {
        AssociativeData::Network(network) => {
            action(&mut network.action);
            for child in &mut network.actions {
                child.dependency = remap(child.dependency);
            }
            for owned in &mut network.owned_actions {
                *owned = remap(*owned);
            }
        }
        AssociativeData::Variable(variable) => {
            action(&mut variable.action);
            for owned in &mut variable.dependencies {
                owned.dependency = remap(owned.dependency);
            }
            if let AssocEvalValue::Handle(handle) = &mut variable.value.value {
                *handle = remap(*handle);
            }
        }
        AssociativeData::ValueDependency(value) => {
            dependency(&mut value.dependency);
            if let AssocEvalValue::Handle(handle) = &mut value.value.value {
                *handle = remap(*handle);
            }
        }
        _ => return Err(UNSUPPORTED),
    }
    Ok(())
}

/// Set the copy's parameter objects, associative variables and member layers
/// to `states`, as BricsCAD leaves a copy it made: for each visibility
/// parameter the state, the values of its table's columns and, for a
/// parameter in `listed`, the marks BricsCAD sets on an instance's own
/// parameter; for each suppression its value, status and the layers of the
/// members it moved to `*SUPPRESS`. With `keep_keys` the key reactors keep
/// whether they are enabled and the number they hold (model space of a block
/// drawing, where BricsCAD leaves the keys of parameters it was not asked
/// about as they are).
pub(super) fn apply_states(
    document: &mut Document,
    copy: Handle,
    members: &[Handle],
    layers: &FxHashMap<Handle, String>,
    states: &BTreeMap<String, String>,
    listed: &BTreeSet<String>,
    keep_keys: bool,
) -> Result<()> {
    let features = features(document, copy);
    let mut columns: FxHashMap<String, f64> = FxHashMap::default();
    let mut suppressions = Vec::new();
    for &feature in &features {
        let mut record = read_record(document, feature).ok_or(UNSUPPORTED)?;
        let properties = record.body.properties();
        match properties.text("Feature_type") {
            Some("BdmConfigurationFeature")
                if properties.text("TableType") == Some("VisibilityStates") => {}
            Some("BdmSuppressFeature") => {
                suppressions.push(feature);
                continue;
            }
            _ => continue,
        }
        let table = super::properties(
            document,
            properties
                .get("ConfigTable")
                .and_then(Value::object)
                .ok_or(UNSUPPORTED)?,
        )
        .ok_or(UNSUPPORTED)?;
        let name = table
            .text("KeyName")
            .or_else(|| properties.text("Name"))
            .ok_or(UNSUPPORTED)?
            .to_string();
        let state = states.get(&name).ok_or(UNSUPPORTED)?;
        let texts = |key: &str| -> Option<Vec<&str>> {
            table.get(key)?.items()?.iter().map(Value::text).collect()
        };
        let keys = texts("KeyValues").ok_or(UNSUPPORTED)?;
        let column_names = texts("ColNames").ok_or(UNSUPPORTED)?;
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
        let row = keys
            .iter()
            .position(|key| key == state)
            .ok_or(NO_SUCH_STATE)?;
        let own = listed.contains(&name);
        let reactors: Vec<Handle> = properties
            .get("Reactors")
            .and_then(Value::items)
            .map(|items| items.iter().filter_map(Value::object).collect())
            .unwrap_or_default();
        record
            .body
            .properties_mut()
            .set("LastKeyValue", Value::Text(state.clone()));
        store(document, &record);

        for reactor in reactors {
            let mut variable_reactor = read_record(document, reactor).ok_or(UNSUPPORTED)?;
            let properties = variable_reactor.body.properties_mut();
            let variable = properties
                .get("VarId")
                .and_then(Value::object)
                .ok_or(UNSUPPORTED)?;
            if properties.get("IsKeyVar") == Some(&Value::Flag(true)) {
                properties.set("String", Value::Text(state.clone()));
                if !keep_keys {
                    properties.set("IsEnabled", Value::Flag(!own));
                }
                if own {
                    properties.set("Value", Value::Real(row as f64));
                    set_variable(
                        document,
                        variable,
                        Some(row.to_string()),
                        AssocEvalVariant {
                            code: 90,
                            value: AssocEvalValue::Long(row as i32),
                        },
                    )?;
                }
            } else {
                let column_name = properties.text("VarName").ok_or(UNSUPPORTED)?.to_string();
                let column = column_names
                    .iter()
                    .position(|name| *name == column_name)
                    .ok_or(UNSUPPORTED)?;
                let value = *values
                    .get(row * column_names.len() + column)
                    .ok_or(UNSUPPORTED)?;
                properties.set("Value", Value::Real(value));
                set_variable(
                    document,
                    variable,
                    None,
                    AssocEvalVariant {
                        code: 40,
                        value: AssocEvalValue::Real(value),
                    },
                )?;
                columns.insert(column_name, value);
            }
            store(document, &variable_reactor);
        }
    }

    let order: FxHashMap<Handle, usize> =
        members.iter().enumerate().map(|(k, h)| (*h, k)).collect();
    let mut suppressed: FxHashSet<Handle> = FxHashSet::default();
    let layer_handle =
        |document: &Document, name: &str| document.layers.get(name).map(|layer| layer.handle);
    for feature in suppressions {
        let mut record = read_record(document, feature).ok_or(UNSUPPORTED)?;
        let reactors: Vec<Handle> = record
            .body
            .properties()
            .get("Reactors")
            .and_then(Value::items)
            .map(|items| items.iter().filter_map(Value::object).collect())
            .unwrap_or_default();
        let mut entities = Vec::new();
        let mut value = None;
        for reactor in reactors {
            let mut reactor_record = read_record(document, reactor).ok_or(UNSUPPORTED)?;
            let properties = reactor_record.body.properties();
            if let Some(dependencies) = properties.get("pers_ids").and_then(Value::items) {
                for dependency in dependencies.iter().filter_map(Value::object) {
                    match read_record(document, dependency).map(|record| record.body) {
                        Some(Body::Dependency { entity, .. }) => entities.push(entity.handle),
                        _ => return Err(UNSUPPORTED),
                    }
                }
            } else if let Some(variable) = properties.get("var").and_then(Value::object) {
                let expression = match document.objects.get(&variable) {
                    Some(ObjectType::Associative(AssociativeObject {
                        data: AssociativeData::Variable(variable),
                        ..
                    })) => variable.expression.clone(),
                    _ => return Err(UNSUPPORTED),
                };
                let suppress = evaluate(&expression, &columns).ok_or(UNSUPPORTED)?;
                reactor_record
                    .body
                    .properties_mut()
                    .set("value", Value::Real(suppress));
                store(document, &reactor_record);
                set_variable(
                    document,
                    variable,
                    None,
                    AssocEvalVariant {
                        code: 90,
                        value: AssocEvalValue::Long(suppress as i32),
                    },
                )?;
                value = Some(suppress);
            }
        }
        let suppress = value.ok_or(UNSUPPORTED)? != 0.0;
        entities.sort_by_key(|entity| order.get(entity).copied().unwrap_or(usize::MAX));
        let properties = record.body.properties_mut();
        properties.set("Status", Value::Integer(if suppress { 32 } else { 0 }));
        let Some(Value::Map { entries, .. }) = properties.get_mut("aEntLayerKey") else {
            return Err(UNSUPPORTED);
        };
        entries.clear();
        if suppress {
            for entity in &entities {
                let layer = layers.get(entity).ok_or(UNSUPPORTED)?;
                let layer = layer_handle(document, layer).ok_or(UNSUPPORTED)?;
                entries.push((
                    Value::Object {
                        kind: 0,
                        target: Ref::new(*entity, 4),
                    },
                    Value::Object {
                        kind: 1,
                        target: Ref::new(layer, 5),
                    },
                ));
            }
            suppressed.extend(entities);
        }
        store(document, &record);
    }

    if !suppressed.is_empty() {
        ensure_suppress_layer(document);
    }
    for member in members {
        let layer = if suppressed.contains(member) {
            SUPPRESS_LAYER.to_string()
        } else {
            layers.get(member).cloned().ok_or(UNSUPPORTED)?
        };
        if document
            .get_entity(*member)
            .is_some_and(|entity| entity.common().layer != layer)
        {
            if let Some(entity) = document.get_entity_mut(*member) {
                entity.common_mut().layer = layer;
            }
        }
    }
    Ok(())
}

/// The value of a suppression's expression, such as `1 - look_3` or
/// `1 - look_13 * look_14`, over the table columns.
fn evaluate(expression: &str, columns: &FxHashMap<String, f64>) -> Option<f64> {
    const RESULT: &str = "bricscad_suppression";
    let mut table = crate::scene::named_parameters::ParameterTable::new();
    for (name, value) in columns {
        table.set(name, &value.to_string()).ok()?;
    }
    table.set(RESULT, expression).ok()?;
    table.resolve(RESULT).ok().filter(|value| value.is_finite())
}

/// Set an associative variable and every read of it: BricsCAD keeps each
/// value dependency at the value of the variable it reads, and takes a
/// variable whose new value its reads already hold for unchanged, leaving
/// what depends on it unevaluated.
pub(super) fn set_variable(
    document: &mut Document,
    variable: Handle,
    expression: Option<String>,
    value: AssocEvalVariant,
) -> Result<()> {
    let Some(ObjectType::Associative(AssociativeObject {
        data: AssociativeData::Variable(object),
        reactors,
        ..
    })) = document.objects.get_mut(&variable)
    else {
        return Err(UNSUPPORTED);
    };
    if let Some(expression) = expression {
        object.expression = expression;
    }
    object.value = value.clone();
    for reactor in reactors.clone() {
        if let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ValueDependency(read),
            ..
        })) = document.objects.get_mut(&reactor)
        {
            if read.dependency.dependent_on == variable {
                read.value = value.clone();
            }
        }
    }
    Ok(())
}

/// The layer BricsCAD keeps hidden members on: frozen and off.
fn ensure_suppress_layer(document: &mut Document) {
    if document.layers.get(SUPPRESS_LAYER).is_some() {
        return;
    }
    let mut layer = document::tables::Layer::new(SUPPRESS_LAYER);
    layer.flags.frozen = true;
    layer.flags.off = true;
    layer.handle = document.allocate_handle();
    let _ = document.layers.add(layer);
}

/// A copy of `master`, other than the block itself, that shows `states`.
fn find_copy(document: &Document, master: Handle, states: &[String]) -> Option<Handle> {
    document
        .block_records
        .iter()
        .filter(|record| record.is_anonymous() && record.handle != master)
        .filter(|record| original_block(document, record.handle) == Some(master))
        .find(|record| {
            let parameters = visibility_parameters(document, record.handle);
            parameters.len() == states.len()
                && parameters.iter().zip(states).all(|(parameter, state)| {
                    parameter
                        .current
                        .and_then(|current| parameter.states.get(current))
                        == Some(state)
                })
        })
        .map(|record| record.handle)
}

/// After the reference `insert` was pointed at `block`, an attribute whose
/// definition there is on `*SUPPRESS` goes there too, and one on `*SUPPRESS`
/// whose definition is not goes back to the definition's layer, as BricsCAD
/// keeps them.
fn follow_attribute_definitions(document: &mut Document, insert: Handle, block: Handle) {
    let definitions: FxHashMap<String, String> =
        crate::scene::block_authoring::members(document, block)
            .into_iter()
            .filter_map(|member| match document.get_entity(member) {
                Some(EntityType::AttributeDefinition(definition)) => Some((
                    definition.tag.to_uppercase(),
                    definition.common.layer.clone(),
                )),
                _ => None,
            })
            .collect();
    let needs = match document.get_entity(insert) {
        Some(EntityType::Insert(reference)) => reference.attributes.iter().any(|attribute| {
            target_layer(&definitions, &attribute.tag, &attribute.common.layer).is_some()
        }),
        _ => false,
    };
    if !needs {
        return;
    }
    if let Some(EntityType::Insert(reference)) = document.get_entity_mut(insert) {
        for attribute in &mut reference.attributes {
            if let Some(layer) = target_layer(&definitions, &attribute.tag, &attribute.common.layer)
            {
                attribute.common.layer = layer;
                attribute.common.raw_record = None;
            }
        }
    }
}

fn target_layer(definitions: &FxHashMap<String, String>, tag: &str, layer: &str) -> Option<String> {
    let definition = definitions.get(&tag.to_uppercase())?;
    if definition == SUPPRESS_LAYER && layer != SUPPRESS_LAYER {
        Some(SUPPRESS_LAYER.to_string())
    } else if definition != SUPPRESS_LAYER && layer == SUPPRESS_LAYER {
        Some(definition.clone())
    } else {
        None
    }
}

/// The reference's entry in model space: the `BmAssemblyParameters` feature
/// its `BDMOBJECT` leads to, with its record.
pub(super) fn find_entry(document: &Document, insert: Handle) -> Option<(Handle, Record)> {
    let reactors = document.get_entity(insert)?.common().reactors.clone();
    reactors.into_iter().find_map(|reactor| {
        let Body::Object { properties, .. } = read_record(document, reactor)?.body else {
            return None;
        };
        properties
            .get("PersIdDependencies")?
            .items()?
            .iter()
            .filter_map(Value::object)
            .find_map(|dependency| {
                read_record(document, dependency)?
                    .reactors
                    .iter()
                    .find_map(|reactor| {
                        let feature = read_record(document, reactor.handle)?.owner.handle;
                        let record = read_record(document, feature)?;
                        (record.body.properties().text("Feature_type")
                            == Some("BmAssemblyParameters"))
                        .then_some((feature, record))
                    })
            })
    })
}

/// Write the reference's entry with `overrides`, creating it when there is
/// none.
fn write_entry(
    document: &mut Document,
    insert: Handle,
    master: Handle,
    overrides: Vec<(String, String)>,
) -> Result<()> {
    let constants = |entries: Vec<(String, String)>| Value::Map {
        key: 2,
        value: 2,
        entries: entries
            .into_iter()
            .map(|(name, state)| (Value::Text(name), Value::Text(state)))
            .collect(),
    };
    if let Some((_, mut record)) = find_entry(document, insert) {
        if !record
            .body
            .properties_mut()
            .set("expr.const", constants(overrides))
        {
            return Err(UNSUPPORTED);
        }
        store(document, &record);
        return Ok(());
    }

    let template = |class: &str| template_record(document, master, class).ok_or(UNSUPPORTED);
    let feature_template = template("BDMDBFEATUREDATA")?;
    let reactor_template = template("BDMDBPERSIDREACTOR")?;
    let dependency_template = template("BDMDBOBJECTDEPENDENCY")?;
    let object_template = template("BDMOBJECT")?;
    let (feature_manager, dependency_manager, constraint_manager) =
        model_database(document, master)?;

    let names: Vec<String> = features(document, document.header.model_space_block_handle)
        .into_iter()
        .filter_map(|feature| {
            super::properties(document, feature)?
                .text("Name")
                .map(str::to_string)
        })
        .collect();
    let base = format!("*{:X}", insert.value());
    let name = std::iter::once(base.clone())
        .chain((1..).map(|n| format!("{base}_{n}")))
        .find(|name| !names.contains(name))
        .unwrap_or(base);
    let mut constraint_record = read_record(document, constraint_manager).ok_or(UNSUPPORTED)?;
    let object_number = match &constraint_record.body {
        Body::ConstraintManager {
            counts, children, ..
        } => {
            children
                .iter()
                .take(usize::try_from(counts[1]).unwrap_or(0))
                .filter_map(|child| match read_record(document, child.handle)?.body {
                    Body::Object { properties, .. } => properties
                        .text("NAME")?
                        .strip_prefix("Objekt_")?
                        .parse::<u32>()
                        .ok(),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
                + 1
        }
        _ => return Err(UNSUPPORTED),
    };

    let [feature, reactor, dependency, object] = [(); 4].map(|_| document.allocate_handle());
    let object_lead = match &object_template.body {
        Body::Object {
            lead,
            lead_count,
            tail,
            ..
        } => (*lead, *lead_count, *tail),
        _ => return Err(UNSUPPORTED),
    };
    let new_records = [
        Record::like(
            &feature_template,
            feature,
            Ref::new(feature_manager, 4),
            Body::Properties(Properties(vec![
                ("_ver".into(), Value::Byte(1)),
                (
                    "Feature_type".into(),
                    Value::Text("BmAssemblyParameters".into()),
                ),
                ("Name".into(), Value::Text(name)),
                (
                    "Reactors".into(),
                    Value::List {
                        element: 8,
                        items: vec![Value::Object {
                            kind: 3,
                            target: Ref::new(reactor, 3),
                        }],
                    },
                ),
                ("Status".into(), Value::Integer(0)),
                ("expr.const".into(), constants(overrides)),
                ("expr.orig".into(), constants(Vec::new())),
                (
                    "modifiedParams".into(),
                    Value::List {
                        element: 2,
                        items: Vec::new(),
                    },
                ),
            ])),
        ),
        Record::like(
            &reactor_template,
            reactor,
            Ref::new(feature, 4),
            Body::Properties(Properties(vec![
                ("_ver".into(), Value::Byte(1)),
                ("mask".into(), Value::Integer(1)),
                (
                    "pers_ids".into(),
                    Value::List {
                        element: 8,
                        items: vec![Value::Object {
                            kind: 0,
                            target: Ref::new(dependency, 4),
                        }],
                    },
                ),
                ("tag".into(), Value::Integer(0)),
            ])),
        ),
        {
            let mut record = Record::like(
                &dependency_template,
                dependency,
                Ref::new(dependency_manager, 4),
                Body::Dependency {
                    properties: Properties::default(),
                    tail: 0,
                    entity: Ref::new(insert, 5),
                },
            );
            record.reactors = vec![Ref::new(reactor, 4)];
            record
        },
        Record::like(
            &object_template,
            object,
            Ref::new(constraint_manager, 4),
            Body::Object {
                lead: object_lead.0,
                lead_count: object_lead.1,
                entity: Ref::new(insert, 4),
                properties: Properties(vec![
                    (
                        "NAME".into(),
                        Value::Text(format!("Objekt_{object_number}")),
                    ),
                    (
                        "PersIdDependencies".into(),
                        Value::List {
                            element: 8,
                            items: vec![Value::Object {
                                kind: 0,
                                target: Ref::new(dependency, 4),
                            }],
                        },
                    ),
                ]),
                tail: object_lead.2,
            },
        ),
    ];
    let classes = [
        "BDMDBFEATUREDATA",
        "BDMDBPERSIDREACTOR",
        "BDMDBOBJECTDEPENDENCY",
        "BDMOBJECT",
    ];
    for (class, record) in classes.iter().zip(&new_records) {
        document
            .objects
            .insert(record.handle, unknown(class, record));
    }

    append_object(document, feature_manager, "Features", feature)?;
    append_object(document, dependency_manager, "deps", dependency)?;
    let Body::ConstraintManager {
        counts, children, ..
    } = &mut constraint_record.body
    else {
        return Err(UNSUPPORTED);
    };
    let at = usize::try_from(counts[1]).map_err(|_| UNSUPPORTED)?;
    children.insert(at.min(children.len()), Ref::new(object, 3));
    counts[1] += 1;
    store(document, &constraint_record);
    if let Some(entity) = document.get_entity_mut(insert) {
        entity.common_mut().reactors.push(object);
    }
    Ok(())
}

/// Append a reference to `target` to the list property `list` of `manager`.
fn append_object(
    document: &mut Document,
    manager: Handle,
    list: &str,
    target: Handle,
) -> Result<()> {
    let mut record = read_record(document, manager).ok_or(UNSUPPORTED)?;
    let Some(Value::List { items, .. }) = record.body.properties_mut().get_mut(list) else {
        return Err(UNSUPPORTED);
    };
    items.push(Value::Object {
        kind: 3,
        target: Ref::new(target, 3),
    });
    store(document, &record);
    Ok(())
}

/// Model space's feature manager, dependency manager and constraint manager,
/// created after those of `master` when model space has none yet.
fn model_database(document: &mut Document, master: Handle) -> Result<(Handle, Handle, Handle)> {
    let model = document.header.model_space_block_handle;
    let entries = |document: &Document, owner: Handle| {
        Some((
            database_entry(document, owner, "FEATURE_MANAGER")?,
            database_entry(document, owner, "PERSIDMANAGER")?,
            database_entry(document, owner, "BDM_DATABASE")?,
        ))
    };
    if let Some(found) = entries(document, model) {
        return Ok(found);
    }
    let model_dictionary = document.ensure_extension_dictionary(model);
    let has_database = matches!(
        document.objects.get(&model_dictionary),
        Some(ObjectType::Dictionary(dictionary)) if dictionary.get("BDM_DATABASE").is_some()
    );
    if has_database {
        // A database without all three managers is not one this writes into.
        return Err(UNSUPPORTED);
    }
    let master_dictionary = document
        .extension_dictionary_handle(master)
        .ok_or(UNSUPPORTED)?;
    let (master_database, hard_owned) = match document.objects.get(&master_dictionary) {
        Some(ObjectType::Dictionary(dictionary)) => (
            dictionary.get("BDM_DATABASE").ok_or(UNSUPPORTED)?,
            dictionary
                .hard_owner_entries
                .iter()
                .any(|key| key.eq_ignore_ascii_case("BDM_DATABASE")),
        ),
        _ => return Err(UNSUPPORTED),
    };
    let Some(ObjectType::Dictionary(template)) = document.objects.get(&master_database).cloned()
    else {
        return Err(UNSUPPORTED);
    };
    let (feature_template, dependency_template, constraint_template) =
        entries(document, master).ok_or(UNSUPPORTED)?;
    let database = document.allocate_handle();
    let mut created = Vec::new();
    let mut handles = FxHashMap::default();
    for (key, template_handle) in [
        ("FEATURE_MANAGER", feature_template),
        ("PERSIDMANAGER", dependency_template),
        ("BDM_DATABASE", constraint_template),
    ] {
        let mut record = read_record(document, template_handle).ok_or(UNSUPPORTED)?;
        let handle = document.allocate_handle();
        record.handle = handle;
        record.owner.handle = database;
        for reactor in &mut record.reactors {
            reactor.handle = database;
        }
        match &mut record.body {
            Body::Properties(properties) => {
                properties
                    .0
                    .retain(|(name, _)| !name.starts_with("original."));
                for list in ["Features", "Contexts", "deps"] {
                    if let Some(Value::List { items, .. }) = properties.get_mut(list) {
                        items.clear();
                    }
                }
            }
            Body::ConstraintManager {
                counts, children, ..
            } => {
                counts[1] = 0;
                counts[3] = 0;
                children.clear();
            }
            _ => return Err(UNSUPPORTED),
        }
        let class = match key {
            "FEATURE_MANAGER" => "BDMDBFEATUREMANAGER",
            "PERSIDMANAGER" => "BdmDbPersIdManager",
            _ => "BDMDBCONSTRAINTMANAGER",
        };
        created.push((handle, unknown(class, &record)));
        handles.insert(key, handle);
    }
    let mut dictionary = template;
    dictionary.handle = database;
    dictionary.owner = model_dictionary;
    dictionary.reactors = vec![model_dictionary];
    dictionary.xdictionary_handle = None;
    for (key, entry) in &mut dictionary.entries {
        *entry = *handles.get(key.as_str()).ok_or(UNSUPPORTED)?;
    }
    document
        .objects
        .insert(database, ObjectType::Dictionary(dictionary));
    for (handle, object) in created {
        document.objects.insert(handle, object);
    }
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&model_dictionary) {
        dictionary.add_entry("BDM_DATABASE", database);
        if hard_owned {
            dictionary.hard_owner_entries.push("BDM_DATABASE".into());
        }
    }
    entries(document, model).ok_or(UNSUPPORTED)
}

/// A record of `class` to make new ones like: one under `master`'s extension
/// dictionary, or any in the drawing.
fn template_record(document: &Document, master: Handle, class: &str) -> Option<Record> {
    let tree: FxHashSet<Handle> = document
        .extension_dictionary_handle(master)
        .and_then(|root| owned_tree(document, root).ok())
        .map(|tree| tree.into_iter().collect())
        .unwrap_or_default();
    let mut candidates: Vec<Handle> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::Unknown { type_name, .. } if type_name == class => Some(*handle),
            _ => None,
        })
        .collect();
    candidates.sort_by_key(|handle| (!tree.contains(handle), *handle));
    candidates
        .into_iter()
        .find_map(|handle| read_record(document, handle))
}

/// The tag BricsCAD gives a parametric block, kept in its extended data and
/// named by every copy (`original.tag`).
fn block_tag(document: &Document, block: Handle) -> Option<String> {
    const NAME: &[u8] = b"ParametricBlockGUID";
    document
        .object_xdata(block)?
        .records()
        .iter()
        .filter(|record| record.application_name == "BDM_DB")
        .flat_map(|record| &record.values)
        .find_map(|value| {
            let document::xdata::XDataValue::BinaryData(bytes) = value else {
                return None;
            };
            let at = bytes
                .windows(NAME.len())
                .position(|window| window == NAME)?
                + NAME.len();
            // A type byte (2: text), the length and the characters.
            if *bytes.get(at)? != 2 {
                return None;
            }
            let length = usize::from(u16::from_le_bytes([
                *bytes.get(at + 1)?,
                *bytes.get(at + 2)?,
            ]));
            let text = bytes.get(at + 3..at + 3 + length)?;
            String::from_utf8(text.to_vec()).ok()
        })
}

/// Give `block` a new tag the way BricsCAD stores it, and return it.
fn give_block_tag(document: &mut Document, block: Handle) -> String {
    const NAME: &[u8] = b"ParametricBlockGUID";
    let tag = crate::scene::block_authoring::new_guid().to_lowercase();
    let mut bytes = vec![1, 0, 0, 0];
    bytes.extend((NAME.len() as u16).to_le_bytes());
    bytes.extend(NAME);
    bytes.push(2);
    bytes.extend((tag.len() as u16).to_le_bytes());
    bytes.extend(tag.as_bytes());
    crate::scene::block_authoring::ensure_app_id(document, "BDM_DB");
    let mut record = document::xdata::ExtendedDataRecord::new("BDM_DB");
    record.add_value(document::xdata::XDataValue::BinaryData(bytes));
    document.set_object_xdata_record(block, record);
    tag
}

/// The handle as BricsCAD stores it in a raw value: its bytes, most
/// significant first, without leading zeros.
fn handle_bytes(handle: Handle) -> Vec<u8> {
    handle
        .value()
        .to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect()
}

/// The next free name of the form `*U<n>`.
fn next_anonymous_name(document: &Document) -> String {
    let next = document
        .block_records
        .iter()
        .filter_map(|record| record.name.strip_prefix("*U")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1);
    format!("*U{next}")
}

/// The raw object for `record`.
pub(super) fn unknown(class: &str, record: &Record) -> ObjectType {
    let (data, handle_bits) = record::encode(record);
    ObjectType::Unknown {
        type_name: class.to_string(),
        handle: record.handle,
        owner: record.owner.handle,
        raw_dxf_codes: None,
        raw_dwg_data: Some(data),
        raw_dwg_handle_bits: handle_bits,
        raw_dwg_version: Some(record.version),
    }
}

/// Store a changed record over the object it was read from.
pub(super) fn store(document: &mut Document, record: &Record) {
    let class = match document.objects.get(&record.handle) {
        Some(ObjectType::Unknown { type_name, .. }) => type_name.clone(),
        _ => return,
    };
    document
        .objects
        .insert(record.handle, unknown(&class, record));
}
