//! Non-graphical object readers for DWG object section.
//!
//! Each reader is the exact inverse of the corresponding writer in
//! `dwg_stream_writers/object_writer/objects.rs`. They read object-specific
//! fields after common non-entity data has already been parsed.

use super::safe_count;
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::objects::{
    Material, MaterialColor, MaterialMap, MaterialProceduralValue, MaterialTexture,
    NamedTableCellStyle, ProxyObjectReference, ProxyReferenceKind, RowCellStyle,
    TableBorderPropertyFlags, TableBorderType, TableCellBorder, TableCellStyleData,
    TableContentFormat, TableGridFormat, TableStyle, VisualStyle, VisualStyleProperty,
    VisualStylePropertyValue, XRecordEntry, XRecordValue,
};
use crate::types::{Color, DxfVersion, Vector2, Vector3};
use crate::types::{Handle, LineWeight};

fn read_visual_style_property(
    reader: &mut DwgMergedReader,
    value: VisualStylePropertyValue,
) -> VisualStyleProperty {
    VisualStyleProperty {
        value,
        enabled: reader.read_bit_short(),
    }
}

fn read_visual_style_property_long(reader: &mut DwgMergedReader) -> VisualStyleProperty {
    let value = reader.read_bit_long();
    read_visual_style_property(reader, VisualStylePropertyValue::Long(value))
}

fn read_visual_style_property_double(reader: &mut DwgMergedReader) -> VisualStyleProperty {
    let value = reader.read_bit_double();
    read_visual_style_property(reader, VisualStylePropertyValue::Double(value))
}

fn read_visual_style_property_bool(reader: &mut DwgMergedReader) -> VisualStyleProperty {
    let value = reader.read_bit();
    read_visual_style_property(reader, VisualStylePropertyValue::Bool(value))
}

fn read_visual_style_property_color(reader: &mut DwgMergedReader) -> VisualStyleProperty {
    let value = reader.read_cm_color();
    read_visual_style_property(reader, VisualStylePropertyValue::Color(value))
}

fn read_visual_style_property_text(reader: &mut DwgMergedReader) -> VisualStyleProperty {
    let value = reader.read_variable_text();
    read_visual_style_property(reader, VisualStylePropertyValue::Text(value))
}

/// Read a native AcDbVisualStyle body.  The common object data has already
/// been consumed by the caller.
pub fn read_visual_style(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> VisualStyle {
    let mut value = VisualStyle::new();
    value.description = reader.read_variable_text();
    value.style_type = reader.read_bit_long() as i16;
    if !version.r2010_plus() {
        value.face_lighting_model = reader.read_bit_long() as i16;
        value.face_lighting_quality = reader.read_bit_long() as i16;
        value.face_color_mode = reader.read_bit_long() as i16;
        value.properties.push(VisualStyleProperty {
            value: VisualStylePropertyValue::Double(reader.read_bit_double()),
            enabled: 1,
        });
        value.properties.push(VisualStyleProperty {
            value: VisualStylePropertyValue::Double(reader.read_bit_double()),
            enabled: 1,
        });
        value.properties.push(VisualStyleProperty {
            value: VisualStylePropertyValue::Color(reader.read_cm_color()),
            enabled: 1,
        });
        value.face_modifier = reader.read_bit_long();
        value.edge_model = reader.read_bit_long();
        value.edge_style = reader.read_bit_long();
        for property in [
            VisualStylePropertyValue::Color(reader.read_cm_color()),
            VisualStylePropertyValue::Color(reader.read_cm_color()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Double(reader.read_bit_double()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Color(reader.read_cm_color()),
            VisualStylePropertyValue::Double(reader.read_bit_double()),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Color(reader.read_cm_color()),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Long(reader.read_byte() as i32),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Bool(reader.read_bit()),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Short(reader.read_bit_short()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Long(reader.read_bit_long()),
            VisualStylePropertyValue::Double(reader.read_bit_double()),
        ] {
            value.properties.push(VisualStyleProperty {
                value: property,
                enabled: 1,
            });
        }
        value.internal_use_only = reader.read_bit();
        return value;
    }

    value.extended_lighting_model = reader.read_bit_short();
    value.internal_use_only = reader.read_bit();
    let mut properties = Vec::with_capacity(58);
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_double(reader));
    properties.push(read_visual_style_property_double(reader));
    properties.push(read_visual_style_property_color(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_color(reader));
    properties.push(read_visual_style_property_color(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_double(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_color(reader));
    properties.push(read_visual_style_property_double(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_color(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_bool(reader));
    properties.push(read_visual_style_property_long(reader));
    properties.push(read_visual_style_property_double(reader));
    properties.push(read_visual_style_property_long(reader));
    if version.r2013_plus(dxf_version) {
        for _ in 0..9 {
            properties.push(read_visual_style_property_bool(reader));
        }
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_double(reader));
        properties.push(read_visual_style_property_double(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_color(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_color(reader));
        properties.push(read_visual_style_property_bool(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_bool(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_color(reader));
        properties.push(read_visual_style_property_double(reader));
        properties.push(read_visual_style_property_long(reader));
        properties.push(read_visual_style_property_text(reader));
        properties.push(read_visual_style_property_bool(reader));
        properties.push(read_visual_style_property_double(reader));
        properties.push(read_visual_style_property_double(reader));
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(0)
    {
        value.face_lighting_model = *v as i16;
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(1)
    {
        value.face_lighting_quality = *v as i16;
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(2)
    {
        value.face_color_mode = *v as i16;
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(3)
    {
        value.face_modifier = *v;
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(7)
    {
        value.edge_model = *v;
    }
    if let Some(VisualStyleProperty {
        value: VisualStylePropertyValue::Long(v),
        ..
    }) = properties.get(8)
    {
        value.edge_style = *v;
    }
    value.properties = properties;
    value
}

fn read_material_color(reader: &mut DwgMergedReader) -> MaterialColor {
    let flag = reader.read_byte();
    let factor = reader.read_bit_double();
    let rgb = (flag == 1).then(|| reader.read_bit_long());
    MaterialColor { flag, factor, rgb }
}

fn read_material_texture(reader: &mut DwgMergedReader, depth: usize) -> MaterialTexture {
    let mode = reader.read_bit_short();
    let mut value = MaterialTexture {
        mode,
        ..MaterialTexture::default()
    };
    if mode == 0 || mode == 1 {
        value.color1 = read_material_color(reader);
        value.color2 = read_material_color(reader);
    } else if mode == 2 {
        let kind = reader.read_bit_short();
        value.procedural = match kind {
            1 => Some(MaterialProceduralValue::Bool(reader.read_bit())),
            2 => Some(MaterialProceduralValue::Integer(reader.read_bit_short())),
            3 => Some(MaterialProceduralValue::Real(reader.read_bit_double())),
            4 => Some(MaterialProceduralValue::Color(reader.read_cm_color())),
            5 => Some(MaterialProceduralValue::Text(reader.read_variable_text())),
            6 if depth < 8 => {
                let count = safe_count(reader.read_bit_short() as i32);
                let mut rows = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let name = reader.read_variable_text();
                    rows.push((name, read_material_texture(reader, depth + 1)));
                }
                value.table_end = reader.read_bit();
                Some(MaterialProceduralValue::Table(rows))
            }
            _ => None,
        };
    }
    value
}

fn read_material_map(reader: &mut DwgMergedReader) -> MaterialMap {
    let blend_factor = reader.read_bit_double();
    let projection = reader.read_byte();
    let tiling = reader.read_byte();
    let auto_transform = reader.read_byte();
    let mut transform = [0.0; 16];
    for item in &mut transform {
        *item = reader.read_bit_double();
    }
    let source = reader.read_byte();
    let (file_name, texture) = if source == 1 {
        (reader.read_variable_text(), None)
    } else if source == 2 {
        (String::new(), Some(read_material_texture(reader, 0)))
    } else {
        (String::new(), None)
    };
    MaterialMap {
        blend_factor,
        projection,
        tiling,
        auto_transform,
        transform,
        source,
        file_name,
        texture,
    }
}

/// Read a complete native AcDbMaterial body.
pub fn read_material(reader: &mut DwgMergedReader, version: DwgVersion) -> Material {
    let mut value = Material::new();
    value.name = reader.read_variable_text();
    value.description = reader.read_variable_text();
    value.ambient_color = read_material_color(reader);
    value.diffuse_color = read_material_color(reader);
    value.diffuse_map = read_material_map(reader);
    value.specular_color = read_material_color(reader);
    value.specular_map = read_material_map(reader);
    value.specular_gloss_factor = reader.read_bit_double();
    value.reflection_map = read_material_map(reader);
    value.opacity_percent = reader.read_bit_double();
    value.opacity_map = read_material_map(reader);
    value.bump_map = read_material_map(reader);
    value.refraction_index = reader.read_bit_double();
    value.refraction_map = read_material_map(reader);
    if version.r2007_plus() {
        value.translucence = reader.read_bit_double();
        value.self_illumination = reader.read_bit_double();
        value.reflectivity = reader.read_bit_double();
        value.illumination_model = reader.read_bit_long();
        value.channel_flags = reader.read_bit_long();
        value.mode = reader.read_bit_long();
    }
    value
}

fn read_legacy_table_row_style(reader: &mut DwgMergedReader, version: DwgVersion) -> RowCellStyle {
    let mut value = RowCellStyle::new();
    let text_style = reader.read_handle();
    value.text_style_handle = (text_style != 0).then(|| Handle::from(text_style));
    value.text_height = reader.read_bit_double();
    value.alignment = crate::objects::CellAlignment::from(reader.read_bit_short());
    value.text_color = reader.read_cm_true_color();
    value.fill_color = reader.read_cm_true_color();
    value.fill_enabled = reader.read_bit();
    let mut borders = Vec::with_capacity(6);
    for _ in 0..6 {
        borders.push(TableCellBorder {
            line_weight: LineWeight::from_value(reader.read_bit_short()),
            is_invisible: !reader.read_bit(),
            color: reader.read_cm_true_color(),
            ..TableCellBorder::default()
        });
    }
    value.top_border = borders[0].clone();
    value.horizontal_inside_border = borders[1].clone();
    value.bottom_border = borders[2].clone();
    value.left_border = borders[3].clone();
    value.vertical_inside_border = borders[4].clone();
    value.right_border = borders[5].clone();
    if version == DwgVersion::AC21 {
        value.data_type = reader.read_bit_long();
        value.unit_type = reader.read_bit_long();
        value.format_string = reader.read_variable_text();
    }
    value
}

fn read_table_content_format(reader: &mut DwgMergedReader) -> TableContentFormat {
    TableContentFormat {
        property_override_flags: reader.read_bit_long(),
        property_flags: reader.read_bit_long(),
        value_data_type: reader.read_bit_long(),
        value_unit_type: reader.read_bit_long(),
        value_format_string: reader.read_variable_text(),
        rotation: reader.read_bit_double(),
        block_scale: reader.read_bit_double(),
        cell_alignment: reader.read_bit_long(),
        content_color: reader.read_cm_true_color(),
        text_style: Handle::from(reader.read_handle()),
        text_height: reader.read_bit_double(),
    }
}

pub(super) fn read_table_cell_style_data(reader: &mut DwgMergedReader) -> TableCellStyleData {
    let mut value = TableCellStyleData {
        style_type: reader.read_bit_long(),
        data_flags: reader.read_bit_short(),
        ..TableCellStyleData::default()
    };
    if value.data_flags == 0 {
        return value;
    }
    value.property_override_flags = reader.read_bit_long();
    value.merge_flags = reader.read_bit_long();
    value.background_color = reader.read_cm_true_color();
    value.content_layout = reader.read_bit_long();
    value.content_format = read_table_content_format(reader);
    value.margin_override_flags = reader.read_bit_short();
    if value.margin_override_flags != 0 {
        value.vertical_margin = reader.read_bit_double();
        value.horizontal_margin = reader.read_bit_double();
        value.bottom_margin = reader.read_bit_double();
        value.right_margin = reader.read_bit_double();
        value.horizontal_spacing = reader.read_bit_double();
        value.vertical_spacing = reader.read_bit_double();
    }
    let count = safe_count(reader.read_bit_long()).min(6);
    value.borders.reserve(count as usize);
    for _ in 0..count {
        let index_mask = reader.read_bit_long();
        if index_mask == 0 {
            value.borders.push(TableGridFormat {
                index_mask,
                border: TableCellBorder::default(),
                line_type: Handle::NULL,
            });
            continue;
        }
        let property_flags = reader.read_bit_long();
        let border_type = reader.read_bit_long();
        let color = reader.read_cm_true_color();
        let line_weight = reader.read_bit_long();
        let line_type = reader.read_handle();
        let visible = reader.read_bit_long();
        let double_line_spacing = reader.read_bit_double();
        value.borders.push(TableGridFormat {
            index_mask,
            border: TableCellBorder {
                property_flags: TableBorderPropertyFlags::from_bits_retain(property_flags),
                border_type: TableBorderType::from(border_type as i16),
                line_weight: LineWeight::from_value(line_weight as i16),
                color,
                is_invisible: visible == 0,
                double_line_spacing,
            },
            line_type: Handle::from(line_type),
        });
    }
    value
}

pub(super) fn read_named_table_cell_style(reader: &mut DwgMergedReader) -> NamedTableCellStyle {
    NamedTableCellStyle {
        cell_style: read_table_cell_style_data(reader),
        id: reader.read_bit_long(),
        style_type: reader.read_bit_long(),
        name: reader.read_variable_text(),
    }
}

/// Read AcDbTableStyle in both the legacy three-row and R2010+ cell-style
/// layouts.
pub fn read_table_style(reader: &mut DwgMergedReader, version: DwgVersion) -> TableStyle {
    let mut value = TableStyle::new("");
    if !version.r2010_plus() {
        value.name = reader.read_variable_text();
        value.flow_direction = crate::objects::TableFlowDirection::from(reader.read_bit_short());
        value.flags = crate::objects::TableStyleFlags::from_bits_retain(reader.read_bit_short());
        value.horizontal_margin = reader.read_bit_double();
        value.vertical_margin = reader.read_bit_double();
        value.title_suppressed = reader.read_bit();
        value.header_suppressed = reader.read_bit();
        value.data_row_style = read_legacy_table_row_style(reader, version);
        value.title_row_style = read_legacy_table_row_style(reader, version);
        value.header_row_style = read_legacy_table_row_style(reader, version);
        return value;
    }
    value.modern_unknown_byte = reader.read_byte();
    value.name = reader.read_variable_text();
    value.modern_unknown_long1 = reader.read_bit_long();
    value.modern_unknown_long2 = reader.read_bit_long();
    value.modern_cell_style_handle = Handle::from(reader.read_handle());
    let modern_style = read_named_table_cell_style(reader);
    value.horizontal_margin = modern_style.cell_style.horizontal_margin;
    value.vertical_margin = modern_style.cell_style.vertical_margin;
    value.data_row_style.data_type = modern_style.cell_style.content_format.value_data_type;
    value.data_row_style.unit_type = modern_style.cell_style.content_format.value_unit_type;
    value.data_row_style.format_string = modern_style
        .cell_style
        .content_format
        .value_format_string
        .clone();
    value.data_row_style.alignment = crate::objects::CellAlignment::from(
        modern_style.cell_style.content_format.cell_alignment as i16,
    );
    value.data_row_style.text_color = modern_style.cell_style.content_format.content_color;
    value.data_row_style.text_style_handle =
        (!modern_style.cell_style.content_format.text_style.is_null())
            .then_some(modern_style.cell_style.content_format.text_style);
    value.data_row_style.text_height = modern_style.cell_style.content_format.text_height;
    value.data_row_style.fill_color = modern_style.cell_style.background_color;
    value.data_row_style.fill_enabled = modern_style.cell_style.data_flags != 0;
    for grid in &modern_style.cell_style.borders {
        match grid.index_mask {
            1 => value.data_row_style.top_border = grid.border.clone(),
            2 => value.data_row_style.right_border = grid.border.clone(),
            4 => value.data_row_style.bottom_border = grid.border.clone(),
            8 => value.data_row_style.left_border = grid.border.clone(),
            16 => value.data_row_style.horizontal_inside_border = grid.border.clone(),
            32 => value.data_row_style.vertical_inside_border = grid.border.clone(),
            _ => {}
        }
    }
    value.modern_style = Some(modern_style);
    let override_count = safe_count(reader.read_bit_long()).min(64);
    value.modern_overrides.reserve(override_count as usize);
    for _ in 0..override_count {
        let key = reader.read_bit_long();
        value
            .modern_overrides
            .push((key, read_named_table_cell_style(reader)));
    }
    value
}

// ════════════════════════════════════════════════════════════════════════
//  Result structs
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryEntry {
    pub name: String,
    pub handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryData {
    pub duplicate_cloning: i16,
    pub hard_owner: bool,
    pub entries: Vec<DictionaryEntry>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryWithDefaultData {
    pub duplicate_cloning: i16,
    pub hard_owner: bool,
    pub entries: Vec<DictionaryEntry>,
    pub default_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryVariableData {
    pub schema_number: u8,
    pub value: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlotSettingsData {
    pub page_name: String,
    pub printer_name: String,
    pub plot_flags: i16,
    pub left_margin: f64,
    pub bottom_margin: f64,
    pub right_margin: f64,
    pub top_margin: f64,
    pub paper_width: f64,
    pub paper_height: f64,
    pub paper_size: String,
    pub origin_x: f64,
    pub origin_y: f64,
    pub paper_units: i16,
    pub rotation: i16,
    pub plot_type: i16,
    pub window_min_x: f64,
    pub window_min_y: f64,
    pub window_max_x: f64,
    pub window_max_y: f64,
    pub scale_numerator: f64,
    pub scale_denominator: f64,
    pub current_style_sheet: String,
    pub scale_type: i16,
    pub scale_factor: f64,
    pub paper_image_x: f64,
    pub paper_image_y: f64,
    pub plot_view_name: String,
    pub shade_plot_mode: i16,
    pub shade_plot_resolution: i16,
    pub shade_plot_dpi: i16,
    pub plot_view_handle: u64,
    pub visual_style_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayoutData {
    pub plot_settings: PlotSettingsData,
    pub name: String,
    pub tab_order: i16,
    pub flags: i16,
    pub ucs_origin: Vector3,
    pub min_limits: (f64, f64),
    pub max_limits: (f64, f64),
    pub insertion_base: Vector3,
    pub x_axis: Vector3,
    pub y_axis: Vector3,
    pub elevation: f64,
    pub ucs_ortho_type: i16,
    pub min_extents: Vector3,
    pub max_extents: Vector3,
    pub viewport_count: i32,
    pub block_record_handle: u64,
    pub viewport_handle: u64,
    pub base_ucs_handle: u64,
    pub named_ucs_handle: u64,
    pub viewport_handles: Vec<u64>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupData {
    pub description: String,
    pub unnamed: i16,
    pub selectable: bool,
    pub entity_handles: Vec<u64>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyleElementData {
    pub offset: f64,
    pub color: Color,
    pub linetype_index_or_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyleData {
    pub name: String,
    pub description: String,
    pub flags: i16,
    pub fill_color: Color,
    pub start_angle: f64,
    pub end_angle: f64,
    pub elements: Vec<MLineStyleElementData>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderStyleData {
    pub content_type: i16,
    pub multileader_draw_order: i16,
    pub leader_draw_order: i16,
    pub max_leader_points: i32,
    pub first_segment_angle: f64,
    pub second_segment_angle: f64,
    pub path_type: i16,
    pub line_color: Color,
    pub line_type_handle: u64,
    pub line_weight: i32,
    pub enable_landing: bool,
    pub landing_gap: f64,
    pub enable_dogleg: bool,
    pub landing_distance: f64,
    pub description: String,
    pub arrowhead_handle: u64,
    pub arrowhead_size: f64,
    pub default_text: String,
    pub text_style_handle: u64,
    pub text_left_attachment: i16,
    pub text_right_attachment: i16,
    pub text_angle_type: i16,
    pub text_alignment: i16,
    pub text_color: Color,
    pub text_height: f64,
    pub text_frame: bool,
    pub text_always_left: bool,
    pub align_space: f64,
    pub block_content_handle: u64,
    pub block_content_color: Color,
    pub block_content_scale_x: f64,
    pub block_content_scale_y: f64,
    pub block_content_scale_z: f64,
    pub enable_block_scale: bool,
    pub block_content_rotation: f64,
    pub enable_block_rotation: bool,
    pub block_content_connection: i16,
    pub scale_factor: f64,
    pub property_changed: bool,
    pub is_annotative: bool,
    pub break_gap_size: f64,
    pub text_attachment_direction: i16,
    pub text_top_attachment: i16,
    pub text_bottom_attachment: i16,
    pub unknown_flag_298: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDefinitionData {
    pub class_version: i32,
    pub size_in_pixels: Vector2,
    pub file_name: String,
    pub is_loaded: bool,
    pub resolution_unit: u8,
    pub pixel_size: Vector2,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDefinitionReactorData {
    pub class_version: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleData {
    pub unknown_bs: i16,
    pub name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
    pub is_unit_scale: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SortEntitiesEntry {
    pub sort_handle: u64,
    pub entity_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SortEntitiesTableData {
    pub entries: Vec<SortEntitiesEntry>,
    pub block_owner_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XRecordData {
    pub cloning_flags: i16,
    pub data_size: i32,
    pub raw_data: Vec<u8>,
    pub entries: Vec<XRecordEntry>,
    pub object_references: Vec<ProxyObjectReference>,
    pub entries_complete: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RasterVariablesData {
    pub class_version: i32,
    pub display_image_frame: i16,
    pub image_quality: i16,
    pub units: i16,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BookColorData {
    pub color: Color,
    pub color_name: String,
    pub book_name: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WipeoutVariablesData {
    pub display_frame: i16,
}

// ════════════════════════════════════════════════════════════════════════
//  Reader functions
// ════════════════════════════════════════════════════════════════════════

/// Dictionary keys are ASCII identifiers ([A-Z0-9_:]). R13/R14 mis-sizes the
/// key string, appending a few control/high bytes from the following field
/// ("ACAD_FILTER\u{80}0…"), which broke exact-name lookups (xclip filters,
/// gradient round-trip records). The trailing garbage is always non-printable
/// or high-bit, so cut the key at the first such byte.
fn clean_dict_key(name: String) -> String {
    match name.find(|c: char| (c as u32) < 0x20 || (c as u32) > 0x7e) {
        Some(pos) => name[..pos].to_string(),
        None => name,
    }
}

pub fn read_dictionary(reader: &mut DwgMergedReader, version: DwgVersion) -> DictionaryData {
    let num_entries = safe_count(reader.read_bit_long());

    let mut duplicate_cloning = 0i16;
    let mut hard_owner = false;
    if reader.dxf_version() == DxfVersion::AC1014 {
        hard_owner = reader.read_byte() != 0;
    } else if version.r2000_plus() {
        duplicate_cloning = reader.read_bit_short();
        hard_owner = reader.read_byte() != 0;
    }

    let mut entries = Vec::with_capacity(num_entries as usize);
    for _ in 0..num_entries {
        let name = clean_dict_key(reader.read_variable_text());
        let handle = reader.read_handle();
        entries.push(DictionaryEntry { name, handle });
    }

    DictionaryData {
        duplicate_cloning,
        hard_owner,
        entries,
    }
}

pub fn read_dictionary_with_default(reader: &mut DwgMergedReader) -> DictionaryWithDefaultData {
    let num_entries = safe_count(reader.read_bit_long());
    let duplicate_cloning = reader.read_bit_short();
    let hard_owner = reader.read_byte() != 0;

    let mut entries = Vec::with_capacity(num_entries as usize);
    for _ in 0..num_entries {
        let name = clean_dict_key(reader.read_variable_text());
        let handle = reader.read_handle();
        entries.push(DictionaryEntry { name, handle });
    }

    let default_handle = reader.read_handle();

    DictionaryWithDefaultData {
        duplicate_cloning,
        hard_owner,
        entries,
        default_handle,
    }
}

pub fn read_dictionary_variable(reader: &mut DwgMergedReader) -> DictionaryVariableData {
    let schema_number = reader.read_byte();
    let value = reader.read_variable_text();
    DictionaryVariableData {
        schema_number,
        value,
    }
}

/// Read an AcDbBlockVisibilityParameter object body (after the common
/// non-entity header). Follows the class chain
/// AcDbEvalExpr → AcDbBlockElement → AcDbBlockParameter →
/// AcDbBlock1PtParameter → AcDbBlockVisibilityParameter.
///
/// Numeric fields come from the main stream, text from the text stream, and
/// handles from the handle stream — the merged reader keeps the three cursors
/// independent, so reads in spec order self-align across the substreams.
///
/// `handle`/`owner` are filled by the caller. Returns the parsed parameter.
pub fn read_block_visibility_parameter(
    reader: &mut DwgMergedReader,
) -> crate::objects::BlockVisibilityParameter {
    use crate::objects::{BlockVisibilityParameter, BlockVisibilityState};
    let mut p = BlockVisibilityParameter::default();

    // ── AcDbEvalExpr ──
    p.eval_parent_id = reader.read_bit_long();
    p.eval_major = reader.read_bit_long();
    p.eval_minor = reader.read_bit_long();
    p.eval_value_code = reader.read_bit_short();
    p.eval_value = match p.eval_value_code {
        40 => crate::objects::BlockEvalValue::Real(reader.read_bit_double()),
        10 | 11 => {
            let point = reader.read_2raw_double();
            crate::objects::BlockEvalValue::Point([point.x, point.y])
        }
        1 => crate::objects::BlockEvalValue::Text(reader.read_variable_text()),
        90 => crate::objects::BlockEvalValue::Long(reader.read_bit_long()),
        91 => {
            crate::objects::BlockEvalValue::Handle(crate::types::Handle::from(reader.read_handle()))
        }
        70 => crate::objects::BlockEvalValue::Short(reader.read_bit_short()),
        _ => crate::objects::BlockEvalValue::None,
    };
    p.eval_node_id = reader.read_bit_long();

    // ── AcDbBlockElement ──
    p.element_name = reader.read_variable_text();
    p.element_major = reader.read_bit_long();
    p.element_minor = reader.read_bit_long();
    p.element_eed_1071 = reader.read_bit_long();

    // ── AcDbBlockParameter ──
    p.show_properties = reader.read_bit();
    p.chain_actions = reader.read_bit();

    // ── AcDbBlock1PtParameter ──
    p.def_point = reader.read_3bit_double(); // 3BD def_pt (1010)
                                             // Two PropInfo blocks: each is a BL connection count + (BL code, T name) pairs.
    for property_index in 0..2 {
        let n = safe_count(reader.read_bit_long());
        for _ in 0..n {
            p.property_info[property_index].connections.push(
                crate::objects::BlockParameterConnection {
                    code: reader.read_bit_long(),
                    name: reader.read_variable_text(),
                },
            );
        }
    }
    p.property_info_count = reader.read_bit_long();

    // ── AcDbBlockVisibilityParameter ──
    p.is_initialized = reader.read_bit();
    p.name = reader.read_variable_text(); // T blockvisi_name (301)
    p.description = reader.read_variable_text(); // T blockvisi_desc (302)
    p.unknown_bool = reader.read_bit();

    let num_blocks = safe_count(reader.read_bit_long()); // BL num_blocks (93)
    for _ in 0..num_blocks {
        p.all_blocks
            .push(crate::types::Handle::from(reader.read_handle()));
    }

    let num_states = safe_count(reader.read_bit_long()); // BL num_states (92)
    for _ in 0..num_states {
        let mut st = BlockVisibilityState {
            name: reader.read_variable_text(), // T state name (303)
            ..Default::default()
        };
        let nb = safe_count(reader.read_bit_long()); // BL (94)
        for _ in 0..nb {
            st.visible_blocks
                .push(crate::types::Handle::from(reader.read_handle()));
        }
        let np = safe_count(reader.read_bit_long()); // BL (95)
        for _ in 0..np {
            st.visible_params
                .push(crate::types::Handle::from(reader.read_handle()));
        }
        p.states.push(st);
    }

    p
}

/// Read an AcDbBlockRepresentationData object body (after the common
/// non-entity header) and return the handle of the dynamic block definition
/// it represents (group code 340, a hard pointer). This is the link from an
/// anonymous evaluated block back to its dynamic block definition.
///
/// Layout: BS flag (70), then the block handle from the handle stream.
pub fn read_block_representation_data(reader: &mut DwgMergedReader) -> crate::types::Handle {
    let _flag = reader.read_bit_short(); // BS (70)
    crate::types::Handle::from(reader.read_handle()) // H block (3, 340)
}

/// Decoded leading portion of an `AcDbField`.
pub struct FieldReadData {
    /// Evaluator id (DXF 1) — `"AcVar"`, `"AcDiesel"`, `"AcExpr"`, `"AcObjProp"`.
    pub id: String,
    /// Field-code / template string (DXF 2).
    pub code: String,
    /// Referenced-object handles (DXF 331), targeted by `%<\_ObjIdx N>%` markers
    /// in an `AcObjProp` field. Empty for most fields.
    pub objects: Vec<u64>,
}

/// Read the leading `AcDbField` data: evaluator id (T, 1), field-code string
/// (T, 2), and the referenced-object handle vector (needed by `AcObjProp`). The
/// value data set and cached value string are left unread — the caller
/// preserves the whole record verbatim and recovers the container→child
/// structure from object owners.
pub fn read_field(reader: &mut DwgMergedReader) -> FieldReadData {
    let id = reader.read_variable_text();
    let code = reader.read_variable_text();
    let num_childs = safe_count(reader.read_bit_long());
    let num_objects = safe_count(reader.read_bit_long());
    // Object-specific handles follow the common owner/reactor/xdict handles in
    // the handle stream: the child fields first (recovered instead from object
    // owners, so skipped here), then the referenced objects.
    for _ in 0..num_childs {
        let _ = reader.read_handle();
    }
    let mut objects = Vec::with_capacity(num_objects.max(0) as usize);
    for _ in 0..num_objects {
        objects.push(reader.read_handle());
    }
    FieldReadData { id, code, objects }
}

/// Read the PlotSettings data portion (shared by Layout and standalone PlotSettings).
pub fn read_plot_settings_data(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> PlotSettingsData {
    let page_name = reader.read_variable_text();
    let printer_name = reader.read_variable_text();
    let plot_flags = reader.read_bit_short();

    let left_margin = reader.read_bit_double();
    let bottom_margin = reader.read_bit_double();
    let right_margin = reader.read_bit_double();
    let top_margin = reader.read_bit_double();

    let paper_width = reader.read_bit_double();
    let paper_height = reader.read_bit_double();

    let paper_size = reader.read_variable_text();

    let origin_x = reader.read_bit_double();
    let origin_y = reader.read_bit_double();

    let paper_units = reader.read_bit_short();
    let rotation = reader.read_bit_short();
    let plot_type = reader.read_bit_short();

    let window_min_x = reader.read_bit_double();
    let window_min_y = reader.read_bit_double();
    let window_max_x = reader.read_bit_double();
    let window_max_y = reader.read_bit_double();

    let plot_view_name = if version.r13_15_only() {
        reader.read_variable_text()
    } else {
        String::new()
    };

    let scale_numerator = reader.read_bit_double();
    let scale_denominator = reader.read_bit_double();
    let current_style_sheet = reader.read_variable_text();
    let scale_type = reader.read_bit_short();
    let scale_factor = reader.read_bit_double();
    let paper_image_x = reader.read_bit_double();
    let paper_image_y = reader.read_bit_double();

    let mut shade_plot_mode = 0i16;
    let mut shade_plot_resolution = 0i16;
    let mut shade_plot_dpi = 0i16;
    let mut plot_view_handle = 0u64;
    if version.r2004_plus() {
        shade_plot_mode = reader.read_bit_short();
        shade_plot_resolution = reader.read_bit_short();
        shade_plot_dpi = reader.read_bit_short();
        plot_view_handle = reader.read_handle();
    }
    let visual_style_handle = if version.r2007_plus() {
        reader.read_handle()
    } else {
        0
    };

    PlotSettingsData {
        page_name,
        printer_name,
        plot_flags,
        left_margin,
        bottom_margin,
        right_margin,
        top_margin,
        paper_width,
        paper_height,
        paper_size,
        origin_x,
        origin_y,
        paper_units,
        rotation,
        plot_type,
        window_min_x,
        window_min_y,
        window_max_x,
        window_max_y,
        scale_numerator,
        scale_denominator,
        current_style_sheet,
        scale_type,
        scale_factor,
        paper_image_x,
        paper_image_y,
        plot_view_name,
        shade_plot_mode,
        shade_plot_resolution,
        shade_plot_dpi,
        plot_view_handle,
        visual_style_handle,
    }
}

pub fn read_layout(reader: &mut DwgMergedReader, version: DwgVersion) -> LayoutData {
    let plot_settings = read_plot_settings_data(reader, version);

    let name = reader.read_variable_text();
    let tab_order = reader.read_bit_short();
    let flags = reader.read_bit_short();

    let insertion_base = reader.read_3bit_double();

    let min_lim_x = reader.read_raw_double();
    let min_lim_y = reader.read_raw_double();
    let max_lim_x = reader.read_raw_double();
    let max_lim_y = reader.read_raw_double();

    let ucs_origin = reader.read_3bit_double();
    let x_axis = reader.read_3bit_double();
    let y_axis = reader.read_3bit_double();
    let elevation = reader.read_bit_double();
    let ucs_ortho_type = reader.read_bit_short();
    let min_extents = reader.read_3bit_double();
    let max_extents = reader.read_3bit_double();

    let viewport_count = if version.r2004_plus() {
        safe_count(reader.read_bit_long())
    } else {
        0
    };

    let block_record_handle = reader.read_handle();
    let viewport_handle = reader.read_handle();
    let base_ucs_handle = reader.read_handle();
    let named_ucs_handle = reader.read_handle();

    // R2004+: viewport handles
    let mut viewport_handles = Vec::with_capacity(viewport_count.max(0) as usize);
    if version.r2004_plus() {
        for _ in 0..viewport_count {
            viewport_handles.push(reader.read_handle());
        }
    }

    LayoutData {
        plot_settings,
        name,
        tab_order,
        flags,
        ucs_origin,
        min_limits: (min_lim_x, min_lim_y),
        max_limits: (max_lim_x, max_lim_y),
        insertion_base,
        x_axis,
        y_axis,
        elevation,
        ucs_ortho_type,
        min_extents,
        max_extents,
        viewport_count,
        block_record_handle,
        viewport_handle,
        base_ucs_handle,
        named_ucs_handle,
        viewport_handles,
    }
}

pub fn read_plot_settings_obj(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> PlotSettingsData {
    read_plot_settings_data(reader, version)
}

pub fn read_group(reader: &mut DwgMergedReader) -> GroupData {
    let description = reader.read_variable_text();
    let unnamed = reader.read_bit_short();
    let selectable = reader.read_bit_short() != 0;

    let num_entities = safe_count(reader.read_bit_long());
    let mut entity_handles = Vec::with_capacity(num_entities as usize);
    for _ in 0..num_entities {
        entity_handles.push(reader.read_handle());
    }

    GroupData {
        description,
        unnamed,
        selectable,
        entity_handles,
    }
}

pub fn read_mlinestyle(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> MLineStyleData {
    let name = reader.read_variable_text();
    let description = reader.read_variable_text();
    let flags = reader.read_bit_short();
    let fill_color = reader.read_cm_color();
    let start_angle = reader.read_bit_double();
    let end_angle = reader.read_bit_double();

    let num_elements = reader.read_byte();
    let mut elements = Vec::with_capacity(num_elements as usize);
    for _ in 0..num_elements {
        let offset = reader.read_bit_double();
        let color = reader.read_cm_color();
        let linetype_index_or_handle = if version.r2018_plus(dxf_version) {
            reader.read_handle()
        } else {
            reader.read_bit_short() as u64
        };
        elements.push(MLineStyleElementData {
            offset,
            color,
            linetype_index_or_handle,
        });
    }

    MLineStyleData {
        name,
        description,
        flags,
        fill_color,
        start_angle,
        end_angle,
        elements,
    }
}

pub fn read_multileader_style(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: crate::types::DxfVersion,
) -> MultiLeaderStyleData {
    // R2010+: Version (BS, expected 2)
    if version.r2010_plus() {
        let _style_version = reader.read_bit_short();
    }

    let content_type = reader.read_bit_short();
    let multileader_draw_order = reader.read_bit_short();
    let leader_draw_order = reader.read_bit_short();
    let max_leader_points = reader.read_bit_long();
    let first_segment_angle = reader.read_bit_double();
    let second_segment_angle = reader.read_bit_double();
    let path_type = reader.read_bit_short();
    let line_color = reader.read_cm_color();
    let line_type_handle = reader.read_handle();
    let line_weight = reader.read_bit_long();
    let enable_landing = reader.read_bit();
    let landing_gap = reader.read_bit_double();
    let enable_dogleg = reader.read_bit();
    let landing_distance = reader.read_bit_double();
    let description = reader.read_variable_text();
    let arrowhead_handle = reader.read_handle();
    let arrowhead_size = reader.read_bit_double();
    let default_text = reader.read_variable_text();
    let text_style_handle = reader.read_handle();
    let text_left_attachment = reader.read_bit_short();
    let text_right_attachment = reader.read_bit_short();
    let text_angle_type = reader.read_bit_short();
    let text_alignment = reader.read_bit_short();
    let text_color = reader.read_cm_color();
    let text_height = reader.read_bit_double();
    let text_frame = reader.read_bit();
    let text_always_left = reader.read_bit();
    let align_space = reader.read_bit_double();
    let block_content_handle = reader.read_handle();
    let block_content_color = reader.read_cm_color();
    let block_content_scale_x = reader.read_bit_double();
    let block_content_scale_y = reader.read_bit_double();
    let block_content_scale_z = reader.read_bit_double();
    let enable_block_scale = reader.read_bit();
    let block_content_rotation = reader.read_bit_double();
    let enable_block_rotation = reader.read_bit();
    let block_content_connection = reader.read_bit_short();
    let scale_factor = reader.read_bit_double();
    let property_changed = reader.read_bit();
    let is_annotative = reader.read_bit();
    let break_gap_size = reader.read_bit_double();

    let mut text_attachment_direction = 0i16;
    let mut text_top_attachment = 0i16;
    let mut text_bottom_attachment = 0i16;
    if version.r2010_plus() {
        text_attachment_direction = reader.read_bit_short();
        text_top_attachment = reader.read_bit_short();
        text_bottom_attachment = reader.read_bit_short();
    }

    let mut unknown_flag_298 = false;
    if dxf_version >= crate::types::DxfVersion::AC1027 {
        unknown_flag_298 = reader.read_bit();
    }

    MultiLeaderStyleData {
        content_type,
        multileader_draw_order,
        leader_draw_order,
        max_leader_points,
        first_segment_angle,
        second_segment_angle,
        path_type,
        line_color,
        line_type_handle,
        line_weight,
        enable_landing,
        landing_gap,
        enable_dogleg,
        landing_distance,
        description,
        arrowhead_handle,
        arrowhead_size,
        default_text,
        text_style_handle,
        text_left_attachment,
        text_right_attachment,
        text_angle_type,
        text_alignment,
        text_color,
        text_height,
        text_frame,
        text_always_left,
        align_space,
        block_content_handle,
        block_content_color,
        block_content_scale_x,
        block_content_scale_y,
        block_content_scale_z,
        enable_block_scale,
        block_content_rotation,
        enable_block_rotation,
        block_content_connection,
        scale_factor,
        property_changed,
        is_annotative,
        break_gap_size,
        text_attachment_direction,
        text_top_attachment,
        text_bottom_attachment,
        unknown_flag_298,
    }
}

pub fn read_image_definition(reader: &mut DwgMergedReader) -> ImageDefinitionData {
    let class_version = reader.read_bit_long();
    let size_in_pixels = reader.read_2raw_double();
    let file_name = reader.read_variable_text();
    let is_loaded = reader.read_bit();
    let resolution_unit = reader.read_byte();
    let pixel_size = reader.read_2raw_double();

    ImageDefinitionData {
        class_version,
        size_in_pixels,
        file_name,
        is_loaded,
        resolution_unit,
        pixel_size,
    }
}

/// Decoded body of an underlay definition object (AcDbUnderlayDefinition).
/// The three flavours (PDF/DWF/DGN) share this identical two-string layout.
pub struct UnderlayDefinitionData {
    pub file_path: String,
    pub page_name: String,
}

/// Read an underlay definition (PDF/DWF/DGN): file path then page/sheet name,
/// both variable text. The common non-entity header is consumed by the caller.
pub fn read_underlay_definition(reader: &mut DwgMergedReader) -> UnderlayDefinitionData {
    let file_path = reader.read_variable_text();
    let page_name = reader.read_variable_text();
    UnderlayDefinitionData {
        file_path,
        page_name,
    }
}

pub fn read_image_definition_reactor(reader: &mut DwgMergedReader) -> ImageDefinitionReactorData {
    let class_version = reader.read_bit_long();
    ImageDefinitionReactorData { class_version }
}

pub fn read_scale(reader: &mut DwgMergedReader) -> ScaleData {
    let unknown_bs = reader.read_bit_short();
    let name = reader.read_variable_text();
    let paper_units = reader.read_bit_double();
    let drawing_units = reader.read_bit_double();
    let is_unit_scale = reader.read_bit();
    ScaleData {
        unknown_bs,
        name,
        paper_units,
        drawing_units,
        is_unit_scale,
    }
}

pub fn read_sort_entities_table(reader: &mut DwgMergedReader) -> SortEntitiesTableData {
    let num_entries = safe_count(reader.read_bit_long());
    // The per-entry sort handles are stored inline in the DATA section; the
    // owner block and the sorted entity handles follow in the handle stream
    // (owner first, then one handle per entry). The previous code read a
    // sort+entity pair per entry from the handle stream and the owner last,
    // which scrambled the order and the owner block on files written by other
    // CAD apps — draw order was effectively ignored. (#146)
    let mut sort_handles = Vec::with_capacity(num_entries as usize);
    for _ in 0..num_entries {
        sort_handles.push(reader.read_main_handle());
    }
    let block_owner_handle = reader.read_handle();
    let mut entries = Vec::with_capacity(num_entries as usize);
    for k in 0..num_entries as usize {
        let entity_handle = reader.read_handle();
        entries.push(SortEntitiesEntry {
            sort_handle: sort_handles[k],
            entity_handle,
        });
    }
    SortEntitiesTableData {
        entries,
        block_owner_handle,
    }
}

fn decode_xrecord_entries(raw: &[u8], unicode: bool) -> (Vec<XRecordEntry>, bool) {
    let read_u16 = |p: usize| u16::from_le_bytes([raw[p], raw[p + 1]]);
    let mut entries = Vec::new();
    let mut position = 0usize;
    let mut complete = true;
    'entries: while position + 2 <= raw.len() {
        let entry_start = position;
        macro_rules! require {
            ($size:expr) => {
                if position.saturating_add($size) > raw.len() {
                    position = entry_start;
                    complete = false;
                    break 'entries;
                }
            };
        }
        let code = read_u16(position) as i16 as i32;
        position += 2;
        let value = match code {
            code if code < 0
                || code == 5
                || code == 105
                || (320..=369).contains(&code)
                || (390..=399).contains(&code)
                || (480..=481).contains(&code)
                || code == 1005 =>
            {
                require!(8);
                let value = u64::from_le_bytes(raw[position..position + 8].try_into().unwrap());
                position += 8;
                XRecordValue::Handle(Handle::from(value))
            }
            0..=4
            | 6..=9
            | 100..=102
            | 300..=309
            | 410..=419
            | 430..=439
            | 470..=479
            | 999
            | 1000..=1003 => {
                require!(2);
                let length = read_u16(position) as usize;
                position += 2;
                let text = if unicode {
                    require!(length.saturating_mul(2));
                    let mut units = Vec::with_capacity(length);
                    for _ in 0..length {
                        units.push(read_u16(position));
                        position += 2;
                    }
                    String::from_utf16_lossy(&units)
                } else {
                    require!(1usize.saturating_add(length));
                    let code_page = raw[position] as u16;
                    position += 1;
                    let value = crate::io::dxf::code_page::encoding_from_dwg_code_page(code_page)
                        .decode(&raw[position..position + length])
                        .0
                        .into_owned();
                    position += length;
                    value
                };
                XRecordValue::String(text)
            }
            10..=37 | 110..=139 | 210..=269 | 1010..=1039 | 1043..=1069 => {
                require!(24);
                let x = f64::from_le_bytes(raw[position..position + 8].try_into().unwrap());
                let y = f64::from_le_bytes(raw[position + 8..position + 16].try_into().unwrap());
                let z = f64::from_le_bytes(raw[position + 16..position + 24].try_into().unwrap());
                position += 24;
                XRecordValue::Point3D(x, y, z)
            }
            38..=59 | 140..=149 | 460..=469 | 1040..=1042 => {
                require!(8);
                let value = f64::from_le_bytes(raw[position..position + 8].try_into().unwrap());
                position += 8;
                XRecordValue::Double(value)
            }
            150..=169 => {
                require!(8);
                let value = i64::from_le_bytes(raw[position..position + 8].try_into().unwrap());
                position += 8;
                XRecordValue::Int64(value)
            }
            60..=79 | 170..=179 | 270..=279 | 370..=389 | 400..=409 | 1070 => {
                require!(2);
                let value = i16::from_le_bytes(raw[position..position + 2].try_into().unwrap());
                position += 2;
                XRecordValue::Int16(value)
            }
            80..=99 | 420..=429 | 440..=459 | 1071 => {
                require!(4);
                let value = i32::from_le_bytes(raw[position..position + 4].try_into().unwrap());
                position += 4;
                XRecordValue::Int32(value)
            }
            280..=289 => {
                require!(1);
                let value = raw[position];
                position += 1;
                XRecordValue::Byte(value)
            }
            290..=299 => {
                require!(1);
                let value = raw[position] != 0;
                position += 1;
                XRecordValue::Bool(value)
            }
            310..=319 | 1004 => {
                require!(1);
                let length = raw[position] as usize;
                position += 1;
                require!(length);
                let value = raw[position..position + length].to_vec();
                position += length;
                XRecordValue::Chunk(value)
            }
            _ => {
                position = entry_start;
                complete = false;
                break;
            }
        };
        entries.push(XRecordEntry { code, value });
    }
    if position != raw.len() {
        complete = false;
    }
    (entries, complete)
}

pub fn read_xrecord(reader: &mut DwgMergedReader) -> XRecordData {
    // This field is a byte length, not an array item count. Real application
    // payloads (FBXASSET in particular) routinely exceed MAX_ARRAY_COUNT.
    // Bound corrupt declarations by the containing object's main-data stream
    // instead of truncating valid XRecords at 100,000 bytes.
    let declared_size = reader.read_bit_long().max(0) as usize;
    let available_size = (reader.main_remaining_bits().max(0) as usize) / 8;
    let data_size = declared_size.min(available_size);
    let mut raw_data = Vec::with_capacity(data_size);
    for _ in 0..data_size {
        raw_data.push(reader.read_byte());
    }
    let cloning_flags = if reader.dxf_version() >= DxfVersion::AC1015 {
        reader.read_bit_short()
    } else {
        0
    };
    let (mut entries, entries_complete) =
        decode_xrecord_entries(&raw_data, reader.dxf_version() >= DxfVersion::AC1021);
    let mut object_references = Vec::new();
    while reader.handle_remaining_bits() >= 8 {
        let (handle, reference_type) = reader.read_typed_handle();
        if handle == 0 {
            break;
        }
        let kind = match reference_type {
            crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined => {
                ProxyReferenceKind::Undefined
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftOwnership => {
                ProxyReferenceKind::SoftOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardOwnership => {
                ProxyReferenceKind::HardOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftPointer => {
                ProxyReferenceKind::SoftPointer
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer => {
                ProxyReferenceKind::HardPointer
            }
        };
        object_references.push(ProxyObjectReference {
            handle: Handle::from(handle),
            kind,
        });
    }
    for (entry, reference) in entries
        .iter_mut()
        .filter(|entry| (330..=369).contains(&entry.code))
        .zip(object_references.iter())
    {
        entry.value = XRecordValue::Handle(reference.handle);
    }
    XRecordData {
        cloning_flags,
        data_size: data_size.min(i32::MAX as usize) as i32,
        raw_data,
        entries,
        object_references,
        entries_complete,
    }
}

pub fn read_raster_variables(reader: &mut DwgMergedReader) -> RasterVariablesData {
    let class_version = reader.read_bit_long();
    let display_image_frame = reader.read_bit_short();
    let image_quality = reader.read_bit_short();
    let units = reader.read_bit_short();
    RasterVariablesData {
        class_version,
        display_image_frame,
        image_quality,
        units,
    }
}

pub fn read_placeholder(_reader: &mut DwgMergedReader) {
    // PlaceHolder has no object-specific data
}

pub fn read_book_color(reader: &mut DwgMergedReader) -> BookColorData {
    let color_index = reader.read_bit_short();
    if reader.dxf_version() >= DxfVersion::AC1018 {
        let true_color = reader.read_bit_long() as u32;
        let flags = reader.read_byte();
        let color_name = if flags & 1 != 0 {
            reader.read_variable_text()
        } else {
            String::new()
        };
        let book_name = if flags & 2 != 0 {
            reader.read_variable_text()
        } else {
            String::new()
        };
        BookColorData {
            color: Color::from_rgb(
                ((true_color >> 16) & 0xFF) as u8,
                ((true_color >> 8) & 0xFF) as u8,
                (true_color & 0xFF) as u8,
            ),
            color_name,
            book_name,
        }
    } else {
        BookColorData {
            color: Color::from_index(color_index),
            color_name: String::new(),
            book_name: String::new(),
        }
    }
}

pub fn read_wipeout_variables(reader: &mut DwgMergedReader) -> WipeoutVariablesData {
    let display_frame = reader.read_bit_short();
    WipeoutVariablesData { display_frame }
}

/// GeoData (AcDbGeoData) object-specific fields.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeoDataData {
    pub version: i32,
    pub host_block: u64,
    pub coordinate_type: i16,
    pub design_point: Vector3,
    pub reference_point: Vector3,
    pub obsolete_observation_point: Vector3,
    pub obsolete_scale_vector: Vector3,
    pub north_direction: Vector2,
    pub up_direction: Vector3,
    pub horizontal_unit_scale: f64,
    pub vertical_unit_scale: f64,
    pub horizontal_units: i32,
    pub vertical_units: i32,
    pub scale_estimation_method: i32,
    pub user_scale_factor: f64,
    pub sea_level_correction: bool,
    pub sea_level_elevation: f64,
    pub coordinate_projection_radius: f64,
    pub coordinate_system_definition: String,
    pub coordinate_system_datum: String,
    pub coordinate_system_wkt: String,
    pub geo_rss_tag: String,
    pub observation_from_tag: String,
    pub observation_to_tag: String,
    pub observation_coverage_tag: String,
    pub mesh_points: Vec<(Vector2, Vector2)>,
    pub mesh_faces: Vec<(i32, i32, i32)>,
    pub civil_data_present: bool,
    pub civil_obsolete_flag: bool,
    pub civil_reference_point1: Vector2,
    pub civil_reference_point2: Vector2,
    pub civil_unknown1: i32,
    pub civil_unknown2: i32,
    pub civil_unknown_flag1: bool,
    pub civil_zero_point1: Vector2,
    pub civil_zero_point2: Vector2,
    pub civil_unknown_flag2: bool,
    pub civil_north_angle_degrees: f64,
    pub civil_north_angle_radians: f64,
}

/// Read the AcDbGeoData object body (after common non-entity data).
///
/// Ported from the reference `DwgObjectReader.readGeoData`. The field order is
/// version-dependent: R2010/R2013 store the coordinate system as a MapGuide XML
/// string, R2009 as a WKT PROJCS string. Trailing geo-mesh points/faces are not
/// read (not needed for the coordinate system; the per-object reader is bounded).
pub fn read_geodata(reader: &mut DwgMergedReader) -> GeoDataData {
    let mut d = GeoDataData {
        horizontal_unit_scale: 1.0,
        vertical_unit_scale: 1.0,
        user_scale_factor: 1.0,
        obsolete_scale_vector: Vector3::new(1.0, 1.0, 1.0),
        ..Default::default()
    };

    // BL object version
    d.version = reader.read_bit_long();
    // H soft pointer to host block
    d.host_block = reader.read_handle();
    // BS design coordinate type
    d.coordinate_type = reader.read_bit_short();

    if d.version == 1 {
        // R2009
        d.reference_point = reader.read_3bit_double();
        d.horizontal_units = reader.read_bit_long();
        d.vertical_units = d.horizontal_units;
        d.design_point = reader.read_3bit_double();
        d.obsolete_observation_point = reader.read_3bit_double();
        d.up_direction = reader.read_3bit_double();
        // BD angle of north direction (radians, clockwise from (0,1))
        let angle = std::f64::consts::FRAC_PI_2 - reader.read_bit_double();
        d.north_direction = Vector2::new(angle.cos(), angle.sin());
        d.obsolete_scale_vector = reader.read_3bit_double();
        d.coordinate_system_definition = reader.read_variable_text();
        d.geo_rss_tag = reader.read_variable_text();
        d.horizontal_unit_scale = reader.read_bit_double();
        d.vertical_unit_scale = d.horizontal_unit_scale;
        d.coordinate_system_datum = reader.read_variable_text();
        d.coordinate_system_wkt = reader.read_variable_text();
    } else {
        // R2010 / R2013 (and newer)
        d.design_point = reader.read_3bit_double();
        d.reference_point = reader.read_3bit_double();
        d.horizontal_unit_scale = reader.read_bit_double();
        d.horizontal_units = reader.read_bit_long();
        d.vertical_unit_scale = reader.read_bit_double();
        d.vertical_units = reader.read_bit_long();
        d.up_direction = reader.read_3bit_double();
        d.north_direction = reader.read_2raw_double();
        d.scale_estimation_method = reader.read_bit_long();
        d.user_scale_factor = reader.read_bit_double();
        d.sea_level_correction = reader.read_bit();
        d.sea_level_elevation = reader.read_bit_double();
        d.coordinate_projection_radius = reader.read_bit_double();
        d.coordinate_system_definition = reader.read_variable_text();
        d.geo_rss_tag = reader.read_variable_text();
    }

    d.observation_from_tag = reader.read_variable_text();
    d.observation_to_tag = reader.read_variable_text();
    d.observation_coverage_tag = reader.read_variable_text();
    let point_count = safe_count(reader.read_bit_long()).min(50_000);
    d.mesh_points.reserve(point_count as usize);
    for _ in 0..point_count {
        d.mesh_points
            .push((reader.read_2raw_double(), reader.read_2raw_double()));
    }
    let face_count = safe_count(reader.read_bit_long()).min(50_000);
    d.mesh_faces.reserve(face_count as usize);
    for _ in 0..face_count {
        d.mesh_faces.push((
            reader.read_bit_long(),
            reader.read_bit_long(),
            reader.read_bit_long(),
        ));
    }
    if d.version == 1 {
        d.civil_data_present = reader.read_bit();
        d.civil_obsolete_flag = reader.read_bit();
        d.civil_reference_point1 = reader.read_2raw_double();
        d.civil_reference_point2 = reader.read_2raw_double();
        d.civil_unknown1 = reader.read_bit_long();
        d.civil_unknown2 = reader.read_bit_long();
        d.civil_unknown_flag1 = reader.read_bit();
        d.civil_zero_point1 = reader.read_2raw_double();
        d.civil_zero_point2 = reader.read_2raw_double();
        d.civil_unknown_flag2 = reader.read_bit();
        d.civil_north_angle_degrees = reader.read_bit_double();
        d.civil_north_angle_radians = reader.read_bit_double();
        d.scale_estimation_method = reader.read_bit_long();
        d.user_scale_factor = reader.read_bit_double();
        d.sea_level_correction = reader.read_bit();
        d.sea_level_elevation = reader.read_bit_double();
        d.coordinate_projection_radius = reader.read_bit_double();
    }
    d
}

/// SpatialFilter (AcDbSpatialFilter) object-specific fields — the XCLIP clip
/// boundary attached to a block reference.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpatialFilterData {
    pub points: Vec<Vector2>,
    pub extrusion: Vector3,
    pub clip_bound_origin: Vector3,
    pub display_enabled: bool,
    pub front_clip: Option<f64>,
    pub back_clip: Option<f64>,
    /// 12 doubles, column-major 4×3 inverse block transform.
    pub inverse_block_transform: [f64; 12],
    /// 12 doubles, column-major 4×3 clip bound transform.
    pub clip_bound_transform: [f64; 12],
}

/// Read the AcDbSpatialFilter object body (after common non-entity data).
///
/// Field order (ODA spec): point count (BS), `count` boundary points (2RD),
/// boundary plane normal (3BD), clip bound origin (3BD), display-enabled flag
/// (BS), front-clip flag (BS) + optional distance (BD), back-clip flag (BS) +
/// optional distance (BD), then the inverse-block and clip-bound transforms as
/// 12 doubles each (BD). The three flags mirror DXF codes 71/72/73 (i16), so
/// they are bit-shorts, not single bits. The transforms use the same
/// column-major layout as DXF code 40.
pub fn read_spatial_filter(reader: &mut DwgMergedReader) -> SpatialFilterData {
    let mut d = SpatialFilterData::default();
    let num = safe_count(reader.read_bit_short() as i32);
    for _ in 0..num {
        d.points.push(reader.read_2raw_double());
    }
    d.extrusion = reader.read_3bit_double();
    d.clip_bound_origin = reader.read_3bit_double();
    d.display_enabled = reader.read_bit_short() != 0;
    if reader.read_bit_short() != 0 {
        d.front_clip = Some(reader.read_bit_double());
    }
    if reader.read_bit_short() != 0 {
        d.back_clip = Some(reader.read_bit_double());
    }
    for i in 0..12 {
        d.inverse_block_transform[i] = reader.read_bit_double();
    }
    for i in 0..12 {
        d.clip_bound_transform[i] = reader.read_bit_double();
    }
    d
}

// ════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_reference_type::DwgReferenceType;
    use crate::io::dwg::dwg_stream_writers::merged_writer::DwgMergedWriter;
    use crate::io::dwg::dwg_version::DwgVersion;
    use crate::types::DxfVersion;

    fn make_reader(
        dwg: DwgVersion,
        dxf: DxfVersion,
        f: impl FnOnce(&mut DwgMergedWriter),
    ) -> DwgMergedReader {
        let mut writer = DwgMergedWriter::new(dwg, dxf);
        f(&mut writer);
        let data = writer.merge();
        let hsb = writer.handle_start_bits();
        DwgMergedReader::new(data, dxf, hsb)
    }

    #[test]
    fn test_dictionary_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(2); // 2 entries
            w.write_bit_short(1); // cloning
            w.write_byte(0); // not hard owner
            w.write_variable_text("ACAD_GROUP");
            w.write_handle(DwgReferenceType::SoftOwnership, 0x10);
            w.write_variable_text("ACAD_MLINESTYLE");
            w.write_handle(DwgReferenceType::SoftOwnership, 0x20);
        });
        let dict = read_dictionary(&mut r, v);
        assert_eq!(dict.entries.len(), 2);
        assert_eq!(dict.entries[0].name, "ACAD_GROUP");
        assert_eq!(dict.entries[1].name, "ACAD_MLINESTYLE");
        assert_eq!(dict.duplicate_cloning, 1);
        assert!(!dict.hard_owner);
    }

    #[test]
    fn test_dictionary_variable_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_byte(0); // schema number
            w.write_variable_text("test_value");
        });
        let dv = read_dictionary_variable(&mut r);
        assert_eq!(dv.schema_number, 0);
        assert_eq!(dv.value, "test_value");
    }

    #[test]
    fn test_geodata_roundtrip_r2013() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let csd = "<Dictionary><ProjectedCoordinateSystem id=\"MO83-WF\"/></Dictionary>";
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(3); // object version (R2013)
            w.write_handle(DwgReferenceType::SoftOwnership, 0x30); // host block
            w.write_bit_short(2); // coordinate type
            w.write_3bit_double(Vector3::new(1.0, 2.0, 3.0)); // design point
            w.write_3bit_double(Vector3::new(4.0, 5.0, 6.0)); // reference point
            w.write_bit_double(1.0); // horizontal unit scale
            w.write_bit_long(9); // horizontal units
            w.write_bit_double(1.0); // vertical unit scale
            w.write_bit_long(9); // vertical units
            w.write_3bit_double(Vector3::new(0.0, 0.0, 1.0)); // up direction
            w.write_2raw_double(Vector2::new(0.0, 1.0)); // north direction
            w.write_bit_long(0); // scale estimation method
            w.write_bit_double(1.0); // user scale factor
            w.write_bit(false); // sea-level correction
            w.write_bit_double(0.0); // sea-level elevation
            w.write_bit_double(6378137.0); // coordinate projection radius
            w.write_variable_text(csd); // coordinate system definition
            w.write_variable_text("georss-tag"); // geo rss tag
            w.write_variable_text("from"); // observation from
            w.write_variable_text("to"); // observation to
            w.write_variable_text("cov"); // observation coverage
            w.write_bit_long(0); // transformation mesh points
            w.write_bit_long(0); // transformation mesh faces
        });
        let g = read_geodata(&mut r);
        assert_eq!(g.version, 3);
        assert_eq!(g.coordinate_type, 2);
        assert_eq!(g.design_point, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(g.reference_point, Vector3::new(4.0, 5.0, 6.0));
        assert_eq!(g.coordinate_system_definition, csd);
        assert_eq!(g.geo_rss_tag, "georss-tag");
        assert_eq!(g.observation_coverage_tag, "cov");
    }

    #[test]
    fn test_spatial_filter_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_short(2); // num boundary points (rectangular clip)
            w.write_2raw_double(Vector2::new(-5.0, -5.0));
            w.write_2raw_double(Vector2::new(5.0, 5.0));
            w.write_3bit_double(Vector3::new(0.0, 0.0, 1.0)); // extrusion
            w.write_3bit_double(Vector3::new(0.0, 0.0, 0.0)); // clip bound origin
            w.write_bit_short(1); // display enabled
            w.write_bit_short(1); // front clip on
            w.write_bit_double(2.5); // front clip dist
            w.write_bit_short(0); // back clip off
            for i in 0..12 {
                w.write_bit_double(i as f64);
            } // inverse block transform
            for i in 0..12 {
                w.write_bit_double((i + 100) as f64);
            } // clip bound transform
        });
        let sf = read_spatial_filter(&mut r);
        assert_eq!(sf.points.len(), 2);
        assert_eq!(sf.points[0], Vector2::new(-5.0, -5.0));
        assert_eq!(sf.points[1], Vector2::new(5.0, 5.0));
        assert_eq!(sf.extrusion, Vector3::new(0.0, 0.0, 1.0));
        assert!(sf.display_enabled);
        assert_eq!(sf.front_clip, Some(2.5));
        assert_eq!(sf.back_clip, None);
        assert_eq!(sf.inverse_block_transform[11], 11.0);
        assert_eq!(sf.clip_bound_transform[0], 100.0);
    }

    #[test]
    fn test_group_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_variable_text("My Group");
            w.write_bit_short(1); // unnamed
            w.write_bit_short(1); // selectable
            w.write_bit_long(2); // 2 entities
            w.write_handle(DwgReferenceType::HardPointer, 0xA0);
            w.write_handle(DwgReferenceType::HardPointer, 0xB0);
        });
        let g = read_group(&mut r);
        assert_eq!(g.description, "My Group");
        assert!(g.selectable);
        assert_eq!(g.entity_handles.len(), 2);
    }

    #[test]
    fn test_scale_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_short(0); // unknown
            w.write_variable_text("1:1");
            w.write_bit_double(1.0);
            w.write_bit_double(1.0);
            w.write_bit(true);
        });
        let s = read_scale(&mut r);
        assert_eq!(s.name, "1:1");
        assert_eq!(s.paper_units, 1.0);
        assert_eq!(s.drawing_units, 1.0);
        assert!(s.is_unit_scale);
    }

    #[test]
    fn test_xrecord_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(0); // data size (comes first per spec)
            w.write_bit_short(0); // cloning flags (comes after data)
        });
        let xr = read_xrecord(&mut r);
        assert_eq!(xr.cloning_flags, 0);
        assert_eq!(xr.data_size, 0);
    }

    #[test]
    fn test_raster_variables_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(0); // class version
            w.write_bit_short(1); // display frame
            w.write_bit_short(0); // quality
            w.write_bit_short(3); // units
        });
        let rv = read_raster_variables(&mut r);
        assert_eq!(rv.class_version, 0);
        assert_eq!(rv.display_image_frame, 1);
        assert_eq!(rv.units, 3);
    }

    #[test]
    fn test_wipeout_variables_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_short(1);
        });
        let wv = read_wipeout_variables(&mut r);
        assert_eq!(wv.display_frame, 1);
    }

    #[test]
    fn test_image_definition_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(0); // class version
            w.write_2raw_double(Vector2::new(1024.0, 768.0)); // size
            w.write_variable_text("test.png"); // filename
            w.write_bit(true); // is_loaded
            w.write_byte(3); // resolution_unit
            w.write_2raw_double(Vector2::new(1.0, 1.0)); // pixel_size
        });
        let def = read_image_definition(&mut r);
        assert_eq!(def.file_name, "test.png");
        assert!(def.is_loaded);
        assert_eq!(def.size_in_pixels.x, 1024.0);
    }

    #[test]
    fn test_sort_entities_table_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(1); // 1 entry
                                 // Layout: sort handles in the DATA section, then owner + entities in
                                 // the handle stream.
            w.write_main_handle(DwgReferenceType::SoftPointer, 0x10); // sort (data)
            w.write_handle(DwgReferenceType::HardPointer, 0x30); // block owner
            w.write_handle(DwgReferenceType::SoftPointer, 0x20); // entity handle
        });
        let st = read_sort_entities_table(&mut r);
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.block_owner_handle, 0x30);
        assert_eq!(st.entries[0].sort_handle, 0x10);
        assert_eq!(st.entries[0].entity_handle, 0x20);
    }
}
