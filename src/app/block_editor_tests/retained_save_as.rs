use super::*;
use crate::scene::named_parameters::DrivingValue;
use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
use codec::objects::{
    AssocConstraintNode, AssocConstraintNodeData as Node, AssociativeData as Data,
    AssociativeObject, ObjectType,
};

fn retained_fixture() -> (OpenCADStudio, Handle, Handle, Handle) {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let line = members(&app)[0];
    let mut dimension = EntityType::Dimension(Box::new(codec::entities::Dimension::Aligned(
        codec::entities::DimensionAligned::new(Vector3::ZERO, Vector3::new(10., 0., 0.)),
    )));
    dimension.common_mut().owner_handle = owner;
    let dimension = app.tabs[i].scene.document.add_entity(dimension).unwrap();
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner));
    set.local_parameters.set("base", "5").unwrap();
    set.local_parameters.set("width", "base*2").unwrap();
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(line)],
        None,
    );
    let distance = set.add(
        ConstraintKind::Distance,
        vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
        Some(DrivingValue::Named("width".into())),
    );
    set.dimensions.insert(distance, dimension);
    app.tabs[i].scene.sync_native_parametric_graph();
    let variables: std::collections::HashMap<_, _> = app.tabs[i]
        .scene
        .document
        .objects
        .iter()
        .filter_map(|(handle, object)| {
            if let ObjectType::Associative(AssociativeObject {
                data: Data::Variable(value),
                ..
            }) = object
            {
                Some((value.name.clone(), *handle))
            } else {
                None
            }
        })
        .collect();
    let width = variables["width"];
    let base = variables["base"];
    let dependency = app.tabs[i].scene.document.allocate_handle();
    let mut object = AssociativeObject::new("ACDBASSOCVALUEDEPENDENCY", "AcDbAssocValueDependency");
    object.handle = dependency;
    object.owner = width;
    object.data = Data::ValueDependency(codec::objects::AssocValueDependency {
        dependency: codec::objects::AssocDependency {
            class_version: 2,
            is_read_dependency: true,
            is_attached_to_object: true,
            dependent_on: base,
            ..Default::default()
        },
        name: "base".into(),
        value: codec::objects::AssocEvalVariant {
            code: 40,
            value: codec::objects::AssocEvalValue::Real(5.),
        },
        ..Default::default()
    });
    app.tabs[i]
        .scene
        .document
        .objects
        .insert(dependency, ObjectType::Associative(object));
    let ObjectType::Associative(AssociativeObject {
        data: Data::Variable(value),
        ..
    }) = app.tabs[i].scene.document.objects.get_mut(&width).unwrap()
    else {
        panic!()
    };
    value
        .dependencies
        .push(codec::objects::AssocVariableDependency {
            dependency,
            flags: 0,
        });
    value.action.max_dependency_index = 1;
    let group_handle = app.tabs[i]
        .scene
        .document
        .objects
        .iter()
        .find_map(|(handle, object)| {
            matches!(
                object,
                ObjectType::Associative(AssociativeObject {
                    data: Data::ConstraintGroup(_),
                    ..
                })
            )
            .then_some(*handle)
        })
        .unwrap();
    let ObjectType::Associative(AssociativeObject {
        data: Data::ConstraintGroup(group),
        ..
    }) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&group_handle)
        .unwrap()
    else {
        panic!()
    };
    let geometry = group
        .nodes
        .iter_mut()
        .find(|node| matches!(node.data, Node::BoundedLine { .. }))
        .unwrap();
    let geometry_id = geometry.node_id;
    if let Node::BoundedLine {
        geometry_node_id, ..
    } = &mut geometry.data
    {
        *geometry_node_id = 99;
    }
    group.nodes.push(AssocConstraintNode {
        node_id: 99,
        class_name: "AcConstrainedRigidSet".into(),
        data: Node::RigidSet {
            geometry_dependency: Handle::NULL,
            geometry_node_id: 0,
            reserved: false,
            transform: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            geometry_ids: vec![geometry_id],
        },
        ..Default::default()
    });
    app.tabs[i]
        .scene
        .load_parametric_constraints_from_document();
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(owner))
        .unwrap();
    assert_eq!(set.retained_standard_groups, vec![group_handle]);
    assert!(set.constraints.iter().any(|c| c.native_origin.is_some()));
    assert!(set.constraints.iter().any(|c| !c.rigid_points.is_empty()));
    (app, reference, owner, group_handle)
}

fn assert_scope_graph(document: &codec::Document, owner: Handle, group: Handle) {
    let ObjectType::Associative(AssociativeObject {
        owner: scope,
        data: Data::ConstraintGroup(value),
        ..
    }) = &document.objects[&group]
    else {
        panic!()
    };
    assert_eq!(value.action.owning_network, *scope);
    assert!(value
        .nodes
        .iter()
        .any(|n| matches!(n.data, Node::RigidSet { .. })));
    assert!(value
        .nodes
        .iter()
        .any(|n| matches!(n.data, Node::Distance { .. })));
    let ObjectType::Associative(AssociativeObject {
        data: Data::Network(network),
        ..
    }) = &document.objects[scope]
    else {
        panic!()
    };
    assert!(network.actions.iter().any(|a| a.dependency == group));
    let width = network.actions.iter().find_map(|a| match &document.objects[&a.dependency] {
        ObjectType::Associative(object) if matches!(&object.data, Data::Variable(value) if value.name == "width") => Some(object),
        _ => None,
    }).unwrap();
    let Data::Variable(width_value) = &width.data else {
        panic!()
    };
    assert_eq!(width_value.expression, "base*2");
    assert_eq!(width_value.dependencies.len(), 1);
    let ObjectType::Associative(dependency) =
        &document.objects[&width_value.dependencies[0].dependency]
    else {
        panic!()
    };
    assert_eq!(dependency.owner, width.handle);
    let Data::ValueDependency(dependency) = &dependency.data else {
        panic!()
    };
    let ObjectType::Associative(base) = &document.objects[&dependency.dependency.dependent_on]
    else {
        panic!()
    };
    assert_eq!(base.owner, *scope);
    assert!(
        matches!(&base.data, Data::Variable(value) if value.name == "base" && value.action.owning_network == *scope)
    );
    let ObjectType::Associative(AssociativeObject {
        data: Data::Network(root),
        ..
    }) = &document.objects[&network.action.owning_network]
    else {
        panic!()
    };
    assert!(root
        .actions
        .iter()
        .any(|a| a.dependency == *scope && !a.is_owned));
    for handle in &value.actions {
        let ObjectType::Associative(object) = &document.objects[handle] else {
            panic!()
        };
        assert_eq!(object.owner, group);
        if let Data::GeomDependency(dependency) = &object.data {
            assert_eq!(
                document
                    .get_entity(dependency.dependency.dependent_on)
                    .unwrap()
                    .common()
                    .owner_handle,
                owner
            );
        }
    }
}

#[test]
fn save_as_retained_constraints_clone_connections_runtime_and_history() {
    let (mut app, reference, owner, group) = retained_fixture();
    let i = app.active_tab;
    let source_group = app.tabs[i].scene.document.objects[&group].clone();
    let source_members = members(&app);
    begin(&mut app, reference);
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner));
    set.constraints
        .iter_mut()
        .find(|c| c.kind == ConstraintKind::Horizontal)
        .unwrap()
        .enabled = false;
    set.local_parameters.set("base", "6").unwrap();
    assert!(
        app.save_block_edit_as(i, "COPY"),
        "{:?}",
        app.command_line.last_error
    );
    let copied = app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .unwrap()
        .handle;
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(copied))
        .unwrap();
    let copied_group = set.retained_standard_groups[0];
    assert_eq!(set.dimensions.len(), 1);
    assert!(set.dimensions.values().all(|handle| app.tabs[i]
        .scene
        .document
        .get_entity(*handle)
        .unwrap()
        .common()
        .owner_handle
        == copied));
    assert_ne!(copied_group, group);
    assert_eq!(set.local_parameters.resolve("width").unwrap(), 12.);
    assert!(set
        .constraints
        .iter()
        .filter_map(|c| c.native_origin)
        .all(|o| o.group == copied_group));
    assert!(set
        .constraints
        .iter()
        .flat_map(|c| c.refs.iter().chain(c.rigid_points.iter().map(|(r, _)| r)))
        .all(|r| !source_members.contains(&r.entity)));
    assert!(set.dof.is_none());
    assert!(set.conflicts.is_empty());
    let source = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(owner))
        .unwrap();
    assert_eq!(source.local_parameters.resolve("width").unwrap(), 10.);
    assert!(
        source
            .constraints
            .iter()
            .find(|c| c.kind == ConstraintKind::Horizontal)
            .unwrap()
            .enabled
    );
    assert_eq!(app.tabs[i].scene.document.objects[&group], source_group);
    assert_scope_graph(&app.tabs[i].scene.document, copied, copied_group);
    app.close_block_edit(i, true);
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .is_none());
    assert!(!app.tabs[i]
        .scene
        .document
        .objects
        .contains_key(&copied_group));
    assert!(app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(copied))
        .is_none());
    app.redo_steps(1);
    assert_scope_graph(&app.tabs[i].scene.document, copied, copied_group);
    app.tabs[i].scene.sync_native_parametric_graph();
    assert_scope_graph(&app.tabs[i].scene.document, copied, copied_group);
    for ext in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(
            &app.tabs[i].scene.document,
            ext,
            app.tabs[i].scene.document.version,
        )
        .unwrap();
        let document = crate::io::load_bytes(&format!("retained-copy.{ext}"), bytes).unwrap();
        assert_scope_graph(&document, owner, group);
        assert_scope_graph(&document, copied, copied_group);
        let mut scene = crate::scene::Scene::new();
        scene.document = document;
        scene.load_parametric_constraints_from_document();
        let copied_set = scene
            .parametric_constraint_set(ParametricScope::Block(copied))
            .unwrap();
        assert_eq!(copied_set.local_parameters.resolve("width").unwrap(), 12.);
        assert_eq!(copied_set.dimensions.len(), 1);
        assert!(copied_set.dimensions.values().all(|handle| scene
            .document
            .get_entity(*handle)
            .unwrap()
            .common()
            .owner_handle
            == copied));
        assert!(
            !copied_set
                .constraints
                .iter()
                .find(|c| c.kind == ConstraintKind::Horizontal)
                .unwrap()
                .enabled
        );
        assert!(
            scene
                .parametric_constraint_set(ParametricScope::Block(owner))
                .unwrap()
                .constraints
                .iter()
                .find(|c| c.kind == ConstraintKind::Horizontal)
                .unwrap()
                .enabled
        );
    }
}

#[test]
fn save_as_retained_constraints_reject_foreign_dependencies_atomically() {
    let (mut app, reference, _, group) = retained_fixture();
    let i = app.active_tab;
    let dependency = match &app.tabs[i].scene.document.objects[&group] {
        ObjectType::Associative(AssociativeObject {
            data: Data::ConstraintGroup(value),
            ..
        }) => value
            .actions
            .iter()
            .copied()
            .find(|h| {
                matches!(
                    &app.tabs[i].scene.document.objects[h],
                    ObjectType::Associative(AssociativeObject {
                        data: Data::GeomDependency(_),
                        ..
                    })
                )
            })
            .unwrap(),
        _ => panic!(),
    };
    let ObjectType::Associative(AssociativeObject {
        data: Data::GeomDependency(value),
        ..
    }) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&dependency)
        .unwrap()
    else {
        panic!()
    };
    value.dependency.dependent_on = reference;
    begin(&mut app, reference);
    circle(&mut app, 2.);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    assert!(!app.save_block_edit_as(i, "COPY"));
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert!(app
        .command_line
        .last_error
        .as_ref()
        .unwrap()
        .contains("outside the copied definition"));
}

#[test]
fn save_as_retained_constraints_create_a_destination_root_network() {
    let (app, _, _, group) = retained_fixture();
    let source = &app.tabs[app.active_tab].scene.document;
    let copied = crate::app::block_save_as::copy_definition(
        source,
        codec::Document::new(),
        "EDIT_ME",
        "COPY",
    )
    .unwrap();
    assert_scope_graph(&copied.document, copied.record, copied.handles[&group]);
    assert!(!copied.document.objects.contains_key(&group));
}

#[test]
fn save_as_local_parameter_formulas_roundtrip_without_constraints() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner));
    set.local_parameters.set("base", "5").unwrap();
    set.local_parameters.set("width", "base*2").unwrap();
    set.local_parameters
        .set_description("base", "Source width factor");
    app.tabs[i].scene.sync_native_parametric_graph();
    app.tabs[i]
        .scene
        .load_parametric_constraints_from_document();
    assert_eq!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(ParametricScope::Block(owner))
            .unwrap()
            .local_parameters
            .resolve("width")
            .unwrap(),
        10.
    );
    begin(&mut app, reference);
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner))
        .local_parameters
        .set("base", "6")
        .unwrap();
    assert!(app.save_block_edit_as(i, "COPY"));
    let copied = app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .unwrap()
        .handle;
    app.close_block_edit(i, true);
    app.tabs[i].scene.sync_native_parametric_graph();
    for ext in ["dwg", "dxf"] {
        let doc = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
        let mut scene = crate::scene::Scene::new();
        scene.document = crate::io::load_bytes(&format!("parameter-copy.{ext}"), bytes).unwrap();
        scene.load_parametric_constraints_from_document();
        for (owner, width) in [(owner, 10.), (copied, 12.)] {
            let set = scene
                .parametric_constraint_set(ParametricScope::Block(owner))
                .unwrap();
            assert!(set.constraints.is_empty());
            assert_eq!(set.local_parameters.resolve("width").unwrap(), width);
            assert_eq!(
                set.local_parameters.description("base"),
                "Source width factor"
            );
        }
    }
}
