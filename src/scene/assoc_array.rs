//! Associative arrays as the drawing stores them: an AcDbAssocAction whose
//! body is an AcDbAssocArrayActionBody, with dependencies on the array INSERT
//! (read + write), the anonymous block holding the item INSERTs (write) and the
//! anonymous block holding the source objects (read). The action's value
//! parameters (Items, ItemSpacing, Rows, …) define where the items go.

use codec::EntityType;
use codec::objects::{
    AssocAction, AssocArrayActionBody, AssocDependency, AssocEvalValue, AssocValueParam,
    AssociativeData, ObjectType,
};
use codec::{Document, Handle};
use glam::{DMat4, DVec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    Rectangular,
    Polar,
    Path,
}

/// One associative array, resolved from its array INSERT.
#[derive(Clone, Debug)]
pub struct AssocArray {
    pub insert: Handle,
    pub action: Handle,
    pub body: Handle,
    /// Block record holding the item INSERTs.
    pub array_block: Handle,
    /// Block record holding the source objects.
    pub source_block: Handle,
    pub kind: ArrayKind,
}

fn dependency(document: &Document, handle: Handle) -> Option<&AssocDependency> {
    match document.objects.get(&handle)? {
        ObjectType::Associative(object) => match &object.data {
            AssociativeData::Dependency(dependency) => Some(dependency),
            AssociativeData::GeomDependency(geom) => Some(&geom.dependency),
            AssociativeData::ValueDependency(value) => Some(&value.dependency),
            _ => None,
        },
        _ => None,
    }
}

pub fn action(document: &Document, handle: Handle) -> Option<&AssocAction> {
    match document.objects.get(&handle)? {
        ObjectType::Associative(object) => match &object.data {
            AssociativeData::Action(action) => Some(action),
            _ => None,
        },
        _ => None,
    }
}

pub fn body(document: &Document, handle: Handle) -> Option<&AssocArrayActionBody> {
    match document.objects.get(&handle)? {
        ObjectType::Associative(object) => match &object.data {
            AssociativeData::ArrayActionBody(body) => Some(body),
            AssociativeData::ArrayModifyActionBody(modify) => Some(&modify.body),
            _ => None,
        },
        _ => None,
    }
}

/// The associative array whose array INSERT is `insert`, if any.
pub fn array_of_insert(document: &Document, insert: Handle) -> Option<AssocArray> {
    document.objects.iter().find_map(|(&handle, object)| {
        let ObjectType::Associative(object) = object else {
            return None;
        };
        let AssociativeData::Action(action) = &object.data else {
            return None;
        };
        let array_body = body(document, action.action_body)?;
        let (mut array_block, mut source_block, mut owns_insert) =
            (Handle::NULL, Handle::NULL, false);
        for link in &action.dependencies {
            let Some(dependency) = dependency(document, link.dependency) else {
                continue;
            };
            match (
                dependency.is_read_dependency,
                dependency.is_write_dependency,
            ) {
                (true, true) if dependency.dependent_on == insert => owns_insert = true,
                (false, true) => array_block = dependency.dependent_on,
                // A path array also reads its path; the source is the block.
                (true, false)
                    if source_block.is_null()
                        && document
                            .block_records
                            .iter()
                            .any(|record| record.handle == dependency.dependent_on) =>
                {
                    source_block = dependency.dependent_on
                }
                _ => {}
            }
        }
        let kind = match array_body.parameter_block.as_str() {
            "AcDbAssocArrayRectangularParameters" => ArrayKind::Rectangular,
            "AcDbAssocArrayPolarParameters" => ArrayKind::Polar,
            "AcDbAssocArrayPathParameters" => ArrayKind::Path,
            _ => return None,
        };
        (owns_insert && !array_block.is_null() && !source_block.is_null()).then_some(AssocArray {
            insert,
            action: handle,
            body: action.action_body,
            array_block,
            source_block,
            kind,
        })
    })
}

/// An array action whose array INSERT is gone (erased or exploded): it has
/// nothing left to drive and is dropped when the networks are written.
pub fn orphaned_array_action(document: &Document, handle: Handle) -> bool {
    let Some(action) = action(document, handle) else {
        return false;
    };
    if body(document, action.action_body).is_none() {
        return false;
    }
    !action.dependencies.iter().any(|link| {
        dependency(document, link.dependency).is_some_and(|dependency| {
            dependency.is_read_dependency
                && dependency.is_write_dependency
                && document.get_entity(dependency.dependent_on).is_some()
        })
    })
}

/// The value parameters of an array action: inline in the action (R2013+)
/// or in the body's parameter section (R2007).
pub fn value_params<'a>(document: &'a Document, array: &AssocArray) -> &'a [AssocValueParam] {
    match action(document, array.action) {
        Some(action) if !action.values.is_empty() => &action.values,
        _ => body(document, array.body).map_or(&[], |body| &body.parameter_body.values),
    }
}

/// The number a value parameter holds, whatever its stored type.
pub fn param_value(params: &[AssocValueParam], name: &str) -> Option<f64> {
    let param = params.iter().find(|param| param.name == name)?;
    match &param.variables.first()?.value.value {
        AssocEvalValue::Real(value) => Some(*value),
        AssocEvalValue::Long(value) => Some(f64::from(*value)),
        AssocEvalValue::Short(value) => Some(f64::from(*value)),
        AssocEvalValue::Byte(value) => Some(f64::from(*value)),
        _ => None,
    }
}

/// Store `value` in a value parameter, keeping its stored type.
pub fn set_param_value(params: &mut [AssocValueParam], name: &str, value: f64) -> bool {
    let Some(variable) = params
        .iter_mut()
        .find(|param| param.name == name)
        .and_then(|param| param.variables.first_mut())
    else {
        return false;
    };
    variable.value.value = match variable.value.value {
        AssocEvalValue::Long(_) => AssocEvalValue::Long(value.round() as i32),
        AssocEvalValue::Short(_) => AssocEvalValue::Short(value.round() as i16),
        AssocEvalValue::Byte(_) => AssocEvalValue::Byte(value.round() as u8),
        _ => AssocEvalValue::Real(value),
    };
    true
}

/// The direction columns step along: the line of the action's "RowProfile"
/// parameter, X when the array has none.
pub fn column_axis(document: &Document, array: &AssocArray) -> DVec3 {
    let Some(action) = action(document, array.action) else {
        return DVec3::X;
    };
    action
        .owned_parameters
        .iter()
        .find_map(|handle| {
            let ObjectType::Associative(object) = document.objects.get(handle)? else {
                return None;
            };
            let AssociativeData::EdgeActionParam(edge) = &object.data else {
                return None;
            };
            if edge.single_dependency.action_param.name != "RowProfile" || edge.action_type != 19 {
                return None;
            }
            match edge.curve.get(1)? {
                codec::objects::AssocCurveValue::Point(direction) => {
                    DVec3::new(direction.x, direction.y, direction.z).try_normalize()
                }
                _ => None,
            }
        })
        .unwrap_or(DVec3::X)
}

/// The plane normal of an array: its "BaseNormal" vertex parameter, Z when
/// the array has none.
pub fn base_normal(document: &Document, array: &AssocArray) -> DVec3 {
    let Some(action) = action(document, array.action) else {
        return DVec3::Z;
    };
    action
        .owned_parameters
        .iter()
        .find_map(|handle| {
            let ObjectType::Associative(object) = document.objects.get(handle)? else {
                return None;
            };
            let AssociativeData::VertexActionParam(vertex) = &object.data else {
                return None;
            };
            if vertex.single_dependency.action_param.name != "BaseNormal" {
                return None;
            }
            DVec3::new(vertex.point.x, vertex.point.y, vertex.point.z).try_normalize()
        })
        .unwrap_or(DVec3::Z)
}

/// Item placements of a rectangular array in array-local coordinates:
/// (column, row, level) and the offset of the item's base point. Columns
/// step along `axis`; rows run at AxesAngle from it about `normal`, and
/// levels along `normal`.
pub fn rectangular_items(
    params: &[AssocValueParam],
    axis: DVec3,
    normal: DVec3,
) -> Vec<([i32; 3], DVec3)> {
    let get = |name: &str, default: f64| param_value(params, name).unwrap_or(default);
    let (columns, rows, levels) = (
        get("Items", 1.0).max(1.0) as i32,
        get("Rows", 1.0).max(1.0) as i32,
        get("Levels", 1.0).max(1.0) as i32,
    );
    let column_step = axis * get("ItemSpacing", 0.0);
    let axes = get("AxesAngle", std::f64::consts::FRAC_PI_2);
    let row_axis = glam::DQuat::from_axis_angle(normal, axes) * axis;
    let row_step = row_axis * get("RowSpacing", 0.0) + normal * get("RowElevation", 0.0);
    let level_step = normal * get("LevelSpacing", 0.0);
    let mut items = Vec::with_capacity((columns * rows * levels).max(0) as usize);
    for level in 0..levels {
        for row in 0..rows {
            for column in 0..columns {
                let offset = column_step * f64::from(column)
                    + row_step * f64::from(row)
                    + level_step * f64::from(level);
                items.push(([column, row, level], offset));
            }
        }
    }
    items
}

/// An array item's placement in array-local coordinates, as stored.
pub fn stored_item_transform(item: &codec::objects::AssocArrayItem) -> DMat4 {
    if item.uses_default_transform {
        let offset = item.x_direction;
        DMat4::from_translation(DVec3::new(offset.x, offset.y, offset.z))
    } else {
        DMat4::from_cols_array(&item.transform).transpose()
    }
}

/// The in-plane X axis angles are measured from: world X for a plan array,
/// the arbitrary-axis direction otherwise.
fn plane_x(normal: DVec3) -> DVec3 {
    if normal.x.abs() < 1.0 / 64.0 && normal.y.abs() < 1.0 / 64.0 {
        DVec3::Y.cross(normal).normalize()
    } else {
        DVec3::Z.cross(normal).normalize()
    }
}

/// The centre of a polar array, recovered from its first item and its
/// Radius and StartAngle.
pub fn polar_center(document: &Document, array: &AssocArray) -> Option<DVec3> {
    let params = value_params(document, array);
    let first = body(document, array.body)?
        .items
        .iter()
        .find(|item| item.location == [0, 0, 0])?;
    let normal = base_normal(document, array);
    let x = plane_x(normal);
    let y = normal.cross(x);
    let start = param_value(params, "StartAngle").unwrap_or(0.0);
    let radius = param_value(params, "Radius")?;
    Some(
        stored_item_transform(first).w_axis.truncate()
            - (x * start.cos() + y * start.sin()) * radius,
    )
}

/// Item placements of a polar array about `center`: each item turns about
/// the centre by its multiple of ItemSpacing (rotated with it unless
/// AlignItems is 0); rows step the radius out, levels step along the normal.
pub fn polar_items(
    params: &[AssocValueParam],
    center: DVec3,
    normal: DVec3,
) -> Vec<([i32; 3], DMat4)> {
    let get = |name: &str, default: f64| param_value(params, name).unwrap_or(default);
    let (count, rows, levels) = (
        get("Items", 1.0).max(1.0) as i32,
        get("Rows", 1.0).max(1.0) as i32,
        get("Levels", 1.0).max(1.0) as i32,
    );
    // ponytail: Direction 0 read as clockwise; no sample with 0 to confirm.
    let sign = if get("Direction", 1.0) == 0.0 {
        -1.0
    } else {
        1.0
    };
    let align = get("AlignItems", 1.0) != 0.0;
    let (spacing, start, radius) = (
        get("ItemSpacing", 0.0),
        get("StartAngle", 0.0),
        get("Radius", 0.0),
    );
    let x = plane_x(normal);
    let y = normal.cross(x);
    let start_dir = x * start.cos() + y * start.sin();
    let mut items = Vec::with_capacity((count * rows * levels).max(0) as usize);
    for level in 0..levels {
        for row in 0..rows {
            let ring = radius + f64::from(row) * get("RowSpacing", 0.0);
            let lift = normal
                * (f64::from(row) * get("RowElevation", 0.0)
                    + f64::from(level) * get("LevelSpacing", 0.0));
            for item in 0..count {
                let turn = glam::DQuat::from_axis_angle(normal, sign * f64::from(item) * spacing);
                let position = center + turn * (start_dir * ring) + lift;
                let placement = if align {
                    DMat4::from_rotation_translation(turn, position)
                } else {
                    DMat4::from_translation(position)
                };
                items.push(([item, row, level], placement));
            }
        }
    }
    items
}

/// The curve a path array follows: what its read dependency on a drawing
/// object (not the source block) watches.
pub fn path_entity(document: &Document, array: &AssocArray) -> Option<(Handle, EntityType)> {
    action(document, array.action)?
        .dependencies
        .iter()
        .find_map(|link| {
            let dependency = dependency(document, link.dependency)?;
            if !dependency.is_read_dependency
                || dependency.is_write_dependency
                || dependency.dependent_on == array.source_block
            {
                return None;
            }
            let entity = document.get_entity(dependency.dependent_on)?;
            Some((dependency.dependent_on, entity.clone()))
        })
}

/// World to array-block coordinates: the inverse of the array reference's
/// transform (identity when the reference is missing).
fn array_block_from_world(document: &Document, insert: Handle) -> DMat4 {
    let Some(EntityType::Insert(reference)) = document.get_entity(insert) else {
        return DMat4::IDENTITY;
    };
    let transform = crate::scene::render_graph::insert_transform(document, reference);
    let at = |x: f64, y: f64, z: f64| {
        let p = transform.apply(codec::types::Vector3::new(x, y, z));
        DVec3::new(p.x, p.y, p.z)
    };
    let origin = at(0.0, 0.0, 0.0);
    DMat4::from_cols(
        (at(1.0, 0.0, 0.0) - origin).extend(0.0),
        (at(0.0, 1.0, 0.0) - origin).extend(0.0),
        (at(0.0, 0.0, 1.0) - origin).extend(0.0),
        origin.extend(1.0),
    )
    .inverse()
}

/// The items of a path array along `path`: `Items` of them `ItemSpacing`
/// apart from the path's start (its end when `PathDirection` is 0), spread
/// over the whole path when `Method` is 0 (Divide), turned to the travel
/// direction when `AlignItems` is set. Rows step to the left of the path
/// and levels up.
pub fn path_items(params: &[AssocValueParam], path: &EntityType) -> Vec<([i32; 3], DMat4)> {
    let get = |name: &str, default: f64| param_value(params, name).unwrap_or(default);
    let Some(curve) = crate::entities::curve::entity_curve(path) else {
        return Vec::new();
    };
    let length = curve.length();
    if !length.is_finite() || length <= 0.0 {
        return Vec::new();
    }
    let at = |distance: f64| DVec3::from_array(curve.point_at_distance(distance.clamp(0.0, length)));
    let count = get("Items", 1.0).clamp(1.0, 100_000.0) as i32;
    let (rows, levels) = (
        get("Rows", 1.0).clamp(1.0, 1000.0) as i32,
        get("Levels", 1.0).clamp(1.0, 1000.0) as i32,
    );
    let divide = param_value(params, "Method") == Some(0.0);
    let spacing = if !divide {
        get("ItemSpacing", 0.0)
    } else if curve.is_closed() {
        length / f64::from(count)
    } else if count > 1 {
        length / f64::from(count - 1)
    } else {
        0.0
    };
    let reversed = get("PathDirection", 1.0) == 0.0;
    let align = get("AlignItems", 1.0) != 0.0;
    let step = (length * 1e-6).max(1e-9);
    let mut items = Vec::with_capacity((count * rows * levels).max(0) as usize);
    for level in 0..levels {
        for row in 0..rows {
            for item in 0..count {
                let along = (spacing * f64::from(item)).min(length);
                let distance = if reversed { length - along } else { along };
                let mut tangent = at(distance + step) - at(distance - step);
                if reversed {
                    tangent = -tangent;
                }
                let tangent = tangent.truncate().try_normalize().unwrap_or(glam::DVec2::X);
                let left = DVec3::new(-tangent.y, tangent.x, 0.0);
                let position = at(distance)
                    + left * (f64::from(row) * get("RowSpacing", 0.0))
                    + DVec3::Z
                        * (f64::from(row) * get("RowElevation", 0.0)
                            + f64::from(level) * get("LevelSpacing", 0.0));
                let placement = if align {
                    DMat4::from_rotation_translation(
                        glam::DQuat::from_rotation_z(tangent.y.atan2(tangent.x)),
                        position,
                    )
                } else {
                    DMat4::from_translation(position)
                };
                items.push(([item, row, level], placement));
            }
        }
    }
    items
}

/// The rows the Properties palette shows for an array: (field, parameter,
/// label, kind) where kind 0 is a count, 1 a length and 2 an angle.
fn property_rows(kind: ArrayKind) -> &'static [(&'static str, &'static str, &'static str, u8)] {
    match kind {
        ArrayKind::Rectangular => &[
            ("assoc_array_columns", "Items", "Columns", 0),
            (
                "assoc_array_column_spacing",
                "ItemSpacing",
                "Column spacing",
                1,
            ),
            ("assoc_array_rows", "Rows", "Rows", 0),
            ("assoc_array_row_spacing", "RowSpacing", "Row spacing", 1),
            ("assoc_array_levels", "Levels", "Levels", 0),
            (
                "assoc_array_level_spacing",
                "LevelSpacing",
                "Level spacing",
                1,
            ),
        ],
        ArrayKind::Polar => &[
            ("assoc_array_items", "Items", "Items", 0),
            ("assoc_array_fill_angle", "FillAngle", "Fill angle", 2),
            ("assoc_array_radius", "Radius", "Radius", 1),
            ("assoc_array_rows", "Rows", "Rows", 0),
            ("assoc_array_row_spacing", "RowSpacing", "Row spacing", 1),
            ("assoc_array_levels", "Levels", "Levels", 0),
            (
                "assoc_array_level_spacing",
                "LevelSpacing",
                "Level spacing",
                1,
            ),
        ],
        ArrayKind::Path => &[
            ("assoc_array_items", "Items", "Items", 0),
            ("assoc_array_item_spacing", "ItemSpacing", "Item spacing", 1),
            ("assoc_array_rows", "Rows", "Rows", 0),
            ("assoc_array_row_spacing", "RowSpacing", "Row spacing", 1),
            ("assoc_array_levels", "Levels", "Levels", 0),
            (
                "assoc_array_level_spacing",
                "LevelSpacing",
                "Level spacing",
                1,
            ),
        ],
    }
}

/// The Properties section of an associative array.
pub fn property_section(
    document: &Document,
    array: &AssocArray,
) -> crate::scene::model::object::PropSection {
    use crate::entities::common::{format_angle, format_length};
    use crate::scene::model::object::{PropSection, PropValue, Property};
    let params = value_params(document, array);
    // A path array whose path is gone keeps its items as they are.
    let fixed = array.kind == ArrayKind::Path && path_entity(document, array).is_none();
    let props = property_rows(array.kind)
        .iter()
        .filter_map(|&(field, name, label, kind)| {
            let value = param_value(params, name)?;
            let text = match kind {
                0 => format!("{}", value.round() as i64),
                1 => format_length(value),
                _ => format_angle(value),
            };
            Some(Property {
                label: crate::t!(label).into_owned(),
                field,
                value: if fixed {
                    PropValue::ReadOnly(text)
                } else {
                    PropValue::EditText(text)
                },
            })
        })
        .collect();
    let title = match array.kind {
        ArrayKind::Rectangular => "Array (Rectangular)",
        ArrayKind::Polar => "Array (Polar)",
        ArrayKind::Path => "Array (Path)",
    };
    PropSection {
        title: crate::t!(title).into_owned(),
        props,
    }
}

/// Write a Properties edit of an array row: the parameter changes and the
/// items are placed again. `text` is the typed value (counts, lengths in
/// drawing units, angles as typed angles).
pub fn apply_field(
    scene: &mut crate::scene::Scene,
    array: &AssocArray,
    field: &str,
    text: &str,
) -> Result<(), String> {
    let Some(&(_, name, _, kind)) = property_rows(array.kind).iter().find(|row| row.0 == field)
    else {
        return Err("unknown array field".into());
    };
    let value = match kind {
        0 => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| *v >= 1.0 && *v <= 100_000.0)
            .map(f64::round),
        1 => crate::entities::common::parse_typed_length(text),
        _ => crate::entities::common::parse_typed_angle(text),
    }
    .filter(|v| v.is_finite())
    .ok_or("invalid value")?;
    update(scene, array, &[(name, value)])
}

/// Change an array's parameters and place its items again: the item
/// INSERTs in the array block, the body's item records and the parameter
/// values move together, so the array stays associative on save.
pub fn update(
    scene: &mut crate::scene::Scene,
    array: &AssocArray,
    changes: &[(&str, f64)],
) -> Result<(), String> {
    use codec::objects::AssocArrayItem;
    let document = &scene.document;
    let mut params = value_params(document, array).to_vec();
    let center = match array.kind {
        ArrayKind::Polar => polar_center(document, array).ok_or("array has no first item")?,
        _ => DVec3::ZERO,
    };
    for (name, value) in changes {
        set_param_value(&mut params, name, *value);
    }
    if array.kind == ArrayKind::Polar
        && changes
            .iter()
            .any(|(name, _)| matches!(*name, "Items" | "FillAngle"))
    {
        let fill = param_value(&params, "FillAngle").unwrap_or(std::f64::consts::TAU);
        let count = param_value(&params, "Items").unwrap_or(1.0).max(1.0);
        let spacing = if (fill.abs() - std::f64::consts::TAU).abs() < 1e-9 {
            fill / count
        } else if count > 1.0 {
            fill / (count - 1.0)
        } else {
            0.0
        };
        set_param_value(&mut params, "ItemSpacing", spacing);
    }
    let normal = base_normal(document, array);
    let path = (array.kind == ArrayKind::Path)
        .then(|| path_entity(document, array))
        .flatten();
    let placements: Vec<([i32; 3], DMat4)> = match array.kind {
        ArrayKind::Rectangular => rectangular_items(&params, column_axis(document, array), normal)
            .into_iter()
            .map(|(location, offset)| (location, DMat4::from_translation(offset)))
            .collect(),
        ArrayKind::Polar => polar_items(&params, center, normal),
        ArrayKind::Path => {
            let (_, path) = path.as_ref().ok_or("the array's path is gone")?;
            // Divide keeps the count and spreads it over the path.
            if param_value(&params, "Method") == Some(0.0) {
                if let Some(curve) = crate::entities::curve::entity_curve(path) {
                    let gaps = param_value(&params, "Items").unwrap_or(1.0)
                        - if curve.is_closed() { 0.0 } else { 1.0 };
                    set_param_value(&mut params, "ItemSpacing", curve.length() / gaps.max(1.0));
                }
            }
            // The path gives world placements; the items live in the array
            // block, so undo where the array reference puts that block.
            let to_block = array_block_from_world(document, array.insert);
            path_items(&params, path)
                .into_iter()
                .map(|(location, placement)| (location, to_block * placement))
                .collect()
        }
    };
    if placements.is_empty() {
        return Err("the array has no items to place".into());
    }
    let current = body(document, array.body).ok_or("array body missing")?;
    let template = current
        .items
        .iter()
        .filter_map(|item| item.first_handle)
        .find_map(|handle| match document.get_entity(handle) {
            Some(EntityType::Insert(insert)) => Some(insert.clone()),
            _ => None,
        })
        .ok_or("array has no item reference")?;
    let old_items: Vec<Handle> = document
        .block_records
        .iter()
        .find(|record| record.handle == array.array_block)
        .map(|record| record.entity_handles.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|handle| matches!(document.get_entity(*handle), Some(EntityType::Insert(_))))
        .collect();

    let document = &mut scene.document;
    for handle in &old_items {
        document.remove_entity(*handle);
    }
    let mut items = Vec::with_capacity(placements.len());
    let mut new_handles = Vec::with_capacity(placements.len());
    for (location, placement) in placements {
        let mut insert = template.clone();
        insert.common.handle = Handle::NULL;
        insert.common.owner_handle = array.array_block;
        insert.common.reactors.clear();
        insert.common.xdictionary_handle = None;
        let position = placement.w_axis.truncate();
        insert.insert_point = codec::types::Vector3::new(position.x, position.y, position.z);
        insert.rotation = placement.x_axis.y.atan2(placement.x_axis.x);
        let handle = document
            .add_entity(EntityType::Insert(insert))
            .map_err(|error| error.to_string())?;
        let turned = (placement.x_axis.truncate() - DVec3::X).length() > 1e-12
            || (placement.y_axis.truncate() - DVec3::Y).length() > 1e-12;
        items.push(AssocArrayItem {
            location,
            flags: if turned { 12 } else { 8 },
            uses_default_transform: !turned,
            x_direction: if turned {
                codec::types::Vector3::ZERO
            } else {
                codec::types::Vector3::new(position.x, position.y, position.z)
            },
            transform: if turned {
                placement.transpose().to_cols_array()
            } else {
                [0.0; 16]
            },
            first_handle: Some(handle),
            ..Default::default()
        });
        new_handles.push(handle);
    }
    if let Some(record) = document
        .block_records
        .iter_mut()
        .find(|record| record.handle == array.source_block)
    {
        record
            .insert_handles
            .retain(|handle| !old_items.contains(handle));
        record.insert_handles.extend(new_handles);
    }
    let values_in_action = action(document, array.action).is_some_and(|a| !a.values.is_empty());
    if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&array.body) {
        let array_body = match &mut object.data {
            AssociativeData::ArrayActionBody(body) => Some(body),
            AssociativeData::ArrayModifyActionBody(modify) => Some(&mut modify.body),
            _ => None,
        };
        if let Some(array_body) = array_body {
            array_body.items = items;
            if !values_in_action {
                array_body.parameter_body.values = params.clone();
            }
        }
    }
    if values_in_action {
        if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&array.action) {
            if let AssociativeData::Action(action) = &mut object.data {
                action.values = params;
            }
        }
    }
    // The path's cached curve follows the path.
    if let Some((_, path)) = &path {
        if let Some((action_type, subcurve_kind, curve)) =
            crate::scene::model::sweep_model::assoc_edge_curve(path)
        {
            let owned = action(document, array.action)
                .map(|action| action.owned_parameters.clone())
                .unwrap_or_default();
            for handle in owned {
                if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&handle) {
                    if let AssociativeData::EdgeActionParam(edge) = &mut object.data {
                        if edge.single_dependency.action_param.name == "RowProfile" {
                            edge.action_type = action_type;
                            edge.subcurve_kind = subcurve_kind;
                            edge.curve = curve.clone();
                        }
                    }
                }
            }
        }
    }
    scene.bump_geometry();
    Ok(())
}

/// Class records the array objects need, with the values the reference
/// product writes: (DXF name, class name, DWG version, maintenance version).
const ARRAY_CLASSES: &[(&str, &str, i32, i32)] = &[
    ("ACDBASSOCNETWORK", "AcDbAssocNetwork", 27, 45),
    ("ACDBASSOCACTION", "AcDbAssocAction", 27, 45),
    ("ACDBASSOCDEPENDENCY", "AcDbAssocDependency", 27, 50),
    (
        "ACDBASSOCVERTEXACTIONPARAM",
        "AcDbAssocVertexActionParam",
        27,
        50,
    ),
    (
        "ACDBASSOCEDGEACTIONPARAM",
        "AcDbAssocEdgeActionParam",
        27,
        50,
    ),
    (
        "ACDBASSOCARRAYACTIONBODY",
        "AcDbAssocArrayActionBody",
        29,
        55,
    ),
    (
        "ACDBASSOCARRAYRECTANGULARPARAMETERS",
        "AcDbAssocArrayRectangularParameters",
        29,
        55,
    ),
    (
        "ACDBASSOCARRAYPOLARPARAMETERS",
        "AcDbAssocArrayPolarParameters",
        29,
        55,
    ),
    (
        "ACDBASSOCARRAYPATHPARAMETERS",
        "AcDbAssocArrayPathParameters",
        29,
        55,
    ),
];

fn register_array_classes(document: &mut Document) {
    for &(dxf_name, cpp_class_name, dwg_version, maintenance_version) in ARRAY_CLASSES {
        if document.classes.get_by_name(dxf_name).is_some() {
            continue;
        }
        document.classes.add_or_update(codec::classes::DxfClass {
            dxf_name: dxf_name.to_string(),
            cpp_class_name: cpp_class_name.to_string(),
            application_name: "ObjectDBX Classes".to_string(),
            proxy_flags: codec::classes::ProxyFlags(1025),
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            item_class_id: 0x1F3,
            dwg_version,
            maintenance_version,
            unknown1: 0,
            unknown2: 0,
        });
    }
}

pub(crate) fn next_anonymous_name(document: &Document) -> String {
    let next = document
        .block_records
        .iter()
        .filter_map(|record| record.name.strip_prefix("*U")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1);
    format!("*U{next}")
}

pub(crate) fn associative(
    handle: Handle,
    owner: Handle,
    dxf_name: &str,
    cpp_class_name: &str,
    data: AssociativeData,
) -> ObjectType {
    let mut object = codec::objects::AssociativeObject::new(dxf_name, cpp_class_name);
    object.handle = handle;
    object.owner = owner;
    object.data = data;
    ObjectType::Associative(object)
}

fn dictionary_entry(document: &Document, dictionary: Handle, key: &str) -> Option<Handle> {
    match document.objects.get(&dictionary)? {
        ObjectType::Dictionary(dictionary) => dictionary.get(key),
        _ => None,
    }
}

/// A dictionary stored under `key` in `parent`, created when absent.
fn child_dictionary(document: &mut Document, parent: Handle, key: &str) -> Handle {
    if let Some(handle) = dictionary_entry(document, parent, key).filter(|handle| {
        matches!(
            document.objects.get(handle),
            Some(ObjectType::Dictionary(_))
        )
    }) {
        return handle;
    }
    let handle = document.allocate_handle();
    let mut dictionary = codec::objects::Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = parent;
    dictionary.reactors.push(parent);
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(parent)) = document.objects.get_mut(&parent) {
        parent.add_entry(key, handle);
    }
    handle
}

/// The network holding the actions of model space, created with the root
/// network above it when the drawing has none.
fn model_space_network(document: &mut Document) -> Handle {
    let model = document.header.model_space_block_handle;
    block_network(document, model)
}

/// The network holding the actions of the space (block record) `owner`,
/// created with the root network above it when the drawing has none.
pub(crate) fn block_network(document: &mut Document, owner: Handle) -> Handle {
    const KEY: &str = "ACAD_ASSOCNETWORK";
    let scope = document.ensure_extension_dictionary(owner);
    let holder = child_dictionary(document, scope, KEY);
    if let Some(network) = dictionary_entry(document, holder, KEY).filter(|handle| {
        matches!(
            document.objects.get(handle),
            Some(ObjectType::Associative(object))
                if matches!(object.data, AssociativeData::Network(_))
        )
    }) {
        return network;
    }
    let root_holder = child_dictionary(document, document.header.named_objects_dict_handle, KEY);
    let root = match dictionary_entry(document, root_holder, KEY) {
        Some(root)
            if matches!(
                document.objects.get(&root),
                Some(ObjectType::Associative(_))
            ) =>
        {
            root
        }
        _ => {
            let root = document.allocate_handle();
            let network = codec::objects::AssocNetwork {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut object = associative(
                root,
                root_holder,
                "ACDBASSOCNETWORK",
                "AcDbAssocNetwork",
                AssociativeData::Network(network),
            );
            if let ObjectType::Associative(object) = &mut object {
                object.reactors.push(root_holder);
            }
            document.objects.insert(root, object);
            if let Some(ObjectType::Dictionary(holder)) = document.objects.get_mut(&root_holder) {
                holder.add_entry(KEY, root);
            }
            root
        }
    };
    let network = document.allocate_handle();
    let mut object = associative(
        network,
        holder,
        "ACDBASSOCNETWORK",
        "AcDbAssocNetwork",
        AssociativeData::Network(codec::objects::AssocNetwork {
            action: AssocAction {
                class_version: 2,
                owning_network: root,
                ..Default::default()
            },
            ..Default::default()
        }),
    );
    if let ObjectType::Associative(object) = &mut object {
        object.reactors.push(holder);
    }
    document.objects.insert(network, object);
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&holder) {
        dictionary.add_entry(KEY, network);
    }
    if let Some(ObjectType::Associative(object)) = document.objects.get_mut(&root) {
        if let AssociativeData::Network(root) = &mut object.data {
            root.network_action_index += 1;
            root.actions.push(codec::objects::AssocActionDependency {
                is_owned: false,
                dependency: network,
            });
        }
    }
    network
}

fn value_param(name: &str, unit_type: i32, value: f64, whole: bool) -> AssocValueParam {
    let variant = if whole {
        codec::objects::AssocEvalVariant {
            code: 90,
            value: AssocEvalValue::Long(value.round() as i32),
        }
    } else {
        codec::objects::AssocEvalVariant {
            code: 40,
            value: AssocEvalValue::Real(value),
        }
    };
    AssocValueParam {
        name: name.to_string(),
        unit_type,
        variables: vec![codec::objects::AssocValueParamVariable {
            value: variant,
            handle: Handle::NULL,
        }],
        ..Default::default()
    }
}

/// The value parameters of a new rectangular array, in the order the
/// reference product writes them.
pub fn rectangular_params(
    columns: u32,
    rows: u32,
    column_spacing: f64,
    row_spacing: f64,
) -> Vec<AssocValueParam> {
    vec![
        value_param("AlignItems", 0, 0.0, true),
        value_param("AxesAngle", 2, std::f64::consts::FRAC_PI_2, false),
        value_param("ItemSpacing", 1, column_spacing, false),
        value_param("Items", 0, f64::from(columns), true),
        value_param("LevelSpacing", 1, 1.0, false),
        value_param("Levels", 0, 1.0, true),
        value_param("RowElevation", 1, 0.0, false),
        value_param("RowSpacing", 1, row_spacing, false),
        value_param("Rows", 0, f64::from(rows), true),
    ]
}

/// The value parameters of a new polar array.
pub fn polar_params(
    items: u32,
    fill_angle: f64,
    radius: f64,
    start_angle: f64,
) -> Vec<AssocValueParam> {
    let count = f64::from(items.max(1));
    let full = (fill_angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
    let spacing = if full {
        fill_angle / count
    } else if items > 1 {
        fill_angle / (count - 1.0)
    } else {
        0.0
    };
    vec![
        value_param("FillAngle", 2, fill_angle, false),
        value_param("ItemSpacing", 2, spacing, false),
        value_param("Items", 0, count, true),
        value_param("LevelSpacing", 1, 1.0, false),
        value_param("Levels", 0, 1.0, true),
        value_param("RowElevation", 1, 0.0, false),
        value_param("RowSpacing", 1, radius.abs().max(1.0), false),
        value_param("Rows", 0, 1.0, true),
        value_param("Radius", 1, radius, false),
        value_param("StartAngle", 2, start_angle, false),
    ]
}

impl crate::scene::Scene {
    /// Place again the items of every path array whose path is among
    /// `changes`, so the array follows its path. Returns the array
    /// references it re-placed.
    pub(crate) fn refresh_associative_path_arrays(
        &mut self,
        changes: &[(Handle, crate::scene::ChangeKind)],
    ) -> Vec<(Handle, crate::scene::ChangeKind)> {
        let mut arrays: Vec<AssocArray> = Vec::new();
        for (handle, kind) in changes {
            if matches!(kind, crate::scene::ChangeKind::Removed) {
                continue;
            }
            let Some(entity) = self.document.get_entity(*handle) else {
                continue;
            };
            for reactor in &entity.common().reactors {
                let Some(ObjectType::Associative(object)) = self.document.objects.get(reactor)
                else {
                    continue;
                };
                let AssociativeData::Dependency(watch) = &object.data else {
                    continue;
                };
                if !watch.is_read_dependency || watch.is_write_dependency {
                    continue;
                }
                // The array reference is what the action both reads and writes.
                let insert = action(&self.document, object.owner).and_then(|action| {
                    action.dependencies.iter().find_map(|link| {
                        let link = dependency(&self.document, link.dependency)?;
                        (link.is_read_dependency && link.is_write_dependency)
                            .then_some(link.dependent_on)
                    })
                });
                let Some(array) = insert.and_then(|insert| array_of_insert(&self.document, insert))
                else {
                    continue;
                };
                if array.kind == ArrayKind::Path && !arrays.iter().any(|a| a.insert == array.insert)
                {
                    arrays.push(array);
                }
            }
        }
        if arrays.is_empty() {
            return Vec::new();
        }
        // The items are rebuilt outside the entity delta.
        self.poison_undo_recording();
        arrays
            .into_iter()
            .filter(|array| update(self, array, &[]).is_ok())
            .map(|array| (array.insert, crate::scene::ChangeKind::Modified))
            .collect()
    }
}

/// The value parameters of a new path array, in the order the reference
/// product writes them. Measure keeps `spacing`; Divide (Method 0, the path
/// filled) derives it from the path's length.
pub fn path_params(items: u32, spacing: f64, divide: bool, reversed: bool) -> Vec<AssocValueParam> {
    vec![
        value_param("ItemSpacing", 1, spacing, false),
        value_param("Items", 0, f64::from(items.max(1)), true),
        value_param("LevelSpacing", 1, 1.0, false),
        value_param("Levels", 0, 1.0, true),
        value_param("RowElevation", 1, 1.0, false),
        value_param("RowSpacing", 1, spacing.abs().max(1.0), false),
        value_param("Rows", 0, 1.0, true),
        value_param("PathDirection", 0, if reversed { 0.0 } else { 1.0 }, true),
        value_param("AlignItems", 0, 1.0, true),
        value_param("FillPath", 0, if divide { 1.0 } else { 0.0 }, false),
        value_param("MaintainZ", 0, 1.0, false),
        value_param("TangentOrientationAngle", 0, 0.0, false),
        value_param("Orientation", 0, 1.0, true),
        value_param("Method", 0, if divide { 0.0 } else { 1.0 }, true),
    ]
}

/// What ARRAYRECT / ARRAYPOLAR / ARRAYPATH ask for an associative array.
#[derive(Clone, Debug, PartialEq)]
pub enum AssocArraySpec {
    Rectangular {
        columns: u32,
        rows: u32,
        column_spacing: f64,
        row_spacing: f64,
    },
    Polar {
        center: DVec3,
        items: u32,
        fill_angle: f64,
    },
    Path {
        path: Handle,
        items: u32,
        spacing: f64,
        divide: bool,
        reversed: bool,
    },
}

/// Turn `sources` into an associative array: they move into an anonymous
/// source block whose origin is `source_point`, an anonymous array block
/// holds one reference to it per item, and an INSERT of the array block at
/// `base` takes their place, tied together by an array action in the
/// model-space network. A polar array turns about `base`. Returns the
/// array INSERT.
pub fn create(
    scene: &mut crate::scene::Scene,
    sources: &[Handle],
    source_point: DVec3,
    base: DVec3,
    kind: ArrayKind,
    params: Vec<AssocValueParam>,
    path: Option<Handle>,
) -> Result<Handle, String> {
    use codec::objects::{
        AssocActionBody, AssocActionDependency, AssocActionParam, AssocArrayItem,
        AssocSingleDependencyActionParam, AssocVertexActionParam,
    };
    let normal = DVec3::Z;
    let placements: Vec<([i32; 3], DMat4)> = match kind {
        ArrayKind::Rectangular => rectangular_items(&params, DVec3::X, normal)
            .into_iter()
            .map(|(location, offset)| (location, DMat4::from_translation(offset)))
            .collect(),
        ArrayKind::Polar => polar_items(&params, DVec3::ZERO, normal),
        ArrayKind::Path => {
            let curve = path
                .and_then(|path| scene.document.get_entity(path))
                .ok_or("the path is gone")?;
            path_items(&params, curve)
        }
    };
    if placements.is_empty() {
        return Err("the array has no items to place".into());
    }
    let mut entities: Vec<EntityType> = sources
        .iter()
        .filter_map(|handle| scene.document.get_entity(*handle).cloned())
        .collect();
    if entities.is_empty() {
        return Err("nothing to array".into());
    }
    let layer = scene.document.header.current_layer_name.clone();
    // The source block holds the objects as the first item draws them: a
    // path array's first item sits on the path, turned to it, so the
    // objects keep where they are drawn.
    let to_block = match kind {
        ArrayKind::Path => {
            let first = placements[0].1;
            let turn = first.x_axis.y.atan2(first.x_axis.x);
            vec![
                crate::command::EntityTransform::Translate(-first.w_axis.truncate()),
                crate::command::EntityTransform::Rotate {
                    center: DVec3::ZERO,
                    axis: DVec3::Z,
                    angle_rad: -turn,
                },
            ]
        }
        _ => vec![crate::command::EntityTransform::Translate(-source_point)],
    };
    for entity in &mut entities {
        for transform in &to_block {
            crate::scene::view::dispatch::apply_transform(entity, transform);
        }
    }
    register_array_classes(&mut scene.document);

    let originals: Vec<Handle> = sources
        .iter()
        .copied()
        .filter(|handle| scene.document.get_entity(*handle).is_some())
        .collect();
    let source_name = next_anonymous_name(&scene.document);
    scene.define_block_raw(&source_name, codec::types::Vector3::ZERO, entities);
    // The copies must not keep the sources' extension dictionaries.
    if let Some(record) = scene.document.block_records.get(&source_name) {
        let copies = record.entity_handles.clone();
        scene.own_copied_xdicts(originals, copies);
    }

    let item = |offset: DMat4| {
        let position = offset.w_axis.truncate();
        let mut insert = codec::entities::Insert::new(
            source_name.clone(),
            codec::types::Vector3::new(position.x, position.y, position.z),
        );
        insert.rotation = offset.x_axis.y.atan2(offset.x_axis.x);
        insert.common.layer = "0".to_string();
        insert.common.color = codec::types::Color::ByBlock;
        insert.common.linetype = "ByBlock".to_string();
        insert.common.line_weight = codec::types::LineWeight::ByBlock;
        insert.common.transparency = codec::types::Transparency::ByBlock;
        EntityType::Insert(insert)
    };
    let array_name = next_anonymous_name(&scene.document);
    scene.define_block_raw(
        &array_name,
        codec::types::Vector3::ZERO,
        placements
            .iter()
            .map(|(_, placement)| item(*placement))
            .collect(),
    );
    let record = |document: &Document, name: &str| {
        document
            .block_records
            .get(name)
            .map(|record| (record.handle, record.entity_handles.clone()))
    };
    let (source_block, _) = record(&scene.document, &source_name).ok_or("source block")?;
    let (array_block, item_handles) = record(&scene.document, &array_name).ok_or("array block")?;
    for name in [&source_name, &array_name] {
        if let Some(record) = scene.document.block_records.get_mut(name) {
            record.flags.anonymous = true;
            record.explodable = true;
            record.scale_uniformly = *name == array_name;
        }
    }
    if let Some(record) = scene.document.block_records.get_mut(&source_name) {
        record.insert_handles = item_handles.clone();
    }

    let mut array_insert = codec::entities::Insert::new(
        array_name.clone(),
        codec::types::Vector3::new(base.x, base.y, base.z),
    );
    array_insert.common.layer = layer;
    let insert = scene.add_entity(EntityType::Insert(array_insert));
    scene.erase_entities(sources);

    let document = &mut scene.document;
    let network = model_space_network(document);
    let [action, body, write_dep, read_dep, both_dep] =
        std::array::from_fn(|_| document.allocate_handle());
    let path_dep = path.map(|_| document.allocate_handle());
    let mut owned_parameters = Vec::new();
    let vertex_params = [
        ("BaseNormal", normal),
        ("Origin", DVec3::ZERO),
        ("basePoint", DVec3::ZERO),
    ];
    for (name, point) in vertex_params {
        let handle = document.allocate_handle();
        document.objects.insert(
            handle,
            associative(
                handle,
                action,
                "ACDBASSOCVERTEXACTIONPARAM",
                "AcDbAssocVertexActionParam",
                AssociativeData::VertexActionParam(AssocVertexActionParam {
                    single_dependency: AssocSingleDependencyActionParam {
                        action_param: AssocActionParam {
                            is_r2013: 1,
                            version: 0,
                            name: name.to_string(),
                        },
                        ..Default::default()
                    },
                    point: codec::types::Vector3::new(point.x, point.y, point.z),
                }),
            ),
        );
        owned_parameters.push(handle);
    }
    // A path array reads its path first, as the reference lists it, and
    // keeps the path's curve in its RowProfile parameter.
    let mut links = Vec::new();
    if let (Some(path), Some(path_dep)) = (path, path_dep) {
        links.push((path_dep, true, false, 0, path));
        if let Some(entity) = document.get_entity_mut(path) {
            entity.common_mut().reactors.push(path_dep);
        }
        let curve = document
            .get_entity(path)
            .and_then(crate::scene::model::sweep_model::assoc_edge_curve);
        if let Some((action_type, subcurve_kind, curve)) = curve {
            let handle = document.allocate_handle();
            document.objects.insert(
                handle,
                associative(
                    handle,
                    action,
                    "ACDBASSOCEDGEACTIONPARAM",
                    "AcDbAssocEdgeActionParam",
                    AssociativeData::EdgeActionParam(codec::objects::AssocEdgeActionParam {
                        single_dependency: AssocSingleDependencyActionParam {
                            action_param: AssocActionParam {
                                is_r2013: 1,
                                version: 0,
                                name: "RowProfile".to_string(),
                            },
                            dependency: path_dep,
                            ..Default::default()
                        },
                        parameter: Handle::NULL,
                        has_action: true,
                        action_type,
                        subcurve_kind,
                        curve,
                    }),
                ),
            );
            owned_parameters.push(handle);
        }
    }
    links.extend([
        (write_dep, false, true, 0, array_block),
        (read_dep, true, false, 0, source_block),
        (both_dep, true, true, i32::MIN, insert),
    ]);
    for (id, (handle, read, write, order, target)) in (1..).zip(links.iter().copied()) {
        document.objects.insert(
            handle,
            associative(
                handle,
                action,
                "ACDBASSOCDEPENDENCY",
                "AcDbAssocDependency",
                AssociativeData::Dependency(AssocDependency {
                    class_version: 2,
                    is_read_dependency: read,
                    is_write_dependency: write,
                    is_attached_to_object: true,
                    is_delegating_to_owning_action: true,
                    order,
                    dependent_on: target,
                    dependency_body_id: id,
                    ..Default::default()
                }),
            ),
        );
    }
    if let Some(EntityType::Insert(array)) = document.get_entity_mut(insert) {
        array.common.reactors.push(both_dep);
    }
    document.add_object_reactor(array_block, write_dep);
    document.add_object_reactor(source_block, read_dep);
    let items = placements
        .iter()
        .zip(&item_handles)
        .map(|((location, placement), handle)| {
            let position = placement.w_axis.truncate();
            let turned = (placement.x_axis.truncate() - DVec3::X).length() > 1e-12;
            AssocArrayItem {
                location: *location,
                flags: if turned { 12 } else { 8 },
                uses_default_transform: !turned,
                x_direction: if turned {
                    codec::types::Vector3::ZERO
                } else {
                    codec::types::Vector3::new(position.x, position.y, position.z)
                },
                transform: if turned {
                    placement.transpose().to_cols_array()
                } else {
                    [0.0; 16]
                },
                first_handle: Some(*handle),
                ..Default::default()
            }
        })
        .collect();
    let parameter_block = match kind {
        ArrayKind::Polar => "AcDbAssocArrayPolarParameters",
        ArrayKind::Path => "AcDbAssocArrayPathParameters",
        ArrayKind::Rectangular => "AcDbAssocArrayRectangularParameters",
    };
    document.objects.insert(
        body,
        associative(
            body,
            action,
            "ACDBASSOCARRAYACTIONBODY",
            "AcDbAssocArrayActionBody",
            AssociativeData::ArrayActionBody(AssocArrayActionBody {
                action_body: AssocActionBody { version: 2 },
                version: 1,
                parameter_block: parameter_block.to_string(),
                item_class: "AcDbAssocArrayItem".to_string(),
                items,
                transform: DMat4::from_translation(base).transpose().to_cols_array(),
                ..Default::default()
            }),
        ),
    );
    let action_index = match document.objects.get_mut(&network) {
        Some(ObjectType::Associative(object)) => match &mut object.data {
            AssociativeData::Network(network) => {
                network.network_action_index += 1;
                network.actions.push(AssocActionDependency {
                    is_owned: true,
                    dependency: action,
                });
                network.network_action_index
            }
            _ => 1,
        },
        _ => 1,
    };
    document.objects.insert(
        action,
        associative(
            action,
            network,
            "ACDBASSOCACTION",
            "AcDbAssocAction",
            AssociativeData::Action(AssocAction {
                class_version: 2,
                owning_network: network,
                action_body: body,
                action_index,
                max_dependency_index: links.len() as i32,
                dependencies: links
                    .iter()
                    .map(|&(dependency, ..)| AssocActionDependency {
                        is_owned: true,
                        dependency,
                    })
                    .collect(),
                owned_parameters,
                values: params,
                ..Default::default()
            }),
        ),
    );
    scene.bump_geometry();
    Ok(insert)
}

/// Build an associative array from `sources` per `spec`, its source block
/// based at their lower-left corner `source_point`.
pub fn create_from_spec(
    scene: &mut crate::scene::Scene,
    sources: &[Handle],
    source_point: DVec3,
    spec: &AssocArraySpec,
) -> Result<Handle, String> {
    match *spec {
        AssocArraySpec::Rectangular {
            columns,
            rows,
            column_spacing,
            row_spacing,
        } => create(
            scene,
            sources,
            source_point,
            source_point,
            ArrayKind::Rectangular,
            rectangular_params(columns, rows, column_spacing, row_spacing),
            None,
        ),
        AssocArraySpec::Polar {
            center,
            items,
            fill_angle,
        } => {
            let offset = source_point - center;
            let params = polar_params(
                items,
                fill_angle,
                offset.truncate().length(),
                offset.y.atan2(offset.x),
            );
            create(
                scene,
                sources,
                source_point,
                center,
                ArrayKind::Polar,
                params,
                None,
            )
        }
        AssocArraySpec::Path {
            path,
            items,
            spacing,
            divide,
            reversed,
        } => create(
            scene,
            sources,
            source_point,
            DVec3::ZERO,
            ArrayKind::Path,
            path_params(items, spacing, divide, reversed),
            Some(path),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::objects::{AssociativeData, ObjectType};

    #[test]
    fn saved_array_networks_keep_the_dxf_name_autocad_reads() {
        let mut scene = crate::scene::Scene::new();
        let circle = scene.add_entity(codec::EntityType::Circle(
            codec::entities::Circle::from_coords(0.0, 0.0, 0.0, 5.0),
        ));
        let spec = AssocArraySpec::Rectangular {
            columns: 3,
            rows: 3,
            column_spacing: 20.0,
            row_spacing: 20.0,
        };
        create_from_spec(&mut scene, &[circle], DVec3::ZERO, &spec).unwrap();
        // Saving rebuilds the networks around the array action.
        scene.sync_native_parametric_graph();

        let names: Vec<_> = scene
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Associative(object)
                    if matches!(object.data, AssociativeData::Network(_)) =>
                {
                    Some(object.dxf_name.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(names, ["ACDBASSOCNETWORK", "ACDBASSOCNETWORK"]);

        let dxf = codec::DxfWriter::new(&scene.document)
            .write_to_vec()
            .unwrap();
        let text = String::from_utf8_lossy(&dxf).replace("\r\n", "\n");
        assert!(!text.contains("\n  0\nASSOCNETWORK\n"), "dxf object name");
        assert_eq!(text.matches("\n  0\nACDBASSOCNETWORK\n").count(), 2);
    }

    fn assert_block_dependency_reactors(document: &Document, insert: Handle) {
        let array = array_of_insert(document, insert).unwrap();
        let action = action(document, array.action).unwrap();
        for (block, read, write, label) in [
            (array.array_block, false, true, "item"),
            (array.source_block, true, false, "source"),
        ] {
            assert!(
                document
                    .block_records
                    .iter()
                    .any(|record| { record.handle == block && record.flags.anonymous }),
                "{label} must be an anonymous BLOCK_RECORD"
            );
            let dependency_handle = action
                .dependencies
                .iter()
                .find_map(|link| {
                    let dep = dependency(document, link.dependency)?;
                    (dep.dependent_on == block
                        && dep.is_read_dependency == read
                        && dep.is_write_dependency == write)
                        .then_some(link.dependency)
                })
                .expect("array block dependency");
            assert!(
                document.object_reactors(block).contains(&dependency_handle),
                "{label} BLOCK_RECORD must carry its dependency reactor",
            );
        }
    }

    #[test]
    fn rectangular_array_block_records_carry_their_dependency_reactors() {
        let mut scene = crate::scene::Scene::new();
        let source = scene.add_entity(codec::EntityType::Circle(
            codec::entities::Circle::from_coords(0.0, 0.0, 0.0, 2.0),
        ));
        let spec = AssocArraySpec::Rectangular {
            columns: 2,
            rows: 2,
            column_spacing: 20.0,
            row_spacing: 20.0,
        };
        let insert = create_from_spec(&mut scene, &[source], DVec3::ZERO, &spec).unwrap();
        assert_block_dependency_reactors(&scene.document, insert);
    }

    #[test]
    fn path_array_block_records_carry_their_dependency_reactors() {
        for divide in [true, false] {
            let mut scene = crate::scene::Scene::new();
            let source = scene.add_entity(codec::EntityType::Circle(
                codec::entities::Circle::from_coords(0.0, 0.0, 0.0, 2.0),
            ));
            let path = scene.add_entity(codec::EntityType::Line(
                codec::entities::Line::from_coords(0.0, 0.0, 0.0, 60.0, 0.0, 0.0),
            ));
            let spec = AssocArraySpec::Path {
                path,
                items: 3,
                spacing: 20.0,
                divide,
                reversed: false,
            };
            let insert = create_from_spec(&mut scene, &[source], DVec3::ZERO, &spec).unwrap();
            assert_block_dependency_reactors(&scene.document, insert);
        }
    }

    #[test]
    fn path_array_moved_with_its_path_stays_on_the_path() {
        let mut scene = crate::scene::Scene::new();
        let source = scene.add_entity(codec::EntityType::Circle(
            codec::entities::Circle::from_coords(0.0, 0.0, 0.0, 2.0),
        ));
        let path = scene.add_entity(codec::EntityType::Line(codec::entities::Line::from_coords(
            0.0, 0.0, 0.0, 100.0, 0.0, 0.0,
        )));
        let spec = AssocArraySpec::Path {
            path,
            items: 5,
            spacing: 25.0,
            divide: false,
            reversed: false,
        };
        let insert = create_from_spec(&mut scene, &[source], DVec3::ZERO, &spec).unwrap();
        scene.transform_entities(
            &[path, insert],
            &crate::command::EntityTransform::Translate(DVec3::new(0.0, 20.0, 0.0)),
        );

        let document = &scene.document;
        let array = array_of_insert(document, insert).unwrap();
        let Some(EntityType::Insert(reference)) = document.get_entity(insert) else {
            panic!("array reference");
        };
        let to_world = crate::scene::render_graph::insert_transform(document, reference);
        let mut world: Vec<(f64, f64)> = document
            .block_records
            .iter()
            .find(|record| record.handle == array.array_block)
            .unwrap()
            .entity_handles
            .iter()
            .filter_map(|handle| match document.get_entity(*handle) {
                Some(EntityType::Insert(item)) => {
                    let p = to_world.apply(item.insert_point);
                    Some((p.x, p.y))
                }
                _ => None,
            })
            .collect();
        world.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(world.len(), 5);
        for (index, (x, y)) in world.into_iter().enumerate() {
            assert!((x - 25.0 * index as f64).abs() < 1e-9, "item {index} x {x}");
            assert!(
                (y - 20.0).abs() < 1e-9,
                "item {index} y {y} off the moved path"
            );
        }
    }
}

#[cfg(test)]
mod xdict_tests {
    use super::*;
    use crate::scene::Scene;
    use codec::entities::{AttributeEntity, Insert, Line};
    use codec::objects::{XRecord, XRecordValue};
    use codec::types::Vector3;
    use codec::EntityType;

    fn attach_test_record(scene: &mut Scene, owner: Handle, peer: Handle) -> (Handle, Handle) {
        let root = scene.document.ensure_extension_dictionary(owner);
        let record = scene.document.allocate_handle();
        let mut xrecord = XRecord::named("TEST");
        xrecord.handle = record;
        xrecord.owner = root;
        xrecord.reactors.push(root);
        xrecord.add_string(1, "payload");
        for (code, handle) in [(330, root), (340, owner), (350, peer), (360, record)] {
            xrecord.add_handle(code, handle);
        }
        scene
            .document
            .objects
            .insert(record, ObjectType::XRecord(xrecord));
        let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get_mut(&root) else {
            panic!("extension dictionary must exist");
        };
        dictionary.add_entry("TEST", record);
        dictionary.reactors.push(owner);
        (root, record)
    }

    fn assert_tree(scene: &Scene, owner: Handle, root: Handle, peer: Handle) {
        let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&root) else {
            panic!("copied dictionary must exist");
        };
        assert_eq!(dictionary.handle, root);
        assert_eq!(dictionary.owner, owner);
        assert_eq!(dictionary.reactors, vec![owner]);
        let record = dictionary.get("TEST").expect("TEST entry preserved");
        let Some(ObjectType::XRecord(xrecord)) = scene.document.objects.get(&record) else {
            panic!("copied TEST XRECORD must exist");
        };
        assert_eq!(xrecord.handle, record);
        assert_eq!(xrecord.owner, root);
        assert_eq!(xrecord.reactors, vec![root]);
        assert_eq!(xrecord.get_string(1), Some("payload"));
        for (code, handle) in [(330, root), (340, owner), (350, peer), (360, record)] {
            assert!(xrecord
                .entries
                .iter()
                .any(|entry| entry.code == code && entry.value == XRecordValue::Handle(handle)));
        }
    }

    #[test]
    fn associative_array_deep_clones_member_xdict() {
        for spec in [
            AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
            AssocArraySpec::Polar {
                center: DVec3::ZERO,
                items: 4,
                fill_angle: std::f64::consts::TAU,
            },
        ] {
            let mut scene = Scene::new();
            let a = scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            )));
            let b = scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_Y,
            )));
            let (root, record) = attach_test_record(&mut scene, a, b);
            scene
                .document
                .get_entity_mut(a)
                .unwrap()
                .common_mut()
                .reactors = vec![b, root];
            let root_before = scene.document.objects[&root].clone();
            let record_before = scene.document.objects[&record].clone();
            scene.begin_undo_recording();
            let insert = create_from_spec(&mut scene, &[a, b], DVec3::ZERO, &spec).unwrap();
            let array = array_of_insert(&scene.document, insert).unwrap();
            let copies = scene
                .document
                .block_records
                .iter()
                .find(|r| r.handle == array.source_block)
                .unwrap()
                .entity_handles
                .clone();
            assert_eq!(copies.len(), 2);
            let copy = scene.document.get_entity(copies[0]).unwrap();
            let copy_root = copy.common().xdictionary_handle.expect("copied dictionary");
            assert_ne!(
                copy_root, root,
                "copy must not share the source extension dictionary"
            );
            assert_eq!(copy.common().reactors, vec![copies[1]]);
            assert_tree(&scene, copies[0], copy_root, copies[1]);
            let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&copy_root)
            else {
                unreachable!();
            };
            assert_ne!(dictionary.get("TEST"), Some(record));
            assert!(scene.document.get_entity(a).is_none());
            assert!(scene.document.get_entity(b).is_none());
            assert_eq!(scene.document.objects.get(&root), Some(&root_before));
            assert_eq!(scene.document.objects.get(&record), Some(&record_before));
            assert!(scene.take_undo_recording().unwrap().is_poisoned());
            let copy_tree = scene.document.objects[&copy_root].clone();
            update(&mut scene, &array, &[("Items", 3.0)]).unwrap();
            assert_eq!(scene.document.objects.get(&copy_root), Some(&copy_tree));
            assert_tree(&scene, copies[0], copy_root, copies[1]);
            for entity in scene.document.entities() {
                if let Some(dictionary) = entity.common().xdictionary_handle {
                    assert_ne!(
                        dictionary, root,
                        "no live entity may claim the consumed source's dictionary"
                    );
                    let Some(ObjectType::Dictionary(d)) = scene.document.objects.get(&dictionary)
                    else {
                        panic!("live extension dictionary must exist");
                    };
                    assert_eq!(d.owner, entity.common().handle);
                }
            }
        }
    }

    #[test]
    fn associative_array_deep_clones_inline_attribute_xdict() {
        let mut scene = Scene::new();
        scene.define_block_raw("B", Vector3::ZERO, vec![]);
        let source = scene.add_entity(EntityType::Insert(Insert::new("B", Vector3::ZERO)));
        let attribute = scene.document.allocate_handle();
        let (root, record) = attach_test_record(&mut scene, attribute, source);
        let mut att = AttributeEntity::new("TAG".into(), "value".into());
        att.common.handle = attribute;
        att.common.owner_handle = source;
        att.common.xdictionary_handle = Some(root);
        att.common.reactors = vec![source];
        let Some(EntityType::Insert(insert)) = scene.document.get_entity_mut(source) else {
            unreachable!();
        };
        insert.attributes.push(att);
        let result = create_from_spec(
            &mut scene,
            &[source],
            DVec3::ZERO,
            &AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
        )
        .unwrap();
        let array = array_of_insert(&scene.document, result).unwrap();
        let copy = scene
            .document
            .block_records
            .iter()
            .find(|r| r.handle == array.source_block)
            .unwrap()
            .entity_handles[0];
        let Some(EntityType::Insert(insert)) = scene.document.get_entity(copy) else {
            unreachable!();
        };
        let att = &insert.attributes[0];
        let copy_root = att.common.xdictionary_handle.unwrap();
        assert_ne!(att.common.handle, attribute);
        assert_ne!(copy_root, root);
        assert_eq!(att.common.owner_handle, copy);
        assert_eq!(att.common.reactors, vec![copy]);
        assert_tree(&scene, att.common.handle, copy_root, copy);
        let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&copy_root)
        else {
            unreachable!();
        };
        assert_ne!(dictionary.get("TEST"), Some(record));
        assert!(scene
            .document
            .entities()
            .all(|entity| entity.common().xdictionary_handle != Some(root)));
    }
}
