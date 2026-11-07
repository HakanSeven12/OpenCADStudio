//! Saved INSERT evaluation: definition geometry stays immutable; parameter
//! values live in the native per-reference AppDataCache.
use super::*;
use document::objects::{Dictionary, XRecord, XRecordEntry, XRecordValue};

pub(super) fn entry(doc: &Document, owner: Handle, key: &str) -> Result<Option<Handle>, &'static str> {
    let Some(ObjectType::Dictionary(d)) = doc.objects.get(&owner) else {
        return Err("The reference contains an invalid parameter dictionary.");
    };
    Ok(d.entries
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, h)| *h))
}

pub(super) fn cache(doc: &Document, insert: Handle, id: i32) -> Result<Option<Handle>, &'static str> {
    let Some(mut owner) = doc.extension_dictionary_handle(insert) else {
        return Ok(None);
    };
    if doc.object_owner(owner) != Some(insert) {
        return Err("The reference parameter dictionary has an invalid owner.");
    }
    for key in [
        "AcDbBlockRepresentation",
        "AppDataCache",
        "ACAD_ENHANCEDBLOCKDATA",
    ] {
        let Some(next) = entry(doc, owner, key)? else {
            return Ok(None);
        };
        if doc.object_owner(next) != Some(owner) {
            return Err("The reference parameter dictionary has an invalid owner.");
        }
        owner = next;
    }
    let result = entry(doc, owner, &id.to_string())?;
    if result.is_some_and(|handle| doc.object_owner(handle) != Some(owner)) {
        return Err("The reference parameter record has an invalid owner.");
    }
    Ok(result)
}

pub(super) fn remove_cache(doc: &mut Document, insert: Handle, id: i32) -> Result<(), &'static str> {
    let Some(handle) = cache(doc, insert, id)? else {
        return Ok(());
    };
    let Some(ObjectType::XRecord(record)) = doc.objects.get(&handle) else {
        return Err("The reference contains an invalid parameter record.");
    };
    let owner = record.owner;
    let Some(ObjectType::Dictionary(owner)) = doc.objects.get_mut(&owner) else {
        return Err("The reference contains an invalid parameter dictionary.");
    };
    owner.entries.retain(|(_, entry)| *entry != handle);
    doc.objects.remove(&handle);
    Ok(())
}

pub(crate) fn clear_canonical_inputs(doc: &mut Document, insert: Handle) {
    crate::scene::view::dispatch::set_entity_xdata(doc, insert, super::flip_graph::INPUTS, None);
}

/// Evaluated definition-plane coordinates, independent of display grip locations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ParameterFrame {
    pub base: DVec3,
    pub end: DVec3,
}

impl ParameterFrame {
    fn validate(self, p: &Parameter) -> Result<Self, &'static str> {
        if !self.base.is_finite()
            || !self.end.is_finite()
            || (self.base.z - p.base.z).abs() > 1e-8
            || (self.end.z - p.end.z).abs() > 1e-8
            || (p.point && self.base.distance(p.base) > 1e-8)
        {
            return Err(
                "The reference parameter frame must be finite and in its definition plane.",
            );
        }
        Ok(self)
    }
}

pub(crate) fn frame(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
) -> Result<ParameterFrame, &'static str> {
    let Some(h) = cache(doc, insert, p.id)? else {
        return ParameterFrame {
            base: p.base,
            end: p.end,
        }
        .validate(p);
    };
    let Some(ObjectType::XRecord(r)) = doc.objects.get(&h) else {
        return Err("The reference contains an invalid parameter record.");
    };
    if !r.entries_complete {
        return Err("The reference parameter cache is incomplete.");
    }
    let points = r
        .entries
        .iter()
        .filter_map(|e| e.value.as_point3d())
        .map(|(x, y, z)| DVec3::new(x, y, z))
        .collect::<Vec<_>>();
    let missing = "The reference parameter cache has no finite endpoint.";
    let a = *points.first().ok_or(missing)?;
    let result = if p.point {
        ParameterFrame {
            base: p.base,
            end: a,
        }
    } else {
        let b = *points.get(1).ok_or(missing)?;
        if p.xy {
            // Native XY caches retain the Y and X corners, respectively.
            ParameterFrame {
                base: DVec3::new(a.x, b.y, a.z),
                end: DVec3::new(b.x, a.y, b.z),
            }
        } else {
            ParameterFrame { base: a, end: b }
        }
    };
    let result = result.validate(p)?;
    if p.rotation
        && points.get(3).is_none_or(|reference| {
            !reference.is_finite() || reference.distance(p.reference + result.base - p.base) > 1e-8
        })
    {
        return Err("The rotation reference point is unavailable or has been relocated.");
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) fn current(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
) -> Result<DVec3, &'static str> {
    let frame = frame(doc, insert, p)?;
    Ok(frame.end)
}

fn frame_value(p: &Parameter, frame: ParameterFrame) -> Value {
    if frame.base.distance(p.base) > 1e-10 {
        Value::Frame {
            base: frame.base,
            end: frame.end,
        }
    } else {
        Value::Point(frame.end)
    }
}

pub(crate) fn test_frame(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
) -> Result<ParameterFrame, &'static str> {
    if cache(doc, insert, p.id)?.is_some() {
        frame(doc, insert, p)
    } else {
        ParameterFrame {
            base: p.base,
            end: super::current(doc, p),
        }
        .validate(p)
    }
}

pub(crate) fn test_value(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
) -> Result<Value, &'static str> {
    if cache(doc, insert, p.id)?.is_some() {
        value(doc, insert, p)
    } else {
        super::value(doc, p)
    }
}

/// Use the reference's resolved reflection line for Flip grip presentation.
pub(crate) fn grip_parameter(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
    testing: bool,
) -> Result<Parameter, &'static str> {
    let mut result = p.clone();
    if p.flip {
        let frame = if testing { test_frame(doc, insert, p)? } else { frame(doc, insert, p)? };
        let delta = frame.base - p.base;
        result.base = frame.base;
        result.end = frame.end;
        result.reference += delta;
        result.grip_location = result.grip_location.map(|position| position + delta);
    }
    Ok(result)
}

pub(crate) fn action_grips(
    doc: &Document, baseline: &Document, definition: Handle, insert: Handle,
    parameter: &Parameter, value: Value, testing: bool,
) -> Vec<crate::scene::block_grips::runtime::Grip> {
    let mut grips = crate::scene::block_grips::runtime::action_grips(baseline, definition, parameter, value);
    if let Some(plan) = super::flip_graph::Plan::new(baseline, definition) {
        if plan.present_grips(doc, baseline, definition, insert, parameter, value, testing, &mut grips).is_err() {
            return Vec::new();
        }
    }
    grips
}

pub(crate) fn value(
    doc: &Document,
    insert: Handle,
    p: &Parameter,
) -> Result<Value, &'static str> {
    if !p.flip {
        let frame = frame(doc, insert, p)?;
        return Ok(frame_value(p, frame));
    }
    let Some(h) = cache(doc, insert, p.id)? else {
        return Ok(Value::Flip(false));
    };
    let Some(ObjectType::XRecord(r)) = doc.objects.get(&h) else {
        return Err("Invalid Flip parameter record.");
    };
    if !r.entries_complete {
        return Err("The Flip parameter cache is incomplete.");
    }
    let points = r
        .entries
        .iter()
        .filter_map(|e| e.value.as_point3d())
        .map(|(x, y, z)| DVec3::new(x, y, z))
        .collect::<Vec<_>>();
    if points.len() < 2
        || !points[0].is_finite()
        || !points[1].is_finite()
        || (points[0].z - p.base.z).abs() > 1e-8
        || (points[1].z - p.end.z).abs() > 1e-8
    {
        return Err("This reference uses an unsupported Flip reflection line.");
    }
    let state = r.entries.iter().skip(4).rev().find_map(|e| match e.value {
        XRecordValue::Int16(s) => Some(s),
        _ => None,
    });
    match state {
        Some(0) => Ok(Value::Flip(false)),
        Some(1) => Ok(Value::Flip(true)),
        _ => Ok(Value::Flip(false)),
    }
}

pub(crate) fn ensure_dictionary(
    doc: &mut Document,
    owner: Handle,
    key: &str,
) -> Result<Handle, &'static str> {
    if let Some(h) = entry(doc, owner, key)? {
        if matches!(doc.objects.get(&h), Some(ObjectType::Dictionary(_)))
            && doc.object_owner(h) == Some(owner)
        {
            return Ok(h);
        }
        return Err("The reference contains an invalid parameter dictionary.");
    }
    let h = doc.allocate_handle();
    let mut d = Dictionary::new();
    d.handle = h;
    d.owner = owner;
    d.reactors.push(owner);
    doc.objects.insert(h, ObjectType::Dictionary(d));
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
        d.add_entry(key, h);
    }
    Ok(h)
}

pub(crate) fn save(
    doc: &mut Document,
    insert: Handle,
    p: &Parameter,
    value: Value,
) -> Result<(), &'static str> {
    if let Value::Frame { base, end } = value {
        return save_frame(doc, insert, p, ParameterFrame { base, end });
    }
    if let Value::Point(end) = value {
        if p.flip {
            return Err("Use the Flip state for this parameter.");
        }
        let mut frame = frame(doc, insert, p)?;
        frame.end = end;
        return save_frame(doc, insert, p, frame);
    }
    if !p.flip {
        return Err("Select a Flip parameter.");
    }
    let Value::Flip(state) = value else {
        unreachable!()
    };
    let mut staged = doc.clone();
    let h = ensure_cache(&mut staged, insert, p)?;
    let Some(ObjectType::XRecord(r)) = staged.objects.get_mut(&h) else {
        return Err("Invalid parameter record.");
    };
    if !r.entries_complete {
        return Err("The reference parameter cache is incomplete.");
    }
    let entry = r
        .entries
        .iter_mut()
        .skip(4)
        .rev()
        .find(|e| matches!(e.value, XRecordValue::Int16(_)))
        .ok_or("The Flip cache has no state.")?;
    entry.value = XRecordValue::Int16(i16::from(state));
    r.raw_data.clear();
    r.raw_dwg_data = None;
    *doc = staged;
    Ok(())
}

/// Persist both endpoints without changing the parameter or any display grip.
pub(crate) fn save_frame(
    doc: &mut Document,
    insert: Handle,
    p: &Parameter,
    frame: ParameterFrame,
) -> Result<(), &'static str> {
    let frame = frame.validate(p)?;
    // Validate existing metadata before allocating or modifying owned records.
    self::frame(doc, insert, p)?;
    let mut staged = doc.clone();
    let h = ensure_cache(&mut staged, insert, p)?;
    let Some(ObjectType::XRecord(r)) = staged.objects.get_mut(&h) else {
        return Err("Invalid parameter record.");
    };
    let mut points = r
        .entries
        .iter_mut()
        .filter(|e| matches!(e.value, XRecordValue::Point3D(..)));
    let mut put = |point: DVec3| -> Result<(), &'static str> {
        let entry = points
            .next()
            .ok_or("The reference parameter cache has no endpoint.")?;
        entry.value = XRecordValue::Point3D(point.x, point.y, point.z);
        Ok(())
    };
    if p.point {
        put(frame.end)?;
    } else if p.xy {
        put(DVec3::new(frame.base.x, frame.end.y, frame.base.z))?;
        put(DVec3::new(frame.end.x, frame.base.y, frame.end.z))?;
    } else {
        put(frame.base)?;
        put(frame.end)?;
    }
    if p.rotation {
        write_rotation_reference(r, p.reference + frame.base - p.base)?;
    }
    r.raw_data.clear();
    r.raw_dwg_data = None;
    *doc = staged;
    Ok(())
}

pub(crate) fn action_base(
    doc: &Document,
    insert: Handle,
    node_id: i32,
) -> Result<Option<DVec3>, &'static str> {
    let Some(h) = cache(doc, insert, node_id)? else {
        return Ok(None);
    };
    let Some(ObjectType::XRecord(record)) = doc.objects.get(&h) else {
        return Err("The reference action cache is unavailable.");
    };
    for entry in &record.entries {
        if let Some((x, y, z)) = entry.value.as_point3d() {
            let pt = DVec3::new(x, y, z);
            if pt.is_finite() {
                return Ok(Some(pt));
            }
        }
    }
    Ok(None)
}

pub(crate) fn save_action_base(
    doc: &mut Document,
    insert: Handle,
    node_id: i32,
    base: DVec3,
) -> Result<(), &'static str> {
    if !base.is_finite() {
        return Err("The action base point must be finite.");
    }
    let mut owner = doc.ensure_extension_dictionary(insert);
    for key in [
        "AcDbBlockRepresentation",
        "AppDataCache",
        "ACAD_ENHANCEDBLOCKDATA",
    ] {
        owner = ensure_dictionary(doc, owner, key)?;
    }
    let key = node_id.to_string();
    if let Some(h) = entry(doc, owner, &key)? {
        let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&h) else {
            return Err("The reference action cache is unavailable.");
        };
        if let Some(entry) = record.entries.iter_mut().find(|e| matches!(e.value, XRecordValue::Point3D(..))) {
            entry.value = XRecordValue::Point3D(base.x, base.y, base.z);
        } else {
            record.entries.push(XRecordEntry::new(10, XRecordValue::Point3D(base.x, base.y, base.z)));
        }
        record.raw_data.clear();
        record.raw_dwg_data = None;
    } else {
        let h = doc.allocate_handle();
        let mut r = XRecord::named(&key);
        r.handle = h;
        r.owner = owner;
        r.reactors.push(owner);
        r.cloning_flags = document::objects::DictionaryCloningFlags::KeepExisting;
        r.entries = vec![
            XRecordEntry::int32(1071, 0),
            XRecordEntry::int32(1071, 0),
            XRecordEntry::int16(70, 25),
            XRecordEntry::int16(70, 104),
            XRecordEntry::new(10, XRecordValue::Point3D(base.x, base.y, base.z)),
        ];
        doc.objects.insert(h, ObjectType::XRecord(r));
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
            d.add_entry(&key, h);
        }
    }
    Ok(())
}

pub(crate) fn load_action_bases(
    doc: &Document,
    insert: Handle,
    definition: Handle,
) -> Result<std::collections::BTreeMap<Handle, DVec3>, &'static str> {
    let mut bases = std::collections::BTreeMap::new();
    for (handle, _, kind, _) in super::actions(doc, definition) {
        if kind != Kind::Scale {
            continue;
        }
        if !super::options::base(doc, handle).is_some_and(|a| a.dependent) {
            continue;
        }
        let Some(node_id) = super::action_node_id(doc, handle) else {
            continue;
        };
        if let Some(base) = action_base(doc, insert, node_id)? {
            bases.insert(handle, base);
        }
    }
    Ok(bases)
}

pub(crate) fn save_action_bases(
    doc: &mut Document,
    insert: Handle,
    _definition: Handle,
    bases: &std::collections::BTreeMap<Handle, DVec3>,
) -> Result<(), &'static str> {
    for (&handle, &base) in bases {
        let Some(node_id) = super::action_node_id(doc, handle) else {
            continue;
        };
        save_action_base(doc, insert, node_id, base)?;
    }
    Ok(())
}

/// Preserve accumulated displacement while an authored pivot moves; caches
/// belonging to a new association or an independent action cannot be reused.
fn migrate_action_bases(doc: &mut Document, before: &Document, insert: Handle, definition: Handle) -> Result<(), &'static str> {
    let parameters = all_parameters(doc, definition);
    let previous_parameters = all_parameters(before, definition);
    let actions: Vec<_> = super::actions(doc, definition).into_iter()
        .filter(|(_, _, kind, _)| *kind == Kind::Scale)
        .map(|(handle, _, _, code)| (handle, code)).collect();
    for (handle, code) in actions {
        let Some(id) = super::action_node_id(doc, handle) else { continue };
        let current = super::options::base(doc, handle).cloned();
        let previous = super::options::base(before, handle);
        let old_code = super::actions(before, definition).into_iter()
            .find(|(h, _, kind, _)| *h == handle && *kind == Kind::Scale).map(|(_, _, _, code)| code);
        let parameter = parameters.iter().find(|p| p.id == code);
        let old_parameter = previous_parameters.iter().find(|p| Some(p.id) == old_code);
        match (current, previous, parameter, old_parameter) {
            (Some(current), Some(previous), Some(p), Some(old))
                if current.dependent && previous.dependent && p.handle == old.handle => {
                if let Some(base) = action_base(doc, insert, id)? {
                    let delta = super::options::pivot(&current, p.base)? - super::options::pivot(previous, old.base)?;
                    if delta.length_squared() > 0. {
                        save_action_base(doc, insert, id, base + delta)?;
                    }
                }
            }
            _ => remove_cache(doc, insert, id)?,
        }
    }
    Ok(())
}

/// Reset discards per-reference dependent pivots along with parameter frames.
fn clear_action_bases(doc: &mut Document, insert: Handle, definition: Handle) -> Result<(), &'static str> {
    let ids: Vec<_> = super::actions(doc, definition).into_iter()
        .filter(|(handle, _, kind, _)| *kind == Kind::Scale && super::options::base(doc, *handle).is_some_and(|a| a.dependent))
        .filter_map(|(handle, _, _, _)| super::action_node_id(doc, handle))
        .collect();
    for id in ids {
        remove_cache(doc, insert, id)?;
    }
    Ok(())
}

pub(crate) fn update_action_bases(
    doc: &Document,
    insert: Handle,
    definition: Handle,
    parameters: &[Parameter],
    previous_frames: &std::collections::BTreeMap<Handle, ParameterFrame>,
    current_frames: &std::collections::BTreeMap<Handle, ParameterFrame>,
    parameter_edited: Handle,
) -> Result<std::collections::BTreeMap<Handle, DVec3>, &'static str> {
    let mut bases = load_action_bases(doc, insert, definition)?;
    let actions = super::actions(doc, definition);
    for &(handle, _, kind, code) in &actions {
        if kind != Kind::Scale {
            continue;
        }
        let Some(action_with_base) = super::options::base(doc, handle) else {
            continue;
        };
        if !action_with_base.dependent {
            continue;
        }
        let Some(p) = parameters.iter().find(|p| p.id == code) else {
            continue;
        };
        let prev_frame = previous_frames
            .get(&p.handle)
            .copied()
            .unwrap_or(ParameterFrame { base: p.base, end: p.end });
        let pivot_def = super::options::pivot(action_with_base, p.base)?;
        let prev_act_base = bases
            .get(&handle)
            .copied()
            .unwrap_or(prev_frame.base + (pivot_def - p.base));

        if p.handle == parameter_edited {
            let mut new_frame = current_frames.get(&p.handle).copied().unwrap_or(prev_frame);
            let mut p_with_base = p.clone();
            p_with_base.end += new_frame.base - p_with_base.base;
            p_with_base.base = new_frame.base;
            if let Ok(constrained) = super::constrain_parameter_position(doc, &p_with_base, new_frame.end) {
                new_frame.end = constrained;
            }
            let prev_dist = prev_frame.end.distance(prev_frame.base);
            let new_dist = new_frame.end.distance(new_frame.base);
            let def_dist = p.end.distance(p.base);
            if new_frame.base.distance(p.base) < 1e-9 && (new_dist - def_dist).abs() < 1e-9 {
                bases.insert(handle, pivot_def);
            } else if prev_dist > 1e-9 && new_dist > 1e-9 {
                let pivot = new_frame.base + (pivot_def - p.base);
                let delta = prev_act_base - pivot;
                let new_act_base = pivot + delta * (new_dist / prev_dist);
                bases.insert(handle, new_act_base);
            } else {
                bases.insert(handle, prev_act_base);
            }
        } else {
            bases.insert(handle, prev_act_base);
        }
    }
    Ok(bases)
}

fn write_rotation_reference(record: &mut XRecord, point: DVec3) -> Result<(), &'static str> {
    let entry = record.entries.iter_mut()
        .filter(|entry| matches!(entry.value, XRecordValue::Point3D(..)))
        .nth(3)
        .ok_or("The rotation parameter cache has no reference point.")?;
    entry.value = XRecordValue::Point3D(point.x, point.y, point.z);
    Ok(())
}

pub(super) fn migrate_rotation_reference(
    doc: &mut Document,
    insert: Handle,
    old: &Parameter,
    new: &Parameter,
) -> Result<(), &'static str> {
    let Some(handle) = cache(doc, insert, old.id)? else {
        return Ok(());
    };
    // A live edit may already have updated the cache when BSAVE sees it again.
    let stored = frame(doc, insert, old).or_else(|_| frame(doc, insert, new))?;
    let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&handle) else {
        return Err("The reference contains an invalid parameter record.");
    };
    write_rotation_reference(record, new.reference + stored.base - new.base)?;
    record.raw_data.clear();
    record.raw_dwg_data = None;
    Ok(())
}

fn ensure_cache(
    doc: &mut Document,
    insert: Handle,
    p: &Parameter,
) -> Result<Handle, &'static str> {
    if !matches!(doc.get_entity(insert), Some(EntityType::Insert(_))) {
        return Err("Select a block reference.");
    }
    let mut owner = doc.ensure_extension_dictionary(insert);
    if doc.object_owner(owner) != Some(insert) {
        return Err("The reference parameter dictionary has an invalid owner.");
    }
    for key in [
        "AcDbBlockRepresentation",
        "AppDataCache",
        "ACAD_ENHANCEDBLOCKDATA",
    ] {
        owner = ensure_dictionary(doc, owner, key)?;
    }
    let key = p.id.to_string();
    let h = if let Some(h) = entry(doc, owner, &key)? {
        if doc.object_owner(h) != Some(owner) {
            return Err("The reference parameter record has an invalid owner.");
        }
        h
    } else {
        let h = doc.allocate_handle();
        let mut r = XRecord::named(&key);
        r.handle = h;
        r.owner = owner;
        r.reactors.push(owner);
        r.cloning_flags = document::objects::DictionaryCloningFlags::KeepExisting;
        r.entries = vec![
            XRecordEntry::int32(1071, 0),
            XRecordEntry::int32(1071, 0),
            XRecordEntry::int16(70, 25),
            XRecordEntry::int16(70, 104),
        ];
        let points = if p.point {
            vec![p.end]
        } else if p.xy {
            vec![
                DVec3::new(p.base.x, p.end.y, p.base.z),
                DVec3::new(p.end.x, p.base.y, p.end.z),
                -DVec3::Z,
            ]
        } else if p.rotation {
            vec![p.base, p.end, -DVec3::Z, p.reference]
        } else if !p.linear && !p.flip {
            vec![p.base, p.end, -DVec3::Z, p.end]
        } else {
            vec![p.base, p.end, -DVec3::Z]
        };
        for v in points {
            r.entries
                .push(XRecordEntry::new(10, XRecordValue::Point3D(v.x, v.y, v.z)));
        }
        if p.flip {
            r.entries.push(XRecordEntry::int16(70, 0));
        }
        doc.objects.insert(h, ObjectType::XRecord(r));
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
            d.add_entry(&key, h);
        }
        h
    };
    Ok(h)
}

pub(crate) fn definition(doc: &Document, insert: Handle) -> Option<Handle> {
    let EntityType::Insert(i) = doc.get_entity(insert)? else {
        return None;
    };
    let s = i.x_scale().abs();
    if s < 1e-12
        || (i.y_scale().abs() - s).abs() > 1e-10 * s.max(1.)
        || (i.z_scale().abs() - s).abs() > 1e-10 * s.max(1.)
    {
        return None;
    }
    doc.dynamic_definition_for_insert(insert)
}

/// Stage and validate completely before exposing any changes to the drawing.
#[cfg(test)]
pub(crate) fn evaluate(
    doc: &Document,
    insert: Handle,
    parameter: Handle,
    local: DVec3,
) -> Result<Option<Document>, &'static str> {
    evaluate_value(doc, insert, parameter, Value::Point(local))
}

pub(crate) fn evaluate_value(
    doc: &Document,
    insert: Handle,
    parameter: Handle,
    input: Value,
) -> Result<Option<Document>, &'static str> {
    if let Value::Frame { base, end } = input {
        return evaluate_frame(doc, insert, parameter, ParameterFrame { base, end });
    }
    if let Value::Point(mut end) = input {
        let def =
            definition(doc, insert).ok_or("Select a uniformly scaled dynamic block reference.")?;
        let p = all_parameters(doc, def)
            .into_iter()
            .find(|p| p.handle == parameter)
            .ok_or("The reference parameter is unavailable.")?;
        if !end.is_finite() {
            return Err("Specify a finite parameter position.");
        }
        end.z = p.end.z;
        let previous = frame(doc, insert, &p)?;
        return evaluate_frame(doc, insert, parameter, ParameterFrame { end, ..previous });
    }
    evaluate_input(doc, insert, parameter, input, None)
}

pub(crate) fn evaluate_frame(
    doc: &Document,
    insert: Handle,
    parameter: Handle,
    requested: ParameterFrame,
) -> Result<Option<Document>, &'static str> {
    evaluate_input(
        doc,
        insert,
        parameter,
        Value::Point(requested.end),
        Some(requested),
    )
}

pub(crate) fn load_frames(
    doc: &Document,
    evaluated: &mut Document,
    insert: Handle,
    parameters: &[Parameter],
) -> Result<std::collections::BTreeMap<Handle, ParameterFrame>, &'static str> {
    let mut frames = std::collections::BTreeMap::new();
    for p in parameters {
        let (frame, value) = if p.flip {
            let f = frame(doc, insert, p).unwrap_or(ParameterFrame {
                base: p.base,
                end: p.end,
            });
            let v = value(doc, insert, p).unwrap_or(Value::Flip(false));
            (f, v)
        } else {
            let frame = frame(doc, insert, p).unwrap_or(ParameterFrame {
                base: p.base,
                end: p.end,
            });
            (frame, Value::Point(frame.end))
        };
        frames.insert(p.handle, frame);
        super::set_value(evaluated, p, value)?;
    }
    Ok(frames)
}

pub(super) fn canonical_flip_input(
    plan: &super::flip_frames::Plan,
    doc: &Document,
    staged: &mut Document,
    insert: Handle,
    definition: Handle,
    parameters: &[Parameter],
    frames: &mut std::collections::BTreeMap<Handle, ParameterFrame>,
    parameter: Handle,
    input: Value,
    testing: bool,
) -> Result<Value, &'static str> {
    let axis = parameters
        .iter()
        .find(|p| p.handle == plan.axis())
        .ok_or("The block parameter frame is unavailable.")?;
    let old = if testing {
        test_value(doc, insert, axis)?
    } else {
        value(doc, insert, axis)?
    };
    let axis_frame = if testing {
        test_frame(doc, insert, axis)?
    } else {
        frame(doc, insert, axis)?
    };
    frames.insert(axis.handle, axis_frame);
    for handle in plan.targets() {
        let p = parameters
            .iter()
            .find(|p| p.handle == handle)
            .ok_or("The block parameter frame is unavailable.")?;
        if cache(doc, insert, p.id)?.is_none() {
            if old == Value::Flip(true) {
                return Err("The block parameter frame is unavailable.");
            }
        } else {
            let actual = if testing {
                test_frame(doc, insert, p)?
            } else {
                frame(doc, insert, p)?
            };
            frames.insert(handle, actual);
            super::set_value(staged, p, Value::Point(actual.end))?;
        }
    }
    if old != Value::Flip(true) {
        return Ok(input);
    }
    let graph = super::chains::Graph::new(staged, definition)?;
    let values = frames.iter().map(|(h, frame)| (*h, frame.end)).collect();
    let bases = frames.iter().map(|(h, frame)| (*h, frame.base)).collect();
    let autonomous = graph.autonomous(staged, Some(&values), Some(&bases))?;
    let active_axis = graph.active_frame(axis.handle, axis_frame, &autonomous);
    let input = plan.reflect_input(parameter, input, active_axis)?;
    plan.reflect_frames(frames, active_axis)?;
    for handle in plan.targets() {
        let p = parameters
            .iter()
            .find(|p| p.handle == handle)
            .ok_or("The block parameter frame is unavailable.")?;
        super::set_value(staged, p, Value::Point(frames[&handle].end))?;
    }
    Ok(input)
}

fn publish_flip_frames(
    plan: &super::flip_frames::Plan,
    staged: &Document,
    parameters: &[Parameter],
    selected: &Parameter,
    frames: &std::collections::BTreeMap<Handle, ParameterFrame>,
    resolved: &mut std::collections::BTreeMap<Handle, ParameterFrame>,
    active: &std::collections::BTreeMap<Handle, ParameterFrame>,
) -> Result<(), &'static str> {
    let mut output = frames.clone();
    output.extend(resolved.iter().map(|(h, frame)| (*h, *frame)));
    if plan.contains(selected.handle) {
        let Value::Point(end) = super::value(staged, selected)? else {
            return Err("The block parameter frame is unavailable.");
        };
        output
            .get_mut(&selected.handle)
            .ok_or("The block parameter frame is unavailable.")?
            .end = end;
    }
    let axis = parameters
        .iter()
        .find(|p| p.handle == plan.axis())
        .ok_or("The block parameter frame is unavailable.")?;
    if super::value(staged, axis)? == Value::Flip(true) {
        let axis_frame = active
            .get(&axis.handle)
            .or_else(|| output.get(&axis.handle))
            .copied()
            .ok_or("The block parameter frame is unavailable.")?;
        plan.reflect_frames(&mut output, axis_frame)?;
    }
    for handle in plan.targets() {
        resolved.insert(handle, output[&handle]);
    }
    Ok(())
}

fn evaluate_input(
    doc: &Document,
    insert: Handle,
    parameter: Handle,
    input: Value,
    requested: Option<ParameterFrame>,
) -> Result<Option<Document>, &'static str> {
    let def =
        definition(doc, insert).ok_or("Select a uniformly scaled dynamic block reference.")?;
    if let Some(plan) = super::flip_graph::Plan::new(doc, def) {
        let input = requested.map_or(input, |f| Value::Frame { base: f.base, end: f.end });
        return plan.evaluate(doc, doc, def, insert, parameter, input, false);
    }
    let parameters = all_parameters(doc, def);
    let selected = parameters
        .iter()
        .find(|p| p.handle == parameter)
        .ok_or("The reference parameter is unavailable.")?;
    let mut evaluated = doc.clone();
    let mut frames = load_frames(doc, &mut evaluated, insert, &parameters)?;
    let plan = super::flip_frames::Plan::new(doc, def);
    let mut input = requested.map_or(input, |frame| Value::Frame {
        base: frame.base,
        end: frame.end,
    });
    if let Some(plan) = &plan {
        input = canonical_flip_input(
            plan,
            doc,
            &mut evaluated,
            insert,
            def,
            &parameters,
            &mut frames,
            parameter,
            input,
            false,
        )?;
    }
    let previous_frames = frames.clone();
    let mut base_changed = false;
    if let Value::Frame { base, end } = input {
        if selected.flip {
            return Err("Use the Flip state for this parameter.");
        }
        let requested = ParameterFrame { base, end }.validate(selected)?;
        let previous = frames
            .get_mut(&parameter)
            .ok_or("The parameter frame is unavailable.")?;
        base_changed = requested.base.distance(previous.base) > 1e-10;
        previous.base = requested.base;
        previous.end = requested.end;
        input = Value::Point(requested.end);
    } else if let Value::Point(end) = input {
        if let Some(frame) = frames.get_mut(&parameter) {
            frame.end = end;
        }
    }
    let action_bases = update_action_bases(
        doc,
        insert,
        def,
        &parameters,
        &previous_frames,
        &frames,
        parameter,
    )?;
    let mut resolved_frames = std::collections::BTreeMap::new();
    let mut resolved_active = std::collections::BTreeMap::new();
    if !super::evaluate_framed(
        &mut evaluated,
        doc,
        def,
        parameter,
        input,
        &frames,
        Some(&action_bases),
        base_changed,
        &previous_frames,
        &mut resolved_frames,
        &mut resolved_active,
        plan.as_ref().map(|p| p.action),
        &[],
    )? {
        return Ok(None);
    }
    let value = super::value(&evaluated, selected)?;
    if let Some(plan) = &plan {
        publish_flip_frames(
            plan,
            &evaluated,
            &parameters,
            selected,
            &frames,
            &mut resolved_frames,
            &resolved_active,
        )?;
    }
    let mut result = materialize(doc, insert, def, &evaluated)?;
    save_resolved_frames(&mut result, &parameters, insert, &resolved_frames)?;
    save_action_bases(&mut result, insert, def, &action_bases)?;
    if let Value::Point(end) = value {
        if plan.as_ref().is_some_and(|p| p.contains(parameter)) {
            return Ok(Some(result));
        }
        let base = resolved_frames
            .get(&parameter)
            .unwrap_or(&frames[&parameter])
            .base;
        save_frame(&mut result, insert, selected, ParameterFrame { base, end })?;
    } else {
        save(&mut result, insert, selected, value)?;
    }
    Ok(Some(result))
}

pub(crate) fn load_test_frames(
    doc: &Document,
    evaluated: &mut Document,
    insert: Handle,
    parameters: &[Parameter],
) -> Result<std::collections::BTreeMap<Handle, ParameterFrame>, &'static str> {
    let mut frames = std::collections::BTreeMap::new();
    for p in parameters {
        let (frame, value) = if p.flip {
            let f = test_frame(doc, insert, p).unwrap_or(ParameterFrame {
                base: p.base,
                end: p.end,
            });
            let v = test_value(doc, insert, p).unwrap_or(Value::Flip(false));
            (f, v)
        } else {
            let frame = test_frame(doc, insert, p).unwrap_or(ParameterFrame {
                base: p.base,
                end: p.end,
            });
            (frame, Value::Point(frame.end))
        };
        frames.insert(p.handle, frame);
        super::set_value(evaluated, p, value)?;
    }
    Ok(frames)
}

/// Evaluate a scratch reference while retaining its immutable definition baseline.
pub(crate) fn evaluate_test_value(
    doc: &Document,
    baseline: &Document,
    definition: Handle,
    insert: Handle,
    parameter: Handle,
    input: Value,
) -> Result<Option<Document>, &'static str> {
    if let Some(plan) = super::flip_graph::Plan::new(baseline, definition) {
        return plan.evaluate(doc, baseline, definition, insert, parameter, input, true);
    }
    let parameters = all_parameters(baseline, definition);
    let selected = parameters
        .iter()
        .find(|p| p.handle == parameter)
        .ok_or("The test parameter is unavailable.")?;
    let mut staged = doc.clone();
    let mut frames = load_test_frames(doc, &mut staged, insert, &parameters)?;
    let plan = super::flip_frames::Plan::new(baseline, definition);
    let input = if let Some(plan) = &plan {
        canonical_flip_input(
            plan,
            doc,
            &mut staged,
            insert,
            definition,
            &parameters,
            &mut frames,
            parameter,
            input,
            true,
        )?
    } else {
        input
    };
    let previous_frames = frames.clone();
    let mut base_changed = false;
    let input = match input {
        Value::Frame { base, end } => {
            if selected.flip {
                return Err("Use the Flip state for this parameter.");
            }
            let requested = ParameterFrame { base, end }.validate(selected)?;
            let previous = frames
                .get_mut(&parameter)
                .ok_or("The test frame is unavailable.")?;
            base_changed = previous.base.distance(requested.base) > 1e-10;
            previous.base = requested.base;
            previous.end = requested.end;
            Value::Point(requested.end)
        }
        Value::Point(end) => {
            if let Some(frame) = frames.get_mut(&parameter) {
                frame.end = end;
            }
            Value::Point(end)
        }
        value => value,
    };
    let action_bases = update_action_bases(
        doc,
        insert,
        definition,
        &parameters,
        &previous_frames,
        &frames,
        parameter,
    )?;
    let mut resolved_frames = std::collections::BTreeMap::new();
    let mut resolved_active = std::collections::BTreeMap::new();
    if !super::evaluate_framed(
        &mut staged,
        baseline,
        definition,
        parameter,
        input,
        &frames,
        Some(&action_bases),
        base_changed,
        &previous_frames,
        &mut resolved_frames,
        &mut resolved_active,
        plan.as_ref().map(|p| p.action),
        &[],
    )? {
        return Ok(None);
    }
    let value = super::value(&staged, selected)?;
    if let Some(plan) = &plan {
        publish_flip_frames(
            plan,
            &staged,
            &parameters,
            selected,
            &frames,
            &mut resolved_frames,
            &resolved_active,
        )?;
    }
    save_resolved_frames(&mut staged, &parameters, insert, &resolved_frames)?;
    save_action_bases(&mut staged, insert, definition, &action_bases)?;
    if let Value::Point(end) = value {
        if !plan.as_ref().is_some_and(|p| p.contains(parameter)) {
            save_frame(
                &mut staged,
                insert,
                selected,
                ParameterFrame {
                    base: resolved_frames
                        .get(&parameter)
                        .unwrap_or(&frames[&parameter])
                        .base,
                    end,
                },
            )?;
        }
    } else {
        save(&mut staged, insert, selected, value)?;
    }
    let EntityType::Insert(reference) = staged
        .get_entity(insert)
        .ok_or("The test reference is unavailable.")?
    else {
        return Err("Select a test reference.");
    };
    let name = reference.block_name.clone();
    super::sync_representation(&mut staged, definition, &name)?;
    Ok(Some(staged))
}

/// Restore one reference without changing the definition or another reference.
pub(crate) fn reset(
    doc: &Document,
    insert: Handle,
) -> Result<Option<Document>, &'static str> {
    let def = doc
        .dynamic_definition_for_insert(insert)
        .ok_or("Select a dynamic block reference.")?;
    let parameters = all_parameters(doc, def);
    let default = |parameter: &Parameter| {
        if parameter.flip {
            Value::Flip(false)
        } else {
            Value::Point(parameter.end)
        }
    };
    let history = match table_history_owner(doc, insert)? {
        Some(owner) => entry(doc, owner, "ACAD_ENHANCEDBLOCKHISTORY")?,
        None => None,
    };
    let unchanged = parameters
        .iter()
        .all(|p| {
            if p.flip {
                value(doc, insert, p).ok() == Some(default(p))
                    && frame(doc, insert, p).ok() == Some(ParameterFrame { base: p.base, end: p.end })
            } else {
                frame(doc, insert, p).ok() == Some(ParameterFrame { base: p.base, end: p.end })
            }
        })
        && (crate::scene::block_visibility::parameter(doc, def).is_none()
            || super::representation::visibility_state(doc, doc, def, insert) == Some(0))
        && history.is_none()
        && load_action_bases(doc, insert, def)?.is_empty()
        && doc.get_entity(insert).is_none_or(|entity| {
            entity.common().extended_data.get_record(super::flip_graph::INPUTS).is_none()
        });
    if unchanged {
        return Ok(None);
    }
    let mut evaluated = doc.clone();
    for p in &parameters {
        super::set_value(&mut evaluated, p, default(p))?;
    }
    super::rebuild(&mut evaluated, doc, def)?;
    let mut result = materialize(doc, insert, def, &evaluated)?;
    for p in &parameters {
        if p.flip {
            save(&mut result, insert, p, default(p))?;
            save_frame(&mut result, insert, p, ParameterFrame { base: p.base, end: p.end })?;
        } else {
            save_frame(
                &mut result,
                insert,
                p,
                ParameterFrame {
                    base: p.base,
                    end: p.end,
                },
            )?;
        }
    }
    if crate::scene::block_visibility::parameter(&result, def).is_some() {
        super::representation::apply_visibility(&mut result, def, insert, 0)?;
    }
    clear_table_history(&mut result, insert, history)?;
    clear_canonical_inputs(&mut result, insert);
    clear_action_bases(&mut result, insert, def)?;
    Ok(Some(result))
}

pub(crate) fn reset_test(
    doc: &Document,
    baseline: &Document,
    def: Handle,
    insert: Handle,
) -> Result<Option<Document>, &'static str> {
    let EntityType::Insert(reference) = doc.get_entity(insert).ok_or("Missing block reference.")?
    else {
        return Err("Select a block reference.");
    };
    let name = reference.block_name.clone();
    let mut result = doc.clone();
    let parameters = all_parameters(baseline, def);
    for p in &parameters {
        let value = if p.flip {
            Value::Flip(false)
        } else {
            Value::Point(p.end)
        };
        super::set_value(&mut result, p, value)?;
        if cache(doc, insert, p.id)?.is_some() {
            if p.flip {
                save(&mut result, insert, p, value)?;
                save_frame(&mut result, insert, p, ParameterFrame { base: p.base, end: p.end })?;
            } else {
                save_frame(
                    &mut result,
                    insert,
                    p,
                    ParameterFrame {
                        base: p.base,
                        end: p.end,
                    },
                )?;
            }
        }
    }
    super::rebuild(&mut result, baseline, def)?;
    super::sync_representation(&mut result, def, &name)?;
    clear_canonical_inputs(&mut result, insert);
    clear_action_bases(&mut result, insert, def)?;
    if let Some(parameter) =
        crate::scene::block_visibility::parameter(&result, def).map(|p| p.handle)
    {
        crate::scene::block_visibility::apply_state(&mut result, parameter, 0);
        super::representation::apply_visibility(&mut result, def, insert, 0)?;
    }
    let history = match table_history_owner(doc, insert)? {
        Some(owner) => entry(doc, owner, "ACAD_ENHANCEDBLOCKHISTORY")?,
        None => None,
    };
    clear_table_history(&mut result, insert, history)?;
    Ok((result != *doc).then_some(result))
}

fn clear_table_history(
    result: &mut Document,
    insert: Handle,
    history: Option<Handle>,
) -> Result<(), &'static str> {
    if let Some(history) = history {
        let owner = table_history_owner(result, insert)?
            .ok_or("The table history owner is unavailable.")?;
        if result.object_owner(history) != Some(owner) {
            return Err("The table history has an invalid owner.");
        }
        if let Some(ObjectType::Dictionary(dictionary)) = result.objects.get_mut(&owner) {
            dictionary.entries.retain(|(_, h)| *h != history);
        }
        remove_owned_cache(result, history);
    }
    Ok(())
}

pub(super) fn remove_owned_cache(result: &mut Document, root: Handle) {
    let mut owned = std::collections::BTreeSet::from([root]);
    loop {
        let children = result
            .objects
            .keys()
            .copied()
            .filter(|h| {
                !owned.contains(h)
                    && result
                        .object_owner(*h)
                        .is_some_and(|owner| owned.contains(&owner))
            })
            .collect::<Vec<_>>();
        if children.is_empty() {
            break;
        }
        owned.extend(children);
    }
    for handle in owned {
        let applications = result
            .object_xdata(handle)
            .map(|data| {
                data.records()
                    .iter()
                    .map(|record| record.application_name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for application in applications {
            result.remove_object_xdata_record(handle, &application);
        }
        result.block_representations.remove(&handle);
        result.block_visibility_params.remove(&handle);
        result.objects.remove(&handle);
    }
}

fn table_history_owner(doc: &Document, insert: Handle) -> Result<Option<Handle>, &'static str> {
    let Some(mut owner) = doc.extension_dictionary_handle(insert) else {
        return Ok(None);
    };
    for key in ["AcDbBlockRepresentation", "AppDataCache"] {
        let Some(child) = entry(doc, owner, key)? else {
            return Ok(None);
        };
        owner = child;
    }
    Ok(Some(owner))
}

pub(crate) fn table_history_row(
    doc: &Document,
    insert: Handle,
    view: &crate::scene::block_properties_table::TableView,
) -> Option<usize> {
    let owner = table_history_owner(doc, insert).ok()??;
    let handle = entry(doc, owner, "ACAD_ENHANCEDBLOCKHISTORY").ok()??;
    let ObjectType::XRecord(record) = doc.objects.get(&handle)? else { return None };
    let entries = &record.entries;
    if entries.first()?.code != 1070 || entries[0].value != XRecordValue::Int16(3)
        || entries.get(1)?.code != 1071
        || entries[1].value != XRecordValue::Int32(view.table.parameter.parameter.element.eval.node_id)
        || entries.get(3)?.code != 70 {
        return None;
    }
    let XRecordValue::Int16(row) = entries[3].value else { return None };
    usize::try_from(row).ok().filter(|row| *row < view.rows.len())
}

pub(crate) fn evaluate_table_row(
    doc: &Document,
    insert: Handle,
    columns: &[crate::scene::block_properties_table::Column],
    row: usize,
) -> Result<Option<Document>, &'static str> {
    evaluate_table_row_context(doc, None, insert, columns, row)
}

pub(crate) fn evaluate_test_table_row(
    doc: &Document,
    baseline: &Document,
    insert: Handle,
    columns: &[crate::scene::block_properties_table::Column],
    row: usize,
) -> Result<Option<Document>, &'static str> {
    evaluate_table_row_context(doc, Some(baseline), insert, columns, row)
}

fn evaluate_table_row_context(
    doc: &Document,
    baseline: Option<&Document>,
    insert: Handle,
    columns: &[crate::scene::block_properties_table::Column],
    row: usize,
) -> Result<Option<Document>, &'static str> {
    let def =
        definition(doc, insert).ok_or("Select a uniformly scaled dynamic block reference.")?;
    let view = crate::scene::block_properties_table::table(doc, def)?
        .ok_or("The Block Properties Table is unavailable.")?;
    let values = view
        .rows
        .get(row)
        .ok_or("The selected table row is unavailable.")?;
    let reflects_parameters = super::flip_graph::Plan::for_rebuild(baseline.unwrap_or(doc), def).is_some();
    let mut staged = doc.clone();
    for (column, value) in columns.iter().zip(values) {
        let p = all_parameters(&staged, def)
            .into_iter()
            .find(|p| p.handle == column.handle)
            .ok_or("The table parameter is unavailable.")?;
        let current_frame = if baseline.is_some() {
            test_frame(&staged, insert, &p)?
        } else {
            frame(&staged, insert, &p)?
        };
        let base = current_frame.base;
        let input = if column.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
            Value::Point(base + DVec3::new(value.cos(), value.sin(), 0.0) * current_frame.end.distance(base))
        } else {
            let axis = if p.polar || (p.linear && reflects_parameters) { (current_frame.end - base).normalize() }
                else { (p.end - p.base).normalize() };
            Value::Point(base + axis * *value)
        };
        let next = if let Some(baseline) = baseline {
            evaluate_test_value(&staged, baseline, def, insert, p.handle, input)?
        } else {
            evaluate_value(&staged, insert, p.handle, input)?
        };
        if let Some(next) = next {
            staged = next;
        }
    }
    write_table_history(&mut staged, insert, &view, row)?;
    Ok(Some(staged))
}

fn write_table_history(
    doc: &mut Document,
    insert: Handle,
    view: &crate::scene::block_properties_table::TableView,
    row: usize,
) -> Result<(), &'static str> {
    let mut owner = doc.ensure_extension_dictionary(insert);
    for key in ["AcDbBlockRepresentation", "AppDataCache"] {
        owner = ensure_dictionary(doc, owner, key)?;
    }
    let def = doc
        .dynamic_definition_for_insert(insert)
        .ok_or("The block definition is unavailable.")?;
    let table_eval = view.table.parameter.parameter.element.eval.node_id;
    let mut entries = vec![
        XRecordEntry::int16(1070, 3),
        XRecordEntry::int32(1071, table_eval),
        XRecordEntry::new(300, XRecordValue::String("Block Table1".into())),
        XRecordEntry::int16(70, row as i16),
        XRecordEntry::int16(1070, -1),
    ];
    let params = all_parameters(doc, def);
    for (column, value) in view.columns.iter().zip(
        view.rows
            .get(row)
            .ok_or("The selected table row is unavailable.")?,
    ) {
        let p = params
            .iter()
            .find(|p| p.handle == column.handle)
            .ok_or("The table parameter is unavailable.")?;
        entries.push(XRecordEntry::int32(1071, p.id));
        entries.push(XRecordEntry::new(300, XRecordValue::String(p.name.clone())));
        entries.push(XRecordEntry::double(40, *value));
    }
    entries.push(XRecordEntry::int16(1070, -2));
    let h = if let Some(h) = entry(doc, owner, "ACAD_ENHANCEDBLOCKHISTORY")? {
        h
    } else {
        doc.allocate_handle()
    };
    let mut rec = XRecord::named("ACAD_ENHANCEDBLOCKHISTORY");
    rec.handle = h;
    rec.owner = owner;
    rec.reactors.push(owner);
    rec.cloning_flags = document::objects::DictionaryCloningFlags::KeepExisting;
    rec.entries = entries;
    doc.objects.insert(h, ObjectType::XRecord(rec));
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
        if !d
            .entries
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("ACAD_ENHANCEDBLOCKHISTORY"))
        {
            d.add_entry("ACAD_ENHANCEDBLOCKHISTORY", h);
        }
    }
    Ok(())
}

/// Refresh existing evaluated references after an authoring change.
pub(crate) fn refresh(doc: &Document, def: Handle) -> Result<Document, &'static str> {
    refresh_after_edit(doc, doc, def)
}

pub(crate) fn refresh_after_edit(
    doc: &Document,
    before: &Document,
    def: Handle,
) -> Result<Document, &'static str> {
    if authoring::graph(doc, def)?.is_none() {
        return Ok(doc.clone());
    }
    let mut staged = doc.clone();
    authoring::tag_members(&mut staged, def);
    let inserts = doc
        .entities()
        .filter_map(|e| match e {
            EntityType::Insert(i)
                if doc.dynamic_definition_for_insert(i.common.handle) == Some(def)
                    && doc
                        .block_records
                        .get(&i.block_name)
                        .is_some_and(|b| b.handle != def) =>
            {
                Some(i.common.handle)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for &insert in &inserts {
        migrate_action_bases(&mut staged, before, insert, def)?;
    }
    let previous_plan = super::flip_graph::Plan::for_rebuild(before, def);
    let plan = super::flip_graph::Plan::for_rebuild(&staged, def);
    if plan.is_some() || previous_plan.is_some() && !inserts.is_empty() {
        let plan = match plan {
            Some(plan) => plan,
            None => super::flip_graph::Plan::exit(&staged, def)?,
        };
        let leaving = !plan.retains_inputs();
        for insert in inserts {
            let state = super::representation::visibility_state(&staged, before, def, insert);
            let has_inputs = |doc: &Document| {
                doc.get_entity(insert).is_some_and(|entity| {
                    entity.common().extended_data.get_record(super::flip_graph::INPUTS).is_some()
                })
            };
            staged = if leaving && !has_inputs(&staged) && has_inputs(before) {
                // A live editor edit already published native frames.
                plan.refresh(&staged, &staged, def, insert, None)?
            } else {
                plan.refresh(&staged, before, def, insert, previous_plan.as_ref())?
            };
            if let Some(state) = state {
                super::representation::apply_visibility(&mut staged, def, insert, state)?;
            }
        }
        return Ok(staged);
    }
    if inserts.iter().any(|insert| {
        doc.get_entity(*insert).is_some_and(|entity| {
            entity.common().extended_data.get_record(super::flip_graph::INPUTS).is_some()
        })
    }) {
        return Err(super::flip_graph::FRAME_ERROR);
    }
    let has_chain = super::chains::Graph::new(before, def)?.receivers().next().is_some()
        || super::chains::Graph::new(doc, def)?.receivers().next().is_some();
    let previous_parameters = if has_chain { all_parameters(before, def) } else { Vec::new() };
    for insert in inserts {
        let mut evaluated = staged.clone();
        let parameters = all_parameters(&staged, def);
        let frames = load_frames(&staged, &mut evaluated, insert, &parameters)?;
        let previous_frames = previous_parameters
            .iter()
            .map(|p| {
                let previous = match frame(before, insert, p) {
                    Err(_) if p.flip => Ok(ParameterFrame { base: p.base, end: p.end }),
                    previous => previous,
                };
                previous.map(|frame| (p.handle, frame))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
        let previous_values = previous_frames.iter().map(|(h, f)| (*h, f.end)).collect();
        let action_bases = load_action_bases(&staged, insert, def)?;
        let resolved_frames = super::rebuild_framed(
            &mut evaluated,
            &staged,
            def,
            &frames,
            Some(&action_bases),
            has_chain.then_some(super::RebuildInputs {
                document: before,
                frames: &previous_frames,
                values: &previous_values,
                requested: &std::collections::BTreeSet::new(),
                migrated_inputs: None,
            }),
        )?;
        let state = super::representation::visibility_state(&staged, before, def, insert);
        staged = materialize(&staged, insert, def, &evaluated)?;
        save_resolved_frames(&mut staged, &parameters, insert, &resolved_frames)?;
        save_action_bases(&mut staged, insert, def, &action_bases)?;
        if let Some(state) = state {
            super::representation::apply_visibility(&mut staged, def, insert, state)?;
        }
    }
    Ok(staged)
}

pub(crate) fn materialize(
    doc: &Document,
    insert: Handle,
    def: Handle,
    evaluated: &Document,
) -> Result<Document, &'static str> {
    let mut scene = crate::scene::Scene::new();
    scene.document = doc.clone();
    authoring::tag_members(&mut scene.document, def);
    scene.dynamic_representation(insert);
    let Some(EntityType::Insert(reference)) = scene.document.get_entity(insert) else {
        return Err("Missing block reference.");
    };
    let name = reference.block_name.clone();
    if scene
        .document
        .block_records
        .get(&name)
        .is_none_or(|b| b.handle == def)
    {
        return Err("Could not isolate the dynamic block reference.");
    }
    super::representation::reconcile(&mut scene, def, &name, evaluated)?;
    if let Some((_, basepoint)) = crate::scene::block_basepoint::parameter(evaluated, def)
        .filter(|(handle, basepoint)| {
            let id = basepoint.parameter.parameter.element.eval.node_id;
            super::actions(evaluated, def).iter().any(|(_, action, kind, _)| {
                matches!(kind, Kind::Stretch | Kind::Flip)
                    && action.dependencies.contains(handle)
                    && action.action_ids.contains(&id)
            })
        })
    {
        let Some(EntityType::Insert(original)) = doc.get_entity(insert) else {
            unreachable!()
        };
        let old = crate::scene::render_graph::block_base_point(doc, &original.block_name);
        let location = basepoint.point;
        let delta = v(location - old);
        let scale = DVec3::new(original.x_scale(), original.y_scale(), original.z_scale());
        // The INSERT point is stored in its own OCS. Moving it with the
        // evaluated origin keeps the transformed block geometry in place.
        let position = v(original.insert_point)
            + glam::DQuat::from_rotation_z(original.rotation) * (scale * delta);
        if !position.is_finite() {
            return Err("The parameter value produces non-finite geometry.");
        }
        let Some(EntityType::Insert(reference)) = scene.document.get_entity_mut(insert) else {
            unreachable!()
        };
        reference.insert_point = native(position);
        let id = basepoint.parameter.parameter.element.eval.node_id;
        if let Some(handle) = cache(&scene.document, insert, id)? {
            let Some(ObjectType::XRecord(record)) = scene.document.objects.get_mut(&handle) else {
                return Err("The reference contains an invalid parameter record.");
            };
            let mut points = record.entries.iter_mut()
                .filter(|entry| matches!(entry.value, XRecordValue::Point3D(..)));
            let point = points.next().filter(|_| record.entries_complete && points.next().is_none())
                .ok_or("The reference parameter cache has no finite endpoint.")?;
            let value = XRecordValue::Point3D(location.x, location.y, location.z);
            if point.value != value {
                point.value = value;
                record.raw_data.clear();
                record.raw_dwg_data = None;
            }
        }
    }
    Ok(scene.document)
}

pub(super) fn save_resolved_frames(
    doc: &mut Document,
    parameters: &[Parameter],
    insert: Handle,
    frames: &std::collections::BTreeMap<Handle, ParameterFrame>,
) -> Result<(), &'static str> {
    for p in parameters {
        if let Some(frame) = frames.get(&p.handle) {
            save_frame(doc, insert, p, *frame)?;
        }
    }
    Ok(())
}
