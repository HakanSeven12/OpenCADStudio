//! Native Linear, Polar and XY parameter authoring with one end grip.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockConnection, BlockElement, BlockEvalValue, BlockGrip, BlockGripExpression,
    BlockLinearParameter, BlockOrientedGrip, BlockParameter, BlockParameterProperty,
    BlockParameterValueSet, BlockPolarParameter, BlockTwoPointParameter, BlockXYParameter,
    DynamicBlockData, ObjectType,
};
use codec::types::{Transform, Vector3};
use codec::{Document, Handle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Linear,
    Polar,
    XY,
}

impl Kind {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "LINEAR" | "L" => Some(Self::Linear),
            "POLAR" | "PO" => Some(Self::Polar),
            "XY" => Some(Self::XY),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Polar => "Polar",
            Self::XY => "XY",
        }
    }
    fn class(self) -> (&'static str, &'static str) {
        match self {
            Self::Linear => ("BLOCKLINEARPARAMETER", "AcDbBlockLinearParameter"),
            Self::Polar => ("BLOCKPOLARPARAMETER", "AcDbBlockPolarParameter"),
            Self::XY => ("BLOCKXYPARAMETER", "AcDbBlockXYParameter"),
        }
    }
}

pub(crate) struct Parameter<'a> {
    pub handle: Handle,
    pub kind: Kind,
    pub native: &'a BlockTwoPointParameter,
    pub labels: Vec<String>,
    pub offsets: [f64; 2],
}

fn graph_handle(document: &Document, definition: Handle) -> Option<Handle> {
    match document
        .objects
        .get(&super::block_authoring::definition_dictionary(document, definition)?)?
    {
        ObjectType::Dictionary(dictionary) => dictionary.get("ACAD_ENHANCEDBLOCK"),
        _ => None,
    }
}

pub(crate) fn parameters(document: &Document, definition: Handle) -> Vec<Parameter<'_>> {
    let Some(graph) = graph_handle(document, definition) else {
        return Vec::new();
    };
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&graph) else {
        return Vec::new();
    };
    let DynamicBlockData::EvaluationGraph(data) = &object.data else {
        return Vec::new();
    };
    data.nodes
        .iter()
        .filter_map(|node| {
            let ObjectType::DynamicBlock(object) = document.objects.get(&node.expression)? else {
                return None;
            };
            if object.owner != graph {
                return None;
            }
            let (kind, native, labels, offsets) = match &object.data {
                DynamicBlockData::LinearParameter(p) => (
                    Kind::Linear,
                    &p.parameter,
                    vec![p.distance_name.clone()],
                    [p.distance, 0.0],
                ),
                DynamicBlockData::PolarParameter(p) => (
                    Kind::Polar,
                    &p.parameter,
                    vec![p.distance_name.clone(), p.angle_name.clone()],
                    [p.offset, 0.0],
                ),
                DynamicBlockData::XYParameter(p) => (
                    Kind::XY,
                    &p.parameter,
                    vec![p.x_label.clone(), p.y_label.clone()],
                    [p.x_value, p.y_value],
                ),
                _ => return None,
            };
            Some(Parameter {
                handle: object.handle,
                kind,
                native,
                labels,
                offsets,
            })
        })
        .collect()
}

pub(crate) fn placement_error(document: &Document, definition: Handle) -> Option<&'static str> {
    authoring::graph(document, definition).err()
}

pub(crate) fn points_error(
    kind: Kind,
    base: Vector3,
    end: Vector3,
    label: Vector3,
) -> Option<&'static str> {
    if ![base, end, label]
        .iter()
        .all(|p| [p.x, p.y, p.z].iter().all(|v| v.is_finite()))
    {
        return Some("Specify finite parameter and label locations.");
    }
    let delta = end - base;
    if !delta.x.hypot(delta.y).is_finite() {
        return Some("The parameter dimensions are too large.");
    }
    if delta.x.hypot(delta.y) <= 1e-10
        || (end.z - base.z).abs() > 1e-8
        || (label.z - base.z).abs() > 1e-8
    {
        return Some("Specify distinct parameter points in the same block XY plane.");
    }
    if kind == Kind::XY && (delta.x.abs() <= 1e-10 || delta.y.abs() <= 1e-10) {
        return Some("Specify an XY endpoint with both X and Y distances.");
    }
    let offsets = offsets(kind, base, end, label);
    if !offsets.iter().all(|value| value.is_finite()) {
        return Some("The parameter dimensions are too large.");
    }
    None
}

fn offsets(kind: Kind, base: Vector3, end: Vector3, label: Vector3) -> [f64; 2] {
    if kind == Kind::XY {
        [label.y - base.y, label.x - base.x]
    } else {
        [(label - base).dot(&dimension_normal(base, end)), 0.0]
    }
}

fn dimension_normal(base: Vector3, end: Vector3) -> Vector3 {
    let d = end - base;
    let length = d.x.hypot(d.y);
    // A definition can be reframed onto an upright UCS after placement.
    if length <= 1e-10 {
        Vector3::UNIT_X
    } else {
        Vector3::new(-d.y / length, d.x / length, 0.0)
    }
}

pub(crate) fn create(
    document: &mut Document,
    definition: Handle,
    kind: Kind,
    base: Vector3,
    end: Vector3,
    label: Vector3,
) -> Result<Handle, &'static str> {
    if let Some(error) = points_error(kind, base, end, label) {
        return Err(error);
    }
    let mut names = parameters(document, definition)
        .into_iter()
        .flat_map(|p| p.labels)
        .collect::<Vec<_>>();
    names.extend(
        super::block_orientation_parameter::parameters(document, definition)
            .into_iter()
            .map(|p| p.label),
    );
    let unique = |prefix: &str| {
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|name| names.iter().all(|old| !old.eq_ignore_ascii_case(name)))
            .unwrap()
    };
    let distance_name = unique("Distance");
    let angle_name = unique("Angle");
    let x_label = unique("DistanceX");
    let y_label = unique("DistanceY");
    let values = offsets(kind, base, end, label);
    let (graph, slots) = authoring::prepare(document, definition)?;
    let (class, cpp) = kind.class();
    let (grip_class, grip_cpp) = match kind {
        Kind::Linear => ("BLOCKLINEARGRIP", "AcDbBlockLinearGrip"),
        Kind::Polar => ("BLOCKPOLARGRIP", "AcDbBlockPolarGrip"),
        Kind::XY => ("BLOCKXYGRIP", "AcDbBlockXYGrip"),
    };
    register_classes(
        document,
        &[
            (class, cpp, 55),
            (grip_class, grip_cpp, 55),
            ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
        ],
    );
    let [parameter, grip, x, y] = [(); 4].map(|_| document.allocate_handle());
    let element = |offset, name: &str| BlockElement {
        eval: expression(slots.eval + offset, NO_VALUE, BlockEvalValue::None),
        name: name.into(),
        major: MAJOR,
        minor: MINOR,
        eed_1071: 0,
    };
    let native = BlockTwoPointParameter {
        parameter: BlockParameter {
            element: element(0, kind.name()),
            show_properties: true,
            chain_actions: false,
        },
        definition_base_point: base,
        definition_end_point: end,
        properties: std::array::from_fn(|i| BlockParameterProperty {
            connections: if i < 2 {
                vec![]
            } else {
                vec![BlockConnection {
                    code: slots.eval + 1,
                    name: if i == 2 {
                        "DisplacementX"
                    } else {
                        "DisplacementY"
                    }
                    .into(),
                }]
            },
        }),
        property_states: [0, slots.eval + 1, 0, 0],
        parameter_base_location: 0,
        ..Default::default()
    };
    let value_set = || BlockParameterValueSet {
        flags: 1,
        ..Default::default()
    };
    let data = match kind {
        Kind::Linear => DynamicBlockData::LinearParameter(BlockLinearParameter {
            parameter: native,
            distance_name,
            distance: values[0],
            value_set: value_set(),
            ..Default::default()
        }),
        Kind::Polar => DynamicBlockData::PolarParameter(BlockPolarParameter {
            parameter: native,
            distance_name,
            angle_name,
            offset: values[0],
            distance_value_set: value_set(),
            angle_value_set: BlockParameterValueSet {
                flags: 3,
                maximum: std::f64::consts::TAU,
                ..Default::default()
            },
            ..Default::default()
        }),
        Kind::XY => DynamicBlockData::XYParameter(BlockXYParameter {
            parameter: native,
            x_label,
            y_label,
            x_value: values[0],
            y_value: values[1],
            x_value_set: value_set(),
            y_value_set: value_set(),
            ..Default::default()
        }),
    };
    document.objects.insert(
        parameter,
        ObjectType::DynamicBlock(dynamic(parameter, graph, class, cpp, data)),
    );
    let native_grip = BlockGrip {
        element: element(1, "End Grip"),
        flags_91: slots.eval + 2,
        flags_92: slots.eval + 3,
        location: end,
        insert_cycling: true,
        insert_cycling_weight: -1,
    };
    let data = match kind {
        Kind::Linear => {
            let d = end - base;
            let length = d.x.hypot(d.y);
            DynamicBlockData::LinearGrip(BlockOrientedGrip {
                grip: native_grip,
                orientation: Vector3::new(d.x / length, d.y / length, 0.0),
            })
        }
        Kind::Polar => DynamicBlockData::PolarGrip(native_grip),
        Kind::XY => DynamicBlockData::XYGrip(native_grip),
    };
    document.objects.insert(
        grip,
        ObjectType::DynamicBlock(dynamic(grip, graph, grip_class, grip_cpp, data)),
    );
    for (handle, offset, axis) in [(x, 2, "UpdatedEndX"), (y, 3, "UpdatedEndY")] {
        document.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(
                handle,
                graph,
                "BLOCKGRIPLOCATIONCOMPONENT",
                "AcDbBlockGripExpr",
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: expression(slots.eval + offset, 40, BlockEvalValue::Real(0.0)),
                    grip_type: slots.eval,
                    expression: axis.into(),
                }),
            )),
        );
    }
    authoring::append(document, graph, slots, [parameter, grip, x, y]);
    Ok(parameter)
}

/// World-space editor graphics, derived entirely from the persisted native fields.
#[derive(Default)]
pub(crate) struct Graphic {
    pub lines: Vec<[Vector3; 2]>,
    pub round_grips: Vec<Vector3>,
    pub base_grips: Vec<Vector3>,
    pub lookup_grips: Vec<Vector3>,
    pub dimensions: Vec<[Vector3; 2]>,
    pub labels: Vec<(Vector3, String)>,
    pub grips: Vec<(Vector3, bool)>, // Linear uses a direction arrow; others a square.
}

pub(crate) fn graphic(parameter: &Parameter<'_>) -> Graphic {
    let base = parameter.native.definition_base_point;
    let end = parameter.native.definition_end_point;
    let mut result = Graphic::default();
    let mut dimension = |a: Vector3, b: Vector3, offset: Vector3, label: String| {
        let start = a + offset;
        let finish = b + offset;
        result
            .lines
            .extend([[a, start], [b, finish], [start, finish]]);
        result.dimensions.push([start, finish]);
        result.labels.push(((start + finish) * 0.5, label));
    };
    if parameter.kind == Kind::XY {
        dimension(
            base,
            Vector3::new(end.x, base.y, base.z),
            Vector3::new(0.0, parameter.offsets[0], 0.0),
            parameter.labels[0].clone(),
        );
        dimension(
            base,
            Vector3::new(base.x, end.y, base.z),
            Vector3::new(parameter.offsets[1], 0.0, 0.0),
            parameter.labels[1].clone(),
        );
    } else {
        let d = end - base;
        let length = d.x.hypot(d.y).hypot(d.z);
        if length <= 1e-10 || !length.is_finite() {
            return result;
        }
        dimension(
            base,
            end,
            dimension_normal(base, end) * parameter.offsets[0],
            parameter.labels[0].clone(),
        );
        if parameter.kind == Kind::Polar {
            let angle = d.y.atan2(d.x);
            let radius = length * 0.25;
            let at = |angle: f64| base + Vector3::new(angle.cos(), angle.sin(), 0.0) * radius;
            result.lines.push([base, at(0.0)]);
            result.lines.push([base, end]);
            for step in 0..24 {
                result.lines.push([
                    at(angle * step as f64 / 24.0),
                    at(angle * (step + 1) as f64 / 24.0),
                ]);
            }
            result
                .labels
                .push((at(angle * 0.5), parameter.labels[1].clone()));
        }
    }
    result.grips.push((end, parameter.kind == Kind::Linear));
    result
}

pub(crate) fn reframe(
    document: &mut Document,
    definition: Handle,
    transform: &Transform,
) -> usize {
    let Some(graph) = graph_handle(document, definition) else {
        return 0;
    };
    let owned = parameters(document, definition)
        .iter()
        .map(|p| {
            (
                p.handle,
                p.native.property_states,
                p.kind,
                graphic(p)
                    .labels
                    .iter()
                    .map(|(point, _)| *point)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let mut changed = 0;
    for (handle, grip_ids, kind, labels) in owned {
        if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) {
            let (native, x, y) = match &mut object.data {
                DynamicBlockData::LinearParameter(p) => (&mut p.parameter, &mut p.distance, None),
                DynamicBlockData::PolarParameter(p) => (&mut p.parameter, &mut p.offset, None),
                DynamicBlockData::XYParameter(p) => {
                    (&mut p.parameter, &mut p.x_value, Some(&mut p.y_value))
                }
                _ => continue,
            };
            super::block_actions::reframe_value(&mut native.parameter.element.eval, native.definition_end_point.z, transform);
            native.definition_base_point = transform.apply(native.definition_base_point);
            native.definition_end_point = transform.apply(native.definition_end_point);
            for point in [
                &mut native.updated_base_point,
                &mut native.base_point,
                &mut native.updated_end_point,
                &mut native.end_point,
            ]
            .into_iter()
            .flatten()
            {
                *point = transform.apply(*point);
            }
            if let Some(label) = labels.first() {
                *x = offsets(
                    kind,
                    native.definition_base_point,
                    native.definition_end_point,
                    transform.apply(*label),
                )[0];
            }
            if let (Some(y), Some(label)) = (y, labels.get(1)) {
                *y = transform.apply(*label).x - native.definition_base_point.x;
            }
            changed += 1;
        }
        for object in document.objects.values_mut() {
            if let ObjectType::DynamicBlock(object) = object {
                if object.owner != graph {
                    continue;
                }
                let grip = match &mut object.data {
                    DynamicBlockData::LinearGrip(p)
                        if grip_ids.contains(&p.grip.element.eval.node_id) =>
                    {
                        p.orientation = transform.apply_rotation(p.orientation);
                        &mut p.grip
                    }
                    DynamicBlockData::PolarGrip(p) | DynamicBlockData::XYGrip(p)
                        if grip_ids.contains(&p.element.eval.node_id)
                            && p.element.eval.node_id > 0 =>
                    {
                        p
                    }
                    _ => continue,
                };
                grip.location = transform.apply(grip.location);
                changed += 1;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{block_point_parameter, block_visibility, Scene};
    use codec::{entities::Line, EntityType};

    fn fixture() -> (Scene, Handle) {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "PARAMETERS",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            ))],
        );
        let definition = scene
            .document
            .block_records
            .get("PARAMETERS")
            .unwrap()
            .handle;
        (scene, definition)
    }

    fn append_batch(document: &mut Document, definition: Handle) -> Vec<Handle> {
        [Kind::Linear, Kind::Polar, Kind::XY]
            .map(|kind| {
                create(
                    document,
                    definition,
                    kind,
                    Vector3::new(2.0, 3.0, 0.0),
                    Vector3::new(6.0, 7.0, 0.0),
                    Vector3::new(1.0, 8.0, 0.0),
                )
                .unwrap()
            })
            .to_vec()
    }

    fn assert_native(document: &Document, handles: &[Handle]) {
        for handle in handles {
            let ObjectType::DynamicBlock(object) = &document.objects[handle] else {
                panic!()
            };
            let native = match &object.data {
                DynamicBlockData::LinearParameter(p) => {
                    assert!(!p.distance_name.is_empty());
                    assert!((p.distance - 6.0 / 2.0_f64.sqrt()).abs() < 1e-12);
                    &p.parameter
                }
                DynamicBlockData::PolarParameter(p) => {
                    assert!(!p.distance_name.is_empty());
                    assert!(!p.angle_name.is_empty());
                    assert!((p.offset - 6.0 / 2.0_f64.sqrt()).abs() < 1e-12);
                    &p.parameter
                }
                DynamicBlockData::XYParameter(p) => {
                    assert_ne!(p.x_label, p.y_label);
                    assert_eq!([p.x_value, p.y_value], [5.0, -1.0]);
                    &p.parameter
                }
                _ => panic!("native parameter"),
            };
            assert_eq!(native.definition_base_point, Vector3::new(2.0, 3.0, 0.0));
            assert_eq!(native.definition_end_point, Vector3::new(6.0, 7.0, 0.0));
            let end = native.property_states[1];
            assert!(end > 0);
            assert_eq!(native.property_states, [0, end, 0, 0]);
            assert!(native.properties[..2]
                .iter()
                .all(|p| p.connections.is_empty()));
            for (index, axis) in [(2, "DisplacementX"), (3, "DisplacementY")] {
                assert_eq!(
                    native.properties[index].connections,
                    vec![BlockConnection {
                        code: end,
                        name: axis.into()
                    }]
                );
            }
            let grip = document
                .objects
                .values()
                .find_map(|o| match o {
                    ObjectType::DynamicBlock(o) if o.owner == object.owner => match &o.data {
                        DynamicBlockData::LinearGrip(g) => Some(&g.grip),
                        DynamicBlockData::PolarGrip(g) | DynamicBlockData::XYGrip(g) => Some(g),
                        _ => None,
                    }
                    .filter(|g| g.element.eval.node_id == end),
                    _ => None,
                })
                .expect("native end grip");
            assert_eq!(grip.location, native.definition_end_point);
            for (id, axis) in [
                (grip.flags_91, "UpdatedEndX"),
                (grip.flags_92, "UpdatedEndY"),
            ] {
                assert!(document.objects.values().any(|o| matches!(o, ObjectType::DynamicBlock(o) if o.owner == object.owner && matches!(&o.data, DynamicBlockData::GripLocationComponent(c) if c.eval.node_id == id && c.grip_type == native.parameter.element.eval.node_id && c.expression == axis))));
            }
        }
    }

    #[test]
    fn distance_parameter_batch_native_graph_preserves_point_visibility_and_roundtrips() {
        let (mut scene, definition) = fixture();
        block_point_parameter::create(
            &mut scene.document,
            definition,
            Vector3::ZERO,
            Vector3::UNIT_X,
        )
        .unwrap();
        block_visibility::create(&mut scene.document, definition, Vector3::UNIT_Y).unwrap();
        let graph = graph_handle(&scene.document, definition).unwrap();
        let before = scene.document.clone();
        let handles = append_batch(&mut scene.document, definition);
        for (handle, object) in &before.objects {
            if *handle != graph {
                assert_eq!(scene.document.objects.get(handle), Some(object));
            }
        }
        assert_native(&scene.document, &handles);
        for extension in ["dwg", "dxf"] {
            let bytes =
                crate::io::save_to_bytes(&scene.document, extension, scene.document.version)
                    .unwrap();
            let document =
                crate::io::load_bytes(&format!("parameters.{extension}"), bytes).unwrap();
            assert_native(&document, &handles);
            assert!(authoring::graph(&document, definition).is_ok());
            assert_eq!(parameters(&document, definition).len(), 3);
            assert_eq!(
                block_point_parameter::parameters(&document, definition).len(),
                1
            );
            assert!(block_visibility::parameter(&document, definition).is_some());
        }
        append_batch(&mut scene.document, definition);
        let labels = parameters(&scene.document, definition)
            .into_iter()
            .flat_map(|p| p.labels)
            .collect::<Vec<_>>();
        let unique = labels.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(
            labels.len(),
            unique.len(),
            "labels are unique across parameter types"
        );
    }

    #[test]
    fn distance_parameter_rejects_degenerate_nonfinite_and_full_graph_without_mutation() {
        let (mut scene, definition) = fixture();
        for kind in [Kind::Linear, Kind::Polar, Kind::XY] {
            for end in [
                Vector3::ZERO,
                Vector3::new(f64::NAN, 1.0, 0.0),
                Vector3::new(1.0, 1.0, 1.0),
            ] {
                let before = scene.document.clone();
                assert!(create(
                    &mut scene.document,
                    definition,
                    kind,
                    Vector3::ZERO,
                    end,
                    Vector3::ZERO
                )
                .is_err());
                assert_eq!(scene.document, before);
            }
        }
        assert!(create(
            &mut scene.document,
            definition,
            Kind::XY,
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::ZERO
        )
        .is_err());
        append_batch(&mut scene.document, definition);
        let graph = graph_handle(&scene.document, definition).unwrap();
        if let ObjectType::DynamicBlock(o) = scene.document.objects.get_mut(&graph).unwrap() {
            if let DynamicBlockData::EvaluationGraph(g) = &mut o.data {
                g.first_node_id = i32::MAX;
            }
        }
        let before = scene.document.clone();
        assert!(create(
            &mut scene.document,
            definition,
            Kind::Linear,
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::ZERO
        )
        .is_err());
        assert_eq!(scene.document, before);
    }

    #[test]
    fn distance_parameter_upright_reframe_keeps_finite_label_and_grip() {
        let (mut scene, definition) = fixture();
        create(
            &mut scene.document,
            definition,
            Kind::Linear,
            Vector3::ZERO,
            Vector3::UNIT_Y,
            Vector3::new(1.0, 0.5, 0.0),
        )
        .unwrap();
        let before = graphic(&parameters(&scene.document, definition)[0]);
        let transform = Transform::from_rotation(Vector3::UNIT_X, std::f64::consts::FRAC_PI_2);
        reframe(&mut scene.document, definition, &transform);
        let parameter = &parameters(&scene.document, definition)[0];
        assert!(parameter.offsets[0].is_finite());
        let after = graphic(parameter);
        let expected = transform.apply(before.labels[0].0);
        assert!((after.labels[0].0 - expected).length() < 1e-10);
        assert!((after.grips[0].0 - transform.apply(before.grips[0].0)).length() < 1e-10);
    }

    #[test]
    fn distance_parameter_reframe_and_editor_only_extents() {
        let (mut scene, definition) = fixture();
        append_batch(&mut scene.document, definition);
        let before = parameters(&scene.document, definition)
            .iter()
            .map(graphic)
            .collect::<Vec<_>>();
        let transform = Transform::from_translation(Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(reframe(&mut scene.document, definition, &transform), 6);
        for (parameter, original) in parameters(&scene.document, definition).iter().zip(before) {
            let new = graphic(parameter);
            for ((point, _), (old, _)) in new.labels.iter().zip(original.labels) {
                assert_eq!(*point, transform.apply(old));
            }
            assert_eq!(new.grips[0].0, Vector3::new(16.0, 27.0, 0.0));
        }
        scene.block_edit_block = Some(definition);
        let (_, max) = scene.visible_block_bounds(definition).unwrap();
        assert!(max.x >= 16.0 && max.y >= 28.0);
        scene.block_edit_block = None;
        let (_, max) = scene.visible_block_bounds(definition).unwrap();
        assert!(max.x <= 1.0 && max.y < 1.0);
    }

    #[test]
    #[ignore = "requires OCS_DYNAMIC_BLOCK_FIXTURES with local DWG fixtures"]
    fn distance_parameter_fixture_batch_roundtrips() {
        use sha2::{Digest, Sha256};
        let directory = std::env::var_os("OCS_DYNAMIC_BLOCK_FIXTURES").expect("fixture directory");
        let mut paths = std::fs::read_dir(directory)
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")))
            .collect::<Vec<_>>();
        paths.sort();
        assert_eq!(paths.len(), 17);
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let checksum = Sha256::digest(&bytes);
            let filename = path.file_name().unwrap().to_string_lossy();
            let mut document = crate::io::load_bytes(&filename, bytes).unwrap();
            let definition = document
                .block_records
                .iter()
                .map(|b| b.handle)
                .find(|h| authoring::graph(&document, *h).is_ok_and(|g| g.is_some()))
                .expect("editable existing graph");
            let graph = graph_handle(&document, definition).unwrap();
            let before = document.clone();
            let handles = append_batch(&mut document, definition);
            for (h, o) in &before.objects {
                if *h != graph {
                    assert_eq!(
                        document.objects.get(h),
                        Some(o),
                        "{filename}: preserved {h}"
                    );
                }
            }
            if let (ObjectType::DynamicBlock(old), ObjectType::DynamicBlock(new)) =
                (&before.objects[&graph], &document.objects[&graph])
            {
                if let (
                    DynamicBlockData::EvaluationGraph(old),
                    DynamicBlockData::EvaluationGraph(new),
                ) = (&old.data, &new.data)
                {
                    assert_eq!(&new.nodes[..old.nodes.len()], &old.nodes);
                    assert_eq!(&new.edges[..old.edges.len()], &old.edges);
                }
            }
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&document, extension, document.version).unwrap();
                let read = crate::io::load_bytes(&format!("batch.{extension}"), bytes).unwrap();
                assert_native(&read, &handles);
                assert!(authoring::graph(&read, definition).is_ok());
            }
            assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), checksum);
            eprintln!("{filename}: Linear/Polar/XY appended to existing graph; DWG/DXF passed; original objects/checksum preserved");
        }
    }
}
