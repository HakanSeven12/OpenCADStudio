//! A network that objects kept only as raw records depend on is written back
//! with its handles while the editor has not changed its scope.
//!
//! BricsCAD keeps the visibility states of its parametric blocks in objects of
//! its own classes. They are read here as raw records and reach the block's
//! associative variables by handle, so a rebuild under new handles leaves them
//! pointing at erased variables and BricsCAD drops the block's parameters.

use super::*;
use document::entities::Line;

/// The objects `bind_raw_network` adds.
struct Bound {
    network: Handle,
    variable: Handle,
    raw: Handle,
}

/// Give `owner` a network with one variable, `look_1` = 1, that lists a raw
/// object as its reactor, the way BricsCAD's variables list the objects that
/// read them.
fn bind_raw_network(document: &mut Document, owner: Handle) -> Bound {
    let dictionary = document.ensure_extension_dictionary(owner);
    let [network, variable, raw] = [(); 3].map(|_| document.allocate_handle());
    document.objects.insert(
        raw,
        ObjectType::Unknown {
            type_name: "BDMDBCONFIGVARREACTOR".to_string(),
            handle: raw,
            owner: dictionary,
            raw_dxf_codes: None,
            raw_dwg_data: Some(vec![0; 8]),
            raw_dwg_handle_bits: 0,
            raw_dwg_version: None,
        },
    );
    document.objects.insert(
        network,
        ObjectType::Associative(AssociativeObject {
            handle: network,
            owner: dictionary,
            reactors: vec![dictionary],
            dxf_name: "ACDBASSOCNETWORK".to_string(),
            cpp_class_name: "AcDbAssocNetwork".to_string(),
            data: AssociativeData::Network(AssocNetwork {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                network_action_index: 1,
                actions: vec![AssocActionDependency {
                    is_owned: true,
                    dependency: variable,
                }],
                ..Default::default()
            }),
            ..Default::default()
        }),
    );
    document.objects.insert(
        variable,
        ObjectType::Associative(AssociativeObject {
            handle: variable,
            owner: network,
            reactors: vec![raw],
            dxf_name: "ACDBASSOCVARIABLE".to_string(),
            cpp_class_name: "AcDbAssocVariable".to_string(),
            data: AssociativeData::Variable(AssocVariable {
                action: AssocAction {
                    class_version: 2,
                    owning_network: network,
                    action_index: 1,
                    ..Default::default()
                },
                class_version: 2,
                name: "look_1".to_string(),
                evaluator: "AcDbCalc:1.0".to_string(),
                value: AssocEvalVariant {
                    code: 40,
                    value: AssocEvalValue::Real(1.0),
                },
                ..Default::default()
            }),
            ..Default::default()
        }),
    );
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.add_entry(NETWORK_DICTIONARY_KEY, network);
    }
    Bound {
        network,
        variable,
        raw,
    }
}

/// A scene with one block definition holding a line.
fn block_scene() -> (Scene, Handle) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "states",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let block = scene.document.block_records.get("states").unwrap().handle;
    (scene, block)
}

/// Read the drawing's parameters and constraints as opening a file does.
fn read_as_opened(scene: &mut Scene) {
    scene.load_named_parameters_from_document();
    scene.load_parametric_constraints_from_document();
}

fn variable_value(document: &Document, network: Handle, name: &str) -> Option<f64> {
    document.objects.values().find_map(|object| match object {
        ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            ..
        }) if variable.action.owning_network == network && variable.name == name => {
            numeric_value(&variable.value)
        }
        _ => None,
    })
}

#[test]
fn a_block_network_raw_objects_depend_on_keeps_its_handles() {
    let (mut scene, block) = block_scene();
    let bound = bind_raw_network(&mut scene.document, block);
    read_as_opened(&mut scene);
    let network = scene.document.objects.get(&bound.network).cloned();
    let variable = scene.document.objects.get(&bound.variable).cloned();

    scene.sync_native_parametric_graph();

    assert_eq!(
        native_scope_network_handle(&scene.document, block),
        Some(bound.network),
        "the block keeps its network"
    );
    assert_eq!(scene.document.objects.get(&bound.network).cloned(), network);
    assert_eq!(
        scene.document.objects.get(&bound.variable).cloned(),
        variable,
        "the variable the raw object reads is written back as it was read"
    );
    assert!(matches!(
        scene.document.objects.get(&bound.raw),
        Some(ObjectType::Unknown { .. })
    ));
}

#[test]
fn a_model_space_network_raw_objects_depend_on_keeps_its_handles() {
    // A drawing that is itself a BricsCAD parametric block keeps its
    // parameters in the model-space network, which is also where the
    // drawing's own parameter table is read from.
    let mut scene = Scene::new();
    let model = scene.document.header.model_space_block_handle;
    let bound = bind_raw_network(&mut scene.document, model);
    read_as_opened(&mut scene);
    assert!(
        scene.named_parameters.get("look_1").is_some(),
        "opening reads the variable into the drawing's parameters"
    );
    let variable = scene.document.objects.get(&bound.variable).cloned();

    scene.sync_native_parametric_graph();

    assert_eq!(
        native_scope_network_handle(&scene.document, model),
        Some(bound.network)
    );
    assert_eq!(
        scene.document.objects.get(&bound.variable).cloned(),
        variable
    );
}

#[test]
fn a_parameter_changed_in_such_a_scope_is_still_written() {
    let (mut scene, block) = block_scene();
    let bound = bind_raw_network(&mut scene.document, block);
    read_as_opened(&mut scene);
    scene
        .parametric_constraint_set_mut(ParametricScope::Block(block))
        .local_parameters
        .set("look_1", "0")
        .unwrap();

    scene.sync_native_parametric_graph();

    let network =
        native_scope_network_handle(&scene.document, block).expect("the block still has a network");
    assert_ne!(network, bound.network, "a changed scope is rebuilt");
    assert_eq!(
        variable_value(&scene.document, network, "look_1"),
        Some(0.0)
    );
}

#[test]
fn a_changed_drawing_parameter_rebuilds_the_model_space_network() {
    let mut scene = Scene::new();
    let model = scene.document.header.model_space_block_handle;
    let bound = bind_raw_network(&mut scene.document, model);
    read_as_opened(&mut scene);
    scene.named_parameters.set("look_1", "0").unwrap();

    scene.sync_native_parametric_graph();

    let network = native_scope_network_handle(&scene.document, model)
        .expect("model space still has a network");
    assert_ne!(network, bound.network, "a changed scope is rebuilt");
    assert_eq!(
        variable_value(&scene.document, network, "look_1"),
        Some(0.0)
    );
}

#[test]
fn a_bricscad_parametric_block_keeps_the_variables_its_objects_read_through_a_dwg_save() {
    // tests/fixtures/bricscad/README.md describes the drawing.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bricscad/visibility-states.dwg"
    );
    let bytes = std::fs::read(path).expect("read the fixture");
    let mut scene = Scene::new();
    scene.document =
        crate::io::load_bytes_finalized(std::path::Path::new("visibility-states.dwg"), bytes)
            .expect("open the fixture");
    read_as_opened(&mut scene);

    // Each variable with its name, network and the objects listed as its
    // reactors: BricsCAD's objects, and the value dependency of a variable
    // another one's expression reads.
    let variables = |document: &Document| {
        let mut rows: Vec<(Handle, String, Handle, Vec<Handle>)> = document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Associative(AssociativeObject {
                    handle,
                    reactors,
                    data: AssociativeData::Variable(variable),
                    ..
                }) => Some((
                    *handle,
                    variable.name.clone(),
                    variable.action.owning_network,
                    reactors.clone(),
                )),
                _ => None,
            })
            .collect();
        rows.sort_by_key(|row| row.0);
        rows
    };
    let value_dependencies = |document: &Document| {
        document
            .objects
            .values()
            .filter(|object| {
                matches!(
                    object,
                    ObjectType::Associative(AssociativeObject {
                        data: AssociativeData::ValueDependency(_),
                        ..
                    })
                )
            })
            .count()
    };
    let before = variables(&scene.document);
    assert_eq!(
        before.len(),
        18,
        "six variables in each of the three blocks"
    );
    assert_eq!(value_dependencies(&scene.document), 6);

    scene.sync_native_parametric_graph();
    let saved = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
        .expect("save as DWG");
    let reopened = crate::io::load_bytes_finalized(std::path::Path::new("saved.dwg"), saved)
        .expect("open the saved DWG");

    assert_eq!(variables(&reopened), before);
    assert_eq!(value_dependencies(&reopened), 6);
    for (handle, name, _, reactors) in &before {
        for reactor in reactors {
            assert!(
                reopened.objects.contains_key(reactor),
                "{name} ({handle:?}) lists {reactor:?}, which the saved drawing lost"
            );
        }
    }
}
