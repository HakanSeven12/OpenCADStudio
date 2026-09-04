//! OCS-facing integration for non-graphical DWG object graphs.
//!
//! Dynamic-block evaluation records, solid construction history and
//! associative networks do not draw standalone geometry. Their native place in
//! OCS is the selected entity's semantic/property view, while the evaluated
//! entity geometry continues through the normal render pipeline.

use acadrust::objects::{
    AssociativeData, BlockEvalValue, DynamicBlockData, ObjectType,
    SolidHistoryNodeBase, SolidHistoryOperation,
};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::model::object::{PropSection, PropValue, Property};

#[derive(Debug, Clone, Default)]
pub struct ObjectDataCache {
    document_sections: std::sync::Arc<Vec<PropSection>>,
    dynamic_objects: std::sync::Arc<Vec<Handle>>,
    associative_objects: std::sync::Arc<Vec<Handle>>,
    light_entities: std::sync::Arc<Vec<Handle>>,
    sun_objects: std::sync::Arc<Vec<Handle>>,
    geo_objects: std::sync::Arc<Vec<Handle>>,
}

pub fn build_cache(document: &CadDocument) -> ObjectDataCache {
    let mut dynamic_objects = Vec::new();
    let mut associative_objects = Vec::new();
    let mut ordered_lights = Vec::new();
    let mut sun_objects = Vec::new();
    let mut geo_objects = Vec::new();
    for (handle, object) in &document.objects {
        match object {
            ObjectType::DynamicBlock(_) => dynamic_objects.push(*handle),
            ObjectType::Associative(_) => associative_objects.push(*handle),
            ObjectType::GeoData(_) => geo_objects.push(*handle),
            ObjectType::ClassObject(value) => match &value.data {
                acadrust::objects::ClassObjectData::LightList(list) => {
                    ordered_lights.extend(list.lights.iter().map(|entry| entry.handle));
                }
                acadrust::objects::ClassObjectData::Sun(_) => sun_objects.push(*handle),
                _ => {}
            },
            _ => {}
        }
    }
    let mut seen_lights = rustc_hash::FxHashSet::default();
    ordered_lights.retain(|handle| seen_lights.insert(*handle));
    for entity in document.entities() {
        if let EntityType::Light(light) = entity {
            if seen_lights.insert(light.common.handle) {
                ordered_lights.push(light.common.handle);
            }
        }
    }
    dynamic_objects.sort_by_key(|handle| handle.value());
    associative_objects.sort_by_key(|handle| handle.value());
    sun_objects.sort_by_key(|handle| handle.value());
    geo_objects.sort_by_key(|handle| handle.value());
    ObjectDataCache {
        document_sections: std::sync::Arc::new(build_document_sections(document)),
        dynamic_objects: std::sync::Arc::new(dynamic_objects),
        associative_objects: std::sync::Arc::new(associative_objects),
        light_entities: std::sync::Arc::new(ordered_lights),
        sun_objects: std::sync::Arc::new(sun_objects),
        geo_objects: std::sync::Arc::new(geo_objects),
    }
}

pub fn cache_is_prepared(cache: &ObjectDataCache) -> bool {
    !cache.document_sections.is_empty()
}

pub fn light_entities(cache: &ObjectDataCache) -> &[Handle] {
    &cache.light_entities
}

pub fn sun_objects(cache: &ObjectDataCache) -> &[Handle] {
    &cache.sun_objects
}

pub fn geo_objects(cache: &ObjectDataCache) -> &[Handle] {
    &cache.geo_objects
}

pub fn update_light_entity(
    cache: &mut ObjectDataCache,
    handle: Handle,
    exists: bool,
) {
    let lights = std::sync::Arc::make_mut(&mut cache.light_entities);
    if exists {
        if !lights.contains(&handle) {
            lights.push(handle);
        }
    } else {
        lights.retain(|candidate| *candidate != handle);
    }
}

fn ro(label: impl Into<String>, value: impl Into<String>) -> Property {
    Property {
        label: label.into(),
        field: "dwg_object_data",
        value: PropValue::ReadOnly(value.into()),
    }
}

fn handle_text(handle: Handle) -> String {
    if handle.is_valid() {
        format!("{:X}", handle.value())
    } else {
        "None".to_string()
    }
}

fn entity_history_handle(entity: &EntityType) -> Option<Handle> {
    match entity {
        EntityType::Solid3D(value) => value.history_handle,
        EntityType::Region(value) => value.history_handle,
        EntityType::Body(value) => value.history_handle,
        EntityType::Surface(value) => value.history_handle,
        _ => None,
    }
    .filter(|handle| handle.is_valid())
}

fn history_operation_name(operation: &SolidHistoryOperation) -> &'static str {
    match operation {
        SolidHistoryOperation::Unknown => "Unknown",
        SolidHistoryOperation::Box(_) => "Box",
        SolidHistoryOperation::Wedge(_) => "Wedge",
        SolidHistoryOperation::Sphere(_) => "Sphere",
        SolidHistoryOperation::Cylinder(_) => "Cylinder",
        SolidHistoryOperation::Cone(_) => "Cone",
        SolidHistoryOperation::Pyramid(_) => "Pyramid",
        SolidHistoryOperation::Torus(_) => "Torus",
        SolidHistoryOperation::Boolean(_) => "Boolean",
        SolidHistoryOperation::Brep(_) => "Brep",
        SolidHistoryOperation::Fillet(_) => "Fillet",
        SolidHistoryOperation::Chamfer(_) => "Chamfer",
        SolidHistoryOperation::Sweep(_) => "Sweep",
        SolidHistoryOperation::Extrusion(_) => "Extrusion",
        SolidHistoryOperation::Loft(_) => "Loft",
        SolidHistoryOperation::Revolve(_) => "Revolve",
    }
}

fn history_step_id(operation: &SolidHistoryOperation) -> Option<i32> {
    let base = match operation {
        SolidHistoryOperation::Unknown => return None,
        SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) => &value.base,
        SolidHistoryOperation::Sphere(value) => &value.base,
        SolidHistoryOperation::Cylinder(value) => &value.base,
        SolidHistoryOperation::Cone(value) => &value.base,
        SolidHistoryOperation::Pyramid(value) => &value.base,
        SolidHistoryOperation::Torus(value) => &value.base,
        SolidHistoryOperation::Boolean(value) => &value.base,
        SolidHistoryOperation::Brep(value) => &value.base,
        SolidHistoryOperation::Fillet(value) => &value.base,
        SolidHistoryOperation::Chamfer(value) => &value.base,
        SolidHistoryOperation::Sweep(value) | SolidHistoryOperation::Extrusion(value) => {
            &value.base
        }
        SolidHistoryOperation::Loft(value) => &value.base,
        SolidHistoryOperation::Revolve(value) => &value.base,
    };
    Some(base.step_id)
}

fn history_base_text(base: &SolidHistoryNodeBase) -> String {
    format!(
        "node {}; version {}.{}; material {}; color {:?}; transform [{:.4}, {:.4}, {:.4}, {:.4}; {:.4}, {:.4}, {:.4}, {:.4}; {:.4}, {:.4}, {:.4}, {:.4}; {:.4}, {:.4}, {:.4}, {:.4}]",
        base.eval.node_id,
        base.major,
        base.minor,
        handle_text(base.material),
        base.color,
        base.transform[0],
        base.transform[1],
        base.transform[2],
        base.transform[3],
        base.transform[4],
        base.transform[5],
        base.transform[6],
        base.transform[7],
        base.transform[8],
        base.transform[9],
        base.transform[10],
        base.transform[11],
        base.transform[12],
        base.transform[13],
        base.transform[14],
        base.transform[15],
    )
}

fn embedded_name(value: Option<&acadrust::entities::EmbeddedEntity>) -> &'static str {
    match value {
        Some(acadrust::entities::EmbeddedEntity::Point(_)) => "Point",
        Some(acadrust::entities::EmbeddedEntity::Line(_)) => "Line",
        Some(acadrust::entities::EmbeddedEntity::Arc(_)) => "Arc",
        Some(acadrust::entities::EmbeddedEntity::Circle(_)) => "Circle",
        Some(acadrust::entities::EmbeddedEntity::Ellipse(_)) => "Ellipse",
        Some(acadrust::entities::EmbeddedEntity::Spline(_)) => "Spline",
        Some(acadrust::entities::EmbeddedEntity::LwPolyline(_)) => "Polyline",
        Some(acadrust::entities::EmbeddedEntity::Region(_)) => "Region",
        Some(acadrust::entities::EmbeddedEntity::Ray(_)) => "Ray",
        Some(acadrust::entities::EmbeddedEntity::XLine(_)) => "XLine",
        Some(acadrust::entities::EmbeddedEntity::Unknown { .. }) => "Unknown",
        None => "None",
    }
}

fn history_operation_text(operation: &SolidHistoryOperation) -> String {
    match operation {
        SolidHistoryOperation::Unknown => "Unknown operation".to_string(),
        SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) => format!(
            "{}; length {:.6}; width {:.6}; height {:.6}",
            history_base_text(&value.base),
            value.length,
            value.width,
            value.height
        ),
        SolidHistoryOperation::Sphere(value) => format!(
            "{}; radius {:.6}",
            history_base_text(&value.base),
            value.radius
        ),
        SolidHistoryOperation::Cylinder(value) => format!(
            "{}; height {:.6}; radii {:.6}/{:.6}; x-radius {:.6}",
            history_base_text(&value.base),
            value.height,
            value.major_radius,
            value.minor_radius,
            value.x_radius
        ),
        SolidHistoryOperation::Cone(value) => format!(
            "{}; height {:.6}; base radii {:.6}/{:.6}; top radius {:.6}",
            history_base_text(&value.base),
            value.height,
            value.base_x_radius,
            value.base_y_radius,
            value.top_radius
        ),
        SolidHistoryOperation::Pyramid(value) => format!(
            "{}; height {:.6}; sides {}; radii {:.6}/{:.6}",
            history_base_text(&value.base),
            value.height,
            value.sides,
            value.radius,
            value.top_radius
        ),
        SolidHistoryOperation::Torus(value) => format!(
            "{}; radii {:.6}/{:.6}",
            history_base_text(&value.base),
            value.major_radius,
            value.minor_radius
        ),
        SolidHistoryOperation::Boolean(value) => format!(
            "{}; operation {}; operands {}/{}",
            history_base_text(&value.base),
            value.operation,
            value.first_operand,
            value.second_operand
        ),
        SolidHistoryOperation::Brep(value) => format!(
            "{}; ACIS {} bytes; materials {}",
            history_base_text(&value.base),
            value.acis_data.sat_data.len() + value.acis_data.sab_data.len(),
            value.acis_data.materials.len()
        ),
        SolidHistoryOperation::Fillet(value) => format!(
            "{}; method {}; edges {}; radii {}; setbacks {}/{}",
            history_base_text(&value.base),
            value.method,
            value.edges.len(),
            value.radii.len(),
            value.start_setbacks.len(),
            value.end_setbacks.len()
        ),
        SolidHistoryOperation::Chamfer(value) => format!(
            "{}; method {}; distances {:.6}/{:.6}; edges {}; base face {}",
            history_base_text(&value.base),
            value.method,
            value.base_distance,
            value.other_distance,
            value.edges.len(),
            value.base_face
        ),
        SolidHistoryOperation::Sweep(value) | SolidHistoryOperation::Extrusion(value) => format!(
            "{}; sweep {}; path {}; direction {:.6},{:.6},{:.6}; draft {:.6}; scale {:.6}; twist {:.6}; align {:.6}; bank {}; intersections {}",
            history_base_text(&value.base),
            embedded_name(value.sweep_entity.as_ref()),
            embedded_name(value.path_entity.as_ref()),
            value.direction.x,
            value.direction.y,
            value.direction.z,
            value.draft_angle,
            value.scale_factor,
            value.twist_angle,
            value.align_angle,
            value.bank,
            value.check_intersections
        ),
        SolidHistoryOperation::Loft(value) => format!(
            "{}; cross sections {}; guides {}",
            history_base_text(&value.base),
            value.cross_sections.len(),
            value.guides.len()
        ),
        SolidHistoryOperation::Revolve(value) => format!(
            "{}; profile {}; axis point {:.6},{:.6},{:.6}; direction {:.6},{:.6}; angle {:.6}; start {:.6}; draft {:.6}; twist {:.6}; close-to-axis {}",
            history_base_text(&value.base),
            embedded_name(value.sweep_entity.as_ref()),
            value.axis_point.x,
            value.axis_point.y,
            value.axis_point.z,
            value.direction.x,
            value.direction.y,
            value.revolve_angle,
            value.start_angle,
            value.draft_angle,
            value.twist_angle,
            value.close_to_axis
        ),
    }
}

fn solid_history_sections(
    document: &CadDocument,
    cache: &ObjectDataCache,
    entity: &EntityType,
) -> Vec<PropSection> {
    let Some(history_handle) = entity_history_handle(entity) else {
        return Vec::new();
    };
    let Some(ObjectType::DynamicBlock(history_object)) =
        document.objects.get(&history_handle)
    else {
        return vec![PropSection {
            title: "Solid History".to_string(),
            props: vec![
                ro("Handle", handle_text(history_handle)),
                ro("Status", "Referenced history object is unresolved"),
            ],
        }];
    };
    let DynamicBlockData::SolidHistory(history) = &history_object.data else {
        return vec![PropSection {
            title: "Solid History".to_string(),
            props: vec![
                ro("Handle", handle_text(history_handle)),
                ro("Status", "Referenced object is not a solid history"),
            ],
        }];
    };
    let mut nodes = cache
        .dynamic_objects
        .iter()
        .filter_map(|handle| match document.objects.get(handle) {
            Some(ObjectType::DynamicBlock(value))
                if document.owner_chain_reaches(value.owner, history_handle) =>
            {
                match &value.data {
                    DynamicBlockData::SolidHistoryNode(node) => Some(node),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| history_step_id(node).unwrap_or(i32::MAX));

    let mut sections = vec![PropSection {
        title: "Solid History".to_string(),
        props: vec![
            ro("Handle", handle_text(history_handle)),
            ro("Version", format!("{}.{}", history.major, history.minor)),
            ro("Root node", history.history_node_id.to_string()),
            ro("Show history", history.show_history.to_string()),
            ro("Record history", history.record_history.to_string()),
            ro("Steps", nodes.len().to_string()),
        ],
    }];
    if !nodes.is_empty() {
        sections.push(PropSection {
            title: "Construction Steps".to_string(),
            props: nodes
                .into_iter()
                .map(|node| {
                    let step = history_step_id(node)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    ro(
                        format!("Step {step} · {}", history_operation_name(node)),
                        history_operation_text(node),
                    )
                })
                .collect(),
        });
    }
    sections
}

fn block_value_text(value: &BlockEvalValue) -> String {
    match value {
        BlockEvalValue::None => "None".to_string(),
        BlockEvalValue::Real(value) => format!("{value:.6}"),
        BlockEvalValue::Point(value) => format!("{:.6}, {:.6}", value[0], value[1]),
        BlockEvalValue::Text(value) => value.clone(),
        BlockEvalValue::Long(value) => value.to_string(),
        BlockEvalValue::Handle(value) => handle_text(*value),
        BlockEvalValue::Short(value) => value.to_string(),
    }
}

fn dynamic_data_text(data: &DynamicBlockData) -> String {
    match data {
        DynamicBlockData::Unknown => "Unknown".to_string(),
        DynamicBlockData::Representation(value) => {
            format!("block {}; flags {}", handle_text(value.block), value.flags)
        }
        DynamicBlockData::ProxyNode(value) => format!(
            "node {}; parent {}; version {}.{}; value {}",
            value.node_id,
            value.parent_id,
            value.major,
            value.minor,
            block_value_text(&value.value)
        ),
        DynamicBlockData::GripLocationComponent(value) => format!(
            "grip type {}; expression {}; value {}",
            value.grip_type,
            value.expression,
            block_value_text(&value.eval.value)
        ),
        DynamicBlockData::AlignmentGrip(value) | DynamicBlockData::LinearGrip(value) => format!(
            "{} at {:.6},{:.6},{:.6}; orientation {:.6},{:.6},{:.6}; cycling {}",
            value.grip.element.name,
            value.grip.location.x,
            value.grip.location.y,
            value.grip.location.z,
            value.orientation.x,
            value.orientation.y,
            value.orientation.z,
            value.grip.insert_cycling
        ),
        DynamicBlockData::FlipGrip(value) => format!(
            "{} at {:.6},{:.6},{:.6}; state {}; orientation {:.6},{:.6},{:.6}",
            value.grip.element.name,
            value.grip.location.x,
            value.grip.location.y,
            value.grip.location.z,
            value.combined_state,
            value.orientation.x,
            value.orientation.y,
            value.orientation.z
        ),
        DynamicBlockData::LookupGrip(value)
        | DynamicBlockData::PolarGrip(value)
        | DynamicBlockData::RotationGrip(value)
        | DynamicBlockData::VisibilityGrip(value)
        | DynamicBlockData::XYGrip(value)
        | DynamicBlockData::PropertiesTableGrip(value) => format!(
            "{} at {:.6},{:.6},{:.6}; flags {}/{}; cycling {} weight {}",
            value.element.name,
            value.location.x,
            value.location.y,
            value.location.z,
            value.flags_91,
            value.flags_92,
            value.insert_cycling,
            value.insert_cycling_weight
        ),
        DynamicBlockData::AlignmentParameter(value) => format!(
            "{}; perpendicular {}; base {:.6},{:.6},{:.6}; end {:.6},{:.6},{:.6}",
            value.parameter.parameter.element.name,
            value.align_perpendicular,
            value.parameter.definition_base_point.x,
            value.parameter.definition_base_point.y,
            value.parameter.definition_base_point.z,
            value.parameter.definition_end_point.x,
            value.parameter.definition_end_point.y,
            value.parameter.definition_end_point.z
        ),
        DynamicBlockData::BasePointParameter(value) => format!(
            "{}; point {:.6},{:.6},{:.6}; base {:.6},{:.6},{:.6}",
            value.parameter.parameter.element.name,
            value.point.x,
            value.point.y,
            value.point.z,
            value.base_point.x,
            value.base_point.y,
            value.base_point.z
        ),
        DynamicBlockData::FlipParameter(value) => format!(
            "{}; labels {}/{}; tooltip {}",
            value.parameter.parameter.element.name,
            value.base_state_label,
            value.flipped_state_label,
            value.tooltip
        ),
        DynamicBlockData::LinearParameter(value) => format!(
            "{}; {} = {:.6}; range {:.6}..{:.6}; increment {:.6}; values {}",
            value.parameter.parameter.element.name,
            value.distance_name,
            value.distance,
            value.value_set.minimum,
            value.value_set.maximum,
            value.value_set.increment,
            value.value_set.values.len()
        ),
        DynamicBlockData::LookupParameter(value) => format!(
            "{}; {} index {}; description {}",
            value.parameter.parameter.element.name,
            value.lookup_name,
            value.index,
            value.lookup_description
        ),
        DynamicBlockData::PointParameter(value) => format!(
            "{}; {}; label at {:.6},{:.6},{:.6}",
            value.parameter.parameter.element.name,
            value.position_name,
            value.definition_label_point.x,
            value.definition_label_point.y,
            value.definition_label_point.z
        ),
        DynamicBlockData::PolarParameter(value) => format!(
            "{}; angle {}; distance {}; offset {:.6}",
            value.parameter.parameter.element.name,
            value.angle_name,
            value.distance_name,
            value.offset
        ),
        DynamicBlockData::RotationParameter(value) => format!(
            "{}; {} = {:.6}; range {:.6}..{:.6}; values {}",
            value.parameter.parameter.element.name,
            value.angle_name,
            value.angle,
            value.value_set.minimum,
            value.value_set.maximum,
            value.value_set.values.len()
        ),
        DynamicBlockData::UserParameter(value) => format!(
            "{}; expression {}; value {}; variable {}",
            value.parameter.parameter.element.name,
            value.expression,
            block_value_text(&value.value),
            handle_text(value.associated_variable)
        ),
        DynamicBlockData::VisibilityParameter(value) => format!(
            "{}; states {}; members {}; point {:.6},{:.6},{:.6}",
            value.name,
            value.states.len(),
            value.all_blocks.len(),
            value.def_point.x,
            value.def_point.y,
            value.def_point.z
        ),
        DynamicBlockData::XYParameter(value) => format!(
            "{}; {} = {:.6}; {} = {:.6}",
            value.parameter.parameter.element.name,
            value.x_label,
            value.x_value,
            value.y_label,
            value.y_value
        ),
        DynamicBlockData::AngularConstraintParameter(value) => format!(
            "{} = {:.6}; dependency {}",
            value.expression_name,
            value.angle,
            handle_text(value.constraint.dependency)
        ),
        DynamicBlockData::DiametricConstraintParameter(value)
        | DynamicBlockData::RadialConstraintParameter(value) => format!(
            "{} = {:.6}; dependency {}",
            value.expression_name,
            value.distance,
            handle_text(value.constraint.dependency)
        ),
        DynamicBlockData::AlignedConstraintParameter(value)
        | DynamicBlockData::LinearConstraintParameter(value)
        | DynamicBlockData::HorizontalConstraintParameter(value)
        | DynamicBlockData::VerticalConstraintParameter(value) => format!(
            "{} = {:.6}; dependency {}",
            value.expression_name,
            value.value,
            handle_text(value.constraint.dependency)
        ),
        DynamicBlockData::ParameterDependencyBody(value) => format!(
            "{}; dependency version {}; dimension version {}; class version {}",
            value.name,
            value.dependency_body_version,
            value.dimension_base_version,
            value.class_version
        ),
        DynamicBlockData::MoveAction(value) => format!(
            "{}; dependencies {}; actions {}; offsets {:.6},{:.6}; angle {:.6}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.action.action_ids.len(),
            value.offsets.offset_x,
            value.offsets.offset_y,
            value.offsets.angle_offset
        ),
        DynamicBlockData::FlipAction(value) => format!(
            "{}; dependencies {}; actions {}; connections {}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.action.action_ids.len(),
            value.connections.len()
        ),
        DynamicBlockData::RotateAction(value) | DynamicBlockData::ScaleAction(value) => format!(
            "{}; dependencies {}; actions {}; base {:.6},{:.6},{:.6}; dependent {}",
            value.action.action.element.name,
            value.action.action.dependencies.len(),
            value.action.action.action_ids.len(),
            value.action.base_point.x,
            value.action.base_point.y,
            value.action.base_point.z,
            value.action.dependent
        ),
        DynamicBlockData::ArrayAction(value) => format!(
            "{}; dependencies {}; columns offset {:.6}; rows offset {:.6}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.column_offset,
            value.row_offset
        ),
        DynamicBlockData::LookupAction(value) => format!(
            "{}; dependencies {}; table {}×{}; expressions {}; rows {}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.row_count,
            value.column_count,
            value.expressions.len(),
            value.rows.len()
        ),
        DynamicBlockData::StretchAction(value) => format!(
            "{}; dependencies {}; points {}; handles {}; codes {}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.points.len(),
            value.handles.len(),
            value.codes.len()
        ),
        DynamicBlockData::PolarStretchAction(value) => format!(
            "{}; dependencies {}; points {}; handles {}; codes {}",
            value.action.element.name,
            value.action.dependencies.len(),
            value.points.len(),
            value.handles.len(),
            value.codes.len()
        ),
        DynamicBlockData::EvaluationGraph(value) => format!(
            "first node {}; nodes {}; edges {}",
            value.first_node_id,
            value.nodes.len(),
            value.edges.len()
        ),
        DynamicBlockData::AngularConstraintParameterEntity(value) => format!(
            "{} = {:.6}; dependency {}",
            value.expression_name,
            value.angle,
            handle_text(value.constraint.dependency)
        ),
        DynamicBlockData::SolidHistory(value) => format!(
            "version {}.{}; root {}; show {}; record {}",
            value.major,
            value.minor,
            value.history_node_id,
            value.show_history,
            value.record_history
        ),
        DynamicBlockData::SolidHistoryNode(value) => history_operation_text(value),
        DynamicBlockData::PropertiesTable
        | DynamicBlockData::AlignmentParameterEntity
        | DynamicBlockData::BasePointParameterEntity
        | DynamicBlockData::FlipParameterEntity
        | DynamicBlockData::LinearParameterEntity
        | DynamicBlockData::PointParameterEntity
        | DynamicBlockData::RotationParameterEntity
        | DynamicBlockData::VisibilityParameterEntity
        | DynamicBlockData::XYParameterEntity
        | DynamicBlockData::FlipGripEntity
        | DynamicBlockData::LinearGripEntity
        | DynamicBlockData::PolarGripEntity
        | DynamicBlockData::RotationGripEntity
        | DynamicBlockData::VisibilityGripEntity
        | DynamicBlockData::XYGripEntity => "Semantic entity marker".to_string(),
    }
}

fn dynamic_block_sections(
    document: &CadDocument,
    cache: &ObjectDataCache,
    handle: Handle,
) -> Vec<PropSection> {
    let Some(definition) = document.dynamic_definition_for_insert(handle) else {
        return Vec::new();
    };
    let mut objects = cache
        .dynamic_objects
        .iter()
        .filter_map(|object_handle| match document.objects.get(object_handle) {
            Some(ObjectType::DynamicBlock(value))
                if document.owner_chain_reaches(value.owner, definition) =>
            {
                Some(value)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let visibility = document.block_visibility_param_for_def(definition);
    if objects.is_empty() && visibility.is_none() {
        return Vec::new();
    }
    objects.sort_by_key(|object| object.handle.value());

    let mut overview = vec![
        ro("Definition", handle_text(definition)),
        ro("Native records", objects.len().to_string()),
    ];
    if let Some(parameter) = visibility {
        overview.push(ro("Visibility", parameter.name.clone()));
        overview.push(ro(
            "Visibility states",
            parameter
                .states
                .iter()
                .map(|state| state.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    let mut sections = vec![PropSection {
        title: "Dynamic Block".to_string(),
        props: overview,
    }];
    if !objects.is_empty() {
        sections.push(PropSection {
            title: "Dynamic Parameters / Actions".to_string(),
            props: objects
                .into_iter()
                .filter(|object| {
                    !matches!(
                        object.data,
                        DynamicBlockData::SolidHistory(_)
                            | DynamicBlockData::SolidHistoryNode(_)
                    )
                })
                .map(|object| {
                    let class = if object.dxf_name.is_empty() {
                        object.cpp_class_name.as_str()
                    } else {
                        object.dxf_name.as_str()
                    };
                    ro(
                        format!("{} · {}", handle_text(object.handle), class),
                        dynamic_data_text(&object.data),
                    )
                })
                .collect(),
        });
    }
    sections
}

fn associative_data_text(data: &AssociativeData) -> String {
    match data {
        AssociativeData::Unknown => "Unknown".to_string(),
        AssociativeData::Dependency(value) => format!(
            "status {}; read {}; write {}; attached {}; target {}; body {}",
            value.status,
            value.is_read_dependency,
            value.is_write_dependency,
            value.is_attached_to_object,
            handle_text(value.dependent_on),
            handle_text(value.dependency_body)
        ),
        AssociativeData::ValueDependency(value) => format!(
            "{}; target {}; value {:?}",
            value.name,
            handle_text(value.dependency.dependent_on),
            value.value.value
        ),
        AssociativeData::GeomDependency(value) => format!(
            "target {}; enabled {}; subentity {}",
            handle_text(value.dependency.dependent_on),
            value.enabled,
            value.persistent_subent.class_name
        ),
        AssociativeData::Action(value) => format!(
            "status {}; network {}; body {}; dependencies {}; parameters {}; values {}",
            value.geometry_status,
            handle_text(value.owning_network),
            handle_text(value.action_body),
            value.dependencies.len(),
            value.owned_parameters.len(),
            value.values.len()
        ),
        AssociativeData::Network(value) => format!(
            "actions {}; owned {}; dependencies {}; values {}",
            value.actions.len(),
            value.owned_actions.len(),
            value.action.dependencies.len(),
            value.action.values.len()
        ),
        AssociativeData::ConstraintGroup(value) => format!(
            "status {}; dependency {}; actions {}; nodes {}",
            value.action.geometry_status,
            handle_text(value.dependency),
            value.actions.len(),
            value.nodes.len()
        ),
        AssociativeData::Variable(value) => format!(
            "{} = {:?}; expression {}; evaluator {}; values {}",
            value.name,
            value.value.value,
            value.expression,
            value.evaluator,
            value.action.values.len()
        ),
        AssociativeData::ArrayParameters(value) => format!(
            "{}; items {}; rows {}; levels {}",
            value.class_name,
            value.items.len(),
            value.row_count,
            value.level_count
        ),
        AssociativeData::ArrayActionBody(value) => format!(
            "parameter block {}; dependencies {}; value parameters {}",
            value.parameter_block,
            value.parameter_body.dependencies.len(),
            value.parameter_body.values.len()
        ),
        AssociativeData::ArrayModifyActionBody(value) => format!(
            "items {}; dependencies {}; value parameters {}",
            value.item_locations.len(),
            value.body.parameter_body.dependencies.len(),
            value.body.parameter_body.values.len()
        ),
        AssociativeData::DimensionAssociation(value) => format!(
            "dimension {}; associativity {}; references {}",
            handle_text(value.dimension),
            value.associativity,
            value.references.iter().flatten().count()
        ),
        other => format!("{} semantic payload", associative_data_name(other)),
    }
}

fn associative_data_name(data: &AssociativeData) -> &'static str {
    match data {
        AssociativeData::Unknown => "Unknown",
        AssociativeData::Dependency(_) => "Dependency",
        AssociativeData::ValueDependency(_) => "Value Dependency",
        AssociativeData::GeomDependency(_) => "Geometry Dependency",
        AssociativeData::SurfaceActionBody(_) => "Surface Action",
        AssociativeData::Action(_) => "Action",
        AssociativeData::Network(_) => "Network",
        AssociativeData::AnnotationActionBody(_) => "Annotation Action",
        AssociativeData::PersSubentManager(_) => "Persistent Subentity Manager",
        AssociativeData::EdgeActionParam(_) => "Edge Parameter",
        AssociativeData::ConstraintGroup(_) => "Constraint Group",
        AssociativeData::Variable(_) => "Variable",
        AssociativeData::ActionParam(_) => "Action Parameter",
        AssociativeData::CompoundActionParam(_) => "Compound Parameter",
        AssociativeData::OsnapPointRefActionParam(_) => "Object Snap Parameter",
        AssociativeData::PointRefActionParam(_) => "Point Reference",
        AssociativeData::ObjectActionParam(_) => "Object Parameter",
        AssociativeData::PathActionParam(_) => "Path Parameter",
        AssociativeData::DimDependencyBody(_) => "Dimension Dependency",
        AssociativeData::FaceActionParam(_) => "Face Parameter",
        AssociativeData::VertexActionParam(_) => "Vertex Parameter",
        AssociativeData::AsmBodyActionParam(_) => "ASM Body Parameter",
        AssociativeData::ArrayParameters(_) => "Array Parameters",
        AssociativeData::ArrayActionBody(_) => "Array Action",
        AssociativeData::ArrayModifyActionBody(_) => "Array Modify Action",
        AssociativeData::DimensionAssociation(_) => "Dimension Association",
        AssociativeData::PersSubentManagerStatic(_) => "Static Subentity Manager",
        AssociativeData::ViewRepActionBody(_) => "View Representation Action",
        AssociativeData::ViewObjectActionParam(_) => "View Object Parameter",
        AssociativeData::ViewRepHatchManager(_) => "View Hatch Manager",
        AssociativeData::ViewRepHatchActionParam(_) => "View Hatch Parameter",
        AssociativeData::ViewLabelActionParam(_) => "View Label Parameter",
    }
}

fn associative_sections(
    document: &CadDocument,
    cache: &ObjectDataCache,
    handle: Handle,
) -> Vec<PropSection> {
    let mut objects = cache
        .associative_objects
        .iter()
        .filter_map(|object_handle| match document.objects.get(object_handle) {
            Some(ObjectType::Associative(value))
                if document.owner_chain_reaches(value.owner, handle)
                    || value.references_handle(handle) =>
            {
                Some(value)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if objects.is_empty() {
        return Vec::new();
    }
    objects.sort_by_key(|object| object.handle.value());
    vec![PropSection {
        title: "Associative Data".to_string(),
        props: objects
            .into_iter()
            .map(|object| {
                let class = if object.dxf_name.is_empty() {
                    object.cpp_class_name.as_str()
                } else {
                    object.dxf_name.as_str()
                };
                ro(
                    format!("{} · {}", handle_text(object.handle), class),
                    associative_data_text(&object.data),
                )
            })
            .collect(),
    }]
}

/// Semantic DWG object sections connected to one selected entity.
pub fn sections(
    document: &CadDocument,
    cache: &ObjectDataCache,
    handle: Handle,
    entity: &EntityType,
) -> Vec<PropSection> {
    let mut sections = solid_history_sections(document, cache, entity);
    if matches!(entity, EntityType::Insert(_)) {
        sections.extend(dynamic_block_sections(document, cache, handle));
    }
    if let EntityType::Table(table) = entity {
        let mut linked = rustc_hash::FxHashSet::default();
        for row in &table.rows {
            for cell in &row.cells {
                if let Some(link_handle) = cell.data_link_handle {
                    if linked.insert(link_handle) {
                        if let Some(ObjectType::ClassObject(object)) =
                            document.objects.get(&link_handle)
                        {
                            if let Some(section) = class_object_section(&object.data) {
                                sections.push(section);
                            }
                        }
                    }
                }
            }
        }
    }
    sections.extend(associative_sections(document, cache, handle));

    // A directly attached typed object can still be useful when its semantic
    // graph contains no backlink (for example a sparse extension dictionary).
    if sections.is_empty() {
        if let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&handle) {
            sections.push(PropSection {
                title: "Dynamic Object".to_string(),
                props: vec![ro("Decoded data", dynamic_data_text(&object.data))],
            });
        }
    }
    sections
}

fn preview_text(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut preview: String = value.chars().take(limit).collect();
    preview.push_str(&format!("… ({} chars)", value.chars().count()));
    preview
}

fn bounded_join<I>(values: I, limit: usize) -> String
where
    I: IntoIterator<Item = String>,
{
    let mut values = values.into_iter();
    let mut visible = Vec::new();
    for _ in 0..limit {
        let Some(value) = values.next() else {
            break;
        };
        visible.push(value);
    }
    let hidden = values.count();
    let mut result = visible.join("; ");
    if hidden != 0 {
        result.push_str(&format!("; … +{hidden}"));
    }
    result
}

fn class_object_section(
    data: &acadrust::objects::ClassObjectData,
) -> Option<PropSection> {
    use acadrust::objects::ClassObjectData;

    let section = match data {
        ClassObjectData::Empty => return None,
        ClassObjectData::SpatialIndex(value) => PropSection {
            title: "Spatial Index".to_string(),
            props: vec![
                ro(
                    "Updated",
                    format!(
                        "Julian {}; {} ms",
                        value.last_updated_julian_day,
                        value.last_updated_milliseconds
                    ),
                ),
                ro(
                    "Extents",
                    format!(
                        "{:.6},{:.6},{:.6} → {:.6},{:.6},{:.6}",
                        value.min_corner.x,
                        value.min_corner.y,
                        value.min_corner.z,
                        value.max_corner.x,
                        value.max_corner.y,
                        value.max_corner.z
                    ),
                ),
                ro("Indexed Objects", value.indexed_objects.len().to_string()),
            ],
        },
        ClassObjectData::LayerFilter(value) => PropSection {
            title: "Layer Filter".to_string(),
            props: vec![ro("Names", bounded_join(value.names.iter().cloned(), 64))],
        },
        ClassObjectData::PartialViewingIndex(value) => PropSection {
            title: "Partial Viewing Index".to_string(),
            props: vec![
                ro("Has Entries", value.has_entries.to_string()),
                ro(
                    "Entries",
                    bounded_join(
                        value.entries.iter().map(|entry| {
                            format!(
                                "{}: {:.4},{:.4},{:.4} → {:.4},{:.4},{:.4}",
                                handle_text(entry.object),
                                entry.extents_min.x,
                                entry.extents_min.y,
                                entry.extents_min.z,
                                entry.extents_max.x,
                                entry.extents_max.y,
                                entry.extents_max.z
                            )
                        }),
                        32,
                    ),
                ),
            ],
        },
        ClassObjectData::VbaProject(value) => PropSection {
            title: "VBA Project".to_string(),
            props: vec![
                ro(
                    "Detached Records",
                    format!(
                        "{} leading / {} bytes; {} trailing / {} bytes",
                        value.storage.leading_records.len(),
                        value
                            .storage
                            .leading_records
                            .iter()
                            .map(|record| record.data.len())
                            .sum::<usize>(),
                        value.storage.trailing_records.len(),
                        value
                            .storage
                            .trailing_records
                            .iter()
                            .map(|record| record.data.len())
                            .sum::<usize>()
                    ),
                ),
                ro(
                    "Streams",
                    value
                        .storage
                        .compound_file
                        .as_ref()
                        .map(|storage| storage.root.entries.len())
                        .unwrap_or_default()
                        .to_string(),
                ),
            ],
        },
        ClassObjectData::SectionManager(value) => PropSection {
            title: "Section Manager".to_string(),
            props: vec![
                ro("Live", value.is_live.to_string()),
                ro(
                    "Sections",
                    bounded_join(
                        value.sections.iter().map(|handle| handle_text(*handle)),
                        64,
                    ),
                ),
            ],
        },
        ClassObjectData::SectionSettings(value) => PropSection {
            title: "Section Settings".to_string(),
            props: vec![
                ro("Current Type", value.current_type.to_string()),
                ro(
                    "Types",
                    bounded_join(
                        value.types.iter().map(|setting| {
                            let geometry = bounded_join(
                                setting.geometry.iter().map(|geometry| {
                                    format!(
                                        "#{} flags {} layer '{}' linetype '{}' scale {:.4} weight {} transparency {}/{} hatch {} '{}' angle {:.4} spacing {:.4} scale {:.4} color {:?}",
                                        geometry.index,
                                        geometry.flags,
                                        geometry.layer,
                                        geometry.linetype,
                                        geometry.linetype_scale,
                                        geometry.lineweight,
                                        geometry.face_transparency,
                                        geometry.edge_transparency,
                                        geometry.hatch_type,
                                        geometry.hatch_pattern,
                                        geometry.hatch_angle,
                                        geometry.hatch_spacing,
                                        geometry.hatch_scale,
                                        geometry.color
                                    )
                                }),
                                16,
                            );
                            format!(
                                "type {} generation {} sources [{}] destination {} file '{}' geometry {} [{}]",
                                setting.section_type,
                                setting.generation,
                                bounded_join(
                                    setting.sources.iter().map(|handle| handle_text(*handle)),
                                    32
                                ),
                                handle_text(setting.destination_block),
                                setting.destination_file,
                                setting.geometry.len(),
                                geometry
                            )
                        }),
                        16,
                    ),
                ),
            ],
        },
        ClassObjectData::GradientBackground(value) => PropSection {
            title: "Gradient Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Colors",
                    format!(
                        "#{:06X} / #{:06X} / #{:06X}",
                        value.color_top & 0x00FF_FFFF,
                        value.color_middle & 0x00FF_FFFF,
                        value.color_bottom & 0x00FF_FFFF
                    ),
                ),
                ro(
                    "Geometry",
                    format!(
                        "horizon {:.6}; height {:.6}; rotation {:.6}",
                        value.horizon, value.height, value.rotation
                    ),
                ),
            ],
        },
        ClassObjectData::GroundPlaneBackground(value) => PropSection {
            title: "Ground Plane Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Colors",
                    format!(
                        "sky #{:06X}/#{:06X}; underground #{:06X}/#{:06X}; near/far #{:06X}/#{:06X}",
                        value.color_sky_zenith & 0x00FF_FFFF,
                        value.color_sky_horizon & 0x00FF_FFFF,
                        value.color_underground_horizon & 0x00FF_FFFF,
                        value.color_underground_azimuth & 0x00FF_FFFF,
                        value.color_near & 0x00FF_FFFF,
                        value.color_far & 0x00FF_FFFF
                    ),
                ),
            ],
        },
        ClassObjectData::IblBackground(value) => PropSection {
            title: "Image Based Lighting Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("Enabled", value.enabled.to_string()),
                ro("Name", value.name.clone()),
                ro("Rotation", format!("{:.6}", value.rotation)),
                ro("Display Image", value.display_image.to_string()),
                ro(
                    "Secondary Background",
                    handle_text(value.secondary_background),
                ),
            ],
        },
        ClassObjectData::ImageBackground(value) => PropSection {
            title: "Image Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("File", value.filename.clone()),
                ro(
                    "Display",
                    format!(
                        "fit {}; aspect {}; tile {}",
                        value.fit_to_screen,
                        value.maintain_aspect_ratio,
                        value.use_tiling
                    ),
                ),
                ro(
                    "Offset / Scale",
                    format!(
                        "{:.6},{:.6} / {:.6},{:.6}",
                        value.offset.x, value.offset.y, value.scale.x, value.scale.y
                    ),
                ),
            ],
        },
        ClassObjectData::SkyLightBackground(value) => PropSection {
            title: "Sky Light Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("Sun", handle_text(value.sun)),
            ],
        },
        ClassObjectData::SolidBackground(value) => PropSection {
            title: "Solid Background".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("Color", format!("#{:06X}", value.color & 0x00FF_FFFF)),
            ],
        },
        ClassObjectData::RenderEntry(value) => PropSection {
            title: "Render History Entry".to_string(),
            props: vec![
                ro("Image", value.image_filename.clone()),
                ro(
                    "Preset / View",
                    format!("{} / {}", value.preset_name, value.view_name),
                ),
                ro("Size", format!("{}×{}", value.width, value.height)),
                ro(
                    "Started",
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                        value.start_year,
                        value.start_month,
                        value.start_day,
                        value.start_hour,
                        value.start_minute,
                        value.start_second,
                        value.start_millisecond
                    ),
                ),
                ro(
                    "Finished",
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                        value.end_year,
                        value.end_month,
                        value.end_day,
                        value.end_hour,
                        value.end_minute,
                        value.end_second,
                        value.end_millisecond
                    ),
                ),
                ro(
                    "Statistics",
                    format!(
                        "{:.4}s; {} memory; {} materials; {} lights; {} triangles; index {}",
                        value.render_time,
                        value.memory_amount,
                        value.material_count,
                        value.light_count,
                        value.triangle_count,
                        value.display_index
                    ),
                ),
            ],
        },
        ClassObjectData::MotionPath(value) => PropSection {
            title: "Motion Path".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Camera / Target / View",
                    format!(
                        "{} / {} / {}",
                        handle_text(value.camera_path),
                        handle_text(value.target_path),
                        handle_text(value.view)
                    ),
                ),
                ro(
                    "Playback",
                    format!(
                        "{} frames at {} fps; corner deceleration {}",
                        value.frames, value.frame_rate, value.corner_deceleration
                    ),
                ),
            ],
        },
        ClassObjectData::CurvePath(value) => PropSection {
            title: "Curve Path".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("Entity", handle_text(value.entity)),
            ],
        },
        ClassObjectData::PointPath(value) => PropSection {
            title: "Point Path".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Point",
                    format!("{:.8}, {:.8}, {:.8}", value.point.x, value.point.y, value.point.z),
                ),
            ],
        },
        ClassObjectData::TvDeviceProperties(value) => PropSection {
            title: "Graphics Device Properties".to_string(),
            props: vec![
                ro("Flags", value.flags.to_string()),
                ro("Max Regen Threads", value.max_regen_threads.to_string()),
                ro(
                    "Palette / Highlight",
                    format!(
                        "{} / {} / {}",
                        value.use_lut_palette,
                        value.alternate_highlight,
                        value.alternate_highlight_color
                    ),
                ),
                ro(
                    "GPU",
                    format!(
                        "geometry shader {}; blending {}; AA {:.4}; reserved {:.4}",
                        value.geometry_shader_usage,
                        value.blending_mode,
                        value.antialiasing_level,
                        value.reserved_double
                    ),
                ),
            ],
        },
        ClassObjectData::PointCloudDefinition(value)
        | ClassObjectData::PointCloudDefinitionEx(value) => PropSection {
            title: "Point Cloud Definition".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro("File", value.source_filename.clone()),
                ro("Loaded", value.is_loaded.to_string()),
                ro("Points", value.point_count.to_string()),
                ro(
                    "Extents",
                    format!(
                        "{:.6},{:.6},{:.6} → {:.6},{:.6},{:.6}",
                        value.extents_min.x,
                        value.extents_min.y,
                        value.extents_min.z,
                        value.extents_max.x,
                        value.extents_max.y,
                        value.extents_max.z
                    ),
                ),
            ],
        },
        ClassObjectData::PointCloudDefinitionReactor(value)
        | ClassObjectData::PointCloudDefinitionReactorEx(value) => PropSection {
            title: "Point Cloud Reactor".to_string(),
            props: vec![ro("Version", value.class_version.to_string())],
        },
        ClassObjectData::PointCloudColorMap(value) => PropSection {
            title: "Point Cloud Color Map".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Defaults",
                    format!(
                        "intensity '{}'; elevation '{}'; classification '{}'",
                        value.default_intensity_scheme,
                        value.default_elevation_scheme,
                        value.default_classification_scheme
                    ),
                ),
                ro(
                    "Ramps",
                    bounded_join(
                        value.color_ramps.iter().map(|ramp| {
                            format!(
                                "v{} [{}]",
                                ramp.class_version,
                                ramp.color_schemes.join(", ")
                            )
                        }),
                        32,
                    ),
                ),
                ro(
                    "Classification Ramps",
                    bounded_join(
                        value.classification_color_ramps.iter().map(|ramp| {
                            format!(
                                "v{} [{}]",
                                ramp.class_version,
                                ramp.color_schemes.join(", ")
                            )
                        }),
                        32,
                    ),
                ),
            ],
        },
        ClassObjectData::NavisworksModelDefinition(value) => PropSection {
            title: "Coordination Model Definition".to_string(),
            props: vec![
                ro("Path", value.path.clone()),
                ro(
                    "State",
                    format!(
                        "flags {}; status {}; host visible {}",
                        value.flags, value.status, value.host_drawing_visibility
                    ),
                ),
                ro(
                    "Extents",
                    format!(
                        "{:.6},{:.6},{:.6} → {:.6},{:.6},{:.6}",
                        value.extents_min.x,
                        value.extents_min.y,
                        value.extents_min.z,
                        value.extents_max.x,
                        value.extents_max.y,
                        value.extents_max.z
                    ),
                ),
            ],
        },
        ClassObjectData::ContextDataManager(value) => PropSection {
            title: "Context Data Manager".to_string(),
            props: vec![
                ro("Object Context", handle_text(value.object_context)),
                ro(
                    "Sub-Managers",
                    bounded_join(
                        value.sub_managers.iter().map(|manager| {
                            format!(
                                "{} [{}]",
                                handle_text(manager.handle),
                                bounded_join(
                                    manager.entries.iter().map(|entry| format!(
                                        "{}={}",
                                        entry.name,
                                        handle_text(entry.item)
                                    )),
                                    32
                                )
                            )
                        }),
                        32,
                    ),
                ),
            ],
        },
        ClassObjectData::SunStudy(value) => PropSection {
            title: "Sun Study".to_string(),
            props: vec![
                ro("Name", value.setup_name.clone()),
                ro("Description", value.description.clone()),
                ro(
                    "Output",
                    format!(
                        "type {}; subset {} '{} / {}'; calendar {}; range {}",
                        value.output_type,
                        value.use_subset,
                        value.sheet_set_name,
                        value.sheet_subset_name,
                        value.select_dates_from_calendar,
                        value.select_range_of_dates
                    ),
                ),
                ro(
                    "Dates",
                    bounded_join(
                        value.dates.iter().map(|date| {
                            format!("Julian {} @ {} ms", date.julian_day, date.milliseconds)
                        }),
                        64,
                    ),
                ),
                ro(
                    "Time",
                    format!(
                        "{}..{} every {}; active hours {}",
                        value.start_time,
                        value.end_time,
                        value.interval,
                        value.hours.iter().filter(|enabled| **enabled).count()
                    ),
                ),
                ro(
                    "Layout",
                    format!(
                        "shade {}; viewports {}; {}×{} spacing {:.6}; lock {}; labels {}",
                        value.shade_plot_type,
                        value.viewport_count,
                        value.rows,
                        value.columns,
                        value.spacing,
                        value.lock_viewports,
                        value.label_viewports
                    ),
                ),
                ro(
                    "References",
                    format!(
                        "wizard {}; view {}; visual style {}; text style {}",
                        handle_text(value.page_setup_wizard),
                        handle_text(value.view),
                        handle_text(value.visual_style),
                        handle_text(value.text_style)
                    ),
                ),
            ],
        },
        ClassObjectData::DataTable(value) => PropSection {
            title: format!("Data Table · {}", value.name),
            props: vec![
                ro(
                    "Shape",
                    format!(
                        "{} rows; {} columns; flags {}",
                        value.row_count,
                        value.columns.len(),
                        value.flags
                    ),
                ),
                ro(
                    "Columns",
                    bounded_join(
                        value.columns.iter().map(|column| {
                            format!(
                                "{} type {} rows [{}]",
                                column.name,
                                column.value_type,
                                bounded_join(
                                    column.rows.iter().map(|cell| format!(
                                        "{} | {:.8} | {}",
                                        cell.integer,
                                        cell.real,
                                        preview_text(&cell.text, 128)
                                    )),
                                    32
                                )
                            )
                        }),
                        32,
                    ),
                ),
            ],
        },
        ClassObjectData::DataLink(value) => PropSection {
            title: "Data Link".to_string(),
            props: vec![
                ro("Adapter", value.data_adapter.clone()),
                ro("Description", value.description.clone()),
                ro("Tooltip", value.tooltip.clone()),
                ro(
                    "Connection",
                    preview_text(&value.connection_string, 1024),
                ),
                ro(
                    "Options",
                    format!(
                        "{} / update {}; flags {}; path {}; status {} '{}'",
                        value.option,
                        value.update_option,
                        value.flags,
                        value.path_option,
                        value.status_flags,
                        value.update_status
                    ),
                ),
                ro(
                    "Updated",
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                        value.year,
                        value.month,
                        value.day,
                        value.hour,
                        value.minute,
                        value.second,
                        value.millisecond
                    ),
                ),
                ro("Owner", handle_text(value.hard_owner)),
                ro(
                    "Custom Data",
                    bounded_join(
                        value.custom_data.iter().map(|entry| {
                            format!(
                                "{}='{}'",
                                handle_text(entry.target),
                                preview_text(&entry.value, 256)
                            )
                        }),
                        64,
                    ),
                ),
            ],
        },
        ClassObjectData::PersistentSubentityManager(value) => PropSection {
            title: "Persistent Subentity Manager".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Reserved",
                    format!("{}/{}", value.reserved_zero, value.reserved_two),
                ),
                ro(
                    "Steps",
                    format!(
                        "{} declared; [{}]",
                        value.associated_step_count,
                        bounded_join(value.steps.iter().map(ToString::to_string), 128)
                    ),
                ),
                ro(
                    "Subentities",
                    format!(
                        "{} declared; [{}]",
                        value.associated_subentity_count,
                        bounded_join(value.subentities.iter().map(ToString::to_string), 128)
                    ),
                ),
            ],
        },
        ClassObjectData::GeoMapImage(value) => PropSection {
            title: "Geographic Map Image".to_string(),
            props: vec![
                ro("Version", value.class_version.to_string()),
                ro(
                    "Origin / Size",
                    format!(
                        "{:.8},{:.8},{:.8} / {:.4}×{:.4}",
                        value.origin.x,
                        value.origin.y,
                        value.origin.z,
                        value.image_size.x,
                        value.image_size.y
                    ),
                ),
                ro(
                    "Display",
                    format!(
                        "properties {}; clip {}; brightness {}; contrast {}; fade {}",
                        value.display_properties,
                        value.clipping_enabled,
                        value.brightness,
                        value.contrast,
                        value.fade
                    ),
                ),
            ],
        },
        ClassObjectData::DetailViewStyle(value) => PropSection {
            title: format!("Detail View Style · {}", value.base.display_name),
            props: vec![
                ro("Description", value.base.description.clone()),
                ro(
                    "State",
                    format!(
                        "base v{} flags {}; modified {}; v{} flags {}",
                        value.base.class_version,
                        value.base.flags,
                        value.base.modified_for_recompute,
                        value.class_version,
                        value.flags
                    ),
                ),
                ro(
                    "Identifier",
                    format!(
                        "style {}; color {:?}; height {:.6}; excluded '{}'; offset {:.6}; placement {}",
                        handle_text(value.identifier_style),
                        value.identifier_color,
                        value.identifier_height,
                        value.identifier_excluded_characters,
                        value.identifier_offset,
                        value.identifier_placement
                    ),
                ),
                ro(
                    "Arrow / Boundary",
                    format!(
                        "symbol {} color {:?} size {:.6}; boundary {} weight {} color {:?}",
                        handle_text(value.arrow_symbol),
                        value.arrow_symbol_color,
                        value.arrow_symbol_size,
                        handle_text(value.boundary_linetype),
                        value.boundary_lineweight,
                        value.boundary_color
                    ),
                ),
                ro(
                    "View Label",
                    format!(
                        "style {} color {:?} height {:.6}; attachment {}; offset {:.6}; alignment {}; pattern '{}'",
                        handle_text(value.view_label_text_style),
                        value.view_label_text_color,
                        value.view_label_text_height,
                        value.view_label_attachment,
                        value.view_label_offset,
                        value.view_label_alignment,
                        value.view_label_pattern
                    ),
                ),
                ro(
                    "Connection / Border",
                    format!(
                        "{} weight {} color {:?}; {} weight {} color {:?}; model edge {}",
                        handle_text(value.connection_linetype),
                        value.connection_lineweight,
                        value.connection_color,
                        handle_text(value.border_linetype),
                        value.border_lineweight,
                        value.border_color,
                        value.model_edge
                    ),
                ),
            ],
        },
        ClassObjectData::SectionViewStyle(value) => PropSection {
            title: format!("Section View Style · {}", value.base.display_name),
            props: vec![
                ro("Description", value.base.description.clone()),
                ro(
                    "State",
                    format!(
                        "base v{} flags {}; modified {}; v{} flags {}; reserved {:?}",
                        value.base.class_version,
                        value.base.flags,
                        value.base.modified_for_recompute,
                        value.class_version,
                        value.flags,
                        value.reserved_flags
                    ),
                ),
                ro(
                    "Identifier / Arrows",
                    format!(
                        "style {} color {:?} height {:.6}; symbols {}/{} color {:?} size {:.6}; excluded '{}'; extension {:.6}",
                        handle_text(value.identifier_style),
                        value.identifier_color,
                        value.identifier_height,
                        handle_text(value.arrow_start_symbol),
                        handle_text(value.arrow_end_symbol),
                        value.arrow_symbol_color,
                        value.arrow_symbol_size,
                        value.identifier_excluded_characters,
                        value.arrow_symbol_extension_length
                    ),
                ),
                ro(
                    "Plane / Bend",
                    format!(
                        "plane {} weight {} color {:?}; bend {} weight {} color {:?}; lengths {:.6}/{:.6}",
                        handle_text(value.plane_linetype),
                        value.plane_lineweight,
                        value.plane_color,
                        handle_text(value.bend_linetype),
                        value.bend_lineweight,
                        value.bend_color,
                        value.bend_line_length,
                        value.end_line_length
                    ),
                ),
                ro(
                    "View Label",
                    format!(
                        "style {} color {:?} height {:.6}; attachment {}; offset {:.6}; alignment {}; pattern '{}'",
                        handle_text(value.view_label_text_style),
                        value.view_label_text_color,
                        value.view_label_text_height,
                        value.view_label_attachment,
                        value.view_label_offset,
                        value.view_label_alignment,
                        value.view_label_pattern
                    ),
                ),
                ro(
                    "Hatch",
                    format!(
                        "color {:?}; background {:?}; pattern '{}'; scale {:.6}; transparency {}; angles [{}]",
                        value.hatch_color,
                        value.hatch_background_color,
                        value.hatch_pattern,
                        value.hatch_scale,
                        value.hatch_transparency,
                        bounded_join(value.hatch_angles.iter().map(|angle| format!("{angle:.6}")), 64)
                    ),
                ),
                ro(
                    "Placement",
                    format!(
                        "identifier {} offset {:.6}; arrow {}; overshoot {:.6}",
                        value.identifier_position,
                        value.identifier_offset,
                        value.arrow_position,
                        value.end_line_overshoot
                    ),
                ),
            ],
        },
        ClassObjectData::AcMeCommandHistory(value) => PropSection {
            title: "Model Documentation Command History".to_string(),
            props: vec![ro("Version", value.class_version.to_string())],
        },
        ClassObjectData::AcMeScope(value) => PropSection {
            title: "Model Documentation Scope".to_string(),
            props: vec![ro("Version", value.class_version.to_string())],
        },
        ClassObjectData::AcMeStateManager(value) => PropSection {
            title: "Model Documentation State Manager".to_string(),
            props: vec![ro("Version", value.class_version.to_string())],
        },
        ClassObjectData::CsacDocumentOptions(value) => PropSection {
            title: "Model Documentation Options".to_string(),
            props: vec![ro("Version", value.class_version.to_string())],
        },
        ClassObjectData::ViewRepSourceManager(value) => PropSection {
            title: "View Representation Source Manager".to_string(),
            props: vec![
                ro("Has Source", value.has_source.to_string()),
                ro("Source", handle_text(value.source)),
                ro("Status", value.status.to_string()),
            ],
        },
        ClassObjectData::ViewRepStandard(value) => PropSection {
            title: "View Representation Standard".to_string(),
            props: vec![ro("Values", format!("{:?}", value.values))],
        },
        ClassObjectData::ViewRepOrientationDefinition => PropSection {
            title: "View Representation Orientation Definition".to_string(),
            props: Vec::new(),
        },
        ClassObjectData::ViewRepOrientation(value) => PropSection {
            title: "View Representation Orientation".to_string(),
            props: vec![
                ro(
                    "Camera",
                    format!("{:.8},{:.8},{:.8}", value.camera.x, value.camera.y, value.camera.z),
                ),
                ro(
                    "Target",
                    format!("{:.8},{:.8},{:.8}", value.target.x, value.target.y, value.target.z),
                ),
                ro(
                    "Normal",
                    format!("{:.8},{:.8},{:.8}", value.normal.x, value.normal.y, value.normal.z),
                ),
            ],
        },
        ClassObjectData::ViewRepSectionDefinition(value) => PropSection {
            title: "View Representation Section Definition".to_string(),
            props: vec![
                ro("Version", value.version.to_string()),
                ro("Depth", format!("{:.8}", value.section_depth)),
                ro("Flags", format!("{:?}", value.flags)),
            ],
        },
        ClassObjectData::ViewRepModelSpaceViewSelectionSet(value) => PropSection {
            title: "View Representation Selection Set".to_string(),
            props: vec![
                ro("Version", value.version.to_string()),
                ro(
                    "Entities",
                    bounded_join(
                        value.entities.iter().map(|handle| handle_text(*handle)),
                        128,
                    ),
                ),
            ],
        },
        ClassObjectData::ViewRep(value) => PropSection {
            title: format!("View Representation · {}", value.name),
            props: vec![
                ro(
                    "Header",
                    format!(
                        "{:?}; scale {}; status {}; description '{}'",
                        value.header_values, value.scale, value.header_status, value.description
                    ),
                ),
                ro(
                    "Source",
                    format!(
                        "id {}; enabled {}; version {}; model {}; guid {:08X}-{:04X}-{:04X}-{:?}",
                        value.source_id,
                        value.source_enabled,
                        value.source_version,
                        value.model_id,
                        value.guid.data1,
                        value.guid.data2,
                        value.guid.data3,
                        value.guid.data4
                    ),
                ),
                ro(
                    "Geometry",
                    format!(
                        "marker {}; transform v{} {:?}; database {}; geometry v{} marker {}; sketches {}",
                        value.marker,
                        value.transform_version,
                        value.transform,
                        value.database_id,
                        value.geometry_version,
                        value.geometry_marker,
                        value.sketches.len()
                    ),
                ),
                ro(
                    "References",
                    format!(
                        "related {:?}; source manager {}; owned {:?}; optional {:?}; orientation {}; linked {:?}; style {}",
                        value.related_objects.map(handle_text),
                        handle_text(value.source_manager),
                        value.owned_objects.map(handle_text),
                        value.optional_objects.map(handle_text),
                        handle_text(value.orientation),
                        value.linked_views.map(handle_text),
                        handle_text(value.style)
                    ),
                ),
                ro(
                    "Placement",
                    format!(
                        "{:.8},{:.8}; rotation {:.8}; active {}; projection {}",
                        value.position.x,
                        value.position.y,
                        value.rotation,
                        value.is_active,
                        value.projection
                    ),
                ),
                ro(
                    "Actions / Parent",
                    format!(
                        "mode {}; action {}; has parent {}; parent {}; section paths {}",
                        value.action_mode,
                        value.action.map(handle_text).unwrap_or_else(|| "None".to_string()),
                        value.has_parent,
                        handle_text(value.parent),
                        value.section_sketches.len()
                    ),
                ),
                ro(
                    "Tail / Block Path",
                    format!(
                        "version {}; state {}; id {}; path count {}; version {}; id {}; enabled {}; entries {}",
                        value.tail_version,
                        value.tail_state,
                        value.tail_id,
                        value.path_count,
                        value.path_version,
                        value.path_id,
                        value.has_block_path,
                        value.block_path.as_ref().map(|path| path.entries.len()).unwrap_or_default()
                    ),
                ),
            ],
        },
        ClassObjectData::ViewRepModelSpaceSource(value) => PropSection {
            title: "View Representation Model Source".to_string(),
            props: vec![
                ro(
                    "Header",
                    format!(
                        "enabled {}; values {:?}; source v{} status {}; tail {:?}",
                        value.enabled,
                        value.header_values,
                        value.source_version,
                        value.source_status,
                        value.tail_values
                    ),
                ),
                ro("Transform", format!("{:?}", value.transform)),
                ro(
                    "References",
                    format!(
                        "model {}; references {:?}; orientation {}",
                        handle_text(value.model),
                        value.references.map(handle_text),
                        handle_text(value.orientation)
                    ),
                ),
                ro(
                    "GUID",
                    format!(
                        "{:08X}-{:04X}-{:04X}-{:?}",
                        value.guid.data1,
                        value.guid.data2,
                        value.guid.data3,
                        value.guid.data4
                    ),
                ),
            ],
        },
        ClassObjectData::LightList(_)
        | ClassObjectData::Sun(_)
        | ClassObjectData::RenderSettings(_)
        | ClassObjectData::MentalRayRenderSettings(_)
        | ClassObjectData::RapidRtRenderSettings(_)
        | ClassObjectData::RenderEnvironment(_)
        | ClassObjectData::RenderGlobal(_) => return None,
    };
    Some(section)
}

fn semantic_property_text(
    value: &acadrust::objects::SemanticPropertyValue,
) -> String {
    use acadrust::objects::SemanticPropertyValue;
    match value {
        SemanticPropertyValue::Text(value) => preview_text(value, 512),
        SemanticPropertyValue::Bool(value) => value.to_string(),
        SemanticPropertyValue::Byte(value) => value.to_string(),
        SemanticPropertyValue::Int16(value) => value.to_string(),
        SemanticPropertyValue::Int32(value) => value.to_string(),
        SemanticPropertyValue::Int64(value) => value.to_string(),
        SemanticPropertyValue::Double(value) => format!("{value:.12}"),
        SemanticPropertyValue::Handle(value) => handle_text(*value),
        SemanticPropertyValue::Binary(value) => format!("{} bytes", value.len()),
    }
}

fn auxiliary_object_property(
    object: &ObjectType,
) -> Option<Property> {
    use acadrust::objects::DataObjectData;

    let (label, value) = match object {
        ObjectType::TableContent(table) => (
            format!("Table Content {}", handle_text(table.common.handle)),
            format!(
                "{} rows × {} columns; {} merged ranges; {} fields",
                table.rows.len(),
                table.columns.len(),
                table.merged_ranges.len(),
                table.field_handles.len()
            ),
        ),
        ObjectType::ImageDefinitionReactor(value) => (
            format!("Image Reactor {}", handle_text(value.handle)),
            format!(
                "owner {}; image {}",
                handle_text(value.owner),
                handle_text(value.image_handle)
            ),
        ),
        ObjectType::BookColor(value) => (
            format!("Book Color {}", handle_text(value.handle)),
            format!("{} / {} = {:?}", value.book_name, value.color_name, value.color),
        ),
        ObjectType::PlaceHolder(value) => (
            format!("Placeholder {}", handle_text(value.handle)),
            format!("owner {}", handle_text(value.owner)),
        ),
        ObjectType::FieldList(value) => (
            format!("Field List {}", handle_text(value.handle)),
            format!(
                "owner {}; unknown {}; fields [{}]",
                handle_text(value.owner),
                value.unknown,
                bounded_join(value.fields.iter().map(|handle| handle_text(*handle)), 128)
            ),
        ),
        ObjectType::DataObject(value) => {
            let text = match &value.data {
                DataObjectData::BreakPointRef => "break point reference".to_string(),
                DataObjectData::BreakData(data) => format!(
                    "v{} dimension {}; reserved {}; point references {}",
                    data.version,
                    handle_text(data.dimension_reference),
                    handle_text(data.reserved_reference),
                    data.point_references.len()
                ),
                DataObjectData::CellStyleMap(data) => {
                    format!("{} named cell styles", data.cells.len())
                }
                DataObjectData::AcDsRecord => "AcDs record".to_string(),
                DataObjectData::AcDsSchema => "AcDs schema".to_string(),
                DataObjectData::Dummy => "dummy helper".to_string(),
                DataObjectData::IdBuffer(data) => format!(
                    "flags {}; objects [{}]",
                    data.flags,
                    bounded_join(
                        data.object_ids.iter().map(|handle| handle_text(*handle)),
                        128
                    )
                ),
                DataObjectData::Index(data) => format!(
                    "updated Julian {} @ {} ms",
                    data.last_updated_julian_day, data.last_updated_milliseconds
                ),
                DataObjectData::LayerIndex(data) => format!(
                    "updated Julian {} @ {} ms; entries [{}]",
                    data.last_updated_julian_day,
                    data.last_updated_milliseconds,
                    bounded_join(
                        data.entries.iter().map(|entry| format!(
                            "{}: {} layers, buffer {}",
                            entry.name,
                            entry.layer_count,
                            handle_text(entry.id_buffer)
                        )),
                        64
                    )
                ),
                DataObjectData::LongTransaction => "long transaction".to_string(),
                DataObjectData::ObjectPointer => "object pointer".to_string(),
                DataObjectData::PartialViewingFilter(_) => {
                    "partial viewing filter".to_string()
                }
                DataObjectData::TableGeometry(data) => format!(
                    "{} rows × {} columns; {} cell geometry records; {} primitives",
                    data.rows,
                    data.columns,
                    data.cells.len(),
                    data.cells.iter().map(|cell| cell.geometry.len()).sum::<usize>()
                ),
            };
            (
                format!("{} {}", value.dxf_name(), handle_text(value.handle)),
                format!("owner {}; {text}", handle_text(value.owner)),
            )
        }
        ObjectType::RegisteredClass(value) => (
            format!(
                "{} {}",
                if value.dxf_name.is_empty() {
                    value.cpp_class_name.as_str()
                } else {
                    value.dxf_name.as_str()
                },
                handle_text(value.handle)
            ),
            format!(
                "owner {}; properties [{}]; payload {} bits / {} records; references {}",
                handle_text(value.owner),
                bounded_join(
                    value.properties.iter().map(|property| format!(
                        "{}:{}={}",
                        property.subclass,
                        property.code,
                        semantic_property_text(&property.value)
                    )),
                    64
                ),
                value.payload.bit_count,
                value.payload.records.len(),
                value.object_ids.len()
            ),
        ),
        ObjectType::ProxyObject(value) => (
            format!(
                "Proxy Object {}",
                handle_text(value.handle)
            ),
            format!(
                "class {} / id {}; subclass '{}'; version {}/{} maintenance {}; DXF {}; payload {} + {} bits; references {}",
                value.class_id,
                value.proxy_id,
                value.dxf_subclass,
                value.version,
                value.dwg_version,
                value.maintenance_version,
                value.from_dxf,
                value.payload.bit_count,
                value.text_payload.bit_count,
                value.object_ids.len()
            ),
        ),
        ObjectType::Unknown {
            type_name,
            handle,
            owner,
            raw_dxf_codes,
            raw_dwg_data,
            raw_dwg_handle_bits,
            ..
        } => (
            format!("Unknown Object {}", handle_text(*handle)),
            format!(
                "{}; owner {}; DXF codes {}; DWG bytes {}; handle bits {}",
                type_name,
                handle_text(*owner),
                raw_dxf_codes.as_ref().map(Vec::len).unwrap_or_default(),
                raw_dwg_data.as_ref().map(Vec::len).unwrap_or_default(),
                raw_dwg_handle_bits
            ),
        ),
        _ => return None,
    };
    Some(ro(label, value))
}

/// Drawing-level standard object data prepared on the file-open worker.
fn build_document_sections(document: &CadDocument) -> Vec<PropSection> {
    use acadrust::objects::ClassObjectData;

    let mut sections = vec![PropSection {
        title: "Drawing".to_string(),
        props: vec![
            ro("Version", format!("{:?}", document.version)),
            ro("Entities", document.entity_count().to_string()),
            ro("Objects", document.objects.len().to_string()),
            ro("Layers", document.layers.len().to_string()),
            ro("Blocks", document.block_records.len().to_string()),
        ],
    }];

    let mut objects = document.objects.iter().collect::<Vec<_>>();
    objects.sort_by_key(|(handle, _)| handle.value());
    for (_, object) in objects {
        match object {
            ObjectType::GeoData(value) => {
                sections.push(PropSection {
                    title: "Geographic Location".to_string(),
                    props: vec![
                        ro("Version", value.version.to_string()),
                        ro("Coordinate Type", value.coordinate_type.to_string()),
                        ro(
                            "Design Point",
                            format!(
                                "{:.8}, {:.8}, {:.8}",
                                value.design_point.x,
                                value.design_point.y,
                                value.design_point.z
                            ),
                        ),
                        ro(
                            "Reference Point",
                            format!(
                                "{:.8}, {:.8}, {:.8}",
                                value.reference_point.x,
                                value.reference_point.y,
                                value.reference_point.z
                            ),
                        ),
                        ro(
                            "North",
                            format!(
                                "{:.8}, {:.8}",
                                value.north_direction.x, value.north_direction.y
                            ),
                        ),
                        ro(
                            "Up",
                            format!(
                                "{:.8}, {:.8}, {:.8}",
                                value.up_direction.x,
                                value.up_direction.y,
                                value.up_direction.z
                            ),
                        ),
                        ro(
                            "Units / Scale",
                            format!(
                                "horizontal {} × {:.12}; vertical {} × {:.12}; user {:.12}",
                                value.horizontal_units,
                                value.horizontal_unit_scale,
                                value.vertical_units,
                                value.vertical_unit_scale,
                                value.user_scale_factor
                            ),
                        ),
                        ro(
                            "Sea Level",
                            format!(
                                "enabled {}; elevation {:.8}; radius {:.8}",
                                value.sea_level_correction,
                                value.sea_level_elevation,
                                value.coordinate_projection_radius
                            ),
                        ),
                        ro(
                            "Coordinate System",
                            preview_text(&value.coordinate_system_definition, 1024),
                        ),
                        ro("Datum", preview_text(&value.coordinate_system_datum, 512)),
                        ro("WKT", preview_text(&value.coordinate_system_wkt, 1024)),
                        ro(
                            "Transformation Mesh",
                            format!(
                                "{} points; {} faces",
                                value.mesh_points.len(),
                                value.mesh_faces.len()
                            ),
                        ),
                        ro(
                            "Observation",
                            format!(
                                "{} → {}; coverage {}; RSS {}",
                                value.observation_from_tag,
                                value.observation_to_tag,
                                value.observation_coverage_tag,
                                value.geo_rss_tag
                            ),
                        ),
                    ],
                });
            }
            ObjectType::RasterVariables(value) => {
                sections.push(PropSection {
                    title: "Raster Settings".to_string(),
                    props: vec![
                        ro("Version", value.class_version.to_string()),
                        ro("Display Frame", value.display_image_frame.to_string()),
                        ro("Quality", value.image_quality.to_string()),
                        ro("Units", value.units.to_string()),
                    ],
                });
            }
            ObjectType::WipeoutVariables(value) => {
                sections.push(PropSection {
                    title: "Wipeout Settings".to_string(),
                    props: vec![ro("Display Frame", value.display_frame.to_string())],
                });
            }
            ObjectType::ClassObject(value) => match &value.data {
                ClassObjectData::LightList(list) => {
                    sections.push(PropSection {
                        title: "Light List".to_string(),
                        props: vec![
                            ro("Version", list.class_version.to_string()),
                            ro("Lights", list.lights.len().to_string()),
                            ro(
                                "Entries",
                                bounded_join(
                                    list.lights.iter().map(|light| {
                                        format!(
                                            "{}={}",
                                            light.name,
                                            handle_text(light.handle)
                                        )
                                    }),
                                    64,
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::Sun(sun) => {
                    sections.push(PropSection {
                        title: "Sun".to_string(),
                        props: vec![
                            ro("Version", sun.class_version.to_string()),
                            ro("On", sun.is_on.to_string()),
                            ro("Color", format!("{:?}", sun.color)),
                            ro("Intensity", format!("{:.6}", sun.intensity)),
                            ro(
                                "Date / Time",
                                format!(
                                    "Julian {}; {} ms; DST {}",
                                    sun.julian_day,
                                    sun.milliseconds,
                                    sun.is_daylight_savings_on
                                ),
                            ),
                            ro(
                                "Shadows",
                                format!(
                                    "enabled {}; type {}; map {}; softness {}",
                                    sun.has_shadow,
                                    sun.shadow_type,
                                    sun.shadow_map_size,
                                    sun.shadow_softness
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::RenderSettings(settings) => {
                    sections.push(PropSection {
                        title: "Render Settings".to_string(),
                        props: vec![
                            ro("Version", settings.class_version.to_string()),
                            ro("Name", settings.name.clone()),
                            ro("Description", settings.description.clone()),
                            ro(
                                "Flags",
                                format!(
                                    "fog {}; fog background {}; backfaces {}; environment {}",
                                    settings.fog_enabled,
                                    settings.fog_background_enabled,
                                    settings.backfaces_enabled,
                                    settings.environment_image_enabled
                                ),
                            ),
                            ro(
                                "Environment Image",
                                settings.environment_image_filename.clone(),
                            ),
                            ro(
                                "Display",
                                format!(
                                    "index {}; predefined {}",
                                    settings.display_index, settings.has_predefined
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::MentalRayRenderSettings(settings) => {
                    sections.push(PropSection {
                        title: "Mental Ray".to_string(),
                        props: vec![
                            ro("Preset", settings.base.name.clone()),
                            ro(
                                "Version",
                                format!(
                                    "base {}; mental ray {}",
                                    settings.base.class_version, settings.version
                                ),
                            ),
                            ro(
                                "Preset State",
                                format!(
                                    "description '{}'; display {}; predefined {}; fog {}/{}; backfaces {}; environment {} '{}'",
                                    settings.base.description,
                                    settings.base.display_index,
                                    settings.base.has_predefined,
                                    settings.base.fog_enabled,
                                    settings.base.fog_background_enabled,
                                    settings.base.backfaces_enabled,
                                    settings.base.environment_image_enabled,
                                    settings.base.environment_image_filename
                                ),
                            ),
                            ro(
                                "Sampling",
                                format!(
                                    "{}..{}; filter {} {:.4}×{:.4}; contrast {:?}",
                                    settings.sampling_min,
                                    settings.sampling_max,
                                    settings.sampling_filter,
                                    settings.sampling_filter_width,
                                    settings.sampling_filter_height,
                                    settings.sampling_contrast
                                ),
                            ),
                            ro(
                                "Ray Tracing",
                                format!(
                                    "{}; depth {:?}; shadows {} / maps {}",
                                    settings.ray_tracing_enabled,
                                    settings.ray_trace_depth,
                                    settings.shadow_mode,
                                    settings.shadow_maps_enabled
                                ),
                            ),
                            ro(
                                "Global Illumination",
                                format!(
                                    "{}; samples {}; radius {} {:.6}; photons {}; trace {:?}",
                                    settings.global_illumination_enabled,
                                    settings.global_illumination_sample_count,
                                    settings.global_illumination_sample_radius_enabled,
                                    settings.global_illumination_sample_radius,
                                    settings.photons_per_light,
                                    settings.photon_trace_depth
                                ),
                            ),
                            ro(
                                "Final Gather",
                                format!(
                                    "{}; rays {}; state {:?}; radius {:?}",
                                    settings.final_gathering_enabled,
                                    settings.final_gathering_ray_count,
                                    settings.final_gathering_sample_radius_state,
                                    settings.final_gathering_sample_radius
                                ),
                            ),
                            ro(
                                "Diagnostics",
                                format!(
                                    "mode {}; grid {} size {:.6}; photon {}; BSP {}; samples {}; luminance {:.6}",
                                    settings.diagnostics_mode,
                                    settings.diagnostics_grid_mode,
                                    settings.diagnostics_grid_size,
                                    settings.diagnostics_photon_mode,
                                    settings.diagnostics_bsp_mode,
                                    settings.diagnostics_samples_mode,
                                    settings.light_luminance_scale
                                ),
                            ),
                            ro(
                                "Execution",
                                format!(
                                    "export {}; description '{}'; tile {} order {}; memory {}; energy {:.6}",
                                    settings.export_mi_enabled,
                                    settings.description,
                                    settings.tile_size,
                                    settings.tile_order,
                                    settings.memory_limit,
                                    settings.energy_multiplier
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::RapidRtRenderSettings(settings) => {
                    sections.push(PropSection {
                        title: "Rapid RT".to_string(),
                        props: vec![
                            ro("Preset", settings.base.name.clone()),
                            ro(
                                "Version",
                                format!(
                                    "base {}; Rapid RT {}",
                                    settings.base.class_version, settings.version
                                ),
                            ),
                            ro(
                                "Preset State",
                                format!(
                                    "description '{}'; display {}; predefined {}; fog {}/{}; backfaces {}; environment {} '{}'",
                                    settings.base.description,
                                    settings.base.display_index,
                                    settings.base.has_predefined,
                                    settings.base.fog_enabled,
                                    settings.base.fog_background_enabled,
                                    settings.base.backfaces_enabled,
                                    settings.base.environment_image_enabled,
                                    settings.base.environment_image_filename
                                ),
                            ),
                            ro(
                                "Target / Level / Time",
                                format!(
                                    "{} / {} / {}",
                                    settings.render_target,
                                    settings.render_level,
                                    settings.render_time
                                ),
                            ),
                            ro(
                                "Lighting / Filter",
                                format!(
                                    "{} / {} {:.4}×{:.4}",
                                    settings.lighting_model,
                                    settings.filter_type,
                                    settings.filter_width,
                                    settings.filter_height
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::RenderEnvironment(environment) => {
                    sections.push(PropSection {
                        title: "Render Environment".to_string(),
                        props: vec![
                            ro("Version", environment.class_version.to_string()),
                            ro(
                                "Fog",
                                format!(
                                    "{}; background {}; color {:?}; density {:.4}..{:.4}; distance {:.4}..{:.4}",
                                    environment.fog_enabled,
                                    environment.fog_background_enabled,
                                    environment.fog_color,
                                    environment.fog_density_near,
                                    environment.fog_density_far,
                                    environment.fog_distance_near,
                                    environment.fog_distance_far
                                ),
                            ),
                            ro(
                                "Environment Image",
                                format!(
                                    "{} · {}",
                                    environment.environment_image_enabled,
                                    environment.environment_image_filename
                                ),
                            ),
                        ],
                    });
                }
                ClassObjectData::RenderGlobal(global) => {
                    sections.push(PropSection {
                        title: "Render Output".to_string(),
                        props: vec![
                            ro("Version", global.class_version.to_string()),
                            ro(
                                "Procedure / Destination",
                                format!("{} / {}", global.procedure, global.destination),
                            ),
                            ro(
                                "Image",
                                format!(
                                    "{}×{}; save {} · {}",
                                    global.image_width,
                                    global.image_height,
                                    global.save_enabled,
                                    global.save_filename
                                ),
                            ),
                            ro(
                                "Display",
                                format!(
                                    "predefined first {}; high-level info {}",
                                    global.predefined_presets_first,
                                    global.high_level_info
                                ),
                            ),
                        ],
                    });
                }
                other => {
                    if let Some(section) = class_object_section(other) {
                        sections.push(section);
                    }
                }
            },
            _ => {}
        }
    }

    if !document.dgn_ls_definitions.is_empty() {
        let mut definitions = document
            .dgn_ls_definitions
            .values()
            .collect::<Vec<_>>();
        definitions.sort_by_key(|definition| definition.handle.value());
        let omitted = definitions.len().saturating_sub(128);
        let mut props = definitions
            .into_iter()
            .take(128)
            .map(|definition| {
                ro(
                    definition.name.clone(),
                    format!(
                        "handle {}; root {}; components {}",
                        handle_text(definition.handle),
                        handle_text(definition.root_component),
                        document.dgn_ls_components.len()
                    ),
                )
            })
            .collect::<Vec<_>>();
        if omitted != 0 {
            props.push(ro("More Definitions", format!("{omitted} omitted")));
        }
        sections.push(PropSection {
            title: "DGN Line Styles".to_string(),
            props,
        });
    }
    let mut auxiliary = document
        .objects
        .values()
        .filter_map(auxiliary_object_property)
        .collect::<Vec<_>>();
    auxiliary.sort_by(|left, right| left.label.cmp(&right.label));
    let auxiliary_omitted = auxiliary.len().saturating_sub(256);
    auxiliary.truncate(256);
    if auxiliary_omitted != 0 {
        auxiliary.push(ro(
            "More Standard Objects",
            format!("{auxiliary_omitted} omitted"),
        ));
    }
    if !auxiliary.is_empty() {
        sections.push(PropSection {
            title: "Standard Object Data".to_string(),
            props: auxiliary,
        });
    }
    let mut per_title = rustc_hash::FxHashMap::default();
    let mut omitted = rustc_hash::FxHashMap::default();
    sections.retain(|section| {
        if section.title == "Drawing" {
            return true;
        }
        let shown = per_title.entry(section.title.clone()).or_insert(0usize);
        if *shown < 4 {
            *shown += 1;
            true
        } else {
            *omitted.entry(section.title.clone()).or_insert(0usize) += 1;
            false
        }
    });
    if !omitted.is_empty() {
        let mut entries = omitted.into_iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        sections.push(PropSection {
            title: "Additional Standard Objects".to_string(),
            props: entries
                .into_iter()
                .map(|(name, count)| ro(name, format!("{count} more records")))
                .collect(),
        });
    }
    sections
}
