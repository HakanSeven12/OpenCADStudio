//! Visibility states of BricsCAD parametric blocks, read from the objects
//! BricsCAD keeps them in.
//!
//! BricsCAD stores a parametric block's parameters in objects of its own
//! classes under `BDM_DATABASE` in the block's extension dictionary. They are
//! read as raw records (`ObjectType::Unknown`); this module decodes the few it
//! needs from their DWG bytes (DWG 2010 and later):
//!
//! - `BDMDBFEATUREMANAGER` (`FEATURE_MANAGER`) lists the block's features.
//! - A `BDMDBFEATUREDATA` of type `BdmConfigurationFeature` whose table type
//!   is `VisibilityStates` is one visibility parameter: its name and the state
//!   the block shows (`LastKeyValue`).
//! - Its `BDMDBCONFIGURATIONTABLE` names the states (`KeyValues`).
//! - The `BdmDbPersIdManager` (`PERSIDMANAGER`) of an anonymous copy names the
//!   block it was made from (`original.block`).
//!
//! A reference in a state other than the definition's shows such a copy, whose
//! own features hold the state it shows; the members a state hides sit on the
//! layer `*SUPPRESS` there.
//!
//! [`record`] decodes and encodes the objects; `switch` changes the state a
//! reference shows the way BricsCAD does.

use document::objects::ObjectType;
use document::{Document, EntityType, Handle};

mod author;
pub(crate) mod record;
mod switch;

pub(crate) use author::{parameter_states, parameter_views, shown_layers, Action, ParameterView};

pub(crate) use switch::can_switch;

use record::{Properties, Record};

/// A visibility parameter of a BricsCAD parametric block.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityParameter {
    pub name: String,
    pub states: Vec<String>,
    /// The state the block shows, by index into `states`.
    pub current: Option<usize>,
    /// The parameter's feature in the block it was read from.
    pub feature: Handle,
}

/// The visibility parameters of block record `block`, in the order its feature
/// list holds them. Empty for a block BricsCAD did not make parametric, and
/// for one whose objects cannot be read.
pub fn visibility_parameters(document: &Document, block: Handle) -> Vec<VisibilityParameter> {
    features(document, block)
        .into_iter()
        .filter_map(|feature| visibility_parameter(document, feature))
        .collect()
}

/// The visibility parameters of the block reference `insert`, in the states
/// it shows.
pub fn reference_parameters(document: &Document, insert: Handle) -> Vec<VisibilityParameter> {
    shown_block(document, insert)
        .map(|block| visibility_parameters(document, block))
        .unwrap_or_default()
}

/// The state parameter `name` of `owner` (block or model space) shows.
pub fn current_state(document: &Document, owner: Handle, name: &str) -> Option<String> {
    visibility_parameters(document, owner)
        .into_iter()
        .find(|parameter| parameter.name.eq_ignore_ascii_case(name))
        .and_then(|parameter| parameter.states.get(parameter.current?).cloned())
}

/// The block an anonymous copy of a BricsCAD parametric block was made from.
pub fn original_block(document: &Document, block: Handle) -> Option<Handle> {
    let manager = database_entry(document, block, "PERSIDMANAGER")?;
    let original = properties(document, manager)?
        .get("original.block")?
        .object()?;
    (!original.is_null() && original != block).then_some(original)
}

/// The BricsCAD parametric block a reference shows, or shows a copy of.
pub fn master_block(document: &Document, insert: Handle) -> Option<Handle> {
    let block = shown_block(document, insert)?;
    let master = original_block(document, block).unwrap_or(block);
    (!visibility_parameters(document, master).is_empty()).then_some(master)
}

/// The block record a reference shows.
fn shown_block(document: &Document, insert: Handle) -> Option<Handle> {
    let Some(EntityType::Insert(reference)) = document.get_entity(insert) else {
        return None;
    };
    document
        .block_records
        .get(&reference.block_name)
        .map(|record| record.handle)
}

/// The features of `block`'s feature manager, in stored order.
fn features(document: &Document, block: Handle) -> Vec<Handle> {
    database_entry(document, block, "FEATURE_MANAGER")
        .and_then(|manager| properties(document, manager))
        .and_then(|manager| {
            Some(
                manager
                    .get("Features")?
                    .items()?
                    .iter()
                    .filter_map(record::Value::object)
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn visibility_parameter(document: &Document, feature: Handle) -> Option<VisibilityParameter> {
    let handle = feature;
    let feature = properties(document, feature)?;
    if feature.text("Feature_type")? != "BdmConfigurationFeature"
        || feature.text("TableType")? != "VisibilityStates"
    {
        return None;
    }
    let table = properties(document, feature.get("ConfigTable")?.object()?)?;
    let states: Vec<String> = table
        .get("KeyValues")?
        .items()?
        .iter()
        .map(|state| state.text().map(str::to_string))
        .collect::<Option<_>>()?;
    let name = table
        .text("KeyName")
        .or_else(|| feature.text("Name"))?
        .to_string();
    let current = feature
        .text("LastKeyValue")
        .and_then(|shown| states.iter().position(|state| state == shown));
    Some(VisibilityParameter {
        name,
        states,
        current,
        feature: handle,
    })
}

/// Entry `key` of the `BDM_DATABASE` dictionary in `owner`'s extension
/// dictionary.
fn database_entry(document: &Document, owner: Handle, key: &str) -> Option<Handle> {
    let dictionary = |handle: Handle| match document.objects.get(&handle)? {
        ObjectType::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    };
    let extension = dictionary(document.extension_dictionary_handle(owner)?)?;
    dictionary(extension.get("BDM_DATABASE")?)?.get(key)
}

/// The record of the BricsCAD object `handle`, decoded from the DWG record it
/// was read from. `None` for any other object, a record from a DXF file or a
/// DWG before 2010, and a record that does not decode in full.
pub(crate) fn record(document: &Document, handle: Handle) -> Option<Record> {
    let ObjectType::Unknown {
        type_name,
        raw_dwg_data: Some(data),
        raw_dwg_handle_bits,
        raw_dwg_version: Some(version),
        ..
    } = document.objects.get(&handle)?
    else {
        return None;
    };
    record::decode(type_name, data, *raw_dwg_handle_bits, *version)
}

/// The properties of the BricsCAD object `handle` whose record is a plain
/// property list (see [`record`]).
pub(crate) fn properties(document: &Document, handle: Handle) -> Option<Properties> {
    match record(document, handle)?.body {
        record::Body::Properties(properties) => Some(properties),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod record_tests;

#[cfg(test)]
mod switch_tests;

#[cfg(test)]
pub(crate) mod author_tests;
