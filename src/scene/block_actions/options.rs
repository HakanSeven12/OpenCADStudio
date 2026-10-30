//! Native displacement overrides and Polar Stretch rotation selections.
use super::*;

pub(crate) fn offsets(doc: &Document, action: Handle) -> Option<BlockActionOffsets> {
    let ObjectType::DynamicBlock(object) = doc.objects.get(&action)? else {
        return None;
    };
    match &object.data {
        DynamicBlockData::MoveAction(action) => Some(action.offsets.clone()),
        DynamicBlockData::StretchAction(action) => Some(action.offsets.clone()),
        DynamicBlockData::PolarStretchAction(action) => Some(BlockActionOffsets {
            distance_multiplier: action.distance_multiplier,
            angle_offset: action.angle_offset,
            flags: 0,
        }),
        _ => None,
    }
}

pub(crate) fn validate(offsets: &BlockActionOffsets) -> Result<(), &'static str> {
    if !offsets.distance_multiplier.is_finite() || !offsets.angle_offset.is_finite() {
        return Err("Action overrides must be finite numbers.");
    }
    if offsets.flags != 0 {
        return Err("This action uses an unsupported displacement mode.");
    }
    Ok(())
}

pub(crate) fn displacement(
    offsets: &BlockActionOffsets,
    delta: DVec3,
) -> Result<DVec3, &'static str> {
    validate(offsets)?;
    let (sin, cos) = offsets.angle_offset.sin_cos();
    let delta = DVec3::new(
        cos * delta.x - sin * delta.y,
        sin * delta.x + cos * delta.y,
        delta.z,
    ) * offsets.distance_multiplier;
    delta
        .is_finite()
        .then_some(delta)
        .ok_or("Action overrides produce non-finite geometry.")
}

pub(crate) fn set(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    multiplier: f64,
    angle: f64,
) -> Result<bool, &'static str> {
    if !actions(doc, definition)
        .iter()
        .any(|(handle, ..)| *handle == action)
    {
        return Err("Select an action belonging to the active block definition.");
    }
    let before = offsets(doc, action).ok_or("This action has no displacement overrides.")?;
    let updated = BlockActionOffsets {
        distance_multiplier: multiplier,
        angle_offset: angle,
        flags: before.flags,
    };
    validate(&updated)?;
    if before == updated {
        return Ok(false);
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&action) else {
        unreachable!()
    };
    match &mut object.data {
        DynamicBlockData::MoveAction(action) => action.offsets = updated,
        DynamicBlockData::StretchAction(action) => action.offsets = updated,
        DynamicBlockData::PolarStretchAction(action) => {
            action.distance_multiplier = multiplier;
            action.angle_offset = angle;
        }
        _ => unreachable!(),
    }
    Ok(true)
}

pub(crate) fn set_rotate_only(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    targets: &[Handle],
) -> Result<bool, &'static str> {
    if !actions(doc, definition)
        .iter()
        .any(|(handle, _, kind, _)| *handle == action && *kind == Kind::PolarStretch)
    {
        return Err("Select a Polar Stretch action belonging to the active block definition.");
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&action) else {
        unreachable!()
    };
    let DynamicBlockData::PolarStretchAction(native) = &object.data else {
        unreachable!()
    };
    super::stretch::validate_polar(doc, definition, native)?;
    let geometric = super::targets::geometry(doc, definition, Kind::PolarStretch, native.connections[0].code, &native.action.dependencies)?;
    let mut targets = targets.to_vec();
    targets.sort();
    targets.dedup();
    if targets
        .iter()
        .any(|handle| !geometric.contains(handle))
    {
        return Err("Select rotate-only objects from this action's selection set.");
    }
    let mut before = native.handles.clone();
    before.sort();
    if targets == before {
        return Ok(false);
    }
    let rebound = before
        .iter()
        .copied()
        .filter(|handle| !targets.contains(handle))
        .collect::<Vec<_>>();
    let plane = native.action.display_location.z;
    let added = super::stretch::bind_existing(doc, &rebound, &native.points, plane)?;
    let mut staged = native.clone();
    staged.handles = targets;
    staged.bindings.retain(|binding| {
        !staged.handles.contains(&binding.handle) && !rebound.contains(&binding.handle)
    });
    staged.bindings.extend(added);
    staged.bindings.sort_by_key(|binding| binding.handle);
    super::stretch::validate_polar(doc, definition, &staged)?;
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&action) else {
        unreachable!()
    };
    object.data = DynamicBlockData::PolarStretchAction(staged);
    Ok(true)
}

/// Native Rotate/Scale pivot configuration; display location is independent.
pub(crate) fn base(doc: &Document, handle: Handle) -> Option<&BlockActionWithBasePoint> {
    let ObjectType::DynamicBlock(object) = doc.objects.get(&handle)? else {
        return None;
    };
    match &object.data {
        DynamicBlockData::RotateAction(action) | DynamicBlockData::ScaleAction(action) => {
            Some(&action.action)
        }
        _ => None,
    }
}

pub(crate) fn pivot(
    base: &BlockActionWithBasePoint,
    parameter_base: DVec3,
) -> Result<DVec3, &'static str> {
    let point = if base.dependent {
        let offset = super::v(base.offset);
        if !offset.is_finite() || offset.z.abs() > 1e-8 {
            return Err("Specify a finite action base offset in the parameter plane.");
        }
        parameter_base + offset
    } else {
        DVec3::new(base.base_point.x, base.base_point.y, base.base_point.z)
    };
    if !point.is_finite() || (point.z - parameter_base.z).abs() > 1e-8 {
        return Err("Specify a finite action base point in the parameter plane.");
    }
    Ok(point)
}

pub(crate) fn set_base(
    doc: &mut Document,
    definition: Handle,
    handle: Handle,
    dependent: bool,
    point: Option<DVec3>,
) -> Result<bool, &'static str> {
    let (_, _, kind, id) = actions(doc, definition)
        .into_iter()
        .find(|(h, ..)| *h == handle)
        .ok_or("Select an action belonging to the active block definition.")?;
    let parameter = parameters(doc, definition, kind)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("This action has an unsupported parameter binding.")?;
    let before = base(doc, handle).ok_or("This action has no configurable base point.")?;
    let mut updated = before.clone();
    updated.dependent = dependent;
    let point = if point.is_none() && before.dependent && !dependent {
        Some(pivot(before, parameter.base)?)
    } else {
        point
    };
    if let Some(point) = point {
        if !point.is_finite() || (point.z - parameter.base.z).abs() > 1e-8 {
            return Err("Specify a finite action base point in the parameter plane.");
        }
        updated.base_point = Vector3::new(point.x, point.y, point.z);
    }
    pivot(&updated, parameter.base)?;
    if *before == updated {
        return Ok(false);
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&handle) else {
        unreachable!()
    };
    match &mut object.data {
        DynamicBlockData::RotateAction(action) | DynamicBlockData::ScaleAction(action) => {
            action.action = updated
        }
        _ => unreachable!(),
    }
    Ok(true)
}

pub(crate) fn set_base_offset(
    doc: &mut Document,
    definition: Handle,
    handle: Handle,
    offset: DVec3,
) -> Result<bool, &'static str> {
    let (_, _, kind, id) = actions(doc, definition)
        .into_iter()
        .find(|(h, ..)| *h == handle)
        .ok_or("Select an action belonging to the active block definition.")?;
    let parameter = parameters(doc, definition, kind)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("This action has an unsupported parameter binding.")?;
    let before = base(doc, handle).ok_or("This action has no configurable base point.")?;
    if !before.dependent {
        return Err("Base offsets require a dependent action base point.");
    }
    let mut updated = before.clone();
    updated.offset = super::native(offset);
    pivot(&updated, parameter.base)?;
    if *before == updated {
        return Ok(false);
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&handle) else {
        unreachable!()
    };
    match &mut object.data {
        DynamicBlockData::RotateAction(action) | DynamicBlockData::ScaleAction(action) => {
            action.action = updated
        }
        _ => unreachable!(),
    }
    Ok(true)
}
