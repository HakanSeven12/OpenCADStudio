//! Codec for type-specific entity bodies embedded in 3D construction data.

use crate::entities::{
    AcisVersion, Arc, Circle, Ellipse, EmbeddedEntity, Line, LwPolyline, LwVertex, Point, Ray,
    Region, Spline, XLine,
};
use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_stream_readers::object_reader::{common, entities};
use crate::io::dwg::dwg_stream_writers::{DwgBitWriter, DwgMergedWriter};
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::{DxfVersion, Vector2, Vector3};

/// Encoded embedded entity body, including exact meaningful bit length.
pub(crate) struct EncodedEmbeddedEntity {
    pub type_code: i32,
    pub bit_length: usize,
    pub bytes: Vec<u8>,
}

/// Compare meaningful body bits without interpreting unused final-byte bits.
/// Byte-sized records may include up to seven zero padding bits, but an
/// otherwise different native layout must retain its original opaque body.
fn preserves_embedded_body(
    encoded: &EncodedEmbeddedEntity,
    type_code: i32,
    bit_length: usize,
    bytes: &[u8],
) -> bool {
    if encoded.type_code != type_code
        || encoded.bit_length > bit_length
        || bit_length - encoded.bit_length > 7
        || (encoded.bit_length != bit_length && bit_length % 8 != 0)
        || bytes.len() < bit_length.div_ceil(8)
        || encoded.bytes.len() < encoded.bit_length.div_ceil(8)
    {
        return false;
    }
    let whole_bytes = encoded.bit_length / 8;
    if encoded.bytes[..whole_bytes] != bytes[..whole_bytes] {
        return false;
    }
    let remainder = encoded.bit_length % 8;
    if remainder != 0 {
        let mask = u8::MAX << (8 - remainder);
        if encoded.bytes[whole_bytes] & mask != bytes[whole_bytes] & mask {
            return false;
        }
    }
    (encoded.bit_length..bit_length).all(|bit| bytes[bit / 8] & (1 << (7 - bit % 8)) == 0)
}

/// Read one embedded entity body directly from an enclosing DWG bitstream.
pub(crate) fn read_embedded_entity(
    reader: &mut DwgMergedReader,
    type_code: i32,
    byte_length: usize,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<EmbeddedEntity> {
    let bytes = reader.read_bytes(byte_length);
    decode_embedded_entity(type_code, byte_length * 8, bytes, version, dxf_version)
}

/// Read an embedded entity whose size prefix is a meaningful bit count.
pub(crate) fn read_embedded_entity_bits(
    reader: &mut DwgMergedReader,
    type_code: i32,
    bit_length: usize,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<EmbeddedEntity> {
    let mut bytes = vec![0u8; bit_length.div_ceil(8)];
    for bit_index in 0..bit_length {
        if reader.read_bit() {
            bytes[bit_index / 8] |= 1 << (7 - bit_index % 8);
        }
    }
    decode_embedded_entity(type_code, bit_length, bytes, version, dxf_version)
}

/// Decode an embedded entity body obtained from a DXF binary group.
pub(crate) fn decode_embedded_entity(
    type_code: i32,
    bit_length: usize,
    mut bytes: Vec<u8>,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<EmbeddedEntity> {
    if type_code == 0 || bit_length == 0 {
        return None;
    }
    bytes.truncate(bit_length.div_ceil(8));
    let preserved_bytes = bytes.clone();
    let bit_reader = DwgBitReader::new(bytes, version, dxf_version);
    let mut reader = DwgMergedReader::from_readers(bit_reader, None, None, dxf_version);
    match type_code as i16 {
        common::OBJ_REGION => {
            let preserve_unknown = || EmbeddedEntity::Unknown {
                type_code,
                bit_count: bit_length,
                bytes: preserved_bytes.clone(),
            };
            if DwgVersion::from_dxf_version(dxf_version).ok() != Some(version) {
                return Some(preserve_unknown());
            }
            let Some(data) =
                entities::read_inline_acis_entity(&mut reader, version, dxf_version, bit_length)
            else {
                return Some(preserve_unknown());
            };
            let mut entity = Region::new();
            entity.point_of_reference = data.point;
            entity.wires = data.wires;
            entity.silhouettes = data.silhouettes;
            entity.acis_data.sat_data = data.sat_data;
            entity.acis_data.sab_data = data.sab_data;
            entity.acis_data.is_binary = data.is_binary;
            entity.acis_data.version = if data.version == 2 {
                AcisVersion::Version2
            } else {
                AcisVersion::Version1
            };
            entity.acis_data.revision = data.revision;
            entity.acis_data.materials = data.materials;
            entity.acis_data.wireframe_data_present = data.wireframe_data_present;
            entity.acis_data.wireframe_point_present = data.wireframe_point_present;
            entity.acis_data.wireframe_isoline_present = data.wireframe_isoline_present;
            entity.acis_data.acis_empty_bit = data.acis_empty_bit;
            entity.acis_data.extra_acis_data = data.extra_acis_data.map(Box::new);
            entity.acis_data.wireframe_isolines = data.isolines;
            let entity = EmbeddedEntity::Region(entity);
            let encoded = encode_embedded_entity(&entity, version, dxf_version);
            if preserves_embedded_body(&encoded, type_code, bit_length, &preserved_bytes) {
                Some(entity)
            } else {
                Some(preserve_unknown())
            }
        }
        common::OBJ_POINT => {
            let data = entities::read_point(&mut reader);
            let mut entity = Point::new();
            entity.location = data.location;
            entity.thickness = data.thickness;
            entity.normal = data.normal;
            entity.x_axis_angle = data.x_axis_angle;
            Some(EmbeddedEntity::Point(entity))
        }
        common::OBJ_LINE => {
            let data = entities::read_line(&mut reader, DwgVersion::AC12);
            let mut entity = Line::new();
            entity.start = data.start;
            entity.end = data.end;
            entity.thickness = data.thickness;
            entity.normal = data.normal;
            Some(EmbeddedEntity::Line(entity))
        }
        common::OBJ_ARC => {
            let data = entities::read_arc(&mut reader);
            let mut entity = Arc::new();
            entity.center = data.center;
            entity.radius = data.radius;
            entity.thickness = data.thickness;
            entity.normal = data.normal;
            entity.start_angle = data.start_angle;
            entity.end_angle = data.end_angle;
            Some(EmbeddedEntity::Arc(entity))
        }
        common::OBJ_CIRCLE => {
            let data = entities::read_circle(&mut reader);
            let mut entity = Circle::new();
            entity.center = data.center;
            entity.radius = data.radius;
            entity.thickness = data.thickness;
            entity.normal = data.normal;
            Some(EmbeddedEntity::Circle(entity))
        }
        common::OBJ_ELLIPSE => {
            let data = entities::read_ellipse(&mut reader);
            let mut entity = Ellipse::new();
            entity.center = data.center;
            entity.major_axis = data.major_axis;
            entity.normal = data.normal;
            entity.minor_axis_ratio = data.minor_axis_ratio;
            entity.start_parameter = data.start_parameter;
            entity.end_parameter = data.end_parameter;
            Some(EmbeddedEntity::Ellipse(entity))
        }
        common::OBJ_SPLINE => {
            let data = entities::read_spline(&mut reader, version, dxf_version);
            let mut entity = Spline::new();
            entity.degree = data.degree;
            entity.flags.rational = data.rational;
            entity.flags.closed = data.closed;
            entity.flags.periodic = data.periodic;
            entity.knots = data.knots;
            entity.control_points = data.control_points;
            entity.weights = data.weights;
            entity.fit_points = data.fit_points;
            entity.knot_tolerance = data.knot_tolerance;
            entity.control_tolerance = data.control_tolerance;
            entity.fit_tolerance = data.fit_tolerance;
            entity.begin_tangent = data.begin_tangent;
            entity.end_tangent = data.end_tangent;
            entity.knot_parameterization = data.knot_param;
            entity.cv_frame_visible = data.flags1 & 2 != 0;
            entity.dwg_flags1 = data.flags1;
            Some(EmbeddedEntity::Spline(entity))
        }
        common::OBJ_LWPOLYLINE => {
            let data = entities::read_embedded_lwpolyline(&mut reader, version);
            let mut entity = LwPolyline::new();
            entity.vertices = data
                .vertices
                .into_iter()
                .map(|vertex| LwVertex {
                    location: Vector2::new(vertex.x, vertex.y),
                    bulge: vertex.bulge,
                    start_width: vertex.start_width,
                    end_width: vertex.end_width,
                    vertex_id: vertex.vertex_id,
                })
                .collect();
            entity.constant_width = data.constant_width;
            entity.elevation = data.elevation;
            entity.thickness = data.thickness;
            entity.normal = data.normal;
            entity.is_closed = data.flag & 0x200 != 0;
            entity.plinegen = data.flag & 0x100 != 0;
            Some(EmbeddedEntity::LwPolyline(entity))
        }
        common::OBJ_RAY => {
            let data = entities::read_ray(&mut reader);
            Some(EmbeddedEntity::Ray(Ray::new(
                data.base_point,
                data.direction,
            )))
        }
        common::OBJ_XLINE => {
            let data = entities::read_xline(&mut reader);
            Some(EmbeddedEntity::XLine(XLine::new(
                data.base_point,
                data.direction,
            )))
        }
        _ => Some(EmbeddedEntity::Unknown {
            type_code,
            bit_count: bit_length,
            bytes: preserved_bytes,
        }),
    }
}

/// Encode one typed embedded entity to its type-specific DWG body.
pub(crate) fn encode_embedded_entity(
    entity: &EmbeddedEntity,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> EncodedEmbeddedEntity {
    let mut writer = DwgBitWriter::new(version, dxf_version);
    let type_code = match entity {
        EmbeddedEntity::Region(entity) => {
            let document = crate::document::CadDocument::with_version(dxf_version);
            let (bit_length, bytes) =
                crate::io::dwg::dwg_stream_writers::object_writer::DwgObjectWriter::embedded_region_body(
                    &document, entity,
                );
            return EncodedEmbeddedEntity {
                type_code: common::OBJ_REGION as i32,
                bit_length,
                bytes,
            };
        }
        EmbeddedEntity::Point(entity) => {
            writer.write_3bit_double(entity.location);
            writer.write_bit_thickness(entity.thickness);
            writer.write_bit_extrusion(entity.normal);
            writer.write_bit_double(entity.x_axis_angle);
            common::OBJ_POINT
        }
        EmbeddedEntity::Line(entity) => {
            writer.write_3bit_double(entity.start);
            writer.write_3bit_double(entity.end);
            writer.write_bit_thickness(entity.thickness);
            writer.write_bit_extrusion(entity.normal);
            common::OBJ_LINE
        }
        EmbeddedEntity::Circle(entity) => {
            writer.write_3bit_double(entity.center);
            writer.write_bit_double(entity.radius);
            writer.write_bit_thickness(entity.thickness);
            writer.write_bit_extrusion(entity.normal);
            common::OBJ_CIRCLE
        }
        EmbeddedEntity::Arc(entity) => {
            writer.write_3bit_double(entity.center);
            writer.write_bit_double(entity.radius);
            writer.write_bit_thickness(entity.thickness);
            writer.write_bit_extrusion(entity.normal);
            writer.write_bit_double(entity.start_angle);
            writer.write_bit_double(entity.end_angle);
            common::OBJ_ARC
        }
        EmbeddedEntity::Ellipse(entity) => {
            writer.write_3bit_double(entity.center);
            writer.write_3bit_double(entity.major_axis);
            writer.write_3bit_double(entity.normal);
            writer.write_bit_double(entity.minor_axis_ratio);
            writer.write_bit_double(entity.start_parameter);
            writer.write_bit_double(entity.end_parameter);
            common::OBJ_ELLIPSE
        }
        EmbeddedEntity::Spline(entity) => {
            write_spline(&mut writer, entity, version, dxf_version);
            common::OBJ_SPLINE
        }
        EmbeddedEntity::LwPolyline(entity) => {
            write_lwpolyline(&mut writer, entity, version);
            common::OBJ_LWPOLYLINE
        }
        EmbeddedEntity::Ray(entity) => {
            writer.write_3bit_double(entity.base_point);
            writer.write_3bit_double(entity.direction);
            common::OBJ_RAY
        }
        EmbeddedEntity::XLine(entity) => {
            writer.write_3bit_double(entity.base_point);
            writer.write_3bit_double(entity.direction);
            common::OBJ_XLINE
        }
        EmbeddedEntity::Unknown {
            type_code,
            bit_count,
            bytes,
        } => {
            return EncodedEmbeddedEntity {
                type_code: *type_code,
                bit_length: *bit_count,
                bytes: bytes.clone(),
            };
        }
    };
    let bit_length = writer.position_in_bits() as usize;
    EncodedEmbeddedEntity {
        type_code: type_code as i32,
        bit_length,
        bytes: writer.into_bytes(),
    }
}

/// Append a byte-sized embedded body to the enclosing bitstream.
pub(crate) fn write_embedded_bytes(writer: &mut DwgMergedWriter, encoded: &EncodedEmbeddedEntity) {
    writer.write_bytes(&encoded.bytes);
}

pub(crate) fn write_embedded_bits_with_length(
    writer: &mut DwgMergedWriter,
    encoded: &EncodedEmbeddedEntity,
    bit_length: usize,
) {
    for bit_index in 0..bit_length {
        let byte = encoded.bytes.get(bit_index / 8).copied().unwrap_or(0);
        writer.write_bit(byte & (1 << (7 - bit_index % 8)) != 0);
    }
}

fn write_lwpolyline(writer: &mut DwgBitWriter, entity: &LwPolyline, version: DwgVersion) {
    let has_widths = entity
        .vertices
        .iter()
        .any(|vertex| vertex.start_width != 0.0 || vertex.end_width != 0.0);
    let has_bulges = entity.vertices.iter().any(|vertex| vertex.bulge != 0.0);
    let has_vertex_ids =
        version.r2010_plus() && entity.vertices.iter().any(|vertex| vertex.vertex_id != 0);
    let mut flags = 0i16;
    if entity.normal != Vector3::UNIT_Z {
        flags |= 0x1;
    }
    if entity.thickness != 0.0 {
        flags |= 0x2;
    }
    if entity.constant_width != 0.0 {
        flags |= 0x4;
    }
    if entity.elevation != 0.0 {
        flags |= 0x8;
    }
    if has_bulges {
        flags |= 0x10;
    }
    if has_widths {
        flags |= 0x20;
    }
    if entity.plinegen {
        flags |= 0x100;
    }
    if entity.is_closed {
        flags |= 0x200;
    }
    if has_vertex_ids {
        flags |= 0x400;
    }
    writer.write_bit_short(flags);
    if entity.constant_width != 0.0 {
        writer.write_bit_double(entity.constant_width);
    }
    if entity.elevation != 0.0 {
        writer.write_bit_double(entity.elevation);
    }
    if entity.thickness != 0.0 {
        writer.write_bit_double(entity.thickness);
    }
    if entity.normal != Vector3::UNIT_Z {
        writer.write_3bit_double(entity.normal);
    }
    let count = entity.vertices.len() as i32;
    writer.write_bit_long(count);
    if has_bulges {
        writer.write_bit_long(count);
    }
    if has_vertex_ids {
        writer.write_bit_long(count);
    }
    if has_widths {
        writer.write_bit_long(count);
    }
    for vertex in &entity.vertices {
        writer.write_raw_double(vertex.location.x);
        writer.write_raw_double(vertex.location.y);
    }
    if has_bulges {
        for vertex in &entity.vertices {
            writer.write_bit_double(vertex.bulge);
        }
    }
    if has_vertex_ids {
        for vertex in &entity.vertices {
            writer.write_bit_long(vertex.vertex_id);
        }
    }
    if has_widths {
        for vertex in &entity.vertices {
            writer.write_bit_double(vertex.start_width);
            writer.write_bit_double(vertex.end_width);
        }
    }
}

fn write_spline(
    writer: &mut DwgBitWriter,
    entity: &Spline,
    version: DwgVersion,
    dxf_version: DxfVersion,
) {
    let r2013_plus = version.r2013_plus(dxf_version);
    let scenario =
        if !entity.fit_points.is_empty() && (!r2013_plus || entity.knot_parameterization != 15) {
            2
        } else {
            1
        };
    if r2013_plus {
        let mut flags = entity.dwg_flags1;
        if entity.cv_frame_visible {
            flags |= 2;
        } else {
            flags &= !2;
        }
        if entity.flags.closed {
            flags |= 4;
        } else {
            flags &= !4;
        }
        if scenario == 2 {
            flags |= 1 | 8;
        } else {
            flags &= !8;
        }
        writer.write_bit_long(scenario);
        writer.write_bit_long(flags);
        writer.write_bit_long(entity.knot_parameterization);
    } else {
        writer.write_bit_long(scenario);
    }
    writer.write_bit_long(entity.degree);
    if scenario == 1 {
        writer.write_bit(entity.flags.rational);
        writer.write_bit(entity.flags.closed);
        writer.write_bit(entity.flags.periodic);
        writer.write_bit_double(entity.knot_tolerance);
        writer.write_bit_double(entity.control_tolerance);
        let knots = if entity.knots.is_empty() && !entity.control_points.is_empty() {
            Spline::generate_clamped_knots(entity.degree as usize, entity.control_points.len())
        } else {
            entity.knots.clone()
        };
        writer.write_bit_long(knots.len() as i32);
        writer.write_bit_long(entity.control_points.len() as i32);
        let has_weights = !entity.weights.is_empty();
        writer.write_bit(has_weights);
        for knot in knots {
            writer.write_bit_double(knot);
        }
        for (index, point) in entity.control_points.iter().enumerate() {
            writer.write_3bit_double(*point);
            if has_weights {
                writer.write_bit_double(entity.weights.get(index).copied().unwrap_or(1.0));
            }
        }
    } else {
        writer.write_bit_double(entity.fit_tolerance);
        writer.write_3bit_double(entity.begin_tangent);
        writer.write_3bit_double(entity.end_tangent);
        writer.write_bit_long(entity.fit_points.len() as i32);
        for point in &entity.fit_points {
            writer.write_3bit_double(*point);
        }
    }
}
