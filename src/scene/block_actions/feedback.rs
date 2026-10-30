//! Read-only authoring dependencies and finite value-set display samples.
use super::*;
use codec::objects::BlockParameterValueSet;

const MAX_MARKERS: usize = 256;

fn samples(set: &BlockParameterValueSet, current: f64) -> Vec<f64> {
    let minimum = (set.flags & 1 != 0).then_some(set.minimum);
    let maximum = (set.flags & 2 != 0).then_some(set.maximum);
    let allowed = |v: f64| {
        v.is_finite() && minimum.is_none_or(|min| v >= min) && maximum.is_none_or(|max| v <= max)
    };
    let mut values = if set.flags & 8 != 0 {
        let step = set.values.len().div_ceil(MAX_MARKERS).max(1);
        set.values
            .iter()
            .step_by(step)
            .copied()
            .filter(|v| allowed(*v))
            .collect::<Vec<_>>()
    } else if set.flags & 4 != 0 && set.increment.is_finite() && set.increment > 0.0 {
        let origin = minimum.unwrap_or(0.0);
        let low = minimum.unwrap_or(current - 10.0 * set.increment);
        let high = maximum.unwrap_or(current + 10.0 * set.increment);
        let first = ((low - origin) / set.increment).ceil();
        let last = ((high - origin) / set.increment).floor();
        if !first.is_finite() || !last.is_finite() || last < first {
            return vec![];
        }
        let stride = ((last - first + 1.0) / ((MAX_MARKERS - 1) as f64))
            .ceil()
            .max(1.0);
        let mut values = (0..MAX_MARKERS - 1)
            .map(|i| first + i as f64 * stride)
            .take_while(|k| *k <= last)
            .map(|k| origin + k * set.increment)
            .filter(|v| allowed(*v))
            .collect::<Vec<_>>();
        let end = origin + last * set.increment;
        if allowed(end) {
            values.push(end);
        }
        values
    } else {
        minimum
            .into_iter()
            .chain(maximum)
            .filter(|v| allowed(*v))
            .collect()
    };
    if set.flags & 8 != 0 {
        if let Some(last) = set.values.last().filter(|value| allowed(**value)) {
            values.push(*last);
        }
    }
    values.sort_by(f64::total_cmp);
    values.dedup();
    values
}

pub(crate) fn value_markers(doc: &Document, p: &Parameter, value: Value) -> Vec<DVec3> {
    if let Value::Frame { base, end } = value {
        let mut evaluated = p.clone();
        evaluated.end += base - p.base;
        evaluated.grip_location = p.grip_location.map(|point| point + base - p.base);
        evaluated.base = base;
        return value_markers(doc, &evaluated, Value::Point(end));
    }
    let Value::Point(current) = value else {
        return vec![];
    };
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&p.handle) else {
        return vec![];
    };
    let delta = current - p.base;
    let mut points = vec![];
    let distance_points = |set: &BlockParameterValueSet| {
        let direction = delta
            .try_normalize()
            .or_else(|| (p.end - p.base).try_normalize())
            .unwrap_or(DVec3::X);
        samples(set, delta.length())
            .into_iter()
            .map(|distance| p.base + direction * distance)
            .collect::<Vec<_>>()
    };
    let angle_points = |set: &BlockParameterValueSet| {
        samples(set, delta.y.atan2(delta.x))
            .into_iter()
            .map(|angle| {
                p.base
                    + DVec3::new(
                        angle.cos() * delta.length(),
                        angle.sin() * delta.length(),
                        delta.z,
                    )
            })
            .collect::<Vec<_>>()
    };
    match &object.data {
        DynamicBlockData::LinearParameter(native) => {
            points.extend(distance_points(&native.value_set))
        }
        DynamicBlockData::PolarParameter(native) => {
            points.extend(distance_points(&native.distance_value_set));
            points.extend(angle_points(&native.angle_value_set));
        }
        DynamicBlockData::RotationParameter(native) => {
            points.extend(angle_points(&native.value_set))
        }
        DynamicBlockData::XYParameter(native) => {
            points.extend(
                samples(&native.x_value_set, delta.x)
                    .into_iter()
                    .map(|x| DVec3::new(p.base.x + x, current.y, current.z)),
            );
            points.extend(
                samples(&native.y_value_set, delta.y)
                    .into_iter()
                    .map(|y| DVec3::new(current.x, p.base.y + y, current.z)),
            );
        }
        _ => {}
    }
    points.retain(|point| point.is_finite());
    points.into_iter().map(|point| grip_position(p, Value::Point(point))).collect()
}

pub(crate) fn dependencies(doc: &Document, definition: Handle, selected: Handle) -> Vec<Handle> {
    let selected_parameter = all_parameters(doc, definition)
        .into_iter()
        .find(|p| p.handle == selected)
        .map(|p| p.id);
    let mut handles: Vec<_> = actions(doc, definition)
        .into_iter()
        .filter(|(handle, _, _, id)| *handle == selected || selected_parameter == Some(*id))
        .flat_map(|(_, action, _, _)| action.dependencies.iter().copied())
        .collect();
    // A table parameter drives the action parameters represented by its columns.
    if let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&selected) {
        if let DynamicBlockData::LookupParameter(_) = &object.data {
            if let Ok(Some((_, table))) =
                super::super::block_lookup::table(doc, definition, selected)
            {
                for column in &table.columns {
                    handles.extend(
                        actions(doc, definition)
                            .into_iter()
                            .filter(|(_, _, _, id)| *id == column.node_id)
                            .flat_map(|(_, action, _, _)| action.dependencies.iter().copied()),
                    );
                }
            }
        } else if let DynamicBlockData::PropertiesTable(_) = &object.data {
            if let Ok(Some(table)) = super::super::block_properties_table::table(doc, definition) {
                for column in table.columns {
                    handles.extend(
                        actions(doc, definition)
                            .into_iter()
                            .filter(|(_, _, _, id)| *id == column.node_id)
                            .flat_map(|(_, action, _, _)| action.dependencies.iter().copied()),
                    );
                }
            }
        }
    }
    if let Some(parameter) = doc.block_visibility_params.get(&selected) {
        handles.extend(parameter.all_blocks.iter().copied());
    }
    handles.sort();
    handles.dedup();
    handles.retain(|handle| doc.get_entity(*handle).is_some());
    handles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_value_markers_follow_polar_angles_and_signed_xy_axes() {
        use crate::scene::block_distance_parameter::{self as distance, Kind};
        let mut scene = crate::scene::Scene::new();
        scene.define_block_raw("MARKERS", Vector3::ZERO, vec![]);
        let definition = scene.document.block_records.get("MARKERS").unwrap().handle;
        let polar = distance::create(
            &mut scene.document,
            definition,
            Kind::Polar,
            Vector3::new(2.0, 3.0, 0.0),
            Vector3::new(12.0, 3.0, 0.0),
            Vector3::new(7.0, 5.0, 0.0),
        )
        .unwrap();
        let Some(ObjectType::DynamicBlock(object)) = scene.document.objects.get_mut(&polar) else {
            panic!()
        };
        let DynamicBlockData::PolarParameter(native) = &mut object.data else {
            panic!()
        };
        native.distance_value_set = BlockParameterValueSet {
            flags: 8,
            values: vec![5.0, 10.0],
            ..Default::default()
        };
        native.angle_value_set = BlockParameterValueSet {
            flags: 8,
            values: vec![0.0, std::f64::consts::FRAC_PI_2],
            ..Default::default()
        };
        let p = all_parameters(&scene.document, definition)
            .into_iter()
            .find(|p| p.handle == polar)
            .unwrap();
        let points = value_markers(&scene.document, &p, Value::Point(p.end));
        assert_eq!(points.len(), 4);
        assert!(points.contains(&DVec3::new(7.0, 3.0, 0.0)));
        assert!(points
            .iter()
            .any(|v| v.distance(DVec3::new(2.0, 13.0, 0.0)) < 1e-12));
        let xy = distance::create(
            &mut scene.document,
            definition,
            Kind::XY,
            Vector3::new(2.0, 3.0, 0.0),
            Vector3::new(12.0, 13.0, 0.0),
            Vector3::new(7.0, 8.0, 0.0),
        )
        .unwrap();
        let Some(ObjectType::DynamicBlock(object)) = scene.document.objects.get_mut(&xy) else {
            panic!()
        };
        let DynamicBlockData::XYParameter(native) = &mut object.data else {
            panic!()
        };
        native.x_value_set = BlockParameterValueSet {
            flags: 8,
            values: vec![-5.0, 0.0, 5.0],
            ..Default::default()
        };
        native.y_value_set = BlockParameterValueSet {
            flags: 8,
            values: vec![0.0, 5.0],
            ..Default::default()
        };
        let p = all_parameters(&scene.document, definition)
            .into_iter()
            .find(|p| p.handle == xy)
            .unwrap();
        assert_eq!(
            value_markers(&scene.document, &p, Value::Point(p.end)),
            vec![
                DVec3::new(-3.0, 13.0, 0.0),
                DVec3::new(2.0, 13.0, 0.0),
                DVec3::new(7.0, 13.0, 0.0),
                DVec3::new(12.0, 3.0, 0.0),
                DVec3::new(12.0, 8.0, 0.0)
            ]
        );
    }
    #[test]
    fn block_value_markers_sample_bounded_and_unbounded_sets_without_overflow() {
        let mut set = BlockParameterValueSet {
            flags: 7,
            minimum: -10.0,
            maximum: 10.0,
            increment: 5.0,
            ..Default::default()
        };
        assert_eq!(samples(&set, 0.0), vec![-10.0, -5.0, 0.0, 5.0, 10.0]);
        set.flags = 4;
        assert!(samples(&set, -20.0).contains(&-20.0));
        set.flags = 7;
        set.maximum = 1e12;
        set.increment = 1e-6;
        let values = samples(&set, 0.0);
        assert!(values.len() <= MAX_MARKERS);
        assert_eq!(values.first(), Some(&-10.0));
        assert_eq!(values.last(), Some(&1e12));
        set.increment = 0.0;
        assert!(samples(&set, 0.0).iter().all(|v| v.is_finite()));
        set.flags = 8;
        set.values = vec![f64::NAN, 2.0, 3.0, f64::INFINITY];
        assert_eq!(samples(&set, 0.0), vec![2.0, 3.0]);
    }
}
