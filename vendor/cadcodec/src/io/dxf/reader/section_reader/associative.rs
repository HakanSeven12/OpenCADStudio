use std::collections::HashMap;

use crate::entities::{AcisData, AcisVersion};
use crate::error::Result;
use crate::objects::*;
use crate::types::{Handle, Vector2, Vector3};

use super::{parse_dxf_handle, SectionReader};

#[derive(Default)]
struct AssocDxfRecord {
    handle: Handle,
    owner: Handle,
    reactors: Vec<Handle>,
    xdictionary_handle: Option<Handle>,
    sections: HashMap<String, Vec<(i32, String)>>,
}

impl AssocDxfRecord {
    fn values(&self, section: &str, code: i32) -> Vec<&str> {
        self.sections
            .get(section)
            .into_iter()
            .flatten()
            .filter(|(item_code, _)| *item_code == code)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn i32(&self, section: &str, code: i32, index: usize) -> i32 {
        self.values(section, code)
            .get(index)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_default()
    }

    fn i16(&self, section: &str, code: i32, index: usize) -> i16 {
        self.i32(section, code, index) as i16
    }

    fn byte(&self, section: &str, code: i32, index: usize) -> u8 {
        self.i32(section, code, index) as u8
    }

    fn bool(&self, section: &str, code: i32, index: usize) -> bool {
        self.i32(section, code, index) != 0
    }

    fn f64(&self, section: &str, code: i32, index: usize) -> f64 {
        self.values(section, code)
            .get(index)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_default()
    }

    fn text(&self, section: &str, code: i32, index: usize) -> String {
        self.values(section, code)
            .get(index)
            .copied()
            .unwrap_or_default()
            .to_string()
    }

    fn handle(&self, section: &str, code: i32, index: usize) -> Handle {
        self.values(section, code)
            .get(index)
            .map(|value| parse_dxf_handle(value))
            .unwrap_or(Handle::NULL)
    }

    fn point(&self, section: &str, code: i32, index: usize) -> Vector3 {
        Vector3::new(
            self.f64(section, code, index),
            self.f64(section, code + 10, index),
            self.f64(section, code + 20, index),
        )
    }
}

struct AssocCursor<'a> {
    entries: &'a [(i32, String)],
    position: usize,
}

impl<'a> AssocCursor<'a> {
    fn new(record: &'a AssocDxfRecord, section: &str) -> Self {
        Self {
            entries: record
                .sections
                .get(section)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            position: 0,
        }
    }

    fn next(&mut self, code: i32) -> Option<&'a str> {
        while let Some((item_code, value)) = self.entries.get(self.position) {
            self.position += 1;
            if *item_code == code {
                return Some(value);
            }
        }
        None
    }

    fn i32(&mut self, code: i32) -> i32 {
        self.next(code)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_default()
    }

    fn i64(&mut self, code: i32) -> i64 {
        self.next(code)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_default()
    }

    fn i16(&mut self, code: i32) -> i16 {
        self.i32(code) as i16
    }

    fn bool(&mut self, code: i32) -> bool {
        self.i32(code) != 0
    }

    fn f64(&mut self, code: i32) -> f64 {
        self.next(code)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_default()
    }

    fn text(&mut self, code: i32) -> String {
        self.next(code).unwrap_or_default().to_string()
    }

    fn handle(&mut self, code: i32) -> Handle {
        self.next(code)
            .map(parse_dxf_handle)
            .unwrap_or(Handle::NULL)
    }

    fn point3(&mut self, code: i32) -> Vector3 {
        Vector3::new(self.f64(code), self.f64(code + 10), self.f64(code + 20))
    }

    fn peek_code(&self) -> Option<i32> {
        self.entries.get(self.position).map(|entry| entry.0)
    }

    fn consecutive_handles(&mut self, code: i32) -> Vec<Handle> {
        let mut values = Vec::new();
        while self.peek_code() == Some(code) {
            values.push(self.handle(code));
        }
        values
    }

    fn consecutive_text(&mut self, code: i32) -> Vec<String> {
        let mut values = Vec::new();
        while self.peek_code() == Some(code) {
            values.push(self.text(code));
        }
        values
    }
}

fn read_eval(cursor: &mut AssocCursor<'_>) -> AssocEvalVariant {
    let code = if cursor.peek_code() == Some(70) {
        let marker = cursor
            .entries
            .get(cursor.position)
            .and_then(|entry| entry.1.trim().parse::<i16>().ok());
        let next_code = cursor
            .entries
            .get(cursor.position + 1)
            .map(|entry| entry.0 as i16);
        if marker.is_some() && marker == next_code {
            cursor.i16(70)
        } else {
            70
        }
    } else {
        cursor.peek_code().unwrap_or_default() as i16
    };
    let value = match code {
        i16::MIN..=-1 | 5 | 105 | 320..=329 | 390..=399 => {
            AssocEvalValue::Handle(cursor.handle(code as i32))
        }
        0..=9 | 100..=102 | 300..=309 | 410..=419 | 430..=439 | 470..=479 | 999 | 1000..=1009 => {
            AssocEvalValue::Text(cursor.text(code as i32))
        }
        38..=59 | 140..=149 | 460..=469 | 1040..=1042 => {
            AssocEvalValue::Real(cursor.f64(code as i32))
        }
        60..=79 | 170..=179 | 270..=279 | 370..=389 | 400..=409 | 1070 => {
            AssocEvalValue::Short(cursor.i16(code as i32))
        }
        80..=99 | 420..=429 | 440..=459 | 1071 => AssocEvalValue::Long(cursor.i32(code as i32)),
        280..=289 => AssocEvalValue::Byte(cursor.i32(code as i32) as u8),
        _ => AssocEvalValue::None,
    };
    AssocEvalVariant { code, value }
}

fn read_value_param(cursor: &mut AssocCursor<'_>) -> AssocValueParam {
    let class_version = cursor.i32(90);
    let name = cursor.text(1);
    let unit_type = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let mut variables = Vec::with_capacity(count as usize);
    for _ in 0..count {
        variables.push(AssocValueParamVariable {
            value: read_eval(cursor),
            handle: cursor.handle(330),
        });
    }
    AssocValueParam {
        class_version,
        name,
        unit_type,
        variables,
        controlled_object_dependency: cursor.handle(330),
    }
}

fn read_dependency(record: &AssocDxfRecord) -> AssocDependency {
    let mut cursor = AssocCursor::new(record, "AcDbAssocDependency");
    let class_version = cursor.i16(90);
    let status = cursor.i32(90);
    let is_read_dependency = cursor.bool(290);
    let is_write_dependency = cursor.bool(290);
    let is_attached_to_object = cursor.bool(290);
    let is_delegating_to_owning_action = cursor.bool(290);
    let order = cursor.i32(90);
    let dependent_on = cursor.handle(330);
    let has_name = cursor.bool(290);
    let name = has_name.then(|| cursor.text(1));
    AssocDependency {
        class_version,
        status,
        is_read_dependency,
        is_write_dependency,
        is_attached_to_object,
        is_delegating_to_owning_action,
        order,
        dependent_on,
        name,
        read_dependency: cursor.handle(330),
        node: cursor.handle(330),
        dependency_body: cursor.handle(360),
        dependency_body_id: cursor.i32(90),
    }
}

fn read_constraint_common(
    cursor: &mut AssocCursor<'_>,
    dxf_version: crate::types::DxfVersion,
) -> AssocConstraintNode {
    let node_id = cursor.i32(90);
    let status_before = dxf_version < crate::types::DxfVersion::AC1027;
    let mut status = if status_before {
        cursor.i32(70) as u8
    } else {
        0
    };
    let connection_count = cursor.i32(90).max(0).min(100_000);
    let mut connections = Vec::with_capacity(connection_count as usize);
    for _ in 0..connection_count {
        connections.push(cursor.i32(90));
    }
    if !status_before {
        status = cursor.i32(70) as u8;
    }
    AssocConstraintNode {
        node_id,
        status,
        connections,
        class_name: String::new(),
        registry_flag: false,
        data: AssocConstraintNodeData::None,
    }
}

fn read_constraint_root(cursor: &mut AssocCursor<'_>) -> AssocConstraintNode {
    let node_id = cursor.i32(90);
    let connection_count = cursor.i32(90).max(0).min(100_000);
    let mut connections = Vec::with_capacity(connection_count as usize);
    for _ in 0..connection_count {
        connections.push(cursor.i32(90));
    }
    AssocConstraintNode {
        node_id,
        status: cursor.i32(290) as u8,
        connections,
        class_name: String::new(),
        registry_flag: false,
        data: AssocConstraintNodeData::None,
    }
}

fn read_constraint_geometry(cursor: &mut AssocCursor<'_>) -> (i32, bool, bool) {
    (cursor.i32(90), cursor.bool(290), cursor.bool(290))
}

fn read_explicit_constraint(cursor: &mut AssocCursor<'_>) -> (i32, bool, bool, Handle, Handle) {
    let (owner_id, is_implied, is_active) = read_constraint_geometry(cursor);
    (
        owner_id,
        is_implied,
        is_active,
        cursor.handle(340),
        cursor.handle(340),
    )
}

fn is_dxf_plain_geometrical_constraint(class_name: &str) -> bool {
    matches!(
        class_name.to_ascii_uppercase().as_str(),
        "ACCENTERPOINTCONSTRAINT"
            | "ACCOLINEARCONSTRAINT"
            | "ACCONCENTRICCONSTRAINT"
            | "ACEQUALCURVATURECONSTRAINT"
            | "ACEQUALDISTANCECONSTRAINT"
            | "ACEQUALHELPPARAMETERCONSTRAINT"
            | "ACEQUALLENGTHCONSTRAINT"
            | "ACEQUALRADIUSCONSTRAINT"
            | "ACFIXEDCONSTRAINT"
            | "ACHORIZONTALCONSTRAINT"
            | "ACMIDPOINTCONSTRAINT"
            | "ACNORMALCONSTRAINT"
            | "ACPERPENDICULARCONSTRAINT"
            | "ACPOINTCOINCIDENCECONSTRAINT"
            | "ACPOINTCURVECONSTRAINT"
            | "ACSYMMETRICCONSTRAINT"
            | "ACTANGENTCONSTRAINT"
            | "ACVERTICALCONSTRAINT"
    )
}

fn read_constraint_data(cursor: &mut AssocCursor<'_>, class_name: &str) -> AssocConstraintNodeData {
    match class_name.to_ascii_uppercase().as_str() {
        "ACCONSTRAINEDCIRCLE" => AssocConstraintNodeData::Circle {
            geometry_dependency: cursor.handle(330),
            geometry_node_id: cursor.i32(90),
            center: cursor.point3(10),
            normal: cursor.point3(10),
            direction: cursor.point3(10),
            radius: cursor.f64(40),
            start_parameter: cursor.f64(40),
            end_parameter: cursor.f64(40),
            reserved: cursor.f64(40),
        },
        "ACCONSTRAINEDARC" => AssocConstraintNodeData::Arc {
            geometry_dependency: cursor.handle(330),
            geometry_node_id: cursor.i32(90),
            center: cursor.point3(10),
            normal: cursor.point3(10),
            direction: cursor.point3(10),
            radius: cursor.f64(40),
            start_parameter: cursor.f64(40),
            end_parameter: cursor.f64(40),
            reserved: cursor.f64(40),
            start_point: cursor.point3(10),
            end_point: cursor.point3(11),
        },
        "ACCONSTRAINEDIMPLICITPOINT" => {
            let geometry_dependency = cursor.handle(330);
            let geometry_node_id = cursor.i32(90);
            let point = if !geometry_dependency.is_null() && cursor.peek_code() == Some(10) {
                Some(cursor.point3(10))
            } else {
                None
            };
            AssocConstraintNodeData::ImplicitPoint {
                geometry_dependency,
                geometry_node_id,
                point,
                point_type: cursor.i32(280) as u8,
                point_index: cursor.i32(90),
                curve_id: cursor.i32(90),
            }
        }
        "ACCONSTRAINEDPOINT" => {
            let geometry_dependency = cursor.handle(330);
            let geometry_node_id = cursor.i32(90);
            let point = if !geometry_dependency.is_null() && cursor.peek_code() == Some(10) {
                Some(cursor.point3(10))
            } else {
                None
            };
            AssocConstraintNodeData::Point {
                geometry_dependency,
                geometry_node_id,
                point,
            }
        }
        "ACCONSTRAINEDLINE"
        | "ACCONSTRAINEDCONSTRUCTIONLINE"
        | "ACCONSTRAINED2POINTSCONSTRUCTIONLINE"
        | "ACCONSTRAINEDDATUMLINE" => AssocConstraintNodeData::Line {
            geometry_dependency: cursor.handle(330),
            geometry_node_id: cursor.i32(90),
            point: cursor.point3(10),
            direction: cursor.point3(10),
        },
        "ACCONSTRAINEDBOUNDEDLINE" => AssocConstraintNodeData::BoundedLine {
            geometry_dependency: cursor.handle(330),
            geometry_node_id: cursor.i32(90),
            point: cursor.point3(10),
            direction: cursor.point3(10),
            is_ray: cursor.bool(290),
            start_point: cursor.point3(10),
            end_point: cursor.point3(11),
        },
        "ACANGLECONSTRAINT" | "AC3POINTANGLECONSTRAINT" => {
            let (owner_id, is_implied, is_active, value_dependency, dimension_dependency) =
                read_explicit_constraint(cursor);
            AssocConstraintNodeData::Angle {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                sector_type: cursor.i32(280) as u8,
            }
        }
        "ACPARALLELCONSTRAINT" => {
            let (owner_id, is_implied, is_active) = read_constraint_geometry(cursor);
            AssocConstraintNodeData::Parallel {
                owner_id,
                is_implied,
                is_active,
                datum_line_index: Some(cursor.i32(90)),
            }
        }
        "ACDISTANCECONSTRAINT" => {
            let (owner_id, is_implied, is_active, value_dependency, dimension_dependency) =
                read_explicit_constraint(cursor);
            let direction_type = cursor.i32(280) as u8;
            AssocConstraintNodeData::Distance {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                direction_type,
                distance: (direction_type != 0).then(|| cursor.point3(10)),
            }
        }
        "ACRADIUSDIAMETERCONSTRAINT" => {
            let (owner_id, is_implied, is_active, value_dependency, dimension_dependency) =
                read_explicit_constraint(cursor);
            AssocConstraintNodeData::RadiusDiameter {
                owner_id,
                is_implied,
                is_active,
                value_dependency,
                dimension_dependency,
                mode: cursor.i32(280) as u8,
            }
        }
        "ACCONSTRAINEDELLIPSE" => {
            let (owner_id, is_implied, is_active) = read_constraint_geometry(cursor);
            AssocConstraintNodeData::Ellipse {
                owner_id,
                is_implied,
                is_active,
                center: cursor.point3(10),
                short_axis: cursor.point3(11),
                axis_ratio: cursor.f64(40),
            }
        }
        "ACCONSTRAINEDBOUNDEDELLIPSE" => {
            let (owner_id, is_implied, is_active) = read_constraint_geometry(cursor);
            AssocConstraintNodeData::BoundedEllipse {
                owner_id,
                is_implied,
                is_active,
                center: cursor.point3(10),
                short_axis: cursor.point3(11),
                axis_ratio: cursor.f64(40),
                start_point: cursor.point3(10),
                end_point: cursor.point3(11),
            }
        }
        _ if is_dxf_plain_geometrical_constraint(class_name) => {
            let (owner_id, is_implied, is_active) = read_constraint_geometry(cursor);
            AssocConstraintNodeData::Geometrical {
                owner_id,
                is_implied,
                is_active,
            }
        }
        _ => AssocConstraintNodeData::None,
    }
}

fn read_action(record: &AssocDxfRecord) -> AssocAction {
    let mut cursor = AssocCursor::new(record, "AcDbAssocAction");
    let class_version = cursor.i16(90);
    let geometry_status = cursor.i32(90);
    let owning_network = cursor.handle(330);
    let action_body = cursor.handle(360);
    let action_index = cursor.i32(90);
    let max_dependency_index = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let mut dependencies = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut dependency = Handle::NULL;
        let mut is_owned = false;
        while let Some((code, value)) = cursor.entries.get(cursor.position) {
            cursor.position += 1;
            if *code == 330 || *code == 360 {
                dependency = parse_dxf_handle(value);
                is_owned = *code == 360;
                break;
            }
        }
        dependencies.push(AssocActionDependency {
            is_owned,
            dependency,
        });
    }
    let mut owned_parameters = Vec::new();
    let mut values = Vec::new();
    if class_version > 1 {
        let _zero = cursor.i16(90);
        let count = cursor.i32(90).max(0).min(100_000);
        owned_parameters.reserve(count as usize);
        for _ in 0..count {
            owned_parameters.push(cursor.handle(360));
        }
        let _zero = cursor.i16(90);
        let count = cursor.i32(90).max(0).min(100_000);
        values.reserve(count as usize);
        for _ in 0..count {
            values.push(read_value_param(&mut cursor));
        }
    }
    AssocAction {
        class_version,
        geometry_status,
        owning_network,
        action_body,
        action_index,
        max_dependency_index,
        dependencies,
        owned_parameters,
        values,
    }
}

fn skip_action(cursor: &mut AssocCursor<'_>) {
    let class_version = cursor.i16(90);
    let _geometry_status = cursor.i32(90);
    let _owning_network = cursor.handle(330);
    let _action_body = cursor.handle(360);
    let _action_index = cursor.i32(90);
    let _max_dependency_index = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    for _ in 0..count {
        if cursor.peek_code() == Some(360) {
            let _dependency = cursor.handle(360);
        } else {
            let _dependency = cursor.handle(330);
        }
    }
    if class_version > 1 {
        let _zero = cursor.i16(90);
        let owned_count = cursor.i32(90).max(0).min(100_000);
        for _ in 0..owned_count {
            let _parameter = cursor.handle(360);
        }
        let _zero = cursor.i16(90);
        let value_count = cursor.i32(90).max(0).min(100_000);
        for _ in 0..value_count {
            let _value = read_value_param(cursor);
        }
    }
}

fn read_action_param(record: &AssocDxfRecord) -> AssocActionParam {
    let mut cursor = AssocCursor::new(record, "AcDbAssocActionParam");
    let is_r2013 = cursor.i16(90);
    let version = if cursor
        .entries
        .get(cursor.position)
        .map(|entry| entry.0 == 90)
        .unwrap_or(false)
    {
        cursor.i32(90)
    } else {
        0
    };
    AssocActionParam {
        is_r2013,
        version,
        name: cursor.text(1),
    }
}

fn read_parameter_body(record: &AssocDxfRecord) -> AssocParamBasedActionBody {
    let mut cursor = AssocCursor::new(record, "AcDbAssocParamBasedActionBody");
    let version = cursor.i32(90);
    let minor = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let mut dependencies = Vec::with_capacity(count as usize);
    for _ in 0..count {
        dependencies.push(cursor.handle(360));
    }
    let marker = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let (empty_value_marker, dependency) = if count == 0 {
        (cursor.i32(90), cursor.handle(330))
    } else {
        (0, Handle::NULL)
    };
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        values.push(read_value_param(&mut cursor));
    }
    AssocParamBasedActionBody {
        version,
        minor,
        dependencies,
        marker,
        values,
        empty_value_marker,
        dependency,
    }
}

fn read_compound(record: &AssocDxfRecord) -> AssocCompoundActionParam {
    let mut cursor = AssocCursor::new(record, "AcDbAssocCompoundActionParam");
    let class_version = cursor.i16(90);
    let status = cursor.i16(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let mut parameters = Vec::with_capacity(count as usize);
    for _ in 0..count {
        parameters.push(cursor.handle(360));
    }
    let child_parameter = if cursor.position < cursor.entries.len() {
        let child_status = cursor.i16(90);
        let id = cursor.i32(90);
        let parameter = cursor.handle(330);
        let (secondary_parameter, marker, tertiary_parameter) = if id != 0 {
            (cursor.handle(330), cursor.i32(90), cursor.handle(330))
        } else {
            (Handle::NULL, 0, Handle::NULL)
        };
        Some(AssocChildParameter {
            status: child_status,
            id,
            parameter,
            secondary_parameter,
            marker,
            tertiary_parameter,
        })
    } else {
        None
    };
    AssocCompoundActionParam {
        action_param: read_action_param(record),
        class_version,
        status,
        parameters,
        child_parameter,
    }
}

fn surface_kind(name: &str) -> Option<AssocSurfaceActionKind> {
    Some(match name {
        "ASSOCPLANESURFACEACTIONBODY" => AssocSurfaceActionKind::Plane,
        "ASSOCEXTENDSURFACEACTIONBODY" => AssocSurfaceActionKind::Extend,
        "ASSOCEXTRUDEDSURFACEACTIONBODY" => AssocSurfaceActionKind::Extruded,
        "ASSOCLOFTEDSURFACEACTIONBODY" => AssocSurfaceActionKind::Lofted,
        "ASSOCNETWORKSURFACEACTIONBODY" => AssocSurfaceActionKind::Network,
        "ASSOCOFFSETSURFACEACTIONBODY" => AssocSurfaceActionKind::Offset,
        "ASSOCREVOLVEDSURFACEACTIONBODY" => AssocSurfaceActionKind::Revolved,
        "ASSOCTRIMSURFACEACTIONBODY" => AssocSurfaceActionKind::Trim,
        "ASSOCBLENDSURFACEACTIONBODY" => AssocSurfaceActionKind::Blend,
        "ASSOCPATCHSURFACEACTIONBODY" => AssocSurfaceActionKind::Patch,
        "ASSOCFILLETSURFACEACTIONBODY" => AssocSurfaceActionKind::Fillet,
        "ASSOCSWEPTSURFACEACTIONBODY" => AssocSurfaceActionKind::Swept,
        "ASSOCEDGECHAMFERACTIONBODY" => AssocSurfaceActionKind::EdgeChamfer,
        "ASSOCEDGEFILLETACTIONBODY" => AssocSurfaceActionKind::EdgeFillet,
        _ => return None,
    })
}

fn surface_section(kind: AssocSurfaceActionKind) -> &'static str {
    match kind {
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
    }
}

fn read_surface(record: &AssocDxfRecord, kind: AssocSurfaceActionKind) -> AssocSurfaceActionBody {
    let action_body = AssocActionBody {
        version: record.i32("AcDbAssocActionBody", 90, 0),
    };
    let parameter_body = read_parameter_body(record);
    let surface_body = AssocSurfaceBody {
        version: record.i32("AcDbAssocSurfaceActionBody", 90, 0),
        dependency: record.handle("AcDbAssocSurfaceActionBody", 330, 0),
        is_semi_associative: record.bool("AcDbAssocSurfaceActionBody", 290, 0),
        marker: record.i32("AcDbAssocSurfaceActionBody", 90, 1),
        is_semi_override: record.bool("AcDbAssocSurfaceActionBody", 290, 1),
        grip_status: record.i16("AcDbAssocSurfaceActionBody", 70, 0),
    };
    let section = surface_section(kind);
    let mut value = AssocSurfaceActionBody {
        kind,
        action_body,
        parameter_body,
        surface_body,
        path_status: record.i32("AcDbAssocPathBasedSurfaceActionBody", 90, 0),
        class_version: record.i32(section, 90, 0),
        ..AssocSurfaceActionBody::default()
    };
    match kind {
        AssocSurfaceActionKind::Extend => value.option = record.byte(section, 280, 0),
        AssocSurfaceActionKind::Offset => value.flags[0] = record.bool(section, 290, 0),
        AssocSurfaceActionKind::Trim => {
            value.flags[0] = record.bool(section, 290, 0);
            value.flags[1] = record.bool(section, 290, 1);
            value.distance = record.f64(section, 40, 0);
        }
        AssocSurfaceActionKind::Blend => {
            for index in 0..5 {
                value.flags[index] = record.bool(section, 290 + index as i32, 0);
            }
            value.status = record.i16(section, 72, 0);
            value.secondary_status = record.i16(section, 73, 0);
        }
        AssocSurfaceActionKind::Fillet => {
            value.status = record.i16(section, 70, 0);
            value.first_point =
                Vector2::new(record.f64(section, 10, 0), record.f64(section, 20, 0));
            value.second_point =
                Vector2::new(record.f64(section, 10, 1), record.f64(section, 20, 1));
        }
        _ => {}
    }
    value
}

fn read_array_parameters(record: &AssocDxfRecord) -> AssocArrayParameters {
    let section = "AcDbAssocArrayCommonParameters";
    let mut cursor = AssocCursor::new(record, section);
    let version = cursor.i32(90);
    let count = cursor.i32(90).max(0).min(100_000);
    let class_name = cursor.text(1);
    let mut items = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let class_version = cursor.i32(90);
        let location = [cursor.i32(90), cursor.i32(90), cursor.i32(90)];
        let flags = cursor.i32(90);
        let uses_default_transform = cursor
            .entries
            .get(cursor.position)
            .map(|entry| entry.0 == 11)
            .unwrap_or(false);
        let mut x_direction = Vector3::ZERO;
        let mut transform = [0.0; 16];
        if uses_default_transform {
            x_direction.x = cursor.f64(11);
            x_direction.y = cursor.f64(21);
            x_direction.z = cursor.f64(31);
        } else {
            for value in &mut transform {
                *value = cursor.f64(40);
            }
        }
        let relative_transform = if flags & 2 != 0 {
            let mut matrix = [0.0; 16];
            for value in &mut matrix {
                *value = cursor.f64(40);
            }
            Some(matrix)
        } else {
            None
        };
        let consecutive_handles = cursor.entries[cursor.position..]
            .iter()
            .take_while(|entry| entry.0 == 330)
            .count();
        let first_handle = if consecutive_handles > usize::from(flags & 0x10 != 0) {
            Some(cursor.handle(330))
        } else {
            None
        };
        let second_handle = (flags & 0x10 != 0).then(|| cursor.handle(330));
        items.push(AssocArrayItem {
            class_version,
            location,
            flags,
            uses_default_transform,
            x_direction,
            transform,
            relative_transform,
            first_handle,
            second_handle,
        });
    }
    AssocArrayParameters {
        version,
        class_name,
        items,
        item_count: cursor.i32(90),
        row_count: cursor.i32(90),
        level_count: cursor.i32(90),
    }
}

fn read_dimension_association_dxf(record: &AssocDxfRecord) -> AssocDimensionAssociation {
    // AutoCAD writes the dimension handle before the scalar DIMASSOC fields,
    // while dwg2.spec lists it after them. Read it independently so both
    // orderings decode without advancing the positional reference cursor.
    let dimension = record.handle("AcDbDimAssoc", 330, 0);
    let mut cursor = AssocCursor::new(record, "AcDbDimAssoc");
    let associativity = cursor.i32(90);
    let trans_space = cursor.bool(70);
    let rotated_type = cursor.i32(71) as u8;
    let mut references: [Vec<AssocDimensionReference>; 4] = std::array::from_fn(|_| Vec::new());
    let mut total_references = 0usize;
    'slots: for slot in 0..4 {
        if associativity & (1 << slot) == 0 {
            continue;
        }
        loop {
            if total_references >= 6 || cursor.peek_code() != Some(1) {
                break 'slots;
            }
            let class_name = cursor.text(1);
            let osnap_type = cursor.i32(72) as u8;
            let xrefs = cursor.consecutive_handles(331);
            let (main_subent_type, main_gs_marker, xref_paths) = if osnap_type != 0 {
                (cursor.i32(73), cursor.i32(91), cursor.consecutive_text(301))
            } else {
                (0, 0, Vec::new())
            };
            let osnap_distance = cursor.f64(40);
            let osnap_point = Vector3::new(cursor.f64(10), cursor.f64(20), cursor.f64(30));
            let (
                intersection_objects,
                intersection_subent_type,
                intersection_gs_marker,
                intersection_xref_paths,
            ) = if osnap_type == 6 || osnap_type == 11 {
                (
                    cursor.consecutive_handles(332),
                    cursor.i32(74),
                    cursor.i32(92),
                    cursor.consecutive_text(302),
                )
            } else {
                (Vec::new(), 0, 0, Vec::new())
            };
            let has_last_point_reference = cursor.bool(75);
            references[slot].push(AssocDimensionReference {
                class_name,
                osnap_type,
                xrefs,
                main_subent_type,
                main_gs_marker,
                xref_paths,
                osnap_distance,
                osnap_point,
                intersection_objects,
                intersection_subent_type,
                intersection_gs_marker,
                intersection_xref_paths,
                has_last_point_reference,
            });
            total_references += 1;
            if !has_last_point_reference {
                break;
            }
        }
    }
    AssocDimensionAssociation {
        associativity,
        trans_space,
        rotated_type,
        dimension,
        references,
    }
}

fn read_static_pers_subent_manager_dxf(record: &AssocDxfRecord) -> PersSubentManager {
    let mut cursor = AssocCursor::new(record, "AcDbPersSubentManager");
    let class_version = cursor.i32(90);
    let marker_zero = cursor.i32(90);
    let marker_two = cursor.i32(90);
    let associative_step_count = cursor.i32(90);
    let associative_subent_count = cursor.i32(90);
    let step_count = cursor.i32(90).max(0).min(100_000) as usize;
    let mut steps = Vec::with_capacity(step_count);
    for _ in 0..step_count {
        steps.push(cursor.i32(90));
    }
    let subent_count = cursor.i32(90).max(0).min(100_000) as usize;
    let mut subents = Vec::with_capacity(subent_count);
    for _ in 0..subent_count {
        subents.push(cursor.i32(90));
    }
    PersSubentManager {
        class_version,
        marker_zero,
        marker_two,
        associative_step_count,
        associative_subent_count,
        steps,
        subents,
    }
}

impl<'a> SectionReader<'a> {
    pub(super) fn read_associative_object_dxf(
        &mut self,
        dxf_name: &str,
        dxf_version: crate::types::DxfVersion,
    ) -> Result<AssociativeObject> {
        let mut record = AssocDxfRecord::default();
        let mut section = String::new();
        let mut group = String::new();
        let mut owner_seen = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => record.handle = parse_dxf_handle(&pair.value_string),
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    record.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    record.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() && section.is_empty() => {
                    record.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                100 => section = pair.value_string.clone(),
                _ => record
                    .sections
                    .entry(section.clone())
                    .or_default()
                    .push((pair.code, pair.value_string)),
            }
            if group == "}" {
                group.clear();
            }
        }
        let canonical = associative_canonical_name(dxf_name);
        let data = match canonical.as_str() {
            "ASSOCDEPENDENCY" => AssociativeData::Dependency(read_dependency(&record)),
            "ASSOCVALUEDEPENDENCY" => {
                let mut cursor = AssocCursor::new(&record, "AcDbAssocValueDependency");
                AssociativeData::ValueDependency(AssocValueDependency {
                    dependency: read_dependency(&record),
                    class_version: cursor.i32(90),
                    name: cursor.text(1),
                    value: read_eval(&mut cursor),
                })
            }
            "ASSOCGEOMDEPENDENCY" => AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: read_dependency(&record),
                class_version: record.i16("AcDbAssocGeomDependency", 90, 0),
                enabled: record.bool("AcDbAssocGeomDependency", 290, 0),
                persistent_subent: AssocPersistentSubentId {
                    class_name: {
                        let value = record.text("AcDbAssocPersSubentId", 1, 0);
                        if value.is_empty() {
                            record.text("AcDbAssocAsmBasedEntityPersSubentId", 1, 0)
                        } else {
                            value
                        }
                    },
                    dependent_on_compound_object: record.bool("AcDbAssocPersSubentId", 290, 0)
                        || record.bool("AcDbAssocAsmBasedEntityPersSubentId", 290, 0),
                },
            }),
            "ASSOCACTION" => AssociativeData::Action(read_action(&record)),
            "ASSOCNETWORK" => {
                let mut cursor = AssocCursor::new(&record, "AcDbAssocNetwork");
                let network_version = cursor.i16(90);
                let network_action_index = cursor.i32(90);
                let count = cursor.i32(90).max(0).min(100_000);
                let mut actions = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let mut dependency = Handle::NULL;
                    let mut is_owned = false;
                    while let Some((code, value)) = cursor.entries.get(cursor.position) {
                        cursor.position += 1;
                        if *code == 330 || *code == 360 {
                            dependency = parse_dxf_handle(value);
                            is_owned = *code == 360;
                            break;
                        }
                    }
                    actions.push(AssocActionDependency {
                        is_owned,
                        dependency,
                    });
                }
                let count = cursor.i32(90).max(0).min(100_000);
                let mut owned_actions = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    owned_actions.push(cursor.handle(330));
                }
                AssociativeData::Network(AssocNetwork {
                    action: read_action(&record),
                    network_version,
                    network_action_index,
                    actions,
                    owned_actions,
                })
            }
            name if surface_kind(name).is_some() => AssociativeData::SurfaceActionBody(
                read_surface(&record, surface_kind(name).unwrap()),
            ),
            "ASSOCRESTOREENTITYSTATEACTIONBODY" => {
                AssociativeData::AnnotationActionBody(AssocAnnotationActionBody {
                    kind: AssocAnnotationKind::RestoreEntityState,
                    action_body: AssocActionBody {
                        version: record.i32("AcDbAssocActionBody", 90, 0),
                    },
                    class_version: record.i32("AcDbAssocRestoreEntityStateActionBody", 90, 0),
                    entity: record.handle("AcDbAssocRestoreEntityStateActionBody", 330, 0),
                    ..AssocAnnotationActionBody::default()
                })
            }
            "ASSOCMLEADERACTIONBODY" => {
                let count = record
                    .i32("AcDbAssocMLeaderActionBody", 90, 1)
                    .max(0)
                    .min(100_000);
                let ids = record.values("AcDbAssocMLeaderActionBody", 90);
                let handles = record.values("AcDbAssocMLeaderActionBody", 330);
                let mut actions = Vec::with_capacity(count as usize);
                for index in 0..count as usize {
                    actions.push(AssocAnnotationDependency {
                        dependency_id: ids
                            .get(index + 2)
                            .and_then(|value| value.parse().ok())
                            .unwrap_or_default(),
                        dependency: handles
                            .get(index)
                            .map(|value| parse_dxf_handle(value))
                            .unwrap_or(Handle::NULL),
                    });
                }
                AssociativeData::AnnotationActionBody(AssocAnnotationActionBody {
                    kind: AssocAnnotationKind::MLeader,
                    annotation: AssocAnnotationBase {
                        action_body: AssocActionBody {
                            version: record.i32("AcDbAssocActionBody", 90, 0),
                        },
                        version: record.i16("AcDbAssocActionBody", 90, 1),
                        dependency: record.handle("AcDbAssocActionBody", 330, 0),
                        parameter_body: AssocParamBasedActionBody::default(),
                    },
                    class_version: record.i32("AcDbAssocMLeaderActionBody", 90, 0),
                    actions,
                    ..AssocAnnotationActionBody::default()
                })
            }
            "ASSOCALIGNEDDIMACTIONBODY"
            | "ASSOC3POINTANGULARDIMACTIONBODY"
            | "ASSOCORDINATEDIMACTIONBODY"
            | "ASSOCROTATEDDIMACTIONBODY" => {
                let (kind, section) = match canonical.as_str() {
                    "ASSOCALIGNEDDIMACTIONBODY" => (
                        AssocAnnotationKind::AlignedDimension,
                        "ACDBASSOCALIGNEDDIMACTIONBODY",
                    ),
                    "ASSOC3POINTANGULARDIMACTIONBODY" => (
                        AssocAnnotationKind::ThreePointAngularDimension,
                        "Assoc3PointAngularDimActionBody",
                    ),
                    "ASSOCORDINATEDIMACTIONBODY" => (
                        AssocAnnotationKind::OrdinateDimension,
                        "AssocOrdinatedDimActionBody",
                    ),
                    _ => (
                        AssocAnnotationKind::RotatedDimension,
                        "AssocRotatedDimActionBody",
                    ),
                };
                AssociativeData::AnnotationActionBody(AssocAnnotationActionBody {
                    kind,
                    annotation: AssocAnnotationBase {
                        action_body: AssocActionBody {
                            version: record.i32("AcDbAssocActionBody", 90, 0),
                        },
                        version: record.i16("AcDbAssocActionBody", 90, 1),
                        dependency: record.handle("AcDbAssocActionBody", 330, 0),
                        parameter_body: AssocParamBasedActionBody::default(),
                    },
                    class_version: record.i32(section, 90, 0),
                    read_node: record.handle(section, 330, 0),
                    dimension_node: record.handle(section, 330, 1),
                    dependency: record.handle(section, 330, 2),
                    ..AssocAnnotationActionBody::default()
                })
            }
            "ASSOCPERSSUBENTMANAGER" => {
                let values = record.values("AcDbAssocPersSubentManager", 90);
                let mut parsed = values
                    .iter()
                    .map(|value| value.parse::<i32>().unwrap_or_default());
                let class_version = parsed.next().unwrap_or_default();
                let markers = [
                    parsed.next().unwrap_or_default(),
                    parsed.next().unwrap_or_default(),
                    parsed.next().unwrap_or_default(),
                ];
                let step_count = parsed.next().unwrap_or_default().max(0).min(100_000);
                let mut steps = Vec::with_capacity(step_count as usize);
                for _ in 0..step_count {
                    steps.push(parsed.next().unwrap_or_default());
                }
                let subent_count = parsed.next().unwrap_or_default();
                AssociativeData::PersSubentManager(AssocPersSubentManager {
                    class_version,
                    markers,
                    steps,
                    subent_count,
                    subent_data: parsed.collect(),
                    final_flag: record.bool("AcDbAssocPersSubentManager", 290, 0),
                })
            }
            "ASSOCEDGEACTIONPARAM" => {
                let single = AssocSingleDependencyActionParam {
                    action_param: read_action_param(&record),
                    dependency_class_version: record.i32(
                        "AcDbAssocSingleDependencyActionParam",
                        90,
                        0,
                    ),
                    dependency: record.handle("AcDbAssocSingleDependencyActionParam", 330, 0),
                    class_version: record.i32("AcDbAssocEdgeActionParam", 90, 0),
                };
                let action_type = record.i32("AcDbAssocEdgeActionParam", 90, 1);
                AssociativeData::EdgeActionParam(AssocEdgeActionParam {
                    single_dependency: single,
                    parameter: record.handle("AcDbAssocEdgeActionParam", 330, 0),
                    has_action: record.bool("AcDbAssocEdgeActionParam", 290, 0),
                    action_type,
                    subcurve_kind: match action_type {
                        11 => AssocSubcurveKind::Arc,
                        17 => AssocSubcurveKind::Ellipse,
                        19 => AssocSubcurveKind::Line,
                        23 => AssocSubcurveKind::LineSegment3d,
                        42 => AssocSubcurveKind::Nurb3d,
                        27 => AssocSubcurveKind::Curve3d,
                        _ => AssocSubcurveKind::None,
                    },
                })
            }
            "ASSOC2DCONSTRAINTGROUP" => {
                let has_group_section = record.sections.contains_key("AcDbAssoc2dConstraintGroup");
                let section = if has_group_section {
                    "AcDbAssoc2dConstraintGroup"
                } else {
                    "AcDbAssocAction"
                };
                let mut cursor = AssocCursor::new(&record, section);
                if !has_group_section {
                    skip_action(&mut cursor);
                }
                let version = cursor.i32(90);
                let flag = cursor.bool(70);
                let mut work_plane = [Vector3::ZERO; 3];
                for point in &mut work_plane {
                    point.x = cursor.f64(10);
                    point.y = cursor.f64(20);
                    point.z = cursor.f64(30);
                }
                let dependency = cursor.handle(360);
                let count = cursor.i32(90).max(0).min(100_000);
                let mut actions = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    actions.push(cursor.handle(360));
                }
                let node_count = cursor.i32(90).max(0).min(100_000);
                let mut nodes = Vec::with_capacity(node_count as usize);
                if node_count > 0 {
                    nodes.push(read_constraint_root(&mut cursor));
                    let after_root = cursor.position;
                    let registered_count = cursor.i32(90).max(0).min(100_000);
                    let has_registry =
                        registered_count <= node_count && cursor.peek_code() == Some(1);
                    if has_registry {
                        let mut registry = Vec::with_capacity(registered_count as usize);
                        for _ in 0..registered_count {
                            registry.push((cursor.text(1), cursor.i32(90)));
                        }
                        for (class_name, registry_node_id) in registry {
                            let mut node = read_constraint_common(&mut cursor, dxf_version);
                            if node.node_id == 0 {
                                node.node_id = registry_node_id;
                            }
                            node.class_name = class_name;
                            node.data = read_constraint_data(&mut cursor, &node.class_name);
                            nodes.push(node);
                        }
                    } else {
                        cursor.position = after_root;
                        for _ in 1..node_count {
                            nodes.push(read_constraint_common(&mut cursor, dxf_version));
                        }
                    }
                }
                AssociativeData::ConstraintGroup(Assoc2dConstraintGroup {
                    action: read_action(&record),
                    version,
                    flag,
                    work_plane,
                    dependency,
                    actions,
                    nodes,
                })
            }
            "ASSOCVARIABLE" => {
                let mut cursor = AssocCursor::new(&record, "AcDbAssocVariable");
                let action = read_action(&record);
                let class_version = cursor.i32(90);
                let name = cursor.text(1);
                let expression = cursor.text(1);
                let evaluator = cursor.text(1);
                let description = cursor.text(1);
                let value = read_eval(&mut cursor);
                let has_cached_value = cursor.bool(290);
                let cached_value = if has_cached_value {
                    cursor.text(1)
                } else {
                    String::new()
                };
                AssociativeData::Variable(AssocVariable {
                    action,
                    class_version,
                    name,
                    expression,
                    evaluator,
                    description,
                    value,
                    has_cached_value,
                    cached_value,
                    flag: cursor.bool(290),
                    reserved: cursor.i32(90),
                })
            }
            "ASSOCACTIONPARAM" => AssociativeData::ActionParam(read_action_param(&record)),
            "ASSOCCOMPOUNDACTIONPARAM" => {
                AssociativeData::CompoundActionParam(read_compound(&record))
            }
            "ASSOCOSNAPPOINTREFACTIONPARAM" => {
                let section = "ACDBASSOCOSNAPPOINTREFACTIONPARAM";
                AssociativeData::OsnapPointRefActionParam(AssocOsnapPointRefActionParam {
                    compound: read_compound(&record),
                    status: record.i16(section, 90, 0),
                    osnap_mode: record.byte(section, 90, 1),
                    parameter: record.f64(section, 40, 0),
                })
            }
            "ASSOCPOINTREFACTIONPARAM" => {
                AssociativeData::PointRefActionParam(read_compound(&record))
            }
            "ASSOCOBJECTACTIONPARAM"
            | "ASSOCFACEACTIONPARAM"
            | "ASSOCVERTEXACTIONPARAM"
            | "ASSOCASMBODYACTIONPARAM" => {
                let (section, class_code) = match canonical.as_str() {
                    "ASSOCFACEACTIONPARAM" => ("AcDbAssocFaceActionParam", 90),
                    "ASSOCVERTEXACTIONPARAM" => ("AcDbAssocVertexActionParam", 90),
                    "ASSOCASMBODYACTIONPARAM" => ("AcDbAssocAsmbodyActionParam", 90),
                    _ => ("AcDbAssocObjectActionParam", 90),
                };
                let single = AssocSingleDependencyActionParam {
                    action_param: read_action_param(&record),
                    dependency_class_version: record.i32(
                        "AcDbAssocSingleDependencyActionParam",
                        90,
                        0,
                    ),
                    dependency: record.handle("AcDbAssocSingleDependencyActionParam", 330, 0),
                    class_version: record.i32(section, class_code, 0),
                };
                match canonical.as_str() {
                    "ASSOCFACEACTIONPARAM" => {
                        AssociativeData::FaceActionParam(AssocFaceActionParam {
                            single_dependency: single,
                            index: record.i32(section, 90, 1),
                        })
                    }
                    "ASSOCVERTEXACTIONPARAM" => {
                        AssociativeData::VertexActionParam(AssocVertexActionParam {
                            single_dependency: single,
                            point: record.point(section, 10, 0),
                        })
                    }
                    "ASSOCASMBODYACTIONPARAM" => {
                        let sat = record
                            .values("AcDbModelerGeometry", 1)
                            .into_iter()
                            .chain(record.values("AcDbModelerGeometry", 3))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let encoded = record.i16("AcDbModelerGeometry", 70, 0) == 1;
                        let sat = if encoded && !sat.is_empty() {
                            AcisData::decode_sat(&sat)
                        } else {
                            sat
                        };
                        AssociativeData::AsmBodyActionParam(AssocAsmBodyActionParam {
                            single_dependency: single,
                            acis_data: AcisData {
                                version: AcisVersion::Version1,
                                sat_data: AcisData::strip_sat_terminator(&sat),
                                ..AcisData::new()
                            },
                            ..AssocAsmBodyActionParam::default()
                        })
                    }
                    _ => AssociativeData::ObjectActionParam(single),
                }
            }
            "ASSOCPATHACTIONPARAM" => AssociativeData::PathActionParam(AssocPathActionParam {
                compound: read_compound(&record),
                version: record.i32("AcDbAssocPathActionParam", 90, 0),
            }),
            "ASSOCDIMDEPENDENCYBODY" => {
                AssociativeData::DimDependencyBody(AssocDimDependencyBody {
                    dependency_body_version: record.i16("AcDbAssocDependencyBody", 90, 0),
                    base_version: record.i16("AcDbImpAssocDimDependencyBodyBase", 90, 0),
                    name: record.text("AcDbImpAssocDimDependencyBodyBase", 1, 0),
                    class_version: record.i16("AcDbAssocDimDependencyBody", 90, 0),
                })
            }
            "ASSOCARRAYMODIFYPARAMETERS"
            | "ASSOCARRAYPATHPARAMETERS"
            | "ASSOCARRAYPOLARPARAMETERS"
            | "ASSOCARRAYRECTANGULARPARAMETERS" => {
                AssociativeData::ArrayParameters(read_array_parameters(&record))
            }
            "ASSOCARRAYACTIONBODY" | "ASSOCARRAYMODIFYACTIONBODY" => {
                let section = "AcDbAssocArrayActionBody";
                let mut matrix = [0.0; 16];
                for (target, source) in matrix
                    .iter_mut()
                    .zip(record.values(section, 40).into_iter())
                {
                    *target = source.parse().unwrap_or_default();
                }
                let body = AssocArrayActionBody {
                    action_body: AssocActionBody {
                        version: record.i32("AcDbAssocActionBody", 90, 0),
                    },
                    parameter_body: read_parameter_body(&record),
                    version: record.i32(section, 90, 0),
                    parameter_block: record.text(section, 1, 0),
                    transform: matrix,
                };
                if canonical == "ASSOCARRAYMODIFYACTIONBODY" {
                    let mut cursor = AssocCursor::new(&record, "AcDbAssocArrayModifyActionBody");
                    let status = cursor.i16(70);
                    let count = cursor.i32(90).max(0).min(100_000);
                    let mut item_locations = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        item_locations.push([cursor.i32(90), cursor.i32(90), cursor.i32(90)]);
                    }
                    AssociativeData::ArrayModifyActionBody(AssocArrayModifyActionBody {
                        body,
                        status,
                        item_locations,
                    })
                } else {
                    AssociativeData::ArrayActionBody(body)
                }
            }
            "ASSOCVIEWREPACTIONBODY" => {
                let section = "AcDbAssocViewRepActionBody";
                AssociativeData::ViewRepActionBody(AssocViewRepActionBody {
                    action_body: AssocActionBody {
                        version: record.i32("AcDbAssocActionBody", 90, 0),
                    },
                    class_version: record.i16(section, 70, 0),
                    view_rep: record.handle(section, 360, 0),
                    view_type: record.i32(section, 90, 0),
                    rotation: record.f64(section, 40, 0),
                })
            }
            "ASSOCVIEWBORDERACTIONPARAM"
            | "ASSOCVIEWREPACTIONPARAM"
            | "ASSOCVIEWSYMBOLACTIONPARAM"
            | "ASSOCVIEWSTYLEACTIONPARAM" => {
                let (kind, section) = match canonical.as_str() {
                    "ASSOCVIEWREPACTIONPARAM" => (
                        AssocViewObjectActionParamKind::ViewRep,
                        "AcDbAssocViewRepActionParam",
                    ),
                    "ASSOCVIEWSYMBOLACTIONPARAM" => (
                        AssocViewObjectActionParamKind::ViewSymbol,
                        "AcDbAssocViewSymbolActionParam",
                    ),
                    "ASSOCVIEWSTYLEACTIONPARAM" => (
                        AssocViewObjectActionParamKind::ViewStyle,
                        "AcDbAssocViewStyleActionParam",
                    ),
                    _ => (
                        AssocViewObjectActionParamKind::ViewBorder,
                        "AcDbAssocViewBorderActionParam",
                    ),
                };
                AssociativeData::ViewObjectActionParam(AssocViewObjectActionParam {
                    kind,
                    single_dependency: AssocSingleDependencyActionParam {
                        action_param: read_action_param(&record),
                        dependency_class_version: record.i32(
                            "AcDbAssocSingleDependencyActionParam",
                            90,
                            0,
                        ),
                        dependency: record.handle("AcDbAssocSingleDependencyActionParam", 330, 0),
                        class_version: record.i32("AcDbAssocObjectActionParam", 90, 0),
                    },
                    class_version: record.i16(section, 70, 0),
                })
            }
            "ASSOCVIEWREPHATCHMANAGER" => {
                let mut cursor = AssocCursor::new(&record, "AcDbAssocViewRepHatchManager");
                let class_version = cursor.i16(70);
                let count = cursor.i32(90).max(0).min(100_000);
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    items.push(AssocViewRepHatchManagerItem {
                        first_id: cursor.i64(160),
                        second_id: cursor.i64(160),
                        status: cursor.i32(90),
                        parameter: cursor.handle(330),
                    });
                }
                AssociativeData::ViewRepHatchManager(AssocViewRepHatchManager {
                    compound: read_compound(&record),
                    class_version,
                    items,
                })
            }
            "ASSOCVIEWREPHATCHACTIONPARAM" => {
                let section = "AcDbAssocViewRepHatchActionParam";
                AssociativeData::ViewRepHatchActionParam(AssocViewRepHatchActionParam {
                    single_dependency: AssocSingleDependencyActionParam {
                        action_param: read_action_param(&record),
                        dependency_class_version: record.i32(
                            "AcDbAssocSingleDependencyActionParam",
                            90,
                            0,
                        ),
                        dependency: record.handle("AcDbAssocSingleDependencyActionParam", 330, 0),
                        class_version: record.i32("AcDbAssocObjectActionParam", 90, 0),
                    },
                    class_version: record.i16(section, 70, 0),
                    normal: record.point(section, 210, 0),
                    hatch_index: record.i32(section, 90, 0),
                    flags: record.i32(section, 90, 1),
                })
            }
            "ASSOCVIEWLABELACTIONPARAM" => {
                let section = "AcDbAssocViewLabelActionParam";
                AssociativeData::ViewLabelActionParam(AssocViewLabelActionParam {
                    single_dependency: AssocSingleDependencyActionParam {
                        action_param: read_action_param(&record),
                        dependency_class_version: record.i32(
                            "AcDbAssocSingleDependencyActionParam",
                            90,
                            0,
                        ),
                        dependency: record.handle("AcDbAssocSingleDependencyActionParam", 330, 0),
                        class_version: record.i32("AcDbAssocObjectActionParam", 90, 0),
                    },
                    class_version: record.i16(section, 70, 0),
                    label_version: record.i16(section, 70, 1),
                    offset: Vector2::new(record.f64(section, 210, 0), record.f64(section, 220, 0)),
                    flag: record.byte(section, 280, 0),
                })
            }
            "DIMASSOC" => {
                AssociativeData::DimensionAssociation(read_dimension_association_dxf(&record))
            }
            "PERSUBENTMGR" => AssociativeData::PersSubentManagerStatic(
                read_static_pers_subent_manager_dxf(&record),
            ),
            _ => AssociativeData::Unknown,
        };
        Ok(AssociativeObject {
            handle: record.handle,
            owner: record.owner,
            reactors: record.reactors,
            xdictionary_handle: record.xdictionary_handle,
            dxf_name: dxf_name.to_string(),
            cpp_class_name: associative_cpp_class_name(dxf_name)
                .unwrap_or("AcDbObject")
                .to_string(),
            data,
            source_version: None,
        })
    }
}
