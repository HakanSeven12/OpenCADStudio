//! DXF section readers

mod associative;
mod table_content;

use super::stream_reader::{DxfStreamReader, PointReader};
use crate::document::CadDocument;
use crate::entities::*;
use crate::error::Result;
use crate::objects::*;
use crate::objects::{
    DetailViewStyle as ClassDetailViewStyle, SectionViewStyle as ClassSectionViewStyle,
};
use crate::tables::linetype::LineTypeElement;
use crate::tables::*;
use crate::types::*;
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

/// Build a [`Matrix4`] from 12 doubles holding a 4×3 transform in DXF
/// column-major order (4 columns of 3 rows each). The implied bottom row is
/// `[0, 0, 0, 1]`.
/// Build a 4×4 from 12 row-major values (a SPATIAL_FILTER transform is stored
/// row-major, matching the DWG builder — reading it column-major transposed the
/// clip and put the xclip region in the wrong place).
fn matrix_from_row_major(v: &[f64]) -> Matrix4 {
    Matrix4 {
        m: [
            [v[0], v[1], v[2], v[3]],
            [v[4], v[5], v[6], v[7]],
            [v[8], v[9], v[10], v[11]],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

fn parse_dxf_handle(value: &str) -> Handle {
    u64::from_str_radix(value.trim(), 16)
        .map(Handle::new)
        .unwrap_or(Handle::NULL)
}

fn append_hex_bytes(target: &mut Vec<u8>, value: &str) {
    let bytes = value.trim().as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if let (Some(high), Some(low)) = (
            (bytes[index] as char).to_digit(16),
            (bytes[index + 1] as char).to_digit(16),
        ) {
            target.push((high * 16 + low) as u8);
        }
        index += 2;
    }
}

fn loft_reference_xdata(data: &ExtendedData) -> Option<(Vec<Handle>, Vec<Handle>, Option<Handle>)> {
    let record = data.get_record("CADCODEC_LOFT_REFERENCES")?;
    let mut values = record.values.iter();
    if values.next() != Some(&XDataValue::Integer16(1)) {
        return None;
    }
    let mut counts = [0usize; 3];
    for count in &mut counts {
        let XDataValue::Integer32(value) = values.next()? else {
            return None;
        };
        *count = usize::try_from(*value).ok()?;
    }
    if counts[2] > 1
        || counts
            .iter()
            .try_fold(0usize, |sum, count| sum.checked_add(*count))?
            != values.len()
    {
        return None;
    }
    let handles: Vec<Handle> = values
        .map(|value| match value {
            XDataValue::Handle(handle) => Some(*handle),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let guide_end = counts[0] + counts[1];
    Some((
        handles[..counts[0]].to_vec(),
        handles[counts[0]..guide_end].to_vec(),
        handles.get(guide_end).copied(),
    ))
}

fn semantic_property_from_pair(
    subclass: &str,
    pair: &super::stream_reader::DxfCodePair,
) -> SemanticProperty {
    use crate::io::dxf::GroupCodeValueType;
    let value = match GroupCodeValueType::from_raw_code(pair.code) {
        GroupCodeValueType::Bool => SemanticPropertyValue::Bool(pair.as_bool().unwrap_or(false)),
        GroupCodeValueType::Byte => SemanticPropertyValue::Byte(pair.as_int().unwrap_or(0) as u8),
        GroupCodeValueType::Int16 => {
            SemanticPropertyValue::Int16(pair.as_int().unwrap_or(0) as i16)
        }
        GroupCodeValueType::Int32 => {
            SemanticPropertyValue::Int32(pair.as_int().unwrap_or(0) as i32)
        }
        GroupCodeValueType::Int64 => SemanticPropertyValue::Int64(pair.as_int().unwrap_or(0)),
        GroupCodeValueType::Double | GroupCodeValueType::Point3D => {
            SemanticPropertyValue::Double(pair.as_double().unwrap_or(0.0))
        }
        GroupCodeValueType::Handle => {
            SemanticPropertyValue::Handle(pair.as_handle().map(Handle::new).unwrap_or(Handle::NULL))
        }
        GroupCodeValueType::BinaryData => {
            let mut bytes = Vec::new();
            append_hex_bytes(&mut bytes, &pair.value_string);
            SemanticPropertyValue::Binary(bytes)
        }
        GroupCodeValueType::String | GroupCodeValueType::None => {
            SemanticPropertyValue::Text(pair.value_string.clone())
        }
    };
    SemanticProperty {
        subclass: subclass.to_string(),
        code: pair.code,
        value,
    }
}

fn take_dgn_property(
    properties: &mut Vec<SemanticProperty>,
    subclass: &str,
    code: i32,
) -> Option<SemanticPropertyValue> {
    let index = properties
        .iter()
        .position(|property| property.subclass == subclass && property.code == code)?;
    Some(properties.remove(index).value)
}

fn take_all_dgn_properties(
    properties: &mut Vec<SemanticProperty>,
    subclass: &str,
    code: i32,
) -> Vec<SemanticPropertyValue> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < properties.len() {
        if properties[index].subclass == subclass && properties[index].code == code {
            result.push(properties.remove(index).value);
        } else {
            index += 1;
        }
    }
    result
}

fn dgn_i32(value: Option<SemanticPropertyValue>, default: i32) -> i32 {
    match value {
        Some(SemanticPropertyValue::Int32(value)) => value,
        Some(SemanticPropertyValue::Int16(value)) => value as i32,
        Some(SemanticPropertyValue::Byte(value)) => value as i32,
        _ => default,
    }
}

fn dgn_f64(value: Option<SemanticPropertyValue>, default: f64) -> f64 {
    match value {
        Some(SemanticPropertyValue::Double(value)) => value,
        _ => default,
    }
}

fn dgn_bool(value: Option<SemanticPropertyValue>) -> bool {
    match value {
        Some(SemanticPropertyValue::Bool(value)) => value,
        Some(SemanticPropertyValue::Byte(value)) => value != 0,
        Some(SemanticPropertyValue::Int16(value)) => value != 0,
        Some(SemanticPropertyValue::Int32(value)) => value != 0,
        _ => false,
    }
}

fn dgn_handle(value: Option<SemanticPropertyValue>) -> Handle {
    match value {
        Some(SemanticPropertyValue::Handle(value)) => value,
        _ => Handle::NULL,
    }
}

fn dgn_uid(value: Option<SemanticPropertyValue>) -> [u8; 16] {
    let mut uid = [0; 16];
    if let Some(SemanticPropertyValue::Binary(value)) = value {
        let count = value.len().min(16);
        uid[..count].copy_from_slice(&value[..count]);
    }
    uid
}

fn read_dgn_stroke_pattern_dxf(
    properties: &mut Vec<SemanticProperty>,
    subclass: &str,
) -> DgnLsStrokePattern {
    let has_iteration_limit = dgn_bool(take_dgn_property(properties, subclass, 290));
    let is_single_segment = dgn_bool(take_dgn_property(properties, subclass, 291));
    let iteration_limit = dgn_i32(take_dgn_property(properties, subclass, 92), 0);
    let auto_phase = dgn_f64(take_dgn_property(properties, subclass, 41), 0.0);
    let phase = dgn_f64(take_dgn_property(properties, subclass, 42), 0.0);
    let phase_mode =
        DgnLsPhaseMode::from_code(dgn_i32(take_dgn_property(properties, subclass, 281), 0) as u8);
    let count = dgn_i32(take_dgn_property(properties, subclass, 93), 0).max(0) as usize;
    let mut dash = take_all_dgn_properties(properties, subclass, 292);
    let mut bypass = take_all_dgn_properties(properties, subclass, 293);
    let mut scalable = take_all_dgn_properties(properties, subclass, 294);
    let mut invert_origin = take_all_dgn_properties(properties, subclass, 295);
    let mut invert_end = take_all_dgn_properties(properties, subclass, 296);
    let mut lengths = take_all_dgn_properties(properties, subclass, 43);
    let mut start_widths = take_all_dgn_properties(properties, subclass, 44);
    let mut end_widths = take_all_dgn_properties(properties, subclass, 45);
    let mut width_modes = take_all_dgn_properties(properties, subclass, 94);
    let mut cap_modes = take_all_dgn_properties(properties, subclass, 95);
    let mut strokes = Vec::with_capacity(count);
    for _ in 0..count {
        strokes.push(DgnLsStroke {
            is_dash: dgn_bool(take_first(&mut dash)),
            bypass_corner: dgn_bool(take_first(&mut bypass)),
            can_be_scaled: dgn_bool(take_first(&mut scalable)),
            invert_at_origin: dgn_bool(take_first(&mut invert_origin)),
            invert_at_end: dgn_bool(take_first(&mut invert_end)),
            length: dgn_f64(take_first(&mut lengths), 0.0),
            start_width: dgn_f64(take_first(&mut start_widths), 0.0),
            end_width: dgn_f64(take_first(&mut end_widths), 0.0),
            width_mode: dgn_i32(take_first(&mut width_modes), 0),
            cap_mode: dgn_i32(take_first(&mut cap_modes), 0),
        });
    }
    DgnLsStrokePattern {
        has_iteration_limit,
        is_single_segment,
        iteration_limit,
        auto_phase,
        phase,
        phase_mode,
        strokes,
    }
}

fn take_first(values: &mut Vec<SemanticPropertyValue>) -> Option<SemanticPropertyValue> {
    if values.is_empty() {
        None
    } else {
        Some(values.remove(0))
    }
}

fn field_next_code<'a>(
    entries: &'a [(i32, String)],
    cursor: &mut usize,
    code: i32,
) -> Option<&'a str> {
    while *cursor < entries.len() {
        let (current_code, value) = &entries[*cursor];
        *cursor += 1;
        if *current_code == code {
            return Some(value);
        }
    }
    None
}

fn read_field_cell_value_dxf(
    entries: &[(i32, String)],
    cursor: &mut usize,
    version: DxfVersion,
) -> CellValue {
    let mut value = CellValue::new();
    if version >= DxfVersion::AC1021 && entries.get(*cursor).map(|entry| entry.0) == Some(93) {
        value.flags = entries[*cursor].1.parse().unwrap_or(0);
        *cursor += 1;
    }
    let Some(type_text) = field_next_code(entries, cursor, 90) else {
        return value;
    };
    let type_code = type_text.parse::<u32>().unwrap_or(0);
    value.raw_type_code = type_code as i32;
    value.value_type = CellValueType::from(type_code);
    if version < DxfVersion::AC1021 || (value.flags & 3) == 0 {
        match type_code {
            0 | 1 => {
                value.numeric_value = field_next_code(entries, cursor, 91)
                    .and_then(|item| item.parse::<i32>().ok())
                    .unwrap_or(0) as f64;
            }
            2 => {
                value.numeric_value = field_next_code(entries, cursor, 140)
                    .and_then(|item| item.parse().ok())
                    .unwrap_or(0.0);
            }
            4 | 0x200 => {
                value.text = field_next_code(entries, cursor, 1)
                    .unwrap_or("")
                    .to_string();
            }
            8 => {
                value.data_size = field_next_code(entries, cursor, 92)
                    .and_then(|item| item.parse().ok())
                    .unwrap_or(0);
                while entries.get(*cursor).map(|entry| entry.0) == Some(310) {
                    append_hex_bytes(&mut value.binary_value, &entries[*cursor].1);
                    *cursor += 1;
                }
            }
            0x10 | 0x20 => {
                value.data_size = field_next_code(entries, cursor, 92)
                    .and_then(|item| item.parse().ok())
                    .unwrap_or(0);
                value.point_value.x = field_next_code(entries, cursor, 11)
                    .and_then(|item| item.parse().ok())
                    .unwrap_or(0.0);
                if entries.get(*cursor).map(|entry| entry.0) == Some(21) {
                    value.point_value.y = entries[*cursor].1.parse().unwrap_or(0.0);
                    *cursor += 1;
                }
                if type_code == 0x20 && entries.get(*cursor).map(|entry| entry.0) == Some(31) {
                    value.point_value.z = entries[*cursor].1.parse().unwrap_or(0.0);
                    *cursor += 1;
                }
            }
            0x40 => {
                value.handle_value = field_next_code(entries, cursor, 330).map(parse_dxf_handle);
            }
            0x80 | 0x100 => {}
            _ => {}
        }
    }
    if version >= DxfVersion::AC1021 {
        if entries.get(*cursor).map(|entry| entry.0) == Some(94) {
            value.raw_unit_type_code = entries[*cursor].1.parse().unwrap_or(0);
            value.unit_type = ValueUnitType::from(value.raw_unit_type_code as u32);
            *cursor += 1;
        }
        if entries.get(*cursor).map(|entry| entry.0) == Some(300) {
            value.format = entries[*cursor].1.clone();
            *cursor += 1;
        }
        if value.raw_unit_type_code != 12 && entries.get(*cursor).map(|entry| entry.0) == Some(302)
        {
            value.formatted_value = entries[*cursor].1.clone();
            *cursor += 1;
        }
    }
    value
}

fn dynamic_entity_cpp_name(dxf_name: &str) -> Option<&'static str> {
    match dxf_name {
        "ALIGNMENTPARAMETERENTITY" => Some("AcDbBlockAlignmentParameterEntity"),
        "BASEPOINTPARAMETERENTITY" => Some("AcDbBlockBasepointParameterEntity"),
        "FLIPPARAMETERENTITY" => Some("AcDbBlockFlipParameterEntity"),
        "LINEARPARAMETERENTITY" => Some("AcDbBlockLinearParameterEntity"),
        "POINTPARAMETERENTITY" => Some("AcDbBlockPointParameterEntity"),
        "ROTATIONPARAMETERENTITY" => Some("AcDbBlockRotationParameterEntity"),
        "VISIBILITYPARAMETERENTITY" => Some("AcDbBlockVisibilityParameterEntity"),
        "FLIPGRIPENTITY" => Some("AcDbBlockFlipGripEntity"),
        "LINEARGRIPENTITY" => Some("AcDbBlockLinearGripEntity"),
        "POLARGRIPENTITY" => Some("AcDbBlockPolarGripEntity"),
        "ROTATIONGRIPENTITY" => Some("AcDbBlockRotationGripEntity"),
        "VISIBILITYGRIPENTITY" => Some("AcDbBlockVisibilityGripEntity"),
        "XYGRIPENTITY" => Some("AcDbBlockXYGripEntity"),
        "XYPARAMETERENTITY" => Some("AcDbBlockXYParameterEntity"),
        _ => None,
    }
}

fn is_dynamic_block_object_name(name: &str) -> bool {
    matches!(
        name,
        "ACSH_BOOLEAN_CLASS"
            | "ACSH_BOX_CLASS"
            | "ACSH_BREP_CLASS"
            | "ACSH_CHAMFER_CLASS"
            | "ACSH_CONE_CLASS"
            | "ACSH_CYLINDER_CLASS"
            | "ACSH_EXTRUSION_CLASS"
            | "ACSH_FILLET_CLASS"
            | "ACSH_HISTORY_CLASS"
            | "ACSH_LOFT_CLASS"
            | "ACSH_PYRAMID_CLASS"
            | "ACSH_REVOLVE_CLASS"
            | "ACSH_SPHERE_CLASS"
            | "ACSH_SWEEP_CLASS"
            | "ACSH_TORUS_CLASS"
            | "ACSH_WEDGE_CLASS"
            | "ACDB_BLOCKREPRESENTATION_DATA"
            | "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION"
            | "ACDB_DYNAMICBLOCKPROXYNODE"
            | "ACAD_EVALUATION_GRAPH"
            | "BLOCKGRIPLOCATIONCOMPONENT"
            | "BLOCKALIGNMENTPARAMETER"
            | "BLOCKALIGNMENTGRIP"
            | "BLOCKBASEPOINTPARAMETER"
            | "BLOCKFLIPACTION"
            | "BLOCKFLIPPARAMETER"
            | "BLOCKFLIPGRIP"
            | "BLOCKLINEARGRIP"
            | "BLOCKLOOKUPGRIP"
            | "BLOCKROTATIONGRIP"
            | "BLOCKMOVEACTION"
            | "BLOCKROTATEACTION"
            | "BLOCKSCALEACTION"
            | "BLOCKVISIBILITYGRIP"
            | "BLOCKVISIBILITYPARAMETER"
            | "BLOCKLINEARPARAMETER"
            | "BLOCKROTATIONPARAMETER"
            | "BLOCKXYPARAMETER"
            | "BLOCKPOLARPARAMETER"
            | "BLOCKPOLARGRIP"
            | "ACDBBLOCKPARAMDEPENDENCYBODY"
            | "BLOCKPARAMDEPENDENCYBODY"
            | "BLOCKALIGNEDCONSTRAINTPARAMETER"
            | "BLOCKANGULARCONSTRAINTPARAMETER"
            | "BLOCKARRAYACTION"
            | "BLOCKDIAMETRICCONSTRAINTPARAMETER"
            | "BLOCKHORIZONTALCONSTRAINTPARAMETER"
            | "BLOCKLINEARCONSTRAINTPARAMETER"
            | "BLOCKRADIALCONSTRAINTPARAMETER"
            | "BLOCKVERTICALCONSTRAINTPARAMETER"
            | "BLOCKLOOKUPACTION"
            | "BLOCKLOOKUPPARAMETER"
            | "BLOCKPOINTPARAMETER"
            | "BLOCKPOLARSTRETCHACTION"
            | "BLOCKSTRETCHACTION"
            | "BLOCKUSERPARAMETER"
            | "BLOCKXYGRIP"
            | "BLOCKPROPERTIESTABLE"
            | "BLOCKPROPERTIESTABLEGRIP"
    )
}

fn is_class_object_name(name: &str) -> bool {
    matches!(
        name,
        "SPATIAL_INDEX"
            | "LAYERFILTER"
            | "PARTIAL_VIEWING_INDEX"
            | "VBA_PROJECT"
            | "SECTION_MANAGER"
            | "SECTION_SETTINGS"
            | "LIGHTLIST"
            | "SUN"
            | "RENDERSETTINGS"
            | "MENTALRAYRENDERSETTINGS"
            | "RAPIDRTRENDERSETTINGS"
            | "GRADIENT_BACKGROUND"
            | "GROUND_PLANE_BACKGROUND"
            | "RAPIDRTRENDERENVIRONMENT"
            | "IBL_BACKGROUND"
            | "IMAGE_BACKGROUND"
            | "SKYLIGHT_BACKGROUND"
            | "SOLID_BACKGROUND"
            | "RENDERENTRY"
            | "RENDERENVIRONMENT"
            | "RENDERGLOBAL"
            | "ACDBMOTIONPATH"
            | "MOTIONPATH"
            | "ACDBCURVEPATH"
            | "CURVEPATH"
            | "ACDBPOINTPATH"
            | "POINTPATH"
            | "TVDEVICEPROPERTIES"
            | "ACDBPOINTCLOUDDEF"
            | "POINTCLOUDDEF"
            | "ACDBPOINTCLOUDDEFEX"
            | "POINTCLOUDDEFEX"
            | "ACDBPOINTCLOUDDEF_REACTOR"
            | "POINTCLOUDDEF_REACTOR"
            | "ACDBPOINTCLOUDDEF_REACTOR_EX"
            | "POINTCLOUDDEF_REACTOR_EX"
            | "ACDBPOINTCLOUDCOLORMAP"
            | "POINTCLOUDCOLORMAP"
            | "NAVISWORKSMODELDEF"
            | "COORDINATION_MODEL_DEFINITION"
            | "CONTEXTDATAMANAGER"
            | "SUNSTUDY"
            | "DATATABLE"
            | "ACDBDATATABLE"
            | "DATALINK"
            | "ACDBPERSSUBENTMANAGER"
            | "PERSUBENTMGR"
            | "GEOMAPIMAGE"
            | "ACDBDETAILVIEWSTYLE"
            | "DETAILVIEWSTYLE"
            | "ACDBSECTIONVIEWSTYLE"
            | "SECTIONVIEWSTYLE"
            | "ACMECOMMANDHISTORY"
            | "ACMESCOPE"
            | "ACMESTATEMGR"
            | "CSACDOCUMENTOPTIONS"
            | "ACDBVIEWREPSOURCEMGR"
            | "ACDBVIEWREPSTANDARD"
            | "ACDBVIEWREPORIENTATIONDEF"
            | "ACDBVIEWREPORIENTATION"
            | "ACDBVIEWREPSECTIONDEFINITION"
            | "ACDBSYMODELSPACEVIEWSELSET"
            | "ACDBVIEWREP"
            | "ACDBVIEWREPMODELSPACESOURCE"
    )
}

fn is_object_context_name(name: &str) -> bool {
    matches!(
        name,
        "ACDB_ANNOTSCALEOBJECTCONTEXTDATA_CLASS"
            | "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS"
            | "ACDB_TEXTOBJECTCONTEXTDATA_CLASS"
            | "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS"
            | "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_DMDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_RADIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS"
            | "ACDB_ORDDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS"
            | "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS"
            | "ACDB_LEADEROBJECTCONTEXTDATA_CLASS"
            | "ACDB_FCFOBJECTCONTEXTDATA_CLASS"
            | "ACDB_HATCHSCALECONTEXTDATA_CLASS"
            | "ACDB_HATCHVIEWCONTEXTDATA_CLASS"
    )
}

fn is_registered_class_entity_name(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "ACAD_PROXY_ENTITY_WRAPPER"
            | "WALL"
            | "MCSDBOBJECT"
            | "NOTEPOSITION"
            | "SPDSLEVELMARK"
            | "SPDSRELATIONMARK"
    )
}

fn is_registered_class_object_name(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "ACAD_PROXY_OBJECT_WRAPPER"
            | "AEC_REFEDIT_STATUS_TRACKER"
            | "EXACXREFPANELOBJECT"
            | "XREFPANELOBJECT"
            | "NPOCOLLECTION"
            | "MCDBCONTAINER2"
    )
}

fn is_dgn_line_style_name(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "LSDEFINITION"
            | "LSSYMBOLCOMPONENT"
            | "LSCOMPOUNDCOMPONENT"
            | "LSSTROKEPATTERNCOMPONENT"
            | "LSPOINTCOMPONENT"
            | "LSINTERNALCOMPONENT"
    )
}

fn registered_class_cpp_name(name: &str) -> &'static str {
    match name.to_uppercase().as_str() {
        "ACAD_PROXY_ENTITY_WRAPPER" => "AcDbProxyEntityWrapper",
        "ACAD_PROXY_OBJECT_WRAPPER" => "AcDbProxyObjectWrapper",
        "AEC_REFEDIT_STATUS_TRACKER" => "AecDbRefEditStatusTracker",
        "EXACXREFPANELOBJECT" | "XREFPANELOBJECT" => "ExAcXREFPanelObject",
        "NPOCOLLECTION" => "AcDbImpNonPersistentObjectsCollection",
        "LSINTERNALCOMPONENT" => "AcDbLSInternalComponent",
        "MCDBCONTAINER2" => "McDbContainer2",
        "MCSDBOBJECT" => "mcsDbObject",
        "NOTEPOSITION" => "mcsDbObjectNotePosition",
        "SPDSLEVELMARK" => "mcsDbObjectLevelMark",
        "SPDSRELATIONMARK" => "mcsDbObjectRelationMark",
        "WALL" => "PtDbWall",
        "LSDEFINITION" => "AcDbLSDefinition",
        "LSSYMBOLCOMPONENT" => "AcDbLSSymbolComponent",
        "LSCOMPOUNDCOMPONENT" => "AcDbLSCompoundComponent",
        "LSSTROKEPATTERNCOMPONENT" => "AcDbLSStrokePatternComponent",
        "LSPOINTCOMPONENT" => "AcDbLSPointComponent",
        _ => "AcDbObject",
    }
}

#[derive(Default)]
struct ClassDxfFields {
    sections: std::collections::HashMap<
        String,
        std::collections::HashMap<i32, std::collections::VecDeque<String>>,
    >,
}

impl ClassDxfFields {
    fn push(&mut self, section: &str, code: i32, value: String) {
        self.sections
            .entry(section.to_string())
            .or_default()
            .entry(code)
            .or_default()
            .push_back(value);
    }

    fn string(&mut self, section: &str, code: i32) -> String {
        self.sections
            .get_mut(section)
            .and_then(|values| values.get_mut(&code))
            .and_then(|values| values.pop_front())
            .unwrap_or_default()
    }

    fn has(&self, section: &str, code: i32) -> bool {
        self.sections
            .get(section)
            .and_then(|values| values.get(&code))
            .map(|values| !values.is_empty())
            .unwrap_or(false)
    }

    fn strings(&mut self, section: &str, code: i32) -> Vec<String> {
        self.sections
            .get_mut(section)
            .and_then(|values| values.remove(&code))
            .map(|values| values.into_iter().collect())
            .unwrap_or_default()
    }

    fn string_skipping(&mut self, section: &str, code: i32, markers: &[&str]) -> String {
        loop {
            let value = self.string(section, code);
            if value.is_empty() || !markers.iter().any(|marker| *marker == value) {
                return value;
            }
        }
    }

    fn i16(&mut self, section: &str, code: i32) -> i16 {
        self.string(section, code).trim().parse().unwrap_or(0)
    }

    fn i32(&mut self, section: &str, code: i32) -> i32 {
        self.string(section, code).trim().parse().unwrap_or(0)
    }

    fn i64(&mut self, section: &str, code: i32) -> i64 {
        self.string(section, code).trim().parse().unwrap_or(0)
    }

    fn f64(&mut self, section: &str, code: i32) -> f64 {
        self.string(section, code).trim().parse().unwrap_or(0.0)
    }

    fn bool(&mut self, section: &str, code: i32) -> bool {
        self.i32(section, code) != 0
    }

    fn handle(&mut self, section: &str, code: i32) -> Handle {
        parse_dxf_handle(&self.string(section, code))
    }

    fn point2(&mut self, section: &str, code: i32) -> Vector2 {
        Vector2::new(self.f64(section, code), self.f64(section, code + 10))
    }

    fn point3(&mut self, section: &str, code: i32) -> Vector3 {
        Vector3::new(
            self.f64(section, code),
            self.f64(section, code + 10),
            self.f64(section, code + 20),
        )
    }
}

fn class_dxf_render_settings(
    fields: &mut ClassDxfFields,
    has_predefined_in_base: bool,
) -> RenderSettings {
    let section = "AcDbRenderSettings";
    RenderSettings {
        class_version: fields.i32(section, 90),
        name: fields.string(section, 1),
        fog_enabled: fields.bool(section, 290),
        fog_background_enabled: fields.bool(section, 290),
        backfaces_enabled: fields.bool(section, 290),
        environment_image_enabled: fields.bool(section, 290),
        environment_image_filename: fields.string(section, 1),
        description: fields.string(section, 1),
        display_index: fields.i32(section, 90),
        has_predefined: has_predefined_in_base && fields.bool(section, 290),
    }
}

fn class_dxf_point_cloud_definition(
    fields: &mut ClassDxfFields,
    section: &str,
) -> PointCloudDefinition {
    PointCloudDefinition {
        class_version: fields.i32(section, 90),
        source_filename: fields.string(section, 1),
        is_loaded: fields.bool(section, 280),
        point_count: fields.i64(section, 160),
        extents_min: fields.point3(section, 10),
        extents_max: fields.point3(section, 11),
    }
}

fn class_dxf_point_cloud_ramps(
    fields: &mut ClassDxfFields,
    section: &str,
) -> Vec<PointCloudColorRamp> {
    let mut result = Vec::new();
    for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
        let class_version = fields.i16(section, 70);
        let mut color_schemes = Vec::new();
        for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
            color_schemes.push(fields.string(section, 1));
        }
        result.push(PointCloudColorRamp {
            class_version,
            color_schemes,
        });
    }
    result
}

fn class_dxf_hatch_scale_context(fields: &mut ClassDxfFields) -> crate::objects::HatchScaleContext {
    let section = "AcDbHatchObjectContextData";
    let mut pattern_lines = Vec::new();
    for _ in 0..fields.i32(section, 78).max(0).min(10_000) {
        let angle = fields.f64(section, 53).to_radians();
        let base_point = Vector2::new(fields.f64(section, 43), fields.f64(section, 44));
        let offset = Vector2::new(fields.f64(section, 45), fields.f64(section, 46));
        let mut dash_lengths = Vec::new();
        for _ in 0..fields.i32(section, 79).max(0).min(10_000) {
            dash_lengths.push(fields.f64(section, 49));
        }
        pattern_lines.push(crate::entities::HatchPatternLine {
            angle,
            base_point,
            offset,
            dash_lengths,
        });
    }
    let pattern_scale = fields.f64(section, 40);
    let pattern_base = fields.point3(section, 10);
    let mut loops = Vec::new();
    for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
        let loop_type = fields.i32(section, 90);
        let supports_context = fields.bool(section, 290);
        loops.push(crate::objects::HatchLoopContext {
            loop_type,
            supports_context,
            boundary: (!supports_context)
                .then(|| class_dxf_hatch_context_boundary(fields, loop_type)),
        });
    }
    crate::objects::HatchScaleContext {
        pattern_lines,
        pattern_scale,
        pattern_base,
        loops,
    }
}

fn class_dxf_hatch_context_boundary(
    fields: &mut ClassDxfFields,
    loop_type: i32,
) -> crate::entities::BoundaryPath {
    use crate::entities::hatch::*;

    let section = "AcDbHatchObjectContextData";
    let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(loop_type as u32));
    if loop_type & BoundaryPathFlags::POLYLINE.bits() as i32 != 0 {
        let has_bulge = fields.bool(section, 72);
        let is_closed = fields.bool(section, 73);
        let mut vertices = Vec::new();
        for _ in 0..fields.i32(section, 93).max(0).min(100_000) {
            let point = fields.point2(section, 10);
            let bulge = if has_bulge {
                fields.f64(section, 42)
            } else {
                0.0
            };
            vertices.push(crate::types::Vector3::new(point.x, point.y, bulge));
        }
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge {
            vertices,
            is_closed,
        }));
        return path;
    }

    for _ in 0..fields.i32(section, 93).max(0).min(100_000) {
        match fields.i16(section, 72) {
            1 => path.add_edge(BoundaryEdge::Line(LineEdge {
                start: fields.point2(section, 10),
                end: fields.point2(section, 11),
            })),
            2 => path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                center: fields.point2(section, 10),
                radius: fields.f64(section, 40),
                start_angle: fields.f64(section, 50).to_radians(),
                end_angle: fields.f64(section, 51).to_radians(),
                counter_clockwise: fields.bool(section, 73),
            })),
            3 => path.add_edge(BoundaryEdge::EllipticArc(EllipticArcEdge {
                center: fields.point2(section, 10),
                major_axis_endpoint: fields.point2(section, 11),
                minor_axis_ratio: fields.f64(section, 40),
                start_angle: fields.f64(section, 50),
                end_angle: fields.f64(section, 51),
                counter_clockwise: fields.bool(section, 73),
            })),
            4 => {
                let degree = fields.i32(section, 94);
                let rational = fields.bool(section, 73);
                let periodic = fields.bool(section, 74);
                let knot_count = fields.i32(section, 95).max(0).min(100_000);
                let control_count = fields.i32(section, 96).max(0).min(100_000);
                let knots = (0..knot_count).map(|_| fields.f64(section, 40)).collect();
                let control_points = (0..control_count)
                    .map(|_| {
                        let point = fields.point2(section, 10);
                        crate::types::Vector3::new(
                            point.x,
                            point.y,
                            if rational {
                                fields.f64(section, 42)
                            } else {
                                1.0
                            },
                        )
                    })
                    .collect();
                let fit_count = fields.i32(section, 97).max(0).min(100_000);
                let fit_points: Vec<_> =
                    (0..fit_count).map(|_| fields.point2(section, 11)).collect();
                let (start_tangent, end_tangent) = if fit_points.is_empty() {
                    (Vector2::ZERO, Vector2::ZERO)
                } else {
                    (fields.point2(section, 12), fields.point2(section, 13))
                };
                path.add_edge(BoundaryEdge::Spline(SplineEdge {
                    degree,
                    rational,
                    periodic,
                    knots,
                    control_points,
                    fit_points,
                    start_tangent,
                    end_tangent,
                }));
            }
            _ => {}
        }
    }
    path
}

fn dynamic_block_cpp_name(name: &str) -> &'static str {
    match name {
        "ACSH_BOOLEAN_CLASS" => "AcDbShBoolean",
        "ACSH_BOX_CLASS" => "AcDbShBox",
        "ACSH_BREP_CLASS" => "AcDbShBrep",
        "ACSH_CHAMFER_CLASS" => "AcDbShChamfer",
        "ACSH_CONE_CLASS" => "AcDbShCone",
        "ACSH_CYLINDER_CLASS" => "AcDbShCylinder",
        "ACSH_EXTRUSION_CLASS" => "AcDbShExtrusion",
        "ACSH_FILLET_CLASS" => "AcDbShFillet",
        "ACSH_HISTORY_CLASS" => "AcDbShHistory",
        "ACSH_LOFT_CLASS" => "AcDbShLoft",
        "ACSH_PYRAMID_CLASS" => "AcDbShPyramid",
        "ACSH_REVOLVE_CLASS" => "AcDbShRevolve",
        "ACSH_SPHERE_CLASS" => "AcDbShSphere",
        "ACSH_SWEEP_CLASS" => "AcDbShSweep",
        "ACSH_TORUS_CLASS" => "AcDbShTorus",
        "ACSH_WEDGE_CLASS" => "AcDbShWedge",
        "ACDB_BLOCKREPRESENTATION_DATA" => "AcDbBlockRepresentationData",
        "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION" => "AcDbDynamicBlockPurgePreventer",
        "ACDB_DYNAMICBLOCKPROXYNODE" => "AcDbDynamicBlockProxyNode",
        "ACAD_EVALUATION_GRAPH" => "AcDbEvalGraph",
        "BLOCKGRIPLOCATIONCOMPONENT" => "AcDbBlockGripExpr",
        "BLOCKALIGNMENTPARAMETER" => "AcDbBlockAlignmentParameter",
        "BLOCKALIGNMENTGRIP" => "AcDbBlockAlignmentGrip",
        "BLOCKBASEPOINTPARAMETER" => "AcDbBlockBasepointParameter",
        "BLOCKFLIPACTION" => "AcDbBlockFlipAction",
        "BLOCKFLIPPARAMETER" => "AcDbBlockFlipParameter",
        "BLOCKFLIPGRIP" => "AcDbBlockFlipGrip",
        "BLOCKLINEARGRIP" => "AcDbBlockLinearGrip",
        "BLOCKLOOKUPGRIP" => "AcDbBlockLookupGrip",
        "BLOCKROTATIONGRIP" => "AcDbBlockRotationGrip",
        "BLOCKMOVEACTION" => "AcDbBlockMoveAction",
        "BLOCKROTATEACTION" => "AcDbBlockRotationAction",
        "BLOCKSCALEACTION" => "AcDbBlockScaleAction",
        "BLOCKVISIBILITYGRIP" => "AcDbBlockVisibilityGrip",
        "BLOCKVISIBILITYPARAMETER" => "AcDbBlockVisibilityParameter",
        "BLOCKLINEARPARAMETER" => "AcDbBlockLinearParameter",
        "BLOCKROTATIONPARAMETER" => "AcDbBlockRotationParameter",
        "BLOCKXYPARAMETER" => "AcDbBlockXYParameter",
        "BLOCKPOLARPARAMETER" => "AcDbBlockPolarParameter",
        "BLOCKPOLARGRIP" => "AcDbBlockPolarGrip",
        "ACDBBLOCKPARAMDEPENDENCYBODY" | "BLOCKPARAMDEPENDENCYBODY" => {
            "AcDbBlockParameterDependencyBody"
        }
        "BLOCKALIGNEDCONSTRAINTPARAMETER" => "AcDbBlockAlignedConstraintParameter",
        "BLOCKANGULARCONSTRAINTPARAMETER" => "AcDbBlockAngularConstraintParameter",
        "BLOCKARRAYACTION" => "AcDbBlockArrayAction",
        "BLOCKDIAMETRICCONSTRAINTPARAMETER" => "AcDbBlockDiametricConstraintParameter",
        "BLOCKHORIZONTALCONSTRAINTPARAMETER" => "AcDbBlockHorizontalConstraintParameter",
        "BLOCKLINEARCONSTRAINTPARAMETER" => "AcDbBlockLinearConstraintParameter",
        "BLOCKRADIALCONSTRAINTPARAMETER" => "AcDbBlockRadialConstraintParameter",
        "BLOCKVERTICALCONSTRAINTPARAMETER" => "AcDbBlockVerticalConstraintParameter",
        "BLOCKLOOKUPACTION" => "AcDbBlockLookupAction",
        "BLOCKLOOKUPPARAMETER" => "AcDbBlockLookupParameter",
        "BLOCKPOINTPARAMETER" => "AcDbBlockPointParameter",
        "BLOCKPOLARSTRETCHACTION" => "AcDbBlockPolarStretchAction",
        "BLOCKSTRETCHACTION" => "AcDbBlockStretchAction",
        "BLOCKUSERPARAMETER" => "AcDbBlockUserParameter",
        "BLOCKXYGRIP" => "AcDbBlockXYGrip",
        "BLOCKPROPERTIESTABLE" => "AcDbBlockPropertiesTable",
        "BLOCKPROPERTIESTABLEGRIP" => "AcDbBlockPropertiesTableGrip",
        _ => "AcDbObject",
    }
}

#[derive(Default)]
struct DynamicDxfFields {
    sections: std::collections::HashMap<String, Vec<(i32, String)>>,
}

impl DynamicDxfFields {
    fn values(&self, section: &str, code: i32) -> Vec<&str> {
        self.sections
            .get(section)
            .into_iter()
            .flatten()
            .filter(|(item_code, _)| *item_code == code)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn text(&self, section: &str, code: i32) -> String {
        self.values(section, code)
            .last()
            .copied()
            .unwrap_or("")
            .to_string()
    }

    fn i32(&self, section: &str, code: i32) -> i32 {
        self.values(section, code)
            .last()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn i16(&self, section: &str, code: i32) -> i16 {
        self.i32(section, code) as i16
    }

    fn f64(&self, section: &str, code: i32) -> f64 {
        self.values(section, code)
            .last()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0.0)
    }

    fn bool(&self, section: &str, code: i32) -> bool {
        self.i32(section, code) != 0
    }

    fn handle(&self, section: &str, code: i32) -> Handle {
        self.values(section, code)
            .last()
            .map(|value| parse_dxf_handle(value))
            .unwrap_or(Handle::NULL)
    }

    fn point(&self, section: &str, x_code: i32) -> Vector3 {
        Vector3::new(
            self.f64(section, x_code),
            self.f64(section, x_code + 10),
            self.f64(section, x_code + 20),
        )
    }
}

fn dynamic_dxf_eval(fields: &DynamicDxfFields) -> BlockEvalExpression {
    let section = "AcDbEvalExpr";
    let short_values = fields.values(section, 70);
    let value_code = short_values
        .first()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    let value = match value_code {
        40 => BlockEvalValue::Real(fields.f64(section, 40)),
        10 | 11 => BlockEvalValue::Point([
            fields.f64(section, value_code as i32),
            fields.f64(section, value_code as i32 + 10),
        ]),
        1 => BlockEvalValue::Text(fields.text(section, 1)),
        90 => {
            let values = fields.values(section, 90);
            BlockEvalValue::Long(
                values
                    .last()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            )
        }
        91 => BlockEvalValue::Handle(fields.handle(section, 91)),
        70 => BlockEvalValue::Short(
            short_values
                .get(1)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0),
        ),
        _ => BlockEvalValue::None,
    };
    BlockEvalExpression {
        parent_id: 0,
        major: fields.i32(section, 98),
        minor: fields.i32(section, 99),
        value_code,
        value,
        node_id: fields
            .values(section, 90)
            .first()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0),
    }
}

fn dynamic_dxf_element(fields: &DynamicDxfFields) -> BlockElement {
    let section = "AcDbBlockElement";
    BlockElement {
        eval: dynamic_dxf_eval(fields),
        name: fields.text(section, 300),
        major: fields.i32(section, 98),
        minor: fields.i32(section, 99),
        eed_1071: fields.i32(section, 1071),
    }
}

fn dynamic_dxf_parameter(fields: &DynamicDxfFields) -> BlockParameter {
    BlockParameter {
        element: dynamic_dxf_element(fields),
        show_properties: fields.bool("AcDbBlockParameter", 280),
        chain_actions: fields.bool("AcDbBlockParameter", 281),
    }
}

fn dynamic_dxf_connections(
    fields: &DynamicDxfFields,
    section: &str,
    code: i32,
    name_code: i32,
) -> Vec<BlockConnection> {
    let codes = fields.values(section, code);
    let names = fields.values(section, name_code);
    let count = codes.len().max(names.len());
    (0..count)
        .map(|index| BlockConnection {
            code: codes
                .get(index)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            name: names.get(index).copied().unwrap_or("").to_string(),
        })
        .collect()
}

fn dynamic_dxf_sequential_connections(
    fields: &DynamicDxfFields,
    section: &str,
    code: i32,
    name_code: i32,
    count: usize,
) -> Vec<BlockConnection> {
    (0..count)
        .map(|index| BlockConnection {
            code: fields.i32(section, code + index as i32),
            name: fields.text(section, name_code + index as i32),
        })
        .collect()
}

fn dynamic_dxf_one_point(fields: &DynamicDxfFields) -> BlockOnePointParameter {
    let section = "AcDbBlock1PtParameter";
    BlockOnePointParameter {
        parameter: dynamic_dxf_parameter(fields),
        definition_point: fields.point(section, 1010),
        properties: [
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 91, 301),
            },
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 92, 302),
            },
        ],
        property_count: fields.i32(section, 93),
    }
}

fn dynamic_dxf_two_point(fields: &DynamicDxfFields) -> BlockTwoPointParameter {
    let section = "AcDbBlock2PtParameter";
    let mut property_states = [0; 4];
    for (target, source) in property_states.iter_mut().zip(fields.values(section, 91)) {
        *target = source.parse().unwrap_or(0);
    }
    BlockTwoPointParameter {
        parameter: dynamic_dxf_parameter(fields),
        definition_base_point: fields.point(section, 1010),
        definition_end_point: fields.point(section, 1011),
        properties: [
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 92, 301),
            },
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 93, 302),
            },
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 94, 303),
            },
            BlockParameterProperty {
                connections: dynamic_dxf_connections(fields, section, 95, 304),
            },
        ],
        property_states,
        parameter_base_location: fields.i16(section, 177),
        updated_base_point: None,
        base_point: None,
        updated_end_point: None,
        end_point: None,
    }
}

fn dynamic_dxf_grip(fields: &DynamicDxfFields) -> BlockGrip {
    let section = "AcDbBlockGrip";
    BlockGrip {
        element: dynamic_dxf_element(fields),
        flags_91: fields.i32(section, 91),
        flags_92: fields.i32(section, 92),
        location: fields.point(section, 1010),
        insert_cycling: fields.bool(section, 280),
        insert_cycling_weight: fields.i32(section, 93),
    }
}

fn dynamic_dxf_action(fields: &DynamicDxfFields) -> BlockAction {
    let section = "AcDbBlockAction";
    BlockAction {
        element: dynamic_dxf_element(fields),
        display_location: fields.point(section, 1010),
        dependencies: fields
            .values(section, 330)
            .into_iter()
            .map(parse_dxf_handle)
            .collect(),
        action_ids: fields
            .values(section, 91)
            .into_iter()
            .filter_map(|value| value.parse().ok())
            .collect(),
    }
}

fn dynamic_dxf_action_with_base(fields: &DynamicDxfFields) -> BlockActionWithBasePoint {
    let section = "AcDbBlockActionWithBasePt";
    BlockActionWithBasePoint {
        action: dynamic_dxf_action(fields),
        offset: fields.point(section, 1011),
        connections: dynamic_dxf_sequential_connections(fields, section, 92, 301, 2),
        dependent: fields.bool(section, 280),
        base_point: fields.point(section, 1012),
    }
}

fn dynamic_dxf_value_set(
    fields: &DynamicDxfFields,
    section: &str,
    flags_code: i32,
    double_code: i32,
    description_code: i32,
) -> BlockParameterValueSet {
    BlockParameterValueSet {
        description: fields.text(section, description_code),
        flags: fields.i32(section, flags_code),
        minimum: fields.f64(section, double_code),
        maximum: fields.f64(section, double_code + 1),
        increment: fields.f64(section, double_code + 2),
        values: fields
            .values(section, double_code + 3)
            .into_iter()
            .filter_map(|value| value.parse().ok())
            .collect(),
    }
}

fn dynamic_dxf_constraint(fields: &DynamicDxfFields) -> BlockConstraintParameter {
    BlockConstraintParameter {
        parameter: dynamic_dxf_two_point(fields),
        dependency: fields.handle("AcDbBlockConstraintParameter", 330),
    }
}

fn dynamic_dxf_linear_constraint(fields: &DynamicDxfFields) -> BlockLinearConstraintParameter {
    let section = "AcDbBlockLinearConstraintParameter";
    BlockLinearConstraintParameter {
        constraint: dynamic_dxf_constraint(fields),
        expression_name: fields.text(section, 305),
        expression_description: fields.text(section, 306),
        value: fields.f64(section, 140),
        value_set: dynamic_dxf_value_set(fields, section, 96, 128, 307),
    }
}

fn dynamic_dxf_history_base(fields: &DynamicDxfFields) -> SolidHistoryNodeBase {
    let section = "AcDbShHistoryNode";
    let mut transform = [0.0; 16];
    for (target, source) in transform.iter_mut().zip(fields.values(section, 40)) {
        *target = source.trim().parse().unwrap_or(0.0);
    }
    let color = if fields.values(section, 420).is_empty() {
        Color::from_index(fields.i16(section, 62))
    } else {
        Color::from_true_color_value(fields.i32(section, 420))
    };
    SolidHistoryNodeBase {
        eval: dynamic_dxf_eval(fields),
        major: fields.i32(section, 90),
        minor: fields.i32(section, 91),
        transform,
        color,
        step_id: fields.i32(section, 92),
        material: fields.handle(section, 347),
    }
}

fn dynamic_dxf_history_sweep(
    fields: &DynamicDxfFields,
    dxf_version: DxfVersion,
) -> SolidHistorySweep {
    let section = "AcDbShSweepBase";
    let mut sweep_entity_transform = [0.0; 16];
    let mut path_entity_transform = [0.0; 16];
    for (target, source) in sweep_entity_transform
        .iter_mut()
        .zip(fields.values(section, 46))
    {
        *target = source.trim().parse().unwrap_or(0.0);
    }
    for (target, source) in path_entity_transform
        .iter_mut()
        .zip(fields.values(section, 47))
    {
        *target = source.trim().parse().unwrap_or(0.0);
    }
    // Group 90 is reused for the operation version and both embedded body
    // sizes. Keep the profile/path boundaries explicit, including absent
    // entities, and accept the padded integer text emitted by DXF writers.
    let mut bodies = [(0, 0usize, Vec::new()), (0, 0usize, Vec::new())];
    let mut current_body = None;
    let mut operation_major = 0;
    for (code, value) in fields.sections.get(section).into_iter().flatten() {
        match *code {
            92 | 93 => {
                let index = usize::from(*code == 93);
                bodies[index].0 = value.trim().parse().unwrap_or(0);
                current_body = Some(index);
            }
            90 => {
                if let Some(index) = current_body {
                    bodies[index].1 = value.trim().parse().unwrap_or(0);
                } else {
                    operation_major = value.trim().parse().unwrap_or(0);
                }
            }
            310 => {
                if let Some(index) = current_body {
                    append_hex_bytes(&mut bodies[index].2, value);
                }
            }
            _ => {}
        }
    }
    let [(profile_type, profile_bits, profile_bytes), (path_type, path_bits, path_bytes)] = bodies;
    let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
        .unwrap_or(crate::io::dwg::DwgVersion::AC24);
    let sweep_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
        profile_type,
        profile_bits,
        profile_bytes,
        dwg_version,
        dxf_version,
    );
    let path_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
        path_type,
        path_bits,
        path_bytes,
        dwg_version,
        dxf_version,
    );
    SolidHistorySweep {
        base: dynamic_dxf_history_base(fields),
        operation_major,
        operation_minor: fields.i32(section, 91),
        direction: fields.point(section, 10),
        sweep_entity,
        path_entity,
        draft_angle: fields.f64(section, 42),
        start_draft_distance: fields.f64(section, 43),
        end_draft_distance: fields.f64(section, 44),
        scale_factor: fields.f64(section, 45),
        twist_angle: fields.f64(section, 48),
        align_angle: fields.f64(section, 49),
        sweep_entity_transform,
        path_entity_transform,
        align_option: fields.i16(section, 70).clamp(0, 255) as u8,
        miter_option: fields.i16(section, 71).clamp(0, 255) as u8,
        has_align_start: fields.bool(section, 290),
        bank: fields.bool(section, 292),
        check_intersections: fields.bool(section, 293),
        flags_294_296: [
            fields.bool(section, 294),
            fields.bool(section, 295),
            fields.bool(section, 296),
        ],
        reference_point: fields.point(section, 11),
    }
}

/// States for the mesh reading state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeshReadState {
    Properties,
    Vertices,
    Faces,
    Edges,
    Creases,
}

/// Section reader for parsing DXF sections
pub struct SectionReader<'a> {
    reader: &'a mut Box<dyn DxfStreamReader>,
    decoded_records: usize,
}

impl<'a> SectionReader<'a> {
    /// Create a new section reader
    pub fn new(reader: &'a mut Box<dyn DxfStreamReader>) -> Self {
        Self {
            reader,
            decoded_records: 0,
        }
    }

    pub fn decoded_records(&self) -> usize {
        self.decoded_records
    }

    /// Read the HEADER section
    pub fn read_header(&mut self, document: &mut CadDocument) -> Result<()> {
        let hdr = &mut document.header;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            if pair.code != 9 {
                continue;
            }

            let var_name = pair.value_string.clone();
            match var_name.as_str() {
                // ── Version / Metadata ──
                "$ACADVER" => {
                    if let Some(p) = self.reader.read_pair()? {
                        document.version = DxfVersion::from_version_string(&p.value_string);
                    }
                }
                "$ACADMAINTVER" => {
                    self.reader.read_pair()?;
                }
                "$REQUIREDVERSIONS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_int() {
                            hdr.required_versions = v;
                        }
                    }
                }
                "$DWGCODEPAGE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.code_page = p.value_string.clone();
                        // Set encoding immediately for pre-2007 files
                        if document.version < DxfVersion::AC1021 {
                            if let Some(enc) =
                                crate::io::dxf::code_page::encoding_from_code_page(&hdr.code_page)
                            {
                                self.reader.set_encoding(enc);
                            }
                        }
                    }
                }
                "$HANDSEED" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Ok(h) = u64::from_str_radix(&p.value_string, 16) {
                            hdr.handle_seed = h;
                        }
                    }
                }
                "$LASTSAVEDBY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.last_saved_by = p.value_string.clone();
                    }
                }
                "$FINGERPRINTGUID" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.fingerprint_guid = p.value_string.clone();
                    }
                }
                "$VERSIONGUID" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.version_guid = p.value_string.clone();
                    }
                }
                "$MENU" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.menu_name = p.value_string.clone();
                    }
                }
                "$PROJECTNAME" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.project_name = p.value_string.clone();
                    }
                }
                "$HYPERLINKBASE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.hyperlink_base = p.value_string.clone();
                    }
                }
                "$STYLESHEET" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.stylesheet = p.value_string.clone();
                    }
                }

                // ── Drawing Mode Booleans ──
                "$DIMASO" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.associate_dimensions = p.as_i16() == Some(1);
                    }
                }
                "$DIMSHO" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.update_dimensions_while_dragging = p.as_i16() == Some(1);
                    }
                }
                "$ORTHOMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.ortho_mode = p.as_i16() == Some(1);
                    }
                }
                "$FILLMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.fill_mode = p.as_i16() == Some(1);
                    }
                }
                "$QTEXTMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.quick_text_mode = p.as_i16() == Some(1);
                    }
                }
                "$MIRRTEXT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.mirror_text = p.as_i16() == Some(1);
                    }
                }
                "$REGENMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.regen_mode = p.as_i16() == Some(1);
                    }
                }
                "$LIMCHECK" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.limit_check = p.as_i16() == Some(1);
                    }
                }
                "$PLIMCHECK" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.paper_space_limit_check = p.as_i16() == Some(1);
                    }
                }
                "$PLINEGEN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.polyline_linetype_generation = p.as_i16() == Some(1);
                    }
                }
                "$PSLTSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.paper_space_linetype_scaling = p.as_i16() == Some(1);
                    }
                }
                "$TILEMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.show_model_space = p.as_i16() == Some(1);
                    }
                }
                "$USRTIMER" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.user_timer = p.as_i16() == Some(1);
                    }
                }
                "$WORLDVIEW" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.world_view = p.as_i16() == Some(1);
                    }
                }
                "$VISRETAIN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.retain_xref_visibility = p.as_i16() == Some(1);
                    }
                }
                "$DISPSILH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.display_silhouette = p.as_i16() == Some(1);
                    }
                }
                "$SPLFRAME" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.spline_frame = p.as_i16() == Some(1);
                    }
                }
                "$DELOBJ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.delete_objects = p.as_i16() == Some(1);
                    }
                }
                "$SOLIDHIST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.record_solid_history = p.as_i16() == Some(1);
                    }
                }
                "$SHOWHIST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.show_solid_history = v.clamp(0, 2);
                        }
                    }
                }
                "$BLIPMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.blip_mode = p.as_i16() == Some(1);
                    }
                }
                "$ATTREQ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.attribute_request = p.as_i16() == Some(1);
                    }
                }
                "$ATTDIA" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.attribute_dialog = p.as_i16() == Some(1);
                    }
                }

                // ── Drawing Mode Integers ──
                "$DRAGMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.drag_mode = v;
                        }
                    }
                }

                // ── Units ──
                "$LUNITS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.linear_unit_format = v;
                        }
                    }
                }
                "$LUPREC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.linear_unit_precision = v;
                        }
                    }
                }
                "$AUNITS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.angular_unit_format = v;
                        }
                    }
                }
                "$AUPREC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.angular_unit_precision = v;
                        }
                    }
                }
                "$INSUNITS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.insertion_units = v;
                        }
                    }
                }
                "$ATTMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.attribute_visibility = v;
                        }
                    }
                }
                "$PDMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.point_display_mode = v;
                        }
                    }
                }
                "$USERI1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.user_int1 = v;
                        }
                    }
                }
                "$USERI2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.user_int2 = v;
                        }
                    }
                }
                "$USERI3" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.user_int3 = v;
                        }
                    }
                }
                "$USERI4" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.user_int4 = v;
                        }
                    }
                }
                "$USERI5" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.user_int5 = v;
                        }
                    }
                }
                "$COORDS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.coords_mode = v;
                        }
                    }
                }
                "$OSMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i32() {
                            hdr.object_snap_mode = v;
                        }
                    }
                }
                "$PICKSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.pick_style = v;
                        }
                    }
                }
                "$SPLINETYPE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.spline_type = v;
                        }
                    }
                }
                "$SPLINESEGS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.spline_segments = v;
                        }
                    }
                }
                "$SURFU" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.surface_u_density = v;
                        }
                    }
                }
                "$SURFV" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.surface_v_density = v;
                        }
                    }
                }
                "$SURFTYPE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.surface_type = v;
                        }
                    }
                }
                "$SURFTAB1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.surface_tab1 = v;
                        }
                    }
                }
                "$SURFTAB2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.surface_tab2 = v;
                        }
                    }
                }
                "$SHADEDGE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.shade_edge = v;
                        }
                    }
                }
                "$SHADEDIF" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.shade_diffuse = v;
                        }
                    }
                }
                "$MAXACTVP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.max_active_viewports = v;
                        }
                    }
                }
                "$ISOLINES" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.isolines = v;
                        }
                    }
                }
                "$CMLJUST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.multiline_justification = v;
                        }
                    }
                }
                "$TEXTQLTY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.text_quality = v;
                        }
                    }
                }
                "$SORTENTS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.sort_entities = v;
                        }
                    }
                }
                "$INDEXCTL" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.index_control = v;
                        }
                    }
                }
                "$HIDETEXT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.hide_text = v;
                        }
                    }
                }
                "$XCLIPFRAME" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.xclip_frame = v;
                        }
                    }
                }
                "$HALOGAP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.halo_gap = v;
                        }
                    }
                }
                "$OBSCOLOR" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.obscured_color = v;
                        }
                    }
                }
                "$OBSLTYPE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.obscured_linetype = v;
                        }
                    }
                }
                "$INTERSECTIONDISPLAY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.intersection_display = v;
                        }
                    }
                }
                "$INTERSECTIONCOLOR" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.intersection_color = v;
                        }
                    }
                }
                "$DIMASSOC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dimension_associativity = v;
                        }
                    }
                }
                "$MEASUREMENT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.measurement = v;
                        }
                    }
                }
                "$PROXYGRAPHICS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.proxy_graphics = v;
                        }
                    }
                }
                "$TREEDEPTH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.tree_depth = v;
                        }
                    }
                }

                // ── Scale / Size Defaults ──
                "$LTSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.linetype_scale = v;
                        }
                    }
                }
                "$TEXTSIZE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.text_height = v;
                        }
                    }
                }
                "$TRACEWID" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.trace_width = v;
                        }
                    }
                }
                "$SKETCHINC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.sketch_increment = v;
                        }
                    }
                }
                "$SKPOLY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.sketch_type = v.clamp(0, 2);
                        }
                    }
                }
                "$SKTOLERANCE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.sketch_tolerance = if v.is_finite() {
                                v.clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                        }
                    }
                }
                "$THICKNESS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.thickness = v;
                        }
                    }
                }
                "$PDSIZE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.point_display_size = v;
                        }
                    }
                }
                "$PLINEWID" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.polyline_width = v;
                        }
                    }
                }
                "$CELTSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.current_entity_linetype_scale = v;
                        }
                    }
                }
                "$VIEWTWIST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.view_twist = v;
                        }
                    }
                }
                "$FILLETRAD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.fillet_radius = v;
                        }
                    }
                }
                "$CHAMFERA" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.chamfer_distance_a = v;
                        }
                    }
                }
                "$CHAMFERB" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.chamfer_distance_b = v;
                        }
                    }
                }
                "$CHAMFERC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.chamfer_length = v;
                        }
                    }
                }
                "$CHAMFERD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.chamfer_angle = v;
                        }
                    }
                }
                "$ANGBASE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.angle_base = v;
                        }
                    }
                }
                "$ANGDIR" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.angle_direction = v;
                        }
                    }
                }
                "$ELEVATION" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.elevation = v;
                        }
                    }
                }
                "$PELEVATION" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.paper_elevation = v;
                        }
                    }
                }
                "$FACETRES" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.facet_resolution = v;
                        }
                    }
                }
                "$CMLSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.multiline_scale = v;
                        }
                    }
                }
                "$USERR1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_real1 = v;
                        }
                    }
                }
                "$USERR2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_real2 = v;
                        }
                    }
                }
                "$USERR3" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_real3 = v;
                        }
                    }
                }
                "$USERR4" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_real4 = v;
                        }
                    }
                }
                "$USERR5" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_real5 = v;
                        }
                    }
                }
                "$PSVPSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.viewport_scale_factor = v;
                        }
                    }
                }
                "$CANNOSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_annotation_scale = p.value_string.clone();
                    }
                }
                "$CANNOSCALEVALUE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.annotation_scale_value = v;
                        }
                    }
                }
                "$SHADOWPLANELOCATION" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.shadow_plane_location = v;
                        }
                    }
                }
                "$LOFTANG1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.loft_angle1 = v;
                        }
                    }
                }
                "$LOFTANG2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.loft_angle2 = v;
                        }
                    }
                }
                "$LOFTMAG1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.loft_magnitude1 = v;
                        }
                    }
                }
                "$LOFTMAG2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.loft_magnitude2 = v;
                        }
                    }
                }
                "$LOFTPARAM" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.loft_param = v;
                        }
                    }
                }
                "$LOFTNORMALS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.loft_normals = v;
                        }
                    }
                }
                "$LATITUDE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.latitude = v;
                        }
                    }
                }
                "$LONGITUDE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.longitude = v;
                        }
                    }
                }
                "$NORTHDIRECTION" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.north_direction = v;
                        }
                    }
                }
                "$TIMEZONE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i32() {
                            hdr.timezone = v;
                        }
                    }
                }
                "$STEPSPERSEC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.steps_per_second = v;
                        }
                    }
                }
                "$STEPSIZE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.step_size = v;
                        }
                    }
                }
                "$LENSLENGTH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.lens_length = v;
                        }
                    }
                }
                "$CAMERAHEIGHT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.camera_height = v;
                        }
                    }
                }
                "$CAMERADISPLAY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.camera_display = p.as_bool() == Some(true);
                    }
                }

                // ── Current Entity Settings ──
                "$CECOLOR" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.current_entity_color = Color::from_index(v);
                        }
                    }
                }
                "$CELWEIGHT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.current_line_weight = v;
                        }
                    }
                }
                "$CEPSNTYPE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.current_plotstyle_type = v;
                        }
                    }
                }
                "$ENDCAPS" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.end_caps = v;
                        }
                    }
                }
                "$JOINSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.join_style = v;
                        }
                    }
                }
                "$LWDISPLAY" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.lineweight_display = p.as_bool() == Some(true);
                    }
                }
                "$XEDIT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.xedit = p.as_bool() == Some(true);
                    }
                }
                "$EXTNAMES" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.extended_names = p.as_bool() == Some(true);
                    }
                }
                "$PSTYLEMODE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.plotstyle_mode = p.as_bool() == Some(true);
                    }
                }
                "$OLESTARTUP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.ole_startup = p.as_bool() == Some(true);
                    }
                }

                // ── Dimension Variables ──
                "$DIMSCALE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_scale = v;
                        }
                    }
                }
                "$DIMASZ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_arrow_size = v;
                        }
                    }
                }
                "$DIMEXO" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_ext_line_offset = v;
                        }
                    }
                }
                "$DIMDLI" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_line_increment = v;
                        }
                    }
                }
                "$DIMEXE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_ext_line_extension = v;
                        }
                    }
                }
                "$DIMRND" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_rounding = v;
                        }
                    }
                }
                "$DIMDLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_line_extension = v;
                        }
                    }
                }
                "$DIMTP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_tolerance_plus = v;
                        }
                    }
                }
                "$DIMTM" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_tolerance_minus = v;
                        }
                    }
                }
                "$DIMTXT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_text_height = v;
                        }
                    }
                }
                "$DIMCEN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_center_mark = v;
                        }
                    }
                }
                "$DIMTSZ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_tick_size = v;
                        }
                    }
                }
                "$DIMALTF" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_alt_scale = v;
                        }
                    }
                }
                "$DIMLFAC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_linear_scale = v;
                        }
                    }
                }
                "$DIMTVP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_text_vertical_pos = v;
                        }
                    }
                }
                "$DIMTFAC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_tolerance_scale = v;
                        }
                    }
                }
                "$DIMGAP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_line_gap = v;
                        }
                    }
                }
                "$DIMALTRND" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.dim_alt_rounding = v;
                        }
                    }
                }
                "$DIMTOL" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_tolerance = p.as_i16() == Some(1);
                    }
                }
                "$DIMLIM" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_limits = p.as_i16() == Some(1);
                    }
                }
                "$DIMTIH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_text_inside_horizontal = p.as_i16() == Some(1);
                    }
                }
                "$DIMTOH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_text_outside_horizontal = p.as_i16() == Some(1);
                    }
                }
                "$DIMSE1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_suppress_ext1 = p.as_i16() == Some(1);
                    }
                }
                "$DIMSE2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_suppress_ext2 = p.as_i16() == Some(1);
                    }
                }
                "$DIMTAD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_text_above = v;
                        }
                    }
                }
                "$DIMZIN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_zero_suppression = v;
                        }
                    }
                }
                "$DIMAZIN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_zero_suppression = v;
                        }
                    }
                }
                "$DIMALT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_alternate_units = p.as_i16() == Some(1);
                    }
                }
                "$DIMALTD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_decimal_places = v;
                        }
                    }
                }
                "$DIMTOFL" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_force_line_inside = p.as_i16() == Some(1);
                    }
                }
                "$DIMSAH" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_separate_arrows = p.as_i16() == Some(1);
                    }
                }
                "$DIMTIX" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_force_text_inside = p.as_i16() == Some(1);
                    }
                }
                "$DIMSOXD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_suppress_outside_ext = p.as_i16() == Some(1);
                    }
                }
                "$DIMCLRD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_line_color = Color::from_index(v);
                        }
                    }
                }
                "$DIMCLRE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_ext_line_color = Color::from_index(v);
                        }
                    }
                }
                "$DIMCLRT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_text_color = Color::from_index(v);
                        }
                    }
                }
                "$DIMADEC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_angular_decimal_places = v;
                        }
                    }
                }
                "$DIMDEC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_decimal_places = v;
                        }
                    }
                }
                "$DIMTDEC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_tolerance_decimal_places = v;
                        }
                    }
                }
                "$DIMALTU" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_units_format = v;
                        }
                    }
                }
                "$DIMALTTD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_tolerance_decimal_places = v;
                        }
                    }
                }
                "$DIMAUNIT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_angular_units = v;
                        }
                    }
                }
                "$DIMFRAC" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_fraction_format = v;
                        }
                    }
                }
                "$DIMLUNIT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_linear_unit_format = v;
                        }
                    }
                }
                "$DIMDSEP" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_decimal_separator = char::from(v as u8);
                        }
                    }
                }
                "$DIMTMOVE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_text_movement = v;
                        }
                    }
                }
                "$DIMJUST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_horizontal_justification = v;
                        }
                    }
                }
                "$DIMSD1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_suppress_line1 = p.as_i16() == Some(1);
                    }
                }
                "$DIMSD2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_suppress_line2 = p.as_i16() == Some(1);
                    }
                }
                "$DIMTOLJ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_tolerance_justification = v;
                        }
                    }
                }
                "$DIMTZIN" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_tolerance_zero_suppression = v;
                        }
                    }
                }
                "$DIMALTZ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_tolerance_zero_suppression = v;
                        }
                    }
                }
                "$DIMALTTZ" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_alt_tolerance_zero_tight = v;
                        }
                    }
                }
                "$DIMATFIT" | "$DIMFIT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_fit = v;
                        }
                    }
                }
                "$DIMUPT" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_user_positioned_text = p.as_i16() == Some(1);
                    }
                }
                "$DIMPOST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_post = p.value_string.clone();
                    }
                }
                "$DIMAPOST" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_alt_post = p.value_string.clone();
                    }
                }
                "$DIMBLK" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_arrow_block = p.value_string.clone();
                    }
                }
                "$DIMBLK1" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_arrow_block1 = p.value_string.clone();
                    }
                }
                "$DIMBLK2" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_arrow_block2 = p.value_string.clone();
                    }
                }
                "$DIMLDRBLK" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.dim_leader_arrow_block = p.value_string.clone();
                    }
                }
                "$DIMLWD" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_line_weight = v;
                        }
                    }
                }
                "$DIMLWE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.dim_ext_line_weight = v;
                        }
                    }
                }

                // ── Name references ──
                "$CLAYER" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_layer_name = p.value_string.clone();
                    }
                }
                "$CELTYPE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_linetype_name = p.value_string.clone();
                    }
                }
                "$TEXTSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_text_style_name = p.value_string.clone();
                    }
                }
                "$DIMSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_dimstyle_name = p.value_string.clone();
                    }
                }
                "$CMLSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.multiline_style = p.value_string.clone();
                    }
                }
                "$CTABLESTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_table_style_name = p.value_string.clone();
                    }
                }
                "$CMLEADERSTYLE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.current_mleader_style_name = p.value_string.clone();
                    }
                }

                // ── Extents / Limits (multi-value XYZ / XY) ──
                "$INSBASE" => {
                    self.read_header_point3(&mut hdr.model_space_insertion_base)?;
                }
                "$EXTMIN" => {
                    self.read_header_point3(&mut hdr.model_space_extents_min)?;
                }
                "$EXTMAX" => {
                    self.read_header_point3(&mut hdr.model_space_extents_max)?;
                }
                "$LIMMIN" => {
                    self.read_header_point2(&mut hdr.model_space_limits_min)?;
                }
                "$LIMMAX" => {
                    self.read_header_point2(&mut hdr.model_space_limits_max)?;
                }
                "$PINSBASE" => {
                    self.read_header_point3(&mut hdr.paper_space_insertion_base)?;
                }
                "$PEXTMIN" => {
                    self.read_header_point3(&mut hdr.paper_space_extents_min)?;
                }
                "$PEXTMAX" => {
                    self.read_header_point3(&mut hdr.paper_space_extents_max)?;
                }
                "$PLIMMIN" => {
                    self.read_header_point2(&mut hdr.paper_space_limits_min)?;
                }
                "$PLIMMAX" => {
                    self.read_header_point2(&mut hdr.paper_space_limits_max)?;
                }

                // ── UCS ──
                "$UCSBASE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.ucs_base = p.value_string.clone();
                    }
                }
                "$UCSNAME" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.model_space_ucs_name = p.value_string.clone();
                    }
                }
                "$PUCSNAME" => {
                    if let Some(p) = self.reader.read_pair()? {
                        hdr.paper_space_ucs_name = p.value_string.clone();
                    }
                }
                "$UCSORG" => {
                    self.read_header_point3(&mut hdr.model_space_ucs_origin)?;
                }
                "$UCSXDIR" => {
                    self.read_header_point3(&mut hdr.model_space_ucs_x_axis)?;
                }
                "$UCSYDIR" => {
                    self.read_header_point3(&mut hdr.model_space_ucs_y_axis)?;
                }
                "$PUCSORG" => {
                    self.read_header_point3(&mut hdr.paper_space_ucs_origin)?;
                }
                "$PUCSXDIR" => {
                    self.read_header_point3(&mut hdr.paper_space_ucs_x_axis)?;
                }
                "$PUCSYDIR" => {
                    self.read_header_point3(&mut hdr.paper_space_ucs_y_axis)?;
                }
                "$UCSORTHOVIEW" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.ucs_ortho_view = v;
                        }
                    }
                }
                "$PUCSORTHOVIEW" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_i16() {
                            hdr.paper_ucs_ortho_view = v;
                        }
                    }
                }

                // ── Date / Time ──
                "$TDCREATE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.create_date_julian = v;
                        }
                    }
                }
                "$TDUCREATE" => {
                    self.reader.read_pair()?;
                } // UTC variant: skip (no field)
                "$TDUPDATE" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.update_date_julian = v;
                        }
                    }
                }
                "$TDUUPDATE" => {
                    self.reader.read_pair()?;
                }
                "$TDINDWG" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.total_editing_time = v;
                        }
                    }
                }
                "$TDUSRTIMER" => {
                    if let Some(p) = self.reader.read_pair()? {
                        if let Some(v) = p.as_double() {
                            hdr.user_elapsed_time = v;
                        }
                    }
                }

                _ => {
                    // Skip unknown header variable value(s) – consume until next code 9 or code 0
                    self.skip_header_variable()?;
                }
            }
        }

        Ok(())
    }

    /// Read a 3D point header variable (up to three successive code/value pairs: 10/20/30).
    /// Older formats (e.g. AC1009/R12) may only supply X and Y for variables like $EXTMIN/$EXTMAX.
    /// Non-coordinate pairs (code 9 = next variable name, code 0 = section end, etc.) are pushed
    /// back so the main header loop can process them normally.
    fn read_header_point3(&mut self, target: &mut Vector3) -> Result<()> {
        for _ in 0..3 {
            if let Some(p) = self.reader.read_pair()? {
                let base = p.code % 100;
                // Coordinate codes are 10–39 (X=1x, Y=2x, Z=3x); anything else belongs to the next token
                if base >= 10 && base < 40 {
                    if let Some(v) = p.as_double() {
                        if base < 20 {
                            target.x = v;
                        } else if base < 30 {
                            target.y = v;
                        } else {
                            target.z = v;
                        }
                    }
                } else {
                    self.reader.push_back(p);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Read a 2D point header variable (two successive code/value pairs: 10/20)
    fn read_header_point2(&mut self, target: &mut Vector2) -> Result<()> {
        for _ in 0..2 {
            if let Some(p) = self.reader.read_pair()? {
                if let Some(v) = p.as_double() {
                    // First value (code 10) → X, second (code 20) → Y
                    if p.code % 100 < 20 {
                        target.x = v;
                    } else {
                        target.y = v;
                    }
                }
            }
        }
        Ok(())
    }

    /// Skip an unknown header variable — consume value pairs until the next $VAR (code 9) or ENDSEC (code 0)
    fn skip_header_variable(&mut self) -> Result<()> {
        while let Some(p) = self.reader.read_pair()? {
            if p.code == 9 || p.code == 0 {
                self.reader.push_back(p);
                break;
            }
        }
        Ok(())
    }

    /// Read the CLASSES section
    pub fn read_classes(&mut self, document: &mut CadDocument) -> Result<()> {
        // Read classes until ENDSEC
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            // Classes are defined with code 0 = "CLASS"
            if pair.code == 0 && pair.value_string == "CLASS" {
                let mut class = crate::classes::DxfClass::new("", "");
                while let Some(class_pair) = self.reader.read_pair()? {
                    if class_pair.code == 0 {
                        self.reader.push_back(class_pair);
                        break;
                    }
                    match class_pair.code {
                        1 => class.dxf_name = class_pair.value_string.clone(),
                        2 => class.cpp_class_name = class_pair.value_string.clone(),
                        3 => class.application_name = class_pair.value_string.clone(),
                        90 => {
                            if let Some(v) = class_pair.as_i32() {
                                class.proxy_flags = crate::classes::ProxyFlags::from(v);
                            }
                        }
                        91 => {
                            if let Some(v) = class_pair.as_i32() {
                                class.instance_count = v;
                            }
                        }
                        280 => {
                            if let Some(v) = class_pair.as_i16() {
                                class.was_zombie = v != 0;
                            }
                        }
                        281 => {
                            if let Some(v) = class_pair.as_i16() {
                                class.is_an_entity = v != 0;
                                class.item_class_id = if v != 0 { 498 } else { 499 };
                            }
                        }
                        _ => {}
                    }
                }
                if !class.dxf_name.is_empty() {
                    document.classes.add_or_update(class);
                }
            }
        }

        Ok(())
    }

    /// Read the TABLES section
    pub fn read_tables(&mut self, document: &mut CadDocument) -> Result<()> {
        // Read tables until ENDSEC
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            // Tables start with code 0 = "TABLE"
            if pair.code == 0 && pair.value_string == "TABLE" {
                // Read table name (code 2)
                if let Some(name_pair) = self.reader.read_pair()? {
                    if name_pair.code == 2 {
                        match name_pair.value_string.as_str() {
                            "LAYER" => self.read_layer_table(document)?,
                            "LTYPE" => self.read_linetype_table(document)?,
                            "STYLE" => self.read_textstyle_table(document)?,
                            "BLOCK_RECORD" => self.read_block_record_table(document)?,
                            "DIMSTYLE" => self.read_dimstyle_table(document)?,
                            "APPID" => self.read_appid_table(document)?,
                            "VIEW" => self.read_view_table(document)?,
                            "VPORT" => self.read_vport_table(document)?,
                            "UCS" => self.read_ucs_table(document)?,
                            _ => {
                                // Skip unknown table
                                self.skip_to_endtab()?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Read the BLOCKS section
    pub fn read_blocks(&mut self, document: &mut CadDocument) -> Result<()> {
        // Read blocks until ENDSEC
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            // Blocks start with code 0 = "BLOCK"
            if pair.code == 0 && pair.value_string == "BLOCK" {
                if self.read_block(document)? {
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }

        Ok(())
    }

    /// Read a single BLOCK...ENDBLK definition
    fn read_block(&mut self, document: &mut CadDocument) -> Result<bool> {
        use crate::entities::Block;
        use crate::types::Vector3;

        let mut block_name = String::new();
        let mut alternate_block_name = String::new();
        let mut base_point = Vector3::new(0.0, 0.0, 0.0);
        let mut description = String::new();
        let mut xref_path = String::new();
        let mut layer = String::from("0");
        let mut handle = Handle::NULL;
        // BLOCK entity code 70 = the authoritative block-type flags
        // (1=anonymous, 2=has-attributes, 4=xref, 8=xref-overlay).
        let mut block_flags: i16 = 0;

        let mut point_reader = PointReader::new();

        // Read BLOCK entity properties
        while let Some(pair) = self.reader.read_pair()? {
            match pair.code {
                0 => {
                    // Start of next entity - put it back and break
                    self.reader.push_back(pair);
                    break;
                }
                2 => {
                    // Block name
                    block_name = pair.value_string.clone();
                }
                3 => {
                    // Block name (alternate)
                    alternate_block_name = pair.value_string.clone();
                }
                4 => {
                    // Description
                    description = pair.value_string.clone();
                }
                1 => {
                    // XRef path
                    xref_path = pair.value_string.clone();
                }
                5 => {
                    // Handle
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        handle = Handle::new(h);
                    }
                }
                8 => {
                    // Layer
                    layer = pair.value_string.clone();
                }
                10 | 20 | 30 => {
                    // Base point coordinates
                    point_reader.add_coordinate(&pair);
                    if let Some(pt) = point_reader.get_point() {
                        base_point = pt;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        block_flags = v;
                    }
                }
                _ => {}
            }
        }

        // Some legacy DXF producers use `$MODEL_SPACE` / `$PAPER_SPACE` as
        // the BLOCK marker name (code 2), while the BLOCK_RECORD table and
        // alternate name (code 3) use the canonical `*...` spelling. Resolve
        // only those aliases so the marker updates the existing space record
        // instead of creating a duplicate block with a null marker handle.
        if block_name.is_empty() {
            block_name = alternate_block_name.clone();
        }
        let canonical_space_name = if block_name.eq_ignore_ascii_case("$MODEL_SPACE") {
            Some("*Model_Space")
        } else if block_name.eq_ignore_ascii_case("$PAPER_SPACE") {
            Some("*Paper_Space")
        } else {
            None
        };
        if let Some(canonical_name) = canonical_space_name {
            let alternate_matches = !alternate_block_name.is_empty()
                && alternate_block_name.eq_ignore_ascii_case(canonical_name)
                && document.block_records.get(&alternate_block_name).is_some();
            if alternate_matches {
                block_name = alternate_block_name;
            } else if document.block_records.get(canonical_name).is_some() {
                block_name = canonical_name.to_string();
            }
        }

        // Create Block entity
        let mut block = Block::new(block_name.clone(), base_point);
        block.common.handle = handle;
        block.common.layer = layer.clone();
        block.description = description;
        block.xref_path = xref_path;

        // Find the corresponding BlockRecord and add entities to it
        let mut block_entities: Vec<EntityType> = Vec::new();
        let mut committed = false;

        // Read entities until ENDBLK
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                match pair.value_string.as_str() {
                    "ENDBLK" => {
                        // Read ENDBLK properties
                        let block_end = self.read_block_end()?;

                        // Insert block entities into the document's flat entity map
                        // and collect their handles for the block record.
                        let mut entity_handles = Vec::with_capacity(block_entities.len());
                        for mut entity in block_entities {
                            let h = if entity.common().handle.is_null() {
                                let new_h = document.allocate_handle();
                                entity.as_entity_mut().set_handle(new_h);
                                new_h
                            } else {
                                entity.common().handle
                            };
                            entity_handles.push(h);
                            let idx = document.entities.len();
                            document.entities.push(std::sync::Arc::new(entity));
                            document.entity_index.insert(h, idx);
                        }

                        // Find the BlockRecord and set handles
                        if document.block_records.get(&block_name).is_none() {
                            let mut br = BlockRecord::new(block_name.clone());
                            br.handle = document.allocate_handle();
                            document.block_records.add_or_replace(br);
                        }

                        if let Some(block_record) = document.block_records.get_mut(&block_name) {
                            block_record.entity_handles = entity_handles;
                            block_record.xref_path = block.xref_path.clone();
                            // Block-type flags come from the BLOCK entity's
                            // code 70 (the BLOCK_RECORD's code 70 is units).
                            block_record.flags.anonymous = (block_flags & 1) != 0;
                            block_record.flags.has_attributes = (block_flags & 2) != 0;
                            block_record.flags.is_xref = (block_flags & 4) != 0;
                            block_record.flags.is_xref_overlay = (block_flags & 8) != 0;
                            // The BLOCKS section is authoritative for marker
                            // identities. Keep missing source handles null so
                            // the post-read pass can assign fresh handles after
                            // it has seen every identity in the file.
                            block_record.block_entity_handle = handle;
                            block_record.block_end_handle = block_end.common.handle;
                        }

                        // Note: Block and BlockEnd are block definition markers, not drawing entities.
                        // They are not added to the document's main entity list.
                        // The block content is stored in the BlockRecord.

                        committed = true;
                        break;
                    }
                    "POINT" => {
                        if let Some(entity) = self.read_point()? {
                            block_entities.push(EntityType::Point(entity));
                        }
                    }
                    "LINE" | "3DLINE" => {
                        if let Some(entity) = self.read_line()? {
                            block_entities.push(EntityType::Line(entity));
                        }
                    }
                    "CIRCLE" => {
                        if let Some(entity) = self.read_circle()? {
                            block_entities.push(EntityType::Circle(entity));
                        }
                    }
                    "ARC" => {
                        if let Some(entity) = self.read_arc()? {
                            block_entities.push(EntityType::Arc(entity));
                        }
                    }
                    "ELLIPSE" => {
                        if let Some(entity) = self.read_ellipse()? {
                            block_entities.push(EntityType::Ellipse(entity));
                        }
                    }
                    "POLYLINE" => {
                        if let Some(entity) = self.read_polyline_entity()? {
                            block_entities.push(entity);
                        }
                    }
                    "LWPOLYLINE" => {
                        if let Some(entity) = self.read_lwpolyline()? {
                            block_entities.push(EntityType::LwPolyline(entity));
                        }
                    }
                    "TEXT" => {
                        if let Some(entity) = self.read_text()? {
                            block_entities.push(EntityType::Text(entity));
                        }
                    }
                    "MTEXT" => {
                        if let Some(entity) = self.read_mtext()? {
                            block_entities.push(EntityType::MText(entity));
                        }
                    }
                    "SPLINE" => {
                        if let Some(entity) = self.read_spline()? {
                            block_entities.push(EntityType::Spline(entity));
                        }
                    }
                    "HELIX" => {
                        if let Some(entity) = self.read_helix()? {
                            block_entities.push(EntityType::Helix(entity));
                        }
                    }
                    "DIMENSION" => {
                        if let Some(entity) = self.read_dimension()? {
                            block_entities.push(EntityType::Dimension(entity));
                        }
                    }
                    "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => {
                        if let Some(entity) = self.read_extended_dimension(&pair.value_string)? {
                            block_entities.push(EntityType::Dimension(entity));
                        }
                    }
                    "HATCH" => {
                        if let Some(entity) = self.read_hatch()? {
                            block_entities.push(EntityType::Hatch(entity));
                        }
                    }
                    "MPOLYGON" => {
                        if let Some(entity) = self.read_mpolygon()? {
                            block_entities.push(EntityType::Hatch(entity));
                        }
                    }
                    "SOLID" => {
                        if let Some(entity) = self.read_solid()? {
                            block_entities.push(EntityType::Solid(entity));
                        }
                    }
                    "TRACE" => {
                        if let Some(mut entity) = self.read_solid()? {
                            entity.is_trace = true;
                            block_entities.push(EntityType::Solid(entity));
                        }
                    }
                    "3DFACE" => {
                        if let Some(entity) = self.read_face3d()? {
                            block_entities.push(EntityType::Face3D(entity));
                        }
                    }
                    "INSERT" | "ACDBVIEWREPBLOCKREFERENCE" => {
                        if let Some(entity) = self.read_insert()? {
                            block_entities.push(EntityType::Insert(entity));
                        }
                    }
                    "RAY" => {
                        if let Some(entity) = self.read_ray()? {
                            block_entities.push(EntityType::Ray(entity));
                        }
                    }
                    "XLINE" => {
                        if let Some(entity) = self.read_xline()? {
                            block_entities.push(EntityType::XLine(entity));
                        }
                    }
                    "ATTDEF" => {
                        if let Some(entity) = self.read_attdef()? {
                            block_entities.push(EntityType::AttributeDefinition(entity));
                        }
                    }
                    "ATTRIB" => {
                        if let Some(entity) = self.read_attrib()? {
                            block_entities.push(EntityType::AttributeEntity(entity));
                        }
                    }
                    "TOLERANCE" => {
                        if let Some(entity) = self.read_tolerance()? {
                            block_entities.push(EntityType::Tolerance(entity));
                        }
                    }
                    "SHAPE" => {
                        if let Some(entity) = self.read_shape()? {
                            block_entities.push(EntityType::Shape(entity));
                        }
                    }
                    "WIPEOUT" => {
                        if let Some(entity) = self.read_wipeout()? {
                            block_entities.push(EntityType::Wipeout(entity));
                        }
                    }
                    "VIEWPORT" => {
                        if let Some(entity) = self.read_viewport()? {
                            block_entities.push(EntityType::Viewport(entity));
                        }
                    }
                    "LEADER" => {
                        if let Some(entity) = self.read_leader()? {
                            block_entities.push(EntityType::Leader(entity));
                        }
                    }
                    "MULTILEADER" | "MLEADER" => {
                        if let Some(entity) = self.read_multileader()? {
                            block_entities.push(EntityType::MultiLeader(entity));
                        }
                    }
                    "MLINE" => {
                        if let Some(entity) = self.read_mline()? {
                            block_entities.push(EntityType::MLine(entity));
                        }
                    }
                    "MESH" => {
                        if let Some(entity) = self.read_mesh()? {
                            block_entities.push(EntityType::Mesh(entity));
                        }
                    }
                    "IMAGE" => {
                        if let Some(entity) = self.read_raster_image()? {
                            block_entities.push(EntityType::RasterImage(entity));
                        }
                    }
                    "3DSOLID" => {
                        if let Some(entity) = self.read_solid3d()? {
                            block_entities.push(EntityType::Solid3D(entity));
                        }
                    }
                    "REGION" => {
                        if let Some(entity) = self.read_region()? {
                            block_entities.push(EntityType::Region(entity));
                        }
                    }
                    "BODY" => {
                        if let Some(entity) = self.read_body()? {
                            block_entities.push(EntityType::Body(entity));
                        }
                    }
                    "ACAD_TABLE" | "TABLE" => {
                        if let Some(entity) = self.read_table_entity()? {
                            block_entities.push(EntityType::Table(entity));
                        }
                    }
                    "PDFUNDERLAY" | "DWFUNDERLAY" | "DGNUNDERLAY" => {
                        if let Some(entity) = self.read_underlay(&pair.value_string)? {
                            block_entities.push(EntityType::Underlay(entity));
                        }
                    }
                    "OLE2FRAME" => {
                        if let Some(entity) = self.read_ole2frame()? {
                            block_entities.push(EntityType::Ole2Frame(entity));
                        }
                    }
                    "SECTIONLINE" => {
                        let entity = self.read_section_symbol_dxf()?;
                        block_entities.push(EntityType::SectionSymbol(entity));
                    }
                    "DRAWINGVIEW" => {
                        let entity = self.read_view_border_dxf()?;
                        block_entities.push(EntityType::ViewBorder(entity));
                    }
                    "CAMERA"
                    | "SECTIONOBJECT"
                    | "ARCALIGNEDTEXT"
                    | "RTEXT"
                    | "POSITIONMARKER"
                    | "GEOPOSITIONMARKER"
                    | "COORDINATION_MODEL"
                    | "NAVISWORKSMODEL"
                    | "ACDBPOINTCLOUD"
                    | "POINTCLOUD"
                    | "ACDBPOINTCLOUDEX"
                    | "POINTCLOUDEX"
                    | "ACAD_PROXY_ENTITY"
                    | "OLEFRAME"
                    | "LAYOUTPRINTCONFIG"
                    | "FORMAT"
                    | "Format"
                    | "REPEAT"
                    | "ENDREP"
                    | "LOAD"
                    | "JUMP"
                    | "ALIGNMENTPARAMETERENTITY"
                    | "BASEPOINTPARAMETERENTITY"
                    | "FLIPPARAMETERENTITY"
                    | "LINEARPARAMETERENTITY"
                    | "POINTPARAMETERENTITY"
                    | "ROTATIONPARAMETERENTITY"
                    | "VISIBILITYPARAMETERENTITY"
                    | "FLIPGRIPENTITY"
                    | "LINEARGRIPENTITY"
                    | "POLARGRIPENTITY"
                    | "ROTATIONGRIPENTITY"
                    | "VISIBILITYGRIPENTITY"
                    | "XYGRIPENTITY"
                    | "BLOCKANGULARCONSTRAINTPARAMETERENTITY"
                    | "XYPARAMETERENTITY" => {
                        if let Some(entity) = self.read_extended_entity(&pair.value_string)? {
                            block_entities.push(EntityType::Extended(entity));
                        }
                    }
                    "SEQEND" => {
                        // Skip SEQEND in blocks — it's consumed by polyline/insert readers
                        self.skip_entity()?;
                    }
                    _ => {
                        // Read as unknown entity in block — common fields preserved
                        let entity = self.read_unknown_entity(&pair.value_string)?;
                        block_entities.push(EntityType::Unknown(entity));
                    }
                }
            }
        }

        Ok(committed)
    }

    /// Read ENDBLK entity
    fn read_block_end(&mut self) -> Result<BlockEnd> {
        use crate::entities::BlockEnd;

        let mut block_end = BlockEnd::new();
        let mut layer = String::from("0");
        let mut handle = Handle::NULL;

        while let Some(pair) = self.reader.read_pair()? {
            match pair.code {
                0 => {
                    // Next entity - push back and break
                    self.reader.push_back(pair);
                    break;
                }
                5 => {
                    // Handle
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        handle = Handle::new(h);
                    }
                }
                8 => {
                    // Layer
                    layer = pair.value_string.clone();
                }
                _ => {}
            }
        }

        block_end.common.handle = handle;
        block_end.common.layer = layer;

        Ok(block_end)
    }

    /// Read the ENTITIES section
    pub fn read_entities(&mut self, document: &mut CadDocument) -> Result<()> {
        // Read entities until ENDSEC
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            // Entities start with code 0
            if pair.code == 0 {
                let entity_type = pair.value_string.clone();
                let before = document.entities().count();

                match entity_type.as_str() {
                    "POINT" => {
                        if let Some(entity) = self.read_point()? {
                            let _ = document.add_entity(EntityType::Point(entity));
                        }
                    }
                    "LINE" | "3DLINE" => {
                        if let Some(entity) = self.read_line()? {
                            let _ = document.add_entity(EntityType::Line(entity));
                        }
                    }
                    "CIRCLE" => {
                        if let Some(entity) = self.read_circle()? {
                            let _ = document.add_entity(EntityType::Circle(entity));
                        }
                    }
                    "ARC" => {
                        if let Some(entity) = self.read_arc()? {
                            let _ = document.add_entity(EntityType::Arc(entity));
                        }
                    }
                    "ELLIPSE" => {
                        if let Some(entity) = self.read_ellipse()? {
                            let _ = document.add_entity(EntityType::Ellipse(entity));
                        }
                    }
                    "POLYLINE" => {
                        if let Some(entity) = self.read_polyline_entity()? {
                            let _ = document.add_entity(entity);
                        }
                    }
                    "LWPOLYLINE" => {
                        if let Some(entity) = self.read_lwpolyline()? {
                            let _ = document.add_entity(EntityType::LwPolyline(entity));
                        }
                    }
                    "TEXT" => {
                        if let Some(entity) = self.read_text()? {
                            let _ = document.add_entity(EntityType::Text(entity));
                        }
                    }
                    "MTEXT" => {
                        if let Some(entity) = self.read_mtext()? {
                            let _ = document.add_entity(EntityType::MText(entity));
                        }
                    }
                    "SPLINE" => {
                        if let Some(entity) = self.read_spline()? {
                            let _ = document.add_entity(EntityType::Spline(entity));
                        }
                    }
                    "HELIX" => {
                        if let Some(entity) = self.read_helix()? {
                            let _ = document.add_entity(EntityType::Helix(entity));
                        }
                    }
                    "DIMENSION" => {
                        if let Some(entity) = self.read_dimension()? {
                            let _ = document.add_entity(EntityType::Dimension(entity));
                        }
                    }
                    "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => {
                        if let Some(entity) = self.read_extended_dimension(&entity_type)? {
                            let _ = document.add_entity(EntityType::Dimension(entity));
                        }
                    }
                    "HATCH" => {
                        if let Some(entity) = self.read_hatch()? {
                            let _ = document.add_entity(EntityType::Hatch(entity));
                        }
                    }
                    "MPOLYGON" => {
                        if let Some(entity) = self.read_mpolygon()? {
                            let _ = document.add_entity(EntityType::Hatch(entity));
                        }
                    }
                    "SOLID" => {
                        if let Some(entity) = self.read_solid()? {
                            let _ = document.add_entity(EntityType::Solid(entity));
                        }
                    }
                    "TRACE" => {
                        if let Some(mut entity) = self.read_solid()? {
                            entity.is_trace = true;
                            let _ = document.add_entity(EntityType::Solid(entity));
                        }
                    }
                    "3DFACE" => {
                        if let Some(entity) = self.read_face3d()? {
                            let _ = document.add_entity(EntityType::Face3D(entity));
                        }
                    }
                    "INSERT" | "ACDBVIEWREPBLOCKREFERENCE" => {
                        if let Some(entity) = self.read_insert()? {
                            let _ = document.add_entity(EntityType::Insert(entity));
                        }
                    }
                    "RAY" => {
                        if let Some(entity) = self.read_ray()? {
                            let _ = document.add_entity(EntityType::Ray(entity));
                        }
                    }
                    "XLINE" => {
                        if let Some(entity) = self.read_xline()? {
                            let _ = document.add_entity(EntityType::XLine(entity));
                        }
                    }
                    "ATTDEF" => {
                        if let Some(entity) = self.read_attdef()? {
                            let _ = document.add_entity(EntityType::AttributeDefinition(entity));
                        }
                    }
                    "TOLERANCE" => {
                        if let Some(entity) = self.read_tolerance()? {
                            let _ = document.add_entity(EntityType::Tolerance(entity));
                        }
                    }
                    "SHAPE" => {
                        if let Some(entity) = self.read_shape()? {
                            let _ = document.add_entity(EntityType::Shape(entity));
                        }
                    }
                    "WIPEOUT" => {
                        if let Some(entity) = self.read_wipeout()? {
                            let _ = document.add_entity(EntityType::Wipeout(entity));
                        }
                    }
                    "VIEWPORT" => {
                        if let Some(entity) = self.read_viewport()? {
                            let _ = document.add_entity(EntityType::Viewport(entity));
                        }
                    }
                    "ATTRIB" => {
                        if let Some(entity) = self.read_attrib()? {
                            let _ = document.add_entity(EntityType::AttributeEntity(entity));
                        }
                    }
                    "LEADER" => {
                        if let Some(entity) = self.read_leader()? {
                            let _ = document.add_entity(EntityType::Leader(entity));
                        }
                    }
                    "MULTILEADER" | "MLEADER" => {
                        if let Some(entity) = self.read_multileader()? {
                            let _ = document.add_entity(EntityType::MultiLeader(entity));
                        }
                    }
                    "MLINE" => {
                        if let Some(entity) = self.read_mline()? {
                            let _ = document.add_entity(EntityType::MLine(entity));
                        }
                    }
                    "MESH" => {
                        if let Some(entity) = self.read_mesh()? {
                            let _ = document.add_entity(EntityType::Mesh(entity));
                        }
                    }
                    "IMAGE" => {
                        if let Some(entity) = self.read_raster_image()? {
                            let _ = document.add_entity(EntityType::RasterImage(entity));
                        }
                    }
                    "3DSOLID" => {
                        if let Some(entity) = self.read_solid3d()? {
                            let _ = document.add_entity(EntityType::Solid3D(entity));
                        }
                    }
                    "REGION" => {
                        if let Some(entity) = self.read_region()? {
                            let _ = document.add_entity(EntityType::Region(entity));
                        }
                    }
                    "BODY" => {
                        if let Some(entity) = self.read_body()? {
                            let _ = document.add_entity(EntityType::Body(entity));
                        }
                    }
                    "LIGHT" => {
                        if let Some(entity) = self.read_light_entity()? {
                            let _ = document.add_entity(EntityType::Light(entity));
                        }
                    }
                    "SURFACE" | "PLANESURFACE" | "EXTRUDEDSURFACE" | "LOFTEDSURFACE"
                    | "REVOLVEDSURFACE" | "SWEPTSURFACE" | "NURBSURFACE" => {
                        if let Some(entity) =
                            self.read_surface_entity(&entity_type, document.version)?
                        {
                            let _ = document.add_entity(EntityType::Surface(entity));
                        }
                    }
                    "ACAD_TABLE" | "TABLE" => {
                        if let Some(entity) = self.read_table_entity()? {
                            let _ = document.add_entity(EntityType::Table(entity));
                        }
                    }
                    "PDFUNDERLAY" | "DWFUNDERLAY" | "DGNUNDERLAY" => {
                        if let Some(entity) = self.read_underlay(&entity_type)? {
                            let _ = document.add_entity(EntityType::Underlay(entity));
                        }
                    }
                    "OLE2FRAME" => {
                        if let Some(entity) = self.read_ole2frame()? {
                            let _ = document.add_entity(EntityType::Ole2Frame(entity));
                        }
                    }
                    "CAMERA"
                    | "SECTIONOBJECT"
                    | "ARCALIGNEDTEXT"
                    | "RTEXT"
                    | "POSITIONMARKER"
                    | "GEOPOSITIONMARKER"
                    | "COORDINATION_MODEL"
                    | "NAVISWORKSMODEL"
                    | "ACDBPOINTCLOUD"
                    | "POINTCLOUD"
                    | "ACDBPOINTCLOUDEX"
                    | "POINTCLOUDEX"
                    | "ACAD_PROXY_ENTITY"
                    | "OLEFRAME"
                    | "LAYOUTPRINTCONFIG"
                    | "FORMAT"
                    | "Format"
                    | "REPEAT"
                    | "ENDREP"
                    | "LOAD"
                    | "JUMP"
                    | "ALIGNMENTPARAMETERENTITY"
                    | "BASEPOINTPARAMETERENTITY"
                    | "FLIPPARAMETERENTITY"
                    | "LINEARPARAMETERENTITY"
                    | "POINTPARAMETERENTITY"
                    | "ROTATIONPARAMETERENTITY"
                    | "VISIBILITYPARAMETERENTITY"
                    | "FLIPGRIPENTITY"
                    | "LINEARGRIPENTITY"
                    | "POLARGRIPENTITY"
                    | "ROTATIONGRIPENTITY"
                    | "VISIBILITYGRIPENTITY"
                    | "XYGRIPENTITY"
                    | "BLOCKANGULARCONSTRAINTPARAMETERENTITY"
                    | "XYPARAMETERENTITY" => {
                        if let Some(entity) = self.read_extended_entity(&entity_type)? {
                            let _ = document.add_entity(EntityType::Extended(entity));
                        }
                    }
                    name if is_registered_class_entity_name(name) => {
                        let entity = self.read_registered_class_entity(name)?;
                        let _ = document.add_entity(EntityType::Extended(entity));
                    }
                    "SEQEND" => {
                        // Standalone SEQEND — skip (normally consumed by polyline/insert reader)
                        self.skip_entity()?;
                    }
                    _ => {
                        // Read as unknown entity — common fields preserved, entity-specific codes discarded
                        document.notifications.notify(
                            crate::notification::NotificationType::NotImplemented,
                            format!(
                                "Entity not supported, read as UnknownEntity: {}",
                                entity_type
                            ),
                        );
                        let entity = self.read_unknown_entity(&entity_type)?;
                        let _ = document.add_entity(EntityType::Unknown(entity));
                    }
                }
                self.decoded_records = self
                    .decoded_records
                    .saturating_add(document.entities().count().saturating_sub(before));
            }
        }

        Ok(())
    }

    fn read_dynamic_block_object_dxf(
        &mut self,
        dxf_name: &str,
        dxf_version: DxfVersion,
    ) -> Result<DynamicBlockObject> {
        let mut object = DynamicBlockObject::new(dxf_name, dynamic_block_cpp_name(dxf_name));
        let mut fields = DynamicDxfFields::default();
        let mut section = String::new();
        let mut group = String::new();
        let mut owner_seen = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => object.handle = parse_dxf_handle(&pair.value_string),
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    object.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    object.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && section.is_empty() => {
                    object.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                100 => section = pair.value_string.clone(),
                _ => fields
                    .sections
                    .entry(section.clone())
                    .or_default()
                    .push((pair.code, pair.value_string.clone())),
            }
            if group == "}" {
                group.clear();
            }
        }

        object.data = match dxf_name {
            "ACSH_HISTORY_CLASS" => {
                let section = "AcDbShHistory";
                DynamicBlockData::SolidHistory(SolidHistory {
                    major: fields.i32(section, 90),
                    minor: fields.i32(section, 91),
                    owner: fields.handle(section, 360),
                    history_node_id: fields.i32(section, 92),
                    show_history: fields.bool(section, 280),
                    record_history: fields.bool(section, 281),
                })
            }
            "ACSH_BOX_CLASS" | "ACSH_WEDGE_CLASS" => {
                let section = if dxf_name == "ACSH_BOX_CLASS" {
                    "AcDbShBox"
                } else {
                    "AcDbShWedge"
                };
                let value = SolidHistoryBox {
                    base: dynamic_dxf_history_base(&fields),
                    operation_major: fields.i32(section, 90),
                    operation_minor: fields.i32(section, 91),
                    length: fields.f64(section, 40),
                    width: fields.f64(section, 41),
                    height: fields.f64(section, 42),
                };
                DynamicBlockData::SolidHistoryNode(if dxf_name == "ACSH_BOX_CLASS" {
                    SolidHistoryOperation::Box(value)
                } else {
                    SolidHistoryOperation::Wedge(value)
                })
            }
            "ACSH_SPHERE_CLASS" => {
                let section = "AcDbShSphere";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Sphere(
                    SolidHistorySphere {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        radius: fields.f64(section, 40),
                    },
                ))
            }
            "ACSH_CYLINDER_CLASS" => {
                let section = "AcDbShCylinder";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Cylinder(
                    SolidHistoryCylinder {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        height: fields.f64(section, 40),
                        major_radius: fields.f64(section, 41),
                        minor_radius: fields.f64(section, 42),
                        x_radius: fields.f64(section, 43),
                    },
                ))
            }
            "ACSH_CONE_CLASS" => {
                let section = "AcDbShCone";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Cone(SolidHistoryCone {
                    base: dynamic_dxf_history_base(&fields),
                    operation_major: fields.i32(section, 90),
                    operation_minor: fields.i32(section, 91),
                    height: fields.f64(section, 40),
                    base_x_radius: fields.f64(section, 41),
                    base_y_radius: fields.f64(section, 42),
                    top_radius: fields.f64(section, 43),
                }))
            }
            "ACSH_PYRAMID_CLASS" => {
                let section = "AcDbShPyramid";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Pyramid(
                    SolidHistoryPyramid {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        height: fields.f64(section, 40),
                        sides: fields.i32(section, 92),
                        radius: fields.f64(section, 41),
                        top_radius: fields.f64(section, 42),
                    },
                ))
            }
            "ACSH_TORUS_CLASS" => {
                let section = "AcDbShTorus";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Torus(
                    SolidHistoryTorus {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        major_radius: fields.f64(section, 40),
                        minor_radius: fields.f64(section, 41),
                    },
                ))
            }
            "ACSH_BOOLEAN_CLASS" => {
                let section = "AcDbShBoolean";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Boolean(
                    SolidHistoryBoolean {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        operation: fields.i16(section, 280).clamp(0, 255) as u8,
                        first_operand: fields.i32(section, 92),
                        second_operand: fields.i32(section, 93),
                    },
                ))
            }
            "ACSH_CHAMFER_CLASS" => {
                let section = "AcDbShChamfer";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Chamfer(
                    SolidHistoryChamfer {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        method: fields.i32(section, 92),
                        base_distance: fields.f64(section, 41),
                        other_distance: fields.f64(section, 42),
                        edges: fields
                            .values(section, 94)
                            .into_iter()
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                        base_face: fields.i32(section, 95),
                    },
                ))
            }
            "ACSH_FILLET_CLASS" => {
                let section = "AcDbShFillet";
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Fillet(
                    SolidHistoryFillet {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        method: fields.i32(section, 92),
                        edges: fields
                            .values(section, 94)
                            .into_iter()
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                        radii: fields
                            .values(section, 41)
                            .into_iter()
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                        start_setbacks: fields
                            .values(section, 42)
                            .into_iter()
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                        end_setbacks: fields
                            .values(section, 43)
                            .into_iter()
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                    },
                ))
            }
            "ACSH_BREP_CLASS" => {
                let section = "AcDbShBrep";
                let mut acis_data = AcisData::new();
                let modeler = "AcDbModelerGeometry";
                let mut text = String::new();
                for value in fields.values(modeler, 1) {
                    text.push_str(value);
                    text.push('\n');
                }
                for value in fields.values(modeler, 3) {
                    text.push_str(value);
                }
                let version = fields.i16(modeler, 70);
                if version == 1 {
                    text = AcisData::decode_sat(&text);
                }
                acis_data.sat_data = AcisData::strip_sat_terminator(&text);
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Brep(SolidHistoryBrep {
                    base: dynamic_dxf_history_base(&fields),
                    operation_major: fields.i32(section, 90),
                    operation_minor: fields.i32(section, 91),
                    acis_data,
                }))
            }
            "ACSH_SWEEP_CLASS" => DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Sweep(
                dynamic_dxf_history_sweep(&fields, dxf_version),
            )),
            "ACSH_EXTRUSION_CLASS" => DynamicBlockData::SolidHistoryNode(
                SolidHistoryOperation::Extrusion(dynamic_dxf_history_sweep(&fields, dxf_version)),
            ),
            "ACSH_LOFT_CLASS" => {
                let section = "AcDbShLoft";
                let mut binary = Vec::new();
                for value in fields.values(section, 310) {
                    append_hex_bytes(&mut binary, value);
                }
                let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                    .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                let mut offset = 0usize;
                let mut decode_list = |type_code: i32, bit_length: usize| {
                    let byte_length = bit_length.div_ceil(8);
                    let end = offset.saturating_add(byte_length).min(binary.len());
                    let bytes = binary[offset..end].to_vec();
                    offset = end;
                    crate::io::dwg::embedded_entity::decode_embedded_entity(
                        type_code,
                        bit_length,
                        bytes,
                        dwg_version,
                        dxf_version,
                    )
                };
                let cross_types = fields.values(section, 93);
                let cross_sizes = fields.values(section, 94);
                let mut cross_sections = Vec::with_capacity(cross_types.len());
                for (index, type_value) in cross_types.iter().enumerate() {
                    let entity_type = type_value.trim().parse().unwrap_or(0);
                    let bit_length = cross_sizes
                        .get(index)
                        .and_then(|value| value.trim().parse().ok())
                        .unwrap_or(0);
                    if let Some(entity) = decode_list(entity_type, bit_length) {
                        cross_sections.push(entity);
                    }
                }
                let guide_types = fields.values(section, 96);
                let guide_sizes = fields.values(section, 97);
                let mut guides = Vec::with_capacity(guide_types.len());
                for (index, type_value) in guide_types.iter().enumerate() {
                    let entity_type = type_value.trim().parse().unwrap_or(0);
                    let bit_length = guide_sizes
                        .get(index)
                        .and_then(|value| value.trim().parse().ok())
                        .unwrap_or(0);
                    if let Some(entity) = decode_list(entity_type, bit_length) {
                        guides.push(entity);
                    }
                }
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Loft(SolidHistoryLoft {
                    base: dynamic_dxf_history_base(&fields),
                    operation_major: fields.i32(section, 90),
                    operation_minor: fields.i32(section, 91),
                    cross_sections,
                    guides,
                    ..Default::default()
                }))
            }
            "ACSH_REVOLVE_CLASS" => {
                let section = "AcDbShRevolve";
                let values_90 = fields.values(section, 90);
                let entity_type = values_90
                    .get(1)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let bit_length = values_90
                    .get(2)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let mut binary = Vec::new();
                for value in fields.values(section, 310) {
                    append_hex_bytes(&mut binary, value);
                }
                let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                    .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                let sweep_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
                    entity_type,
                    bit_length,
                    binary,
                    dwg_version,
                    dxf_version,
                );
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Revolve(
                    SolidHistoryRevolve {
                        base: dynamic_dxf_history_base(&fields),
                        operation_major: fields.i32(section, 90),
                        operation_minor: fields.i32(section, 91),
                        axis_point: fields.point(section, 10),
                        direction: fields.point(section, 11),
                        revolve_angle: fields.f64(section, 40),
                        start_angle: fields.f64(section, 41),
                        draft_angle: fields.f64(section, 43),
                        field_44: fields.f64(section, 44),
                        field_45: fields.f64(section, 45),
                        twist_angle: fields.f64(section, 46),
                        flag_290: fields.bool(section, 290),
                        close_to_axis: fields.bool(section, 291),
                        sweep_entity,
                    },
                ))
            }
            "ACDB_BLOCKREPRESENTATION_DATA" => {
                DynamicBlockData::Representation(BlockRepresentationData {
                    flags: fields.i16("AcDbBlockRepresentationData", 70),
                    block: fields.handle("AcDbBlockRepresentationData", 340),
                })
            }
            "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION" => {
                DynamicBlockData::Representation(BlockRepresentationData {
                    flags: fields.i16("AcDbDynamicBlockPurgePreventer", 70),
                    block: fields.handle("AcDbDynamicBlockPurgePreventer", 340),
                })
            }
            "ACDB_DYNAMICBLOCKPROXYNODE" => DynamicBlockData::ProxyNode(dynamic_dxf_eval(&fields)),
            "BLOCKGRIPLOCATIONCOMPONENT" => {
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: dynamic_dxf_eval(&fields),
                    grip_type: fields.i32("AcDbBlockGripExpr", 91),
                    expression: fields.text("AcDbBlockGripExpr", 300),
                })
            }
            "BLOCKALIGNMENTGRIP" => DynamicBlockData::AlignmentGrip(BlockOrientedGrip {
                grip: dynamic_dxf_grip(&fields),
                orientation: fields.point("AcDbBlockAlignmentGrip", 140),
            }),
            "BLOCKFLIPGRIP" => DynamicBlockData::FlipGrip(BlockFlipGrip {
                grip: dynamic_dxf_grip(&fields),
                combined_state: fields.i32("AcDbBlockFlipGrip", 93),
                orientation: fields.point("AcDbBlockFlipGrip", 140),
            }),
            "BLOCKLINEARGRIP" => DynamicBlockData::LinearGrip(BlockOrientedGrip {
                grip: dynamic_dxf_grip(&fields),
                orientation: fields.point("AcDbBlockLinearGrip", 140),
            }),
            "BLOCKLOOKUPGRIP" => DynamicBlockData::LookupGrip(dynamic_dxf_grip(&fields)),
            "BLOCKPOLARGRIP" => DynamicBlockData::PolarGrip(dynamic_dxf_grip(&fields)),
            "BLOCKROTATIONGRIP" => DynamicBlockData::RotationGrip(dynamic_dxf_grip(&fields)),
            "BLOCKVISIBILITYGRIP" => DynamicBlockData::VisibilityGrip(dynamic_dxf_grip(&fields)),
            "BLOCKXYGRIP" => DynamicBlockData::XYGrip(dynamic_dxf_grip(&fields)),
            "BLOCKPROPERTIESTABLEGRIP" => {
                DynamicBlockData::PropertiesTableGrip(dynamic_dxf_grip(&fields))
            }
            "BLOCKALIGNMENTPARAMETER" => {
                DynamicBlockData::AlignmentParameter(BlockAlignmentParameter {
                    parameter: dynamic_dxf_two_point(&fields),
                    align_perpendicular: fields.bool("AcDbBlockAlignmentParameter", 280),
                })
            }
            "BLOCKBASEPOINTPARAMETER" => {
                DynamicBlockData::BasePointParameter(BlockBasePointParameter {
                    parameter: dynamic_dxf_one_point(&fields),
                    point: fields.point("AcDbBlockBasepointParameter", 1011),
                    base_point: fields.point("AcDbBlockBasepointParameter", 1012),
                })
            }
            "BLOCKFLIPPARAMETER" => DynamicBlockData::FlipParameter(BlockFlipParameter {
                parameter: dynamic_dxf_two_point(&fields),
                flip_label: fields.text("AcDbBlockFlipParameter", 305),
                flip_label_description: fields.text("AcDbBlockFlipParameter", 306),
                base_state_label: fields.text("AcDbBlockFlipParameter", 307),
                flipped_state_label: fields.text("AcDbBlockFlipParameter", 308),
                definition_label_point: fields.point("AcDbBlockFlipParameter", 1012),
                flags_96: fields.i32("AcDbBlockFlipParameter", 96),
                tooltip: fields.text("AcDbBlockFlipParameter", 309),
            }),
            "BLOCKLINEARPARAMETER" => {
                let section = "AcDbBlockLinearParameter";
                DynamicBlockData::LinearParameter(BlockLinearParameter {
                    parameter: dynamic_dxf_two_point(&fields),
                    distance_name: fields.text(section, 305),
                    distance_description: fields.text(section, 306),
                    distance: fields.f64(section, 140),
                    value_set: dynamic_dxf_value_set(&fields, section, 96, 141, 307),
                })
            }
            "BLOCKLOOKUPPARAMETER" => DynamicBlockData::LookupParameter(BlockLookupParameter {
                parameter: dynamic_dxf_one_point(&fields),
                index: fields.i32("AcDbBlockLookupParameter", 94),
                lookup_name: fields.text("AcDbBlockLookupParameter", 303),
                lookup_description: fields.text("AcDbBlockLookupParameter", 304),
                unknown_text: String::new(),
            }),
            "BLOCKPOINTPARAMETER" => DynamicBlockData::PointParameter(BlockPointParameter {
                parameter: dynamic_dxf_one_point(&fields),
                position_name: fields.text("AcDbBlockPointParameter", 303),
                position_description: fields.text("AcDbBlockPointParameter", 304),
                definition_label_point: fields.point("AcDbBlockPointParameter", 1011),
            }),
            "BLOCKPOLARPARAMETER" => {
                let section = "AcDbBlockPolarParameter";
                DynamicBlockData::PolarParameter(BlockPolarParameter {
                    parameter: dynamic_dxf_two_point(&fields),
                    angle_name: fields
                        .values(section, 305)
                        .first()
                        .copied()
                        .unwrap_or("")
                        .to_string(),
                    angle_description: fields
                        .values(section, 306)
                        .first()
                        .copied()
                        .unwrap_or("")
                        .to_string(),
                    distance_name: fields
                        .values(section, 305)
                        .get(1)
                        .copied()
                        .unwrap_or("")
                        .to_string(),
                    distance_description: fields
                        .values(section, 306)
                        .get(1)
                        .copied()
                        .unwrap_or("")
                        .to_string(),
                    offset: fields.f64(section, 140),
                    angle_value_set: dynamic_dxf_value_set(&fields, section, 96, 142, 410),
                    distance_value_set: dynamic_dxf_value_set(&fields, section, 97, 146, 309),
                })
            }
            "BLOCKROTATIONPARAMETER" => {
                let section = "AcDbBlockRotationParameter";
                DynamicBlockData::RotationParameter(BlockRotationParameter {
                    parameter: dynamic_dxf_two_point(&fields),
                    definition_base_angle_point: fields.point(section, 1011),
                    angle_name: fields.text(section, 305),
                    angle_description: fields.text(section, 306),
                    angle: fields.f64(section, 140),
                    value_set: dynamic_dxf_value_set(&fields, section, 96, 141, 307),
                })
            }
            "BLOCKXYPARAMETER" => {
                let section = "AcDbBlockXYParameter";
                DynamicBlockData::XYParameter(BlockXYParameter {
                    parameter: dynamic_dxf_two_point(&fields),
                    x_label: fields.text(section, 305),
                    x_label_description: fields.text(section, 306),
                    y_label: fields.text(section, 307),
                    y_label_description: fields.text(section, 308),
                    x_value: fields.f64(section, 142),
                    y_value: fields.f64(section, 141),
                    x_value_set: dynamic_dxf_value_set(&fields, section, 96, 142, 410),
                    y_value_set: dynamic_dxf_value_set(&fields, section, 97, 146, 309),
                })
            }
            "BLOCKUSERPARAMETER" => {
                let section = "AcDbBlockUserParameter";
                let value_code = fields.i16(section, 70);
                let eval_value = match value_code {
                    40 => BlockEvalValue::Real(fields.f64(section, 40)),
                    1 => BlockEvalValue::Text(fields.text(section, 1)),
                    90 => BlockEvalValue::Long(fields.i32(section, 90)),
                    91 => BlockEvalValue::Handle(fields.handle(section, 91)),
                    70 => BlockEvalValue::Short(fields.i16(section, 70)),
                    _ => BlockEvalValue::None,
                };
                DynamicBlockData::UserParameter(BlockUserParameter {
                    parameter: dynamic_dxf_one_point(&fields),
                    flags: fields.i16(section, 90),
                    associated_variable: fields.handle(section, 330),
                    expression: fields.text(section, 301),
                    value_code,
                    value: eval_value,
                    value_type: fields.i16(section, 170),
                })
            }
            "BLOCKANGULARCONSTRAINTPARAMETER" => {
                let section = "AcDbBlockAngularConstraintParameter";
                DynamicBlockData::AngularConstraintParameter(BlockAngularConstraintParameter {
                    constraint: dynamic_dxf_constraint(&fields),
                    center_point: fields.point(section, 1011),
                    end_point: fields.point(section, 1012),
                    expression_name: fields.text(section, 305),
                    expression_description: fields.text(section, 306),
                    angle: fields.f64(section, 140),
                    orientation_on_both_grips: fields.bool(section, 280),
                    value_set: dynamic_dxf_value_set(&fields, section, 96, 128, 307),
                })
            }
            "BLOCKDIAMETRICCONSTRAINTPARAMETER" | "BLOCKRADIALCONSTRAINTPARAMETER" => {
                let section = if dxf_name == "BLOCKDIAMETRICCONSTRAINTPARAMETER" {
                    "AcDbBlockDiametricConstraintParameter"
                } else {
                    "AcDbBlockRadialConstraintParameter"
                };
                let value = BlockDistanceConstraintParameter {
                    constraint: dynamic_dxf_constraint(&fields),
                    expression_name: fields.text(section, 305),
                    expression_description: fields.text(section, 306),
                    distance: fields.f64(section, 140),
                    value_set: dynamic_dxf_value_set(&fields, section, 96, 128, 307),
                };
                if dxf_name == "BLOCKDIAMETRICCONSTRAINTPARAMETER" {
                    DynamicBlockData::DiametricConstraintParameter(value)
                } else {
                    DynamicBlockData::RadialConstraintParameter(value)
                }
            }
            "BLOCKALIGNEDCONSTRAINTPARAMETER" => {
                DynamicBlockData::AlignedConstraintParameter(dynamic_dxf_linear_constraint(&fields))
            }
            "BLOCKLINEARCONSTRAINTPARAMETER" => {
                DynamicBlockData::LinearConstraintParameter(dynamic_dxf_linear_constraint(&fields))
            }
            "BLOCKHORIZONTALCONSTRAINTPARAMETER" => {
                DynamicBlockData::HorizontalConstraintParameter(dynamic_dxf_linear_constraint(
                    &fields,
                ))
            }
            "BLOCKVERTICALCONSTRAINTPARAMETER" => DynamicBlockData::VerticalConstraintParameter(
                dynamic_dxf_linear_constraint(&fields),
            ),
            "ACDBBLOCKPARAMDEPENDENCYBODY" | "BLOCKPARAMDEPENDENCYBODY" => {
                DynamicBlockData::ParameterDependencyBody(BlockParameterDependencyBody {
                    dependency_body_version: fields.i16("AcDbAssocDependencyBody", 90),
                    dimension_base_version: fields.i16("AcDbImpAssocDimDependencyBodyBase", 90),
                    name: fields.text("AcDbImpAssocDimDependencyBodyBase", 1),
                    class_version: fields.i16("AcDbBlockParameterDependencyBody", 90),
                })
            }
            "BLOCKMOVEACTION" => {
                let section = "AcDbBlockMoveAction";
                let connections = dynamic_dxf_sequential_connections(&fields, section, 92, 301, 2);
                DynamicBlockData::MoveAction(BlockMoveAction {
                    action: dynamic_dxf_action(&fields),
                    connections: [
                        connections.first().cloned().unwrap_or_default(),
                        connections.get(1).cloned().unwrap_or_default(),
                    ],
                    offsets: BlockActionOffsets {
                        offset_x: fields.f64(section, 140),
                        offset_y: fields.f64(section, 141),
                        angle_offset: 0.0,
                    },
                })
            }
            "BLOCKFLIPACTION" => {
                let section = "AcDbBlockFlipAction";
                let values = dynamic_dxf_sequential_connections(&fields, section, 92, 301, 4);
                DynamicBlockData::FlipAction(BlockFlipAction {
                    action: dynamic_dxf_action(&fields),
                    connections: std::array::from_fn(|index| {
                        values.get(index).cloned().unwrap_or_default()
                    }),
                })
            }
            "BLOCKROTATEACTION" | "BLOCKSCALEACTION" => {
                let section = if dxf_name == "BLOCKROTATEACTION" {
                    "AcDbBlockRotationAction"
                } else {
                    "AcDbBlockScaleAction"
                };
                let value = BlockBasePointAction {
                    action: dynamic_dxf_action_with_base(&fields),
                    connections: dynamic_dxf_sequential_connections(
                        &fields,
                        section,
                        94,
                        303,
                        if dxf_name == "BLOCKROTATEACTION" {
                            1
                        } else {
                            3
                        },
                    ),
                };
                if dxf_name == "BLOCKROTATEACTION" {
                    DynamicBlockData::RotateAction(value)
                } else {
                    DynamicBlockData::ScaleAction(value)
                }
            }
            "BLOCKARRAYACTION" => {
                let section = "AcDbBlockArrayAction";
                let values = dynamic_dxf_sequential_connections(&fields, section, 92, 301, 4);
                DynamicBlockData::ArrayAction(BlockArrayAction {
                    action: dynamic_dxf_action(&fields),
                    connections: std::array::from_fn(|index| {
                        values.get(index).cloned().unwrap_or_default()
                    }),
                    column_offset: fields.f64(section, 140),
                    row_offset: fields.f64(section, 141),
                })
            }
            "BLOCKLOOKUPACTION" => {
                let section = "AcDbBlockLookupAction";
                let row_count = fields.i32(section, 92);
                let column_count = fields.i32(section, 93);
                let count = row_count.saturating_mul(column_count).max(0) as usize;
                let code0 = fields.values(section, 94);
                let code1 = fields.values(section, 95);
                let code2 = fields.values(section, 96);
                let name0 = fields.values(section, 303);
                let name1 = fields.values(section, 304);
                let name2 = fields.values(section, 305);
                let flag282 = fields.values(section, 282);
                let flag281 = fields.values(section, 281);
                let rows = (0..count)
                    .map(|index| BlockLookupRow {
                        connections: [
                            BlockConnection {
                                code: code0
                                    .get(index)
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0),
                                name: name0.get(index).copied().unwrap_or("").to_string(),
                            },
                            BlockConnection {
                                code: code1
                                    .get(index)
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0),
                                name: name1.get(index).copied().unwrap_or("").to_string(),
                            },
                            BlockConnection {
                                code: code2
                                    .get(index)
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0),
                                name: name2.get(index).copied().unwrap_or("").to_string(),
                            },
                        ],
                        flag_282: flag282
                            .get(index)
                            .and_then(|value| value.parse::<i32>().ok())
                            .unwrap_or(0)
                            != 0,
                        flag_281: flag281
                            .get(index)
                            .and_then(|value| value.parse::<i32>().ok())
                            .unwrap_or(0)
                            != 0,
                    })
                    .collect();
                DynamicBlockData::LookupAction(BlockLookupAction {
                    action: dynamic_dxf_action(&fields),
                    row_count,
                    column_count,
                    expressions: fields
                        .values(section, 302)
                        .into_iter()
                        .take(count)
                        .map(str::to_string)
                        .collect(),
                    rows,
                    flag_280: fields.bool(section, 280),
                })
            }
            "BLOCKSTRETCHACTION" => {
                let section = "AcDbBlockStretchAction";
                let connections = dynamic_dxf_sequential_connections(&fields, section, 92, 301, 2);
                let xs = fields.values(section, 1011);
                let ys = fields.values(section, 1021);
                let handles_raw = fields.values(section, 331);
                let handle_counts: Vec<usize> = fields
                    .values(section, 74)
                    .into_iter()
                    .map(|value| value.parse::<usize>().unwrap_or(0))
                    .collect();
                let code_values = fields.values(section, 95);
                let code_counts: Vec<usize> = fields
                    .values(section, 76)
                    .into_iter()
                    .map(|value| value.parse::<usize>().unwrap_or(0))
                    .collect();
                let indexes: Vec<i32> = fields
                    .values(section, 94)
                    .into_iter()
                    .filter_map(|value| value.parse().ok())
                    .collect();
                let mut index_offset = 0usize;
                let handles = handles_raw
                    .iter()
                    .enumerate()
                    .map(|(index, handle)| {
                        let count = handle_counts.get(index).copied().unwrap_or(0);
                        let item_indexes = indexes
                            .get(index_offset..index_offset.saturating_add(count))
                            .unwrap_or(&[])
                            .to_vec();
                        index_offset = index_offset.saturating_add(count);
                        BlockStretchHandle {
                            handle: parse_dxf_handle(handle),
                            indexes: item_indexes,
                        }
                    })
                    .collect();
                let codes = code_values
                    .iter()
                    .enumerate()
                    .map(|(index, code)| {
                        let count = code_counts.get(index).copied().unwrap_or(0);
                        let item_indexes = indexes
                            .get(index_offset..index_offset.saturating_add(count))
                            .unwrap_or(&[])
                            .to_vec();
                        index_offset = index_offset.saturating_add(count);
                        BlockStretchCode {
                            code: code.parse().unwrap_or(0),
                            indexes: item_indexes,
                        }
                    })
                    .collect();
                DynamicBlockData::StretchAction(BlockStretchAction {
                    action: dynamic_dxf_action(&fields),
                    connections: [
                        connections.first().cloned().unwrap_or_default(),
                        connections.get(1).cloned().unwrap_or_default(),
                    ],
                    points: xs
                        .iter()
                        .zip(ys.iter())
                        .map(|(x, y)| {
                            Vector2::new(x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0))
                        })
                        .collect(),
                    handles,
                    codes,
                    offsets: BlockActionOffsets {
                        offset_x: fields.f64(section, 140),
                        offset_y: fields.f64(section, 141),
                        angle_offset: 0.0,
                    },
                })
            }
            "BLOCKPOLARSTRETCHACTION" => {
                let section = "AcDbBlockPolarStretchAction";
                let connections = dynamic_dxf_sequential_connections(&fields, section, 92, 301, 6);
                let xs = fields.values(section, 10);
                let ys = fields.values(section, 20);
                DynamicBlockData::PolarStretchAction(BlockPolarStretchAction {
                    action: dynamic_dxf_action(&fields),
                    connections: std::array::from_fn(|index| {
                        connections.get(index).cloned().unwrap_or_default()
                    }),
                    points: xs
                        .iter()
                        .zip(ys.iter())
                        .map(|(x, y)| {
                            Vector2::new(x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0))
                        })
                        .collect(),
                    handles: fields
                        .values(section, 331)
                        .into_iter()
                        .map(parse_dxf_handle)
                        .collect(),
                    handle_flags: fields
                        .values(section, 74)
                        .into_iter()
                        .filter_map(|value| value.parse().ok())
                        .collect(),
                    codes: fields
                        .values(section, 76)
                        .into_iter()
                        .filter_map(|value| value.parse().ok())
                        .collect(),
                })
            }
            "BLOCKVISIBILITYPARAMETER" => {
                let one = dynamic_dxf_one_point(&fields);
                let section = "AcDbBlockVisibilityParameter";
                let all_blocks = fields
                    .values(section, 331)
                    .into_iter()
                    .map(parse_dxf_handle)
                    .collect();
                let state_names = fields.values(section, 303);
                let state_blocks = fields.values(section, 332);
                let state_params = fields.values(section, 333);
                let block_counts = fields.values(section, 94);
                let param_counts = fields.values(section, 95);
                let mut block_offset = 0usize;
                let mut param_offset = 0usize;
                let states = state_names
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let block_count = block_counts
                            .get(index)
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        let param_count = param_counts
                            .get(index)
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        let visible_blocks = state_blocks
                            .iter()
                            .skip(block_offset)
                            .take(block_count)
                            .map(|value| parse_dxf_handle(value))
                            .collect();
                        let visible_params = state_params
                            .iter()
                            .skip(param_offset)
                            .take(param_count)
                            .map(|value| parse_dxf_handle(value))
                            .collect();
                        block_offset += block_count;
                        param_offset += param_count;
                        BlockVisibilityState {
                            name: name.to_string(),
                            visible_blocks,
                            visible_params,
                        }
                    })
                    .collect();
                DynamicBlockData::VisibilityParameter(BlockVisibilityParameter {
                    handle: object.handle,
                    owner: object.owner,
                    eval_parent_id: one.parameter.element.eval.parent_id,
                    eval_major: one.parameter.element.eval.major,
                    eval_minor: one.parameter.element.eval.minor,
                    eval_value_code: one.parameter.element.eval.value_code,
                    eval_value: one.parameter.element.eval.value,
                    eval_node_id: one.parameter.element.eval.node_id,
                    element_name: one.parameter.element.name,
                    element_major: one.parameter.element.major,
                    element_minor: one.parameter.element.minor,
                    element_eed_1071: one.parameter.element.eed_1071,
                    show_properties: one.parameter.show_properties,
                    chain_actions: one.parameter.chain_actions,
                    name: fields.text(section, 301),
                    description: fields.text(section, 302),
                    def_point: one.definition_point,
                    property_info: std::array::from_fn(|index| BlockParameterPropertyInfo {
                        connections: one.properties[index]
                            .connections
                            .iter()
                            .map(|item| BlockParameterConnection {
                                code: item.code,
                                name: item.name.clone(),
                            })
                            .collect(),
                    }),
                    property_info_count: one.property_count,
                    is_initialized: fields.bool(section, 281),
                    unknown_bool: fields.bool(section, 91),
                    all_blocks,
                    states,
                })
            }
            "BLOCKPROPERTIESTABLE" => DynamicBlockData::PropertiesTable,
            "ACAD_EVALUATION_GRAPH" => {
                let entries = fields
                    .sections
                    .get("AcDbEvalGraph")
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let mut nodes = Vec::new();
                let mut edges = Vec::new();
                let mut index = 0usize;
                while index < entries.len() {
                    if index + 7 < entries.len()
                        && entries[index].0 == 91
                        && entries[index + 1].0 == 93
                        && entries[index + 2].0 == 95
                        && entries[index + 3].0 == 360
                    {
                        let mut node_data = [0; 4];
                        for (target, item) in node_data
                            .iter_mut()
                            .zip(entries[index + 4..index + 8].iter())
                        {
                            *target = item.1.parse().unwrap_or(0);
                        }
                        nodes.push(BlockEvaluationNode {
                            id: entries[index].1.parse().unwrap_or(0),
                            edge_flags: entries[index + 1].1.parse().unwrap_or(0),
                            next_id: entries[index + 2].1.parse().unwrap_or(0),
                            expression: parse_dxf_handle(&entries[index + 3].1),
                            node_data,
                            active_cycles: None,
                        });
                        index += 8;
                    } else if index + 9 < entries.len()
                        && entries[index].0 == 92
                        && entries[index + 1].0 == 93
                        && entries[index + 2].0 == 94
                        && entries[index + 3].0 == 91
                        && entries[index + 4].0 == 91
                    {
                        let mut outgoing_edges = [0; 5];
                        for (target, item) in outgoing_edges
                            .iter_mut()
                            .zip(entries[index + 5..index + 10].iter())
                        {
                            *target = item.1.parse().unwrap_or(0);
                        }
                        edges.push(BlockEvaluationEdge {
                            id: entries[index].1.parse().unwrap_or(0),
                            next_id: entries[index + 1].1.parse().unwrap_or(0),
                            incoming_edge: entries[index + 2].1.parse().unwrap_or(0),
                            source_node: entries[index + 3].1.parse().unwrap_or(0),
                            destination_node: entries[index + 4].1.parse().unwrap_or(0),
                            outgoing_edges,
                        });
                        index += 10;
                    } else {
                        index += 1;
                    }
                }
                DynamicBlockData::EvaluationGraph(BlockEvaluationGraph {
                    first_node_id: fields.i32("AcDbEvalGraph", 96),
                    first_node_id_copy: fields.i32("AcDbEvalGraph", 97),
                    nodes,
                    edges,
                })
            }
            _ => DynamicBlockData::Unknown,
        };
        Ok(object)
    }

    fn read_object_context_data_dxf(&mut self, dxf_name: &str) -> Result<ObjectContextData> {
        let mut handle = Handle::NULL;
        let mut owner_handle = Handle::NULL;
        let mut reactors = Vec::new();
        let mut xdictionary_handle = None;
        let mut fields = ClassDxfFields::default();
        let mut section = String::new();
        let mut group = String::new();
        let mut owner_seen = false;
        let is_mleader = dxf_name.eq_ignore_ascii_case("ACDB_MLEADEROBJECTCONTEXTDATA_CLASS");
        let mut mleader_context = crate::entities::multileader::MultiLeaderAnnotContext::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 if handle.is_null() => {
                    handle = parse_dxf_handle(&pair.value_string);
                }
                100 => section = pair.value_string.clone(),
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() => {
                    owner_handle = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                300 if is_mleader && pair.value_string.starts_with("CONTEXT_DATA") => {
                    self.read_mleader_context(&mut mleader_context)?;
                }
                _ => fields.push(&section, pair.code, pair.value_string),
            }
            if group == "}" {
                group.clear();
            }
        }

        let class_version = fields.i16("AcDbObjectContextData", 70);
        let is_default = fields.bool("AcDbObjectContextData", 290);
        let scale = fields.handle("AcDbAnnotScaleObjectContextData", 340);
        let name = dxf_name.to_uppercase();
        let kind = match name.as_str() {
            "ACDB_ANNOTSCALEOBJECTCONTEXTDATA_CLASS" => ObjectContextKind::AnnotScale,
            "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS" => {
                let section = "AcDbBlkRefObjectContextData";
                ObjectContextKind::BlkRef {
                    rotation: fields.f64(section, 50),
                    insertion: fields.point3(section, 10),
                    scale_factor: Vector3::new(
                        fields.f64(section, 41),
                        fields.f64(section, 42),
                        fields.f64(section, 43),
                    ),
                }
            }
            "ACDB_TEXTOBJECTCONTEXTDATA_CLASS" => {
                let section = "AcDbTextObjectContextData";
                ObjectContextKind::Text {
                    horizontal_mode: fields.i16(section, 70),
                    rotation: fields.f64(section, 50),
                    insertion: fields.point2(section, 10),
                    alignment: fields.point2(section, 11),
                }
            }
            "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS" => {
                let section = "AcDbMTextObjectContextData";
                let attachment = fields.i32(section, 70);
                let insertion = fields.point3(section, 10);
                let x_axis_dir = fields.point3(section, 11);
                let rect_width = fields.f64(section, 40);
                let rect_height = fields.f64(section, 41);
                let extents_width = fields.f64(section, 42);
                let extents_height = fields.f64(section, 43);
                let column_type = fields.i32(section, 71);
                let columns = if column_type != 0 {
                    let num_heights = fields.i32(section, 72);
                    let width = fields.f64(section, 44);
                    let gutter = fields.f64(section, 45);
                    let auto_height = fields.bool(section, 73);
                    let flow_reversed = fields.bool(section, 74);
                    let mut heights = Vec::new();
                    if !auto_height && column_type == 2 {
                        for _ in 0..num_heights.max(0).min(100_000) {
                            heights.push(fields.f64(section, 46));
                        }
                    }
                    Some(MTextColumns {
                        num_heights,
                        width,
                        gutter,
                        auto_height,
                        flow_reversed,
                        heights,
                    })
                } else {
                    None
                };
                ObjectContextKind::MText(MTextContext {
                    attachment,
                    x_axis_dir,
                    insertion,
                    rect_width,
                    rect_height,
                    extents_width,
                    extents_height,
                    column_type,
                    columns,
                })
            }
            "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_DMDIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_RADIMOBJECTCONTEXTDATA_CLASS"
            | "ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS"
            | "ACDB_ORDDIMOBJECTCONTEXTDATA_CLASS" => {
                let section = "AcDbDimensionObjectContextData";
                let block = fields.handle(section, 2);
                let b293 = fields.bool(section, 293);
                let def_pt = fields.point2(section, 10);
                let is_def_textloc = fields.bool(section, 294);
                let text_rotation = fields.f64(section, 140);
                let dimtofl = fields.bool(section, 298);
                let dimosxd = fields.bool(section, 291);
                let dimatfit = fields.bool(section, 70);
                let dimtix = fields.bool(section, 292);
                let dimtmove = fields.bool(section, 71);
                let override_code = fields.i16(section, 280) as u8;
                let has_arrow2 = fields.bool(section, 295);
                let flip_arrow2 = fields.bool(section, 296);
                let flip_arrow1 = fields.bool(section, 297);
                let subtype = match name.as_str() {
                    "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS" => DimSubtype::Aligned {
                        dimline_pt: fields.point3("AcDbAlignedDimensionObjectContextData", 11),
                    },
                    "ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS" => DimSubtype::Angular {
                        arc_pt: fields.point3("AcDbAngularDimensionObjectContextData", 11),
                    },
                    "ACDB_DMDIMOBJECTCONTEXTDATA_CLASS" => DimSubtype::Diametric {
                        first_arc_pt: fields.point3("AcDbDiametricDimensionObjectContextData", 11),
                        def_pt: fields.point3("AcDbDiametricDimensionObjectContextData", 12),
                    },
                    "ACDB_RADIMOBJECTCONTEXTDATA_CLASS" => DimSubtype::Radial {
                        first_arc_pt: fields.point3("AcDbRadialDimensionObjectContextData", 11),
                    },
                    "ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS" => DimSubtype::RadialLarge {
                        ovr_center: fields.point3("AcDbRadialDimensionLargeObjectContextData", 12),
                        jog_point: fields.point3("AcDbRadialDimensionLargeObjectContextData", 13),
                    },
                    _ => DimSubtype::Ordinate {
                        feature_location_pt: fields
                            .point3("AcDbOrdinateDimensionObjectContextData", 11),
                        leader_endpt: fields.point3("AcDbOrdinateDimensionObjectContextData", 12),
                    },
                };
                ObjectContextKind::Dim(DimContext {
                    def_pt,
                    is_def_textloc,
                    text_rotation,
                    block,
                    b293,
                    dimtofl,
                    dimosxd,
                    dimatfit,
                    dimtix,
                    dimtmove,
                    override_code,
                    has_arrow2,
                    flip_arrow2,
                    flip_arrow1,
                    subtype,
                })
            }
            "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS" => ObjectContextKind::MLeader(mleader_context),
            "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS" => {
                let text_section = if fields.has("AcDbTextObjectContextData", 70) {
                    "AcDbTextObjectContextData"
                } else {
                    "AcDbAnnotScaleObjectContextData"
                };
                let horizontal_mode = fields.i16(text_section, 70);
                let rotation = fields.f64(text_section, 50).to_radians();
                let insertion = fields.point2(text_section, 10);
                let alignment = fields.point2(text_section, 11);
                let enable_section = if fields.has("AcDbMTextAttributeObjectContextData", 290) {
                    "AcDbMTextAttributeObjectContextData"
                } else {
                    "AcDbAnnotScaleObjectContextData"
                };
                let enable_context = fields.bool(enable_section, 290);
                let context = if enable_context && fields.has("AcDbObjectContextData", 70) {
                    let context_class_version = fields.i16("AcDbObjectContextData", 70);
                    let context_is_default = fields.bool("AcDbObjectContextData", 290);
                    let section = "AcDbAnnotScaleObjectContextData";
                    let context_scale = fields.handle(section, 340);
                    let attachment = fields.i32(section, 70);
                    let x_axis_dir = fields.point3(section, 10);
                    let context_insertion = fields.point3(section, 11);
                    let rect_width = fields.f64(section, 40);
                    let rect_height = fields.f64(section, 41);
                    let extents_width = fields.f64(section, 42);
                    let extents_height = fields.f64(section, 43);
                    let column_type = fields.i32(section, 71);
                    let columns = if column_type != 0 {
                        let num_heights = fields.i32(section, 72);
                        let width = fields.f64(section, 44);
                        let gutter = fields.f64(section, 45);
                        let auto_height = fields.bool(section, 73);
                        let flow_reversed = fields.bool(section, 74);
                        let mut heights = Vec::new();
                        if !auto_height && column_type == 2 {
                            for _ in 0..num_heights.max(0).min(100_000) {
                                heights.push(fields.f64(section, 46));
                            }
                        }
                        Some(MTextColumns {
                            num_heights,
                            width,
                            gutter,
                            auto_height,
                            flow_reversed,
                            heights,
                        })
                    } else {
                        None
                    };
                    Some(crate::objects::EmbeddedMTextContext {
                        owner_handle: Handle::NULL,
                        reactors: Vec::new(),
                        xdictionary_handle: None,
                        has_binary_data: false,
                        class_version: context_class_version,
                        is_default: context_is_default,
                        scale: context_scale,
                        mtext: MTextContext {
                            attachment,
                            x_axis_dir,
                            insertion: context_insertion,
                            rect_width,
                            rect_height,
                            extents_width,
                            extents_height,
                            column_type,
                            columns,
                        },
                    })
                } else {
                    None
                };
                ObjectContextKind::MTextAttribute(MTextAttributeContext {
                    horizontal_mode,
                    rotation,
                    insertion,
                    alignment,
                    enable_context,
                    context,
                })
            }
            "ACDB_LEADEROBJECTCONTEXTDATA_CLASS" => {
                let section = if fields.has("AcDbLeaderObjectContextData", 70) {
                    "AcDbLeaderObjectContextData"
                } else {
                    "AcDbAnnotScaleObjectContextData"
                };
                let mut points = Vec::new();
                for _ in 0..fields.i32(section, 70).max(0).min(100_000) {
                    points.push(fields.point3(section, 10));
                }
                ObjectContextKind::Leader(LeaderContext {
                    points,
                    x_direction: fields.point3(section, 11),
                    annotation_enabled: fields.bool(section, 290),
                    insertion_offset: fields.point3(section, 12),
                    endpoint_projection: fields.point3(section, 13),
                })
            }
            "ACDB_FCFOBJECTCONTEXTDATA_CLASS" => {
                let section = "AcDbFcfObjectContextData";
                ObjectContextKind::Fcf {
                    location: fields.point3(section, 10),
                    horizontal_direction: fields.point3(section, 11),
                }
            }
            "ACDB_HATCHSCALECONTEXTDATA_CLASS" => {
                ObjectContextKind::HatchScale(class_dxf_hatch_scale_context(&mut fields))
            }
            "ACDB_HATCHVIEWCONTEXTDATA_CLASS" => {
                let hatch = class_dxf_hatch_scale_context(&mut fields);
                let section = "AcDbHatchViewContextData";
                ObjectContextKind::HatchView(crate::objects::HatchViewContext {
                    hatch,
                    view: fields.handle(section, 330),
                    view_normal: fields.point3(section, 10),
                    view_rotation: fields.f64(section, 51).to_radians(),
                    evaluate_hatch: fields.bool(section, 290),
                })
            }
            _ => ObjectContextKind::Opaque,
        };

        Ok(ObjectContextData {
            handle,
            owner_handle,
            reactors,
            xdictionary_handle,
            class_version,
            is_default,
            scale,
            kind,
        })
    }

    fn read_class_object_dxf(&mut self, dxf_name: &str) -> Result<ClassObject> {
        let mut object = ClassObject::default();
        let mut fields = ClassDxfFields::default();
        let mut section = String::new();
        let mut group = String::new();
        let mut owner_seen = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 if object.handle.is_null() => {
                    object.handle = parse_dxf_handle(&pair.value_string);
                }
                100 => section = pair.value_string.clone(),
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    object.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    object.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() => {
                    object.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                _ => fields.push(&section, pair.code, pair.value_string),
            }
            if group == "}" {
                group.clear();
            }
        }

        let name = dxf_name.to_uppercase();
        object.data = match name.as_str() {
            "SPATIAL_INDEX" => {
                let updated = fields.f64("AcDbIndex", 40);
                let last_updated_julian_day = updated.floor() as i32;
                let last_updated_milliseconds =
                    ((updated - updated.floor()) * 86_400_000.0).round() as i32;
                let section = "AcDbSpatialIndex";
                let min_corner = Vector3::new(
                    fields.f64(section, 40),
                    fields.f64(section, 40),
                    fields.f64(section, 40),
                );
                let max_corner = Vector3::new(
                    fields.f64(section, 40),
                    fields.f64(section, 40),
                    fields.f64(section, 40),
                );
                let mut indexed_objects = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    indexed_objects.push(fields.handle(section, 330));
                }
                let _cached_binary_size = fields.i32(section, 90);
                let _cached_binary_data = fields.strings(section, 310);
                ClassObjectData::SpatialIndex(SpatialIndex {
                    last_updated_julian_day,
                    last_updated_milliseconds,
                    min_corner,
                    max_corner,
                    indexed_objects,
                })
            }
            "LAYERFILTER" => {
                let section = "AcDbLayerFilter";
                let count = fields.i32(section, 90).max(0).min(100_000);
                let mut names = Vec::new();
                for _ in 0..count {
                    names.push(fields.string(section, 8));
                }
                ClassObjectData::LayerFilter(LayerFilter { names })
            }
            "PARTIAL_VIEWING_INDEX" => {
                let section = "OdDbPartialViewingIndex";
                let count = fields.i32(section, 90).max(0).min(100_000);
                let has_entries = fields.bool(section, 290);
                let mut entries = Vec::new();
                for _ in 0..count {
                    entries.push(PartialViewingIndexEntry {
                        extents_min: fields.point3(section, 10),
                        extents_max: fields.point3(section, 11),
                        object: fields.handle(section, 330),
                    });
                }
                ClassObjectData::PartialViewingIndex(PartialViewingIndex {
                    has_entries,
                    entries,
                })
            }
            "VBA_PROJECT" => {
                let section = "AcDbVbaProject";
                let _size = fields.i32(section, 90);
                let mut data = Vec::new();
                for value in fields.strings(section, 310) {
                    append_hex_bytes(&mut data, &value);
                }
                ClassObjectData::VbaProject(VbaProject {
                    storage: crate::compound_file::StructuredStoragePayload::decode(&data),
                })
            }
            "SECTION_MANAGER" => {
                let section = "AcDbSectionManager";
                let is_live = fields.bool(section, 70);
                let mut sections = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    sections.push(fields.handle(section, 330));
                }
                ClassObjectData::SectionManager(SectionManager { is_live, sections })
            }
            "SECTION_SETTINGS" => {
                let section = "AcDbSectionSettings";
                let current_type = fields.i32(section, 90);
                let type_count = fields.i32(section, 91).max(0).min(4);
                let mut types = Vec::new();
                for _ in 0..type_count {
                    let section_type = fields.i32(section, 90);
                    let generation = fields.i32(section, 91);
                    let mut sources = Vec::new();
                    for _ in 0..fields.i32(section, 92).max(0).min(100_000) {
                        sources.push(fields.handle(section, 330));
                    }
                    let destination_block = fields.handle(section, 331);
                    let destination_file =
                        fields.string_skipping(section, 1, &["SectionTypeSettings"]);
                    let mut geometry = Vec::new();
                    for _ in 0..fields.i32(section, 93).max(0).min(100_000) {
                        geometry.push(SectionGeometrySettings {
                            geometry_count: fields.i32(section, 90),
                            index: fields.i32(section, 91),
                            flags: fields.i32(section, 92),
                            color: Color::from_index(fields.i16(section, 62)),
                            layer: fields.string(section, 8),
                            linetype: fields.string(section, 6),
                            linetype_scale: fields.f64(section, 40),
                            plot_style: fields.string(section, 1),
                            lineweight: fields.i32(section, 370),
                            face_transparency: fields.i16(section, 70),
                            edge_transparency: fields.i16(section, 71),
                            hatch_type: fields.i16(section, 72),
                            hatch_pattern: fields.string_skipping(
                                section,
                                2,
                                &["SectionGeometrySettings"],
                            ),
                            hatch_angle: fields.f64(section, 41),
                            hatch_spacing: fields.f64(section, 42),
                            hatch_scale: fields.f64(section, 43),
                        });
                    }
                    types.push(SectionTypeSettings {
                        section_type,
                        generation,
                        sources,
                        destination_block,
                        destination_file,
                        geometry,
                    });
                }
                ClassObjectData::SectionSettings(SectionSettings {
                    current_type,
                    types,
                })
            }
            "LIGHTLIST" => {
                let section = "AcDbLightList";
                let class_version = fields.i32(section, 90);
                let mut lights = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    lights.push(LightListEntry {
                        handle: fields.handle(section, 5),
                        name: fields.string(section, 1),
                    });
                }
                ClassObjectData::LightList(LightList {
                    class_version,
                    lights,
                })
            }
            "SUN" => {
                let section = "AcDbSun";
                ClassObjectData::Sun(Sun {
                    class_version: fields.i32(section, 90),
                    is_on: fields.bool(section, 290),
                    color: Color::from_index(fields.i16(section, 63)),
                    intensity: fields.f64(section, 40),
                    has_shadow: fields.bool(section, 291),
                    julian_day: fields.i32(section, 91),
                    milliseconds: fields.i32(section, 92),
                    is_daylight_savings_on: fields.bool(section, 292),
                    shadow_type: fields.i32(section, 70),
                    shadow_map_size: fields.i16(section, 71),
                    shadow_softness: fields.i16(section, 280) as u8,
                })
            }
            "RENDERSETTINGS" => {
                ClassObjectData::RenderSettings(class_dxf_render_settings(&mut fields, true))
            }
            "MENTALRAYRENDERSETTINGS" => {
                let base = class_dxf_render_settings(&mut fields, true);
                let section = "AcDbMentalRayRenderSettings";
                ClassObjectData::MentalRayRenderSettings(MentalRayRenderSettings {
                    base,
                    version: fields.i32(section, 90),
                    sampling_min: fields.i32(section, 90),
                    sampling_max: fields.i32(section, 90),
                    sampling_filter: fields.i16(section, 70),
                    sampling_filter_width: fields.f64(section, 40),
                    sampling_filter_height: fields.f64(section, 40),
                    sampling_contrast: [
                        fields.f64(section, 40),
                        fields.f64(section, 40),
                        fields.f64(section, 40),
                        fields.f64(section, 40),
                    ],
                    shadow_mode: fields.i16(section, 70),
                    shadow_maps_enabled: fields.bool(section, 290),
                    ray_tracing_enabled: fields.bool(section, 290),
                    ray_trace_depth: [
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                    ],
                    global_illumination_enabled: fields.bool(section, 290),
                    global_illumination_sample_count: fields.i32(section, 90),
                    global_illumination_sample_radius_enabled: fields.bool(section, 290),
                    global_illumination_sample_radius: fields.f64(section, 40),
                    photons_per_light: fields.i32(section, 90),
                    photon_trace_depth: [
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                    ],
                    final_gathering_enabled: fields.bool(section, 290),
                    final_gathering_ray_count: fields.i32(section, 90),
                    final_gathering_sample_radius_state: [
                        fields.bool(section, 290),
                        fields.bool(section, 290),
                        fields.bool(section, 290),
                    ],
                    final_gathering_sample_radius: [
                        fields.f64(section, 40),
                        fields.f64(section, 40),
                    ],
                    light_luminance_scale: fields.f64(section, 40),
                    diagnostics_mode: fields.i16(section, 70),
                    diagnostics_grid_mode: fields.i16(section, 70),
                    diagnostics_grid_size: fields.f64(section, 40),
                    diagnostics_photon_mode: fields.i16(section, 70),
                    diagnostics_bsp_mode: fields.i16(section, 70),
                    export_mi_enabled: fields.bool(section, 290),
                    description: fields.string(section, 1),
                    tile_size: fields.i32(section, 90),
                    tile_order: fields.i16(section, 70),
                    memory_limit: fields.i32(section, 90),
                    diagnostics_samples_mode: fields.bool(section, 290),
                    energy_multiplier: fields.f64(section, 40),
                })
            }
            "RAPIDRTRENDERSETTINGS" => {
                let section = "AcDbRapidRTRenderSettings";
                let mut base = class_dxf_render_settings(&mut fields, false);
                let version = fields.i32(section, 90);
                let render_target = fields.i32(section, 70);
                let render_level = fields.i32(section, 90);
                let render_time = fields.i32(section, 90);
                let lighting_model = fields.i32(section, 70);
                let filter_type = fields.i32(section, 70);
                let filter_width = fields.f64(section, 40);
                let filter_height = fields.f64(section, 40);
                base.has_predefined = fields.bool(section, 290);
                ClassObjectData::RapidRtRenderSettings(RapidRtRenderSettings {
                    base,
                    version,
                    render_target,
                    render_level,
                    render_time,
                    lighting_model,
                    filter_type,
                    filter_width,
                    filter_height,
                })
            }
            "GRADIENT_BACKGROUND" => {
                let section = "AcDbGradientBackground";
                ClassObjectData::GradientBackground(GradientBackground {
                    class_version: fields.i32(section, 90),
                    color_top: fields.i32(section, 90) as u32,
                    color_middle: fields.i32(section, 91) as u32,
                    color_bottom: fields.i32(section, 92) as u32,
                    horizon: fields.f64(section, 140),
                    height: fields.f64(section, 141),
                    rotation: fields.f64(section, 142),
                })
            }
            "GROUND_PLANE_BACKGROUND" => {
                let section = "AcDbGroundPlaneBackground";
                ClassObjectData::GroundPlaneBackground(GroundPlaneBackground {
                    class_version: fields.i32(section, 90),
                    color_sky_zenith: fields.i32(section, 90) as u32,
                    color_sky_horizon: fields.i32(section, 91) as u32,
                    color_underground_horizon: fields.i32(section, 92) as u32,
                    color_underground_azimuth: fields.i32(section, 93) as u32,
                    color_near: fields.i32(section, 94) as u32,
                    color_far: fields.i32(section, 95) as u32,
                })
            }
            "RAPIDRTRENDERENVIRONMENT" | "IBL_BACKGROUND" => {
                let section = "AcDbIBLBackground";
                ClassObjectData::IblBackground(IblBackground {
                    class_version: fields.i32(section, 90),
                    enabled: fields.bool(section, 290),
                    name: fields.string(section, 1),
                    rotation: fields.f64(section, 40),
                    display_image: fields.bool(section, 290),
                    secondary_background: fields.handle(section, 340),
                })
            }
            "IMAGE_BACKGROUND" => {
                let section = "AcDbImageBackground";
                ClassObjectData::ImageBackground(ImageBackground {
                    class_version: fields.i32(section, 90),
                    filename: fields.string(section, 300),
                    fit_to_screen: fields.bool(section, 290),
                    maintain_aspect_ratio: fields.bool(section, 291),
                    use_tiling: fields.bool(section, 292),
                    offset: fields.point2(section, 140),
                    scale: fields.point2(section, 142),
                })
            }
            "SKYLIGHT_BACKGROUND" => {
                let section = "AcDbSkyBackground";
                ClassObjectData::SkyLightBackground(SkyLightBackground {
                    class_version: fields.i32(section, 90),
                    sun: fields.handle(section, 340),
                })
            }
            "SOLID_BACKGROUND" => {
                let section = "AcDbSolidBackground";
                ClassObjectData::SolidBackground(SolidBackground {
                    class_version: fields.i32(section, 90),
                    color: fields.i32(section, 90) as u32,
                })
            }
            "RENDERENTRY" => {
                let section = "AcDbRenderEntry";
                ClassObjectData::RenderEntry(RenderEntry {
                    class_version: fields.i32(section, 90),
                    image_filename: fields.string(section, 1),
                    preset_name: fields.string(section, 1),
                    view_name: fields.string(section, 1),
                    width: fields.i32(section, 90),
                    height: fields.i32(section, 90),
                    start_year: fields.i16(section, 70),
                    start_month: fields.i16(section, 70),
                    start_day: fields.i16(section, 70),
                    start_hour: fields.i16(section, 70),
                    start_minute: fields.i16(section, 70),
                    start_second: fields.i16(section, 70),
                    start_millisecond: fields.i16(section, 70),
                    end_year: fields.i16(section, 70),
                    end_month: fields.i16(section, 70),
                    end_day: fields.i16(section, 70),
                    end_hour: fields.i16(section, 70),
                    end_minute: fields.i16(section, 70),
                    end_second: fields.i16(section, 70),
                    end_millisecond: fields.i16(section, 70),
                    render_time: fields.f64(section, 40),
                    memory_amount: fields.i32(section, 90),
                    material_count: fields.i32(section, 90),
                    light_count: fields.i32(section, 90),
                    triangle_count: fields.i32(section, 90),
                    display_index: fields.i32(section, 90),
                })
            }
            "RENDERENVIRONMENT" => {
                let section = "AcDbRenderEnvironment";
                ClassObjectData::RenderEnvironment(RenderEnvironment {
                    class_version: fields.i32(section, 90),
                    fog_enabled: fields.bool(section, 290),
                    fog_background_enabled: fields.bool(section, 290),
                    fog_color: [
                        fields.i16(section, 280) as u8,
                        fields.i16(section, 280) as u8,
                        fields.i16(section, 280) as u8,
                    ],
                    fog_density_near: fields.f64(section, 40),
                    fog_density_far: fields.f64(section, 40),
                    fog_distance_near: fields.f64(section, 40),
                    fog_distance_far: fields.f64(section, 40),
                    environment_image_enabled: fields.bool(section, 290),
                    environment_image_filename: fields.string(section, 1),
                })
            }
            "RENDERGLOBAL" => {
                let section = "AcDbRenderGlobal";
                ClassObjectData::RenderGlobal(RenderGlobal {
                    class_version: fields.i32(section, 90),
                    procedure: fields.i32(section, 90),
                    destination: fields.i32(section, 90),
                    save_enabled: fields.bool(section, 290),
                    save_filename: fields.string(section, 1),
                    image_width: fields.i32(section, 90),
                    image_height: fields.i32(section, 90),
                    predefined_presets_first: fields.bool(section, 290),
                    high_level_info: fields.bool(section, 290),
                })
            }
            "ACDBMOTIONPATH" | "MOTIONPATH" => {
                let section = "AcDbMotionPath";
                ClassObjectData::MotionPath(MotionPath {
                    class_version: fields.i32(section, 90),
                    camera_path: fields.handle(section, 340),
                    target_path: fields.handle(section, 340),
                    view: fields.handle(section, 340),
                    frames: fields.i16(section, 90),
                    frame_rate: fields.i16(section, 90),
                    corner_deceleration: fields.bool(section, 290),
                })
            }
            "ACDBCURVEPATH" | "CURVEPATH" => {
                let section = "AcDbCurvePath";
                ClassObjectData::CurvePath(CurvePath {
                    class_version: fields.i32(section, 90),
                    entity: fields.handle(section, 340),
                })
            }
            "ACDBPOINTPATH" | "POINTPATH" => {
                let section = "AcDbPointPath";
                ClassObjectData::PointPath(PointPath {
                    class_version: fields.i16(section, 90),
                    point: fields.point3(section, 10),
                })
            }
            "TVDEVICEPROPERTIES" => {
                let section = "AcDbTvDeviceProperties";
                ClassObjectData::TvDeviceProperties(TvDeviceProperties {
                    flags: fields.i32(section, 90) as u32,
                    max_regen_threads: fields.i16(section, 70),
                    use_lut_palette: fields.i32(section, 91),
                    alternate_highlight: fields.i64(section, 160),
                    alternate_highlight_color: fields.i64(section, 161),
                    geometry_shader_usage: fields.i64(section, 162),
                    blending_mode: fields.i32(section, 92),
                    antialiasing_level: fields.f64(section, 40),
                    reserved_double: fields.f64(section, 41),
                })
            }
            "ACDBPOINTCLOUDDEF" | "POINTCLOUDDEF" => ClassObjectData::PointCloudDefinition(
                class_dxf_point_cloud_definition(&mut fields, "AcDbPointCloudDef"),
            ),
            "ACDBPOINTCLOUDDEFEX" | "POINTCLOUDDEFEX" => ClassObjectData::PointCloudDefinitionEx(
                class_dxf_point_cloud_definition(&mut fields, "AcDbPointCloudDefEx"),
            ),
            "ACDBPOINTCLOUDDEF_REACTOR" | "POINTCLOUDDEF_REACTOR" => {
                ClassObjectData::PointCloudDefinitionReactor(PointCloudDefinitionReactor {
                    class_version: fields.i32("AcDbPointCloudDefReactor", 90),
                })
            }
            "ACDBPOINTCLOUDDEF_REACTOR_EX" | "POINTCLOUDDEF_REACTOR_EX" => {
                ClassObjectData::PointCloudDefinitionReactorEx(PointCloudDefinitionReactor {
                    class_version: fields.i32("AcDbPointCloudDefReactorEx", 90),
                })
            }
            "ACDBPOINTCLOUDCOLORMAP" | "POINTCLOUDCOLORMAP" => {
                let section = "AcDbPointCloudColorMap";
                let class_version = fields.i16(section, 70);
                let default_intensity_scheme = fields.string(section, 1);
                let default_elevation_scheme = fields.string(section, 1);
                let default_classification_scheme = fields.string(section, 1);
                let color_ramps = class_dxf_point_cloud_ramps(&mut fields, section);
                let classification_color_ramps = class_dxf_point_cloud_ramps(&mut fields, section);
                ClassObjectData::PointCloudColorMap(PointCloudColorMap {
                    class_version,
                    default_intensity_scheme,
                    default_elevation_scheme,
                    default_classification_scheme,
                    color_ramps,
                    classification_color_ramps,
                })
            }
            "NAVISWORKSMODELDEF" | "COORDINATION_MODEL_DEFINITION" => {
                let section = "AcDbNavisworksModelDef";
                ClassObjectData::NavisworksModelDefinition(NavisworksModelDefinition {
                    flags: fields.i16(section, 70),
                    path: fields.string(section, 1),
                    status: fields.bool(section, 290),
                    extents_min: fields.point3(section, 10),
                    extents_max: fields.point3(section, 11),
                    host_drawing_visibility: fields.bool(section, 290),
                })
            }
            "CONTEXTDATAMANAGER" => {
                let section = "AcDbContextDataManager";
                let object_context = fields.handle(section, 340);
                let mut sub_managers = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    let handle = fields.handle(section, 340);
                    let mut entries = Vec::new();
                    for _ in 0..fields.i32(section, 91).max(0).min(100_000) {
                        entries.push(ContextDataEntry {
                            item: fields.handle(section, 350),
                            name: fields.string(section, 3),
                        });
                    }
                    sub_managers.push(ContextDataSubManager { handle, entries });
                }
                ClassObjectData::ContextDataManager(ContextDataManager {
                    object_context,
                    sub_managers,
                })
            }
            "SUNSTUDY" => {
                let section = "AcDbSunStudy";
                let class_version = fields.i32(section, 90);
                let setup_name = fields.string(section, 1);
                let description = fields.string(section, 2);
                let output_type = fields.i32(section, 70);
                let (use_subset, sheet_set_name, sheet_subset_name) = if output_type == 0 {
                    (
                        fields.bool(section, 290),
                        fields.string(section, 3),
                        fields.string(section, 4),
                    )
                } else {
                    (false, String::new(), String::new())
                };
                let select_dates_from_calendar = fields.bool(section, 291);
                let mut dates = Vec::new();
                for _ in 0..fields.i32(section, 91).max(0).min(10_000) {
                    dates.push(SunStudyDate {
                        julian_day: fields.i32(section, 90),
                        milliseconds: fields.i32(section, 90),
                    });
                }
                let select_range_of_dates = fields.bool(section, 292);
                let start_time = fields.i32(section, 93);
                let end_time = fields.i32(section, 94);
                let interval = fields.i32(section, 95);
                let mut hours = Vec::new();
                for _ in 0..fields.i32(section, 91).max(0).min(10_000) {
                    hours.push(fields.bool(section, 290));
                }
                ClassObjectData::SunStudy(SunStudy {
                    class_version,
                    setup_name,
                    description,
                    output_type,
                    use_subset,
                    sheet_set_name,
                    sheet_subset_name,
                    select_dates_from_calendar,
                    dates,
                    select_range_of_dates,
                    start_time,
                    end_time,
                    interval,
                    hours,
                    shade_plot_type: fields.i32(section, 74),
                    viewport_count: fields.i32(section, 75),
                    rows: fields.i32(section, 76),
                    columns: fields.i32(section, 77),
                    spacing: fields.f64(section, 40),
                    lock_viewports: fields.bool(section, 293),
                    label_viewports: fields.bool(section, 294),
                    page_setup_wizard: fields.handle(section, 340),
                    view: fields.handle(section, 341),
                    visual_style: fields.handle(section, 342),
                    text_style: fields.handle(section, 343),
                })
            }
            "DATATABLE" | "ACDBDATATABLE" => {
                let section = "AcDbDataTable";
                let flags = fields.i16(section, 70);
                let column_count = fields.i32(section, 90).max(0).min(100_000);
                let row_count = fields.i32(section, 91);
                let name = fields.string(section, 1);
                let mut columns = Vec::new();
                for _ in 0..column_count {
                    let value_type = fields.i32(section, 92);
                    let name = fields.string(section, 2);
                    let mut rows = Vec::new();
                    for _ in 0..row_count.max(0).min(100_000) {
                        rows.push(DataTableValue {
                            integer: fields.i32(section, 93),
                            real: fields.f64(section, 40),
                            text: fields.string(section, 3),
                        });
                    }
                    columns.push(DataTableColumn {
                        value_type,
                        name,
                        rows,
                    });
                }
                ClassObjectData::DataTable(DataTable {
                    flags,
                    name,
                    row_count,
                    columns,
                })
            }
            "DATALINK" => {
                let section = "AcDbDataLink";
                let data_adapter = fields.string(section, 1);
                let description = fields.string(section, 300);
                let tooltip = fields.string(section, 301);
                let connection_string = fields.string(section, 302);
                let option = fields.i32(section, 90);
                let update_option = fields.i32(section, 91);
                let flags = fields.i32(section, 92);
                let year = fields.i16(section, 170);
                let month = fields.i16(section, 171);
                let day = fields.i16(section, 172);
                let hour = fields.i16(section, 173);
                let minute = fields.i16(section, 174);
                let second = fields.i16(section, 175);
                let millisecond = fields.i16(section, 176);
                let path_option = fields.i16(section, 177);
                let status_flags = fields.i32(section, 93);
                let update_status = fields.string(section, 304);
                let mut custom_data = Vec::new();
                for _ in 0..fields.i32(section, 94).max(0).min(100_000) {
                    custom_data.push(DataLinkCustomData {
                        target: fields.handle(section, 330),
                        value: fields.string(section, 304),
                    });
                }
                ClassObjectData::DataLink(DataLink {
                    data_adapter,
                    description,
                    tooltip,
                    connection_string,
                    option,
                    update_option,
                    flags,
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                    millisecond,
                    path_option,
                    status_flags,
                    update_status,
                    custom_data,
                    hard_owner: fields.handle(section, 360),
                })
            }
            "ACDBPERSSUBENTMANAGER" | "PERSUBENTMGR" => {
                let section = "AcDbPersSubentManager";
                let class_version = fields.i32(section, 90);
                let reserved_zero = fields.i32(section, 90);
                let reserved_two = fields.i32(section, 90);
                let associated_step_count = fields.i32(section, 90);
                let associated_subentity_count = fields.i32(section, 90);
                let mut steps = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    steps.push(fields.i32(section, 90));
                }
                let mut subentities = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    subentities.push(fields.i32(section, 90));
                }
                ClassObjectData::PersistentSubentityManager(PersistentSubentityManager {
                    class_version,
                    reserved_zero,
                    reserved_two,
                    associated_step_count,
                    associated_subentity_count,
                    steps,
                    subentities,
                })
            }
            "GEOMAPIMAGE" => {
                let section = "AcDbGeomapImage";
                ClassObjectData::GeoMapImage(GeoMapImage {
                    class_version: fields.i32(section, 90),
                    origin: fields.point3(section, 10),
                    image_size: fields.point2(section, 13),
                    display_properties: fields.i16(section, 70),
                    clipping_enabled: fields.bool(section, 280),
                    brightness: fields.i16(section, 281) as u8,
                    contrast: fields.i16(section, 282) as u8,
                    fade: fields.i16(section, 283) as u8,
                })
            }
            "ACDBDETAILVIEWSTYLE" | "DETAILVIEWSTYLE" => {
                let base_section = "AcDbModelDocViewStyle";
                let section = "AcDbDetailViewStyle";
                let base = ModelDocViewStyle {
                    class_version: fields.i16(base_section, 70),
                    description: fields.string(base_section, 3),
                    modified_for_recompute: fields.bool(base_section, 290),
                    display_name: fields.string(base_section, 300),
                    flags: fields.i32(base_section, 90),
                };
                let _minor_version = fields.i16(section, 71);
                let flags = fields.i32(section, 90);
                let _identifier_group = fields.i16(section, 71);
                let identifier_style = fields.handle(section, 340);
                let identifier_color = Color::from_index(fields.i16(section, 62));
                let identifier_height = fields.f64(section, 40);
                let arrow_symbol = fields.handle(section, 340);
                let arrow_symbol_color = Color::from_index(fields.i16(section, 62));
                let arrow_symbol_size = fields.f64(section, 40);
                let identifier_excluded_characters = fields.string(section, 300);
                let identifier_offset = fields.f64(section, 40);
                let identifier_placement = fields.i16(section, 280) as u8;
                let _boundary_group = fields.i16(section, 71);
                let boundary_linetype = fields.handle(section, 340);
                let boundary_lineweight = fields.i32(section, 90);
                let boundary_color = Color::from_index(fields.i16(section, 62));
                let _view_label_group = fields.i16(section, 71);
                let view_label_text_style = fields.handle(section, 340);
                let view_label_text_color = Color::from_index(fields.i16(section, 62));
                let view_label_text_height = fields.f64(section, 40);
                let view_label_attachment = fields.i32(section, 90);
                let view_label_offset = fields.f64(section, 40);
                let view_label_alignment = fields.i32(section, 90);
                let view_label_pattern = fields.string(section, 300);
                let _connection_group = fields.i16(section, 71);
                ClassObjectData::DetailViewStyle(ClassDetailViewStyle {
                    base,
                    class_version: fields.i16(section, 70),
                    flags,
                    identifier_style,
                    identifier_color,
                    identifier_height,
                    identifier_excluded_characters,
                    identifier_offset,
                    identifier_placement,
                    arrow_symbol,
                    arrow_symbol_color,
                    arrow_symbol_size,
                    boundary_linetype,
                    boundary_lineweight,
                    boundary_color,
                    view_label_text_style,
                    view_label_text_color,
                    view_label_text_height,
                    view_label_attachment,
                    view_label_offset,
                    view_label_alignment,
                    view_label_pattern,
                    connection_linetype: fields.handle(section, 340),
                    connection_lineweight: fields.i32(section, 90),
                    connection_color: Color::from_index(fields.i16(section, 62)),
                    border_linetype: fields.handle(section, 340),
                    border_lineweight: fields.i32(section, 90),
                    border_color: Color::from_index(fields.i16(section, 62)),
                    model_edge: fields.i16(section, 280) as u8,
                })
            }
            "ACDBSECTIONVIEWSTYLE" | "SECTIONVIEWSTYLE" => {
                let base_section = "AcDbModelDocViewStyle";
                let section = "AcDbSectionViewStyle";
                let base = ModelDocViewStyle {
                    class_version: fields.i16(base_section, 70),
                    description: fields.string(base_section, 3),
                    modified_for_recompute: fields.bool(base_section, 290),
                    display_name: fields.string(base_section, 300),
                    flags: fields.i32(base_section, 90),
                };
                let _minor_version = fields.i16(section, 71);
                let flags = fields.i32(section, 90);
                let _identifier_group = fields.i16(section, 71);
                let identifier_style = fields.handle(section, 340);
                let identifier_color = Color::from_index(fields.i16(section, 62));
                let identifier_height = fields.f64(section, 40);
                let arrow_start_symbol = fields.handle(section, 340);
                let arrow_end_symbol = fields.handle(section, 340);
                let arrow_symbol_color = Color::from_index(fields.i16(section, 62));
                let arrow_symbol_size = fields.f64(section, 40);
                let identifier_excluded_characters = fields.string(section, 300);
                let arrow_symbol_extension_length = fields.f64(section, 40);
                let identifier_position = fields.i32(section, 90);
                let identifier_offset = fields.f64(section, 40);
                let arrow_position = fields.i32(section, 90);
                let _plane_group = fields.i16(section, 71);
                let plane_linetype = fields.handle(section, 340);
                let plane_lineweight = fields.i32(section, 90);
                let plane_color = Color::from_index(fields.i16(section, 62));
                let bend_linetype = fields.handle(section, 340);
                let bend_lineweight = fields.i32(section, 90);
                let bend_color = Color::from_index(fields.i16(section, 62));
                let bend_line_length = fields.f64(section, 40);
                let end_line_overshoot = fields.f64(section, 40);
                let end_line_length = fields.f64(section, 40);
                let _view_label_group = fields.i16(section, 71);
                let view_label_text_style = fields.handle(section, 340);
                let view_label_text_color = Color::from_index(fields.i16(section, 62));
                let view_label_text_height = fields.f64(section, 40);
                let view_label_attachment = fields.i32(section, 90);
                let view_label_offset = fields.f64(section, 40);
                let view_label_alignment = fields.i32(section, 90);
                let view_label_pattern = fields.string(section, 300);
                let _hatch_group = fields.i16(section, 71);
                let hatch_color = Color::from_index(fields.i16(section, 62));
                let hatch_background_color = Color::from_index(fields.i16(section, 62));
                let hatch_pattern = fields.string(section, 300);
                let hatch_scale = fields.f64(section, 40);
                let hatch_transparency = fields.i32(section, 90);
                let mut hatch_angles = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    hatch_angles.push(fields.f64(section, 40));
                }
                ClassObjectData::SectionViewStyle(ClassSectionViewStyle {
                    base,
                    class_version: fields.i16(section, 70),
                    flags,
                    identifier_style,
                    identifier_color,
                    identifier_height,
                    arrow_start_symbol,
                    arrow_end_symbol,
                    arrow_symbol_color,
                    arrow_symbol_size,
                    identifier_excluded_characters,
                    arrow_symbol_extension_length,
                    plane_linetype,
                    plane_lineweight,
                    plane_color,
                    bend_linetype,
                    bend_lineweight,
                    bend_color,
                    bend_line_length,
                    end_line_length,
                    view_label_text_style,
                    view_label_text_color,
                    view_label_text_height,
                    view_label_attachment,
                    view_label_offset,
                    view_label_alignment,
                    view_label_pattern,
                    hatch_color,
                    hatch_background_color,
                    hatch_pattern,
                    hatch_scale,
                    hatch_transparency,
                    reserved_flags: [fields.bool(section, 290), fields.bool(section, 290)],
                    identifier_position,
                    identifier_offset,
                    arrow_position,
                    end_line_overshoot,
                    hatch_angles,
                })
            }
            "ACMECOMMANDHISTORY" => {
                ClassObjectData::AcMeCommandHistory(AcMeCommandHistory::default())
            }
            "ACMESCOPE" => ClassObjectData::AcMeScope(AcMeScope::default()),
            "ACMESTATEMGR" => ClassObjectData::AcMeStateManager(AcMeStateManager::default()),
            "CSACDOCUMENTOPTIONS" => {
                ClassObjectData::CsacDocumentOptions(CsacDocumentOptions::default())
            }
            "ACDBVIEWREP" => {
                let section = "AcDbViewRep";
                let header_values = [
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                ];
                let name = fields.string(section, 1);
                let scale = fields.i32(section, 90);
                let header_status = fields.i32(section, 90);
                let description = fields.string(section, 1);
                let source_id = fields.i64(section, 160);
                let source_enabled = fields.bool(section, 290);
                let source_version = fields.i32(section, 90);
                let model_id = fields.i64(section, 160);
                let data1 = fields.i32(section, 90);
                let data2 = fields.i16(section, 70);
                let data3 = fields.i16(section, 70);
                let mut data4 = [0; 8];
                for item in &mut data4 {
                    *item = fields.i32(section, 280) as u8;
                }
                let marker = fields.i32(section, 280) as u8;
                let mut transform = [0.0; 16];
                for item in &mut transform {
                    *item = fields.f64(section, 40);
                }
                let transform_version = fields.i32(section, 90);
                let database_id = fields.i64(section, 160);
                let geometry_version = fields.i32(section, 90);
                let geometry_marker = fields.i32(section, 90);
                let mut sketches = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    let id = fields.i32(section, 90);
                    let sketch_version = fields.i32(section, 90);
                    let mut references = Vec::new();
                    for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                        references.push(ViewRepSketchReference {
                            object: fields.handle(section, 330),
                            flag: fields.bool(section, 290),
                        });
                    }
                    let reserved = fields.i32(section, 90);
                    let enabled = fields.bool(section, 290);
                    let type_code = fields.i32(section, 90);
                    let geometry = match type_code {
                        19 | 23 => ViewRepSketchGeometry::Line {
                            type_code,
                            first: fields.point3(section, 10),
                            second: fields.point3(section, 10),
                        },
                        11 => ViewRepSketchGeometry::Circle {
                            type_code,
                            center: fields.point3(section, 10),
                            normal: fields.point3(section, 10),
                            direction: fields.point3(section, 10),
                            radius: fields.f64(section, 40),
                            start_parameter: fields.f64(section, 40),
                            end_parameter: fields.f64(section, 40),
                            reserved: fields.f64(section, 40),
                        },
                        42 => {
                            let flags = [fields.bool(section, 70), fields.bool(section, 70)];
                            let degree = fields.i32(section, 90);
                            let tolerance = fields.f64(section, 40);
                            let knot_header = [
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                            ];
                            let mut knots = Vec::new();
                            for _ in 0..knot_header[0].max(0).min(100_000) {
                                knots.push(fields.f64(section, 40));
                            }
                            let weight_header = [
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                            ];
                            let mut weights = Vec::new();
                            for _ in 0..weight_header[0].max(0).min(100_000) {
                                weights.push(fields.f64(section, 40));
                            }
                            let point_header = [
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                                fields.i32(section, 90),
                            ];
                            let mut control_points = Vec::new();
                            for _ in 0..point_header[0].max(0).min(100_000) {
                                control_points.push(fields.point3(section, 10));
                            }
                            ViewRepSketchGeometry::Nurb {
                                type_code,
                                flags,
                                degree,
                                tolerance,
                                knot_header,
                                knots,
                                weight_header,
                                weights,
                                point_header,
                                control_points,
                            }
                        }
                        _ => ViewRepSketchGeometry::None,
                    };
                    let final_flag = fields.bool(section, 290);
                    sketches.push(ViewRepSketch {
                        id,
                        version: sketch_version,
                        references,
                        reserved,
                        enabled,
                        geometry,
                        final_flag,
                    });
                }
                let related_objects = [fields.handle(section, 330), fields.handle(section, 330)];
                let source_manager = fields.handle(section, 340);
                let owned_objects = [fields.handle(section, 360), fields.handle(section, 360)];
                let optional_objects = [fields.handle(section, 330), fields.handle(section, 330)];
                let position = fields.point2(section, 10);
                let rotation = fields.f64(section, 40);
                let orientation = fields.handle(section, 340);
                let is_active = fields.bool(section, 290);
                let projection = fields.i16(section, 70);
                let linked_views = [fields.handle(section, 330), fields.handle(section, 330)];
                let mut section_sketches = Vec::new();
                for _ in 0..fields.i32(section, 90).max(0).min(100_000) {
                    let class_name = fields.string(section, 1);
                    let path_count = fields.i16(section, 70).max(0).min(10_000) as usize;
                    let mut objects = Vec::with_capacity(path_count.saturating_add(1));
                    for _ in 0..=path_count {
                        objects.push(fields.handle(section, 330));
                    }
                    section_sketches.push(ViewRepObjectPath {
                        class_name,
                        objects,
                    });
                }
                let action_mode = fields.i32(section, 90);
                let action = if action_mode != 0 {
                    Some(fields.handle(section, 360))
                } else {
                    None
                };
                let has_parent = fields.bool(section, 290);
                let parent = fields.handle(section, 330);
                let tail_version = fields.i32(section, 90);
                let tail_state = fields.i32(section, 90);
                let tail_id = fields.i64(section, 160);
                let path_count = fields.i32(section, 90);
                let path_version = fields.i32(section, 90);
                let path_id = fields.i64(section, 160);
                let has_block_path = fields.bool(section, 290);
                let block_path = if has_block_path {
                    let class_name = fields.string(section, 1);
                    let version = fields.i32(section, 90);
                    let mut entries = Vec::new();
                    for _ in 0..fields.i32(section, 91).max(0).min(100_000) {
                        entries.push(ViewRepBlockPathEntry {
                            flag: fields.i32(section, 281) as u8,
                            kind: fields.i32(section, 280) as u8,
                            object: fields.handle(section, 332),
                        });
                    }
                    Some(ViewRepBlockPath {
                        class_name,
                        version,
                        entries,
                    })
                } else {
                    None
                };
                let style = fields.handle(section, 340);
                ClassObjectData::ViewRep(ViewRep {
                    header_values,
                    name,
                    scale,
                    header_status,
                    description,
                    source_id,
                    source_enabled,
                    source_version,
                    model_id,
                    guid: ViewRepGuid {
                        data1,
                        data2,
                        data3,
                        data4,
                    },
                    marker,
                    transform,
                    transform_version,
                    database_id,
                    geometry_version,
                    geometry_marker,
                    sketches,
                    related_objects,
                    source_manager,
                    owned_objects,
                    optional_objects,
                    position,
                    rotation,
                    orientation,
                    is_active,
                    projection,
                    linked_views,
                    section_sketches,
                    action_mode,
                    action,
                    has_parent,
                    parent,
                    tail_version,
                    tail_state,
                    tail_id,
                    path_count,
                    path_version,
                    path_id,
                    has_block_path,
                    block_path,
                    style,
                })
            }
            "ACDBVIEWREPMODELSPACESOURCE" => {
                let section = "AcDbViewRepModelSpaceSource";
                let enabled = fields.bool(section, 290);
                let header_values = [
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                    fields.i32(section, 90),
                ];
                let mut transform = [0.0; 16];
                for item in &mut transform {
                    *item = fields.f64(section, 40);
                }
                let source_version = fields.i32(section, 90);
                let source_status = fields.i32(section, 90);
                let model = fields.handle(section, 5);
                let data1 = fields.i32(section, 90);
                let data2 = fields.i16(section, 70);
                let data3 = fields.i16(section, 70);
                let mut data4 = [0; 8];
                for item in &mut data4 {
                    *item = fields.i32(section, 280) as u8;
                }
                ClassObjectData::ViewRepModelSpaceSource(ViewRepModelSpaceSource {
                    enabled,
                    header_values,
                    transform,
                    source_version,
                    source_status,
                    model,
                    guid: ViewRepGuid {
                        data1,
                        data2,
                        data3,
                        data4,
                    },
                    references: [fields.handle(section, 330), fields.handle(section, 330)],
                    tail_values: [fields.i32(section, 90), fields.i32(section, 90)],
                    orientation: fields.handle(section, 350),
                })
            }
            "ACDBVIEWREPSOURCEMGR" => {
                let section = "AcDbViewRepSourceMgr";
                ClassObjectData::ViewRepSourceManager(ViewRepSourceManager {
                    has_source: fields.bool(section, 290),
                    source: fields.handle(section, 350),
                    status: fields.i32(section, 90),
                })
            }
            "ACDBVIEWREPSTANDARD" => {
                let section = "AcDbViewRepStandard";
                ClassObjectData::ViewRepStandard(ViewRepStandard {
                    values: [
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                        fields.i32(section, 90),
                    ],
                })
            }
            "ACDBVIEWREPORIENTATIONDEF" => ClassObjectData::ViewRepOrientationDefinition,
            "ACDBVIEWREPORIENTATION" => {
                let section = "AcDbViewRepOrientation";
                ClassObjectData::ViewRepOrientation(ViewRepOrientation {
                    camera: fields.point3(section, 10),
                    target: fields.point3(section, 10),
                    normal: fields.point3(section, 210),
                })
            }
            "ACDBVIEWREPSECTIONDEFINITION" => {
                let section = "AcDbViewRepSectionDefinition";
                ClassObjectData::ViewRepSectionDefinition(ViewRepSectionDefinition {
                    version: fields.i32(section, 90),
                    section_depth: fields.f64(section, 40),
                    flags: [fields.i32(section, 90), fields.i32(section, 90)],
                })
            }
            "ACDBSYMODELSPACEVIEWSELSET" => {
                let section = "AcDbViewRepModelSpaceViewSelSet";
                let version = fields.i32(section, 90);
                let count = fields.i32(section, 90).max(0).min(100_000);
                let mut entities = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    entities.push(fields.handle(section, 330));
                }
                ClassObjectData::ViewRepModelSpaceViewSelectionSet(
                    ViewRepModelSpaceViewSelectionSet { version, entities },
                )
            }
            _ => ClassObjectData::Empty,
        };
        Ok(object)
    }

    /// Read the OBJECTS section
    pub fn read_objects(&mut self, document: &mut CadDocument) -> Result<()> {
        // Clear default objects created by initialize_defaults() before
        // reading the file's own objects.  The file supplies its own
        // complete set of dictionaries, layouts, etc.  Keeping defaults
        // causes phantom layouts with stale block_record handles and
        // orphaned dictionary entries.
        document.objects.clear();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }

            if pair.code == 0 {
                let before = document.objects.len();
                match pair.value_string.as_str() {
                    "DICTIONARY" => {
                        if let Some(obj) = self.read_dictionary()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::Dictionary(obj));
                        }
                    }
                    "LAYOUT" => {
                        if let Some(obj) = self.read_layout()? {
                            document.objects.insert(obj.handle, ObjectType::Layout(obj));
                        }
                    }
                    "XRECORD" => {
                        if let Some(obj) = self.read_xrecord()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::XRecord(obj));
                        }
                    }
                    "GROUP" => {
                        if let Some(obj) = self.read_group()? {
                            document.objects.insert(obj.handle, ObjectType::Group(obj));
                        }
                    }
                    "MLINESTYLE" => {
                        if let Some(obj) = self.read_mlinestyle_object()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::MLineStyle(obj));
                        }
                    }
                    "IMAGEDEF" => {
                        if let Some(obj) = self.read_image_definition()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::ImageDefinition(obj));
                        }
                    }
                    "PDFDEFINITION" | "DWFDEFINITION" | "DGNDEFINITION" => {
                        use crate::entities::underlay::UnderlayType;
                        let utype = match pair.value_string.as_str() {
                            "DWFDEFINITION" => UnderlayType::Dwf,
                            "DGNDEFINITION" => UnderlayType::Dgn,
                            _ => UnderlayType::Pdf,
                        };
                        if let Some(obj) = self.read_underlay_definition(utype)? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::UnderlayDefinition(obj));
                        }
                    }
                    "MLEADERSTYLE" => {
                        if let Some(obj) = self.read_multileader_style()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::MultiLeaderStyle(obj));
                        }
                    }
                    "PLOTSETTINGS" => {
                        if let Some(obj) = self.read_plot_settings()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::PlotSettings(obj));
                        }
                    }
                    "TABLESTYLE" => {
                        if let Some(obj) = self.read_table_style()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::TableStyle(obj));
                        }
                    }
                    "TABLECONTENT" => {
                        if let Some(obj) = self.read_table_content_object_typed_dxf()? {
                            document
                                .objects
                                .insert(obj.common.handle, ObjectType::TableContent(obj));
                        }
                    }
                    "SCALE" => {
                        if let Some(obj) = self.read_scale()? {
                            document.objects.insert(obj.handle, ObjectType::Scale(obj));
                        }
                    }
                    "SORTENTSTABLE" => {
                        if let Some(obj) = self.read_sort_entities_table(document.version)? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::SortEntitiesTable(obj));
                        }
                    }
                    "DICTIONARYVAR" => {
                        if let Some(obj) = self.read_dictionary_variable()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::DictionaryVariable(obj));
                        }
                    }
                    "VISUALSTYLE" => {
                        if let Some(obj) = self.read_visualstyle()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::VisualStyle(obj));
                        }
                    }
                    "MATERIAL" => {
                        if let Some(obj) = self.read_material()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::Material(obj));
                        }
                    }
                    "IMAGEDEF_REACTOR" => {
                        if let Some(obj) = self.read_imagedef_reactor()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::ImageDefinitionReactor(obj));
                        }
                    }
                    "GEODATA" => {
                        let obj = self.read_geodata()?;
                        document
                            .objects
                            .insert(obj.handle, ObjectType::GeoData(obj));
                    }
                    "BREAKDATA"
                    | "BREAKPOINTREF"
                    | "IDBUFFER"
                    | "INDEX"
                    | "LAYER_INDEX"
                    | "PARTIAL_VIEWING_FILTER"
                    | "CELLSTYLEMAP"
                    | "TABLEGEOMETRY"
                    | "LONG_TRANSACTION"
                    | "ACDSRECORD"
                    | "ACDSSCHEMA"
                    | "DUMMY"
                    | "OBJECT_PTR" => {
                        let obj = self.read_data_object_dxf(&pair.value_string)?;
                        document
                            .objects
                            .insert(obj.handle, ObjectType::DataObject(obj));
                    }
                    // AutoCAD emits the record name with an underscore; accept
                    // both spellings so the INSERT xclip chain resolves.
                    "SPATIAL_FILTER" | "SPATIALFILTER" => {
                        if let Some(obj) = self.read_spatial_filter()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::SpatialFilter(obj));
                        }
                    }
                    "RASTERVARIABLES" => {
                        if let Some(obj) = self.read_raster_variables()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::RasterVariables(obj));
                        }
                    }
                    "DBCOLOR" => {
                        if let Some(obj) = self.read_bookcolor()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::BookColor(obj));
                        }
                    }
                    "ACDBPLACEHOLDER" => {
                        let obj = self.read_stub_object::<PlaceHolder>()?;
                        document
                            .objects
                            .insert(obj.handle, ObjectType::PlaceHolder(obj));
                    }
                    "ACDBDICTIONARYWDFLT" => {
                        // Already handled as DICTIONARY above — this handles standalone cases
                        if let Some(obj) = self.read_dict_with_default()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::DictionaryWithDefault(obj));
                        }
                    }
                    "WIPEOUTVARIABLES" => {
                        if let Some(obj) = self.read_wipeout_variables()? {
                            document
                                .objects
                                .insert(obj.handle, ObjectType::WipeoutVariables(obj));
                        }
                    }
                    "FIELD" | "ACDBFIELD" => {
                        let obj = self.read_field_object_dxf(document.version)?;
                        document.fields.insert(
                            obj.handle,
                            crate::document::FieldDef {
                                handle: obj.handle,
                                owner: obj.owner,
                                evaluator: obj.evaluator_id.clone(),
                                code: obj.code.clone(),
                                objects: obj.referenced_objects.clone(),
                            },
                        );
                        document.objects.insert(obj.handle, ObjectType::Field(obj));
                    }
                    "FIELDLIST" | "ACDBFIELDLIST" => {
                        let obj = self.read_field_list_dxf()?;
                        document
                            .objects
                            .insert(obj.handle, ObjectType::FieldList(obj));
                    }
                    name if is_dynamic_block_object_name(name) => {
                        let object = self.read_dynamic_block_object_dxf(name, document.version)?;
                        if let DynamicBlockData::VisibilityParameter(parameter) = &object.data {
                            document
                                .block_visibility_params
                                .insert(parameter.handle, parameter.clone());
                            document.objects.insert(
                                parameter.handle,
                                ObjectType::BlockVisibilityParameter(parameter.clone()),
                            );
                        } else {
                            document
                                .objects
                                .insert(object.handle, ObjectType::DynamicBlock(object));
                        }
                    }
                    name if is_object_context_name(name) => {
                        let object = self.read_object_context_data_dxf(name)?;
                        if !object.scale.is_null() {
                            document.context_scales.insert(object.handle, object.scale);
                        }
                        document
                            .objects
                            .insert(object.handle, ObjectType::ObjectContextData(object));
                    }
                    name if is_associative_object_name(name) => {
                        let object = self.read_associative_object_dxf(name, document.version)?;
                        document
                            .objects
                            .insert(object.handle, ObjectType::Associative(object));
                    }
                    name if is_class_object_name(name) => {
                        let object = self.read_class_object_dxf(name)?;
                        if let ClassObjectData::SectionViewStyle(style) = &object.data {
                            document.section_view_style = Some(crate::entities::SectionViewStyle {
                                show_arrows: style.flags & 0x02 != 0,
                                show_plane_line: style.flags & 0x08 != 0,
                                show_end_lines: style.flags & 0x20 != 0,
                                arrow_size: style.arrow_symbol_size,
                                arrow_extension: style.arrow_symbol_extension_length,
                                label_height: style.identifier_height,
                                label_offset: style.identifier_offset,
                                label_position: style.identifier_position,
                                arrow_position: style.arrow_position,
                                end_line_length: style.end_line_length,
                                end_line_overshoot: style.end_line_overshoot,
                                arrow_start_handle: style.arrow_start_symbol.value(),
                                arrow_end_handle: style.arrow_end_symbol.value(),
                                arrow_is_default: style.arrow_start_symbol.is_null()
                                    && style.arrow_end_symbol.is_null(),
                            });
                        }
                        document
                            .objects
                            .insert(object.handle, ObjectType::ClassObject(object));
                    }
                    name if is_dgn_line_style_name(name) => {
                        let object = self.read_dgn_line_style_object(name)?;
                        document
                            .objects
                            .insert(object.handle, ObjectType::DgnLineStyle(object));
                    }
                    "ACAD_PROXY_OBJECT" => {
                        let object = self.read_proxy_object_dxf()?;
                        let handle = object.handle;
                        let object_type = if let Some(envelope) =
                            crate::objects::semantic_property::decode_registered_class_envelope(
                                &object.payload,
                            ) {
                            ObjectType::RegisteredClass(RegisteredClassObject {
                                handle: object.handle,
                                owner: object.owner,
                                reactors: object.reactors,
                                xdictionary_handle: object.xdictionary_handle,
                                dxf_name: envelope.dxf_name,
                                cpp_class_name: envelope.cpp_class_name,
                                properties: envelope.properties,
                                payload: envelope.payload,
                                object_ids: object.object_ids,
                                raw_dwg_data: None,
                                raw_dwg_handle_bits: 0,
                                raw_dwg_version: None,
                            })
                        } else {
                            ObjectType::ProxyObject(object)
                        };
                        document.objects.insert(handle, object_type);
                    }
                    name if is_registered_class_object_name(name) => {
                        let object = self.read_registered_class_object(name)?;
                        document
                            .objects
                            .insert(object.handle, ObjectType::RegisteredClass(object));
                    }
                    _ => {
                        document.notifications.notify(
                            crate::notification::NotificationType::NotImplemented,
                            format!(
                                "Object not supported, read as Unknown: {}",
                                pair.value_string
                            ),
                        );
                        let type_name = pair.value_string.clone();
                        let (handle, owner, raw_codes) = self.read_unknown_object_full()?;
                        document.objects.insert(
                            handle,
                            ObjectType::Unknown {
                                type_name,
                                handle,
                                owner,
                                raw_dxf_codes: if raw_codes.is_empty() {
                                    None
                                } else {
                                    Some(raw_codes)
                                },
                                raw_dwg_data: None,
                                raw_dwg_handle_bits: 0,
                                raw_dwg_version: None,
                            },
                        );
                    }
                }
                self.decoded_records = self
                    .decoded_records
                    .saturating_add(document.objects.len().saturating_sub(before));
            }
        }

        let viewport_handles_by_block: std::collections::HashMap<Handle, Vec<Handle>> = document
            .block_records
            .iter()
            .map(|block| {
                let viewports = block
                    .entity_handles
                    .iter()
                    .copied()
                    .filter(|handle| {
                        document
                            .entity_index
                            .get(handle)
                            .and_then(|index| document.entities.get(*index))
                            .is_some_and(|entity| {
                                matches!(entity.as_ref(), EntityType::Viewport(_))
                            })
                    })
                    .collect();
                (block.handle, viewports)
            })
            .collect();
        for object in document.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                if let Some(viewports) = viewport_handles_by_block.get(&layout.block_record) {
                    layout.viewports = viewports.clone();
                }
            }
        }

        // ── Post-pass: resolve the root dictionary handle ─────────────
        // Unlike DWG, the DXF stream carries no NAMED OBJECTS DICTIONARY
        // handle, so the header keeps the default (0x0C) set by
        // initialize_defaults(). If the file's real root dictionary lives
        // at another handle, the writer would emit the wrong dictionary
        // first in the OBJECTS section and consumers would audit away the
        // real root's children as orphans (issue #64 follow-up). Scan for
        // the dictionary owned by NULL.
        let current_root = document.header.named_objects_dict_handle;
        let current_is_root = matches!(
            document.objects.get(&current_root),
            Some(crate::objects::ObjectType::Dictionary(dict)) if dict.owner.is_null()
        );
        if !current_is_root {
            let mut best = Handle::NULL;
            let mut best_count = 0usize;
            for (handle, object) in &document.objects {
                if let crate::objects::ObjectType::Dictionary(dict) = object {
                    if dict.owner.is_null() {
                        if dict.entries.len() > best_count
                            || (dict.entries.len() == best_count && handle.value() > best.value())
                        {
                            best = *handle;
                            best_count = dict.entries.len();
                        }
                    }
                }
            }
            if !best.is_null() {
                document.header.named_objects_dict_handle = best;
            }
        }

        Ok(())
    }

    /// Read a DICTIONARY object
    fn read_dictionary(&mut self) -> Result<Option<Dictionary>> {
        let mut dict = Dictionary::new();
        let mut current_key: Option<String> = None;

        while let Some(pair) = self.reader.read_pair()? {
            match pair.code {
                0 => {
                    // Next object - push back and break
                    self.reader.push_back(pair);
                    break;
                }
                5 => {
                    // Handle
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        dict.handle = Handle::new(h);
                    }
                }
                330 => {
                    // Owner handle
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        dict.owner = Handle::new(h);
                    }
                }
                281 => {
                    // Duplicate record cloning flag
                    if let Some(value) = pair.as_i16() {
                        dict.duplicate_cloning = value;
                    }
                }
                280 => {
                    // Hard owner flag
                    if let Some(value) = pair.as_i16() {
                        dict.hard_owner = value != 0;
                    }
                }
                3 => {
                    // Entry key (name)
                    current_key = Some(pair.value_string.clone());
                }
                350 | 360 => {
                    // Entry value (handle) - 350 is soft owner, 360 is hard owner.
                    // When the dictionary-wide hard-owner flag (280) is set,
                    // every entry is hard-owned regardless of the group code
                    // the producer used. Canonical hard-owner NOD keys are
                    // treated the same way so the model matches what the
                    // writer emits and write→read→write cycles stay stable
                    // (issue #51).
                    if let Some(key) = current_key.take() {
                        if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                            if pair.code == 360
                                || dict.hard_owner
                                || Dictionary::is_canonical_hard_owner_key(&key)
                            {
                                dict.set_entry_hard_owner(&key, true);
                            }
                            dict.add_entry(key, Handle::new(h));
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(Some(dict))
    }

    /// Read a LAYOUT object
    fn read_layout(&mut self) -> Result<Option<Layout>> {
        let mut layout = Layout::new("");

        // Track which subclass we're in: 0=header, 1=AcDbPlotSettings, 2=AcDbLayout
        let mut section = 0u8;
        let mut plot_settings_codes: Vec<(i32, String)> = Vec::new();
        // Track owner vs block_record — both use code 330
        let mut owner_set = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => {
                    // Subclass marker transitions
                    match pair.value_string.as_str() {
                        "AcDbPlotSettings" => section = 1,
                        "AcDbLayout" => section = 2,
                        _ => {}
                    }
                    continue;
                }
                102 => {
                    // Extension dictionary / reactor groups (header area)
                    if pair.value_string == "{ACAD_XDICTIONARY" {
                        // Next pair is the xdictionary handle (code 360), then closing "}"
                        while let Some(p2) = self.reader.read_pair()? {
                            if p2.code == 360 {
                                if let Ok(h) = u64::from_str_radix(&p2.value_string, 16) {
                                    layout.xdictionary_handle = Some(Handle::new(h));
                                }
                            }
                            if p2.code == 102 {
                                break;
                            } // closing "}"
                        }
                    } else if pair.value_string == "{ACAD_REACTORS" {
                        while let Some(p2) = self.reader.read_pair()? {
                            if p2.code == 102 {
                                break;
                            }
                            if p2.code == 330 {
                                if let Ok(h) = u64::from_str_radix(&p2.value_string, 16) {
                                    layout.reactors.push(Handle::new(h));
                                }
                            }
                        }
                    }
                    continue;
                }
                _ => {}
            }

            match section {
                0 => {
                    // Before any subclass: handle, owner
                    match pair.code {
                        5 => {
                            if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                layout.handle = Handle::new(h);
                            }
                        }
                        330 => {
                            if !owner_set {
                                if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                    layout.owner = Handle::new(h);
                                }
                                owner_set = true;
                            }
                        }
                        _ => {}
                    }
                }
                1 => {
                    // AcDbPlotSettings — capture all codes as raw pairs
                    plot_settings_codes.push((pair.code, pair.value_string.clone()));
                }
                2 => {
                    // AcDbLayout — parse the layout-specific fields
                    match pair.code {
                        1 => layout.name = pair.value_string.clone(),
                        70 => {
                            if let Some(v) = pair.as_i16() {
                                layout.flags = v;
                            }
                        }
                        71 => {
                            if let Some(v) = pair.as_i16() {
                                layout.tab_order = v;
                            }
                        }
                        10 => {
                            if let Some(v) = pair.as_double() {
                                layout.min_limits.0 = v;
                            }
                        }
                        20 => {
                            if let Some(v) = pair.as_double() {
                                layout.min_limits.1 = v;
                            }
                        }
                        11 => {
                            if let Some(v) = pair.as_double() {
                                layout.max_limits.0 = v;
                            }
                        }
                        21 => {
                            if let Some(v) = pair.as_double() {
                                layout.max_limits.1 = v;
                            }
                        }
                        12 => {
                            if let Some(v) = pair.as_double() {
                                layout.insertion_base.0 = v;
                            }
                        }
                        22 => {
                            if let Some(v) = pair.as_double() {
                                layout.insertion_base.1 = v;
                            }
                        }
                        32 => {
                            if let Some(v) = pair.as_double() {
                                layout.insertion_base.2 = v;
                            }
                        }
                        14 => {
                            if let Some(v) = pair.as_double() {
                                layout.min_extents.0 = v;
                            }
                        }
                        24 => {
                            if let Some(v) = pair.as_double() {
                                layout.min_extents.1 = v;
                            }
                        }
                        34 => {
                            if let Some(v) = pair.as_double() {
                                layout.min_extents.2 = v;
                            }
                        }
                        15 => {
                            if let Some(v) = pair.as_double() {
                                layout.max_extents.0 = v;
                            }
                        }
                        25 => {
                            if let Some(v) = pair.as_double() {
                                layout.max_extents.1 = v;
                            }
                        }
                        35 => {
                            if let Some(v) = pair.as_double() {
                                layout.max_extents.2 = v;
                            }
                        }
                        330 => {
                            // In AcDbLayout, code 330 = block_record handle
                            if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                layout.block_record = Handle::new(h);
                            }
                        }
                        331 => {
                            // Viewport handle
                            if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                layout.viewport = Handle::new(h);
                            }
                        }
                        345 => {
                            if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                layout.named_ucs = Handle::new(h);
                            }
                        }
                        346 => {
                            if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                                layout.base_ucs = Handle::new(h);
                            }
                        }
                        146 => {
                            if let Some(v) = pair.as_double() {
                                layout.elevation = v;
                            }
                        }
                        13 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_origin.0 = v;
                            }
                        }
                        23 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_origin.1 = v;
                            }
                        }
                        33 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_origin.2 = v;
                            }
                        }
                        16 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_x_axis.0 = v;
                            }
                        }
                        26 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_x_axis.1 = v;
                            }
                        }
                        36 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_x_axis.2 = v;
                            }
                        }
                        17 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_y_axis.0 = v;
                            }
                        }
                        27 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_y_axis.1 = v;
                            }
                        }
                        37 => {
                            if let Some(v) = pair.as_double() {
                                layout.ucs_y_axis.2 = v;
                            }
                        }
                        76 => {
                            if let Some(v) = pair.as_i16() {
                                layout.ucs_ortho_type = v;
                            }
                        }
                        _ => {} // codes not currently stored
                    }
                }
                _ => {}
            }
        }

        if !plot_settings_codes.is_empty() {
            for &(code, ref val) in &plot_settings_codes {
                match code {
                    1 => layout.plot_page_name = val.clone(),
                    2 => layout.plot_printer_name = val.clone(),
                    4 => layout.paper_size = val.clone(),
                    6 => layout.plot_view_name = val.clone(),
                    7 => layout.plot_style_sheet = val.clone(),
                    40 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_margin_left = v;
                        }
                    }
                    41 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_margin_bottom = v;
                        }
                    }
                    42 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_margin_right = v;
                        }
                    }
                    43 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_margin_top = v;
                        }
                    }
                    44 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.paper_width = v;
                        }
                    }
                    45 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.paper_height = v;
                        }
                    }
                    46 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_origin_x = v;
                        }
                    }
                    47 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_origin_y = v;
                        }
                    }
                    48 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_window_min_x = v;
                        }
                    }
                    49 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_window_min_y = v;
                        }
                    }
                    70 => {
                        if let Ok(v) = val.parse::<i32>() {
                            layout.plot_flags = crate::objects::PlotFlags::from_bits(v);
                        }
                    }
                    72 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.plot_paper_units = v;
                        }
                    }
                    73 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.plot_rotation = v;
                        }
                    }
                    74 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.plot_type = v;
                        }
                    }
                    75 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.plot_scale_type = v;
                        }
                    }
                    76 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.shade_plot_mode = v;
                        }
                    }
                    77 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.shade_plot_resolution = v;
                        }
                    }
                    78 => {
                        if let Ok(v) = val.parse::<i16>() {
                            layout.shade_plot_dpi = v;
                        }
                    }
                    140 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_window_max_x = v;
                        }
                    }
                    141 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_window_max_y = v;
                        }
                    }
                    142 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_scale_numerator = v;
                        }
                    }
                    143 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_scale_denominator = v;
                        }
                    }
                    147 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.plot_scale_factor = v;
                        }
                    }
                    148 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.paper_image_origin_x = v;
                        }
                    }
                    149 => {
                        if let Ok(v) = val.parse::<f64>() {
                            layout.paper_image_origin_y = v;
                        }
                    }
                    333 => {
                        if let Ok(v) = u64::from_str_radix(val, 16) {
                            layout.visual_style_handle = Handle::new(v);
                        }
                    }
                    _ => {}
                }
            }
            layout.raw_plot_settings_codes = Some(plot_settings_codes);
        }

        Ok(Some(layout))
    }

    /// Skip an unknown object type
    /// Read a VISUALSTYLE object
    fn read_visualstyle(&mut self) -> Result<Option<VisualStyle>> {
        let mut obj = VisualStyle::new();
        let mut style_type_seen = false;
        let mut extended_seen = false;
        let mut property_count = None;
        let mut pending_property = None;
        let mut legacy_properties = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if property_count.is_some() {
                match pair.code {
                    90 => {
                        pending_property = Some(VisualStylePropertyValue::Long(
                            pair.as_i32().unwrap_or_default(),
                        ));
                        continue;
                    }
                    40 => {
                        pending_property = Some(VisualStylePropertyValue::Double(
                            pair.as_double().unwrap_or_default(),
                        ));
                        continue;
                    }
                    290 => {
                        pending_property = Some(VisualStylePropertyValue::Bool(
                            pair.as_bool().unwrap_or(false),
                        ));
                        continue;
                    }
                    62 => {
                        pending_property = Some(VisualStylePropertyValue::Color(
                            Color::from_index(pair.as_i16().unwrap_or(256)),
                        ));
                        continue;
                    }
                    420 => {
                        pending_property = Some(VisualStylePropertyValue::Color(
                            Color::from_true_color_value(pair.as_i32().unwrap_or_default()),
                        ));
                        continue;
                    }
                    1 => {
                        pending_property =
                            Some(VisualStylePropertyValue::Text(pair.value_string.clone()));
                        continue;
                    }
                    176 => {
                        if let Some(value) = pending_property.take() {
                            obj.properties.push(VisualStyleProperty {
                                value,
                                enabled: pair.as_i16().unwrap_or_default(),
                            });
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            if extended_seen && property_count.is_none() {
                match pair.code {
                    40 | 41 | 42 | 43 | 44 => {
                        pending_property = Some(VisualStylePropertyValue::Double(
                            pair.as_double().unwrap_or_default(),
                        ));
                    }
                    63..=67 => {
                        pending_property =
                            Some(VisualStylePropertyValue::Color(Color::from_index(
                                pair.as_i16()
                                    .or_else(|| pair.value_string.trim().parse::<i16>().ok())
                                    .unwrap_or(256),
                            )));
                    }
                    75..=79 | 170 | 171 | 173 | 175 | 92 | 93 => {
                        pending_property = Some(VisualStylePropertyValue::Long(
                            pair.as_i32()
                                .or_else(|| pair.as_i16().map(i32::from))
                                .unwrap_or_default(),
                        ));
                    }
                    290 => {
                        pending_property = Some(VisualStylePropertyValue::Bool(
                            pair.as_bool().unwrap_or(false),
                        ));
                    }
                    420 => {
                        pending_property = Some(VisualStylePropertyValue::Color(
                            Color::from_true_color_value(pair.as_i32().unwrap_or_default()),
                        ));
                    }
                    _ => {}
                }
            }
            if style_type_seen && !extended_seen {
                let property = match pair.code {
                    40 | 41 | 42 | 43 | 45 => Some(VisualStylePropertyValue::Double(
                        pair.as_double().unwrap_or_default(),
                    )),
                    44 => Some(VisualStylePropertyValue::Long(
                        pair.as_double().unwrap_or_default() as i32,
                    )),
                    63..=67 => Some(VisualStylePropertyValue::Color(Color::from_index(
                        pair.as_i16()
                            .or_else(|| pair.value_string.trim().parse::<i16>().ok())
                            .unwrap_or(256),
                    ))),
                    75 | 78 | 92 | 93 | 170 | 173 => Some(VisualStylePropertyValue::Long(
                        pair.as_i32()
                            .or_else(|| pair.as_i16().map(i32::from))
                            .or_else(|| pair.as_double().map(|value| value as i32))
                            .unwrap_or_default(),
                    )),
                    76 | 77 | 79 | 171 | 174 | 175 => Some(VisualStylePropertyValue::Short(
                        pair.as_i16().unwrap_or_default(),
                    )),
                    290 => Some(VisualStylePropertyValue::Bool(
                        pair.as_bool().unwrap_or(false),
                    )),
                    420 => {
                        if let Some(VisualStyleProperty {
                            value: VisualStylePropertyValue::Color(value),
                            ..
                        }) = legacy_properties.last_mut()
                        {
                            *value =
                                Color::from_true_color_value(pair.as_i32().unwrap_or_default());
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(value) = property {
                    legacy_properties.push(VisualStyleProperty { value, enabled: 1 });
                }
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                102 if pair.value_string.trim() == "{ACAD_REACTORS" => {
                    obj.reactors = self.read_reactor_handles()?;
                }
                102 if pair.value_string.trim() == "{ACAD_XDICTIONARY" => {
                    obj.xdictionary_handle = self.read_xdictionary_handle()?;
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                2 => obj.description = pair.value_string.clone(),
                70 if !style_type_seen => {
                    style_type_seen = true;
                    obj.style_type = pair.as_i16().unwrap_or_default();
                }
                70 if extended_seen => {
                    property_count = Some(pair.as_i16().unwrap_or_default().max(0) as usize);
                }
                177 => {
                    extended_seen = true;
                    obj.extended_lighting_model = pair.as_i16().unwrap_or_default();
                }
                71 => {
                    obj.face_lighting_model = pair.as_i16().unwrap_or_default();
                    if extended_seen {
                        pending_property = Some(VisualStylePropertyValue::Long(
                            obj.face_lighting_model as i32,
                        ));
                    }
                }
                72 => {
                    obj.face_lighting_quality = pair.as_i16().unwrap_or_default();
                    if extended_seen {
                        pending_property = Some(VisualStylePropertyValue::Long(
                            obj.face_lighting_quality as i32,
                        ));
                    }
                }
                73 => {
                    obj.face_color_mode = pair.as_i16().unwrap_or_default();
                    if extended_seen {
                        pending_property =
                            Some(VisualStylePropertyValue::Long(obj.face_color_mode as i32));
                    }
                }
                90 => {
                    obj.face_modifier = pair.as_i32().unwrap_or_default();
                    if extended_seen {
                        pending_property = Some(VisualStylePropertyValue::Long(obj.face_modifier));
                    }
                }
                74 => {
                    obj.edge_model = pair.as_i32().unwrap_or_default();
                    if extended_seen {
                        pending_property = Some(VisualStylePropertyValue::Long(obj.edge_model));
                    }
                }
                91 => {
                    obj.edge_style = pair.as_i32().unwrap_or_default();
                    if extended_seen {
                        pending_property = Some(VisualStylePropertyValue::Long(obj.edge_style));
                    }
                }
                291 => obj.internal_use_only = pair.as_bool().unwrap_or(false),
                176 if extended_seen => {
                    if let Some(value) = pending_property.take() {
                        obj.properties.push(VisualStyleProperty {
                            value,
                            enabled: pair.as_i16().unwrap_or_default(),
                        });
                    }
                }
                _ => {}
            }
        }
        if let Some(count) = property_count {
            obj.properties.truncate(count);
        } else if !extended_seen {
            obj.properties = legacy_properties;
        }
        if extended_seen {
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.first()
            {
                obj.face_lighting_model = *value as i16;
            }
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.get(1)
            {
                obj.face_lighting_quality = *value as i16;
            }
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.get(2)
            {
                obj.face_color_mode = *value as i16;
            }
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.get(3)
            {
                obj.face_modifier = *value;
            }
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.get(7)
            {
                obj.edge_model = *value;
            }
            if let Some(VisualStyleProperty {
                value: VisualStylePropertyValue::Long(value),
                ..
            }) = obj.properties.get(8)
            {
                obj.edge_style = *value;
            }
        }
        Ok(Some(obj))
    }

    fn read_material_dxf_texture_color(
        &mut self,
        flag_code: i32,
        factor_code: i32,
        rgb_code: i32,
    ) -> Result<MaterialColor> {
        let mut value = MaterialColor::default();
        let Some(flag) = self.reader.read_pair()? else {
            return Ok(value);
        };
        if flag.code != flag_code {
            self.reader.push_back(flag);
            return Ok(value);
        }
        value.flag = flag.as_i16().unwrap_or_default() as u8;

        let Some(factor) = self.reader.read_pair()? else {
            return Ok(value);
        };
        if factor.code != factor_code {
            self.reader.push_back(factor);
            return Ok(value);
        }
        value.factor = factor.as_double().unwrap_or_default();

        if value.flag == 1 {
            let Some(rgb) = self.reader.read_pair()? else {
                return Ok(value);
            };
            if rgb.code == rgb_code {
                value.rgb = rgb.as_i32();
            } else {
                self.reader.push_back(rgb);
            }
        }
        Ok(value)
    }

    fn read_material_dxf_texture(&mut self, depth: usize) -> Result<Option<MaterialTexture>> {
        if depth > 32 {
            return Ok(None);
        }
        let Some(mode_pair) = self.reader.read_pair()? else {
            return Ok(None);
        };
        if mode_pair.code != 277 {
            self.reader.push_back(mode_pair);
            return Ok(None);
        }
        let mut value = MaterialTexture::default();
        value.mode = mode_pair.as_i16().unwrap_or_default();
        match value.mode {
            0 => {
                value.color1 = self.read_material_dxf_texture_color(278, 460, 95)?;
                value.color2 = self.read_material_dxf_texture_color(279, 461, 96)?;
            }
            1 => {
                value.color1 = self.read_material_dxf_texture_color(280, 465, 97)?;
                value.color2 = self.read_material_dxf_texture_color(281, 466, 98)?;
            }
            2 => {
                let Some(kind) = self.reader.read_pair()? else {
                    return Ok(Some(value));
                };
                value.procedural = match kind.code {
                    291 => Some(MaterialProceduralValue::Bool(
                        kind.as_bool().unwrap_or(false),
                    )),
                    271 => Some(MaterialProceduralValue::Integer(
                        kind.as_i16().unwrap_or_default(),
                    )),
                    469 => Some(MaterialProceduralValue::Real(
                        kind.as_double().unwrap_or_default(),
                    )),
                    62 => {
                        let mut color = Color::from_index(kind.as_i16().unwrap_or(256));
                        if let Some(true_color) = self.reader.read_pair()? {
                            if true_color.code == 420 {
                                color = Color::from_true_color_value(
                                    true_color.as_i32().unwrap_or_default(),
                                );
                            } else {
                                self.reader.push_back(true_color);
                            }
                        }
                        Some(MaterialProceduralValue::Color(color))
                    }
                    420 => Some(MaterialProceduralValue::Color(
                        Color::from_true_color_value(kind.as_i32().unwrap_or_default()),
                    )),
                    301 => Some(MaterialProceduralValue::Text(kind.value_string)),
                    300 => {
                        let mut items = Vec::new();
                        let mut name = kind.value_string;
                        loop {
                            if let Some(texture) = self.read_material_dxf_texture(depth + 1)? {
                                items.push((name, texture));
                            } else {
                                break;
                            }
                            let Some(next) = self.reader.read_pair()? else {
                                break;
                            };
                            match next.code {
                                300 => name = next.value_string,
                                292 => {
                                    value.table_end = next.as_bool().unwrap_or(false);
                                    break;
                                }
                                _ => {
                                    self.reader.push_back(next);
                                    break;
                                }
                            }
                        }
                        Some(MaterialProceduralValue::Table(items))
                    }
                    _ => {
                        self.reader.push_back(kind);
                        None
                    }
                };
            }
            _ => {}
        }
        Ok(Some(value))
    }

    /// Read a MATERIAL object
    fn read_material(&mut self) -> Result<Option<Material>> {
        let mut obj = Material::new();
        let mut diffuse_matrix = 0usize;
        let mut specular_matrix = 0usize;
        let mut reflection_matrix = 0usize;
        let mut opacity_matrix = 0usize;
        let mut bump_matrix = 0usize;
        let mut refraction_matrix = 0usize;
        let mut normal_matrix = 0usize;
        let mut advanced = false;
        let mut ambient_rgb_seen = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                102 if pair.value_string.trim() == "{ACAD_REACTORS" => {
                    obj.reactors = self.read_reactor_handles()?;
                }
                102 if pair.value_string.trim() == "{ACAD_XDICTIONARY" => {
                    obj.xdictionary_handle = self.read_xdictionary_handle()?;
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                1 => obj.name = pair.value_string.clone(),
                2 => obj.description = pair.value_string.clone(),
                70 => obj.ambient_color.flag = pair.as_i16().unwrap_or_default() as u8,
                40 => obj.ambient_color.factor = pair.as_double().unwrap_or_default(),
                90 if !ambient_rgb_seen && obj.ambient_color.flag == 1 => {
                    obj.ambient_color.rgb = pair.as_i32();
                    ambient_rgb_seen = true;
                }
                90 => obj.self_illumination = pair.as_i32().unwrap_or_default() as f64,
                71 => obj.diffuse_color.flag = pair.as_i16().unwrap_or_default() as u8,
                41 => obj.diffuse_color.factor = pair.as_double().unwrap_or_default(),
                91 => obj.diffuse_color.rgb = pair.as_i32(),
                42 if advanced => {
                    obj.normal_map.blend_factor = pair.as_double().unwrap_or_default()
                }
                42 => obj.diffuse_map.blend_factor = pair.as_double().unwrap_or_default(),
                72 if advanced => {
                    obj.normal_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.normal_map.source == 2 {
                        obj.normal_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                72 => {
                    obj.diffuse_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.diffuse_map.source == 2 {
                        obj.diffuse_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                3 if advanced => obj.normal_map.file_name = pair.value_string.clone(),
                3 => obj.diffuse_map.file_name = pair.value_string.clone(),
                73 if advanced => {
                    obj.normal_map.projection = pair.as_i16().unwrap_or_default() as u8
                }
                73 => obj.diffuse_map.projection = pair.as_i16().unwrap_or_default() as u8,
                74 if advanced => obj.normal_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                74 => obj.diffuse_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                75 if advanced => {
                    obj.normal_map.auto_transform = pair.as_i16().unwrap_or_default() as u8
                }
                75 => obj.diffuse_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                43 if advanced => {
                    if normal_matrix < 16 {
                        obj.normal_map.transform[normal_matrix] =
                            pair.as_double().unwrap_or_default();
                        normal_matrix += 1;
                    }
                }
                43 => {
                    if diffuse_matrix < 16 {
                        obj.diffuse_map.transform[diffuse_matrix] =
                            pair.as_double().unwrap_or_default();
                        diffuse_matrix += 1;
                    }
                }
                76 => obj.specular_color.flag = pair.as_i16().unwrap_or_default() as u8,
                45 => obj.specular_color.factor = pair.as_double().unwrap_or_default(),
                92 => obj.specular_color.rgb = pair.as_i32(),
                44 => obj.specular_gloss_factor = pair.as_double().unwrap_or_default(),
                46 => obj.specular_map.blend_factor = pair.as_double().unwrap_or_default(),
                77 => {
                    obj.specular_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.specular_map.source == 2 {
                        obj.specular_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                4 => obj.specular_map.file_name = pair.value_string.clone(),
                78 => obj.specular_map.projection = pair.as_i16().unwrap_or_default() as u8,
                79 => obj.specular_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                170 => obj.specular_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                47 => {
                    if specular_matrix < 16 {
                        obj.specular_map.transform[specular_matrix] =
                            pair.as_double().unwrap_or_default();
                        specular_matrix += 1;
                    }
                }
                48 => obj.reflection_map.blend_factor = pair.as_double().unwrap_or_default(),
                171 => {
                    obj.reflection_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.reflection_map.source == 2 {
                        obj.reflection_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                6 => obj.reflection_map.file_name = pair.value_string.clone(),
                172 => obj.reflection_map.projection = pair.as_i16().unwrap_or_default() as u8,
                173 => obj.reflection_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                174 => obj.reflection_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                49 => {
                    if reflection_matrix < 16 {
                        obj.reflection_map.transform[reflection_matrix] =
                            pair.as_double().unwrap_or_default();
                        reflection_matrix += 1;
                    }
                }
                140 => obj.opacity_percent = pair.as_double().unwrap_or_default(),
                141 => obj.opacity_map.blend_factor = pair.as_double().unwrap_or_default(),
                175 => {
                    obj.opacity_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.opacity_map.source == 2 {
                        obj.opacity_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                7 => obj.opacity_map.file_name = pair.value_string.clone(),
                176 => obj.opacity_map.projection = pair.as_i16().unwrap_or_default() as u8,
                177 => obj.opacity_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                178 => obj.opacity_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                142 => {
                    if opacity_matrix < 16 {
                        obj.opacity_map.transform[opacity_matrix] =
                            pair.as_double().unwrap_or_default();
                        opacity_matrix += 1;
                    }
                }
                143 => obj.bump_map.blend_factor = pair.as_double().unwrap_or_default(),
                179 => {
                    obj.bump_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.bump_map.source == 2 {
                        obj.bump_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                8 => obj.bump_map.file_name = pair.value_string.clone(),
                270 if advanced => obj.luminance_mode = pair.as_i16().unwrap_or_default(),
                270 => obj.bump_map.projection = pair.as_i16().unwrap_or_default() as u8,
                271 if advanced => obj.normal_map_method = pair.as_i16().unwrap_or_default(),
                271 => obj.bump_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                272 if advanced => obj.global_illumination = pair.as_i16().unwrap_or_default(),
                272 => obj.bump_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                144 => {
                    if bump_matrix < 16 {
                        obj.bump_map.transform[bump_matrix] = pair.as_double().unwrap_or_default();
                        bump_matrix += 1;
                    }
                }
                145 => obj.refraction_index = pair.as_double().unwrap_or_default(),
                146 => obj.refraction_map.blend_factor = pair.as_double().unwrap_or_default(),
                273 if advanced => obj.final_gather = pair.as_i16().unwrap_or_default(),
                273 => {
                    obj.refraction_map.source = pair.as_i16().unwrap_or_default() as u8;
                    if obj.refraction_map.source == 2 {
                        obj.refraction_map.texture = self.read_material_dxf_texture(0)?;
                    }
                }
                9 => obj.refraction_map.file_name = pair.value_string.clone(),
                274 => obj.refraction_map.projection = pair.as_i16().unwrap_or_default() as u8,
                275 => obj.refraction_map.tiling = pair.as_i16().unwrap_or_default() as u8,
                276 => obj.refraction_map.auto_transform = pair.as_i16().unwrap_or_default() as u8,
                147 => {
                    if refraction_matrix < 16 {
                        obj.refraction_map.transform[refraction_matrix] =
                            pair.as_double().unwrap_or_default();
                        refraction_matrix += 1;
                    }
                }
                148 => obj.translucence = pair.as_double().unwrap_or_default(),
                149 => obj.self_illumination = pair.as_double().unwrap_or_default(),
                468 => obj.reflectivity = pair.as_double().unwrap_or_default(),
                93 => obj.illumination_model = pair.as_i32().unwrap_or_default(),
                94 => obj.channel_flags = pair.as_i32().unwrap_or_default(),
                282 => obj.mode = pair.as_i16().unwrap_or_default() as i32,
                460 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.color_bleed_scale = pair.as_double().unwrap_or_default();
                }
                461 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.indirect_bump_scale = pair.as_double().unwrap_or_default();
                }
                462 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.reflectance_scale = pair.as_double().unwrap_or_default();
                }
                463 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.transmittance_scale = pair.as_double().unwrap_or_default();
                }
                290 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.two_sided_material = pair.as_bool().unwrap_or(false);
                }
                464 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.luminance = pair.as_double().unwrap_or_default();
                }
                465 if advanced => obj.normal_map_strength = pair.as_double().unwrap_or_default(),
                293 => {
                    advanced = true;
                    obj.advanced_data_present = true;
                    obj.is_anonymous = pair.as_bool().unwrap_or(false);
                }
                _ => {}
            }
        }
        Ok(Some(obj))
    }

    /// Read an IMAGEDEF_REACTOR object
    fn read_imagedef_reactor(&mut self) -> Result<Option<ImageDefinitionReactor>> {
        let mut obj = ImageDefinitionReactor::new(Handle::NULL);
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                _ => {}
            }
        }
        Ok(Some(obj))
    }

    fn read_geodata(&mut self) -> Result<GeoData> {
        let mut obj = GeoData::new();
        let mut group = String::new();
        let mut owner_seen = false;
        let mut source_x = None;
        let mut source_y = None;
        let mut destination_x = None;
        let mut face_first = None;
        let mut face_second = None;
        let mut civil_mode = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                3 if pair.value_string == "CIVIL3D_DATA_BEGIN" => {
                    civil_mode = true;
                    obj.civil_data_present = true;
                }
                4 if pair.value_string == "CIVIL3D_DATA_END" => {
                    civil_mode = false;
                }
                5 => obj.handle = parse_dxf_handle(&pair.value_string),
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    obj.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    obj.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen => {
                    obj.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                330 => {
                    obj.host_block = parse_dxf_handle(&pair.value_string);
                }
                90 => obj.version = pair.as_i32().unwrap_or(obj.version),
                70 => obj.coordinate_type = pair.as_i16().unwrap_or(obj.coordinate_type),
                10 => obj.design_point.x = pair.as_double().unwrap_or(obj.design_point.x),
                20 => obj.design_point.y = pair.as_double().unwrap_or(obj.design_point.y),
                30 => obj.design_point.z = pair.as_double().unwrap_or(obj.design_point.z),
                11 => obj.reference_point.x = pair.as_double().unwrap_or(obj.reference_point.x),
                21 => obj.reference_point.y = pair.as_double().unwrap_or(obj.reference_point.y),
                31 => obj.reference_point.z = pair.as_double().unwrap_or(obj.reference_point.z),
                12 => obj.north_direction.x = pair.as_double().unwrap_or(obj.north_direction.x),
                22 => obj.north_direction.y = pair.as_double().unwrap_or(obj.north_direction.y),
                210 => obj.up_direction.x = pair.as_double().unwrap_or(obj.up_direction.x),
                220 => obj.up_direction.y = pair.as_double().unwrap_or(obj.up_direction.y),
                230 => obj.up_direction.z = pair.as_double().unwrap_or(obj.up_direction.z),
                40 => {
                    obj.horizontal_unit_scale =
                        pair.as_double().unwrap_or(obj.horizontal_unit_scale)
                }
                41 => obj.vertical_unit_scale = pair.as_double().unwrap_or(obj.vertical_unit_scale),
                91 => obj.horizontal_units = pair.as_i32().unwrap_or(obj.horizontal_units),
                92 => obj.vertical_units = pair.as_i32().unwrap_or(obj.vertical_units),
                95 => {
                    obj.scale_estimation_method =
                        pair.as_i32().unwrap_or(obj.scale_estimation_method)
                }
                141 => obj.user_scale_factor = pair.as_double().unwrap_or(obj.user_scale_factor),
                294 => {
                    obj.sea_level_correction =
                        pair.as_i16().map(|value| value != 0).unwrap_or(false)
                }
                142 => {
                    obj.sea_level_elevation = pair.as_double().unwrap_or(obj.sea_level_elevation)
                }
                143 => {
                    obj.coordinate_projection_radius =
                        pair.as_double().unwrap_or(obj.coordinate_projection_radius)
                }
                301 => obj.coordinate_system_definition = pair.value_string,
                303 if obj.version == 1 => obj.coordinate_system_datum = pair.value_string,
                303 => obj
                    .coordinate_system_definition
                    .push_str(&pair.value_string),
                304 => obj.coordinate_system_wkt = pair.value_string,
                302 => obj.geo_rss_tag = pair.value_string,
                305 => obj.observation_from_tag = pair.value_string,
                306 => obj.observation_to_tag = pair.value_string,
                307 => obj.observation_coverage_tag = pair.value_string,
                13 => source_x = pair.as_double(),
                23 => source_y = pair.as_double(),
                14 if civil_mode => obj.civil_reference_point1.x = pair.as_double().unwrap_or(0.0),
                24 if civil_mode => obj.civil_reference_point1.y = pair.as_double().unwrap_or(0.0),
                15 if civil_mode => obj.civil_reference_point2.x = pair.as_double().unwrap_or(0.0),
                25 if civil_mode => obj.civil_reference_point2.y = pair.as_double().unwrap_or(0.0),
                16 if civil_mode => obj.civil_zero_point1.x = pair.as_double().unwrap_or(0.0),
                26 if civil_mode => obj.civil_zero_point1.y = pair.as_double().unwrap_or(0.0),
                17 if civil_mode => obj.civil_zero_point2.x = pair.as_double().unwrap_or(0.0),
                27 if civil_mode => obj.civil_zero_point2.y = pair.as_double().unwrap_or(0.0),
                14 => destination_x = pair.as_double(),
                24 => {
                    if let (
                        Some(source_x),
                        Some(source_y),
                        Some(destination_x),
                        Some(destination_y),
                    ) = (
                        source_x.take(),
                        source_y.take(),
                        destination_x.take(),
                        pair.as_double(),
                    ) {
                        obj.mesh_points.push(GeoDataMeshPoint {
                            source: Vector2::new(source_x, source_y),
                            destination: Vector2::new(destination_x, destination_y),
                        });
                    }
                }
                292 if civil_mode => {
                    obj.civil_obsolete_flag = pair.as_i16().is_some_and(|value| value != 0)
                }
                293 if civil_mode => {
                    obj.civil_unknown_flag1 = pair.as_i16().is_some_and(|value| value != 0)
                }
                93 if civil_mode => obj.civil_unknown1 = pair.as_i32().unwrap_or(0),
                94 if civil_mode => obj.civil_unknown2 = pair.as_i32().unwrap_or(0),
                54 if civil_mode => obj.civil_north_angle_degrees = pair.as_double().unwrap_or(0.0),
                140 if civil_mode => {
                    obj.civil_north_angle_radians = pair.as_double().unwrap_or(0.0)
                }
                97 => face_first = pair.as_i32(),
                98 => face_second = pair.as_i32(),
                99 => {
                    if let (Some(first), Some(second), Some(third)) =
                        (face_first.take(), face_second.take(), pair.as_i32())
                    {
                        obj.mesh_faces.push(GeoDataMeshFace {
                            first,
                            second,
                            third,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(obj)
    }

    fn read_data_object_dxf(&mut self, dxf_name: &str) -> Result<DataObject> {
        let data = match dxf_name.to_uppercase().as_str() {
            "BREAKDATA" => DataObjectData::BreakData(BreakData::default()),
            "BREAKPOINTREF" => DataObjectData::BreakPointRef,
            "CELLSTYLEMAP" => DataObjectData::CellStyleMap(CellStyleMap::default()),
            "ACDSRECORD" => DataObjectData::AcDsRecord,
            "ACDSSCHEMA" => DataObjectData::AcDsSchema,
            "DUMMY" => DataObjectData::Dummy,
            "IDBUFFER" => DataObjectData::IdBuffer(IdBuffer::default()),
            "INDEX" => DataObjectData::Index(Index::default()),
            "LAYER_INDEX" => DataObjectData::LayerIndex(LayerIndex::default()),
            "LONG_TRANSACTION" => DataObjectData::LongTransaction,
            "OBJECT_PTR" => DataObjectData::ObjectPointer,
            "PARTIAL_VIEWING_FILTER" => {
                DataObjectData::PartialViewingFilter(PartialViewingFilter::default())
            }
            "TABLEGEOMETRY" => DataObjectData::TableGeometry(TableGeometry::default()),
            _ => DataObjectData::BreakPointRef,
        };
        let mut obj = DataObject::new(data);
        let mut section = String::new();
        let mut group = String::new();
        let mut owner_seen = false;
        let mut layer_header_seen = false;
        let mut pending_layer_count = 0;
        let mut pending_layer_name: Option<String> = None;
        let mut style_section = String::new();
        let mut pending_grid_index = 0;
        let mut grid_open = false;
        let mut margin_index = 0usize;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => obj.handle = parse_dxf_handle(&pair.value_string),
                100 => {
                    section = pair.value_string.clone();
                    if section == "AcDbBreakPointRef" {
                        if let DataObjectData::BreakData(value) = &mut obj.data {
                            value.point_references.push(BreakPointReference::default());
                        }
                    }
                }
                1 => {
                    style_section = pair.value_string.clone();
                    if style_section == "GRIDFORMAT_BEGIN" {
                        if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                            if let Some(cell) = value.cells.last_mut() {
                                cell.cell_style.borders.push(TableGridFormat {
                                    index_mask: pending_grid_index,
                                    ..TableGridFormat::default()
                                });
                                grid_open = true;
                            }
                        }
                    } else if style_section == "CELLMARGIN_BEGIN" {
                        margin_index = 0;
                    }
                }
                309 => {
                    if pair.value_string == "GRIDFORMAT_END" {
                        if !grid_open {
                            if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                                if let Some(cell) = value.cells.last_mut() {
                                    cell.cell_style.borders.push(TableGridFormat::default());
                                }
                            }
                        }
                        grid_open = false;
                        pending_grid_index = 0;
                    }
                    style_section = match pair.value_string.as_str() {
                        "CONTENTFORMAT_END" | "CELLMARGIN_END" | "GRIDFORMAT_END" => {
                            "TABLEFORMAT_BEGIN".to_string()
                        }
                        _ => String::new(),
                    };
                }
                102 => group = pair.value_string.clone(),
                330 if group == "{ACAD_REACTORS" => {
                    obj.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    obj.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen => {
                    obj.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                330 => match &mut obj.data {
                    DataObjectData::BreakData(value) => {
                        value.dimension_reference = parse_dxf_handle(&pair.value_string);
                    }
                    DataObjectData::IdBuffer(value) => {
                        value.object_ids.push(parse_dxf_handle(&pair.value_string))
                    }
                    DataObjectData::TableGeometry(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.table_geometry = parse_dxf_handle(&pair.value_string);
                        }
                    }
                    _ => {}
                },
                331 => {
                    if let DataObjectData::BreakData(value) = &mut obj.data {
                        value.dimension_reference = parse_dxf_handle(&pair.value_string);
                    }
                }
                40 => match &mut obj.data {
                    DataObjectData::Index(value) => {
                        if let Some(timestamp) = pair.as_double() {
                            value.last_updated_julian_day = timestamp.floor() as i32;
                            value.last_updated_milliseconds =
                                ((timestamp - timestamp.floor()) * 86_400_000.0).round() as i32;
                        }
                    }
                    DataObjectData::LayerIndex(value) => {
                        if let Some(timestamp) = pair.as_double() {
                            value.last_updated_julian_day = timestamp.floor() as i32;
                            value.last_updated_milliseconds =
                                ((timestamp - timestamp.floor()) * 86_400_000.0).round() as i32;
                        }
                    }
                    DataObjectData::CellStyleMap(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            let number = pair.as_double().unwrap_or(0.0);
                            match style_section.as_str() {
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.rotation = number;
                                }
                                "CELLMARGIN_BEGIN" => {
                                    let target = match margin_index {
                                        0 => &mut cell.cell_style.vertical_margin,
                                        1 => &mut cell.cell_style.horizontal_margin,
                                        2 => &mut cell.cell_style.bottom_margin,
                                        3 => &mut cell.cell_style.right_margin,
                                        4 => &mut cell.cell_style.horizontal_spacing,
                                        _ => &mut cell.cell_style.vertical_spacing,
                                    };
                                    *target = number;
                                    margin_index += 1;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.double_line_spacing = number;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.width_with_gap = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    _ => {}
                },
                70 => {
                    if let DataObjectData::BreakData(value) = &mut obj.data {
                        if section == "AcDbBreakData" {
                            value.version = pair.as_i16().unwrap_or_default();
                        }
                    }
                }
                71 => {
                    if let DataObjectData::BreakData(value) = &mut obj.data {
                        if section == "AcDbBreakPointRef" {
                            if let Some(reference) = value.point_references.last_mut() {
                                reference.reference_type = pair.as_i32().unwrap_or_default();
                            }
                        }
                    }
                }
                72 => {
                    if let DataObjectData::BreakData(value) = &mut obj.data {
                        if section == "AcDbBreakPointRef" {
                            if let Some(reference) = value.point_references.last_mut() {
                                reference.flags = pair.as_i16().unwrap_or_default();
                            }
                        }
                    }
                }
                90 => match &mut obj.data {
                    DataObjectData::LayerIndex(value) => {
                        if !layer_header_seen {
                            layer_header_seen = true;
                        } else if pending_layer_name.is_none() {
                            if let Some(entry) = value.entries.last_mut() {
                                entry.layer_count = pair.as_i32().unwrap_or_default();
                            }
                        } else {
                            pending_layer_count = pair.as_i32().unwrap_or_default();
                        }
                    }
                    DataObjectData::CellStyleMap(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            let number = pair.as_i32().unwrap_or_default();
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.style_type = number;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.property_override_flags = number;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.property_flags =
                                            TableBorderPropertyFlags::from_bits_retain(number);
                                    }
                                }
                                "CELLSTYLE_BEGIN" => cell.id = number,
                                _ => {}
                            }
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        value.rows = pair.as_i32().unwrap_or_default();
                    }
                    _ => {}
                },
                91 => match &mut obj.data {
                    DataObjectData::BreakData(value) => {
                        if section == "AcDbBreakPointRef" {
                            if let Some(reference) = value.point_references.last_mut() {
                                reference.identifier = pair.as_i32().unwrap_or_default();
                            }
                        }
                    }
                    DataObjectData::CellStyleMap(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            let number = pair.as_i32().unwrap_or_default();
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.property_override_flags = number;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.property_flags = number;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.border_type =
                                            TableBorderType::from(number as i16);
                                    }
                                }
                                "CELLSTYLE_BEGIN" => cell.style_type = number,
                                _ => {}
                            }
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        value.columns = pair.as_i32().unwrap_or_default();
                    }
                    _ => {}
                },
                92 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            let number = pair.as_i32().unwrap_or_default();
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.merge_flags = number;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.value_data_type = number;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.line_weight =
                                            LineWeight::from_value(number as i16);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                93 => match &mut obj.data {
                    DataObjectData::CellStyleMap(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            let number = pair.as_i32().unwrap_or_default();
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.content_layout = number;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.value_unit_type = number;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.is_invisible = number == 0;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        value.cells.push(TableGeometryCell {
                            geometry_data_flag: pair.as_i32().unwrap_or_default(),
                            ..TableGeometryCell::default()
                        });
                    }
                    _ => {}
                },
                94 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            if style_section == "CONTENTFORMAT_BEGIN" {
                                cell.cell_style.content_format.cell_alignment =
                                    pair.as_i32().unwrap_or_default();
                            }
                        }
                    }
                }
                95 => match &mut obj.data {
                    DataObjectData::CellStyleMap(_) => {
                        pending_grid_index = pair.as_i32().unwrap_or_default();
                    }
                    DataObjectData::TableGeometry(value) => {
                        if let Some(geometry) = value
                            .cells
                            .last_mut()
                            .and_then(|cell| cell.geometry.last_mut())
                        {
                            geometry.flags = pair.as_i32().unwrap_or_default();
                        }
                    }
                    _ => {}
                },
                170 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.cell_style.data_flags = pair.as_i16().unwrap_or_default();
                        }
                    }
                }
                171 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.cell_style.margin_override_flags =
                                pair.as_i16().unwrap_or_default();
                        }
                    }
                }
                300 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if pair.value_string == "CELLSTYLE" && style_section.is_empty() {
                            value.cells.push(NamedTableCellStyle::default());
                        } else if let Some(cell) = value.cells.last_mut() {
                            match style_section.as_str() {
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.value_format_string =
                                        pair.value_string;
                                }
                                "CELLSTYLE_BEGIN" => {
                                    cell.name = pair.value_string;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                62 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            let color = Color::from_index(pair.as_i16().unwrap_or(256));
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.background_color = color;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.content_color = color;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.color = color;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                420 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            let color =
                                Color::from_true_color_value(pair.as_i32().unwrap_or_default());
                            match style_section.as_str() {
                                "TABLEFORMAT_BEGIN" => {
                                    cell.cell_style.background_color = color;
                                }
                                "CONTENTFORMAT_BEGIN" => {
                                    cell.cell_style.content_format.content_color = color;
                                }
                                "GRIDFORMAT_BEGIN" => {
                                    if let Some(grid) = cell.cell_style.borders.last_mut() {
                                        grid.border.color = color;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                140 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.cell_style.content_format.block_scale =
                                pair.as_double().unwrap_or_default();
                        }
                    }
                }
                144 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.cell_style.content_format.text_height =
                                pair.as_double().unwrap_or_default();
                        }
                    }
                }
                340 => {
                    if let DataObjectData::CellStyleMap(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            let handle = parse_dxf_handle(&pair.value_string);
                            if style_section == "CONTENTFORMAT_BEGIN" {
                                cell.cell_style.content_format.text_style = handle;
                            } else if style_section == "GRIDFORMAT_BEGIN" {
                                if let Some(grid) = cell.cell_style.borders.last_mut() {
                                    grid.line_type = handle;
                                }
                            }
                        }
                    }
                }
                41 => {
                    if let DataObjectData::TableGeometry(value) = &mut obj.data {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.height_with_gap = pair.as_double().unwrap_or_default();
                        }
                    }
                }
                10 => match &mut obj.data {
                    DataObjectData::BreakData(value) if section == "AcDbBreakPointRef" => {
                        if let Some(reference) = value.point_references.last_mut() {
                            reference.first_point.x = pair.as_double().unwrap_or_default();
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        if let Some(cell) = value.cells.last_mut() {
                            cell.geometry.push(CellContentGeometry {
                                distance_to_top_left: Vector3::new(
                                    pair.as_double().unwrap_or_default(),
                                    0.0,
                                    0.0,
                                ),
                                distance_to_center: Vector3::ZERO,
                                width: 0.0,
                                height: 0.0,
                                outer_width: 0.0,
                                outer_height: 0.0,
                                flags: 0,
                            });
                        }
                    }
                    _ => {}
                },
                20 | 30 | 11 | 21 | 31 => match &mut obj.data {
                    DataObjectData::BreakData(value) if section == "AcDbBreakPointRef" => {
                        if let Some(reference) = value.point_references.last_mut() {
                            let number = pair.as_double().unwrap_or_default();
                            match pair.code {
                                20 => reference.first_point.y = number,
                                30 => reference.first_point.z = number,
                                11 => reference.second_point.x = number,
                                21 => reference.second_point.y = number,
                                31 => reference.second_point.z = number,
                                _ => {}
                            }
                        }
                    }
                    DataObjectData::TableGeometry(value) => {
                        if let Some(geometry) = value
                            .cells
                            .last_mut()
                            .and_then(|cell| cell.geometry.last_mut())
                        {
                            let number = pair.as_double().unwrap_or_default();
                            match pair.code {
                                20 => {
                                    geometry.distance_to_top_left.y = number;
                                }
                                30 => {
                                    geometry.distance_to_top_left.z = number;
                                }
                                11 => geometry.distance_to_center.x = number,
                                21 => geometry.distance_to_center.y = number,
                                31 => geometry.distance_to_center.z = number,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                },
                43 | 44 | 45 | 46 => {
                    if let DataObjectData::TableGeometry(value) = &mut obj.data {
                        if let Some(geometry) = value
                            .cells
                            .last_mut()
                            .and_then(|cell| cell.geometry.last_mut())
                        {
                            let number = pair.as_double().unwrap_or_default();
                            match pair.code {
                                43 => geometry.width = number,
                                44 => geometry.height = number,
                                45 => geometry.outer_width = number,
                                46 => geometry.outer_height = number,
                                _ => {}
                            }
                        }
                    }
                }
                8 => match &mut obj.data {
                    DataObjectData::LayerIndex(_) => pending_layer_name = Some(pair.value_string),
                    DataObjectData::PartialViewingFilter(_) => {}
                    _ => {}
                },
                360 if section == "AcDbLayerIndex" => {
                    if let DataObjectData::LayerIndex(value) = &mut obj.data {
                        value.entries.push(LayerIndexEntry {
                            layer_count: pending_layer_count,
                            name: pending_layer_name.take().unwrap_or_default(),
                            id_buffer: parse_dxf_handle(&pair.value_string),
                        });
                        pending_layer_count = 0;
                    }
                }
                _ => {}
            }
        }
        Ok(obj)
    }

    /// Read a SPATIAL_FILTER object (block reference / XCLIP clip boundary).
    ///
    /// Group code layout (AcDbSpatialFilter):
    ///   70  number of boundary points
    ///   10/20  boundary point (2D), repeated `count` times
    ///   210/220/230  boundary plane normal
    ///   11/21/31  clip boundary local origin
    ///   71  display enabled flag
    ///   72  front clip flag, 40 front clip distance (only when 72 set)
    ///   73  back clip flag, 41 back clip distance (only when 73 set)
    ///   40 ×12  inverse block transform (column-major 4×3)
    ///   40 ×12  clip bound transform (column-major 4×3)
    ///
    /// The front clip distance reuses code 40, so the first code-40 value is
    /// treated as the front distance only while the front flag is set and no
    /// matrix values have been read yet; all later code-40 values feed the two
    /// transformation matrices.
    fn read_spatial_filter(&mut self) -> Result<Option<SpatialFilter>> {
        let mut obj = SpatialFilter::new();
        let mut front_flag = false;
        let mut pending_x: Option<f64> = None;
        let mut mat: Vec<f64> = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                70 => {} // point count is implied by the 10/20 pairs we read
                10 => {
                    if let Some(v) = pair.as_double() {
                        pending_x = Some(v);
                    }
                }
                20 => {
                    if let (Some(x), Some(y)) = (pending_x.take(), pair.as_double()) {
                        obj.boundary_points.push(Vector2::new(x, y));
                    }
                }
                210 => {
                    if let Some(v) = pair.as_double() {
                        obj.normal.x = v;
                    }
                }
                220 => {
                    if let Some(v) = pair.as_double() {
                        obj.normal.y = v;
                    }
                }
                230 => {
                    if let Some(v) = pair.as_double() {
                        obj.normal.z = v;
                    }
                }
                11 => {
                    if let Some(v) = pair.as_double() {
                        obj.origin.x = v;
                    }
                }
                21 => {
                    if let Some(v) = pair.as_double() {
                        obj.origin.y = v;
                    }
                }
                31 => {
                    if let Some(v) = pair.as_double() {
                        obj.origin.z = v;
                    }
                }
                71 => {
                    obj.display_enabled = pair.as_i16().map(|v| v != 0).unwrap_or(true);
                }
                72 => {
                    front_flag = pair.as_i16().map(|v| v != 0).unwrap_or(false);
                }
                73 => {} // back clip distance arrives as code 41 below
                40 => {
                    if let Some(v) = pair.as_double() {
                        if front_flag && obj.front_clip.is_none() && mat.is_empty() {
                            obj.front_clip = Some(v);
                        } else {
                            mat.push(v);
                        }
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        obj.back_clip = Some(v);
                    }
                }
                _ => {}
            }
        }
        if mat.len() >= 12 {
            obj.inverse_block_transform = matrix_from_row_major(&mat[0..12]);
        }
        if mat.len() >= 24 {
            obj.clip_bound_transform = matrix_from_row_major(&mat[12..24]);
        }
        Ok(Some(obj))
    }

    /// Read a RASTERVARIABLES object
    fn read_raster_variables(&mut self) -> Result<Option<RasterVariables>> {
        let mut obj = RasterVariables::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        obj.class_version = v;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        obj.display_image_frame = v;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        obj.image_quality = v;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        obj.units = v;
                    }
                }
                _ => {}
            }
        }
        Ok(Some(obj))
    }

    /// Read a DBCOLOR object
    fn read_bookcolor(&mut self) -> Result<Option<BookColor>> {
        let mut obj = BookColor::new();
        let mut indexed_color = None;
        let mut true_color = None;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                1 => obj.color_name = pair.value_string.clone(),
                2 => obj.book_name = pair.value_string.clone(),
                62 => indexed_color = pair.as_i16().map(Color::from_index),
                420 => true_color = pair.as_i32().map(Color::from_true_color_value),
                430 => {
                    let (book_name, color_name) =
                        crate::io::dxf::split_color_book_name(&pair.value_string);
                    obj.book_name = book_name.unwrap_or_default();
                    obj.color_name = color_name.unwrap_or_default();
                }
                _ => {}
            }
        }
        if let Some(color) = true_color.or(indexed_color) {
            obj.color = color;
        }
        Ok(Some(obj))
    }

    /// Read an ACDBDICTIONARYWDFLT object (dictionary with default)
    fn read_dict_with_default(&mut self) -> Result<Option<DictionaryWithDefault>> {
        let mut obj = DictionaryWithDefault::new();
        let mut current_key: Option<String> = None;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                280 => obj.hard_owner = pair.as_i16().map(|v| v != 0).unwrap_or(false),
                281 => {
                    if let Some(v) = pair.as_i16() {
                        obj.duplicate_cloning = v;
                    }
                }
                3 => {
                    current_key = Some(pair.value_string.clone());
                }
                340 => {
                    // Could be default handle or entry value
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        if obj.default_handle == Handle::NULL && current_key.is_none() {
                            obj.default_handle = Handle::new(h);
                        }
                    }
                }
                350 | 360 => {
                    if let Some(key) = current_key.take() {
                        if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                            obj.entries.push((key, Handle::new(h)));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(Some(obj))
    }

    /// Read a WIPEOUTVARIABLES object
    fn read_wipeout_variables(&mut self) -> Result<Option<WipeoutVariables>> {
        let mut obj = WipeoutVariables::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.owner = Handle::new(h);
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        obj.display_frame = v;
                    }
                }
                _ => {}
            }
        }
        Ok(Some(obj))
    }

    /// Trait-based generic reader for minimal stub objects (handle + owner only)
    fn read_stub_object<T: StubObject>(&mut self) -> Result<T> {
        let mut obj = T::new_stub();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.set_handle(Handle::new(h));
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        obj.set_owner(Handle::new(h));
                    }
                }
                _ => {}
            }
        }
        Ok(obj)
    }

    fn read_field_object_dxf(&mut self, version: DxfVersion) -> Result<Field> {
        let mut value = Field::default();
        let mut entries = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => value.handle = parse_dxf_handle(&pair.value_string),
                330 if value.owner.is_null() => value.owner = parse_dxf_handle(&pair.value_string),
                100 => {}
                _ => entries.push((pair.code, pair.value_string.clone())),
            }
        }

        let mut cursor = 0usize;
        value.evaluator_id = field_next_code(&entries, &mut cursor, 1)
            .unwrap_or("")
            .to_string();
        while cursor < entries.len() && matches!(entries[cursor].0, 2 | 3) {
            value.code.push_str(&entries[cursor].1);
            cursor += 1;
        }
        if version < DxfVersion::AC1021 && entries.get(cursor).map(|entry| entry.0) == Some(4) {
            value.format = entries[cursor].1.clone();
            cursor += 1;
        }
        let child_count = field_next_code(&entries, &mut cursor, 90)
            .and_then(|item| item.parse::<usize>().ok())
            .unwrap_or(0)
            .min(20_000);
        while value.child_fields.len() < child_count
            && entries.get(cursor).map(|entry| entry.0) == Some(360)
        {
            value
                .child_fields
                .push(parse_dxf_handle(&entries[cursor].1));
            cursor += 1;
        }
        let object_count = field_next_code(&entries, &mut cursor, 97)
            .and_then(|item| item.parse::<usize>().ok())
            .unwrap_or(0)
            .min(20_000);
        while value.referenced_objects.len() < object_count
            && entries.get(cursor).map(|entry| entry.0) == Some(331)
        {
            value
                .referenced_objects
                .push(parse_dxf_handle(&entries[cursor].1));
            cursor += 1;
        }
        value.evaluation_option = field_next_code(&entries, &mut cursor, 91)
            .and_then(|item| item.parse().ok())
            .unwrap_or(0);
        value.filing_option = field_next_code(&entries, &mut cursor, 92)
            .and_then(|item| item.parse().ok())
            .unwrap_or(0);
        value.state = field_next_code(&entries, &mut cursor, 94)
            .and_then(|item| item.parse().ok())
            .unwrap_or(0);
        value.evaluation_status = field_next_code(&entries, &mut cursor, 95)
            .and_then(|item| item.parse().ok())
            .unwrap_or(0);
        value.evaluation_error_code = field_next_code(&entries, &mut cursor, 96)
            .and_then(|item| item.parse().ok())
            .unwrap_or(0);
        value.evaluation_error_message = field_next_code(&entries, &mut cursor, 300)
            .unwrap_or("")
            .to_string();
        value.value = read_field_cell_value_dxf(&entries, &mut cursor, version);
        let child_value_count = if entries.get(cursor).map(|entry| entry.0) == Some(93) {
            let count = entries[cursor].1.parse::<usize>().unwrap_or(0).min(20_000);
            cursor += 1;
            count
        } else {
            0
        };
        for _ in 0..child_value_count {
            let key = field_next_code(&entries, &mut cursor, 6)
                .unwrap_or("")
                .to_string();
            value.child_values.push(FieldChildValue {
                key,
                value: read_field_cell_value_dxf(&entries, &mut cursor, version),
            });
        }
        if entries.get(cursor).map(|entry| entry.0) == Some(301) {
            value.value_string = entries[cursor].1.clone();
            cursor += 1;
        }
        if entries.get(cursor).map(|entry| entry.0) == Some(98) {
            value.value_string_length = entries[cursor].1.parse().unwrap_or(0);
        }
        Ok(value)
    }

    fn read_field_list_dxf(&mut self) -> Result<FieldList> {
        let mut value = FieldList::default();
        let mut count = 0usize;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => value.handle = parse_dxf_handle(&pair.value_string),
                330 if value.owner.is_null() => value.owner = parse_dxf_handle(&pair.value_string),
                90 => count = pair.value_string.parse::<usize>().unwrap_or(0).min(20_000),
                290 => value.unknown = pair.value_string.parse::<i32>().unwrap_or(0) != 0,
                330 => {
                    if value.fields.len() < count {
                        value.fields.push(parse_dxf_handle(&pair.value_string));
                    }
                }
                _ => {}
            }
        }
        Ok(value)
    }

    /// Read an unknown object, capturing handle, owner and all group-code pairs
    /// for lossless DXF round-trip.
    fn read_unknown_object_full(&mut self) -> Result<(Handle, Handle, Vec<(i32, String)>)> {
        let mut handle = Handle::NULL;
        let mut owner = Handle::NULL;
        let mut raw_codes: Vec<(i32, String)> = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        handle = Handle::new(h);
                    }
                }
                330 => {
                    // First 330 outside a 102-group is the owner.
                    // Subsequent 330s inside reactor groups are stored as raw codes.
                    if owner == Handle::NULL {
                        if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                            owner = Handle::new(h);
                        }
                    } else {
                        raw_codes.push((pair.code, pair.value_string.clone()));
                    }
                }
                _ => {
                    raw_codes.push((pair.code, pair.value_string.clone()));
                }
            }
        }
        Ok((handle, owner, raw_codes))
    }

    /// Skip to ENDTAB
    fn skip_to_endtab(&mut self) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }
        }
        Ok(())
    }

    // ===== Table Readers =====

    /// Read LAYER table
    fn read_layer_table(&mut self, document: &mut CadDocument) -> Result<()> {
        let mut table_handle = document.layers.handle();
        let mut xdictionary_handle = None;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 0 && pair.value_string == "LAYER" {
                if let Some((layer, xdictionary)) = self.read_layer_entry()? {
                    if let Some(xdictionary) = xdictionary {
                        document.xdic_by_handle.insert(layer.handle, xdictionary);
                    }
                    document.layers.add_or_replace(layer);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            } else if pair.code == 5 {
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    table_handle = Handle::new(handle);
                    document.layers.set_handle(table_handle);
                    document.header.layer_control_handle = table_handle;
                }
            } else if pair.code == 102 && pair.value_string.trim() == "{ACAD_XDICTIONARY" {
                xdictionary_handle = self.read_xdictionary_handle()?;
            }
        }
        if let Some(xdictionary_handle) = xdictionary_handle {
            document
                .xdic_by_handle
                .insert(table_handle, xdictionary_handle);
        }
        Ok(())
    }

    /// Read a single LAYER entry
    fn read_layer_entry(&mut self) -> Result<Option<(Layer, Option<Handle>)>> {
        let mut layer = Layer::new("0");
        let mut xdictionary_handle = None;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                // Next entity - push back and break
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        layer.handle = Handle::new(h);
                    }
                }
                102 if pair.value_string.trim() == "{ACAD_XDICTIONARY" => {
                    xdictionary_handle = self.read_xdictionary_handle()?;
                }
                2 => layer.name = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        // A NEGATIVE colour index is how DXF encodes an OFF
                        // layer — code 70 has no off bit (#314).
                        layer.flags.off = color_index < 0;
                        layer.color = Color::from_index(color_index.abs());
                    }
                }
                // True color (code 420): packed 24-bit RGB, overrides the ACI
                // index in code 62 (which is 7 for a true-colour layer). Without
                // this every RGB-coloured layer read back as Index(7)/white on
                // DXF import while the DWG reader kept the RGB. (#223)
                420 => {
                    if let Some(v) = pair.as_i32() {
                        layer.color = Color::from_true_color_value(v);
                    }
                }
                430 => {
                    let (book_name, color_name) =
                        crate::io::dxf::split_color_book_name(&pair.value_string);
                    layer.book_name = book_name;
                    layer.color_name = color_name;
                }
                6 => layer.line_type = pair.value_string.clone(),
                70 => {
                    if let Some(flags) = pair.as_i16() {
                        layer.flags.frozen = (flags & 1) != 0;
                        layer.flags.frozen_in_new_viewport = (flags & 2) != 0;
                        layer.flags.locked = (flags & 4) != 0;
                        // Bit 2 is "frozen by default in NEW viewports", NOT
                        // off — the AEC sample's GRIDLINES layer carries it
                        // while fully visible, and reading it as off hid every
                        // grid line on DXF import (#314).
                        layer.flags.xref_dependent = (flags & 0x10) != 0;
                    }
                }
                290 => {
                    if let Some(plotting) = pair.as_bool() {
                        layer.is_plottable = plotting;
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        layer.line_weight = LineWeight::from_value(lw);
                    }
                }
                1001 => {
                    self.reader.push_back(pair);
                    let (xdata, next_pair) = self.read_extended_data()?;
                    if let Some(next_pair) = next_pair {
                        self.reader.push_back(next_pair);
                    }
                    if let Some(value) = xdata.get_record("AcCmTransparency").and_then(|record| {
                        record.values.iter().find_map(|value| match value {
                            XDataValue::Integer32(value) => Some(*value),
                            _ => None,
                        })
                    }) {
                        layer.transparency =
                            crate::types::Transparency::from_alpha_value(value as u32);
                    }
                }
                _ => {}
            }
        }

        Ok(Some((layer, xdictionary_handle)))
    }

    /// Read LTYPE table
    fn read_linetype_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.line_types.set_handle(Handle::new(handle));
                    document.header.linetype_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "LTYPE" {
                if let Some(linetype) = self.read_linetype_entry()? {
                    document.line_types.add_or_replace(linetype);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single LTYPE entry
    fn read_linetype_entry(&mut self) -> Result<Option<LineType>> {
        let mut linetype = LineType::new("Continuous");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        linetype.handle = Handle::new(h);
                    }
                }
                2 => linetype.name = pair.value_string.clone(),
                3 => linetype.description = pair.value_string.clone(),
                70 => {
                    if let Some(flags) = pair.as_i16() {
                        linetype.xref_dependent = (flags & 0x10) != 0;
                    }
                }
                73 => {
                    if let Some(count) = pair.as_i16() {
                        linetype.elements.reserve(count as usize);
                    }
                }
                40 => {
                    if let Some(length) = pair.as_double() {
                        linetype.pattern_length = length;
                    }
                }
                49 => {
                    if let Some(dash) = pair.as_double() {
                        linetype.elements.push(LineTypeElement {
                            length: dash,
                            complex: None,
                        });
                    }
                }
                9 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if last.complex.is_some() {
                            last.complex_mut().set_text(pair.value_string.clone());
                        }
                    }
                }
                44 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let (Some(c), Some(v)) = (last.complex.as_mut(), pair.as_double()) {
                            c.offset[0] = v;
                        }
                    }
                }
                45 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let (Some(c), Some(v)) = (last.complex.as_mut(), pair.as_double()) {
                            c.offset[1] = v;
                        }
                    }
                }
                46 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let Some(c) = last.complex.as_mut() {
                            c.scale = pair.value_string.parse().unwrap_or(1.0);
                        }
                    }
                }
                50 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let Some(c) = last.complex.as_mut() {
                            // Stored as radians (the DWG reader + renderer treat
                            // it so); DXF code 50 is in degrees, so convert.
                            c.rotation =
                                pair.value_string.parse::<f64>().unwrap_or(0.0).to_radians();
                        }
                    }
                }
                // DXF LTYPE per-element codes: 74 = element-type FLAGS
                // (0 = plain dash — must NOT materialize complex data, or every
                // dashed linetype turns "complex" and renders nothing, #314),
                // 75 = shape number. These were read swapped AND
                // unconditionally, so AutoCAD's plain `74 0` gave every element
                // a Shape{0} complex record.
                74 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let Some(flags) = pair.as_i16() {
                            if flags != 0 {
                                last.complex_mut().apply_dxf_flags(flags);
                            }
                        }
                    }
                }
                75 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if last.complex.is_some() {
                            last.complex_mut()
                                .set_shape_number(pair.as_i16().unwrap_or(0));
                        }
                    }
                }
                340 => {
                    if let Some(last) = linetype.elements.last_mut() {
                        if let (Some(c), Ok(h)) = (
                            last.complex.as_mut(),
                            u64::from_str_radix(&pair.value_string, 16),
                        ) {
                            c.style_handle = Handle::new(h);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(Some(linetype))
    }

    /// Read STYLE table
    fn read_textstyle_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.text_styles.set_handle(Handle::new(handle));
                    document.header.style_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "STYLE" {
                if let Some(style) = self.read_textstyle_entry()? {
                    document.text_styles.add_or_replace(style);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single STYLE entry
    fn read_textstyle_entry(&mut self) -> Result<Option<TextStyle>> {
        let mut style = TextStyle::new("Standard");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.handle = Handle::new(h);
                    }
                }
                2 => style.name = pair.value_string.clone(),
                70 => {
                    if let Some(f) = pair.as_i16() {
                        style.is_shape_file = (f & 0x01) != 0;
                        style.is_vertical = (f & 0x04) != 0;
                        style.xref_dependent = (f & 0x10) != 0;
                    }
                }
                3 => style.font_file = pair.value_string.clone(),
                4 => style.big_font_file = pair.value_string.clone(),
                40 => {
                    if let Some(height) = pair.as_double() {
                        style.height = height;
                    }
                }
                41 => {
                    if let Some(width) = pair.as_double() {
                        style.width_factor = width;
                    }
                }
                50 => {
                    if let Some(angle) = pair.as_double() {
                        style.oblique_angle = angle;
                    }
                }
                71 => {
                    if let Some(flags) = pair.as_i16() {
                        style.flags.backward = (flags & 2) != 0;
                        style.flags.upside_down = (flags & 4) != 0;
                    }
                }
                42 => {
                    if let Some(lh) = pair.as_double() {
                        style.last_height = lh;
                    }
                }
                1001 => {
                    if pair.value_string == "AcadAnnotative" {
                        style.annotative = self.read_annotative_xdata(pair)?;
                    }
                }
                _ => {}
            }
        }

        Ok(Some(style))
    }

    /// Read BLOCK_RECORD table
    fn read_block_record_table(&mut self, document: &mut CadDocument) -> Result<()> {
        // Save old block record handles so we can update Layout references
        let old_model_handle = document.header.model_space_block_handle;
        let old_paper_handle = document.header.paper_space_block_handle;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.block_records.set_handle(Handle::new(handle));
                    document.header.block_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "BLOCK_RECORD" {
                if let Some(block_record) = self.read_block_record_entry()? {
                    let name = block_record.name.clone();
                    if let Err(_) = document.block_records.add(block_record.clone()) {
                        // Entry already exists (from initialize_defaults),
                        // update it with the data from the file
                        if let Some(existing) = document.block_records.get_mut(&name) {
                            if !block_record.handle.is_null() {
                                existing.set_handle(block_record.handle);
                            }
                            if !block_record.layout.is_null() {
                                existing.layout = block_record.layout;
                            }
                            existing.units = block_record.units;
                            existing.flags = block_record.flags;
                        }
                    }
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }

        // Update header block handles to match what was read from the file
        if let Some(ms) = document.block_records.get("*Model_Space") {
            if !ms.handle.is_null() {
                document.header.model_space_block_handle = ms.handle;
            }
        }
        if let Some(ps) = document.block_records.get("*Paper_Space") {
            if !ps.handle.is_null() {
                document.header.paper_space_block_handle = ps.handle;
            }
        }

        // Update Layout objects created by initialize_defaults() to reference
        // the file's block record handles instead of the initialized ones
        let new_model_handle = document.header.model_space_block_handle;
        let new_paper_handle = document.header.paper_space_block_handle;

        if old_model_handle != new_model_handle || old_paper_handle != new_paper_handle {
            for (_, obj) in document.objects.iter_mut() {
                if let ObjectType::Layout(layout) = obj {
                    if layout.block_record == old_model_handle {
                        layout.block_record = new_model_handle;
                    } else if layout.block_record == old_paper_handle {
                        layout.block_record = new_paper_handle;
                    }
                }
            }
        }

        Ok(())
    }

    /// Read a single BLOCK_RECORD entry
    fn read_block_record_entry(&mut self) -> Result<Option<BlockRecord>> {
        let mut block_record = BlockRecord::new("*Model_Space");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        block_record.handle = Handle::new(h);
                    }
                }
                2 => block_record.name = pair.value_string.clone(),
                70 => {
                    // AcDbBlockTableRecord code 70 is the block INSERTION UNITS
                    // (0=unitless, 1=in, 4=mm, 6=cm, …), NOT the block-type
                    // flags. Those (anonymous/xref/has-attr) live on the BLOCK
                    // entity and are read in read_block — parsing them here
                    // spuriously marked regular blocks as xrefs (which the
                    // renderer then fades).
                    if let Some(v) = pair.as_i16() {
                        block_record.units = v;
                    }
                }
                280 => {
                    // Block explodability (1 = explodable).
                    if let Some(v) = pair.as_i16() {
                        block_record.explodable = v != 0;
                    }
                }
                281 => {
                    // Block scalability (1 = scale uniformly).
                    if let Some(v) = pair.as_i16() {
                        block_record.scale_uniformly = v != 0;
                    }
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        block_record.layout = Handle::new(h);
                    }
                }
                _ => {}
            }
        }

        Ok(Some(block_record))
    }

    /// Read DIMSTYLE table
    fn read_dimstyle_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.dim_styles.set_handle(Handle::new(handle));
                    document.header.dimstyle_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "DIMSTYLE" {
                if let Some(dimstyle) = self.read_dimstyle_entry()? {
                    document.dim_styles.add_or_replace(dimstyle);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single DIMSTYLE entry
    fn read_dimstyle_entry(&mut self) -> Result<Option<DimStyle>> {
        let mut ds = DimStyle::new("Standard");
        let mut seen_table_flags = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                105 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.handle = Handle::new(h);
                    }
                }
                2 => ds.name = pair.value_string.clone(),
                3 => ds.dimpost = pair.value_string.clone(),
                4 => ds.dimapost = pair.value_string.clone(),
                5 => ds.dimblk_name = pair.value_string.clone(),
                6 => ds.dimblk1_name = pair.value_string.clone(),
                7 => ds.dimblk2_name = pair.value_string.clone(),
                // Scale / lines
                40 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimscale = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimasz = v;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimexo = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimdli = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimexe = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimrnd = v;
                    }
                }
                46 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimdle = v;
                    }
                }
                47 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtp = v;
                    }
                }
                48 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtm = v;
                    }
                }
                49 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimfxl = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimjogang = v;
                    }
                }
                140 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtxt = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimcen = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtsz = v;
                    }
                }
                143 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimaltf = v;
                    }
                }
                144 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimlfac = v;
                    }
                }
                145 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtvp = v;
                    }
                }
                146 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimtfac = v;
                    }
                }
                147 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimgap = v;
                    }
                }
                148 => {
                    if let Some(v) = pair.as_double() {
                        ds.dimaltrnd = v;
                    }
                }
                // Integer codes
                69 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtfill = v;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        if seen_table_flags {
                            ds.dimtfillclr = v;
                        } else {
                            seen_table_flags = true;
                            ds.xref_dependent = (v & 0x10) != 0;
                            ds.xref_resolved = (v & 0x20) != 0;
                            ds.xref_reference = (v & 0x40) != 0;
                        }
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtol = v != 0;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimlim = v != 0;
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtih = v != 0;
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtoh = v != 0;
                    }
                }
                75 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimse1 = v != 0;
                    }
                }
                76 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimse2 = v != 0;
                    }
                }
                77 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtad = v;
                    }
                }
                78 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimzin = v;
                    }
                }
                79 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimazin = v;
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        ds.dimarcsym = v as i16;
                    }
                }
                170 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimalt = v != 0;
                    }
                }
                171 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimaltd = v;
                    }
                }
                172 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtofl = v != 0;
                    }
                }
                173 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimsah = v != 0;
                    }
                }
                174 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtix = v != 0;
                    }
                }
                175 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimsoxd = v != 0;
                    }
                }
                176 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimclrd = v;
                    }
                }
                177 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimclre = v;
                    }
                }
                178 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimclrt = v;
                    }
                }
                179 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimadec = v;
                    }
                }
                270 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimunit = v;
                    }
                }
                271 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimdec = v;
                    }
                }
                272 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtdec = v;
                    }
                }
                273 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimaltu = v;
                    }
                }
                274 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimalttd = v;
                    }
                }
                275 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimaunit = v;
                    }
                }
                276 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimfrac = v;
                    }
                }
                277 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimlunit = v;
                    }
                }
                278 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimdsep = v;
                    }
                }
                279 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtmove = v;
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimjust = v;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimsd1 = v != 0;
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimsd2 = v != 0;
                    }
                }
                283 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtolj = v;
                    }
                }
                284 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtzin = v;
                    }
                }
                285 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimaltz = v;
                    }
                }
                286 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimalttz = v;
                    }
                }
                287 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimfit = v;
                    }
                }
                288 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimupt = v != 0;
                    }
                }
                289 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimatfit = v;
                    }
                }
                290 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimfxlon = v != 0;
                    }
                }
                294 | 295 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimtxtdirection = v != 0;
                    }
                }
                // Handle references
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimtxsty_handle = Handle::new(h);
                    }
                }
                341 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimldrblk = Handle::new(h);
                    }
                }
                342 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimblk = Handle::new(h);
                    }
                }
                343 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimblk1 = Handle::new(h);
                    }
                }
                344 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimblk2 = Handle::new(h);
                    }
                }
                345 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimltex_handle = Handle::new(h);
                    }
                }
                346 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimltex1_handle = Handle::new(h);
                    }
                }
                347 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ds.dimltex2_handle = Handle::new(h);
                    }
                }
                371 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimlwd = v;
                    }
                }
                372 => {
                    if let Some(v) = pair.as_i16() {
                        ds.dimlwe = v;
                    }
                }
                1001 => {
                    if pair.value_string == "AcadAnnotative" {
                        ds.annotative = self.read_annotative_xdata(pair)?;
                    }
                }
                _ => {}
            }
        }

        Ok(Some(ds))
    }

    /// Read APPID table
    fn read_appid_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.app_ids.set_handle(Handle::new(handle));
                    document.header.appid_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "APPID" {
                if let Some(appid) = self.read_appid_entry()? {
                    document.app_ids.add_or_replace(appid);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single APPID entry
    fn read_appid_entry(&mut self) -> Result<Option<AppId>> {
        let mut appid = AppId::new("ACAD");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        appid.handle = Handle::new(h);
                    }
                }
                2 => appid.name = pair.value_string.clone(),
                _ => {}
            }
        }

        Ok(Some(appid))
    }

    /// Read VIEW table
    fn read_view_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.views.set_handle(Handle::new(handle));
                    document.header.view_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "VIEW" {
                if let Some(view) = self.read_view_entry()? {
                    document.views.add_or_replace(view);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single VIEW entry
    fn read_view_entry(&mut self) -> Result<Option<View>> {
        let mut view = View::new("*Active");
        let mut center = PointReader::new();
        let mut target = PointReader::new();
        let mut direction = PointReader::new();
        let mut ucs_origin = PointReader::new();
        let mut ucs_x_axis = PointReader::new();
        let mut ucs_y_axis = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.handle = Handle::new(h);
                    }
                }
                2 => view.name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    direction.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    target.add_coordinate(&pair);
                }
                110 | 120 | 130 => {
                    ucs_origin.add_coordinate(&pair);
                }
                111 | 121 | 131 => {
                    ucs_x_axis.add_coordinate(&pair);
                }
                112 | 122 | 132 => {
                    ucs_y_axis.add_coordinate(&pair);
                }
                40 => {
                    if let Some(height) = pair.as_double() {
                        view.height = height;
                    }
                }
                41 => {
                    if let Some(width) = pair.as_double() {
                        view.width = width;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        view.lens_length = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        view.front_clip = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        view.back_clip = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        view.twist_angle = v.to_radians();
                    }
                }
                63 => {
                    if let Some(v) = pair.as_i16() {
                        view.ambient_color = Color::from_index(v);
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        view.paper_space = (v & 1) != 0;
                        view.xref_dependent = (v & 0x10) != 0;
                        view.xref_resolved = (v & 0x20) != 0;
                        view.xref_reference = (v & 0x40) != 0;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        view.perspective = (v & 1) != 0;
                        view.front_clipping = (v & 2) != 0;
                        view.back_clipping = (v & 4) != 0;
                        view.front_clip_at_eye = (v & 16) != 0;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        view.ucs_associated = v != 0;
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        view.camera_plottable = v != 0;
                    }
                }
                79 => {
                    if let Some(v) = pair.as_i16() {
                        view.ucs_ortho_type = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        view.brightness = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        view.contrast = v;
                    }
                }
                146 => {
                    if let Some(v) = pair.as_double() {
                        view.ucs_elevation = v;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        view.render_mode = ViewportRenderMode::from_value(v);
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        view.default_lighting_type = v;
                    }
                }
                292 => {
                    if let Some(v) = pair.as_i16() {
                        view.use_default_lights = v != 0;
                    }
                }
                332 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.background_handle = Handle::new(h);
                    }
                }
                334 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.live_section_handle = Handle::new(h);
                    }
                }
                345 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.named_ucs_handle = Handle::new(h);
                    }
                }
                346 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.base_ucs_handle = Handle::new(h);
                    }
                }
                348 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.visual_style_handle = Handle::new(h);
                    }
                }
                361 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        view.sun_handle = Handle::new(h);
                    }
                }
                421 => {
                    if let Some(v) = pair.as_i32() {
                        view.ambient_color = Color::from_true_color_value(v);
                    }
                }
                _ => {}
            }
        }

        if let Some(pt) = center.get_point() {
            view.center = pt;
        }
        if let Some(pt) = target.get_point() {
            view.target = pt;
        }
        if let Some(pt) = direction.get_point() {
            view.direction = pt;
        }
        if let Some(pt) = ucs_origin.get_point() {
            view.ucs_origin = pt;
        }
        if let Some(pt) = ucs_x_axis.get_point() {
            view.ucs_x_axis = pt;
        }
        if let Some(pt) = ucs_y_axis.get_point() {
            view.ucs_y_axis = pt;
        }

        Ok(Some(view))
    }

    /// Read VPORT table
    fn read_vport_table(&mut self, document: &mut CadDocument) -> Result<()> {
        document.vports.clear();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.vports.set_handle(Handle::new(handle));
                    document.header.vport_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "VPORT" {
                if let Some(vport) = self.read_vport_entry()? {
                    document.vports.add_allow_duplicate(vport);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single VPORT entry
    fn read_vport_entry(&mut self) -> Result<Option<VPort>> {
        let mut vport = VPort::new("*Active");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.handle = Handle::new(h);
                    }
                }
                2 => vport.name = pair.value_string.clone(),
                10 => {
                    if let Some(v) = pair.as_double() {
                        vport.lower_left.x = v;
                    }
                }
                20 => {
                    if let Some(v) = pair.as_double() {
                        vport.lower_left.y = v;
                    }
                }
                11 => {
                    if let Some(v) = pair.as_double() {
                        vport.upper_right.x = v;
                    }
                }
                21 => {
                    if let Some(v) = pair.as_double() {
                        vport.upper_right.y = v;
                    }
                }
                12 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_center.x = v;
                    }
                }
                22 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_center.y = v;
                    }
                }
                13 => {
                    if let Some(v) = pair.as_double() {
                        vport.snap_base.x = v;
                    }
                }
                23 => {
                    if let Some(v) = pair.as_double() {
                        vport.snap_base.y = v;
                    }
                }
                14 => {
                    if let Some(v) = pair.as_double() {
                        vport.snap_spacing.x = v;
                    }
                }
                24 => {
                    if let Some(v) = pair.as_double() {
                        vport.snap_spacing.y = v;
                    }
                }
                15 => {
                    if let Some(v) = pair.as_double() {
                        vport.grid_spacing.x = v;
                    }
                }
                25 => {
                    if let Some(v) = pair.as_double() {
                        vport.grid_spacing.y = v;
                    }
                }
                16 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_direction.x = v;
                    }
                }
                26 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_direction.y = v;
                    }
                }
                36 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_direction.z = v;
                    }
                }
                17 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_target.x = v;
                    }
                }
                27 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_target.y = v;
                    }
                }
                37 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_target.z = v;
                    }
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_height = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        vport.aspect_ratio = v;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        vport.lens_length = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        vport.front_clip = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        vport.back_clip = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        vport.snap_rotation = v.to_radians();
                    }
                }
                51 => {
                    if let Some(v) = pair.as_double() {
                        vport.view_twist = v.to_radians();
                    }
                }
                60 => {
                    if let Some(v) = pair.as_i16() {
                        vport.grid_flags = GridFlags::from_bits(v);
                    }
                }
                61 => {
                    if let Some(v) = pair.as_i16() {
                        vport.grid_major = v;
                    }
                }
                63 => {
                    if let Some(v) = pair.as_i16() {
                        vport.ambient_color = Color::from_index(v);
                    }
                }
                65 => {
                    if let Some(v) = pair.as_i16() {
                        vport.ucs_per_viewport = v != 0;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        vport.xref_dependent = (v & 0x10) != 0;
                        vport.xref_resolved = (v & 0x20) != 0;
                        vport.xref_reference = (v & 0x40) != 0;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        vport.perspective = (v & 1) != 0;
                        vport.front_clipping = (v & 2) != 0;
                        vport.back_clipping = (v & 4) != 0;
                        vport.ucsfollow = (v & 8) != 0;
                        vport.front_clip_at_eye = (v & 16) != 0;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        vport.circle_zoom = v;
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        vport.fast_zoom = v != 0;
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        vport.ucsicon_lower = (v & 1) != 0;
                        vport.ucsicon_origin = (v & 2) != 0;
                    }
                }
                75 => {
                    if let Some(v) = pair.as_i16() {
                        vport.snap_on = v != 0;
                    }
                }
                76 => {
                    if let Some(v) = pair.as_i16() {
                        vport.grid_on = v != 0;
                    }
                }
                77 => {
                    if let Some(v) = pair.as_i16() {
                        vport.snap_style = v != 0;
                    }
                }
                78 => {
                    if let Some(v) = pair.as_i16() {
                        vport.snap_isopair = v;
                    }
                }
                79 => {
                    if let Some(v) = pair.as_i16() {
                        vport.ucs_ortho_type = v;
                    }
                }
                110 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_origin.x = v;
                    }
                }
                120 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_origin.y = v;
                    }
                }
                130 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_origin.z = v;
                    }
                }
                111 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_x_axis.x = v;
                    }
                }
                121 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_x_axis.y = v;
                    }
                }
                131 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_x_axis.z = v;
                    }
                }
                112 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_y_axis.x = v;
                    }
                }
                122 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_y_axis.y = v;
                    }
                }
                132 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_y_axis.z = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        vport.brightness = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        vport.contrast = v;
                    }
                }
                146 => {
                    if let Some(v) = pair.as_double() {
                        vport.ucs_elevation = v;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        vport.render_mode = ViewportRenderMode::from_value(v);
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        vport.default_lighting_type = v;
                    }
                }
                292 => {
                    if let Some(v) = pair.as_i16() {
                        vport.use_default_lights = v != 0;
                    }
                }
                332 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.background_handle = Handle::new(h);
                    }
                }
                345 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.named_ucs_handle = Handle::new(h);
                    }
                }
                346 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.base_ucs_handle = Handle::new(h);
                    }
                }
                348 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.visual_style_handle = Handle::new(h);
                    }
                }
                361 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vport.sun_handle = Handle::new(h);
                    }
                }
                421 => {
                    if let Some(v) = pair.as_i32() {
                        vport.ambient_color = Color::from_true_color_value(v);
                    }
                }
                _ => {}
            }
        }

        Ok(Some(vport))
    }

    /// Read UCS table
    fn read_ucs_table(&mut self, document: &mut CadDocument) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDTAB" {
                break;
            }

            if pair.code == 5 {
                // Symbol table headers carry their control handle in code 5;
                // adopt it so tables with non-default handles keep them
                // (issue #64: dropped control handles caused duplicate handles
                // between tables on round-trip).
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    document.ucss.set_handle(Handle::new(handle));
                    document.header.ucs_control_handle = Handle::new(handle);
                }
            } else if pair.code == 0 && pair.value_string == "UCS" {
                if let Some(ucs) = self.read_ucs_entry()? {
                    document.ucss.add_or_replace(ucs);
                    self.decoded_records = self.decoded_records.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// Read a single UCS entry
    fn read_ucs_entry(&mut self) -> Result<Option<Ucs>> {
        let mut ucs = Ucs::new("World");
        let mut origin = PointReader::new();
        let mut x_axis = PointReader::new();
        let mut y_axis = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ucs.handle = Handle::new(h);
                    }
                }
                2 => ucs.name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    origin.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    x_axis.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    y_axis.add_coordinate(&pair);
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        ucs.xref_dependent = (v & 0x10) != 0;
                        ucs.xref_resolved = (v & 0x20) != 0;
                        ucs.xref_reference = (v & 0x40) != 0;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        ucs.ortho_type = v;
                    }
                }
                79 => {
                    if let Some(v) = pair.as_i16() {
                        ucs.ortho_view_type = v;
                    }
                }
                146 => {
                    if let Some(v) = pair.as_double() {
                        ucs.elevation = v;
                    }
                }
                345 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ucs.named_ucs_handle = Handle::new(h);
                    }
                }
                346 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ucs.base_ucs_handle = Handle::new(h);
                    }
                }
                _ => {}
            }
        }

        if let Some(pt) = origin.get_point() {
            ucs.origin = pt;
        }
        if let Some(pt) = x_axis.get_point() {
            ucs.x_axis = pt;
        }
        if let Some(pt) = y_axis.get_point() {
            ucs.y_axis = pt;
        }

        Ok(Some(ucs))
    }

    // ===== Common Entity/Object Code Helpers =====

    /// Read an ATTRIB/ATTDEF R2018+ embedded MTEXT object (code 101 block) and
    /// return its text. A multiline attribute keeps its real text here (the
    /// entity's own code 1 is empty), so the caller adopts this when non-empty.
    /// MTEXT splits long text into 250-char `3` continuation chunks ending in a
    /// final `1` chunk; concatenate in that order.
    fn read_attrib_embedded_text(&mut self) -> Result<String> {
        let mut text = String::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 || pair.code >= 1000 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                3 => text.push_str(&pair.value_string),
                1 => text.push_str(&pair.value_string),
                _ => {}
            }
        }
        Ok(text)
    }

    /// Read an MTEXT R2018+ embedded object (code 101 block), extracting the
    /// column layout. The MTEXT column data lives here rather than in the
    /// entity's own codes; other embedded-object codes are consumed and
    /// ignored. Stops at the next 0-code or the XDATA (>=1000) region.
    fn read_mtext_embedded_object(&mut self) -> Result<crate::entities::mtext::MTextColumnData> {
        let mut col = crate::entities::mtext::MTextColumnData::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 || pair.code >= 1000 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                71 => {
                    if let Some(v) = pair.as_i16() {
                        col.column_type = v;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        col.column_count = v as i32;
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        col.auto_height = v != 0;
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        col.flow_reversed = v != 0;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        col.width = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        col.gutter = v;
                    }
                }
                46 => {
                    if let Some(v) = pair.as_double() {
                        col.heights.push(v);
                    }
                }
                _ => {}
            }
        }
        Ok(col)
    }

    /// Try to read common entity codes, including visual-style handle 348.
    /// Returns true if the code was consumed, false if not recognized.
    fn try_read_common_entity_code(
        &mut self,
        pair: &super::stream_reader::DxfCodePair,
        common: &mut EntityCommon,
    ) -> Result<bool> {
        match pair.code {
            5 => {
                if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    common.handle = Handle::new(h);
                }
                Ok(true)
            }
            6 => {
                common.linetype = pair.value_string.clone();
                Ok(true)
            }
            48 => {
                if let Some(scale) = pair.as_double() {
                    common.linetype_scale = scale;
                }
                Ok(true)
            }
            60 => {
                if let Some(v) = pair.as_i16() {
                    common.invisible = v != 0;
                }
                Ok(true)
            }
            330 => {
                if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    common.owner_handle = Handle::new(h);
                }
                Ok(true)
            }
            // Entity visual-style override handles. DXF uses code 348 for
            // full, face and edge overrides in that order.
            348 => {
                if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    let handle = Handle::new(h);
                    if common.full_visual_style_handle.is_none() {
                        common.full_visual_style_handle = Some(handle);
                    } else if common.face_visual_style_handle.is_none() {
                        common.face_visual_style_handle = Some(handle);
                    } else if common.edge_visual_style_handle.is_none() {
                        common.edge_visual_style_handle = Some(handle);
                    }
                }
                Ok(true)
            }
            102 => {
                let val = pair.value_string.trim();
                if val == "{ACAD_REACTORS" {
                    common.reactors = self.read_reactor_handles()?;
                } else if val == "{ACAD_XDICTIONARY" {
                    common.xdictionary_handle = self.read_xdictionary_handle()?;
                } else if val.starts_with('{') {
                    // Skip unknown defined groups
                    self.skip_defined_group()?;
                }
                // "}" closing tokens are handled inside the group readers
                Ok(true)
            }
            // True color (code 420): packed 24-bit RGB overrides ACI index.
            420 => {
                if let Some(v) = pair.as_i32() {
                    common.color = Color::from_true_color_value(v);
                }
                Ok(true)
            }
            430 => {
                common.color_name = Some(pair.value_string.clone());
                Ok(true)
            }
            // Transparency (code 440): packed alpha value.
            440 => {
                if let Some(v) = pair.as_i32() {
                    common.transparency = Transparency::from_alpha_value(v as u32);
                }
                Ok(true)
            }
            // Paper space flag (67 = 1 means the entity is in paper space).
            // R2000+ also carries an explicit owner (code 330), but R12 does
            // not, so record the flag (entity_mode 1 = paper) — resolve_references
            // uses it to place unowned entities in paper vs model space instead
            // of dumping every R12 paper-space entity into model space.
            67 => {
                if pair.as_i16() == Some(1) {
                    common.entity_mode = Some(1);
                }
                Ok(true)
            }
            // Extended data - read and store
            1001 => {
                // Push back the pair and read XDATA
                self.reader.push_back(pair.clone());
                let (extended_data, next_pair) = self.read_extended_data()?;
                common.extended_data = extended_data;
                // Push back the non-XDATA pair for next iteration
                if let Some(p) = next_pair {
                    self.reader.push_back(p);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Read reactor handles from a {ACAD_REACTORS group.
    /// Assumes the opening "102 {ACAD_REACTORS" has already been consumed.
    fn read_reactor_handles(&mut self) -> Result<Vec<Handle>> {
        let mut handles = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 102 {
                // Closing "}"
                break;
            }
            if pair.code == 330 {
                if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    handles.push(Handle::new(h));
                }
            }
        }
        Ok(handles)
    }

    /// Read an xdictionary handle from a {ACAD_XDICTIONARY group.
    /// Assumes the opening "102 {ACAD_XDICTIONARY" has already been consumed.
    fn read_xdictionary_handle(&mut self) -> Result<Option<Handle>> {
        let mut handle = None;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 102 {
                // Closing "}"
                break;
            }
            if pair.code == 360 {
                if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    handle = Some(Handle::new(h));
                }
            }
        }
        Ok(handle)
    }

    /// Skip an unknown defined group (reads pairs until closing "}")
    fn skip_defined_group(&mut self) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 102 && pair.value_string.trim() == "}" {
                break;
            }
        }
        Ok(())
    }

    /// Skip all pairs for the current entity until the next entity (code 0) or section end
    fn skip_entity(&mut self) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
        }
        Ok(())
    }

    /// Read an unknown entity, capturing common data and preserving
    /// entity-specific group codes for round-trip fidelity.
    fn read_unknown_entity(&mut self, dxf_name: &str) -> Result<UnknownEntity> {
        let mut entity = UnknownEntity::new(dxf_name);
        let mut raw_codes: Vec<(i32, String)> = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            // Try common entity codes first
            let consumed = self.try_read_common_entity_code(&pair, &mut entity.common)?;
            if !consumed {
                // Entity-specific code → store for round-trip
                raw_codes.push((pair.code, pair.value_string.clone()));
            }
        }
        if !raw_codes.is_empty() {
            entity.raw_dxf_codes = Some(raw_codes);
        }
        Ok(entity)
    }

    fn read_section_symbol_dxf(&mut self) -> Result<SectionSymbol> {
        let mut entity = SectionSymbol::new();
        let mut subclass = String::new();
        let mut view_symbol_70_index = 0usize;
        let mut section_90_index = 0usize;
        let mut point_index: Option<usize> = None;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string;
                continue;
            }
            if subclass == "AcDbViewSymbol" {
                match pair.code {
                    70 => {
                        let value = pair.as_i16().unwrap_or(0);
                        if view_symbol_70_index == 0 {
                            entity.view_symbol_version = value;
                        } else if view_symbol_70_index == 1 {
                            entity.raw_view_symbol_70 = value;
                        }
                        view_symbol_70_index += 1;
                    }
                    340 => {
                        entity.style_handle =
                            pair.as_handle().map(Handle::new).unwrap_or(Handle::NULL);
                    }
                    40 => {
                        entity.symbol_scale = pair.as_double().unwrap_or(1.0);
                    }
                    330 => {
                        entity.view_rep_handle =
                            pair.as_handle().map(Handle::new).unwrap_or(Handle::NULL);
                    }
                    _ => {}
                }
            } else if subclass == "AcDbSectionSymbol" {
                match pair.code {
                    70 => {
                        entity.version = pair.as_i16().unwrap_or(0);
                    }
                    90 => {
                        let value = pair.as_i32().unwrap_or(0);
                        match section_90_index {
                            0 => entity.raw_point_count_90 = value,
                            1 => entity.raw_flags_90 = value,
                            2 => {
                                entity.raw_point_record_count = value;
                                if let Ok(count) = usize::try_from(value) {
                                    entity.points.reserve(count.min(1_000_000));
                                }
                            }
                            _ => {}
                        }
                        section_90_index += 1;
                    }
                    10 => {
                        let mut point = crate::entities::SectionSymbolPoint::new();
                        point.point.x = pair.as_double().unwrap_or(0.0);
                        entity.points.push(point);
                        point_index = Some(entity.points.len() - 1);
                    }
                    code => {
                        let Some(index) = point_index else {
                            continue;
                        };
                        let point = &mut entity.points[index];
                        match code {
                            20 => {
                                point.point.y = pair.as_double().unwrap_or(0.0);
                            }
                            30 => {
                                point.point.z = pair.as_double().unwrap_or(0.0);
                            }
                            40 => {
                                point.bulge = pair.as_double().unwrap_or(0.0);
                            }
                            1 => point.label = pair.value_string,
                            11 => {
                                point.label_offset.x = pair.as_double().unwrap_or(0.0);
                            }
                            21 => {
                                point.label_offset.y = pair.as_double().unwrap_or(0.0);
                            }
                            31 => {
                                point.label_offset.z = pair.as_double().unwrap_or(0.0);
                            }
                            280 => {
                                point.raw_flag_280 = pair.as_i16().unwrap_or(0) as u8;
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                self.try_read_common_entity_code(&pair, &mut entity.common)?;
            }
        }
        entity.sync_display_fields();
        Ok(entity)
    }

    fn read_view_border_dxf(&mut self) -> Result<ViewBorder> {
        let mut entity = ViewBorder::new();
        let mut subclass = String::new();
        let mut point_index = 0usize;
        let mut double_index = 0usize;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string;
                continue;
            }
            if subclass == "AcDbViewBorder" {
                match pair.code {
                    70 => entity.version = pair.as_i16().unwrap_or(0),
                    10 => {
                        let value = pair.as_double().unwrap_or(0.0);
                        if point_index == 0 {
                            entity.min[0] = value;
                        } else if point_index == 1 {
                            entity.max[0] = value;
                        }
                    }
                    20 => {
                        let value = pair.as_double().unwrap_or(0.0);
                        if point_index == 0 {
                            entity.min[1] = value;
                        } else if point_index == 1 {
                            entity.max[1] = value;
                        }
                        point_index += 1;
                    }
                    330 => {
                        entity.active_viewport =
                            pair.as_handle().map(Handle::new).unwrap_or(Handle::NULL);
                    }
                    40 => {
                        let value = pair.as_double().unwrap_or(0.0);
                        match double_index {
                            0 => entity.scale = value,
                            1 => entity.rotation_angle = value,
                            2 => entity.center[0] = value,
                            3 => entity.center[1] = value,
                            _ => {}
                        }
                        double_index += 1;
                    }
                    340 => {
                        entity.scale_handle =
                            pair.as_handle().map(Handle::new).unwrap_or(Handle::NULL);
                    }
                    _ => {}
                }
            } else {
                self.try_read_common_entity_code(&pair, &mut entity.common)?;
            }
        }
        Ok(entity)
    }

    fn read_registered_class_entity(&mut self, dxf_name: &str) -> Result<ExtendedEntity> {
        let mut common = EntityCommon::new();
        let mut subclass = String::new();
        let mut properties = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string;
                continue;
            }
            match pair.code {
                8 => common.layer = pair.value_string,
                62 => {
                    common.color = Color::from_index(pair.as_i16().unwrap_or(256));
                }
                370 => {
                    if let Some(value) = pair.as_i16() {
                        common.line_weight = LineWeight::from_value(value);
                    }
                }
                _ => {
                    let is_common_section = subclass.is_empty() || subclass == "AcDbEntity";
                    if !is_common_section
                        || !self.try_read_common_entity_code(&pair, &mut common)?
                    {
                        properties.push(semantic_property_from_pair(&subclass, &pair));
                    }
                }
            }
        }
        Ok(ExtendedEntity {
            common,
            data: ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                dxf_name: dxf_name.to_string(),
                cpp_class_name: registered_class_cpp_name(dxf_name).to_string(),
                properties,
                payload: crate::objects::ProxyPayload::default(),
                object_ids: Vec::new(),
            }),
        })
    }

    fn read_registered_class_object(&mut self, dxf_name: &str) -> Result<RegisteredClassObject> {
        let mut object = RegisteredClassObject {
            dxf_name: dxf_name.to_string(),
            cpp_class_name: registered_class_cpp_name(dxf_name).to_string(),
            ..RegisteredClassObject::default()
        };
        let mut subclass = String::new();
        let mut group = String::new();
        let mut owner_seen = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 if object.handle.is_null() => {
                    object.handle = parse_dxf_handle(&pair.value_string);
                }
                100 => subclass = pair.value_string,
                102 => group = pair.value_string,
                330 if group == "{ACAD_REACTORS" => {
                    object.reactors.push(parse_dxf_handle(&pair.value_string))
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    object.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() => {
                    object.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                _ => object
                    .properties
                    .push(semantic_property_from_pair(&subclass, &pair)),
            }
            if group == "}" {
                group.clear();
            }
        }
        Ok(object)
    }

    fn read_proxy_object_dxf(&mut self) -> Result<ProxyObject> {
        let mut object = ProxyObject::default();
        let mut group = String::new();
        let mut owner_seen = false;
        let mut binary = Vec::new();
        let mut object_data_bits = 0u32;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 if object.handle.is_null() => {
                    object.handle = parse_dxf_handle(&pair.value_string);
                }
                102 => group = pair.value_string,
                330 if group == "{ACAD_REACTORS" => {
                    object.reactors.push(parse_dxf_handle(&pair.value_string))
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    object.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() => {
                    object.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                code @ (330 | 340 | 350 | 360) if group.is_empty() => {
                    let kind = match code {
                        330 => crate::objects::ProxyReferenceKind::SoftOwnership,
                        340 => crate::objects::ProxyReferenceKind::HardOwnership,
                        350 => crate::objects::ProxyReferenceKind::SoftPointer,
                        360 => crate::objects::ProxyReferenceKind::HardPointer,
                        _ => unreachable!(),
                    };
                    object
                        .object_ids
                        .push(crate::objects::ProxyObjectReference {
                            handle: parse_dxf_handle(&pair.value_string),
                            kind,
                        });
                }
                90 => object.proxy_id = pair.as_i32().unwrap_or(499),
                91 => object.class_id = pair.as_i32().unwrap_or(499),
                95 => object.version = pair.as_i32().unwrap_or(0),
                71 => object.dwg_version = pair.as_i32().unwrap_or(0),
                97 => object.maintenance_version = pair.as_i32().unwrap_or(0),
                70 => object.from_dxf = pair.as_i16().unwrap_or(0) != 0,
                93 => {
                    object_data_bits = pair.as_i32().unwrap_or(0).max(0) as u32;
                }
                310 => append_hex_bytes(&mut binary, &pair.value_string),
                _ => {}
            }
            if group == "}" {
                group.clear();
            }
        }
        object.payload = crate::objects::ProxyPayload::from_bits(&binary, object_data_bits);
        if object.dwg_version == 0 && object.maintenance_version == 0 {
            object.dwg_version = object.version & 0xffff;
            object.maintenance_version = object.version >> 16;
        } else {
            object.version = (object.maintenance_version << 16) | (object.dwg_version & 0xffff);
        }
        Ok(object)
    }

    fn read_dgn_line_style_object(&mut self, dxf_name: &str) -> Result<DgnLineStyleObject> {
        let mut object = self.read_registered_class_object(dxf_name)?;
        let name = dxf_name.to_uppercase();
        if name == "LSDEFINITION" {
            let subclass = "AcDbLSDefinition";
            let description = match take_dgn_property(&mut object.properties, subclass, 1) {
                Some(SemanticPropertyValue::Text(value)) => value,
                _ => String::new(),
            };
            let version = dgn_i32(take_dgn_property(&mut object.properties, subclass, 90), 0);
            let style_number = dgn_i32(take_dgn_property(&mut object.properties, subclass, 91), 0);
            return Ok(DgnLineStyleObject {
                handle: object.handle,
                owner: object.owner,
                reactors: object.reactors,
                xdictionary_handle: object.xdictionary_handle,
                data: DgnLineStyleData::Definition {
                    description,
                    version,
                    style_number,
                    component_uid: dgn_uid(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        310,
                    )),
                    is_continuous: dgn_bool(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        290,
                    )),
                    unit_definition: dgn_f64(
                        take_dgn_property(&mut object.properties, subclass, 40),
                        0.0,
                    ),
                    unit_scale: dgn_f64(
                        take_dgn_property(&mut object.properties, subclass, 41),
                        0.0,
                    ),
                    units_type: dgn_i32(take_dgn_property(&mut object.properties, subclass, 92), 0),
                    is_element: dgn_bool(take_dgn_property(&mut object.properties, subclass, 291)),
                    is_physical: dgn_bool(take_dgn_property(&mut object.properties, subclass, 292)),
                    is_scale_independent: dgn_bool(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        293,
                    )),
                    is_snappable: dgn_bool(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        294,
                    )),
                    root_component: dgn_handle(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        340,
                    )),
                    properties: object.properties,
                },
            });
        }
        let (kind, subclass) = match name.as_str() {
            "LSSYMBOLCOMPONENT" => (DgnLsComponentType::Symbol, "AcDbLSSymbolComponent"),
            "LSCOMPOUNDCOMPONENT" => (DgnLsComponentType::Compound, "AcDbLSCompoundComponent"),
            "LSSTROKEPATTERNCOMPONENT" => {
                (DgnLsComponentType::Stroke, "AcDbLSStrokePatternComponent")
            }
            "LSPOINTCOMPONENT" => (DgnLsComponentType::Point, "AcDbLSPointComponent"),
            "LSINTERNALCOMPONENT" => (DgnLsComponentType::Internal, "AcDbLSInternalComponent"),
            _ => {
                return Ok(DgnLineStyleObject {
                    handle: object.handle,
                    owner: object.owner,
                    reactors: object.reactors,
                    xdictionary_handle: object.xdictionary_handle,
                    data: DgnLineStyleData::Registered {
                        dxf_name: dxf_name.to_string(),
                        properties: object.properties,
                        payload: object.payload,
                        object_ids: object.object_ids,
                    },
                });
            }
        };
        let description = match take_dgn_property(&mut object.properties, subclass, 1) {
            Some(SemanticPropertyValue::Text(value)) => value,
            _ => String::new(),
        };
        let version = dgn_i32(take_dgn_property(&mut object.properties, subclass, 90), 0);
        let _component_type = take_dgn_property(&mut object.properties, subclass, 91);
        let component_uid = dgn_uid(take_dgn_property(&mut object.properties, subclass, 310));
        let scale = dgn_f64(take_dgn_property(&mut object.properties, subclass, 40), 1.0);
        let property_flags =
            dgn_i32(take_dgn_property(&mut object.properties, subclass, 280), 0) as u8;
        let component = match kind {
            DgnLsComponentType::Symbol => DgnLsComponentData::Symbol(DgnLsSymbolComponent {
                stored_unit_scale: dgn_f64(
                    take_dgn_property(&mut object.properties, subclass, 41),
                    1.0,
                ),
                unit_scale: dgn_f64(take_dgn_property(&mut object.properties, subclass, 42), 1.0),
                has_unit_scale: dgn_bool(take_dgn_property(&mut object.properties, subclass, 290)),
                is_3d: dgn_bool(take_dgn_property(&mut object.properties, subclass, 291)),
                block: dgn_handle(take_dgn_property(&mut object.properties, subclass, 340)),
            }),
            DgnLsComponentType::Compound => {
                let offsets = take_all_dgn_properties(&mut object.properties, subclass, 41);
                let handles = take_all_dgn_properties(&mut object.properties, subclass, 340);
                let count = offsets.len().max(handles.len());
                let mut entries = Vec::with_capacity(count);
                for index in 0..count {
                    entries.push(DgnLsCompoundEntry {
                        component: dgn_handle(handles.get(index).cloned()),
                        offset: dgn_f64(offsets.get(index).cloned(), 0.0),
                    });
                }
                DgnLsComponentData::Compound(DgnLsCompoundComponent { entries })
            }
            DgnLsComponentType::Stroke => DgnLsComponentData::Stroke(read_dgn_stroke_pattern_dxf(
                &mut object.properties,
                subclass,
            )),
            DgnLsComponentType::Point => {
                let count = dgn_i32(take_dgn_property(&mut object.properties, subclass, 93), 0)
                    .max(0) as usize;
                let stroke_component =
                    dgn_handle(take_dgn_property(&mut object.properties, subclass, 340));
                let mut symbol_handles =
                    take_all_dgn_properties(&mut object.properties, subclass, 341);
                let mut partial = take_all_dgn_properties(&mut object.properties, subclass, 290);
                let mut clip = take_all_dgn_properties(&mut object.properties, subclass, 291);
                let mut stretch = take_all_dgn_properties(&mut object.properties, subclass, 292);
                let mut projected = take_all_dgn_properties(&mut object.properties, subclass, 293);
                let mut colors = take_all_dgn_properties(&mut object.properties, subclass, 294);
                let mut lineweights =
                    take_all_dgn_properties(&mut object.properties, subclass, 295);
                let mut justify = take_all_dgn_properties(&mut object.properties, subclass, 92);
                let mut rotations = take_all_dgn_properties(&mut object.properties, subclass, 94);
                let mut vertices = take_all_dgn_properties(&mut object.properties, subclass, 95);
                let mut x_offsets = take_all_dgn_properties(&mut object.properties, subclass, 41);
                let mut y_offsets = take_all_dgn_properties(&mut object.properties, subclass, 42);
                let mut angles = take_all_dgn_properties(&mut object.properties, subclass, 43);
                let mut stroke_numbers =
                    take_all_dgn_properties(&mut object.properties, subclass, 96);
                let mut symbols = Vec::with_capacity(count);
                for _ in 0..count {
                    symbols.push(DgnLsSymbolReference {
                        symbol_component: dgn_handle(take_first(&mut symbol_handles)),
                        partial_strokes: dgn_bool(take_first(&mut partial)),
                        clip_partial: dgn_bool(take_first(&mut clip)),
                        allow_stretch: dgn_bool(take_first(&mut stretch)),
                        partial_projected: dgn_bool(take_first(&mut projected)),
                        use_symbol_color: dgn_bool(take_first(&mut colors)),
                        use_symbol_lineweight: dgn_bool(take_first(&mut lineweights)),
                        justify: dgn_i32(take_first(&mut justify), 0),
                        rotation_type: dgn_i32(take_first(&mut rotations), 0),
                        vertex_mask: dgn_i32(take_first(&mut vertices), 0),
                        x_offset: dgn_f64(take_first(&mut x_offsets), 0.0),
                        y_offset: dgn_f64(take_first(&mut y_offsets), 0.0),
                        angle: dgn_f64(take_first(&mut angles), 0.0),
                        stroke_number: dgn_i32(take_first(&mut stroke_numbers), 0),
                    });
                }
                DgnLsComponentData::Point(DgnLsPointComponent {
                    stroke_component,
                    symbols,
                })
            }
            DgnLsComponentType::Internal => {
                let pattern = read_dgn_stroke_pattern_dxf(&mut object.properties, subclass);
                DgnLsComponentData::Internal(DgnLsInternalComponent {
                    pattern,
                    internal_version: dgn_i32(
                        take_dgn_property(&mut object.properties, subclass, 96),
                        0,
                    ),
                    hardware_style: dgn_i32(
                        take_dgn_property(&mut object.properties, subclass, 97),
                        0,
                    ),
                    is_hardware_style: dgn_bool(take_dgn_property(
                        &mut object.properties,
                        subclass,
                        297,
                    )),
                    line_code: dgn_i32(take_dgn_property(&mut object.properties, subclass, 98), 0),
                })
            }
        };
        Ok(DgnLineStyleObject {
            handle: object.handle,
            owner: object.owner,
            reactors: object.reactors,
            xdictionary_handle: object.xdictionary_handle,
            data: DgnLineStyleData::Component {
                kind,
                description,
                version,
                component_uid,
                scale,
                property_flags,
                component,
                properties: object.properties,
            },
        })
    }

    /// Read an OLE2FRAME entity
    fn read_ole2frame(&mut self) -> Result<Option<Ole2Frame>> {
        let mut ole = Ole2Frame::new();
        let mut binary_chunks: Vec<Vec<u8>> = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                70 => {
                    if let Some(v) = pair.as_i16() {
                        ole.version = v;
                    }
                }
                3 => ole.source_application = pair.value_string.clone(),
                10 => {
                    if let Some(v) = pair.as_double() {
                        ole.upper_left_corner.x = v;
                    }
                }
                20 => {
                    if let Some(v) = pair.as_double() {
                        ole.upper_left_corner.y = v;
                    }
                }
                30 => {
                    if let Some(v) = pair.as_double() {
                        ole.upper_left_corner.z = v;
                    }
                }
                11 => {
                    if let Some(v) = pair.as_double() {
                        ole.lower_right_corner.x = v;
                    }
                }
                21 => {
                    if let Some(v) = pair.as_double() {
                        ole.lower_right_corner.y = v;
                    }
                }
                31 => {
                    if let Some(v) = pair.as_double() {
                        ole.lower_right_corner.z = v;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        ole.ole_object_type = OleObjectType::from_i16(v);
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        ole.is_paper_space = v != 0;
                        ole.dwg_mode = if ole.is_paper_space { 1 } else { 0 };
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        ole.lock_aspect = v.clamp(0, u8::MAX as i16) as u8;
                    }
                }
                310 => {
                    // Binary data chunk (hex-encoded)
                    let hex = pair.value_string.trim();
                    if let Ok(bytes) = (0..hex.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&hex[i..i.min(hex.len()).max(i + 2)], 16))
                        .collect::<std::result::Result<Vec<u8>, _>>()
                    {
                        binary_chunks.push(bytes);
                    }
                }
                1 | 90 => { /* end marker and length */ }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut ole.common)?;
                }
            }
        }

        // Concatenate binary chunks
        let data: Vec<u8> = binary_chunks.into_iter().flatten().collect();
        let (storage, envelope, _, _) = Ole2Frame::decode_payload(&data);
        ole.storage = storage;
        ole.envelope = envelope;

        Ok(Some(ole))
    }

    // ===== Entity Readers =====

    /// Read a POINT entity
    fn read_point(&mut self) -> Result<Option<Point>> {
        let mut point = Point::new();
        let mut location = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => point.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        point.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        point.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    location.add_coordinate(&pair);
                }
                39 => {
                    if let Some(thickness) = pair.as_double() {
                        point.thickness = thickness;
                    }
                }
                50 => {
                    if let Some(a) = pair.as_double() {
                        point.x_axis_angle = a;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut point.common)?;
                }
            }
        }

        if let Some(pt) = location.get_point() {
            point.location = pt;
        }
        if let Some(n) = normal.get_point() {
            point.normal = n;
        }

        Ok(Some(point))
    }

    /// Read a LINE entity
    fn read_line(&mut self) -> Result<Option<Line>> {
        let mut line = Line::new();
        let mut start = PointReader::new();
        let mut end = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                // Push back the code 0 pair so it can be read by the caller
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => line.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        line.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    start.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    end.add_coordinate(&pair);
                }
                39 => {
                    if let Some(thickness) = pair.as_double() {
                        line.thickness = thickness;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut line.common)?;
                }
            }
        }

        if let Some(pt) = start.get_point() {
            line.start = pt;
        }
        if let Some(pt) = end.get_point() {
            line.end = pt;
        }
        if let Some(n) = normal.get_point() {
            line.normal = n;
        }

        Ok(Some(line))
    }

    /// Read a CIRCLE entity
    fn read_circle(&mut self) -> Result<Option<Circle>> {
        let mut circle = Circle::new();
        let mut center = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => circle.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        circle.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        circle.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                40 => {
                    if let Some(radius) = pair.as_double() {
                        circle.radius = radius;
                    }
                }
                39 => {
                    if let Some(thickness) = pair.as_double() {
                        circle.thickness = thickness;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut circle.common)?;
                }
            }
        }

        if let Some(pt) = center.get_point() {
            circle.center = pt;
        }
        if let Some(n) = normal.get_point() {
            circle.normal = n;
        }

        Ok(Some(circle))
    }

    /// Read an ARC entity
    fn read_arc(&mut self) -> Result<Option<Arc>> {
        let mut arc = Arc::new();
        let mut center = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => arc.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        arc.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        arc.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                40 => {
                    if let Some(radius) = pair.as_double() {
                        arc.radius = radius;
                    }
                }
                50 => {
                    if let Some(angle) = pair.as_double() {
                        arc.start_angle = angle.to_radians();
                    }
                }
                51 => {
                    if let Some(angle) = pair.as_double() {
                        arc.end_angle = angle.to_radians();
                    }
                }
                39 => {
                    if let Some(thickness) = pair.as_double() {
                        arc.thickness = thickness;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut arc.common)?;
                }
            }
        }

        if let Some(pt) = center.get_point() {
            arc.center = pt;
        }
        if let Some(n) = normal.get_point() {
            arc.normal = n;
        }

        Ok(Some(arc))
    }

    /// Read an ELLIPSE entity
    fn read_ellipse(&mut self) -> Result<Option<Ellipse>> {
        let mut ellipse = Ellipse::new();
        let mut center = PointReader::new();
        let mut major_axis = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => ellipse.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        ellipse.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        ellipse.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    major_axis.add_coordinate(&pair);
                }
                40 => {
                    if let Some(ratio) = pair.as_double() {
                        ellipse.minor_axis_ratio = ratio;
                    }
                }
                41 => {
                    if let Some(angle) = pair.as_double() {
                        ellipse.start_parameter = angle;
                    }
                }
                42 => {
                    if let Some(angle) = pair.as_double() {
                        ellipse.end_parameter = angle;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut ellipse.common)?;
                }
            }
        }

        if let Some(pt) = center.get_point() {
            ellipse.center = pt;
        }
        if let Some(pt) = major_axis.get_point() {
            ellipse.major_axis = pt;
        }
        if let Some(n) = normal.get_point() {
            ellipse.normal = n;
        }

        Ok(Some(ellipse))
    }

    /// Read a POLYLINE or POLYFACE MESH entity, returning the appropriate EntityType.
    fn read_polyline_entity(&mut self) -> Result<Option<EntityType>> {
        use crate::entities::polyface_mesh::{
            PolyfaceFace, PolyfaceMesh, PolyfaceMeshFlags, PolyfaceVertex, PolyfaceVertexFlags,
        };
        use crate::entities::polygon_mesh::{
            PolygonMesh, PolygonMeshFlags, PolygonMeshVertex, SurfaceSmoothType,
        };
        use crate::entities::polyline::{PolylineFlags, SmoothSurfaceType, Vertex2D, VertexFlags};
        use crate::entities::polyline3d::{
            Polyline3D, Polyline3DFlags, SmoothSurfaceType as SmoothSurface3D, Vertex3DPolyline,
        };

        // One captured geometry vertex — mapped to the target vertex type once
        // the POLYLINE flags (code 70) tell us which kind of polyline this is.
        struct RawVertex {
            loc: crate::types::Vector3,
            vflags: i16,
            start_width: f64,
            end_width: f64,
            // Whether codes 40/41 were actually present on this VERTEX. An
            // absent width inherits the POLYLINE default; an explicit 0 means a
            // genuinely zero-width segment. Collapsing both to 0.0 loses that
            // distinction and makes tapered polylines render as constant width.
            has_start_width: bool,
            has_end_width: bool,
            bulge: f64,
            tangent: f64,
            handle: u64,
            layer: String,
        }

        let mut common = EntityCommon::new();
        let mut flags: i16 = 0;
        let mut elevation = 0.0f64;
        let mut thickness = 0.0f64;
        let mut def_start_width = 0.0f64;
        let mut def_end_width = 0.0f64;
        let mut count_m: i16 = 0; // 71 (mesh M / pface vert count)
        let mut count_n: i16 = 0; // 72 (mesh N / pface face count)
        let mut density_m: i16 = 0; // 73
        let mut density_n: i16 = 0; // 74
        let mut smooth: i16 = 0; // 75 (smooth surface type)
        let mut normal = PointReader::new();
        let mut geom_vertices: Vec<RawVertex> = Vec::new();
        let mut pface_vertices: Vec<PolyfaceVertex> = Vec::new();
        let mut pface_faces: Vec<PolyfaceFace> = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                if pair.value_string == "VERTEX" {
                    // --- Read one VERTEX subentity ---
                    let mut loc = crate::types::Vector3::ZERO;
                    let mut vflags: i16 = 0;
                    let mut sw = 0.0f64;
                    let mut ew = 0.0f64;
                    let mut has_sw = false;
                    let mut has_ew = false;
                    let mut bulge = 0.0f64;
                    let mut tangent = 0.0f64;
                    let mut vi1: i16 = 0;
                    let mut vi2: i16 = 0;
                    let mut vi3: i16 = 0;
                    let mut vi4: i16 = 0;
                    let mut vcolor: Option<Color> = None;
                    let mut vhandle: u64 = 0;
                    let mut vlayer = String::from("0");
                    while let Some(vpair) = self.reader.read_pair()? {
                        if vpair.code == 0 {
                            self.reader.push_back(vpair);
                            break;
                        }
                        match vpair.code {
                            5 => {
                                if let Some(h) = vpair.as_handle() {
                                    vhandle = h;
                                }
                            }
                            8 => {
                                vlayer = vpair.value_string.clone();
                            }
                            10 => {
                                if let Some(v) = vpair.as_double() {
                                    loc.x = v;
                                }
                            }
                            20 => {
                                if let Some(v) = vpair.as_double() {
                                    loc.y = v;
                                }
                            }
                            30 => {
                                if let Some(v) = vpair.as_double() {
                                    loc.z = v;
                                }
                            }
                            40 => {
                                if let Some(v) = vpair.as_double() {
                                    sw = v;
                                    has_sw = true;
                                }
                            }
                            41 => {
                                if let Some(v) = vpair.as_double() {
                                    ew = v;
                                    has_ew = true;
                                }
                            }
                            42 => {
                                if let Some(v) = vpair.as_double() {
                                    bulge = v;
                                }
                            }
                            50 => {
                                if let Some(v) = vpair.as_double() {
                                    tangent = v;
                                }
                            }
                            62 => {
                                if let Some(ci) = vpair.as_i16() {
                                    vcolor = Some(Color::from_index(ci));
                                }
                            }
                            420 => {
                                if let Some(tc) = vpair.as_i32() {
                                    vcolor = Some(Color::from_true_color_value(tc));
                                }
                            }
                            70 => {
                                if let Some(v) = vpair.as_i16() {
                                    vflags = v;
                                }
                            }
                            71 => {
                                if let Some(v) = vpair.as_i16() {
                                    vi1 = v;
                                }
                            }
                            72 => {
                                if let Some(v) = vpair.as_i16() {
                                    vi2 = v;
                                }
                            }
                            73 => {
                                if let Some(v) = vpair.as_i16() {
                                    vi3 = v;
                                }
                            }
                            74 => {
                                if let Some(v) = vpair.as_i16() {
                                    vi4 = v;
                                }
                            }
                            _ => {}
                        }
                    }
                    // Geometry vertex detection: bit 6 (64 = POLYGON_MESH) trumps bit 7
                    // (128 = POLYFACE_MESH).  Internally vertices are stored with
                    // flags=128, then ORed with 64 by the writer => written flag = 192.
                    // Face records are written with flag=128 only.
                    // Therefore: check bit 64 FIRST.
                    if (vflags & 64) != 0 {
                        // Polyface geometry vertex
                        pface_vertices.push(PolyfaceVertex {
                            common: EntityCommon::default(),
                            location: loc,
                            flags: PolyfaceVertexFlags::from_bits_truncate(vflags),
                            bulge: 0.0,
                            start_width: 0.0,
                            end_width: 0.0,
                            curve_tangent: 0.0,
                            id: 0,
                        });
                    } else if (vflags & 128) != 0 {
                        // Face record (only bit 128 set, no bit 64)
                        pface_faces.push(PolyfaceFace {
                            common: EntityCommon::default(),
                            flags: PolyfaceVertexFlags::from_bits_truncate(vflags),
                            index1: vi1,
                            index2: vi2,
                            index3: vi3,
                            index4: vi4,
                            color: vcolor,
                        });
                    } else {
                        // Plain polyline / polygon-mesh vertex — keep every field
                        // so the type-specific mapping below can use them.
                        geom_vertices.push(RawVertex {
                            loc,
                            vflags,
                            start_width: sw,
                            end_width: ew,
                            has_start_width: has_sw,
                            has_end_width: has_ew,
                            bulge,
                            tangent,
                            handle: vhandle,
                            layer: vlayer.clone(),
                        });
                    }
                } else if pair.value_string == "SEQEND" {
                    while let Some(seqend_pair) = self.reader.read_pair()? {
                        if seqend_pair.code == 0 {
                            self.reader.push_back(seqend_pair);
                            break;
                        }
                    }
                    break;
                } else {
                    self.reader.push_back(pair);
                    break;
                }
            } else {
                match pair.code {
                    8 => common.layer = pair.value_string.clone(),
                    62 => {
                        if let Some(ci) = pair.as_i16() {
                            common.color = Color::from_index(ci);
                        }
                    }
                    370 => {
                        if let Some(lw) = pair.as_i16() {
                            common.line_weight = LineWeight::from_value(lw);
                        }
                    }
                    70 => {
                        if let Some(f) = pair.as_i16() {
                            flags = f;
                        }
                    }
                    30 => {
                        if let Some(z) = pair.as_double() {
                            elevation = z;
                        }
                    }
                    39 => {
                        if let Some(t) = pair.as_double() {
                            thickness = t;
                        }
                    }
                    40 => {
                        if let Some(w) = pair.as_double() {
                            def_start_width = w;
                        }
                    }
                    41 => {
                        if let Some(w) = pair.as_double() {
                            def_end_width = w;
                        }
                    }
                    71 => {
                        if let Some(v) = pair.as_i16() {
                            count_m = v;
                        }
                    }
                    72 => {
                        if let Some(v) = pair.as_i16() {
                            count_n = v;
                        }
                    }
                    73 => {
                        if let Some(v) = pair.as_i16() {
                            density_m = v;
                        }
                    }
                    74 => {
                        if let Some(v) = pair.as_i16() {
                            density_n = v;
                        }
                    }
                    75 => {
                        if let Some(v) = pair.as_i16() {
                            smooth = v;
                        }
                    }
                    210 | 220 | 230 => {
                        normal.add_coordinate(&pair);
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                }
            }
        }

        let normal_v = normal
            .get_point()
            .unwrap_or(crate::types::Vector3::new(0.0, 0.0, 1.0));

        // Route by POLYLINE flag bits (code 70): 64 = polyface mesh, 16 = polygon
        // mesh, 8 = 3D polyline, otherwise a 2D (heavy) polyline. Previously
        // every non-polyface POLYLINE collapsed to a generic Polyline, dropping
        // the 2D widths/bulge, the polygon-mesh grid and the 3D-polyline type.
        if (flags & 64) != 0 || !pface_vertices.is_empty() || !pface_faces.is_empty() {
            let mut mesh = PolyfaceMesh::new();
            mesh.common = common;
            mesh.flags = PolyfaceMeshFlags::from_bits_truncate(flags);
            mesh.elevation = elevation;
            mesh.vertices = pface_vertices;
            mesh.faces = pface_faces;
            Ok(Some(EntityType::PolyfaceMesh(mesh)))
        } else if (flags & 16) != 0 {
            let mut mesh = PolygonMesh::new();
            mesh.common = common;
            mesh.flags = PolygonMeshFlags::from_bits_truncate(flags);
            mesh.m_vertex_count = count_m;
            mesh.n_vertex_count = count_n;
            mesh.m_smooth_density = density_m;
            mesh.n_smooth_density = density_n;
            mesh.smooth_type = SurfaceSmoothType::from_i16(smooth);
            mesh.elevation = elevation;
            mesh.normal = normal_v;
            mesh.vertices = geom_vertices
                .iter()
                .map(|rv| PolygonMeshVertex {
                    common: EntityCommon::default(),
                    location: rv.loc,
                    flags: rv.vflags,
                })
                .collect();
            Ok(Some(EntityType::PolygonMesh(mesh)))
        } else if (flags & 8) != 0 {
            let mut pl = Polyline3D::new();
            pl.common = common;
            pl.flags = Polyline3DFlags::from_bits(flags as i32);
            pl.smooth_type = SmoothSurface3D::from_value(smooth);
            pl.default_start_width = def_start_width;
            pl.default_end_width = def_end_width;
            pl.elevation = elevation;
            pl.normal = normal_v;
            pl.vertices = geom_vertices
                .iter()
                .map(|rv| {
                    let mut v = Vertex3DPolyline::new(rv.loc);
                    v.flags = rv.vflags as i32;
                    v.handle = Handle::new(rv.handle);
                    v.layer = rv.layer.clone();
                    v
                })
                .collect();
            Ok(Some(EntityType::Polyline3D(pl)))
        } else {
            let mut pl = Polyline2D::new();
            pl.common = common;
            pl.flags = PolylineFlags::from_bits(flags as u16);
            pl.smooth_surface = SmoothSurfaceType::from(smooth);
            pl.thickness = thickness;
            pl.elevation = elevation;
            pl.normal = normal_v;
            // A VERTEX without codes 40/41 inherits the POLYLINE default width;
            // one that carries them (even as 0) keeps its own value. Bake the
            // effective width into each vertex so tapered polylines survive —
            // this mirrors how a heavy polyline down-saves to LWPOLYLINE.
            pl.vertices = geom_vertices
                .iter()
                .map(|rv| {
                    let mut v = Vertex2D::new(rv.loc);
                    v.flags = VertexFlags::from_bits(rv.vflags as u8);
                    v.start_width = if rv.has_start_width {
                        rv.start_width
                    } else {
                        def_start_width
                    };
                    v.end_width = if rv.has_end_width {
                        rv.end_width
                    } else {
                        def_end_width
                    };
                    v.bulge = rv.bulge;
                    v.curve_tangent = rv.tangent;
                    v
                })
                .collect();
            // When any vertex specifies its own width, the widths now live
            // per-vertex; keeping a non-zero polyline default would let a
            // renderer bleed it onto the intentionally zero-width segments.
            let per_vertex_widths = geom_vertices
                .iter()
                .any(|rv| rv.has_start_width || rv.has_end_width);
            if per_vertex_widths {
                pl.start_width = 0.0;
                pl.end_width = 0.0;
            } else {
                pl.start_width = def_start_width;
                pl.end_width = def_end_width;
            }
            Ok(Some(EntityType::Polyline2D(pl)))
        }
    }

    /// Read an LWPOLYLINE entity
    fn read_lwpolyline(&mut self) -> Result<Option<LwPolyline>> {
        use crate::entities::lwpolyline::LwVertex;
        use crate::types::Vector2;

        let mut lwpolyline = LwPolyline::new();
        let mut normal = PointReader::new();
        // Track per-vertex state: code 10 starts a new vertex, codes 20/40/41/42
        // apply to the current vertex. Omitted codes default to 0.0.
        let mut vertices: Vec<LwVertex> = Vec::new();
        let mut current_x: Option<f64> = None;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => lwpolyline.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        lwpolyline.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        lwpolyline.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                70 => {
                    if let Some(flags) = pair.as_i16() {
                        lwpolyline.is_closed = (flags & 1) != 0;
                        lwpolyline.plinegen = (flags & 128) != 0;
                    }
                }
                38 => {
                    if let Some(elevation) = pair.as_double() {
                        lwpolyline.elevation = elevation;
                    }
                }
                39 => {
                    if let Some(thickness) = pair.as_double() {
                        lwpolyline.thickness = thickness;
                    }
                }
                43 => {
                    if let Some(cw) = pair.as_double() {
                        lwpolyline.constant_width = cw;
                    }
                }
                10 => {
                    // Code 10 starts a new vertex with defaults
                    if let Some(x) = pair.as_double() {
                        current_x = Some(x);
                    }
                }
                20 => {
                    // Code 20 completes the vertex position; push a new vertex
                    if let (Some(x), Some(y)) = (current_x.take(), pair.as_double()) {
                        vertices.push(LwVertex {
                            location: Vector2::new(x, y),
                            bulge: 0.0,
                            start_width: 0.0,
                            end_width: 0.0,
                            vertex_id: 0,
                        });
                    }
                }
                42 => {
                    if let Some(bulge) = pair.as_double() {
                        if let Some(v) = vertices.last_mut() {
                            v.bulge = bulge;
                        }
                    }
                }
                91 => {
                    if let Some(vertex_id) = pair.as_i32() {
                        if let Some(v) = vertices.last_mut() {
                            v.vertex_id = vertex_id;
                        }
                    }
                }
                40 => {
                    if let Some(width) = pair.as_double() {
                        if let Some(v) = vertices.last_mut() {
                            v.start_width = width;
                        }
                    }
                }
                41 => {
                    if let Some(width) = pair.as_double() {
                        if let Some(v) = vertices.last_mut() {
                            v.end_width = width;
                        }
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut lwpolyline.common)?;
                }
            }
        }

        lwpolyline.vertices = vertices;

        if let Some(n) = normal.get_point() {
            lwpolyline.normal = n;
        }

        Ok(Some(lwpolyline))
    }

    /// Read a TEXT entity
    fn read_text(&mut self) -> Result<Option<Text>> {
        use crate::entities::text::{TextHorizontalAlignment, TextVerticalAlignment};

        let mut text = Text::new();
        let mut insertion = PointReader::new();
        let mut alignment = PointReader::new();
        let mut normal = PointReader::new();
        let mut has_alignment_point = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => text.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        text.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        text.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    insertion.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    alignment.add_coordinate(&pair);
                    has_alignment_point = true;
                }
                1 => text.value = pair.value_string.clone(),
                40 => {
                    if let Some(height) = pair.as_double() {
                        text.height = height;
                    }
                }
                50 => {
                    if let Some(rotation) = pair.as_double() {
                        text.rotation = rotation.to_radians();
                    }
                }
                41 => {
                    if let Some(width_factor) = pair.as_double() {
                        text.width_factor = width_factor;
                    }
                }
                51 => {
                    if let Some(oblique) = pair.as_double() {
                        text.oblique_angle = oblique.to_radians();
                    }
                }
                39 => {
                    if let Some(t) = pair.as_double() {
                        text.thickness = t;
                    }
                }
                71 => {
                    if let Some(g) = pair.as_i16() {
                        text.generation_flags = g;
                    }
                }
                72 => {
                    if let Some(h) = pair.as_i16() {
                        text.horizontal_alignment = match h {
                            1 => TextHorizontalAlignment::Center,
                            2 => TextHorizontalAlignment::Right,
                            3 => TextHorizontalAlignment::Aligned,
                            4 => TextHorizontalAlignment::Middle,
                            5 => TextHorizontalAlignment::Fit,
                            _ => TextHorizontalAlignment::Left,
                        };
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        text.vertical_alignment = match v {
                            1 => TextVerticalAlignment::Bottom,
                            2 => TextVerticalAlignment::Middle,
                            3 => TextVerticalAlignment::Top,
                            _ => TextVerticalAlignment::Baseline,
                        };
                    }
                }
                7 => text.style = pair.value_string.clone(),
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut text.common)?;
                }
            }
        }

        if let Some(pt) = insertion.get_point() {
            text.insertion_point = pt;
        }
        if has_alignment_point {
            text.alignment_point = alignment.get_point();
        }
        if let Some(n) = normal.get_point() {
            text.normal = n;
        }

        Ok(Some(text))
    }

    /// Read an MTEXT entity
    fn read_mtext(&mut self) -> Result<Option<MText>> {
        use crate::entities::mtext::{AttachmentPoint, DrawingDirection};

        let mut mtext = MText::new();
        let mut insertion = PointReader::new();
        let mut normal = PointReader::new();
        let mut x_direction = PointReader::new();
        let mut reading_columns = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => mtext.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        mtext.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        mtext.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    insertion.add_coordinate(&pair);
                }
                1 | 3 => {
                    // Text content (can be split across multiple codes)
                    mtext.value.push_str(&pair.value_string);
                }
                40 => {
                    if let Some(height) = pair.as_double() {
                        mtext.height = height;
                    }
                }
                41 => {
                    if let Some(width) = pair.as_double() {
                        mtext.rectangle_width = width;
                    }
                }
                42 => {
                    if let Some(w) = pair.as_double() {
                        mtext.extents_width = w;
                    }
                }
                43 => {
                    if let Some(h) = pair.as_double() {
                        mtext.extents_height = h;
                    }
                }
                50 => {
                    if let Some(value) = pair.as_double() {
                        if reading_columns {
                            let columns = &mut mtext.column_data;
                            let height_count = columns.column_count.max(0) as usize;
                            if columns.column_type == 2
                                && !columns.auto_height
                                && columns.heights.len() < height_count
                            {
                                columns.heights.push(value);
                            }
                        } else {
                            mtext.rotation = value.to_radians();
                        }
                    }
                }
                71 => {
                    if let Some(ap) = pair.as_i16() {
                        mtext.attachment_point = match ap {
                            2 => AttachmentPoint::TopCenter,
                            3 => AttachmentPoint::TopRight,
                            4 => AttachmentPoint::MiddleLeft,
                            5 => AttachmentPoint::MiddleCenter,
                            6 => AttachmentPoint::MiddleRight,
                            7 => AttachmentPoint::BottomLeft,
                            8 => AttachmentPoint::BottomCenter,
                            9 => AttachmentPoint::BottomRight,
                            _ => AttachmentPoint::TopLeft,
                        };
                    }
                }
                72 => {
                    if let Some(dd) = pair.as_i16() {
                        mtext.drawing_direction = match dd {
                            3 => DrawingDirection::TopToBottom,
                            5 => DrawingDirection::ByStyle,
                            _ => DrawingDirection::LeftToRight,
                        };
                    }
                }
                44 => {
                    if let Some(lsf) = pair.as_double() {
                        mtext.line_spacing_factor = lsf;
                    }
                }
                // Standard DXF column data. These codes follow 75 and reuse
                // numbers that mean something else earlier in AcDbMText.
                75 => {
                    if let Some(value) = pair.as_i16() {
                        mtext.column_data.column_type = value;
                        reading_columns = value != 0;
                    }
                }
                76 if reading_columns => {
                    if let Some(value) = pair.as_i16() {
                        mtext.column_data.column_count = value as i32;
                    }
                }
                78 if reading_columns => {
                    if let Some(value) = pair.as_i16() {
                        mtext.column_data.flow_reversed = value != 0;
                    }
                }
                79 if reading_columns => {
                    if let Some(value) = pair.as_i16() {
                        mtext.column_data.auto_height = value != 0;
                    }
                }
                48 if reading_columns => {
                    if let Some(value) = pair.as_double() {
                        mtext.column_data.width = value;
                    }
                }
                49 if reading_columns => {
                    if let Some(value) = pair.as_double() {
                        mtext.column_data.gutter = value;
                    }
                }
                73 => {
                    if let Some(ls) = pair.as_i16() {
                        mtext.line_spacing_style = crate::entities::LineSpacingStyle::from(ls);
                    }
                }
                7 => mtext.style = pair.value_string.clone(),
                // X-axis direction vector (takes priority over rotation 50).
                11 | 21 | 31 => {
                    x_direction.add_coordinate(&pair);
                }
                // Defined rectangle height (0 = auto).
                46 => {
                    if let Some(h) = pair.as_double() {
                        mtext.rectangle_height = if h != 0.0 { Some(h) } else { None };
                    }
                }
                // Background fill: flags / scale / colour (ACI or true colour) /
                // transparency.
                90 => {
                    if let Some(f) = pair.as_i32() {
                        mtext.background_fill_flags = f;
                    }
                }
                45 => {
                    if let Some(s) = pair.as_double() {
                        mtext.background_scale = s;
                    }
                }
                63 => {
                    // Background fill colour (ACI). Parse the raw value: code 63
                    // is not classified as an integer group code, so as_i16()
                    // returns None. A following 421 (true colour) overrides.
                    if let Ok(ci) = pair.value_string.trim().parse::<i16>() {
                        mtext.background_color = Color::from_index(ci);
                    }
                }
                421 => {
                    // Background fill true colour (24-bit RGB); same typing
                    // caveat as 63 — parse the raw value directly.
                    if let Ok(v) = pair.value_string.trim().parse::<i32>() {
                        mtext.background_color = Color::from_true_color_value(v);
                    }
                }
                441 => {
                    if let Some(t) = pair.as_i32() {
                        mtext.background_transparency = t;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                // R2018+ column/layout companion — its codes shadow the
                // entity's own, so it must not fall through this match. The
                // embedded object carries the MTEXT column layout.
                101 => {
                    mtext.column_data = self.read_mtext_embedded_object()?;
                    reading_columns = false;
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut mtext.common)?;
                }
            }
        }

        if let Some(pt) = insertion.get_point() {
            mtext.insertion_point = pt;
        }
        if let Some(n) = normal.get_point() {
            mtext.normal = n;
        }
        // When an explicit X-axis direction is given it defines the rotation
        // (DXF prefers 11/21/31 over the rotation angle in code 50).
        if let Some(xd) = x_direction.get_point() {
            if xd.x != 0.0 || xd.y != 0.0 {
                mtext.rotation = xd.y.atan2(xd.x);
            }
        }

        Ok(Some(mtext))
    }

    /// Read a SPLINE entity
    fn read_spline(&mut self) -> Result<Option<Spline>> {
        let mut spline = Spline::new();
        let mut normal = PointReader::new();
        let mut current_control_point = PointReader::new();
        let mut current_fit_point = PointReader::new();
        let mut begin_tangent = PointReader::new();
        let mut end_tangent = PointReader::new();
        let mut reading_control = false;
        let mut reading_fit = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => spline.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        spline.common.color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        spline.common.line_weight = LineWeight::from_value(lw);
                    }
                }
                70 => {
                    if let Some(flags_val) = pair.as_i16() {
                        spline.flags.closed = (flags_val & 1) != 0;
                        spline.flags.periodic = (flags_val & 2) != 0;
                        spline.flags.rational = (flags_val & 4) != 0;
                        spline.flags.planar = (flags_val & 8) != 0;
                        spline.flags.linear = (flags_val & 16) != 0;
                    }
                }
                71 => {
                    if let Some(degree) = pair.as_i16() {
                        spline.degree = degree as i32;
                    }
                }
                40 => {
                    if let Some(knot) = pair.as_double() {
                        spline.knots.push(knot);
                    }
                }
                41 => {
                    if let Some(weight) = pair.as_double() {
                        spline.weights.push(weight);
                    }
                }
                42 => {
                    if let Some(t) = pair.as_double() {
                        spline.knot_tolerance = t;
                    }
                }
                43 => {
                    if let Some(t) = pair.as_double() {
                        spline.control_tolerance = t;
                    }
                }
                44 => {
                    if let Some(t) = pair.as_double() {
                        spline.fit_tolerance = t;
                    }
                }
                12 | 22 | 32 => {
                    begin_tangent.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    end_tangent.add_coordinate(&pair);
                }
                10 | 20 | 30 => {
                    // Control point coordinates
                    if pair.code == 10 {
                        // Save previous control point if complete
                        if reading_control {
                            if let Some(pt) = current_control_point.get_point() {
                                spline.control_points.push(pt);
                            }
                        }
                        current_control_point = PointReader::new();
                        reading_control = true;
                    }
                    current_control_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    // Fit point coordinates
                    if pair.code == 11 {
                        // Save previous fit point if complete
                        if reading_fit {
                            if let Some(pt) = current_fit_point.get_point() {
                                spline.fit_points.push(pt);
                            }
                        }
                        current_fit_point = PointReader::new();
                        reading_fit = true;
                    }
                    current_fit_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut spline.common)?;
                }
            }
        }

        // Save last control point if any
        if reading_control {
            if let Some(pt) = current_control_point.get_point() {
                spline.control_points.push(pt);
            }
        }

        // Save last fit point if any
        if reading_fit {
            if let Some(pt) = current_fit_point.get_point() {
                spline.fit_points.push(pt);
            }
        }

        if let Some(n) = normal.get_point() {
            spline.normal = n;
        }
        if let Some(t) = begin_tangent.get_point() {
            spline.begin_tangent = t;
        }
        if let Some(t) = end_tangent.get_point() {
            spline.end_tangent = t;
        }

        Ok(Some(spline))
    }

    /// Read a HELIX entity: AcDbSpline geometry followed by AcDbHelix
    /// parameters. Group codes shared by the two subclasses (10/11/12/40/41/42)
    /// are disambiguated by the active `100` subclass marker.
    fn read_helix(&mut self) -> Result<Option<crate::entities::Helix>> {
        use crate::entities::HelixConstraint;
        let mut helix = crate::entities::Helix::new();
        let mut in_helix = false;

        // Spline accumulators (mirror read_spline).
        let mut normal = PointReader::new();
        let mut current_control_point = PointReader::new();
        let mut current_fit_point = PointReader::new();
        let mut begin_tangent = PointReader::new();
        let mut end_tangent = PointReader::new();
        let mut reading_control = false;
        let mut reading_fit = false;

        // Helix point accumulators.
        let mut axis_base = PointReader::new();
        let mut start_pt = PointReader::new();
        let mut axis_vec = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                100 => in_helix = pair.value_string == "AcDbHelix",
                8 => helix.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(ci) = pair.as_i16() {
                        helix.common.color = Color::from_index(ci);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        helix.common.line_weight = LineWeight::from_value(lw);
                    }
                }

                // ── AcDbHelix parameters ──
                90 if in_helix => {
                    if let Some(v) = pair.as_i32() {
                        helix.major_version = v;
                    }
                }
                91 if in_helix => {
                    if let Some(v) = pair.as_i32() {
                        helix.maintenance_version = v;
                    }
                }
                290 => {
                    if let Some(v) = pair.as_i16() {
                        helix.handedness = v != 0;
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        helix.constraint = HelixConstraint::from_code(v as u8);
                    }
                }
                10 | 20 | 30 if in_helix => {
                    axis_base.add_coordinate(&pair);
                }
                11 | 21 | 31 if in_helix => {
                    start_pt.add_coordinate(&pair);
                }
                12 | 22 | 32 if in_helix => {
                    axis_vec.add_coordinate(&pair);
                }
                40 if in_helix => {
                    if let Some(v) = pair.as_double() {
                        helix.radius = v;
                    }
                }
                41 if in_helix => {
                    if let Some(v) = pair.as_double() {
                        helix.turns = v;
                    }
                }
                42 if in_helix => {
                    if let Some(v) = pair.as_double() {
                        helix.turn_height = v;
                    }
                }

                // ── AcDbSpline geometry ──
                70 => {
                    if let Some(f) = pair.as_i16() {
                        helix.spline.flags.closed = (f & 1) != 0;
                        helix.spline.flags.periodic = (f & 2) != 0;
                        helix.spline.flags.rational = (f & 4) != 0;
                        helix.spline.flags.planar = (f & 8) != 0;
                        helix.spline.flags.linear = (f & 16) != 0;
                    }
                }
                71 => {
                    if let Some(d) = pair.as_i16() {
                        helix.spline.degree = d as i32;
                    }
                }
                40 => {
                    if let Some(k) = pair.as_double() {
                        helix.spline.knots.push(k);
                    }
                }
                41 => {
                    if let Some(w) = pair.as_double() {
                        helix.spline.weights.push(w);
                    }
                }
                42 => {
                    if let Some(t) = pair.as_double() {
                        helix.spline.knot_tolerance = t;
                    }
                }
                43 => {
                    if let Some(t) = pair.as_double() {
                        helix.spline.control_tolerance = t;
                    }
                }
                44 => {
                    if let Some(t) = pair.as_double() {
                        helix.spline.fit_tolerance = t;
                    }
                }
                12 | 22 | 32 => {
                    begin_tangent.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    end_tangent.add_coordinate(&pair);
                }
                10 | 20 | 30 => {
                    if pair.code == 10 {
                        if reading_control {
                            if let Some(pt) = current_control_point.get_point() {
                                helix.spline.control_points.push(pt);
                            }
                        }
                        current_control_point = PointReader::new();
                        reading_control = true;
                    }
                    current_control_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    if pair.code == 11 {
                        if reading_fit {
                            if let Some(pt) = current_fit_point.get_point() {
                                helix.spline.fit_points.push(pt);
                            }
                        }
                        current_fit_point = PointReader::new();
                        reading_fit = true;
                    }
                    current_fit_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut helix.common)?;
                }
            }
        }

        if reading_control {
            if let Some(pt) = current_control_point.get_point() {
                helix.spline.control_points.push(pt);
            }
        }
        if reading_fit {
            if let Some(pt) = current_fit_point.get_point() {
                helix.spline.fit_points.push(pt);
            }
        }
        if let Some(n) = normal.get_point() {
            helix.spline.normal = n;
        }
        if let Some(t) = begin_tangent.get_point() {
            helix.spline.begin_tangent = t;
        }
        if let Some(t) = end_tangent.get_point() {
            helix.spline.end_tangent = t;
        }
        if let Some(p) = axis_base.get_point() {
            helix.axis_base_point = p;
        }
        if let Some(p) = start_pt.get_point() {
            helix.start_point = p;
        }
        if let Some(p) = axis_vec.get_point() {
            helix.axis_vector = p;
        }

        Ok(Some(helix))
    }

    fn read_extended_entity(&mut self, entity_name: &str) -> Result<Option<ExtendedEntity>> {
        match entity_name.to_ascii_uppercase().as_str() {
            "CAMERA" => self.read_camera_entity(),
            "SECTIONOBJECT" => self.read_section_object_entity(),
            "ARCALIGNEDTEXT" => self.read_arc_aligned_text_entity(),
            "RTEXT" => self.read_remote_text_entity(),
            "POSITIONMARKER" | "GEOPOSITIONMARKER" => self.read_geo_position_marker_entity(),
            "COORDINATION_MODEL" | "NAVISWORKSMODEL" => self.read_coordination_model_entity(),
            "ACDBPOINTCLOUD" | "POINTCLOUD" => self.read_point_cloud_entity(),
            "ACDBPOINTCLOUDEX" | "POINTCLOUDEX" => self.read_point_cloud_ex_entity(),
            "ACAD_PROXY_ENTITY" => self.read_proxy_entity_dxf(),
            "OLEFRAME" => self.read_ole_frame_entity(),
            "LAYOUTPRINTCONFIG" => self.read_layout_print_config_entity(),
            "FORMAT" => self.read_format_entity(),
            "REPEAT" | "ENDREP" | "LOAD" | "JUMP" => self.read_legacy_entity(entity_name),
            "BLOCKANGULARCONSTRAINTPARAMETERENTITY" => {
                self.read_block_angular_constraint_parameter_entity()
            }
            name => {
                if dynamic_entity_cpp_name(name).is_some() {
                    self.read_empty_dynamic_entity(name)
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn read_block_angular_constraint_parameter_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        use crate::objects::{
            BlockAngularConstraintParameterEntity, BlockConnection, BlockEvalValue,
            DynamicBlockData,
        };
        let mut common = EntityCommon::new();
        let mut value = BlockAngularConstraintParameterEntity::default();
        let mut subclass = String::new();
        let mut eval_point = [0.0; 2];
        let mut definition_base = PointReader::new();
        let mut definition_end = PointReader::new();
        let mut center = PointReader::new();
        let mut label = PointReader::new();
        let mut property_state_index = 0usize;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string.clone();
                continue;
            }
            match subclass.as_str() {
                "AcDbEvalExpr" => match pair.code {
                    90 if value.constraint.parameter.parameter.element.eval.value_code == 90 => {
                        value.constraint.parameter.parameter.element.eval.value =
                            BlockEvalValue::Long(pair.as_i32().unwrap_or(0))
                    }
                    90 => {
                        value.constraint.parameter.parameter.element.eval.node_id =
                            pair.as_i32().unwrap_or(0)
                    }
                    98 => {
                        value.constraint.parameter.parameter.element.eval.major =
                            pair.as_i32().unwrap_or(0)
                    }
                    99 => {
                        value.constraint.parameter.parameter.element.eval.minor =
                            pair.as_i32().unwrap_or(0)
                    }
                    70 => {
                        value.constraint.parameter.parameter.element.eval.value_code =
                            pair.as_i16().unwrap_or(0)
                    }
                    40 => {
                        value.constraint.parameter.parameter.element.eval.value =
                            BlockEvalValue::Real(pair.as_double().unwrap_or(0.0))
                    }
                    10 => eval_point[0] = pair.as_double().unwrap_or(0.0),
                    20 => {
                        eval_point[1] = pair.as_double().unwrap_or(0.0);
                        value.constraint.parameter.parameter.element.eval.value =
                            BlockEvalValue::Point(eval_point);
                    }
                    1 => {
                        value.constraint.parameter.parameter.element.eval.value =
                            BlockEvalValue::Text(pair.value_string.clone())
                    }
                    91 => {
                        value.constraint.parameter.parameter.element.eval.value =
                            BlockEvalValue::Handle(parse_dxf_handle(&pair.value_string))
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                },
                "AcDbBlockElement" => match pair.code {
                    300 => {
                        value.constraint.parameter.parameter.element.name =
                            pair.value_string.clone()
                    }
                    98 => {
                        value.constraint.parameter.parameter.element.major =
                            pair.as_i32().unwrap_or(0)
                    }
                    99 => {
                        value.constraint.parameter.parameter.element.minor =
                            pair.as_i32().unwrap_or(0)
                    }
                    1071 => {
                        value.constraint.parameter.parameter.element.eed_1071 =
                            pair.as_i32().unwrap_or(0)
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                },
                "AcDbBlockParameter" => match pair.code {
                    280 => {
                        value.constraint.parameter.parameter.show_properties =
                            pair.as_i16().unwrap_or(0) != 0
                    }
                    281 => {
                        value.constraint.parameter.parameter.chain_actions =
                            pair.as_i16().unwrap_or(0) != 0
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                },
                "AcDbBlock2PtParameter" => match pair.code {
                    1010 | 1020 | 1030 => {
                        definition_base.add_coordinate(&pair);
                    }
                    1011 | 1021 | 1031 => {
                        definition_end.add_coordinate(&pair);
                    }
                    91 => {
                        if property_state_index < value.constraint.parameter.property_states.len() {
                            value.constraint.parameter.property_states[property_state_index] =
                                pair.as_i32().unwrap_or(0);
                            property_state_index += 1;
                        }
                    }
                    92..=95 => {
                        let index = (pair.code - 92) as usize;
                        value.constraint.parameter.properties[index]
                            .connections
                            .push(BlockConnection {
                                code: pair.as_i32().unwrap_or(0),
                                name: String::new(),
                            });
                    }
                    301..=304 => {
                        let index = (pair.code - 301) as usize;
                        if let Some(connection) = value.constraint.parameter.properties[index]
                            .connections
                            .last_mut()
                        {
                            connection.name = pair.value_string.clone();
                        }
                    }
                    177 => {
                        value.constraint.parameter.parameter_base_location =
                            pair.as_i16().unwrap_or(0)
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                },
                "AcDbBlockConstraintParameter" => {
                    if pair.code == 330 {
                        value.constraint.dependency = parse_dxf_handle(&pair.value_string);
                    } else {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                }
                "AcDbBlockAngularConstraintParameterEntity" => match pair.code {
                    1011 | 1021 | 1031 => {
                        center.add_coordinate(&pair);
                    }
                    1012 | 1022 | 1032 => {
                        label.add_coordinate(&pair);
                    }
                    305 => value.expression_name = pair.value_string.clone(),
                    306 => value.expression_description = pair.value_string.clone(),
                    140 => value.angle = pair.as_double().unwrap_or(0.0),
                    280 => value.orientation_on_both_grips = pair.as_i16().unwrap_or(0) != 0,
                    307 => value.value_set.description = pair.value_string.clone(),
                    96 => value.value_set.flags = pair.as_i32().unwrap_or(0),
                    128 => value.value_set.minimum = pair.as_double().unwrap_or(0.0),
                    129 => value.value_set.maximum = pair.as_double().unwrap_or(0.0),
                    130 => value.value_set.increment = pair.as_double().unwrap_or(0.0),
                    131 => value.value_set.values.push(pair.as_double().unwrap_or(0.0)),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut common)?;
                    }
                },
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }
        value.constraint.parameter.definition_base_point =
            definition_base.get_point().unwrap_or(Vector3::ZERO);
        value.constraint.parameter.definition_end_point =
            definition_end.get_point().unwrap_or(Vector3::ZERO);
        value.center_point = center.get_point().unwrap_or(Vector3::ZERO);
        value.label_point = label.get_point().unwrap_or(Vector3::ZERO);
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::DynamicBlock(
                DynamicBlockData::AngularConstraintParameterEntity(value),
            ),
        }))
    }

    fn read_empty_dynamic_entity(&mut self, dxf_name: &str) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code != 100 {
                self.read_extended_common(&pair, &mut common)?;
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::DynamicBlock(
                DynamicBlockData::empty_entity_from_dxf_name(dxf_name).unwrap_or_default(),
            ),
        }))
    }

    fn read_layout_print_config_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut value = LayoutPrintConfigData::default();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 93 {
                value.flag = pair.as_i16().unwrap_or(0);
            } else if pair.code != 100 {
                self.read_extended_common(&pair, &mut common)?;
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::LayoutPrintConfig(value),
        }))
    }

    fn read_format_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut raw_dxf_codes = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            raw_dxf_codes.push((pair.code, pair.value_string.clone()));
            if pair.code != 100 {
                self.read_extended_common(&pair, &mut common)?;
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::Format(FormatData {
                raw_dxf_codes: Some(raw_dxf_codes),
                ..FormatData::default()
            }),
        }))
    }

    fn read_legacy_entity(&mut self, entity_name: &str) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut columns = 0;
        let mut rows = 0;
        let mut column_spacing = 0.0;
        let mut row_spacing = 0.0;
        let mut filename = String::new();
        let mut address = 0u32;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                70 => columns = pair.as_i16().unwrap_or(0),
                71 => rows = pair.as_i16().unwrap_or(0),
                40 => column_spacing = pair.as_double().unwrap_or(0.0),
                41 => row_spacing = pair.as_double().unwrap_or(0.0),
                1 => filename = pair.value_string.clone(),
                90 => address = pair.as_i32().unwrap_or(0).max(0) as u32,
                100 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        let data = match entity_name.to_ascii_uppercase().as_str() {
            "REPEAT" => LegacyEntityData::Repeat,
            "ENDREP" => LegacyEntityData::EndRepeat {
                columns,
                rows,
                column_spacing,
                row_spacing,
            },
            "LOAD" => LegacyEntityData::Load { filename },
            "JUMP" => LegacyEntityData::Jump { address },
            _ => return Ok(None),
        };
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::Legacy(data),
        }))
    }

    fn read_extended_common(
        &mut self,
        pair: &super::stream_reader::DxfCodePair,
        common: &mut EntityCommon,
    ) -> Result<()> {
        match pair.code {
            8 => common.layer = pair.value_string.clone(),
            62 => {
                if let Some(value) = pair.as_i16() {
                    common.color = Color::from_index(value);
                }
            }
            370 => {
                if let Some(value) = pair.as_i16() {
                    common.line_weight = LineWeight::from_value(value);
                }
            }
            _ => {
                self.try_read_common_entity_code(pair, common)?;
            }
        }
        Ok(())
    }

    fn read_camera_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut view_handle = Handle::NULL;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 340 {
                view_handle = parse_dxf_handle(&pair.value_string);
            } else {
                self.read_extended_common(&pair, &mut common)?;
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::Camera { view_handle },
        }))
    }

    fn read_section_object_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut current_subclass = String::new();
        let mut state = 0;
        let mut flags = 0;
        let mut name = String::new();
        let mut vertical_direction = PointReader::new();
        let mut top_height = 0.0;
        let mut bottom_height = 0.0;
        let mut indicator_alpha = 0;
        let mut indicator_color = Color::ByLayer;
        let mut vertices = Vec::new();
        let mut back_line_vertices = Vec::new();
        let mut vertex = PointReader::new();
        let mut back_vertex = PointReader::new();
        let mut settings_handle = Handle::NULL;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                if let Some(point) = vertex.get_point() {
                    vertices.push(point);
                }
                if let Some(point) = back_vertex.get_point() {
                    back_line_vertices.push(point);
                }
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => current_subclass = pair.value_string.clone(),
                90 if current_subclass == "AcDbSection" => state = pair.as_i32().unwrap_or(0),
                91 if current_subclass == "AcDbSection" => flags = pair.as_i32().unwrap_or(0),
                1 if current_subclass == "AcDbSection" => name = pair.value_string.clone(),
                10 | 20 | 30 if current_subclass == "AcDbSection" => {
                    vertical_direction.add_coordinate(&pair);
                }
                40 if current_subclass == "AcDbSection" => {
                    top_height = pair.as_double().unwrap_or(0.0)
                }
                41 if current_subclass == "AcDbSection" => {
                    bottom_height = pair.as_double().unwrap_or(0.0)
                }
                70 if current_subclass == "AcDbSection" => {
                    indicator_alpha = pair.as_i16().unwrap_or(0)
                }
                62 if current_subclass == "AcDbSection" => {
                    indicator_color = Color::from_index(pair.as_i16().unwrap_or(256))
                }
                11 => {
                    if pair.code == 11 && vertex.get_point().is_some() {
                        vertices.push(vertex.get_point().unwrap_or(Vector3::ZERO));
                        vertex = PointReader::new();
                    }
                    vertex.add_coordinate(&pair);
                }
                21 | 31 => {
                    vertex.add_coordinate(&pair);
                    if pair.code == 31 {
                        vertices.push(vertex.get_point().unwrap_or(Vector3::ZERO));
                        vertex = PointReader::new();
                    }
                }
                12 => {
                    if back_vertex.get_point().is_some() {
                        back_line_vertices.push(back_vertex.get_point().unwrap_or(Vector3::ZERO));
                        back_vertex = PointReader::new();
                    }
                    back_vertex.add_coordinate(&pair);
                }
                22 | 32 => {
                    back_vertex.add_coordinate(&pair);
                    if pair.code == 32 {
                        back_line_vertices.push(back_vertex.get_point().unwrap_or(Vector3::ZERO));
                        back_vertex = PointReader::new();
                    }
                }
                360 if current_subclass == "AcDbSection" => {
                    settings_handle = parse_dxf_handle(&pair.value_string)
                }
                92 | 93 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::SectionObject(SectionObjectData {
                state,
                flags,
                name,
                vertical_direction: vertical_direction.get_point().unwrap_or(Vector3::UNIT_Z),
                top_height,
                bottom_height,
                indicator_alpha,
                indicator_color,
                vertices,
                back_line_vertices,
                settings_handle,
            }),
        }))
    }

    fn read_arc_aligned_text_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut data = ArcAlignedTextData {
            text: String::new(),
            font_name: String::new(),
            big_font_name: String::new(),
            style_name: String::new(),
            center: Vector3::ZERO,
            radius: 0.0,
            x_scale: 1.0,
            text_size: 0.0,
            character_spacing: 0.0,
            offset_from_arc: 0.0,
            right_offset: 0.0,
            left_offset: 0.0,
            start_angle: 0.0,
            end_angle: 0.0,
            reverse: false,
            text_direction: 0,
            alignment: 0,
            text_position: 0,
            bold: false,
            italic: false,
            underlined: false,
            character_set: 0,
            pitch_and_family: 0,
            is_shx: false,
            text_color: 0,
            normal: Vector3::UNIT_Z,
            wizard_flag: false,
            arc_handle: Handle::NULL,
        };
        let mut center = PointReader::new();
        let mut normal = PointReader::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                1 => data.text = pair.value_string.clone(),
                2 => data.font_name = pair.value_string.clone(),
                3 => data.big_font_name = pair.value_string.clone(),
                7 => data.style_name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                40 => data.radius = pair.as_double().unwrap_or(0.0),
                41 => data.x_scale = pair.as_double().unwrap_or(1.0),
                42 => data.text_size = pair.as_double().unwrap_or(0.0),
                43 => data.character_spacing = pair.as_double().unwrap_or(0.0),
                44 => data.offset_from_arc = pair.as_double().unwrap_or(0.0),
                45 => data.right_offset = pair.as_double().unwrap_or(0.0),
                46 => data.left_offset = pair.as_double().unwrap_or(0.0),
                50 => data.start_angle = pair.as_double().unwrap_or(0.0),
                51 => data.end_angle = pair.as_double().unwrap_or(0.0),
                70 => data.reverse = pair.as_i16().unwrap_or(0) != 0,
                71 => data.text_direction = pair.as_i16().unwrap_or(0),
                72 => data.alignment = pair.as_i16().unwrap_or(0),
                73 => data.text_position = pair.as_i16().unwrap_or(0),
                74 => data.bold = pair.as_i16().unwrap_or(0) != 0,
                75 => data.italic = pair.as_i16().unwrap_or(0) != 0,
                76 => data.underlined = pair.as_i16().unwrap_or(0) != 0,
                77 => data.character_set = pair.as_i16().unwrap_or(0),
                78 => data.pitch_and_family = pair.as_i16().unwrap_or(0),
                79 => data.is_shx = pair.as_i16().unwrap_or(0) != 0,
                90 => data.text_color = pair.as_i32().unwrap_or(0),
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                280 => data.wizard_flag = pair.as_i16().unwrap_or(0) != 0,
                330 => data.arc_handle = parse_dxf_handle(&pair.value_string),
                100 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        data.center = center.get_point().unwrap_or(Vector3::ZERO);
        data.normal = normal.get_point().unwrap_or(Vector3::UNIT_Z);
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::ArcAlignedText(data),
        }))
    }

    fn read_remote_text_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut position = PointReader::new();
        let mut normal = PointReader::new();
        let mut rotation = 0.0;
        let mut height = 0.0;
        let mut style_name = String::new();
        let mut flags = 0;
        let mut text = String::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                10 | 20 | 30 => {
                    position.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                50 => rotation = pair.as_double().unwrap_or(0.0),
                40 => height = pair.as_double().unwrap_or(0.0),
                7 => style_name = pair.value_string.clone(),
                70 => flags = pair.as_i16().unwrap_or(0),
                1 => text = pair.value_string.clone(),
                100 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::RemoteText(RemoteTextData {
                position: position.get_point().unwrap_or(Vector3::ZERO),
                normal: normal.get_point().unwrap_or(Vector3::UNIT_Z),
                rotation,
                height,
                style_handle: Handle::NULL,
                style_name,
                flags,
                text,
            }),
        }))
    }

    fn read_geo_position_marker_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut current_subclass = String::new();
        let mut class_version = 0;
        let mut position = PointReader::new();
        let mut radius = 0.0;
        let mut notes = String::new();
        let mut landing_gap = 0.0;
        let mut mtext_visible = false;
        let mut text_alignment = 0;
        let mut enable_frame_text = false;
        let mut double_40_index = 0;
        let mut bool_290_index = 0;
        let mut embedded = MText::new();
        let mut embedded_position = PointReader::new();
        let mut embedded_normal = PointReader::new();
        let mut has_embedded = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => {
                    current_subclass = pair.value_string.clone();
                    if current_subclass == "AcDbMTextObjectEmbedded" {
                        has_embedded = true;
                    }
                }
                90 if current_subclass == "AcDbGeoPositionMarker" => {
                    class_version = pair.as_i32().unwrap_or(0)
                }
                10 | 20 | 30 if current_subclass == "AcDbGeoPositionMarker" => {
                    position.add_coordinate(&pair);
                }
                40 if current_subclass == "AcDbGeoPositionMarker" => {
                    if double_40_index == 0 {
                        radius = pair.as_double().unwrap_or(0.0);
                    } else {
                        landing_gap = pair.as_double().unwrap_or(0.0);
                    }
                    double_40_index += 1;
                }
                1 if current_subclass == "AcDbGeoPositionMarker" => {
                    notes = pair.value_string.clone()
                }
                290 if current_subclass == "AcDbGeoPositionMarker" => {
                    if bool_290_index == 0 {
                        mtext_visible = pair.as_i16().unwrap_or(0) != 0;
                    } else {
                        enable_frame_text = pair.as_i16().unwrap_or(0) != 0;
                    }
                    bool_290_index += 1;
                }
                280 if current_subclass == "AcDbGeoPositionMarker" => {
                    text_alignment = pair.as_i16().unwrap_or(0) as u8
                }
                10 | 20 | 30 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded_position.add_coordinate(&pair);
                }
                210 | 220 | 230 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded_normal.add_coordinate(&pair);
                }
                40 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded.height = pair.as_double().unwrap_or(0.0)
                }
                41 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded.rectangle_width = pair.as_double().unwrap_or(0.0)
                }
                1 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded.value = pair.value_string.clone()
                }
                7 if current_subclass == "AcDbMTextObjectEmbedded" => {
                    embedded.style = pair.value_string.clone()
                }
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        if has_embedded {
            embedded.insertion_point = embedded_position
                .get_point()
                .unwrap_or(position.get_point().unwrap_or(Vector3::ZERO));
            embedded.normal = embedded_normal.get_point().unwrap_or(Vector3::UNIT_Z);
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::GeoPositionMarker(GeoPositionMarkerData {
                class_version,
                position: position.get_point().unwrap_or(Vector3::ZERO),
                radius,
                notes,
                landing_gap,
                mtext_visible,
                text_alignment,
                enable_frame_text,
                embedded_mtext: if has_embedded { Some(embedded) } else { None },
            }),
        }))
    }

    fn read_coordination_model_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut flags = 0;
        let mut definition_handle = Handle::NULL;
        let mut values = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                70 => flags = pair.as_i16().unwrap_or(0),
                340 => definition_handle = parse_dxf_handle(&pair.value_string),
                40 => values.push(pair.as_double().unwrap_or(0.0)),
                100 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        let mut transform = [0.0; 16];
        for (target, value) in transform.iter_mut().zip(values.iter().copied()) {
            *target = value;
        }
        let unit_factor = values.get(16).copied().unwrap_or(1.0);
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::CoordinationModel(CoordinationModelData {
                flags,
                definition_handle,
                transform,
                unit_factor,
            }),
        }))
    }

    fn read_ole_frame_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut flag = 0;
        let mut mode = 0;
        let mut data = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                70 => flag = pair.as_i16().unwrap_or(0),
                71 => mode = pair.as_i16().unwrap_or(0),
                90 | 100 => {}
                310..=319 => append_hex_bytes(&mut data, &pair.value_string),
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::OleFrame(OleFrameData {
                flag,
                mode,
                storage: crate::compound_file::StructuredStoragePayload::decode(&data),
            }),
        }))
    }

    fn read_point_cloud_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut current_subclass = String::new();
        let mut class_version = 0;
        let mut origin = PointReader::new();
        let mut saved_filename = String::new();
        let mut source_files = Vec::new();
        let mut extents_min = PointReader::new();
        let mut extents_max = PointReader::new();
        let mut point_count = 0;
        let mut ucs_name = String::new();
        let mut ucs_origin = PointReader::new();
        let mut ucs_x_direction = PointReader::new();
        let mut ucs_y_direction = PointReader::new();
        let mut ucs_z_direction = PointReader::new();
        let mut definition_handle = Handle::NULL;
        let mut reactor_handle = Handle::NULL;
        let mut show_intensity = false;
        let mut intensity_scheme = 0;
        let mut minimum_intensity = 0.0;
        let mut maximum_intensity = 0.0;
        let mut low_intensity_threshold = 0.0;
        let mut high_intensity_threshold = 0.0;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => current_subclass = pair.value_string.clone(),
                70 if current_subclass == "AcDbPointCloud" => {
                    class_version = pair.as_i16().unwrap_or(0)
                }
                10 | 20 | 30 if current_subclass == "AcDbPointCloud" => {
                    origin.add_coordinate(&pair);
                }
                1 if current_subclass == "AcDbPointCloud" => {
                    saved_filename = pair.value_string.clone()
                }
                2 if current_subclass == "AcDbPointCloud" => {
                    source_files.push(pair.value_string.clone())
                }
                3 if current_subclass == "AcDbPointCloud" => ucs_name = pair.value_string.clone(),
                11 | 21 | 31 => {
                    extents_min.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    extents_max.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    ucs_origin.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    ucs_x_direction.add_coordinate(&pair);
                }
                211 | 221 | 231 => {
                    ucs_y_direction.add_coordinate(&pair);
                }
                212 | 222 | 232 => {
                    ucs_z_direction.add_coordinate(&pair);
                }
                92 => point_count = pair.as_int().unwrap_or(0),
                330 if current_subclass == "AcDbPointCloud" => {
                    definition_handle = parse_dxf_handle(&pair.value_string)
                }
                360 if current_subclass == "AcDbPointCloud" => {
                    reactor_handle = parse_dxf_handle(&pair.value_string)
                }
                290 => show_intensity = pair.as_i16().unwrap_or(0) != 0,
                71 => intensity_scheme = pair.as_i16().unwrap_or(0),
                40 => minimum_intensity = pair.as_double().unwrap_or(0.0),
                41 => maximum_intensity = pair.as_double().unwrap_or(0.0),
                42 => low_intensity_threshold = pair.as_double().unwrap_or(0.0),
                43 => high_intensity_threshold = pair.as_double().unwrap_or(0.0),
                90 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::PointCloud(PointCloudData {
                class_version,
                origin: origin.get_point().unwrap_or(Vector3::ZERO),
                saved_filename,
                source_files,
                extents_min: extents_min.get_point().unwrap_or(Vector3::ZERO),
                extents_max: extents_max.get_point().unwrap_or(Vector3::ZERO),
                point_count,
                ucs_name,
                ucs_origin: ucs_origin.get_point().unwrap_or(Vector3::ZERO),
                ucs_x_direction: ucs_x_direction.get_point().unwrap_or(Vector3::UNIT_X),
                ucs_y_direction: ucs_y_direction.get_point().unwrap_or(Vector3::UNIT_Y),
                ucs_z_direction: ucs_z_direction.get_point().unwrap_or(Vector3::UNIT_Z),
                definition_handle,
                reactor_handle,
                show_intensity,
                intensity_scheme,
                minimum_intensity,
                maximum_intensity,
                low_intensity_threshold,
                high_intensity_threshold,
                show_clipping: false,
                clippings: Vec::new(),
            }),
        }))
    }

    fn read_point_cloud_ex_entity(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut current_subclass = String::new();
        let mut class_version = 0;
        let mut extents_min = PointReader::new();
        let mut extents_max = PointReader::new();
        let mut ucs_origin = PointReader::new();
        let mut ucs_x_direction = PointReader::new();
        let mut ucs_y_direction = PointReader::new();
        let mut ucs_z_direction = PointReader::new();
        let mut locked = false;
        let mut definition_handle = Handle::NULL;
        let mut reactor_handle = Handle::NULL;
        let mut name = String::new();
        let mut show_intensity = false;
        let mut show_cropping = false;
        let mut stylization_type = 0;
        let mut strings = Vec::new();
        let mut elevation_min = 0.0;
        let mut elevation_max = 0.0;
        let mut intensity_min = 0;
        let mut intensity_max = 0;
        let mut intensity_out_of_range_behavior = 0;
        let mut elevation_out_of_range_behavior = 0;
        let mut elevation_apply_to_fixed_range = false;
        let mut intensity_as_gradient = false;
        let mut elevation_as_gradient = false;
        let mut behavior_71_seen = false;
        let mut croppings = Vec::new();
        let mut crop: Option<PointCloudExCrop> = None;
        let mut crop_plane = PointReader::new();
        let mut crop_x_direction = PointReader::new();
        let mut crop_y_direction = PointReader::new();
        let mut crop_point = PointReader::new();
        let mut reading_crop_points = false;
        let mut crop_bool_index = 0;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                if let Some(mut value) = crop.take() {
                    value.plane = crop_plane.get_point().unwrap_or(Vector3::ZERO);
                    value.x_direction = crop_x_direction.get_point().unwrap_or(Vector3::UNIT_X);
                    value.y_direction = crop_y_direction.get_point().unwrap_or(Vector3::UNIT_Y);
                    croppings.push(value);
                }
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => current_subclass = pair.value_string.clone(),
                70 => class_version = pair.as_i16().unwrap_or(0),
                10 | 20 | 30 => {
                    extents_min.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    extents_max.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    ucs_origin.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    ucs_x_direction.add_coordinate(&pair);
                }
                211 | 221 | 231 => {
                    ucs_y_direction.add_coordinate(&pair);
                }
                212 | 222 | 232 => {
                    ucs_z_direction.add_coordinate(&pair);
                }
                290 if crop.is_none() => locked = pair.as_i16().unwrap_or(0) != 0,
                330 if current_subclass == "AcDbPointCloud" => {
                    definition_handle = parse_dxf_handle(&pair.value_string)
                }
                360 if current_subclass == "AcDbPointCloud" => {
                    reactor_handle = parse_dxf_handle(&pair.value_string)
                }
                1 => {
                    if name.is_empty() {
                        name = pair.value_string.clone();
                    } else {
                        strings.push(pair.value_string.clone());
                    }
                }
                291 => show_intensity = pair.as_i16().unwrap_or(0) != 0,
                71 if !behavior_71_seen => {
                    stylization_type = pair.as_i16().unwrap_or(0);
                    behavior_71_seen = true;
                }
                71 => intensity_out_of_range_behavior = pair.as_i16().unwrap_or(0),
                72 => elevation_out_of_range_behavior = pair.as_i16().unwrap_or(0),
                40 => elevation_min = pair.as_double().unwrap_or(0.0),
                41 => elevation_max = pair.as_double().unwrap_or(0.0),
                90 => intensity_min = pair.as_i32().unwrap_or(0),
                91 => intensity_max = pair.as_i32().unwrap_or(0),
                292 => elevation_apply_to_fixed_range = pair.as_i16().unwrap_or(0) != 0,
                293 => intensity_as_gradient = pair.as_i16().unwrap_or(0) != 0,
                294 => elevation_as_gradient = pair.as_i16().unwrap_or(0) != 0,
                295 => show_cropping = pair.as_i16().unwrap_or(0) != 0,
                280 => {
                    if let Some(mut value) = crop.take() {
                        value.plane = crop_plane.get_point().unwrap_or(Vector3::ZERO);
                        value.x_direction = crop_x_direction.get_point().unwrap_or(Vector3::UNIT_X);
                        value.y_direction = crop_y_direction.get_point().unwrap_or(Vector3::UNIT_Y);
                        croppings.push(value);
                    }
                    crop = Some(PointCloudExCrop {
                        crop_type: pair.as_i16().unwrap_or(0),
                        inside: false,
                        inverted: false,
                        plane: Vector3::ZERO,
                        x_direction: Vector3::UNIT_X,
                        y_direction: Vector3::UNIT_Y,
                        points: Vec::new(),
                    });
                    crop_plane = PointReader::new();
                    crop_x_direction = PointReader::new();
                    crop_y_direction = PointReader::new();
                    crop_point = PointReader::new();
                    reading_crop_points = false;
                    crop_bool_index = 0;
                }
                290 if crop.is_some() => {
                    if let Some(value) = &mut crop {
                        if crop_bool_index == 0 {
                            value.inside = pair.as_i16().unwrap_or(0) != 0;
                        } else {
                            value.inverted = pair.as_i16().unwrap_or(0) != 0;
                        }
                    }
                    crop_bool_index += 1;
                }
                13 | 23 | 33 if crop.is_some() && !reading_crop_points => {
                    crop_plane.add_coordinate(&pair);
                }
                213 | 223 | 233 if crop.is_some() => {
                    if crop_x_direction.get_point().is_none() {
                        crop_x_direction.add_coordinate(&pair);
                    } else {
                        crop_y_direction.add_coordinate(&pair);
                    }
                }
                93 if crop.is_some() => reading_crop_points = true,
                13 | 23 | 33 if crop.is_some() && reading_crop_points => {
                    crop_point.add_coordinate(&pair);
                    if pair.code == 33 {
                        if let Some(value) = &mut crop {
                            value
                                .points
                                .push(crop_point.get_point().unwrap_or(Vector3::ZERO));
                        }
                        crop_point = PointReader::new();
                    }
                }
                92 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::PointCloudEx(PointCloudExData {
                class_version,
                extents_min: extents_min.get_point().unwrap_or(Vector3::ZERO),
                extents_max: extents_max.get_point().unwrap_or(Vector3::ZERO),
                ucs_origin: ucs_origin.get_point().unwrap_or(Vector3::ZERO),
                ucs_x_direction: ucs_x_direction.get_point().unwrap_or(Vector3::UNIT_X),
                ucs_y_direction: ucs_y_direction.get_point().unwrap_or(Vector3::UNIT_Y),
                ucs_z_direction: ucs_z_direction.get_point().unwrap_or(Vector3::UNIT_Z),
                locked,
                definition_handle,
                reactor_handle,
                name,
                show_intensity,
                show_cropping,
                unknown_bl0: 0,
                unknown_bl1: 0,
                stylization_type,
                intensity_color_scheme: strings.first().cloned().unwrap_or_default(),
                current_color_scheme: strings.get(1).cloned().unwrap_or_default(),
                classification_color_scheme: strings.get(2).cloned().unwrap_or_default(),
                elevation_min,
                elevation_max,
                intensity_min,
                intensity_max,
                intensity_out_of_range_behavior,
                elevation_out_of_range_behavior,
                elevation_apply_to_fixed_range,
                intensity_as_gradient,
                elevation_as_gradient,
                croppings,
            }),
        }))
    }

    fn read_proxy_entity_dxf(&mut self) -> Result<Option<ExtendedEntity>> {
        let mut common = EntityCommon::new();
        let mut proxy_id = 498;
        let mut class_id = 0;
        let mut version = 0;
        let mut dwg_version = 0;
        let mut maintenance_version = 0;
        let mut from_dxf = false;
        let mut proxy_data_size = 0usize;
        let mut object_data_bits = 0u32;
        let mut proxy_data = Vec::new();
        let mut object_data = Vec::new();
        let mut object_ids = Vec::new();
        let mut reading_object_data = false;
        let mut subclass = String::new();
        let mut common_graphics_size = 0usize;
        let mut common_graphics = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => subclass = pair.value_string.clone(),
                92 | 160 if subclass == "AcDbEntity" => {
                    common_graphics_size = pair.as_int().unwrap_or(0).max(0) as usize;
                }
                310..=319 if subclass == "AcDbEntity" => {
                    append_hex_bytes(&mut common_graphics, &pair.value_string);
                }
                90 => proxy_id = pair.as_i32().unwrap_or(498),
                91 => class_id = pair.as_i32().unwrap_or(0),
                95 => version = pair.as_i32().unwrap_or(0),
                71 => dwg_version = pair.as_i32().unwrap_or(0),
                97 => maintenance_version = pair.as_i32().unwrap_or(0),
                70 => from_dxf = pair.as_i16().unwrap_or(0) != 0,
                92 => {
                    proxy_data_size = pair.as_i32().unwrap_or(0).max(0) as usize;
                    reading_object_data = false;
                }
                93 => {
                    object_data_bits = pair.as_i32().unwrap_or(0).max(0) as u32;
                    reading_object_data = true;
                }
                310..=319 => {
                    if reading_object_data
                        || (proxy_data_size > 0 && proxy_data.len() >= proxy_data_size)
                    {
                        append_hex_bytes(&mut object_data, &pair.value_string);
                    } else {
                        append_hex_bytes(&mut proxy_data, &pair.value_string);
                    }
                }
                code @ (330 | 340 | 350 | 360) if reading_object_data => {
                    let kind = match code {
                        330 => crate::objects::ProxyReferenceKind::SoftOwnership,
                        340 => crate::objects::ProxyReferenceKind::HardOwnership,
                        350 => crate::objects::ProxyReferenceKind::SoftPointer,
                        360 => crate::objects::ProxyReferenceKind::HardPointer,
                        _ => unreachable!(),
                    };
                    object_ids.push(crate::objects::ProxyObjectReference {
                        handle: parse_dxf_handle(&pair.value_string),
                        kind,
                    });
                }
                94 => {}
                _ => self.read_extended_common(&pair, &mut common)?,
            }
        }
        common_graphics.truncate(common_graphics_size);
        if common_graphics_size != 0 || !common_graphics.is_empty() {
            common.graphic_data = Some(common_graphics);
        }
        proxy_data.truncate(proxy_data_size);
        if dwg_version == 0 && maintenance_version == 0 {
            dwg_version = version & 0xffff;
            maintenance_version = version >> 16;
        } else {
            version = (maintenance_version << 16) | (dwg_version & 0xffff);
        }
        let payload = crate::objects::ProxyPayload::from_bits(&object_data, object_data_bits);
        if let Some(envelope) =
            crate::objects::semantic_property::decode_registered_class_envelope(&payload)
        {
            if !proxy_data.is_empty() {
                common.graphic_data = Some(proxy_data);
            }
            return Ok(Some(ExtendedEntity {
                common,
                data: ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                    dxf_name: envelope.dxf_name,
                    cpp_class_name: envelope.cpp_class_name,
                    properties: envelope.properties,
                    payload: envelope.payload,
                    object_ids,
                }),
            }));
        }
        Ok(Some(ExtendedEntity {
            common,
            data: ExtendedEntityData::Proxy(ProxyEntityData {
                proxy_id,
                class_id,
                dxf_subclass: String::new(),
                version,
                dwg_version,
                maintenance_version,
                from_dxf,
                graphics: crate::objects::ProxyPayload::from_bytes(&proxy_data),
                payload,
                text_payload: crate::objects::ProxyPayload::default(),
                object_ids,
            }),
        }))
    }

    /// Read class-based arc-length and large-radius DIMENSION entities.
    fn read_extended_dimension(&mut self, entity_name: &str) -> Result<Option<Dimension>> {
        use crate::entities::dimension::*;

        let is_arc = entity_name.eq_ignore_ascii_case("ARC_DIMENSION");
        let mut base = DimensionBase::new(if is_arc {
            DimensionType::ArcLength
        } else {
            DimensionType::LargeRadial
        });
        let mut common = EntityCommon::new();
        let mut current_subclass = String::new();
        let mut definition_point = PointReader::new();
        let mut text_middle_point = PointReader::new();
        let mut first = PointReader::new();
        let mut second = PointReader::new();
        let mut third = PointReader::new();
        let mut fourth = PointReader::new();
        let mut fifth = PointReader::new();
        let mut is_partial = false;
        let mut has_leader = false;
        let mut first_value = 0.0;
        let mut second_value = 0.0;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                100 => current_subclass = pair.value_string.clone(),
                8 => common.layer = pair.value_string.clone(),
                1 if current_subclass == "AcDbDimension" => base.text = pair.value_string.clone(),
                2 => base.block_name = pair.value_string.clone(),
                3 => base.style_name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    definition_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    text_middle_point.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    first.add_coordinate(&pair);
                }
                14 | 24 | 34 => {
                    second.add_coordinate(&pair);
                }
                15 | 25 | 35 => {
                    third.add_coordinate(&pair);
                }
                16 | 26 | 36 => {
                    fourth.add_coordinate(&pair);
                }
                17 | 27 | 37 => {
                    fifth.add_coordinate(&pair);
                }
                40 if current_subclass == "AcDbArcDimension"
                    || current_subclass == "AcDbRadialDimensionLarge" =>
                {
                    first_value = pair.as_double().unwrap_or(0.0)
                }
                41 if current_subclass == "AcDbArcDimension" => {
                    second_value = pair.as_double().unwrap_or(0.0)
                }
                42 if current_subclass == "AcDbDimension" => {
                    base.actual_measurement = pair.as_double().unwrap_or(0.0)
                }
                44 if current_subclass == "AcDbDimension" => {
                    base.line_spacing_factor = pair.as_double().unwrap_or(1.0)
                }
                53 if current_subclass == "AcDbDimension" => {
                    base.text_rotation = pair.as_double().unwrap_or(0.0).to_radians()
                }
                70 if current_subclass == "AcDbArcDimension" => {
                    is_partial = pair.as_i16().unwrap_or(0) != 0
                }
                70 if current_subclass == "AcDbDimension" => {
                    let flags = pair.as_i16().unwrap_or(0);
                    base.text_user_positioned = (flags & 0x80) != 0;
                }
                71 if current_subclass == "AcDbArcDimension" => {
                    has_leader = pair.as_i16().unwrap_or(0) != 0
                }
                71 if current_subclass == "AcDbDimension" => {
                    base.attachment_point = match pair.as_i16().unwrap_or(5) {
                        1 => AttachmentPointType::TopLeft,
                        2 => AttachmentPointType::TopCenter,
                        3 => AttachmentPointType::TopRight,
                        4 => AttachmentPointType::MiddleLeft,
                        6 => AttachmentPointType::MiddleRight,
                        7 => AttachmentPointType::BottomLeft,
                        8 => AttachmentPointType::BottomCenter,
                        9 => AttachmentPointType::BottomRight,
                        _ => AttachmentPointType::MiddleCenter,
                    };
                }
                72 if current_subclass == "AcDbDimension" => {
                    base.line_spacing_style = pair.as_i16().unwrap_or(1)
                }
                210 | 220 | 230 => {
                    let mut normal = PointReader::new();
                    normal.add_coordinate(&pair);
                    match pair.code {
                        210 => base.normal.x = pair.as_double().unwrap_or(0.0),
                        220 => base.normal.y = pair.as_double().unwrap_or(0.0),
                        230 => base.normal.z = pair.as_double().unwrap_or(1.0),
                        _ => {}
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        base.common = common;
        if let Some(point) = definition_point.get_point() {
            base.definition_point = point;
        }
        if let Some(point) = text_middle_point.get_point() {
            base.text_middle_point = point;
        }

        if is_arc {
            let mut dim = DimensionArc::default();
            dim.base = base;
            dim.definition_point = dim.base.definition_point;
            dim.first_extension_point = first.get_point().unwrap_or(Vector3::ZERO);
            dim.second_extension_point = second.get_point().unwrap_or(Vector3::ZERO);
            dim.center_point = third.get_point().unwrap_or(Vector3::ZERO);
            dim.is_partial = is_partial;
            dim.arc_start_parameter = first_value;
            dim.arc_end_parameter = second_value;
            dim.has_leader = has_leader;
            dim.first_leader_point = fourth.get_point().unwrap_or(Vector3::ZERO);
            dim.second_leader_point = fifth.get_point().unwrap_or(Vector3::ZERO);
            Ok(Some(Dimension::Arc(dim)))
        } else {
            let mut dim = DimensionLargeRadial::default();
            dim.base = base;
            dim.definition_point = dim.base.definition_point;
            dim.jog_point = first.get_point().unwrap_or(Vector3::ZERO);
            dim.override_center = second.get_point().unwrap_or(Vector3::ZERO);
            dim.chord_point = third.get_point().unwrap_or(Vector3::ZERO);
            dim.jog_angle = first_value;
            Ok(Some(Dimension::LargeRadial(dim)))
        }
    }

    /// Read a DIMENSION entity
    fn read_dimension(&mut self) -> Result<Option<Dimension>> {
        use crate::entities::dimension::*;

        let mut dim_type = DimensionType::Linear;
        let mut text_user_positioned = false;
        let mut definition_point = PointReader::new();
        let mut text_middle_point = PointReader::new();
        let mut insertion_point = PointReader::new();
        let mut first_point = PointReader::new();
        let mut second_point = PointReader::new();
        let mut third_point = PointReader::new();
        let mut fourth_point = PointReader::new();
        let mut text = String::new();
        let mut style_name = String::from("Standard");
        // Name of the anonymous block that holds the baked dimension picture
        // (DXF group code 2). Without it the dimension has no geometry block to
        // render from and consumers must recompute the picture, which drifts
        // from the authored one.
        let mut block_name = String::new();
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut line_weight = LineWeight::ByLayer;
        let mut rotation = 0.0;
        let mut text_rotation = 0.0f64;
        let mut horizontal_direction = 0.0f64;
        let mut ext_line_rotation = 0.0f64;
        let mut ordinate_is_x = true;
        // Missing group 42 preserves the computed measurement.
        let mut actual_measurement = None;
        let mut leader_length = 0.0;
        let mut line_spacing_factor = 1.0f64;
        let mut line_spacing_style = 1i16;
        let mut attachment_point = AttachmentPointType::MiddleCenter;
        let mut normal = PointReader::new();
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line_weight = LineWeight::from_value(lw);
                    }
                }
                70 => {
                    if let Some(type_val) = pair.as_i16() {
                        dim_type = match type_val & 0x0F {
                            0 => DimensionType::Linear,
                            1 => DimensionType::Aligned,
                            2 => DimensionType::Angular,
                            3 => DimensionType::Diameter,
                            4 => DimensionType::Radius,
                            5 => DimensionType::Angular3Point,
                            6 => DimensionType::Ordinate,
                            _ => DimensionType::Linear,
                        };
                        // Bit 0x80: text was positioned at a user-defined
                        // location rather than the style default.
                        text_user_positioned = (type_val & 0x80) != 0;
                        // Bit 0x40: ordinate dimension measures the X datum
                        // (cleared = Y). Independent of the 0x80 text bit.
                        ordinate_is_x = (type_val & 0x40) != 0;
                    }
                }
                1 => text = pair.value_string.clone(),
                2 => block_name = pair.value_string.clone(),
                3 => style_name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    definition_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    text_middle_point.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    insertion_point.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    first_point.add_coordinate(&pair);
                }
                14 | 24 | 34 => {
                    second_point.add_coordinate(&pair);
                }
                15 | 25 | 35 => {
                    third_point.add_coordinate(&pair);
                }
                16 | 26 | 36 => {
                    fourth_point.add_coordinate(&pair);
                }
                50 => {
                    // DXF stores dimension-line rotation in degrees; internal
                    // representation is radians.
                    if let Some(rot) = pair.as_double() {
                        rotation = rot.to_radians();
                    }
                }
                52 => {
                    // Extension-line (oblique) angle, degrees -> radians.
                    if let Some(v) = pair.as_double() {
                        ext_line_rotation = v.to_radians();
                    }
                }
                53 => {
                    // Text rotation, degrees -> radians.
                    if let Some(v) = pair.as_double() {
                        text_rotation = v.to_radians();
                    }
                }
                51 => {
                    if let Some(v) = pair.as_double() {
                        horizontal_direction = v.to_radians();
                    }
                }
                41 => {
                    if let Some(lsf) = pair.as_double() {
                        line_spacing_factor = lsf;
                    }
                }
                71 => {
                    attachment_point = match pair.as_i16().unwrap_or(5) {
                        1 => AttachmentPointType::TopLeft,
                        2 => AttachmentPointType::TopCenter,
                        3 => AttachmentPointType::TopRight,
                        4 => AttachmentPointType::MiddleLeft,
                        6 => AttachmentPointType::MiddleRight,
                        7 => AttachmentPointType::BottomLeft,
                        8 => AttachmentPointType::BottomCenter,
                        9 => AttachmentPointType::BottomRight,
                        _ => AttachmentPointType::MiddleCenter,
                    };
                }
                72 => line_spacing_style = pair.as_i16().unwrap_or(1),
                42 => {
                    if let Some(measurement) = pair.as_double() {
                        actual_measurement = Some(measurement);
                    }
                }
                40 => {
                    if let Some(length) = pair.as_double() {
                        leader_length = length;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        // Build the appropriate dimension type
        // True color from code 420 (stored in common by try_read_common_entity_code) overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        let pt1 = first_point.get_point().unwrap_or(Vector3::zero());
        let pt2 = second_point.get_point().unwrap_or(Vector3::zero());
        let pt3 = third_point.get_point().unwrap_or(Vector3::zero());
        let _pt4 = fourth_point.get_point().unwrap_or(Vector3::zero());

        let mut dimension = match dim_type {
            DimensionType::Aligned => {
                let mut dim = DimensionAligned::new(pt1, pt2);
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                dim.ext_line_rotation = ext_line_rotation;
                if let Some(def_pt) = definition_point.get_point() {
                    dim.definition_point = def_pt;
                }
                Dimension::Aligned(dim)
            }
            DimensionType::Linear => {
                let mut dim = DimensionLinear::rotated(pt1, pt2, rotation);
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                dim.ext_line_rotation = ext_line_rotation;
                if let Some(def_pt) = definition_point.get_point() {
                    dim.definition_point = def_pt;
                }
                Dimension::Linear(dim)
            }
            DimensionType::Radius => {
                // Group 10 is the centre; group 15 is the chord point.
                let center = definition_point.get_point().unwrap_or(Vector3::zero());
                let chord_point = pt3;
                let mut dim = DimensionRadius::new(center, chord_point);
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                dim.leader_length = leader_length;
                Dimension::Radius(dim)
            }
            DimensionType::Diameter => {
                // Group 15 is the first chord; group 10 is its opposite.
                let chord_point = pt3;
                let far_chord_point = definition_point.get_point().unwrap_or(Vector3::zero());
                let mut dim = DimensionDiameter::new(chord_point, far_chord_point);
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                Dimension::Diameter(dim)
            }
            DimensionType::Angular => {
                // 13-14 and 10-15 define the lines; 16 locates the arc.
                let mut dim = DimensionAngular2Ln::default();
                dim.first_point = pt1;
                dim.second_point = pt2;
                dim.angle_vertex = pt3;
                dim.dimension_arc = _pt4;
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                Dimension::Angular2Ln(dim)
            }
            DimensionType::Angular3Point => {
                // 13=first, 14=second, 15=angle_vertex; the arc location is the
                // base definition point (code 10).
                let mut dim = DimensionAngular3Pt::default();
                dim.first_point = pt1;
                dim.second_point = pt2;
                dim.angle_vertex = pt3;
                if let Some(arc) = definition_point.get_point() {
                    dim.definition_point = arc;
                }
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                Dimension::Angular3Pt(dim)
            }
            DimensionType::Ordinate => {
                // 13=feature_location, 14=leader_endpoint; X vs Y datum from the
                // group-70 0x40 bit; group 10 is the datum origin.
                let mut dim = DimensionOrdinate::new(pt1, pt2, ordinate_is_x);
                if let Some(origin) = definition_point.get_point() {
                    dim.definition_point = origin;
                }
                dim.base.common.layer = layer;
                dim.base.common.color = color;
                dim.base.common.line_weight = line_weight;
                dim.base.text = text;
                dim.base.style_name = style_name;
                if let Some(measurement) = actual_measurement {
                    dim.base.actual_measurement = measurement;
                }
                Dimension::Ordinate(dim)
            }
            DimensionType::ArcLength => Dimension::Arc(DimensionArc::default()),
            DimensionType::LargeRadial => Dimension::LargeRadial(DimensionLargeRadial::default()),
        };

        {
            let dc = dimension.base_mut();
            common.layer = std::mem::take(&mut dc.common.layer);
            common.color = dc.common.color;
            common.line_weight = dc.common.line_weight;
            dc.common = common;
            dc.block_name = block_name;
            if let Some(pt) = text_middle_point.get_point() {
                dc.text_middle_point = pt;
            }
            dc.text_rotation = text_rotation;
            dc.horizontal_direction = horizontal_direction;
            dc.text_user_positioned = text_user_positioned;
            dc.attachment_point = attachment_point;
            dc.line_spacing_style = line_spacing_style;
            if line_spacing_factor != 1.0 {
                dc.line_spacing_factor = line_spacing_factor;
            }
            if let Some(n) = normal.get_point() {
                dc.normal = n;
            }
        }

        // Radius group 10 is the centre, not the chord point.
        if !matches!(dimension, Dimension::Radius(_)) {
            if let Some(point) = definition_point.get_point() {
                dimension.set_definition_point(point);
            }
        }
        if let Dimension::Ordinate(value) = &mut dimension {
            value.refresh_measurement();
        }

        Ok(Some(dimension))
    }

    /// Read a HATCH entity
    fn read_hatch(&mut self) -> Result<Option<Hatch>> {
        self.read_hatch_kind(false)
    }

    fn read_mpolygon(&mut self) -> Result<Option<Hatch>> {
        self.read_hatch_kind(true)
    }

    fn read_hatch_kind(&mut self, is_mpolygon: bool) -> Result<Option<Hatch>> {
        use crate::entities::hatch::*;

        let mut hatch = Hatch::new();
        hatch.is_mpolygon = is_mpolygon;
        let mut current_subclass = String::new();
        let mut elevation_point = PointReader::new();
        let mut normal = PointReader::new();
        let mut mpolygon_x_direction = PointReader::new();
        let mut pattern_name = String::from("SOLID");
        let mut pattern_type = HatchPatternType::Predefined;
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut line_weight = LineWeight::ByLayer;
        let mut _num_boundary_paths = 0;
        let mut current_path_edges: Vec<BoundaryEdge> = Vec::new();
        let mut current_path_flags = BoundaryPathFlags::new();
        let mut current_path_handles: Vec<Handle> = Vec::new();
        let mut reading_boundary = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                100 => current_subclass = pair.value_string.clone(),
                8 => layer = pair.value_string.clone(),
                62 if is_mpolygon && current_subclass == "AcDbMPolygon" => {
                    if let Some(color_index) = pair.as_i16() {
                        hatch.mpolygon_hatch_color = Color::from_index(color_index);
                    }
                }
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line_weight = LineWeight::from_value(lw);
                    }
                }
                2 => pattern_name = pair.value_string.clone(),
                10 | 20 | 30 if !reading_boundary => {
                    elevation_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                11 | 21 if is_mpolygon && current_subclass == "AcDbMPolygon" => {
                    mpolygon_x_direction.add_coordinate(&pair);
                }
                99 if is_mpolygon => {
                    hatch.mpolygon_boundary_handle_count = pair.as_i32().unwrap_or(0)
                }
                70 => {
                    if let Some(solid_fill) = pair.as_i16() {
                        hatch.is_solid = solid_fill != 0;
                    }
                }
                71 => {
                    if let Some(associative) = pair.as_i16() {
                        hatch.is_associative = associative != 0;
                    }
                }
                75 => {
                    if let Some(style) = pair.as_i16() {
                        hatch.style = match style {
                            0 => HatchStyleType::Normal,
                            1 => HatchStyleType::Outer,
                            2 => HatchStyleType::Ignore,
                            _ => HatchStyleType::Normal,
                        };
                    }
                }
                76 => {
                    if let Some(ptype) = pair.as_i16() {
                        pattern_type = match ptype {
                            0 => HatchPatternType::UserDefined,
                            1 => HatchPatternType::Predefined,
                            2 => HatchPatternType::Custom,
                            _ => HatchPatternType::Predefined,
                        };
                    }
                }
                52 => {
                    if let Some(a) = pair.as_double() {
                        hatch.pattern_angle = a.to_radians();
                    }
                }
                41 => {
                    if let Some(s) = pair.as_double() {
                        hatch.pattern_scale = s;
                    }
                }
                77 => {
                    if let Some(d) = pair.as_i16() {
                        hatch.is_double = d != 0;
                    }
                }
                78 => {
                    // Number of pattern definition lines. Each line follows as
                    // 53 (angle), 43/44 (base point), 45/46 (offset), then 79
                    // (dash count) and that many 49 (dash length) codes.
                    let num_lines = pair.as_i16().unwrap_or(0).max(0) as usize;
                    for _ in 0..num_lines {
                        let line = self.read_hatch_pattern_line()?;
                        hatch.pattern.lines.push(line);
                    }
                }
                91 => {
                    if let Some(num_paths) = pair.as_i32() {
                        _num_boundary_paths = num_paths;
                    }
                }
                92 => {
                    // Boundary path type flags - indicates start of a new boundary path
                    if reading_boundary && !current_path_edges.is_empty() {
                        // Save previous path
                        let path = BoundaryPath {
                            flags: current_path_flags,
                            edges: current_path_edges.clone(),
                            boundary_handles: current_path_handles.clone(),
                        };
                        hatch.paths.push(path);
                        current_path_edges.clear();
                        current_path_handles.clear();
                    }
                    reading_boundary = true;
                    let flags_bits = pair.as_i32().unwrap_or(0) as u32;
                    current_path_flags = BoundaryPathFlags::from_bits(flags_bits);

                    // Polyline boundary — dispatch immediately
                    if current_path_flags.is_polyline() {
                        let edge = self.read_hatch_polyline_boundary()?;
                        current_path_edges.push(BoundaryEdge::Polyline(edge));
                    }
                }
                93 => {
                    // Number of edges in this boundary path (non-polyline)
                    // Already handled by reading edges individually; consume only.
                }
                72 => {
                    // Edge type - read edge data from subsequent group codes
                    if let Some(edge_type) = pair.as_i16() {
                        match edge_type {
                            1 => {
                                let edge = self.read_hatch_line_edge()?;
                                current_path_edges.push(BoundaryEdge::Line(edge));
                            }
                            2 => {
                                let edge = self.read_hatch_circular_arc_edge()?;
                                current_path_edges.push(BoundaryEdge::CircularArc(edge));
                            }
                            3 => {
                                let edge = self.read_hatch_elliptic_arc_edge()?;
                                current_path_edges.push(BoundaryEdge::EllipticArc(edge));
                            }
                            4 => {
                                let edge = self.read_hatch_spline_edge()?;
                                current_path_edges.push(BoundaryEdge::Spline(edge));
                            }
                            _ => {}
                        }
                    }
                }
                97 => {
                    // Number of source boundary objects
                    let num_handles = pair.as_i32().unwrap_or(0);
                    for _ in 0..num_handles {
                        if let Some(hp) = self.reader.read_pair()? {
                            if hp.code == 330 {
                                if let Some(h) = hp.as_handle() {
                                    current_path_handles.push(Handle::new(h));
                                }
                            }
                        }
                    }
                }
                47 => {
                    // Pixel size (offset vector length used to bound the hatch).
                    if let Some(v) = pair.as_double() {
                        hatch.pixel_size = v;
                    }
                }
                98 => {
                    // Seed point count, followed by that many 10/20 point pairs.
                    // These sit in the trailer past every boundary path, so the
                    // 10/20 codes read here can't be confused with edge vertices.
                    let n = pair.as_i32().unwrap_or(0).max(0);
                    for _ in 0..n {
                        let mut x = 0.0;
                        let mut y = 0.0;
                        if let Some(p) = self.reader.read_pair()? {
                            if p.code == 10 {
                                x = p.as_double().unwrap_or(0.0);
                            } else {
                                self.reader.push_back(p);
                                break;
                            }
                        }
                        if let Some(p) = self.reader.read_pair()? {
                            if p.code == 20 {
                                y = p.as_double().unwrap_or(0.0);
                            } else {
                                self.reader.push_back(p);
                            }
                        }
                        hatch.seed_points.push(Vector2::new(x, y));
                    }
                }
                450 => {
                    // Gradient fill definition (codes 450-470). Parse the block
                    // inline; a non-gradient code ends it and is pushed back for
                    // the outer loop. Each colour is a 463 (value) plus a 63
                    // (ACI) and/or 421 (24-bit RGB) — RGB wins when both appear.
                    hatch.gradient_color.enabled = pair.as_i32().unwrap_or(0) != 0;
                    while let Some(gp) = self.reader.read_pair()? {
                        match gp.code {
                            451 => hatch.gradient_color.reserved = gp.as_i32().unwrap_or(0),
                            452 => {
                                hatch.gradient_color.is_single_color = gp.as_i16().unwrap_or(0) != 0
                            }
                            453 => { /* colour count — inferred from 463 entries */ }
                            460 => {
                                if let Some(v) = gp.as_double() {
                                    hatch.gradient_color.angle = v;
                                }
                            }
                            461 => {
                                if let Some(v) = gp.as_double() {
                                    hatch.gradient_color.shift = v;
                                }
                            }
                            462 => {
                                if let Some(v) = gp.as_double() {
                                    hatch.gradient_color.color_tint = v;
                                }
                            }
                            463 => {
                                let value = gp.as_double().unwrap_or(0.0);
                                hatch.gradient_color.colors.push(GradientColorEntry {
                                    value,
                                    color: Color::ByLayer,
                                });
                            }
                            63 => {
                                if let (Some(entry), Ok(aci)) = (
                                    hatch.gradient_color.colors.last_mut(),
                                    gp.value_string.trim().parse::<i16>(),
                                ) {
                                    entry.color = Color::from_index(aci);
                                }
                            }
                            421 => {
                                if let (Some(entry), Ok(rgb)) = (
                                    hatch.gradient_color.colors.last_mut(),
                                    gp.value_string.trim().parse::<i32>(),
                                ) {
                                    entry.color = Color::from_true_color_value(rgb);
                                }
                            }
                            470 => hatch.gradient_color.name = gp.value_string.clone(),
                            _ => {
                                self.reader.push_back(gp);
                                break;
                            }
                        }
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut hatch.common)?;
                }
            }
        }

        // Save last boundary path if any
        if reading_boundary && !current_path_edges.is_empty() {
            let path = BoundaryPath {
                flags: current_path_flags,
                edges: current_path_edges,
                boundary_handles: current_path_handles,
            };
            hatch.paths.push(path);
        }

        hatch.common.layer = layer;
        // True color from code 420 overrides ACI (420 went directly into hatch.common)
        if !hatch.common.color.is_true_color() {
            hatch.common.color = color;
        }
        hatch.common.line_weight = line_weight;
        hatch.pattern.name = pattern_name;
        hatch.pattern_type = pattern_type;
        if let Some(point) = elevation_point.get_point() {
            hatch.elevation = point.z;
        }
        if let Some(value) = normal.get_point() {
            hatch.normal = value;
        }
        if let Some(value) = mpolygon_x_direction.get_point() {
            hatch.mpolygon_x_direction = Vector2::new(value.x, value.y);
        }

        Ok(Some(hatch))
    }

    /// Read one pattern definition line for a HATCH (codes 53, 43/44, 45/46,
    /// 79 + repeated 49). Angle is stored in radians to match the DWG reader.
    fn read_hatch_pattern_line(&mut self) -> Result<crate::entities::hatch::HatchPatternLine> {
        let mut line = crate::entities::hatch::HatchPatternLine {
            angle: 0.0,
            base_point: Vector2::new(0.0, 0.0),
            offset: Vector2::new(0.0, 0.0),
            dash_lengths: Vec::new(),
        };
        let mut num_dashes = 0usize;
        // Fixed prefix ending at 79 (dash count); tolerate a missing/reordered
        // code by pushing it back and stopping.
        while let Some(p) = self.reader.read_pair()? {
            match p.code {
                53 => line.angle = p.as_double().unwrap_or(0.0).to_radians(),
                43 => line.base_point.x = p.as_double().unwrap_or(0.0),
                44 => line.base_point.y = p.as_double().unwrap_or(0.0),
                45 => line.offset.x = p.as_double().unwrap_or(0.0),
                46 => line.offset.y = p.as_double().unwrap_or(0.0),
                79 => {
                    num_dashes = p.as_i16().unwrap_or(0).max(0) as usize;
                    break;
                }
                _ => {
                    self.reader.push_back(p);
                    break;
                }
            }
        }
        for _ in 0..num_dashes {
            match self.reader.read_pair()? {
                Some(p) if p.code == 49 => line.dash_lengths.push(p.as_double().unwrap_or(0.0)),
                Some(p) => {
                    self.reader.push_back(p);
                    break;
                }
                None => break,
            }
        }
        Ok(line)
    }

    /// Read a line edge for a HATCH boundary path (codes 10/20, 11/21)
    fn read_hatch_line_edge(&mut self) -> Result<crate::entities::hatch::LineEdge> {
        let mut edge = crate::entities::hatch::LineEdge {
            start: Vector2::new(0.0, 0.0),
            end: Vector2::new(0.0, 0.0),
        };
        // Expected sequence: 10, 20, 11, 21
        for _ in 0..4 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    10 => edge.start.x = p.as_double().unwrap_or(0.0),
                    20 => edge.start.y = p.as_double().unwrap_or(0.0),
                    11 => edge.end.x = p.as_double().unwrap_or(0.0),
                    21 => edge.end.y = p.as_double().unwrap_or(0.0),
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }
        Ok(edge)
    }

    /// Read a circular arc edge for a HATCH boundary path (codes 10/20, 40, 50, 51, 73)
    fn read_hatch_circular_arc_edge(&mut self) -> Result<crate::entities::hatch::CircularArcEdge> {
        let mut edge = crate::entities::hatch::CircularArcEdge {
            center: Vector2::new(0.0, 0.0),
            radius: 0.0,
            start_angle: 0.0,
            end_angle: 0.0,
            counter_clockwise: true,
        };
        // Expected sequence: 10, 20, 40, 50, 51, 73
        for _ in 0..6 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    10 => edge.center.x = p.as_double().unwrap_or(0.0),
                    20 => edge.center.y = p.as_double().unwrap_or(0.0),
                    40 => edge.radius = p.as_double().unwrap_or(0.0),
                    50 => edge.start_angle = p.as_double().unwrap_or(0.0).to_radians(),
                    51 => edge.end_angle = p.as_double().unwrap_or(0.0).to_radians(),
                    73 => edge.counter_clockwise = p.as_i16().unwrap_or(1) != 0,
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }
        Ok(edge)
    }

    /// Read an elliptic arc edge for a HATCH boundary path (codes 10/20, 11/21, 40, 50, 51, 73)
    fn read_hatch_elliptic_arc_edge(&mut self) -> Result<crate::entities::hatch::EllipticArcEdge> {
        let mut edge = crate::entities::hatch::EllipticArcEdge {
            center: Vector2::new(0.0, 0.0),
            major_axis_endpoint: Vector2::new(1.0, 0.0),
            minor_axis_ratio: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
            counter_clockwise: true,
        };
        // Expected sequence: 10, 20, 11, 21, 40, 50, 51, 73
        for _ in 0..8 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    10 => edge.center.x = p.as_double().unwrap_or(0.0),
                    20 => edge.center.y = p.as_double().unwrap_or(0.0),
                    11 => edge.major_axis_endpoint.x = p.as_double().unwrap_or(0.0),
                    21 => edge.major_axis_endpoint.y = p.as_double().unwrap_or(0.0),
                    40 => edge.minor_axis_ratio = p.as_double().unwrap_or(1.0),
                    50 => edge.start_angle = p.as_double().unwrap_or(0.0),
                    51 => edge.end_angle = p.as_double().unwrap_or(std::f64::consts::TAU),
                    73 => edge.counter_clockwise = p.as_i16().unwrap_or(1) != 0,
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }
        Ok(edge)
    }

    /// Read a spline edge for a HATCH boundary path
    fn read_hatch_spline_edge(&mut self) -> Result<crate::entities::hatch::SplineEdge> {
        let mut edge = crate::entities::hatch::SplineEdge {
            degree: 3,
            rational: false,
            periodic: false,
            knots: Vec::new(),
            control_points: Vec::new(),
            fit_points: Vec::new(),
            start_tangent: Vector2::new(0.0, 0.0),
            end_tangent: Vector2::new(0.0, 0.0),
        };
        let mut num_knots: i32 = 0;
        let mut num_control_points: i32 = 0;
        let mut num_fit_points: i32 = 0;

        // Read header codes: 94 (degree), 73 (rational), 74 (periodic), 95 (num knots), 96 (num control points)
        for _ in 0..5 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    94 => edge.degree = p.as_i32().unwrap_or(3),
                    73 => edge.rational = p.as_i16().unwrap_or(0) != 0,
                    74 => edge.periodic = p.as_i16().unwrap_or(0) != 0,
                    95 => num_knots = p.as_i32().unwrap_or(0),
                    96 => num_control_points = p.as_i32().unwrap_or(0),
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }

        // Read knot values (code 40)
        for _ in 0..num_knots {
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 40 {
                    edge.knots.push(p.as_double().unwrap_or(0.0));
                } else {
                    self.reader.push_back(p);
                    break;
                }
            }
        }

        // Read control points (codes 10/20, with optional weight 42)
        for _ in 0..num_control_points {
            let mut x = 0.0;
            let mut y = 0.0;
            let mut w = 1.0;
            // Read 10, 20
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 10 {
                    x = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                    continue;
                }
            }
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 20 {
                    y = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                }
            }
            // Peek for optional weight (code 42)
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 42 {
                    w = p.as_double().unwrap_or(1.0);
                } else {
                    self.reader.push_back(p);
                }
            }
            edge.control_points.push(Vector3::new(x, y, w));
        }

        // Check for fit data: code 97 = num fit points
        if let Some(p) = self.reader.read_pair()? {
            if p.code == 97 {
                num_fit_points = p.as_i32().unwrap_or(0);
            } else {
                self.reader.push_back(p);
            }
        }

        // Read fit points (codes 11/21)
        for _ in 0..num_fit_points {
            let mut x = 0.0;
            let mut y = 0.0;
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 11 {
                    x = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                    continue;
                }
            }
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 21 {
                    y = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                }
            }
            edge.fit_points.push(Vector2::new(x, y));
        }

        // Read optional start/end tangents (codes 12/22, 13/23)
        for _ in 0..4 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    12 => edge.start_tangent.x = p.as_double().unwrap_or(0.0),
                    22 => edge.start_tangent.y = p.as_double().unwrap_or(0.0),
                    13 => edge.end_tangent.x = p.as_double().unwrap_or(0.0),
                    23 => edge.end_tangent.y = p.as_double().unwrap_or(0.0),
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }

        Ok(edge)
    }

    /// Read a polyline boundary for a HATCH (codes 72, 73, 93, then 10/20/42 per vertex)
    fn read_hatch_polyline_boundary(&mut self) -> Result<crate::entities::hatch::PolylineEdge> {
        let mut has_bulge = false;
        let mut is_closed = false;
        let mut num_vertices: i32 = 0;

        // Read 72 (has_bulge), 73 (is_closed), 93 (num_vertices)
        for _ in 0..3 {
            if let Some(p) = self.reader.read_pair()? {
                match p.code {
                    72 => has_bulge = p.as_i16().unwrap_or(0) != 0,
                    73 => is_closed = p.as_i16().unwrap_or(0) != 0,
                    93 => num_vertices = p.as_i32().unwrap_or(0),
                    _ => {
                        self.reader.push_back(p);
                        break;
                    }
                }
            }
        }

        let mut edge = crate::entities::hatch::PolylineEdge {
            vertices: Vec::with_capacity(num_vertices as usize),
            is_closed,
        };

        // Read vertices: 10/20 (coords), optional 42 (bulge)
        for _ in 0..num_vertices {
            let mut x = 0.0;
            let mut y = 0.0;
            let mut bulge = 0.0;
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 10 {
                    x = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                    continue;
                }
            }
            if let Some(p) = self.reader.read_pair()? {
                if p.code == 20 {
                    y = p.as_double().unwrap_or(0.0);
                } else {
                    self.reader.push_back(p);
                }
            }
            if has_bulge {
                if let Some(p) = self.reader.read_pair()? {
                    if p.code == 42 {
                        bulge = p.as_double().unwrap_or(0.0);
                    } else {
                        self.reader.push_back(p);
                    }
                }
            }
            edge.vertices.push(Vector3::new(x, y, bulge));
        }

        Ok(edge)
    }

    /// Read a SOLID entity
    fn read_solid(&mut self) -> Result<Option<Solid>> {
        let mut corner1 = PointReader::new();
        let mut corner2 = PointReader::new();
        let mut corner3 = PointReader::new();
        let mut corner4 = PointReader::new();
        let mut normal = PointReader::new();
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut line_weight = LineWeight::ByLayer;
        let mut thickness = 0.0f64;
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    corner1.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    corner2.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    corner3.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    corner4.add_coordinate(&pair);
                }
                39 => {
                    if let Some(t) = pair.as_double() {
                        thickness = t;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let pt1 = corner1.get_point().unwrap_or(Vector3::zero());
        let pt2 = corner2.get_point().unwrap_or(Vector3::zero());
        let pt3 = corner3.get_point().unwrap_or(Vector3::zero());
        let pt4 = corner4.get_point().unwrap_or(pt3);

        let mut solid = Solid::new(pt1, pt2, pt3, pt4);
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        common.line_weight = line_weight;
        solid.common = common;
        solid.thickness = thickness;
        if let Some(n) = normal.get_point() {
            solid.normal = n;
        }

        Ok(Some(solid))
    }

    /// Read a 3DFACE entity
    fn read_face3d(&mut self) -> Result<Option<Face3D>> {
        let mut corner1 = PointReader::new();
        let mut corner2 = PointReader::new();
        let mut corner3 = PointReader::new();
        let mut corner4 = PointReader::new();
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut line_weight = LineWeight::ByLayer;
        let mut invisible_flags = 0i16;
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line_weight = LineWeight::from_value(lw);
                    }
                }
                10 | 20 | 30 => {
                    corner1.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    corner2.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    corner3.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    corner4.add_coordinate(&pair);
                }
                70 => {
                    if let Some(flags) = pair.as_i16() {
                        invisible_flags = flags;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let pt1 = corner1.get_point().unwrap_or(Vector3::zero());
        let pt2 = corner2.get_point().unwrap_or(Vector3::zero());
        let pt3 = corner3.get_point().unwrap_or(Vector3::zero());
        let pt4 = corner4.get_point().unwrap_or(pt3);

        use crate::entities::face3d::InvisibleEdgeFlags;
        let mut invisible_edges = InvisibleEdgeFlags::new();
        invisible_edges.set_first_invisible((invisible_flags & 1) != 0);
        invisible_edges.set_second_invisible((invisible_flags & 2) != 0);
        invisible_edges.set_third_invisible((invisible_flags & 4) != 0);
        invisible_edges.set_fourth_invisible((invisible_flags & 8) != 0);

        let mut face = Face3D::new(pt1, pt2, pt3, pt4);
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        common.line_weight = line_weight;
        face.common = common;
        face.invisible_edges = invisible_edges;

        Ok(Some(face))
    }

    /// Read an INSERT entity
    fn read_insert(&mut self) -> Result<Option<Insert>> {
        let mut block_name = String::new();
        let mut insertion = PointReader::new();
        let mut normal = PointReader::new();
        let mut x_scale = 1.0;
        let mut y_scale = 1.0;
        let mut z_scale = 1.0;
        let mut rotation = 0.0;
        let mut column_count = 1u16;
        let mut row_count = 1u16;
        let mut column_spacing = 0.0;
        let mut row_spacing = 0.0;
        let mut has_attributes = false;
        let mut view_rep_subclass = false;
        let mut view_rep_handle = None;
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut line_weight = LineWeight::ByLayer;
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                100 => {
                    view_rep_subclass = pair
                        .value_string
                        .trim()
                        .eq_ignore_ascii_case("AcDbViewRepBlockReference");
                }
                330 if view_rep_subclass => {
                    view_rep_handle = pair.as_handle().map(Handle::from);
                }
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                370 => {
                    if let Some(lw) = pair.as_i16() {
                        line_weight = LineWeight::from_value(lw);
                    }
                }
                2 => block_name = pair.value_string.clone(),
                66 => {
                    has_attributes = pair.as_i16() == Some(1);
                }
                10 | 20 | 30 => {
                    insertion.add_coordinate(&pair);
                }
                41 => {
                    if let Some(sx) = pair.as_double() {
                        x_scale = sx;
                    }
                }
                42 => {
                    if let Some(sy) = pair.as_double() {
                        y_scale = sy;
                    }
                }
                43 => {
                    if let Some(sz) = pair.as_double() {
                        z_scale = sz;
                    }
                }
                50 => {
                    if let Some(rot) = pair.as_double() {
                        rotation = rot.to_radians();
                    }
                }
                70 => {
                    if let Some(col_count) = pair.as_i16() {
                        column_count = col_count.max(1) as u16;
                    }
                }
                71 => {
                    if let Some(r_count) = pair.as_i16() {
                        row_count = r_count.max(1) as u16;
                    }
                }
                44 => {
                    if let Some(col_spacing_val) = pair.as_double() {
                        column_spacing = col_spacing_val;
                    }
                }
                45 => {
                    if let Some(row_spacing_val) = pair.as_double() {
                        row_spacing = row_spacing_val;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let insert_point = insertion.get_point().unwrap_or(Vector3::zero());
        let mut insert = Insert::new(block_name, insert_point);
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        common.line_weight = line_weight;
        insert.common = common;
        insert.set_x_scale(x_scale);
        insert.set_y_scale(y_scale);
        insert.set_z_scale(z_scale);
        insert.rotation = rotation;
        insert.column_count = column_count;
        insert.row_count = row_count;
        insert.column_spacing = column_spacing;
        insert.row_spacing = row_spacing;
        insert.view_rep_handle = view_rep_handle;
        if let Some(n) = normal.get_point() {
            insert.normal = n;
        }

        // Collect trailing ATTRIB entities (terminated by SEQEND)
        if has_attributes {
            loop {
                // Peek at the next entity type
                if let Some(pair) = self.reader.read_pair()? {
                    if pair.code == 0 {
                        let entity_name = pair.value_string.trim().to_uppercase();
                        match entity_name.as_str() {
                            "ATTRIB" => {
                                if let Some(att) = self.read_attrib()? {
                                    insert.attributes.push(att);
                                }
                            }
                            "SEQEND" => {
                                // Consume SEQEND contents and stop
                                self.skip_entity()?;
                                break;
                            }
                            _ => {
                                // Unexpected entity – push back and stop
                                self.reader.push_back(pair);
                                break;
                            }
                        }
                    } else {
                        // Not a code-0 pair; shouldn't happen, push back
                        self.reader.push_back(pair);
                        break;
                    }
                } else {
                    break; // EOF
                }
            }
        }

        Ok(Some(insert))
    }

    /// Read a RAY entity
    fn read_ray(&mut self) -> Result<Option<Ray>> {
        let mut base_point = PointReader::new();
        let mut direction = PointReader::new();
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                10 | 20 | 30 => {
                    base_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    direction.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let bp = base_point.get_point().unwrap_or(Vector3::zero());
        let dir = direction.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0));
        let mut ray = Ray::new(bp, dir);
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        ray.common = common;

        Ok(Some(ray))
    }

    /// Read an XLINE entity
    fn read_xline(&mut self) -> Result<Option<XLine>> {
        let mut base_point = PointReader::new();
        let mut direction = PointReader::new();
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut common = EntityCommon::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                10 | 20 | 30 => {
                    base_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    direction.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let bp = base_point.get_point().unwrap_or(Vector3::zero());
        let dir = direction.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0));
        let mut xline = XLine::new(bp, dir);
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        xline.common = common;

        Ok(Some(xline))
    }

    /// Read an ATTDEF entity
    fn read_attdef(&mut self) -> Result<Option<AttributeDefinition>> {
        let mut tag = String::new();
        let mut prompt = String::new();
        let mut default_value = String::new();
        let mut insertion_point = PointReader::new();
        let mut height = 0.0;
        let mut rotation = 0.0;
        let mut layer = String::from("0");
        let mut color = Color::ByLayer;
        let mut common = EntityCommon::new();
        let mut lock_position = false;
        // Code 280 appears twice: first the version byte, then lock-position.
        let mut seen_version = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        color = Color::from_index(color_index);
                    }
                }
                1 => default_value = pair.value_string.clone(),
                2 => tag = pair.value_string.clone(),
                3 => prompt = pair.value_string.clone(),
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                40 => {
                    if let Some(h) = pair.as_double() {
                        height = h;
                    }
                }
                50 => {
                    if let Some(r) = pair.as_double() {
                        rotation = r.to_radians();
                    }
                }
                280 => {
                    if !seen_version {
                        seen_version = true;
                    } else if let Some(v) = pair.as_i16() {
                        lock_position = v != 0;
                    }
                }
                // Multiline attribute-definition embedded MTEXT (R2018+) —
                // carries the real default text when the own code 1 is empty.
                101 => {
                    let t = self.read_attrib_embedded_text()?;
                    if !t.is_empty() {
                        default_value = t;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        let mut attdef = AttributeDefinition::new(tag, prompt, default_value);
        attdef.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        attdef.height = height;
        attdef.rotation = rotation;
        attdef.lock_position = lock_position;
        // True color from code 420 overrides ACI
        if common.color.is_true_color() {
            color = common.color;
        }
        common.layer = layer;
        common.color = color;
        attdef.common = common;

        Ok(Some(attdef))
    }

    /// Read a TOLERANCE entity
    fn read_tolerance(&mut self) -> Result<Option<Tolerance>> {
        let mut tolerance = Tolerance::new();
        let mut insertion_point = PointReader::new();
        let mut direction = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => tolerance.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        tolerance.common.color = Color::from_index(color_index);
                    }
                }
                1 => tolerance.text = pair.value_string.clone(),
                3 => tolerance.dimension_style_name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    direction.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut tolerance.common)?;
                }
            }
        }

        tolerance.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        tolerance.direction = direction.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0));
        if let Some(normal) = normal.get_point() {
            tolerance.normal = normal;
        }

        Ok(Some(tolerance))
    }

    /// Read a SHAPE entity
    fn read_shape(&mut self) -> Result<Option<Shape>> {
        let mut shape = Shape::new();
        let mut insertion_point = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => shape.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        shape.common.color = Color::from_index(color_index);
                    }
                }
                2 => shape.shape_name = pair.value_string.clone(),
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                40 => {
                    if let Some(s) = pair.as_double() {
                        shape.size = s;
                    }
                }
                50 => {
                    if let Some(r) = pair.as_double() {
                        shape.rotation = r;
                    }
                }
                // Previously dropped: thickness, relative X scale, oblique
                // angle, extrusion — all present on the struct + DWG path.
                39 => {
                    if let Some(t) = pair.as_double() {
                        shape.thickness = t;
                    }
                }
                41 => {
                    if let Some(s) = pair.as_double() {
                        shape.relative_x_scale = s;
                    }
                }
                51 => {
                    if let Some(o) = pair.as_double() {
                        shape.oblique_angle = o;
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut shape.common)?;
                }
            }
        }

        shape.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        if let Some(n) = normal.get_point() {
            shape.normal = n;
        }

        Ok(Some(shape))
    }

    /// Read a WIPEOUT entity
    fn read_wipeout(&mut self) -> Result<Option<Wipeout>> {
        let mut wipeout = Wipeout::new();
        let mut insertion_point = PointReader::new();
        let mut u_vector = PointReader::new();
        let mut v_vector = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }

            match pair.code {
                8 => wipeout.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(color_index) = pair.as_i16() {
                        wipeout.common.color = Color::from_index(color_index);
                    }
                }
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    u_vector.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    v_vector.add_coordinate(&pair);
                }
                13 => {
                    if let Some(v) = pair.as_double() {
                        wipeout.size.x = v;
                    }
                }
                23 => {
                    if let Some(v) = pair.as_double() {
                        wipeout.size.y = v;
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        wipeout.class_version = v;
                    }
                }
                340 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    if !handle.is_null() {
                        wipeout.definition_handle = Some(handle);
                    }
                }
                360 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    if !handle.is_null() {
                        wipeout.definition_reactor_handle = Some(handle);
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.flags = WipeoutDisplayFlags::from_bits_truncate(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.clip_type = crate::entities::wipeout::WipeoutClipType::from(v);
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.clipping_enabled = v != 0;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.brightness = v.clamp(0, 100) as u8;
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.contrast = v.clamp(0, 100) as u8;
                    }
                }
                283 => {
                    if let Some(v) = pair.as_i16() {
                        wipeout.fade = v.clamp(0, 100) as u8;
                    }
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        wipeout.clip_mode = if v {
                            crate::entities::wipeout::WipeoutClipMode::Inside
                        } else {
                            crate::entities::wipeout::WipeoutClipMode::Outside
                        };
                    }
                }
                91 => {
                    // Boundary vertex count precedes the 14/24 pairs; drop the
                    // default-seeded corners so the file's vertices don't append.
                    wipeout.clip_boundary_vertices.clear();
                }
                14 => {
                    if let Some(x) = pair.as_double() {
                        wipeout.clip_boundary_vertices.push(Vector2::new(x, 0.0));
                    }
                }
                24 => {
                    if let Some(y) = pair.as_double() {
                        if let Some(last) = wipeout.clip_boundary_vertices.last_mut() {
                            last.y = y;
                        }
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut wipeout.common)?;
                }
            }
        }

        wipeout.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        wipeout.u_vector = u_vector.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0));
        wipeout.v_vector = v_vector.get_point().unwrap_or(Vector3::new(0.0, 1.0, 0.0));

        Ok(Some(wipeout))
    }

    /// Read extended data (XDATA) from the current position
    /// Returns the extended data and the last pair read (which is not part of XDATA)
    fn read_extended_data(
        &mut self,
    ) -> Result<(ExtendedData, Option<super::stream_reader::DxfCodePair>)> {
        let mut xdata = ExtendedData::new();
        let mut current_record: Option<ExtendedDataRecord> = None;
        let mut point_reader = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            match pair.code {
                // Application name - start of new record
                1001 => {
                    // Save previous record if exists
                    if let Some(record) = current_record.take() {
                        xdata.add_record(record);
                    }
                    // Start new record
                    current_record = Some(ExtendedDataRecord::new(pair.value_string.clone()));
                }
                // String value
                1000 => {
                    if let Some(ref mut record) = current_record {
                        record.add_value(XDataValue::String(pair.value_string.clone()));
                    }
                }
                // Control string
                1002 => {
                    if let Some(ref mut record) = current_record {
                        record.add_value(XDataValue::ControlString(pair.value_string.clone()));
                    }
                }
                // Layer name
                1003 => {
                    if let Some(ref mut record) = current_record {
                        record.add_value(XDataValue::LayerName(pair.value_string.clone()));
                    }
                }
                // Binary data
                1004 => {
                    if let Some(ref mut record) = current_record {
                        // Parse hex string to bytes
                        let bytes: Vec<u8> = (0..pair.value_string.len())
                            .step_by(2)
                            .filter_map(|i| {
                                let end = (i + 2).min(pair.value_string.len());
                                u8::from_str_radix(&pair.value_string[i..end], 16).ok()
                            })
                            .collect();
                        record.add_value(XDataValue::BinaryData(bytes));
                    }
                }
                // Database handle
                1005 => {
                    if let Some(ref mut record) = current_record {
                        if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                            record.add_value(XDataValue::Handle(Handle::new(h)));
                        }
                    }
                }
                // 3D point (1010, 1020, 1030)
                1010 | 1020 | 1030 => {
                    if let Some(ref mut record) = current_record {
                        if pair.code == 1010 {
                            point_reader.reset();
                        }
                        point_reader.add_coordinate(&pair);
                        if pair.code == 1030 {
                            if let Some(pt) = point_reader.get_point() {
                                record.add_value(XDataValue::Point3D(pt));
                            }
                            point_reader.reset();
                        }
                    }
                }
                // 3D position (1011, 1021, 1031)
                1011 | 1021 | 1031 => {
                    if let Some(ref mut record) = current_record {
                        if pair.code == 1011 {
                            point_reader.reset();
                        }
                        point_reader.add_coordinate(&pair);
                        if pair.code == 1031 {
                            if let Some(pt) = point_reader.get_point() {
                                record.add_value(XDataValue::Position3D(pt));
                            }
                            point_reader.reset();
                        }
                    }
                }
                // 3D displacement (1012, 1022, 1032)
                1012 | 1022 | 1032 => {
                    if let Some(ref mut record) = current_record {
                        if pair.code == 1012 {
                            point_reader.reset();
                        }
                        point_reader.add_coordinate(&pair);
                        if pair.code == 1032 {
                            if let Some(pt) = point_reader.get_point() {
                                record.add_value(XDataValue::Displacement3D(pt));
                            }
                            point_reader.reset();
                        }
                    }
                }
                // 3D direction (1013, 1023, 1033)
                1013 | 1023 | 1033 => {
                    if let Some(ref mut record) = current_record {
                        if pair.code == 1013 {
                            point_reader.reset();
                        }
                        point_reader.add_coordinate(&pair);
                        if pair.code == 1033 {
                            if let Some(pt) = point_reader.get_point() {
                                record.add_value(XDataValue::Direction3D(pt));
                            }
                            point_reader.reset();
                        }
                    }
                }
                // Real value
                1040 => {
                    if let Some(ref mut record) = current_record {
                        if let Some(value) = pair.as_double() {
                            record.add_value(XDataValue::Real(value));
                        }
                    }
                }
                // Distance
                1041 => {
                    if let Some(ref mut record) = current_record {
                        if let Some(value) = pair.as_double() {
                            record.add_value(XDataValue::Distance(value));
                        }
                    }
                }
                // Scale factor
                1042 => {
                    if let Some(ref mut record) = current_record {
                        if let Some(value) = pair.as_double() {
                            record.add_value(XDataValue::ScaleFactor(value));
                        }
                    }
                }
                // 16-bit integer
                1070 => {
                    if let Some(ref mut record) = current_record {
                        if let Some(value) = pair.as_i16() {
                            record.add_value(XDataValue::Integer16(value));
                        }
                    }
                }
                // 32-bit integer
                1071 => {
                    if let Some(ref mut record) = current_record {
                        if let Some(value) = pair.as_i32() {
                            record.add_value(XDataValue::Integer32(value));
                        }
                    }
                }
                // Not XDATA - return what we have
                _ => {
                    // Save last record if exists
                    if let Some(record) = current_record.take() {
                        xdata.add_record(record);
                    }
                    return Ok((xdata, Some(pair)));
                }
            }
        }

        // End of file - save last record if exists
        if let Some(record) = current_record.take() {
            xdata.add_record(record);
        }

        Ok((xdata, None))
    }

    /// Parse the `AcadAnnotative` XDATA following a `1001` pair on a style
    /// record and return its annotative flag. The block has the form
    /// `AnnotativeData { 1 <flag> }`; the flag is the last 16-bit integer.
    /// The terminating non-XDATA pair is pushed back for the caller's loop.
    fn read_annotative_xdata(&mut self, pair: super::stream_reader::DxfCodePair) -> Result<bool> {
        use crate::xdata::XDataValue;
        self.reader.push_back(pair);
        let (xdata, next_pair) = self.read_extended_data()?;
        if let Some(p) = next_pair {
            self.reader.push_back(p);
        }
        let flag = xdata
            .get_record("AcadAnnotative")
            .and_then(|r| {
                r.values
                    .iter()
                    .filter_map(|v| match v {
                        XDataValue::Integer16(n) => Some(*n),
                        _ => None,
                    })
                    .last()
            })
            .map(|n| n != 0)
            .unwrap_or(false);
        Ok(flag)
    }

    // ===== New Entity Readers =====

    /// Read a VIEWPORT entity
    fn read_viewport(&mut self) -> Result<Option<Viewport>> {
        let mut vp = Viewport::new();
        let mut center = PointReader::new();
        let mut view_center = PointReader::new();
        let mut view_direction = PointReader::new();
        let mut view_target = PointReader::new();
        let mut snap_base_x: Option<f64> = None;
        let mut snap_base_y: Option<f64> = None;
        let mut snap_spacing_x: Option<f64> = None;
        let mut snap_spacing_y: Option<f64> = None;
        let mut grid_spacing_x: Option<f64> = None;
        let mut grid_spacing_y: Option<f64> = None;
        let mut ucs_origin = PointReader::new();
        let mut ucs_x_axis = PointReader::new();
        let mut ucs_y_axis = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => vp.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        vp.common.color = Color::from_index(v);
                    }
                }
                10 | 20 | 30 => {
                    center.add_coordinate(&pair);
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        vp.width = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        vp.height = v;
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        vp.status = crate::entities::viewport::ViewportStatusFlags::from_bits(v);
                    }
                }
                69 => {
                    if let Some(v) = pair.as_i16() {
                        vp.id = v;
                    }
                }
                12 => {
                    if let Some(v) = pair.as_double() {
                        view_center.add_coordinate(&pair);
                        let _ = v;
                    }
                }
                22 => {
                    view_center.add_coordinate(&pair);
                }
                13 => {
                    snap_base_x = pair.as_double();
                }
                23 => {
                    snap_base_y = pair.as_double();
                }
                14 => {
                    snap_spacing_x = pair.as_double();
                }
                24 => {
                    snap_spacing_y = pair.as_double();
                }
                15 => {
                    grid_spacing_x = pair.as_double();
                }
                25 => {
                    grid_spacing_y = pair.as_double();
                }
                16 | 26 | 36 => {
                    view_direction.add_coordinate(&pair);
                }
                17 | 27 | 37 => {
                    view_target.add_coordinate(&pair);
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        vp.lens_length = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        vp.front_clip_z = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        vp.back_clip_z = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        vp.view_height = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        vp.snap_angle = v;
                    }
                }
                51 => {
                    if let Some(v) = pair.as_double() {
                        vp.twist_angle = v;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        vp.circle_sides = v;
                    }
                }
                331 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        vp.frozen_layers.push(Handle::new(h));
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        vp.render_mode =
                            crate::entities::viewport::ViewportRenderMode::from_value(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        vp.ucs_per_viewport = v != 0;
                    }
                }
                110 | 120 | 130 => {
                    ucs_origin.add_coordinate(&pair);
                }
                111 | 121 | 131 => {
                    ucs_x_axis.add_coordinate(&pair);
                }
                112 | 122 | 132 => {
                    ucs_y_axis.add_coordinate(&pair);
                }
                146 => {
                    if let Some(v) = pair.as_double() {
                        vp.elevation = v;
                    }
                }
                61 => {
                    if let Some(v) = pair.as_i16() {
                        vp.grid_major = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        vp.brightness = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        vp.contrast = v;
                    }
                }
                292 => {
                    if let Some(v) = pair.as_bool() {
                        vp.default_lighting = v;
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        vp.default_lighting_type = v;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut vp.common)?;
                }
            }
        }

        if let Some(pt) = center.get_point() {
            vp.center = pt;
        }
        if let Some(pt) = view_direction.get_point() {
            vp.view_direction = pt;
        }
        if let Some(pt) = view_target.get_point() {
            vp.view_target = pt;
        }
        if let Some(pt) = ucs_origin.get_point() {
            vp.ucs_origin = pt;
        }
        if let Some(pt) = ucs_x_axis.get_point() {
            vp.ucs_x_axis = pt;
        }
        if let Some(pt) = ucs_y_axis.get_point() {
            vp.ucs_y_axis = pt;
        }
        // For 2D points, assemble manually
        if let (Some(x), Some(y)) = (
            view_center.get_point().map(|p| p.x),
            view_center.get_point().map(|p| p.y),
        ) {
            vp.view_center = Vector3::new(x, y, 0.0);
        }
        vp.snap_base = Vector3::new(snap_base_x.unwrap_or(0.0), snap_base_y.unwrap_or(0.0), 0.0);
        vp.snap_spacing = Vector3::new(
            snap_spacing_x.unwrap_or(10.0),
            snap_spacing_y.unwrap_or(10.0),
            0.0,
        );
        vp.grid_spacing = Vector3::new(
            grid_spacing_x.unwrap_or(10.0),
            grid_spacing_y.unwrap_or(10.0),
            0.0,
        );

        Ok(Some(vp))
    }

    /// Read an ATTRIB entity
    fn read_attrib(&mut self) -> Result<Option<AttributeEntity>> {
        let mut attrib = AttributeEntity::new(String::new(), String::new());
        let mut insertion_point = PointReader::new();
        let mut alignment_point = PointReader::new();
        // Code 280 appears twice: first the version byte, then lock-position.
        let mut seen_attrib_version = false;
        // Code 71 means different things by subclass: AcDbText → text
        // generation flags (2=backward, 4=upside-down); AcDbAttribute → the
        // MTEXT flag (2=multiline). Conflating them mirrored multiline text.
        let mut in_attribute_subclass = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => {
                    if pair.value_string == "AcDbAttribute" {
                        in_attribute_subclass = true;
                    }
                }
                8 => attrib.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        attrib.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        attrib.common.line_weight = LineWeight::from_value(v);
                    }
                }
                1 => attrib.value = pair.value_string.clone(),
                2 => attrib.tag = pair.value_string.clone(),
                7 => attrib.text_style = pair.value_string.clone(),
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    alignment_point.add_coordinate(&pair);
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        attrib.height = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        attrib.width_factor = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        attrib.rotation = v;
                    }
                }
                51 => {
                    if let Some(v) = pair.as_double() {
                        attrib.oblique_angle = v;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        attrib.flags =
                            crate::entities::attribute_definition::AttributeFlags::from_bits(
                                v as i32,
                            );
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        if in_attribute_subclass {
                            // AcDbAttribute: MTEXT flag, not generation flags.
                            use crate::entities::attribute_definition::MTextFlag;
                            attrib.mtext_flag = match v {
                                2 => MTextFlag::MultiLine,
                                4 => MTextFlag::ConstantMultiLine,
                                _ => MTextFlag::SingleLine,
                            };
                            attrib.is_multiline =
                                !matches!(attrib.mtext_flag, MTextFlag::SingleLine);
                        } else {
                            attrib.text_generation_flags = v;
                        }
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        attrib.horizontal_alignment =
                            crate::entities::attribute_definition::HorizontalAlignment::from_value(
                                v,
                            );
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        attrib.vertical_alignment =
                            crate::entities::attribute_definition::VerticalAlignment::from_value(v);
                    }
                }
                280 => {
                    // First 280 is the version byte; the second is lock-position.
                    if !seen_attrib_version {
                        seen_attrib_version = true;
                    } else if let Some(v) = pair.as_i16() {
                        attrib.lock_position = v != 0;
                    }
                }
                // Multiline attribute's embedded MTEXT (R2018+) — carries the
                // real text; the entity's own code 1 is empty in that case.
                101 => {
                    let t = self.read_attrib_embedded_text()?;
                    if !t.is_empty() {
                        attrib.value = t;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut attrib.common)?;
                }
            }
        }

        attrib.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        attrib.alignment_point = alignment_point.get_point().unwrap_or(Vector3::zero());

        Ok(Some(attrib))
    }

    /// Read a LEADER entity
    fn read_leader(&mut self) -> Result<Option<Leader>> {
        let mut leader = Leader::new();
        let mut normal = PointReader::new();
        let mut horiz_dir = PointReader::new();
        let mut block_offset = PointReader::new();
        let mut annotation_offset = PointReader::new();
        let mut reading_vertex = false;
        let mut current_vertex = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => leader.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        leader.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        leader.common.line_weight = LineWeight::from_value(v);
                    }
                }
                3 => leader.dimension_style = pair.value_string.clone(),
                71 => {
                    if let Some(v) = pair.as_i16() {
                        leader.arrow_enabled = v != 0;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        leader.path_type = if v == 1 {
                            crate::entities::leader::LeaderPathType::Spline
                        } else {
                            crate::entities::leader::LeaderPathType::StraightLine
                        };
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        leader.creation_type = match v {
                            0 => crate::entities::leader::LeaderCreationType::WithText,
                            1 => crate::entities::leader::LeaderCreationType::WithTolerance,
                            2 => crate::entities::leader::LeaderCreationType::WithBlock,
                            _ => crate::entities::leader::LeaderCreationType::NoAnnotation,
                        };
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        leader.hookline_direction = if v == 1 {
                            crate::entities::leader::HooklineDirection::Same
                        } else {
                            crate::entities::leader::HooklineDirection::Opposite
                        };
                    }
                }
                75 => {
                    if let Some(v) = pair.as_i16() {
                        leader.hookline_enabled = v != 0;
                    }
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        leader.text_height = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        leader.text_width = v;
                    }
                }
                10 => {
                    // Save previous vertex
                    if reading_vertex {
                        if let Some(pt) = current_vertex.get_point() {
                            leader.vertices.push(pt);
                        }
                    }
                    current_vertex = PointReader::new();
                    current_vertex.add_coordinate(&pair);
                    reading_vertex = true;
                }
                20 | 30 => {
                    current_vertex.add_coordinate(&pair);
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        leader.annotation_handle = Handle::new(h);
                    }
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                211 | 221 | 231 => {
                    horiz_dir.add_coordinate(&pair);
                }
                212 | 222 | 232 => {
                    block_offset.add_coordinate(&pair);
                }
                213 | 223 | 233 => {
                    annotation_offset.add_coordinate(&pair);
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut leader.common)?;
                }
            }
        }

        // Save last vertex
        if reading_vertex {
            if let Some(pt) = current_vertex.get_point() {
                leader.vertices.push(pt);
            }
        }
        if let Some(pt) = normal.get_point() {
            leader.normal = pt;
        }
        if let Some(pt) = horiz_dir.get_point() {
            leader.horizontal_direction = pt;
        }
        if let Some(pt) = block_offset.get_point() {
            leader.block_offset = pt;
        }
        if let Some(pt) = annotation_offset.get_point() {
            leader.annotation_offset = pt;
        }

        Ok(Some(leader))
    }

    /// Read a MULTILEADER entity. The geometry/content lives in the nested
    /// `300 CONTEXT_DATA{ … 302 LEADER{ … 304 LEADER_LINE{ … }}}` sections —
    /// they reuse the entity-level group codes, so each nesting level is
    /// parsed by its own loop (letting them fall through the entity match
    /// used to leave the context empty: no leader lines, no text — invisible
    /// multileaders from DXF while the same drawing's DWG was fine).
    /// Code mapping mirrors `write_multileader` for round-trip fidelity.
    fn read_multileader(&mut self) -> Result<Option<MultiLeader>> {
        use crate::entities::multileader as mlt;
        let mut ml = MultiLeader::new();
        let mut block_scale = PointReader::new();
        let mut section = String::new();
        let mut current_block_attribute: Option<mlt::BlockAttribute> = None;
        let mut proxy_graphics_size = 0usize;
        let mut proxy_graphics = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => section = pair.value_string.clone(),
                8 => ml.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        ml.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        ml.common.line_weight = LineWeight::from_value(v);
                    }
                }
                92 | 160 if section == "AcDbEntity" => {
                    proxy_graphics_size = pair.as_int().unwrap_or(0).max(0) as usize;
                }
                310 if section == "AcDbEntity" => {
                    append_hex_bytes(&mut proxy_graphics, &pair.value_string);
                }
                102 => {
                    let group = pair.value_string.trim().to_string();
                    if group.starts_with('{') {
                        while let Some(group_pair) = self.reader.read_pair()? {
                            if group_pair.code == 102 && group_pair.value_string.trim() == "}" {
                                break;
                            }
                            if group == "{ACAD_REACTORS" && group_pair.code == 330 {
                                if let Ok(h) =
                                    u64::from_str_radix(group_pair.value_string.trim(), 16)
                                {
                                    ml.common.reactors.push(Handle::new(h));
                                }
                            } else if group == "{ACAD_XDICTIONARY" && group_pair.code == 360 {
                                if let Ok(h) =
                                    u64::from_str_radix(group_pair.value_string.trim(), 16)
                                {
                                    ml.common.xdictionary_handle = Some(Handle::new(h));
                                }
                            }
                        }
                    }
                }
                270 => {
                    if let Some(value) = pair.as_i16() {
                        ml.dwg_version = value;
                    }
                }
                300 if pair.value_string.starts_with("CONTEXT_DATA") => {
                    self.read_mleader_context(&mut ml.context)?;
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ml.style_handle = Some(Handle::new(h));
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        ml.property_override_flags =
                            mlt::MultiLeaderPropertyOverrideFlags::from_bits_retain(v as u32);
                    }
                }
                170 => {
                    if let Some(v) = pair.as_i16() {
                        ml.path_type = mlt::MultiLeaderPathType::from(v);
                    }
                }
                91 => {
                    if let Some(v) = pair.as_i32() {
                        ml.line_color = color_from_i32(v);
                    }
                }
                341 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ml.line_type_handle = Some(Handle::new(h));
                    }
                }
                171 => {
                    if let Some(v) = pair.as_i16() {
                        ml.line_weight = LineWeight::from_value(v);
                    }
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        ml.enable_landing = v;
                    }
                }
                291 => {
                    if let Some(v) = pair.as_bool() {
                        ml.enable_dogleg = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        ml.dogleg_length = v;
                    }
                }
                342 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        if h != 0 {
                            ml.arrowhead_handle = Some(Handle::new(h));
                        }
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        ml.arrowhead_size = v;
                    }
                }
                172 => {
                    if let Some(v) = pair.as_i16() {
                        ml.content_type = match v {
                            1 => mlt::LeaderContentType::Block,
                            2 => mlt::LeaderContentType::MText,
                            _ => mlt::LeaderContentType::None,
                        };
                    }
                }
                343 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ml.text_style_handle = Some(Handle::new(h));
                    }
                }
                173 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_left_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                95 => {
                    if let Some(v) = pair.as_i32() {
                        ml.text_right_attachment = mlt::TextAttachmentType::from(v as i16);
                    }
                }
                174 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_angle_type = mlt::TextAngleType::from(v);
                    }
                }
                175 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_alignment = mlt::TextAlignmentType::from(v);
                    }
                }
                92 => {
                    if let Some(v) = pair.as_i32() {
                        ml.text_color = color_from_i32(v);
                    }
                }
                292 => {
                    if let Some(v) = pair.as_bool() {
                        ml.text_frame = v;
                    }
                }
                344 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ml.block_content_handle = Some(Handle::new(h));
                    }
                }
                93 => {
                    if let Some(v) = pair.as_i32() {
                        ml.block_content_color = color_from_i32(v);
                    }
                }
                10 | 20 | 30 => {
                    block_scale.add_coordinate(&pair);
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        ml.block_rotation = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        ml.scale_factor = v;
                    }
                }
                176 => {
                    if let Some(v) = pair.as_i16() {
                        ml.block_connection_type = mlt::BlockContentConnectionType::from(v);
                    }
                }
                293 => {
                    if let Some(v) = pair.as_bool() {
                        ml.enable_annotation_scale = v;
                    }
                }
                94 => {
                    if let Some(index) = pair.as_i32() {
                        ml.arrowhead_overrides
                            .push(mlt::MultiLeaderArrowheadOverride {
                                index,
                                is_default: index == 0,
                                arrowhead_handle: None,
                            });
                    }
                }
                345 => {
                    if let Ok(value) = u64::from_str_radix(pair.value_string.trim(), 16) {
                        let entry = ml.arrowhead_overrides.last_mut();
                        if let Some(entry) = entry {
                            entry.arrowhead_handle = (value != 0).then(|| Handle::new(value));
                        }
                    }
                }
                330 if section == "AcDbMLeader" => {
                    if let Some(attribute) = current_block_attribute.take() {
                        ml.block_attributes.push(attribute);
                    }
                    let mut attribute = mlt::BlockAttribute::default();
                    if let Ok(value) = u64::from_str_radix(pair.value_string.trim(), 16) {
                        attribute.attribute_definition_handle =
                            (value != 0).then(|| Handle::new(value));
                    }
                    current_block_attribute = Some(attribute);
                }
                177 if current_block_attribute.is_some() => {
                    if let (Some(attribute), Some(value)) =
                        (current_block_attribute.as_mut(), pair.as_i16())
                    {
                        attribute.index = value;
                    }
                }
                44 if current_block_attribute.is_some() => {
                    if let (Some(attribute), Some(value)) =
                        (current_block_attribute.as_mut(), pair.as_double())
                    {
                        attribute.width = value;
                    }
                }
                302 if current_block_attribute.is_some() => {
                    if let Some(mut attribute) = current_block_attribute.take() {
                        attribute.text = pair.value_string.clone();
                        ml.block_attributes.push(attribute);
                    }
                }
                294 => {
                    if let Some(v) = pair.as_bool() {
                        ml.text_direction_negative = v;
                    }
                }
                178 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_align_in_ipe = v;
                    }
                }
                179 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_attachment_point = mlt::TextAttachmentPointType::from(v);
                    }
                }
                271 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_attachment_direction = mlt::TextAttachmentDirectionType::from(v);
                    }
                }
                272 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_bottom_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                273 => {
                    if let Some(v) = pair.as_i16() {
                        ml.text_top_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                295 => {
                    if let Some(v) = pair.as_bool() {
                        ml.extend_leader_to_text = v;
                    }
                }
                // Lenient extras some exporters emit at entity level.
                44 => {
                    if let Some(v) = pair.as_double() {
                        ml.text_height = v;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut ml.common)?;
                }
            }
        }

        if let Some(s) = block_scale.get_point() {
            ml.block_scale = s;
        }
        if let Some(attribute) = current_block_attribute {
            ml.block_attributes.push(attribute);
        }
        if proxy_graphics_size != 0 || !proxy_graphics.is_empty() {
            proxy_graphics.truncate(proxy_graphics_size);
            ml.common.graphic_data = Some(proxy_graphics);
        }
        Ok(Some(ml))
    }

    /// Read a MULTILEADER `CONTEXT_DATA{ … }` section (up to its closing
    /// `301 }`), including nested `302 LEADER{ … }` roots.
    fn read_mleader_context(
        &mut self,
        ctx: &mut crate::entities::multileader::MultiLeaderAnnotContext,
    ) -> Result<()> {
        use crate::entities::multileader as mlt;
        let mut content_base = PointReader::new();
        let mut text_normal = PointReader::new();
        let mut text_location = PointReader::new();
        let mut text_direction = PointReader::new();
        let mut block_normal = PointReader::new();
        let mut block_location = PointReader::new();
        let mut block_scale = PointReader::new();
        let mut base_point = PointReader::new();
        let mut base_direction = PointReader::new();
        let mut base_vertical = PointReader::new();
        let mut transform_index = 0usize;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                // Malformed: entity ended inside the section.
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                301 => break, // "}"
                302 if pair.value_string.starts_with("LEADER") => {
                    let root = self.read_mleader_leader_root()?;
                    ctx.leader_roots.push(root);
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        ctx.scale_factor = v;
                    }
                }
                10 | 20 | 30 => {
                    content_base.add_coordinate(&pair);
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        ctx.text_height = v;
                    }
                }
                140 => {
                    if let Some(v) = pair.as_double() {
                        ctx.arrowhead_size = v;
                    }
                }
                145 => {
                    if let Some(v) = pair.as_double() {
                        ctx.landing_gap = v;
                    }
                }
                174 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_left_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                175 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_right_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                176 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_alignment = mlt::TextAlignmentType::from(v);
                    }
                }
                177 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.block_connection_type = mlt::BlockContentConnectionType::from(v);
                    }
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.has_text_contents = v;
                    }
                }
                304 => ctx.text_string = pair.value_string.clone(),
                11 | 21 | 31 => {
                    text_normal.add_coordinate(&pair);
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ctx.text_style_handle = Some(Handle::new(h));
                    }
                }
                12 | 22 | 32 => {
                    text_location.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    text_direction.add_coordinate(&pair);
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        ctx.text_rotation = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        ctx.text_width = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        ctx.text_boundary_height = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        ctx.line_spacing_factor = v;
                    }
                }
                170 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.line_spacing_style = crate::entities::LineSpacingStyle::from(v);
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        ctx.text_color = color_from_i32(v);
                    }
                }
                171 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_attachment_point = mlt::TextAttachmentPointType::from(v);
                    }
                }
                172 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_flow_direction = mlt::FlowDirectionType::from(v);
                    }
                }
                91 => {
                    if let Some(v) = pair.as_i32() {
                        ctx.background_fill_color = color_from_i32(v);
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        ctx.background_scale_factor = v;
                    }
                }
                92 => {
                    if let Some(v) = pair.as_i32() {
                        ctx.background_transparency = v;
                    }
                }
                291 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.background_fill_enabled = v;
                    }
                }
                292 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.background_mask_fill_on = v;
                    }
                }
                173 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.column_type = v;
                    }
                }
                293 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.text_height_automatic = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        ctx.column_width = v;
                    }
                }
                143 => {
                    if let Some(v) = pair.as_double() {
                        ctx.column_gutter = v;
                    }
                }
                144 => {
                    if let Some(v) = pair.as_double() {
                        ctx.column_sizes.push(v);
                    }
                }
                294 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.column_flow_reversed = v;
                    }
                }
                295 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.word_break = v;
                    }
                }
                296 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.has_block_contents = v;
                    }
                }
                341 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ctx.block_content_handle = Some(Handle::new(h));
                    }
                }
                14 | 24 | 34 => {
                    block_normal.add_coordinate(&pair);
                }
                15 | 25 | 35 => {
                    block_location.add_coordinate(&pair);
                }
                16 | 26 | 36 => {
                    block_scale.add_coordinate(&pair);
                }
                46 => {
                    if let Some(v) = pair.as_double() {
                        ctx.block_rotation = v;
                    }
                }
                93 => {
                    if let Some(v) = pair.as_i32() {
                        ctx.block_content_color = color_from_i32(v);
                    }
                }
                47 => {
                    if let Some(value) = pair.as_double() {
                        if transform_index < ctx.transform_matrix.len() {
                            ctx.transform_matrix[transform_index] = value;
                            transform_index += 1;
                        }
                    }
                }
                110 | 120 | 130 => {
                    base_point.add_coordinate(&pair);
                }
                111 | 121 | 131 => {
                    base_direction.add_coordinate(&pair);
                }
                112 | 122 | 132 => {
                    base_vertical.add_coordinate(&pair);
                }
                297 => {
                    if let Some(v) = pair.as_bool() {
                        ctx.normal_reversed = v;
                    }
                }
                273 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_top_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                272 => {
                    if let Some(v) = pair.as_i16() {
                        ctx.text_bottom_attachment = mlt::TextAttachmentType::from(v);
                    }
                }
                _ => {}
            }
        }

        if let Some(p) = content_base.get_point() {
            ctx.content_base_point = p;
        }
        if let Some(p) = text_normal.get_point() {
            ctx.text_normal = p;
        }
        if let Some(p) = text_location.get_point() {
            ctx.text_location = p;
        }
        if let Some(p) = text_direction.get_point() {
            ctx.text_direction = p;
        }
        if let Some(p) = block_normal.get_point() {
            ctx.block_content_normal = p;
        }
        if let Some(p) = block_location.get_point() {
            ctx.block_content_location = p;
        }
        if let Some(p) = block_scale.get_point() {
            ctx.block_content_scale = p;
        }
        if let Some(p) = base_point.get_point() {
            ctx.base_point = p;
        }
        if let Some(p) = base_direction.get_point() {
            ctx.base_direction = p;
        }
        if let Some(p) = base_vertical.get_point() {
            ctx.base_vertical = p;
        }
        Ok(())
    }

    /// Read one `LEADER{ … }` root (up to its closing `303 }`), including
    /// nested `304 LEADER_LINE{ … }` lines.
    fn read_mleader_leader_root(&mut self) -> Result<crate::entities::multileader::LeaderRoot> {
        use crate::entities::multileader::{LeaderRoot, StartEndPointPair};
        let mut root = LeaderRoot::new(0);
        let mut connection = PointReader::new();
        let mut direction = PointReader::new();
        // Break start/end pairs arrive as repeated 12/13 triples.
        let mut break_start = PointReader::new();
        let mut break_end = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                303 => break, // "}"
                304 if pair.value_string.starts_with("LEADER_LINE") => {
                    let line = self.read_mleader_leader_line(root.lines.len() as i32)?;
                    root.lines.push(line);
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        root.content_valid = v;
                    }
                }
                291 => {
                    if let Some(v) = pair.as_bool() {
                        root.unknown = v;
                    }
                }
                10 | 20 | 30 => {
                    connection.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    direction.add_coordinate(&pair);
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        root.leader_index = v;
                    }
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        root.landing_distance = v;
                    }
                }
                12 => {
                    if let (Some(start), Some(end)) =
                        (break_start.get_point(), break_end.get_point())
                    {
                        root.break_points.push(StartEndPointPair::new(start, end));
                        break_start.reset();
                        break_end.reset();
                    }
                    break_start.add_coordinate(&pair);
                }
                22 | 32 => {
                    break_start.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    break_end.add_coordinate(&pair);
                }
                271 => {
                    if let Some(v) = pair.as_i16() {
                        root.text_attachment_direction =
                            crate::entities::multileader::TextAttachmentDirectionType::from(v);
                    }
                }
                _ => {}
            }
        }

        if let Some(p) = connection.get_point() {
            root.connection_point = p;
        }
        if let Some(p) = direction.get_point() {
            root.direction = p;
        }
        if let (Some(s), Some(e)) = (break_start.get_point(), break_end.get_point()) {
            root.break_points.push(StartEndPointPair::new(s, e));
        }
        Ok(root)
    }

    /// Read one `LEADER_LINE{ … }` (up to its closing `305 }`). The vertex
    /// list arrives as repeated 10/20/30 triples.
    fn read_mleader_leader_line(
        &mut self,
        index: i32,
    ) -> Result<crate::entities::multileader::LeaderLine> {
        use crate::entities::multileader as mlt;
        let mut line = mlt::LeaderLine::from_points(index, Vec::new());
        let mut break_start = PointReader::new();
        let mut break_end = PointReader::new();
        let mut current_break_segment: Option<i32> = None;
        let mut current_break_points = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                305 => break, // "}"
                10 => {
                    if let Some(v) = pair.as_double() {
                        line.points.push(Vector3::new(v, 0.0, 0.0));
                    }
                }
                20 => {
                    if let (Some(v), Some(p)) = (pair.as_double(), line.points.last_mut()) {
                        p.y = v;
                    }
                }
                30 => {
                    if let (Some(v), Some(p)) = (pair.as_double(), line.points.last_mut()) {
                        p.z = v;
                    }
                }
                90 => {
                    if let Some(value) = pair.as_i32() {
                        if let (Some(start), Some(end)) =
                            (break_start.get_point(), break_end.get_point())
                        {
                            current_break_points.push(mlt::StartEndPointPair::new(start, end));
                        }
                        break_start.reset();
                        break_end.reset();
                        if let Some(segment_index) = current_break_segment.take() {
                            line.break_infos.push(mlt::LeaderLineBreakInfo {
                                segment_index,
                                break_points: std::mem::take(&mut current_break_points),
                            });
                        }
                        current_break_segment = Some(value);
                    }
                }
                11 => {
                    current_break_segment.get_or_insert(0);
                    if let (Some(start), Some(end)) =
                        (break_start.get_point(), break_end.get_point())
                    {
                        current_break_points.push(mlt::StartEndPointPair::new(start, end));
                        break_start.reset();
                        break_end.reset();
                    }
                    break_start.add_coordinate(&pair);
                }
                21 | 31 => {
                    break_start.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    break_end.add_coordinate(&pair);
                }
                91 => {
                    if let Some(v) = pair.as_i32() {
                        line.index = v;
                    }
                }
                170 => {
                    if let Some(v) = pair.as_i16() {
                        line.path_type = mlt::MultiLeaderPathType::from(v);
                    }
                }
                92 => {
                    if let Some(v) = pair.as_i32() {
                        line.line_color = color_from_i32(v);
                    }
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        if h != 0 {
                            line.line_type_handle = Some(Handle::new(h));
                        }
                    }
                }
                171 => {
                    if let Some(v) = pair.as_i16() {
                        line.line_weight = LineWeight::from_value(v);
                    }
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        line.arrowhead_size = v;
                    }
                }
                341 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        if h != 0 {
                            line.arrowhead_handle = Some(Handle::new(h));
                        }
                    }
                }
                93 => {
                    if let Some(v) = pair.as_i32() {
                        line.override_flags =
                            mlt::LeaderLinePropertyOverrideFlags::from_bits_retain(v as u32);
                    }
                }
                _ => {}
            }
        }
        if let (Some(start), Some(end)) = (break_start.get_point(), break_end.get_point()) {
            current_break_points.push(mlt::StartEndPointPair::new(start, end));
        }
        if let Some(segment_index) = current_break_segment {
            line.break_infos.push(mlt::LeaderLineBreakInfo {
                segment_index,
                break_points: current_break_points,
            });
        }
        line.break_info_count = line.break_infos.len() as i32;
        if let Some(info) = line.break_infos.first() {
            line.segment_index = info.segment_index;
            line.break_points = info.break_points.clone();
        }
        Ok(line)
    }

    /// Read an MLINE entity
    fn read_mline(&mut self) -> Result<Option<MLine>> {
        use crate::entities::mline::*;
        let mut mline = MLine::new();
        let mut start_point = PointReader::new();
        let mut normal = PointReader::new();
        let mut current_vertex_pos = PointReader::new();
        let mut current_vertex_dir = PointReader::new();
        let mut current_vertex_miter = PointReader::new();
        let mut vertices: Vec<MLineVertex> = Vec::new();
        let mut reading_vertices = false;
        let mut num_elements = 0usize;
        let mut current_segments: Vec<MLineSegment> = Vec::new();
        let mut current_params: Vec<f64> = Vec::new();
        let mut current_area_fill: Vec<f64> = Vec::new();
        let mut reading_params = false;
        let mut reading_area_fill = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => mline.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        mline.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        mline.common.line_weight = LineWeight::from_value(v);
                    }
                }
                2 => mline.style_name = pair.value_string.clone(),
                40 => {
                    if let Some(v) = pair.as_double() {
                        mline.scale_factor = v;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        mline.justification = MLineJustification::from(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        mline.flags = MLineFlags::from_bits_truncate(v);
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        num_elements = v as usize;
                    }
                }
                10 | 20 | 30 => {
                    start_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        mline.style_handle = Some(Handle::new(h));
                    }
                }
                11 => {
                    // New vertex – save previous if any
                    if reading_vertices {
                        self.finalize_mline_vertex(
                            &mut vertices,
                            &current_vertex_pos,
                            &current_vertex_dir,
                            &current_vertex_miter,
                            &mut current_segments,
                            &mut current_params,
                            &mut current_area_fill,
                            &mut reading_params,
                            &mut reading_area_fill,
                        );
                    }
                    current_vertex_pos = PointReader::new();
                    current_vertex_pos.add_coordinate(&pair);
                    current_vertex_dir = PointReader::new();
                    current_vertex_miter = PointReader::new();
                    current_segments = Vec::new();
                    current_params = Vec::new();
                    current_area_fill = Vec::new();
                    reading_params = false;
                    reading_area_fill = false;
                    reading_vertices = true;
                }
                21 | 31 => {
                    current_vertex_pos.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    current_vertex_dir.add_coordinate(&pair);
                }
                13 | 23 | 33 => {
                    current_vertex_miter.add_coordinate(&pair);
                }
                74 => {
                    // Number of parameters for this element
                    if reading_params {
                        current_segments.push(MLineSegment {
                            parameters: std::mem::take(&mut current_params),
                            area_fill_parameters: std::mem::take(&mut current_area_fill),
                        });
                    }
                    reading_params = true;
                    reading_area_fill = false;
                    current_params = Vec::new();
                    current_area_fill = Vec::new();
                }
                75 => {
                    reading_area_fill = true;
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        if reading_area_fill {
                            current_area_fill.push(v);
                        } else if reading_params {
                            current_params.push(v);
                        }
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        current_area_fill.push(v);
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut mline.common)?;
                }
            }
        }

        // Finalize last vertex
        if reading_vertices {
            self.finalize_mline_vertex(
                &mut vertices,
                &current_vertex_pos,
                &current_vertex_dir,
                &current_vertex_miter,
                &mut current_segments,
                &mut current_params,
                &mut current_area_fill,
                &mut reading_params,
                &mut reading_area_fill,
            );
        }

        if let Some(pt) = start_point.get_point() {
            mline.start_point = pt;
        }
        if let Some(pt) = normal.get_point() {
            mline.normal = pt;
        }
        mline.vertices = vertices;
        mline.style_element_count = num_elements;

        Ok(Some(mline))
    }

    fn finalize_mline_vertex(
        &self,
        vertices: &mut Vec<crate::entities::mline::MLineVertex>,
        pos: &PointReader,
        dir: &PointReader,
        miter: &PointReader,
        segments: &mut Vec<crate::entities::mline::MLineSegment>,
        params: &mut Vec<f64>,
        area_fill: &mut Vec<f64>,
        reading_params: &mut bool,
        _reading_area_fill: &mut bool,
    ) {
        use crate::entities::mline::*;
        if *reading_params {
            segments.push(MLineSegment {
                parameters: std::mem::take(params),
                area_fill_parameters: std::mem::take(area_fill),
            });
        }
        vertices.push(MLineVertex {
            position: pos.get_point().unwrap_or(Vector3::zero()),
            direction: dir.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0)),
            miter: miter.get_point().unwrap_or(Vector3::new(0.0, 1.0, 0.0)),
            segments: std::mem::take(segments),
        });
        *reading_params = false;
    }

    /// Read a MESH entity
    fn read_mesh(&mut self) -> Result<Option<Mesh>> {
        use crate::entities::mesh::*;
        let mut mesh = Mesh::new();
        let mut reading_state = MeshReadState::Properties;
        let mut vertex_count = 0usize;
        let mut _face_count = 0usize;
        let mut _edge_count = 0usize;
        let mut _crease_count = 0usize;
        let mut current_vertex = PointReader::new();
        let mut face_indices: Vec<usize> = Vec::new();
        let mut face_subcount: Option<usize> = None;
        let mut edge_buf: Vec<usize> = Vec::new();
        let mut crease_values: Vec<f64> = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => mesh.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        mesh.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        mesh.common.line_weight = LineWeight::from_value(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        mesh.version = v;
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        mesh.blend_crease = v != 0;
                    }
                }
                91 => match reading_state {
                    MeshReadState::Properties => {
                        if let Some(v) = pair.as_i32() {
                            mesh.subdivision_level = v;
                        }
                    }
                    _ => {}
                },
                92 => {
                    if let Some(v) = pair.as_i32() {
                        vertex_count = v as usize;
                        reading_state = MeshReadState::Vertices;
                    }
                }
                93 => {
                    if let Some(v) = pair.as_i32() {
                        _face_count = v as usize;
                        reading_state = MeshReadState::Faces;
                    }
                }
                94 => {
                    if let Some(v) = pair.as_i32() {
                        _edge_count = v as usize;
                        reading_state = MeshReadState::Edges;
                    }
                }
                95 => {
                    if let Some(v) = pair.as_i32() {
                        _crease_count = v as usize;
                        reading_state = MeshReadState::Creases;
                    }
                }
                10 | 20 | 30 => {
                    if reading_state == MeshReadState::Vertices {
                        current_vertex.add_coordinate(&pair);
                        if pair.code == 30 {
                            if let Some(pt) = current_vertex.get_point() {
                                mesh.vertices.push(pt);
                                current_vertex = PointReader::new();
                            }
                        }
                    }
                }
                90 => {
                    if let Some(v) = pair.as_i32() {
                        match reading_state {
                            MeshReadState::Faces => {
                                if face_subcount.is_none() {
                                    face_subcount = Some(v as usize);
                                } else {
                                    face_indices.push(v as usize);
                                    if face_indices.len() == face_subcount.unwrap() {
                                        mesh.faces.push(MeshFace {
                                            vertices: std::mem::take(&mut face_indices),
                                        });
                                        face_subcount = None;
                                    }
                                }
                            }
                            MeshReadState::Edges => {
                                edge_buf.push(v as usize);
                                if edge_buf.len() == 2 {
                                    mesh.edges.push(MeshEdge {
                                        start: edge_buf[0],
                                        end: edge_buf[1],
                                        crease: None,
                                    });
                                    edge_buf.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                140 => {
                    if reading_state == MeshReadState::Creases {
                        if let Some(v) = pair.as_double() {
                            crease_values.push(v);
                        }
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut mesh.common)?;
                }
            }
        }

        // Apply crease values to edges. A zero crease is the "no crease"
        // default, so leave those edges as None to match the DWG reader
        // (which reports None for every sharp edge); only a real, non-zero
        // crease is attached.
        for (i, crease) in crease_values.into_iter().enumerate() {
            if crease != 0.0 {
                if let Some(edge) = mesh.edges.get_mut(i) {
                    edge.crease = Some(crease);
                }
            }
        }

        let _ = vertex_count;

        Ok(Some(mesh))
    }

    /// Read an IMAGE entity
    fn read_raster_image(&mut self) -> Result<Option<RasterImage>> {
        let mut img = RasterImage::new("", Vector3::zero(), 1.0, 1.0);
        let mut insertion_point = PointReader::new();
        let mut u_vector = PointReader::new();
        let mut v_vector = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => img.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        img.common.color = Color::from_index(v);
                    }
                }
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    u_vector.add_coordinate(&pair);
                }
                12 | 22 | 32 => {
                    v_vector.add_coordinate(&pair);
                }
                13 => {
                    if let Some(v) = pair.as_double() {
                        img.size.x = v;
                    }
                }
                23 => {
                    if let Some(v) = pair.as_double() {
                        img.size.y = v;
                    }
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        img.definition_handle = Some(Handle::new(h));
                    }
                }
                360 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        img.definition_reactor_handle = Some(Handle::new(h));
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        img.flags = ImageDisplayFlags::from_bits_truncate(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        img.clip_boundary.clip_type = ClipType::from(v);
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        img.clipping_enabled = v != 0;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        img.brightness = v as u8;
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        img.contrast = v as u8;
                    }
                }
                283 => {
                    if let Some(v) = pair.as_i16() {
                        img.fade = v as u8;
                    }
                }
                290 => {
                    // Clip mode (0 = outside, 1 = inside). Bool-typed group code.
                    if let Some(v) = pair.as_bool() {
                        img.clip_boundary.clip_mode = if v {
                            ClipMode::Inside
                        } else {
                            ClipMode::Outside
                        };
                    }
                }
                91 => {
                    // Clip boundary vertex count precedes the 14/24 pairs; drop
                    // the default-seeded corners so file vertices don't append.
                    img.clip_boundary.vertices.clear();
                }
                14 => {
                    if let Some(x) = pair.as_double() {
                        img.clip_boundary.vertices.push(Vector2::new(x, 0.0));
                    }
                }
                24 => {
                    if let Some(y) = pair.as_double() {
                        if let Some(last) = img.clip_boundary.vertices.last_mut() {
                            last.y = y;
                        }
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut img.common)?;
                }
            }
        }

        img.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        img.u_vector = u_vector.get_point().unwrap_or(Vector3::new(1.0, 0.0, 0.0));
        img.v_vector = v_vector.get_point().unwrap_or(Vector3::new(0.0, 1.0, 0.0));

        Ok(Some(img))
    }

    /// Read modeler geometry (ACIS) data — shared between 3DSOLID, REGION, BODY
    fn read_modeler_geometry(&mut self) -> Result<(EntityCommon, String, String, Option<Handle>)> {
        let mut common = EntityCommon::new();
        let mut acis_data = String::new();
        let mut uid = String::new();
        let mut history_handle = None;
        let mut acis_version: u8 = 1; // default to Version 1 (encoded)

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        common.line_weight = LineWeight::from_value(v);
                    }
                }
                1 | 3 => {
                    acis_data.push_str(&pair.value_string);
                    acis_data.push('\n');
                }
                2 => uid = pair.value_string.clone(),
                350 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    history_handle = (!handle.is_null()).then_some(handle);
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        acis_version = v as u8;
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut common)?;
                }
            }
        }

        // Version 1: SAT data is stored with a character cipher — decode it.
        if acis_version == 1 && !acis_data.is_empty() {
            acis_data = crate::entities::solid3d::AcisData::decode_sat(&acis_data);
        }

        // Normalise: strip "End-of-ACIS-data" / "End-of-ASM-data" terminator.
        acis_data = crate::entities::solid3d::AcisData::strip_sat_terminator(&acis_data);

        Ok((common, uid, acis_data, history_handle))
    }

    /// Read a 3DSOLID entity
    fn read_solid3d(&mut self) -> Result<Option<Solid3D>> {
        let (common, uid, acis_data, history_handle) = self.read_modeler_geometry()?;
        let mut solid = Solid3D::new();
        solid.common = common;
        solid.uid = uid;
        solid.acis_data.sat_data = acis_data;
        solid.history_handle = history_handle;
        Ok(Some(solid))
    }

    /// Read a REGION entity
    fn read_region(&mut self) -> Result<Option<Region>> {
        let (common, _uid, acis_data, history_handle) = self.read_modeler_geometry()?;
        let mut region = Region::new();
        region.common = common;
        region.acis_data.sat_data = acis_data;
        region.history_handle = history_handle;
        Ok(Some(region))
    }

    /// Read a BODY entity
    fn read_body(&mut self) -> Result<Option<Body>> {
        let (common, _uid, acis_data, history_handle) = self.read_modeler_geometry()?;
        let mut body = Body::new();
        body.common = common;
        body.acis_data.sat_data = acis_data;
        body.history_handle = history_handle;
        Ok(Some(body))
    }

    fn read_light_entity(&mut self) -> Result<Option<Light>> {
        let mut light = Light::new();
        let mut position = PointReader::new();
        let mut target = PointReader::new();
        let mut web_rotation = PointReader::new();
        let mut photo = LightPhotometricData::default();
        let mut in_photometric = false;
        let mut subclass = String::new();
        let mut proxy_graphics_size = 0usize;
        let mut proxy_graphics = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string.clone();
                continue;
            }
            match pair.code {
                92 | 160 if subclass == "AcDbEntity" => {
                    proxy_graphics_size = pair.as_int().unwrap_or(0).max(0) as usize;
                }
                310 if subclass == "AcDbEntity" => {
                    append_hex_bytes(&mut proxy_graphics, &pair.value_string);
                }
                1 if !in_photometric => light.name = pair.value_string.clone(),
                90 if !in_photometric => {
                    light.class_version = pair.as_i32().unwrap_or(light.class_version)
                }
                70 if !in_photometric => {
                    light.light_type = pair.as_i32().unwrap_or(light.light_type)
                }
                290 if !in_photometric => light.status = pair.as_i16().unwrap_or(0) != 0,
                63 if !in_photometric => {
                    light.light_color = Color::from_index(pair.as_i16().unwrap_or(256))
                }
                421 => light.light_color = Color::from_true_color_value(pair.as_i32().unwrap_or(0)),
                291 => light.plot_glyph = pair.as_i16().unwrap_or(0) != 0,
                40 if !in_photometric => {
                    light.intensity = pair.as_double().unwrap_or(light.intensity)
                }
                10 | 20 | 30 => {
                    position.add_coordinate(&pair);
                }
                11 | 21 | 31 => {
                    target.add_coordinate(&pair);
                }
                72 if !in_photometric => {
                    light.attenuation_type = pair.as_i32().unwrap_or(light.attenuation_type)
                }
                292 => light.use_attenuation_limits = pair.as_i16().unwrap_or(0) != 0,
                41 if !in_photometric => {
                    light.attenuation_start_limit =
                        pair.as_double().unwrap_or(light.attenuation_start_limit)
                }
                42 if !in_photometric => {
                    light.attenuation_end_limit =
                        pair.as_double().unwrap_or(light.attenuation_end_limit)
                }
                50 if !in_photometric => {
                    light.hotspot_angle = pair.as_double().unwrap_or(light.hotspot_angle)
                }
                51 if !in_photometric => {
                    light.falloff_angle = pair.as_double().unwrap_or(light.falloff_angle)
                }
                293 => light.cast_shadows = pair.as_i16().unwrap_or(0) != 0,
                73 if !in_photometric => {
                    light.shadow_type = pair.as_i32().unwrap_or(light.shadow_type)
                }
                91 => light.shadow_map_size = pair.as_i16().unwrap_or(light.shadow_map_size),
                280 if !in_photometric => {
                    light.shadow_map_softness = pair.as_i16().unwrap_or(0).clamp(0, 255) as u8
                }
                295 => {
                    in_photometric = true;
                    light.photometric_mode = true;
                }
                290 if in_photometric => photo.has_web_file = pair.as_i16().unwrap_or(0) != 0,
                300 if in_photometric => photo.web_file = pair.value_string.clone(),
                70 if in_photometric => {
                    photo.physical_intensity_method = pair.as_i16().unwrap_or(0)
                }
                40 if in_photometric => photo.physical_intensity = pair.as_double().unwrap_or(0.0),
                41 if in_photometric => {
                    photo.illuminance_distance = pair.as_double().unwrap_or(0.0)
                }
                71 if in_photometric => photo.lamp_color_type = pair.as_i16().unwrap_or(0),
                42 if in_photometric => {
                    photo.lamp_color_temperature = pair.as_double().unwrap_or(0.0)
                }
                72 if in_photometric => photo.lamp_color_preset = pair.as_i16().unwrap_or(0),
                43 | 53 | 63 if in_photometric => {
                    web_rotation.add_coordinate(&pair);
                }
                73 if in_photometric => photo.extended_light_shape = pair.as_i16().unwrap_or(0),
                46 if in_photometric => {
                    photo.extended_light_length = pair.as_double().unwrap_or(0.0)
                }
                47 if in_photometric => {
                    photo.extended_light_width = pair.as_double().unwrap_or(0.0)
                }
                48 if in_photometric => {
                    photo.extended_light_radius = pair.as_double().unwrap_or(0.0)
                }
                74 if in_photometric => photo.web_file_type = pair.as_i16().unwrap_or(0),
                75 if in_photometric => photo.web_symmetry = pair.as_i16().unwrap_or(0),
                76 if in_photometric => photo.has_target_grip = pair.as_i16().unwrap_or(0),
                49 if in_photometric => photo.web_flux = pair.as_double().unwrap_or(0.0),
                50..=54 if in_photometric => {
                    photo.web_angles[(pair.code - 50) as usize] = pair.as_double().unwrap_or(0.0)
                }
                77 if in_photometric => photo.glyph_display_type = pair.as_i16().unwrap_or(0),
                _ => {
                    self.try_read_common_entity_code(&pair, &mut light.common)?;
                }
            }
        }

        light.position = position.get_point().unwrap_or(Vector3::ZERO);
        light.target = target.get_point().unwrap_or(Vector3::ZERO);
        if in_photometric {
            photo.web_rotation = web_rotation
                .get_point()
                .unwrap_or(Vector3::new(1.0, 1.0, 1.0));
            light.photometric_data = Some(photo);
        }
        if proxy_graphics_size != 0 || !proxy_graphics.is_empty() {
            proxy_graphics.truncate(proxy_graphics_size);
            light.common.graphic_data = Some(proxy_graphics);
        }
        Ok(Some(light))
    }

    fn read_surface_entity(
        &mut self,
        entity_name: &str,
        dxf_version: DxfVersion,
    ) -> Result<Option<Surface>> {
        let kind = SurfaceKind::from_dxf_name(entity_name);
        let mut surface = Surface::new(kind);
        let mut subclass = String::new();
        let mut acis_text = String::new();
        let mut acis_version = 1i16;
        let mut point_10 = PointReader::new();
        let mut point_11 = PointReader::new();
        let mut point_12 = PointReader::new();
        let mut point_13 = PointReader::new();
        let mut matrix = Vec::new();
        let mut path_matrix = Vec::new();
        let mut option_sweep_matrix = Vec::new();
        let mut option_path_matrix = Vec::new();
        let mut sweep_data = Vec::new();
        let mut path_data = Vec::new();
        let mut swept_binary_target = 0u8;
        let mut swept_class_version_seen = false;
        let mut sweep_entity_type = 0i32;
        let mut sweep_entity_bits = 0usize;
        let mut path_entity_type = 0i32;
        let mut path_entity_bits = 0usize;
        // Native loft input records use 90/91/92 for section/guide/path
        // entity types, then 90 for bit length and 310 for body chunks.
        let mut loft_entities: Vec<(i32, i32, usize, Vec<u8>)> = Vec::new();
        let mut loft_expecting_length = false;
        let mut loft_options_seen = false;
        let mut proxy_graphics_size = 0usize;
        let mut proxy_graphics = Vec::new();
        let swept_has_class_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
            .map(|version| version.r2007_plus())
            .unwrap_or(true);

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                subclass = pair.value_string.clone();
                continue;
            }
            if subclass == "AcDbEntity" {
                match pair.code {
                    92 | 160 => {
                        proxy_graphics_size = pair.as_int().unwrap_or(0).max(0) as usize;
                    }
                    310 => {
                        append_hex_bytes(&mut proxy_graphics, &pair.value_string);
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                }
                continue;
            }
            if subclass == "AcDbModelerGeometry" {
                match pair.code {
                    1 | 3 => {
                        acis_text.push_str(&pair.value_string);
                        acis_text.push('\n');
                    }
                    70 => acis_version = pair.as_i16().unwrap_or(1),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                }
                continue;
            }
            if subclass == "AcDbSurface" {
                match pair.code {
                    71 => surface.u_isolines = pair.as_i16().unwrap_or(0),
                    72 => surface.v_isolines = pair.as_i16().unwrap_or(0),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                }
                continue;
            }

            match &mut surface.surface_data {
                SurfaceData::Generic => {
                    self.try_read_common_entity_code(&pair, &mut surface.common)?;
                }
                SurfaceData::Plane { class_version } => match pair.code {
                    90 => *class_version = pair.as_i32().unwrap_or(0),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
                SurfaceData::Extruded {
                    sweep_entity: _,
                    options,
                    sweep_vector: _,
                    sweep_transform: _,
                } => match pair.code {
                    90 if swept_binary_target == 0 => {
                        sweep_entity_type = pair.as_i32().unwrap_or(0);
                        swept_binary_target = 1;
                    }
                    90 => {
                        sweep_entity_bits = pair.as_i32().unwrap_or(0).max(0) as usize;
                        swept_binary_target = 2;
                    }
                    310 => append_hex_bytes(&mut sweep_data, &pair.value_string),
                    10 | 20 | 30 => {
                        point_10.add_coordinate(&pair);
                    }
                    11 | 21 | 31 => {
                        point_11.add_coordinate(&pair);
                    }
                    40 => matrix.push(pair.as_double().unwrap_or(0.0)),
                    42 => options.draft_angle = pair.as_double().unwrap_or(0.0),
                    43 => options.draft_start_distance = pair.as_double().unwrap_or(0.0),
                    44 => options.draft_end_distance = pair.as_double().unwrap_or(0.0),
                    45 => options.twist_angle = pair.as_double().unwrap_or(0.0),
                    48 => options.scale_factor = pair.as_double().unwrap_or(1.0),
                    49 => options.align_angle = pair.as_double().unwrap_or(0.0),
                    46 => option_sweep_matrix.push(pair.as_double().unwrap_or(0.0)),
                    47 => option_path_matrix.push(pair.as_double().unwrap_or(0.0)),
                    290 => options.is_solid = pair.as_bool().unwrap_or(false),
                    70 => options.sweep_alignment_flags = pair.as_i16().unwrap_or(0),
                    71 => options.path_flags = pair.as_i16().unwrap_or(0),
                    292 => options.align_start = pair.as_bool().unwrap_or(false),
                    293 => options.bank = pair.as_bool().unwrap_or(false),
                    294 => options.base_point_set = pair.as_bool().unwrap_or(false),
                    295 => {
                        options.sweep_entity_transform_computed = pair.as_bool().unwrap_or(false)
                    }
                    296 => options.path_entity_transform_computed = pair.as_bool().unwrap_or(false),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
                SurfaceData::Lofted {
                    plane_normal_lofting_type,
                    start_draft_angle,
                    end_draft_angle,
                    start_draft_magnitude,
                    end_draft_magnitude,
                    arc_length_parameterization,
                    no_twist,
                    align_direction,
                    simple_surfaces,
                    closed_surfaces,
                    solid,
                    ruled_surface,
                    virtual_guide,
                    cross_sections,
                    guide_curves,
                    path_curve,
                    ..
                } => match pair.code {
                    40 => matrix.push(pair.as_double().unwrap_or(0.0)),
                    90 if loft_expecting_length => {
                        if let Some((_, _, bits, _)) = loft_entities.last_mut() {
                            *bits = pair.as_i32().unwrap_or(0).max(0) as usize;
                        }
                        loft_expecting_length = false;
                    }
                    90..=92 if !loft_options_seen => {
                        loft_entities.push((pair.code, pair.as_i32().unwrap_or(0), 0, Vec::new()));
                        loft_expecting_length = true;
                    }
                    310 if !loft_options_seen => {
                        if let Some((_, _, _, bytes)) = loft_entities.last_mut() {
                            append_hex_bytes(bytes, &pair.value_string);
                        }
                    }
                    70 => {
                        loft_options_seen = true;
                        *plane_normal_lofting_type = pair.as_i32().unwrap_or(0)
                    }
                    41 => *start_draft_angle = pair.as_double().unwrap_or(0.0),
                    42 => *end_draft_angle = pair.as_double().unwrap_or(0.0),
                    43 => *start_draft_magnitude = pair.as_double().unwrap_or(0.0),
                    44 => *end_draft_magnitude = pair.as_double().unwrap_or(0.0),
                    290 => *arc_length_parameterization = pair.as_i16().unwrap_or(0) != 0,
                    291 => *no_twist = pair.as_i16().unwrap_or(0) != 0,
                    292 => *align_direction = pair.as_i16().unwrap_or(0) != 0,
                    293 => *simple_surfaces = pair.as_i16().unwrap_or(0) != 0,
                    294 => *closed_surfaces = pair.as_i16().unwrap_or(0) != 0,
                    295 => *solid = pair.as_i16().unwrap_or(0) != 0,
                    296 => *ruled_surface = pair.as_i16().unwrap_or(0) != 0,
                    297 => *virtual_guide = pair.as_i16().unwrap_or(0) != 0,
                    310 => {
                        // Older codec output put ungrouped handles after the
                        // options. Preserve them, but never interpret native
                        // embedded-entity chunks as database references.
                        let handle = parse_dxf_handle(&pair.value_string);
                        if path_curve.is_none() {
                            cross_sections.push(handle);
                        } else {
                            guide_curves.push(handle);
                        }
                    }
                    350 => *path_curve = Some(parse_dxf_handle(&pair.value_string)),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
                SurfaceData::Revolved {
                    revolve_entity: _,
                    class_version: _,
                    entity_id: _,
                    axis_point: _,
                    axis_vector: _,
                    revolve_angle,
                    start_angle,
                    draft_angle,
                    draft_start_distance,
                    draft_end_distance,
                    twist_angle,
                    solid,
                    close_to_axis,
                    ..
                } => match pair.code {
                    90 if swept_binary_target == 0 => {
                        sweep_entity_type = pair.as_i32().unwrap_or(0);
                        swept_binary_target = 1;
                    }
                    90 => {
                        sweep_entity_bits = pair.as_i32().unwrap_or(0).max(0) as usize;
                        swept_binary_target = 2;
                    }
                    310 => append_hex_bytes(&mut sweep_data, &pair.value_string),
                    10 | 20 | 30 => {
                        point_10.add_coordinate(&pair);
                    }
                    11 | 21 | 31 => {
                        point_11.add_coordinate(&pair);
                    }
                    40 => *revolve_angle = pair.as_double().unwrap_or(0.0),
                    41 => *start_angle = pair.as_double().unwrap_or(0.0),
                    42 => matrix.push(pair.as_double().unwrap_or(0.0)),
                    43 => *draft_angle = pair.as_double().unwrap_or(0.0),
                    44 => *draft_start_distance = pair.as_double().unwrap_or(0.0),
                    45 => *draft_end_distance = pair.as_double().unwrap_or(0.0),
                    46 => *twist_angle = pair.as_double().unwrap_or(0.0),
                    290 => *solid = pair.as_i16().unwrap_or(0) != 0,
                    291 => *close_to_axis = pair.as_i16().unwrap_or(0) != 0,
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
                SurfaceData::Swept {
                    class_version,
                    sweep_entity: _,
                    path_entity: _,
                    sweep_transform: _,
                    path_transform: _,
                    options,
                } => match pair.code {
                    90 if swept_has_class_version && !swept_class_version_seen => {
                        *class_version = pair.as_i32().unwrap_or(0);
                        swept_class_version_seen = true;
                    }
                    90 if swept_binary_target == 0 => {
                        sweep_entity_type = pair.as_i32().unwrap_or(0);
                        swept_binary_target = 1;
                    }
                    90 if swept_binary_target == 1 => {
                        sweep_entity_bits = pair.as_i32().unwrap_or(0).max(0) as usize;
                        swept_binary_target = 2;
                    }
                    90 if swept_binary_target == 2 => {
                        path_entity_type = pair.as_i32().unwrap_or(0);
                        swept_binary_target = 3;
                    }
                    91 => {
                        path_entity_type = pair.as_i32().unwrap_or(0);
                        swept_binary_target = 3;
                    }
                    90 => {
                        path_entity_bits = pair.as_i32().unwrap_or(0).max(0) as usize;
                        swept_binary_target = 4;
                    }
                    310 if swept_binary_target == 2 => {
                        append_hex_bytes(&mut sweep_data, &pair.value_string)
                    }
                    310 => append_hex_bytes(&mut path_data, &pair.value_string),
                    40 => matrix.push(pair.as_double().unwrap_or(0.0)),
                    41 => path_matrix.push(pair.as_double().unwrap_or(0.0)),
                    11 | 21 | 31 => {
                        point_11.add_coordinate(&pair);
                    }
                    42 => options.draft_angle = pair.as_double().unwrap_or(0.0),
                    43 => options.draft_start_distance = pair.as_double().unwrap_or(0.0),
                    44 => options.draft_end_distance = pair.as_double().unwrap_or(0.0),
                    45 => options.twist_angle = pair.as_double().unwrap_or(0.0),
                    48 => options.scale_factor = pair.as_double().unwrap_or(1.0),
                    49 => options.align_angle = pair.as_double().unwrap_or(0.0),
                    46 => option_sweep_matrix.push(pair.as_double().unwrap_or(0.0)),
                    47 => option_path_matrix.push(pair.as_double().unwrap_or(0.0)),
                    290 => options.is_solid = pair.as_bool().unwrap_or(false),
                    70 => options.sweep_alignment_flags = pair.as_i16().unwrap_or(0),
                    71 => options.path_flags = pair.as_i16().unwrap_or(0),
                    292 => options.align_start = pair.as_bool().unwrap_or(false),
                    293 => options.bank = pair.as_bool().unwrap_or(false),
                    294 => options.base_point_set = pair.as_bool().unwrap_or(false),
                    295 => {
                        options.sweep_entity_transform_computed = pair.as_bool().unwrap_or(false)
                    }
                    296 => options.path_entity_transform_computed = pair.as_bool().unwrap_or(false),
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
                SurfaceData::Nurb {
                    short_170,
                    cv_hull_display,
                    ..
                } => match pair.code {
                    170 => *short_170 = pair.as_i16().unwrap_or(0),
                    290 => *cv_hull_display = pair.as_i16().unwrap_or(0) != 0,
                    10 | 20 | 30 => {
                        point_10.add_coordinate(&pair);
                    }
                    11 | 21 | 31 => {
                        point_11.add_coordinate(&pair);
                    }
                    12 | 22 | 32 => {
                        point_12.add_coordinate(&pair);
                    }
                    13 | 23 | 33 => {
                        point_13.add_coordinate(&pair);
                    }
                    _ => {
                        self.try_read_common_entity_code(&pair, &mut surface.common)?;
                    }
                },
            }
        }

        if acis_version == 1 && !acis_text.is_empty() {
            acis_text = AcisData::decode_sat(&acis_text);
        }
        surface.acis_data.sat_data = AcisData::strip_sat_terminator(&acis_text);
        surface.acis_data.version = if acis_version == 2 {
            AcisVersion::Version2
        } else {
            AcisVersion::Version1
        };

        let fill_matrix = |target: &mut [f64; 16], values: &[f64]| {
            for (to, from) in target.iter_mut().zip(values.iter()) {
                *to = *from;
            }
        };
        match &mut surface.surface_data {
            SurfaceData::Extruded {
                sweep_entity,
                options,
                sweep_vector,
                sweep_transform,
            } => {
                let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                    .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                *sweep_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
                    sweep_entity_type,
                    sweep_entity_bits,
                    sweep_data,
                    dwg_version,
                    dxf_version,
                );
                *sweep_vector = point_10.get_point().unwrap_or(Vector3::ZERO);
                options.reference_vector = point_11.get_point().unwrap_or(Vector3::UNIT_Z);
                fill_matrix(sweep_transform, &matrix);
                fill_matrix(&mut options.sweep_entity_transform, &option_sweep_matrix);
                fill_matrix(&mut options.path_entity_transform, &option_path_matrix);
            }
            SurfaceData::Lofted {
                loft_transform,
                cross_section_entities,
                guide_entities,
                path_entity,
                cross_sections,
                guide_curves,
                path_curve,
                ..
            } => {
                fill_matrix(loft_transform, &matrix);
                let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                    .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                for (group, entity_type, bit_length, bytes) in loft_entities {
                    if bytes.len() < bit_length.div_ceil(8) {
                        continue;
                    }
                    if let Some(entity) = crate::io::dwg::embedded_entity::decode_embedded_entity(
                        entity_type,
                        bit_length,
                        bytes,
                        dwg_version,
                        dxf_version,
                    ) {
                        match group {
                            90 => cross_section_entities.push(entity),
                            91 => guide_entities.push(entity),
                            92 => *path_entity = Some(entity),
                            _ => {}
                        }
                    }
                }
                if let Some((sections, guides, path)) =
                    loft_reference_xdata(&surface.common.extended_data)
                {
                    *cross_sections = sections;
                    *guide_curves = guides;
                    *path_curve = path;
                }
            }
            SurfaceData::Revolved {
                revolve_entity,
                class_version,
                entity_id,
                axis_point,
                axis_vector,
                entity_transform,
                ..
            } => {
                if sweep_data.is_empty() {
                    *class_version = sweep_entity_type;
                    *entity_id = sweep_entity_bits as i32;
                } else {
                    let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                        .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                    *revolve_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
                        sweep_entity_type,
                        sweep_entity_bits,
                        sweep_data,
                        dwg_version,
                        dxf_version,
                    );
                }
                *axis_point = point_10.get_point().unwrap_or(Vector3::ZERO);
                *axis_vector = point_11.get_point().unwrap_or(Vector3::UNIT_Z);
                fill_matrix(entity_transform, &matrix);
            }
            SurfaceData::Swept {
                sweep_entity,
                path_entity,
                sweep_transform,
                path_transform,
                options,
                ..
            } => {
                let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(dxf_version)
                    .unwrap_or(crate::io::dwg::DwgVersion::AC24);
                *sweep_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
                    sweep_entity_type,
                    sweep_entity_bits,
                    sweep_data,
                    dwg_version,
                    dxf_version,
                );
                *path_entity = crate::io::dwg::embedded_entity::decode_embedded_entity(
                    path_entity_type,
                    path_entity_bits,
                    path_data,
                    dwg_version,
                    dxf_version,
                );
                fill_matrix(sweep_transform, &matrix);
                fill_matrix(path_transform, &path_matrix);
                options.reference_vector = point_11.get_point().unwrap_or(Vector3::UNIT_Z);
                fill_matrix(&mut options.sweep_entity_transform, &option_sweep_matrix);
                fill_matrix(&mut options.path_entity_transform, &option_path_matrix);
            }
            SurfaceData::Nurb {
                u_vector1,
                v_vector1,
                u_vector2,
                v_vector2,
                ..
            } => {
                *u_vector1 = point_10.get_point().unwrap_or(Vector3::ZERO);
                *v_vector1 = point_11.get_point().unwrap_or(Vector3::ZERO);
                *u_vector2 = point_12.get_point().unwrap_or(Vector3::ZERO);
                *v_vector2 = point_13.get_point().unwrap_or(Vector3::ZERO);
            }
            _ => {}
        }
        if proxy_graphics_size != 0 || !proxy_graphics.is_empty() {
            proxy_graphics.truncate(proxy_graphics_size);
            surface.common.graphic_data = Some(proxy_graphics);
        }
        Ok(Some(surface))
    }

    /// Read a TABLE entity (basic properties)
    fn read_table_entity(&mut self) -> Result<Option<crate::entities::Table>> {
        use crate::entities::table::{
            CellContent, CellStateFlags, CellType, CellValueType, LegacyBorderOverrides,
            LegacyTableStyleOverride, TableAttribute, TableCell, TableCellContentType, TableColumn,
            TableRow, ValueUnitType,
        };

        let mut insertion_point = PointReader::new();
        let mut horizontal = PointReader::new();
        let mut normal = PointReader::new();
        let mut table = crate::entities::Table::new(Vector3::zero(), 0, 0);
        table.rows.clear();
        table.columns.clear();

        let mut row_heights: Vec<f64> = Vec::new();
        let mut col_widths: Vec<f64> = Vec::new();
        let mut cells: Vec<TableCell> = Vec::new();
        let mut nrows: usize = 0;
        let mut ncols: usize = 0;
        let mut cur: Option<TableCell> = None;
        let mut section = String::new();
        let mut pending_attribute_index: Option<usize> = None;
        let mut proxy_graphics_size = 0usize;
        let mut proxy_graphics = Vec::new();
        // True while inside a cell's CELL_VALUE block (301 … 304), so per-cell
        // codes that collide with table-level ones (92, 90) are routed to the
        // cell value rather than the table header.
        let mut in_value = false;
        // Ensure the current cell has at least one content to receive a value.
        fn ensure_content(cell: &mut TableCell) {
            if cell.contents.is_empty() {
                cell.contents.push(CellContent::new());
            }
        }

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                100 => section = pair.value_string.clone(),
                8 => table.common.layer = pair.value_string.clone(),
                62 if cur.is_none() => {
                    if let Some(v) = pair.as_i16() {
                        table.common.color = Color::from_index(v);
                    }
                }
                370 if cur.is_none() => {
                    if let Some(v) = pair.as_i16() {
                        table.common.line_weight = LineWeight::from_value(v);
                    }
                }
                92 | 160 if section == "AcDbEntity" => {
                    proxy_graphics_size = pair.as_int().unwrap_or(0).max(0) as usize;
                }
                310 if section == "AcDbEntity" => {
                    append_hex_bytes(&mut proxy_graphics, &pair.value_string);
                }
                102 if cur.is_none() => {
                    let group = pair.value_string.trim().to_string();
                    if group.starts_with('{') {
                        while let Some(group_pair) = self.reader.read_pair()? {
                            if group_pair.code == 102 && group_pair.value_string.trim() == "}" {
                                break;
                            }
                            if group == "{ACAD_REACTORS" && group_pair.code == 330 {
                                if let Ok(h) =
                                    u64::from_str_radix(group_pair.value_string.trim(), 16)
                                {
                                    table.common.reactors.push(Handle::new(h));
                                }
                            } else if group == "{ACAD_XDICTIONARY" && group_pair.code == 360 {
                                if let Ok(h) =
                                    u64::from_str_radix(group_pair.value_string.trim(), 16)
                                {
                                    table.common.xdictionary_handle = Some(Handle::new(h));
                                }
                            }
                        }
                    }
                }
                2 if cur.is_none() => {
                    table.block_name = pair.value_string.clone();
                }
                343 if cur.is_none() => {
                    if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                        table.block_record_handle = Some(Handle::new(h));
                    }
                }
                2 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        let content = c.contents.last_mut().unwrap();
                        content.content_type = TableCellContentType::Value;
                        content.value.text.push_str(&pair.value_string);
                        if content.value.value_type == CellValueType::Unknown {
                            content.value.value_type = CellValueType::String;
                            content.value.raw_type_code = CellValueType::String as i32;
                        }
                    }
                }
                342 => {
                    if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                        table.table_style_handle = Some(Handle::new(h));
                    }
                }
                280 if cur.is_none() && table.legacy_style_override.is_some() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_bool())
                    {
                        style.title_suppressed = Some(value);
                    }
                }
                281 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_bool())
                    {
                        style.header_suppressed = Some(value);
                    }
                }
                70 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_i16())
                    {
                        style.flow_direction = Some(value);
                    }
                }
                40 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_double())
                    {
                        style.horizontal_cell_margin = Some(value);
                    }
                }
                41 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_double())
                    {
                        style.vertical_cell_margin = Some(value);
                    }
                }
                280 if cur.is_none() => {
                    if let Some(v) = pair.as_i16() {
                        table.data_version = v;
                    }
                }
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                11 | 21 | 31 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_double()) {
                        ensure_content(c);
                        let value = &mut c.contents.last_mut().unwrap().value;
                        if (value.flags & 3) == 0 {
                            match pair.code {
                                11 => value.point_value.x = v,
                                21 => value.point_value.y = v,
                                31 => value.point_value.z = v,
                                _ => {}
                            }
                        }
                    }
                }
                11 | 21 | 31 => {
                    horizontal.add_coordinate(&pair);
                }
                // Table-level column count. A per-cell 92 (extended cell flags,
                // emitted while a cell is open) must NOT overwrite it, or the
                // whole row/col distribution collapses to zero columns.
                92 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i32()) {
                        ensure_content(c);
                        c.contents.last_mut().unwrap().value.data_size = v;
                    }
                }
                92 if cur.is_none() => {
                    if let Some(v) = pair.as_i32() {
                        ncols = v.max(0) as usize;
                    }
                }
                92 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i32()) {
                        c.state = CellStateFlags::from_bits_retain(value as u32);
                    }
                }
                90 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        table.value_flags = value;
                    }
                }
                91 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        nrows = value.max(0) as usize;
                    }
                }
                93 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        table.override_flag = value != 0;
                        table.legacy_style_override =
                            (value != 0).then(|| LegacyTableStyleOverride {
                                flags: value,
                                ..LegacyTableStyleOverride::default()
                            });
                    }
                }
                94 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        table.override_border_color = value != 0;
                        table.legacy_border_colors = (value != 0).then(|| LegacyBorderOverrides {
                            flags: value,
                            values: Vec::new(),
                        });
                    }
                }
                95 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        table.override_border_line_weight = value != 0;
                        table.legacy_border_line_weights =
                            (value != 0).then(|| LegacyBorderOverrides {
                                flags: value,
                                values: Vec::new(),
                            });
                    }
                }
                96 if cur.is_none() => {
                    if let Some(value) = pair.as_i32() {
                        table.override_border_visibility = value != 0;
                        table.legacy_border_visibility =
                            (value != 0).then(|| LegacyBorderOverrides {
                                flags: value,
                                values: Vec::new(),
                            });
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        row_heights.push(v);
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        col_widths.push(v);
                    }
                }
                // ── Cells ──
                171 => {
                    if let Some(c) = cur.take() {
                        cells.push(c);
                    }
                    in_value = false;
                    pending_attribute_index = None;
                    let mut c = TableCell::new();
                    if let Some(v) = pair.as_i16() {
                        c.cell_type = if v == 2 {
                            CellType::Block
                        } else {
                            CellType::Text
                        };
                    }
                    cur = Some(c);
                }
                172 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i16()) {
                        c.edge_flags = value as u8;
                    }
                }
                173 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i16()) {
                        c.merged = value as i32;
                    }
                }
                174 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i16()) {
                        c.auto_fit = v != 0;
                    }
                }
                175 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i16()) {
                        c.merge_width = v as i32;
                    }
                }
                176 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i16()) {
                        c.merge_height = v as i32;
                    }
                }
                177 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i16()) {
                        c.flag = v as i32;
                    }
                }
                178 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i16()) {
                        c.virtual_edge = value;
                    }
                }
                145 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_double()) {
                        c.rotation = value;
                    }
                }
                144 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_double()) {
                        c.block_scale = v;
                    }
                }
                170 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_i16())
                    {
                        style.row_alignments.push(value);
                    }
                }
                170 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i16()) {
                        c.style
                            .get_or_insert_with(crate::entities::CellStyle::new)
                            .alignment = v as i32;
                    }
                }
                1 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        let content = c.contents.last_mut().unwrap();
                        content.content_type = TableCellContentType::Value;
                        let value = &mut content.value;
                        if !in_value || (value.flags & 3) == 0 {
                            value.text.push_str(&pair.value_string);
                            if value.value_type == CellValueType::Unknown {
                                value.value_type = CellValueType::String;
                                value.raw_type_code = CellValueType::String as i32;
                            }
                        }
                    }
                }
                140 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_double())
                    {
                        style.row_heights.push(value);
                    }
                }
                140 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_double()) {
                        ensure_content(c);
                        let cv = &mut c.contents.last_mut().unwrap().value;
                        if (cv.flags & 3) == 0 {
                            cv.numeric_value = v;
                        }
                    }
                }
                140 => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_double()) {
                        c.style
                            .get_or_insert_with(crate::entities::CellStyle::new)
                            .text_height = v;
                    }
                }
                90 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i32()) {
                        ensure_content(c);
                        let value = &mut c.contents.last_mut().unwrap().value;
                        value.raw_type_code = v;
                        value.value_type = CellValueType::from(v as u32);
                    }
                }
                91 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i32()) {
                        ensure_content(c);
                        let value = &mut c.contents.last_mut().unwrap().value;
                        if (value.flags & 3) == 0 {
                            value.numeric_value = v as f64;
                        }
                    }
                }
                91 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i32()) {
                        c.flag = value;
                    }
                }
                93 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i32()) {
                        ensure_content(c);
                        c.contents.last_mut().unwrap().value.flags = v;
                    }
                }
                94 if in_value => {
                    if let (Some(c), Some(v)) = (cur.as_mut(), pair.as_i32()) {
                        ensure_content(c);
                        let value = &mut c.contents.last_mut().unwrap().value;
                        value.raw_unit_type_code = v;
                        value.unit_type = ValueUnitType::from(v as u32);
                    }
                }
                179 => {
                    pending_attribute_index = None;
                }
                // CELL_VALUE block start: the cell has an actual value → mark
                // its content as Value.
                301 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        let content = c.contents.last_mut().unwrap();
                        content.content_type = TableCellContentType::Value;
                        content.value = crate::entities::table::CellValue::new();
                        in_value = true;
                    }
                }
                // Formatted string value inside the CELL_VALUE block.
                302 => {
                    if in_value {
                        if let Some(c) = cur.as_mut() {
                            if let Some(content) = c.contents.last_mut() {
                                if content.value.raw_unit_type_code != 12 {
                                    content.value.formatted_value = pair.value_string.clone();
                                }
                            }
                        }
                    }
                }
                // CELL_VALUE block end.
                304 => {
                    in_value = false;
                }
                300 => {
                    if let Some(c) = cur.as_mut() {
                        if let Some(content) = c.contents.last_mut() {
                            if let Some(index) = pending_attribute_index.take() {
                                if let Some(attribute) = content.attributes.get_mut(index) {
                                    attribute.value = pair.value_string.clone();
                                }
                            } else {
                                content.value.format = pair.value_string.clone();
                            }
                        }
                    }
                }
                310 if in_value => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        let value = &mut c.contents.last_mut().unwrap().value;
                        if (value.flags & 3) == 0 {
                            append_hex_bytes(&mut value.binary_value, &pair.value_string);
                        }
                    }
                }
                330 if in_value => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                            c.contents.last_mut().unwrap().value.handle_value =
                                Some(Handle::new(h));
                        }
                    }
                }
                340 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        if let (Some(content), Ok(h)) = (
                            c.contents.last_mut(),
                            u64::from_str_radix(pair.value_string.trim(), 16),
                        ) {
                            content.block_handle = Some(Handle::new(h));
                            content.content_type = TableCellContentType::Block;
                        }
                    }
                }
                344 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        if let Ok(value) = u64::from_str_radix(pair.value_string.trim(), 16) {
                            let content = c.contents.last_mut().unwrap();
                            content.field_handle = (value != 0).then(|| Handle::new(value));
                            content.content_type = TableCellContentType::Field;
                        }
                    }
                }
                7 if cur.is_none() => {
                    if let Some(style) = table.legacy_style_override.as_mut() {
                        style.text_style_names.push(pair.value_string.clone());
                    }
                }
                7 => {
                    if let Some(c) = cur.as_mut() {
                        c.style
                            .get_or_insert_with(crate::entities::CellStyle::new)
                            .text_style_name = pair.value_string.clone();
                    }
                }
                63 | 64 | 65 | 66 | 68 | 69 if cur.is_none() => {
                    if let Some(value) = pair.as_i16() {
                        let color = Color::from_index(value);
                        let mut consumed = false;
                        if let Some(style) = table.legacy_style_override.as_mut() {
                            if pair.code == 64 {
                                let expected = [0x0020, 0x0040, 0x0080]
                                    .iter()
                                    .filter(|bit| style.flags & **bit != 0)
                                    .count();
                                if style.row_colors.len() < expected {
                                    style.row_colors.push(color);
                                    consumed = true;
                                }
                            } else if pair.code == 63 {
                                let expected = [0x0800, 0x1000, 0x2000]
                                    .iter()
                                    .filter(|bit| style.flags & **bit != 0)
                                    .count();
                                if style.row_fill_colors.len() < expected {
                                    style.row_fill_colors.push(color);
                                    consumed = true;
                                }
                            }
                        }
                        if !consumed {
                            if let Some(overrides) = table.legacy_border_colors.as_mut() {
                                overrides.values.push(color);
                            }
                        }
                    }
                }
                63 | 64 | 65 | 66 | 68 | 69 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i16()) {
                        let style = c.style.get_or_insert_with(crate::entities::CellStyle::new);
                        let color = Color::from_index(value);
                        match pair.code {
                            63 => style.background_color = color,
                            64 => style.content_color = color,
                            65 => style.right_border.color = color,
                            66 => style.bottom_border.color = color,
                            68 => style.left_border.color = color,
                            69 => style.top_border.color = color,
                            _ => {}
                        }
                    }
                }
                274..=279 if cur.is_none() => {
                    if let (Some(overrides), Some(value)) =
                        (table.legacy_border_line_weights.as_mut(), pair.as_i16())
                    {
                        overrides.values.push(LineWeight::from_value(value));
                    }
                }
                275 | 276 | 278 | 279 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_i16()) {
                        let style = c.style.get_or_insert_with(crate::entities::CellStyle::new);
                        let border = match pair.code {
                            275 => &mut style.right_border,
                            276 => &mut style.bottom_border,
                            278 => &mut style.left_border,
                            _ => &mut style.top_border,
                        };
                        border.line_weight = LineWeight::from_value(value);
                    }
                }
                283 if cur.is_none() => {
                    if let (Some(style), Some(value)) =
                        (table.legacy_style_override.as_mut(), pair.as_bool())
                    {
                        style.row_fill_none.push(value);
                    }
                }
                283 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_bool()) {
                        c.style
                            .get_or_insert_with(crate::entities::CellStyle::new)
                            .fill_enabled = value;
                    }
                }
                284..=289 if cur.is_none() => {
                    if let (Some(overrides), Some(value)) =
                        (table.legacy_border_visibility.as_mut(), pair.as_bool())
                    {
                        overrides.values.push(value);
                    }
                }
                285 | 286 | 288 | 289 => {
                    if let (Some(c), Some(value)) = (cur.as_mut(), pair.as_bool()) {
                        let style = c.style.get_or_insert_with(crate::entities::CellStyle::new);
                        let border = match pair.code {
                            285 => &mut style.right_border,
                            286 => &mut style.bottom_border,
                            288 => &mut style.left_border,
                            _ => &mut style.top_border,
                        };
                        border.invisible = !value;
                    }
                }
                331 => {
                    if let Some(c) = cur.as_mut() {
                        ensure_content(c);
                        if let (Some(content), Ok(value)) = (
                            c.contents.last_mut(),
                            u64::from_str_radix(pair.value_string.trim(), 16),
                        ) {
                            let index = content.attributes.len();
                            content.attributes.push(TableAttribute {
                                definition_handle: Handle::new(value),
                                value: String::new(),
                                index: index as i32,
                            });
                            pending_attribute_index = Some(index);
                        }
                    }
                }
                1001 => {
                    self.try_read_common_entity_code(&pair, &mut table.common)?;
                }
                _ => {
                    if cur.is_none() {
                        self.try_read_common_entity_code(&pair, &mut table.common)?;
                    }
                }
            }
        }
        if let Some(c) = cur.take() {
            cells.push(c);
        }

        // Assemble columns/rows and distribute the row-major cell stream.
        for w in col_widths {
            table.columns.push(TableColumn {
                name: String::new(),
                width: w,
                style: None,
                custom_data: 0,
                custom_data_items: Vec::new(),
                style_id: 0,
            });
        }
        if ncols == 0 {
            ncols = table.columns.len();
        }
        while row_heights.len() < nrows {
            row_heights.push(0.0);
        }
        for h in row_heights {
            table.rows.push(TableRow {
                height: h,
                cells: Vec::new(),
                style: None,
                custom_data: 0,
                custom_data_items: Vec::new(),
                style_id: 0,
            });
        }
        if ncols > 0 && !table.rows.is_empty() {
            for (i, cell) in cells.into_iter().enumerate() {
                let r = i / ncols;
                if r < table.rows.len() {
                    table.rows[r].cells.push(cell);
                }
            }
        } else if !table.rows.is_empty() {
            table.rows[0].cells = cells;
        }

        table.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        if let Some(h) = horizontal.get_point() {
            table.horizontal_direction = h;
        }
        if let Some(value) = normal.get_point() {
            table.normal = value;
        }
        if proxy_graphics_size != 0 || !proxy_graphics.is_empty() {
            proxy_graphics.truncate(proxy_graphics_size);
            table.common.graphic_data = Some(proxy_graphics);
        }
        Ok(Some(table))
    }

    /// Read a PDF/DWF/DGN UNDERLAY entity
    fn read_underlay(&mut self, type_name: &str) -> Result<Option<Underlay>> {
        use crate::entities::underlay::{UnderlayDisplayFlags, UnderlayType};
        let utype = match type_name {
            "DWFUNDERLAY" => UnderlayType::Dwf,
            "DGNUNDERLAY" => UnderlayType::Dgn,
            _ => UnderlayType::Pdf,
        };
        let mut underlay = Underlay::new(utype);
        let mut insertion_point = PointReader::new();
        let mut normal = PointReader::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                8 => underlay.common.layer = pair.value_string.clone(),
                62 => {
                    if let Some(v) = pair.as_i16() {
                        underlay.common.color = Color::from_index(v);
                    }
                }
                370 => {
                    if let Some(v) = pair.as_i16() {
                        underlay.common.line_weight = LineWeight::from_value(v);
                    }
                }
                10 | 20 | 30 => {
                    insertion_point.add_coordinate(&pair);
                }
                210 | 220 | 230 => {
                    normal.add_coordinate(&pair);
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        underlay.x_scale = v;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        underlay.y_scale = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        underlay.z_scale = v;
                    }
                }
                50 => {
                    if let Some(v) = pair.as_double() {
                        underlay.rotation = v;
                    }
                }
                280 => {
                    // Display flags (bit0 clipping, bit1 on, bit2 monochrome,
                    // bit3 adjust-for-background, bit4 clip-inside/inverted).
                    if let Some(v) = pair.as_i16() {
                        let flags = UnderlayDisplayFlags::from_bits_truncate(v as u8);
                        underlay.flags = flags;
                        underlay.clip_inverted = flags.contains(UnderlayDisplayFlags::CLIP_INSIDE);
                    }
                }
                281 => {
                    if let Some(v) = pair.as_i16() {
                        underlay.contrast = v as u8;
                    }
                }
                282 => {
                    if let Some(v) = pair.as_i16() {
                        underlay.fade = v as u8;
                    }
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        underlay.definition_handle = Handle::new(h);
                    }
                }
                11 => {
                    if let Some(x) = pair.as_double() {
                        underlay.clip_boundary_vertices.push(Vector2::new(x, 0.0));
                    }
                }
                21 => {
                    if let Some(y) = pair.as_double() {
                        if let Some(last) = underlay.clip_boundary_vertices.last_mut() {
                            last.y = y;
                        }
                    }
                }
                _ => {
                    self.try_read_common_entity_code(&pair, &mut underlay.common)?;
                }
            }
        }

        underlay.insertion_point = insertion_point.get_point().unwrap_or(Vector3::zero());
        underlay.normal = normal.get_point().unwrap_or(Vector3::new(0.0, 0.0, 1.0));
        Ok(Some(underlay))
    }

    // ===== New Object Readers =====

    /// Read an XRECORD object
    fn read_xrecord(&mut self) -> Result<Option<XRecord>> {
        let mut xr = XRecord::new();
        let mut group = String::new();
        let mut owner_seen = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        xr.handle = Handle::new(h);
                    }
                }
                100 if pair.value_string == "AcDbXrecord" => {}
                102 if pair.value_string == "{ACAD_REACTORS"
                    || pair.value_string == "{ACAD_XDICTIONARY" =>
                {
                    group = pair.value_string.clone();
                }
                102 if !group.is_empty() && pair.value_string == "}" => {
                    group.clear();
                }
                330 if group == "{ACAD_REACTORS" => {
                    xr.reactors.push(parse_dxf_handle(&pair.value_string));
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    xr.xdictionary_handle = Some(parse_dxf_handle(&pair.value_string));
                }
                330 if !owner_seen && group.is_empty() => {
                    xr.owner = parse_dxf_handle(&pair.value_string);
                    owner_seen = true;
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        xr.cloning_flags = DictionaryCloningFlags::from_value(v);
                    }
                }
                _ => {
                    let value = match XRecordValueType::from_code(pair.code) {
                        XRecordValueType::String => XRecordValue::String(pair.value_string.clone()),
                        XRecordValueType::Point3D => {
                            let x = pair.value_string.trim().parse::<f64>().unwrap_or(0.0);
                            let mut y = 0.0;
                            let mut z = 0.0;
                            if let Some(next) = self.reader.read_pair()? {
                                if next.code == pair.code + 10 {
                                    y = next.value_string.trim().parse::<f64>().unwrap_or(0.0);
                                    if let Some(next_z) = self.reader.read_pair()? {
                                        if next_z.code == pair.code + 20 {
                                            z = next_z
                                                .value_string
                                                .trim()
                                                .parse::<f64>()
                                                .unwrap_or(0.0);
                                        } else {
                                            self.reader.push_back(next_z);
                                        }
                                    }
                                } else {
                                    self.reader.push_back(next);
                                }
                            }
                            XRecordValue::Point3D(x, y, z)
                        }
                        XRecordValueType::Double => pair
                            .value_string
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .map(XRecordValue::Double)
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Int16 => pair
                            .value_string
                            .trim()
                            .parse::<i16>()
                            .ok()
                            .map(XRecordValue::Int16)
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Int32 => pair
                            .value_string
                            .trim()
                            .parse::<i32>()
                            .ok()
                            .map(XRecordValue::Int32)
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Int64 => pair
                            .value_string
                            .trim()
                            .parse::<i64>()
                            .ok()
                            .map(XRecordValue::Int64)
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Byte => pair
                            .value_string
                            .trim()
                            .parse::<u8>()
                            .ok()
                            .map(XRecordValue::Byte)
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Bool => pair
                            .value_string
                            .trim()
                            .parse::<i32>()
                            .ok()
                            .map(|value| XRecordValue::Bool(value != 0))
                            .unwrap_or(XRecordValue::String(pair.value_string.clone())),
                        XRecordValueType::Handle | XRecordValueType::ObjectId => {
                            u64::from_str_radix(pair.value_string.trim(), 16)
                                .map(|h| XRecordValue::Handle(Handle::new(h)))
                                .unwrap_or(XRecordValue::String(pair.value_string.clone()))
                        }
                        XRecordValueType::Chunk => {
                            let hex = pair.value_string.trim().as_bytes();
                            let mut bytes = Vec::with_capacity(hex.len() / 2);
                            let mut i = 0;
                            while i + 1 < hex.len() {
                                if let (Some(hi), Some(lo)) = (
                                    (hex[i] as char).to_digit(16),
                                    (hex[i + 1] as char).to_digit(16),
                                ) {
                                    bytes.push((hi * 16 + lo) as u8);
                                }
                                i += 2;
                            }
                            xr.raw_data.extend_from_slice(&bytes);
                            XRecordValue::Chunk(bytes)
                        }
                        _ => {
                            xr.entries_complete = false;
                            XRecordValue::String(pair.value_string.clone())
                        }
                    };
                    if (330..=369).contains(&pair.code) {
                        if let XRecordValue::Handle(handle) = &value {
                            let kind = match pair.code {
                                330..=339 => ProxyReferenceKind::SoftPointer,
                                340..=349 => ProxyReferenceKind::HardPointer,
                                350..=359 => ProxyReferenceKind::SoftOwnership,
                                360..=369 => ProxyReferenceKind::HardOwnership,
                                _ => ProxyReferenceKind::Undefined,
                            };
                            xr.object_references.push(ProxyObjectReference {
                                handle: *handle,
                                kind,
                            });
                        }
                    }
                    xr.entries.push(XRecordEntry {
                        code: pair.code,
                        value,
                    });
                }
            }
        }

        Ok(Some(xr))
    }

    /// Read a GROUP object
    fn read_group(&mut self) -> Result<Option<Group>> {
        let mut group = Group::new("");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        group.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        group.owner = Handle::new(h);
                    }
                }
                300 => group.description = pair.value_string.clone(),
                70 => {
                    if let Some(v) = pair.as_i16() {
                        group.unnamed = v != 0;
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        group.selectable = v != 0;
                    }
                }
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        group.entities.push(Handle::new(h));
                    }
                }
                _ => {}
            }
        }

        Ok(Some(group))
    }

    /// Read an MLINESTYLE object
    fn read_mlinestyle_object(&mut self) -> Result<Option<crate::objects::MLineStyle>> {
        use crate::objects::MLineStyleElement;
        let mut style = crate::objects::MLineStyle::new("");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.owner = Handle::new(h);
                    }
                }
                2 => style.name = pair.value_string.clone(),
                3 => style.description = pair.value_string.clone(),
                // DXF stores MLINESTYLE angles in degrees; the model stores
                // radians (the DWG stream and the DXF writer both use
                // radians/degrees respectively). Reading them raw made every
                // read→write cycle multiply the angle by 180/π (issue #51).
                51 => {
                    if let Some(v) = pair.as_double() {
                        style.start_angle = v.to_radians();
                    }
                }
                52 => {
                    if let Some(v) = pair.as_double() {
                        style.end_angle = v.to_radians();
                    }
                }
                62 => {
                    if let Some(v) = pair.as_i16() {
                        if let Some(last) = style.elements.last_mut() {
                            last.color = Color::from_index(v);
                        } else {
                            style.fill_color = Color::from_index(v);
                        }
                    }
                }
                49 => {
                    // Element offset — start a new element
                    if let Some(v) = pair.as_double() {
                        style.elements.push(MLineStyleElement {
                            offset: v,
                            color: Color::ByLayer,
                            linetype: String::from("BYLAYER"),
                        });
                    }
                }
                6 => {
                    if let Some(last) = style.elements.last_mut() {
                        last.linetype = pair.value_string.clone();
                    }
                }
                _ => {}
            }
        }

        Ok(Some(style))
    }

    /// Read an IMAGEDEF object
    fn read_image_definition(&mut self) -> Result<Option<crate::objects::ImageDefinition>> {
        let mut def = crate::objects::ImageDefinition::new("");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        def.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        def.owner = Handle::new(h);
                    }
                }
                1 => def.file_name = pair.value_string.clone(),
                10 => {
                    if let Some(v) = pair.as_double() {
                        def.size_in_pixels.0 = v as u32;
                    }
                }
                20 => {
                    if let Some(v) = pair.as_double() {
                        def.size_in_pixels.1 = v as u32;
                    }
                }
                11 => {
                    if let Some(v) = pair.as_double() {
                        def.pixel_size.0 = v;
                    }
                }
                21 => {
                    if let Some(v) = pair.as_double() {
                        def.pixel_size.1 = v;
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        def.is_loaded = v != 0;
                    }
                }
                _ => {}
            }
        }

        Ok(Some(def))
    }

    /// Read a PDF/DWF/DGN underlay definition object.
    fn read_underlay_definition(
        &mut self,
        utype: crate::entities::underlay::UnderlayType,
    ) -> Result<Option<crate::objects::UnderlayDefinition>> {
        let mut def = crate::objects::UnderlayDefinition::new(utype);

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        def.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        def.owner_handle = Handle::new(h);
                    }
                }
                1 => def.file_path = pair.value_string.clone(),
                2 => def.page_name = pair.value_string.clone(),
                _ => {}
            }
        }

        Ok(Some(def))
    }

    /// Read an MLEADERSTYLE object
    fn read_multileader_style(&mut self) -> Result<Option<MultiLeaderStyle>> {
        let mut style = MultiLeaderStyle::new("Standard");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.owner_handle = Handle::new(h);
                    }
                }
                3 => style.name = pair.value_string.clone(),
                300 => style.description = pair.value_string.clone(),
                170 => {
                    if let Some(v) = pair.as_i16() {
                        style.content_type = crate::objects::LeaderContentType::from(v);
                    }
                }
                173 => {
                    if let Some(v) = pair.as_i16() {
                        style.path_type = crate::objects::MultiLeaderPathType::from(v);
                    }
                }
                91 => {
                    if let Some(v) = pair.as_i32() {
                        style.line_color = Color::from_index(v as i16);
                    }
                }
                92 => {
                    if let Some(v) = pair.as_i32() {
                        style.line_weight = LineWeight::from_value(v as i16);
                    }
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        style.enable_landing = v;
                    }
                }
                291 => {
                    if let Some(v) = pair.as_bool() {
                        style.enable_dogleg = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        style.landing_distance = v;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        style.landing_gap = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        style.arrowhead_size = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        style.text_height = v;
                    }
                }
                93 => {
                    if let Some(v) = pair.as_i32() {
                        style.text_color = Color::from_index(v as i16);
                    }
                }
                292 => {
                    if let Some(v) = pair.as_bool() {
                        style.text_frame = v;
                    }
                }
                174 => {
                    if let Some(v) = pair.as_i16() {
                        style.text_left_attachment = crate::objects::TextAttachmentType::from(v);
                    }
                }
                178 => {
                    if let Some(v) = pair.as_i16() {
                        style.text_right_attachment = crate::objects::TextAttachmentType::from(v);
                    }
                }
                175 => {
                    if let Some(v) = pair.as_i16() {
                        style.text_angle_type = crate::objects::TextAngleType::from(v);
                    }
                }
                176 => {
                    if let Some(v) = pair.as_i16() {
                        style.text_alignment = crate::objects::TextAlignmentType::from(v);
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        style.scale_factor = v;
                    }
                }
                304 => style.default_text = pair.value_string.clone(),
                340 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.line_type_handle = Some(Handle::new(h));
                    }
                }
                341 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.arrowhead_handle = Some(Handle::new(h));
                    }
                }
                342 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.text_style_handle = Some(Handle::new(h));
                    }
                }
                343 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        style.block_content_handle = Some(Handle::new(h));
                    }
                }
                296 => {
                    if let Some(v) = pair.as_bool() {
                        style.is_annotative = v;
                    }
                }
                _ => {}
            }
        }

        Ok(Some(style))
    }

    /// Read a PLOTSETTINGS object
    fn read_plot_settings(&mut self) -> Result<Option<PlotSettings>> {
        let mut ps = PlotSettings::new("");
        let mut group = String::new();
        let mut owner_seen = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ps.handle = Handle::new(h);
                    }
                }
                102 => {
                    group = pair.value_string.clone();
                }
                330 if group == "{ACAD_REACTORS" => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ps.reactors.push(Handle::new(h));
                    }
                }
                360 if group == "{ACAD_XDICTIONARY" => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ps.xdictionary_handle = Some(Handle::new(h));
                    }
                }
                330 if group.is_empty() && !owner_seen => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ps.owner = Handle::new(h);
                    }
                    owner_seen = true;
                }
                1 => ps.page_name = pair.value_string.clone(),
                2 => ps.printer_name = pair.value_string.clone(),
                4 => ps.paper_size = pair.value_string.clone(),
                6 => ps.plot_view_name = pair.value_string.clone(),
                7 => ps.current_style_sheet = pair.value_string.clone(),
                40 => {
                    if let Some(v) = pair.as_double() {
                        ps.margins.left = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        ps.margins.bottom = v;
                    }
                }
                42 => {
                    if let Some(v) = pair.as_double() {
                        ps.margins.right = v;
                    }
                }
                43 => {
                    if let Some(v) = pair.as_double() {
                        ps.margins.top = v;
                    }
                }
                44 => {
                    if let Some(v) = pair.as_double() {
                        ps.paper_width = v;
                    }
                }
                45 => {
                    if let Some(v) = pair.as_double() {
                        ps.paper_height = v;
                    }
                }
                46 => {
                    if let Some(v) = pair.as_double() {
                        ps.origin_x = v;
                    }
                }
                47 => {
                    if let Some(v) = pair.as_double() {
                        ps.origin_y = v;
                    }
                }
                48 => {
                    if let Some(v) = pair.as_double() {
                        ps.plot_window.lower_left_x = v;
                    }
                }
                49 => {
                    if let Some(v) = pair.as_double() {
                        ps.plot_window.lower_left_y = v;
                    }
                }
                70 => {
                    if let Some(v) = pair.as_i32() {
                        ps.flags = crate::objects::PlotFlags::from_bits(v);
                    }
                }
                72 => {
                    if let Some(v) = pair.as_i16() {
                        ps.paper_units = crate::objects::PlotPaperUnits::from_code(v);
                    }
                }
                73 => {
                    if let Some(v) = pair.as_i16() {
                        ps.rotation = crate::objects::PlotRotation::from_code(v);
                    }
                }
                74 => {
                    if let Some(v) = pair.as_i16() {
                        ps.plot_type = crate::objects::PlotType::from_code(v);
                    }
                }
                75 => {
                    if let Some(v) = pair.as_i16() {
                        ps.scale_type = crate::objects::ScaledType::from_code(v);
                    }
                }
                76 => {
                    if let Some(v) = pair.as_i16() {
                        ps.shade_plot_mode = crate::objects::ShadePlotMode::from_code(v);
                    }
                }
                77 => {
                    if let Some(v) = pair.as_i16() {
                        ps.shade_plot_resolution =
                            crate::objects::ShadePlotResolutionLevel::from_code(v);
                    }
                }
                78 => {
                    if let Some(v) = pair.as_i16() {
                        ps.shade_plot_dpi = v;
                    }
                }
                140 => {
                    if let Some(v) = pair.as_double() {
                        ps.plot_window.upper_right_x = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        ps.plot_window.upper_right_y = v;
                    }
                }
                142 => {
                    if let Some(v) = pair.as_double() {
                        ps.scale_numerator = v;
                    }
                }
                143 => {
                    if let Some(v) = pair.as_double() {
                        ps.scale_denominator = v;
                    }
                }
                147 => {
                    if let Some(v) = pair.as_double() {
                        ps.standard_scale_factor = v;
                    }
                }
                148 => {
                    if let Some(v) = pair.as_double() {
                        ps.paper_image_origin_x = v;
                    }
                }
                149 => {
                    if let Some(v) = pair.as_double() {
                        ps.paper_image_origin_y = v;
                    }
                }
                333 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ps.visual_style_handle = Handle::new(h);
                    }
                }
                _ => {}
            }
            if group == "}" {
                group.clear();
            }
        }

        Ok(Some(ps))
    }

    /// Read a TABLESTYLE object
    fn read_table_style(&mut self) -> Result<Option<TableStyle>> {
        fn border_mut(style: &mut RowCellStyle, index: usize) -> &mut TableCellBorder {
            match index {
                0 => &mut style.top_border,
                1 => &mut style.horizontal_inside_border,
                2 => &mut style.bottom_border,
                3 => &mut style.left_border,
                4 => &mut style.vertical_inside_border,
                _ => &mut style.right_border,
            }
        }

        let mut ts = TableStyle::new("Standard");
        let mut raw_dxf_codes = Vec::new();
        let mut rows = Vec::<RowCellStyle>::with_capacity(3);
        let mut saw_name = false;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            raw_dxf_codes.push((pair.code, pair.value_string.clone()));
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ts.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        ts.owner_handle = Handle::new(h);
                    }
                }
                3 => {
                    ts.name = pair.value_string.clone();
                    saw_name = true;
                }
                70 => {
                    if let Some(v) = pair.as_i16() {
                        ts.flow_direction = TableFlowDirection::from(v);
                    }
                }
                71 => {
                    if let Some(v) = pair.as_i16() {
                        ts.flags = TableStyleFlags::from_bits_retain(v);
                    }
                }
                40 => {
                    if let Some(v) = pair.as_double() {
                        ts.horizontal_margin = v;
                    }
                }
                41 => {
                    if let Some(v) = pair.as_double() {
                        ts.vertical_margin = v;
                    }
                }
                280 if saw_name => {
                    if let Some(v) = pair.as_bool() {
                        ts.title_suppressed = v;
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        ts.version = v;
                    }
                }
                281 => {
                    if let Some(v) = pair.as_bool() {
                        ts.header_suppressed = v;
                    }
                }
                7 if rows.len() < 3 => {
                    let mut row = RowCellStyle::new();
                    row.text_style_name = pair.value_string.clone();
                    rows.push(row);
                }
                140 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_double()) {
                        row.text_height = v;
                    }
                }
                170 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i16()) {
                        row.alignment = CellAlignment::from(v);
                    }
                }
                62 | 63 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i16()) {
                        let color = Color::from_index(if v > 256 { v & 0xff } else { v });
                        if pair.code == 62 {
                            row.text_color = color;
                        } else {
                            row.fill_color = color;
                        }
                    }
                }
                283 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_bool()) {
                        row.fill_enabled = v;
                    }
                }
                90 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i32()) {
                        row.data_type = v;
                    }
                }
                91 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i32()) {
                        row.unit_type = v;
                    }
                }
                1 => {
                    if let Some(row) = rows.last_mut() {
                        row.format_string = pair.value_string.clone();
                    }
                }
                274..=279 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i16()) {
                        border_mut(row, (pair.code - 274) as usize).line_weight =
                            LineWeight::from_value(v);
                    }
                }
                284..=289 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_bool()) {
                        border_mut(row, (pair.code - 284) as usize).is_invisible = !v;
                    }
                }
                64..=69 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i16()) {
                        border_mut(row, (pair.code - 64) as usize).color = Color::from_index(v);
                    }
                }
                420 | 421 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i32()) {
                        let color = Color::from_true_color_value(v);
                        if pair.code == 420 {
                            row.text_color = color;
                        } else {
                            row.fill_color = color;
                        }
                    }
                }
                422..=427 => {
                    if let (Some(row), Some(v)) = (rows.last_mut(), pair.as_i32()) {
                        border_mut(row, (pair.code - 422) as usize).color =
                            Color::from_true_color_value(v);
                    }
                }
                1001 => {
                    if pair.value_string == "AcadAnnotative" {
                        ts.annotative = self.read_annotative_xdata(pair)?;
                    }
                }
                _ => {}
            }
        }

        if let Some(row) = rows.first() {
            ts.data_row_style = row.clone();
        }
        if let Some(row) = rows.get(1) {
            ts.title_row_style = row.clone();
        }
        if let Some(row) = rows.get(2) {
            ts.header_row_style = row.clone();
        }
        ts.raw_dxf_codes = Some(raw_dxf_codes);
        Ok(Some(ts))
    }

    /// Read a SCALE object
    fn read_scale(&mut self) -> Result<Option<Scale>> {
        let mut scale = Scale::new("1:1", 1.0, 1.0);

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        scale.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        scale.owner_handle = Handle::new(h);
                    }
                }
                300 => scale.name = pair.value_string.clone(),
                140 => {
                    if let Some(v) = pair.as_double() {
                        scale.paper_units = v;
                    }
                }
                141 => {
                    if let Some(v) = pair.as_double() {
                        scale.drawing_units = v;
                    }
                }
                290 => {
                    if let Some(v) = pair.as_bool() {
                        scale.is_unit_scale = v;
                    }
                }
                _ => {}
            }
        }

        Ok(Some(scale))
    }

    /// Read a SORTENTSTABLE object
    fn read_sort_entities_table(
        &mut self,
        version: crate::types::DxfVersion,
    ) -> Result<Option<SortEntitiesTable>> {
        let mut set = SortEntitiesTable::new();
        let mut entity_handle: Option<Handle> = None;
        let mut raw_dxf_codes = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            raw_dxf_codes.push((pair.code, pair.value_string.clone()));
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        if set.handle.is_null() {
                            set.handle = Handle::new(h);
                        } else if let Some(eh) = entity_handle.take() {
                            set.add_entry(eh, Handle::new(h));
                        }
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        set.block_owner_handle = Handle::new(h);
                    }
                }
                331 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        entity_handle = Some(Handle::new(h));
                    }
                }
                _ => {}
            }
        }

        set.raw_dxf_codes = Some(raw_dxf_codes);
        set.raw_dxf_version = Some(version);
        Ok(Some(set))
    }

    /// Read a DICTIONARYVAR object
    fn read_dictionary_variable(&mut self) -> Result<Option<DictionaryVariable>> {
        let mut dv = DictionaryVariable::new("", "");

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match pair.code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        dv.handle = Handle::new(h);
                    }
                }
                330 => {
                    if let Ok(h) = u64::from_str_radix(&pair.value_string, 16) {
                        dv.owner_handle = Handle::new(h);
                    }
                }
                280 => {
                    if let Some(v) = pair.as_i16() {
                        dv.schema_number = v;
                    }
                }
                1 => dv.value = pair.value_string.clone(),
                _ => {}
            }
        }

        Ok(Some(dv))
    }
}

/// Decode a colour stored as a raw CMC i32 (MULTILEADER 90/91/92/93 codes).
fn color_from_i32(v: i32) -> Color {
    // AutoCAD-produced DXF encodes a color in the high "method" byte:
    //   0xC0 ByLayer, 0xC1 ByBlock, 0xC2 true-color RGB, 0xC3 ACI index.
    // MLEADER/MLEADERSTYLE colours use this form; check it before the
    // writer's own plain scheme (0=ByBlock, 256=ByLayer, 1..255=ACI, else RGB).
    let u = v as u32;
    match (u >> 24) & 0xFF {
        0xC0 => return Color::ByLayer,
        0xC1 => return Color::ByBlock,
        0xC2 => {
            return Color::Rgb {
                r: ((u >> 16) & 0xFF) as u8,
                g: ((u >> 8) & 0xFF) as u8,
                b: (u & 0xFF) as u8,
            }
        }
        0xC3 => return Color::from_index((u & 0xFF) as i16),
        0xC8 => return Color::None,
        _ => {}
    }
    match v {
        0 => Color::ByBlock,
        256 => Color::ByLayer,
        1..=255 => Color::from_index(v as i16),
        _ => Color::Rgb {
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: (v & 0xFF) as u8,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a document, write to DXF, read back.
    fn roundtrip(doc: CadDocument) -> CadDocument {
        let writer = crate::io::dxf::writer::DxfWriter::new(&doc);
        let bytes = writer.write_to_vec().expect("write_to_vec");
        let cursor = std::io::Cursor::new(bytes);
        let reader = crate::io::dxf::reader::DxfReader::from_reader(cursor).expect("from_reader");
        reader.read().expect("read")
    }

    #[test]
    fn test_dxf_roundtrip_line_normal() {
        let mut doc = CadDocument::new();
        let mut line = crate::entities::line::Line::new();
        line.start = Vector3::new(1.0, 2.0, 3.0);
        line.end = Vector3::new(4.0, 5.0, 6.0);
        line.normal = Vector3::new(0.0, 1.0, 0.0);
        line.thickness = 2.5;
        line.common.layer = "TestLayer".to_string();
        let _ = doc.add_entity(EntityType::Line(line));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Line(ref l) = entities[0] {
            assert_eq!(l.common.layer, "TestLayer");
            assert!((l.normal.x - 0.0).abs() < 1e-9);
            assert!((l.normal.y - 1.0).abs() < 1e-9);
            assert!((l.normal.z - 0.0).abs() < 1e-9);
            assert!((l.thickness - 2.5).abs() < 1e-9);
            assert!((l.start.x - 1.0).abs() < 1e-9);
            assert!((l.end.x - 4.0).abs() < 1e-9);
        } else {
            panic!("Expected Line entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_circle_normal() {
        let mut doc = CadDocument::new();
        let mut circle = crate::entities::circle::Circle::new();
        circle.center = Vector3::new(10.0, 20.0, 0.0);
        circle.radius = 5.0;
        circle.normal = Vector3::new(0.0, 0.0, -1.0);
        circle.thickness = 1.5;
        let _ = doc.add_entity(EntityType::Circle(circle));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Circle(ref c) = entities[0] {
            assert!((c.normal.z - (-1.0)).abs() < 1e-9);
            assert!((c.thickness - 1.5).abs() < 1e-9);
            assert!((c.radius - 5.0).abs() < 1e-9);
        } else {
            panic!("Expected Circle entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_arc_normal() {
        let mut doc = CadDocument::new();
        let mut arc = crate::entities::arc::Arc::new();
        arc.center = Vector3::new(0.0, 0.0, 0.0);
        arc.radius = 10.0;
        arc.start_angle = 0.0;
        arc.end_angle = 90.0;
        arc.normal = Vector3::new(1.0, 0.0, 0.0);
        arc.thickness = 3.0;
        let _ = doc.add_entity(EntityType::Arc(arc));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Arc(ref a) = entities[0] {
            assert!((a.normal.x - 1.0).abs() < 1e-9);
            assert!((a.thickness - 3.0).abs() < 1e-9);
        } else {
            panic!("Expected Arc entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_text_properties() {
        use crate::entities::text::{TextHorizontalAlignment, TextVerticalAlignment};

        let mut doc = CadDocument::new();
        let mut text = crate::entities::text::Text::new();
        text.value = "Hello".to_string();
        text.insertion_point = Vector3::new(1.0, 2.0, 3.0);
        text.alignment_point = Some(Vector3::new(10.0, 20.0, 0.0));
        text.height = 2.5;
        text.rotation = 45.0_f64.to_radians();
        text.horizontal_alignment = TextHorizontalAlignment::Center;
        text.vertical_alignment = TextVerticalAlignment::Middle;
        text.normal = Vector3::new(0.0, 1.0, 0.0);
        let _ = doc.add_entity(EntityType::Text(text));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Text(ref t) = entities[0] {
            assert_eq!(t.value, "Hello");
            assert!((t.height - 2.5).abs() < 1e-9);
            assert!((t.rotation - 45.0_f64.to_radians()).abs() < 1e-6);
            assert_eq!(t.horizontal_alignment, TextHorizontalAlignment::Center);
            assert_eq!(t.vertical_alignment, TextVerticalAlignment::Middle);
            assert!((t.normal.y - 1.0).abs() < 1e-9);
            assert!(t.alignment_point.is_some());
            let ap = t.alignment_point.unwrap();
            assert!((ap.x - 10.0).abs() < 1e-9);
        } else {
            panic!("Expected Text entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_mtext_properties() {
        use crate::entities::mtext::{AttachmentPoint, DrawingDirection};

        let mut doc = CadDocument::new();
        let mut mtext = crate::entities::mtext::MText::new();
        mtext.value = "Multi\\Pline".to_string();
        mtext.insertion_point = Vector3::new(5.0, 10.0, 0.0);
        mtext.height = 3.0;
        mtext.rectangle_width = 50.0;
        mtext.attachment_point = AttachmentPoint::MiddleCenter;
        mtext.drawing_direction = DrawingDirection::TopToBottom;
        mtext.line_spacing_factor = 1.5;
        mtext.normal = Vector3::new(0.0, 0.0, -1.0);
        let _ = doc.add_entity(EntityType::MText(mtext));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::MText(ref m) = entities[0] {
            assert_eq!(m.value, "Multi\\Pline");
            assert!((m.height - 3.0).abs() < 1e-9);
            assert!((m.rectangle_width - 50.0).abs() < 1e-9);
            assert_eq!(m.attachment_point, AttachmentPoint::MiddleCenter);
            assert_eq!(m.drawing_direction, DrawingDirection::TopToBottom);
            assert!((m.line_spacing_factor - 1.5).abs() < 1e-9);
            assert!((m.normal.z - (-1.0)).abs() < 1e-9);
        } else {
            panic!("Expected MText entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_lwpolyline_properties() {
        use crate::entities::lwpolyline::{LwPolyline, LwVertex};
        use crate::types::Vector2;

        let mut doc = CadDocument::new();
        let mut lwpoly = LwPolyline::new();
        lwpoly.is_closed = true;
        lwpoly.elevation = 5.0;
        lwpoly.thickness = 2.0;
        lwpoly.constant_width = 0.5;
        lwpoly.normal = Vector3::new(0.0, 1.0, 0.0);
        lwpoly.vertices = vec![
            LwVertex::new(Vector2::new(0.0, 0.0)),
            LwVertex::new(Vector2::new(10.0, 0.0)),
            LwVertex::new(Vector2::new(10.0, 10.0)),
        ];
        let _ = doc.add_entity(EntityType::LwPolyline(lwpoly));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::LwPolyline(ref lw) = entities[0] {
            assert!(lw.is_closed);
            assert!((lw.elevation - 5.0).abs() < 1e-9);
            assert!((lw.thickness - 2.0).abs() < 1e-9);
            assert!((lw.constant_width - 0.5).abs() < 1e-9);
            assert!((lw.normal.y - 1.0).abs() < 1e-9);
            assert_eq!(lw.vertices.len(), 3);
        } else {
            panic!("Expected LwPolyline entity");
        }
    }

    /// Roundtrip test: LWPOLYLINE with bulge/width on specific vertices
    #[test]
    fn test_dxf_roundtrip_lwpolyline_bulge_per_vertex() {
        use crate::entities::lwpolyline::{LwPolyline, LwVertex};
        use crate::types::Vector2;

        let mut doc = CadDocument::new();
        let mut lwpoly = LwPolyline::new();
        lwpoly.vertices = vec![
            LwVertex {
                location: Vector2::new(0.0, 0.0),
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                vertex_id: 0,
            },
            LwVertex {
                location: Vector2::new(10.0, 0.0),
                bulge: 0.5,
                start_width: 1.0,
                end_width: 2.0,
                vertex_id: 0,
            },
            LwVertex {
                location: Vector2::new(20.0, 0.0),
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                vertex_id: 0,
            },
            LwVertex {
                location: Vector2::new(30.0, 0.0),
                bulge: -0.3,
                start_width: 0.5,
                end_width: 0.5,
                vertex_id: 0,
            },
        ];
        let _ = doc.add_entity(EntityType::LwPolyline(lwpoly));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::LwPolyline(ref lw) = entities[0] {
            assert_eq!(lw.vertices.len(), 4);
            // Vertex 0: no bulge, no widths
            assert!(
                (lw.vertices[0].bulge).abs() < 1e-9,
                "v0 bulge should be 0.0, got {}",
                lw.vertices[0].bulge
            );
            assert!((lw.vertices[0].start_width).abs() < 1e-9);
            assert!((lw.vertices[0].end_width).abs() < 1e-9);
            // Vertex 1: bulge=0.5, widths=1.0/2.0
            assert!(
                (lw.vertices[1].bulge - 0.5).abs() < 1e-9,
                "v1 bulge should be 0.5, got {}",
                lw.vertices[1].bulge
            );
            assert!((lw.vertices[1].start_width - 1.0).abs() < 1e-9);
            assert!((lw.vertices[1].end_width - 2.0).abs() < 1e-9);
            // Vertex 2: no bulge, no widths
            assert!(
                (lw.vertices[2].bulge).abs() < 1e-9,
                "v2 bulge should be 0.0, got {}",
                lw.vertices[2].bulge
            );
            assert!((lw.vertices[2].start_width).abs() < 1e-9);
            assert!((lw.vertices[2].end_width).abs() < 1e-9);
            // Vertex 3: bulge=-0.3, widths=0.5/0.5
            assert!(
                (lw.vertices[3].bulge - (-0.3)).abs() < 1e-9,
                "v3 bulge should be -0.3, got {}",
                lw.vertices[3].bulge
            );
            assert!((lw.vertices[3].start_width - 0.5).abs() < 1e-9);
            assert!((lw.vertices[3].end_width - 0.5).abs() < 1e-9);
        } else {
            panic!("Expected LwPolyline entity");
        }
    }

    /// Parse hand-crafted DXF where code 42 is omitted for zero-bulge vertices.
    /// This is the exact scenario that caused the original misalignment bug.
    #[test]
    fn test_dxf_read_lwpolyline_sparse_bulge() {
        // Minimal DXF: LWPOLYLINE with 4 vertices, code 42 only on vertex 1
        let dxf = "\
  0\r\nSECTION\r\n\
  2\r\nENTITIES\r\n\
  0\r\nLWPOLYLINE\r\n\
  5\r\n1\r\n\
100\r\nAcDbEntity\r\n\
  8\r\n0\r\n\
100\r\nAcDbPolyline\r\n\
 90\r\n4\r\n\
 70\r\n0\r\n\
 38\r\n0.0\r\n\
 10\r\n0.0\r\n\
 20\r\n0.0\r\n\
 10\r\n10.0\r\n\
 20\r\n0.0\r\n\
 42\r\n0.5\r\n\
 10\r\n20.0\r\n\
 20\r\n0.0\r\n\
 10\r\n30.0\r\n\
 20\r\n0.0\r\n\
  0\r\nENDSEC\r\n\
  0\r\nEOF\r\n";

        let cursor = std::io::Cursor::new(dxf.as_bytes());
        let reader = crate::io::dxf::reader::DxfReader::from_reader(cursor).expect("from_reader");
        let doc = reader.read().expect("read");

        let entities: Vec<_> = doc.entities().collect();
        assert_eq!(
            entities.len(),
            1,
            "Expected 1 entity, got {}",
            entities.len()
        );
        if let EntityType::LwPolyline(ref lw) = entities[0] {
            assert_eq!(lw.vertices.len(), 4);
            assert!((lw.vertices[0].location.x - 0.0).abs() < 1e-9);
            assert!((lw.vertices[1].location.x - 10.0).abs() < 1e-9);
            assert!((lw.vertices[2].location.x - 20.0).abs() < 1e-9);
            assert!((lw.vertices[3].location.x - 30.0).abs() < 1e-9);
            // The critical check: bulge 0.5 must be on vertex 1, not vertex 0
            assert!(
                (lw.vertices[0].bulge).abs() < 1e-9,
                "v0 bulge should be 0.0, got {}",
                lw.vertices[0].bulge
            );
            assert!(
                (lw.vertices[1].bulge - 0.5).abs() < 1e-9,
                "v1 bulge should be 0.5, got {}",
                lw.vertices[1].bulge
            );
            assert!(
                (lw.vertices[2].bulge).abs() < 1e-9,
                "v2 bulge should be 0.0, got {}",
                lw.vertices[2].bulge
            );
            assert!(
                (lw.vertices[3].bulge).abs() < 1e-9,
                "v3 bulge should be 0.0, got {}",
                lw.vertices[3].bulge
            );
        } else {
            panic!("Expected LwPolyline entity");
        }
    }

    /// Parse hand-crafted DXF where codes 40/41/42 are all sparse across vertices.
    #[test]
    fn test_dxf_read_lwpolyline_sparse_widths_and_bulge() {
        // vertex 0: no optional codes
        // vertex 1: only code 42 (bulge)
        // vertex 2: only codes 40/41 (widths)
        // vertex 3: codes 40/41/42 all present
        let dxf = "\
  0\r\nSECTION\r\n\
  2\r\nENTITIES\r\n\
  0\r\nLWPOLYLINE\r\n\
  5\r\n1\r\n\
100\r\nAcDbEntity\r\n\
  8\r\n0\r\n\
100\r\nAcDbPolyline\r\n\
 90\r\n4\r\n\
 70\r\n0\r\n\
 38\r\n0.0\r\n\
 10\r\n0.0\r\n\
 20\r\n0.0\r\n\
 10\r\n10.0\r\n\
 20\r\n0.0\r\n\
 42\r\n0.5\r\n\
 10\r\n20.0\r\n\
 20\r\n0.0\r\n\
 40\r\n1.0\r\n\
 41\r\n2.0\r\n\
 10\r\n30.0\r\n\
 20\r\n0.0\r\n\
 40\r\n0.5\r\n\
 41\r\n0.5\r\n\
 42\r\n-0.3\r\n\
  0\r\nENDSEC\r\n\
  0\r\nEOF\r\n";

        let cursor = std::io::Cursor::new(dxf.as_bytes());
        let reader = crate::io::dxf::reader::DxfReader::from_reader(cursor).expect("from_reader");
        let doc = reader.read().expect("read");

        let entities: Vec<_> = doc.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::LwPolyline(ref lw) = entities[0] {
            assert_eq!(lw.vertices.len(), 4);
            // Vertex 0: all defaults
            assert!((lw.vertices[0].bulge).abs() < 1e-9);
            assert!((lw.vertices[0].start_width).abs() < 1e-9);
            assert!((lw.vertices[0].end_width).abs() < 1e-9);
            // Vertex 1: only bulge
            assert!(
                (lw.vertices[1].bulge - 0.5).abs() < 1e-9,
                "v1 bulge wrong: {}",
                lw.vertices[1].bulge
            );
            assert!((lw.vertices[1].start_width).abs() < 1e-9);
            assert!((lw.vertices[1].end_width).abs() < 1e-9);
            // Vertex 2: only widths
            assert!((lw.vertices[2].bulge).abs() < 1e-9);
            assert!(
                (lw.vertices[2].start_width - 1.0).abs() < 1e-9,
                "v2 start_width wrong: {}",
                lw.vertices[2].start_width
            );
            assert!(
                (lw.vertices[2].end_width - 2.0).abs() < 1e-9,
                "v2 end_width wrong: {}",
                lw.vertices[2].end_width
            );
            // Vertex 3: all present
            assert!(
                (lw.vertices[3].bulge - (-0.3)).abs() < 1e-9,
                "v3 bulge wrong: {}",
                lw.vertices[3].bulge
            );
            assert!((lw.vertices[3].start_width - 0.5).abs() < 1e-9);
            assert!((lw.vertices[3].end_width - 0.5).abs() < 1e-9);
        } else {
            panic!("Expected LwPolyline entity");
        }
    }

    #[test]
    fn test_dxf_roundtrip_linetype_and_scale() {
        let mut doc = CadDocument::new();
        let mut line = crate::entities::line::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.linetype = "DASHED".to_string();
        line.common.linetype_scale = 2.5;
        let _ = doc.add_entity(EntityType::Line(line));

        let doc2 = roundtrip(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Line(ref l) = entities[0] {
            assert_eq!(l.common.linetype, "DASHED");
            assert!((l.common.linetype_scale - 2.5).abs() < 1e-9);
        } else {
            panic!("Expected Line entity");
        }
    }

    /// Helper: write to binary DXF, read back.
    fn roundtrip_binary(doc: CadDocument) -> CadDocument {
        let writer = crate::io::dxf::writer::DxfWriter::new_binary(&doc);
        let bytes = writer.write_to_vec().expect("binary write_to_vec");
        let cursor = std::io::Cursor::new(bytes);
        let reader =
            crate::io::dxf::reader::DxfReader::from_reader(cursor).expect("binary from_reader");
        reader.read().expect("binary read")
    }

    #[test]
    fn test_binary_dxf_roundtrip_line() {
        let mut doc = CadDocument::new();
        let mut line = crate::entities::line::Line::new();
        line.start = Vector3::new(1.0, 2.0, 3.0);
        line.end = Vector3::new(4.0, 5.0, 6.0);
        line.thickness = 1.5;
        let _ = doc.add_entity(EntityType::Line(line));

        let doc2 = roundtrip_binary(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Line(ref l) = entities[0] {
            assert!((l.start.x - 1.0).abs() < 1e-9);
            assert!((l.end.z - 6.0).abs() < 1e-9);
            assert!((l.thickness - 1.5).abs() < 1e-9);
        } else {
            panic!("Expected Line entity");
        }
    }

    #[test]
    fn test_binary_dxf_roundtrip_mtext_newlines() {
        let mut doc = CadDocument::new();
        let mut mtext = crate::entities::mtext::MText::new();
        mtext.value = "Hello\nWorld".to_string();
        mtext.insertion_point = Vector3::new(10.0, 20.0, 0.0);
        let _ = doc.add_entity(EntityType::MText(mtext));

        let doc2 = roundtrip_binary(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::MText(ref m) = entities[0] {
            // Newlines should have been converted to \P paragraph markers
            assert!(
                m.value.contains("\\P"),
                "Expected \\P paragraph marker, got: {}",
                m.value
            );
            assert!(
                !m.value.contains('\n'),
                "Literal newline should not survive roundtrip"
            );
        } else {
            panic!("Expected MText entity");
        }
    }

    #[test]
    fn test_binary_dxf_roundtrip_circle() {
        let mut doc = CadDocument::new();
        let mut circle = crate::entities::circle::Circle::new();
        circle.center = Vector3::new(5.0, 10.0, 0.0);
        circle.radius = 3.5;
        let _ = doc.add_entity(EntityType::Circle(circle));

        let doc2 = roundtrip_binary(doc);
        let entities: Vec<_> = doc2.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Circle(ref c) = entities[0] {
            assert!((c.center.x - 5.0).abs() < 1e-9);
            assert!((c.center.y - 10.0).abs() < 1e-9);
            assert!((c.radius - 3.5).abs() < 1e-9);
        } else {
            panic!("Expected Circle entity");
        }
    }
}
