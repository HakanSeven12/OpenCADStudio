//! VISIBILITYSTATES against BricsCAD: the drawings in
//! `tests/fixtures/bricscad/authoring-*.dwg` are what BricsCAD V26 saved
//! along one run of the command on five lines (see the README there). The
//! same run here leaves the same visibility setup.

use super::author::{Action, Naming};
use super::record::Value;
use super::*;
use crate::scene::Scene;
use document::objects::{AssociativeData, AssociativeObject};

pub(crate) fn fixture(name: &str) -> Scene {
    let path = format!(
        "{}/tests/fixtures/bricscad/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("read {path}"));
    let mut scene = Scene::new();
    scene.document = crate::io::load_bytes_finalized(std::path::Path::new(name), bytes)
        .unwrap_or_else(|error| panic!("open {name}: {error}"));
    scene.load_named_parameters_from_document();
    scene.load_parametric_constraints_from_document();
    scene
}

/// The five lines, L1 to L5, by their handles in the fixtures.
pub(crate) const LINES: [u64; 5] = [0x92, 0x93, 0x94, 0x95, 0x96];

fn line(n: usize) -> Handle {
    Handle::from(LINES[n - 1])
}

/// One step of the run: parameter, state, action, lines by number.
pub(super) type Step = (&'static str, &'static str, Action, &'static [usize]);

/// The run up to the drawing BricsCAD saved as `authoring-select.dwg`.
pub(super) const TO_SELECT: &[Step] = &[
    ("P1", "S1", Action::Add, &[1, 2]),
    ("P1", "S2", Action::Invisible, &[1]),
    ("P1", "S3", Action::Invisible, &[2]),
    ("P1", "S2", Action::Visible, &[1]),
    ("P1", "S1", Action::Add, &[3]),
    ("P1", "S1", Action::Remove, &[3]),
    ("P2", "Sichtbar", Action::Add, &[4]),
    ("P2", "Unsichtbar", Action::Invisible, &[4]),
    ("P2", "Sichtbar", Action::Add, &[2]),
    ("P1", "S4", Action::Visible, &[1]),
    ("P1", "S2", Action::Select, &[]),
    ("P1", "S3", Action::Select, &[]),
];

/// From the drawing after `S4` (before the two selections) on to the one
/// BricsCAD saved as `authoring-edit.dwg`.
pub(super) const TO_EDIT: &[Step] = &[
    ("P1", "S1", Action::Invisible, &[5]),
    ("P1", "S2", Action::Visible, &[4]),
    ("P1", "S1", Action::Remove, &[1]),
    ("P1", "S3", Action::Add, &[2]),
    ("P1", "S2", Action::Add, &[3]),
];

pub(super) fn run(scene: &mut Scene, steps: &[Step]) {
    let model = scene.document.header.model_space_block_handle;
    for (parameter, state, action, lines) in steps {
        let objects: Vec<Handle> = lines.iter().map(|n| line(*n)).collect();
        scene
            .edit_bricscad_visibility_named(
                model,
                parameter,
                state,
                *action,
                &objects,
                Naming::GERMAN,
            )
            .unwrap_or_else(|error| panic!("{parameter} {state} {action:?} {lines:?}: {error}"));
    }
}

/// The visibility setup of model space as text, without handles: parameters
/// with their tables and current states, suppressions with their members,
/// state and kept layers, the variables with their values, and each line's
/// layer and object.
pub(crate) fn describe(document: &Document) -> Vec<String> {
    let model = document.header.model_space_block_handle;
    let mut out = Vec::new();
    let variable = |handle: Handle| match document.objects.get(&handle) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            ..
        })) => Some(variable),
        _ => None,
    };
    let number = |value: &document::objects::AssocEvalVariant| match &value.value {
        document::objects::AssocEvalValue::Real(v) => format!("{v}"),
        document::objects::AssocEvalValue::Long(v) => format!("{v}"),
        other => format!("{other:?}"),
    };
    let layer = |handle: Handle| {
        document
            .layers
            .iter()
            .find(|layer| layer.handle == handle)
            .map_or_else(|| format!("{handle:?}"), |layer| layer.name.clone())
    };
    let objects = |properties: &record::Properties, name: &str| -> Vec<Handle> {
        properties
            .get(name)
            .and_then(Value::items)
            .map(|items| items.iter().filter_map(Value::object).collect())
            .unwrap_or_default()
    };
    let mut groups_of: std::collections::BTreeMap<u64, String> = Default::default();
    for feature in features(document, model) {
        let properties = super::properties(document, feature).expect("a feature decodes");
        match properties.text("Feature_type") {
            Some("BdmConfigurationFeature") => {
                let table = super::properties(
                    document,
                    properties.get("ConfigTable").unwrap().object().unwrap(),
                )
                .unwrap();
                out.push(format!(
                    "parameter {} shows {} table {:?}",
                    properties.text("Name").unwrap(),
                    properties.text("LastKeyValue").unwrap(),
                    table.0
                ));
                for reactor in objects(&properties, "Reactors") {
                    let reactor = super::properties(document, reactor).unwrap();
                    let key = reactor.get("IsKeyVar") == Some(&Value::Flag(true));
                    let var = variable(reactor.get("VarId").unwrap().object().unwrap()).unwrap();
                    out.push(format!(
                        "  reactor {} key {key} enabled {:?} string {:?} value {} -> {} {:?} {}",
                        reactor.text("VarName").unwrap(),
                        reactor.get("IsEnabled").unwrap(),
                        reactor.text("String").unwrap(),
                        // The key variable's number follows no rule BricsCAD
                        // keeps; it reads the state from the string.
                        if key {
                            "-".into()
                        } else {
                            format!("{:?}", reactor.get("Value").unwrap())
                        },
                        var.name,
                        var.description,
                        if key { "-".into() } else { number(&var.value) },
                    ));
                }
            }
            Some("BdmSuppressFeature") => {
                let name = properties.text("Name").unwrap().to_string();
                let mut line = format!(
                    "suppression {name} status {:?} kept {:?}",
                    properties.get("Status").unwrap(),
                    properties
                        .get("aEntLayerKey")
                        .and_then(Value::entries)
                        .unwrap()
                        .iter()
                        .map(|(entity, kept)| format!(
                            "{:X}->{}",
                            entity.object().unwrap().value(),
                            layer(kept.object().unwrap())
                        ))
                        .collect::<Vec<_>>()
                );
                for reactor in objects(&properties, "Reactors") {
                    let reactor_properties = super::properties(document, reactor).unwrap();
                    if reactor_properties.get("pers_ids").is_some() {
                        let mut members: Vec<String> = objects(&reactor_properties, "pers_ids")
                            .into_iter()
                            .map(
                                |dependency| match record(document, dependency).unwrap().body {
                                    record::Body::Dependency { entity, .. } => {
                                        groups_of.insert(entity.handle.value(), name.clone());
                                        format!("{:X}", entity.handle.value())
                                    }
                                    other => panic!("{other:?}"),
                                },
                            )
                            .collect();
                        members.sort();
                        line.push_str(&format!(" members {members:?}"));
                    } else {
                        let var =
                            variable(reactor_properties.get("var").unwrap().object().unwrap())
                                .unwrap();
                        let mut reads: Vec<String> = var
                            .dependencies
                            .iter()
                            .map(
                                |dependency| match document.objects.get(&dependency.dependency) {
                                    Some(ObjectType::Associative(AssociativeObject {
                                        data: AssociativeData::ValueDependency(value),
                                        ..
                                    })) => variable(value.dependency.dependent_on)
                                        .unwrap()
                                        .name
                                        .clone(),
                                    other => panic!("{other:?}"),
                                },
                            )
                            .collect();
                        reads.sort();
                        line.push_str(&format!(
                            " value {:?} variable {} = {} [{}] reads {reads:?}",
                            reactor_properties.get("value").unwrap(),
                            var.name,
                            var.expression,
                            number(&var.value)
                        ));
                    }
                }
                out.push(line);
            }
            _ => {}
        }
    }
    for &handle in &LINES {
        let entity = document.get_entity(Handle::from(handle)).unwrap();
        let objects: Vec<String> = entity
            .common()
            .reactors
            .iter()
            .filter_map(|reactor| match record(document, *reactor)?.body {
                record::Body::Object { properties, .. } => Some(format!(
                    "{} with {} dependencies",
                    properties.text("NAME").unwrap(),
                    properties
                        .get("PersIdDependencies")
                        .unwrap()
                        .items()
                        .unwrap()
                        .len()
                )),
                _ => None,
            })
            .collect();
        out.push(format!(
            "line {handle:X} layer {} in {:?} objects {objects:?}",
            entity.common().layer,
            groups_of.get(&handle)
        ));
    }
    out
}

fn assert_same(ours: &Document, theirs: &Document) {
    let ours = describe(ours);
    let theirs = describe(theirs);
    let differences: Vec<String> = ours
        .iter()
        .zip(&theirs)
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("ours:   {a}\ntheirs: {b}"))
        .collect();
    assert!(
        differences.is_empty() && ours.len() == theirs.len(),
        "{}\n\nours:\n{}\n\ntheirs:\n{}",
        differences.join("\n"),
        ours.join("\n"),
        theirs.join("\n")
    );
}

#[test]
fn the_run_leaves_what_bricscad_leaves_up_to_two_selections() {
    let mut scene = fixture("authoring-start.dwg");
    run(&mut scene, TO_SELECT);
    assert_same(&scene.document, &fixture("authoring-select.dwg").document);
}

#[test]
fn the_run_leaves_what_bricscad_leaves_through_remove_and_re_add() {
    let mut scene = fixture("authoring-start.dwg");
    run(&mut scene, &TO_SELECT[..10]);
    run(&mut scene, TO_EDIT);
    assert_same(&scene.document, &fixture("authoring-edit.dwg").document);
}

/// What BricsCAD keeps that the run's tables do not show: every read of a
/// variable holds the variable's value (BricsCAD does not evaluate past a
/// read that already holds a new value), a variable's reads are numbered,
/// and each key has the number of the state it shows.
#[test]
fn reads_hold_their_values_and_keys_the_number_of_their_state() {
    let mut scene = fixture("authoring-start.dwg");
    run(&mut scene, TO_SELECT);
    run(
        &mut scene,
        &[
            ("P3", "S1", Action::Add, &[2]),
            ("P1", "S1", Action::Select, &[]),
        ],
    );
    let document = &scene.document;
    let variable = |handle: Handle| match document.objects.get(&handle) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            ..
        })) => variable,
        other => panic!("{other:?}"),
    };
    let mut reads = 0;
    let mut most = 0;
    for object in document.objects.values() {
        match object {
            ObjectType::Associative(AssociativeObject {
                data: AssociativeData::ValueDependency(read),
                ..
            }) => {
                let read_variable = variable(read.dependency.dependent_on);
                assert_eq!(read.value, read_variable.value, "{}", read_variable.name);
                reads += 1;
            }
            ObjectType::Associative(AssociativeObject {
                data: AssociativeData::Variable(variable),
                ..
            }) => {
                let flags: Vec<usize> = variable
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.flags as usize)
                    .collect();
                assert_eq!(
                    flags,
                    (0..flags.len()).collect::<Vec<_>>(),
                    "{}",
                    variable.name
                );
                most = most.max(flags.len());
            }
            _ => {}
        }
    }
    assert!(reads > 0);
    assert_eq!(most, 3, "L2 is ruled by three parameters");
    let model = document.header.model_space_block_handle;
    let mut keys = 0;
    for feature in features(document, model) {
        let properties = super::properties(document, feature).unwrap();
        if properties.text("Feature_type") != Some("BdmConfigurationFeature") {
            continue;
        }
        let parameter = visibility_parameter(document, feature).unwrap();
        for reactor in properties.get("Reactors").unwrap().items().unwrap() {
            let reactor = super::properties(document, reactor.object().unwrap()).unwrap();
            if reactor.get("IsKeyVar") != Some(&Value::Flag(true)) {
                continue;
            }
            let row = parameter.current.unwrap() as f64;
            assert_eq!(
                reactor.get("Value"),
                Some(&Value::Real(row)),
                "{}",
                parameter.name
            );
            let key = variable(reactor.get("VarId").unwrap().object().unwrap());
            assert_eq!(
                key.value.value,
                document::objects::AssocEvalValue::Real(row),
                "{}",
                parameter.name
            );
            keys += 1;
        }
    }
    assert_eq!(keys, 3);
}

/// BricsCAD finds variables by name, and a second variable of a name gets
/// the two mixed up. Some of its drawings have a group whose variable is
/// named after another group (`Unterdrücken_1` with the variable
/// `Unterdrücken_7`): a new group takes a name no variable has.
#[test]
fn a_new_group_takes_a_name_no_variable_has() {
    let mut scene = fixture("authoring-select.dwg");
    let model = scene.document.header.model_space_block_handle;
    let group_variable = features(&scene.document, model)
        .into_iter()
        .filter_map(|feature| super::properties(&scene.document, feature))
        .filter(|properties| properties.text("Name") == Some("Unterdrücken_2"))
        .flat_map(|properties| {
            properties
                .get("Reactors")
                .unwrap()
                .items()
                .unwrap()
                .to_vec()
        })
        .filter_map(|reactor| super::properties(&scene.document, reactor.object()?))
        .find_map(|reactor| reactor.get("var")?.object())
        .unwrap();
    match scene.document.objects.get_mut(&group_variable) {
        Some(ObjectType::Associative(AssociativeObject {
            data: AssociativeData::Variable(variable),
            ..
        })) => variable.name = "Unterdrücken_4".into(),
        other => panic!("{other:?}"),
    }
    // As if the drawing had been opened so.
    scene.load_named_parameters_from_document();
    scene.load_parametric_constraints_from_document();
    run(&mut scene, &[("P1", "S1", Action::Add, &[5])]);
    let document = &scene.document;
    let mut names: Vec<String> = document
        .objects
        .values()
        .filter_map(|object| match object {
            ObjectType::Associative(AssociativeObject {
                data: AssociativeData::Variable(variable),
                ..
            }) => Some(variable.name.to_lowercase()),
            _ => None,
        })
        .collect();
    let all = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all, "{names:?}");
    let described = describe(document);
    assert!(
        described
            .iter()
            .any(|line| line.starts_with("suppression Unterdrücken_5 ")
                && line.contains("members [\"96\"]")),
        "{described:#?}"
    );
}

/// Each state of `parameter` by name with the objects it shows.
fn shown_by_state(
    document: &Document,
    parameter: &str,
) -> std::collections::BTreeMap<String, Vec<u64>> {
    let model = document.header.model_space_block_handle;
    let view = super::parameter_views(document, model)
        .unwrap()
        .into_iter()
        .find(|p| p.name == parameter)
        .unwrap();
    view.states
        .iter()
        .enumerate()
        .map(|(s, state)| {
            (
                state.clone(),
                view.objects
                    .iter()
                    .filter(|(_, pattern)| pattern[s])
                    .map(|(entity, _)| entity.value())
                    .collect(),
            )
        })
        .collect()
}

fn states_of(document: &Document, parameter: &str) -> (Vec<String>, usize) {
    let model = document.header.model_space_block_handle;
    let view = super::parameter_views(document, model)
        .unwrap()
        .into_iter()
        .find(|p| p.name == parameter)
        .unwrap();
    (view.states, view.current)
}

/// The number the key variable of `parameter` holds.
fn key_number(document: &Document, parameter: &str) -> Value {
    let model = document.header.model_space_block_handle;
    for feature in features(document, model) {
        let properties = super::properties(document, feature).unwrap();
        if properties.text("Name") != Some(parameter) {
            continue;
        }
        for reactor in properties.get("Reactors").unwrap().items().unwrap() {
            let reactor = super::properties(document, reactor.object().unwrap()).unwrap();
            if reactor.get("IsKeyVar") == Some(&Value::Flag(true)) {
                return reactor.get("Value").unwrap().clone();
            }
        }
    }
    panic!("no key of {parameter}");
}

#[test]
fn a_state_moves_with_its_objects_and_the_shown_state_stays_shown() {
    let mut scene = fixture("authoring-select.dwg");
    let before = shown_by_state(&scene.document, "P1");
    assert_eq!(
        states_of(&scene.document, "P1"),
        (vec!["S1".into(), "S2".into(), "S3".into(), "S4".into()], 2)
    );
    let layers = |document: &Document| -> Vec<String> {
        LINES
            .iter()
            .map(|h| {
                document
                    .get_entity(Handle::from(*h))
                    .unwrap()
                    .common()
                    .layer
                    .clone()
            })
            .collect()
    };
    let shown = layers(&scene.document);
    run(&mut scene, &[("P1", "S3", Action::MoveUp, &[])]);
    assert_eq!(
        states_of(&scene.document, "P1"),
        (vec!["S1".into(), "S3".into(), "S2".into(), "S4".into()], 1)
    );
    assert_eq!(shown_by_state(&scene.document, "P1"), before);
    assert_eq!(layers(&scene.document), shown, "the drawing shows the same");
    assert_eq!(key_number(&scene.document, "P1"), Value::Real(1.0));
    run(&mut scene, &[("P1", "S1", Action::MoveDown, &[])]);
    assert_eq!(
        states_of(&scene.document, "P1"),
        (vec!["S3".into(), "S1".into(), "S2".into(), "S4".into()], 0)
    );
    assert_eq!(shown_by_state(&scene.document, "P1"), before);
    // Past either end nothing moves.
    let model = scene.document.header.model_space_block_handle;
    let unchanged = scene.document.clone();
    for (state, action) in [("S3", Action::MoveUp), ("S4", Action::MoveDown)] {
        assert!(scene
            .edit_bricscad_visibility_named(model, "P1", state, action, &[], Naming::GERMAN)
            .is_err());
    }
    assert!(scene.document == unchanged);
}

#[test]
fn a_new_state_hides_every_object_of_its_parameter_and_is_shown() {
    let mut scene = fixture("authoring-select.dwg");
    run(&mut scene, &[("P1", "Leer", Action::NewState, &[])]);
    let (states, current) = states_of(&scene.document, "P1");
    assert_eq!(states.last().map(String::as_str), Some("Leer"));
    assert_eq!(current, states.len() - 1);
    assert_eq!(
        shown_by_state(&scene.document, "P1")["Leer"],
        Vec::<u64>::new()
    );
    // L1 and L2 are ruled by P1 (L2 also by P2): both hidden now.
    for handle in [0x92, 0x93] {
        assert_eq!(
            scene
                .document
                .get_entity(Handle::from(handle))
                .unwrap()
                .common()
                .layer,
            "*SUPPRESS"
        );
    }
    let model = scene.document.header.model_space_block_handle;
    assert!(scene
        .edit_bricscad_visibility_named(model, "P1", "leer", Action::NewState, &[], Naming::GERMAN)
        .is_err());
    assert!(scene
        .edit_bricscad_visibility_named(model, "P9", "S1", Action::NewState, &[], Naming::GERMAN)
        .is_err());
}

/// The key of `parameter`: whether enabled, its number, its variable's
/// expression and value.
fn key_of(
    document: &Document,
    parameter: &str,
) -> (Value, Value, String, document::objects::AssocEvalValue) {
    let model = document.header.model_space_block_handle;
    for feature in features(document, model) {
        let properties = super::properties(document, feature).unwrap();
        if properties.text("Name") != Some(parameter) {
            continue;
        }
        for reactor in properties.get("Reactors").unwrap().items().unwrap() {
            let reactor = super::properties(document, reactor.object().unwrap()).unwrap();
            if reactor.get("IsKeyVar") != Some(&Value::Flag(true)) {
                continue;
            }
            let handle = reactor.get("VarId").unwrap().object().unwrap();
            let Some(ObjectType::Associative(AssociativeObject {
                data: AssociativeData::Variable(variable),
                ..
            })) = document.objects.get(&handle)
            else {
                panic!()
            };
            return (
                reactor.get("IsEnabled").unwrap().clone(),
                reactor.get("Value").unwrap().clone(),
                variable.expression.clone(),
                variable.value.value.clone(),
            );
        }
    }
    panic!("no key of {parameter}");
}

/// openCirt's templates have keys BricsCAD left disabled over a variable
/// with an expression of its own (a key showing the second state over `0`).
/// BricsCAD reads such a state by name and leaves the key alone until the
/// parameter's state is picked; then it enables the key and drops the
/// expression. A key re-enabled under its expression would make BricsCAD
/// show the first state on its next evaluation.
#[test]
fn a_disabled_key_is_left_alone_until_its_state_is_picked() {
    use document::objects::{AssocEvalValue, AssocEvalVariant};
    let mut scene = fixture("authoring-select.dwg");
    let model = scene.document.header.model_space_block_handle;
    // P2 shows Sichtbar (row 0); make its key like the templates' ones.
    for feature in features(&scene.document, model) {
        let properties = super::properties(&scene.document, feature).unwrap();
        if properties.text("Name") != Some("P2") {
            continue;
        }
        for reactor in properties.get("Reactors").unwrap().items().unwrap() {
            let handle = reactor.object().unwrap();
            let mut record = record(&scene.document, handle).unwrap();
            let reactor = record.body.properties_mut();
            if reactor.get("IsKeyVar") != Some(&Value::Flag(true)) {
                continue;
            }
            reactor.set("IsEnabled", Value::Flag(false));
            reactor.set("Value", Value::Real(1.0));
            let variable = reactor.get("VarId").unwrap().object().unwrap();
            super::switch::store(&mut scene.document, &record);
            super::switch::set_variable(
                &mut scene.document,
                variable,
                Some("1".into()),
                AssocEvalVariant {
                    code: 90,
                    value: AssocEvalValue::Long(1),
                },
            )
            .unwrap();
        }
    }
    scene.load_named_parameters_from_document();
    let before = key_of(&scene.document, "P2");
    run(
        &mut scene,
        &[
            ("P1", "S1", Action::Select, &[]),
            ("P1", "S4", Action::MoveUp, &[]),
            ("P1", "Neu", Action::NewState, &[]),
            ("P1", "S2", Action::Visible, &[5]),
        ],
    );
    assert_eq!(
        key_of(&scene.document, "P2"),
        before,
        "P2 was not asked about"
    );
    assert_eq!(
        states_of(&scene.document, "P2").1,
        0,
        "still shows Sichtbar"
    );
    run(&mut scene, &[("P2", "Unsichtbar", Action::Select, &[])]);
    assert_eq!(
        key_of(&scene.document, "P2"),
        (
            Value::Flag(true),
            Value::Real(1.0),
            String::new(),
            AssocEvalValue::Real(1.0)
        )
    );
}
