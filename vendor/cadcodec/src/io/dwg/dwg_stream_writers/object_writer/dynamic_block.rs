use super::DwgObjectWriter;
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::objects::*;

impl<'a> DwgObjectWriter<'a> {
    fn write_dynamic_eval_value(&mut self, value: &BlockEvalValue) {
        match value {
            BlockEvalValue::Real(value) => self.writer.write_bit_double(*value),
            BlockEvalValue::Point(value) => self
                .writer
                .write_2raw_double(crate::types::Vector2::new(value[0], value[1])),
            BlockEvalValue::Text(value) => self.writer.write_variable_text(value),
            BlockEvalValue::Long(value) => self.writer.write_bit_long(*value),
            BlockEvalValue::Handle(value) => self
                .writer
                .write_handle(DwgReferenceType::HardPointer, value.value()),
            BlockEvalValue::Short(value) => self.writer.write_bit_short(*value),
            BlockEvalValue::None => {}
        }
    }

    fn write_dynamic_eval(&mut self, value: &BlockEvalExpression) {
        self.writer.write_bit_long(value.parent_id);
        self.writer.write_bit_long(value.major);
        self.writer.write_bit_long(value.minor);
        self.writer.write_bit_short(value.value_code);
        self.write_dynamic_eval_value(&value.value);
        self.writer.write_bit_long(value.node_id);
    }

    fn write_dynamic_element(&mut self, value: &BlockElement) {
        self.write_dynamic_eval(&value.eval);
        self.writer.write_variable_text(&value.name);
        self.writer.write_bit_long(value.major);
        self.writer.write_bit_long(value.minor);
        self.writer.write_bit_long(value.eed_1071);
    }

    fn write_dynamic_connection(&mut self, value: &BlockConnection) {
        self.writer.write_bit_long(value.code);
        self.writer.write_variable_text(&value.name);
    }

    fn write_dynamic_property(&mut self, value: &BlockParameterProperty) {
        self.writer.write_bit_long(value.connections.len() as i32);
        for connection in &value.connections {
            self.write_dynamic_connection(connection);
        }
    }

    fn write_dynamic_parameter(&mut self, value: &BlockParameter) {
        self.write_dynamic_element(&value.element);
        self.writer.write_bit(value.show_properties);
        self.writer.write_bit(value.chain_actions);
    }

    fn write_dynamic_one_point(&mut self, value: &BlockOnePointParameter) {
        self.write_dynamic_parameter(&value.parameter);
        self.writer.write_3bit_double(value.definition_point);
        for property in &value.properties {
            self.write_dynamic_property(property);
        }
        self.writer.write_bit_long(value.property_count);
    }

    fn write_dynamic_two_point(&mut self, value: &BlockTwoPointParameter) {
        self.write_dynamic_parameter(&value.parameter);
        self.writer.write_3bit_double(value.definition_base_point);
        self.writer.write_3bit_double(value.definition_end_point);
        for property in &value.properties {
            self.write_dynamic_property(property);
        }
        for state in value.property_states {
            self.writer.write_bit_long(state);
        }
        self.writer.write_bit_short(value.parameter_base_location);
    }

    fn write_dynamic_grip(&mut self, value: &BlockGrip) {
        self.write_dynamic_element(&value.element);
        self.writer.write_bit_long(value.flags_91);
        self.writer.write_bit_long(value.flags_92);
        self.writer.write_3bit_double(value.location);
        self.writer.write_bit(value.insert_cycling);
        self.writer.write_bit_long(value.insert_cycling_weight);
    }

    fn write_dynamic_action(&mut self, value: &BlockAction) {
        self.write_dynamic_element(&value.element);
        self.writer.write_3bit_double(value.display_location);
        self.writer.write_bit_long(value.dependencies.len() as i32);
        for handle in &value.dependencies {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, handle.value());
        }
        self.writer.write_bit_long(value.action_ids.len() as i32);
        for id in &value.action_ids {
            self.writer.write_bit_long(*id);
        }
    }

    fn write_dynamic_action_with_base(&mut self, value: &BlockActionWithBasePoint) {
        self.write_dynamic_action(&value.action);
        self.writer.write_3bit_double(value.offset);
        for connection in &value.connections {
            self.write_dynamic_connection(connection);
        }
        self.writer.write_bit(value.dependent);
        self.writer.write_3bit_double(value.base_point);
    }

    fn write_dynamic_value_set(&mut self, value: &BlockParameterValueSet) {
        self.writer.write_bit_long(value.flags);
        self.writer.write_bit_double(value.minimum);
        self.writer.write_bit_double(value.maximum);
        self.writer.write_bit_double(value.increment);
        self.writer.write_bit_short(value.values.len() as i16);
        for item in &value.values {
            self.writer.write_bit_double(*item);
        }
    }

    fn write_dynamic_constraint(&mut self, value: &BlockConstraintParameter) {
        self.write_dynamic_two_point(&value.parameter);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, value.dependency.value());
    }

    fn write_dynamic_linear_constraint(&mut self, value: &BlockLinearConstraintParameter) {
        self.write_dynamic_constraint(&value.constraint);
        self.writer.write_variable_text(&value.expression_name);
        self.writer
            .write_variable_text(&value.expression_description);
        self.writer.write_bit_double(value.value);
        self.write_dynamic_value_set(&value.value_set);
    }

    fn write_dynamic_offsets(&mut self, value: &BlockActionOffsets) {
        self.writer.write_bit_double(value.offset_x);
        self.writer.write_bit_double(value.offset_y);
        self.writer.write_bit_double(value.angle_offset);
    }

    pub(super) fn write_dynamic_angular_constraint_entity(
        &mut self,
        value: &BlockAngularConstraintParameterEntity,
    ) {
        self.write_dynamic_constraint(&value.constraint);
        self.writer.write_3bit_double(value.center_point);
        self.writer.write_3bit_double(value.label_point);
        self.writer.write_variable_text(&value.expression_name);
        self.writer
            .write_variable_text(&value.expression_description);
        self.writer.write_bit_double(value.angle);
        self.writer.write_bit(value.orientation_on_both_grips);
        self.write_dynamic_value_set(&value.value_set);
    }

    fn write_solid_history_base(&mut self, value: &SolidHistoryNodeBase) {
        self.write_dynamic_eval(&value.eval);
        self.writer.write_bit_long(value.major);
        self.writer.write_bit_long(value.minor);
        for item in value.transform {
            self.writer.write_bit_double(item);
        }
        self.writer.write_cm_color(&value.color);
        self.writer.write_bit_long(value.step_id);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, value.material.value());
    }

    fn write_solid_history_sweep(&mut self, value: &SolidHistorySweep) {
        self.write_solid_history_base(&value.base);
        self.writer.write_bit_long(value.operation_major);
        self.writer.write_bit_long(value.operation_minor);
        self.writer.write_3bit_double(value.direction);
        if let Some(entity) = &value.sweep_entity {
            let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                entity,
                self.version,
                self.dxf_version,
            );
            self.writer.write_bit_long(encoded.type_code);
            self.writer.write_bit_long(encoded.bytes.len() as i32);
            crate::io::dwg::embedded_entity::write_embedded_bytes(&mut self.writer, &encoded);
        } else {
            self.writer.write_bit_long(0);
            self.writer.write_bit_long(0);
        }
        if let Some(entity) = &value.path_entity {
            let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                entity,
                self.version,
                self.dxf_version,
            );
            self.writer.write_bit_long(encoded.type_code);
            self.writer.write_bit_long(encoded.bytes.len() as i32);
            crate::io::dwg::embedded_entity::write_embedded_bytes(&mut self.writer, &encoded);
        } else {
            self.writer.write_bit_long(0);
            self.writer.write_bit_long(0);
        }
        self.writer.write_bit_double(value.draft_angle);
        self.writer.write_bit_double(value.start_draft_distance);
        self.writer.write_bit_double(value.end_draft_distance);
        self.writer.write_bit_double(value.scale_factor);
        self.writer.write_bit_double(value.twist_angle);
        self.writer.write_bit_double(value.align_angle);
        for item in value.sweep_entity_transform {
            self.writer.write_bit_double(item);
        }
        for item in value.path_entity_transform {
            self.writer.write_bit_double(item);
        }
        self.writer.write_byte(value.align_option);
        self.writer.write_byte(value.miter_option);
        self.writer.write_bit(value.has_align_start);
        self.writer.write_bit(value.bank);
        self.writer.write_bit(value.check_intersections);
        for item in value.flags_294_296 {
            self.writer.write_bit(item);
        }
        self.writer.write_3bit_double(value.reference_point);
    }

    fn write_solid_history_operation(&mut self, value: &SolidHistoryOperation) {
        match value {
            SolidHistoryOperation::Unknown => {}
            SolidHistoryOperation::Box(value) | SolidHistoryOperation::Wedge(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.length);
                self.writer.write_bit_double(value.width);
                self.writer.write_bit_double(value.height);
            }
            SolidHistoryOperation::Sphere(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.radius);
            }
            SolidHistoryOperation::Cylinder(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.height);
                self.writer.write_bit_double(value.major_radius);
                self.writer.write_bit_double(value.minor_radius);
                self.writer.write_bit_double(value.x_radius);
            }
            SolidHistoryOperation::Cone(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.height);
                self.writer.write_bit_double(value.base_x_radius);
                self.writer.write_bit_double(value.base_y_radius);
                self.writer.write_bit_double(value.top_radius);
            }
            SolidHistoryOperation::Pyramid(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.height);
                self.writer.write_bit_long(value.sides);
                self.writer.write_bit_double(value.radius);
                self.writer.write_bit_double(value.top_radius);
            }
            SolidHistoryOperation::Torus(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_double(value.major_radius);
                self.writer.write_bit_double(value.minor_radius);
            }
            SolidHistoryOperation::Boolean(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_byte(value.operation);
                self.writer.write_bit_long(value.first_operand);
                self.writer.write_bit_long(value.second_operand);
            }
            SolidHistoryOperation::Brep(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.write_acis_data(crate::types::Vector3::ZERO, &value.acis_data, &[], &[]);
            }
            SolidHistoryOperation::Fillet(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_long(value.method);
                self.writer.write_bit_long(value.edges.len() as i32);
                for item in &value.edges {
                    self.writer.write_bit_long(*item);
                }
                self.writer.write_bit_long(value.radii.len() as i32);
                for item in &value.radii {
                    self.writer.write_bit_double(*item);
                }
                self.writer
                    .write_bit_long(value.start_setbacks.len() as i32);
                self.writer.write_bit_long(value.end_setbacks.len() as i32);
                for item in &value.end_setbacks {
                    self.writer.write_bit_double(*item);
                }
                for item in &value.start_setbacks {
                    self.writer.write_bit_double(*item);
                }
            }
            SolidHistoryOperation::Chamfer(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_bit_long(value.method);
                self.writer.write_bit_double(value.base_distance);
                self.writer.write_bit_double(value.other_distance);
                self.writer.write_bit_long(value.edges.len() as i32);
                for item in &value.edges {
                    self.writer.write_bit_long(*item);
                }
                self.writer.write_bit_long(value.base_face);
            }
            SolidHistoryOperation::Sweep(value) | SolidHistoryOperation::Extrusion(value) => {
                self.write_solid_history_sweep(value);
            }
            SolidHistoryOperation::Loft(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer
                    .write_bit_long(value.cross_sections.len() as i32);
                for entity in &value.cross_sections {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(encoded.bytes.len() as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bytes(
                        &mut self.writer,
                        &encoded,
                    );
                }
                self.writer.write_bit_long(value.guides.len() as i32);
                for entity in &value.guides {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(encoded.bytes.len() as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bytes(
                        &mut self.writer,
                        &encoded,
                    );
                }
            }
            SolidHistoryOperation::Revolve(value) => {
                self.write_solid_history_base(&value.base);
                self.writer.write_bit_long(value.operation_major);
                self.writer.write_bit_long(value.operation_minor);
                self.writer.write_3bit_double(value.axis_point);
                self.writer.write_3raw_double(value.direction);
                self.writer.write_bit_double(value.revolve_angle);
                self.writer.write_bit_double(value.start_angle);
                self.writer.write_bit_double(value.draft_angle);
                self.writer.write_bit_double(value.field_44);
                self.writer.write_bit_double(value.field_45);
                self.writer.write_bit_double(value.twist_angle);
                self.writer.write_bit(value.flag_290);
                self.writer.write_bit(value.close_to_axis);
                if let Some(entity) = &value.sweep_entity {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(encoded.bytes.len() as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bytes(
                        &mut self.writer,
                        &encoded,
                    );
                } else {
                    self.writer.write_bit_long(0);
                    self.writer.write_bit_long(0);
                }
            }
        }
    }

    pub(super) fn write_dynamic_block(&mut self, object: &DynamicBlockObject) {
        let type_code = self.class_type_code(&object.dxf_name, 500);
        self.write_common_non_entity_data(
            type_code,
            object.handle,
            object.owner,
            &object.reactors,
            &object.xdictionary_handle,
        );
        match &object.data {
            DynamicBlockData::Unknown => return,
            DynamicBlockData::Representation(value) => {
                self.writer.write_bit_short(value.flags);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.block.value());
            }
            DynamicBlockData::ProxyNode(value) => self.write_dynamic_eval(value),
            DynamicBlockData::GripLocationComponent(value) => {
                self.write_dynamic_eval(&value.eval);
                self.writer.write_bit_long(value.grip_type);
                self.writer.write_variable_text(&value.expression);
            }
            DynamicBlockData::AlignmentGrip(value) | DynamicBlockData::LinearGrip(value) => {
                self.write_dynamic_grip(&value.grip);
                self.writer.write_3bit_double(value.orientation);
            }
            DynamicBlockData::FlipGrip(value) => {
                self.write_dynamic_grip(&value.grip);
                self.writer.write_bit_long(value.combined_state);
                self.writer.write_3bit_double(value.orientation);
            }
            DynamicBlockData::LookupGrip(value)
            | DynamicBlockData::PolarGrip(value)
            | DynamicBlockData::RotationGrip(value)
            | DynamicBlockData::VisibilityGrip(value)
            | DynamicBlockData::XYGrip(value)
            | DynamicBlockData::PropertiesTableGrip(value) => {
                self.write_dynamic_grip(value);
            }
            DynamicBlockData::AlignmentParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer.write_bit(value.align_perpendicular);
            }
            DynamicBlockData::BasePointParameter(value) => {
                self.write_dynamic_one_point(&value.parameter);
                self.writer.write_3bit_double(value.point);
                self.writer.write_3bit_double(value.base_point);
            }
            DynamicBlockData::FlipParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer.write_variable_text(&value.flip_label);
                self.writer
                    .write_variable_text(&value.flip_label_description);
                self.writer.write_variable_text(&value.base_state_label);
                self.writer.write_variable_text(&value.flipped_state_label);
                self.writer.write_3bit_double(value.definition_label_point);
                self.writer.write_bit_long(value.flags_96);
                self.writer.write_variable_text(&value.tooltip);
            }
            DynamicBlockData::LinearParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer.write_variable_text(&value.distance_name);
                self.writer.write_variable_text(&value.distance_description);
                self.writer.write_bit_double(value.distance);
                self.write_dynamic_value_set(&value.value_set);
            }
            DynamicBlockData::LookupParameter(value) => {
                self.write_dynamic_one_point(&value.parameter);
                self.writer.write_bit_long(value.index);
                self.writer.write_variable_text(&value.lookup_name);
                self.writer.write_variable_text(&value.lookup_description);
                self.writer.write_variable_text(&value.unknown_text);
            }
            DynamicBlockData::PointParameter(value) => {
                self.write_dynamic_one_point(&value.parameter);
                self.writer.write_variable_text(&value.position_name);
                self.writer.write_variable_text(&value.position_description);
                self.writer.write_3bit_double(value.definition_label_point);
            }
            DynamicBlockData::PolarParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer.write_variable_text(&value.angle_name);
                self.writer.write_variable_text(&value.angle_description);
                self.writer.write_variable_text(&value.distance_name);
                self.writer.write_variable_text(&value.distance_description);
                self.writer.write_bit_double(value.offset);
                self.write_dynamic_value_set(&value.angle_value_set);
                self.write_dynamic_value_set(&value.distance_value_set);
            }
            DynamicBlockData::RotationParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer
                    .write_3bit_double(value.definition_base_angle_point);
                self.writer.write_variable_text(&value.angle_name);
                self.writer.write_variable_text(&value.angle_description);
                self.writer.write_bit_double(value.angle);
                self.write_dynamic_value_set(&value.value_set);
            }
            DynamicBlockData::UserParameter(value) => {
                self.write_dynamic_one_point(&value.parameter);
                self.writer.write_bit_short(value.flags);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.associated_variable.value(),
                );
                self.writer.write_variable_text(&value.expression);
                self.writer.write_bit_short(value.value_code);
                self.write_dynamic_eval_value(&value.value);
                self.writer.write_bit_short(value.value_type);
            }
            DynamicBlockData::VisibilityParameter(_) => return,
            DynamicBlockData::XYParameter(value) => {
                self.write_dynamic_two_point(&value.parameter);
                self.writer.write_variable_text(&value.x_label);
                self.writer.write_variable_text(&value.x_label_description);
                self.writer.write_variable_text(&value.y_label);
                self.writer.write_variable_text(&value.y_label_description);
                self.writer.write_bit_double(value.x_value);
                self.writer.write_bit_double(value.y_value);
                self.write_dynamic_value_set(&value.x_value_set);
                self.write_dynamic_value_set(&value.y_value_set);
            }
            DynamicBlockData::AngularConstraintParameter(value) => {
                self.write_dynamic_constraint(&value.constraint);
                self.writer.write_3bit_double(value.center_point);
                self.writer.write_3bit_double(value.end_point);
                self.writer.write_variable_text(&value.expression_name);
                self.writer
                    .write_variable_text(&value.expression_description);
                self.writer.write_bit_double(value.angle);
                self.writer.write_bit(value.orientation_on_both_grips);
                self.write_dynamic_value_set(&value.value_set);
            }
            DynamicBlockData::DiametricConstraintParameter(value)
            | DynamicBlockData::RadialConstraintParameter(value) => {
                self.write_dynamic_constraint(&value.constraint);
                self.writer.write_variable_text(&value.expression_name);
                self.writer
                    .write_variable_text(&value.expression_description);
                self.writer.write_bit_double(value.distance);
                self.write_dynamic_value_set(&value.value_set);
            }
            DynamicBlockData::AlignedConstraintParameter(value)
            | DynamicBlockData::LinearConstraintParameter(value)
            | DynamicBlockData::HorizontalConstraintParameter(value)
            | DynamicBlockData::VerticalConstraintParameter(value) => {
                self.write_dynamic_linear_constraint(value);
            }
            DynamicBlockData::ParameterDependencyBody(value) => {
                self.writer.write_bit_short(value.dependency_body_version);
                self.writer.write_bit_short(value.dimension_base_version);
                self.writer.write_variable_text(&value.name);
                self.writer.write_bit_short(value.class_version);
            }
            DynamicBlockData::MoveAction(value) => {
                self.write_dynamic_action(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
                self.write_dynamic_offsets(&value.offsets);
            }
            DynamicBlockData::FlipAction(value) => {
                self.write_dynamic_action(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
            }
            DynamicBlockData::RotateAction(value) | DynamicBlockData::ScaleAction(value) => {
                self.write_dynamic_action_with_base(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
            }
            DynamicBlockData::ArrayAction(value) => {
                self.write_dynamic_action(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
                self.writer.write_bit_double(value.column_offset);
                self.writer.write_bit_double(value.row_offset);
            }
            DynamicBlockData::LookupAction(value) => {
                self.write_dynamic_action(&value.action);
                self.writer.write_bit_long(value.row_count);
                self.writer.write_bit_long(value.column_count);
                for expression in &value.expressions {
                    self.writer.write_variable_text(expression);
                }
                for row in &value.rows {
                    for connection in &row.connections {
                        self.write_dynamic_connection(connection);
                    }
                    self.writer.write_bit(row.flag_282);
                    self.writer.write_bit(row.flag_281);
                }
                self.writer.write_bit(value.flag_280);
            }
            DynamicBlockData::StretchAction(value) => {
                self.write_dynamic_action(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
                self.writer.write_bit_long(value.points.len() as i32);
                for point in &value.points {
                    self.writer.write_2raw_double(*point);
                }
                self.writer.write_bit_long(value.handles.len() as i32);
                for item in &value.handles {
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, item.handle.value());
                    self.writer.write_bit_short(item.indexes.len() as i16);
                    for index in &item.indexes {
                        self.writer.write_bit_long(*index);
                    }
                }
                self.writer.write_bit_long(value.codes.len() as i32);
                for item in &value.codes {
                    self.writer.write_bit_long(item.code);
                    self.writer.write_bit_short(item.indexes.len() as i16);
                    for index in &item.indexes {
                        self.writer.write_bit_long(*index);
                    }
                }
                self.write_dynamic_offsets(&value.offsets);
            }
            DynamicBlockData::PolarStretchAction(value) => {
                self.write_dynamic_action(&value.action);
                for connection in &value.connections {
                    self.write_dynamic_connection(connection);
                }
                self.writer.write_bit_long(value.points.len() as i32);
                for point in &value.points {
                    self.writer.write_2raw_double(*point);
                }
                self.writer.write_bit_long(value.handles.len() as i32);
                for handle in &value.handles {
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, handle.value());
                }
                for flag in &value.handle_flags {
                    self.writer.write_bit_short(*flag);
                }
                self.writer.write_bit_long(value.codes.len() as i32);
                for code in &value.codes {
                    self.writer.write_bit_long(*code);
                }
            }
            DynamicBlockData::PropertiesTable => {}
            DynamicBlockData::EvaluationGraph(value) => {
                self.writer.write_bit_long(value.first_node_id);
                self.writer.write_bit_long(value.first_node_id_copy);
                self.writer.write_bit_long(value.nodes.len() as i32);
                for node in &value.nodes {
                    self.writer.write_bit_long(node.id);
                    self.writer.write_bit_long(node.edge_flags);
                    self.writer.write_bit_long(node.next_id);
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, node.expression.value());
                    for item in node.node_data {
                        self.writer.write_bit_long(item);
                    }
                    if let Some(active) = node.active_cycles {
                        self.writer.write_bit(active);
                    }
                }
                self.writer.write_bit_long(value.edges.len() as i32);
                for edge in &value.edges {
                    self.writer.write_bit_long(edge.id);
                    self.writer.write_bit_long(edge.next_id);
                    self.writer.write_bit_long(edge.incoming_edge);
                    self.writer.write_bit_long(edge.source_node);
                    self.writer.write_bit_long(edge.destination_node);
                    for item in edge.outgoing_edges {
                        self.writer.write_bit_long(item);
                    }
                }
            }
            DynamicBlockData::AlignmentParameterEntity
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
            | DynamicBlockData::XYGripEntity => {}
            DynamicBlockData::AngularConstraintParameterEntity(_) => {}
            DynamicBlockData::SolidHistory(value) => {
                self.writer.write_bit_long(value.major);
                self.writer.write_bit_long(value.minor);
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, value.owner.value());
                self.writer.write_bit_long(value.history_node_id);
                self.writer.write_bit(value.show_history);
                self.writer.write_bit(value.record_history);
            }
            DynamicBlockData::SolidHistoryNode(value) => {
                self.write_solid_history_operation(value);
            }
        }
        self.register_object(object.handle);
    }
}
