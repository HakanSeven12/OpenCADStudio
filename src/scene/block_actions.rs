//! Native Move/Scale/Stretch/Rotate/Flip/Array authoring and deterministic block evaluation.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockAction, BlockActionOffsets, BlockActionWithBasePoint, BlockArrayAction,
    BlockBasePointAction, BlockConnection, BlockElement, BlockEvalValue, BlockFlipAction,
    BlockMoveAction, BlockPolarStretchAction, BlockStretchAction, DynamicBlockData, ObjectType,
};
use codec::types::{Transform, Vector3};
use codec::{Document, EntityType, Handle};
use glam::DVec3;
pub(crate) mod grip_roles;
pub(crate) mod instance;
pub(crate) mod feedback;
pub(crate) mod association;
mod representation;
mod stretch;
mod targets;
mod array;
pub(crate) mod options;

/// Smallest signed distance between two angles in radians, accounting for the
/// wrap at ±π. Used for BPT rotation snapping and current-row detection.
pub(crate) fn angle_distance(a: f64, b: f64) -> f64 {
    ((a - b + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs()
}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod frame_evaluation_tests;
#[cfg(test)]
mod polar_frame_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Move,
    Scale,
    Rotate,
    Flip,
    Stretch,
    PolarStretch,
    Array,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XyKeyPoint {
    Base,
    X,
    Y,
    End,
}
impl Kind {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "MOVE" | "M" => Some(Self::Move),
            "SCALE" | "S" => Some(Self::Scale),
            "ROTATE" | "R" => Some(Self::Rotate),
            "FLIP" | "F" => Some(Self::Flip),
            "STRETCH" | "ST" => Some(Self::Stretch),
            "POLARSTRETCH" | "POLAR STRETCH" | "PS" => Some(Self::PolarStretch),
            "ARRAY" | "A" => Some(Self::Array),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::Scale => "Scale",
            Self::Rotate => "Rotate",
            Self::Flip => "Flip",
            Self::Stretch => "Stretch",
            Self::PolarStretch => "Polar Stretch",
            Self::Array => "Array",
        }
    }
}

/// Actions that author a rectangular frame before selecting objects.
pub(crate) fn needs_frame(kind: Kind) -> bool {
    matches!(kind, Kind::Stretch | Kind::PolarStretch)
}

#[derive(Clone, Debug)]
pub(crate) struct Parameter {
    pub handle: Handle,
    pub id: i32,
    pub grip: Handle,
    pub grip_location: Option<DVec3>,
    pub name: String,
    pub base: DVec3,
    pub end: DVec3,
    pub linear: bool,
    pub point: bool,
    pub xy: bool,
    pub polar: bool,
    pub rotation: bool,
    pub flip: bool,
    pub reference: DVec3,
}
pub(crate) fn v(p: Vector3) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}
pub(crate) fn native(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Point(DVec3),
    Frame { base: DVec3, end: DVec3 },
    Flip(bool),
}
impl Value {
    pub(crate) fn map_points(self, map: impl Fn(DVec3) -> DVec3) -> Self {
        match self {
            Self::Point(point) => Self::Point(map(point)),
            Self::Frame { base, end } => Self::Frame { base: map(base), end: map(end) },
            state => state,
        }
    }
}

pub(crate) fn all_parameters(doc: &Document, definition: Handle) -> Vec<Parameter> {
    let mut result = parameters(doc, definition, Kind::Move);
    result.extend(parameters(doc, definition, Kind::Rotate));
    result.extend(parameters(doc, definition, Kind::Flip));
    result.sort_by_key(|p| p.id);
    result
}

/// Resolve native action association for runtime grip interaction.
pub(crate) fn has_action(doc: &Document, definition: Handle, parameter: Handle) -> bool {
    let Some(p) = all_parameters(doc, definition).into_iter().find(|p| p.handle == parameter) else { return false; };
    actions(doc, definition).iter().any(|(_, _, _, node)| *node == p.id)
}

pub(crate) fn compatible(p: &Parameter, kind: Kind) -> bool {
    match kind {
        Kind::Move => !p.rotation && !p.flip,
        Kind::Stretch => !p.rotation && !p.flip,
        Kind::PolarStretch => p.polar,
        Kind::Scale => !p.rotation && !p.flip && !p.point && !p.xy,
        Kind::Rotate => p.rotation,
        Kind::Flip => p.flip,
        Kind::Array => !p.rotation && !p.flip && !p.point,
    }
}

fn flip_component(doc: &Document, p: &Parameter) -> Result<Handle, &'static str> {
    let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&p.handle) else {
        return Err("Missing Flip parameter.");
    };
    let state_id = match doc.objects.get(&p.grip) {
        Some(ObjectType::DynamicBlock(grip)) => match &grip.data {
            DynamicBlockData::FlipGrip(grip) => Some(grip.combined_state),
            _ => return Err("Missing Flip grip."),
        },
        _ => None,
    };
    let candidates = doc
        .objects
        .iter()
        .filter_map(|(h, x)| match x {
            ObjectType::DynamicBlock(x) if x.owner == o.owner => match &x.data {
                DynamicBlockData::GripLocationComponent(c)
                    if c.grip_type == p.id && state_id.is_none_or(|id| c.eval.node_id == id) && c.expression == "UpdatedFlip" =>
                {
                    Some(*h)
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err("The Flip state binding is unavailable.");
    }
    Ok(candidates[0])
}

pub(crate) fn value(doc: &Document, p: &Parameter) -> Result<Value, &'static str> {
    if !p.flip {
        return Ok(Value::Point(current(doc, p)));
    }
    let h = flip_component(doc, p)?;
    let ObjectType::DynamicBlock(o) = &doc.objects[&h] else {
        unreachable!()
    };
    let DynamicBlockData::GripLocationComponent(c) = &o.data else {
        unreachable!()
    };
    match c.eval.value {
        BlockEvalValue::Short(0) => Ok(Value::Flip(false)),
        BlockEvalValue::Short(1) => Ok(Value::Flip(true)),
        _ => Err("The Flip state is not zero or one."),
    }
}

pub(crate) fn set_value(
    doc: &mut Document,
    p: &Parameter,
    value: Value,
) -> Result<(), &'static str> {
    if let Value::Flip(state) = value {
        if !p.flip {
            return Err("Select a Flip parameter.");
        }
        let h = flip_component(doc, p)?;
        let Some(ObjectType::DynamicBlock(o)) = doc.objects.get_mut(&h) else {
            unreachable!()
        };
        let DynamicBlockData::GripLocationComponent(c) = &mut o.data else {
            unreachable!()
        };
        c.eval.value = BlockEvalValue::Short(i16::from(state));
        return Ok(());
    }
    let Value::Point(position) = value else {
        return Err("Evaluate complete parameter frames in a reference context.");
    };
    if !position.is_finite() {
        return Err("Specify a finite parameter position.");
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&p.handle) else {
        return Err("Missing parameter.");
    };
    let eval = match &mut object.data {
        DynamicBlockData::PointParameter(p) => &mut p.parameter.parameter.element.eval,
        DynamicBlockData::LinearParameter(p) => &mut p.parameter.parameter.element.eval,
        DynamicBlockData::PolarParameter(p) => &mut p.parameter.parameter.element.eval,
        DynamicBlockData::XYParameter(p) => &mut p.parameter.parameter.element.eval,
        DynamicBlockData::RotationParameter(p) => &mut p.parameter.parameter.element.eval,
        _ => return Err("Unsupported parameter."),
    };
    // Scratch evaluation output belongs to the parameter, not its display grip.
    // Evaluated definitions are temporary; saved INSERT values use AppDataCache.
    eval.value_code = 10;
    eval.value = BlockEvalValue::Point([position.x, position.y]);
    Ok(())
}

pub(crate) fn grip_position(p: &Parameter, value: Value) -> DVec3 {
    match value {
        Value::Point(at) | Value::Frame { end: at, .. } => at + grip_offset(p),
        Value::Flip(_) => p.grip_location.unwrap_or(p.reference),
    }
}

/// Native display placement is independent of the evaluated endpoint.
pub(crate) fn grip_offset(p: &Parameter) -> DVec3 { p.grip_location.map(|at| at - p.end).unwrap_or(DVec3::ZERO) }

/// A Flip arrow points across its reflection line and reverses with its state.
pub(crate) fn grip_direction(p: &Parameter, value: Value) -> Option<DVec3> {
    let Value::Flip(flipped) = value else {
        return None;
    };
    let across = DVec3::Z.cross(p.end - p.base).try_normalize()?;
    Some(if flipped { -across } else { across })
}

pub(crate) fn parameters(doc: &Document, definition: Handle, kind: Kind) -> Vec<Parameter> {
    let Ok(Some(graph)) = authoring::graph(doc, definition) else {
        return vec![];
    };
    let mut result = vec![];
    for (handle, p) in super::block_point_parameter::parameters(doc, definition) {
        if matches!(kind, Kind::Move | Kind::Stretch) {
            result.push(Parameter {
                handle,
                id: p.parameter.parameter.element.eval.node_id,
                grip: Handle::NULL,
                grip_location: None,
                name: p.position_name.clone(),
                base: v(p.parameter.definition_point),
                end: v(p.parameter.definition_point),
                linear: false,
                point: true,
                xy: false,
                polar: false,
                rotation: false,
                flip: false,
                reference: DVec3::ZERO,
            });
        }
    }
    for p in super::block_distance_parameter::parameters(doc, definition) {
        if matches!(kind, Kind::Rotate | Kind::Flip) {
            continue;
        }
        let polar = p.kind == super::block_distance_parameter::Kind::Polar;
        let xy = p.kind == super::block_distance_parameter::Kind::XY;
        if kind == Kind::Scale && xy {
            continue;
        }
        if kind == Kind::PolarStretch && !polar {
            continue;
        }
        result.push(Parameter {
            handle: p.handle,
            id: p.native.parameter.element.eval.node_id,
            grip: Handle::NULL,
            grip_location: None,
            name: p.labels.first().cloned().unwrap_or_default(),
            base: v(p.native.definition_base_point),
            end: v(p.native.definition_end_point),
            linear: p.kind == super::block_distance_parameter::Kind::Linear,
            point: false,
            xy,
            polar,
            rotation: false,
            flip: false,
            reference: DVec3::ZERO,
        });
    }
    for p in super::block_orientation_parameter::parameters(doc, definition) {
        use super::block_orientation_parameter::Kind as O;
        if !matches!(
            (kind, p.kind),
            (Kind::Rotate, O::Rotation) | (Kind::Flip, O::Flip)
        ) {
            continue;
        }
        result.push(Parameter {
            handle: p.handle,
            id: p.native.parameter.element.eval.node_id,
            grip: Handle::NULL,
            grip_location: None,
            name: p.label,
            base: v(p.native.definition_base_point),
            end: v(p.native.definition_end_point),
            linear: false,
            point: false,
            xy: false,
            polar: false,
            rotation: kind == Kind::Rotate,
            flip: kind == Kind::Flip,
            reference: v(p.reference),
        });
    }
    // Resolve the actual linked grip, rather than assuming expression IDs are contiguous.
    result.retain_mut(|p| {
        let grip_id = if p.rotation || p.flip {
            let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&p.handle) else {
                return false;
            };
            match &o.data {
                DynamicBlockData::RotationParameter(p) => p.parameter.property_states[1],
                DynamicBlockData::FlipParameter(p) => p.flags_96,
                _ => return false,
            }
        } else if p.point {
            super::block_point_parameter::parameters(doc, definition)
                .into_iter()
                .find(|(h, _)| *h == p.handle)
                .unwrap()
                .1
                .parameter
                .property_count
        } else {
            super::block_distance_parameter::parameters(doc, definition)
                .into_iter()
                .find(|q| q.handle == p.handle)
                .unwrap()
                .native
                .properties[2]
                .connections
                .first()
                .map(|c| c.code)
                .unwrap_or(-1)
        };
        let grips = doc
            .objects
            .iter()
            .filter_map(|(h, o)| {
                let ObjectType::DynamicBlock(o) = o else {
                    return None;
                };
                if o.owner != graph {
                    return None;
                }
                let grip = match &o.data {
                    DynamicBlockData::XYGrip(g)
                    | DynamicBlockData::PolarGrip(g)
                    | DynamicBlockData::RotationGrip(g) => g,
                    DynamicBlockData::FlipGrip(g) => &g.grip,
                    DynamicBlockData::LinearGrip(g) => &g.grip,
                    _ => return None,
                };
                (grip.element.eval.node_id == grip_id).then_some(*h)
            })
            .collect::<Vec<_>>();
        if grips.len() > 1
            || (grips.is_empty() && grip_id > 0)
            || !p.base.is_finite()
            || !p.end.is_finite()
            || (!p.point && (p.end.distance(p.base) < 1e-9 || !p.end.distance(p.base).is_finite()))
        {
            return false;
        }
        p.grip = grips.first().copied().unwrap_or(Handle::NULL);
        p.grip_location = doc.objects.get(&p.grip).and_then(|object| {
            let ObjectType::DynamicBlock(object) = object else { return None };
            match &object.data {
                DynamicBlockData::LinearGrip(g) => Some(v(g.grip.location)),
                DynamicBlockData::FlipGrip(g) => Some(v(g.grip.location)),
                DynamicBlockData::XYGrip(g) | DynamicBlockData::PolarGrip(g) | DynamicBlockData::RotationGrip(g) => Some(v(g.location)),
                _ => None,
            }
        });
        if p.flip {
            if let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&p.grip) {
                if let DynamicBlockData::FlipGrip(g) = &o.data {
                    let at = v(g.grip.location);
                    if at.distance(p.base) > 1e-8 {
                        p.reference = at;
                    }
                }
            }
            if p.reference.distance(p.base) < 1e-8 {
                p.reference = p.end;
            }
        }
        true
    });
    result.sort_by_key(|p| p.id);
    result
}

pub(crate) fn supported(e: &EntityType) -> bool {
    matches!(
        e,
        EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::LwPolyline(_)
            | EntityType::Ellipse(_)
            | EntityType::Point(_)
            | EntityType::Text(_)
            | EntityType::MText(_)
    )
}
#[cfg(test)]
pub(crate) fn create(
    doc: &mut Document,
    definition: Handle,
    kind: Kind,
    parameter: Handle,
    targets: &[Handle],
    location: DVec3,
) -> Result<Handle, &'static str> {
    create_with_options(
        doc, definition, kind, parameter, None, targets, location, None, None,
    )
}

#[cfg(test)]
pub(crate) fn create_array(
    doc: &mut Document,
    definition: Handle,
    parameter: Handle,
    targets: &[Handle],
    location: DVec3,
    column_offset: f64,
    row_offset: f64,
) -> Result<Handle, &'static str> {
    create_with_options(
        doc,
        definition,
        Kind::Array,
        parameter,
        None,
        targets,
        location,
        None,
        Some((column_offset, row_offset)),
    )
}

#[cfg(test)]
pub(crate) fn create_with_frame(
    doc: &mut Document,
    definition: Handle,
    kind: Kind,
    parameter: Handle,
    targets: &[Handle],
    location: DVec3,
    frame: Option<[DVec3; 2]>,
) -> Result<Handle, &'static str> {
    create_with_frame_points(
        doc, definition, kind, parameter, None, targets, location,
        frame.as_ref().map(|points| points.as_slice()), None,
    )
}

#[cfg(test)]
pub(crate) fn create_with_options(
    doc: &mut Document,
    definition: Handle,
    kind: Kind,
    parameter: Handle,
    xy_key_point: Option<XyKeyPoint>,
    targets: &[Handle],
    location: DVec3,
    frame: Option<[DVec3; 2]>,
    array_offsets: Option<(f64, f64)>,
) -> Result<Handle, &'static str> {
    create_with_frame_points(doc, definition, kind, parameter, xy_key_point, targets, location,
        frame.as_ref().map(|points| points.as_slice()), array_offsets)
}

pub(crate) fn create_with_frame_points(
    doc: &mut Document,
    definition: Handle,
    kind: Kind,
    parameter: Handle,
    xy_key_point: Option<XyKeyPoint>,
    targets: &[Handle],
    location: DVec3,
    frame: Option<&[DVec3]>,
    array_offsets: Option<(f64, f64)>,
) -> Result<Handle, &'static str> {
    if !location.is_finite() {
        return Err("Specify a finite action location.");
    }
    let p = parameters(doc, definition, kind)
        .into_iter()
        .find(|p| p.handle == parameter)
        .ok_or("Select a compatible parameter belonging to this block.")?;
    if xy_key_point.is_some_and(|point| {
        !matches!(kind, Kind::Move | Kind::Stretch | Kind::PolarStretch)
            || (p.xy
                && !matches!(
                    point,
                    XyKeyPoint::Base | XyKeyPoint::X | XyKeyPoint::Y | XyKeyPoint::End
                ))
            || (!p.xy && !matches!(point, XyKeyPoint::Base | XyKeyPoint::End))
    }) {
        return Err("Action key points are not compatible with this parameter.");
    }
    if targets.is_empty() {
        return Err("Select at least one object for the action.");
    }
    let members = &doc
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .ok_or("The block definition is unavailable.")?
        .entity_handles;
    if targets.iter().any(|h| {
        !members.contains(h)
            || doc
                .get_entity(*h)
                .is_none_or(|e| !supported(e) || e.common().owner_handle != definition)
    }) {
        return Err(
            "Select lines, lightweight polylines, arcs, circles, ellipses, points or text belonging to this block.",
        );
    }
    let (column_offset, row_offset) = if kind == Kind::Array {
        let (col, row) = array_offsets.unwrap_or((1.0, 1.0));
        if col <= 1e-6 || !col.is_finite() {
            return Err("Specify a positive column offset.");
        }
        if p.xy && (row <= 1e-6 || !row.is_finite()) {
            return Err("Specify a positive row offset.");
        }
        (col, row)
    } else {
        (0.0, 0.0)
    };
    let stretch_binding = if kind == Kind::Stretch {
        if (location.z - p.end.z).abs() > 1e-8 {
            return Err("Place the Stretch marker in the parameter's XY plane.");
        }
        Some(stretch::bind_frame(
            doc,
            targets,
            frame.ok_or("Specify a stretch frame before selecting objects.")?,
            p.end.z,
        )?)
    } else {
        None
    };
    let polar_binding = if kind == Kind::PolarStretch {
        if (location.z - p.end.z).abs() > 1e-8 {
            return Err("Place the Polar Stretch marker in the parameter's XY plane.");
        }
        Some(stretch::bind_frame(
            doc,
            targets,
            frame.ok_or("Specify a Polar Stretch frame before selecting objects.")?,
            p.end.z,
        )?)
    } else {
        None
    };
    let mut staged = doc.clone();
    let (graph, slots) = authoring::prepare(&mut staged, definition)?;
    let (dxf, cpp) = match kind {
        Kind::Move => ("BLOCKMOVEACTION", "AcDbBlockMoveAction"),
        Kind::Scale => ("BLOCKSCALEACTION", "AcDbBlockScaleAction"),
        Kind::Rotate => ("BLOCKROTATEACTION", "AcDbBlockRotationAction"),
        Kind::Flip => ("BLOCKFLIPACTION", "AcDbBlockFlipAction"),
        Kind::Stretch => ("BLOCKSTRETCHACTION", "AcDbBlockStretchAction"),
        Kind::PolarStretch => ("BLOCKPOLARSTRETCHACTION", "AcDbBlockPolarStretchAction"),
        Kind::Array => ("BLOCKARRAYACTION", "AcDbBlockArrayAction"),
    };
    register_classes(&mut staged, &[(dxf, cpp, 55)]);
    let handle = staged.allocate_handle();
    let mut dependencies = targets.to_vec();
    dependencies.sort();
    dependencies.dedup();
    let action = BlockAction {
        element: BlockElement {
            eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
            name: format!("{}{}", kind.name(), slots.eval),
            major: MAJOR,
            minor: MINOR,
            eed_1071: 0,
        },
        display_location: native(location),
        dependencies: dependencies.clone(),
        action_ids: vec![],
    };
    let connection = |name: &str| BlockConnection {
        code: p.id,
        name: name.into(),
    };
    let data = match kind {
        Kind::Array => DynamicBlockData::ArrayAction(BlockArrayAction {
            action,
            connections: [
                connection("Base"),
                connection("End"),
                connection("UpdatedBase"),
                connection("UpdatedEnd"),
            ],
            column_offset,
            row_offset,
        }),
        Kind::Stretch => {
            let (points, handles) = stretch_binding.unwrap();
            DynamicBlockData::StretchAction(BlockStretchAction {
                action,
                connections: move_connections(&p, xy_key_point),
                points,
                handles,
                codes: vec![],
                offsets: BlockActionOffsets {
                    distance_multiplier: 1.,
                    angle_offset: 0.,
                    flags: 0,
                },
            })
        }
        Kind::PolarStretch => {
            let (points, bindings) = polar_binding.unwrap();
            DynamicBlockData::PolarStretchAction(BlockPolarStretchAction {
                action,
                connections: polar_connections(&p, xy_key_point),
                points,
                handles: vec![],
                bindings,
                codes: vec![],
                distance_multiplier: 1.,
                angle_offset: 0.,
                extra: vec![],
            })
        }
        Kind::Move => DynamicBlockData::MoveAction(BlockMoveAction {
            action,
            connections: move_connections(&p, xy_key_point),
            offsets: BlockActionOffsets {
                distance_multiplier: 1.,
                angle_offset: 0.,
                flags: 0,
            },
        }),
        Kind::Flip => DynamicBlockData::FlipAction(BlockFlipAction {
            action,
            connections: [
                connection("Flip"),
                connection("UpdatedFlip"),
                connection("UpdatedBase"),
                connection("UpdatedEnd"),
            ],
        }),
        Kind::Rotate => DynamicBlockData::RotateAction(BlockBasePointAction {
            action: BlockActionWithBasePoint {
                action,
                offset: Vector3::ZERO,
                connections: vec![connection("UpdatedBaseX"), connection("UpdatedBaseY")],
                dependent: true,
                base_point: Vector3::ZERO,
            },
            connections: vec![connection("AngleDelta")],
            dwg_tail_byte: 0,
        }),
        Kind::Scale => DynamicBlockData::ScaleAction(BlockBasePointAction {
            action: BlockActionWithBasePoint {
                action,
                offset: Vector3::ZERO,
                connections: vec![connection("UpdatedBaseX"), connection("UpdatedBaseY")],
                dependent: true,
                base_point: Vector3::ZERO,
            },
            connections: vec![
                connection("Scale"),
                connection("XScale"),
                connection("YScale"),
            ],
            dwg_tail_byte: 0,
        }),
    };
    staged.objects.insert(
        handle,
        ObjectType::DynamicBlock(dynamic(handle, graph, dxf, cpp, data)),
    );
    authoring::append_single(&mut staged, graph, slots, handle);
    authoring::graph::bind_ids(&mut staged, graph, slots.eval, &[p.id])?;
    *doc = staged;
    Ok(handle)
}

fn move_connections(p: &Parameter, key_point: Option<XyKeyPoint>) -> [BlockConnection; 2] {
    let names = match (p.point, p.xy, key_point) {
        (true, _, _) => ["XDelta", "YDelta"],
        (_, _, Some(XyKeyPoint::Base)) => ["BaseXDelta", "BaseYDelta"],
        (_, true, Some(XyKeyPoint::X)) => ["EndXDelta", "BaseYDelta"],
        (_, true, Some(XyKeyPoint::Y)) => ["BaseXDelta", "EndYDelta"],
        (_, true, Some(XyKeyPoint::End) | None) | (_, false, _) => ["EndXDelta", "EndYDelta"],
    };
    names.map(|name| BlockConnection {
        code: p.id,
        name: name.into(),
    })
}

fn polar_connections(p: &Parameter, key_point: Option<XyKeyPoint>) -> [BlockConnection; 6] {
    let (x, y) = if key_point == Some(XyKeyPoint::Base) {
        ("BaseXDelta", "BaseYDelta")
    } else { ("EndXDelta", "EndYDelta") };
    [x, y, "Base", "End", "UpdatedBase", "UpdatedEnd"].map(|name| BlockConnection { code: p.id, name: name.into() })
}

fn action_move_delta(
    parameter: &Parameter,
    current_base: DVec3,
    current_end: DVec3,
    connections: &[BlockConnection; 2],
) -> Option<DVec3> {
    if connections
        .iter()
        .any(|connection| connection.code != parameter.id)
    {
        return None;
    }
    let delta = current_end - parameter.end;
    let names = [connections[0].name.as_str(), connections[1].name.as_str()];
    if parameter.point && names == ["XDelta", "YDelta"] {
        return Some(delta);
    }
    let base_delta = current_base - parameter.base;
    match names {
        ["BaseXDelta", "BaseYDelta"] => Some(base_delta),
        ["EndXDelta", "BaseYDelta"] if parameter.xy => Some(DVec3::new(delta.x, base_delta.y, 0.)),
        ["BaseXDelta", "EndYDelta"] if parameter.xy => Some(DVec3::new(base_delta.x, delta.y, 0.)),
        ["EndXDelta", "EndYDelta"] => Some(delta),
        _ => None,
    }
}

/// Retarget an existing native action, retaining identity, graph and options.
#[cfg(test)]
pub(crate) fn retarget(
    doc: &mut Document,
    definition: Handle,
    handle: Handle,
    targets: &[Handle],
    frame: Option<[DVec3; 2]>,
) -> Result<bool, &'static str> {
    retarget_frame_points(doc, definition, handle, targets, frame.as_ref().map(|points| points.as_slice()))
}

pub(crate) fn validate_frame_points(points: &[DVec3]) -> Result<(), &'static str> {
    let plane = points.first().ok_or("Specify a stretch frame.")?.z;
    stretch::native_frame(points, plane).map(|_| ())
}

pub(crate) fn retarget_frame_points(
    doc: &mut Document,
    definition: Handle,
    handle: Handle,
    targets: &[Handle],
    frame: Option<&[DVec3]>,
) -> Result<bool, &'static str> {
    let (_, action, kind, id) = actions(doc, definition)
        .into_iter()
        .find(|(h, _, _, _)| *h == handle)
        .ok_or("Select an action belonging to this block.")?;
    let mut targets = targets.to_vec();
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err("An action needs at least one object.");
    }
    let members = &doc
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .ok_or("The definition is unavailable.")?
        .entity_handles;
    if targets.iter().any(|h| {
        !members.contains(h)
            || doc
                .get_entity(*h)
                .is_none_or(|e| e.common().owner_handle != definition || !supported(e))
    }) {
        return Err("Select supported drawing objects belonging to this block.");
    }
    let old_targets = action.dependencies.clone();
    let old_geometry = targets::geometry(doc, definition, kind, id, &old_targets)?;
    let bookkeeping = old_targets.iter().copied().filter(|handle| !old_geometry.contains(handle)).collect::<Vec<_>>();
    let requested_targets = targets.clone();
    let mut staged = doc.clone();
    let ObjectType::DynamicBlock(o) = staged.objects.get_mut(&handle).unwrap() else {
        unreachable!()
    };
    match &mut o.data {
        DynamicBlockData::StretchAction(a) => {
            let plane = parameters(doc, definition, Kind::Stretch)
                .into_iter()
                .find(|p| p.id == id)
                .ok_or("The Stretch parameter is unavailable.")?
                .end
                .z;
            stretch::validate_options(a)?;
            if let Some(corners) = frame {
                let (points, handles) = stretch::bind_frame(doc, &targets, corners, plane)?;
                a.points = points;
                a.handles = handles;
            } else {
                a.handles.retain(|h| targets.contains(&h.handle));
                let mut retained = a.clone();
                retained.action.dependencies.retain(|h| targets.contains(h));
                stretch::validate(doc, definition, &retained)?;
                let added = targets
                    .iter()
                    .copied()
                    .filter(|h| !old_targets.contains(h))
                    .collect::<Vec<_>>();
                a.handles
                    .extend(stretch::bind_existing(doc, &added, &a.points, plane)?);
                a.handles.sort_by_key(|h| h.handle);
            }
            a.action.dependencies = targets;
        }
        DynamicBlockData::PolarStretchAction(a) => {
            stretch::validate_polar(doc, definition, a)?;
            let plane = parameters(doc, definition, Kind::PolarStretch)
                .into_iter()
                .find(|p| p.id == id)
                .ok_or("The Polar Stretch parameter is unavailable.")?
                .end
                .z;
            let stretching = targets.iter().copied().filter(|handle| !a.handles.contains(handle)).collect::<Vec<_>>();
            if let Some(corners) = frame {
                let (points, bindings) = stretch::bind_frame(doc, &stretching, corners, plane)?;
                a.points = points;
                a.bindings = bindings;
            } else {
                a.bindings.retain(|b| stretching.contains(&b.handle));
                let added = targets
                    .iter()
                    .copied()
                    .filter(|h| !old_targets.contains(h) && !a.handles.contains(h))
                    .collect::<Vec<_>>();
                let frame = a.points.clone();
                a.bindings
                    .extend(stretch::bind_existing(doc, &added, &frame, plane)?);
                a.bindings.sort_by_key(|b| b.handle);
            }
            a.handles.retain(|h| targets.contains(h));
            a.action.dependencies = targets;
        }
        DynamicBlockData::MoveAction(a) => a.action.dependencies = targets,
        DynamicBlockData::FlipAction(a) => a.action.dependencies = targets,
        DynamicBlockData::ScaleAction(a) | DynamicBlockData::RotateAction(a) => {
            a.action.action.dependencies = targets
        }
        DynamicBlockData::ArrayAction(a) => a.action.dependencies = targets,
        _ => return Err("This action's selection cannot be edited yet."),
    }
    if !bookkeeping.is_empty() {
        let action = targets::action_mut(&mut o.data).unwrap();
        action.dependencies = old_targets.iter().copied().filter(|handle| bookkeeping.contains(handle) || requested_targets.contains(handle)).collect();
        for handle in requested_targets {
            if !action.dependencies.contains(&handle) { action.dependencies.push(handle); }
        }
    }
    if frame.is_some() && !needs_frame(kind) {
        return Err("Only Stretch actions have an action frame.");
    }
    if staged == *doc {
        return Ok(false);
    }
    *doc = staged;
    Ok(true)
}

pub(crate) fn actions(
    doc: &Document,
    definition: Handle,
) -> Vec<(Handle, &BlockAction, Kind, i32)> {
    let Ok(Some(graph)) = authoring::graph(doc, definition) else {
        return vec![];
    };
    let ObjectType::DynamicBlock(o) = &doc.objects[&graph] else {
        return vec![];
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        return vec![];
    };
    g.nodes
        .iter()
        .filter_map(|n| {
            let ObjectType::DynamicBlock(o) = doc.objects.get(&n.expression)? else {
                return None;
            };
            if o.owner != graph {
                return None;
            }
            match &o.data {
                DynamicBlockData::MoveAction(a) => {
                    Some((o.handle, &a.action, Kind::Move, a.connections[0].code))
                }
                DynamicBlockData::StretchAction(a) => {
                    Some((o.handle, &a.action, Kind::Stretch, a.connections[0].code))
                }
                DynamicBlockData::PolarStretchAction(a) => Some((
                    o.handle,
                    &a.action,
                    Kind::PolarStretch,
                    a.connections[0].code,
                )),
                DynamicBlockData::FlipAction(a) => {
                    Some((o.handle, &a.action, Kind::Flip, a.connections[0].code))
                }
                DynamicBlockData::RotateAction(a) => a
                    .connections
                    .first()
                    .map(|c| (o.handle, &a.action.action, Kind::Rotate, c.code)),
                DynamicBlockData::ScaleAction(a) => a
                    .connections
                    .first()
                    .map(|c| (o.handle, &a.action.action, Kind::Scale, c.code)),
                DynamicBlockData::ArrayAction(a) => a
                    .connections
                    .first()
                    .map(|c| (o.handle, &a.action, Kind::Array, c.code)),
                _ => None,
            }
        })
        .collect()
}
/// Drop erased objects from action targets without replacing graph nodes.
pub(crate) fn prune_erased_targets(doc: &mut Document, definition: Handle) {
    let mut live = authoring::members(doc, definition)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if let Ok(Some(graph)) = authoring::graph(doc, definition) {
        live.extend(doc.objects.iter().filter_map(|(handle, object)| match object {
            ObjectType::DynamicBlock(object) if object.owner == graph => Some(*handle),
            _ => None,
        }));
    }
    let handles = actions(doc, definition)
        .into_iter()
        .map(|(h, _, _, _)| h)
        .collect::<Vec<_>>();
    for h in handles {
        let Some(ObjectType::DynamicBlock(o)) = doc.objects.get_mut(&h) else {
            continue;
        };
        let action = match &mut o.data {
            DynamicBlockData::MoveAction(a) => &mut a.action,
            DynamicBlockData::FlipAction(a) => &mut a.action,
            DynamicBlockData::ArrayAction(a) => &mut a.action,
            DynamicBlockData::StretchAction(a) => {
                a.handles.retain(|b| live.contains(&b.handle));
                &mut a.action
            }
            DynamicBlockData::PolarStretchAction(a) => {
                a.handles.retain(|h| live.contains(h));
                a.bindings.retain(|b| live.contains(&b.handle));
                &mut a.action
            }
            DynamicBlockData::ScaleAction(a) | DynamicBlockData::RotateAction(a) => {
                &mut a.action.action
            }
            _ => continue,
        };
        action.dependencies.retain(|h| live.contains(h));
    }
}

pub(crate) fn frame_outline(doc: &Document, handle: Handle) -> Vec<Vector3> {
    let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&handle) else {
        return vec![];
    };
    let DynamicBlockData::StretchAction(a) = &o.data else {
        if let DynamicBlockData::PolarStretchAction(a) = &o.data {
            return stretch::outline(&a.points, a.action.display_location.z);
        }
        return vec![];
    };
    stretch::outline(&a.points, a.action.display_location.z)
}

pub(crate) fn reframe(doc: &mut Document, definition: Handle, t: &Transform) -> usize {
    let handles = actions(doc, definition)
        .iter()
        .map(|(h, _, _, _)| *h)
        .collect::<Vec<_>>();
    for h in &handles {
        let ObjectType::DynamicBlock(o) = doc.objects.get_mut(h).unwrap() else {
            unreachable!()
        };
        match &mut o.data {
            DynamicBlockData::MoveAction(a) => {
                a.action.display_location = t.apply(a.action.display_location)
            }
            DynamicBlockData::ArrayAction(a) => {
                a.action.display_location = t.apply(a.action.display_location)
            }
            DynamicBlockData::StretchAction(a) => {
                let plane = a.action.display_location.z;
                a.action.display_location = t.apply(a.action.display_location);
                a.points = stretch::outline(&a.points, plane)
                    .into_iter()
                    .map(|p| {
                        let p = t.apply(p);
                        codec::types::Vector2::new(p.x, p.y)
                    })
                    .collect();
            }
            DynamicBlockData::PolarStretchAction(a) => {
                let plane = a.action.display_location.z;
                a.action.display_location = t.apply(a.action.display_location);
                a.points = stretch::outline(&a.points, plane)
                    .into_iter()
                    .map(|p| {
                        let p = t.apply(p);
                        codec::types::Vector2::new(p.x, p.y)
                    })
                    .collect();
            }
            DynamicBlockData::FlipAction(a) => {
                a.action.display_location = t.apply(a.action.display_location)
            }

            DynamicBlockData::ScaleAction(a) | DynamicBlockData::RotateAction(a) => {
                a.action.action.display_location = t.apply(a.action.action.display_location);
                a.action.base_point = t.apply(a.action.base_point);
                a.action.offset = t.apply_rotation(a.action.offset);
            }
            _ => {}
        }
    }
    handles.len()
}

/// Keep scratch parameter output in the same coordinate frame as its definition.
pub(crate) fn reframe_value(eval: &mut codec::objects::BlockEvalExpression, plane: f64, transform: &Transform) {
    if let BlockEvalValue::Point([x, y]) = eval.value {
        if matches!(eval.value_code, 10 | 11) {
            let point = transform.apply(Vector3::new(x, y, plane));
            eval.value = BlockEvalValue::Point([point.x, point.y]);
        }
    }
}

pub(crate) fn current(doc: &Document, p: &Parameter) -> DVec3 {
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&p.handle) else {
        return p.end;
    };
    let eval = match &object.data {
        DynamicBlockData::PointParameter(p) => &p.parameter.parameter.element.eval,
        DynamicBlockData::LinearParameter(p) => &p.parameter.parameter.element.eval,
        DynamicBlockData::PolarParameter(p) => &p.parameter.parameter.element.eval,
        DynamicBlockData::XYParameter(p) => &p.parameter.parameter.element.eval,
        DynamicBlockData::RotationParameter(p) => &p.parameter.parameter.element.eval,
        _ => return p.end,
    };
    match eval.value {
        BlockEvalValue::Point([x, y]) if matches!(eval.value_code, 10 | 11) => DVec3::new(x, y, p.end.z),
        _ => p.end,
    }
}

/// Visibility can give the test INSERT an anonymous representation. Keep its
/// parallel members in sync, retaining their own handles and visibility flags.
pub(crate) fn sync_representation(
    doc: &mut Document,
    definition: Handle,
    name: &str,
) -> Result<(), &'static str> {
    let original = doc
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .ok_or("The test definition is unavailable.")?;
    if original.name == name {
        return Ok(());
    }
    let source = original.entity_handles.clone();
    let target = doc
        .block_records
        .get(name)
        .ok_or("The test representation is unavailable.")?
        .entity_handles
        .clone();
    let has_array = actions(doc, definition)
        .iter()
        .any(|(_, _, k, _)| *k == Kind::Array);
    if source.len() == target.len() && !has_array {
        let affected = actions(doc, definition)
            .iter()
            .flat_map(|(_, a, _, _)| a.dependencies.iter().copied())
            .collect::<std::collections::BTreeSet<_>>();
        for (from, to) in source.into_iter().zip(target) {
            if !affected.contains(&from) {
                continue;
            }
            let mut entity = doc
                .get_entity(from)
                .cloned()
                .ok_or("An action target is unavailable.")?;
            let current = doc
                .get_entity_mut(to)
                .ok_or("An action representation target is unavailable.")?;
            *entity.common_mut() = current.common().clone();
            *current = entity;
        }
        return Ok(());
    }
    let evaluated = doc.clone();
    let mut scene = crate::scene::Scene::new();
    scene.document = doc.clone();
    representation::reconcile(&mut scene, definition, name, &evaluated)?;
    *doc = scene.document;
    Ok(())
}

/// Re-evaluate from the scratch document's immutable starting geometry. This
/// avoids incremental drift and makes repeated changes/order independent.
#[cfg(test)]
pub(crate) fn evaluate(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
    parameter: Handle,
    position: DVec3,
) -> Result<bool, &'static str> {
    evaluate_value(doc, baseline, definition, parameter, Value::Point(position))
}

#[cfg(test)]
pub(crate) fn evaluate_value(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
    parameter: Handle,
    input: Value,
) -> Result<bool, &'static str> {
    evaluate_impl(doc, baseline, definition, parameter, input, Evaluation::default())
}

pub(crate) fn rebuild(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
) -> Result<(), &'static str> {
    let Some(p) = all_parameters(baseline, definition).into_iter().next() else {
        return Ok(());
    };
    let input = value(doc, &p)?;
    evaluate_impl(doc, baseline, definition, p.handle, input, Evaluation { rebuild: true, ..Evaluation::default() })?;
    Ok(())
}

#[derive(Default)]
struct Evaluation<'a> {
    rebuild: bool,
    bases: Option<&'a std::collections::BTreeMap<Handle, DVec3>>,
    base_changed: bool,
}

pub(crate) fn evaluate_framed(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
    parameter: Handle,
    input: Value,
    frames: &std::collections::BTreeMap<Handle, instance::ParameterFrame>,
    base_changed: bool,
) -> Result<bool, &'static str> {
    let bases = frames.iter().map(|(handle, frame)| (*handle, frame.base)).collect();
    evaluate_impl(doc, baseline, definition, parameter, input,
        Evaluation { bases: Some(&bases), base_changed, rebuild: false })
}

pub(crate) fn rebuild_framed(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
    frames: &std::collections::BTreeMap<Handle, instance::ParameterFrame>,
) -> Result<(), &'static str> {
    let Some(p) = all_parameters(baseline, definition).into_iter().next() else { return Ok(()) };
    let bases = frames.iter().map(|(handle, frame)| (*handle, frame.base)).collect();
    evaluate_impl(doc, baseline, definition, p.handle, value(doc, &p)?,
        Evaluation { bases: Some(&bases), rebuild: true, base_changed: false })?;
    Ok(())
}

fn evaluate_impl(
    doc: &mut Document,
    baseline: &Document,
    definition: Handle,
    parameter: Handle,
    input: Value,
    evaluation: Evaluation<'_>,
) -> Result<bool, &'static str> {
    let force = evaluation.rebuild;
    let base_for = |p: &Parameter| evaluation.bases.and_then(|bases| bases.get(&p.handle)).copied().unwrap_or(p.base);
    if authoring::graph(baseline, definition)?.is_none() {
        return Err("The block authoring graph is unavailable.");
    }
    let mut p = all_parameters(baseline, definition)
        .into_iter()
        .find(|p| p.handle == parameter)
        .ok_or("The block parameter is unavailable.")?;
    let base = base_for(&p);
    if !base.is_finite() || (base.z - p.base.z).abs() > 1e-8 { return Err("Specify a finite parameter base in the definition plane."); }
    p.end += base - p.base;
    p.base = base;
    let input = match input {
        Value::Frame { .. } => return Err("Evaluate complete parameter frames in a reference context."),
        Value::Flip(_) if !p.flip => return Err("Select a Flip parameter."),
        Value::Point(_) if p.flip => return Err("Use the Flip grip to toggle its state."),
        Value::Point(mut position) => {
            if !position.is_finite() {
                return Err("Specify a finite parameter position.");
            }
            position.z = p.end.z;
            if !force {
                position = constrain_parameter_position(baseline, &p, position)?;
            }
            if p.linear {
                let axis = (p.end - p.base).normalize();
                let length = (position - p.base).dot(axis);
                if length < 1e-9 {
                    return Err("Specify a positive parameter length.");
                }
                let mut snapped_length = length;
                if !force {
                    if let Some(table) = super::block_properties_table::table(baseline, definition)?
                    {
                        if table.must_match
                            && table.columns.iter().any(|column| {
                                column.handle == p.handle
                                    && column
                                        .connection_name
                                        .eq_ignore_ascii_case("UpdatedDistance")
                            })
                        {
                            let values = table
                                .columns
                                .iter()
                                .position(|column| {
                                    column.handle == p.handle
                                        && column
                                            .connection_name
                                            .eq_ignore_ascii_case("UpdatedDistance")
                                })
                                .map(|i| {
                                    table
                                        .rows
                                        .iter()
                                        .filter_map(|r| r.get(i).copied())
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            if let Some(nearest) = values.iter().copied().min_by(|a, b| {
                                (a - length)
                                    .abs()
                                    .total_cmp(&(b - length).abs())
                                    .then_with(|| a.total_cmp(b))
                            }) {
                                snapped_length = nearest;
                            }
                        }
                    }
                }
                position = p.base + axis * snapped_length;
            }
            if p.rotation {
                let delta = position - p.base;
                if delta.length() < 1e-9 {
                    return Err("Specify a rotation direction away from the parameter base.");
                }
                let mut angle = delta.y.atan2(delta.x);
                if !force {
                    if let Some(table) = super::block_properties_table::table(baseline, definition)?
                    {
                        if table.must_match {
                            if let Some(col) = table.columns.iter().position(|column| {
                                column.handle == p.handle
                                    && column.connection_name.eq_ignore_ascii_case("UpdatedAngle")
                            }) {
                                if let Some(nearest) = table
                                    .rows
                                    .iter()
                                    .filter_map(|row| row.get(col).copied())
                                    .min_by(|a, b| {
                                        angle_distance(*a, angle)
                                            .total_cmp(&angle_distance(*b, angle))
                                            .then_with(|| a.total_cmp(b))
                                    })
                                {
                                    angle = nearest;
                                }
                            }
                        }
                    }
                }
                position =
                    p.base + DVec3::new(angle.cos(), angle.sin(), 0.0) * p.end.distance(p.base);
            }
            Value::Point(position)
        }
        value => value,
    };
    let previous = value(doc, &p)?;
    if !force && !evaluation.base_changed
        && match (previous, input) {
            (Value::Flip(a), Value::Flip(b)) => a == b,
            (Value::Point(a), Value::Point(b)) => a.distance(b) < 1e-10,
            _ => false,
        }
    {
        return Ok(false);
    }
    let mut staged = doc.clone();
    set_value(&mut staged, &p, input)?;
    let baseline_members = authoring::members(baseline, definition)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let extra_handles = staged
        .entities()
        .filter(|e| {
            e.common().owner_handle == definition && !baseline_members.contains(&e.common().handle)
        })
        .map(|e| e.common().handle)
        .collect::<Vec<_>>();
    for h in extra_handles {
        staged.remove_entity(h);
    }
    if let Some(record) = staged
        .block_records
        .iter_mut()
        .find(|b| b.handle == definition)
    {
        record.entity_handles.retain(|h| {
            baseline_members.contains(h)
                || *h == record.block_entity_handle
                || *h == record.block_end_handle
        });
    }
    let all = actions(baseline, definition);
    let mut affected = std::collections::BTreeSet::new();
    let mut geometric_targets = std::collections::BTreeMap::new();
    let mut prepared = std::collections::BTreeMap::new();
    let mut array_plans = std::collections::BTreeMap::new();
    let mut remaining_array_entities = array::MAX_GENERATED_ENTITIES;
    for (action_handle, a, kind, id) in &all {
        if *kind == Kind::Stretch {
            let ObjectType::DynamicBlock(o) = &baseline.objects[action_handle] else {
                unreachable!()
            };
            let DynamicBlockData::StretchAction(s) = &o.data else {
                unreachable!()
            };
            stretch::validate(baseline, definition, s)?;
        } else if *kind == Kind::PolarStretch {
            let ObjectType::DynamicBlock(o) = &baseline.objects[action_handle] else {
                unreachable!()
            };
            let DynamicBlockData::PolarStretchAction(s) = &o.data else {
                unreachable!()
            };
            stretch::validate_polar(baseline, definition, s)?;
        }
        let geometric = targets::geometry(baseline, definition, *kind, *id, &a.dependencies)?;
        affected.extend(geometric.iter().copied());
        let target_count = geometric.len();
        geometric_targets.insert(*action_handle, geometric);
        if *kind == Kind::Array {
            let ObjectType::DynamicBlock(object) = &baseline.objects[action_handle] else { unreachable!() };
            let DynamicBlockData::ArrayAction(arr) = &object.data else { unreachable!() };
            let q = parameters(baseline, definition, Kind::Array)
                .into_iter().find(|p| p.id == *id)
                .ok_or("An action has an unsupported parameter binding.")?;
            if !supported_parameter(baseline, q.handle) {
                return Err("This parameter uses value constraints or chaining not supported yet.");
            }
            if arr.column_offset <= 1e-6 || !arr.column_offset.is_finite()
                || (q.xy && (arr.row_offset <= 1e-6 || !arr.row_offset.is_finite()))
                || grip_roles::scalar_mask(&q, &object.data) == Some(0)
            {
                return Err("This Array action uses options not supported yet.");
            }
            if !a.action_ids.is_empty() {
                return Err("Chained actions are not supported yet.");
            }
            array_plans.insert(*action_handle, array::Plan::new(
                &q, base_for(&q), current(&staged, &q), arr, target_count,
                &mut remaining_array_entities,
            )?);
            continue;
        }
        if a.dependencies.is_empty() { continue; }
        let q = parameters(baseline, definition, *kind)
            .into_iter()
            .find(|p| p.id == *id)
            .ok_or("An action has an unsupported parameter binding.")?;
        if !supported_parameter(baseline, q.handle) {
            return Err("This parameter uses value constraints or chaining not supported yet.");
        }
        let end = current(&staged, &q);
        let base = base_for(&q);
        let ObjectType::DynamicBlock(action_object) = &baseline.objects[action_handle] else {
            unreachable!()
        };
        let move_translation = match &action_object.data {
            DynamicBlockData::MoveAction(move_action) => {
                Some(action_move_delta(&q, base, end, &move_action.connections)
                    .map(|delta| options::displacement(&move_action.offsets, delta)).transpose()?)
            }
            _ => None,
        };
        let transform = match kind {
            Kind::Move => crate::command::EntityTransform::Translate(
                move_translation
                    .flatten()
                    .ok_or("This Move action uses unsupported parameter connections.")?,
            ),
            Kind::Stretch => {
                let DynamicBlockData::StretchAction(action) = &action_object.data else { unreachable!() };
                let delta = action_move_delta(&q, base, end, &action.connections)
                    .ok_or("This Stretch action uses unsupported parameter connections.")?;
                crate::command::EntityTransform::Translate(options::displacement(&action.offsets, delta)?)
            }
            Kind::PolarStretch => crate::command::EntityTransform::Translate(end - q.end),
            Kind::Rotate => {
                let a = end - base;
                let b = q.end - q.base;
                crate::command::EntityTransform::Rotate {
                    center: options::pivot(options::base(baseline, *action_handle).ok_or("The Rotate action base is unavailable.")?, q.base)?,
                    axis: DVec3::Z,
                    angle_rad: a.y.atan2(a.x) - b.y.atan2(b.x),
                }
            }
            Kind::Flip => {
                if value(&staged, &q)? == Value::Flip(false) {
                    crate::command::EntityTransform::Translate(DVec3::ZERO)
                } else {
                    crate::command::EntityTransform::Mirror {
                        p1: q.base,
                        p2: q.end,
                        working_normal: DVec3::Z,
                    }
                }
            }
            Kind::Scale => {
                let factor = end.distance(base) / q.end.distance(q.base);
                if !factor.is_finite() || factor < 1e-9 {
                    return Err("Specify a positive scale distance.");
                }
                crate::command::EntityTransform::Scale {
                    center: options::pivot(options::base(baseline, *action_handle).ok_or("The Scale action base is unavailable.")?, q.base)?,
                    factor,
                }
            }
            Kind::Array => unreachable!(),
        };
        // Do not silently ignore retained native action chains.
        if !a.action_ids.is_empty() {
            return Err("Chained actions are not supported yet.");
        }
        let o = action_object;
        match &o.data {
            DynamicBlockData::MoveAction(m)
                if action_move_delta(&q, base, end, &m.connections).is_none() =>
            {
                return Err("This Move action uses options not supported yet.");
            }
            DynamicBlockData::FlipAction(_)
            | DynamicBlockData::RotateAction(_)
            | DynamicBlockData::ScaleAction(_)
                if grip_roles::scalar_mask(&q, &o.data) == Some(0) =>
            {
                return Err(match &o.data {
                    DynamicBlockData::FlipAction(_) => "This Flip action uses options not supported yet.",
                    DynamicBlockData::RotateAction(_) => "This Rotate action uses options not supported yet.",
                    _ => "This Scale action uses options not supported yet.",
                });
            }
            _ => {}
        }
        if *kind == Kind::PolarStretch {
            let (old_length, new_length) = (q.end.distance(q.base), end.distance(base));
            if !old_length.is_finite() || !new_length.is_finite() || old_length <= 1e-9 || new_length <= 1e-9 {
                return Err("The Polar parameter needs a nonzero distance.");
            }
        }
        prepared.insert(*action_handle, (q, base, end, transform));
    }
    for h in affected {
        let mut entity = baseline
            .get_entity(h)
            .cloned()
            .ok_or("An action target is unavailable.")?;
        if !supported(&entity) {
            return Err("This action contains an unsupported object type.");
        }
        // State flags may have changed through the existing visibility grip.
        if let Some(e) = staged.get_entity(h) {
            entity.common_mut().invisible = e.common().invisible;
        }
        for (action_handle, a, kind, _id) in &all {
            if !a.dependencies.contains(&h) || *kind == Kind::Array {
                continue;
            }
            let (q, base, end, transform) = prepared.get(action_handle).unwrap();
            let (base, end) = (*base, *end);
            let ObjectType::DynamicBlock(o) = &baseline.objects[action_handle] else { unreachable!() };
            if let DynamicBlockData::StretchAction(s) = &o.data {
                let crate::command::EntityTransform::Translate(delta) = transform else { unreachable!() };
                stretch::apply(&mut entity, s, *delta)?;
            } else if let DynamicBlockData::PolarStretchAction(s) = &o.data {
                if s.connections.iter().any(|c| c.code != q.id) {
                    return Err("This Polar Stretch action uses a different parameter binding.");
                }
                let direction = q.end - q.base;
                if direction.length() <= 1e-9 {
                    return Err("The Polar parameter needs a nonzero distance.");
                }
                stretch::apply_polar_frame(&mut entity, s, q.base, q.end, base, end)?;
            } else {
                super::view::dispatch::apply_transform(&mut entity, transform);
            }
            if matches!(kind, Kind::Rotate | Kind::Scale)
                && options::base(baseline, *action_handle).is_some_and(|action| action.dependent)
            {
                super::view::dispatch::apply_transform(&mut entity,
                    &crate::command::EntityTransform::Translate(base - q.base));
            }
            let bounds = entity.as_entity().bounding_box();
            if !v(bounds.min).is_finite() || !v(bounds.max).is_finite() {
                return Err("The parameter value produces non-finite geometry.");
            }
        }
        *staged
            .get_entity_mut(h)
            .ok_or("An action target was removed.")? = entity;
    }
    for (action_handle, _, kind, _) in &all {
        if *kind != Kind::Array {
            continue;
        }
        for (k, disp) in array_plans[action_handle].displacements() {
            for target_h in &geometric_targets[action_handle] {
                let mut clone = staged
                    .get_entity(*target_h)
                    .cloned()
                    .ok_or("An action target is unavailable.")?;
                super::view::dispatch::apply_transform(
                    &mut clone,
                    &crate::command::EntityTransform::Translate(disp),
                );
                let bounds = clone.as_entity().bounding_box();
                if !v(bounds.min).is_finite() || !v(bounds.max).is_finite() {
                    return Err("The parameter value produces non-finite geometry.");
                }
                let new_h = staged.allocate_handle();
                clone.common_mut().handle = new_h;
                clone.common_mut().owner_handle = definition;
                clone.common_mut().raw_record = None;
                clone.common_mut().xdictionary_handle = None;
                clone.common_mut().reactors.clear();
                crate::scene::Scene::reset_clone_subhandles(&mut staged, &mut clone);
                let base_id = authoring::element_id(&clone);
                staged
                    .add_entity(clone)
                    .map_err(|_| "Could not add array item.")?;
                if let Some(base_id) = base_id {
                    let clone_id = base_id.wrapping_add((k as i32).wrapping_mul(100_000));
                    authoring::set_entity_tag(
                        &mut staged,
                        new_h,
                        authoring::element_tag(clone_id, new_h),
                    );
                }
            }
        }
    }
    *doc = staged;
    Ok(true)
}

fn supported_parameter(doc: &Document, handle: Handle) -> bool {
    let Some(ObjectType::DynamicBlock(o)) = doc.objects.get(&handle) else {
        return false;
    };
    let valid_value_set = |s: &codec::objects::BlockParameterValueSet| {
        s.flags & !0x0f == 0
            && !(s.flags & 4 != 0 && s.flags & 8 != 0)
            && s.minimum.is_finite()
            && s.maximum.is_finite()
            && s.increment.is_finite()
            && (s.flags & 4 == 0 || s.increment > 0.)
            && (s.flags & 8 == 0 || !s.values.is_empty())
            && s.values.iter().all(|v| v.is_finite())
            && !s.values.windows(2).any(|pair| pair[0] >= pair[1])
            && !(s.flags & 3 == 3 && s.minimum > s.maximum)
    };
    match &o.data {
        DynamicBlockData::FlipParameter(p) => !p.parameter.parameter.chain_actions,
        DynamicBlockData::RotationParameter(p) => {
            !p.parameter.parameter.chain_actions && valid_value_set(&p.value_set)
        }
        DynamicBlockData::PointParameter(p) => !p.parameter.parameter.chain_actions,
        DynamicBlockData::LinearParameter(p) => {
            !p.parameter.parameter.chain_actions && valid_value_set(&p.value_set)
        }
        DynamicBlockData::XYParameter(p) => {
            !p.parameter.parameter.chain_actions
                && valid_value_set(&p.x_value_set)
                && valid_value_set(&p.y_value_set)
        }
        DynamicBlockData::PolarParameter(p) => {
            !p.parameter.parameter.chain_actions
                && valid_value_set(&p.distance_value_set)
                && valid_value_set(&p.angle_value_set)
        }
        _ => false,
    }
}

pub(crate) fn constrain_value_set(value: f64, set: &codec::objects::BlockParameterValueSet) -> f64 {
    let minimum = (set.flags & 1 != 0).then_some(set.minimum);
    let maximum = (set.flags & 2 != 0).then_some(set.maximum);
    let clamp = |value: f64| {
        value
            .max(minimum.unwrap_or(f64::NEG_INFINITY))
            .min(maximum.unwrap_or(f64::INFINITY))
    };
    let value = clamp(value);
    if set.flags & 8 != 0 {
        return set
            .values
            .iter()
            .copied()
            .filter(|candidate| {
                minimum.is_none_or(|min| *candidate >= min)
                    && maximum.is_none_or(|max| *candidate <= max)
            })
            .min_by(|a, b| {
                (a - value)
                    .abs()
                    .total_cmp(&(b - value).abs())
                    .then_with(|| a.total_cmp(b))
            })
            .unwrap_or(value);
    }
    if set.flags & 4 != 0 {
        let origin = minimum.unwrap_or(0.0);
        let steps = ((value - origin) / set.increment).round();
        return clamp(origin + steps * set.increment);
    }
    value
}

pub(crate) fn constrain_angle(value: f64, set: &codec::objects::BlockParameterValueSet) -> f64 {
    if set.flags == 0 {
        return value;
    }
    let origin = if set.flags & 1 != 0 { set.minimum } else { 0.0 };
    let angle = origin + (value - origin).rem_euclid(std::f64::consts::TAU);
    if set.flags & 8 != 0 {
        return set
            .values
            .iter()
            .copied()
            .filter(|candidate| {
                (set.flags & 1 == 0 || *candidate >= set.minimum)
                    && (set.flags & 2 == 0 || *candidate <= set.maximum)
            })
            .min_by(|a, b| {
                angle_distance(*a, angle)
                    .total_cmp(&angle_distance(*b, angle))
                    .then_with(|| a.total_cmp(b))
            })
            .unwrap_or(angle);
    }
    constrain_value_set(angle, set)
}

pub(super) fn constrain_parameter_position(
    doc: &Document,
    p: &Parameter,
    mut position: DVec3,
) -> Result<DVec3, &'static str> {
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&p.handle) else {
        return Ok(position);
    };
    match &object.data {
        DynamicBlockData::LinearParameter(parameter) => {
            let axis = (p.end - p.base).normalize();
            let length = (position - p.base).dot(axis);
            let length = constrain_value_set(length, &parameter.value_set);
            position = p.base + axis * length;
        }
        DynamicBlockData::PolarParameter(parameter) => {
            let delta = position - p.base;
            let length = delta.length();
            let distance = constrain_value_set(length, &parameter.distance_value_set);
            let input_angle = delta.y.atan2(delta.x);
            let angle = constrain_angle(input_angle, &parameter.angle_value_set);
            if angle_distance(angle, input_angle) > 1e-14 {
                position = p.base + DVec3::new(angle.cos(), angle.sin(), 0.0) * distance;
            } else if distance != length && length > 1e-12 {
                position = p.base + delta * (distance / length);
            }
        }
        DynamicBlockData::XYParameter(parameter) => {
            let delta = position - p.base;
            position = p.base
                + DVec3::new(
                    constrain_value_set(delta.x, &parameter.x_value_set),
                    constrain_value_set(delta.y, &parameter.y_value_set),
                    delta.z,
                );
        }
        DynamicBlockData::RotationParameter(parameter) => {
            let delta = position - p.base;
            let angle = constrain_angle(delta.y.atan2(delta.x), &parameter.value_set);
            position = p.base + DVec3::new(angle.cos(), angle.sin(), 0.0) * delta.length();
        }
        _ => {}
    }
    Ok(position)
}
