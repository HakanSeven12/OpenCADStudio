//! Switching on the BricsCAD drawing in `tests/fixtures/bricscad` (see its
//! README): EE shows the block in On/Shown, 146 the copy *U2 in Off/Shown and
//! 19E the copy *U3 in Off/Hidden; 146 and 19E have entries in model space.

use super::record::{Body, Value};
use super::*;
use crate::scene::Scene;
use std::collections::{BTreeMap, BTreeSet};

const MASTER: &str = "ocs-bricscad-visibility-block";

fn opened() -> Scene {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bricscad/visibility-states.dwg"
    );
    let bytes = std::fs::read(path).expect("read the fixture");
    let mut scene = Scene::new();
    scene.document =
        crate::io::load_bytes_finalized(std::path::Path::new("visibility-states.dwg"), bytes)
            .expect("open the fixture");
    scene.load_named_parameters_from_document();
    scene.load_parametric_constraints_from_document();
    scene
}

fn reference(value: u64) -> Handle {
    Handle::from(value)
}

fn block_of(document: &Document, insert: u64) -> String {
    match document.get_entity(reference(insert)) {
        Some(EntityType::Insert(insert)) => insert.block_name.clone(),
        other => panic!("{other:?}"),
    }
}

/// Each parameter and the state the reference shows.
fn shown(document: &Document, insert: u64) -> Vec<(String, String)> {
    reference_parameters(document, reference(insert))
        .into_iter()
        .map(|parameter| {
            let state = parameter.states[parameter.current.unwrap()].clone();
            (parameter.name, state)
        })
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// The parameters the reference's entry in model space lists.
fn entry(document: &Document, insert: u64) -> Option<Vec<(String, String)>> {
    let (_, record) = super::switch::find_entry(document, reference(insert))?;
    Some(
        record
            .body
            .properties()
            .get("expr.const")?
            .entries()?
            .iter()
            .map(|(key, value)| {
                (
                    key.text().unwrap().to_string(),
                    value.text().unwrap().to_string(),
                )
            })
            .collect(),
    )
}

/// The references in the BricsCAD objects of `document` that name an object
/// the drawing does not have, as (holder, target).
fn dangling_references(document: &Document) -> BTreeSet<(Handle, Handle)> {
    let mut dangling = BTreeSet::new();
    for (handle, object) in &document.objects {
        if !matches!(object, ObjectType::Unknown { type_name, .. } if type_name.to_ascii_uppercase().starts_with("BDM"))
        {
            continue;
        }
        let mut record = record(document, *handle).expect("a BricsCAD object decodes");
        record.refs_mut(|reference| {
            let target = reference.handle;
            let known = target.is_null()
                || document.objects.contains_key(&target)
                || document.get_entity(target).is_some()
                || document.block_records.iter().any(|r| r.handle == target)
                || document.layers.iter().any(|layer| layer.handle == target);
            if !known {
                dangling.insert((*handle, target));
            }
        });
    }
    dangling
}

/// No reference in the BricsCAD objects names an object the drawing lacks,
/// apart from those BricsCAD itself left in the fixture: its global constraint
/// manager still lists child managers it purged.
fn assert_no_dangling_references(document: &Document) {
    let left_by_bricscad = dangling_references(&opened().document);
    let dangling = dangling_references(document);
    let new: Vec<_> = dangling.difference(&left_by_bricscad).collect();
    assert!(
        new.is_empty(),
        "references to objects the drawing lacks: {new:?}"
    );
}

#[test]
fn a_state_another_copy_shows_shares_that_copy_and_gives_the_reference_an_entry() {
    let mut scene = opened();
    assert_eq!(entry(&scene.document, 0xEE), None);
    assert!(scene
        .set_bricscad_visibility(reference(0xEE), "LAMP", "Off")
        .unwrap());
    let document = &scene.document;
    assert_eq!(
        block_of(document, 0xEE),
        "*U2",
        "BricsCAD shares copies too"
    );
    assert_eq!(
        shown(document, 0xEE),
        pairs(&[("LAMP", "Off"), ("LABEL", "Shown")])
    );
    assert_eq!(entry(document, 0xEE), Some(pairs(&[("LAMP", "Off")])));
    assert_eq!(
        shown(document, 0x146),
        pairs(&[("LAMP", "Off"), ("LABEL", "Shown")])
    );
    assert_no_dangling_references(document);
}

#[test]
fn the_block_s_own_states_show_the_block_and_the_entry_keeps_the_parameter() {
    let mut scene = opened();
    assert!(scene
        .set_bricscad_visibility(reference(0x146), "LAMP", "On")
        .unwrap());
    let document = &scene.document;
    assert_eq!(block_of(document, 0x146), MASTER);
    assert_eq!(
        shown(document, 0x146),
        pairs(&[("LAMP", "On"), ("LABEL", "Shown")])
    );
    // BricsCAD keeps a parameter in the entry once set, also in the block's
    // own state.
    assert_eq!(entry(document, 0x146), Some(pairs(&[("LAMP", "On")])));
}

#[test]
fn a_state_no_block_shows_gets_a_new_copy() {
    let mut scene = opened();
    let records = scene.document.block_records.iter().count();
    assert!(scene
        .set_bricscad_visibility(reference(0xEE), "LABEL", "Hidden")
        .unwrap());
    let document = &scene.document;
    assert_eq!(document.block_records.iter().count(), records + 1);
    let copy = block_of(document, 0xEE);
    assert_eq!(copy, "*U4");
    let copy = document.block_records.get(&copy).unwrap().handle;
    let master = document.block_records.get(MASTER).unwrap().handle;
    assert_eq!(original_block(document, copy), Some(master));
    assert_eq!(
        shown(document, 0xEE),
        pairs(&[("LAMP", "On"), ("LABEL", "Hidden")])
    );
    assert_eq!(entry(document, 0xEE), Some(pairs(&[("LABEL", "Hidden")])));
    let layers: Vec<String> = crate::scene::block_authoring::members(document, copy)
        .into_iter()
        .map(|member| document.get_entity(member).unwrap().common().layer.clone())
        .collect();
    assert_eq!(
        layers,
        ["0", "0", "0", "0", "*SUPPRESS"],
        "only the text is hidden"
    );
    // The block and the other references are left alone.
    assert_eq!(
        shown(document, 0x146),
        pairs(&[("LAMP", "Off"), ("LABEL", "Shown")])
    );
    assert_eq!(
        visibility_parameters(document, master)
            .iter()
            .map(|p| p.current)
            .collect::<Vec<_>>(),
        [Some(0), Some(0)]
    );
    assert_no_dangling_references(document);
}

/// A property value with references written as the kind, code and class of
/// what they name, and members as their place in the block.
fn normalized(document: &Document, members: &[Handle], value: &Value) -> String {
    match value {
        Value::Object { kind, target } => {
            let what = members
                .iter()
                .position(|member| *member == target.handle)
                .map(|index| format!("member {index}"))
                .or_else(|| {
                    document
                        .objects
                        .get(&target.handle)
                        .map(|object| match object {
                            ObjectType::Unknown { type_name, .. } => type_name.clone(),
                            ObjectType::Associative(object) => object.dxf_name.clone(),
                            other => format!("{:?}", std::mem::discriminant(other)),
                        })
                })
                .or_else(|| {
                    document
                        .layers
                        .iter()
                        .find(|layer| layer.handle == target.handle)
                        .map(|layer| format!("layer {}", layer.name))
                })
                .or_else(|| {
                    document
                        .block_records
                        .iter()
                        .find(|record| record.handle == target.handle)
                        .map(|record| format!("block {}", record.name))
                })
                .unwrap_or_else(|| format!("{:?}", target.handle));
            format!("ref({kind},{},{what})", target.code)
        }
        Value::List { element, items } => format!(
            "list {element} [{}]",
            items
                .iter()
                .map(|item| normalized(document, members, item))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Map {
            key,
            value,
            entries,
        } => format!(
            "map {key} {value} [{}]",
            entries
                .iter()
                .map(|(k, v)| format!(
                    "{} => {}",
                    normalized(document, members, k),
                    normalized(document, members, v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Raw(bytes) => {
            // A raw handle of a member of the block the copy was made from.
            let handle = Handle::from(bytes.iter().fold(0u64, |v, b| v << 8 | u64::from(*b)));
            let master = document.block_records.get(MASTER).unwrap().handle;
            let index = crate::scene::block_authoring::members(document, master)
                .iter()
                .position(|member| *member == handle);
            format!("raw member {index:?}")
        }
        other => format!("{other:?}"),
    }
}

/// Every BricsCAD object under `block`'s extension dictionary, in the order of
/// a walk through its feature and other managers, each as its class and
/// normalized properties.
fn parameter_objects(document: &Document, block: Handle) -> Vec<String> {
    let members = crate::scene::block_authoring::members(document, block);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<Handle> = ["BDM_DATABASE", "FEATURE_MANAGER", "PERSIDMANAGER"]
        .iter()
        .filter_map(|key| database_entry(document, block, key))
        .collect();
    stack.reverse();
    while let Some(handle) = stack.pop() {
        if !seen.insert(handle) {
            continue;
        }
        let Some(record) = record(document, handle) else {
            continue;
        };
        let class = match document.objects.get(&handle) {
            Some(ObjectType::Unknown { type_name, .. }) => type_name.clone(),
            _ => continue,
        };
        let mut children = Vec::new();
        let properties = match &record.body {
            Body::Properties(properties) => properties.clone(),
            Body::ConstraintManager {
                counts,
                children: owned,
                settings,
            } => {
                children.extend(owned.iter().map(|r| r.handle));
                out.push(format!("{class} counts {counts:?}"));
                settings.clone()
            }
            Body::Object {
                entity, properties, ..
            } => {
                out.push(format!(
                    "{class} entity {}",
                    normalized(
                        document,
                        &members,
                        &Value::Object {
                            kind: 0,
                            target: *entity
                        }
                    )
                ));
                properties.clone()
            }
            Body::Dependency {
                entity, properties, ..
            } => {
                out.push(format!(
                    "{class} entity {}",
                    normalized(
                        document,
                        &members,
                        &Value::Object {
                            kind: 0,
                            target: *entity
                        }
                    )
                ));
                properties.clone()
            }
        };
        for (name, value) in &properties.0 {
            out.push(format!(
                "{class}.{name} = {}",
                normalized(document, &members, value)
            ));
            let mut value = value.clone();
            let mut collect = |value: &Value| {
                if let Value::Object { target, .. } = value {
                    children.push(target.handle);
                }
            };
            walk(&mut value, &mut collect);
        }
        stack.extend(children.into_iter().rev());
    }
    out
}

fn walk(value: &mut Value, visit: &mut impl FnMut(&Value)) {
    visit(value);
    match value {
        Value::List { items, .. } => items.iter_mut().for_each(|item| walk(item, visit)),
        Value::Map { entries, .. } => entries.iter_mut().for_each(|(k, v)| {
            walk(k, visit);
            walk(v, visit);
        }),
        _ => {}
    }
}

#[test]
fn a_new_copy_holds_what_bricscad_s_own_copy_in_the_same_states_holds() {
    let mut scene = opened();
    let master = scene.document.block_records.get(MASTER).unwrap().handle;
    let bricscad = scene.document.block_records.get("*U3").unwrap().handle;
    let states: BTreeMap<String, String> = [("LAMP", "Off"), ("LABEL", "Hidden")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let listed: BTreeSet<String> = ["LAMP", "LABEL"].iter().map(|s| s.to_string()).collect();
    let copy = scene
        .clone_parametric_copy(master, &states, &listed)
        .unwrap();
    let ours = parameter_objects(&scene.document, copy);
    let theirs = parameter_objects(&scene.document, bricscad);
    assert!(ours.len() > 40, "{ours:#?}");
    // The key variables' numbers follow no rule BricsCAD keeps: the same
    // state carries 0 in one of its copies and 1 in another.
    let differences: Vec<(&String, &String)> = ours
        .iter()
        .zip(&theirs)
        .filter(|(a, b)| a != b)
        .filter(|(a, _)| !a.starts_with("BDMDBCONFIGVARREACTOR.Value"))
        .collect();
    assert_eq!(ours.len(), theirs.len(), "{ours:#?}\n{theirs:#?}");
    assert!(differences.is_empty(), "{differences:#?}");
    // Member layers too.
    let layers = |block: Handle| -> Vec<String> {
        crate::scene::block_authoring::members(&scene.document, block)
            .into_iter()
            .map(|m| scene.document.get_entity(m).unwrap().common().layer.clone())
            .collect()
    };
    assert_eq!(layers(copy), layers(bricscad));
}

#[test]
fn the_switched_drawing_keeps_its_parameters_through_a_dwg_save() {
    let mut scene = opened();
    scene
        .set_bricscad_visibility(reference(0xEE), "LABEL", "Hidden")
        .unwrap();
    scene
        .set_bricscad_visibility(reference(0x19E), "LAMP", "On")
        .unwrap();
    scene.sync_native_parametric_graph();
    let saved = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
        .expect("save as DWG");
    let reopened = crate::io::load_bytes_finalized(std::path::Path::new("saved.dwg"), saved)
        .expect("open the saved DWG");
    assert_eq!(
        shown(&reopened, 0xEE),
        pairs(&[("LAMP", "On"), ("LABEL", "Hidden")])
    );
    assert_eq!(
        shown(&reopened, 0x146),
        pairs(&[("LAMP", "Off"), ("LABEL", "Shown")])
    );
    assert_eq!(
        shown(&reopened, 0x19E),
        pairs(&[("LAMP", "On"), ("LABEL", "Hidden")])
    );
    assert_eq!(
        block_of(&reopened, 0x19E),
        block_of(&reopened, 0xEE),
        "shared"
    );
    assert_eq!(
        entry(&reopened, 0x19E),
        Some(pairs(&[("LABEL", "Hidden"), ("LAMP", "On")]))
    );
    assert_no_dangling_references(&reopened);
}

#[test]
fn an_unknown_parameter_or_state_is_refused_and_a_shown_state_changes_nothing() {
    let mut scene = opened();
    let before = scene.document.clone();
    assert!(scene
        .set_bricscad_visibility(reference(0xEE), "NOPE", "On")
        .is_err());
    assert!(scene
        .set_bricscad_visibility(reference(0xEE), "LAMP", "Dim")
        .is_err());
    assert_eq!(
        scene.set_bricscad_visibility(reference(0xEE), "LAMP", "On"),
        Ok(false)
    );
    assert!(scene.document == before);
}

/// BricsCAD keeps each read of a variable at the variable's value, and takes a
/// variable whose reads already hold its new value for unchanged: a stale
/// read would keep it from evaluating what depends on the variable.
#[test]
fn each_read_of_a_variable_holds_the_variable_s_value_after_a_switch() {
    use document::objects::{AssociativeData, AssociativeObject};
    let mut scene = opened();
    assert!(scene
        .set_bricscad_visibility(reference(0xEE), "LABEL", "Hidden")
        .unwrap());
    let document = &scene.document;
    let copy = document
        .block_records
        .get(&block_of(document, 0xEE))
        .unwrap()
        .handle;
    // The copy's network sits in its extension dictionary, behind a
    // dictionary of the same name.
    let entry = |dictionary: Handle| match document.objects.get(&dictionary) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary.get("ACAD_ASSOCNETWORK"),
        _ => None,
    };
    let mut network = entry(document.extension_dictionary_handle(copy).unwrap()).unwrap();
    while let Some(inner) = entry(network) {
        network = inner;
    }
    let mut reads = 0;
    for object in document.objects.values() {
        let ObjectType::Associative(AssociativeObject {
            data: AssociativeData::ValueDependency(read),
            ..
        }) = object
        else {
            continue;
        };
        let Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            owner,
            ..
        })) = document.objects.get(&read.dependency.dependent_on)
        else {
            panic!("a read of something other than a variable");
        };
        assert_eq!(read.value, variable.value, "{}", variable.name);
        reads += usize::from(*owner == network);
    }
    assert!(reads > 0, "the copy's variables are read");
}
