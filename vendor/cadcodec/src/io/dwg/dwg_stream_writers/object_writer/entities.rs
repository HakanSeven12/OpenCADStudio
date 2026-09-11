//! Entity serialization for DWG object records.
//!
//! Each entity writer:
//! 1. Calls `write_common_entity_data()` (type code + preamble)
//! 2. Writes type-specific fields via the merged writer
//! 3. Calls `register_object()` (CRC, output, handle map)
//!
//! Ported from the reference `DwgObjectWriter.Entities.cs`.

use crate::entities::multileader::LeaderLineBreakInfo;
use crate::entities::raster_image::{ClipBoundary, ClipType};
use crate::entities::*;
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::types::{Color, Handle, LineWeight, Vector2, Vector3};

use super::common;
use super::DwgObjectWriter;

impl<'a> DwgObjectWriter<'a> {
    /// Encode only a REGION body for a construction-history profile. Common
    /// entity data, database handles and external AcDs records do not belong
    /// to an embedded entity, so use the shared inline modeler writer.
    pub(crate) fn embedded_region_body(
        document: &'a crate::document::CadDocument,
        entity: &Region,
    ) -> (usize, Vec<u8>) {
        let mut writer = Self::new(document)
            .expect("embedded entity version was validated by its enclosing writer");
        writer.write_acis_data_impl(
            entity.point_of_reference,
            &entity.acis_data,
            &entity.wires,
            &entity.silhouettes,
            true,
        );
        (
            writer.writer.main().position_in_bits() as usize,
            writer.writer.main().to_bytes_snapshot(),
        )
    }

    // â”€â”€ Entity dispatch â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Write a single entity record.
    pub(super) fn write_entity(&mut self, entity: &EntityType) {
        match entity {
            EntityType::Point(e) => self.write_point(e),
            EntityType::Line(e) => self.write_line(e),
            EntityType::Circle(e) => self.write_circle(e),
            EntityType::Arc(e) => self.write_arc(e),
            EntityType::Ellipse(e) => self.write_ellipse(e),
            EntityType::Text(e) => self.write_text(e),
            EntityType::MText(e) => self.write_mtext(e),
            EntityType::Solid(e) => self.write_solid(e),
            EntityType::Face3D(e) => self.write_face3d(e),
            EntityType::Insert(e) => self.write_insert(e),
            EntityType::LwPolyline(e) => self.write_lwpolyline(e),
            EntityType::Spline(e) => self.write_spline(e),
            EntityType::Helix(e) => self.write_helix(e),
            EntityType::Ray(e) => self.write_ray(e),
            EntityType::XLine(e) => self.write_xline(e),
            EntityType::Leader(e) => self.write_leader(e),
            EntityType::Tolerance(e) => self.write_tolerance(e),
            EntityType::Shape(e) => self.write_shape(e),
            EntityType::Hatch(e) => self.write_hatch(e),
            EntityType::Viewport(e) => self.write_viewport_entity(e),
            EntityType::Dimension(e) => self.write_dimension(e),
            EntityType::Polyline2D(e) => self.write_polyline2d(e),
            EntityType::Polyline3D(e) => self.write_polyline3d(e),
            EntityType::PolyfaceMesh(e) => self.write_polyface_mesh(e),
            EntityType::PolygonMesh(e) => self.write_polygon_mesh(e),
            EntityType::Seqend(e) => self.write_seqend(e),
            EntityType::Mesh(e) => self.write_mesh(e),
            EntityType::MLine(e) => self.write_mline(e),
            EntityType::RasterImage(e) => self.write_raster_image(e),
            EntityType::Wipeout(e) => self.write_wipeout(e),
            EntityType::Ole2Frame(e) => self.write_ole2frame(e),
            EntityType::MultiLeader(e) => self.write_multileader(e),
            EntityType::AttributeDefinition(e) => self.write_attribute_definition(e),
            EntityType::AttributeEntity(e) => self.write_attribute_entity(e),
            EntityType::Polyline(e) => self.write_polyline_old(e),
            // Skip types that are structural or unsupported in DWG
            EntityType::Block(_) | EntityType::BlockEnd(_) => {}
            EntityType::Solid3D(e) => self.write_solid3d(e),
            EntityType::Region(e) => self.write_region(e),
            EntityType::Body(e) => self.write_body(e),
            EntityType::Surface(e) => self.write_surface(e),
            EntityType::Underlay(e) => self.write_underlay(e),
            EntityType::Table(e) => self.write_table(e),
            EntityType::Light(e) => self.write_light(e),
            EntityType::SectionSymbol(e) => self.write_section_symbol(e),
            EntityType::ViewBorder(e) => self.write_view_border(e),
            EntityType::Extended(e) => {
                let raw = match &e.data {
                    ExtendedEntityData::Format(data) => data
                        .raw_dwg_data
                        .as_ref()
                        .map(|raw| (raw, data.raw_dwg_handle_bits, data.raw_dwg_version)),
                    ExtendedEntityData::LayoutPrintConfig(data) => data
                        .raw_dwg_data
                        .as_ref()
                        .map(|raw| (raw, data.raw_dwg_handle_bits, data.raw_dwg_version)),
                    _ => None,
                };
                if let Some((raw, handle_bits, version)) = raw {
                    if self.raw_passthrough_compatible(version) {
                        self.register_raw_object(e.common.handle, raw, handle_bits);
                        return;
                    }
                }
                self.write_extended_entity(e);
            }
            EntityType::Unknown(e) => {
                // Write raw DWG data verbatim only when the target matches the
                // source encoding family; otherwise drop rather than corrupt.
                if let Some(ref raw_data) = e.raw_dwg_data {
                    if self.raw_passthrough_compatible(e.dwg_source_version) {
                        self.register_raw_object(e.common.handle, raw_data, e.dwg_handle_bits);
                    }
                }
            }
        }
    }

    // â”€â”€ Helper: write entity preamble â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn entity_preamble(&mut self, type_code: i16, c: &EntityCommon) {
        self.write_common_entity_data(
            type_code,
            c.handle,
            c.owner_handle,
            &c.layer,
            &c.color,
            &c.line_weight,
            &c.transparency,
            c.invisible,
            c.linetype_scale,
            &c.linetype,
            &c.linetype_handle,
            &c.extended_data,
            &c.reactors,
            &c.xdictionary_handle,
            c.graphic_data.as_deref(),
            c.entity_mode,
            c.material_flags,
            &c.material_handle,
            c.shadow_flags,
            c.plotstyle_flags,
            &c.plotstyle_handle,
            &c.color_book_handle,
            &c.full_visual_style_handle,
            &c.face_visual_style_handle,
            &c.edge_visual_style_handle,
        );
    }

    fn write_extended_entity(&mut self, e: &ExtendedEntity) {
        let type_code = match &e.data {
            ExtendedEntityData::Camera { .. } => self.class_type_code("CAMERA", common::OBJ_CAMERA),
            ExtendedEntityData::SectionObject(_) => {
                self.class_type_code("SECTIONOBJECT", common::OBJ_SECTIONOBJECT)
            }
            ExtendedEntityData::ArcAlignedText(_) => {
                self.class_type_code("ARCALIGNEDTEXT", common::OBJ_ARCALIGNEDTEXT)
            }
            ExtendedEntityData::RemoteText(_) => self.class_type_code("RTEXT", common::OBJ_RTEXT),
            ExtendedEntityData::GeoPositionMarker(_) => {
                self.class_type_code("POSITIONMARKER", common::OBJ_GEOPOSITIONMARKER)
            }
            ExtendedEntityData::CoordinationModel(_) => {
                self.class_type_code("COORDINATION_MODEL", common::OBJ_NAVISWORKSMODEL)
            }
            ExtendedEntityData::PointCloud(_) => {
                self.class_type_code("ACDBPOINTCLOUD", common::OBJ_POINTCLOUD)
            }
            ExtendedEntityData::PointCloudEx(_) => {
                self.class_type_code("ACDBPOINTCLOUDEX", common::OBJ_POINTCLOUDEX)
            }
            ExtendedEntityData::Proxy(_) => common::OBJ_PROXY_ENTITY,
            ExtendedEntityData::OleFrame(_) => common::OBJ_OLEFRAME,
            ExtendedEntityData::LayoutPrintConfig(_) => {
                self.class_type_code("LAYOUTPRINTCONFIG", 0)
            }
            ExtendedEntityData::Format(_) => self.class_type_code("Format", 0),
            ExtendedEntityData::Legacy(_) => return,
            ExtendedEntityData::DynamicBlock(data) => {
                let Some(name) = data.entity_dxf_name() else {
                    return;
                };
                self.class_type_code(name, 0)
            }
            ExtendedEntityData::RegisteredClass(data) => {
                if data.properties.is_empty() {
                    self.class_type_code(&data.dxf_name, 0)
                } else {
                    common::OBJ_PROXY_ENTITY
                }
            }
        };

        if let ExtendedEntityData::Proxy(data) = &e.data {
            let mut proxy_common = e.common.clone();
            proxy_common.graphic_data = Some(data.graphics.data());
            self.entity_preamble(type_code, &proxy_common);
        } else {
            self.entity_preamble(type_code, &e.common);
        }

        match &e.data {
            ExtendedEntityData::Camera { view_handle } => {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, view_handle.value());
            }
            ExtendedEntityData::SectionObject(data) => {
                self.writer.write_bit_long(data.state);
                self.writer.write_bit_long(data.flags);
                self.writer.write_variable_text(&data.name);
                self.writer.write_3bit_double(data.vertical_direction);
                self.writer.write_bit_double(data.top_height);
                self.writer.write_bit_double(data.bottom_height);
                self.writer.write_bit_short(data.indicator_alpha);
                self.writer.write_cm_color(&data.indicator_color);
                self.writer.write_bit_long(data.vertices.len() as i32);
                for point in &data.vertices {
                    self.writer.write_3bit_double(*point);
                }
                self.writer
                    .write_bit_long(data.back_line_vertices.len() as i32);
                for point in &data.back_line_vertices {
                    self.writer.write_3bit_double(*point);
                }
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, data.settings_handle.value());
            }
            ExtendedEntityData::ArcAlignedText(data) => {
                self.writer.write_bit_double(data.text_size);
                self.writer.write_bit_double(data.x_scale);
                self.writer.write_bit_double(data.character_spacing);
                self.writer.write_variable_text(&data.style_name);
                self.writer.write_variable_text(&data.font_name);
                self.writer.write_variable_text(&data.big_font_name);
                self.writer.write_variable_text(&data.text);
                self.writer.write_bit_double(data.offset_from_arc);
                self.writer.write_bit_double(data.right_offset);
                self.writer.write_bit_double(data.left_offset);
                self.writer.write_3bit_double(data.center);
                self.writer.write_bit_double(data.radius);
                self.writer.write_bit_double(data.start_angle);
                self.writer.write_bit_double(data.end_angle);
                self.writer.write_3bit_double(data.normal);
                self.writer.write_bit_long(data.text_color);
                self.writer.write_bit_short(data.character_set);
                self.writer.write_bit_short(data.pitch_and_family);
                self.writer.write_bit_short(data.is_shx as i16);
                self.writer.write_bit_short(data.bold as i16);
                self.writer.write_bit_short(data.italic as i16);
                self.writer.write_bit_short(data.underlined as i16);
                self.writer.write_bit_short(data.alignment);
                self.writer.write_bit_short(data.reverse as i16);
                self.writer.write_bit_short(data.wizard_flag as i16);
                self.writer.write_bit_short(data.text_position);
                self.writer.write_bit_short(data.text_direction);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, data.arc_handle.value());
            }
            ExtendedEntityData::RemoteText(data) => {
                self.writer.write_3bit_double(data.position);
                self.writer.write_3bit_double(data.normal);
                self.writer.write_bit_double(data.rotation);
                self.writer.write_bit_double(data.height);
                self.writer.write_bit_short(data.flags);
                self.writer.write_variable_text(&data.text);
                let style_handle = if data.style_handle != Handle::NULL {
                    data.style_handle
                } else {
                    self.document
                        .text_styles
                        .get(&data.style_name)
                        .map(|style| style.handle)
                        .unwrap_or(Handle::NULL)
                };
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, style_handle.value());
            }
            ExtendedEntityData::GeoPositionMarker(data) => {
                self.writer.write_bit_long(data.class_version);
                self.writer.write_3bit_double(data.position);
                self.writer.write_bit_double(data.radius);
                self.writer.write_variable_text(&data.notes);
                self.writer.write_bit_double(data.landing_gap);
                self.writer.write_bit(data.mtext_visible);
                self.writer.write_byte(data.text_alignment);
                self.writer.write_bit(data.enable_frame_text);
                if data.enable_frame_text {
                    self.write_embedded_attribute_mtext(
                        data.embedded_mtext.as_ref(),
                        &data.notes,
                        data.position,
                        Vector3::UNIT_Z,
                        0.0,
                        data.radius,
                        "STANDARD",
                    );
                }
            }
            ExtendedEntityData::CoordinationModel(data) => {
                self.writer.write_bit_short(data.flags);
                self.writer.write_handle(
                    DwgReferenceType::SoftOwnership,
                    data.definition_handle.value(),
                );
                for value in data.transform {
                    self.writer.write_bit_double(value);
                }
                self.writer.write_bit_double(data.unit_factor);
            }
            ExtendedEntityData::PointCloud(data) => {
                self.write_point_cloud_data(data);
            }
            ExtendedEntityData::PointCloudEx(data) => {
                self.write_point_cloud_ex_data(data);
            }
            ExtendedEntityData::Proxy(data) => {
                self.writer.write_bit_long(data.class_id);
                if self.dxf_version > crate::types::DxfVersion::AC1015 {
                    let dxf_subclass = if data.dxf_subclass.is_empty() {
                        self.document
                            .classes
                            .iter()
                            .find(|class| i32::from(class.class_number) == data.class_id)
                            .map(|class| class.dxf_name.as_str())
                            .unwrap_or("")
                    } else {
                        &data.dxf_subclass
                    };
                    self.writer.write_variable_text(dxf_subclass);
                }
                if self.version.r2018_plus(self.dxf_version) {
                    self.writer.write_bit_long(data.dwg_version);
                    self.writer.write_bit_long(data.maintenance_version);
                } else {
                    self.writer.write_bit_long(
                        (data.maintenance_version << 16) | (data.dwg_version & 0xffff),
                    );
                }
                if self.version.r2000_plus() {
                    self.writer.write_bit(data.from_dxf);
                }
                let payload = data.payload.data();
                for bit_index in 0..data.payload.bit_count as usize {
                    let byte = payload.get(bit_index / 8).copied().unwrap_or(0);
                    self.writer
                        .write_bit((byte & (0x80 >> (bit_index % 8))) != 0);
                }
                let text_payload = data.text_payload.data();
                for bit_index in 0..data.text_payload.bit_count as usize {
                    let byte = text_payload.get(bit_index / 8).copied().unwrap_or(0);
                    self.writer
                        .write_text_bit((byte & (0x80 >> (bit_index % 8))) != 0);
                }
                for object_id in &data.object_ids {
                    let reference_type = match object_id.kind {
                        crate::objects::ProxyReferenceKind::Undefined => {
                            DwgReferenceType::Undefined
                        }
                        crate::objects::ProxyReferenceKind::SoftOwnership => {
                            DwgReferenceType::SoftOwnership
                        }
                        crate::objects::ProxyReferenceKind::HardOwnership => {
                            DwgReferenceType::HardOwnership
                        }
                        crate::objects::ProxyReferenceKind::SoftPointer => {
                            DwgReferenceType::SoftPointer
                        }
                        crate::objects::ProxyReferenceKind::HardPointer => {
                            DwgReferenceType::HardPointer
                        }
                    };
                    self.writer
                        .write_handle(reference_type, object_id.handle.value());
                }
            }
            ExtendedEntityData::OleFrame(data) => {
                self.writer.write_bit_short(data.flag);
                if self.version.r2000_plus() {
                    self.writer.write_bit_short(data.mode);
                }
                let bytes = data.storage.encode();
                self.writer.write_bit_long(bytes.len() as i32);
                self.writer.write_bytes(&bytes);
            }
            ExtendedEntityData::LayoutPrintConfig(data) => {
                self.writer.write_bit_short(data.class_version);
                self.writer.write_bit_short(data.flag);
            }
            ExtendedEntityData::Format(_) => {}
            ExtendedEntityData::Legacy(_) => {}
            ExtendedEntityData::DynamicBlock(
                crate::objects::DynamicBlockData::AngularConstraintParameterEntity(data),
            ) => self.write_dynamic_angular_constraint_entity(data),
            ExtendedEntityData::DynamicBlock(_) => {}
            ExtendedEntityData::RegisteredClass(data) => {
                if data.properties.is_empty() {
                    self.write_registered_payload(&data.payload, &data.object_ids);
                } else {
                    self.writer.write_bit_long(498);
                    if self.dxf_version > crate::types::DxfVersion::AC1015 {
                        self.writer.write_variable_text(&data.dxf_name);
                    }
                    if self.version.r2018_plus(self.dxf_version) {
                        self.writer.write_bit_long(0);
                        self.writer.write_bit_long(0);
                    } else {
                        self.writer.write_bit_long(0);
                    }
                    if self.version.r2000_plus() {
                        self.writer.write_bit(true);
                    }
                    let payload =
                        crate::objects::semantic_property::encode_registered_class_envelope(
                            &data.dxf_name,
                            &data.cpp_class_name,
                            &data.properties,
                            &data.payload,
                        );
                    self.write_registered_payload(&payload, &data.object_ids);
                }
            }
        }
        self.register_object(e.common.handle);
    }

    fn write_section_symbol(&mut self, value: &SectionSymbol) {
        let type_code = self.class_type_code("SECTIONLINE", 0);
        self.entity_preamble(type_code, &value.common);
        self.writer.write_bit_short(value.view_symbol_version);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, value.style_handle.value());
        self.writer.write_bit_double(value.symbol_scale);
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, value.view_rep_handle.value());
        self.writer.write_bit_short(value.raw_view_symbol_70);
        self.writer.write_bit_short(value.version);
        self.writer.write_bit_long(value.raw_point_count_90);
        self.writer.write_bit_long(value.raw_flags_90);
        self.writer.write_bit_long(value.raw_point_record_count);
        for point in &value.points {
            self.writer.write_3bit_double(point.point);
            self.writer.write_bit_double(point.bulge);
            self.writer.write_variable_text(&point.label);
            self.writer.write_3bit_double(point.label_offset);
            self.writer.write_byte(point.raw_flag_280);
        }
        self.register_object(value.common.handle);
    }

    fn write_view_border(&mut self, value: &ViewBorder) {
        let type_code = self.class_type_code("DRAWINGVIEW", 0);
        self.entity_preamble(type_code, &value.common);
        self.writer.write_bit_short(value.version);
        self.writer.write_raw_double(value.min[0]);
        self.writer.write_raw_double(value.min[1]);
        self.writer.write_raw_double(value.max[0]);
        self.writer.write_raw_double(value.max[1]);
        self.writer.write_raw_double(value.scale);
        self.writer.write_raw_double(value.rotation_angle);
        self.writer.write_raw_double(value.center[0]);
        self.writer.write_raw_double(value.center[1]);
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, value.active_viewport.value());
        self.writer
            .write_handle(DwgReferenceType::HardPointer, value.scale_handle.value());
        self.register_object(value.common.handle);
    }

    fn write_point_cloud_data(&mut self, data: &PointCloudData) {
        self.writer.write_bit_short(data.class_version);
        self.writer.write_3bit_double(data.origin);
        self.writer.write_variable_text(&data.saved_filename);
        self.writer.write_bit_long(data.source_files.len() as i32);
        if data.source_files.is_empty() {
            self.writer.write_3bit_double(data.extents_min);
            self.writer.write_3bit_double(data.extents_max);
            self.writer.write_bit_long_long(data.point_count);
            self.writer.write_variable_text(&data.ucs_name);
            self.writer.write_3bit_double(data.ucs_origin);
            self.writer.write_3bit_double(data.ucs_x_direction);
            self.writer.write_3bit_double(data.ucs_y_direction);
            self.writer.write_3bit_double(data.ucs_z_direction);
            if self.version.r2013_plus(self.dxf_version) {
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    data.definition_handle.value(),
                );
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, data.reactor_handle.value());
                self.writer.write_bit(data.show_intensity);
                self.writer.write_bit_short(data.intensity_scheme);
                self.writer.write_bit_double(data.minimum_intensity);
                self.writer.write_bit_double(data.maximum_intensity);
                self.writer.write_bit_double(data.low_intensity_threshold);
                self.writer.write_bit_double(data.high_intensity_threshold);
                self.writer.write_bit(data.show_clipping);
                self.writer.write_bit_long(data.clippings.len() as i32);
                for clipping in &data.clippings {
                    self.writer.write_bit(clipping.inverted);
                    self.writer.write_bit_short(clipping.clip_type);
                    if clipping.clip_type == 3 {
                        self.writer.write_bit_long(clipping.vertices.len() as i32);
                    }
                    for point in &clipping.vertices {
                        self.writer.write_2raw_double(*point);
                    }
                    if clipping.clip_type == 1 {
                        self.writer.write_bit_double(clipping.z_min);
                        self.writer.write_bit_double(clipping.z_max);
                    }
                }
            }
        }
        for source_file in &data.source_files {
            self.writer.write_variable_text(source_file);
        }
    }

    fn write_point_cloud_ex_data(&mut self, data: &PointCloudExData) {
        self.writer.write_bit_short(data.class_version);
        self.writer.write_3bit_double(data.extents_min);
        self.writer.write_3bit_double(data.extents_max);
        self.writer.write_3bit_double(data.ucs_origin);
        self.writer.write_3bit_double(data.ucs_x_direction);
        self.writer.write_3bit_double(data.ucs_y_direction);
        self.writer.write_3bit_double(data.ucs_z_direction);
        self.writer.write_bit(data.locked);
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            data.definition_handle.value(),
        );
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, data.reactor_handle.value());
        self.writer.write_variable_text(&data.name);
        self.writer.write_bit(data.show_intensity);
        self.writer.write_bit(data.show_cropping);
        self.writer.write_bit_long(data.croppings.len() as i32);
        if data.croppings.is_empty() {
            self.writer.write_bit_long(data.unknown_bl0);
            self.writer.write_bit_long(data.unknown_bl1);
            self.writer.write_bit_short(data.stylization_type);
            self.writer
                .write_variable_text(&data.intensity_color_scheme);
            self.writer.write_variable_text(&data.current_color_scheme);
            self.writer
                .write_variable_text(&data.classification_color_scheme);
            self.writer.write_bit_double(data.elevation_min);
            self.writer.write_bit_double(data.elevation_max);
            self.writer.write_bit_long(data.intensity_min);
            self.writer.write_bit_long(data.intensity_max);
            self.writer
                .write_bit_short(data.intensity_out_of_range_behavior);
            self.writer
                .write_bit_short(data.elevation_out_of_range_behavior);
            self.writer.write_bit(data.elevation_apply_to_fixed_range);
            self.writer.write_bit(data.intensity_as_gradient);
            self.writer.write_bit(data.elevation_as_gradient);
        }
        for cropping in &data.croppings {
            self.writer.write_bit_short(cropping.crop_type);
            self.writer.write_bit(cropping.inside);
            self.writer.write_bit(cropping.inverted);
            self.writer.write_3bit_double(cropping.plane);
            self.writer.write_3bit_double(cropping.x_direction);
            self.writer.write_3bit_double(cropping.y_direction);
            self.writer.write_bit_long(cropping.points.len() as i32);
            for point in &cropping.points {
                self.writer.write_3bit_double(*point);
            }
        }
    }

    // â”€â”€ Point â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_point(&mut self, e: &Point) {
        self.entity_preamble(common::OBJ_POINT, &e.common);
        self.writer.write_3bit_double(e.location);
        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_extrusion(e.normal);
        self.writer.write_bit_double(e.x_axis_angle);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Line â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_line(&mut self, e: &Line) {
        self.entity_preamble(common::OBJ_LINE, &e.common);

        if self.version.r13_14_only() {
            self.writer.write_3bit_double(e.start);
            self.writer.write_3bit_double(e.end);
        } else {
            // R2000+: z-are-zero optimization
            let z_are_zero = e.start.z == 0.0 && e.end.z == 0.0;
            self.writer.write_bit(z_are_zero);
            self.writer.write_raw_double(e.start.x);
            self.writer
                .write_bit_double_with_default(e.end.x, e.start.x);
            self.writer.write_raw_double(e.start.y);
            self.writer
                .write_bit_double_with_default(e.end.y, e.start.y);
            if !z_are_zero {
                self.writer.write_raw_double(e.start.z);
                self.writer
                    .write_bit_double_with_default(e.end.z, e.start.z);
            }
        }

        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_extrusion(e.normal);

        self.register_object(e.common.handle);
    }

    // â”€â”€ Circle â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_circle(&mut self, e: &Circle) {
        self.entity_preamble(common::OBJ_CIRCLE, &e.common);
        self.writer.write_3bit_double(e.center);
        self.writer.write_bit_double(e.radius);
        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_extrusion(e.normal);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Arc â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_arc(&mut self, e: &Arc) {
        self.entity_preamble(common::OBJ_ARC, &e.common);
        self.writer.write_3bit_double(e.center);
        self.writer.write_bit_double(e.radius);
        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_extrusion(e.normal);
        self.writer.write_bit_double(e.start_angle);
        self.writer.write_bit_double(e.end_angle);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Ellipse â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_ellipse(&mut self, e: &Ellipse) {
        self.entity_preamble(common::OBJ_ELLIPSE, &e.common);
        self.writer.write_3bit_double(e.center);
        self.writer.write_3bit_double(e.major_axis);
        self.writer.write_3bit_double(e.normal);
        self.writer.write_bit_double(e.minor_axis_ratio);
        self.writer.write_bit_double(e.start_parameter);
        self.writer.write_bit_double(e.end_parameter);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Text â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_text(&mut self, e: &Text) {
        self.entity_preamble(common::OBJ_TEXT, &e.common);

        let alignment_point = e.alignment_point.unwrap_or(Vector3::ZERO);

        if self.version.r13_14_only() {
            // Elevation BD
            self.writer.write_bit_double(e.insertion_point.z);
            // Insertion pt 2RD 10
            self.writer.write_raw_double(e.insertion_point.x);
            self.writer.write_raw_double(e.insertion_point.y);
            // Alignment pt 2RD 11
            self.writer.write_raw_double(alignment_point.x);
            self.writer.write_raw_double(alignment_point.y);
            // Extrusion 3BD 210
            self.writer.write_3bit_double(e.normal);
            // Thickness BD 39
            self.writer.write_bit_double(e.thickness);
            // Oblique ang BD 51
            self.writer.write_bit_double(e.oblique_angle);
            // Rotation ang BD 50
            self.writer.write_bit_double(e.rotation);
            // Height BD 40
            self.writer.write_bit_double(e.height);
            // Width factor BD 41
            self.writer.write_bit_double(e.width_factor);
            // Text value TV 1
            self.writer.write_variable_text(&e.value);
            // Generation BS 71
            self.writer.write_bit_short(e.generation_flags);
            // Horiz align BS 72
            self.writer.write_bit_short(e.horizontal_alignment as i16);
            // Vert align BS 73
            self.writer.write_bit_short(e.vertical_alignment as i16);
        } else {
            // R2000+: DataFlags RC — presence bits for subsequent data
            let mut data_flags: u8 = 0;
            // 0x01 = elevation (InsertPoint.Z) is 0
            if e.insertion_point.z == 0.0 {
                data_flags |= 0x01;
            }
            // 0x02 = alignment point is zero
            if alignment_point.x == 0.0 && alignment_point.y == 0.0 && alignment_point.z == 0.0 {
                data_flags |= 0x02;
            }
            // 0x04 = oblique angle is 0
            if e.oblique_angle == 0.0 {
                data_flags |= 0x04;
            }
            // 0x08 = rotation is 0
            if e.rotation == 0.0 {
                data_flags |= 0x08;
            }
            // 0x10 = width factor is 1.0
            if e.width_factor == 1.0 {
                data_flags |= 0x10;
            }
            // 0x20 = generation (mirror) flag is None (0)
            if e.generation_flags == 0 {
                data_flags |= 0x20;
            }
            // 0x40 = horizontal alignment is Left (0)
            if e.horizontal_alignment as u8 == 0 {
                data_flags |= 0x40;
            }
            // 0x80 = vertical alignment is Baseline (0)
            if e.vertical_alignment as u8 == 0 {
                data_flags |= 0x80;
            }
            self.writer.write_byte(data_flags);

            // Elevation RD — present if !(DataFlags & 0x01)
            if (data_flags & 0x01) == 0 {
                self.writer.write_raw_double(e.insertion_point.z);
            }
            // Insertion pt 2RD 10
            self.writer.write_raw_double(e.insertion_point.x);
            self.writer.write_raw_double(e.insertion_point.y);
            // Alignment pt 2DD 11 — present if !(DataFlags & 0x02)
            // Uses insertion pt X,Y as default values
            if (data_flags & 0x02) == 0 {
                self.writer
                    .write_bit_double_with_default(alignment_point.x, e.insertion_point.x);
                self.writer
                    .write_bit_double_with_default(alignment_point.y, e.insertion_point.y);
            }
            // Extrusion BE 210
            self.writer.write_bit_extrusion(e.normal);
            // Thickness BT 39
            self.writer.write_bit_thickness(e.thickness);
            // Oblique ang RD 51 — present if !(DataFlags & 0x04)
            if (data_flags & 0x04) == 0 {
                self.writer.write_raw_double(e.oblique_angle);
            }
            // Rotation ang RD 50 — present if !(DataFlags & 0x08)
            if (data_flags & 0x08) == 0 {
                self.writer.write_raw_double(e.rotation);
            }
            // Height RD 40 (always present)
            self.writer.write_raw_double(e.height);
            // Width factor RD 41 — present if !(DataFlags & 0x10)
            if (data_flags & 0x10) == 0 {
                self.writer.write_raw_double(e.width_factor);
            }
            // Text value TV 1
            self.writer.write_variable_text(&e.value);
            // Generation BS 71 — present if !(DataFlags & 0x20)
            if (data_flags & 0x20) == 0 {
                self.writer.write_bit_short(e.generation_flags);
            }
            // Horiz align BS 72 — present if !(DataFlags & 0x40)
            if (data_flags & 0x40) == 0 {
                self.writer.write_bit_short(e.horizontal_alignment as i16);
            }
            // Vert align BS 73 — present if !(DataFlags & 0x80)
            if (data_flags & 0x80) == 0 {
                self.writer.write_bit_short(e.vertical_alignment as i16);
            }
        }

        // Style handle
        let style_handle = self
            .document
            .text_styles
            .get(&e.style)
            .map(|s| s.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ MText â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_mtext(&mut self, e: &MText) {
        self.entity_preamble(common::OBJ_MTEXT, &e.common);

        // Insertion pt 3BD 10
        self.writer.write_3bit_double(e.insertion_point);
        // Extrusion 3BD 210 (NOT BitExtrusion — full 3BD per spec)
        self.writer.write_3bit_double(e.normal);

        // X-axis dir 3BD 11 (alignment point / direction vector)
        let x_dir = e
            .dwg_x_direction
            .filter(|direction| direction.y.atan2(direction.x) == e.rotation)
            .unwrap_or_else(|| Vector3::new(e.rotation.cos(), e.rotation.sin(), 0.0));
        self.writer.write_3bit_double(x_dir);

        // Rect width BD 41
        self.writer.write_bit_double(e.rectangle_width);

        // R2007+: Rect height BD 46
        if self.version.r2007_plus() {
            self.writer
                .write_bit_double(e.rectangle_height.unwrap_or(0.0));
        }

        // Text height BD 40
        self.writer.write_bit_double(e.height);
        // Attachment BS 71
        self.writer.write_bit_short(e.attachment_point as i16);
        // Drawing dir BS 72 (unconditional — written for ALL versions)
        self.writer.write_bit_short(e.drawing_direction as i16);

        // Extents ht BD (DXF 43, output-only)
        self.writer.write_bit_double(e.extents_height);
        // Extents wid BD (DXF 42, output-only)
        self.writer.write_bit_double(e.extents_width);

        // Text TV 1
        self.writer.write_variable_text(&e.value);

        // H 7 STYLE (hard pointer) — written BEFORE R2000+ block
        let style_handle = self
            .document
            .text_styles
            .get(&e.style)
            .map(|s| s.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());

        // Linespacing Style BS 73 (1=At Least, 2=Exact)
        self.writer.write_bit_short(e.line_spacing_style as i16);
        // Linespacing Factor BD 44
        self.writer.write_bit_double(e.line_spacing_factor);
        // Unknown bit B
        self.writer.write_bit(false);

        // R2004+:
        if self.version.r2004_plus() {
            // Background flags BL 90 (0 = no background)
            self.writer.write_bit_long(e.background_fill_flags);

            // The background-fill block is written when the UseBackgroundFillColor
            // bit (0x01) is set, or — for R2018+ — when the TextFrame bit (0x10)
            // is set. Mirrors read_mtext.
            if (e.background_fill_flags & 0x01) != 0
                || (self.version.r2018_plus(self.dxf_version)
                    && (e.background_fill_flags & 0x10) != 0)
            {
                // Background scale factor BD 45
                self.writer.write_bit_double(e.background_scale);
                // Background color CMC 63
                self.writer.write_cm_color(&e.background_color);
                // Background transparency BL 441
                self.writer.write_bit_long(e.background_transparency);
            }
        }

        // R2018+:
        if self.version.r2018_plus(self.dxf_version) {
            // Is NOT annotative B
            self.writer.write_bit(!e.is_annotative);

            // IF MTEXT is not annotative: redundant fields + column data.
            if !e.is_annotative {
                // Version BS (default 0; the reference implementation emits 4)
                self.writer.write_bit_short(4);
                // Default flag B (default true)
                self.writer.write_bit(true);
                // Registered application H (null hard pointer)
                self.writer.write_handle(DwgReferenceType::HardPointer, 0);

                // ── BEGIN redundant fields (discarded on read) ──
                // Attachment point BL
                self.writer.write_bit_long(e.attachment_point as i32);
                // X-axis dir 3BD
                let x_dir_redundant = x_dir;
                self.writer.write_3bit_double(x_dir_redundant);
                // Insertion point 3BD
                self.writer.write_3bit_double(e.insertion_point);
                // Rect width BD
                self.writer.write_bit_double(e.rectangle_width);
                // Rect height BD
                self.writer
                    .write_bit_double(e.rectangle_height.unwrap_or(0.0));
                // Extents width BD
                self.writer.write_bit_double(0.0);
                // Extents height BD
                self.writer.write_bit_double(0.0);
                // ── END redundant fields ──

                let col = &e.column_data;
                // Column type BS 71
                self.writer.write_bit_short(col.column_type);
                if col.column_type != 0 {
                    // Column height count BL 72. For dynamic, non-auto-height
                    // columns the reader consumes exactly this many height
                    // doubles, so it must match the number we emit below.
                    let has_heights = col.column_type == 2 && !col.auto_height;
                    let height_count = if has_heights {
                        col.heights.len() as i32
                    } else {
                        col.column_count
                    };
                    self.writer.write_bit_long(height_count);
                    // Column width BD 44
                    self.writer.write_bit_double(col.width);
                    // Gutter BD 45
                    self.writer.write_bit_double(col.gutter);
                    // Auto height? B 73
                    self.writer.write_bit(col.auto_height);
                    // Flow reversed? B 74
                    self.writer.write_bit(col.flow_reversed);

                    // Per-column heights only for dynamic, non-auto columns.
                    if !col.auto_height && col.column_type == 2 {
                        for h in &col.heights {
                            // Column height BD 46
                            self.writer.write_bit_double(*h);
                        }
                    }
                }
            }
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ Solid â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_solid(&mut self, e: &Solid) {
        let type_code = if e.is_trace {
            common::OBJ_TRACE
        } else {
            common::OBJ_SOLID
        };
        self.entity_preamble(type_code, &e.common);
        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_double(e.first_corner.z);
        self.writer
            .write_2raw_double(Vector2::new(e.first_corner.x, e.first_corner.y));
        self.writer
            .write_2raw_double(Vector2::new(e.second_corner.x, e.second_corner.y));
        self.writer
            .write_2raw_double(Vector2::new(e.third_corner.x, e.third_corner.y));
        self.writer
            .write_2raw_double(Vector2::new(e.fourth_corner.x, e.fourth_corner.y));
        self.writer.write_bit_extrusion(e.normal);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Face3D â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_face3d(&mut self, e: &Face3D) {
        self.entity_preamble(common::OBJ_3DFACE, &e.common);

        if self.version.r13_14_only() {
            self.writer.write_3bit_double(e.first_corner);
            self.writer.write_3bit_double(e.second_corner);
            self.writer.write_3bit_double(e.third_corner);
            self.writer.write_3bit_double(e.fourth_corner);
            self.writer.write_bit_short(e.invisible_edges.bits() as i16);
        } else {
            // R2000+
            let has_no_flags = e.invisible_edges.bits() == 0;
            self.writer.write_bit(has_no_flags);

            let z_is_zero = e.first_corner.z == 0.0
                && e.second_corner.z == 0.0
                && e.third_corner.z == 0.0
                && e.fourth_corner.z == 0.0;
            self.writer.write_bit(z_is_zero);

            self.writer.write_raw_double(e.first_corner.x);
            self.writer.write_raw_double(e.first_corner.y);
            if !z_is_zero {
                self.writer.write_raw_double(e.first_corner.z);
            }

            // Corners 2-4 are full 3DD (x/y/z) relative to the previous
            // corner. z_is_zero only governs corner1.z (the RD above), NOT
            // these default-double corners: when z is zero each z-DD is just
            // the 2-bit "equals default" code, but it must still be present
            // (the reader always consumes a full 3DD here).
            // 2nd corner 3DD (default = 1st corner)
            self.writer
                .write_bit_double_with_default(e.second_corner.x, e.first_corner.x);
            self.writer
                .write_bit_double_with_default(e.second_corner.y, e.first_corner.y);
            self.writer
                .write_bit_double_with_default(e.second_corner.z, e.first_corner.z);

            // 3rd corner 3DD (default = 2nd corner)
            self.writer
                .write_bit_double_with_default(e.third_corner.x, e.second_corner.x);
            self.writer
                .write_bit_double_with_default(e.third_corner.y, e.second_corner.y);
            self.writer
                .write_bit_double_with_default(e.third_corner.z, e.second_corner.z);

            // 4th corner 3DD (default = 3rd corner)
            self.writer
                .write_bit_double_with_default(e.fourth_corner.x, e.third_corner.x);
            self.writer
                .write_bit_double_with_default(e.fourth_corner.y, e.third_corner.y);
            self.writer
                .write_bit_double_with_default(e.fourth_corner.z, e.third_corner.z);

            if !has_no_flags {
                self.writer.write_bit_short(e.invisible_edges.bits() as i16);
            }
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ Insert â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_insert(&mut self, e: &Insert) {
        let is_minsert = e.is_minsert();
        let is_view_rep = e.view_rep_handle.is_some();
        let type_code = if is_view_rep {
            self.class_type_code("ACDBVIEWREPBLOCKREFERENCE", common::OBJ_INSERT)
        } else if is_minsert {
            common::OBJ_MINSERT
        } else {
            common::OBJ_INSERT
        };
        self.entity_preamble(type_code, &e.common);

        // Ins pt 3BD 10
        self.writer.write_3bit_double(e.insert_point);

        if self.version.r13_14_only() {
            // R13-R14: X/Y/Z Scale as separate BD values
            self.writer.write_bit_double(e.x_scale());
            self.writer.write_bit_double(e.y_scale());
            self.writer.write_bit_double(e.z_scale());
        }

        if self.version.r2000_plus() {
            // R2000+: Data flags BB + conditional scale data
            let sx = e.x_scale();
            let sy = e.y_scale();
            let sz = e.z_scale();

            if sx == 1.0 && sy == 1.0 && sz == 1.0 {
                // 11 - scale is (1.0, 1.0, 1.0), no data stored
                self.writer.write_2bits(3);
            } else if sx == sy && sx == sz {
                // 10 - 41 value stored as RD, 42 & 43 assumed equal to 41
                self.writer.write_2bits(2);
                self.writer.write_raw_double(sx);
            } else if sx == 1.0 {
                // 01 - 41 is 1.0, 2 DD's present using 1.0 as default
                self.writer.write_2bits(1);
                self.writer.write_bit_double_with_default(sy, 1.0);
                self.writer.write_bit_double_with_default(sz, 1.0);
            } else {
                // 00 - 41 as RD, then 42 as DD (default=41), 43 as DD (default=41)
                self.writer.write_2bits(0);
                self.writer.write_raw_double(sx);
                self.writer.write_bit_double_with_default(sy, sx);
                self.writer.write_bit_double_with_default(sz, sx);
            }
        }

        // Rotation BD 50
        self.writer.write_bit_double(e.rotation);
        // Extrusion 3BD 210
        self.writer.write_3bit_double(e.normal);
        // Has ATTRIBs B 66
        self.writer.write_bit(e.has_attributes());

        // R2004+: owned object count when has_attribs
        let (attrib_handles, seqend_handle) = if e.has_attributes() {
            // Preserve existing attribute handles and allocate only missing ones.
            let ahs: Vec<Handle> = e
                .attributes
                .iter()
                .map(|a| {
                    if a.common.handle.is_null() {
                        self.alloc_handle()
                    } else {
                        a.common.handle
                    }
                })
                .collect();
            let sh = e
                .seqend_handle
                .filter(|handle| !handle.is_null())
                .unwrap_or_else(|| self.alloc_handle());

            if self.version.r2004_plus() {
                // owned_object_count = attribs (SEQEND written separately)
                self.writer.write_bit_long(e.attributes.len() as i32);
            }
            (ahs, sh)
        } else {
            (Vec::new(), Handle::NULL)
        };

        // Block header ref (hard pointer)
        let block_handle = self
            .document
            .block_records
            .get(&e.block_name)
            .map(|br| br.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, block_handle.value());

        if let Some(view_rep_handle) = e.view_rep_handle {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, view_rep_handle.value());
        }

        // MINSERT extends INSERT with its rectangular-array definition.
        if is_minsert {
            self.writer.write_bit_short(e.column_count.max(1) as i16);
            self.writer.write_bit_short(e.row_count.max(1) as i16);
            self.writer.write_bit_double(e.column_spacing);
            self.writer.write_bit_double(e.row_spacing);
        }

        // Attribute owned handles (if present)
        if e.has_attributes() {
            if self.version.r13_15_only() {
                // R13-R2000: first attrib, last attrib
                let first = attrib_handles.first().copied().unwrap_or(Handle::NULL);
                let last = attrib_handles.last().copied().unwrap_or(Handle::NULL);
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, first.value());
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, last.value());
            } else if self.version.r2004_plus() {
                for &ah in &attrib_handles {
                    self.writer
                        .write_handle(DwgReferenceType::HardOwnership, ah.value());
                }
            }
            // SEQEND handle
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, seqend_handle.value());
        }

        self.register_object(e.common.handle);

        // Write child ATTRIB entities + SEQEND
        if e.has_attributes() {
            let saved_prev = self.prev_handle.take();
            let saved_next = self.next_handle.take();

            for (i, (att, &ah)) in e.attributes.iter().zip(attrib_handles.iter()).enumerate() {
                // Record owner override for extension dictionary
                if let Some(xdic) = &att.common.xdictionary_handle {
                    if !xdic.is_null() {
                        self.owner_overrides.insert(*xdic, ah);
                    }
                }
                self.prev_handle = if i > 0 {
                    Some(attrib_handles[i - 1])
                } else {
                    None
                };
                self.next_handle = if i + 1 < attrib_handles.len() {
                    Some(attrib_handles[i + 1])
                } else {
                    None
                };
                self.write_attribute_entity_child(att, ah, e.common.handle);
            }

            // Write SEQEND
            self.prev_handle = None;
            self.next_handle = None;
            self.write_common_entity_data(
                common::OBJ_SEQEND,
                seqend_handle,
                e.common.handle,
                &e.common.layer,
                &e.common.color,
                &crate::types::LineWeight::ByLayer,
                &crate::types::Transparency::default(),
                false,
                1.0,
                "ByLayer",
                &None,
                &crate::xdata::ExtendedData::default(),
                &[],
                &None,
                None,
                None,
                0,
                &None,
                0,
                0,
                &None,
                &None,
                &None,
                &None,
                &None,
            );
            self.register_object(seqend_handle);

            self.prev_handle = saved_prev;
            self.next_handle = saved_next;
        }
    }

    /// Write a child ATTRIB entity owned by an INSERT.
    fn write_attribute_entity_child(
        &mut self,
        att: &AttributeEntity,
        handle: Handle,
        owner: Handle,
    ) {
        self.write_common_entity_data(
            common::OBJ_ATTRIB,
            handle,
            owner,
            &att.common.layer,
            &att.common.color,
            &att.common.line_weight,
            &att.common.transparency,
            att.common.invisible,
            att.common.linetype_scale,
            &att.common.linetype,
            &att.common.linetype_handle,
            &att.common.extended_data,
            &att.common.reactors,
            &att.common.xdictionary_handle,
            att.common.graphic_data.as_deref(),
            att.common.entity_mode,
            att.common.material_flags,
            &att.common.material_handle,
            att.common.shadow_flags,
            att.common.plotstyle_flags,
            &att.common.plotstyle_handle,
            &att.common.color_book_handle,
            &att.common.full_visual_style_handle,
            &att.common.face_visual_style_handle,
            &att.common.edge_visual_style_handle,
        );
        self.write_text_entity_data(
            att.insertion_point,
            att.alignment_point,
            att.normal,
            0.0, // thickness
            att.oblique_angle,
            att.rotation,
            att.height,
            att.width_factor,
            &att.value,
            att.text_generation_flags,
            att.horizontal_alignment as i16,
            att.vertical_alignment as i16,
        );

        // writeCommonAttData: R2010+ version byte
        if self.version.r2010_plus() {
            self.writer.write_byte(0);
        }

        // R2018+: AttributeType byte
        if self.version.r2018_plus(self.dxf_version) {
            let att_type = if att.embedded_mtext.is_some() || att.is_multiline {
                att.mtext_flag.to_value().max(2) as u8
            } else {
                1
            };
            self.writer.write_byte(att_type);
            if att_type > 1 {
                self.write_embedded_attribute_mtext(
                    att.embedded_mtext.as_deref(),
                    &att.value,
                    att.insertion_point,
                    att.normal,
                    att.rotation,
                    att.height,
                    &att.text_style,
                );
                self.writer.write_bit_short(0);
            }
        }

        // Tag, field length, flags
        self.writer.write_variable_text(&att.tag);
        self.writer.write_bit_short(att.field_length);
        let flag_byte = att.flags.to_bits();
        self.writer.write_byte(flag_byte as u8);

        // R2007+: lock position
        if self.version.r2007_plus() {
            self.writer.write_bit(att.lock_position);
        }
        let style_handle = self
            .document
            .text_styles
            .get(&att.text_style)
            .map(|s| s.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());

        self.register_object(handle);
    }

    // â”€â”€ LwPolyline â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_lwpolyline(&mut self, e: &LwPolyline) {
        self.entity_preamble(common::OBJ_LWPOLYLINE, &e.common);

        let num_pts = e.vertices.len() as i32;

        // Check for presence of optional data
        let has_widths = e
            .vertices
            .iter()
            .any(|v| v.start_width != 0.0 || v.end_width != 0.0);
        let has_bulges = e.vertices.iter().any(|v| v.bulge != 0.0);
        let has_vertex_ids =
            self.version.r2010_plus() && e.vertices.iter().any(|v| v.vertex_id != 0);
        let has_constant_width = e.constant_width != 0.0;
        let has_elevation = e.elevation != 0.0;
        let has_thickness = e.thickness != 0.0;
        let has_normal = e.normal != Vector3::UNIT_Z;

        // Build flags
        let mut flag: i16 = 0;
        if has_normal {
            flag |= 0x1;
        }
        if has_thickness {
            flag |= 0x2;
        }
        if has_constant_width {
            flag |= 0x4;
        }
        if has_elevation {
            flag |= 0x8;
        }
        if has_bulges {
            flag |= 0x10;
        }
        if has_widths {
            flag |= 0x20;
        }
        if has_vertex_ids {
            flag |= 0x400;
        }
        if e.plinegen {
            flag |= 0x100;
        }
        if e.is_closed {
            flag |= 0x200;
        }

        self.writer.write_bit_short(flag);

        if has_constant_width {
            self.writer.write_bit_double(e.constant_width);
        }
        if has_elevation {
            self.writer.write_bit_double(e.elevation);
        }
        // LWPOLYLINE stores its own thickness/extrusion as plain BD / 3BD, NOT
        // the self-compressing BT / BE forms (which would desync every reader).
        // Matches the reference writeLwPolyline and read_lwpolyline above.
        if has_thickness {
            self.writer.write_bit_double(e.thickness);
        }
        if has_normal {
            self.writer.write_3bit_double(e.normal);
        }

        self.writer.write_bit_long(num_pts);

        if has_bulges {
            self.writer.write_bit_long(num_pts);
        }
        if has_vertex_ids {
            self.writer.write_bit_long(num_pts);
        }
        if has_widths {
            self.writer.write_bit_long(num_pts);
        }

        // R13-R14: simple 2RD for each vertex
        if self.version.r13_14_only() {
            for v in &e.vertices {
                self.writer.write_raw_double(v.location.x);
                self.writer.write_raw_double(v.location.y);
            }
        }

        // R2000+: first vertex is 2RD, rest are 2DD with previous as default
        if self.version.r2000_plus() && !e.vertices.is_empty() {
            let first = &e.vertices[0];
            self.writer.write_raw_double(first.location.x);
            self.writer.write_raw_double(first.location.y);

            for i in 1..e.vertices.len() {
                let curr = &e.vertices[i];
                let prev = &e.vertices[i - 1];
                self.writer
                    .write_bit_double_with_default(curr.location.x, prev.location.x);
                self.writer
                    .write_bit_double_with_default(curr.location.y, prev.location.y);
            }
        }

        // Bulges
        if has_bulges {
            for v in &e.vertices {
                self.writer.write_bit_double(v.bulge);
            }
        }

        if has_vertex_ids {
            for v in &e.vertices {
                self.writer.write_bit_long(v.vertex_id);
            }
        }

        // Widths
        if has_widths {
            for v in &e.vertices {
                self.writer.write_bit_double(v.start_width);
                self.writer.write_bit_double(v.end_width);
            }
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ Ray â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_ray(&mut self, e: &Ray) {
        self.entity_preamble(common::OBJ_RAY, &e.common);
        self.writer.write_3bit_double(e.base_point);
        self.writer.write_3bit_double(e.direction);
        self.register_object(e.common.handle);
    }

    // â”€â”€ XLine â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_xline(&mut self, e: &XLine) {
        self.entity_preamble(common::OBJ_XLINE, &e.common);
        self.writer.write_3bit_double(e.base_point);
        self.writer.write_3bit_double(e.direction);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Spline â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_spline(&mut self, e: &Spline) {
        self.entity_preamble(common::OBJ_SPLINE, &e.common);
        self.write_spline_data(e);
        self.register_object(e.common.handle);
    }

    /// Write the AcDbSpline object-data body (everything after the entity
    /// preamble, no handle registration). Shared by SPLINE and HELIX, which
    /// embeds a spline as its curve geometry.
    fn write_spline_data(&mut self, e: &Spline) {
        let r2013_plus = self.version.r2013_plus(self.dxf_version);
        // R2013+ derives the storage scenario from flags1 and knot
        // parameterization. Custom knots always use control points.
        let scenario: i32 =
            if !e.fit_points.is_empty() && (!r2013_plus || e.knot_parameterization != 15) {
                2
            } else {
                1
            };

        if r2013_plus {
            // R2013+: scenario BL, flags1 BL, knot parametrization BL
            let mut flags1 = e.dwg_flags1;
            if e.cv_frame_visible {
                flags1 |= 2;
            } else {
                flags1 &= !2;
            }
            if scenario == 2 {
                // Fit-point storage requires both MethodFitPoints and
                // UseKnotParameter. Omitting bit 8 makes readers parse the
                // following fit-point body as control-point data.
                flags1 |= 1 | 8;
                if e.flags.closed {
                    flags1 |= 4;
                } else {
                    flags1 &= !4;
                }
            } else {
                // Control-point records carry their closed state in the
                // scenario body. Preserve the source flag word exactly;
                // synthesizing bit 4 here changes otherwise stable records.
                flags1 &= !8;
            }
            self.writer.write_bit_long(scenario);
            self.writer.write_bit_long(flags1);
            self.writer.write_bit_long(e.knot_parameterization); // knot parametrization
        } else {
            // Scenario BL
            self.writer.write_bit_long(scenario);
        }

        // Degree BL (common, before scenario switch)
        self.writer.write_bit_long(e.degree);

        let has_weights = !e.weights.is_empty();

        match scenario {
            1 => {
                // Scenario 1: control-point spline
                // Rational B (flag bit 2)
                self.writer.write_bit(e.flags.rational);
                // Closed B (flag bit 0)
                self.writer.write_bit(e.flags.closed);
                // Periodic B (flag bit 1)
                self.writer.write_bit(e.flags.periodic);

                // Knot tol BD 42
                self.writer.write_bit_double(e.knot_tolerance);
                // Ctrl tol BD 43
                self.writer.write_bit_double(e.control_tolerance);

                // Generate clamped uniform knot vector if not provided
                let knots: Vec<f64> = if e.knots.is_empty() && !e.control_points.is_empty() {
                    Spline::generate_clamped_knots(e.degree as usize, e.control_points.len())
                } else {
                    e.knots.clone()
                };

                // Numknots BL 72
                self.writer.write_bit_long(knots.len() as i32);
                // Numctrlpts BL 73
                self.writer.write_bit_long(e.control_points.len() as i32);

                // Weight B (echo of rational flag for weights present)
                self.writer.write_bit(has_weights);

                // Knots
                for k in &knots {
                    self.writer.write_bit_double(*k);
                }

                // Control points + weights
                for (i, pt) in e.control_points.iter().enumerate() {
                    self.writer.write_3bit_double(*pt);
                    if has_weights {
                        let w = e.weights.get(i).copied().unwrap_or(1.0);
                        self.writer.write_bit_double(w);
                    }
                }
            }
            _ => {
                // Scenario 2: fit-point spline
                // Fit Tol BD 44
                self.writer.write_bit_double(e.fit_tolerance);
                // Beg tan vec 3BD 12
                self.writer.write_3bit_double(e.begin_tangent);
                // End tan vec 3BD 13
                self.writer.write_3bit_double(e.end_tangent);
                // num fit pts BL 74
                self.writer.write_bit_long(e.fit_points.len() as i32);
                // Fit points
                for pt in &e.fit_points {
                    self.writer.write_3bit_double(*pt);
                }
            }
        }
    }

    // ── Helix ───────────────────────────────────────────────────────

    /// Write a HELIX entity (AcDbHelix): the full spline record followed by
    /// the helix parameters. HELIX is UNLISTED, so its type code comes from
    /// the registered class number.
    fn write_helix(&mut self, e: &Helix) {
        let type_code = self.class_type_code("HELIX", common::OBJ_HELIX);
        self.entity_preamble(type_code, &e.common);

        // AcDbSpline part (curve geometry).
        self.write_spline_data(&e.spline);

        // AcDbHelix part.
        self.writer.write_bit_long(e.major_version);
        self.writer.write_bit_long(e.maintenance_version);
        self.writer.write_3bit_double(e.axis_base_point);
        self.writer.write_3bit_double(e.start_point);
        self.writer.write_3bit_double(e.axis_vector);
        self.writer.write_bit_double(e.radius);
        self.writer.write_bit_double(e.turns);
        self.writer.write_bit_double(e.turn_height);
        self.writer.write_bit(e.handedness);
        self.writer.write_byte(e.constraint.to_code());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Leader â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_leader(&mut self, e: &Leader) {
        self.entity_preamble(common::OBJ_LEADER, &e.common);

        // Unknown B
        self.writer.write_bit(e.dwg_unknown_bit1);
        // Annotation type BS
        self.writer.write_bit_short(e.creation_type.to_value());
        // Path type BS
        self.writer.write_bit_short(e.path_type as i16);

        // Numpts BL + vertices
        self.writer.write_bit_long(e.vertices.len() as i32);
        for pt in &e.vertices {
            self.writer.write_3bit_double(*pt);
        }

        // Origin 3BD (first vertex by default)
        let origin = if e.origin == Vector3::ZERO {
            e.vertices.first().copied().unwrap_or(Vector3::ZERO)
        } else {
            e.origin
        };
        self.writer.write_3bit_double(origin);
        // Extrusion 3BD 210
        self.writer.write_3bit_double(e.normal);
        // X direction 3BD 211
        self.writer.write_3bit_double(e.horizontal_direction);
        // Offsettoblockinspt 3BD 212
        self.writer.write_3bit_double(e.block_offset);

        // R14+: Endptproj 3BD (annotation offset) — not present in R13
        if self.dxf_version >= crate::types::DxfVersion::AC1014 {
            self.writer.write_3bit_double(e.annotation_offset);
        }

        // R13-R14 Only: DIMGAP and arrowhead data
        if self.version.r13_14_only() {
            self.writer.write_bit_double(e.dimension_gap);
        }

        // R13-R2007: annotation box height / width.
        if self.dxf_version <= crate::types::DxfVersion::AC1021 {
            self.writer.write_bit_double(e.text_height);
            self.writer.write_bit_double(e.text_width);
        }

        // Hooklineonxdir B
        self.writer
            .write_bit(e.hookline_direction == HooklineDirection::Same);
        // Arrowheadon B
        self.writer.write_bit(e.arrow_enabled);

        // R13-R14 names this field arrowhead type.  R2000+ retains the same
        // stream slot as an undocumented bit-short.
        let arrowhead_or_unknown = if self.version.r13_14_only() {
            if e.hookline_enabled {
                e.arrowhead_type | 8
            } else {
                e.arrowhead_type & !8
            }
        } else {
            e.dwg_unknown_short1
        };
        self.writer.write_bit_short(arrowhead_or_unknown);
        if self.version.r13_14_only() {
            self.writer.write_bit_double(e.arrow_size);
            self.writer.write_bit(e.dwg_unknown_bit2);
            self.writer.write_bit(e.dwg_unknown_bit3);
            self.writer.write_bit_short(e.dwg_unknown_short1);
            self.writer.write_bit_short(e.byblock_color);
            self.writer.write_bit(e.dwg_unknown_bit4);
            self.writer.write_bit(e.dwg_unknown_bit5);
        } else {
            self.writer.write_bit(e.dwg_unknown_bit4);
            self.writer.write_bit(e.dwg_unknown_bit5);
        }

        // H 340 Associated annotation (hard pointer, null)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, e.annotation_handle.value());

        // H 2 DIMSTYLE (hard pointer)
        let dimstyle_handle = self
            .document
            .dim_styles
            .get(&e.dimension_style)
            .map(|d| d.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, dimstyle_handle.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Tolerance â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_tolerance(&mut self, e: &Tolerance) {
        self.entity_preamble(common::OBJ_TOLERANCE, &e.common);

        // R13-R14 Only:
        if self.version.r13_14_only() {
            self.writer.write_bit_short(e.dwg_unknown_short);
            self.writer.write_bit_double(e.text_height); // Height BD
            self.writer.write_bit_double(e.dimension_gap); // Dimgap BD
        }

        // Common:
        // Ins pt 3BD 10
        self.writer.write_3bit_double(e.insertion_point);
        // X direction 3BD 11
        self.writer.write_3bit_double(e.direction);
        // Extrusion 3BD 210
        self.writer.write_3bit_double(e.normal);
        // Text string BS 1
        self.writer.write_variable_text(&e.text);

        // Dim style handle (hard pointer)
        let ds_handle = e.dimension_style_handle.unwrap_or(
            self.document
                .dim_styles
                .get(&e.dimension_style_name)
                .map(|d| d.handle)
                .unwrap_or(Handle::NULL),
        );
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ds_handle.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Shape â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_shape(&mut self, e: &Shape) {
        self.entity_preamble(common::OBJ_SHAPE, &e.common);

        // Ins pt 3BD 10
        self.writer.write_3bit_double(e.insertion_point);
        // Size BD 40
        self.writer.write_bit_double(e.size);
        // Rotation BD 50
        self.writer.write_bit_double(e.rotation);
        // Relative X Scale BD 41
        self.writer.write_bit_double(e.relative_x_scale);
        // Oblique angle BD 51
        self.writer.write_bit_double(e.oblique_angle);
        // Thickness BD 39
        self.writer.write_bit_double(e.thickness);
        // Shape index BS 2
        self.writer.write_bit_short(e.shape_number as i16);
        // Extrusion 3BD 210
        self.writer.write_3bit_double(e.normal);

        // SHAPEFILE style handle (hard pointer)
        let sh = e.style_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, sh.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Hatch â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_hatch(&mut self, e: &Hatch) {
        if e.is_mpolygon {
            self.write_mpolygon(e);
            return;
        }
        self.entity_preamble(common::OBJ_HATCH, &e.common);

        // Gradient color data (R2004+)
        if self.version.r2004_plus() {
            let is_gradient = e.gradient_color.enabled;
            self.writer.write_bit_long(if is_gradient { 1 } else { 0 });

            // All gradient fields must be written unconditionally
            self.writer.write_bit_long(e.gradient_color.reserved);
            self.writer.write_bit_double(e.gradient_color.angle);
            self.writer.write_bit_double(e.gradient_color.shift);
            self.writer
                .write_bit_long(if e.gradient_color.is_single_color {
                    1
                } else {
                    0
                });
            self.writer.write_bit_double(e.gradient_color.color_tint);

            self.writer
                .write_bit_long(e.gradient_color.colors.len() as i32);
            for entry in &e.gradient_color.colors {
                self.writer.write_bit_double(entry.value);
                self.writer.write_cm_color(&entry.color);
            }

            self.writer.write_variable_text(&e.gradient_color.name);
        }

        // Elevation (Z of insertion point)
        self.writer.write_bit_double(e.elevation);
        self.writer.write_3bit_double(e.normal);
        self.writer.write_variable_text(&e.pattern.name);

        // Solid fill flag
        self.writer.write_bit(e.is_solid);
        // Associative flag
        self.writer.write_bit(e.is_associative);

        // Boundary paths
        let mut has_derived_boundary = false;
        self.writer.write_bit_long(e.paths.len() as i32);
        for path in &e.paths {
            if path.flags.is_derived() {
                has_derived_boundary = true;
            }
            self.write_hatch_boundary_path(path);
        }

        // Hatch style
        self.writer.write_bit_short(e.style as i16);
        // Pattern type
        self.writer.write_bit_short(e.pattern_type as i16);

        if !e.is_solid {
            // Pattern angle + scale + double flag
            self.writer.write_bit_double(e.pattern_angle);
            self.writer.write_bit_double(e.pattern_scale);
            self.writer.write_bit(e.is_double);

            // Pattern definition lines
            self.writer.write_bit_short(e.pattern.lines.len() as i16);
            for line in &e.pattern.lines {
                self.writer.write_bit_double(line.angle);
                self.writer.write_2bit_double(line.base_point);
                self.writer.write_2bit_double(line.offset);
                self.writer.write_bit_short(line.dash_lengths.len() as i16);
                for d in &line.dash_lengths {
                    self.writer.write_bit_double(*d);
                }
            }
        }

        // Pixel size — only written when a Derived boundary path exists
        if has_derived_boundary {
            self.writer.write_bit_double(e.pixel_size);
        }

        // Seed points
        self.writer.write_bit_long(e.seed_points.len() as i32);
        for sp in &e.seed_points {
            self.writer.write_2raw_double(*sp);
        }

        // Boundary object handles
        for path in &e.paths {
            for h in &path.boundary_handles {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, h.value());
            }
        }

        self.register_object(e.common.handle);
    }

    fn write_mpolygon(&mut self, e: &Hatch) {
        let type_code = self.class_type_code("MPOLYGON", common::OBJ_MPOLYGON);
        self.entity_preamble(type_code, &e.common);
        self.writer.write_bit_short(e.style as i16);

        if self.version.r2004_plus() {
            self.writer
                .write_bit_long(if e.gradient_color.enabled { 1 } else { 0 });
            self.writer.write_bit_long(e.gradient_color.reserved);
            self.writer.write_bit_double(e.gradient_color.angle);
            self.writer.write_bit_double(e.gradient_color.shift);
            self.writer
                .write_bit_long(if e.gradient_color.is_single_color {
                    1
                } else {
                    0
                });
            self.writer.write_bit_double(e.gradient_color.color_tint);
            self.writer
                .write_bit_long(e.gradient_color.colors.len() as i32);
            for entry in &e.gradient_color.colors {
                self.writer.write_bit_double(entry.value);
                self.writer.write_cm_color(&entry.color);
            }
            self.writer.write_variable_text(&e.gradient_color.name);
        }

        self.writer.write_bit_double(e.elevation);
        self.writer.write_3bit_double(e.normal);
        self.writer.write_variable_text(&e.pattern.name);
        self.writer.write_bit(e.is_solid);
        self.writer.write_bit(e.is_associative);
        self.writer.write_bit_long(e.paths.len() as i32);
        for path in &e.paths {
            self.write_hatch_boundary_path(path);
        }
        self.writer.write_bit_short(e.style as i16);
        self.writer.write_bit_short(e.pattern_type as i16);
        if !e.is_solid {
            self.writer.write_bit_double(e.pattern_angle);
            self.writer.write_bit_double(e.pattern_scale);
            self.writer.write_bit(e.is_double);
            self.writer.write_bit_short(e.pattern.lines.len() as i16);
            for line in &e.pattern.lines {
                self.writer.write_bit_double(line.angle);
                self.writer.write_2bit_double(line.base_point);
                self.writer.write_2bit_double(line.offset);
                self.writer.write_bit_short(line.dash_lengths.len() as i16);
                for dash in &line.dash_lengths {
                    self.writer.write_bit_double(*dash);
                }
            }
        }
        self.writer.write_cm_color(&e.mpolygon_hatch_color);
        self.writer.write_2raw_double(e.mpolygon_x_direction);
        self.writer.write_bit_long(e.mpolygon_boundary_handle_count);
        for path in &e.paths {
            for handle in &path.boundary_handles {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, handle.value());
            }
        }
        self.register_object(e.common.handle);
    }

    fn write_hatch_boundary_path(&mut self, path: &BoundaryPath) {
        self.writer.write_bit_long(path.flags.bits() as i32);
        self.write_hatch_boundary_path_contents(path, path.flags.bits() as i32, true);
    }

    pub(super) fn write_hatch_boundary_path_contents(
        &mut self,
        path: &BoundaryPath,
        flags: i32,
        has_boundary_handles: bool,
    ) {
        let is_polyline = (flags & 2) != 0;

        if !is_polyline {
            // Edges
            self.writer.write_bit_long(path.edges.len() as i32);
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Line(le) => {
                        self.writer.write_byte(1);
                        self.writer.write_2raw_double(le.start);
                        self.writer.write_2raw_double(le.end);
                    }
                    BoundaryEdge::CircularArc(ca) => {
                        self.writer.write_byte(2);
                        self.writer.write_2raw_double(ca.center);
                        self.writer.write_bit_double(ca.radius);
                        self.writer.write_bit_double(ca.start_angle);
                        self.writer.write_bit_double(ca.end_angle);
                        self.writer.write_bit(ca.counter_clockwise);
                    }
                    BoundaryEdge::EllipticArc(ea) => {
                        self.writer.write_byte(3);
                        self.writer.write_2raw_double(ea.center);
                        self.writer.write_2raw_double(ea.major_axis_endpoint);
                        self.writer.write_bit_double(ea.minor_axis_ratio);
                        self.writer.write_bit_double(ea.start_angle);
                        self.writer.write_bit_double(ea.end_angle);
                        self.writer.write_bit(ea.counter_clockwise);
                    }
                    BoundaryEdge::Spline(se) => {
                        self.writer.write_byte(4);
                        self.writer.write_bit_long(se.degree as i32);
                        self.writer.write_bit(se.rational);
                        self.writer.write_bit(se.periodic);

                        self.writer.write_bit_long(se.knots.len() as i32);
                        self.writer.write_bit_long(se.control_points.len() as i32);
                        for k in &se.knots {
                            self.writer.write_bit_double(*k);
                        }
                        for pt in &se.control_points {
                            // Control points are 2D in hatch boundary splines
                            self.writer.write_2raw_double(Vector2::new(pt.x, pt.y));
                            if se.rational {
                                // Weight stored in Z
                                self.writer.write_bit_double(pt.z);
                            }
                        }

                        // Fit data — R2010+ only
                        if self.version.r2010_plus() {
                            self.writer.write_bit_long(se.fit_points.len() as i32);
                            if !se.fit_points.is_empty() {
                                for pt in &se.fit_points {
                                    self.writer.write_2raw_double(*pt);
                                }

                                self.writer.write_2raw_double(se.start_tangent);
                                self.writer.write_2raw_double(se.end_tangent);
                            }
                        }
                    }
                    BoundaryEdge::Polyline(pe) => {
                        // Polyline edges should use polyline flag path
                        self.writer.write_byte(1);
                        // Simplified: write as line segments
                        for (i, _v) in pe.vertices.iter().enumerate() {
                            if i + 1 < pe.vertices.len() {
                                let s = pe.vertices[i];
                                let e = pe.vertices[i + 1];
                                self.writer.write_2raw_double(Vector2::new(s.x, s.y));
                                self.writer.write_2raw_double(Vector2::new(e.x, e.y));
                            }
                        }
                    }
                }
            }
        } else {
            // Polyline boundary path
            // Find the polyline edge
            if let Some(BoundaryEdge::Polyline(pe)) = path.edges.first() {
                let has_bulge = pe.vertices.iter().any(|v| v.z != 0.0); // z stores bulge
                self.writer.write_bit(has_bulge);
                self.writer.write_bit(pe.is_closed);
                self.writer.write_bit_long(pe.vertices.len() as i32);
                for v in &pe.vertices {
                    self.writer.write_2raw_double(Vector2::new(v.x, v.y));
                    if has_bulge {
                        self.writer.write_bit_double(v.z); // bulge
                    }
                }
            } else {
                self.writer.write_bit(false);
                self.writer.write_bit(false);
                self.writer.write_bit_long(0);
            }
        }

        // Boundary object count
        if has_boundary_handles {
            self.writer
                .write_bit_long(path.boundary_handles.len() as i32);
        }
    }

    // â”€â”€ Viewport entity â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_viewport_entity(&mut self, e: &Viewport) {
        self.entity_preamble(common::OBJ_VIEWPORT, &e.common);

        // Center 3BD 10
        self.writer.write_3bit_double(e.center);
        // Width BD 40
        self.writer.write_bit_double(e.width);
        // Height BD 41
        self.writer.write_bit_double(e.height);

        // View data (written for all versions)
        self.writer.write_3bit_double(e.view_target);
        self.writer.write_3bit_double(e.view_direction);
        self.writer.write_bit_double(e.twist_angle);
        self.writer.write_bit_double(e.view_height);
        self.writer.write_bit_double(e.lens_length);
        self.writer.write_bit_double(e.front_clip_z);
        self.writer.write_bit_double(e.back_clip_z);
        self.writer.write_bit_double(e.snap_angle);
        self.writer
            .write_2raw_double(Vector2::new(e.view_center.x, e.view_center.y));
        self.writer
            .write_2raw_double(Vector2::new(e.snap_base.x, e.snap_base.y));
        self.writer
            .write_2raw_double(Vector2::new(e.snap_spacing.x, e.snap_spacing.y));
        self.writer
            .write_2raw_double(Vector2::new(e.grid_spacing.x, e.grid_spacing.y));
        // Circle Zoom BS 72
        self.writer.write_bit_short(e.circle_sides);

        // R2007+: Grid Major BS 61
        if self.version.r2007_plus() {
            self.writer
                .write_bit_short(if e.grid_major > 0 { e.grid_major } else { 5 });
        }

        // Status/UCS data (written for all versions)
        // Frozen layer count BL
        self.writer.write_bit_long(e.frozen_layers.len() as i32);
        // Status flags BL 90
        self.writer.write_bit_long(e.status.to_bits());
        // Style Sheet TV 1
        self.writer.write_variable_text(&e.style_sheet);
        // Render Mode RC 281
        self.writer.write_byte(e.render_mode as u8);
        // UCS at origin B 74
        self.writer.write_bit(e.ucs_at_origin);
        // UCS per viewport B 71
        self.writer.write_bit(e.ucs_per_viewport);
        // UCS Origin 3BD 110
        self.writer.write_3bit_double(e.ucs_origin);
        // UCS X Axis 3BD 111
        self.writer.write_3bit_double(e.ucs_x_axis);
        // UCS Y Axis 3BD 112
        self.writer.write_3bit_double(e.ucs_y_axis);
        // UCS Elevation BD 146
        self.writer.write_bit_double(e.elevation);
        // UCS Ortho View Type BS 79
        self.writer.write_bit_short(e.ucs_ortho_type);

        // R2004+: ShadePlot Mode BS 170
        if self.version.r2004_plus() {
            self.writer.write_bit_short(e.shade_plot_mode);
        }

        // R2007+: lighting + ambient
        if self.version.r2007_plus() {
            self.writer.write_bit(e.default_lighting);
            self.writer.write_byte(e.default_lighting_type as u8);
            self.writer.write_bit_double(e.brightness);
            self.writer.write_bit_double(e.contrast);
            self.writer.write_cm_color(&e.ambient_color);
        }

        // Frozen layer handles (written for all versions)
        for h in &e.frozen_layers {
            if self.version.r2004_plus() {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, h.value());
            } else {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, h.value());
            }
        }

        // Clip boundary handle (hard pointer)
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            e.clip_boundary_handle.value(),
        );

        // R2000 (AC1015) only: VIEWPORT ENT HEADER
        if self.version == crate::io::dwg::dwg_version::DwgVersion::AC15 {
            self.writer.write_handle(DwgReferenceType::HardPointer, 0);
        }

        // Named UCS and Base UCS handles (written for all versions)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, e.ucs_handle.value());
        self.writer
            .write_handle(DwgReferenceType::HardPointer, e.base_ucs_handle.value());

        // R2007+: 4 additional handles
        if self.version.r2007_plus() {
            // Background (soft pointer)
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, e.background_handle.value());
            // Visual Style (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, e.visual_style_handle.value());
            // Shadeplot ID (soft pointer)
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, e.shade_plot_handle.value());
            // Sun (hard owner)
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, e.sun_handle.value());
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ Dimension (dispatch) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_dimension(&mut self, dim: &Dimension) {
        match dim {
            Dimension::Linear(d) => self.write_dimension_linear(d),
            Dimension::Aligned(d) => self.write_dimension_aligned(d),
            Dimension::Radius(d) => self.write_dimension_radius(d),
            Dimension::Diameter(d) => self.write_dimension_diameter(d),
            Dimension::Angular2Ln(d) => self.write_dimension_angular_2ln(d),
            Dimension::Angular3Pt(d) => self.write_dimension_angular_3pt(d),
            Dimension::Ordinate(d) => self.write_dimension_ordinate(d),
            Dimension::Arc(d) => self.write_dimension_arc(d),
            Dimension::LargeRadial(d) => self.write_dimension_large_radial(d),
        }
    }

    /// Write the common dimension data shared by all dimension types.
    fn write_common_dimension_data(&mut self, type_code: i16, base: &DimensionBase) {
        self.entity_preamble(type_code, &base.common);

        // R2010+: Version RC 280
        if self.version.r2010_plus() {
            self.writer.write_byte(base.version);
        }

        // Extrusion 3BD 210
        self.writer.write_3bit_double(base.normal);
        // Text midpt 2RD 11
        self.writer.write_2raw_double(Vector2::new(
            base.text_middle_point.x,
            base.text_middle_point.y,
        ));
        // Elevation BD 11 Z-coord
        self.writer.write_bit_double(base.text_middle_point.z);

        // Flags byte — bit 0: text positioned at a user-defined location.
        let flags_byte = if base.text_user_positioned {
            base.dwg_flags_byte | 0x01
        } else {
            base.dwg_flags_byte & !0x01
        };
        self.writer.write_byte(flags_byte);

        // User text TV 1
        self.writer
            .write_variable_text(base.text_override().unwrap_or(""));

        // Text rot BD 53
        self.writer.write_bit_double(base.text_rotation);
        // Horiz dir BD 51
        self.writer.write_bit_double(base.horizontal_direction);

        self.writer.write_3bit_double(base.insertion_scale);
        self.writer.write_bit_double(base.insertion_rotation);

        // R2000+:
        if self.version.r2000_plus() {
            // Attachment Point BS 71
            self.writer.write_bit_short(base.attachment_point as i16);
            // Linespacing Style BS 72
            self.writer.write_bit_short(base.line_spacing_style);
            // Linespacing Factor BD 41
            self.writer.write_bit_double(base.line_spacing_factor);
            // Actual Measurement BD 42
            self.writer.write_bit_double(base.actual_measurement);
        }

        // R2007+:
        if self.version.r2007_plus() {
            self.writer.write_bit(base.dwg_unknown_bit);
            self.writer.write_bit(base.flip_arrow1);
            self.writer.write_bit(base.flip_arrow2);
        }

        // 12-pt 2RD 12
        self.writer
            .write_2raw_double(Vector2::new(base.insertion_point.x, base.insertion_point.y));

        // Dim style handle (hard pointer)
        let ds_handle = self
            .document
            .dim_styles
            .get(&base.style_name)
            .map(|d| d.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ds_handle.value());

        // Block handle (hard pointer)
        let block_handle = self
            .document
            .block_records
            .get(&base.block_name)
            .map(|br| br.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, block_handle.value());
    }

    fn write_dimension_linear(&mut self, d: &DimensionLinear) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_LINEAR, &d.base);
        self.writer.write_3bit_double(d.first_point);
        self.writer.write_3bit_double(d.second_point);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_bit_double(d.ext_line_rotation);
        self.writer.write_bit_double(d.rotation);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_aligned(&mut self, d: &DimensionAligned) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_ALIGNED, &d.base);
        self.writer.write_3bit_double(d.first_point);
        self.writer.write_3bit_double(d.second_point);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_bit_double(d.ext_line_rotation);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_radius(&mut self, d: &DimensionRadius) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_RADIUS, &d.base);
        self.writer.write_3bit_double(d.angle_vertex);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_bit_double(d.leader_length);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_diameter(&mut self, d: &DimensionDiameter) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_DIAMETER, &d.base);
        self.writer.write_3bit_double(d.angle_vertex);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_bit_double(d.leader_length);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_angular_2ln(&mut self, d: &DimensionAngular2Ln) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_ANG_2LN, &d.base);
        self.writer
            .write_2raw_double(Vector2::new(d.dimension_arc.x, d.dimension_arc.y));
        self.writer.write_3bit_double(d.first_point);
        self.writer.write_3bit_double(d.second_point);
        self.writer.write_3bit_double(d.angle_vertex);
        self.writer.write_3bit_double(d.definition_point);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_angular_3pt(&mut self, d: &DimensionAngular3Pt) {
        self.write_common_dimension_data(common::OBJ_DIMENSION_ANG_3PT, &d.base);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_3bit_double(d.first_point);
        self.writer.write_3bit_double(d.second_point);
        self.writer.write_3bit_double(d.angle_vertex);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_ordinate(&mut self, d: &DimensionOrdinate) {
        let mut base = d.base.clone();
        base.actual_measurement = d.measurement();
        self.write_common_dimension_data(common::OBJ_DIMENSION_ORDINATE, &base);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_3bit_double(d.feature_location);
        self.writer.write_3bit_double(d.leader_endpoint);
        // Ordinate type: 1 = X, 0 = Y
        self.writer
            .write_byte(if d.is_ordinate_type_x { 1 } else { 0 });
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_arc(&mut self, d: &DimensionArc) {
        let type_code = self.class_type_code("ARC_DIMENSION", common::OBJ_ARC_DIMENSION);
        self.write_common_dimension_data(type_code, &d.base);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_3bit_double(d.first_extension_point);
        self.writer.write_3bit_double(d.second_extension_point);
        self.writer.write_3bit_double(d.center_point);
        self.writer.write_bit(d.is_partial);
        self.writer.write_bit_double(d.arc_start_parameter);
        self.writer.write_bit_double(d.arc_end_parameter);
        self.writer.write_bit(d.has_leader);
        self.writer.write_3bit_double(d.first_leader_point);
        self.writer.write_3bit_double(d.second_leader_point);
        self.register_object(d.base.common.handle);
    }

    fn write_dimension_large_radial(&mut self, d: &DimensionLargeRadial) {
        let type_code =
            self.class_type_code("LARGE_RADIAL_DIMENSION", common::OBJ_LARGE_RADIAL_DIMENSION);
        self.write_common_dimension_data(type_code, &d.base);
        self.writer.write_3bit_double(d.definition_point);
        self.writer.write_3bit_double(d.chord_point);
        self.writer.write_bit_double(d.jog_angle);
        self.writer.write_3bit_double(d.override_center);
        self.writer.write_3bit_double(d.jog_point);
        self.register_object(d.base.common.handle);
    }

    // â”€â”€ Polyline2D â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_polyline2d(&mut self, e: &Polyline2D) {
        self.entity_preamble(common::OBJ_POLYLINE_2D, &e.common);

        self.writer.write_bit_short(e.flags.bits() as i16);
        self.writer.write_bit_short(e.smooth_surface as i16); // BS 75 curve type
        self.writer.write_bit_double(e.start_width);
        self.writer.write_bit_double(e.end_width);
        self.writer.write_bit_thickness(e.thickness);
        self.writer.write_bit_double(e.elevation);
        self.writer.write_bit_extrusion(e.normal);

        // Allocate handles for vertices and seqend
        let vertex_handles: Vec<Handle> =
            (0..e.vertices.len()).map(|_| self.alloc_handle()).collect();
        let seqend_handle = self.alloc_handle();

        if self.version.r2004_plus() {
            self.writer.write_bit_long(e.vertices.len() as i32);
        }

        // Vertex handles
        if self.version.r13_15_only() {
            let first = vertex_handles.first().copied().unwrap_or(Handle::NULL);
            let last = vertex_handles.last().copied().unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, first.value());
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, last.value());
        } else if self.version.r2004_plus() {
            for &vh in &vertex_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, vh.value());
            }
        }

        // Seqend handle
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, seqend_handle.value());

        self.register_object(e.common.handle);

        // Write vertices as child entities — set up internal entity chain
        let saved_prev = self.prev_handle.take();
        let saved_next = self.next_handle.take();

        for (i, (v, &vh)) in e.vertices.iter().zip(vertex_handles.iter()).enumerate() {
            self.prev_handle = if i > 0 {
                Some(vertex_handles[i - 1])
            } else {
                None
            };
            self.next_handle = (i + 1 < vertex_handles.len()).then(|| vertex_handles[i + 1]);
            self.write_vertex2d(v, vh, e.common.handle, &e.common.layer, &e.common.color);
        }

        // Write SEQEND — last in polyline chain
        self.prev_handle = None;
        self.next_handle = None;
        self.write_common_entity_data(
            common::OBJ_SEQEND,
            seqend_handle,
            e.common.handle,
            &e.common.layer,
            &e.common.color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );
        self.register_object(seqend_handle);

        // Restore block-level entity chain
        self.prev_handle = saved_prev;
        self.next_handle = saved_next;
    }

    fn write_vertex2d(
        &mut self,
        v: &Vertex2D,
        vertex_handle: Handle,
        owner: Handle,
        parent_layer: &str,
        parent_color: &crate::types::Color,
    ) {
        self.write_common_entity_data(
            common::OBJ_VERTEX_2D,
            vertex_handle,
            owner,
            parent_layer,
            parent_color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );

        // Flags EC 70 NOT bit-pair-coded
        self.writer.write_byte(v.flags.bits() as u8);

        // Point 3BD 10; real files may carry per-vertex Z.
        self.writer.write_bit_double(v.location.x);
        self.writer.write_bit_double(v.location.y);
        self.writer.write_bit_double(v.location.z);

        // Start width BD 40 — negative = compression trick
        if v.start_width != 0.0 && v.end_width == v.start_width {
            self.writer.write_bit_double(-v.start_width);
        } else {
            self.writer.write_bit_double(v.start_width);
            // End width BD 41 — only present if start >= 0
            self.writer.write_bit_double(v.end_width);
        }

        // Bulge BD 42
        self.writer.write_bit_double(v.bulge);

        // R2010+: Vertex ID BL 91
        if self.version.r2010_plus() {
            self.writer.write_bit_long(v.id);
        }

        // Tangent dir BD 50
        self.writer.write_bit_double(v.curve_tangent);

        self.register_object(vertex_handle);
    }

    // â”€â”€ Polyline3D â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_polyline3d(&mut self, e: &Polyline3D) {
        self.entity_preamble(common::OBJ_POLYLINE_3D, &e.common);

        // Byte 1: smooth surface type (C# hardcodes 0)
        self.writer.write_byte(e.smooth_type as u8);
        // Byte 2: closed flag only — bit 3 (Is3DPolyline) is implied by
        // the object type code and must NOT be written in the DWG data
        let closed_flag = if e.flags.closed { 1u8 } else { 0u8 };
        self.writer.write_byte(closed_flag);

        // Allocate handles for any vertex that doesn't have one
        let vertex_handles: Vec<Handle> = e
            .vertices
            .iter()
            .map(|v| {
                if v.handle.is_null() {
                    self.alloc_handle()
                } else {
                    v.handle
                }
            })
            .collect();
        let seqend_handle = self.alloc_handle();

        if self.version.r2004_plus() {
            self.writer.write_bit_long(e.vertices.len() as i32);
        }

        // Vertex handles
        if self.version.r13_15_only() {
            let first = vertex_handles.first().copied().unwrap_or(Handle::NULL);
            let last = vertex_handles.last().copied().unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, first.value());
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, last.value());
        } else if self.version.r2004_plus() {
            for &vh in &vertex_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, vh.value());
            }
        }

        // Seqend
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, seqend_handle.value());

        self.register_object(e.common.handle);

        // Write vertices — set up internal entity chain
        let saved_prev = self.prev_handle.take();
        let saved_next = self.next_handle.take();

        for (i, (v, &vh)) in e.vertices.iter().zip(vertex_handles.iter()).enumerate() {
            self.prev_handle = if i > 0 {
                Some(vertex_handles[i - 1])
            } else {
                None
            };
            self.next_handle = (i + 1 < vertex_handles.len()).then(|| vertex_handles[i + 1]);
            self.write_vertex3d(v, vh, e.common.handle, &e.common.layer, &e.common.color);
        }

        // Write SEQEND — last in polyline chain
        self.prev_handle = None;
        self.next_handle = None;
        self.write_common_entity_data(
            common::OBJ_SEQEND,
            seqend_handle,
            e.common.handle,
            &e.common.layer,
            &e.common.color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );
        self.register_object(seqend_handle);

        // Restore block-level entity chain
        self.prev_handle = saved_prev;
        self.next_handle = saved_next;
    }

    fn write_vertex3d(
        &mut self,
        v: &Vertex3DPolyline,
        vertex_handle: Handle,
        owner: Handle,
        parent_layer: &str,
        parent_color: &crate::types::Color,
    ) {
        self.write_common_entity_data(
            common::OBJ_VERTEX_3D,
            vertex_handle,
            owner,
            parent_layer,
            parent_color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );

        self.writer.write_byte(v.flags as u8); // Flags EC 70
        self.writer.write_3bit_double(v.position);
        self.register_object(vertex_handle);
    }

    // â”€â”€ PolyfaceMesh â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_polyface_mesh(&mut self, e: &PolyfaceMesh) {
        self.entity_preamble(common::OBJ_POLYLINE_PFACE, &e.common);

        self.writer.write_bit_short(e.vertices.len() as i16);
        self.writer.write_bit_short(e.faces.len() as i16);

        // Allocate handles for vertices and faces that don't have one
        let vertex_handles: Vec<Handle> = e
            .vertices
            .iter()
            .map(|v| {
                if v.common.handle.is_null() {
                    self.alloc_handle()
                } else {
                    v.common.handle
                }
            })
            .collect();
        let face_handles: Vec<Handle> = e
            .faces
            .iter()
            .map(|f| {
                if f.common.handle.is_null() {
                    self.alloc_handle()
                } else {
                    f.common.handle
                }
            })
            .collect();
        let seqend_handle = e
            .seqend_handle
            .filter(|h| !h.is_null())
            .unwrap_or_else(|| self.alloc_handle());

        let total_owned = e.vertices.len() + e.faces.len();

        if self.version.r2004_plus() {
            self.writer.write_bit_long(total_owned as i32);
        }

        if self.version.r13_15_only() {
            let first = vertex_handles
                .first()
                .or_else(|| face_handles.first())
                .copied()
                .unwrap_or(Handle::NULL);
            let last = face_handles
                .last()
                .or_else(|| vertex_handles.last())
                .copied()
                .unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, first.value());
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, last.value());
        } else if self.version.r2004_plus() {
            for &vh in &vertex_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, vh.value());
            }
            for &fh in &face_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, fh.value());
            }
        }

        // Seqend
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, seqend_handle.value());

        self.register_object(e.common.handle);

        // Build combined sub-entity handle chain for prev/next linking
        let saved_prev = self.prev_handle.take();
        let saved_next = self.next_handle.take();

        let mut all_sub_handles: Vec<Handle> = Vec::with_capacity(total_owned);
        all_sub_handles.extend_from_slice(&vertex_handles);
        all_sub_handles.extend_from_slice(&face_handles);

        let mut sub_idx = 0usize;

        // Write vertex child entities (OBJ_VERTEX_PFACE = 13)
        for (v, &vh) in e.vertices.iter().zip(vertex_handles.iter()) {
            self.prev_handle = if sub_idx > 0 {
                Some(all_sub_handles[sub_idx - 1])
            } else {
                None
            };
            self.next_handle = if sub_idx + 1 < all_sub_handles.len() {
                Some(all_sub_handles[sub_idx + 1])
            } else {
                None
            };
            // Use vertex's own entity common (owner forced to polyface mesh)
            let mut vc = v.common.clone();
            vc.handle = vh;
            vc.owner_handle = e.common.handle;
            self.entity_preamble(common::OBJ_VERTEX_PFACE, &vc);
            self.writer.write_byte(v.flags.bits() as u8);
            self.writer.write_3bit_double(v.location);
            self.register_object(vh);
            sub_idx += 1;
        }

        // Write face child entities (OBJ_VERTEX_PFACE_FACE = 14)
        for (f, &fh) in e.faces.iter().zip(face_handles.iter()) {
            self.prev_handle = if sub_idx > 0 {
                Some(all_sub_handles[sub_idx - 1])
            } else {
                None
            };
            self.next_handle = if sub_idx + 1 < all_sub_handles.len() {
                Some(all_sub_handles[sub_idx + 1])
            } else {
                None
            };
            // Use face's own entity common (owner forced to polyface mesh)
            let mut fc = f.common.clone();
            fc.handle = fh;
            fc.owner_handle = e.common.handle;
            self.entity_preamble(common::OBJ_VERTEX_PFACE_FACE, &fc);
            self.writer.write_bit_short(f.index1);
            self.writer.write_bit_short(f.index2);
            self.writer.write_bit_short(f.index3);
            self.writer.write_bit_short(f.index4);
            self.register_object(fh);
            sub_idx += 1;
        }

        // Write SEQEND — last in polyface chain
        self.prev_handle = None;
        self.next_handle = None;
        self.write_common_entity_data(
            common::OBJ_SEQEND,
            seqend_handle,
            e.common.handle,
            &e.common.layer,
            &e.common.color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );
        self.register_object(seqend_handle);

        // Restore block-level entity chain
        self.prev_handle = saved_prev;
        self.next_handle = saved_next;
    }

    // â”€â”€ PolygonMesh â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_polygon_mesh(&mut self, e: &PolygonMeshEntity) {
        self.entity_preamble(common::OBJ_POLYLINE_MESH, &e.common);

        self.writer.write_bit_short(e.flags.bits() as i16);
        self.writer.write_bit_short(e.smooth_type as i16);
        self.writer.write_bit_short(e.m_vertex_count);
        self.writer.write_bit_short(e.n_vertex_count);
        self.writer.write_bit_short(e.m_smooth_density);
        self.writer.write_bit_short(e.n_smooth_density);

        // Allocate handles for vertices that don't have one
        let vertex_handles: Vec<Handle> = e
            .vertices
            .iter()
            .map(|v| {
                if v.common.handle.is_null() {
                    self.alloc_handle()
                } else {
                    v.common.handle
                }
            })
            .collect();
        let seqend_handle = self.alloc_handle();

        if self.version.r2004_plus() {
            self.writer.write_bit_long(e.vertices.len() as i32);
        }

        if self.version.r13_15_only() {
            let first = vertex_handles.first().copied().unwrap_or(Handle::NULL);
            let last = vertex_handles.last().copied().unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, first.value());
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, last.value());
        } else if self.version.r2004_plus() {
            for &vh in &vertex_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, vh.value());
            }
        }

        // Seqend
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, seqend_handle.value());

        self.register_object(e.common.handle);

        // Write vertex child entities (OBJ_VERTEX_MESH = 12) with internal chain
        let saved_prev = self.prev_handle.take();
        let saved_next = self.next_handle.take();

        for (i, (v, &vh)) in e.vertices.iter().zip(vertex_handles.iter()).enumerate() {
            self.prev_handle = if i > 0 {
                Some(vertex_handles[i - 1])
            } else {
                None
            };
            self.next_handle = (i + 1 < vertex_handles.len()).then(|| vertex_handles[i + 1]);
            self.write_common_entity_data(
                common::OBJ_VERTEX_MESH,
                vh,
                e.common.handle,
                &e.common.layer,
                &e.common.color,
                &crate::types::LineWeight::ByLayer,
                &crate::types::Transparency::default(),
                false,
                1.0,
                "ByLayer",
                &None,
                &crate::xdata::ExtendedData::default(),
                &[],
                &None,
                None,
                None,
                0,
                &None,
                0,
                0,
                &None,
                &None,
                &None,
                &None,
                &None,
            );
            self.writer.write_byte(v.flags as u8);
            self.writer.write_3bit_double(v.location);
            self.register_object(vh);
        }

        // Write SEQEND — last in polygon mesh chain
        self.prev_handle = None;
        self.next_handle = None;
        self.write_common_entity_data(
            common::OBJ_SEQEND,
            seqend_handle,
            e.common.handle,
            &e.common.layer,
            &e.common.color,
            &crate::types::LineWeight::ByLayer,
            &crate::types::Transparency::default(),
            false,
            1.0,
            "ByLayer",
            &None,
            &crate::xdata::ExtendedData::default(),
            &[],
            &None,
            None,
            None,
            0,
            &None,
            0,
            0,
            &None,
            &None,
            &None,
            &None,
            &None,
        );
        self.register_object(seqend_handle);

        // Restore block-level entity chain
        self.prev_handle = saved_prev;
        self.next_handle = saved_next;
    }

    // â”€â”€ Seqend â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_seqend(&mut self, e: &Seqend) {
        self.entity_preamble(common::OBJ_SEQEND, &e.common);
        self.register_object(e.common.handle);
    }

    // â”€â”€ Mesh (ACAD_MESH) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_mesh(&mut self, e: &Mesh) {
        // UNLISTED entity type — always use DXF class number (500+)
        let type_code = self.class_type_code("MESH", common::OBJ_MESH);
        self.entity_preamble(type_code, &e.common);

        // 71 BS Version
        self.writer.write_bit_short(e.version);
        // 72 B BlendCrease (BIT, not byte!)
        self.writer.write_bit(e.blend_crease);
        // 91 BL SubdivisionLevel
        self.writer.write_bit_long(e.subdivision_level);

        // 92 BL nvertices
        self.writer.write_bit_long(e.vertices.len() as i32);
        for v in &e.vertices {
            // 10 3BD vertice
            self.writer.write_3bit_double(*v);
        }

        // Faces: count = sum of (1 + face.vertices.len()) for each face
        let nfaces: i32 = e.faces.iter().map(|f| 1 + f.vertices.len() as i32).sum();
        self.writer.write_bit_long(nfaces);
        for face in &e.faces {
            self.writer.write_bit_long(face.vertices.len() as i32);
            for idx in &face.vertices {
                self.writer.write_bit_long(*idx as i32);
            }
        }

        // Edges
        self.writer.write_bit_long(e.edges.len() as i32);
        for edge in &e.edges {
            self.writer.write_bit_long(edge.start as i32);
            self.writer.write_bit_long(edge.end as i32);
        }

        // Crease values: must write for EVERY edge, use 0 if no crease
        self.writer.write_bit_long(e.edges.len() as i32);
        for edge in &e.edges {
            let crease = edge.crease.unwrap_or(0.0);
            self.writer.write_bit_double(crease);
        }

        // Trailing value (override option for meshes)
        self.writer.write_bit_long(e.override_option);

        self.register_object(e.common.handle);
    }

    // â”€â”€ MLine â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_mline(&mut self, e: &MLine) {
        self.entity_preamble(common::OBJ_MLINE, &e.common);

        self.writer.write_bit_double(e.scale_factor);
        self.writer.write_byte(e.justification as u8);
        self.writer.write_3bit_double(e.start_point);
        self.writer.write_3bit_double(e.normal);

        // Openclosed BS: open (1), closed (3) — always has HAS_VERTICES flag
        let flag_value: i16 = if e.flags.contains(MLineFlags::CLOSED) {
            3
        } else {
            1
        };
        self.writer.write_bit_short(flag_value);

        // Linesinstyle RC 73 — number of segments from first vertex
        let nlines: u8 = if let Some(first_v) = e.vertices.first() {
            first_v.segments.len() as u8
        } else {
            e.style_element_count as u8
        };
        self.writer.write_byte(nlines);

        // Vertices
        self.writer.write_bit_short(e.vertices.len() as i16);
        for v in &e.vertices {
            self.writer.write_3bit_double(v.position);
            self.writer.write_3bit_double(v.direction);
            self.writer.write_3bit_double(v.miter);

            for seg in &v.segments {
                self.writer.write_bit_short(seg.parameters.len() as i16);
                for p in &seg.parameters {
                    self.writer.write_bit_double(*p);
                }
                self.writer
                    .write_bit_short(seg.area_fill_parameters.len() as i16);
                for p in &seg.area_fill_parameters {
                    self.writer.write_bit_double(*p);
                }
            }
        }

        // MLine style handle — fall back to document's current MLine style
        let sh = e
            .style_handle
            .filter(|h| !h.is_null())
            .unwrap_or(self.document.header.current_multiline_style_handle);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, sh.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Underlay (PDF / DWF / DGN) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Write an UNDERLAY reference (AcDbUnderlayReference).
    ///
    // ── Table (ACAD_TABLE) ──────────────────────────────────────────
    //
    // Inverse of the table reader. Cell styles / borders acadrust does not
    // model are written as empty presence-flag stubs (the anonymous block
    // renders the visual), and the retained data — dimensions, cell contents
    // (text/number) — is written in full so it round-trips.

    fn write_table_string_value(&mut self, s: &str) {
        if self.version.r2007_plus() {
            let mut bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            bytes.push(0);
            bytes.push(0); // trailing UTF-16 NUL
            self.writer.write_bit_long(bytes.len() as i32);
            self.writer.write_bytes(&bytes);
        } else {
            let bytes = self.writer.encode_legacy_text(s);
            self.writer
                .write_bit_long(bytes.len().saturating_add(1) as i32);
            self.writer.write_bytes(&bytes);
            self.writer.write_byte(0);
        }
    }

    pub(super) fn write_table_cad_value(&mut self, v: &CellValue) {
        self.write_table_cad_value_with_schema(v, self.version.r2007_plus());
    }

    fn write_table_cad_value_with_schema(&mut self, v: &CellValue, modern_schema: bool) {
        // Down-saved TABLECONTENT keeps modern type codes, but its framing
        // remains pre-R2007.
        if self.version.r2007_plus() {
            self.writer.write_bit_long(v.flags);
        }
        let code = if modern_schema {
            v.type_code()
        } else {
            v.type_code() & !0x200
        };
        self.writer.write_bit_long(code);
        if !self.version.r2007_plus() || (v.flags & 1) == 0 {
            match code {
                0 | 1 => self.writer.write_bit_long(v.numeric_value as i32),
                2 => self.writer.write_bit_double(v.numeric_value),
                4 => self.write_table_string_value(&v.text),
                8 => {
                    let size = if v.data_size > 0 && v.data_size as usize == v.binary_value.len() {
                        v.data_size
                    } else {
                        v.binary_value.len() as i32
                    };
                    self.writer.write_bit_long(size);
                    if size > 0 {
                        self.writer.write_bytes(&v.binary_value);
                    }
                }
                0x10 => {
                    self.writer
                        .write_bit_long(if v.data_size != 0 { v.data_size } else { 16 });
                    self.writer
                        .write_2raw_double(Vector2::new(v.point_value.x, v.point_value.y));
                }
                0x20 => {
                    self.writer
                        .write_bit_long(if v.data_size != 0 { v.data_size } else { 24 });
                    self.writer.write_raw_double(v.point_value.x);
                    self.writer.write_raw_double(v.point_value.y);
                    self.writer.write_raw_double(v.point_value.z);
                }
                0x40 => self.writer.write_handle(
                    DwgReferenceType::SoftPointer,
                    v.handle_value.map(|h| h.value()).unwrap_or(0),
                ),
                0x80 | 0x100 => {}
                0x200 => self.write_table_string_value(&v.text),
                _ => {}
            }
        }
        if self.version.r2007_plus() {
            let unit_code = v.unit_type_code();
            self.writer.write_bit_long(unit_code);
            self.writer.write_variable_text(&v.format);
            if unit_code != 12 {
                self.writer.write_variable_text(&v.formatted_value);
            }
        }
    }

    fn write_table_custom_data(&mut self, data: &TableCustomData) {
        self.writer.write_variable_text(&data.name);
        self.write_table_cad_value_with_schema(&data.value, true);
    }

    fn table_text_style_handle(&self, handle: Option<Handle>, name: &str) -> u64 {
        handle
            .filter(|value| !value.is_null())
            .or_else(|| {
                (!name.is_empty())
                    .then(|| self.document.text_styles.get(name))
                    .flatten()
                    .map(|style| style.handle)
            })
            .unwrap_or(Handle::NULL)
            .value()
    }

    fn write_table_content_format(&mut self, content: &CellContent) {
        self.writer.write_bit_long(content.format_override_flags);
        self.writer.write_bit_long(content.format_property_flags);
        self.writer.write_bit_long(content.format_value_data_type);
        self.writer.write_bit_long(content.format_value_unit_type);
        self.writer.write_variable_text(&content.value_format);
        self.writer.write_bit_double(content.rotation);
        self.writer.write_bit_double(content.scale);
        self.writer.write_bit_long(content.alignment);
        self.writer.write_cm_true_color(&content.color);
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            self.table_text_style_handle(content.text_style_handle, &content.text_style_name),
        );
        self.writer.write_bit_double(content.text_height);
    }

    fn write_table_style_content_format(&mut self, style: &CellStyle) {
        self.writer
            .write_bit_long(style.content_format_override_flags);
        self.writer.write_bit_long(style.content_property_flags);
        self.writer.write_bit_long(style.value_data_type);
        self.writer.write_bit_long(style.value_unit_type);
        self.writer.write_variable_text(&style.value_format);
        self.writer.write_bit_double(style.rotation);
        self.writer.write_bit_double(style.scale);
        self.writer.write_bit_long(style.alignment);
        self.writer.write_cm_true_color(&style.content_color);
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            self.table_text_style_handle(style.text_style_handle, &style.text_style_name),
        );
        self.writer.write_bit_double(style.text_height);
    }

    fn write_table_border(&mut self, border: &CellBorder) {
        self.writer
            .write_bit_long(border.override_flags.bits() as i32);
        self.writer.write_bit_long(border.border_type as i32);
        self.writer.write_cm_true_color(&border.color);
        self.writer
            .write_bit_long(border.line_weight.as_i16() as i32);
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            border.line_type_handle.map(|h| h.value()).unwrap_or(0),
        );
        self.writer.write_bit_long(border.invisible as i32);
        self.writer.write_bit_double(border.double_spacing);
    }

    fn write_table_cell_style(&mut self, style: Option<&CellStyle>) {
        let Some(style) = style else {
            self.writer.write_bit_long(0);
            self.writer.write_bit_short(0);
            return;
        };
        self.writer.write_bit_long(style.style_type as i32);
        self.writer.write_bit_short(1);
        self.writer.write_bit_long(style.override_flags);
        self.writer
            .write_bit_long(style.property_flags.bits() as i32);
        self.writer.write_cm_true_color(&style.background_color);
        self.writer.write_bit_long(style.layout_flags.bits() as i32);
        self.write_table_style_content_format(style);
        self.writer.write_bit_short(style.margin_override_flags);
        if style.margin_override_flags & 0x01 != 0 {
            self.writer.write_bit_double(style.margin_top);
            self.writer.write_bit_double(style.margin_left);
            self.writer.write_bit_double(style.margin_bottom);
            self.writer.write_bit_double(style.margin_right);
            self.writer.write_bit_double(style.horizontal_spacing);
            self.writer.write_bit_double(style.vertical_spacing);
        }
        let mut borders: Vec<(u32, &CellBorder)> = Vec::new();
        for (edge, border) in [
            (CellEdgeFlags::TOP, &style.top_border),
            (CellEdgeFlags::RIGHT, &style.right_border),
            (CellEdgeFlags::BOTTOM, &style.bottom_border),
            (CellEdgeFlags::LEFT, &style.left_border),
        ] {
            if style.applied_border_edges.contains(edge) {
                borders.push((edge.bits(), border));
            }
        }
        borders.extend(
            style
                .additional_borders
                .iter()
                .map(|(edge, border)| (*edge, border)),
        );
        self.writer.write_bit_long(borders.len() as i32);
        for (edge, border) in borders {
            self.writer.write_bit_long(edge as i32);
            self.write_table_border(border);
        }
    }

    fn write_table_geometry(&mut self, geometry: &CellContentGeometry) {
        self.writer.write_3bit_double(geometry.distance_to_top_left);
        self.writer.write_3bit_double(geometry.distance_to_center);
        self.writer.write_bit_double(geometry.width);
        self.writer.write_bit_double(geometry.height);
        self.writer.write_bit_double(geometry.outer_width);
        self.writer.write_bit_double(geometry.outer_height);
        self.writer.write_bit_long(geometry.flags);
    }

    fn write_table_cell_content(&mut self, content: &CellContent) {
        let ct: i32 = match content.content_type {
            TableCellContentType::Value => 1,
            TableCellContentType::Field => 2,
            TableCellContentType::Block => 4,
            _ => 0,
        };
        self.writer.write_bit_long(ct);
        match content.content_type {
            TableCellContentType::Value => {
                self.write_table_cad_value_with_schema(&content.value, true)
            }
            TableCellContentType::Field => self.writer.write_handle(
                DwgReferenceType::HardPointer,
                content.field_handle.map(|h| h.value()).unwrap_or(0),
            ),
            TableCellContentType::Block => self.writer.write_handle(
                DwgReferenceType::HardPointer,
                content.block_handle.map(|h| h.value()).unwrap_or(0),
            ),
            _ => {}
        }
        self.writer.write_bit_long(content.attributes.len() as i32);
        for attribute in &content.attributes {
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                attribute.definition_handle.value(),
            );
            self.writer.write_variable_text(&attribute.value);
            self.writer.write_bit_long(attribute.index);
        }
        let has_format = content.format_override_flags != 0
            || content.format_property_flags != 0
            || content.format_value_data_type != 0
            || content.format_value_unit_type != 0
            || content.alignment != 0
            || content.text_style_handle.is_some()
            || content.color != crate::types::Color::ByBlock
            || content.rotation != 0.0
            || content.scale != 1.0
            || content.text_height != 0.18
            || !content.value_format.is_empty();
        self.writer.write_bit_short(has_format as i16);
        if has_format {
            self.write_table_content_format(content);
        }
    }

    /// R2010+ inline cell.
    fn write_table_cell_r2010(&mut self, cell: &TableCell) {
        self.writer.write_bit_long(cell.state.bits() as i32);
        self.writer.write_variable_text(&cell.tooltip);
        self.writer.write_bit_long(cell.custom_data);
        self.writer
            .write_bit_long(cell.custom_data_items.len() as i32);
        for data in &cell.custom_data_items {
            self.write_table_custom_data(data);
        }
        self.writer.write_bit_long(cell.has_linked_data as i32);
        if cell.has_linked_data {
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                cell.data_link_handle.map(|h| h.value()).unwrap_or(0),
            );
            self.writer.write_bit_long(cell.data_link_rows);
            self.writer.write_bit_long(cell.data_link_columns);
            self.writer.write_bit_long(cell.data_link_unknown);
        }
        self.writer.write_bit_long(cell.contents.len() as i32);
        for content in &cell.contents {
            self.write_table_cell_content(content);
        }
        self.write_table_cell_style(cell.style.as_ref());
        self.writer.write_bit_long(cell.style_id);
        let geometries: Vec<CellContentGeometry> = if !cell.geometries.is_empty() {
            cell.geometries.clone()
        } else {
            let content_geometry: Vec<_> = cell
                .contents
                .iter()
                .filter_map(|content| content.geometry.clone())
                .collect();
            if !content_geometry.is_empty() {
                content_geometry
            } else {
                cell.geometry.iter().cloned().collect()
            }
        };
        let has_geometry = !geometries.is_empty()
            || cell.geometry_handle.is_some()
            || cell.geometry_data_flag != 0
            || cell.geometry_width_with_gap != 0.0
            || cell.geometry_height_with_gap != 0.0
            || cell.geometry_flags != 0
            || cell.flag != 0;
        self.writer.write_bit_long(has_geometry as i32);
        if has_geometry {
            self.writer.write_bit_long(cell.geometry_data_flag);
            self.writer.write_bit_double(cell.geometry_width_with_gap);
            self.writer.write_bit_double(cell.geometry_height_with_gap);
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                cell.geometry_handle.map(|h| h.value()).unwrap_or(0),
            );
            self.writer.write_bit_long(geometries.len() as i32);
            for geometry in &geometries {
                self.write_table_geometry(geometry);
            }
        }
    }

    /// R2010+ AcDbLinkedTableData body.
    pub(super) fn write_table_content(&mut self, e: &table::Table) {
        self.writer.write_variable_text(&e.name);
        self.writer.write_variable_text(&e.description);
        self.writer.write_bit_long(e.columns.len() as i32);
        for col in &e.columns {
            self.writer.write_variable_text(&col.name);
            self.writer.write_bit_long(col.custom_data);
            self.writer
                .write_bit_long(col.custom_data_items.len() as i32);
            for data in &col.custom_data_items {
                self.write_table_custom_data(data);
            }
            self.write_table_cell_style(col.style.as_ref());
            self.writer.write_bit_long(col.style_id);
            self.writer.write_bit_double(col.width);
        }
        self.writer.write_bit_long(e.rows.len() as i32);
        for row in &e.rows {
            self.writer.write_bit_long(row.cells.len() as i32);
            for cell in &row.cells {
                self.write_table_cell_r2010(cell);
            }
            self.writer.write_bit_long(row.custom_data);
            self.writer
                .write_bit_long(row.custom_data_items.len() as i32);
            for data in &row.custom_data_items {
                self.write_table_custom_data(data);
            }
            self.write_table_cell_style(row.style.as_ref());
            self.writer.write_bit_long(row.style_id);
            self.writer.write_bit_double(row.height);
        }
        self.writer.write_bit_long(e.field_handles.len() as i32);
        for handle in &e.field_handles {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, handle.value());
        }
        self.write_table_cell_style(e.base_style.as_ref());
        self.writer.write_bit_long(e.merged_ranges.len() as i32);
        for range in &e.merged_ranges {
            self.writer.write_bit_long(range.top_row as i32);
            self.writer.write_bit_long(range.left_col as i32);
            self.writer.write_bit_long(range.bottom_row as i32);
            self.writer.write_bit_long(range.right_col as i32);
        }
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            e.table_style_handle.map(|h| h.value()).unwrap_or(0),
        );
    }

    /// Pre-R2010 flat-format cell.
    fn write_table_cell_data(&mut self, cell: &TableCell) {
        let cell_type = match cell.cell_type {
            CellType::Block => 2,
            CellType::Text if cell.contents.is_empty() && cell.value_handle.is_none() => 0,
            CellType::Text => 1,
        };
        self.writer.write_bit_short(cell_type);
        self.writer.write_byte(cell.edge_flags);
        self.writer.write_bit(cell.merged != 0);
        self.writer.write_bit(cell.auto_fit);
        self.writer.write_bit_long(cell.merge_width);
        self.writer.write_bit_long(cell.merge_height);
        self.writer.write_bit_double(cell.rotation);
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            cell.value_handle
                .or_else(|| cell.contents.first().and_then(|c| c.block_handle))
                .map(|h| h.value())
                .unwrap_or(0),
        );
        if cell_type == 1 {
            if !self.version.r2007_plus() && cell.value_handle.is_none() {
                self.writer.write_variable_text(cell.text_value());
            }
        } else if cell_type == 2 {
            self.writer.write_bit_double(cell.block_scale);
            let attributes = cell
                .contents
                .iter()
                .find(|content| content.content_type == TableCellContentType::Block)
                .map(|content| content.attributes.as_slice())
                .unwrap_or(&[]);
            self.writer.write_bit(!attributes.is_empty());
            if !attributes.is_empty() {
                self.writer.write_bit_short(attributes.len() as i16);
                for attribute in attributes {
                    self.writer.write_handle(
                        DwgReferenceType::SoftPointer,
                        attribute.definition_handle.value(),
                    );
                    self.writer.write_bit_short(attribute.index as i16);
                    self.writer.write_variable_text(&attribute.value);
                }
            }
        }
        self.writer.write_bit(cell.style.is_some());
        if let Some(style) = &cell.style {
            let flags = style.override_flags;
            self.writer.write_bit_long(flags);
            self.writer.write_byte(cell.virtual_edge as u8);
            if flags & 0x01 != 0 {
                self.writer.write_bit_short(style.alignment as i16);
            }
            if flags & 0x02 != 0 {
                self.writer.write_bit(!style.fill_enabled);
            }
            if flags & 0x04 != 0 {
                self.writer.write_cm_color(&style.background_color);
            }
            if flags & 0x08 != 0 {
                self.writer.write_cm_color(&style.content_color);
            }
            if flags & 0x10 != 0 {
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    self.table_text_style_handle(style.text_style_handle, &style.text_style_name),
                );
            }
            if flags & 0x20 != 0 {
                self.writer.write_bit_double(style.text_height);
            }
            for (color_bit, lw_bit, border) in [
                (0x40, 0x400, &style.top_border),
                (0x80, 0x800, &style.right_border),
                (0x100, 0x1000, &style.bottom_border),
                (0x200, 0x2000, &style.left_border),
            ] {
                if flags & color_bit != 0 {
                    self.writer.write_cm_color(&border.color);
                }
                if flags & lw_bit != 0 {
                    self.writer.write_bit_short(border.line_weight.as_i16());
                    self.writer.write_bit_short((!border.invisible) as i16);
                }
            }
        }
        if self.version.r2007_plus() {
            self.writer.write_bit_long(cell.flag);
            let empty = CellValue::new();
            let value = cell
                .contents
                .iter()
                .find(|content| content.content_type == TableCellContentType::Value)
                .or_else(|| cell.contents.first())
                .map(|content| &content.value)
                .unwrap_or(&empty);
            self.write_table_cad_value(value);
        }
    }

    fn write_legacy_table_style_override(&mut self, value: &LegacyTableStyleOverride) {
        let flags = value.flags;
        self.writer.write_bit_long(flags);
        if flags & 0x0001 != 0 {
            self.writer
                .write_bit(value.title_suppressed.unwrap_or(false));
        }
        if flags & 0x0002 != 0 {
            self.writer
                .write_bit(value.header_suppressed.unwrap_or(false));
        }
        if flags & 0x0004 != 0 {
            self.writer
                .write_bit_short(value.flow_direction.unwrap_or(0));
        }
        if flags & 0x0008 != 0 {
            self.writer
                .write_bit_double(value.horizontal_cell_margin.unwrap_or(0.0));
        }
        if flags & 0x0010 != 0 {
            self.writer
                .write_bit_double(value.vertical_cell_margin.unwrap_or(0.0));
        }
        let mut index = 0;
        for bit in [0x0020, 0x0040, 0x0080] {
            if flags & bit != 0 {
                let color = value
                    .row_colors
                    .get(index)
                    .cloned()
                    .unwrap_or(Color::ByBlock);
                self.writer.write_cm_color(&color);
                index += 1;
            }
        }
        index = 0;
        for bit in [0x0100, 0x0200, 0x0400] {
            if flags & bit != 0 {
                self.writer
                    .write_bit(value.row_fill_none.get(index).copied().unwrap_or(false));
                index += 1;
            }
        }
        index = 0;
        for bit in [0x0800, 0x1000, 0x2000] {
            if flags & bit != 0 {
                let color = value
                    .row_fill_colors
                    .get(index)
                    .cloned()
                    .unwrap_or(Color::ByBlock);
                self.writer.write_cm_color(&color);
                index += 1;
            }
        }
        index = 0;
        for bit in [0x4000, 0x8000, 0x10000] {
            if flags & bit != 0 {
                self.writer
                    .write_bit_short(value.row_alignments.get(index).copied().unwrap_or(0));
                index += 1;
            }
        }
        index = 0;
        for bit in [0x20000, 0x40000, 0x80000] {
            if flags & bit != 0 {
                let handle = value
                    .text_style_handles
                    .get(index)
                    .copied()
                    .filter(|handle| !handle.is_null())
                    .or_else(|| {
                        value.text_style_names.get(index).and_then(|name| {
                            self.document
                                .text_styles
                                .get(name)
                                .map(|style| style.handle)
                        })
                    })
                    .unwrap_or(Handle::NULL);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, handle.value());
                index += 1;
            }
        }
        index = 0;
        for bit in [0x100000, 0x200000, 0x400000] {
            if flags & bit != 0 {
                self.writer
                    .write_bit_double(value.row_heights.get(index).copied().unwrap_or(0.0));
                index += 1;
            }
        }
    }

    fn write_legacy_border_colors(&mut self, value: &LegacyBorderOverrides<Color>) {
        self.writer.write_bit_long(value.flags);
        let mut index = 0;
        for bit in 0..18 {
            if value.flags & (1 << bit) != 0 {
                let color = value.values.get(index).cloned().unwrap_or(Color::ByBlock);
                self.writer.write_cm_color(&color);
                index += 1;
            }
        }
    }

    fn write_legacy_border_line_weights(&mut self, value: &LegacyBorderOverrides<LineWeight>) {
        self.writer.write_bit_long(value.flags);
        let mut index = 0;
        for bit in 0..18 {
            if value.flags & (1 << bit) != 0 {
                self.writer.write_bit_short(
                    value
                        .values
                        .get(index)
                        .copied()
                        .unwrap_or(LineWeight::ByLayer)
                        .as_i16(),
                );
                index += 1;
            }
        }
    }

    fn write_legacy_border_visibility(&mut self, value: &LegacyBorderOverrides<bool>) {
        self.writer.write_bit_long(value.flags);
        let mut index = 0;
        for bit in 0..18 {
            if value.flags & (1 << bit) != 0 {
                self.writer
                    .write_bit_short(value.values.get(index).copied().unwrap_or(false) as i16);
                index += 1;
            }
        }
    }

    fn write_table(&mut self, e: &table::Table) {
        let type_code = self.class_type_code("ACAD_TABLE", common::OBJ_TABLE);
        self.entity_preamble(type_code, &e.common);

        // Insert base (mirrors read_insert; tables carry no attributes).
        self.writer.write_3bit_double(e.insertion_point);
        if self.version.r13_14_only() {
            self.writer.write_bit_double(1.0);
            self.writer.write_bit_double(1.0);
            self.writer.write_bit_double(1.0);
        }
        if self.version.r2000_plus() {
            self.writer.write_2bits(3); // scale (1,1,1), no data
        }
        self.writer.write_bit_double(0.0); // rotation
        self.writer.write_3bit_double(e.normal);
        self.writer.write_bit(false); // has attributes
        let block_record_handle = e
            .block_record_handle
            .filter(|handle| !handle.is_null())
            .or_else(|| {
                (!e.block_name.is_empty())
                    .then(|| self.document.block_records.get(&e.block_name))
                    .flatten()
                    .map(|record| record.handle)
            })
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, block_record_handle.value());

        if self.version.r2010_plus() {
            self.writer.write_byte(e.dwg_unknown_byte);
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                e.dwg_unknown_handle.map(|h| h.value()).unwrap_or(0),
            );
            self.writer.write_bit_long(e.dwg_unknown_long1);
            if self.version.r2013_plus(self.dxf_version) {
                self.writer.write_bit_long(e.dwg_unknown_long2);
            } else {
                self.writer.write_bit(e.dwg_unknown_long2 != 0);
            }
            self.write_table_content(e);
            self.writer.write_bit_short(e.dwg_unknown_short);
            self.writer.write_3bit_double(e.horizontal_direction);
            let has_break_data =
                !e.break_options.is_empty() || !e.break_data.is_empty() || e.break_spacing != 0.0;
            self.writer.write_bit_long(has_break_data as i32);
            if has_break_data {
                self.writer.write_bit_long(e.break_options.bits() as i32);
                self.writer.write_bit_long(e.break_flow_direction as i32);
                self.writer.write_bit_double(e.break_spacing);
                self.writer.write_bit_long(0);
                self.writer.write_bit_long(0);
                self.writer.write_bit_long(e.break_data.len() as i32);
                for data in &e.break_data {
                    self.writer.write_3bit_double(data.position);
                    self.writer.write_bit_double(data.height);
                    self.writer.write_bit_long(data.flags);
                }
            }
            self.writer.write_bit_long(e.break_ranges.len() as i32);
            for range in &e.break_ranges {
                self.writer.write_3bit_double(range.position);
                self.writer.write_bit_long(range.start_row);
                self.writer.write_bit_long(range.end_row);
            }
        } else {
            self.writer.write_bit_short(e.value_flags as i16);
            self.writer.write_3bit_double(e.horizontal_direction);
            self.writer.write_bit_long(e.columns.len() as i32);
            self.writer.write_bit_long(e.rows.len() as i32);
            for col in &e.columns {
                self.writer.write_bit_double(col.width);
            }
            for row in &e.rows {
                self.writer.write_bit_double(row.height);
            }
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                e.table_style_handle.map(|h| h.value()).unwrap_or(0),
            );
            for row in &e.rows {
                for cell in &row.cells {
                    self.write_table_cell_data(cell);
                }
            }
            self.writer.write_bit(e.legacy_style_override.is_some());
            if let Some(value) = &e.legacy_style_override {
                self.write_legacy_table_style_override(value);
            }
            self.writer.write_bit(e.legacy_border_colors.is_some());
            if let Some(value) = &e.legacy_border_colors {
                self.write_legacy_border_colors(value);
            }
            self.writer
                .write_bit(e.legacy_border_line_weights.is_some());
            if let Some(value) = &e.legacy_border_line_weights {
                self.write_legacy_border_line_weights(value);
            }
            self.writer.write_bit(e.legacy_border_visibility.is_some());
            if let Some(value) = &e.legacy_border_visibility {
                self.write_legacy_border_visibility(value);
            }
        }

        self.register_object(e.common.handle);
    }

    /// Mirrors [`read_underlay`] field-for-field. Unlike the raster image
    /// writer there is a single (definition) handle, no version-gated
    /// clip-inversion bit, and the clip boundary is always a bit-long count
    /// followed by raw 2D vertices.
    fn write_underlay(&mut self, e: &Underlay) {
        // UNLISTED entity type — always resolve to the DXF class number (500+).
        let dxf_name = e.entity_name();
        let fallback = match e.underlay_type {
            UnderlayType::Dwf => common::OBJ_DWFUNDERLAY,
            UnderlayType::Dgn => common::OBJ_DGNUNDERLAY,
            UnderlayType::Pdf => common::OBJ_PDFUNDERLAY,
        };
        let type_code = self.class_type_code(dxf_name, fallback);
        self.entity_preamble(type_code, &e.common);

        self.writer.write_3bit_double(e.normal);
        self.writer.write_3bit_double(e.insertion_point);
        self.writer.write_bit_double(e.rotation);
        self.writer.write_bit_double(e.x_scale);
        self.writer.write_bit_double(e.y_scale);
        self.writer.write_bit_double(e.z_scale);
        self.writer.write_byte(e.flags.bits());
        self.writer.write_byte(e.contrast);
        self.writer.write_byte(e.fade);

        // Definition handle (hard pointer) — drawn from the handle stream, so
        // it is emitted here mid-record without disturbing the data cursor.
        self.writer
            .write_handle(DwgReferenceType::HardPointer, e.definition_handle.value());

        self.writer
            .write_bit_long(e.clip_boundary_vertices.len() as i32);
        for v in &e.clip_boundary_vertices {
            self.writer.write_2raw_double(*v);
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ RasterImage â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_raster_image(&mut self, e: &RasterImage) {
        // UNLISTED entity type — always use DXF class number (500+)
        let type_code = self.class_type_code("IMAGE", common::OBJ_IMAGE);
        self.entity_preamble(type_code, &e.common);

        self.writer.write_bit_long(e.class_version);
        self.writer.write_3bit_double(e.insertion_point);
        self.writer.write_3bit_double(e.u_vector);
        self.writer.write_3bit_double(e.v_vector);
        self.writer.write_2raw_double(e.size);
        self.writer.write_bit_short(e.flags.bits() as i16);
        self.writer.write_bit(e.clipping_enabled);
        self.writer.write_byte(e.brightness);
        self.writer.write_byte(e.contrast);
        self.writer.write_byte(e.fade);

        if self.version.r2010_plus() {
            self.writer.write_bit(false); // clip is inverted
        }

        // Clip boundary
        self.write_clip_boundary(&e.clip_boundary);

        // Image def handle
        let def = e.definition_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, def.value());

        // Image def reactor handle
        let reactor = e.definition_reactor_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, reactor.value());

        self.register_object(e.common.handle);
    }

    fn write_clip_boundary(&mut self, clip: &ClipBoundary) {
        self.writer.write_bit_short(clip.clip_type as i16);

        match clip.clip_type {
            ClipType::Rectangular => {
                // Rectangular clips: exactly 2 vertices, no count written
                if clip.vertices.len() >= 2 {
                    self.writer.write_2raw_double(clip.vertices[0]);
                    self.writer.write_2raw_double(clip.vertices[1]);
                } else {
                    // Default to origin
                    self.writer.write_2raw_double(Vector2::ZERO);
                    self.writer.write_2raw_double(Vector2::ZERO);
                }
            }
            ClipType::Polygonal => {
                // Polygonal clips: count + vertices
                self.writer.write_bit_long(clip.vertices.len() as i32);
                for v in &clip.vertices {
                    self.writer.write_2raw_double(*v);
                }
            }
        }
    }

    // â”€â”€ Wipeout â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_wipeout(&mut self, e: &Wipeout) {
        // UNLISTED entity type — always use DXF class number (500+)
        // Wipeout uses the "WIPEOUT" DXF class name
        let type_code = self.class_type_code("WIPEOUT", common::OBJ_IMAGE);
        self.entity_preamble(type_code, &e.common);

        self.writer.write_bit_long(e.class_version);
        self.writer.write_3bit_double(e.insertion_point);
        self.writer.write_3bit_double(e.u_vector);
        self.writer.write_3bit_double(e.v_vector);
        self.writer.write_2raw_double(e.size);
        self.writer.write_bit_short(e.flags.bits() as i16);
        self.writer.write_bit(e.clipping_enabled);
        self.writer.write_byte(e.brightness);
        self.writer.write_byte(e.contrast);
        self.writer.write_byte(e.fade);

        if self.version.r2010_plus() {
            self.writer
                .write_bit(e.clip_mode == crate::entities::WipeoutClipMode::Inside);
        }

        // Clip boundary
        self.writer.write_bit_short(e.clip_type as i16);
        match e.clip_type {
            crate::entities::WipeoutClipType::Rectangular => {
                let defaults = [Vector2::new(-0.5, -0.5), Vector2::new(0.5, 0.5)];
                for index in 0..2 {
                    self.writer.write_2raw_double(
                        e.clip_boundary_vertices
                            .get(index)
                            .copied()
                            .unwrap_or(defaults[index]),
                    );
                }
            }
            crate::entities::WipeoutClipType::Polygonal => {
                self.writer
                    .write_bit_long(e.clip_boundary_vertices.len() as i32);
                for v in &e.clip_boundary_vertices {
                    self.writer.write_2raw_double(*v);
                }
            }
        }

        // Definition + reactor handles
        let def = e.definition_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, def.value());
        let reactor = e.definition_reactor_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, reactor.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ OLE2Frame â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_ole2frame(&mut self, e: &Ole2Frame) {
        self.entity_preamble(common::OBJ_OLE2FRAME, &e.common);

        // OLE object type (DXF group 71)
        self.writer.write_bit_short(e.ole_object_type as i16);

        // R2000+: Mode BS (tile mode descriptor)
        if self.version.r2000_plus() {
            let mode = if e.is_paper_space {
                1
            } else if e.dwg_mode == 1 {
                0
            } else {
                e.dwg_mode
            };
            self.writer.write_bit_short(mode);
        }

        // Data Length BL + data bytes
        let data = e.encoded_payload();
        self.writer.write_bit_long(data.len() as i32);
        self.writer.write_bytes(&data);

        // R2000+: lock aspect ratio
        if self.version.r2000_plus() {
            self.writer.write_byte(e.lock_aspect);
        }

        self.register_object(e.common.handle);
    }

    // â”€â”€ MultiLeader â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_light(&mut self, e: &Light) {
        let type_code = self.class_type_code("LIGHT", common::OBJ_LIGHT);
        self.entity_preamble(type_code, &e.common);
        self.writer.write_bit_long(e.class_version);
        self.writer.write_variable_text(&e.name);
        self.writer.write_bit_long(e.light_type);
        self.writer.write_bit(e.status);
        self.writer.write_cm_color(&e.light_color);
        self.writer.write_bit(e.plot_glyph);
        self.writer.write_bit_double(e.intensity);
        self.writer.write_3bit_double(e.position);
        self.writer.write_3bit_double(e.target);
        self.writer.write_bit_long(e.attenuation_type);
        self.writer.write_bit(e.use_attenuation_limits);
        self.writer.write_bit_double(e.attenuation_start_limit);
        self.writer.write_bit_double(e.attenuation_end_limit);
        self.writer.write_bit_double(e.hotspot_angle);
        self.writer.write_bit_double(e.falloff_angle);
        self.writer.write_bit(e.cast_shadows);
        self.writer.write_bit_long(e.shadow_type);
        self.writer.write_bit_short(e.shadow_map_size);
        self.writer.write_byte(e.shadow_map_softness);
        if e.photometric_mode {
            self.writer.write_bit(e.photometric_data.is_some());
            if let Some(data) = &e.photometric_data {
                self.writer.write_bit(data.has_web_file);
                self.writer.write_variable_text(&data.web_file);
                self.writer.write_bit_short(data.physical_intensity_method);
                self.writer.write_bit_double(data.physical_intensity);
                self.writer.write_bit_double(data.illuminance_distance);
                self.writer.write_bit_short(data.lamp_color_type);
                self.writer.write_bit_double(data.lamp_color_temperature);
                self.writer.write_bit_short(data.lamp_color_preset);
                self.writer.write_3bit_double(data.web_rotation);
                self.writer.write_bit_short(data.extended_light_shape);
                self.writer.write_bit_double(data.extended_light_length);
                self.writer.write_bit_double(data.extended_light_width);
                self.writer.write_bit_double(data.extended_light_radius);
                self.writer.write_bit_short(data.web_file_type);
                self.writer.write_bit_short(data.web_symmetry);
                self.writer.write_bit_short(data.has_target_grip);
                self.writer.write_bit_double(data.web_flux);
                for angle in data.web_angles {
                    self.writer.write_bit_double(angle);
                }
                self.writer.write_bit_short(data.glyph_display_type);
            }
        }
        self.register_object(e.common.handle);
    }

    fn write_multileader(&mut self, e: &MultiLeader) {
        // UNLISTED entity type — always use DXF class number (500+)
        let type_code = self.class_type_code("MULTILEADER", common::OBJ_MULTILEADER);
        self.entity_preamble(type_code, &e.common);

        // R2010+: native entity version
        if self.version.r2010_plus() {
            self.writer.write_bit_short(e.dwg_version);
        }

        // Write annotation context sub-object FIRST
        self.write_multileader_annotation_context(&e.context, true);

        // === MultiLeader Common Data ===

        // 340 Leader StyleId (handle) - HardPointer
        let style = e.style_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style.value());

        // 90 Property Override Flags (BL)
        self.writer
            .write_bit_long(e.property_override_flags.bits() as i32);

        // 170 LeaderLineType / PathType (BS)
        self.writer.write_bit_short(e.path_type as i16);

        // 91 Leader LineColor (CMC)
        self.writer.write_cm_color(&e.line_color);

        // 341 LeaderLineTypeID (handle) - HardPointer
        let lt = e.line_type_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, lt.value());

        // 171 LeaderLine Weight (BL not BS!)
        self.writer.write_bit_long(e.line_weight.as_i16() as i32);

        // 290 Enable Landing (B)
        self.writer.write_bit(e.enable_landing);

        // 291 Enable Dogleg (B)
        self.writer.write_bit(e.enable_dogleg);

        // 41 Dogleg Length / Landing distance (BD)
        self.writer.write_bit_double(e.dogleg_length);

        // 342 Arrowhead ID (handle) - HardPointer
        let ah = e.arrowhead_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ah.value());

        // 42 Arrowhead Size (BD)
        self.writer.write_bit_double(e.arrowhead_size);

        // 172 Content Type (BS)
        self.writer.write_bit_short(e.content_type as i16);

        // 343 Text Style ID (handle) - HardPointer
        let ts = e.text_style_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ts.value());

        // 173 Text Left Attachment Type (BS)
        self.writer.write_bit_short(e.text_left_attachment as i16);

        // 95 Text Right Attachment Type (BS)
        self.writer.write_bit_short(e.text_right_attachment as i16);

        // 174 Text Angle Type (BS)
        self.writer.write_bit_short(e.text_angle_type as i16);

        // 175 Text Alignment Type (BS)
        self.writer.write_bit_short(e.text_alignment as i16);

        // 92 Text Color (CMC)
        self.writer.write_cm_color(&e.text_color);

        // 292 Enable Frame Text (B)
        self.writer.write_bit(e.text_frame);

        // 344 Block Content ID (handle) - HardPointer
        let bc = e.block_content_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, bc.value());

        // 93 Block Content Color (CMC)
        self.writer.write_cm_color(&e.block_content_color);

        // 10 Block Content Scale (3BD)
        self.writer.write_3bit_double(e.block_scale);

        // 43 Block Content Rotation (BD)
        self.writer.write_bit_double(e.block_rotation);

        // 176 Block Content Connection Type (BS)
        self.writer.write_bit_short(e.block_connection_type as i16);

        // 293 Enable Annotation Scale / Is annotative (B)
        self.writer.write_bit(e.enable_annotation_scale);

        // Pre-R2007 only: num_arrowheads (BL) + the override-arrowhead list
        // (typically empty). R2007+ drops this list.
        if !self.version.r2007_plus() {
            self.writer
                .write_bit_long(e.arrowhead_overrides.len() as i32);
            for override_value in &e.arrowhead_overrides {
                self.writer.write_bit(override_value.is_default);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    override_value
                        .arrowhead_handle
                        .unwrap_or(Handle::NULL)
                        .value(),
                );
            }
        }

        // All MLeader versions carry block labels and this common tail. For
        // pre-R2007 records it follows the arrowhead override list.
        self.writer.write_bit_long(e.block_attributes.len() as i32);
        for ba in &e.block_attributes {
            // 330 Block Attribute definition handle (hard pointer)
            let def = ba.attribute_definition_handle.unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, def.value());
            // 302 Block Attribute Text String
            self.writer.write_variable_text(&ba.text);
            // 177 Block Attribute Index
            self.writer.write_bit_short(ba.index);
            // 44 Block Attribute Width
            self.writer.write_bit_double(ba.width);
        }

        // 294 Text Direction Negative (B)
        self.writer.write_bit(e.text_direction_negative);
        // 178 Text Align in IPE (BS)
        self.writer.write_bit_short(e.text_align_in_ipe);
        // 179 Text Attachment Point (BS)
        self.writer.write_bit_short(e.text_attachment_point as i16);
        // 45 ScaleFactor (BD)
        self.writer.write_bit_double(e.scale_factor);

        // R2010+: attachment directions — order is dir(271), bottom(272),
        // top(273) per AutoCAD, NOT the dir/top/bottom of the
        // public libredwg spec.
        if self.version.r2010_plus() {
            // 271 Text attachment direction (BS)
            self.writer
                .write_bit_short(e.text_attachment_direction as i16);
            // 272 Bottom text attachment direction (BS)
            self.writer.write_bit_short(e.text_bottom_attachment as i16);
            // 273 Top text attachment direction (BS)
            self.writer.write_bit_short(e.text_top_attachment as i16);
        }

        // R2013+ field
        if self.version.r2013_plus(self.dxf_version) {
            // 295 Leader extended to text (B)
            self.writer.write_bit(e.extend_leader_to_text);
        }

        self.register_object(e.common.handle);
    }

    pub(super) fn write_multileader_annotation_context(
        &mut self,
        ctx: &MultiLeaderAnnotContext,
        write_leader_roots_count: bool,
    ) {
        let leader_root_count = ctx.leader_roots.len();

        if write_leader_roots_count || !ctx.standalone_uses_root_flags {
            // BL - Number of leader roots
            self.writer.write_bit_long(leader_root_count as i32);
        } else {
            self.writer.write_bit_long(0);
            for bit in 0..5 {
                self.writer
                    .write_bit((ctx.standalone_flags & (1 << bit)) != 0);
            }
            self.writer.write_bit(leader_root_count == 2); // b5
            self.writer.write_bit(leader_root_count == 1); // b6
        }

        // Write each leader root
        for root in &ctx.leader_roots {
            self.write_leader_root(root);
        }

        // === Common data ===

        // BD 40 Overall scale
        self.writer.write_bit_double(ctx.scale_factor);
        // 3BD 10 Content base point
        self.writer.write_3bit_double(ctx.content_base_point);
        // BD 41 Text height
        self.writer.write_bit_double(ctx.text_height);
        // BD 140 Arrow head size
        self.writer.write_bit_double(ctx.arrowhead_size);
        // BD 145 Landing gap
        self.writer.write_bit_double(ctx.landing_gap);
        // BS 174 Style left text attachment type
        self.writer.write_bit_short(ctx.text_left_attachment as i16);
        // BS 175 Style right text attachment type
        self.writer
            .write_bit_short(ctx.text_right_attachment as i16);
        // BS 176 Text align type
        self.writer.write_bit_short(ctx.text_alignment as i16);
        // BS 177 Attachment type (content extents or insertion point)
        self.writer
            .write_bit_short(ctx.block_connection_type as i16);

        // B 290 Has text contents
        self.writer.write_bit(ctx.has_text_contents);

        if ctx.has_text_contents {
            // TV 304 Text label
            self.writer.write_variable_text(&ctx.text_string);
            // 3BD 11 Normal vector
            self.writer.write_3bit_double(ctx.text_normal);
            // H 340 Text style handle (hard pointer)
            let ts = ctx.text_style_handle.unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ts.value());
            // 3BD 12 Location
            self.writer.write_3bit_double(ctx.text_location);
            // 3BD 13 Direction
            self.writer.write_3bit_double(ctx.text_direction);
            // BD 42 Rotation (radians)
            self.writer.write_bit_double(ctx.text_rotation);
            // BD 43 Boundary width
            self.writer.write_bit_double(ctx.text_width);
            // BD 44 Boundary height
            self.writer.write_bit_double(ctx.text_boundary_height);
            // BD 45 Line spacing factor
            self.writer.write_bit_double(ctx.line_spacing_factor);
            // BS 170 Line spacing style
            self.writer.write_bit_short(ctx.line_spacing_style as i16);
            // CMC 90 Text color
            self.writer.write_cm_color(&ctx.text_color);
            // BS 171 Alignment / Text Attachment Point
            self.writer
                .write_bit_short(ctx.text_attachment_point as i16);
            // BS 172 Flow direction
            self.writer.write_bit_short(ctx.text_flow_direction as i16);
            // CMC 91 Background fill color
            self.writer.write_cm_color(&ctx.background_fill_color);
            // BD 141 Background scale factor
            self.writer.write_bit_double(ctx.background_scale_factor);
            // BL 92 Background transparency
            self.writer.write_bit_long(ctx.background_transparency);
            // B 291 Is background fill enabled
            self.writer.write_bit(ctx.background_fill_enabled);
            // B 292 Is background mask fill on
            self.writer.write_bit(ctx.background_mask_fill_on);
            // BS 173 Column type
            self.writer.write_bit_short(ctx.column_type);
            // B 293 Is text height automatic
            self.writer.write_bit(ctx.text_height_automatic);
            // BD 142 Column width
            self.writer.write_bit_double(ctx.column_width);
            // BD 143 Column gutter
            self.writer.write_bit_double(ctx.column_gutter);
            // B 294 Column flow reversed
            self.writer.write_bit(ctx.column_flow_reversed);

            // Column sizes (BL count + BD values)
            self.writer.write_bit_long(ctx.column_sizes.len() as i32);
            for size in &ctx.column_sizes {
                self.writer.write_bit_double(*size);
            }

            // B 295 Word break
            self.writer.write_bit(ctx.word_break);
            // B Unknown
            self.writer.write_bit(ctx.dwg_unknown_text_bit);
        } else {
            // B 296 Has contents block - only written when has_text_contents is false
            self.writer.write_bit(ctx.has_block_contents);

            if ctx.has_block_contents {
                // H 341 Block table record handle (soft pointer)
                let bh = ctx.block_content_handle.unwrap_or(Handle::NULL);
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, bh.value());
                // 3BD 14 Normal vector
                self.writer.write_3bit_double(ctx.block_content_normal);
                // 3BD 15 Location
                self.writer.write_3bit_double(ctx.block_content_location);
                // 3BD 16 Scale vector
                self.writer.write_3bit_double(ctx.block_content_scale);
                // BD 46 Rotation (radians)
                self.writer.write_bit_double(ctx.block_rotation);
                // CMC 93 Block color
                self.writer.write_cm_color(&ctx.block_content_color);

                // BD (16) 47 - 16 doubles for transformation matrix
                for i in 0..16 {
                    self.writer.write_bit_double(ctx.transform_matrix[i]);
                }
            }
        }

        // 3BD 110 Base point
        self.writer.write_3bit_double(ctx.base_point);
        // 3BD 111 Base direction
        self.writer.write_3bit_double(ctx.base_direction);
        // 3BD 112 Base vertical
        self.writer.write_3bit_double(ctx.base_vertical);
        // B 297 Is normal reversed
        self.writer.write_bit(ctx.normal_reversed);

        // R2010+ fields
        if self.version.r2010_plus() {
            // BS 273 Style top attachment
            self.writer.write_bit_short(ctx.text_top_attachment as i16);
            // BS 272 Style bottom attachment
            self.writer
                .write_bit_short(ctx.text_bottom_attachment as i16);
        }
    }

    fn write_leader_root(&mut self, root: &LeaderRoot) {
        // B 290 Is content valid
        self.writer.write_bit(root.content_valid);
        // B 291 Unknown (ODA writes true)
        self.writer.write_bit(root.unknown);
        // 3BD 10 Connection point
        self.writer.write_3bit_double(root.connection_point);
        // 3BD 11 Direction
        self.writer.write_3bit_double(root.direction);

        // Break start/end point pairs
        self.writer.write_bit_long(root.break_points.len() as i32);
        for pair in &root.break_points {
            // 3BD 12 Break start point
            self.writer.write_3bit_double(pair.start_point);
            // 3BD 13 Break end point
            self.writer.write_3bit_double(pair.end_point);
        }

        // BL 90 Leader index
        self.writer.write_bit_long(root.leader_index);
        // BD 40 Landing distance
        self.writer.write_bit_double(root.landing_distance);

        // Leader lines
        self.writer.write_bit_long(root.lines.len() as i32);
        for line in &root.lines {
            self.write_leader_line(line);
        }

        // R2010+
        if self.version.r2010_plus() {
            // BS 271 Attachment direction
            self.writer
                .write_bit_short(root.text_attachment_direction as i16);
        }
    }

    fn write_leader_line(&mut self, line: &LeaderLine) {
        // Points
        self.writer.write_bit_long(line.points.len() as i32);
        for pt in &line.points {
            self.writer.write_3bit_double(*pt);
        }

        // Break info
        let legacy_break_info;
        let break_infos = if line.break_infos.is_empty() {
            legacy_break_info = [LeaderLineBreakInfo {
                segment_index: line.segment_index,
                break_points: line.break_points.clone(),
            }];
            if line.break_info_count > 0 || !line.break_points.is_empty() {
                &legacy_break_info[..]
            } else {
                &[]
            }
        } else {
            line.break_infos.as_slice()
        };
        self.writer.write_bit_long(break_infos.len() as i32);
        for info in break_infos {
            // BL 90 Segment index
            self.writer.write_bit_long(info.segment_index);
            // Start/end point pairs
            self.writer.write_bit_long(info.break_points.len() as i32);
            for sep in &info.break_points {
                self.writer.write_3bit_double(sep.start_point);
                self.writer.write_3bit_double(sep.end_point);
            }
        }

        // BL 91 Leader line index
        self.writer.write_bit_long(line.index);

        // R2010+ line properties
        if self.version.r2010_plus() {
            // BS 170 Leader type (path type)
            self.writer.write_bit_short(line.path_type as i16);
            // CMC 92 Line color
            self.writer.write_cm_color(&line.line_color);
            // H 340 Line type handle (hard pointer)
            let lt = line.line_type_handle.unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, lt.value());
            // BL 171 Line weight
            self.writer.write_bit_long(line.line_weight.as_i16() as i32);
            // BD 40 Arrow size
            self.writer.write_bit_double(line.arrowhead_size);
            // H 341 Arrow symbol handle (hard pointer)
            let ah = line.arrowhead_handle.unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ah.value());
            // BL 93 Override flags
            self.writer
                .write_bit_long(line.override_flags.bits() as i32);
        }
    }

    // â”€â”€ Attribute Definition â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[allow(clippy::too_many_arguments)]
    fn write_embedded_attribute_mtext(
        &mut self,
        embedded: Option<&MText>,
        value: &str,
        insertion_point: Vector3,
        normal: Vector3,
        rotation: f64,
        height: f64,
        style: &str,
    ) {
        let mut fallback = MText::with_value(value, insertion_point);
        fallback.normal = normal;
        fallback.rotation = rotation;
        fallback.height = height;
        fallback.style = style.to_string();
        let mtext = embedded.unwrap_or(&fallback);

        // AcDbMTextObjectEmbedded has a reduced common-entity header whose
        // order differs from a standalone MTEXT entity.
        // Embedded MTEXT is a payload, not a model/paper-space entity.  Mode
        // zero still requires its (nullable) owner slot in the handle stream.
        self.writer.write_2bits(0);
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, Handle::NULL.value());
        self.writer.write_bit_long(0);
        self.writer.write_bit(true);
        self.writer.write_bit(false);
        self.writer
            .write_bit_short(mtext.common.color.index().unwrap_or(256) as i16);
        self.writer.write_bit_double(mtext.common.linetype_scale);
        self.writer.write_2bits(0);
        self.writer.write_2bits(0);
        self.writer.write_2bits(0);
        self.writer.write_byte(mtext.common.shadow_flags);
        self.writer.write_bit(false);
        self.writer.write_bit(false);
        self.writer.write_bit(false);
        self.writer
            .write_bit_short(if mtext.common.invisible { 1 } else { 0 });
        self.writer
            .write_byte(mtext.common.line_weight.to_dwg_index());

        let layer_handle = self
            .document
            .layers
            .get(&mtext.common.layer)
            .map(|layer| layer.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, layer_handle.value());

        self.writer.write_3bit_double(mtext.insertion_point);
        self.writer.write_3bit_double(mtext.normal);
        let x_direction = mtext
            .dwg_x_direction
            .filter(|direction| direction.y.atan2(direction.x) == mtext.rotation)
            .unwrap_or_else(|| Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0));
        self.writer.write_3bit_double(x_direction);
        self.writer.write_bit_double(mtext.rectangle_width);
        self.writer
            .write_bit_double(mtext.rectangle_height.unwrap_or(0.0));
        self.writer.write_bit_double(mtext.height);
        self.writer.write_bit_short(mtext.attachment_point as i16);
        self.writer.write_bit_short(mtext.drawing_direction as i16);
        self.writer.write_bit_double(mtext.extents_width);
        self.writer.write_bit_double(mtext.extents_height);
        self.writer.write_variable_text(&mtext.value);

        let style_handle = self
            .document
            .text_styles
            .get(&mtext.style)
            .map(|text_style| text_style.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());
        self.writer.write_bit_short(mtext.line_spacing_style as i16);
        self.writer.write_bit_double(mtext.line_spacing_factor);
        self.writer.write_bit(false);
        self.writer.write_bit_long(mtext.background_fill_flags);
        if mtext.background_fill_flags & 1 != 0
            || (self.version.r2018_plus(self.dxf_version)
                && mtext.background_fill_flags & 0x10 != 0)
        {
            self.writer.write_bit_double(mtext.background_scale);
            self.writer.write_cm_color(&mtext.background_color);
            self.writer.write_bit_long(mtext.background_transparency);
        }
        self.writer.write_bit(!mtext.is_annotative);
        if !mtext.is_annotative {
            self.writer.write_bit_short(4);
            self.writer.write_bit(true);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, Handle::NULL.value());
            self.writer.write_bit_long(mtext.attachment_point as i32);
            self.writer.write_3bit_double(x_direction);
            self.writer.write_3bit_double(mtext.insertion_point);
            self.writer.write_bit_double(mtext.rectangle_width);
            self.writer
                .write_bit_double(mtext.rectangle_height.unwrap_or(0.0));
            self.writer.write_bit_double(mtext.extents_width);
            self.writer.write_bit_double(mtext.extents_height);

            let columns = &mtext.column_data;
            self.writer.write_bit_short(columns.column_type);
            if columns.column_type != 0 {
                let height_count = if columns.column_type == 2 && !columns.auto_height {
                    columns.heights.len() as i32
                } else {
                    columns.column_count
                };
                self.writer.write_bit_long(height_count);
                self.writer.write_bit_double(columns.width);
                self.writer.write_bit_double(columns.gutter);
                self.writer.write_bit(columns.auto_height);
                self.writer.write_bit(columns.flow_reversed);
                if columns.column_type == 2 && !columns.auto_height {
                    for height in &columns.heights {
                        self.writer.write_bit_double(*height);
                    }
                }
            }
        }
    }

    fn write_attribute_definition(&mut self, e: &AttributeDefinition) {
        self.entity_preamble(common::OBJ_ATTDEF, &e.common);

        // writeTextEntity portion
        self.write_text_entity_data(
            e.insertion_point,
            e.alignment_point,
            e.normal,
            0.0, // thickness
            e.oblique_angle,
            e.rotation,
            e.height,
            e.width_factor,
            &e.default_value,
            e.text_generation_flags,
            e.horizontal_alignment as i16,
            e.vertical_alignment as i16,
        );

        // writeCommonAttData: R2010+ version byte
        if self.version.r2010_plus() {
            self.writer.write_byte(0); // version
        }

        // R2018+: AttributeType byte
        if self.version.r2018_plus(self.dxf_version) {
            let att_type = if e.embedded_mtext.is_some() || e.is_multiline {
                e.mtext_flag.to_value().max(2) as u8
            } else {
                1
            };
            self.writer.write_byte(att_type);
            if att_type > 1 {
                self.write_embedded_attribute_mtext(
                    e.embedded_mtext.as_deref(),
                    &e.default_value,
                    e.insertion_point,
                    e.normal,
                    e.rotation,
                    e.height,
                    &e.text_style,
                );
                // Attribute-level annotative payload follows the complete
                // embedded MTEXT object.
                self.writer.write_bit_short(0);
            }
        }

        // Tag, field length, flags
        self.writer.write_variable_text(&e.tag);
        self.writer.write_bit_short(e.field_length);
        let flag_byte = e.flags.to_bits();
        self.writer.write_byte(flag_byte as u8);

        // R2007+: lock position
        if self.version.r2007_plus() {
            self.writer.write_bit(e.lock_position);
        }

        // writeAttDefinition: R2010+ version byte (second)
        if self.version.r2010_plus() {
            self.writer.write_byte(0);
        }

        // Prompt
        self.writer.write_variable_text(&e.prompt);

        // The outer TEXT style is the final ATTDEF handle.  For multiline
        // attributes the embedded MTEXT layer/style handles precede it.
        let style_handle = self
            .document
            .text_styles
            .get(&e.text_style)
            .map(|s| s.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Attribute Entity â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_attribute_entity(&mut self, e: &AttributeEntity) {
        self.entity_preamble(common::OBJ_ATTRIB, &e.common);

        // writeTextEntity portion
        self.write_text_entity_data(
            e.insertion_point,
            e.alignment_point,
            e.normal,
            0.0, // thickness
            e.oblique_angle,
            e.rotation,
            e.height,
            e.width_factor,
            &e.value,
            e.text_generation_flags,
            e.horizontal_alignment as i16,
            e.vertical_alignment as i16,
        );

        // writeCommonAttData: R2010+ version byte
        if self.version.r2010_plus() {
            self.writer.write_byte(0); // version
        }

        // R2018+: AttributeType byte
        if self.version.r2018_plus(self.dxf_version) {
            let att_type = if e.embedded_mtext.is_some() || e.is_multiline {
                e.mtext_flag.to_value().max(2) as u8
            } else {
                1
            };
            self.writer.write_byte(att_type);
            if att_type > 1 {
                self.write_embedded_attribute_mtext(
                    e.embedded_mtext.as_deref(),
                    &e.value,
                    e.insertion_point,
                    e.normal,
                    e.rotation,
                    e.height,
                    &e.text_style,
                );
                self.writer.write_bit_short(0);
            }
        }

        // Tag, field length, flags
        self.writer.write_variable_text(&e.tag);
        self.writer.write_bit_short(e.field_length);
        let flag_byte = e.flags.to_bits();
        self.writer.write_byte(flag_byte as u8);

        // R2007+: lock position
        if self.version.r2007_plus() {
            self.writer.write_bit(e.lock_position);
        }
        // The outer TEXT style follows the embedded MTEXT handles.
        let style_handle = self
            .document
            .text_styles
            .get(&e.text_style)
            .map(|s| s.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, style_handle.value());

        self.register_object(e.common.handle);
    }

    // â”€â”€ Shared text entity data (used by AttDef/AttEntity) â”€â”€â”€â”€â”€â”€â”€â”€

    /// Write the TEXT entity data structure shared by Text, AttDef, and AttEntity.
    /// This matches the C# `writeTextEntity` method.
    #[allow(clippy::too_many_arguments)]
    fn write_text_entity_data(
        &mut self,
        insertion_point: Vector3,
        alignment_point: Vector3,
        normal: Vector3,
        thickness: f64,
        oblique_angle: f64,
        rotation: f64,
        height: f64,
        width_factor: f64,
        text_value: &str,
        generation: i16,
        horizontal_alignment: i16,
        vertical_alignment: i16,
    ) {
        if self.version.r13_14_only() {
            // R13-R14: all fields present
            self.writer.write_bit_double(insertion_point.z); // elevation
            self.writer.write_raw_double(insertion_point.x);
            self.writer.write_raw_double(insertion_point.y);
            self.writer.write_raw_double(alignment_point.x);
            self.writer.write_raw_double(alignment_point.y);
            self.writer.write_3bit_double(normal);
            self.writer.write_bit_double(thickness);
            self.writer.write_bit_double(oblique_angle);
            self.writer.write_bit_double(rotation);
            self.writer.write_bit_double(height);
            self.writer.write_bit_double(width_factor);
            self.writer.write_variable_text(text_value);
            self.writer.write_bit_short(generation);
            self.writer.write_bit_short(horizontal_alignment);
            self.writer.write_bit_short(vertical_alignment);
        } else {
            // R2000+: DataFlags-based conditional encoding
            let mut data_flags: u8 = 0;
            if insertion_point.z == 0.0 {
                data_flags |= 0b0000_0001; // elevation is zero
            }
            if alignment_point == Vector3::ZERO {
                data_flags |= 0b0000_0010; // alignment point is zero
            }
            if oblique_angle == 0.0 {
                data_flags |= 0b0000_0100;
            }
            if rotation == 0.0 {
                data_flags |= 0b0000_1000;
            }
            if width_factor == 1.0 {
                data_flags |= 0b0001_0000;
            }
            if generation == 0 {
                data_flags |= 0b0010_0000; // no mirror
            }
            if horizontal_alignment == 0 {
                data_flags |= 0b0100_0000; // left
            }
            if vertical_alignment == 0 {
                data_flags |= 0b1000_0000; // baseline
            }

            self.writer.write_byte(data_flags);

            // Elevation RD — if !(flags & 0x01)
            if (data_flags & 0x01) == 0 {
                self.writer.write_raw_double(insertion_point.z);
            }
            // Insertion pt 2RD 10
            self.writer.write_raw_double(insertion_point.x);
            self.writer.write_raw_double(insertion_point.y);
            // Alignment pt 2DD 11 — if !(flags & 0x02)
            if (data_flags & 0x02) == 0 {
                self.writer
                    .write_bit_double_with_default(alignment_point.x, insertion_point.x);
                self.writer
                    .write_bit_double_with_default(alignment_point.y, insertion_point.y);
            }
            // Extrusion BE 210
            self.writer.write_bit_extrusion(normal);
            // Thickness BT 39
            self.writer.write_bit_thickness(thickness);
            // Oblique ang RD 51 — if !(flags & 0x04)
            if (data_flags & 0x04) == 0 {
                self.writer.write_raw_double(oblique_angle);
            }
            // Rotation ang RD 50 — if !(flags & 0x08)
            if (data_flags & 0x08) == 0 {
                self.writer.write_raw_double(rotation);
            }
            // Height RD 40
            self.writer.write_raw_double(height);
            // Width factor RD 41 — if !(flags & 0x10)
            if (data_flags & 0x10) == 0 {
                self.writer.write_raw_double(width_factor);
            }
            // Text value TV 1
            self.writer.write_variable_text(text_value);
            // Generation BS 71 — if !(flags & 0x20)
            if (data_flags & 0x20) == 0 {
                self.writer.write_bit_short(generation);
            }
            // Horiz align BS 72 — if !(flags & 0x40)
            if (data_flags & 0x40) == 0 {
                self.writer.write_bit_short(horizontal_alignment);
            }
            // Vert align BS 73 — if !(flags & 0x80)
            if (data_flags & 0x80) == 0 {
                self.writer.write_bit_short(vertical_alignment);
            }
        }
    }

    // â”€â”€ Legacy Polyline (2D) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_polyline_old(&mut self, e: &Polyline) {
        // Legacy Polyline — convert to Polyline3D for DWG output.
        // The DXF reader collapses Polyline2D/3D/PolyfaceMesh into this
        // legacy variant; we re-emit as Polyline3D so data isn't lost.
        let mut p3d = Polyline3D::new();
        p3d.common = e.common.clone();
        p3d.flags.closed = e.flags.is_closed();
        for v in &e.vertices {
            p3d.vertices.push(Vertex3DPolyline {
                handle: Handle::NULL,
                layer: e.common.layer.clone(),
                position: v.location,
                flags: v.flags.bits() as i32,
            });
        }
        self.write_polyline3d(&p3d);
    }

    // â”€â”€ ACIS entities (3DSOLID, REGION, BODY) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn write_solid3d(&mut self, e: &Solid3D) {
        // R2013+: mark the entity as data-store-backed when its geometry will be
        // emitted as a SAB blob into the AcDs section (below), so readers pair
        // the blob with this solid. Must precede the common-data preamble.
        self.pending_has_ds_data = self.needs_acds_section() && e.acis_data.contributes_sab();
        self.entity_preamble(common::OBJ_3DSOLID, &e.common);

        let acds = self.needs_acds_section();
        let tail_written = if acds {
            // AC1027+: ACIS data is stored in the AcDsPrototype_1b section.
            // Entity stream writes acis_empty=true with no inline data.
            self.write_acis_empty(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes);
            self.queue_sab_entry(&e.acis_data, e.common.handle);
            false
        } else {
            self.write_acis_data(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes)
        };

        // SAB binary path already wrote trailing fields; skip for SAT/empty.
        if !tail_written {
            // AcDs-backed R2013+ bodies have no trailing acis-empty bit when
            // the wireframe flag is false. The next field is the R2007
            // unknown BL.
            self.writer.write_bit_long(0);

            // R2013+: modeler-geometry revision block (COMMON_3DSOLID).
            // Empty / SAT bodies carry no materials block (materials only
            // follow binary SAB, handled on the early-return path). Omitting
            // this block corrupts the entity stream for R2013+ and prevents
            // the file from opening in AutoCAD/TrueView/BricsCAD.
            if self.version.r2013_plus(self.dxf_version) {
                self.write_acis_revision(&e.acis_data.revision);
            }
        }

        // 3DSOLID R2007+: history_id handle
        if self.version.r2007_plus() {
            let h = e.history_handle.map(|h| h.value()).unwrap_or(0);
            self.writer.write_handle(DwgReferenceType::SoftPointer, h);
        }

        self.register_object(e.common.handle);
    }

    fn write_region(&mut self, e: &Region) {
        self.pending_has_ds_data = self.needs_acds_section() && e.acis_data.contributes_sab();
        self.entity_preamble(common::OBJ_REGION, &e.common);

        let acds = self.needs_acds_section();
        let tail_written = if acds {
            self.write_acis_empty(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes);
            self.queue_sab_entry(&e.acis_data, e.common.handle);
            false
        } else {
            self.write_acis_data(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes)
        };

        // SAB binary path already wrote trailing fields; skip for SAT/empty.
        if !tail_written {
            self.writer.write_bit_long(0);

            // R2013+: modeler-geometry revision block (COMMON_3DSOLID).
            // Empty / SAT bodies carry no materials block (materials only
            // follow binary SAB, handled on the early-return path). Omitting
            // this block corrupts the entity stream for R2013+ and prevents
            // the file from opening in AutoCAD/TrueView/BricsCAD.
            if self.version.r2013_plus(self.dxf_version) {
                self.write_acis_revision(&e.acis_data.revision);
            }
        }

        self.register_object(e.common.handle);
    }

    fn write_body(&mut self, e: &Body) {
        self.pending_has_ds_data = self.needs_acds_section() && e.acis_data.contributes_sab();
        self.entity_preamble(common::OBJ_BODY, &e.common);

        let acds = self.needs_acds_section();
        let tail_written = if acds {
            self.write_acis_empty(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes);
            self.queue_sab_entry(&e.acis_data, e.common.handle);
            false
        } else {
            self.write_acis_data(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes)
        };

        // SAB binary path already wrote trailing fields; skip for SAT/empty.
        if !tail_written {
            self.writer.write_bit_long(0);

            // R2013+: modeler-geometry revision block (COMMON_3DSOLID).
            // Empty / SAT bodies carry no materials block (materials only
            // follow binary SAB, handled on the early-return path). Omitting
            // this block corrupts the entity stream for R2013+ and prevents
            // the file from opening in AutoCAD/TrueView/BricsCAD.
            if self.version.r2013_plus(self.dxf_version) {
                self.write_acis_revision(&e.acis_data.revision);
            }
        }

        if self.version.r2007_plus() && !acds {
            let h = e.history_handle.map(|h| h.value()).unwrap_or(0);
            self.writer.write_handle(DwgReferenceType::SoftPointer, h);
        }

        self.register_object(e.common.handle);
    }

    fn write_surface_matrix(&mut self, value: &[f64; 16]) {
        for item in value {
            self.writer.write_bit_double(*item);
        }
    }

    fn write_surface_embedded_entity(
        &mut self,
        entity: &crate::entities::EmbeddedEntity,
        byte_aligned: bool,
    ) {
        let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
            entity,
            self.version,
            self.dxf_version,
        );
        let bit_length = if byte_aligned {
            encoded.bytes.len() * 8
        } else {
            encoded.bit_length
        };
        self.writer.write_bit_long(encoded.type_code);
        self.writer.write_bit_long(bit_length as i32);
        crate::io::dwg::embedded_entity::write_embedded_bits_with_length(
            &mut self.writer,
            &encoded,
            bit_length,
        );
    }

    fn write_surface_sweep_options(&mut self, value: &SurfaceSweepOptions) {
        self.writer.write_bit_double(value.draft_angle);
        self.writer.write_bit_double(value.draft_start_distance);
        self.writer.write_bit_double(value.draft_end_distance);
        self.writer.write_bit_double(value.twist_angle);
        self.writer.write_bit_double(value.scale_factor);
        self.writer.write_bit_double(value.align_angle);
        self.writer.write_bit(value.is_solid);
        self.writer.write_bit_short(value.sweep_alignment_flags);
        self.writer.write_bit_short(value.path_flags);
        self.writer.write_bit(value.align_start);
        self.writer.write_bit(value.bank);
        self.writer.write_bit(value.base_point_set);
        self.writer.write_bit(value.sweep_entity_transform_computed);
        self.writer.write_bit(value.path_entity_transform_computed);
        self.writer.write_3bit_double(value.reference_vector);
        self.write_surface_matrix(&value.sweep_entity_transform);
        self.write_surface_matrix(&value.path_entity_transform);
    }

    fn write_surface(&mut self, e: &Surface) {
        self.pending_has_ds_data = self.needs_acds_section() && e.acis_data.contributes_sab();
        let type_code = self.class_type_code(e.kind.dxf_name(), common::OBJ_SURFACE);
        self.entity_preamble(type_code, &e.common);

        let acds = self.needs_acds_section();
        let tail_written = if acds {
            self.write_acis_empty(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes);
            self.queue_sab_entry(&e.acis_data, e.common.handle);
            false
        } else {
            self.write_acis_data(e.point_of_reference, &e.acis_data, &e.wires, &e.silhouettes)
        };
        if !tail_written {
            self.writer.write_bit_long(0);
            if self.version.r2013_plus(self.dxf_version) {
                self.write_acis_revision(&e.acis_data.revision);
            }
        }
        if matches!(
            e.kind,
            SurfaceKind::Lofted | SurfaceKind::Revolved | SurfaceKind::Swept
        ) && self.version.r2007_plus()
        {
            self.writer.write_bit_short(e.modeler_format_version);
        }
        self.writer.write_bit_short(e.u_isolines);
        self.writer.write_bit_short(e.v_isolines);

        match &e.surface_data {
            SurfaceData::Generic | SurfaceData::Plane { .. } => {}
            SurfaceData::Extruded {
                sweep_entity,
                options,
                sweep_vector,
                sweep_transform,
            } => {
                self.write_surface_sweep_options(options);
                self.writer.write_3bit_double(*sweep_vector);
                self.write_surface_matrix(sweep_transform);
                if let Some(entity) = sweep_entity {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    let bit_length = encoded.bytes.len() * 8;
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(bit_length as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bits_with_length(
                        &mut self.writer,
                        &encoded,
                        bit_length,
                    );
                } else {
                    self.writer.write_bit_long(0);
                    self.writer.write_bit_long(0);
                }
            }
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
            } => {
                self.write_surface_matrix(loft_transform);
                if !self.version.r2007_plus() {
                    self.writer
                        .write_bit_short(cross_section_entities.len() as i16);
                    self.writer.write_bit_short(guide_entities.len() as i16);
                    self.writer.write_bit(path_entity.is_some());
                    self.writer.write_bit_double(*start_draft_angle);
                    self.writer.write_bit_double(*end_draft_angle);
                    self.writer.write_bit_double(*start_draft_magnitude);
                    self.writer.write_bit_double(*end_draft_magnitude);
                    self.writer.write_bit(*arc_length_parameterization);
                    self.writer.write_bit(*no_twist);
                    self.writer.write_bit(*align_direction);
                    self.writer.write_bit(*simple_surfaces);
                    self.writer.write_bit(*closed_surfaces);
                    self.writer.write_bit(*solid);
                    self.writer.write_bit(*ruled_surface);
                    self.writer.write_bit(*virtual_guide);
                    self.writer.write_bit_long(*plane_normal_lofting_type);
                    for entity in cross_section_entities {
                        self.write_surface_embedded_entity(entity, true);
                    }
                    for entity in guide_entities {
                        self.write_surface_embedded_entity(entity, true);
                    }
                    if let Some(entity) = path_entity {
                        self.write_surface_embedded_entity(entity, true);
                    }
                } else {
                    self.writer.write_bit_long(*plane_normal_lofting_type);
                    self.writer.write_bit_double(*start_draft_angle);
                    self.writer.write_bit_double(*end_draft_angle);
                    self.writer.write_bit_double(*start_draft_magnitude);
                    self.writer.write_bit_double(*end_draft_magnitude);
                    self.writer.write_bit(*arc_length_parameterization);
                    self.writer.write_bit(*no_twist);
                    self.writer.write_bit(*align_direction);
                    self.writer.write_bit(*simple_surfaces);
                    self.writer.write_bit(*closed_surfaces);
                    self.writer.write_bit(*solid);
                    self.writer.write_bit(*ruled_surface);
                    self.writer.write_bit(*virtual_guide);
                    self.writer.write_bit_short(cross_sections.len() as i16);
                    self.writer.write_bit_short(guide_curves.len() as i16);
                    for handle in cross_sections {
                        self.writer
                            .write_handle(DwgReferenceType::HardPointer, handle.value());
                    }
                    for handle in guide_curves {
                        self.writer
                            .write_handle(DwgReferenceType::HardPointer, handle.value());
                    }
                    self.writer.write_handle(
                        DwgReferenceType::HardPointer,
                        path_curve.unwrap_or(Handle::NULL).value(),
                    );
                }
            }
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
            } => {
                if self.version.r2007_plus() {
                    self.writer.write_bit_long(*class_version);
                    self.writer.write_bit_long(*entity_id);
                } else {
                    self.writer.write_bit_double(*draft_angle);
                    self.writer.write_bit_double(*draft_start_distance);
                    self.writer.write_bit_double(*draft_end_distance);
                    self.writer.write_bit_double(*twist_angle);
                    self.writer.write_bit(*solid);
                    self.writer.write_bit(*close_to_axis);
                }
                self.writer.write_3bit_double(*axis_point);
                self.writer.write_3bit_double(*axis_vector);
                self.writer.write_bit_double(*revolve_angle);
                self.writer.write_bit_double(*start_angle);
                self.write_surface_matrix(entity_transform);
                if self.version.r2007_plus() {
                    self.writer.write_bit_double(*draft_angle);
                    self.writer.write_bit_double(*draft_start_distance);
                    self.writer.write_bit_double(*draft_end_distance);
                    self.writer.write_bit_double(*twist_angle);
                    self.writer.write_bit(*solid);
                    self.writer.write_bit(*close_to_axis);
                } else if let Some(entity) = revolve_entity {
                    self.write_surface_embedded_entity(entity, true);
                } else {
                    self.writer.write_bit_long(0);
                    self.writer.write_bit_long(0);
                }
            }
            SurfaceData::Swept {
                class_version,
                sweep_entity,
                path_entity,
                sweep_transform,
                path_transform,
                options,
            } => {
                if self.version.r2007_plus() {
                    self.writer.write_bit_long(*class_version);
                } else {
                    self.write_surface_sweep_options(options);
                    self.write_surface_matrix(sweep_transform);
                    self.write_surface_matrix(path_transform);
                }
                if let Some(entity) = sweep_entity {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    let bit_length = encoded.bytes.len() * 8;
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(bit_length as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bits_with_length(
                        &mut self.writer,
                        &encoded,
                        bit_length,
                    );
                } else {
                    self.writer.write_bit_long(0);
                    self.writer.write_bit_long(0);
                }
                if let Some(entity) = path_entity {
                    let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
                        entity,
                        self.version,
                        self.dxf_version,
                    );
                    let bit_length = encoded.bytes.len() * 8;
                    self.writer.write_bit_long(encoded.type_code);
                    self.writer.write_bit_long(bit_length as i32);
                    crate::io::dwg::embedded_entity::write_embedded_bits_with_length(
                        &mut self.writer,
                        &encoded,
                        bit_length,
                    );
                } else {
                    self.writer.write_bit_long(0);
                    self.writer.write_bit_long(0);
                }
                if self.version.r2007_plus() {
                    self.write_surface_sweep_options(options);
                }
            }
            SurfaceData::Nurb {
                short_170,
                cv_hull_display,
                u_vector1,
                v_vector1,
                u_vector2,
                v_vector2,
            } => {
                if self.version.r2013_plus(self.dxf_version) {
                    self.writer.write_bit_short(*short_170);
                    self.writer.write_bit(*cv_hull_display);
                    self.writer.write_3bit_double(*u_vector1);
                    self.writer.write_3bit_double(*v_vector1);
                    self.writer.write_3bit_double(*u_vector2);
                    self.writer.write_3bit_double(*v_vector2);
                }
            }
        }
        self.register_object(e.common.handle);
    }

    /// Write an empty ACIS entity stub (AC1027+).
    ///
    /// For R2013 and later, ACIS data lives in the AcDsPrototype_1b section.
    /// The entity stream indicates that modeler geometry is not inline, but its
    /// native COMMON_3DSOLID wireframe cache still remains in the entity.
    fn write_acis_empty(
        &mut self,
        _point: Vector3,
        _acis: &AcisData,
        _wires: &[Wire],
        _silhouettes: &[Silhouette],
    ) {
        // R2013+ AcDs-backed records no longer carry the legacy leading
        // `acis_empty` bit.  Their first modeler-geometry bit is the
        // wireframe-presence flag.
        //
        // Producer-specific wire/silhouette caches are optional and several
        // valid drawings use incompatible cache tails. Geometry itself is the
        // SAB blob in AcDs. Emit no cache here, matching ODA's canonical
        // round-trip, instead of reconstructing a subtly malformed object.
        self.writer.write_bit(false);
    }

    /// Write the R2013+ modeler-geometry revision block (`COMMON_3DSOLID`).
    ///
    /// Layout: `B has_guid | BL major | BS minor1 | BS minor2 | RC×8 bytes | BL end_marker`.
    fn write_acis_revision(&mut self, rev: &crate::entities::solid3d::AcisRevision) {
        self.writer.write_bit(rev.has_guid);
        self.writer.write_bit_long(rev.major as i32);
        self.writer.write_bit_short(rev.minor1);
        self.writer.write_bit_short(rev.minor2);
        self.writer.write_bytes(&rev.bytes);
        self.writer.write_bit_long(rev.end_marker as i32);
    }

    fn write_acis_materials(&mut self, acis: &AcisData, inline: bool) {
        self.writer.write_bit_long(acis.materials.len() as i32);
        for material in &acis.materials {
            self.writer.write_bit_long(material.array_index);
            self.writer.write_bit_long(material.absolute_reference);
            let handle = material.material_handle.unwrap_or(Handle::NULL).value();
            if inline {
                self.writer
                    .write_main_handle(DwgReferenceType::HardPointer, handle);
            } else {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, handle);
            }
        }
    }

    fn write_extra_acis_data(&mut self, acis: &AcisData) {
        let Some(extra) = &acis.extra_acis_data else {
            return;
        };
        self.writer.write_bit(false);
        if extra.is_binary && !extra.sab_data.is_empty() {
            self.writer.write_bit_short(2);
            self.writer.write_bytes(&extra.sab_data);
            return;
        }

        self.writer.write_bit_short(1);
        let stripped = AcisData::strip_sat_terminator(&extra.sat_data);
        let encrypted: Vec<u8> = stripped
            .bytes()
            .map(|byte| {
                if byte <= 32 {
                    byte
                } else {
                    159u8.wrapping_sub(byte)
                }
            })
            .collect();
        self.writer.write_bit_long(encrypted.len() as i32);
        self.writer.write_bytes(&encrypted);
        self.writer.write_bit_long(0);
    }

    /// Queue SAB data for writing into the AcDsPrototype_1b section.
    ///
    /// Converts SAT text → SAB binary if needed (mirroring the DXF writer's
    /// `queue_sab_data()` approach).
    fn queue_sab_entry(&mut self, acis: &AcisData, entity_handle: Handle) {
        if acis.is_binary && !acis.sab_data.is_empty() {
            // Already have SAB binary data
            self.sab_entries
                .push((entity_handle, acis.sab_data.clone()));
        } else if !acis.sat_data.is_empty() {
            // Convert SAT text → SAB binary via SatDocument
            if let Ok(mut sat_doc) = crate::entities::acis::SatDocument::parse(&acis.sat_data) {
                sat_doc.strip_for_sab();
                let sab = crate::entities::acis::SabWriter::write(&sat_doc);
                self.sab_entries.push((entity_handle, sab));
            }
        }
    }

    /// Write ACIS/SAT modeler geometry data shared by 3DSOLID, REGION, BODY.
    ///
    /// Returns `true` when the shared trailing fields were written here.
    /// AcDs-backed R2013+ callers write their out-of-line tail separately.
    pub(super) fn write_acis_data(
        &mut self,
        point: Vector3,
        acis: &AcisData,
        wires: &[Wire],
        silhouettes: &[Silhouette],
    ) -> bool {
        self.write_acis_data_impl(point, acis, wires, silhouettes, false)
    }

    fn write_acis_data_impl(
        &mut self,
        point: Vector3,
        acis: &AcisData,
        wires: &[Wire],
        silhouettes: &[Silhouette],
        inline: bool,
    ) -> bool {
        let has_data = acis.has_data();
        self.writer.write_bit(!has_data); // acis_empty (inverted: true = empty)

        if has_data {
            // Unknown bit — per ODA spec / LibreDWG this B
            // is always present between acis_empty and the version BS.
            self.writer.write_bit(false);

            if acis.is_binary && !acis.sab_data.is_empty() {
                // SAB binary (version 2) — write raw bytes directly.
                self.writer.write_bit_short(2_i16);
                self.writer.write_bytes(&acis.sab_data);
                if self.version.r2007_plus() {
                    let wireframe_present =
                        self.write_acis_wireframe(point, acis, wires, silhouettes);
                    if inline || wireframe_present || !self.version.r2013_plus(self.dxf_version) {
                        self.writer.write_bit(
                            acis.extra_acis_data
                                .as_ref()
                                .map(|_| false)
                                .unwrap_or(acis.acis_empty_bit),
                        );
                        self.write_extra_acis_data(acis);
                    }
                    self.write_acis_materials(acis, inline);
                    if self.version.r2013_plus(self.dxf_version) {
                        self.write_acis_revision(&acis.revision);
                    }
                    return true;
                }
                // R2004–R2006: the payload is self-delimiting; the wireframe
                // section and trailing fields follow inline like SAT.
            } else {
                // SAT text (version 1).
                self.writer.write_bit_short(1_i16);

                // Obtain SAT text — convert from SAB if needed.
                let sat_text = if !acis.sat_data.is_empty() {
                    // Already have SAT text
                    acis.sat_data.clone()
                } else if !acis.sab_data.is_empty() {
                    // Convert SAB binary → SAT text via SabReader + SatDocument
                    match crate::entities::acis::SabReader::read(&acis.sab_data) {
                        Ok(sat_doc) => sat_doc.to_sat_string(),
                        Err(_) => String::new(),
                    }
                } else {
                    String::new()
                };

                // SAT text — all DWG versions use the same encoding:
                // BL-sized blocks of encrypted bytes (cipher: 159 - byte)
                // terminated by BL(0).  Per LibreDWG dwg.spec.
                let stripped = AcisData::strip_sat_terminator(&sat_text);
                // R13-R2010 inline modeler streams need both the SAT end
                // marker and the following empty DWG block. Without the SAT
                // marker, readers may reject an otherwise valid object.
                let mut full = stripped;
                full.push_str("End-of-ACIS-data\n");
                let plain = full.as_bytes();

                // Encrypt with selective 159-substitution cipher
                // (per LibreDWG dwg.spec: bytes <= 32 pass through, bytes > 32: 159 - byte)
                let mut encrypted = Vec::with_capacity(plain.len());
                for &b in plain.iter() {
                    if b <= 32 {
                        encrypted.push(b);
                    } else {
                        encrypted.push(159u8.wrapping_sub(b));
                    }
                }

                // Write as a single block + terminating BL(0)
                self.writer.write_bit_long(encrypted.len() as i32);
                self.writer.write_bytes(&encrypted);
                self.writer.write_bit_long(0); // terminating empty block
            }
        }

        let wireframe_present = self.write_acis_wireframe(point, acis, wires, silhouettes);
        if inline || wireframe_present || !self.version.r2013_plus(self.dxf_version) {
            self.writer.write_bit(
                acis.extra_acis_data
                    .as_ref()
                    .map(|_| false)
                    .unwrap_or(acis.acis_empty_bit),
            );
            self.write_extra_acis_data(acis);
        }
        if self.version.r2007_plus() {
            self.writer.write_bit_long(0); // unknown_2007
        }
        if self.version.r2013_plus(self.dxf_version) {
            self.write_acis_revision(&acis.revision);
        }
        true
    }

    /// Write native COMMON_3DSOLID wireframe, silhouette and ISOLINES data.
    fn write_acis_wireframe(
        &mut self,
        point: Vector3,
        acis: &AcisData,
        wires: &[Wire],
        silhouettes: &[Silhouette],
    ) -> bool {
        // ACIS payload presence does not imply an inline wireframe cache.
        // Some valid AcDs-backed solids intentionally carry geometry only
        // (no point, isolines, wires or silhouettes). Synthesizing a cache for
        // those changes COMMON_3DSOLID and ODA rejects the object.
        let wireframe_present = acis.wireframe_data_present
            || point != Vector3::ZERO
            || acis.wireframe_isolines != 0
            || !wires.is_empty()
            || !silhouettes.is_empty();
        self.writer.write_bit(wireframe_present);

        if wireframe_present {
            // Wireframe anchor: the entity's stored reference point (bbox
            // centre in AutoCAD-authored files), falling back to the first
            // wire vertex.
            let anchor = if point != Vector3::ZERO {
                point
            } else {
                wires
                    .first()
                    .and_then(|w| w.points.first().copied())
                    .unwrap_or(Vector3::ZERO)
            };
            let point_present = acis.wireframe_point_present || point != Vector3::ZERO;
            self.writer.write_bit(point_present);
            if point_present {
                self.writer.write_3bit_double(anchor);
            }
            self.writer.write_bit_long(acis.wireframe_isolines);
            let isoline_present = acis.wireframe_isoline_present || !wires.is_empty();
            self.writer.write_bit(isoline_present);

            if isoline_present {
                self.writer.write_bit_long(wires.len() as i32);
                for wire in wires {
                    self.write_wire(wire);
                }
            }

            self.writer.write_bit_long(silhouettes.len() as i32);
            for sil in silhouettes {
                self.writer.write_bit_long_long(sil.viewport_id);
                self.writer.write_3bit_double(sil.target);
                self.writer.write_3bit_double(sil.view_direction);
                self.writer.write_3bit_double(sil.up_vector);
                self.writer.write_bit(sil.is_perspective);
                let has_wires = sil.has_wires || !sil.wires.is_empty();
                self.writer.write_bit(has_wires);
                if !has_wires {
                    continue;
                }
                self.writer.write_bit_long(sil.wires.len() as i32);
                for wire in &sil.wires {
                    self.write_wire(wire);
                }
            }
        }
        wireframe_present
    }

    /// Write a single wire struct (shared by wires and silhouette wires).
    /// Field order/types per LibreDWG `Dwg_3DSOLID_wire` (mirrors the reader):
    /// RC type, BLd selection_marker, BS/BL color, BLd acis_index, BL num_points.
    fn write_wire(&mut self, wire: &Wire) {
        self.writer.write_byte(wire.wire_type as u8);
        self.writer.write_bit_long(wire.selection_marker);
        let color_val: i16 = match wire.color {
            crate::types::Color::ByLayer => 256,
            crate::types::Color::ByBlock => 0,
            crate::types::Color::Index(idx) => idx as i16,
            _ => 256,
        };
        if self.version.r2004_plus() {
            self.writer.write_bit_long(color_val as i32);
        } else {
            self.writer.write_bit_short(color_val);
        }
        self.writer.write_bit_long(wire.acis_index);
        self.writer.write_bit_long(wire.points.len() as i32);
        for pt in &wire.points {
            self.writer.write_3bit_double(*pt);
        }
        self.writer.write_bit(wire.has_transform);
        if wire.has_transform {
            self.writer.write_3bit_double(wire.x_axis);
            self.writer.write_3bit_double(wire.y_axis);
            self.writer.write_3bit_double(wire.z_axis);
            self.writer.write_3bit_double(wire.translation);
            self.writer.write_3bit_double(wire.scale);
            self.writer.write_bit(wire.has_rotation);
            self.writer.write_bit(wire.has_reflection);
            self.writer.write_bit(wire.has_shear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadDocument;
    use crate::entities::{EntityCommon, Line, Point};
    use crate::types::{Handle, Vector3};

    fn make_doc_with_entity(entity: EntityType) -> CadDocument {
        let mut doc = CadDocument::new();
        let handle = entity.common().handle;
        let idx = doc.entities.len();
        doc.entities.push(std::sync::Arc::new(entity));
        doc.entity_index.insert(handle, idx);
        if let Some(br) = doc.block_records.get_mut("*Model_Space") {
            br.entity_handles.push(handle);
        }
        doc
    }

    #[test]
    fn write_point_entity() {
        let pt = Point {
            common: EntityCommon {
                handle: Handle::new(0x100),
                ..Default::default()
            },
            location: Vector3::new(1.0, 2.0, 3.0),
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
            x_axis_angle: 0.0,
        };
        let doc = make_doc_with_entity(EntityType::Point(pt));
        let writer = DwgObjectWriter::new(&doc).unwrap();
        let (output, _map, _, _) = writer.write();
        assert!(!output.is_empty());
    }

    #[test]
    fn write_line_entity() {
        let line = Line {
            common: EntityCommon {
                handle: Handle::new(0x101),
                ..Default::default()
            },
            start: Vector3::new(0.0, 0.0, 0.0),
            end: Vector3::new(10.0, 20.0, 0.0),
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
        };
        let doc = make_doc_with_entity(EntityType::Line(line));
        let writer = DwgObjectWriter::new(&doc).unwrap();
        let (output, _map, _, _) = writer.write();
        assert!(!output.is_empty());
    }
}
