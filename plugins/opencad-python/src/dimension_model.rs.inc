// Hand-written Dimension model, `include!`d next to the generated CRUD code.
//
// `EntityType::Dimension` wraps a nine-variant enum, so the generator cannot
// map it. Python sees one flat dict: `subtype` (fixed at creation), the
// shared base fields, and the fields of that subtype only. Fields that belong
// to another subtype are rejected rather than ignored. `actual_measurement`
// and the base `definition_point` are host-derived, never scripted.

enum DimField<'a> {
    V(&'a mut Vector3),
    F(&'a mut f64),
    B(&'a mut bool),
}

const DIM_SUBTYPES: &[&str] = &[
    "Aligned", "Linear", "Radius", "Diameter", "Angular2Ln", "Angular3Pt", "Ordinate", "Arc",
    "LargeRadial",
];
const DIM_BASE_KEYS: &[&str] = &[
    "text", "style_name", "normal", "text_middle_point", "attachment_point", "text_rotation",
    "horizontal_direction", "flip_arrow1", "flip_arrow2", "text_user_positioned",
];
const DIM_READ_ONLY_KEYS: &[&str] = &["subtype", "actual_measurement"];

fn dimension_subtype_name(dim: &codec::entities::Dimension) -> &'static str {
    use codec::entities::Dimension as D;
    match dim {
        D::Aligned(_) => "Aligned",
        D::Linear(_) => "Linear",
        D::Radius(_) => "Radius",
        D::Diameter(_) => "Diameter",
        D::Angular2Ln(_) => "Angular2Ln",
        D::Angular3Pt(_) => "Angular3Pt",
        D::Ordinate(_) => "Ordinate",
        D::Arc(_) => "Arc",
        D::LargeRadial(_) => "LargeRadial",
    }
}

/// Subtype-specific fields in stable order.
fn dimension_fields(dim: &mut codec::entities::Dimension) -> Vec<(&'static str, DimField<'_>)> {
    use codec::entities::Dimension as D;
    use DimField::{B, F, V};
    match dim {
        D::Aligned(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("first_point", V(&mut d.first_point)),
            ("second_point", V(&mut d.second_point)),
            ("ext_line_rotation", F(&mut d.ext_line_rotation)),
        ],
        D::Linear(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("first_point", V(&mut d.first_point)),
            ("second_point", V(&mut d.second_point)),
            ("rotation", F(&mut d.rotation)),
            ("ext_line_rotation", F(&mut d.ext_line_rotation)),
        ],
        D::Radius(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("angle_vertex", V(&mut d.angle_vertex)),
            ("leader_length", F(&mut d.leader_length)),
        ],
        D::Diameter(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("angle_vertex", V(&mut d.angle_vertex)),
            ("leader_length", F(&mut d.leader_length)),
        ],
        D::Angular2Ln(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("dimension_arc", V(&mut d.dimension_arc)),
            ("first_point", V(&mut d.first_point)),
            ("second_point", V(&mut d.second_point)),
            ("angle_vertex", V(&mut d.angle_vertex)),
        ],
        D::Angular3Pt(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("first_point", V(&mut d.first_point)),
            ("second_point", V(&mut d.second_point)),
            ("angle_vertex", V(&mut d.angle_vertex)),
        ],
        D::Ordinate(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("feature_location", V(&mut d.feature_location)),
            ("leader_endpoint", V(&mut d.leader_endpoint)),
            ("is_ordinate_type_x", B(&mut d.is_ordinate_type_x)),
        ],
        D::Arc(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("first_extension_point", V(&mut d.first_extension_point)),
            ("second_extension_point", V(&mut d.second_extension_point)),
            ("center_point", V(&mut d.center_point)),
            ("is_partial", B(&mut d.is_partial)),
            ("arc_start_parameter", F(&mut d.arc_start_parameter)),
            ("arc_end_parameter", F(&mut d.arc_end_parameter)),
            ("has_leader", B(&mut d.has_leader)),
            ("first_leader_point", V(&mut d.first_leader_point)),
            ("second_leader_point", V(&mut d.second_leader_point)),
        ],
        D::LargeRadial(d) => vec![
            ("definition_point", V(&mut d.definition_point)),
            ("chord_point", V(&mut d.chord_point)),
            ("jog_angle", F(&mut d.jog_angle)),
            ("override_center", V(&mut d.override_center)),
            ("jog_point", V(&mut d.jog_point)),
        ],
    }
}

/// Fields `ocs.add()` must receive (the geometry that defines the dimension).
fn dimension_required(subtype: &str) -> &'static [&'static str] {
    match subtype {
        "Aligned" | "Linear" => &["definition_point", "first_point", "second_point"],
        "Radius" | "Diameter" => &["definition_point", "angle_vertex"],
        "Angular2Ln" => &["definition_point", "dimension_arc", "first_point", "second_point", "angle_vertex"],
        "Angular3Pt" => &["definition_point", "first_point", "second_point", "angle_vertex"],
        "Ordinate" => &["feature_location", "leader_endpoint"],
        "Arc" => &["definition_point", "first_extension_point", "second_extension_point", "center_point"],
        "LargeRadial" => &["definition_point", "chord_point"],
        _ => &[],
    }
}

fn dimension_base_mut(dim: &mut codec::entities::Dimension) -> &mut codec::entities::DimensionBase {
    dim.base_mut()
}

fn dim_attachment_point_name(value: codec::entities::AttachmentPointType) -> &'static str {
    use codec::entities::AttachmentPointType as A;
    match value {
        A::TopLeft => "TopLeft",
        A::TopCenter => "TopCenter",
        A::TopRight => "TopRight",
        A::MiddleLeft => "MiddleLeft",
        A::MiddleCenter => "MiddleCenter",
        A::MiddleRight => "MiddleRight",
        A::BottomLeft => "BottomLeft",
        A::BottomCenter => "BottomCenter",
        A::BottomRight => "BottomRight",
    }
}

fn parse_dim_attachment_point(
    value: &str,
    vm: &VirtualMachine,
) -> PyResult<codec::entities::AttachmentPointType> {
    use codec::entities::AttachmentPointType as A;
    Ok(match value {
        "TopLeft" => A::TopLeft,
        "TopCenter" => A::TopCenter,
        "TopRight" => A::TopRight,
        "MiddleLeft" => A::MiddleLeft,
        "MiddleCenter" => A::MiddleCenter,
        "MiddleRight" => A::MiddleRight,
        "BottomLeft" => A::BottomLeft,
        "BottomCenter" => A::BottomCenter,
        "BottomRight" => A::BottomRight,
        other => {
            return Err(vm.new_value_error(format!("ocs: unsupported AttachmentPointType value: {other}")))
        }
    })
}

fn dimension_to_dict(
    vm: &VirtualMachine,
    dimension: &codec::entities::Dimension,
) -> PyResult<PyObjectRef> {
    let mut dim = dimension.clone();
    let dict = vm.ctx.new_dict();
    let base = dimension.base();
    dict.set_item("kind", vm.new_pyobj("Dimension"), vm)?;
    dict.set_item("handle", vm.new_pyobj(base.common.handle.value()), vm)?;
    dict.set_item("layer", vm.new_pyobj(base.common.layer.clone()), vm)?;
    dict.set_item("owner_handle", vm.new_pyobj(base.common.owner_handle.value()), vm)?;
    dict.set_item("subtype", vm.new_pyobj(dimension_subtype_name(dimension)), vm)?;
    dict.set_item("actual_measurement", vm.new_pyobj(base.actual_measurement), vm)?;
    dict.set_item("text", vm.new_pyobj(base.text.clone()), vm)?;
    dict.set_item("style_name", vm.new_pyobj(base.style_name.clone()), vm)?;
    dict.set_item("normal", vector3_to_py_dict(vm, &base.normal)?, vm)?;
    dict.set_item("text_middle_point", vector3_to_py_dict(vm, &base.text_middle_point)?, vm)?;
    dict.set_item("attachment_point", vm.new_pyobj(dim_attachment_point_name(base.attachment_point)), vm)?;
    dict.set_item("text_rotation", vm.new_pyobj(base.text_rotation), vm)?;
    dict.set_item("horizontal_direction", vm.new_pyobj(base.horizontal_direction), vm)?;
    dict.set_item("flip_arrow1", vm.new_pyobj(base.flip_arrow1), vm)?;
    dict.set_item("flip_arrow2", vm.new_pyobj(base.flip_arrow2), vm)?;
    dict.set_item("text_user_positioned", vm.new_pyobj(base.text_user_positioned), vm)?;
    for (name, field) in dimension_fields(&mut dim) {
        let value: PyObjectRef = match field {
            DimField::V(v) => vector3_to_py_dict(vm, v)?,
            DimField::F(v) => vm.new_pyobj(*v),
            DimField::B(v) => vm.new_pyobj(*v),
        };
        dict.set_item(name, value, vm)?;
    }
    Ok(dict.into())
}

/// Merge `dict` onto `dim`. With `creating`, geometry the subtype needs must
/// be present so a script cannot silently default a point to the origin.
fn dimension_merge(
    dim: &mut codec::entities::Dimension,
    dict: &rustpython_vm::builtins::PyDictRef,
    creating: bool,
    vm: &VirtualMachine,
) -> PyResult<()> {
    let subtype = dimension_subtype_name(dim);
    let applicable: Vec<&'static str> =
        dimension_fields(&mut dim.clone()).into_iter().map(|(name, _)| name).collect();
    for key in dict.keys_vec() {
        let key = key.try_into_value::<String>(vm)?;
        let key = key.as_str();
        if matches!(key, "kind" | "handle" | "layer" | "owner_handle") || DIM_BASE_KEYS.contains(&key) {
            continue;
        }
        if key == "subtype" {
            let value = dict.get_item("subtype", vm)?.try_into_value::<String>(vm)?;
            if value != subtype {
                return Err(vm.new_value_error("Dimension.subtype is read-only".to_owned()));
            }
            continue;
        }
        if key == "actual_measurement" {
            return Err(vm.new_value_error("Dimension.actual_measurement is read-only".to_owned()));
        }
        if !applicable.contains(&key) {
            let known = [
                "definition_point", "first_point", "second_point", "angle_vertex", "dimension_arc",
                "feature_location", "leader_endpoint", "first_extension_point", "second_extension_point",
                "center_point", "first_leader_point", "second_leader_point", "chord_point",
                "override_center", "jog_point", "rotation", "ext_line_rotation", "leader_length",
                "arc_start_parameter", "arc_end_parameter", "jog_angle", "is_ordinate_type_x",
                "is_partial", "has_leader",
            ];
            return Err(vm.new_value_error(if known.contains(&key) {
                format!("Dimension.{key} does not apply to the {subtype} subtype")
            } else {
                format!("ocs: Dimension property {key:?} is outside the editable schema")
            }));
        }
    }
    if creating {
        for name in dimension_required(subtype) {
            if dict.get_item_opt(*name, vm)?.is_none() {
                return Err(vm.new_value_error(format!("ocs.add: Dimension {subtype} requires {name}")));
            }
        }
    }
    for (name, field) in dimension_fields(dim) {
        let Some(value) = dict.get_item_opt(name, vm)? else { continue };
        match field {
            DimField::V(target) => *target = py_to_vector3_dict(value, vm)?,
            DimField::F(target) => *target = py_number_to_f64(value, vm)?,
            DimField::B(target) => *target = value.try_into_value::<bool>(vm)?,
        }
    }
    let base = dimension_base_mut(dim);
    if let Some(v) = dict.get_item_opt("text", vm)? { base.text = v.try_into_value::<String>(vm)?; }
    if let Some(v) = dict.get_item_opt("style_name", vm)? { base.style_name = v.try_into_value::<String>(vm)?; }
    if let Some(v) = dict.get_item_opt("normal", vm)? { base.normal = py_to_vector3_dict(v, vm)?; }
    if let Some(v) = dict.get_item_opt("text_middle_point", vm)? { base.text_middle_point = py_to_vector3_dict(v, vm)?; }
    if let Some(v) = dict.get_item_opt("attachment_point", vm)? {
        base.attachment_point = parse_dim_attachment_point(&v.try_into_value::<String>(vm)?, vm)?;
    }
    if let Some(v) = dict.get_item_opt("text_rotation", vm)? { base.text_rotation = py_number_to_f64(v, vm)?; }
    if let Some(v) = dict.get_item_opt("horizontal_direction", vm)? { base.horizontal_direction = py_number_to_f64(v, vm)?; }
    if let Some(v) = dict.get_item_opt("flip_arrow1", vm)? { base.flip_arrow1 = v.try_into_value::<bool>(vm)?; }
    if let Some(v) = dict.get_item_opt("flip_arrow2", vm)? { base.flip_arrow2 = v.try_into_value::<bool>(vm)?; }
    if let Some(v) = dict.get_item_opt("text_user_positioned", vm)? { base.text_user_positioned = v.try_into_value::<bool>(vm)?; }
    Ok(())
}

fn dimension_from_dict(
    dict: &rustpython_vm::builtins::PyDictRef,
    vm: &VirtualMachine,
) -> PyResult<codec::entities::Dimension> {
    use codec::entities::*;
    let subtype = match dict.get_item_opt("subtype", vm)? {
        Some(v) => v.try_into_value::<String>(vm)?,
        None => {
            return Err(vm.new_value_error(format!(
                "ocs.add: Dimension requires subtype, one of {DIM_SUBTYPES:?}"
            )))
        }
    };
    let mut dim = match subtype.as_str() {
        "Aligned" => Dimension::Aligned(DimensionAligned::default()),
        "Linear" => Dimension::Linear(DimensionLinear::default()),
        "Radius" => Dimension::Radius(DimensionRadius::default()),
        "Diameter" => Dimension::Diameter(DimensionDiameter::default()),
        "Angular2Ln" => Dimension::Angular2Ln(DimensionAngular2Ln::default()),
        "Angular3Pt" => Dimension::Angular3Pt(DimensionAngular3Pt::default()),
        "Ordinate" => Dimension::Ordinate(DimensionOrdinate::default()),
        "Arc" => Dimension::Arc(DimensionArc::default()),
        "LargeRadial" => Dimension::LargeRadial(DimensionLargeRadial::default()),
        other => {
            return Err(vm.new_value_error(format!(
                "ocs.add: unsupported Dimension subtype {other:?}, expected one of {DIM_SUBTYPES:?}"
            )))
        }
    };
    dimension_merge(&mut dim, dict, true, vm)?;
    let base = dimension_base_mut(&mut dim);
    base.common.handle = codec::Handle::new(get_opt_u64(dict, "handle", vm)?);
    base.common.layer = get_opt_string(dict, "layer", "0", vm)?;
    base.common.owner_handle = codec::Handle::new(get_opt_u64(dict, "owner_handle", vm)?);
    Ok(dim)
}

fn dimension_apply(
    existing: &codec::entities::Dimension,
    dict: &rustpython_vm::builtins::PyDictRef,
    vm: &VirtualMachine,
) -> PyResult<codec::entities::Dimension> {
    let mut dim = existing.clone();
    dimension_merge(&mut dim, dict, false, vm)?;
    let base = dimension_base_mut(&mut dim);
    if let Some(v) = dict.get_item_opt("layer", vm)? {
        if !vm.is_none(&v) {
            base.common.layer = v.try_into_value::<String>(vm)?;
        }
    }
    if let Some(owner) = dict.get_item_opt("owner_handle", vm)? {
        if owner.try_into_value::<u64>(vm)? != base.common.owner_handle.value() {
            return Err(vm.new_value_error("owner_handle is read-only after creation".to_owned()));
        }
    }
    Ok(dim)
}
