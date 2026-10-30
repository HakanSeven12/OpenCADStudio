use super::*;
use crate::scene::block_actions::{self as actions, instance::ParameterFrame, Parameter, Value};
use codec::types::Transform;
use glam::DVec3;

pub const START: usize = crate::scene::pick::grip::CUSTOM_GRIP_ID_START;

pub fn id(parameter: i32, role: usize) -> Option<usize> {
    if role >= 4 {
        return None;
    }
    START
        .checked_add(usize::try_from(parameter).ok()?.checked_mul(4)?)?
        .checked_add(role)
        .filter(|id| *id < usize::MAX - 1024)
}

pub fn decode(id: usize) -> Option<(i32, usize)> {
    if id >= usize::MAX - 1024 {
        return None;
    }
    let slot = id.checked_sub(START)?;
    Some((i32::try_from(slot / 4).ok()?, slot % 4))
}

pub struct Grip {
    pub id: usize,
    pub role: usize,
    pub point: DVec3,
    pub offset: DVec3,
    pub direction: Option<DVec3>,
}

pub fn grips(doc: &Document, definition: Handle, p: &Parameter, value: Value) -> Vec<Grip> {
    let frame = match value {
        Value::Point(end) => ParameterFrame { base: p.base, end },
        Value::Frame { base, end } => ParameterFrame { base, end },
        Value::Flip(_) => ParameterFrame {
            base: p.base,
            end: p.end,
        },
    };
    config::locations(doc, definition, p.handle)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|native| {
            let offset = actions::v(native.point - native.key_point);
            let key = if p.point {
                frame.end
            } else {
                match native.role {
                    0 => frame.base,
                    1 => frame.end,
                    2 => DVec3::new(frame.end.x, frame.base.y, frame.base.z),
                    _ => DVec3::new(frame.base.x, frame.end.y, frame.base.z),
                }
            };
            Some(Grip {
                id: id(p.id, native.role)?,
                role: native.role,
                point: key + offset,
                offset,
                direction: if p.flip {
                    actions::grip_direction(p, value)
                } else {
                    native.direction.map(actions::v)
                },
            })
        })
        .collect()
}

/// Configured grip locations filtered by native action key-point association.
pub fn action_grips(
    doc: &Document,
    definition: Handle,
    parameter: &Parameter,
    value: Value,
) -> Vec<Grip> {
    let mask = actions::grip_roles::mask(doc, definition, parameter);
    grips(doc, definition, parameter, value)
        .into_iter()
        .filter(|grip| mask & (1 << grip.role) != 0)
        .collect()
}

pub(crate) fn action_grip_available(
    doc: &Document,
    definition: Handle,
    parameter: Handle,
    id: usize,
) -> bool {
    let Some((node, role)) = decode(id) else {
        return false;
    };
    let Some(p) = actions::all_parameters(doc, definition)
        .into_iter()
        .find(|p| p.handle == parameter && p.id == node)
    else {
        return false;
    };
    actions::grip_roles::mask(doc, definition, &p) & (1 << role) != 0
        && config::locations(doc, definition, parameter)
            .is_ok_and(|grips| grips.iter().any(|grip| grip.role == role))
}

pub struct Input {
    pub id: usize,
    pub role: usize,
    pub frame: ParameterFrame,
    pub offset: DVec3,
    pub point: bool,
    pub linear_axis: Option<DVec3>,
    pub limits: Limits,
    pub to_world: Transform,
    pub to_local: Transform,
}

#[derive(Default)]
pub struct Limits {
    distance: Option<codec::objects::BlockParameterValueSet>,
    angle: Option<codec::objects::BlockParameterValueSet>,
    x: Option<codec::objects::BlockParameterValueSet>,
    y: Option<codec::objects::BlockParameterValueSet>,
    table: Vec<f64>,
}

impl Limits {
    pub fn read(doc: &Document, definition: Handle, p: &Parameter) -> Self {
        let mut limits = Self::default();
        if let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&p.handle) {
            match &object.data {
                DynamicBlockData::LinearParameter(p) => limits.distance = Some(p.value_set.clone()),
                DynamicBlockData::PolarParameter(p) => {
                    limits.distance = Some(p.distance_value_set.clone());
                    limits.angle = Some(p.angle_value_set.clone());
                }
                DynamicBlockData::XYParameter(p) => {
                    limits.x = Some(p.x_value_set.clone());
                    limits.y = Some(p.y_value_set.clone());
                }
                _ => {}
            }
        }
        if p.linear {
            if let Ok(Some(table)) = crate::scene::block_properties_table::table(doc, definition) {
                if table.must_match {
                    if let Some(index) = table.columns.iter().position(|column| {
                        column.handle == p.handle
                            && column
                                .connection_name
                                .eq_ignore_ascii_case("UpdatedDistance")
                    }) {
                        limits.table = table
                            .rows
                            .iter()
                            .filter_map(|row| row.get(index).copied())
                            .collect();
                    }
                }
            }
        }
        limits
    }

    fn delta(&self, delta: DVec3) -> DVec3 {
        let scalar = |value, set: &Option<codec::objects::BlockParameterValueSet>| {
            set.as_ref()
                .map_or(value, |set| actions::constrain_value_set(value, set))
        };
        if self.x.is_some() || self.y.is_some() {
            return DVec3::new(scalar(delta.x, &self.x), scalar(delta.y, &self.y), delta.z);
        }
        let mut length = scalar(delta.length(), &self.distance);
        if let Some(nearest) = self.table.iter().copied().min_by(|a, b| {
            (a - length)
                .abs()
                .total_cmp(&(b - length).abs())
                .then_with(|| a.total_cmp(b))
        }) {
            length = nearest;
        }
        let direction = if let Some(set) = &self.angle {
            let angle = actions::constrain_angle(delta.y.atan2(delta.x), set);
            DVec3::new(angle.cos(), angle.sin(), 0.)
        } else {
            delta.try_normalize().unwrap_or(DVec3::X)
        };
        direction * length
    }
}

pub fn markers(
    doc: &Document,
    definition: Handle,
    p: &Parameter,
    value: Value,
    id: usize,
) -> Vec<DVec3> {
    let Some(grip) = grips(doc, definition, p, value)
        .into_iter()
        .find(|grip| grip.id == id)
    else {
        return vec![];
    };
    let frame = match value {
        Value::Point(end) => ParameterFrame { base: p.base, end },
        Value::Frame { base, end } => ParameterFrame { base, end },
        Value::Flip(_) => return vec![],
    };
    actions::feedback::value_markers(doc, p, value)
        .into_iter()
        .map(|point| {
            let end = point - actions::grip_offset(p);
            let delta = end - frame.base;
            let key = if p.point {
                end
            } else {
                match grip.role {
                    0 => frame.end - delta,
                    1 => end,
                    2 => DVec3::new(end.x, frame.end.y - delta.y, frame.base.z),
                    _ => DVec3::new(frame.end.x - delta.x, end.y, frame.base.z),
                }
            };
            key + grip.offset
        })
        .collect()
}

impl Input {
    pub fn value(&self, world: DVec3) -> Value {
        let mut at = actions::v(self.to_local.apply(actions::native(world))) - self.offset;
        let mut frame = self.frame;
        at.z = frame.base.z;
        if self.point {
            frame.end = at;
        } else {
            match self.role {
                0 => {
                    frame.base = self
                        .linear_axis
                        .map_or(at, |axis| frame.end + axis * (at - frame.end).dot(axis))
                }
                1 => {
                    frame.end = self
                        .linear_axis
                        .map_or(at, |axis| frame.base + axis * (at - frame.base).dot(axis))
                }
                2 => {
                    frame.end.x = at.x;
                    frame.base.y = at.y;
                }
                _ => {
                    frame.base.x = at.x;
                    frame.end.y = at.y;
                }
            }
        }
        if !self.point {
            let delta = self.limits.delta(frame.end - frame.base);
            match self.role {
                0 => frame.base = frame.end - delta,
                2 => {
                    frame.end.x = frame.base.x + delta.x;
                    frame.base.y = frame.end.y - delta.y;
                }
                3 => {
                    frame.base.x = frame.end.x - delta.x;
                    frame.end.y = frame.base.y + delta.y;
                }
                _ => {}
            }
        }
        Value::Frame {
            base: frame.base,
            end: frame.end,
        }
        .map_points(|point| actions::v(self.to_world.apply(actions::native(point))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_runtime_ids_exclude_entity_menu_sentinels_and_invalid_roles() {
        for node in [0, 17, 1024] {
            for role in 0..4 {
                assert_eq!(decode(id(node, role).unwrap()), Some((node, role)));
            }
        }
        for sentinel in [
            usize::MAX,
            usize::MAX - 1,
            usize::MAX - 2,
            usize::MAX - 1024,
            START - 1,
        ] {
            assert!(decode(sentinel).is_none());
        }
        assert!(id(-1, 0).is_none());
        assert!(id(0, 4).is_none());
    }

    #[test]
    fn block_runtime_base_markers_hold_the_opposite_endpoint_and_native_offsets() {
        let mut scene = crate::scene::Scene::new();
        scene.define_block_raw("RUNTIME", Vector3::ZERO, vec![]);
        let doc = &mut scene.document;
        let definition = doc.block_records.get("RUNTIME").unwrap().handle;
        let parameter = crate::scene::block_distance_parameter::create(
            doc,
            definition,
            crate::scene::block_distance_parameter::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::new(5., 2., 0.),
        )
        .unwrap();
        config::set(doc, definition, parameter, Some(2)).unwrap();
        for grip in config::locations(doc, definition, parameter).unwrap() {
            config::set_location(
                doc,
                definition,
                parameter,
                grip.role,
                grip.point + Vector3::new(15., 7., 0.),
            )
            .unwrap();
        }
        let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&parameter) else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(native) = &mut object.data else {
            panic!()
        };
        native.value_set.flags = 8;
        native.value_set.values = vec![5., 10., 15.];
        let p = actions::all_parameters(doc, definition).remove(0);
        let before = doc.clone();
        let value = Value::Frame {
            base: DVec3::new(-5., 0., 0.),
            end: DVec3::new(10., 0., 0.),
        };
        assert_eq!(
            markers(doc, definition, &p, value, id(p.id, 0).unwrap()),
            vec![
                DVec3::new(20., 7., 0.),
                DVec3::new(15., 7., 0.),
                DVec3::new(10., 7., 0.)
            ]
        );
        assert_eq!(
            markers(doc, definition, &p, value, id(p.id, 1).unwrap()),
            vec![
                DVec3::new(15., 7., 0.),
                DVec3::new(20., 7., 0.),
                DVec3::new(25., 7., 0.)
            ]
        );
        assert_eq!(*doc, before);
        let input = Input {
            id: id(p.id, 0).unwrap(),
            role: 0,
            frame: ParameterFrame {
                base: DVec3::new(-5., 0., 0.),
                end: DVec3::new(10., 0., 0.),
            },
            offset: DVec3::new(15., 7., 0.),
            point: false,
            linear_axis: Some(DVec3::X),
            limits: Limits::read(doc, definition, &p),
            to_world: Transform::identity(),
            to_local: Transform::identity(),
        };
        assert_eq!(
            input.value(DVec3::new(11., 7., 0.)),
            value,
            "Base snapping must keep the opposite endpoint fixed."
        );
    }

    #[test]
    fn block_runtime_xy_corner_constraints_keep_the_opposite_corner_fixed() {
        let set = codec::objects::BlockParameterValueSet {
            flags: 8,
            values: vec![5., 10., 15.],
            ..Default::default()
        };
        let input = Input {
            id: id(0, 2).unwrap(),
            role: 2,
            frame: ParameterFrame {
                base: DVec3::ZERO,
                end: DVec3::new(10., 10., 0.),
            },
            offset: DVec3::ZERO,
            point: false,
            linear_axis: None,
            limits: Limits {
                x: Some(set.clone()),
                y: Some(set),
                ..Limits::default()
            },
            to_world: Transform::identity(),
            to_local: Transform::identity(),
        };
        assert_eq!(
            input.value(DVec3::new(14., -4., 0.)),
            Value::Frame {
                base: DVec3::new(0., -5., 0.),
                end: DVec3::new(15., 10., 0.)
            }
        );
    }
}
