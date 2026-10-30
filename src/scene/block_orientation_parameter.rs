//! Native Rotation, Alignment and Flip parameter placement.
use super::block_authoring::{self as authoring, *};
use super::block_distance_parameter::Graphic;
use codec::objects::{
    BlockAlignmentParameter, BlockConnection, BlockElement, BlockEvalValue, BlockFlipGrip,
    BlockFlipParameter, BlockGrip, BlockGripExpression, BlockOrientedGrip, BlockParameter,
    BlockParameterProperty, BlockParameterValueSet, BlockRotationParameter, BlockTwoPointParameter,
    DynamicBlockData, ObjectType,
};
use codec::types::{Transform, Vector3};
use codec::{Document, Handle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rotation,
    Alignment,
    Flip,
}

impl Kind {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "ROTATION" | "R" => Some(Self::Rotation),
            "ALIGNMENT" | "A" => Some(Self::Alignment),
            "FLIP" | "F" => Some(Self::Flip),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Rotation => "Rotation",
            Self::Alignment => "Alignment",
            Self::Flip => "Flip",
        }
    }
    fn classes(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Rotation => (
                "BLOCKROTATIONPARAMETER",
                "AcDbBlockRotationParameter",
                "BLOCKROTATIONGRIP",
                "AcDbBlockRotationGrip",
            ),
            Self::Alignment => (
                "BLOCKALIGNMENTPARAMETER",
                "AcDbBlockAlignmentParameter",
                "BLOCKALIGNMENTGRIP",
                "AcDbBlockAlignmentGrip",
            ),
            Self::Flip => (
                "BLOCKFLIPPARAMETER",
                "AcDbBlockFlipParameter",
                "BLOCKFLIPGRIP",
                "AcDbBlockFlipGrip",
            ),
        }
    }
}

pub(crate) struct Parameter<'a> {
    pub handle: Handle,
    pub kind: Kind,
    pub native: &'a BlockTwoPointParameter,
    pub reference: Vector3,
    pub label: String,
    pub offset: f64,
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
        return vec![];
    };
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&graph) else {
        return vec![];
    };
    let DynamicBlockData::EvaluationGraph(data) = &object.data else {
        return vec![];
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
            let (kind, native, reference, label, offset) = match &object.data {
                DynamicBlockData::RotationParameter(p) => (
                    Kind::Rotation,
                    &p.parameter,
                    p.definition_base_angle_point,
                    p.angle_name.clone(),
                    p.angle,
                ),
                DynamicBlockData::AlignmentParameter(p) => (
                    Kind::Alignment,
                    &p.parameter,
                    p.parameter.definition_end_point,
                    p.parameter.parameter.element.name.clone(),
                    0.0,
                ),
                DynamicBlockData::FlipParameter(p) => (
                    Kind::Flip,
                    &p.parameter,
                    p.definition_label_point,
                    p.flip_label.clone(),
                    0.0,
                ),
                _ => return None,
            };
            Some(Parameter {
                handle: object.handle,
                kind,
                native,
                reference,
                label,
                offset,
            })
        })
        .collect()
}

pub(crate) fn set_alignment_type(
    document: &mut Document, definition: Handle, handle: Handle, perpendicular: bool,
) -> Result<bool, &'static str> {
    if !parameters(document, definition).iter().any(|p| p.handle == handle && p.kind == Kind::Alignment) {
        return Err("Select an Alignment parameter in the active block definition.");
    }
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) else { unreachable!() };
    let DynamicBlockData::AlignmentParameter(parameter) = &mut object.data else { unreachable!() };
    if parameter.align_perpendicular == perpendicular { return Ok(false); }
    parameter.align_perpendicular = perpendicular;
    Ok(true)
}

pub(crate) fn placement_error(
    document: &Document,
    definition: Handle,
    kind: Kind,
) -> Option<&'static str> {
    let nodes = if kind == Kind::Flip { 5 } else { 4 };
    authoring::capacity(document, definition, nodes, nodes - 1).err()
}

pub(crate) fn points_error(
    kind: Kind,
    base: Vector3,
    end: Vector3,
    reference: Vector3,
) -> Option<&'static str> {
    if ![base, end, reference]
        .iter()
        .all(|p| [p.x, p.y, p.z].iter().all(|v| v.is_finite()))
    {
        return Some("Specify finite parameter locations.");
    }
    let d = end - base;
    let length = d.x.hypot(d.y);
    if !length.is_finite() {
        return Some("The parameter dimensions are too large.");
    }
    if length <= 1e-10 || (end.z - base.z).abs() > 1e-8 || (reference.z - base.z).abs() > 1e-8 {
        return Some("Specify distinct parameter points in the same block XY plane.");
    }
    if kind == Kind::Rotation {
        let r = reference - base;
        if !r.length().is_finite() || !d.length().is_finite() {
            return Some("The parameter dimensions are too large.");
        }
        if !r.x.hypot(r.y).is_finite() || (r.x.hypot(r.y) - length).abs() > length.max(1.0) * 1e-8 {
            return Some("The rotation grip and base-angle point must share the specified radius.");
        }
    }
    None
}

pub(crate) fn create(
    document: &mut Document,
    definition: Handle,
    kind: Kind,
    base: Vector3,
    end: Vector3,
    reference: Vector3,
) -> Result<Handle, &'static str> {
    if let Some(error) = points_error(kind, base, end, reference)
        .or_else(|| placement_error(document, definition, kind))
    {
        return Err(error);
    }
    let mut names = super::block_distance_parameter::parameters(document, definition)
        .into_iter()
        .flat_map(|p| p.labels)
        .collect::<Vec<_>>();
    names.extend(
        parameters(document, definition)
            .into_iter()
            .map(|p| p.label),
    );
    let prefix = match kind {
        Kind::Rotation => "Angle",
        Kind::Alignment => "Alignment",
        Kind::Flip => "Flip state",
    };
    let label = (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|name| names.iter().all(|old| !old.eq_ignore_ascii_case(name)))
        .unwrap();
    let (graph, slots) = authoring::prepare(document, definition)?;
    let (class, cpp, grip_class, grip_cpp) = kind.classes();
    register_classes(
        document,
        &[
            (class, cpp, 55),
            (grip_class, grip_cpp, 55),
            ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
        ],
    );
    let count = if kind == Kind::Flip { 5 } else { 4 };
    let handles = (0..count)
        .map(|_| document.allocate_handle())
        .collect::<Vec<_>>();
    let element = |offset, name: &str| BlockElement {
        eval: expression(slots.eval + offset, NO_VALUE, BlockEvalValue::None),
        name: name.into(),
        major: MAJOR,
        minor: MINOR,
        eed_1071: 0,
    };
    let native = BlockTwoPointParameter {
        parameter: BlockParameter {
            element: element(
                0,
                if kind == Kind::Alignment {
                    &label
                } else {
                    kind.name()
                },
            ),
            show_properties: true,
            chain_actions: false,
        },
        definition_base_point: base,
        definition_end_point: end,
        properties: std::array::from_fn(|i| BlockParameterProperty {
            connections: if kind == Kind::Rotation && i >= 2 {
                vec![BlockConnection {
                    code: slots.eval + 1,
                    name: if i == 2 {
                        "DisplacementX"
                    } else {
                        "DisplacementY"
                    }
                    .into(),
                }]
            } else {
                vec![]
            },
        }),
        property_states: if kind == Kind::Rotation {
            [0, slots.eval + 1, 0, 0]
        } else {
            [slots.eval + 1, 0, 0, 0]
        },
        ..Default::default()
    };
    let data = match kind {
        Kind::Rotation => DynamicBlockData::RotationParameter(BlockRotationParameter {
            parameter: native,
            definition_base_angle_point: reference,
            angle_name: label,
            angle: (end - base).length() * 0.15,
            value_set: BlockParameterValueSet {
                flags: 3,
                maximum: std::f64::consts::TAU,
                ..Default::default()
            },
            ..Default::default()
        }),
        Kind::Alignment => DynamicBlockData::AlignmentParameter(BlockAlignmentParameter {
            parameter: native,
            align_perpendicular: false,
        }),
        Kind::Flip => DynamicBlockData::FlipParameter(BlockFlipParameter {
            parameter: native,
            flip_label: label,
            // Unset labels use UI defaults without persisting a UI language.
            base_state_label: String::new(),
            flipped_state_label: String::new(),
            definition_label_point: reference,
            flags_96: slots.eval + 1,
            tooltip: "UpdatedFlip".into(),
            ..Default::default()
        }),
    };
    document.objects.insert(
        handles[0],
        ObjectType::DynamicBlock(dynamic(handles[0], graph, class, cpp, data)),
    );
    let x_offset = if kind == Kind::Flip { 3 } else { 2 };
    let grip = BlockGrip {
        element: element(1, "Grip"),
        flags_91: slots.eval + x_offset,
        flags_92: slots.eval + x_offset + 1,
        location: if kind == Kind::Rotation { end } else { base },
        insert_cycling: kind == Kind::Alignment,
        insert_cycling_weight: -1,
    };
    let data = match kind {
        Kind::Rotation => DynamicBlockData::RotationGrip(grip),
        Kind::Alignment => DynamicBlockData::AlignmentGrip(BlockOrientedGrip {
            grip,
            orientation: end - base,
        }),
        Kind::Flip => DynamicBlockData::FlipGrip(BlockFlipGrip {
            grip,
            combined_state: slots.eval + 2,
            orientation: end - base,
        }),
    };
    document.objects.insert(
        handles[1],
        ObjectType::DynamicBlock(dynamic(handles[1], graph, grip_class, grip_cpp, data)),
    );
    for (offset, axis, code, value) in [
        (
            x_offset,
            if kind == Kind::Rotation {
                "UpdatedEndX"
            } else {
                "UpdatedBaseX"
            },
            40,
            BlockEvalValue::Real(0.0),
        ),
        (
            x_offset + 1,
            if kind == Kind::Rotation {
                "UpdatedEndY"
            } else {
                "UpdatedBaseY"
            },
            40,
            BlockEvalValue::Real(0.0),
        ),
    ]
    .into_iter()
    .chain((kind == Kind::Flip).then_some((2, "UpdatedFlip", 70, BlockEvalValue::Short(0))))
    {
        let handle = handles[offset as usize];
        document.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(
                handle,
                graph,
                "BLOCKGRIPLOCATIONCOMPONENT",
                "AcDbBlockGripExpr",
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: expression(slots.eval + offset, code, value),
                    grip_type: slots.eval,
                    expression: axis.into(),
                }),
            )),
        );
    }
    let components: &[usize] = if kind == Kind::Flip {
        &[3, 4, 2]
    } else {
        &[2, 3]
    };
    authoring::append_components(document, graph, slots, &handles, components);
    Ok(handles[0])
}

pub(crate) fn graphic(parameter: &Parameter<'_>) -> Graphic {
    let base = parameter.native.definition_base_point;
    let end = parameter.native.definition_end_point;
    let d = end - base;
    let radius = d.x.hypot(d.y).hypot(d.z);
    let mut result = Graphic::default();
    if !radius.is_finite() || radius <= 1e-10 {
        return result;
    }
    match parameter.kind {
        Kind::Rotation => {
            let reference = parameter.reference - base;
            let u = reference.normalize();
            let mut v = if u.z.abs() < 1e-10 && d.z.abs() < 1e-10 {
                Vector3::new(-u.y, u.x, 0.0)
            } else {
                d - u * d.dot(&u)
            };
            if v.length() <= 1e-10 {
                v = Vector3::new(-u.y, u.x, 0.0);
            }
            if v.length() <= 1e-10 {
                v = Vector3::UNIT_Y;
            }
            let v = v.normalize();
            let mut angle = d.dot(&v).atan2(d.dot(&u)).rem_euclid(std::f64::consts::TAU);
            if angle.abs() <= 1e-10 {
                angle = std::f64::consts::TAU;
            }
            let at = |angle: f64, r: f64| base + (u * angle.cos() + v * angle.sin()) * r;
            result
                .lines
                .extend([[base, parameter.reference], [base, end]]);
            for step in 0..48 {
                result.lines.push([
                    at(angle * step as f64 / 48.0, radius),
                    at(angle * (step + 1) as f64 / 48.0, radius),
                ]);
            }
            result.labels.push((
                at(angle * 0.5, radius + parameter.offset),
                parameter.label.clone(),
            ));
            result.round_grips.push(end);
        }
        Kind::Alignment => {
            result.lines.push([base, end]);
            result.labels.push((end, parameter.label.clone()));
            result.grips.push((base, true));
        }
        Kind::Flip => {
            for step in (0..24).step_by(2) {
                result.lines.push([
                    base + d * (step as f64 / 24.0),
                    base + d * ((step + 1) as f64 / 24.0),
                ]);
            }
            result.lines.push([end, parameter.reference]);
            result
                .labels
                .push((parameter.reference, parameter.label.clone()));
            result.grips.push((base, true));
        }
    }
    result
}

/// Tail position used to orient a base grip in the shared screen overlay.
pub(crate) fn marker_tail(parameter: &Parameter<'_>) -> Vector3 {
    let base = parameter.native.definition_base_point;
    let d = parameter.native.definition_end_point - base;
    if parameter.kind == Kind::Flip {
        base - Vector3::new(-d.y, d.x, 0.0)
    } else {
        base - d
    }
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
        .map(|p| (p.handle, p.native.property_states))
        .collect::<Vec<_>>();
    let mut changed = 0;
    for (handle, ids) in owned {
        if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) {
            let native = match &mut object.data {
                DynamicBlockData::RotationParameter(p) => {
                    p.definition_base_angle_point = transform.apply(p.definition_base_angle_point);
                    &mut p.parameter
                }
                DynamicBlockData::AlignmentParameter(p) => &mut p.parameter,
                DynamicBlockData::FlipParameter(p) => {
                    p.definition_label_point = transform.apply(p.definition_label_point);
                    &mut p.parameter
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
            changed += 1;
        }
        for object in document.objects.values_mut() {
            let ObjectType::DynamicBlock(object) = object else {
                continue;
            };
            if object.owner != graph {
                continue;
            }
            let grip = match &mut object.data {
                DynamicBlockData::RotationGrip(g)
                    if g.element.eval.node_id > 0 && ids.contains(&g.element.eval.node_id) =>
                {
                    g
                }
                DynamicBlockData::AlignmentGrip(g)
                    if g.grip.element.eval.node_id > 0
                        && ids.contains(&g.grip.element.eval.node_id) =>
                {
                    g.orientation = transform.apply_rotation(g.orientation);
                    &mut g.grip
                }
                DynamicBlockData::FlipGrip(g)
                    if g.grip.element.eval.node_id > 0
                        && ids.contains(&g.grip.element.eval.node_id) =>
                {
                    g.orientation = transform.apply_rotation(g.orientation);
                    &mut g.grip
                }
                _ => continue,
            };
            grip.location = transform.apply(grip.location);
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests;
