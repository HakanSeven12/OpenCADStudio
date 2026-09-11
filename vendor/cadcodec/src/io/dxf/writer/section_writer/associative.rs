use crate::error::Result;
use crate::io::dxf::writer::stream_writer::{DxfStreamWriter, DxfStreamWriterExt};
use crate::objects::*;
use crate::types::Handle;

use super::SectionWriter;

impl<'a, W: DxfStreamWriter> SectionWriter<'a, W> {
    fn write_assoc_header(&mut self, value: &AssociativeObject) -> Result<()> {
        self.writer.write_string(0, &value.dxf_name)?;
        self.writer.write_handle(5, value.handle)?;
        if !value.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &value.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = value.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, value.owner)?;
        Ok(())
    }

    fn write_assoc_eval(&mut self, value: &AssocEvalVariant) -> Result<()> {
        let code = |fallback: i32| {
            if value.code == 0 {
                fallback
            } else {
                value.code as i32
            }
        };
        match &value.value {
            AssocEvalValue::None => {}
            AssocEvalValue::Real(item) => {
                self.writer.write_double(code(40), *item)?;
            }
            AssocEvalValue::Long(item) => {
                self.writer.write_i32(code(90), *item)?;
            }
            AssocEvalValue::Short(item) => {
                self.writer.write_i16(code(70), *item)?;
            }
            AssocEvalValue::Byte(item) => {
                self.writer.write_byte(code(280), *item)?;
            }
            AssocEvalValue::Text(item) => {
                self.writer.write_string(code(1), item)?;
            }
            AssocEvalValue::Handle(item) => {
                self.writer.write_handle(code(330), *item)?;
            }
        }
        Ok(())
    }

    fn write_assoc_value_param(&mut self, value: &AssocValueParam) -> Result<()> {
        self.writer.write_i32(90, value.class_version)?;
        self.writer.write_string(1, &value.name)?;
        self.writer.write_i32(90, value.unit_type)?;
        self.writer.write_i32(90, value.variables.len() as i32)?;
        for variable in &value.variables {
            self.write_assoc_eval(&variable.value)?;
            self.writer.write_handle(330, variable.handle)?;
        }
        self.writer
            .write_handle(330, value.controlled_object_dependency)?;
        Ok(())
    }

    fn write_assoc_dependency(&mut self, value: &AssocDependency) -> Result<()> {
        self.writer.write_subclass("AcDbAssocDependency")?;
        self.writer.write_i16(90, value.class_version)?;
        self.writer.write_i32(90, value.status)?;
        self.writer.write_bool(290, value.is_read_dependency)?;
        self.writer.write_bool(290, value.is_write_dependency)?;
        self.writer.write_bool(290, value.is_attached_to_object)?;
        self.writer
            .write_bool(290, value.is_delegating_to_owning_action)?;
        self.writer.write_i32(90, value.order)?;
        self.writer.write_handle(330, value.dependent_on)?;
        self.writer.write_bool(290, value.name.is_some())?;
        if let Some(name) = &value.name {
            self.writer.write_string(1, name)?;
        }
        self.writer.write_handle(330, value.read_dependency)?;
        self.writer.write_handle(330, value.node)?;
        self.writer.write_handle(360, value.dependency_body)?;
        self.writer.write_i32(90, value.dependency_body_id)?;
        Ok(())
    }

    fn write_constraint_common(&mut self, node: &AssocConstraintNode) -> Result<()> {
        self.writer.write_i32(90, node.node_id)?;
        if self.dxf_version < crate::types::DxfVersion::AC1027 {
            self.writer.write_byte(70, node.status)?;
        }
        self.writer.write_i32(90, node.connections.len() as i32)?;
        for connection in &node.connections {
            self.writer.write_i32(90, *connection)?;
        }
        if self.dxf_version >= crate::types::DxfVersion::AC1027 {
            self.writer.write_byte(70, node.status)?;
        }
        Ok(())
    }

    fn write_constraint_data(&mut self, value: &AssocConstraintNodeData) -> Result<()> {
        match value {
            AssocConstraintNodeData::None => {}
            AssocConstraintNodeData::Geometrical {
                owner_id,
                is_implied,
                is_active,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
            }
            AssocConstraintNodeData::Angle {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                sector_type,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                self.writer.write_handle(340, *value_dependency)?;
                self.writer.write_handle(340, *dimension_dependency)?;
                self.writer.write_byte(280, *sector_type)?;
            }
            AssocConstraintNodeData::Parallel {
                owner_id,
                is_implied,
                is_active,
                datum_line_index,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                if let Some(datum_line_index) = datum_line_index {
                    self.writer.write_i32(90, *datum_line_index)?;
                }
            }
            AssocConstraintNodeData::Distance {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                direction_type,
                distance,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                self.writer.write_handle(340, *value_dependency)?;
                self.writer.write_handle(340, *dimension_dependency)?;
                self.writer.write_byte(280, *direction_type)?;
                if let Some(distance) = distance {
                    self.writer.write_point3d(10, *distance)?;
                }
            }
            AssocConstraintNodeData::RadiusDiameter {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                mode,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                self.writer.write_handle(340, *value_dependency)?;
                self.writer.write_handle(340, *dimension_dependency)?;
                self.writer.write_byte(280, *mode)?;
            }
            AssocConstraintNodeData::ImplicitPoint {
                geometry_dependency,
                geometry_node_id,
                point,
                point_type,
                point_index,
                curve_id,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                if let Some(point) = point {
                    self.writer.write_point3d(10, *point)?;
                }
                self.writer.write_byte(280, *point_type)?;
                self.writer.write_i32(90, *point_index)?;
                self.writer.write_i32(90, *curve_id)?;
            }
            AssocConstraintNodeData::Point {
                geometry_dependency,
                geometry_node_id,
                point,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                if let Some(point) = point {
                    self.writer.write_point3d(10, *point)?;
                }
            }
            AssocConstraintNodeData::Line {
                geometry_dependency,
                geometry_node_id,
                point,
                direction,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                self.writer.write_point3d(10, *point)?;
                self.writer.write_point3d(10, *direction)?;
            }
            AssocConstraintNodeData::BoundedLine {
                geometry_dependency,
                geometry_node_id,
                point,
                direction,
                is_ray,
                start_point,
                end_point,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                self.writer.write_point3d(10, *point)?;
                self.writer.write_point3d(10, *direction)?;
                self.writer.write_bool(290, *is_ray)?;
                self.writer.write_point3d(10, *start_point)?;
                self.writer.write_point3d(11, *end_point)?;
            }
            AssocConstraintNodeData::Circle {
                geometry_dependency,
                geometry_node_id,
                center,
                normal,
                direction,
                radius,
                start_parameter,
                end_parameter,
                reserved,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                self.writer.write_point3d(10, *center)?;
                self.writer.write_point3d(10, *normal)?;
                self.writer.write_point3d(10, *direction)?;
                self.writer.write_double(40, *radius)?;
                self.writer.write_double(40, *start_parameter)?;
                self.writer.write_double(40, *end_parameter)?;
                self.writer.write_double(40, *reserved)?;
            }
            AssocConstraintNodeData::Arc {
                geometry_dependency,
                geometry_node_id,
                center,
                normal,
                direction,
                radius,
                start_parameter,
                end_parameter,
                reserved,
                start_point,
                end_point,
            } => {
                self.writer.write_handle(330, *geometry_dependency)?;
                self.writer.write_i32(90, *geometry_node_id)?;
                self.writer.write_point3d(10, *center)?;
                self.writer.write_point3d(10, *normal)?;
                self.writer.write_point3d(10, *direction)?;
                self.writer.write_double(40, *radius)?;
                self.writer.write_double(40, *start_parameter)?;
                self.writer.write_double(40, *end_parameter)?;
                self.writer.write_double(40, *reserved)?;
                self.writer.write_point3d(10, *start_point)?;
                self.writer.write_point3d(11, *end_point)?;
            }
            AssocConstraintNodeData::Ellipse {
                owner_id,
                is_implied,
                is_active,
                center,
                short_axis,
                axis_ratio,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                self.writer.write_point3d(10, *center)?;
                self.writer.write_point3d(11, *short_axis)?;
                self.writer.write_double(40, *axis_ratio)?;
            }
            AssocConstraintNodeData::BoundedEllipse {
                owner_id,
                is_implied,
                is_active,
                center,
                short_axis,
                axis_ratio,
                start_point,
                end_point,
            } => {
                self.writer.write_i32(90, *owner_id)?;
                self.writer.write_bool(290, *is_implied)?;
                self.writer.write_bool(290, *is_active)?;
                self.writer.write_point3d(10, *center)?;
                self.writer.write_point3d(11, *short_axis)?;
                self.writer.write_double(40, *axis_ratio)?;
                self.writer.write_point3d(10, *start_point)?;
                self.writer.write_point3d(11, *end_point)?;
            }
        }
        Ok(())
    }

    fn write_assoc_action(&mut self, value: &AssocAction) -> Result<()> {
        self.writer.write_subclass("AcDbAssocAction")?;
        self.writer.write_i16(90, value.class_version)?;
        self.writer.write_i32(90, value.geometry_status)?;
        self.writer.write_handle(330, value.owning_network)?;
        self.writer.write_handle(360, value.action_body)?;
        self.writer.write_i32(90, value.action_index)?;
        self.writer.write_i32(90, value.max_dependency_index)?;
        self.writer.write_i32(90, value.dependencies.len() as i32)?;
        for dependency in &value.dependencies {
            self.writer.write_handle(
                if dependency.is_owned { 360 } else { 330 },
                dependency.dependency,
            )?;
        }
        if value.class_version > 1 {
            self.writer.write_i16(90, 0)?;
            self.writer
                .write_i32(90, value.owned_parameters.len() as i32)?;
            for parameter in &value.owned_parameters {
                self.writer.write_handle(360, *parameter)?;
            }
            self.writer.write_i16(90, 0)?;
            self.writer.write_i32(90, value.values.len() as i32)?;
            for item in &value.values {
                self.write_assoc_value_param(item)?;
            }
        }
        Ok(())
    }

    fn write_assoc_action_param(&mut self, value: &AssocActionParam) -> Result<()> {
        self.writer.write_subclass("AcDbAssocActionParam")?;
        self.writer.write_i16(90, value.is_r2013)?;
        if self.dxf_version >= crate::types::DxfVersion::AC1027 {
            self.writer.write_i32(90, value.version)?;
        }
        self.writer.write_string(1, &value.name)?;
        Ok(())
    }

    fn write_assoc_action_body(&mut self, value: &AssocActionBody) -> Result<()> {
        self.writer.write_subclass("AcDbAssocActionBody")?;
        self.writer.write_i32(90, value.version)
    }

    fn write_assoc_parameter_body(&mut self, value: &AssocParamBasedActionBody) -> Result<()> {
        if value.version == 0
            && value.minor == 0
            && value.dependencies.is_empty()
            && value.values.is_empty()
            && value.dependency.is_null()
        {
            return Ok(());
        }
        self.writer
            .write_subclass("AcDbAssocParamBasedActionBody")?;
        self.writer.write_i32(90, value.version)?;
        self.writer.write_i32(90, value.minor)?;
        self.writer.write_i32(90, value.dependencies.len() as i32)?;
        for dependency in &value.dependencies {
            self.writer.write_handle(360, *dependency)?;
        }
        self.writer.write_i32(90, value.marker)?;
        self.writer.write_i32(90, value.values.len() as i32)?;
        if value.values.is_empty() {
            self.writer.write_i32(90, value.empty_value_marker)?;
            self.writer.write_handle(330, value.dependency)?;
        }
        for item in &value.values {
            self.write_assoc_value_param(item)?;
        }
        Ok(())
    }

    fn write_assoc_surface(&mut self, value: &AssocSurfaceActionBody) -> Result<()> {
        self.write_assoc_action_body(&value.action_body)?;
        self.write_assoc_parameter_body(&value.parameter_body)?;
        self.writer.write_subclass("AcDbAssocSurfaceActionBody")?;
        self.writer.write_i32(90, value.surface_body.version)?;
        self.writer
            .write_handle(330, value.surface_body.dependency)?;
        self.writer
            .write_bool(290, value.surface_body.is_semi_associative)?;
        self.writer.write_i32(90, value.surface_body.marker)?;
        self.writer
            .write_bool(290, value.surface_body.is_semi_override)?;
        self.writer.write_i16(70, value.surface_body.grip_status)?;
        self.writer
            .write_subclass("AcDbAssocPathBasedSurfaceActionBody")?;
        self.writer.write_i32(90, value.path_status)?;
        let section = match value.kind {
            AssocSurfaceActionKind::Plane => "AcDbAssocPlaneSurfaceActionBody",
            AssocSurfaceActionKind::Extend => "AcDbAssocExtendSurfaceActionBody",
            AssocSurfaceActionKind::Extruded => "AcDbAssocExtrudedSurfaceActionBody",
            AssocSurfaceActionKind::Lofted => "AcDbAssocLoftedSurfaceActionBody",
            AssocSurfaceActionKind::Network => "AcDbAssocNetworkSurfaceActionBody",
            AssocSurfaceActionKind::Offset => "AcDbAssocOffsetSurfaceActionBody",
            AssocSurfaceActionKind::Revolved => "AcDbAssocRevolvedSurfaceActionBody",
            AssocSurfaceActionKind::Trim => "AcDbAssocTrimSurfaceActionBody",
            AssocSurfaceActionKind::Blend => "AcDbAssocBlendSurfaceActionBody",
            AssocSurfaceActionKind::Patch => "AcDbAssocPatchSurfaceActionBody",
            AssocSurfaceActionKind::Fillet => "AcDbAssocFilletSurfaceActionBody",
            AssocSurfaceActionKind::Swept => "AcDbAssocSweptSurfaceActionBody",
            AssocSurfaceActionKind::EdgeChamfer => "AcDbAssocEdgeChamferActionBody",
            AssocSurfaceActionKind::EdgeFillet => "AcDbAssocEdgeFilletActionBody",
        };
        self.writer.write_subclass(section)?;
        if !matches!(
            value.kind,
            AssocSurfaceActionKind::EdgeChamfer | AssocSurfaceActionKind::EdgeFillet
        ) {
            self.writer.write_i32(90, value.class_version)?;
        }
        match value.kind {
            AssocSurfaceActionKind::Extend => self.writer.write_byte(280, value.option)?,
            AssocSurfaceActionKind::Offset => self.writer.write_bool(290, value.flags[0])?,
            AssocSurfaceActionKind::Trim => {
                self.writer.write_bool(290, value.flags[0])?;
                self.writer.write_bool(290, value.flags[1])?;
                self.writer.write_double(40, value.distance)?;
            }
            AssocSurfaceActionKind::Blend => {
                self.writer.write_bool(290, value.flags[0])?;
                self.writer.write_bool(291, value.flags[1])?;
                self.writer.write_bool(292, value.flags[2])?;
                self.writer.write_i16(72, value.status)?;
                self.writer.write_bool(293, value.flags[3])?;
                self.writer.write_bool(294, value.flags[4])?;
                self.writer.write_i16(73, value.secondary_status)?;
            }
            AssocSurfaceActionKind::Fillet => {
                self.writer.write_i16(70, value.status)?;
                self.writer.write_point2d(10, value.first_point)?;
                self.writer.write_point2d(10, value.second_point)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn write_assoc_annotation_base(&mut self, value: &AssocAnnotationBase) -> Result<()> {
        self.write_assoc_action_body(&value.action_body)?;
        self.writer.write_i16(90, value.version)?;
        self.writer.write_handle(330, value.dependency)?;
        Ok(())
    }

    fn write_assoc_annotation(&mut self, value: &AssocAnnotationActionBody) -> Result<()> {
        if value.kind == AssocAnnotationKind::RestoreEntityState {
            self.write_assoc_action_body(&value.action_body)?;
            self.writer
                .write_subclass("AcDbAssocRestoreEntityStateActionBody")?;
            self.writer.write_i32(90, value.class_version)?;
            self.writer.write_handle(330, value.entity)?;
            return Ok(());
        }
        self.write_assoc_annotation_base(&value.annotation)?;
        let section = match value.kind {
            AssocAnnotationKind::MLeader => "AcDbAssocMLeaderActionBody",
            AssocAnnotationKind::AlignedDimension => "ACDBASSOCALIGNEDDIMACTIONBODY",
            AssocAnnotationKind::ThreePointAngularDimension => "Assoc3PointAngularDimActionBody",
            AssocAnnotationKind::OrdinateDimension => "AssocOrdinatedDimActionBody",
            AssocAnnotationKind::RotatedDimension => "AssocRotatedDimActionBody",
            AssocAnnotationKind::RestoreEntityState => unreachable!(),
        };
        self.writer.write_subclass(section)?;
        self.writer.write_i32(90, value.class_version)?;
        match value.kind {
            AssocAnnotationKind::MLeader => {
                self.writer.write_i32(90, value.actions.len() as i32)?;
                for action in &value.actions {
                    self.writer.write_i32(90, action.dependency_id)?;
                    self.writer.write_handle(330, action.dependency)?;
                }
            }
            AssocAnnotationKind::AlignedDimension
            | AssocAnnotationKind::OrdinateDimension
            | AssocAnnotationKind::RotatedDimension => {
                self.writer.write_handle(330, value.read_node)?;
                self.writer.write_handle(330, value.dimension_node)?;
            }
            AssocAnnotationKind::ThreePointAngularDimension => {
                self.writer.write_handle(330, value.read_node)?;
                self.writer.write_handle(330, value.dimension_node)?;
                self.writer.write_handle(330, value.dependency)?;
            }
            AssocAnnotationKind::RestoreEntityState => {}
        }
        Ok(())
    }

    fn write_assoc_single_dependency(
        &mut self,
        value: &AssocSingleDependencyActionParam,
        leaf: &str,
    ) -> Result<()> {
        self.write_assoc_action_param(&value.action_param)?;
        self.writer
            .write_subclass("AcDbAssocSingleDependencyActionParam")?;
        self.writer.write_i32(90, value.dependency_class_version)?;
        self.writer.write_handle(330, value.dependency)?;
        self.writer.write_subclass(leaf)?;
        self.writer.write_i32(90, value.class_version)?;
        Ok(())
    }

    fn write_assoc_compound(&mut self, value: &AssocCompoundActionParam) -> Result<()> {
        self.write_assoc_action_param(&value.action_param)?;
        self.writer.write_subclass("AcDbAssocCompoundActionParam")?;
        self.writer.write_i16(90, value.class_version)?;
        self.writer.write_i16(90, value.status)?;
        self.writer.write_i32(90, value.parameters.len() as i32)?;
        for parameter in &value.parameters {
            self.writer.write_handle(360, *parameter)?;
        }
        if let Some(child) = &value.child_parameter {
            self.writer.write_i16(90, child.status)?;
            self.writer.write_i32(90, child.id)?;
            self.writer.write_handle(330, child.parameter)?;
            if child.id != 0 {
                self.writer.write_handle(330, child.secondary_parameter)?;
                self.writer.write_i32(90, child.marker)?;
                self.writer.write_handle(330, child.tertiary_parameter)?;
            }
        }
        Ok(())
    }

    fn write_assoc_array_body(&mut self, value: &AssocArrayActionBody) -> Result<()> {
        self.write_assoc_action_body(&value.action_body)?;
        self.write_assoc_parameter_body(&value.parameter_body)?;
        self.writer.write_subclass("AcDbAssocArrayActionBody")?;
        self.writer.write_i32(90, value.version)?;
        self.writer.write_string(1, &value.parameter_block)?;
        for item in value.transform {
            self.writer.write_double(40, item)?;
        }
        Ok(())
    }

    fn write_assoc_array_parameters(&mut self, value: &AssocArrayParameters) -> Result<()> {
        self.writer
            .write_subclass("AcDbAssocArrayCommonParameters")?;
        self.writer.write_i32(90, value.version)?;
        self.writer.write_i32(90, value.items.len() as i32)?;
        self.writer.write_string(1, &value.class_name)?;
        for item in &value.items {
            self.writer.write_i32(90, item.class_version)?;
            for location in item.location {
                self.writer.write_i32(90, location)?;
            }
            self.writer.write_i32(90, item.flags)?;
            if item.uses_default_transform {
                self.writer.write_point3d(11, item.x_direction)?;
            } else {
                for matrix_value in item.transform {
                    self.writer.write_double(40, matrix_value)?;
                }
            }
            if let Some(matrix) = item.relative_transform {
                for matrix_value in matrix {
                    self.writer.write_double(40, matrix_value)?;
                }
            }
            if let Some(first) = item.first_handle {
                self.writer.write_handle(330, first)?;
            }
            if item.flags & 0x10 != 0 {
                self.writer
                    .write_handle(330, item.second_handle.unwrap_or(Handle::NULL))?;
            }
        }
        self.writer.write_i32(90, value.item_count)?;
        self.writer.write_i32(90, value.row_count)?;
        self.writer.write_i32(90, value.level_count)?;
        Ok(())
    }

    fn write_dimension_association_dxf(&mut self, value: &AssocDimensionAssociation) -> Result<()> {
        self.writer.write_subclass("AcDbDimAssoc")?;
        self.writer.write_handle(330, value.dimension)?;
        self.writer.write_i32(90, value.associativity)?;
        self.writer.write_bool(70, value.trans_space)?;
        self.writer.write_byte(71, value.rotated_type)?;
        for slot in 0..4 {
            if value.associativity & (1 << slot) == 0 {
                continue;
            }
            let fallback = AssocDimensionReference::default();
            let stored = value
                .references
                .get(slot)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let references = if stored.is_empty() {
                std::slice::from_ref(&fallback)
            } else {
                stored
            };
            for (index, reference) in references.iter().enumerate() {
                self.writer.write_string(1, &reference.class_name)?;
                self.writer.write_byte(72, reference.osnap_type)?;
                for xref in &reference.xrefs {
                    self.writer.write_handle(331, *xref)?;
                }
                if reference.osnap_type != 0 {
                    self.writer.write_i32(73, reference.main_subent_type)?;
                    self.writer.write_i32(91, reference.main_gs_marker)?;
                    for path in &reference.xref_paths {
                        self.writer.write_string(301, path)?;
                    }
                }
                self.writer.write_double(40, reference.osnap_distance)?;
                self.writer.write_point3d(10, reference.osnap_point)?;
                if reference.osnap_type == 6 || reference.osnap_type == 11 {
                    for object in &reference.intersection_objects {
                        self.writer.write_handle(332, *object)?;
                    }
                    self.writer
                        .write_i32(74, reference.intersection_subent_type)?;
                    self.writer
                        .write_i32(92, reference.intersection_gs_marker)?;
                    for path in &reference.intersection_xref_paths {
                        self.writer.write_string(302, path)?;
                    }
                }
                self.writer.write_bool(75, index + 1 < references.len())?;
            }
        }
        Ok(())
    }

    fn write_static_pers_subent_manager_dxf(&mut self, value: &PersSubentManager) -> Result<()> {
        self.writer.write_subclass("AcDbPersSubentManager")?;
        self.writer.write_i32(90, value.class_version)?;
        self.writer.write_i32(90, value.marker_zero)?;
        self.writer.write_i32(90, value.marker_two)?;
        self.writer.write_i32(90, value.associative_step_count)?;
        self.writer.write_i32(90, value.associative_subent_count)?;
        self.writer.write_i32(90, value.steps.len() as i32)?;
        for step in &value.steps {
            self.writer.write_i32(90, *step)?;
        }
        if value.associative_subent_count != 0 || !value.subents.is_empty() {
            self.writer.write_i32(90, value.subents.len() as i32)?;
            for subent in &value.subents {
                self.writer.write_i32(90, *subent)?;
            }
        }
        Ok(())
    }

    pub(super) fn write_associative_object_dxf(
        &mut self,
        object: &AssociativeObject,
    ) -> Result<()> {
        self.write_assoc_header(object)?;
        match &object.data {
            AssociativeData::Unknown => {}
            AssociativeData::Dependency(value) => self.write_assoc_dependency(value)?,
            AssociativeData::ValueDependency(value) => {
                self.write_assoc_dependency(&value.dependency)?;
                self.writer.write_subclass("AcDbAssocValueDependency")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_string(1, &value.name)?;
                self.write_assoc_eval(&value.value)?;
            }
            AssociativeData::GeomDependency(value) => {
                self.write_assoc_dependency(&value.dependency)?;
                self.writer.write_subclass("AcDbAssocGeomDependency")?;
                self.writer.write_i16(90, value.class_version)?;
                self.writer.write_bool(290, value.enabled)?;
                self.writer.write_subclass("AcDbAssocPersSubentId")?;
                self.writer
                    .write_string(1, &value.persistent_subent.class_name)?;
                self.writer
                    .write_bool(290, value.persistent_subent.dependent_on_compound_object)?;
            }
            AssociativeData::SurfaceActionBody(value) => self.write_assoc_surface(value)?,
            AssociativeData::Action(value) => self.write_assoc_action(value)?,
            AssociativeData::Network(value) => {
                self.write_assoc_action(&value.action)?;
                self.writer.write_subclass("AcDbAssocNetwork")?;
                self.writer.write_i16(90, value.network_version)?;
                self.writer.write_i32(90, value.network_action_index)?;
                self.writer.write_i32(90, value.actions.len() as i32)?;
                for action in &value.actions {
                    self.writer
                        .write_handle(if action.is_owned { 360 } else { 330 }, action.dependency)?;
                }
                self.writer
                    .write_i32(90, value.owned_actions.len() as i32)?;
                for action in &value.owned_actions {
                    self.writer.write_handle(330, *action)?;
                }
            }
            AssociativeData::AnnotationActionBody(value) => self.write_assoc_annotation(value)?,
            AssociativeData::PersSubentManager(value) => {
                self.writer.write_subclass("AcDbAssocPersSubentManager")?;
                self.writer.write_i32(90, value.class_version)?;
                for marker in value.markers {
                    self.writer.write_i32(90, marker)?;
                }
                self.writer.write_i32(90, value.steps.len() as i32)?;
                for step in &value.steps {
                    self.writer.write_i32(90, *step)?;
                }
                self.writer.write_i32(90, value.subent_count)?;
                for item in &value.subent_data {
                    self.writer.write_i32(90, *item)?;
                }
                self.writer.write_bool(290, value.final_flag)?;
            }
            AssociativeData::EdgeActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocEdgeActionParam",
                )?;
                self.writer.write_handle(330, value.parameter)?;
                self.writer.write_bool(290, value.has_action)?;
                self.writer.write_i32(90, value.action_type)?;
            }
            AssociativeData::ConstraintGroup(value) => {
                self.write_assoc_action(&value.action)?;
                self.writer.write_subclass("AcDbAssoc2dConstraintGroup")?;
                self.writer.write_i32(90, value.version)?;
                self.writer.write_bool(70, value.flag)?;
                for point in value.work_plane {
                    self.writer.write_point3d(10, point)?;
                }
                self.writer.write_handle(360, value.dependency)?;
                self.writer.write_i32(90, value.actions.len() as i32)?;
                for action in &value.actions {
                    self.writer.write_handle(360, *action)?;
                }
                let root = value.nodes.iter().find(|node| node.class_name.is_empty());
                let registered: Vec<&AssocConstraintNode> = value
                    .nodes
                    .iter()
                    .filter(|node| !node.class_name.is_empty())
                    .collect();
                self.writer.write_i32(90, registered.len() as i32 + 1)?;
                if let Some(root) = root {
                    self.writer.write_i32(90, root.node_id)?;
                    self.writer.write_i32(90, root.connections.len() as i32)?;
                    for connection in &root.connections {
                        self.writer.write_i32(90, *connection)?;
                    }
                    self.writer.write_bool(290, root.status != 0)?;
                } else {
                    self.writer.write_i32(90, 0)?;
                    self.writer.write_i32(90, 0)?;
                    self.writer.write_bool(290, false)?;
                }
                self.writer.write_i32(90, registered.len() as i32)?;
                for node in &registered {
                    self.writer.write_string(1, &node.class_name)?;
                    self.writer.write_i32(90, node.node_id)?;
                }
                for node in registered {
                    self.write_constraint_common(node)?;
                    self.write_constraint_data(&node.data)?;
                }
            }
            AssociativeData::Variable(value) => {
                self.write_assoc_action(&value.action)?;
                self.writer.write_subclass("AcDbAssocVariable")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_string(1, &value.name)?;
                self.writer.write_string(1, &value.expression)?;
                self.writer.write_string(1, &value.evaluator)?;
                self.writer.write_string(1, &value.description)?;
                self.write_assoc_eval(&value.value)?;
                self.writer.write_bool(290, value.has_cached_value)?;
                if value.has_cached_value {
                    self.writer.write_string(1, &value.cached_value)?;
                }
                self.writer.write_bool(290, value.flag)?;
                self.writer.write_i32(90, value.reserved)?;
            }
            AssociativeData::ActionParam(value) => self.write_assoc_action_param(value)?,
            AssociativeData::CompoundActionParam(value)
            | AssociativeData::PointRefActionParam(value) => self.write_assoc_compound(value)?,
            AssociativeData::OsnapPointRefActionParam(value) => {
                self.write_assoc_compound(&value.compound)?;
                self.writer
                    .write_subclass("ACDBASSOCOSNAPPOINTREFACTIONPARAM")?;
                self.writer.write_i16(90, value.status)?;
                self.writer.write_byte(90, value.osnap_mode)?;
                self.writer.write_double(40, value.parameter)?;
            }
            AssociativeData::ObjectActionParam(value) => {
                self.write_assoc_single_dependency(value, "AcDbAssocObjectActionParam")?
            }
            AssociativeData::PathActionParam(value) => {
                self.write_assoc_compound(&value.compound)?;
                self.writer.write_subclass("AcDbAssocPathActionParam")?;
                self.writer.write_i32(90, value.version)?;
            }
            AssociativeData::DimDependencyBody(value) => {
                self.writer.write_subclass("AcDbAssocDependencyBody")?;
                self.writer.write_i16(90, value.dependency_body_version)?;
                self.writer
                    .write_subclass("AcDbImpAssocDimDependencyBodyBase")?;
                self.writer.write_i16(90, value.base_version)?;
                self.writer.write_string(1, &value.name)?;
                self.writer.write_subclass("AcDbAssocDimDependencyBody")?;
                self.writer.write_i16(90, value.class_version)?;
            }
            AssociativeData::FaceActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocFaceActionParam",
                )?;
                self.writer.write_i32(90, value.index)?;
            }
            AssociativeData::VertexActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocVertexActionParam",
                )?;
                self.writer.write_point3d(10, value.point)?;
            }
            AssociativeData::AsmBodyActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocAsmbodyActionParam",
                )?;
                self.writer.write_subclass("AcDbModelerGeometry")?;
                self.write_acis_data(&value.acis_data)?;
            }
            AssociativeData::ArrayParameters(value) => {
                self.write_assoc_array_parameters(value)?;
                self.writer.write_subclass(&object.cpp_class_name)?;
            }
            AssociativeData::ArrayActionBody(value) => self.write_assoc_array_body(value)?,
            AssociativeData::ArrayModifyActionBody(value) => {
                self.write_assoc_array_body(&value.body)?;
                self.writer
                    .write_subclass("AcDbAssocArrayModifyActionBody")?;
                self.writer.write_i16(70, value.status)?;
                self.writer
                    .write_i32(90, value.item_locations.len() as i32)?;
                for location in &value.item_locations {
                    for item in location {
                        self.writer.write_i32(90, *item)?;
                    }
                }
            }
            AssociativeData::DimensionAssociation(value) => {
                self.write_dimension_association_dxf(value)?
            }
            AssociativeData::PersSubentManagerStatic(value) => {
                self.write_static_pers_subent_manager_dxf(value)?
            }
            AssociativeData::ViewRepActionBody(value) => {
                self.write_assoc_action_body(&value.action_body)?;
                self.writer.write_subclass("AcDbAssocViewRepActionBody")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_handle(360, value.view_rep)?;
                self.writer.write_i32(90, value.view_type)?;
                self.writer.write_double(40, value.rotation)?;
            }
            AssociativeData::ViewObjectActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocObjectActionParam",
                )?;
                let section = match value.kind {
                    AssocViewObjectActionParamKind::ViewBorder => "AcDbAssocViewBorderActionParam",
                    AssocViewObjectActionParamKind::ViewRep => "AcDbAssocViewRepActionParam",
                    AssocViewObjectActionParamKind::ViewSymbol => "AcDbAssocViewSymbolActionParam",
                    AssocViewObjectActionParamKind::ViewStyle => "AcDbAssocViewStyleActionParam",
                };
                self.writer.write_subclass(section)?;
                self.writer.write_i16(70, value.class_version)?;
            }
            AssociativeData::ViewRepHatchManager(value) => {
                self.write_assoc_compound(&value.compound)?;
                self.writer.write_subclass("AcDbAssocViewRepHatchManager")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_i32(90, value.items.len() as i32)?;
                for item in &value.items {
                    self.writer.write_i64(160, item.first_id)?;
                    self.writer.write_i64(160, item.second_id)?;
                    self.writer.write_i32(90, item.status)?;
                    self.writer.write_handle(330, item.parameter)?;
                }
            }
            AssociativeData::ViewRepHatchActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocObjectActionParam",
                )?;
                self.writer
                    .write_subclass("AcDbAssocViewRepHatchActionParam")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_point3d(210, value.normal)?;
                self.writer.write_i32(90, value.hatch_index)?;
                self.writer.write_i32(90, value.flags)?;
            }
            AssociativeData::ViewLabelActionParam(value) => {
                self.write_assoc_single_dependency(
                    &value.single_dependency,
                    "AcDbAssocObjectActionParam",
                )?;
                self.writer
                    .write_subclass("AcDbAssocViewLabelActionParam")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_i16(70, value.label_version)?;
                self.writer.write_double(210, value.offset.x)?;
                self.writer.write_double(220, value.offset.y)?;
                self.writer.write_byte(280, value.flag)?;
            }
        }
        Ok(())
    }
}
