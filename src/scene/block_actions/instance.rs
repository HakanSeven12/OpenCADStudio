//! Saved INSERT evaluation: definition geometry stays immutable; parameter
//! values live in the native per-reference AppDataCache.
use super::*;
use codec::objects::{Dictionary, XRecord, XRecordEntry, XRecordValue};

fn entry(doc: &Document, owner: Handle, key: &str) -> Result<Option<Handle>, &'static str> {
    let Some(ObjectType::Dictionary(d)) = doc.objects.get(&owner) else {
        return Err("The reference contains an invalid parameter dictionary.");
    };
    Ok(d.entries
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, h)| *h))
}

fn cache(doc: &Document, insert: Handle, id: i32) -> Result<Option<Handle>, &'static str> {
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
        || points[0].distance(p.base) > 1e-8
        || points[1].distance(p.end) > 1e-8
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
        _ => Err("The Flip state is not zero or one."),
    }
}

fn ensure_dictionary(
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
    if p.flip {
        return Err("Use the Flip state for this parameter.");
    }
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
        let reference = r
            .entries
            .iter_mut()
            .filter(|entry| matches!(entry.value, XRecordValue::Point3D(..)))
            .nth(3)
            .ok_or("The rotation parameter cache has no reference point.")?;
        let point = p.reference + frame.base - p.base;
        reference.value = XRecordValue::Point3D(point.x, point.y, point.z);
    }
    r.raw_data.clear();
    r.raw_dwg_data = None;
    *doc = staged;
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
        r.cloning_flags = codec::objects::DictionaryCloningFlags::KeepExisting;
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
        let value = if p.flip {
            value(doc, insert, p)?
        } else {
            let frame = frame(doc, insert, p)?;
            frames.insert(p.handle, frame);
            Value::Point(frame.end)
        };
        super::set_value(evaluated, p, value)?;
    }
    Ok(frames)
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
    let parameters = all_parameters(doc, def);
    let selected = parameters
        .iter()
        .find(|p| p.handle == parameter)
        .ok_or("The reference parameter is unavailable.")?;
    let mut evaluated = doc.clone();
    let mut frames = load_frames(doc, &mut evaluated, insert, &parameters)?;
    let mut base_changed = false;
    if let Some(requested) = requested {
        if selected.flip {
            return Err("Use the Flip state for this parameter.");
        }
        let requested = requested.validate(selected)?;
        let previous = frames
            .get_mut(&parameter)
            .ok_or("The parameter frame is unavailable.")?;
        base_changed = requested.base.distance(previous.base) > 1e-10;
        previous.base = requested.base;
    }
    if !super::evaluate_framed(
        &mut evaluated,
        doc,
        def,
        parameter,
        input,
        &frames,
        base_changed,
    )? {
        return Ok(None);
    }
    let value = super::value(&evaluated, selected)?;
    let mut result = materialize(doc, insert, def, &evaluated)?;
    if let Value::Point(end) = value {
        let base = frames[&parameter].base;
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
        let value = if p.flip {
            test_value(doc, insert, p)?
        } else {
            let frame = test_frame(doc, insert, p)?;
            frames.insert(p.handle, frame);
            Value::Point(frame.end)
        };
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
    let parameters = all_parameters(baseline, definition);
    let selected = parameters
        .iter()
        .find(|p| p.handle == parameter)
        .ok_or("The test parameter is unavailable.")?;
    let mut staged = doc.clone();
    let mut frames = load_test_frames(doc, &mut staged, insert, &parameters)?;
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
            Value::Point(requested.end)
        }
        value => value,
    };
    if !super::evaluate_framed(
        &mut staged,
        baseline,
        definition,
        parameter,
        input,
        &frames,
        base_changed,
    )? {
        return Ok(None);
    }
    let value = super::value(&staged, selected)?;
    if let Value::Point(end) = value {
        save_frame(
            &mut staged,
            insert,
            selected,
            ParameterFrame {
                base: frames[&parameter].base,
                end,
            },
        )?;
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
        .all(|p| value(doc, insert, p).ok() == Some(default(p)))
        && (crate::scene::block_visibility::parameter(doc, def).is_none()
            || super::representation::visibility_state(doc, doc, def, insert) == Some(0))
        && history.is_none();
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
        let mut owned = std::collections::BTreeSet::from([history]);
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
    Ok(())
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
            let axis = if p.polar { (current_frame.end - base).normalize() }
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
    rec.cloning_flags = codec::objects::DictionaryCloningFlags::KeepExisting;
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
    for insert in inserts {
        let mut evaluated = staged.clone();
        let parameters = all_parameters(&staged, def);
        let frames = load_frames(&staged, &mut evaluated, insert, &parameters)?;
        super::rebuild_framed(&mut evaluated, &staged, def, &frames)?;
        let state = super::representation::visibility_state(&staged, before, def, insert);
        staged = materialize(&staged, insert, def, &evaluated)?;
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
    Ok(scene.document)
}
