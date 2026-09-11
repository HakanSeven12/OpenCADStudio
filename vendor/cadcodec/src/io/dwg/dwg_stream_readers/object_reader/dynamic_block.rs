use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::objects::*;
use crate::types::DxfVersion;
use crate::types::Handle;

use super::safe_count;

fn read_eval_value(reader: &mut DwgMergedReader, code: i16) -> BlockEvalValue {
    match code {
        40 => BlockEvalValue::Real(reader.read_bit_double()),
        10 | 11 => {
            let point = reader.read_2raw_double();
            BlockEvalValue::Point([point.x, point.y])
        }
        1 => BlockEvalValue::Text(reader.read_variable_text()),
        90 => BlockEvalValue::Long(reader.read_bit_long()),
        91 => BlockEvalValue::Handle(Handle::from(reader.read_handle())),
        70 => BlockEvalValue::Short(reader.read_bit_short()),
        _ => BlockEvalValue::None,
    }
}

fn read_eval(reader: &mut DwgMergedReader) -> BlockEvalExpression {
    let parent_id = reader.read_bit_long();
    let major = reader.read_bit_long();
    let minor = reader.read_bit_long();
    let value_code = reader.read_bit_short();
    let value = read_eval_value(reader, value_code);
    let node_id = reader.read_bit_long();
    BlockEvalExpression {
        parent_id,
        major,
        minor,
        value_code,
        value,
        node_id,
    }
}

fn read_element(reader: &mut DwgMergedReader) -> BlockElement {
    BlockElement {
        eval: read_eval(reader),
        name: reader.read_variable_text(),
        major: reader.read_bit_long(),
        minor: reader.read_bit_long(),
        eed_1071: reader.read_bit_long(),
    }
}

fn read_connection(reader: &mut DwgMergedReader) -> BlockConnection {
    BlockConnection {
        code: reader.read_bit_long(),
        name: reader.read_variable_text(),
    }
}

fn read_property(reader: &mut DwgMergedReader) -> BlockParameterProperty {
    let count = safe_count(reader.read_bit_long());
    let mut connections = Vec::with_capacity(count as usize);
    for _ in 0..count {
        connections.push(read_connection(reader));
    }
    BlockParameterProperty { connections }
}

fn read_parameter(reader: &mut DwgMergedReader) -> BlockParameter {
    BlockParameter {
        element: read_element(reader),
        show_properties: reader.read_bit(),
        chain_actions: reader.read_bit(),
    }
}

fn read_one_point(reader: &mut DwgMergedReader) -> BlockOnePointParameter {
    let parameter = read_parameter(reader);
    let definition_point = reader.read_3bit_double();
    let properties = [read_property(reader), read_property(reader)];
    let property_count = reader.read_bit_long();
    BlockOnePointParameter {
        parameter,
        definition_point,
        properties,
        property_count,
    }
}

fn read_two_point(reader: &mut DwgMergedReader) -> BlockTwoPointParameter {
    let parameter = read_parameter(reader);
    let definition_base_point = reader.read_3bit_double();
    let definition_end_point = reader.read_3bit_double();
    let properties = [
        read_property(reader),
        read_property(reader),
        read_property(reader),
        read_property(reader),
    ];
    let mut property_states = [0; 4];
    for state in &mut property_states {
        *state = reader.read_bit_long();
    }
    BlockTwoPointParameter {
        parameter,
        definition_base_point,
        definition_end_point,
        properties,
        property_states,
        parameter_base_location: reader.read_bit_short(),
        updated_base_point: None,
        base_point: None,
        updated_end_point: None,
        end_point: None,
    }
}

fn read_grip(reader: &mut DwgMergedReader) -> BlockGrip {
    BlockGrip {
        element: read_element(reader),
        flags_91: reader.read_bit_long(),
        flags_92: reader.read_bit_long(),
        location: reader.read_3bit_double(),
        insert_cycling: reader.read_bit(),
        insert_cycling_weight: reader.read_bit_long(),
    }
}

fn read_action(reader: &mut DwgMergedReader) -> BlockAction {
    let element = read_element(reader);
    let display_location = reader.read_3bit_double();
    let dependency_count = safe_count(reader.read_bit_long());
    let mut dependencies = Vec::with_capacity(dependency_count as usize);
    for _ in 0..dependency_count {
        dependencies.push(Handle::from(reader.read_handle()));
    }
    let action_count = safe_count(reader.read_bit_long());
    let mut action_ids = Vec::with_capacity(action_count as usize);
    for _ in 0..action_count {
        action_ids.push(reader.read_bit_long());
    }
    BlockAction {
        element,
        display_location,
        dependencies,
        action_ids,
    }
}

fn read_action_with_base(reader: &mut DwgMergedReader) -> BlockActionWithBasePoint {
    BlockActionWithBasePoint {
        action: read_action(reader),
        offset: reader.read_3bit_double(),
        connections: vec![read_connection(reader), read_connection(reader)],
        dependent: reader.read_bit(),
        base_point: reader.read_3bit_double(),
    }
}

fn read_value_set(reader: &mut DwgMergedReader) -> BlockParameterValueSet {
    let flags = reader.read_bit_long();
    let minimum = reader.read_bit_double();
    let maximum = reader.read_bit_double();
    let increment = reader.read_bit_double();
    let count = safe_count(reader.read_bit_short() as i32);
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        values.push(reader.read_bit_double());
    }
    BlockParameterValueSet {
        description: String::new(),
        flags,
        minimum,
        maximum,
        increment,
        values,
    }
}

fn read_constraint(reader: &mut DwgMergedReader) -> BlockConstraintParameter {
    BlockConstraintParameter {
        parameter: read_two_point(reader),
        dependency: Handle::from(reader.read_handle()),
    }
}

fn read_linear_constraint(reader: &mut DwgMergedReader) -> BlockLinearConstraintParameter {
    BlockLinearConstraintParameter {
        constraint: read_constraint(reader),
        expression_name: reader.read_variable_text(),
        expression_description: reader.read_variable_text(),
        value: reader.read_bit_double(),
        value_set: read_value_set(reader),
    }
}

fn read_offsets(reader: &mut DwgMergedReader) -> BlockActionOffsets {
    BlockActionOffsets {
        offset_x: reader.read_bit_double(),
        offset_y: reader.read_bit_double(),
        angle_offset: reader.read_bit_double(),
    }
}

fn read_history_node_base(reader: &mut DwgMergedReader) -> SolidHistoryNodeBase {
    let eval = read_eval(reader);
    let major = reader.read_bit_long();
    let minor = reader.read_bit_long();
    let mut transform = [0.0; 16];
    for value in &mut transform {
        *value = reader.read_bit_double();
    }
    SolidHistoryNodeBase {
        eval,
        major,
        minor,
        transform,
        color: reader.read_cm_color(),
        step_id: reader.read_bit_long(),
        material: Handle::from(reader.read_handle()),
    }
}

fn read_history_sweep(
    reader: &mut DwgMergedReader,
    base: SolidHistoryNodeBase,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> SolidHistorySweep {
    let operation_major = reader.read_bit_long();
    let operation_minor = reader.read_bit_long();
    let direction = reader.read_3bit_double();
    let sweep_entity_type = reader.read_bit_long();
    let sweep_size = safe_count(reader.read_bit_long()) as usize;
    let sweep_entity = crate::io::dwg::embedded_entity::read_embedded_entity(
        reader,
        sweep_entity_type,
        sweep_size,
        version,
        dxf_version,
    );
    let path_entity_type = reader.read_bit_long();
    let path_size = safe_count(reader.read_bit_long()) as usize;
    let path_entity = crate::io::dwg::embedded_entity::read_embedded_entity(
        reader,
        path_entity_type,
        path_size,
        version,
        dxf_version,
    );
    let draft_angle = reader.read_bit_double();
    let start_draft_distance = reader.read_bit_double();
    let end_draft_distance = reader.read_bit_double();
    let scale_factor = reader.read_bit_double();
    let twist_angle = reader.read_bit_double();
    let align_angle = reader.read_bit_double();
    let mut sweep_entity_transform = [0.0; 16];
    let mut path_entity_transform = [0.0; 16];
    for value in &mut sweep_entity_transform {
        *value = reader.read_bit_double();
    }
    for value in &mut path_entity_transform {
        *value = reader.read_bit_double();
    }
    SolidHistorySweep {
        base,
        operation_major,
        operation_minor,
        direction,
        sweep_entity,
        path_entity,
        draft_angle,
        start_draft_distance,
        end_draft_distance,
        scale_factor,
        twist_angle,
        align_angle,
        sweep_entity_transform,
        path_entity_transform,
        align_option: reader.read_byte(),
        miter_option: reader.read_byte(),
        has_align_start: reader.read_bit(),
        bank: reader.read_bit(),
        check_intersections: reader.read_bit(),
        flags_294_296: [reader.read_bit(), reader.read_bit(), reader.read_bit()],
        reference_point: reader.read_3bit_double(),
    }
}

pub fn read_solid_history_data(
    reader: &mut DwgMergedReader,
    dxf_name: &str,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<DynamicBlockData> {
    if dxf_name == "ACSH_HISTORY_CLASS" {
        return Some(DynamicBlockData::SolidHistory(SolidHistory {
            major: reader.read_bit_long(),
            minor: reader.read_bit_long(),
            owner: Handle::from(reader.read_handle()),
            history_node_id: reader.read_bit_long(),
            show_history: reader.read_bit(),
            record_history: reader.read_bit(),
        }));
    }

    if !matches!(
        dxf_name,
        "ACSH_BOX_CLASS"
            | "ACSH_WEDGE_CLASS"
            | "ACSH_SPHERE_CLASS"
            | "ACSH_CYLINDER_CLASS"
            | "ACSH_CONE_CLASS"
            | "ACSH_PYRAMID_CLASS"
            | "ACSH_TORUS_CLASS"
            | "ACSH_BOOLEAN_CLASS"
            | "ACSH_CHAMFER_CLASS"
            | "ACSH_FILLET_CLASS"
            | "ACSH_BREP_CLASS"
            | "ACSH_SWEEP_CLASS"
            | "ACSH_EXTRUSION_CLASS"
            | "ACSH_LOFT_CLASS"
            | "ACSH_REVOLVE_CLASS"
    ) {
        return None;
    }

    let base = read_history_node_base(reader);
    let operation = match dxf_name {
        "ACSH_BOX_CLASS" | "ACSH_WEDGE_CLASS" => {
            let value = SolidHistoryBox {
                base,
                operation_major: reader.read_bit_long(),
                operation_minor: reader.read_bit_long(),
                length: reader.read_bit_double(),
                width: reader.read_bit_double(),
                height: reader.read_bit_double(),
            };
            if dxf_name == "ACSH_BOX_CLASS" {
                SolidHistoryOperation::Box(value)
            } else {
                SolidHistoryOperation::Wedge(value)
            }
        }
        "ACSH_SPHERE_CLASS" => SolidHistoryOperation::Sphere(SolidHistorySphere {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            radius: reader.read_bit_double(),
        }),
        "ACSH_CYLINDER_CLASS" => SolidHistoryOperation::Cylinder(SolidHistoryCylinder {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            height: reader.read_bit_double(),
            major_radius: reader.read_bit_double(),
            minor_radius: reader.read_bit_double(),
            x_radius: reader.read_bit_double(),
        }),
        "ACSH_CONE_CLASS" => SolidHistoryOperation::Cone(SolidHistoryCone {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            height: reader.read_bit_double(),
            base_x_radius: reader.read_bit_double(),
            base_y_radius: reader.read_bit_double(),
            top_radius: reader.read_bit_double(),
        }),
        "ACSH_PYRAMID_CLASS" => SolidHistoryOperation::Pyramid(SolidHistoryPyramid {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            height: reader.read_bit_double(),
            sides: reader.read_bit_long(),
            radius: reader.read_bit_double(),
            top_radius: reader.read_bit_double(),
        }),
        "ACSH_TORUS_CLASS" => SolidHistoryOperation::Torus(SolidHistoryTorus {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            major_radius: reader.read_bit_double(),
            minor_radius: reader.read_bit_double(),
        }),
        "ACSH_BOOLEAN_CLASS" => SolidHistoryOperation::Boolean(SolidHistoryBoolean {
            base,
            operation_major: reader.read_bit_long(),
            operation_minor: reader.read_bit_long(),
            operation: reader.read_byte(),
            first_operand: reader.read_bit_long(),
            second_operand: reader.read_bit_long(),
        }),
        "ACSH_CHAMFER_CLASS" => {
            let operation_major = reader.read_bit_long();
            let operation_minor = reader.read_bit_long();
            let method = reader.read_bit_long();
            let base_distance = reader.read_bit_double();
            let other_distance = reader.read_bit_double();
            let count = safe_count(reader.read_bit_long());
            let mut edges = Vec::with_capacity(count as usize);
            for _ in 0..count {
                edges.push(reader.read_bit_long());
            }
            SolidHistoryOperation::Chamfer(SolidHistoryChamfer {
                base,
                operation_major,
                operation_minor,
                method,
                base_distance,
                other_distance,
                edges,
                base_face: reader.read_bit_long(),
            })
        }
        "ACSH_FILLET_CLASS" => {
            let operation_major = reader.read_bit_long();
            let operation_minor = reader.read_bit_long();
            let method = reader.read_bit_long();
            let edge_count = safe_count(reader.read_bit_long());
            let mut edges = Vec::with_capacity(edge_count as usize);
            for _ in 0..edge_count {
                edges.push(reader.read_bit_long());
            }
            let radius_count = safe_count(reader.read_bit_long());
            let mut radii = Vec::with_capacity(radius_count as usize);
            for _ in 0..radius_count {
                radii.push(reader.read_bit_double());
            }
            let start_count = safe_count(reader.read_bit_long());
            let end_count = safe_count(reader.read_bit_long());
            let mut end_setbacks = Vec::with_capacity(end_count as usize);
            for _ in 0..end_count {
                end_setbacks.push(reader.read_bit_double());
            }
            let mut start_setbacks = Vec::with_capacity(start_count as usize);
            for _ in 0..start_count {
                start_setbacks.push(reader.read_bit_double());
            }
            SolidHistoryOperation::Fillet(SolidHistoryFillet {
                base,
                operation_major,
                operation_minor,
                method,
                edges,
                radii,
                start_setbacks,
                end_setbacks,
            })
        }
        "ACSH_BREP_CLASS" => {
            let operation_major = reader.read_bit_long();
            let operation_minor = reader.read_bit_long();
            let data =
                super::entities::read_history_acis_entity(reader, version, dxf_version);
            let mut acis_data = crate::entities::AcisData::new();
            acis_data.sat_data = data.sat_data;
            acis_data.sab_data = data.sab_data;
            acis_data.is_binary = data.is_binary;
            acis_data.revision = data.revision;
            acis_data.materials = data.materials;
            acis_data.wireframe_data_present = data.wireframe_data_present;
            acis_data.wireframe_point_present = data.wireframe_point_present;
            acis_data.wireframe_isoline_present = data.wireframe_isoline_present;
            acis_data.acis_empty_bit = data.acis_empty_bit;
            acis_data.extra_acis_data = data.extra_acis_data.map(Box::new);
            acis_data.wireframe_isolines = data.isolines;
            SolidHistoryOperation::Brep(SolidHistoryBrep {
                base,
                operation_major,
                operation_minor,
                acis_data,
            })
        }
        "ACSH_SWEEP_CLASS" => {
            SolidHistoryOperation::Sweep(read_history_sweep(reader, base, version, dxf_version))
        }
        "ACSH_EXTRUSION_CLASS" => {
            SolidHistoryOperation::Extrusion(read_history_sweep(reader, base, version, dxf_version))
        }
        "ACSH_LOFT_CLASS" => {
            let operation_major = reader.read_bit_long();
            let operation_minor = reader.read_bit_long();
            let cross_count = safe_count(reader.read_bit_long());
            let mut cross_sections = Vec::with_capacity(cross_count as usize);
            for _ in 0..cross_count {
                let entity_type = reader.read_bit_long();
                let byte_length = safe_count(reader.read_bit_long()) as usize;
                if let Some(entity) = crate::io::dwg::embedded_entity::read_embedded_entity(
                    reader,
                    entity_type,
                    byte_length,
                    version,
                    dxf_version,
                ) {
                    cross_sections.push(entity);
                }
            }
            let guide_count = safe_count(reader.read_bit_long());
            let mut guides = Vec::with_capacity(guide_count as usize);
            for _ in 0..guide_count {
                let entity_type = reader.read_bit_long();
                let byte_length = safe_count(reader.read_bit_long()) as usize;
                if let Some(entity) = crate::io::dwg::embedded_entity::read_embedded_entity(
                    reader,
                    entity_type,
                    byte_length,
                    version,
                    dxf_version,
                ) {
                    guides.push(entity);
                }
            }
            SolidHistoryOperation::Loft(SolidHistoryLoft {
                base,
                operation_major,
                operation_minor,
                cross_sections,
                guides,
                parameters: None,
            })
        }
        "ACSH_REVOLVE_CLASS" => {
            let operation_major = reader.read_bit_long();
            let operation_minor = reader.read_bit_long();
            let axis_point = reader.read_3bit_double();
            let direction = reader.read_3raw_double();
            let revolve_angle = reader.read_bit_double();
            let start_angle = reader.read_bit_double();
            let draft_angle = reader.read_bit_double();
            let field_44 = reader.read_bit_double();
            let field_45 = reader.read_bit_double();
            let twist_angle = reader.read_bit_double();
            let flag_290 = reader.read_bit();
            let close_to_axis = reader.read_bit();
            let entity_type = reader.read_bit_long();
            let byte_length = safe_count(reader.read_bit_long()) as usize;
            let sweep_entity = crate::io::dwg::embedded_entity::read_embedded_entity(
                reader,
                entity_type,
                byte_length,
                version,
                dxf_version,
            );
            SolidHistoryOperation::Revolve(SolidHistoryRevolve {
                base,
                operation_major,
                operation_minor,
                axis_point,
                direction,
                revolve_angle,
                start_angle,
                draft_angle,
                field_44,
                field_45,
                twist_angle,
                flag_290,
                close_to_axis,
                sweep_entity,
            })
        }
        _ => return None,
    };
    Some(DynamicBlockData::SolidHistoryNode(operation))
}

pub fn read_dynamic_block_data(
    reader: &mut DwgMergedReader,
    dxf_name: &str,
) -> Option<DynamicBlockData> {
    let data = match dxf_name.to_ascii_uppercase().as_str() {
        "ACDB_BLOCKREPRESENTATION_DATA"
        | "BLOCKREPRESENTATION"
        | "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION"
        | "DYNAMICBLOCKPURGEPREVENTER" => {
            DynamicBlockData::Representation(BlockRepresentationData {
                flags: reader.read_bit_short(),
                block: Handle::from(reader.read_handle()),
            })
        }
        "DYNAMICBLOCKPROXYNODE" | "ACDB_DYNAMICBLOCKPROXYNODE" => {
            DynamicBlockData::ProxyNode(read_eval(reader))
        }
        "BLOCKGRIPLOCATIONCOMPONENT" => {
            DynamicBlockData::GripLocationComponent(BlockGripExpression {
                eval: read_eval(reader),
                grip_type: reader.read_bit_long(),
                expression: reader.read_variable_text(),
            })
        }
        "BLOCKALIGNMENTGRIP" => DynamicBlockData::AlignmentGrip(BlockOrientedGrip {
            grip: read_grip(reader),
            orientation: reader.read_3bit_double(),
        }),
        "BLOCKFLIPGRIP" => DynamicBlockData::FlipGrip(BlockFlipGrip {
            grip: read_grip(reader),
            combined_state: reader.read_bit_long(),
            orientation: reader.read_3bit_double(),
        }),
        "BLOCKLINEARGRIP" => DynamicBlockData::LinearGrip(BlockOrientedGrip {
            grip: read_grip(reader),
            orientation: reader.read_3bit_double(),
        }),
        "BLOCKLOOKUPGRIP" => DynamicBlockData::LookupGrip(read_grip(reader)),
        "BLOCKPOLARGRIP" => DynamicBlockData::PolarGrip(read_grip(reader)),
        "BLOCKROTATIONGRIP" => DynamicBlockData::RotationGrip(read_grip(reader)),
        "BLOCKVISIBILITYGRIP" => DynamicBlockData::VisibilityGrip(read_grip(reader)),
        "BLOCKXYGRIP" => DynamicBlockData::XYGrip(read_grip(reader)),
        "BLOCKPROPERTIESTABLEGRIP" => DynamicBlockData::PropertiesTableGrip(read_grip(reader)),
        "BLOCKALIGNMENTPARAMETER" => {
            DynamicBlockData::AlignmentParameter(BlockAlignmentParameter {
                parameter: read_two_point(reader),
                align_perpendicular: reader.read_bit(),
            })
        }
        "BLOCKBASEPOINTPARAMETER" => {
            DynamicBlockData::BasePointParameter(BlockBasePointParameter {
                parameter: read_one_point(reader),
                point: reader.read_3bit_double(),
                base_point: reader.read_3bit_double(),
            })
        }
        "BLOCKFLIPPARAMETER" => DynamicBlockData::FlipParameter(BlockFlipParameter {
            parameter: read_two_point(reader),
            flip_label: reader.read_variable_text(),
            flip_label_description: reader.read_variable_text(),
            base_state_label: reader.read_variable_text(),
            flipped_state_label: reader.read_variable_text(),
            definition_label_point: reader.read_3bit_double(),
            flags_96: reader.read_bit_long(),
            tooltip: reader.read_variable_text(),
        }),
        "BLOCKLINEARPARAMETER" => DynamicBlockData::LinearParameter(BlockLinearParameter {
            parameter: read_two_point(reader),
            distance_name: reader.read_variable_text(),
            distance_description: reader.read_variable_text(),
            distance: reader.read_bit_double(),
            value_set: read_value_set(reader),
        }),
        "BLOCKLOOKUPPARAMETER" => DynamicBlockData::LookupParameter(BlockLookupParameter {
            parameter: read_one_point(reader),
            index: reader.read_bit_long(),
            lookup_name: reader.read_variable_text(),
            lookup_description: reader.read_variable_text(),
            unknown_text: reader.read_variable_text(),
        }),
        "BLOCKPOINTPARAMETER" => DynamicBlockData::PointParameter(BlockPointParameter {
            parameter: read_one_point(reader),
            position_name: reader.read_variable_text(),
            position_description: reader.read_variable_text(),
            definition_label_point: reader.read_3bit_double(),
        }),
        "BLOCKPOLARPARAMETER" => DynamicBlockData::PolarParameter(BlockPolarParameter {
            parameter: read_two_point(reader),
            angle_name: reader.read_variable_text(),
            angle_description: reader.read_variable_text(),
            distance_name: reader.read_variable_text(),
            distance_description: reader.read_variable_text(),
            offset: reader.read_bit_double(),
            angle_value_set: read_value_set(reader),
            distance_value_set: read_value_set(reader),
        }),
        "BLOCKROTATIONPARAMETER" => DynamicBlockData::RotationParameter(BlockRotationParameter {
            parameter: read_two_point(reader),
            definition_base_angle_point: reader.read_3bit_double(),
            angle_name: reader.read_variable_text(),
            angle_description: reader.read_variable_text(),
            angle: reader.read_bit_double(),
            value_set: read_value_set(reader),
        }),
        "BLOCKXYPARAMETER" => DynamicBlockData::XYParameter(BlockXYParameter {
            parameter: read_two_point(reader),
            x_label: reader.read_variable_text(),
            x_label_description: reader.read_variable_text(),
            y_label: reader.read_variable_text(),
            y_label_description: reader.read_variable_text(),
            x_value: reader.read_bit_double(),
            y_value: reader.read_bit_double(),
            x_value_set: read_value_set(reader),
            y_value_set: read_value_set(reader),
        }),
        "BLOCKUSERPARAMETER" => {
            let parameter = read_one_point(reader);
            let flags = reader.read_bit_short();
            let associated_variable = Handle::from(reader.read_handle());
            let expression = reader.read_variable_text();
            let value_code = reader.read_bit_short();
            DynamicBlockData::UserParameter(BlockUserParameter {
                parameter,
                flags,
                associated_variable,
                expression,
                value_code,
                value: read_eval_value(reader, value_code),
                value_type: reader.read_bit_short(),
            })
        }
        "BLOCKANGULARCONSTRAINTPARAMETERENTITY" => {
            DynamicBlockData::AngularConstraintParameterEntity(
                BlockAngularConstraintParameterEntity {
                    constraint: read_constraint(reader),
                    center_point: reader.read_3bit_double(),
                    label_point: reader.read_3bit_double(),
                    expression_name: reader.read_variable_text(),
                    expression_description: reader.read_variable_text(),
                    angle: reader.read_bit_double(),
                    orientation_on_both_grips: reader.read_bit(),
                    value_set: read_value_set(reader),
                },
            )
        }
        "BLOCKANGULARCONSTRAINTPARAMETER" => {
            DynamicBlockData::AngularConstraintParameter(BlockAngularConstraintParameter {
                constraint: read_constraint(reader),
                center_point: reader.read_3bit_double(),
                end_point: reader.read_3bit_double(),
                expression_name: reader.read_variable_text(),
                expression_description: reader.read_variable_text(),
                angle: reader.read_bit_double(),
                orientation_on_both_grips: reader.read_bit(),
                value_set: read_value_set(reader),
            })
        }
        "BLOCKDIAMETRICCONSTRAINTPARAMETER" => {
            DynamicBlockData::DiametricConstraintParameter(BlockDistanceConstraintParameter {
                constraint: read_constraint(reader),
                expression_name: reader.read_variable_text(),
                expression_description: reader.read_variable_text(),
                distance: reader.read_bit_double(),
                value_set: read_value_set(reader),
            })
        }
        "BLOCKRADIALCONSTRAINTPARAMETER" => {
            DynamicBlockData::RadialConstraintParameter(BlockDistanceConstraintParameter {
                constraint: read_constraint(reader),
                expression_name: reader.read_variable_text(),
                expression_description: reader.read_variable_text(),
                distance: reader.read_bit_double(),
                value_set: read_value_set(reader),
            })
        }
        "BLOCKALIGNEDCONSTRAINTPARAMETER" => {
            DynamicBlockData::AlignedConstraintParameter(read_linear_constraint(reader))
        }
        "BLOCKLINEARCONSTRAINTPARAMETER" => {
            DynamicBlockData::LinearConstraintParameter(read_linear_constraint(reader))
        }
        "BLOCKHORIZONTALCONSTRAINTPARAMETER" => {
            DynamicBlockData::HorizontalConstraintParameter(read_linear_constraint(reader))
        }
        "BLOCKVERTICALCONSTRAINTPARAMETER" => {
            DynamicBlockData::VerticalConstraintParameter(read_linear_constraint(reader))
        }
        "ACDBBLOCKPARAMDEPENDENCYBODY" | "BLOCKPARAMDEPENDENCYBODY" => {
            DynamicBlockData::ParameterDependencyBody(BlockParameterDependencyBody {
                dependency_body_version: reader.read_bit_short(),
                dimension_base_version: reader.read_bit_short(),
                name: reader.read_variable_text(),
                class_version: reader.read_bit_short(),
            })
        }
        "BLOCKMOVEACTION" => DynamicBlockData::MoveAction(BlockMoveAction {
            action: read_action(reader),
            connections: [read_connection(reader), read_connection(reader)],
            offsets: read_offsets(reader),
        }),
        "BLOCKFLIPACTION" => DynamicBlockData::FlipAction(BlockFlipAction {
            action: read_action(reader),
            connections: [
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
            ],
        }),
        "BLOCKROTATEACTION" => DynamicBlockData::RotateAction(BlockBasePointAction {
            action: read_action_with_base(reader),
            connections: vec![read_connection(reader)],
        }),
        "BLOCKSCALEACTION" => DynamicBlockData::ScaleAction(BlockBasePointAction {
            action: read_action_with_base(reader),
            connections: vec![
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
            ],
        }),
        "BLOCKARRAYACTION" => DynamicBlockData::ArrayAction(BlockArrayAction {
            action: read_action(reader),
            connections: [
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
            ],
            column_offset: reader.read_bit_double(),
            row_offset: reader.read_bit_double(),
        }),
        "BLOCKLOOKUPACTION" => {
            let action = read_action(reader);
            let row_count = reader.read_bit_long();
            let column_count = reader.read_bit_long();
            let count = safe_count(row_count.saturating_mul(column_count));
            let mut expressions = Vec::with_capacity(count as usize);
            for _ in 0..count {
                expressions.push(reader.read_variable_text());
            }
            let mut rows = Vec::with_capacity(count as usize);
            for _ in 0..count {
                rows.push(BlockLookupRow {
                    connections: [
                        read_connection(reader),
                        read_connection(reader),
                        read_connection(reader),
                    ],
                    flag_282: reader.read_bit(),
                    flag_281: reader.read_bit(),
                });
            }
            DynamicBlockData::LookupAction(BlockLookupAction {
                action,
                row_count,
                column_count,
                expressions,
                rows,
                flag_280: reader.read_bit(),
            })
        }
        "BLOCKSTRETCHACTION" => {
            let action = read_action(reader);
            let connections = [read_connection(reader), read_connection(reader)];
            let point_count = safe_count(reader.read_bit_long());
            let mut points = Vec::with_capacity(point_count as usize);
            for _ in 0..point_count {
                points.push(reader.read_2raw_double());
            }
            let handle_count = safe_count(reader.read_bit_long());
            let mut handles = Vec::with_capacity(handle_count as usize);
            for _ in 0..handle_count {
                let handle = Handle::from(reader.read_handle());
                let count = safe_count(reader.read_bit_short() as i32);
                let mut indexes = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    indexes.push(reader.read_bit_long());
                }
                handles.push(BlockStretchHandle { handle, indexes });
            }
            let code_count = safe_count(reader.read_bit_long());
            let mut codes = Vec::with_capacity(code_count as usize);
            for _ in 0..code_count {
                let code = reader.read_bit_long();
                let count = safe_count(reader.read_bit_short() as i32);
                let mut indexes = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    indexes.push(reader.read_bit_long());
                }
                codes.push(BlockStretchCode { code, indexes });
            }
            DynamicBlockData::StretchAction(BlockStretchAction {
                action,
                connections,
                points,
                handles,
                codes,
                offsets: read_offsets(reader),
            })
        }
        "BLOCKPOLARSTRETCHACTION" => {
            let action = read_action(reader);
            let connections = [
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
                read_connection(reader),
            ];
            let point_count = safe_count(reader.read_bit_long());
            let mut points = Vec::with_capacity(point_count as usize);
            for _ in 0..point_count {
                points.push(reader.read_2raw_double());
            }
            let handle_count = safe_count(reader.read_bit_long());
            let mut handles = Vec::with_capacity(handle_count as usize);
            for _ in 0..handle_count {
                handles.push(Handle::from(reader.read_handle()));
            }
            let mut handle_flags = Vec::with_capacity(handle_count as usize);
            for _ in 0..handle_count {
                handle_flags.push(reader.read_bit_short());
            }
            let code_count = safe_count(reader.read_bit_long());
            let mut codes = Vec::with_capacity(code_count as usize);
            for _ in 0..code_count {
                codes.push(reader.read_bit_long());
            }
            DynamicBlockData::PolarStretchAction(BlockPolarStretchAction {
                action,
                connections,
                points,
                handles,
                handle_flags,
                codes,
            })
        }
        "BLOCKPROPERTIESTABLE" => DynamicBlockData::PropertiesTable,
        "EVALUATION_GRAPH" | "ACAD_EVALUATION_GRAPH" => {
            let first_node_id = reader.read_bit_long();
            let first_node_id_copy = reader.read_bit_long();
            let node_count = safe_count(reader.read_bit_long());
            let mut nodes = Vec::with_capacity(node_count as usize);
            for _ in 0..node_count {
                let id = reader.read_bit_long();
                let edge_flags = reader.read_bit_long();
                let next_id = reader.read_bit_long();
                let expression = Handle::from(reader.read_handle());
                let mut node_data = [0; 4];
                for value in &mut node_data {
                    *value = reader.read_bit_long();
                }
                nodes.push(BlockEvaluationNode {
                    id,
                    edge_flags,
                    next_id,
                    expression,
                    node_data,
                    active_cycles: None,
                });
            }
            let edge_count = safe_count(reader.read_bit_long());
            let mut edges = Vec::with_capacity(edge_count as usize);
            for _ in 0..edge_count {
                let id = reader.read_bit_long();
                let next_id = reader.read_bit_long();
                let incoming_edge = reader.read_bit_long();
                let source_node = reader.read_bit_long();
                let destination_node = reader.read_bit_long();
                let mut outgoing_edges = [0; 5];
                for value in &mut outgoing_edges {
                    *value = reader.read_bit_long();
                }
                edges.push(BlockEvaluationEdge {
                    id,
                    next_id,
                    incoming_edge,
                    source_node,
                    destination_node,
                    outgoing_edges,
                });
            }
            DynamicBlockData::EvaluationGraph(BlockEvaluationGraph {
                first_node_id,
                first_node_id_copy,
                nodes,
                edges,
            })
        }
        _ => return None,
    };
    Some(data)
}
