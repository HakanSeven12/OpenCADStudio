//! Entity readers for DWG object section.
//!
//! Each reader is the exact inverse of the corresponding writer in
//! `dwg_stream_writers/object_writer/entities.rs`. They read entity-specific
//! fields after common entity data has already been parsed.

use super::{safe_count, MAX_MESH_FACES, MAX_MESH_FACE_INDICES};
use crate::entities::multileader::*;
use crate::entities::solid3d::{AcisMaterial, AcisRevision, Silhouette, Wire, WireType};
use crate::entities::table::{
    BorderPropertyFlags, BorderType, CellBorder, CellContent, CellContentGeometry, CellEdgeFlags,
    CellStateFlags, CellStyle, CellStylePropertyFlags, CellStyleType, CellType, CellValue,
    CellValueType, ContentLayoutFlags, LegacyBorderOverrides, LegacyTableStyleOverride,
    TableAttribute, TableBreakData, TableBreakRange, TableCell, TableCellContentType, TableColumn,
    TableCustomData, TableRow, ValueUnitType,
};
use crate::entities::{
    ArcAlignedTextData, CoordinationModelData, ExtendedEntityData, GeoPositionMarkerData,
    LayoutPrintConfigData, LightPhotometricData, OleFrameData, PointCloudClip, PointCloudData,
    PointCloudExCrop, PointCloudExData, ProxyEntityData, RegisteredClassEntityData, RemoteTextData,
    SectionObjectData, SurfaceData, SurfaceKind, SurfaceSweepOptions,
};
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::{Color, DxfVersion, Handle, LineWeight, Vector2, Vector3};

// ════════════════════════════════════════════════════════════════════════
//  Result structs
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointData {
    pub location: Vector3,
    pub thickness: f64,
    pub normal: Vector3,
    pub x_axis_angle: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineData {
    pub start: Vector3,
    pub end: Vector3,
    pub thickness: f64,
    pub normal: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CircleData {
    pub center: Vector3,
    pub radius: f64,
    pub thickness: f64,
    pub normal: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArcData {
    pub center: Vector3,
    pub radius: f64,
    pub thickness: f64,
    pub normal: Vector3,
    pub start_angle: f64,
    pub end_angle: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EllipseData {
    pub center: Vector3,
    pub major_axis: Vector3,
    pub normal: Vector3,
    pub minor_axis_ratio: f64,
    pub start_parameter: f64,
    pub end_parameter: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RayData {
    pub base_point: Vector3,
    pub direction: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XLineData {
    pub base_point: Vector3,
    pub direction: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidData {
    pub thickness: f64,
    pub elevation: f64,
    pub first_corner: Vector2,
    pub second_corner: Vector2,
    pub third_corner: Vector2,
    pub fourth_corner: Vector2,
    pub normal: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Face3DData {
    pub first_corner: Vector3,
    pub second_corner: Vector3,
    pub third_corner: Vector3,
    pub fourth_corner: Vector3,
    pub invisible_edges: i16,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InsertData {
    pub insert_point: Vector3,
    pub x_scale: f64,
    pub y_scale: f64,
    pub z_scale: f64,
    pub rotation: f64,
    pub normal: Vector3,
    pub has_attribs: bool,
    pub block_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MInsertData {
    pub insert: InsertData,
    pub column_count: i16,
    pub row_count: i16,
    pub column_spacing: f64,
    pub row_spacing: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LwPolylineVertex {
    pub x: f64,
    pub y: f64,
    pub bulge: f64,
    pub start_width: f64,
    pub end_width: f64,
    pub vertex_id: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LwPolylineData {
    pub flag: i16,
    pub constant_width: f64,
    pub elevation: f64,
    pub thickness: f64,
    pub normal: Vector3,
    pub vertices: Vec<LwPolylineVertex>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SplineData {
    pub scenario: i32,
    pub degree: i32,
    pub rational: bool,
    pub closed: bool,
    pub periodic: bool,
    pub knot_tolerance: f64,
    pub control_tolerance: f64,
    pub knots: Vec<f64>,
    pub control_points: Vec<Vector3>,
    pub weights: Vec<f64>,
    pub fit_tolerance: f64,
    pub begin_tangent: Vector3,
    pub end_tangent: Vector3,
    pub fit_points: Vec<Vector3>,
    /// Knot parameterization method (R2013+): 0=Chord, 1=SquareRoot,
    /// 2=Uniform, 15=Custom. Zero for pre-R2013 splines.
    pub knot_param: i32,
    pub flags1: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextEntityData {
    pub insertion_point: Vector3,
    pub alignment_point: Vector3,
    pub normal: Vector3,
    pub thickness: f64,
    pub oblique_angle: f64,
    pub rotation: f64,
    pub height: f64,
    pub width_factor: f64,
    pub value: String,
    pub generation: i16,
    pub horizontal_alignment: i16,
    pub vertical_alignment: i16,
    pub style_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextData {
    pub insertion_point: Vector3,
    pub normal: Vector3,
    pub x_direction: Vector3,
    pub rectangle_width: f64,
    pub rectangle_height: f64,
    pub height: f64,
    pub attachment_point: i16,
    pub drawing_direction: i16,
    pub extents_height: f64,
    pub extents_width: f64,
    pub value: String,
    pub style_handle: u64,
    pub linespacing_style: i16,
    pub linespacing_factor: f64,
    pub unknown_bit: bool,
    pub background_flags: i32,
    pub background_scale: f64,
    pub background_color: Color,
    pub background_transparency: i32,
    pub is_annotative: bool,
    pub column_type: i16,
    pub column_count: i32,
    pub column_flow_reversed: bool,
    pub column_auto_height: bool,
    pub column_width: f64,
    pub column_gutter: f64,
    pub column_heights: Vec<f64>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShapeData {
    pub insertion_point: Vector3,
    pub size: f64,
    pub rotation: f64,
    pub relative_x_scale: f64,
    pub oblique_angle: f64,
    pub thickness: f64,
    pub shape_number: i16,
    pub normal: Vector3,
    pub style_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LeaderData {
    pub unknown_bit: bool,
    pub annotation_type: i16,
    pub path_type: i16,
    pub vertices: Vec<Vector3>,
    pub origin: Vector3,
    pub normal: Vector3,
    pub horizontal_direction: Vector3,
    pub block_offset: Vector3,
    pub annotation_offset: Vector3,
    pub text_height: f64,
    pub text_width: f64,
    pub hookline_on_x_dir: bool,
    pub arrowhead_on: bool,
    pub annotation_handle: u64,
    pub dimstyle_handle: u64,
    pub dimgap: f64,
    pub arrowhead_type: i16,
    pub dimasz: f64,
    pub unknown_bit2: bool,
    pub unknown_bit3: bool,
    pub unknown_short1: i16,
    pub byblock_color: i16,
    pub unknown_bit4: bool,
    pub unknown_bit5: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ToleranceData {
    pub insertion_point: Vector3,
    pub direction: Vector3,
    pub normal: Vector3,
    pub text: String,
    pub dimstyle_handle: u64,
    pub unknown_short: i16,
    pub text_height: f64,
    pub dimgap: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LightData {
    pub class_version: i32,
    pub name: String,
    pub light_type: i32,
    pub status: bool,
    pub light_color: Color,
    pub plot_glyph: bool,
    pub intensity: f64,
    pub position: Vector3,
    pub target: Vector3,
    pub attenuation_type: i32,
    pub use_attenuation_limits: bool,
    pub attenuation_start_limit: f64,
    pub attenuation_end_limit: f64,
    pub hotspot_angle: f64,
    pub falloff_angle: f64,
    pub cast_shadows: bool,
    pub shadow_type: i32,
    pub shadow_map_size: i16,
    pub shadow_map_softness: u8,
    pub photometric_mode: bool,
    pub photometric_data: Option<LightPhotometricData>,
}

// ════════════════════════════════════════════════════════════════════════
//  Reader functions — Simple entities
// ════════════════════════════════════════════════════════════════════════

pub fn read_point(reader: &mut DwgMergedReader) -> PointData {
    let location = reader.read_3bit_double();
    let thickness = reader.read_bit_thickness();
    let normal = reader.read_bit_extrusion();
    let x_axis_angle = reader.read_bit_double();
    PointData {
        location,
        thickness,
        normal,
        x_axis_angle,
    }
}

pub fn read_light(reader: &mut DwgMergedReader, photometric_mode: bool) -> LightData {
    let class_version = reader.read_bit_long();
    let name = reader.read_variable_text();
    let light_type = reader.read_bit_long();
    let status = reader.read_bit();
    let light_color = reader.read_cm_color();
    let plot_glyph = reader.read_bit();
    let intensity = reader.read_bit_double();
    let position = reader.read_3bit_double();
    let target = reader.read_3bit_double();
    let attenuation_type = reader.read_bit_long();
    let use_attenuation_limits = reader.read_bit();
    let attenuation_start_limit = reader.read_bit_double();
    let attenuation_end_limit = reader.read_bit_double();
    let hotspot_angle = reader.read_bit_double();
    let falloff_angle = reader.read_bit_double();
    let cast_shadows = reader.read_bit();
    let shadow_type = reader.read_bit_long();
    let shadow_map_size = reader.read_bit_short();
    let shadow_map_softness = reader.read_byte();

    let photometric_data = if photometric_mode && reader.read_bit() {
        let has_web_file = reader.read_bit();
        let web_file = reader.read_variable_text();
        let physical_intensity_method = reader.read_bit_short();
        let physical_intensity = reader.read_bit_double();
        let illuminance_distance = reader.read_bit_double();
        let lamp_color_type = reader.read_bit_short();
        let lamp_color_temperature = reader.read_bit_double();
        let lamp_color_preset = reader.read_bit_short();
        let web_rotation = reader.read_3bit_double();
        let extended_light_shape = reader.read_bit_short();
        let extended_light_length = reader.read_bit_double();
        let extended_light_width = reader.read_bit_double();
        let extended_light_radius = reader.read_bit_double();
        let web_file_type = reader.read_bit_short();
        let web_symmetry = reader.read_bit_short();
        let has_target_grip = reader.read_bit_short();
        let web_flux = reader.read_bit_double();
        let mut web_angles = [0.0; 5];
        for angle in &mut web_angles {
            *angle = reader.read_bit_double();
        }
        let glyph_display_type = reader.read_bit_short();
        Some(LightPhotometricData {
            has_web_file,
            web_file,
            physical_intensity_method,
            physical_intensity,
            illuminance_distance,
            lamp_color_type,
            lamp_color_temperature,
            lamp_color_preset,
            web_rotation,
            extended_light_shape,
            extended_light_length,
            extended_light_width,
            extended_light_radius,
            web_file_type,
            web_symmetry,
            has_target_grip,
            web_flux,
            web_angles,
            glyph_display_type,
        })
    } else {
        None
    };
    LightData {
        class_version,
        name,
        light_type,
        status,
        light_color,
        plot_glyph,
        intensity,
        position,
        target,
        attenuation_type,
        use_attenuation_limits,
        attenuation_start_limit,
        attenuation_end_limit,
        hotspot_angle,
        falloff_angle,
        cast_shadows,
        shadow_type,
        shadow_map_size,
        shadow_map_softness,
        photometric_mode,
        photometric_data,
    }
}

pub fn read_camera(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    ExtendedEntityData::Camera {
        view_handle: Handle::new(reader.read_handle()),
    }
}

pub fn read_section_object(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    let state = reader.read_bit_long();
    let flags = reader.read_bit_long();
    let name = reader.read_variable_text();
    let vertical_direction = reader.read_3bit_double();
    let top_height = reader.read_bit_double();
    let bottom_height = reader.read_bit_double();
    let indicator_alpha = reader.read_bit_short();
    let indicator_color = reader.read_cm_color();
    let vertex_count = safe_count(reader.read_bit_long()) as usize;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(reader.read_3bit_double());
    }
    let back_line_count = safe_count(reader.read_bit_long()) as usize;
    let mut back_line_vertices = Vec::with_capacity(back_line_count);
    for _ in 0..back_line_count {
        back_line_vertices.push(reader.read_3bit_double());
    }
    ExtendedEntityData::SectionObject(SectionObjectData {
        state,
        flags,
        name,
        vertical_direction,
        top_height,
        bottom_height,
        indicator_alpha,
        indicator_color,
        vertices,
        back_line_vertices,
        settings_handle: Handle::new(reader.read_handle()),
    })
}

pub fn read_arc_aligned_text(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    let text_size = reader.read_bit_double();
    let x_scale = reader.read_bit_double();
    let character_spacing = reader.read_bit_double();
    let style_name = reader.read_variable_text();
    let font_name = reader.read_variable_text();
    let big_font_name = reader.read_variable_text();
    let text = reader.read_variable_text();
    let offset_from_arc = reader.read_bit_double();
    let right_offset = reader.read_bit_double();
    let left_offset = reader.read_bit_double();
    let center = reader.read_3bit_double();
    let radius = reader.read_bit_double();
    let start_angle = reader.read_bit_double();
    let end_angle = reader.read_bit_double();
    let normal = reader.read_3bit_double();
    let text_color = reader.read_bit_long();
    let character_set = reader.read_bit_short();
    let pitch_and_family = reader.read_bit_short();
    let is_shx = reader.read_bit_short() != 0;
    let bold = reader.read_bit_short() != 0;
    let italic = reader.read_bit_short() != 0;
    let underlined = reader.read_bit_short() != 0;
    let alignment = reader.read_bit_short();
    let reverse = reader.read_bit_short() != 0;
    let wizard_flag = reader.read_bit_short() != 0;
    let text_position = reader.read_bit_short();
    let text_direction = reader.read_bit_short();
    let arc_handle = Handle::new(reader.read_handle());
    ExtendedEntityData::ArcAlignedText(ArcAlignedTextData {
        text,
        font_name,
        big_font_name,
        style_name,
        center,
        radius,
        x_scale,
        text_size,
        character_spacing,
        offset_from_arc,
        right_offset,
        left_offset,
        start_angle,
        end_angle,
        reverse,
        text_direction,
        alignment,
        text_position,
        bold,
        italic,
        underlined,
        character_set,
        pitch_and_family,
        is_shx,
        text_color,
        normal,
        wizard_flag,
        arc_handle,
    })
}

pub fn read_remote_text(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    let position = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let rotation = reader.read_bit_double();
    let height = reader.read_bit_double();
    let flags = reader.read_bit_short();
    let text = reader.read_variable_text();
    let style_handle = Handle::new(reader.read_handle());
    ExtendedEntityData::RemoteText(RemoteTextData {
        position,
        normal,
        rotation,
        height,
        style_handle,
        style_name: String::new(),
        flags,
        text,
    })
}

#[derive(Debug, Clone)]
pub struct GeoPositionMarkerReadData {
    pub data: GeoPositionMarkerData,
    pub embedded_mtext: Option<MTextData>,
}

pub fn read_geo_position_marker(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> GeoPositionMarkerReadData {
    let class_version = reader.read_bit_long();
    let position = reader.read_3bit_double();
    let radius = reader.read_bit_double();
    let notes = reader.read_variable_text();
    let landing_gap = reader.read_bit_double();
    let mtext_visible = reader.read_bit();
    let text_alignment = reader.read_byte();
    let enable_frame_text = reader.read_bit();
    let embedded_mtext = if enable_frame_text {
        Some(read_embedded_mtext(reader, version, dxf_version))
    } else {
        None
    };
    GeoPositionMarkerReadData {
        data: GeoPositionMarkerData {
            class_version,
            position,
            radius,
            notes,
            landing_gap,
            mtext_visible,
            text_alignment,
            enable_frame_text,
            embedded_mtext: None,
        },
        embedded_mtext,
    }
}

pub fn read_coordination_model(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    let flags = reader.read_bit_short();
    let definition_handle = Handle::new(reader.read_handle());
    let mut transform = [0.0; 16];
    for value in &mut transform {
        *value = reader.read_bit_double();
    }
    let unit_factor = reader.read_bit_double();
    ExtendedEntityData::CoordinationModel(CoordinationModelData {
        flags,
        definition_handle,
        transform,
        unit_factor,
    })
}

pub fn read_point_cloud(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> ExtendedEntityData {
    let class_version = reader.read_bit_short();
    let origin = reader.read_3bit_double();
    let saved_filename = reader.read_variable_text();
    let source_count = safe_count(reader.read_bit_long()) as usize;
    let mut extents_min = Vector3::ZERO;
    let mut extents_max = Vector3::ZERO;
    let mut point_count = 0;
    let mut ucs_name = String::new();
    let mut ucs_origin = Vector3::ZERO;
    let mut ucs_x_direction = Vector3::UNIT_X;
    let mut ucs_y_direction = Vector3::UNIT_Y;
    let mut ucs_z_direction = Vector3::UNIT_Z;
    let mut definition_handle = Handle::NULL;
    let mut reactor_handle = Handle::NULL;
    let mut show_intensity = false;
    let mut intensity_scheme = 0;
    let mut minimum_intensity = 0.0;
    let mut maximum_intensity = 0.0;
    let mut low_intensity_threshold = 0.0;
    let mut high_intensity_threshold = 0.0;
    let mut show_clipping = false;
    let mut clippings = Vec::new();

    if source_count == 0 {
        extents_min = reader.read_3bit_double();
        extents_max = reader.read_3bit_double();
        point_count = reader.read_bit_long_long();
        ucs_name = reader.read_variable_text();
        ucs_origin = reader.read_3bit_double();
        ucs_x_direction = reader.read_3bit_double();
        ucs_y_direction = reader.read_3bit_double();
        ucs_z_direction = reader.read_3bit_double();
        if version.r2013_plus(dxf_version) {
            definition_handle = Handle::new(reader.read_handle());
            reactor_handle = Handle::new(reader.read_handle());
            show_intensity = reader.read_bit();
            intensity_scheme = reader.read_bit_short();
            minimum_intensity = reader.read_bit_double();
            maximum_intensity = reader.read_bit_double();
            low_intensity_threshold = reader.read_bit_double();
            high_intensity_threshold = reader.read_bit_double();
            show_clipping = reader.read_bit();
            let clip_count = safe_count(reader.read_bit_long()) as usize;
            clippings.reserve(clip_count);
            for _ in 0..clip_count {
                let inverted = reader.read_bit();
                let clip_type = reader.read_bit_short();
                let vertex_count = if clip_type == 3 {
                    safe_count(reader.read_bit_long()) as usize
                } else {
                    2
                };
                let mut vertices = Vec::with_capacity(vertex_count);
                for _ in 0..vertex_count {
                    vertices.push(reader.read_2raw_double());
                }
                let (z_min, z_max) = if clip_type == 1 {
                    (reader.read_bit_double(), reader.read_bit_double())
                } else {
                    (0.0, 0.0)
                };
                clippings.push(PointCloudClip {
                    inverted,
                    clip_type,
                    vertices,
                    z_min,
                    z_max,
                });
            }
        }
    }

    let mut source_files = Vec::with_capacity(source_count);
    for _ in 0..source_count {
        source_files.push(reader.read_variable_text());
    }
    ExtendedEntityData::PointCloud(PointCloudData {
        class_version,
        origin,
        saved_filename,
        source_files,
        extents_min,
        extents_max,
        point_count,
        ucs_name,
        ucs_origin,
        ucs_x_direction,
        ucs_y_direction,
        ucs_z_direction,
        definition_handle,
        reactor_handle,
        show_intensity,
        intensity_scheme,
        minimum_intensity,
        maximum_intensity,
        low_intensity_threshold,
        high_intensity_threshold,
        show_clipping,
        clippings,
    })
}

pub fn read_point_cloud_ex(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    let class_version = reader.read_bit_short();
    let extents_min = reader.read_3bit_double();
    let extents_max = reader.read_3bit_double();
    let ucs_origin = reader.read_3bit_double();
    let ucs_x_direction = reader.read_3bit_double();
    let ucs_y_direction = reader.read_3bit_double();
    let ucs_z_direction = reader.read_3bit_double();
    let locked = reader.read_bit();
    let definition_handle = Handle::new(reader.read_handle());
    let reactor_handle = Handle::new(reader.read_handle());
    let name = reader.read_variable_text();
    let show_intensity = reader.read_bit();
    let show_cropping = reader.read_bit();
    let crop_count = safe_count(reader.read_bit_long()) as usize;
    let mut unknown_bl0 = 0;
    let mut unknown_bl1 = 0;
    let mut stylization_type = 0;
    let mut intensity_color_scheme = String::new();
    let mut current_color_scheme = String::new();
    let mut classification_color_scheme = String::new();
    let mut elevation_min = 0.0;
    let mut elevation_max = 0.0;
    let mut intensity_min = 0;
    let mut intensity_max = 0;
    let mut intensity_out_of_range_behavior = 0;
    let mut elevation_out_of_range_behavior = 0;
    let mut elevation_apply_to_fixed_range = false;
    let mut intensity_as_gradient = false;
    let mut elevation_as_gradient = false;
    if crop_count == 0 {
        unknown_bl0 = reader.read_bit_long();
        unknown_bl1 = reader.read_bit_long();
        stylization_type = reader.read_bit_short();
        intensity_color_scheme = reader.read_variable_text();
        current_color_scheme = reader.read_variable_text();
        classification_color_scheme = reader.read_variable_text();
        elevation_min = reader.read_bit_double();
        elevation_max = reader.read_bit_double();
        intensity_min = reader.read_bit_long();
        intensity_max = reader.read_bit_long();
        intensity_out_of_range_behavior = reader.read_bit_short();
        elevation_out_of_range_behavior = reader.read_bit_short();
        elevation_apply_to_fixed_range = reader.read_bit();
        intensity_as_gradient = reader.read_bit();
        elevation_as_gradient = reader.read_bit();
    }
    let mut croppings = Vec::with_capacity(crop_count);
    for _ in 0..crop_count {
        let crop_type = reader.read_bit_short();
        let inside = reader.read_bit();
        let inverted = reader.read_bit();
        let plane = reader.read_3bit_double();
        let x_direction = reader.read_3bit_double();
        let y_direction = reader.read_3bit_double();
        let point_count = safe_count(reader.read_bit_long()) as usize;
        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            points.push(reader.read_3bit_double());
        }
        croppings.push(PointCloudExCrop {
            crop_type,
            inside,
            inverted,
            plane,
            x_direction,
            y_direction,
            points,
        });
    }
    ExtendedEntityData::PointCloudEx(PointCloudExData {
        class_version,
        extents_min,
        extents_max,
        ucs_origin,
        ucs_x_direction,
        ucs_y_direction,
        ucs_z_direction,
        locked,
        definition_handle,
        reactor_handle,
        name,
        show_intensity,
        show_cropping,
        unknown_bl0,
        unknown_bl1,
        stylization_type,
        intensity_color_scheme,
        current_color_scheme,
        classification_color_scheme,
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
    })
}

pub fn read_ole_frame(reader: &mut DwgMergedReader, version: DwgVersion) -> ExtendedEntityData {
    let flag = reader.read_bit_short();
    let mode = if version.r2000_plus() {
        reader.read_bit_short()
    } else {
        0
    };
    let size = safe_count(reader.read_bit_long()) as usize;
    ExtendedEntityData::OleFrame(OleFrameData {
        flag,
        mode,
        storage: crate::compound_file::StructuredStoragePayload::decode(&reader.read_bytes(size)),
    })
}

pub fn read_layout_print_config(reader: &mut DwgMergedReader) -> ExtendedEntityData {
    ExtendedEntityData::LayoutPrintConfig(LayoutPrintConfigData {
        class_version: reader.read_bit_short(),
        flag: reader.read_bit_short(),
        raw_dwg_data: None,
        raw_dwg_handle_bits: 0,
        raw_dwg_version: None,
    })
}

pub fn read_proxy_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    proxy_data: Vec<u8>,
    current_handle: u64,
) -> ExtendedEntityData {
    let class_id = reader.read_bit_long();
    let dxf_subclass = if dxf_version > DxfVersion::AC1015 {
        reader.read_variable_text()
    } else {
        String::new()
    };
    let (version_value, dwg_version, maintenance_version) = if version.r2018_plus(dxf_version) {
        let dwg = reader.read_bit_long();
        let maintenance = reader.read_bit_long();
        ((maintenance << 16) | (dwg & 0xffff), dwg, maintenance)
    } else {
        let combined = reader.read_bit_long();
        (combined, combined & 0xffff, combined >> 16)
    };
    let from_dxf = if version.r2000_plus() {
        reader.read_bit()
    } else {
        false
    };
    let object_data_bits = reader.main_remaining_bits() as u32;
    let mut object_data = vec![0u8; object_data_bits.div_ceil(8) as usize];
    for bit_index in 0..object_data_bits as usize {
        if reader.read_bit() {
            object_data[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    let text_data_bits = reader.text_remaining_bits() as u32;
    let mut text_data = vec![0u8; text_data_bits.div_ceil(8) as usize];
    for bit_index in 0..text_data_bits as usize {
        if reader.read_text_bit() {
            text_data[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    let mut object_ids = Vec::new();
    while reader.handle_remaining_bits() >= 8 {
        let (handle, reference_type) = reader.read_handle_reference(current_handle);
        let kind = match reference_type {
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftOwnership => {
                crate::objects::ProxyReferenceKind::SoftOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardOwnership => {
                crate::objects::ProxyReferenceKind::HardOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftPointer => {
                crate::objects::ProxyReferenceKind::SoftPointer
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer => {
                crate::objects::ProxyReferenceKind::HardPointer
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined => {
                crate::objects::ProxyReferenceKind::Undefined
            }
        };
        object_ids.push(crate::objects::ProxyObjectReference {
            handle: Handle::from(handle),
            kind,
        });
    }
    let payload = crate::objects::ProxyPayload::from_bits(&object_data, object_data_bits);
    if let Some(envelope) =
        crate::objects::semantic_property::decode_registered_class_envelope(&payload)
    {
        return ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
            dxf_name: envelope.dxf_name,
            cpp_class_name: envelope.cpp_class_name,
            properties: envelope.properties,
            payload: envelope.payload,
            object_ids,
        });
    }
    ExtendedEntityData::Proxy(ProxyEntityData {
        proxy_id: 498,
        class_id,
        dxf_subclass,
        version: version_value,
        dwg_version,
        maintenance_version,
        from_dxf,
        graphics: crate::objects::ProxyPayload::from_bytes(&proxy_data),
        payload,
        text_payload: crate::objects::ProxyPayload::from_bits(&text_data, text_data_bits),
        object_ids,
    })
}

pub fn read_line(reader: &mut DwgMergedReader, version: DwgVersion) -> LineData {
    let (start, end);
    if version.r13_14_only() {
        start = reader.read_3bit_double();
        end = reader.read_3bit_double();
    } else {
        let z_are_zero = reader.read_bit();
        let sx = reader.read_raw_double();
        let ex = reader.read_bit_double_with_default(sx);
        let sy = reader.read_raw_double();
        let ey = reader.read_bit_double_with_default(sy);
        let (sz, ez) = if !z_are_zero {
            let sz = reader.read_raw_double();
            let ez = reader.read_bit_double_with_default(sz);
            (sz, ez)
        } else {
            (0.0, 0.0)
        };
        start = Vector3::new(sx, sy, sz);
        end = Vector3::new(ex, ey, ez);
    }
    let thickness = reader.read_bit_thickness();
    let normal = reader.read_bit_extrusion();
    LineData {
        start,
        end,
        thickness,
        normal,
    }
}

pub fn read_circle(reader: &mut DwgMergedReader) -> CircleData {
    let center = reader.read_3bit_double();
    let radius = reader.read_bit_double();
    let thickness = reader.read_bit_thickness();
    let normal = reader.read_bit_extrusion();
    CircleData {
        center,
        radius,
        thickness,
        normal,
    }
}

pub fn read_arc(reader: &mut DwgMergedReader) -> ArcData {
    let center = reader.read_3bit_double();
    let radius = reader.read_bit_double();
    let thickness = reader.read_bit_thickness();
    let normal = reader.read_bit_extrusion();
    let start_angle = reader.read_bit_double();
    let end_angle = reader.read_bit_double();
    ArcData {
        center,
        radius,
        thickness,
        normal,
        start_angle,
        end_angle,
    }
}

pub fn read_ellipse(reader: &mut DwgMergedReader) -> EllipseData {
    let center = reader.read_3bit_double();
    let major_axis = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let minor_axis_ratio = reader.read_bit_double();
    let start_parameter = reader.read_bit_double();
    let end_parameter = reader.read_bit_double();
    EllipseData {
        center,
        major_axis,
        normal,
        minor_axis_ratio,
        start_parameter,
        end_parameter,
    }
}

pub fn read_ray(reader: &mut DwgMergedReader) -> RayData {
    let base_point = reader.read_3bit_double();
    let direction = reader.read_3bit_double();
    RayData {
        base_point,
        direction,
    }
}

pub fn read_xline(reader: &mut DwgMergedReader) -> XLineData {
    let base_point = reader.read_3bit_double();
    let direction = reader.read_3bit_double();
    XLineData {
        base_point,
        direction,
    }
}

pub fn read_solid(reader: &mut DwgMergedReader) -> SolidData {
    let thickness = reader.read_bit_thickness();
    let elevation = reader.read_bit_double();
    let first_corner = reader.read_2raw_double();
    let second_corner = reader.read_2raw_double();
    let third_corner = reader.read_2raw_double();
    let fourth_corner = reader.read_2raw_double();
    let normal = reader.read_bit_extrusion();
    SolidData {
        thickness,
        elevation,
        first_corner,
        second_corner,
        third_corner,
        fourth_corner,
        normal,
    }
}

pub fn read_face3d(reader: &mut DwgMergedReader, version: DwgVersion) -> Face3DData {
    if version.r13_14_only() {
        let first_corner = reader.read_3bit_double();
        let second_corner = reader.read_3bit_double();
        let third_corner = reader.read_3bit_double();
        let fourth_corner = reader.read_3bit_double();
        let invisible_edges = reader.read_bit_short();
        Face3DData {
            first_corner,
            second_corner,
            third_corner,
            fourth_corner,
            invisible_edges,
        }
    } else {
        let has_no_flags = reader.read_bit();
        // ODA spec "Z is zero" — corner1's Z is omitted from the stream
        // (treated as 0.0) when set. Corners 2–4 always encode their Z as
        // BD-with-default (with the previous corner's Z as the default),
        // independent of this flag. Skipping those reads on the later
        // corners desynchronises the bit cursor: corner-3 Y and corner-4
        // X then collapse to defaults, and the quad reads as a degenerate
        // edge along the corner-1 Y line.
        let z_is_zero = reader.read_bit();

        let x1 = reader.read_raw_double();
        let y1 = reader.read_raw_double();
        let z1 = if !z_is_zero {
            reader.read_raw_double()
        } else {
            0.0
        };

        let x2 = reader.read_bit_double_with_default(x1);
        let y2 = reader.read_bit_double_with_default(y1);
        let z2 = reader.read_bit_double_with_default(z1);

        let x3 = reader.read_bit_double_with_default(x2);
        let y3 = reader.read_bit_double_with_default(y2);
        let z3 = reader.read_bit_double_with_default(z2);

        let x4 = reader.read_bit_double_with_default(x3);
        let y4 = reader.read_bit_double_with_default(y3);
        let z4 = reader.read_bit_double_with_default(z3);

        let invisible_edges = if !has_no_flags {
            reader.read_bit_short()
        } else {
            0
        };

        Face3DData {
            first_corner: Vector3::new(x1, y1, z1),
            second_corner: Vector3::new(x2, y2, z2),
            third_corner: Vector3::new(x3, y3, z3),
            fourth_corner: Vector3::new(x4, y4, z4),
            invisible_edges,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
//  Reader functions — Moderate entities
// ════════════════════════════════════════════════════════════════════════

pub fn read_insert(reader: &mut DwgMergedReader, version: DwgVersion) -> InsertData {
    let insert_point = reader.read_3bit_double();
    let (x_scale, y_scale, z_scale);

    if version.r13_14_only() {
        x_scale = reader.read_bit_double();
        y_scale = reader.read_bit_double();
        z_scale = reader.read_bit_double();
    } else {
        // R2000+
        let data_flags = reader.main_mut().read_2bits();
        match data_flags {
            3 => {
                x_scale = 1.0;
                y_scale = 1.0;
                z_scale = 1.0;
            }
            2 => {
                x_scale = reader.read_raw_double();
                y_scale = x_scale;
                z_scale = x_scale;
            }
            1 => {
                x_scale = 1.0;
                y_scale = reader.read_bit_double_with_default(1.0);
                z_scale = reader.read_bit_double_with_default(1.0);
            }
            _ => {
                x_scale = reader.read_raw_double();
                y_scale = reader.read_bit_double_with_default(x_scale);
                z_scale = reader.read_bit_double_with_default(x_scale);
            }
        }
    }

    let rotation = reader.read_bit_double();
    let normal = reader.read_3bit_double();
    let has_attribs = reader.read_bit();
    let block_handle = reader.read_handle();

    InsertData {
        insert_point,
        x_scale,
        y_scale,
        z_scale,
        rotation,
        normal,
        has_attribs,
        block_handle,
    }
}

pub fn read_minsert(reader: &mut DwgMergedReader, version: DwgVersion) -> MInsertData {
    let insert = read_insert(reader, version);
    let column_count = reader.read_bit_short();
    let row_count = reader.read_bit_short();
    let column_spacing = reader.read_bit_double();
    let row_spacing = reader.read_bit_double();
    MInsertData {
        insert,
        column_count,
        row_count,
        column_spacing,
        row_spacing,
    }
}

pub fn read_lwpolyline(reader: &mut DwgMergedReader, version: DwgVersion) -> LwPolylineData {
    read_lwpolyline_impl(reader, version, false)
}

pub fn read_embedded_lwpolyline(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> LwPolylineData {
    read_lwpolyline_impl(reader, version, true)
}

fn read_lwpolyline_impl(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    raw_points: bool,
) -> LwPolylineData {
    let flag = reader.read_bit_short();
    let has_constant_width = (flag & 0x4) != 0;
    let has_elevation = (flag & 0x8) != 0;
    let has_thickness = (flag & 0x2) != 0;
    let has_normal = (flag & 0x1) != 0;
    let has_bulges = (flag & 0x10) != 0;
    let has_widths = (flag & 0x20) != 0;

    let constant_width = if has_constant_width {
        reader.read_bit_double()
    } else {
        0.0
    };
    let elevation = if has_elevation {
        reader.read_bit_double()
    } else {
        0.0
    };
    // LWPOLYLINE stores its own thickness/extrusion as plain BD / 3BD — NOT the
    // self-compressing BT / BE forms used in the common entity data. Reading BT
    // (1 bit) where a BD (2-bit selector) lives, or BE (1 bit) where a 3BD lives,
    // under-reads and desyncs every field after it (garbage normal, garbage
    // point count) for any polyline that carries a thickness or extrusion flag,
    // while flag-free polylines still parse. Matches the reference readLwPolyline.
    let thickness = if has_thickness {
        reader.read_bit_double()
    } else {
        0.0
    };
    let normal = if has_normal {
        reader.read_3bit_double()
    } else {
        Vector3::UNIT_Z
    };

    let num_pts = safe_count(reader.read_bit_long());
    let num_bulges = if has_bulges {
        safe_count(reader.read_bit_long())
    } else {
        0
    };
    let has_vertex_ids = (flag & 0x400) != 0;
    let num_vertex_ids = if has_vertex_ids {
        safe_count(reader.read_bit_long())
    } else {
        0
    };
    let num_widths = if has_widths {
        safe_count(reader.read_bit_long())
    } else {
        0
    };

    // Read vertex positions
    let mut xs = Vec::with_capacity(num_pts as usize);
    let mut ys = Vec::with_capacity(num_pts as usize);

    if raw_points || version.r13_14_only() {
        for _ in 0..num_pts {
            xs.push(reader.read_raw_double());
            ys.push(reader.read_raw_double());
        }
    } else if num_pts > 0 {
        // R2000+: first vertex is 2RD, rest are 2DD
        xs.push(reader.read_raw_double());
        ys.push(reader.read_raw_double());
        for i in 1..num_pts as usize {
            let px = xs[i - 1];
            let py = ys[i - 1];
            xs.push(reader.read_bit_double_with_default(px));
            ys.push(reader.read_bit_double_with_default(py));
        }
    }

    // Read bulges
    let mut bulges = vec![0.0f64; num_pts as usize];
    if has_bulges {
        for i in 0..num_bulges as usize {
            if i < bulges.len() {
                bulges[i] = reader.read_bit_double();
            }
        }
    }

    // Read vertex IDs (R2010+, flag 0x400)
    let mut vertex_ids = vec![0; num_pts as usize];
    if has_vertex_ids {
        for i in 0..num_vertex_ids as usize {
            let vertex_id = reader.read_bit_long();
            if i < vertex_ids.len() {
                vertex_ids[i] = vertex_id;
            }
        }
    }

    // Read widths
    let mut start_widths = vec![0.0f64; num_pts as usize];
    let mut end_widths = vec![0.0f64; num_pts as usize];
    if has_widths {
        for i in 0..num_widths as usize {
            if i < start_widths.len() {
                start_widths[i] = reader.read_bit_double();
                end_widths[i] = reader.read_bit_double();
            }
        }
    }

    let mut vertices = Vec::with_capacity(num_pts as usize);
    for i in 0..num_pts as usize {
        vertices.push(LwPolylineVertex {
            x: xs[i],
            y: ys[i],
            bulge: bulges[i],
            start_width: start_widths[i],
            end_width: end_widths[i],
            vertex_id: vertex_ids[i],
        });
    }

    LwPolylineData {
        flag,
        constant_width,
        elevation,
        thickness,
        normal,
        vertices,
    }
}

pub fn read_spline(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> SplineData {
    let mut flags1 = 0i32;
    let mut knot_param = 0i32;

    let mut scenario = reader.read_bit_long();
    if version.r2013_plus(dxf_version) {
        flags1 = reader.read_bit_long();
        knot_param = reader.read_bit_long();
        // R2013+ derives the storage method from knot parametrization and the
        // UseKnotParameter flag. Bit 1 is only CV-frame visibility.
        scenario = if knot_param == 15 || flags1 & 8 == 0 {
            1
        } else {
            2
        };
    }

    let degree = reader.read_bit_long();

    let mut rational = false;
    // Fit-point splines have no closed bit in their scenario body. R2013+
    // stores it in splineflags1 instead.
    let mut closed = flags1 & 4 != 0;
    let mut periodic = false;
    let mut knot_tolerance = 0.0;
    let mut control_tolerance = 0.0;
    let mut knots = Vec::new();
    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    let mut fit_tolerance = 0.0;
    let mut begin_tangent = Vector3::ZERO;
    let mut end_tangent = Vector3::ZERO;
    let mut fit_points = Vec::new();

    match scenario {
        1 => {
            rational = reader.read_bit();
            closed = reader.read_bit();
            periodic = reader.read_bit();
            knot_tolerance = reader.read_bit_double();
            control_tolerance = reader.read_bit_double();
            let num_knots = safe_count(reader.read_bit_long());
            let num_ctrl = safe_count(reader.read_bit_long());
            let has_weights = reader.read_bit();

            for _ in 0..num_knots {
                knots.push(reader.read_bit_double());
            }
            for _ in 0..num_ctrl {
                let pt = reader.read_3bit_double();
                control_points.push(pt);
                if has_weights {
                    weights.push(reader.read_bit_double());
                }
            }
        }
        _ => {
            fit_tolerance = reader.read_bit_double();
            begin_tangent = reader.read_3bit_double();
            end_tangent = reader.read_3bit_double();
            let num_fit = safe_count(reader.read_bit_long());
            for _ in 0..num_fit {
                fit_points.push(reader.read_3bit_double());
            }
        }
    }

    SplineData {
        scenario,
        degree,
        rational,
        closed,
        periodic,
        knot_tolerance,
        control_tolerance,
        knots,
        control_points,
        weights,
        fit_tolerance,
        begin_tangent,
        end_tangent,
        fit_points,
        knot_param,
        flags1,
    }
}

/// Shared text entity data reader (used by Text, AttDef, AttEntity).
pub fn read_text_entity_data(reader: &mut DwgMergedReader, version: DwgVersion) -> TextEntityData {
    if version.r13_14_only() {
        let elevation = reader.read_bit_double();
        let ix = reader.read_raw_double();
        let iy = reader.read_raw_double();
        let ax = reader.read_raw_double();
        let ay = reader.read_raw_double();
        let normal = reader.read_3bit_double();
        let thickness = reader.read_bit_double();
        let oblique_angle = reader.read_bit_double();
        let rotation = reader.read_bit_double();
        let height = reader.read_bit_double();
        let width_factor = reader.read_bit_double();
        let value = reader.read_variable_text();
        let generation = reader.read_bit_short();
        let horizontal_alignment = reader.read_bit_short();
        let vertical_alignment = reader.read_bit_short();

        TextEntityData {
            insertion_point: Vector3::new(ix, iy, elevation),
            alignment_point: Vector3::new(ax, ay, elevation),
            normal,
            thickness,
            oblique_angle,
            rotation,
            height,
            width_factor,
            value,
            generation,
            horizontal_alignment,
            vertical_alignment,
            style_handle: 0,
        }
    } else {
        let data_flags = reader.read_byte();
        let elevation = if (data_flags & 0x01) == 0 {
            reader.read_raw_double()
        } else {
            0.0
        };
        let ix = reader.read_raw_double();
        let iy = reader.read_raw_double();
        let (ax, ay) = if (data_flags & 0x02) == 0 {
            (
                reader.read_bit_double_with_default(ix),
                reader.read_bit_double_with_default(iy),
            )
        } else {
            (0.0, 0.0)
        };
        let normal = reader.read_bit_extrusion();
        let thickness = reader.read_bit_thickness();
        let oblique_angle = if (data_flags & 0x04) == 0 {
            reader.read_raw_double()
        } else {
            0.0
        };
        let rotation = if (data_flags & 0x08) == 0 {
            reader.read_raw_double()
        } else {
            0.0
        };
        let height = reader.read_raw_double();
        let width_factor = if (data_flags & 0x10) == 0 {
            reader.read_raw_double()
        } else {
            1.0
        };
        let value = reader.read_variable_text();
        let generation = if (data_flags & 0x20) == 0 {
            reader.read_bit_short()
        } else {
            0
        };
        let horizontal_alignment = if (data_flags & 0x40) == 0 {
            reader.read_bit_short()
        } else {
            0
        };
        let vertical_alignment = if (data_flags & 0x80) == 0 {
            reader.read_bit_short()
        } else {
            0
        };

        TextEntityData {
            insertion_point: Vector3::new(ix, iy, elevation),
            alignment_point: Vector3::new(ax, ay, elevation),
            normal,
            thickness,
            oblique_angle,
            rotation,
            height,
            width_factor,
            value,
            generation,
            horizontal_alignment,
            vertical_alignment,
            style_handle: 0,
        }
    }
}

/// Read TEXT entity (wraps read_text_entity_data + style handle).
pub fn read_text(reader: &mut DwgMergedReader, version: DwgVersion) -> TextEntityData {
    let mut data = read_text_entity_data(reader, version);
    data.style_handle = reader.read_handle();
    data
}

pub fn read_mtext(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> MTextData {
    let insertion_point = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let x_direction = reader.read_3bit_double();
    let rectangle_width = reader.read_bit_double();
    let rectangle_height = if version.r2007_plus() {
        reader.read_bit_double()
    } else {
        0.0
    };
    let height = reader.read_bit_double();
    let attachment_point = reader.read_bit_short();
    let drawing_direction = reader.read_bit_short();
    let extents_height = reader.read_bit_double();
    let extents_width = reader.read_bit_double();
    let value = reader.read_variable_text();

    let style_handle = reader.read_handle();

    let linespacing_style = reader.read_bit_short();
    let linespacing_factor = reader.read_bit_double();
    let unknown_bit = reader.read_bit();

    let mut background_flags = 0i32;
    let mut background_scale = 1.5;
    let mut background_color = Color::ByLayer;
    let mut background_transparency = 0i32;
    if version.r2004_plus() {
        // Background flags BL 90: 0 = none, 1 = fill, 2 = drawing window color,
        // 0x10 = text frame (R2018+).
        background_flags = reader.read_bit_long();

        // The background-fill block follows when the UseBackgroundFillColor bit
        // (0x01) is set, or — for R2018+ — when the TextFrame bit (0x10) is set.
        if (background_flags & 0x01) != 0
            || (version.r2018_plus(dxf_version) && (background_flags & 0x10) != 0)
        {
            // Background scale factor BD 45 (default 1.5)
            background_scale = reader.read_bit_double();
            // Background color CMC 63
            background_color = reader.read_cm_color();
            // Background transparency BL 441
            background_transparency = reader.read_bit_long();
        }
    }

    // R2018+: "is NOT annotative" bit, then (when not annotative) a block of
    // redundant fields followed by optional column data. The inline annotative
    // bit exists only from R2018 on; for older files MTEXT annotativeness is
    // carried by its text style / annotation context, not the entity, so the
    // default here must be `false` — defaulting to `true` would mark *every*
    // pre-R2018 MTEXT annotative and (mis)scale it in annotation-scaled
    // viewports.
    let mut is_annotative = false;
    let mut column_type = 0i16;
    let mut column_count = 0i32;
    let mut column_flow_reversed = false;
    let mut column_auto_height = false;
    let mut column_width = 0.0;
    let mut column_gutter = 0.0;
    let mut column_heights = Vec::new();
    if version.r2018_plus(dxf_version) {
        // Is NOT annotative B
        let is_not_annotative = reader.read_bit();
        is_annotative = !is_not_annotative;

        if is_not_annotative {
            // Version BS (default 0)
            let _version_bs = reader.read_bit_short();
            // Default flag B (default true)
            let _default_flag = reader.read_bit();
            // Registered application H (hard pointer)
            let _app_handle = reader.read_handle();

            // ── BEGIN redundant fields (already captured above; discarded) ──
            // Attachment point BL
            let _attachment = reader.read_bit_long();
            // X-axis dir 3BD
            let _x_axis = reader.read_3bit_double();
            // Insertion point 3BD
            let _insertion = reader.read_3bit_double();
            // Rect width BD
            let _rect_width = reader.read_bit_double();
            // Rect height BD
            let _rect_height = reader.read_bit_double();
            // Extents width BD
            let _extents_width = reader.read_bit_double();
            // Extents height BD
            let _extents_height = reader.read_bit_double();
            // ── END redundant fields ──

            // Column type BS 71: 0 = none, 1 = static, 2 = dynamic
            column_type = reader.read_bit_short();
            if column_type != 0 {
                // Column height count BL 72
                column_count = safe_count(reader.read_bit_long());
                // Column width BD 44
                column_width = reader.read_bit_double();
                // Gutter BD 45
                column_gutter = reader.read_bit_double();
                // Auto height? B 73
                column_auto_height = reader.read_bit();
                // Flow reversed? B 74
                column_flow_reversed = reader.read_bit();

                // Per-column heights only for dynamic, non-auto-height columns.
                if !column_auto_height && column_type == 2 && column_count > 0 {
                    column_heights.reserve(column_count as usize);
                    for _ in 0..column_count {
                        // Column height BD 46
                        column_heights.push(reader.read_bit_double());
                    }
                }
            }
        }
    }

    MTextData {
        insertion_point,
        normal,
        x_direction,
        rectangle_width,
        rectangle_height,
        height,
        attachment_point,
        drawing_direction,
        extents_height,
        extents_width,
        value,
        style_handle,
        linespacing_style,
        linespacing_factor,
        unknown_bit,
        background_flags,
        background_scale,
        background_color,
        background_transparency,
        is_annotative,
        column_type,
        column_count,
        column_flow_reversed,
        column_auto_height,
        column_width,
        column_gutter,
        column_heights,
    }
}

pub fn read_shape(reader: &mut DwgMergedReader) -> ShapeData {
    let insertion_point = reader.read_3bit_double();
    let size = reader.read_bit_double();
    let rotation = reader.read_bit_double();
    let relative_x_scale = reader.read_bit_double();
    let oblique_angle = reader.read_bit_double();
    let thickness = reader.read_bit_double();
    let shape_number = reader.read_bit_short();
    let normal = reader.read_3bit_double();
    let style_handle = reader.read_handle();
    ShapeData {
        insertion_point,
        size,
        rotation,
        relative_x_scale,
        oblique_angle,
        thickness,
        shape_number,
        normal,
        style_handle,
    }
}

pub fn read_leader(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> LeaderData {
    let unknown_bit = reader.read_bit();
    let annotation_type = reader.read_bit_short();
    let path_type = reader.read_bit_short();

    let num_pts = safe_count(reader.read_bit_long());
    let mut vertices = Vec::with_capacity(num_pts as usize);
    for _ in 0..num_pts {
        vertices.push(reader.read_3bit_double());
    }

    let origin = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let horizontal_direction = reader.read_3bit_double();
    let block_offset = reader.read_3bit_double();
    let annotation_offset = if dxf_version >= DxfVersion::AC1014 {
        reader.read_3bit_double()
    } else {
        Vector3::ZERO
    };

    let dimgap = if version.r13_14_only() {
        reader.read_bit_double()
    } else {
        0.0
    };

    let (text_height, text_width) = if dxf_version <= DxfVersion::AC1021 {
        (reader.read_bit_double(), reader.read_bit_double())
    } else {
        (0.0, 0.0)
    };

    let hookline_on_x_dir = reader.read_bit();
    let arrowhead_on = reader.read_bit();

    let common_arrowhead_or_unknown = reader.read_bit_short();
    let mut arrowhead_type = 0;
    let mut dimasz = 0.0;
    let mut unknown_bit2 = false;
    let mut unknown_bit3 = false;
    let unknown_short1;
    let mut byblock_color = 0;
    let unknown_bit4;
    let unknown_bit5;
    if version.r13_14_only() {
        arrowhead_type = common_arrowhead_or_unknown;
        dimasz = reader.read_bit_double();
        unknown_bit2 = reader.read_bit();
        unknown_bit3 = reader.read_bit();
        unknown_short1 = reader.read_bit_short();
        byblock_color = reader.read_bit_short();
        unknown_bit4 = reader.read_bit();
        unknown_bit5 = reader.read_bit();
    } else {
        unknown_short1 = common_arrowhead_or_unknown;
        unknown_bit4 = reader.read_bit();
        unknown_bit5 = reader.read_bit();
    }

    let annotation_handle = reader.read_handle();
    let dimstyle_handle = reader.read_handle();

    LeaderData {
        unknown_bit,
        annotation_type,
        path_type,
        vertices,
        origin,
        normal,
        horizontal_direction,
        block_offset,
        annotation_offset,
        text_height,
        text_width,
        hookline_on_x_dir,
        arrowhead_on,
        annotation_handle,
        dimstyle_handle,
        dimgap,
        arrowhead_type,
        dimasz,
        unknown_bit2,
        unknown_bit3,
        unknown_short1,
        byblock_color,
        unknown_bit4,
        unknown_bit5,
    }
}

pub fn read_tolerance(reader: &mut DwgMergedReader, version: DwgVersion) -> ToleranceData {
    let (unknown_short, text_height, dimgap) = if version.r13_14_only() {
        (
            reader.read_bit_short(),
            reader.read_bit_double(),
            reader.read_bit_double(),
        )
    } else {
        (0, 0.18, 0.09)
    };

    let insertion_point = reader.read_3bit_double();
    let direction = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let text = reader.read_variable_text();
    let dimstyle_handle = reader.read_handle();

    ToleranceData {
        insertion_point,
        direction,
        normal,
        text,
        dimstyle_handle,
        unknown_short,
        text_height,
        dimgap,
    }
}

// ════════════════════════════════════════════════════════════════════════
//  Result structs — Complex entities
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionCommonData {
    pub version_byte: u8,
    pub normal: Vector3,
    pub text_middle_point: Vector3,
    pub flags_byte: u8,
    pub text: String,
    pub text_rotation: f64,
    pub horizontal_direction: f64,
    pub ins_scale: Vector3,
    pub ins_rotation: f64,
    pub attachment_point: i16,
    pub linespacing_style: i16,
    pub linespacing_factor: f64,
    pub actual_measurement: f64,
    pub unknown_bit: bool,
    pub flip_arrow1: bool,
    pub flip_arrow2: bool,
    pub insertion_point: Vector2,
    pub dimstyle_handle: u64,
    pub block_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLinearData {
    pub common: DimensionCommonData,
    pub first_point: Vector3,
    pub second_point: Vector3,
    pub definition_point: Vector3,
    pub ext_line_rotation: f64,
    pub rotation: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAlignedData {
    pub common: DimensionCommonData,
    pub first_point: Vector3,
    pub second_point: Vector3,
    pub definition_point: Vector3,
    pub ext_line_rotation: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionRadiusData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub angle_vertex: Vector3,
    pub leader_length: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionDiameterData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub angle_vertex: Vector3,
    pub leader_length: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAngular2LnData {
    pub common: DimensionCommonData,
    pub dimension_arc: Vector2,
    pub first_point: Vector3,
    pub second_point: Vector3,
    pub angle_vertex: Vector3,
    pub definition_point: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAngular3PtData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub first_point: Vector3,
    pub second_point: Vector3,
    pub angle_vertex: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionOrdinateData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub feature_location: Vector3,
    pub leader_endpoint: Vector3,
    pub is_ordinate_type_x: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionArcData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub first_extension_point: Vector3,
    pub second_extension_point: Vector3,
    pub center_point: Vector3,
    pub is_partial: bool,
    pub arc_start_parameter: f64,
    pub arc_end_parameter: f64,
    pub has_leader: bool,
    pub first_leader_point: Vector3,
    pub second_leader_point: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLargeRadialData {
    pub common: DimensionCommonData,
    pub definition_point: Vector3,
    pub chord_point: Vector3,
    pub jog_angle: f64,
    pub override_center: Vector3,
    pub jog_point: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchBoundaryEdgeLine {
    pub start: Vector2,
    pub end: Vector2,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchBoundaryEdgeArc {
    pub center: Vector2,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchBoundaryEdgeEllipse {
    pub center: Vector2,
    pub major_endpoint: Vector2,
    pub minor_ratio: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchBoundaryEdgeSpline {
    pub degree: i32,
    pub rational: bool,
    pub periodic: bool,
    pub knots: Vec<f64>,
    pub control_points: Vec<Vector3>,
    pub fit_points: Vec<Vector2>,
    pub start_tangent: Vector2,
    pub end_tangent: Vector2,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HatchEdge {
    Line(HatchBoundaryEdgeLine),
    Arc(HatchBoundaryEdgeArc),
    Ellipse(HatchBoundaryEdgeEllipse),
    Spline(HatchBoundaryEdgeSpline),
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchBoundaryPath {
    pub flags: i32,
    pub edges: Vec<HatchEdge>,
    pub polyline_vertices: Vec<(Vector2, f64)>,
    pub polyline_closed: bool,
    pub boundary_handle_count: i32,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchPatternLine {
    pub angle: f64,
    pub base_point: Vector2,
    pub offset: Vector2,
    pub dashes: Vec<f64>,
}
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HatchData {
    pub is_mpolygon: bool,
    pub mpolygon_initial_style: i16,
    pub gradient_enabled: bool,
    pub gradient_reserved: i32,
    pub gradient_angle: f64,
    pub gradient_shift: f64,
    pub gradient_single_color: bool,
    pub gradient_tint: f64,
    pub gradient_colors: Vec<(f64, crate::types::Color)>,
    pub gradient_name: String,
    pub elevation: f64,
    pub normal: Vector3,
    pub pattern_name: String,
    pub is_solid: bool,
    pub is_associative: bool,
    pub paths: Vec<HatchBoundaryPath>,
    pub style: i16,
    pub pattern_type: i16,
    pub pattern_angle: f64,
    pub pattern_scale: f64,
    pub is_double: bool,
    pub pattern_lines: Vec<HatchPatternLine>,
    pub pixel_size: f64,
    pub seed_points: Vec<Vector2>,
    pub mpolygon_hatch_color: Color,
    pub mpolygon_x_direction: Vector2,
    pub mpolygon_boundary_handle_count: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewportData {
    pub center: Vector3,
    pub width: f64,
    pub height: f64,
    pub view_target: Vector3,
    pub view_direction: Vector3,
    pub twist_angle: f64,
    pub view_height: f64,
    pub lens_length: f64,
    pub front_clip_z: f64,
    pub back_clip_z: f64,
    pub snap_angle: f64,
    pub view_center: Vector2,
    pub snap_base: Vector2,
    pub snap_spacing: Vector2,
    pub grid_spacing: Vector2,
    pub circle_sides: i16,
    pub grid_major: i16,
    pub frozen_layer_count: i32,
    pub status_flags: i32,
    pub style_sheet: String,
    pub render_mode: u8,
    pub ucs_at_origin: bool,
    pub ucs_per_viewport: bool,
    pub ucs_origin: Vector3,
    pub ucs_x_axis: Vector3,
    pub ucs_y_axis: Vector3,
    pub ucs_elevation: f64,
    pub ucs_ortho_type: i16,
    pub shade_plot_mode: i16,
    pub default_lighting: bool,
    pub default_lighting_type: u8,
    pub brightness: f64,
    pub contrast: f64,
    pub ambient_color: crate::types::Color,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline2DData {
    pub flags: i16,
    pub smooth_surface: i16,
    pub start_width: f64,
    pub end_width: f64,
    pub thickness: f64,
    pub elevation: f64,
    pub normal: Vector3,
    pub owned_count: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex2DData {
    pub handle: crate::types::Handle,
    pub flags: u8,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub start_width: f64,
    pub end_width: f64,
    pub bulge: f64,
    pub vertex_id: i32,
    pub tangent_dir: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline3DData {
    pub smooth_type: u8,
    pub closed_flag: u8,
    pub owned_count: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex3DData {
    pub handle: crate::types::Handle,
    pub flags: u8,
    pub position: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineVertexData {
    pub position: Vector3,
    pub direction: Vector3,
    pub miter: Vector3,
    pub segments: Vec<MLineSegmentData>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineSegmentData {
    pub parameters: Vec<f64>,
    pub area_fill_parameters: Vec<f64>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineData {
    pub scale_factor: f64,
    pub justification: u8,
    pub start_point: Vector3,
    pub normal: Vector3,
    pub openclosed: i16,
    pub lines_in_style: u8,
    pub vertex_count: i16,
    pub vertices: Vec<MLineVertexData>,
    pub style_handle: u64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeshData {
    pub version: i16,
    pub blend_crease: bool,
    pub subdivision_level: i32,
    pub vertices: Vec<Vector3>,
    pub faces: Vec<Vec<i32>>,
    pub edges: Vec<(i32, i32)>,
    pub crease_values: Vec<f64>,
    pub override_option: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RasterImageData {
    pub class_version: i32,
    pub insertion_point: Vector3,
    pub u_vector: Vector3,
    pub v_vector: Vector3,
    pub size: Vector2,
    pub flags: i16,
    pub clipping_enabled: bool,
    pub brightness: u8,
    pub contrast: u8,
    pub fade: u8,
    pub clip_inverted: bool,
    pub clip_type: i16,
    pub definition_handle: u64,
    pub reactor_handle: u64,
    /// Clip boundary vertices in image pixel space (range 0..size for rect;
    /// arbitrary polygon for polygonal). For rectangular clips two corners
    /// are stored; for polygonal, three or more sequential vertices.
    pub clip_boundary_vertices: Vec<Vector2>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ole2FrameData {
    pub object_type: i16,
    pub mode: i16,
    pub storage: crate::compound_file::StructuredStoragePayload,
    pub envelope: crate::entities::OleFrameEnvelope,
    pub lock_aspect: u8,
    /// Frame corners decoded from the OLE data blob (see `ole2frame_corners`).
    pub upper_left: Vector3,
    pub lower_right: Vector3,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributeCommonData {
    pub text_data: TextEntityData,
    pub att_version: u8,
    pub att_type: u8,
    pub embedded_mtext: Option<MTextData>,
    pub tag: String,
    /// ATTDEF prompt string. Empty for ATTRIB entities (the stream carries no
    /// prompt for an attribute instance — it lives on the definition).
    pub prompt: String,
    pub field_length: i16,
    pub flags: u8,
    pub lock_position: bool,
}

// ════════════════════════════════════════════════════════════════════════
//  Reader functions — Complex entities
// ════════════════════════════════════════════════════════════════════════

/// Read common dimension data shared by all dimension types.
pub fn read_common_dimension_data(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    _dxf_version: DxfVersion,
) -> DimensionCommonData {
    let version_byte = if version.r2010_plus() {
        reader.read_byte()
    } else {
        0
    };
    let normal = reader.read_3bit_double();
    let text_mid = reader.read_2raw_double();
    let text_mid_z = reader.read_bit_double();
    let flags_byte = reader.read_byte();
    let text = reader.read_variable_text();
    let text_rotation = reader.read_bit_double();
    let horizontal_direction = reader.read_bit_double();
    let ins_scale = reader.read_3bit_double();
    let ins_rotation = reader.read_bit_double();

    let mut attachment_point = 0i16;
    let mut linespacing_style = 1i16;
    let mut linespacing_factor = 1.0;
    let mut actual_measurement = 0.0;
    if version.r2000_plus() {
        attachment_point = reader.read_bit_short();
        linespacing_style = reader.read_bit_short();
        linespacing_factor = reader.read_bit_double();
        actual_measurement = reader.read_bit_double();
    }

    let mut unknown_bit = false;
    let mut flip_arrow1 = false;
    let mut flip_arrow2 = false;
    if version.r2007_plus() {
        unknown_bit = reader.read_bit();
        flip_arrow1 = reader.read_bit();
        flip_arrow2 = reader.read_bit();
    }

    let insertion_point = reader.read_2raw_double();
    let dimstyle_handle = reader.read_handle();
    let block_handle = reader.read_handle();

    DimensionCommonData {
        version_byte,
        normal,
        text_middle_point: Vector3::new(text_mid.x, text_mid.y, text_mid_z),
        flags_byte,
        text,
        text_rotation,
        horizontal_direction,
        ins_scale,
        ins_rotation,
        attachment_point,
        linespacing_style,
        linespacing_factor,
        actual_measurement,
        unknown_bit,
        flip_arrow1,
        flip_arrow2,
        insertion_point,
        dimstyle_handle,
        block_handle,
    }
}

pub fn read_dimension_linear(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionLinearData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    let first_point = reader.read_3bit_double();
    let second_point = reader.read_3bit_double();
    let definition_point = reader.read_3bit_double();
    let ext_line_rotation = reader.read_bit_double();
    let rotation = reader.read_bit_double();
    DimensionLinearData {
        common,
        first_point,
        second_point,
        definition_point,
        ext_line_rotation,
        rotation,
    }
}

pub fn read_dimension_aligned(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionAlignedData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    let first_point = reader.read_3bit_double();
    let second_point = reader.read_3bit_double();
    let definition_point = reader.read_3bit_double();
    let ext_line_rotation = reader.read_bit_double();
    DimensionAlignedData {
        common,
        first_point,
        second_point,
        definition_point,
        ext_line_rotation,
    }
}

pub fn read_dimension_radius(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionRadiusData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    // Radius stores the centre before the chord point.
    let angle_vertex = reader.read_3bit_double();
    let definition_point = reader.read_3bit_double();
    let leader_length = reader.read_bit_double();
    DimensionRadiusData {
        common,
        definition_point,
        angle_vertex,
        leader_length,
    }
}

pub fn read_dimension_diameter(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionDiameterData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    // Diameter stores the first chord before its opposite.
    let angle_vertex = reader.read_3bit_double();
    let definition_point = reader.read_3bit_double();
    let leader_length = reader.read_bit_double();
    DimensionDiameterData {
        common,
        definition_point,
        angle_vertex,
        leader_length,
    }
}

pub fn read_dimension_angular_2ln(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionAngular2LnData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    let dimension_arc = reader.read_2raw_double();
    let first_point = reader.read_3bit_double();
    let second_point = reader.read_3bit_double();
    let angle_vertex = reader.read_3bit_double();
    let definition_point = reader.read_3bit_double();
    DimensionAngular2LnData {
        common,
        dimension_arc,
        first_point,
        second_point,
        angle_vertex,
        definition_point,
    }
}

pub fn read_dimension_angular_3pt(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionAngular3PtData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    let definition_point = reader.read_3bit_double();
    let first_point = reader.read_3bit_double();
    let second_point = reader.read_3bit_double();
    let angle_vertex = reader.read_3bit_double();
    DimensionAngular3PtData {
        common,
        definition_point,
        first_point,
        second_point,
        angle_vertex,
    }
}

pub fn read_dimension_ordinate(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionOrdinateData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    let definition_point = reader.read_3bit_double();
    let feature_location = reader.read_3bit_double();
    let leader_endpoint = reader.read_3bit_double();
    let is_ordinate_type_x = reader.read_byte() == 1;
    DimensionOrdinateData {
        common,
        definition_point,
        feature_location,
        leader_endpoint,
        is_ordinate_type_x,
    }
}

pub fn read_dimension_arc(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionArcData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    DimensionArcData {
        common,
        definition_point: reader.read_3bit_double(),
        first_extension_point: reader.read_3bit_double(),
        second_extension_point: reader.read_3bit_double(),
        center_point: reader.read_3bit_double(),
        is_partial: reader.read_bit(),
        arc_start_parameter: reader.read_bit_double(),
        arc_end_parameter: reader.read_bit_double(),
        has_leader: reader.read_bit(),
        first_leader_point: reader.read_3bit_double(),
        second_leader_point: reader.read_3bit_double(),
    }
}

pub fn read_dimension_large_radial(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> DimensionLargeRadialData {
    let common = read_common_dimension_data(reader, version, dxf_version);
    DimensionLargeRadialData {
        common,
        definition_point: reader.read_3bit_double(),
        chord_point: reader.read_3bit_double(),
        jog_angle: reader.read_bit_double(),
        override_center: reader.read_3bit_double(),
        jog_point: reader.read_3bit_double(),
    }
}

/// Read a hatch boundary path (both polyline and non-polyline variants).
pub fn read_hatch_boundary_path(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> HatchBoundaryPath {
    let flags = reader.read_bit_long();
    read_hatch_boundary_path_contents(reader, version, flags, true)
}

/// Read the geometry following an already-decoded hatch path flag value.
pub fn read_hatch_boundary_path_contents(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    flags: i32,
    has_boundary_handles: bool,
) -> HatchBoundaryPath {
    let is_polyline = (flags & 2) != 0;

    let mut edges = Vec::new();
    let mut polyline_vertices = Vec::new();
    let mut polyline_closed = false;

    if !is_polyline {
        let num_edges = safe_count(reader.read_bit_long());
        for _ in 0..num_edges {
            let edge_type = reader.read_byte();
            match edge_type {
                1 => {
                    let start = reader.read_2raw_double();
                    let end = reader.read_2raw_double();
                    edges.push(HatchEdge::Line(HatchBoundaryEdgeLine { start, end }));
                }
                2 => {
                    let center = reader.read_2raw_double();
                    let radius = reader.read_bit_double();
                    let start_angle = reader.read_bit_double();
                    let end_angle = reader.read_bit_double();
                    let ccw = reader.read_bit();
                    edges.push(HatchEdge::Arc(HatchBoundaryEdgeArc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                        ccw,
                    }));
                }
                3 => {
                    let center = reader.read_2raw_double();
                    let major_endpoint = reader.read_2raw_double();
                    let minor_ratio = reader.read_bit_double();
                    let start_angle = reader.read_bit_double();
                    let end_angle = reader.read_bit_double();
                    let ccw = reader.read_bit();
                    edges.push(HatchEdge::Ellipse(HatchBoundaryEdgeEllipse {
                        center,
                        major_endpoint,
                        minor_ratio,
                        start_angle,
                        end_angle,
                        ccw,
                    }));
                }
                4 => {
                    let degree = reader.read_bit_long();
                    let rational = reader.read_bit();
                    let periodic = reader.read_bit();
                    let num_knots = safe_count(reader.read_bit_long());
                    let num_ctrl = safe_count(reader.read_bit_long());
                    let mut knots = Vec::new();
                    for _ in 0..num_knots {
                        knots.push(reader.read_bit_double());
                    }
                    let mut control_points = Vec::new();
                    for _ in 0..num_ctrl {
                        let pt = reader.read_2raw_double();
                        let w = if rational {
                            reader.read_bit_double()
                        } else {
                            1.0
                        };
                        control_points.push(Vector3::new(pt.x, pt.y, w));
                    }
                    let mut fit_points = Vec::new();
                    let mut start_tangent = Vector2::ZERO;
                    let mut end_tangent = Vector2::ZERO;
                    if version.r2010_plus() {
                        let num_fit = safe_count(reader.read_bit_long());
                        if num_fit > 0 {
                            for _ in 0..num_fit {
                                fit_points.push(reader.read_2raw_double());
                            }
                            start_tangent = reader.read_2raw_double();
                            end_tangent = reader.read_2raw_double();
                        }
                    }
                    edges.push(HatchEdge::Spline(HatchBoundaryEdgeSpline {
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
    } else {
        let has_bulge = reader.read_bit();
        polyline_closed = reader.read_bit();
        let num_verts = safe_count(reader.read_bit_long());
        for _ in 0..num_verts {
            let pt = reader.read_2raw_double();
            let bulge = if has_bulge {
                reader.read_bit_double()
            } else {
                0.0
            };
            polyline_vertices.push((pt, bulge));
        }
    }

    // Cap the boundary-handle count to a sane upper bound. Corrupt /
    // misaligned hatch records have been seen to emit ~1.9 × 10^9 here,
    // which spins read_handle() for tens of seconds per record. AutoCAD
    // hatches realistically carry well under MAX_ARRAY_COUNT (100k)
    // associative boundary references.
    let boundary_handle_count = if has_boundary_handles {
        safe_count(reader.read_bit_long())
    } else {
        0
    };

    HatchBoundaryPath {
        flags,
        edges,
        polyline_vertices,
        polyline_closed,
        boundary_handle_count,
    }
}

pub fn read_hatch(reader: &mut DwgMergedReader, version: DwgVersion) -> HatchData {
    read_hatch_kind(reader, version, false)
}

pub fn read_mpolygon(reader: &mut DwgMergedReader, version: DwgVersion) -> HatchData {
    read_hatch_kind(reader, version, true)
}

fn read_hatch_kind(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    is_mpolygon: bool,
) -> HatchData {
    let mpolygon_initial_style = if is_mpolygon {
        reader.read_bit_short()
    } else {
        0
    };
    let mut gradient_enabled = false;
    let mut gradient_reserved = 0i32;
    let mut gradient_angle = 0.0;
    let mut gradient_shift = 0.0;
    let mut gradient_single_color = false;
    let mut gradient_tint = 0.0;
    let mut gradient_colors = Vec::new();
    let mut gradient_name = String::new();
    if version.r2004_plus() {
        let is_gradient = reader.read_bit_long();
        gradient_enabled = is_gradient != 0;
        gradient_reserved = reader.read_bit_long();
        gradient_angle = reader.read_bit_double();
        gradient_shift = reader.read_bit_double();
        gradient_single_color = reader.read_bit_long() != 0;
        gradient_tint = reader.read_bit_double();
        let num_colors = safe_count(reader.read_bit_long());
        for _ in 0..num_colors {
            let value = reader.read_bit_double();
            let color = reader.read_cm_color();
            gradient_colors.push((value, color));
        }
        gradient_name = reader.read_variable_text();
    }

    let elevation = reader.read_bit_double();
    let normal = reader.read_3bit_double();
    let pattern_name = reader.read_variable_text();
    let is_solid = reader.read_bit();
    let is_associative = reader.read_bit();

    let num_paths = safe_count(reader.read_bit_long());
    let mut paths = Vec::new();
    let mut has_derived = false;
    for _ in 0..num_paths {
        let p = read_hatch_boundary_path(reader, version);
        if (p.flags & 4) != 0 {
            has_derived = true;
        }
        paths.push(p);
    }

    let style = reader.read_bit_short();
    let pattern_type = reader.read_bit_short();

    let mut pattern_angle = 0.0;
    let mut pattern_scale = 1.0;
    let mut is_double = false;
    let mut pattern_lines = Vec::new();
    if !is_solid {
        pattern_angle = reader.read_bit_double();
        pattern_scale = reader.read_bit_double();
        is_double = reader.read_bit();
        let num_lines = reader.read_bit_short();
        for _ in 0..num_lines {
            let angle = reader.read_bit_double();
            let base_pt = reader.read_2bit_double();
            let offset = reader.read_2bit_double();
            let num_dashes = reader.read_bit_short();
            let mut dashes = Vec::new();
            for _ in 0..num_dashes {
                dashes.push(reader.read_bit_double());
            }
            pattern_lines.push(HatchPatternLine {
                angle,
                base_point: base_pt,
                offset,
                dashes,
            });
        }
    }

    let (
        pixel_size,
        seed_points,
        mpolygon_hatch_color,
        mpolygon_x_direction,
        mpolygon_boundary_handle_count,
    ) = if is_mpolygon {
        (
            0.0,
            Vec::new(),
            reader.read_cm_color(),
            reader.read_2raw_double(),
            reader.read_bit_long(),
        )
    } else {
        let pixel_size = if has_derived {
            reader.read_bit_double()
        } else {
            0.0
        };
        let num_seeds = safe_count(reader.read_bit_long());
        let mut seed_points = Vec::new();
        for _ in 0..num_seeds {
            seed_points.push(reader.read_2raw_double());
        }
        (
            pixel_size,
            seed_points,
            Color::ByLayer,
            Vector2::new(1.0, 0.0),
            0,
        )
    };

    // boundary handles are read externally (for each path, path.boundary_handle_count handles)

    HatchData {
        is_mpolygon,
        mpolygon_initial_style,
        gradient_enabled,
        gradient_reserved,
        gradient_angle,
        gradient_shift,
        gradient_single_color,
        gradient_tint,
        gradient_colors,
        gradient_name,
        elevation,
        normal,
        pattern_name,
        is_solid,
        is_associative,
        paths,
        style,
        pattern_type,
        pattern_angle,
        pattern_scale,
        is_double,
        pattern_lines,
        pixel_size,
        seed_points,
        mpolygon_hatch_color,
        mpolygon_x_direction,
        mpolygon_boundary_handle_count,
    }
}

pub fn read_viewport(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    _dxf_version: DxfVersion,
) -> ViewportData {
    let center = reader.read_3bit_double();
    let width = reader.read_bit_double();
    let height = reader.read_bit_double();

    // View data (read for all versions)
    let view_target = reader.read_3bit_double();
    let view_direction = reader.read_3bit_double();
    let twist_angle = reader.read_bit_double();
    let view_height = reader.read_bit_double();
    let lens_length = reader.read_bit_double();
    let front_clip_z = reader.read_bit_double();
    let back_clip_z = reader.read_bit_double();
    let snap_angle = reader.read_bit_double();
    let view_center = reader.read_2raw_double();
    let snap_base = reader.read_2raw_double();
    let snap_spacing = reader.read_2raw_double();
    let grid_spacing = reader.read_2raw_double();
    let circle_sides = reader.read_bit_short();

    let grid_major = if version.r2007_plus() {
        reader.read_bit_short()
    } else {
        0
    };

    // Status/UCS data (read for all versions)
    let frozen_layer_count = reader.read_bit_long();
    let status_flags = reader.read_bit_long();
    let style_sheet = reader.read_variable_text();
    let render_mode = reader.read_byte();
    let ucs_at_origin = reader.read_bit();
    let ucs_per_viewport = reader.read_bit();
    let ucs_origin = reader.read_3bit_double();
    let ucs_x_axis = reader.read_3bit_double();
    let ucs_y_axis = reader.read_3bit_double();
    let ucs_elevation = reader.read_bit_double();
    let ucs_ortho_type = reader.read_bit_short();

    let shade_plot_mode = if version.r2004_plus() {
        reader.read_bit_short()
    } else {
        0
    };
    let (default_lighting, default_lighting_type, brightness, contrast, ambient_color) =
        if version.r2007_plus() {
            let dl = reader.read_bit();
            let dlt = reader.read_byte();
            let br = reader.read_bit_double();
            let co = reader.read_bit_double();
            let ambient = reader.read_cm_color();
            (dl, dlt, br, co, ambient)
        } else {
            (false, 0, 0.0, 0.0, crate::types::Color::from_index(0))
        };

    ViewportData {
        center,
        width,
        height,
        view_target,
        view_direction,
        twist_angle,
        view_height,
        lens_length,
        front_clip_z,
        back_clip_z,
        snap_angle,
        view_center,
        snap_base,
        snap_spacing,
        grid_spacing,
        circle_sides,
        grid_major,
        frozen_layer_count,
        status_flags,
        style_sheet,
        render_mode,
        ucs_at_origin,
        ucs_per_viewport,
        ucs_origin,
        ucs_x_axis,
        ucs_y_axis,
        ucs_elevation,
        ucs_ortho_type,
        shade_plot_mode,
        default_lighting,
        default_lighting_type,
        brightness,
        contrast,
        ambient_color,
    }
}

pub fn read_polyline2d(reader: &mut DwgMergedReader, version: DwgVersion) -> Polyline2DData {
    let flags = reader.read_bit_short();
    let smooth_surface = reader.read_bit_short();
    let start_width = reader.read_bit_double();
    let end_width = reader.read_bit_double();
    let thickness = reader.read_bit_thickness();
    let elevation = reader.read_bit_double();
    let normal = reader.read_bit_extrusion();
    let owned_count = if version.r2004_plus() {
        reader.read_bit_long()
    } else {
        0
    };
    Polyline2DData {
        flags,
        smooth_surface,
        start_width,
        end_width,
        thickness,
        elevation,
        normal,
        owned_count,
    }
}

pub fn read_vertex2d(reader: &mut DwgMergedReader, version: DwgVersion) -> Vertex2DData {
    let flags = reader.read_byte();
    let x = reader.read_bit_double();
    let y = reader.read_bit_double();
    let z = reader.read_bit_double();
    let sw = reader.read_bit_double();
    let (start_width, end_width) = if sw < 0.0 {
        (-sw, -sw) // negative = both widths equal
    } else {
        let ew = reader.read_bit_double();
        (sw, ew)
    };
    let bulge = reader.read_bit_double();
    let vertex_id = if version.r2010_plus() {
        reader.read_bit_long()
    } else {
        0
    };
    let tangent_dir = reader.read_bit_double();
    Vertex2DData {
        handle: crate::types::Handle::NULL,
        flags,
        x,
        y,
        z,
        start_width,
        end_width,
        bulge,
        vertex_id,
        tangent_dir,
    }
}

pub fn read_polyline3d(reader: &mut DwgMergedReader, version: DwgVersion) -> Polyline3DData {
    let smooth_type = reader.read_byte();
    let closed_flag = reader.read_byte();
    let owned_count = if version.r2004_plus() {
        reader.read_bit_long()
    } else {
        0
    };
    Polyline3DData {
        smooth_type,
        closed_flag,
        owned_count,
    }
}

pub fn read_vertex3d(reader: &mut DwgMergedReader) -> Vertex3DData {
    let flags = reader.read_byte();
    let position = reader.read_3bit_double();
    Vertex3DData {
        handle: crate::types::Handle::NULL,
        flags,
        position,
    }
}

pub fn read_polyface_mesh(reader: &mut DwgMergedReader, version: DwgVersion) -> (i16, i16, i32) {
    let num_verts = reader.read_bit_short();
    let num_faces = reader.read_bit_short();
    let owned_count = if version.r2004_plus() {
        reader.read_bit_long()
    } else {
        0
    };
    (num_verts, num_faces, owned_count)
}

/// Face record data from OBJ_VERTEX_PFACE_FACE (type 14).
pub struct PfaceFaceData {
    pub handle: crate::types::Handle,
    pub index1: i16,
    pub index2: i16,
    pub index3: i16,
    pub index4: i16,
}

/// Read a VERTEX_PFACE_FACE record (type code 14).
/// Format: 4 × BS (vertex indices), no flags byte.
pub fn read_pface_face(reader: &mut DwgMergedReader) -> PfaceFaceData {
    let index1 = reader.read_bit_short();
    let index2 = reader.read_bit_short();
    let index3 = reader.read_bit_short();
    let index4 = reader.read_bit_short();
    PfaceFaceData {
        handle: crate::types::Handle::NULL,
        index1,
        index2,
        index3,
        index4,
    }
}

pub fn read_polygon_mesh(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> (i16, i16, i16, i16, i16, i16, i32) {
    let flags = reader.read_bit_short();
    let smooth_type = reader.read_bit_short();
    let m_count = reader.read_bit_short();
    let n_count = reader.read_bit_short();
    let m_smooth = reader.read_bit_short();
    let n_smooth = reader.read_bit_short();
    let owned_count = if version.r2004_plus() {
        reader.read_bit_long()
    } else {
        0
    };
    (
        flags,
        smooth_type,
        m_count,
        n_count,
        m_smooth,
        n_smooth,
        owned_count,
    )
}

pub fn read_seqend(_reader: &mut DwgMergedReader) {
    // SEQEND has no entity-specific data
}

pub fn read_mline(reader: &mut DwgMergedReader) -> MLineData {
    let scale_factor = reader.read_bit_double();
    let justification = reader.read_byte();
    let start_point = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let openclosed = reader.read_bit_short();
    let lines_in_style = reader.read_byte();
    let vertex_count = reader.read_bit_short();

    // Read vertices (position + direction + miter + segments)
    let mut vertices = Vec::with_capacity(vertex_count as usize);
    for _ in 0..vertex_count {
        let pos = reader.read_3bit_double();
        let dir = reader.read_3bit_double();
        let miter = reader.read_3bit_double();
        let mut segments = Vec::with_capacity(lines_in_style as usize);
        for _ in 0..lines_in_style {
            let num_params = reader.read_bit_short();
            let mut params = Vec::with_capacity(num_params as usize);
            for _ in 0..num_params {
                params.push(reader.read_bit_double());
            }
            let num_area = reader.read_bit_short();
            let mut area_params = Vec::with_capacity(num_area as usize);
            for _ in 0..num_area {
                area_params.push(reader.read_bit_double());
            }
            segments.push(MLineSegmentData {
                parameters: params,
                area_fill_parameters: area_params,
            });
        }
        vertices.push(MLineVertexData {
            position: pos,
            direction: dir,
            miter,
            segments,
        });
    }

    let style_handle = reader.read_handle();

    MLineData {
        scale_factor,
        justification,
        start_point,
        normal,
        openclosed,
        lines_in_style,
        vertex_count,
        vertices,
        style_handle,
    }
}

pub fn read_mesh(reader: &mut DwgMergedReader) -> MeshData {
    let version = reader.read_bit_short();
    let blend_crease = reader.read_bit();
    let subdivision_level = reader.read_bit_long();

    let num_verts = safe_count(reader.read_bit_long());
    let mut vertices = Vec::with_capacity(num_verts as usize);
    for _ in 0..num_verts {
        vertices.push(reader.read_3bit_double());
    }

    let declared_face_data = reader.read_bit_long();
    let available_face_data = reader.main_remaining_bits().saturating_sub(6) / 2;
    let total_face_data = usize::try_from(declared_face_data)
        .unwrap_or(0)
        .min(usize::try_from(available_face_data).unwrap_or(0));
    let mut faces = Vec::new();
    let mut consumed = 0usize;
    let mut stored_indices = 0usize;
    while consumed < total_face_data {
        let declared_vertices = reader.read_bit_long();
        consumed = match consumed.checked_add(1) {
            Some(value) => value,
            None => break,
        };

        let remaining = total_face_data - consumed;
        let face_vertex_count = match usize::try_from(declared_vertices) {
            Ok(value) if value <= remaining => value,
            _ => {
                for _ in 0..remaining {
                    reader.read_bit_long();
                }
                break;
            }
        };

        let store_face = (3..=num_verts as usize).contains(&face_vertex_count)
            && faces.len() < MAX_MESH_FACES
            && stored_indices
                .checked_add(face_vertex_count)
                .is_some_and(|count| count <= MAX_MESH_FACE_INDICES);
        let mut valid_face = store_face;
        let mut face = Vec::with_capacity(if valid_face { face_vertex_count } else { 0 });
        for _ in 0..face_vertex_count {
            let vertex = reader.read_bit_long();
            if store_face {
                if valid_face && vertex >= 0 && vertex < num_verts {
                    face.push(vertex);
                } else {
                    valid_face = false;
                    face.clear();
                }
            }
        }
        consumed = match consumed.checked_add(face_vertex_count) {
            Some(value) => value,
            None => break,
        };
        if valid_face {
            stored_indices += face_vertex_count;
            faces.push(face);
        }
    }

    let num_edges = safe_count(reader.read_bit_long());
    let mut edges = Vec::with_capacity(num_edges as usize);
    for _ in 0..num_edges {
        let s = reader.read_bit_long();
        let e = reader.read_bit_long();
        edges.push((s, e));
    }

    let num_creases = safe_count(reader.read_bit_long());
    let mut crease_values = Vec::with_capacity(num_creases as usize);
    for _ in 0..num_creases {
        crease_values.push(reader.read_bit_double());
    }

    let override_option = reader.read_bit_long();

    MeshData {
        version,
        blend_crease,
        subdivision_level,
        vertices,
        faces,
        edges,
        crease_values,
        override_option,
    }
}

/// Decoded fields of an UNDERLAY reference (PDF / DWF / DGN).
///
/// Which of the three underlay flavours this is comes from the object's DXF
/// class name, resolved by the builder — the bitstream layout is identical for
/// all three (AcDbUnderlayReference).
pub struct UnderlayData {
    pub normal: Vector3,
    pub insertion_point: Vector3,
    pub rotation: f64,
    pub x_scale: f64,
    pub y_scale: f64,
    pub z_scale: f64,
    pub flags: u8,
    pub contrast: u8,
    pub fade: u8,
    pub definition_handle: u64,
    pub clip_boundary_vertices: Vec<Vector2>,
}

/// Read an UNDERLAY reference (AcDbUnderlayReference: PDF/DWF/DGN underlay).
///
/// The definition handle sits in the middle of the object data in the encoder's
/// ordering, but it is drawn from the separate handle stream, so reading it here
/// does not disturb the object-stream cursor used by the trailing clip vertices.
pub fn read_underlay(reader: &mut DwgMergedReader) -> UnderlayData {
    let normal = reader.read_3bit_double();
    let insertion_point = reader.read_3bit_double();
    let rotation = reader.read_bit_double();
    let x_scale = reader.read_bit_double();
    let y_scale = reader.read_bit_double();
    let z_scale = reader.read_bit_double();
    let flags = reader.read_byte();
    let contrast = reader.read_byte();
    let fade = reader.read_byte();
    let definition_handle = reader.read_handle();

    let n = safe_count(reader.read_bit_long()) as usize;
    let mut clip_boundary_vertices: Vec<Vector2> = Vec::with_capacity(n);
    for _ in 0..n {
        clip_boundary_vertices.push(reader.read_2raw_double());
    }

    UnderlayData {
        normal,
        insertion_point,
        rotation,
        x_scale,
        y_scale,
        z_scale,
        flags,
        contrast,
        fade,
        definition_handle,
        clip_boundary_vertices,
    }
}

// ── Table (ACAD_TABLE) content — R2010+ ──────────────────────────────
//
// The table entity is INSERT-derived; after the insert base the R2010+ record
// carries the full table content inline (equivalent to the TABLECONTENT
// object). This ports the reference readTableContent + sub-parsers. acadrust's
// model does not hold every cell-style / border / geometry detail, so those
// sub-structures are read (to stay positioned) but only their meaningful data
// (column widths, row heights, cell text/value) is retained.

/// Decoded ACAD_TABLE entity (insert base + parsed content).
pub struct TableEntityData {
    pub insert: InsertData,
    pub value_flags: i32,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub horizontal_direction: Vector3,
    pub style_handle: u64,
    pub name: String,
    pub description: String,
    pub field_handles: Vec<Handle>,
    pub base_style: Option<CellStyle>,
    pub merged_ranges: Vec<crate::entities::table::CellRange>,
    pub break_options: i32,
    pub break_flow_direction: i32,
    pub break_spacing: f64,
    pub break_data: Vec<TableBreakData>,
    pub break_ranges: Vec<TableBreakRange>,
    pub unknown_byte: u8,
    pub unknown_handle: u64,
    pub unknown_long1: i32,
    pub unknown_long2: i32,
    pub unknown_short: i16,
    pub legacy_style_override: Option<LegacyTableStyleOverride>,
    pub legacy_border_colors: Option<LegacyBorderOverrides<Color>>,
    pub legacy_border_line_weights: Option<LegacyBorderOverrides<LineWeight>>,
    pub legacy_border_visibility: Option<LegacyBorderOverrides<bool>>,
}

fn cell_value_type(code: i32) -> CellValueType {
    match code {
        1 => CellValueType::Long,
        2 => CellValueType::Double,
        4 => CellValueType::String,
        8 => CellValueType::Date,
        0x10 => CellValueType::Point2D,
        0x20 => CellValueType::Point3D,
        0x40 => CellValueType::Handle,
        0x80 => CellValueType::Buffer,
        0x100 => CellValueType::ResultBuffer,
        0x200 => CellValueType::General,
        _ => CellValueType::Unknown,
    }
}

/// General/String value: BL byte count then bytes (R2007+ = UTF-16LE minus the
/// trailing NUL, else the reader's ANSI code page).
fn read_string_cad_value(reader: &mut DwgMergedReader, version: DwgVersion) -> String {
    let len = safe_count(reader.read_bit_long()) as usize;
    let bytes = reader.read_bytes(len);
    if version.r2007_plus() {
        let n = len.saturating_sub(2);
        let units: Vec<u16> = bytes[..n.min(bytes.len())]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        let stored_len = len.min(bytes.len());
        let end = stored_len.saturating_sub(usize::from(
            bytes.get(stored_len.saturating_sub(1)) == Some(&0),
        ));
        reader.decode_legacy_text(&bytes[..end])
    }
}

/// A single table cell value (AcDbCellValue). See the reference readCadValue.
pub(super) fn read_cad_value(reader: &mut DwgMergedReader, version: DwgVersion) -> CellValue {
    read_cad_value_with_schema(reader, version, version.r2007_plus())
}

fn read_cad_value_with_schema(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    modern_schema: bool,
) -> CellValue {
    // TABLECONTENT can be down-saved into AC1018 while retaining modern value
    // type codes. Stream framing (flags and unit/format strings) still follows
    // the containing file version.
    let mut v = CellValue::new();
    if version.r2007_plus() {
        v.flags = reader.read_bit_long();
    }
    let stored_type_code = reader.read_bit_long();
    v.raw_type_code = stored_type_code;
    let type_code = if modern_schema {
        stored_type_code
    } else {
        stored_type_code & !0x200
    };
    v.value_type = cell_value_type(type_code);

    // TABLE_value_fields: R2007+ flag bit 0 suppresses the value body.
    if !version.r2007_plus() || (v.flags & 1) == 0 {
        match type_code {
            0 | 1 => v.numeric_value = reader.read_bit_long() as f64,
            2 => v.numeric_value = reader.read_bit_double(),
            4 => v.text = read_string_cad_value(reader, version),
            8 => {
                v.data_size = reader.read_bit_long();
                if v.data_size > 0 {
                    v.binary_value = reader.read_bytes(v.data_size as usize);
                }
            }
            0x10 => {
                v.data_size = reader.read_bit_long();
                if v.data_size > 0 {
                    let point = reader.read_2raw_double();
                    v.point_value = Vector3::new(point.x, point.y, 0.0);
                }
            }
            0x20 => {
                v.data_size = reader.read_bit_long();
                if v.data_size > 0 {
                    v.point_value = Vector3::new(
                        reader.read_raw_double(),
                        reader.read_raw_double(),
                        reader.read_raw_double(),
                    );
                }
            }
            0x40 => v.handle_value = Some(Handle::from(reader.read_handle())),
            // kBuffer and kResBuf have no body in TABLE_value_fields.
            0x80 | 0x100 => {}
            0x200 => {
                v.text = read_string_cad_value(reader, version);
            }
            // Vendor-defined type: no body is specified, but its exact code is
            // retained and emitted again.
            _ => {}
        }
    }

    if version.r2007_plus() {
        v.raw_unit_type_code = reader.read_bit_long();
        v.unit_type = ValueUnitType::from(v.raw_unit_type_code as u32);
        v.format = reader.read_variable_text();
        if v.raw_unit_type_code != 12 {
            v.formatted_value = reader.read_variable_text();
        }
    }
    v
}

fn read_border(reader: &mut DwgMergedReader) -> CellBorder {
    let mut border = CellBorder::new();
    border.override_flags = BorderPropertyFlags::from_bits_retain(reader.read_bit_long() as u32);
    border.border_type = BorderType::from(reader.read_bit_long() as i16);
    // Linked/formatted table data always carries the full CMTC payload,
    // including when the object is down-saved into an AC1014/AC1015 file.
    border.color = reader.read_cm_true_color();
    border.line_weight = crate::types::LineWeight::from_value(reader.read_bit_long() as i16);
    let line_type = reader.read_handle();
    border.line_type_handle = (line_type != 0).then(|| Handle::from(line_type));
    border.invisible = reader.read_bit_long() != 0;
    border.double_spacing = reader.read_bit_double();
    border
}

struct TableContentFormatData {
    override_flags: i32,
    property_flags: i32,
    value_data_type: i32,
    value_unit_type: i32,
    value_format: String,
    rotation: f64,
    scale: f64,
    alignment: i32,
    color: Color,
    text_style: Option<Handle>,
    text_height: f64,
}

fn read_cell_content_format(reader: &mut DwgMergedReader) -> TableContentFormatData {
    let override_flags = reader.read_bit_long();
    let property_flags = reader.read_bit_long();
    let value_data_type = reader.read_bit_long();
    let value_unit_type = reader.read_bit_long();
    let value_format = reader.read_variable_text();
    let rotation = reader.read_bit_double();
    let scale = reader.read_bit_double();
    let alignment = reader.read_bit_long();
    let color = reader.read_cm_true_color();
    let text_style = reader.read_handle();
    let text_height = reader.read_bit_double();
    TableContentFormatData {
        override_flags,
        property_flags,
        value_data_type,
        value_unit_type,
        value_format,
        rotation,
        scale,
        alignment,
        color,
        text_style: (text_style != 0).then(|| Handle::from(text_style)),
        text_height,
    }
}

fn read_cell_content_geometry(reader: &mut DwgMergedReader) -> CellContentGeometry {
    CellContentGeometry {
        distance_to_top_left: reader.read_3bit_double(),
        distance_to_center: reader.read_3bit_double(),
        width: reader.read_bit_double(),
        height: reader.read_bit_double(),
        outer_width: reader.read_bit_double(),
        outer_height: reader.read_bit_double(),
        flags: reader.read_bit_long(),
    }
}

fn read_cell_style(reader: &mut DwgMergedReader) -> Option<CellStyle> {
    let style_type = reader.read_bit_long();
    let data_flags = reader.read_bit_short();
    if data_flags == 0 {
        return None;
    }
    let mut style = CellStyle::new();
    style.style_type = CellStyleType::from(style_type as u8);
    style.override_flags = reader.read_bit_long();
    style.property_flags = CellStylePropertyFlags::from_bits_retain(reader.read_bit_long() as u32);
    style.background_color = reader.read_cm_true_color();
    style.fill_enabled = style.background_color != Color::ByBlock;
    style.layout_flags = ContentLayoutFlags::from_bits_retain(reader.read_bit_long() as u32);
    let format = read_cell_content_format(reader);
    style.content_format_override_flags = format.override_flags;
    style.content_property_flags = format.property_flags;
    style.value_data_type = format.value_data_type;
    style.value_unit_type = format.value_unit_type;
    style.value_format = format.value_format;
    style.content_color = format.color;
    style.text_style_handle = format.text_style;
    style.text_height = format.text_height;
    style.rotation = format.rotation;
    style.scale = format.scale;
    style.alignment = format.alignment;
    // Margin override flags: bit 0 present => six margin doubles follow.
    style.margin_override_flags = reader.read_bit_short();
    if style.margin_override_flags & 0x01 != 0 {
        style.margin_top = reader.read_bit_double();
        style.margin_left = reader.read_bit_double();
        style.margin_bottom = reader.read_bit_double();
        style.margin_right = reader.read_bit_double();
        style.horizontal_spacing = reader.read_bit_double();
        style.vertical_spacing = reader.read_bit_double();
    }
    let nborders = safe_count(reader.read_bit_long());
    for _ in 0..nborders {
        let edge = reader.read_bit_long() as u32;
        if matches!(edge, 1 | 2 | 4 | 8 | 0x10 | 0x20) {
            style.applied_border_edges |= CellEdgeFlags::from_bits_retain(edge);
            let border = read_border(reader);
            match edge {
                1 => style.top_border = border,
                2 => style.right_border = border,
                4 => style.bottom_border = border,
                8 => style.left_border = border,
                _ => style.additional_borders.push((edge, border)),
            }
        }
    }
    Some(style)
}

fn read_custom_table_data(reader: &mut DwgMergedReader, version: DwgVersion) -> TableCustomData {
    let name = reader.read_variable_text();
    let value = read_cad_value_with_schema(reader, version, true);
    TableCustomData { name, value }
}

fn read_table_cell_content(reader: &mut DwgMergedReader, version: DwgVersion) -> CellContent {
    let mut content = CellContent::new();
    let content_type = reader.read_bit_long();
    content.content_type = match content_type {
        1 => TableCellContentType::Value,
        2 => TableCellContentType::Field,
        4 => TableCellContentType::Block,
        _ => TableCellContentType::Unknown,
    };
    match content_type {
        1 => content.value = read_cad_value_with_schema(reader, version, true),
        2 => {
            let handle = reader.read_handle();
            content.field_handle = (handle != 0).then(|| Handle::from(handle));
        }
        4 => {
            let bh = reader.read_handle(); // block record handle
            if bh != 0 {
                content.block_handle = Some(Handle::from(bh));
            }
        }
        _ => {}
    }
    let natts = safe_count(reader.read_bit_long());
    for _ in 0..natts {
        content.attributes.push(TableAttribute {
            definition_handle: Handle::from(reader.read_handle()),
            value: reader.read_variable_text(),
            index: reader.read_bit_long(),
        });
    }
    if reader.read_bit_short() != 0 {
        let format = read_cell_content_format(reader);
        content.format_override_flags = format.override_flags;
        content.format_property_flags = format.property_flags;
        content.format_value_data_type = format.value_data_type;
        content.format_value_unit_type = format.value_unit_type;
        content.value_format = format.value_format;
        content.color = format.color;
        content.text_style_handle = format.text_style;
        content.text_height = format.text_height;
        content.rotation = format.rotation;
        content.scale = format.scale;
        content.alignment = format.alignment;
    }
    content
}

fn read_table_cell(reader: &mut DwgMergedReader, version: DwgVersion) -> TableCell {
    let mut cell = TableCell::new();
    cell.state = CellStateFlags::from_bits_retain(reader.read_bit_long() as u32);
    cell.tooltip = reader.read_variable_text();
    cell.custom_data = reader.read_bit_long();
    let ndata = safe_count(reader.read_bit_long());
    for _ in 0..ndata {
        cell.custom_data_items
            .push(read_custom_table_data(reader, version));
    }
    cell.has_linked_data = reader.read_bit_long() == 1;
    if cell.has_linked_data {
        let handle = reader.read_handle();
        cell.data_link_handle = (handle != 0).then(|| Handle::from(handle));
        cell.data_link_rows = reader.read_bit_long();
        cell.data_link_columns = reader.read_bit_long();
        cell.data_link_unknown = reader.read_bit_long();
    }
    let ncontents = safe_count(reader.read_bit_long());
    for _ in 0..ncontents {
        cell.contents.push(read_table_cell_content(reader, version));
    }
    cell.style = read_cell_style(reader);
    cell.style_id = reader.read_bit_long();
    let has_geometry = reader.read_bit_long();
    if has_geometry != 0 {
        cell.geometry_data_flag = reader.read_bit_long();
        cell.geometry_width_with_gap = reader.read_bit_double();
        cell.geometry_height_with_gap = reader.read_bit_double();
        let handle = reader.read_handle();
        cell.geometry_handle = (handle != 0).then(|| Handle::from(handle));
        let geometry_count = safe_count(reader.read_bit_long());
        cell.geometry_flags = geometry_count;
        cell.geometries.reserve(geometry_count as usize);
        for index in 0..geometry_count as usize {
            let geometry = read_cell_content_geometry(reader);
            if let Some(content) = cell.contents.get_mut(index) {
                content.geometry = Some(geometry.clone());
            }
            cell.geometries.push(geometry);
        }
        cell.geometry = cell.geometries.first().cloned();
    }
    cell
}

/// The AcDbLinkedTableData / TABLECONTENT body. Returns the columns, rows and
/// the trailing table-style handle.
pub(crate) fn read_table_content(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
) -> (
    String,
    String,
    Vec<TableColumn>,
    Vec<TableRow>,
    Vec<Handle>,
    Option<CellStyle>,
    Vec<crate::entities::table::CellRange>,
    u64,
) {
    let name = reader.read_variable_text();
    let description = reader.read_variable_text();

    let ncols = safe_count(reader.read_bit_long());
    let mut columns = Vec::with_capacity(ncols as usize);
    for _ in 0..ncols {
        let name = reader.read_variable_text();
        let custom_data = reader.read_bit_long();
        let ndata = safe_count(reader.read_bit_long());
        let mut custom_data_items = Vec::with_capacity(ndata as usize);
        for _ in 0..ndata {
            custom_data_items.push(read_custom_table_data(reader, version));
        }
        let style = read_cell_style(reader);
        let style_id = reader.read_bit_long();
        let width = reader.read_bit_double();
        columns.push(TableColumn {
            name,
            width,
            style,
            custom_data,
            custom_data_items,
            style_id,
        });
    }

    let nrows = safe_count(reader.read_bit_long());
    let mut rows = Vec::with_capacity(nrows as usize);
    for _ in 0..nrows {
        let ncells = safe_count(reader.read_bit_long());
        let mut cells = Vec::with_capacity(ncells as usize);
        for _ in 0..ncells {
            cells.push(read_table_cell(reader, version));
        }
        let custom_data = reader.read_bit_long();
        let ndata = safe_count(reader.read_bit_long());
        let mut custom_data_items = Vec::with_capacity(ndata as usize);
        for _ in 0..ndata {
            custom_data_items.push(read_custom_table_data(reader, version));
        }
        let style = read_cell_style(reader);
        let style_id = reader.read_bit_long();
        let height = reader.read_bit_double();
        rows.push(TableRow {
            height,
            cells,
            style,
            custom_data,
            custom_data_items,
            style_id,
        });
    }

    let nfields = safe_count(reader.read_bit_long());
    let mut field_handles = Vec::with_capacity(nfields as usize);
    for _ in 0..nfields {
        field_handles.push(Handle::from(reader.read_handle()));
    }
    let base_style = read_cell_style(reader);
    let nranges = safe_count(reader.read_bit_long());
    let mut merged_ranges = Vec::with_capacity(nranges as usize);
    for _ in 0..nranges {
        merged_ranges.push(crate::entities::table::CellRange {
            top_row: reader.read_bit_long().max(0) as usize,
            left_col: reader.read_bit_long().max(0) as usize,
            bottom_row: reader.read_bit_long().max(0) as usize,
            right_col: reader.read_bit_long().max(0) as usize,
        });
    }
    let style_handle = reader.read_handle();
    (
        name,
        description,
        columns,
        rows,
        field_handles,
        base_style,
        merged_ranges,
        style_handle,
    )
}

/// Pre-R2010 flat-format cell (AcDbTable cell data). The cell value/text is
/// carried in the trailing R2007+ AcDbCellValue block, so the per-cell style
/// override block must be parsed in full to reach it.
fn read_table_cell_data(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> TableCell {
    let mut cell = TableCell::new();
    let ctype = reader.read_bit_short(); // 1 = text, 2 = block
    cell.cell_type = if ctype == 2 {
        CellType::Block
    } else {
        CellType::Text
    };
    cell.edge_flags = reader.read_byte();
    cell.merged = reader.read_bit() as i32;
    cell.auto_fit = reader.read_bit();
    cell.merge_width = reader.read_bit_long();
    cell.merge_height = reader.read_bit_long();
    cell.rotation = reader.read_bit_double();
    let value_handle = reader.read_handle();
    cell.value_handle = (value_handle != 0).then(|| Handle::from(value_handle));

    match ctype {
        1 => {
            // Text: the string is inline only before R2007.
            if value_handle == 0 && dxf_version < DxfVersion::AC1021 {
                let text = reader.read_variable_text();
                cell.contents.push(CellContent::text(&text));
            }
        }
        2 => {
            cell.block_scale = reader.read_bit_double();
            let mut content = CellContent::new();
            content.content_type = TableCellContentType::Block;
            content.block_handle = cell.value_handle;
            if reader.read_bit() {
                let natts = safe_count(reader.read_bit_short() as i32);
                for _ in 0..natts {
                    content.attributes.push(TableAttribute {
                        definition_handle: Handle::from(reader.read_handle()),
                        index: reader.read_bit_short() as i32,
                        value: reader.read_variable_text(),
                    });
                }
            }
            cell.contents.push(content);
        }
        _ => {}
    }

    // Per-cell style override block (each conditional gated on the override
    // flags; the grid line-weight bit gates both the weight and the visibility
    // read, mirroring the source exactly to stay byte-aligned).
    if reader.read_bit() {
        let flags = reader.read_bit_long();
        let mut style = CellStyle::new();
        style.override_flags = flags;
        cell.virtual_edge = reader.read_byte() as i16;
        if flags & 0x01 != 0 {
            style.alignment = reader.read_bit_short() as i32;
        }
        if flags & 0x02 != 0 {
            style.fill_enabled = !reader.read_bit();
        }
        if flags & 0x04 != 0 {
            style.background_color = reader.read_cm_color();
        }
        if flags & 0x08 != 0 {
            style.content_color = reader.read_cm_color();
        }
        if flags & 0x10 != 0 {
            let handle = reader.read_handle();
            style.text_style_handle = (handle != 0).then(|| Handle::from(handle));
        }
        if flags & 0x20 != 0 {
            style.text_height = reader.read_bit_double();
        }
        // Per edge: colour, line weight, visibility.
        for (color_bit, lw_bit, border) in [
            (0x40, 0x400, &mut style.top_border),
            (0x80, 0x800, &mut style.right_border),
            (0x100, 0x1000, &mut style.bottom_border),
            (0x200, 0x2000, &mut style.left_border),
        ] {
            if flags & color_bit != 0 {
                border.color = reader.read_cm_color();
            }
            if flags & lw_bit != 0 {
                border.line_weight = crate::types::LineWeight::from_value(reader.read_bit_short());
            }
            if flags & lw_bit != 0 {
                border.invisible = reader.read_bit_short() == 0;
            }
        }
        cell.style = Some(style);
    }

    // R2007+: unknown then the cell value (holds the text for R2007 files).
    if version.r2007_plus() {
        cell.flag = reader.read_bit_long();
        let mut content = CellContent::new();
        content.content_type = TableCellContentType::Value;
        content.value = read_cad_value(reader, version);
        cell.contents.push(content);
    }

    cell
}

fn read_legacy_table_style_override(reader: &mut DwgMergedReader) -> LegacyTableStyleOverride {
    let flags = reader.read_bit_long();
    let mut value = LegacyTableStyleOverride {
        flags,
        ..LegacyTableStyleOverride::default()
    };
    if flags & 0x0001 != 0 {
        value.title_suppressed = Some(reader.read_bit());
    }
    if flags & 0x0002 != 0 {
        value.header_suppressed = Some(reader.read_bit());
    }
    if flags & 0x0004 != 0 {
        value.flow_direction = Some(reader.read_bit_short());
    }
    if flags & 0x0008 != 0 {
        value.horizontal_cell_margin = Some(reader.read_bit_double());
    }
    if flags & 0x0010 != 0 {
        value.vertical_cell_margin = Some(reader.read_bit_double());
    }
    for bit in [0x0020, 0x0040, 0x0080] {
        if flags & bit != 0 {
            value.row_colors.push(reader.read_cm_color());
        }
    }
    for bit in [0x0100, 0x0200, 0x0400] {
        if flags & bit != 0 {
            value.row_fill_none.push(reader.read_bit());
        }
    }
    for bit in [0x0800, 0x1000, 0x2000] {
        if flags & bit != 0 {
            value.row_fill_colors.push(reader.read_cm_color());
        }
    }
    for bit in [0x4000, 0x8000, 0x10000] {
        if flags & bit != 0 {
            value.row_alignments.push(reader.read_bit_short());
        }
    }
    for bit in [0x20000, 0x40000, 0x80000] {
        if flags & bit != 0 {
            value
                .text_style_handles
                .push(Handle::from(reader.read_handle()));
        }
    }
    for bit in [0x100000, 0x200000, 0x400000] {
        if flags & bit != 0 {
            value.row_heights.push(reader.read_bit_double());
        }
    }
    value
}

fn read_legacy_border_colors(reader: &mut DwgMergedReader) -> LegacyBorderOverrides<Color> {
    let flags = reader.read_bit_long();
    let mut values = Vec::new();
    for bit in 0..18 {
        if flags & (1 << bit) != 0 {
            values.push(reader.read_cm_color());
        }
    }
    LegacyBorderOverrides { flags, values }
}

fn read_legacy_border_line_weights(
    reader: &mut DwgMergedReader,
) -> LegacyBorderOverrides<LineWeight> {
    let flags = reader.read_bit_long();
    let mut values = Vec::new();
    for bit in 0..18 {
        if flags & (1 << bit) != 0 {
            values.push(LineWeight::from_value(reader.read_bit_short()));
        }
    }
    LegacyBorderOverrides { flags, values }
}

fn read_legacy_border_visibility(reader: &mut DwgMergedReader) -> LegacyBorderOverrides<bool> {
    let flags = reader.read_bit_long();
    let mut values = Vec::new();
    for bit in 0..18 {
        if flags & (1 << bit) != 0 {
            values.push(reader.read_bit_short() != 0);
        }
    }
    LegacyBorderOverrides { flags, values }
}

/// Read a full ACAD_TABLE entity: the insert base plus, on R2010+, the inline
/// table content.
pub fn read_table(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> TableEntityData {
    let insert = read_insert(reader, version);

    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let horizontal_direction;
    let style_handle;
    let mut name = String::new();
    let mut description = String::new();
    let mut field_handles = Vec::new();
    let mut base_style = None;
    let mut merged_ranges = Vec::new();
    let mut break_options = 0;
    let mut break_flow_direction = 0;
    let mut break_spacing = 0.0;
    let mut break_data = Vec::new();
    let mut break_ranges = Vec::new();
    let mut unknown_byte = 0;
    let mut unknown_handle = 0;
    let mut unknown_long1 = 0;
    let mut unknown_long2 = 0;
    let mut unknown_short = 0;
    let mut value_flags = 0;
    let mut legacy_style_override = None;
    let mut legacy_border_colors = None;
    let mut legacy_border_line_weights = None;
    let mut legacy_border_visibility = None;

    if version.r2010_plus() {
        unknown_byte = reader.read_byte();
        unknown_handle = reader.read_handle();
        unknown_long1 = reader.read_bit_long();
        if version.r2013_plus(dxf_version) {
            unknown_long2 = reader.read_bit_long();
        } else {
            unknown_long2 = reader.read_bit() as i32;
        }

        let (n, d, cols, rws, fields, table_style, ranges, sh) =
            read_table_content(reader, version);
        name = n;
        description = d;
        columns = cols;
        rows = rws;
        field_handles = fields;
        base_style = table_style;
        merged_ranges = ranges;
        style_handle = sh;

        unknown_short = reader.read_bit_short();
        horizontal_direction = reader.read_3bit_double();

        if reader.read_bit_long() == 1 {
            break_options = reader.read_bit_long();
            break_flow_direction = reader.read_bit_long();
            break_spacing = reader.read_bit_double();
            reader.read_bit_long();
            reader.read_bit_long();
            let num = safe_count(reader.read_bit_long());
            for _ in 0..num {
                break_data.push(TableBreakData {
                    position: reader.read_3bit_double(),
                    height: reader.read_bit_double(),
                    flags: reader.read_bit_long(),
                });
            }
        }
        let nranges = safe_count(reader.read_bit_long());
        for _ in 0..nranges {
            break_ranges.push(TableBreakRange {
                position: reader.read_3bit_double(),
                start_row: reader.read_bit_long(),
                end_row: reader.read_bit_long(),
            });
        }
    } else {
        // Pre-R2010 flat format: value flag, direction, dimensions, grid cells,
        // then optional table-wide and per-border overrides.
        value_flags = reader.read_bit_short() as i32;
        horizontal_direction = reader.read_3bit_double();
        let ncols = safe_count(reader.read_bit_long());
        let nrows = safe_count(reader.read_bit_long());
        for _ in 0..ncols {
            let width = reader.read_bit_double();
            columns.push(TableColumn {
                name: String::new(),
                width,
                style: None,
                custom_data: 0,
                custom_data_items: Vec::new(),
                style_id: 0,
            });
        }
        for _ in 0..nrows {
            let height = reader.read_bit_double();
            rows.push(TableRow {
                height,
                cells: Vec::new(),
                style: None,
                custom_data: 0,
                custom_data_items: Vec::new(),
                style_id: 0,
            });
        }
        style_handle = reader.read_handle();
        for ri in 0..(nrows as usize) {
            for _ in 0..ncols {
                let cell = read_table_cell_data(reader, version, dxf_version);
                rows[ri].cells.push(cell);
            }
        }
        if reader.read_bit() {
            legacy_style_override = Some(read_legacy_table_style_override(reader));
        }
        if reader.read_bit() {
            legacy_border_colors = Some(read_legacy_border_colors(reader));
        }
        if reader.read_bit() {
            legacy_border_line_weights = Some(read_legacy_border_line_weights(reader));
        }
        if reader.read_bit() {
            legacy_border_visibility = Some(read_legacy_border_visibility(reader));
        }
    }

    TableEntityData {
        insert,
        value_flags,
        columns,
        rows,
        horizontal_direction,
        style_handle,
        name,
        description,
        field_handles,
        base_style,
        merged_ranges,
        break_options,
        break_flow_direction,
        break_spacing,
        break_data,
        break_ranges,
        unknown_byte,
        unknown_handle,
        unknown_long1,
        unknown_long2,
        unknown_short,
        legacy_style_override,
        legacy_border_colors,
        legacy_border_line_weights,
        legacy_border_visibility,
    }
}

pub fn read_raster_image(reader: &mut DwgMergedReader, version: DwgVersion) -> RasterImageData {
    let class_version = reader.read_bit_long();
    let insertion_point = reader.read_3bit_double();
    let u_vector = reader.read_3bit_double();
    let v_vector = reader.read_3bit_double();
    let size = reader.read_2raw_double();
    let flags = reader.read_bit_short();
    let clipping_enabled = reader.read_bit();
    let brightness = reader.read_byte();
    let contrast = reader.read_byte();
    let fade = reader.read_byte();
    let clip_inverted = if version.r2010_plus() {
        reader.read_bit()
    } else {
        false
    };

    // Clip boundary
    let clip_type = reader.read_bit_short();
    let mut clip_boundary_vertices: Vec<Vector2> = Vec::new();
    if clip_type == 1 {
        // Rectangular: 2 opposite-corner vertices
        clip_boundary_vertices.push(reader.read_2raw_double());
        clip_boundary_vertices.push(reader.read_2raw_double());
    } else {
        // Polygonal
        let n = safe_count(reader.read_bit_long()) as usize;
        clip_boundary_vertices.reserve(n);
        for _ in 0..n {
            clip_boundary_vertices.push(reader.read_2raw_double());
        }
    }

    let definition_handle = reader.read_handle();
    let reactor_handle = reader.read_handle();

    RasterImageData {
        class_version,
        insertion_point,
        u_vector,
        v_vector,
        size,
        flags,
        clipping_enabled,
        brightness,
        contrast,
        fade,
        clip_inverted,
        clip_type,
        definition_handle,
        reactor_handle,
        clip_boundary_vertices,
    }
}

pub fn read_wipeout(reader: &mut DwgMergedReader, version: DwgVersion) -> RasterImageData {
    // Wipeout uses the same data layout as RasterImage
    read_raster_image(reader, version)
}

pub fn read_ole2frame(reader: &mut DwgMergedReader, version: DwgVersion) -> Ole2FrameData {
    let object_type = reader.read_bit_short();
    let mode = if version.r2000_plus() {
        reader.read_bit_short()
    } else {
        0
    };
    // OLE binary data can be very large (embedded images/documents), so
    // don't use safe_count (100 KB cap). Bound the declared length by what's
    // actually left in the object stream instead of an arbitrary cap — a
    // 10 MB ceiling used to truncate big embedded pictures mid-stream.
    let declared = reader.read_bit_long().max(0) as usize;
    let data_len = declared.min(reader.remaining_bytes());
    let data = reader.read_bytes(data_len);
    let lock_aspect = if version.r2000_plus() {
        reader.read_byte()
    } else {
        0
    };
    let (storage, envelope, upper_left, lower_right) =
        crate::entities::Ole2Frame::decode_payload(&data);
    Ole2FrameData {
        object_type,
        mode,
        storage,
        envelope,
        lock_aspect,
        upper_left,
        lower_right,
    }
}

/// R2018+ multiline attribute payload. When an ATTRIB/ATTDEF reports
/// `mtext_type > 1` the stream carries an embedded MTEXT object
/// (`AcDbMTextObjectEmbedded`) between the type byte and the tag. It must be
/// consumed in full or the tag / field-length / flags that follow shift.
/// The only field we keep is the MTEXT `text` — it holds the real multiline
/// value (`A\PB`) that the plain single-line `text_value` truncates to `A`.
/// The R2018 redundant MTEXT and column tail is part of the embedded object;
/// the attribute-level annotative payload starts only after this returns.
pub(crate) fn read_embedded_mtext(
    reader: &mut DwgMergedReader,
    _version: DwgVersion,
    _dxf_version: DxfVersion,
) -> MTextData {
    // Reduced common-entity preamble.
    let entmode = reader.main_mut().read_2bits();
    if entmode == 0 {
        let _owner = reader.read_handle();
    }
    let _num_reactors = reader.read_bit_long();
    let _is_xdic_missing = reader.read_bit();
    let _has_ds_data = reader.read_bit();
    let _color_raw = reader.read_bit_short();
    let _ltype_scale = reader.read_bit_double();
    let _ltype_flags = reader.main_mut().read_2bits();
    let _plotstyle_flags = reader.main_mut().read_2bits();
    let _material_flags = reader.main_mut().read_2bits();
    let _shadow_flags = reader.read_byte();
    let _has_full_visualstyle = reader.read_bit();
    let _has_face_visualstyle = reader.read_bit();
    let _has_edge_visualstyle = reader.read_bit();
    let _invisible = reader.read_bit_short();
    let _linewt = reader.read_byte();
    let _layer = reader.read_handle();

    // MTEXT geometry.
    let insertion_point = reader.read_3bit_double();
    let normal = reader.read_3bit_double();
    let x_direction = reader.read_3bit_double();
    let rectangle_width = reader.read_bit_double();
    let rectangle_height = reader.read_bit_double();
    let height = reader.read_bit_double();
    let attachment_point = reader.read_bit_short();
    let drawing_direction = reader.read_bit_short();
    let extents_width = reader.read_bit_double();
    let extents_height = reader.read_bit_double();
    let value = reader.read_variable_text();
    let style_handle = reader.read_handle();
    let linespacing_style = reader.read_bit_short();
    let linespacing_factor = reader.read_bit_double();
    let unknown_bit = reader.read_bit();
    let background_flags = reader.read_bit_long();
    let mut background_scale = 1.5;
    let mut background_color = Color::ByLayer;
    let mut background_transparency = 0;
    if background_flags & 1 != 0
        || (_version.r2018_plus(_dxf_version) && background_flags & 0x10 != 0)
    {
        background_scale = reader.read_bit_double();
        background_color = reader.read_cm_color();
        background_transparency = reader.read_bit_long();
    }
    let is_not_annotative = reader.read_bit();
    let mut column_type = 0;
    let mut column_count = 0;
    let mut column_flow_reversed = false;
    let mut column_auto_height = false;
    let mut column_width = 0.0;
    let mut column_gutter = 0.0;
    let mut column_heights = Vec::new();
    if is_not_annotative {
        let _version = reader.read_bit_short();
        let _default_flag = reader.read_bit();
        let _application = reader.read_handle();
        let _attachment = reader.read_bit_long();
        let _x_direction = reader.read_3bit_double();
        let _insertion_point = reader.read_3bit_double();
        let _rectangle_width = reader.read_bit_double();
        let _rectangle_height = reader.read_bit_double();
        let _extents_width = reader.read_bit_double();
        let _extents_height = reader.read_bit_double();

        column_type = reader.read_bit_short();
        if column_type != 0 {
            column_count = safe_count(reader.read_bit_long());
            column_width = reader.read_bit_double();
            column_gutter = reader.read_bit_double();
            column_auto_height = reader.read_bit();
            column_flow_reversed = reader.read_bit();
            if column_type == 2 && !column_auto_height && column_count > 0 {
                column_heights.reserve(column_count as usize);
                for _ in 0..column_count {
                    column_heights.push(reader.read_bit_double());
                }
            }
        }
    }
    MTextData {
        insertion_point,
        normal,
        x_direction,
        rectangle_width,
        rectangle_height,
        height,
        attachment_point,
        drawing_direction,
        extents_height,
        extents_width,
        value,
        style_handle,
        linespacing_style,
        linespacing_factor,
        unknown_bit,
        background_flags,
        background_scale,
        background_color,
        background_transparency,
        is_annotative: !is_not_annotative,
        column_type,
        column_count,
        column_flow_reversed,
        column_auto_height,
        column_width,
        column_gutter,
        column_heights,
    }
}

pub fn read_attribute_definition(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> AttributeCommonData {
    let mut text_data = read_text_entity_data(reader, version);

    let att_version = if version.r2010_plus() {
        reader.read_byte()
    } else {
        0
    };
    let att_type = if version.r2018_plus(dxf_version) {
        reader.read_byte()
    } else {
        1
    };

    // A multiline attribute (mtext_type > 1) embeds a full MTEXT here; its text
    // is the real multiline default value, which the single-line field above
    // truncates to the first line.
    let embedded_mtext = if att_type > 1 {
        let mtext = read_embedded_mtext(reader, version, dxf_version);
        let annotative_data_size = reader.read_bit_short();
        if annotative_data_size > 0 {
            let _ = reader.read_bytes(annotative_data_size as usize);
            let _application = reader.read_handle();
            let _unknown = reader.read_bit_short();
        }
        if !mtext.value.is_empty() {
            text_data.value = mtext.value.clone();
        }
        Some(mtext)
    } else {
        None
    };

    let tag = reader.read_variable_text();
    let field_length = reader.read_bit_short();
    let flags = reader.read_byte();
    let lock_position = if version.r2007_plus() {
        reader.read_bit()
    } else {
        false
    };

    // AttDef-specific: second version byte + prompt
    if version.r2010_plus() {
        let _version2 = reader.read_byte();
    }
    let prompt = reader.read_variable_text();
    // The outer TEXT style is the final ATTDEF handle.  Multiline attributes
    // place both embedded-MTEXT handles before it in the handle stream.
    text_data.style_handle = reader.read_handle();

    AttributeCommonData {
        text_data,
        att_version,
        att_type,
        embedded_mtext,
        tag,
        prompt,
        field_length,
        flags,
        lock_position,
    }
}

pub fn read_attribute_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> AttributeCommonData {
    let mut text_data = read_text_entity_data(reader, version);

    let att_version = if version.r2010_plus() {
        reader.read_byte()
    } else {
        0
    };
    let att_type = if version.r2018_plus(dxf_version) {
        reader.read_byte()
    } else {
        1
    };

    // A multiline attribute (mtext_type > 1) embeds a full MTEXT here; its text
    // is the real multiline value, which the single-line field above truncates
    // to the first line ("A" instead of "A\PB").
    let embedded_mtext = if att_type > 1 {
        let mtext = read_embedded_mtext(reader, version, dxf_version);
        let annotative_data_size = reader.read_bit_short();
        if annotative_data_size > 0 {
            let _ = reader.read_bytes(annotative_data_size as usize);
            let _application = reader.read_handle();
            let _unknown = reader.read_bit_short();
        }
        if !mtext.value.is_empty() {
            text_data.value = mtext.value.clone();
        }
        Some(mtext)
    } else {
        None
    };

    let tag = reader.read_variable_text();
    let field_length = reader.read_bit_short();
    let flags = reader.read_byte();
    let lock_position = if version.r2007_plus() {
        reader.read_bit()
    } else {
        false
    };
    // The outer TEXT style follows the embedded-MTEXT handles.
    text_data.style_handle = reader.read_handle();

    // An ATTRIB instance carries no prompt in the stream — that lives on the
    // ATTDEF. Keep it empty so the shared struct stays consistent.
    AttributeCommonData {
        text_data,
        att_version,
        att_type,
        embedded_mtext,
        tag,
        prompt: String::new(),
        field_length,
        flags,
        lock_position,
    }
}

// ════════════════════════════════════════════════════════════════════════
//  MultiLeader reader
// ════════════════════════════════════════════════════════════════════════

/// Data returned by the multileader reader.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderData {
    pub dwg_version: i16,
    pub context: MultiLeaderAnnotContext,
    pub style_handle: u64,
    pub property_override_flags: u32,
    pub path_type: i16,
    pub line_color: Color,
    pub line_type_handle: u64,
    pub line_weight: i32,
    pub enable_landing: bool,
    pub enable_dogleg: bool,
    pub dogleg_length: f64,
    pub arrowhead_handle: u64,
    pub arrowhead_size: f64,
    pub content_type: i16,
    pub text_style_handle: u64,
    pub text_left_attachment: i16,
    pub text_right_attachment: i16,
    pub text_angle_type: i16,
    pub text_alignment: i16,
    pub text_color: Color,
    pub text_frame: bool,
    pub block_content_handle: u64,
    pub block_content_color: Color,
    pub block_scale: Vector3,
    pub block_rotation: f64,
    pub block_connection_type: i16,
    pub enable_annotation_scale: bool,
    pub block_attributes: Vec<BlockAttribute>,
    pub arrowhead_overrides: Vec<MultiLeaderArrowheadOverride>,
    pub text_direction_negative: bool,
    pub text_align_in_ipe: i16,
    pub text_attachment_point: i16,
    pub scale_factor: f64,
    pub text_attachment_direction: i16,
    pub text_bottom_attachment: i16,
    pub text_top_attachment: i16,
    pub extend_leader_to_text: bool,
}

pub fn read_multileader(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> MultiLeaderData {
    let dwg_version = if version.r2010_plus() {
        reader.read_bit_short()
    } else {
        2
    };

    // Annotation context (inline)
    let mut context = read_multileader_annotation_context(reader, version, dxf_version, false);

    // Common data
    let style_handle = reader.read_handle();
    let property_override_flags = reader.read_bit_long() as u32;
    let path_type = reader.read_bit_short();
    let line_color = reader.read_cm_color();
    let line_type_handle = reader.read_handle();
    let line_weight = reader.read_bit_long();
    let enable_landing = reader.read_bit();
    let enable_dogleg = reader.read_bit();
    let dogleg_length = reader.read_bit_double();
    let arrowhead_handle = reader.read_handle();
    let arrowhead_size = reader.read_bit_double();
    let content_type = reader.read_bit_short();
    let text_style_handle = reader.read_handle();
    let text_left_attachment = reader.read_bit_short();
    let text_right_attachment = reader.read_bit_short();
    let text_angle_type = reader.read_bit_short();
    let text_alignment = reader.read_bit_short();
    let text_color = reader.read_cm_color();
    let text_frame = reader.read_bit();
    let block_content_handle = reader.read_handle();
    let block_content_color = reader.read_cm_color();
    let block_scale = reader.read_3bit_double();
    let block_rotation = reader.read_bit_double();
    let block_connection_type = reader.read_bit_short();
    let enable_annotation_scale = reader.read_bit();

    // R2010 introduced per-leader-line appearance fields. Older records omit
    // them entirely, so each line inherits the corresponding MLeader common
    // value instead of acquiring unrelated synthetic defaults.
    if !version.r2010_plus() {
        let inherited_path_type = MultiLeaderPathType::from(path_type);
        let inherited_line_weight = crate::types::LineWeight::from_value(line_weight as i16);
        let inherited_line_type_handle =
            (line_type_handle != 0).then(|| Handle::from(line_type_handle));
        let inherited_arrowhead_handle =
            (arrowhead_handle != 0).then(|| Handle::from(arrowhead_handle));
        for root in &mut context.leader_roots {
            for line in &mut root.lines {
                line.path_type = inherited_path_type;
                line.line_color = line_color;
                line.line_type_handle = inherited_line_type_handle;
                line.line_weight = inherited_line_weight;
                line.arrowhead_handle = inherited_arrowhead_handle;
                line.arrowhead_size = arrowhead_size;
            }
        }
    }

    // Pre-R2007 only: num_arrowheads (BL) + override-arrowhead list.
    let mut arrowhead_overrides = Vec::new();
    if !version.r2007_plus() {
        let ah_count = safe_count(reader.read_bit_long());
        arrowhead_overrides.reserve(ah_count as usize);
        for index in 0..ah_count {
            let is_default = reader.read_bit();
            let handle = reader.read_handle();
            arrowhead_overrides.push(MultiLeaderArrowheadOverride {
                index: index as i32,
                is_default,
                arrowhead_handle: (handle != 0).then(|| Handle::from(handle)),
            });
        }
    }

    // All MLeader versions carry num_blocklabels (BL) + block labels, then
    // text-direction / alignment / attachment-point / scale. These fields
    // immediately follow the pre-R2007 arrowhead override list when present.
    let mut block_attributes = Vec::new();
    let ba_count = safe_count(reader.read_bit_long());
    block_attributes.reserve(ba_count as usize);
    for _ in 0..ba_count {
        let def_handle = reader.read_handle();
        let text = reader.read_variable_text();
        let index = reader.read_bit_short();
        let width = reader.read_bit_double();
        block_attributes.push(BlockAttribute {
            attribute_definition_handle: if def_handle != 0 {
                Some(Handle::from(def_handle))
            } else {
                None
            },
            text,
            index,
            width,
        });
    }
    let text_direction_negative = reader.read_bit();
    let text_align_in_ipe = reader.read_bit_short();
    let text_attachment_point = reader.read_bit_short();
    let scale_factor = reader.read_bit_double();

    let mut text_attachment_direction: i16 = 0;
    let mut text_bottom_attachment: i16 = 9; // CenterOfText — matches MultiLeader::new() default
    let mut text_top_attachment: i16 = 9; // CenterOfText — matches MultiLeader::new() default
    if version.r2010_plus() {
        // Order: dir (271), bottom (272), top (273) — per AutoCAD.
        text_attachment_direction = reader.read_bit_short();
        text_bottom_attachment = reader.read_bit_short();
        text_top_attachment = reader.read_bit_short();
    }

    let mut extend_leader_to_text = false;
    if version.r2013_plus(dxf_version) {
        extend_leader_to_text = reader.read_bit();
    }

    MultiLeaderData {
        dwg_version,
        context,
        style_handle,
        property_override_flags,
        path_type,
        line_color,
        line_type_handle,
        line_weight,
        enable_landing,
        enable_dogleg,
        dogleg_length,
        arrowhead_handle,
        arrowhead_size,
        content_type,
        text_style_handle,
        text_left_attachment,
        text_right_attachment,
        text_angle_type,
        text_alignment,
        text_color,
        text_frame,
        block_content_handle,
        block_content_color,
        block_scale,
        block_rotation,
        block_connection_type,
        enable_annotation_scale,
        block_attributes,
        arrowhead_overrides,
        text_direction_negative,
        text_align_in_ipe,
        text_attachment_point,
        scale_factor,
        text_attachment_direction,
        text_bottom_attachment,
        text_top_attachment,
        extend_leader_to_text,
    }
}

pub(crate) fn read_multileader_annotation_context(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    standalone: bool,
) -> MultiLeaderAnnotContext {
    // Leader root count
    let mut leader_root_count = safe_count(reader.read_bit_long());
    let standalone_uses_root_flags = standalone && leader_root_count == 0;
    let mut standalone_flags = 0u8;
    if standalone && leader_root_count == 0 {
        for bit in 0..5 {
            if reader.read_bit() {
                standalone_flags |= 1 << bit;
            }
        }
        let has_two_roots = reader.read_bit();
        let has_one_root = reader.read_bit();
        leader_root_count = if has_two_roots {
            2
        } else if has_one_root {
            1
        } else {
            0
        };
    }

    // Read each leader root
    let mut leader_roots = Vec::with_capacity(leader_root_count as usize);
    for _ in 0..leader_root_count {
        leader_roots.push(read_leader_root(reader, version, dxf_version));
    }

    // Common data
    let scale_factor = reader.read_bit_double();
    let content_base_point = reader.read_3bit_double();
    let text_height = reader.read_bit_double();
    let arrowhead_size = reader.read_bit_double();
    let landing_gap = reader.read_bit_double();
    let text_left_attachment = TextAttachmentType::from(reader.read_bit_short());
    let text_right_attachment = TextAttachmentType::from(reader.read_bit_short());
    let text_alignment = TextAlignmentType::from(reader.read_bit_short());
    let block_connection_type = BlockContentConnectionType::from(reader.read_bit_short());

    let has_text_contents = reader.read_bit();

    let mut text_string = String::new();
    let mut text_normal = Vector3::ZERO;
    let mut text_style_handle: Option<Handle> = None;
    let mut text_location = Vector3::ZERO;
    let mut text_direction = Vector3::UNIT_X;
    let mut text_rotation = 0.0;
    let mut text_width = 0.0;
    let mut text_boundary_height = 0.0;
    let mut line_spacing_factor = 1.0;
    let mut line_spacing_style = LineSpacingStyle::default();
    let mut text_color = Color::ByLayer;
    let mut text_attachment_point = TextAttachmentPointType::default();
    let mut text_flow_direction = FlowDirectionType::default();
    let mut background_fill_color = Color::ByLayer;
    let mut background_scale_factor = 1.5;
    let mut background_transparency = 0i32;
    let mut background_fill_enabled = false;
    let mut background_mask_fill_on = false;
    let mut column_type = 0i16;
    let mut text_height_automatic = false;
    let mut column_width = 0.0;
    let mut column_gutter = 0.0;
    let mut column_flow_reversed = false;
    let mut column_sizes: Vec<f64> = Vec::new();
    let mut word_break = false;
    let mut dwg_unknown_text_bit = false;

    if has_text_contents {
        text_string = reader.read_variable_text();
        text_normal = reader.read_3bit_double();
        let ts_handle = reader.read_handle();
        text_style_handle = if ts_handle != 0 {
            Some(Handle::from(ts_handle))
        } else {
            None
        };
        text_location = reader.read_3bit_double();
        text_direction = reader.read_3bit_double();
        text_rotation = reader.read_bit_double();
        text_width = reader.read_bit_double();
        text_boundary_height = reader.read_bit_double();
        line_spacing_factor = reader.read_bit_double();
        line_spacing_style = LineSpacingStyle::from(reader.read_bit_short());
        text_color = reader.read_cm_color();
        text_attachment_point = TextAttachmentPointType::from(reader.read_bit_short());
        text_flow_direction = FlowDirectionType::from(reader.read_bit_short());
        background_fill_color = reader.read_cm_color();
        background_scale_factor = reader.read_bit_double();
        background_transparency = reader.read_bit_long();
        background_fill_enabled = reader.read_bit();
        background_mask_fill_on = reader.read_bit();
        column_type = reader.read_bit_short();
        text_height_automatic = reader.read_bit();
        column_width = reader.read_bit_double();
        column_gutter = reader.read_bit_double();
        column_flow_reversed = reader.read_bit();

        let col_count = safe_count(reader.read_bit_long());
        column_sizes = Vec::with_capacity(col_count as usize);
        for _ in 0..col_count {
            column_sizes.push(reader.read_bit_double());
        }

        word_break = reader.read_bit();
        dwg_unknown_text_bit = reader.read_bit();
    }

    // has_block_contents bit is only present when has_text_contents is false
    // (else-if structure in the DWG format — text and block are mutually exclusive)
    let mut has_block_contents = false;

    let mut block_content_handle: Option<Handle> = None;
    let mut block_content_normal = Vector3::UNIT_Z;
    let mut block_content_location = Vector3::ZERO;
    let mut block_content_scale = Vector3::new(1.0, 1.0, 1.0);
    let mut block_rotation = 0.0;
    let mut block_content_color = Color::ByBlock;
    let mut transform_matrix = [0.0f64; 16];
    // Set identity
    transform_matrix[0] = 1.0;
    transform_matrix[5] = 1.0;
    transform_matrix[10] = 1.0;
    transform_matrix[15] = 1.0;

    if !has_text_contents {
        has_block_contents = reader.read_bit();

        if has_block_contents {
            let bh = reader.read_handle();
            block_content_handle = if bh != 0 {
                Some(Handle::from(bh))
            } else {
                None
            };
            block_content_normal = reader.read_3bit_double();
            block_content_location = reader.read_3bit_double();
            block_content_scale = reader.read_3bit_double();
            block_rotation = reader.read_bit_double();
            block_content_color = reader.read_cm_color();

            for i in 0..16 {
                transform_matrix[i] = reader.read_bit_double();
            }
        }
    }

    let base_point = reader.read_3bit_double();
    let base_direction = reader.read_3bit_double();
    let base_vertical = reader.read_3bit_double();
    let normal_reversed = reader.read_bit();

    let mut text_top_attachment = TextAttachmentType::CenterOfText;
    let mut text_bottom_attachment = TextAttachmentType::CenterOfText;
    if version.r2010_plus() {
        text_top_attachment = TextAttachmentType::from(reader.read_bit_short());
        text_bottom_attachment = TextAttachmentType::from(reader.read_bit_short());
    }

    MultiLeaderAnnotContext {
        leader_roots,
        standalone_flags,
        standalone_uses_root_flags,
        scale_factor,
        content_base_point,
        has_text_contents,
        text_string,
        text_normal,
        text_location,
        text_direction,
        text_rotation,
        text_height,
        text_width,
        text_boundary_height,
        line_spacing_factor,
        line_spacing_style,
        text_color,
        text_attachment_point,
        text_flow_direction,
        text_alignment,
        text_left_attachment,
        text_right_attachment,
        text_top_attachment,
        text_bottom_attachment,
        text_height_automatic,
        word_break,
        dwg_unknown_text_bit,
        text_style_handle,
        has_block_contents,
        block_content_handle,
        block_content_normal,
        block_content_location,
        block_content_scale,
        block_rotation,
        block_content_color,
        block_connection_type,
        column_type,
        column_width,
        column_gutter,
        column_flow_reversed,
        column_sizes,
        background_fill_enabled,
        background_mask_fill_on,
        background_fill_color,
        background_scale_factor,
        background_transparency,
        base_point,
        base_direction,
        base_vertical,
        normal_reversed,
        arrowhead_size,
        landing_gap,
        transform_matrix,
        scale_handle: None,
    }
}

fn read_leader_root(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    _dxf_version: DxfVersion,
) -> LeaderRoot {
    let content_valid = reader.read_bit();
    let unknown = reader.read_bit();
    let connection_point = reader.read_3bit_double();
    let direction = reader.read_3bit_double();

    let bp_count = safe_count(reader.read_bit_long());
    let mut break_points = Vec::with_capacity(bp_count as usize);
    for _ in 0..bp_count {
        let start_point = reader.read_3bit_double();
        let end_point = reader.read_3bit_double();
        break_points.push(StartEndPointPair {
            start_point,
            end_point,
        });
    }

    let leader_index = reader.read_bit_long();
    let landing_distance = reader.read_bit_double();

    let line_count = safe_count(reader.read_bit_long());
    let mut lines = Vec::with_capacity(line_count as usize);
    for _ in 0..line_count {
        lines.push(read_leader_line(reader, version));
    }

    let mut text_attachment_direction = TextAttachmentDirectionType::default();
    if version.r2010_plus() {
        text_attachment_direction = TextAttachmentDirectionType::from(reader.read_bit_short());
    }

    LeaderRoot {
        content_valid,
        unknown,
        connection_point,
        direction,
        break_points,
        leader_index,
        landing_distance,
        lines,
        text_attachment_direction,
    }
}

fn read_leader_line(reader: &mut DwgMergedReader, version: DwgVersion) -> LeaderLine {
    let pt_count = safe_count(reader.read_bit_long());
    let mut points = Vec::with_capacity(pt_count as usize);
    for _ in 0..pt_count {
        points.push(reader.read_3bit_double());
    }

    let break_info_count = safe_count(reader.read_bit_long());
    let mut break_infos = Vec::with_capacity(break_info_count as usize);
    for _ in 0..break_info_count {
        let segment_index = reader.read_bit_long();
        let sep_count = safe_count(reader.read_bit_long());
        let mut break_points = Vec::with_capacity(sep_count as usize);
        for _ in 0..sep_count {
            let start_point = reader.read_3bit_double();
            let end_point = reader.read_3bit_double();
            break_points.push(StartEndPointPair {
                start_point,
                end_point,
            });
        }
        break_infos.push(LeaderLineBreakInfo {
            segment_index,
            break_points,
        });
    }
    let (segment_index, break_points) = break_infos.first().map_or_else(
        || (0, Vec::new()),
        |info| (info.segment_index, info.break_points.clone()),
    );

    let index = reader.read_bit_long();

    let mut path_type = MultiLeaderPathType::default();
    let mut line_color = Color::ByBlock;
    let mut line_type_handle: Option<Handle> = None;
    // Defaults for a pre-R2010 source upconverted to R2010+: a leader line that
    // does not override these must match AutoCAD's emission — ByBlock weight and
    // a 0.0 arrow size. 0.0 is load-bearing: write_bit_double emits the 2-bit
    // BD-zero code, not a 66-bit double, so the annotation context stays the
    // length AutoCAD's R2018 reader expects (a non-zero default over-runs it →
    // eDwgObjectImproperlyRead).
    let mut line_weight = crate::types::LineWeight::ByBlock;
    let mut arrowhead_size = 0.0;
    let mut arrowhead_handle: Option<Handle> = None;
    let mut override_flags = LeaderLinePropertyOverrideFlags::NONE;

    if version.r2010_plus() {
        path_type = MultiLeaderPathType::from(reader.read_bit_short());
        line_color = reader.read_cm_color();
        let lt_handle = reader.read_handle();
        line_type_handle = if lt_handle != 0 {
            Some(Handle::from(lt_handle))
        } else {
            None
        };
        let lw = reader.read_bit_long();
        line_weight = crate::types::LineWeight::from_value(lw as i16);
        arrowhead_size = reader.read_bit_double();
        let ah_handle = reader.read_handle();
        arrowhead_handle = if ah_handle != 0 {
            Some(Handle::from(ah_handle))
        } else {
            None
        };
        override_flags =
            LeaderLinePropertyOverrideFlags::from_bits_retain(reader.read_bit_long() as u32);
    }

    LeaderLine {
        points,
        break_info_count,
        segment_index,
        break_points,
        break_infos,
        index,
        path_type,
        line_color,
        line_type_handle,
        line_weight,
        arrowhead_size,
        arrowhead_handle,
        override_flags,
    }
}

// ════════════════════════════════════════════════════════════════════════
//  ACIS / Modeler-geometry readers (3DSOLID, REGION, BODY)
// ════════════════════════════════════════════════════════════════════════

/// Data returned by the ACIS entity reader (shared between 3DSOLID, REGION, BODY).
#[derive(Debug, Clone)]
pub struct AcisEntityData {
    /// True when the entity carries no ACIS data (empty body).
    pub acis_empty: bool,
    /// SAT text data (version 1, pre-R2007).
    pub sat_data: String,
    /// SAB binary data (version 2, R2007+).
    pub sab_data: Vec<u8>,
    /// Whether the data is binary SAB (true) or text SAT (false).
    pub is_binary: bool,
    /// ACIS version marker as read from the stream.
    pub version: i16,
    /// Point on entity (wireframe anchor), if wireframe data was present.
    pub point: Vector3,
    /// Whether the entity has a history handle (3DSOLID only, R2007+).
    pub has_history: bool,
    /// ISOLINES display setting from the wireframe section.
    pub isolines: i32,
    pub wireframe_data_present: bool,
    pub wireframe_point_present: bool,
    pub wireframe_isoline_present: bool,
    pub acis_empty_bit: bool,
    pub extra_acis_data: Option<crate::entities::solid3d::AcisData>,
    /// Wireframe edges for visualization.
    pub wires: Vec<Wire>,
    /// Silhouette data for viewports.
    pub silhouettes: Vec<Silhouette>,
    /// R2013+ modeler-geometry revision block (`COMMON_3DSOLID`).
    pub revision: AcisRevision,
    /// R2007+ material bindings.
    pub materials: Vec<AcisMaterial>,
}

#[derive(Debug, Clone)]
pub struct SurfaceEntityData {
    pub acis: AcisEntityData,
    pub modeler_format_version: i16,
    pub u_isolines: i16,
    pub v_isolines: i16,
    pub surface_data: SurfaceData,
    pub history_handle: u64,
}

/// Read modeler-geometry (ACIS) data shared by 3DSOLID, REGION, BODY.
///
/// This reads both `DECODE_3DSOLID` (acis data) and the wireframe +
/// `acis_empty_bit` + R2007+ trailing fields from `COMMON_3DSOLID`.
/// The caller must still read the 3DSOLID-specific history_id handle.
fn read_extra_acis_data(
    reader: &mut DwgMergedReader,
    inline_end: Option<i64>,
) -> Option<crate::entities::solid3d::AcisData> {
    let prefix_start = reader.position_in_bits();
    let _unknown = reader.read_bit();
    let extra_version = reader.read_bit_short();

    if extra_version == 2 {
        let data_start = reader.position_in_bits();
        let remaining_bits =
            (inline_end.unwrap_or_else(|| reader.handle_start()) - data_start).max(0) as usize;
        let probe = reader.read_bytes(remaining_bits / 8);
        if !probe.starts_with(b"ACIS BinaryFile")
            && !(inline_end.is_some() && probe.starts_with(b"ASM BinaryFile"))
        {
            reader.set_position_in_bits(prefix_start);
            return None;
        }
        if let Ok((_, used)) = crate::entities::acis::SabReader::read_with_consumed(&probe) {
            if used <= probe.len() {
                reader.set_position_in_bits(data_start + used as i64 * 8);
                return Some(crate::entities::solid3d::AcisData::from_sab(
                    probe[..used].to_vec(),
                ));
            }
        }
        reader.set_position_in_bits(prefix_start);
        return None;
    }

    if extra_version == 1 {
        let mut encrypted = Vec::new();
        loop {
            let block_size = reader.read_bit_long();
            if block_size == 0 {
                break;
            }
            let available = inline_end.map_or_else(
                || reader.remaining_bytes(),
                |end| (end - reader.position_in_bits()).max(0) as usize / 8,
            );
            if block_size < 0 || block_size as usize > available {
                reader.set_position_in_bits(prefix_start);
                return None;
            }
            encrypted.extend_from_slice(&reader.read_bytes(block_size as usize));
        }
        if encrypted.is_empty() {
            reader.set_position_in_bits(prefix_start);
            return None;
        }
        let decoded: Vec<u8> = encrypted
            .into_iter()
            .map(|byte| {
                if byte <= 32 {
                    byte
                } else {
                    159u8.wrapping_sub(byte)
                }
            })
            .collect();
        let sat = String::from_utf8_lossy(&decoded);
        return Some(crate::entities::solid3d::AcisData::from_sat(&sat));
    }

    reader.set_position_in_bits(prefix_start);
    None
}

pub fn read_acis_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    has_ds_data: bool,
) -> AcisEntityData {
    read_acis_entity_impl(reader, version, dxf_version, has_ds_data, false, false, None)
        .expect("database ACIS decoding retains unrecognized legacy payloads")
}

/// Solid-history B-rep objects keep their modeler body inline even in R2013+
/// drawings, while their material references still use the object handle stream.
pub(super) fn read_history_acis_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> AcisEntityData {
    read_acis_entity_impl(reader, version, dxf_version, false, false, true, None)
        .expect("history ACIS decoding retains unrecognized modeler payloads")
}

/// Embedded construction profiles have no database handle or AcDs entry.
/// Their modeler body remains inline even in newer drawing versions.
pub(crate) fn read_inline_acis_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    bit_length: usize,
) -> Option<AcisEntityData> {
    let start = reader.position_in_bits();
    let end = start.checked_add(i64::try_from(bit_length).ok()?)?;
    if end > reader.main_mut().data_len() as i64 * 8 {
        return None;
    }
    let data = read_acis_entity_impl(reader, version, dxf_version, false, true, true, Some(end))?;
    let remaining = end - reader.position_in_bits();
    // Byte-sized enclosing records may carry up to seven zero padding bits.
    // Never normalize an unrecognized modeler tail into a partial REGION.
    if !(0..=7).contains(&remaining) || (0..remaining).any(|_| reader.read_bit()) {
        return None;
    }
    Some(data)
}

fn read_acis_entity_impl(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    has_ds_data: bool,
    allow_extra: bool,
    inline_layout: bool,
    inline_end: Option<i64>,
) -> Option<AcisEntityData> {
    // R2013+ moved modeler data into AcDs and removed the leading
    // `acis_empty` bit from the entity record.  The first bit after common
    // entity data is `wireframe_data_present` in that layout.  Consuming the
    // legacy bit here shifts every wire/material/revision field by one.
    let acis_empty = if version.r2013_plus(dxf_version) && !inline_layout {
        !has_ds_data
    } else {
        reader.read_bit()
    };

    let mut sat_data = String::new();
    let mut sab_data = Vec::new();
    let mut is_binary = false;
    let mut acis_version: i16 = 0;

    // R2013+ entities with `has_ds_data` keep geometry in AcDs. Their inline
    // record still contains the shared wire/material/revision tail and, for
    // surfaces, subtype data, so continue after marking it as binary v2.
    if has_ds_data && version.r2013_plus(dxf_version) {
        is_binary = true;
        acis_version = 2;
    }

    if !acis_empty && !has_ds_data {
        // Unknown bit — per ODA spec / LibreDWG this B
        // is always present between acis_empty and the version BS.
        let _unknown = reader.read_bit();

        acis_version = reader.read_bit_short();
        if inline_end.is_some() && !matches!(acis_version, 1 | 2) {
            return None;
        }

        if acis_version == 1 {
            // SAT text — all DWG versions use the same encoding:
            // BL-sized blocks of encrypted bytes (cipher: 159 - byte)
            // terminated by BL(0).  Per LibreDWG dwg.spec.
            is_binary = false;

            let mut all_bytes = Vec::new();
            loop {
                let raw_block_size = reader.read_bit_long();
                if let Some(end) = inline_end {
                    if raw_block_size < 0
                        || reader.position_in_bits() > end
                        || raw_block_size as i64 > (end - reader.position_in_bits()) / 8
                    {
                        return None;
                    }
                }
                let block_size = raw_block_size.max(0) as usize;
                if block_size == 0 || block_size > 50_000_000 {
                    break;
                }
                let block = reader.read_bytes(block_size);
                all_bytes.extend_from_slice(&block);
            }

            // Decrypt with selective 159-substitution cipher
            // (per LibreDWG dwg.spec: bytes <= 32 pass through, bytes > 32: 159 - byte)
            let mut decoded = Vec::with_capacity(all_bytes.len());
            for &b in &all_bytes {
                if b <= 32 {
                    decoded.push(b);
                } else {
                    decoded.push(159u8.wrapping_sub(b));
                }
            }
            sat_data = String::from_utf8_lossy(&decoded).to_string();
            sat_data = crate::entities::solid3d::AcisData::strip_sat_terminator(&sat_data);
        } else if let Some(end) = inline_end {
            // Embedded bodies have no handle/text streams. Bound the SAB
            // probe by the enclosing entity's meaningful bit count instead
            // of the absent database handle-stream offset.
            is_binary = true;
            let start = reader.position_in_bits();
            let available = usize::try_from(end.checked_sub(start)?).ok()? / 8;
            let probe = reader.read_bytes(available);
            let (_, used) = crate::entities::acis::SabReader::read_with_consumed(&probe).ok()?;
            if used == 0 || used > probe.len() {
                return None;
            }
            sab_data = probe[..used].to_vec();
            reader.set_position_in_bits(start + used as i64 * 8);
        } else if !version.r2007_plus() {
            // SAB binary, R2004–R2006: the bytes flow with NO length prefix
            // ("ACIS BinaryFile…" starts right after the version BS —
            // bit-verified on an AC1018 save). The payload is self-delimiting
            // (End-of-ACIS-data record), so parse once to measure it, keep
            // exactly that many bytes, then restore the bit cursor immediately
            // after the SAB. The COMMON_3DSOLID wireframe tail follows there.
            is_binary = true;
            let start = reader.position_in_bits();
            let avail = reader.remaining_bytes();
            let probe = reader.read_bytes(avail);
            match crate::entities::acis::SabReader::read_with_consumed(&probe) {
                Ok((_, used)) => {
                    let used = used.min(probe.len());
                    sab_data = probe[..used].to_vec();
                    reader.set_position_in_bits(start + used as i64 * 8);
                }
                Err(_) => sab_data = probe,
            }
        } else {
            // Inline SAB is self-delimited. Measure only its binary records;
            // shared ACIS and surface subtype fields follow in the main stream.
            is_binary = true;
            let start = reader.main_mut().position_in_bits();
            let remaining_bits = (reader.handle_start() - 1 - start).max(0) as usize;
            let probe = reader.read_bytes(remaining_bits / 8);
            let used = crate::entities::acis::SabReader::read_with_consumed(&probe)
                .map(|(_, used)| used)
                .unwrap_or(probe.len());
            sab_data = probe[..used.min(probe.len())].to_vec();
            reader
                .main_mut()
                .set_position_in_bits(start + used as i64 * 8);
            if used == probe.len() {
                return Some(AcisEntityData {
                    acis_empty,
                    sat_data,
                    sab_data,
                    is_binary,
                    version: acis_version,
                    point: crate::types::Vector3::ZERO,
                    has_history: false,
                    isolines: 0,
                    wireframe_data_present: false,
                    wireframe_point_present: false,
                    wireframe_isoline_present: false,
                    acis_empty_bit: false,
                    extra_acis_data: None,
                    wires: Vec::new(),
                    silhouettes: Vec::new(),
                    revision: AcisRevision::default(),
                    materials: Vec::new(),
                });
            }
        }
    }

    // Wireframe data (version=1 SAT, R2004–R2006 inline SAB and AcDs-backed
    // empty bodies; R2007+ inline SAB may return early above).
    // Layout per LibreDWG COMMON_3DSOLID,
    // bit-verified against an AutoCAD AC1015 (R2000) v1-SAT sample — the
    // `point` decodes to the body's exact bounding-box centre only WITH the
    // point_present gate (AC1032's solids are SAB and skip this path, so it
    // was never really exercised before):
    //   B wireframe_data_present
    //   if set: B point_present, [3BD point],
    //           BL isolines, B isoline_present,
    //           if set: BL num_wires, wires..., BL num_silhouettes, sils...
    // AutoCAD writes isoline_present=1 with zero counts when there is no
    // wire cache.
    let wireframe_present = reader.read_bit();
    let mut point = Vector3::ZERO;
    let mut point_present = false;
    let mut isoline_present = false;
    let mut acis_empty_bit = false;
    let mut isolines: i32 = 0;
    let mut wires = Vec::new();
    let mut silhouettes = Vec::new();

    if wireframe_present {
        // LibreDWG COMMON_3DSOLID: a point_present bit gates the 3BD point.
        point_present = reader.read_bit();
        if point_present {
            point = reader.read_3bit_double();
        }
        isolines = reader.read_bit_long();
        isoline_present = reader.read_bit();
        if isoline_present {
            let num_wires = safe_count(reader.read_bit_long());
            for _ in 0..num_wires {
                wires.push(read_wire(reader, version));
            }
        }

        // Silhouettes belong to the wireframe body, but are independent of
        // the isoline-data gate above.
        let num_silhouettes = safe_count(reader.read_bit_long());
        for _ in 0..num_silhouettes {
            let viewport_id = reader.read_bit_long_long();
            let target = reader.read_3bit_double();
            let view_direction = reader.read_3bit_double();
            let up_vector = reader.read_3bit_double();
            let is_perspective = reader.read_bit();
            let mut sil_wires = Vec::new();
            let has_sil_wires = reader.read_bit();
            if has_sil_wires {
                let num_sw = safe_count(reader.read_bit_long());
                sil_wires.reserve(num_sw as usize);
                for _ in 0..num_sw {
                    sil_wires.push(read_wire(reader, version));
                }
            }
            silhouettes.push(Silhouette {
                viewport_id,
                view_direction,
                up_vector,
                target,
                is_perspective,
                has_wires: has_sil_wires,
                wires: sil_wires,
            });
        }
    }

    // The legacy inline layout always carries this bit. AcDs-backed R2013+
    // records carry it only inside a present wireframe cache.
    if inline_end.is_some() || wireframe_present || !version.r2013_plus(dxf_version) {
        acis_empty_bit = reader.read_bit();
    }
    let extra_acis_data = if allow_extra && !acis_empty_bit {
        read_extra_acis_data(reader, inline_end)
    } else {
        None
    };

    let mut materials = Vec::new();
    if version.r2007_plus() {
        if acis_version > 1 && !has_ds_data {
            let count = safe_count(reader.read_bit_long());
            materials.reserve(count as usize);
            for _ in 0..count {
                let array_index = reader.read_bit_long();
                let absolute_reference = reader.read_bit_long();
                let material_handle = if inline_end.is_some() {
                    reader.read_main_handle()
                } else {
                    reader.read_handle()
                };
                materials.push(AcisMaterial {
                    array_index,
                    absolute_reference,
                    material_handle: (material_handle != 0).then(|| Handle::from(material_handle)),
                });
            }
        } else {
            // AcDs-backed R2013+ entities and version-1 bodies carry the
            // legacy R2007 unknown BL here, not a materials array.
            let _unknown_2007 = reader.read_bit_long();
        }
    }

    // R2013+ (AC1027+): the modeler-geometry revision block. It must be read
    // (and later written back) or the entity stream desyncs — AutoCAD/TrueView
    // then reject the file.
    let revision = if version.r2013_plus(dxf_version) {
        let has_guid = reader.read_bit();
        let major = reader.read_bit_long() as u32;
        let minor1 = reader.read_bit_short();
        let minor2 = reader.read_bit_short();
        let raw = reader.read_bytes(8);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&raw[..8.min(raw.len())]);
        let end_marker = reader.read_bit_long() as u32;
        AcisRevision {
            has_guid,
            major,
            minor1,
            minor2,
            bytes,
            end_marker,
        }
    } else {
        AcisRevision::default()
    };

    if inline_end.is_some_and(|end| reader.position_in_bits() > end) {
        return None;
    }
    Some(AcisEntityData {
        acis_empty,
        sat_data,
        sab_data,
        is_binary,
        version: acis_version,
        point,
        has_history: false,
        isolines,
        wireframe_data_present: wireframe_present,
        wireframe_point_present: point_present,
        wireframe_isoline_present: isoline_present,
        acis_empty_bit,
        extra_acis_data,
        wires,
        silhouettes,
        revision,
        materials,
    })
}

fn read_surface_matrix(reader: &mut DwgMergedReader) -> [f64; 16] {
    let mut value = [0.0; 16];
    for item in &mut value {
        *item = reader.read_bit_double();
    }
    value
}

fn read_surface_sweep_options(reader: &mut DwgMergedReader) -> SurfaceSweepOptions {
    let draft_angle = reader.read_bit_double();
    let draft_start_distance = reader.read_bit_double();
    let draft_end_distance = reader.read_bit_double();
    let twist_angle = reader.read_bit_double();
    let scale_factor = reader.read_bit_double();
    let align_angle = reader.read_bit_double();
    let is_solid = reader.read_bit();
    let sweep_alignment_flags = reader.read_bit_short();
    let path_flags = reader.read_bit_short();
    let align_start = reader.read_bit();
    let bank = reader.read_bit();
    let base_point_set = reader.read_bit();
    let sweep_entity_transform_computed = reader.read_bit();
    let path_entity_transform_computed = reader.read_bit();
    let reference_vector = reader.read_3bit_double();
    let sweep_entity_transform = read_surface_matrix(reader);
    let path_entity_transform = read_surface_matrix(reader);
    SurfaceSweepOptions {
        draft_angle,
        draft_start_distance,
        draft_end_distance,
        twist_angle,
        scale_factor,
        align_angle,
        sweep_entity_transform,
        path_entity_transform,
        is_solid,
        sweep_alignment_flags,
        path_flags,
        align_start,
        bank,
        base_point_set,
        sweep_entity_transform_computed,
        path_entity_transform_computed,
        reference_vector,
    }
}

fn read_surface_embedded_entity(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<crate::entities::EmbeddedEntity> {
    let type_code = reader.read_bit_long();
    let bit_length = safe_count(reader.read_bit_long()) as usize;
    crate::io::dwg::embedded_entity::read_embedded_entity_bits(
        reader,
        type_code,
        bit_length,
        version,
        dxf_version,
    )
}

pub fn read_surface(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    has_ds_data: bool,
    kind: SurfaceKind,
) -> SurfaceEntityData {
    let acis = read_acis_entity_impl(
        reader,
        version,
        dxf_version,
        has_ds_data,
        true,
        false,
        None,
    )
        .expect("database surface decoding retains unrecognized legacy payloads");
    // Surface records do not have the 3DSOLID history-id handle slot.
    let history_handle = 0;
    let mut modeler_format_version = 1;
    if matches!(
        kind,
        SurfaceKind::Lofted | SurfaceKind::Revolved | SurfaceKind::Swept
    ) && version.r2007_plus()
    {
        modeler_format_version = reader.read_bit_short();
    }
    let u_isolines = reader.read_bit_short();
    let v_isolines = reader.read_bit_short();
    let surface_data = match kind {
        SurfaceKind::Generic => SurfaceData::Generic,
        SurfaceKind::Plane => SurfaceData::Plane { class_version: 0 },
        SurfaceKind::Extruded => {
            let options = read_surface_sweep_options(reader);
            let sweep_vector = reader.read_3bit_double();
            let sweep_transform = read_surface_matrix(reader);
            let type_code = reader.read_bit_long();
            let bit_length = safe_count(reader.read_bit_long()) as usize;
            let sweep_entity = crate::io::dwg::embedded_entity::read_embedded_entity_bits(
                reader,
                type_code,
                bit_length,
                version,
                dxf_version,
            );
            SurfaceData::Extruded {
                sweep_entity,
                options,
                sweep_vector,
                sweep_transform,
            }
        }
        SurfaceKind::Lofted => {
            let loft_transform = read_surface_matrix(reader);
            let mut cross_section_entities = Vec::new();
            let mut guide_entities = Vec::new();
            let mut path_entity = None;
            let mut cross_sections = Vec::new();
            let mut guide_curves = Vec::new();
            let mut path_curve = None;
            let (
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
            ) = if version.r2007_plus() {
                let values = (
                    reader.read_bit_long(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                );
                let cross_count = safe_count(reader.read_bit_short() as i32);
                let guide_count = safe_count(reader.read_bit_short() as i32);
                cross_sections.reserve(cross_count as usize);
                guide_curves.reserve(guide_count as usize);
                for _ in 0..cross_count {
                    let handle = reader.read_handle();
                    if handle != 0 {
                        cross_sections.push(Handle::new(handle));
                    }
                }
                for _ in 0..guide_count {
                    let handle = reader.read_handle();
                    if handle != 0 {
                        guide_curves.push(Handle::new(handle));
                    }
                }
                let handle = reader.read_handle();
                if handle != 0 {
                    path_curve = Some(Handle::new(handle));
                }
                values
            } else {
                let cross_count = safe_count(reader.read_bit_short() as i32);
                let guide_count = safe_count(reader.read_bit_short() as i32);
                let has_path = reader.read_bit();
                let values = (
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit_long(),
                );
                cross_section_entities.reserve(cross_count as usize);
                guide_entities.reserve(guide_count as usize);
                for _ in 0..cross_count {
                    if let Some(entity) = read_surface_embedded_entity(reader, version, dxf_version)
                    {
                        cross_section_entities.push(entity);
                    }
                }
                for _ in 0..guide_count {
                    if let Some(entity) = read_surface_embedded_entity(reader, version, dxf_version)
                    {
                        guide_entities.push(entity);
                    }
                }
                if has_path {
                    path_entity = read_surface_embedded_entity(reader, version, dxf_version);
                }
                (
                    values.12, values.0, values.1, values.2, values.3, values.4, values.5,
                    values.6, values.7, values.8, values.9, values.10, values.11,
                )
            };
            SurfaceData::Lofted {
                loft_transform,
                cross_section_entities,
                guide_entities,
                path_entity,
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
            }
        }
        SurfaceKind::Revolved => {
            let (
                class_version,
                entity_id,
                draft_angle,
                draft_start_distance,
                draft_end_distance,
                twist_angle,
                solid,
                close_to_axis,
            ) = if version.r2007_plus() {
                (
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    false,
                    false,
                )
            } else {
                (
                    0,
                    0,
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit(),
                    reader.read_bit(),
                )
            };
            let axis_point = reader.read_3bit_double();
            let axis_vector = reader.read_3bit_double();
            let revolve_angle = reader.read_bit_double();
            let start_angle = reader.read_bit_double();
            let entity_transform = read_surface_matrix(reader);
            let (
                draft_angle,
                draft_start_distance,
                draft_end_distance,
                twist_angle,
                solid,
                close_to_axis,
            ) = if version.r2007_plus() {
                (
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit(),
                    reader.read_bit(),
                )
            } else {
                (
                    draft_angle,
                    draft_start_distance,
                    draft_end_distance,
                    twist_angle,
                    solid,
                    close_to_axis,
                )
            };
            let revolve_entity = if version.r2007_pre() {
                read_surface_embedded_entity(reader, version, dxf_version)
            } else {
                None
            };
            SurfaceData::Revolved {
                revolve_entity,
                class_version,
                entity_id,
                axis_point,
                axis_vector,
                revolve_angle,
                start_angle,
                entity_transform,
                draft_angle,
                draft_start_distance,
                draft_end_distance,
                twist_angle,
                solid,
                close_to_axis,
            }
        }
        SurfaceKind::Swept => {
            let class_version = if version.r2007_plus() {
                reader.read_bit_long()
            } else {
                0
            };
            let (sweep_transform, path_transform, early_options) = if version.r2007_pre() {
                let options = read_surface_sweep_options(reader);
                let sweep_transform = read_surface_matrix(reader);
                let path_transform = read_surface_matrix(reader);
                (sweep_transform, path_transform, Some(options))
            } else {
                (
                    crate::entities::surface::identity_matrix(),
                    crate::entities::surface::identity_matrix(),
                    None,
                )
            };
            let sweep_entity_id = reader.read_bit_long();
            let sweep_size = safe_count(reader.read_bit_long()) as usize;
            let sweep_entity = crate::io::dwg::embedded_entity::read_embedded_entity_bits(
                reader,
                sweep_entity_id,
                sweep_size,
                version,
                dxf_version,
            );
            let path_entity_id = reader.read_bit_long();
            let path_size = safe_count(reader.read_bit_long()) as usize;
            let path_entity = crate::io::dwg::embedded_entity::read_embedded_entity_bits(
                reader,
                path_entity_id,
                path_size,
                version,
                dxf_version,
            );
            let options = early_options.unwrap_or_else(|| read_surface_sweep_options(reader));
            SurfaceData::Swept {
                class_version,
                sweep_entity,
                path_entity,
                sweep_transform,
                path_transform,
                options,
            }
        }
        SurfaceKind::Nurb => {
            if version.r2013_plus(dxf_version) {
                SurfaceData::Nurb {
                    short_170: reader.read_bit_short(),
                    cv_hull_display: reader.read_bit(),
                    u_vector1: reader.read_3bit_double(),
                    v_vector1: reader.read_3bit_double(),
                    u_vector2: reader.read_3bit_double(),
                    v_vector2: reader.read_3bit_double(),
                }
            } else {
                SurfaceData::Nurb {
                    short_170: 0,
                    cv_hull_display: false,
                    u_vector1: Vector3::ZERO,
                    v_vector1: Vector3::ZERO,
                    u_vector2: Vector3::ZERO,
                    v_vector2: Vector3::ZERO,
                }
            }
        }
    };
    SurfaceEntityData {
        acis,
        modeler_format_version,
        u_isolines,
        v_isolines,
        surface_data,
        history_handle,
    }
}

/// Read a single wire struct from the DWG stream.
/// Field order/types per LibreDWG `Dwg_3DSOLID_wire`:
/// RC type, BLd selection_marker, BS/BL color, BLd acis_index, BL num_points,
/// 3BD points…, B transform_present [+ axes/translation/scale/flags].
fn read_wire(reader: &mut DwgMergedReader, version: DwgVersion) -> Wire {
    let wire_type_raw = reader.read_byte();
    let selection_marker = reader.read_bit_long();
    let color_val = if version.r2004_plus() {
        reader.read_bit_long()
    } else {
        reader.read_bit_short() as i32
    };
    let acis_index = reader.read_bit_long();
    let num_pts = safe_count(reader.read_bit_long());
    let mut pts = Vec::with_capacity(num_pts as usize);
    for _ in 0..num_pts {
        pts.push(reader.read_3bit_double());
    }
    let has_transform = reader.read_bit();
    let (mut x_axis, mut y_axis, mut z_axis) = (Vector3::UNIT_X, Vector3::UNIT_Y, Vector3::UNIT_Z);
    let mut translation = Vector3::ZERO;
    let mut scale = Vector3::new(1.0, 1.0, 1.0);
    let (mut has_rotation, mut has_reflection, mut has_shear) = (false, false, false);
    if has_transform {
        x_axis = reader.read_3bit_double();
        y_axis = reader.read_3bit_double();
        z_axis = reader.read_3bit_double();
        translation = reader.read_3bit_double();
        scale = reader.read_3bit_double();
        has_rotation = reader.read_bit();
        has_reflection = reader.read_bit();
        has_shear = reader.read_bit();
    }
    let color = if color_val == 256 {
        Color::ByLayer
    } else if color_val == 0 {
        Color::ByBlock
    } else {
        Color::Index(color_val as u8)
    };
    Wire {
        acis_index,
        wire_type: WireType::from(wire_type_raw),
        selection_marker,
        color,
        points: pts,
        has_transform,
        has_rotation,
        has_reflection,
        has_shear,
        scale,
        translation,
        x_axis,
        y_axis,
        z_axis,
    }
}

// ════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_point_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_3bit_double(Vector3::new(1.0, 2.0, 3.0));
            w.write_bit_thickness(0.5);
            w.write_bit_extrusion(Vector3::UNIT_Z);
            w.write_bit_double(45.0);
        });
        let pt = read_point(&mut r);
        assert_eq!(pt.location, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(pt.thickness, 0.5);
        assert_eq!(pt.x_axis_angle, 45.0);
    }

    #[test]
    fn test_line_roundtrip_r2000() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit(false); // z_are_zero = false
            w.write_raw_double(1.0); // start.x
            w.write_bit_double_with_default(4.0, 1.0); // end.x
            w.write_raw_double(2.0); // start.y
            w.write_bit_double_with_default(5.0, 2.0); // end.y
            w.write_raw_double(3.0); // start.z
            w.write_bit_double_with_default(6.0, 3.0); // end.z
            w.write_bit_thickness(0.0);
            w.write_bit_extrusion(Vector3::UNIT_Z);
        });
        let ln = read_line(&mut r, v);
        assert_eq!(ln.start, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(ln.end, Vector3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn test_circle_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_3bit_double(Vector3::new(10.0, 20.0, 0.0));
            w.write_bit_double(5.0);
            w.write_bit_thickness(0.0);
            w.write_bit_extrusion(Vector3::UNIT_Z);
        });
        let c = read_circle(&mut r);
        assert_eq!(c.center, Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(c.radius, 5.0);
    }

    #[test]
    fn test_ellipse_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_3bit_double(Vector3::new(5.0, 5.0, 0.0));
            w.write_3bit_double(Vector3::new(10.0, 0.0, 0.0));
            w.write_3bit_double(Vector3::UNIT_Z);
            w.write_bit_double(0.5);
            w.write_bit_double(0.0);
            w.write_bit_double(std::f64::consts::TAU);
        });
        let e = read_ellipse(&mut r);
        assert_eq!(e.center, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(e.major_axis, Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(e.minor_axis_ratio, 0.5);
    }

    #[test]
    fn test_insert_roundtrip_r2000() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_3bit_double(Vector3::new(100.0, 200.0, 0.0));
            w.write_2bits(3); // all-ones scale
            w.write_bit_double(0.0); // rotation
            w.write_3bit_double(Vector3::UNIT_Z); // normal
            w.write_bit(false); // has_attribs
            w.write_handle(
                crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer,
                0x50,
            );
        });
        let ins = read_insert(&mut r, v);
        assert_eq!(ins.insert_point, Vector3::new(100.0, 200.0, 0.0));
        assert_eq!(ins.x_scale, 1.0);
        assert_eq!(ins.y_scale, 1.0);
        assert_eq!(ins.z_scale, 1.0);
        assert_eq!(ins.block_handle, 0x50);
    }

    #[test]
    fn test_spline_roundtrip_scenario1() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;
        let mut r = make_reader(v, d, |w| {
            w.write_bit_long(1); // scenario
            w.write_bit_long(3); // degree
            w.write_bit(false); // rational
            w.write_bit(false); // closed
            w.write_bit(false); // periodic
            w.write_bit_double(1e-10); // knot_tol
            w.write_bit_double(1e-10); // ctrl_tol
            w.write_bit_long(6); // num_knots
            w.write_bit_long(3); // num_ctrl
            w.write_bit(false); // has_weights
            for k in &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
                w.write_bit_double(*k);
            }
            w.write_3bit_double(Vector3::new(0.0, 0.0, 0.0));
            w.write_3bit_double(Vector3::new(5.0, 5.0, 0.0));
            w.write_3bit_double(Vector3::new(10.0, 0.0, 0.0));
        });
        let sp = read_spline(&mut r, v, d);
        assert_eq!(sp.scenario, 1);
        assert_eq!(sp.degree, 3);
        assert_eq!(sp.knots.len(), 6);
        assert_eq!(sp.control_points.len(), 3);
    }

    #[test]
    fn test_acis_sat_roundtrip_r2004() {
        use crate::entities::solid3d::AcisData;

        let sat = "700 0 1 0\n\
                   @7 unknown 12 ACIS 7.0 NT 24 Wed Jan 01 00:00:00 2025 1.0 9.9999999999999995e-007 1e-010\n\
                   body $-1 $1 $-1 $-1 #\n\
                   lump $-1 $-1 $2 $0 #\n\
                   shell $-1 $-1 $-1 $3 $-1 $1 #\n\
                   face $-1 $-1 $-1 $4 $2 $5 forward single #\n\
                   loop $-1 $-1 $6 $3 #\n\
                   plane-surface $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
                   coedge $-1 $6 $6 $-1 $7 forward $4 $-1 #\n\
                   edge $-1 $8 0 $8 1 $6 $9 forward #\n\
                   vertex $-1 $7 $10 #\n\
                   straight-curve $-1 -5 -5 5 1 0 0 I I #\n\
                   point $-1 -5 -5 5 #\n\
                   End-of-ACIS-data\n";

        // Write using the writer infrastructure
        let v = DwgVersion::AC18;
        let d = DxfVersion::AC1018;
        let acis = AcisData::from_sat(sat);
        let mut w = DwgMergedWriter::new(v, d);
        // Write: acis_empty + unknown + version + encrypted blocks + wireframe + acis_empty_bit
        w.write_bit(false); // acis_empty = false (has data)
        w.write_bit(false); // unknown bit (per ODA/LibreDWG spec)
        w.write_bit_short(1); // acis_version = 1 (SAT text)
                              // Encrypt SAT with selective 159-substitution cipher
        let mut full_sat = acis.sat_data.clone();
        full_sat.push_str("End-of-ACIS-data\n");
        let plain = full_sat.as_bytes();
        let mut encrypted = Vec::with_capacity(plain.len());
        for &b in plain.iter() {
            if b <= 32 {
                encrypted.push(b);
            } else {
                encrypted.push(159u8.wrapping_sub(b));
            }
        }
        w.write_bit_long(encrypted.len() as i32);
        w.write_bytes(&encrypted);
        w.write_bit_long(0); // terminating empty block
        w.write_bit(false); // wireframe_present = false
        w.write_bit(false); // acis_empty_bit

        let data = w.merge();
        let hsb = w.handle_start_bits();
        let mut r = DwgMergedReader::new(data, d, hsb);

        let result = read_acis_entity(&mut r, v, d, false);
        assert!(!result.acis_empty);
        assert!(!result.is_binary);
        assert_eq!(result.version, 1);
        assert!(result.sat_data.contains("body"));
        assert!(result.sat_data.contains("plane-surface"));
        assert!(result.wires.is_empty());
    }

    #[test]
    fn test_acis_sab_roundtrip_r2007() {
        // Test SAB binary roundtrip (version 2)
        // The reader calculates SAB size from bit positions, not a BL prefix.
        let v = DwgVersion::AC21;
        let d = DxfVersion::AC1021;

        let sab_data: Vec<u8> = vec![
            0x41, 0x53, 0x4D, 0x20, // "ASM "
            0x42, 0x69, 0x6E, 0x00, // "Bin\0"
            0x01, 0x02, 0x03, 0x04, // some dummy data
        ];

        let mut w = DwgMergedWriter::new(v, d);
        w.write_bit(false); // acis_empty = false
        w.write_bit(false); // unknown bit (per ODA/LibreDWG spec)
        w.write_bit_short(2); // acis_version = 2 (SAB binary)
                              // NO BL size prefix — reader infers size from remaining bits
        w.write_bytes(&sab_data);
        w.write_bit(false); // wireframe_present = false
        w.write_bit(false); // acis_empty_bit

        let data = w.merge();
        let hsb = w.handle_start_bits();
        let mut r = DwgMergedReader::new(data, d, hsb);
        r.set_handle_start(hsb); // required for SAB size calculation

        let result = read_acis_entity(&mut r, v, d, false);
        assert!(!result.acis_empty);
        assert!(result.is_binary);
        assert_eq!(result.version, 2);
        assert_eq!(result.sab_data, sab_data);
        assert!(result.sat_data.is_empty());
    }

    #[test]
    fn test_acis_empty_roundtrip() {
        let v = DwgVersion::AC15;
        let d = DxfVersion::AC1015;

        let mut w = DwgMergedWriter::new(v, d);
        w.write_bit(true); // acis_empty = true
        w.write_bit(false); // wireframe_present = false
        w.write_bit(false); // acis_empty_bit

        let data = w.merge();
        let hsb = w.handle_start_bits();
        let mut r = DwgMergedReader::new(data, d, hsb);

        let result = read_acis_entity(&mut r, v, d, false);
        assert!(result.acis_empty);
        assert!(result.sat_data.is_empty());
        assert!(result.sab_data.is_empty());
    }
}
