use super::super::super::named_parameters::DrivingValue;
use super::*;
use crate::scene::parametric_constraints::{ParametricConstraintSet, ParametricScope};
use codec::entities::{Arc, Circle, Ellipse, Line};

fn fixture() -> (codec::Document, Vec<Handle>, ParametricConstraintSet) {
    let mut doc = codec::Document::new();
    let mut handles = vec![];
    for entity in [
        EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X)),
        EntityType::Line(Line::from_points(
            Vector3::new(3., 0., 0.),
            Vector3::new(4., 0., 0.),
        )),
        EntityType::Circle(Circle::from_center_radius(Vector3::new(8., 0., 0.), 2.)),
    ] {
        handles.push(doc.add_entity(entity).unwrap());
    }
    (
        doc,
        handles,
        ParametricConstraintSet::new(ParametricScope::ModelSpace),
    )
}

#[test]
fn block_constraint_status_distinguishes_independent_entities_without_mutation() {
    let (doc, handles, mut set) = fixture();
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(handles[1])],
        None,
    );
    let before = doc.clone();
    let before_constraints = set.constraints.clone();
    let report = analyze(&doc, &ParameterTable::new(), &set);
    assert_eq!(report[&handles[0]], Status::Full);
    assert_eq!(report[&handles[1]], Status::Partial);
    assert_eq!(
        report
            .get(&handles[2])
            .copied()
            .unwrap_or(Status::Unconstrained),
        Status::Unconstrained
    );
    assert_eq!(doc, before);
    assert_eq!(set.constraints, before_constraints);
    assert_eq!(set.dof, None);
    assert!(set.conflicts.is_empty());
}

#[test]
fn block_constraint_status_respects_coupled_point_freedom_and_disabled_constraints() {
    let (doc, handles, mut set) = fixture();
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::point(handles[0], 1),
            ParametricRef::point(handles[1], 0),
        ],
        None,
    );
    let report = analyze(&doc, &ParameterTable::new(), &set);
    assert_eq!(report[&handles[0]], Status::Full);
    assert_eq!(
        report[&handles[1]],
        Status::Partial,
        "the other endpoint remains free"
    );
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::point(handles[1], 1)],
        None,
    );
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&handles[1]],
        Status::Full
    );
    for constraint in &mut set.constraints {
        constraint.enabled = false;
    }
    assert!(analyze(&doc, &ParameterTable::new(), &set)
        .values()
        .all(|status| *status == Status::Unconstrained));
}

#[test]
fn block_constraint_status_detects_redundancy_and_inconsistent_targets() {
    let (doc, handles, mut set) = fixture();
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&handles[0]],
        Status::Over
    );
    let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
    for radius in [2., 4.] {
        set.add(
            ConstraintKind::Radius,
            vec![ParametricRef::whole(handles[2])],
            Some(DrivingValue::Literal(radius)),
        );
    }
    let before = doc.clone();
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&handles[2]],
        Status::Over
    );
    assert_eq!(doc, before);
}

#[test]
fn block_constraint_status_accounts_for_arc_intrinsic_equations() {
    let (mut doc, _, mut set) = fixture();
    let arc = doc
        .add_entity(EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::ZERO,
            2.,
            0.,
            1.,
        )))
        .unwrap();
    set.add(
        ConstraintKind::Radius,
        vec![ParametricRef::whole(arc)],
        Some(DrivingValue::Literal(2.)),
    );
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&arc],
        Status::Partial
    );
    set.constraints.clear();
    set.add(ConstraintKind::Fixed, vec![ParametricRef::whole(arc)], None);
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&arc],
        Status::Full
    );
}

#[test]
fn block_constraint_status_leaves_unsupported_references_unavailable() {
    let (doc, handles, mut set) = fixture();
    set.add(
        ConstraintKind::Radius,
        vec![ParametricRef::whole(handles[0])],
        Some(DrivingValue::Literal(2.)),
    );
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&handles[0]],
        Status::Unavailable
    );
}

#[test]
fn block_constraint_status_fixed_ellipse_holds_its_focus_and_minor_axis() {
    let (mut doc, _, mut set) = fixture();
    let ellipse = doc
        .add_entity(EntityType::Ellipse(Ellipse::from_center_axes(
            Vector3::ZERO,
            Vector3::new(4., 1., 0.),
            0.5,
        )))
        .unwrap();
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(ellipse)],
        None,
    );
    assert_eq!(
        analyze(&doc, &ParameterTable::new(), &set)[&ellipse],
        Status::Full
    );
}

#[test]
fn block_constraint_status_does_not_classify_entities_from_a_foreign_scope() {
    let (doc, handles, mut set) = fixture();
    set.scope = ParametricScope::Block(Handle::from(0xFFFFu64));
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    assert!(analyze(&doc, &ParameterTable::new(), &set).is_empty());
}

fn display_fixture() -> (Scene, Handle, Vec<Handle>) {
    let mut scene = Scene::new();
    let mut a = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    a.common.color = codec::types::Color::Index(1);
    let mut b = Line::from_points(Vector3::new(3., 0., 0.), Vector3::new(4., 0., 0.));
    b.common.color = codec::types::Color::Index(5);
    scene.define_block_raw(
        "STATUS_DISPLAY",
        Vector3::ZERO,
        vec![
            EntityType::Line(a),
            EntityType::Line(b),
            EntityType::Circle(Circle::from_center_radius(Vector3::new(8., 0., 0.), 2.)),
        ],
    );
    let record = scene.document.block_records.get("STATUS_DISPLAY").unwrap();
    let (definition, handles) = (record.handle, record.entity_handles.clone());
    scene.block_edit_block = Some(definition);
    let set = scene.parametric_constraint_set_mut(ParametricScope::Block(definition));
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(handles[0])],
        None,
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(handles[1])],
        None,
    );
    (scene, definition, handles)
}

fn colors() -> [String; 4] {
    [
        "RGB:20,30,40".into(),
        "RGB:50,60,70".into(),
        "RGB:80,90,100".into(),
        "RGB:110,120,130".into(),
    ]
}

fn wire_color(wires: &[crate::scene::WireModel], handle: Handle) -> [f32; 4] {
    wires
        .iter()
        .find(|wire| Scene::handle_from_wire_name(&wire.name) == Some(handle))
        .expect("displayed entity wire")
        .color
}

#[test]
fn block_constraint_shading_reuses_reports_and_updates_after_constraint_changes() {
    let (mut scene, definition, handles) = display_fixture();
    let before = scene.document.clone();
    scene.set_block_constraint_display(true, colors());
    let report = scene.block_constraint_report(definition);
    assert!(std::sync::Arc::ptr_eq(
        &report,
        &scene.block_constraint_report(definition)
    ));
    let palette = scene.constraint_palette();
    let wires = scene.entity_wires();
    assert_eq!(wire_color(&wires, handles[0]), palette[2]);
    assert_eq!(wire_color(&wires, handles[1]), palette[1]);
    assert_eq!(wire_color(&wires, handles[2]), palette[0]);
    scene
        .parametric_constraint_set_mut(ParametricScope::Block(definition))
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(handles[1])],
            None,
        );
    assert_eq!(wire_color(&scene.entity_wires(), handles[1]), palette[3]);
    let epoch = scene.geometry_epoch;
    for set in &mut scene.parametric_constraints {
        for constraint in &mut set.constraints {
            constraint.enabled = false;
        }
    }
    let updated = scene.entity_wires();
    assert_eq!(
        scene.geometry_epoch, epoch,
        "constraint-only edits need no geometry change"
    );
    for handle in &handles {
        assert_eq!(wire_color(&updated, *handle), palette[0]);
    }
    assert!(!std::sync::Arc::ptr_eq(
        &report,
        &scene.block_constraint_report(definition)
    ));
    assert_eq!(scene.document, before);
}

#[test]
fn block_constraint_shading_never_changes_reference_or_plot_colors() {
    let (mut scene, definition, handles) = display_fixture();
    let insert = scene
        .document
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "STATUS_DISPLAY",
            Vector3::ZERO,
        )))
        .unwrap();
    let baseline = scene.entity_wires();
    let reference = scene.wire_models_for(&[insert]);
    let source = scene.document.clone();
    scene.set_block_constraint_display(true, colors());
    let colored = scene.entity_wires();
    assert_ne!(
        wire_color(&colored, handles[0]),
        wire_color(&baseline, handles[0])
    );
    assert_eq!(
        scene
            .wire_models_for(&[insert])
            .iter()
            .map(|wire| wire.color)
            .collect::<Vec<_>>(),
        reference.iter().map(|wire| wire.color).collect::<Vec<_>>()
    );
    let (plot, _) = scene.plot_wire_groups(None);
    assert_eq!(
        wire_color(&plot, handles[0]),
        wire_color(&baseline, handles[0])
    );
    assert!(
        scene.constraint_shading_active(),
        "plot suppression restores the display mode"
    );
    assert_eq!(
        wire_color(&scene.entity_wires(), handles[0]),
        wire_color(&colored, handles[0])
    );
    scene.set_block_constraint_display(false, colors());
    assert_eq!(
        wire_color(&scene.entity_wires(), handles[0]),
        wire_color(&baseline, handles[0])
    );
    assert_eq!(scene.block_edit_block, Some(definition));
    assert_eq!(scene.document, source);
}

#[test]
fn block_constraint_report_tracks_named_parameters_and_geometry_epochs() {
    let (mut scene, definition, handles) = display_fixture();
    scene.named_parameters.set("radius", "2").unwrap();
    scene
        .parametric_constraint_set_mut(ParametricScope::Block(definition))
        .add(
            ConstraintKind::Radius,
            vec![ParametricRef::whole(handles[2])],
            Some(DrivingValue::Named("radius".into())),
        );
    let report = scene.block_constraint_report(definition);
    assert_eq!(report[&handles[2]], Status::Partial);
    scene.named_parameters.set("radius", "0").unwrap();
    let invalid = scene.block_constraint_report(definition);
    assert_eq!(invalid[&handles[2]], Status::Unavailable);
    assert!(!std::sync::Arc::ptr_eq(&report, &invalid));
    scene.bump_geometry();
    assert!(!std::sync::Arc::ptr_eq(
        &invalid,
        &scene.block_constraint_report(definition)
    ));
}

#[test]
fn block_constraint_mesh_shading_changes_only_display_materials() {
    use crate::scene::model::mesh_model::MeshModel;
    use crate::scene::MeshLodSet;
    let (mut scene, definition, handles) = display_fixture();
    let mesh = MeshModel {
        name: handles[0].value().to_string(),
        verts: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        verts_low: vec![],
        normals: vec![],
        indices: vec![0, 1, 2],
        triangle_material_handles: vec![],
        triangle_colors: vec![Some([1., 0., 0., 1.])],
        color: [1., 0., 0., 1.],
        selected: false,
    };
    let original = MeshLodSet::from_single(mesh);
    let mut models = vec![original.clone()];
    scene.set_block_constraint_display(true, colors());
    scene.shade_constraint_meshes(definition, &mut models);
    assert_eq!(
        models[0].display_color(),
        Some(scene.constraint_palette()[2])
    );
    assert_eq!(models[0].lods[0].verts, original.lods[0].verts);
    assert_eq!(models[0].lods[0].indices, original.lods[0].indices);
    assert_eq!(original.lods[0].color, [1., 0., 0., 1.]);
    assert!(models[0].lods[0].triangle_colors.is_empty());
}

#[test]
fn block_constraint_shading_covers_gradient_fills_and_restores_their_colors() {
    use crate::scene::model::hatch_model::{GradientKind, HatchPattern};
    let (mut scene, _, _) = display_fixture();
    let solid = codec::entities::Solid::new(
        Vector3::ZERO,
        Vector3::UNIT_X,
        Vector3::UNIT_Y,
        Vector3::new(1., 1., 0.),
    );
    let mut hatch = Scene::solid_hatch_model(&solid, [0.1, 0.8, 0.2, 1.]);
    hatch.pattern = HatchPattern::Gradient {
        angle_deg: 0.,
        color2: [0.8, 0.1, 0.2, 1.],
        kind: GradientKind::Linear,
        invert: false,
        shift: 0.,
    };
    scene.add_hatch(hatch, Some("0"), None);
    let original = scene.hatch_models_arc();
    assert!(!original.is_empty());
    let document = scene.document.clone();
    scene.set_block_constraint_display(true, colors());
    let shaded = scene.hatch_models_arc();
    assert_eq!(shaded[0].color, scene.constraint_palette()[0]);
    if let HatchPattern::Gradient { color2, .. } = &shaded[0].pattern {
        assert_eq!(*color2, shaded[0].color);
    } else {
        panic!("gradient retained");
    }
    scene.set_block_constraint_display(false, colors());
    assert_eq!(scene.hatch_models_arc()[0].color, original[0].color);
    assert_eq!(scene.document, document);
}
