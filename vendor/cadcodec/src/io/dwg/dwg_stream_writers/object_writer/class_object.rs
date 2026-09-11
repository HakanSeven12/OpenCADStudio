//! Writers for native class-registered non-graphical objects.

use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::objects::*;

use super::DwgObjectWriter;

impl<'a> DwgObjectWriter<'a> {
    pub(super) fn write_class_object(&mut self, object: &ClassObject) {
        if matches!(&object.data, ClassObjectData::VbaProject(_)) && !self.version.r2000_plus() {
            return;
        }
        let type_code = if matches!(&object.data, ClassObjectData::VbaProject(_)) {
            crate::io::dwg::dwg_stream_readers::object_reader::common::OBJ_VBA_PROJECT
        } else {
            self.class_type_code(object.dxf_name(), 500)
        };
        if matches!(&object.data, ClassObjectData::ViewRep(_)) {
            self.write_common_non_entity_data_relative_owner(
                type_code,
                object.handle,
                object.owner,
                &object.reactors,
                &object.xdictionary_handle,
            );
        } else {
            self.write_common_non_entity_data(
                type_code,
                object.handle,
                object.owner,
                &object.reactors,
                &object.xdictionary_handle,
            );
        }
        self.write_class_object_data(&object.data);
        self.register_object(object.handle);
    }

    fn write_render_settings_data(&mut self, value: &RenderSettings, rapid_rt: bool) {
        let class_version = if rapid_rt && self.dxf_version == crate::types::DxfVersion::AC1027 {
            value.class_version - 1
        } else if !rapid_rt && self.version.r2013_plus(self.dxf_version) {
            value.class_version + 1
        } else {
            value.class_version
        };
        self.writer.write_bit_long(class_version);
        self.writer.write_variable_text(&value.name);
        self.writer.write_bit(value.fog_enabled);
        self.writer.write_bit(value.fog_background_enabled);
        self.writer.write_bit(value.backfaces_enabled);
        self.writer.write_bit(value.environment_image_enabled);
        self.writer
            .write_variable_text(&value.environment_image_filename);
        self.writer.write_variable_text(&value.description);
        self.writer.write_bit_long(value.display_index);
        if !rapid_rt && self.version.r2013_plus(self.dxf_version) {
            self.writer.write_bit(value.has_predefined);
        }
    }

    fn write_point_cloud_definition(&mut self, value: &PointCloudDefinition) {
        self.writer.write_bit_long(value.class_version);
        self.writer.write_variable_text(&value.source_filename);
        self.writer.write_bit(value.is_loaded);
        self.writer.write_bit_long_long(value.point_count);
        self.writer.write_3bit_double(value.extents_min);
        self.writer.write_3bit_double(value.extents_max);
    }

    fn write_point_cloud_ramps(&mut self, ramps: &[PointCloudColorRamp]) {
        self.writer.write_bit_long(ramps.len() as i32);
        for ramp in ramps {
            self.writer.write_bit_short(ramp.class_version);
            self.writer.write_bit_long(ramp.color_schemes.len() as i32);
            for scheme in &ramp.color_schemes {
                self.writer.write_variable_text(scheme);
            }
        }
    }

    fn write_view_rep_handle(&mut self, kind: DwgReferenceType, value: crate::types::Handle) {
        self.writer.write_handle(kind, value.value());
    }

    fn write_view_rep_geometry(&mut self, value: &ViewRepSketchGeometry) {
        match value {
            ViewRepSketchGeometry::None => self.writer.write_bit_long(0),
            ViewRepSketchGeometry::Line {
                type_code,
                first,
                second,
            } => {
                self.writer.write_bit_long(*type_code);
                self.writer.write_3bit_double(*first);
                self.writer.write_3bit_double(*second);
            }
            ViewRepSketchGeometry::Circle {
                type_code,
                center,
                normal,
                direction,
                radius,
                start_parameter,
                end_parameter,
                reserved,
            } => {
                self.writer.write_bit_long(*type_code);
                self.writer.write_3bit_double(*center);
                self.writer.write_3bit_double(*normal);
                self.writer.write_3bit_double(*direction);
                self.writer.write_bit_double(*radius);
                self.writer.write_bit_double(*start_parameter);
                self.writer.write_bit_double(*end_parameter);
                self.writer.write_bit_double(*reserved);
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
            } => {
                self.writer.write_bit_long(*type_code);
                self.writer.write_bit(flags[0]);
                self.writer.write_bit(flags[1]);
                self.writer.write_bit_short(*degree as i16);
                self.writer.write_bit_double(*tolerance);
                for item in knot_header {
                    self.writer.write_bit_long(*item);
                }
                for item in knots {
                    self.writer.write_bit_double(*item);
                }
                for item in weight_header {
                    self.writer.write_bit_long(*item);
                }
                for item in weights {
                    self.writer.write_bit_double(*item);
                }
                for item in point_header {
                    self.writer.write_bit_long(*item);
                }
                for item in control_points {
                    self.writer.write_3bit_double(*item);
                }
            }
        }
    }

    fn write_view_rep(&mut self, value: &ViewRep) {
        for item in value.header_values {
            self.writer.write_bit_long(item);
        }
        self.writer.write_variable_text(&value.name);
        self.writer.write_bit_long(value.scale);
        self.writer.write_bit_long(value.header_status);
        self.writer.write_variable_text(&value.description);
        self.writer.write_bit_long_long(value.source_id);
        self.writer.write_bit(value.source_enabled);
        self.writer.write_bit_long(value.source_version);
        self.writer.write_bit_long_long(value.model_id);
        self.writer.write_bit_long(value.guid.data1);
        self.writer.write_bit_short(value.guid.data2);
        self.writer.write_bit_short(value.guid.data3);
        for item in value.guid.data4 {
            self.writer.write_byte(item);
        }
        self.writer.write_byte(value.marker);
        for item in value.transform {
            self.writer.write_bit_double(item);
        }
        self.writer.write_bit_long(value.transform_version);
        self.writer.write_bit_long_long(value.database_id);
        self.writer.write_bit_long(value.geometry_version);
        self.writer.write_bit_long(value.geometry_marker);
        self.writer.write_bit_long(value.sketches.len() as i32);
        for sketch in &value.sketches {
            self.writer.write_bit_long(sketch.id);
            self.writer.write_bit_long(sketch.version);
            self.writer.write_bit_long(sketch.references.len() as i32);
            for reference in &sketch.references {
                self.write_view_rep_handle(DwgReferenceType::SoftPointer, reference.object);
                self.writer.write_bit(reference.flag);
            }
            self.writer.write_bit_long(sketch.reserved);
            self.writer.write_bit(sketch.enabled);
            self.write_view_rep_geometry(&sketch.geometry);
            self.writer.write_bit(sketch.final_flag);
        }
        for handle in value.related_objects {
            self.write_view_rep_handle(DwgReferenceType::SoftPointer, handle);
        }
        self.write_view_rep_handle(DwgReferenceType::HardPointer, value.source_manager);
        for handle in value.owned_objects {
            self.write_view_rep_handle(DwgReferenceType::HardOwnership, handle);
        }
        for handle in value.optional_objects {
            self.write_view_rep_handle(DwgReferenceType::SoftPointer, handle);
        }
        self.writer.write_2raw_double(value.position);
        self.writer.write_bit_double(value.rotation);
        self.write_view_rep_handle(DwgReferenceType::HardPointer, value.orientation);
        self.writer.write_bit(value.is_active);
        self.writer.write_bit_short(value.projection);
        for handle in value.linked_views {
            self.write_view_rep_handle(DwgReferenceType::SoftPointer, handle);
        }
        self.writer
            .write_bit_long(value.section_sketches.len() as i32);
        for path in &value.section_sketches {
            self.writer.write_variable_text(&path.class_name);
            self.writer
                .write_bit_short(path.objects.len().saturating_sub(1) as i16);
            for handle in &path.objects {
                self.write_view_rep_handle(DwgReferenceType::SoftPointer, *handle);
            }
        }
        self.writer.write_bit_long(value.action_mode);
        if let Some(action) = value.action {
            self.write_view_rep_handle(DwgReferenceType::HardOwnership, action);
        }
        self.writer.write_bit(value.has_parent);
        self.write_view_rep_handle(DwgReferenceType::SoftPointer, value.parent);
        self.writer.write_bit_long(value.tail_version);
        self.writer.write_bit_long(value.tail_state);
        self.writer.write_bit_long_long(value.tail_id);
        self.writer.write_bit_long(value.path_count);
        self.writer.write_bit_long(value.path_version);
        self.writer.write_bit_long_long(value.path_id);
        self.writer.write_bit(value.has_block_path);
        if let Some(path) = &value.block_path {
            self.writer.write_variable_text(&path.class_name);
            self.writer.write_bit_long(path.version);
            self.writer.write_bit_long(path.entries.len() as i32);
            for entry in &path.entries {
                self.writer.write_byte(entry.flag);
                self.writer.write_byte(entry.kind);
                self.write_view_rep_handle(DwgReferenceType::SoftPointer, entry.object);
            }
        }
        self.write_view_rep_handle(DwgReferenceType::HardPointer, value.style);
    }

    fn write_class_object_data(&mut self, data: &ClassObjectData) {
        match data {
            ClassObjectData::Empty => {}
            ClassObjectData::ViewRepModelSpaceSource(value) => {
                self.writer.write_bit(value.enabled);
                for item in value.header_values {
                    self.writer.write_bit_long(item);
                }
                for item in value.transform {
                    self.writer.write_bit_double(item);
                }
                self.writer.write_bit_long(value.source_version);
                self.writer.write_bit_long(value.source_status);
                self.writer
                    .write_main_handle(DwgReferenceType::Undefined, value.model.value());
                self.writer.write_bit_long(value.guid.data1);
                self.writer.write_bit_short(value.guid.data2);
                self.writer.write_bit_short(value.guid.data3);
                for item in value.guid.data4 {
                    self.writer.write_byte(item);
                }
                for handle in value.references {
                    self.write_view_rep_handle(DwgReferenceType::SoftPointer, handle);
                }
                for item in value.tail_values {
                    self.writer.write_bit_long(item);
                }
                self.write_view_rep_handle(DwgReferenceType::SoftOwnership, value.orientation);
            }
            ClassObjectData::ViewRep(value) => self.write_view_rep(value),
            ClassObjectData::ViewRepSourceManager(value) => {
                self.writer.write_bit(value.has_source);
                self.writer
                    .write_handle(DwgReferenceType::SoftOwnership, value.source.value());
                self.writer.write_bit_long(value.status);
            }
            ClassObjectData::ViewRepStandard(value) => {
                for item in value.values {
                    self.writer.write_bit_long(item);
                }
            }
            ClassObjectData::ViewRepOrientationDefinition => {}
            ClassObjectData::ViewRepOrientation(value) => {
                self.writer.write_3bit_double(value.camera);
                self.writer.write_3bit_double(value.target);
                self.writer.write_3bit_double(value.normal);
            }
            ClassObjectData::ViewRepSectionDefinition(value) => {
                self.writer.write_bit_long(value.version);
                self.writer.write_bit_double(value.section_depth);
                self.writer.write_bit_long(value.flags[0]);
                self.writer.write_bit_long(value.flags[1]);
            }
            ClassObjectData::ViewRepModelSpaceViewSelectionSet(value) => {
                self.writer.write_bit_long(value.version);
                self.writer.write_bit_long(value.entities.len() as i32);
                for handle in &value.entities {
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, handle.value());
                }
            }
            ClassObjectData::SpatialIndex(value) => {
                self.writer.write_bit_long(value.last_updated_julian_day);
                self.writer.write_bit_long(value.last_updated_milliseconds);
                for component in [
                    value.min_corner.x,
                    value.min_corner.y,
                    value.min_corner.z,
                    value.max_corner.x,
                    value.max_corner.y,
                    value.max_corner.z,
                ] {
                    self.writer.write_bit_double(component);
                }
                self.writer
                    .write_bit_long(value.indexed_objects.len() as i32);
                for handle in &value.indexed_objects {
                    self.writer
                        .write_main_handle(DwgReferenceType::HardPointer, handle.value());
                }
                // The undocumented trailing block is only AutoCAD's derived
                // acceleration cache. A zero-length cache is canonical and
                // lets the host rebuild it from the semantic object list.
                self.writer.write_bit_long(0);
            }
            ClassObjectData::LayerFilter(value) => {
                self.writer.write_bit_long(value.names.len() as i32);
                for name in &value.names {
                    self.writer.write_variable_text(name);
                }
            }
            ClassObjectData::PartialViewingIndex(value) => {
                self.writer.write_bit_long(value.entries.len() as i32);
                if !value.entries.is_empty() {
                    self.writer.write_bit(value.has_entries);
                }
                for entry in &value.entries {
                    self.writer.write_3bit_double(entry.extents_min);
                    self.writer.write_3bit_double(entry.extents_max);
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, entry.object.value());
                }
            }
            ClassObjectData::VbaProject(value) => {
                let data = value.storage.encode();
                self.writer.write_bit_long(data.len() as i32);
                self.writer.write_bytes(&data);
            }
            ClassObjectData::SectionManager(value) => {
                self.writer.write_bit(value.is_live);
                self.writer.write_bit_short(value.sections.len() as i16);
                for section in &value.sections {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, section.value());
                }
            }
            ClassObjectData::SectionSettings(value) => {
                self.writer.write_bit_long(value.current_type);
                self.writer.write_bit_long(value.types.len() as i32);
                for section_type in &value.types {
                    self.writer.write_bit_long(section_type.section_type);
                    self.writer.write_bit_long(section_type.generation);
                    self.writer
                        .write_bit_long(section_type.sources.len() as i32);
                    for source in &section_type.sources {
                        self.writer
                            .write_handle(DwgReferenceType::HardPointer, source.value());
                    }
                    self.writer.write_handle(
                        DwgReferenceType::SoftPointer,
                        section_type.destination_block.value(),
                    );
                    self.writer
                        .write_variable_text(&section_type.destination_file);
                    self.writer
                        .write_bit_long(section_type.geometry.len() as i32);
                    for geometry in &section_type.geometry {
                        self.writer.write_bit_long(geometry.geometry_count);
                        self.writer.write_bit_long(geometry.index);
                        self.writer.write_bit_long(geometry.flags);
                        self.writer.write_cm_color(&geometry.color);
                        self.writer.write_variable_text(&geometry.layer);
                        self.writer.write_variable_text(&geometry.linetype);
                        self.writer.write_bit_double(geometry.linetype_scale);
                        self.writer.write_variable_text(&geometry.plot_style);
                        if self.version.r2000_plus() {
                            self.writer.write_bit_long(geometry.lineweight);
                        }
                        self.writer.write_bit_short(geometry.face_transparency);
                        self.writer.write_bit_short(geometry.edge_transparency);
                        self.writer.write_bit_short(geometry.hatch_type);
                        self.writer.write_variable_text(&geometry.hatch_pattern);
                        self.writer.write_bit_double(geometry.hatch_angle);
                        self.writer.write_bit_double(geometry.hatch_spacing);
                        self.writer.write_bit_double(geometry.hatch_scale);
                    }
                }
            }
            ClassObjectData::LightList(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.lights.len() as i32);
                for light in &value.lights {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, light.handle.value());
                    self.writer.write_variable_text(&light.name);
                }
            }
            ClassObjectData::Sun(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit(value.is_on);
                self.writer.write_cm_color(&value.color);
                self.writer.write_bit_double(value.intensity);
                self.writer.write_bit(value.has_shadow);
                self.writer.write_bit_long(value.julian_day);
                self.writer.write_bit_long(value.milliseconds);
                self.writer.write_bit(value.is_daylight_savings_on);
                self.writer.write_bit_long(value.shadow_type);
                self.writer.write_bit_short(value.shadow_map_size);
                self.writer.write_byte(value.shadow_softness);
            }
            ClassObjectData::RenderSettings(value) => {
                self.write_render_settings_data(value, false);
            }
            ClassObjectData::MentalRayRenderSettings(value) => {
                self.write_render_settings_data(&value.base, false);
                self.writer.write_bit_long(value.version);
                self.writer.write_bit_long(value.sampling_min);
                self.writer.write_bit_long(value.sampling_max);
                self.writer.write_bit_short(value.sampling_filter);
                self.writer.write_bit_double(value.sampling_filter_width);
                self.writer.write_bit_double(value.sampling_filter_height);
                for component in value.sampling_contrast {
                    self.writer.write_bit_double(component);
                }
                self.writer.write_bit_short(value.shadow_mode);
                self.writer.write_bit(value.shadow_maps_enabled);
                self.writer.write_bit(value.ray_tracing_enabled);
                for depth in value.ray_trace_depth {
                    self.writer.write_bit_long(depth);
                }
                self.writer.write_bit(value.global_illumination_enabled);
                self.writer
                    .write_bit_long(value.global_illumination_sample_count);
                self.writer
                    .write_bit(value.global_illumination_sample_radius_enabled);
                self.writer
                    .write_bit_double(value.global_illumination_sample_radius);
                self.writer.write_bit_long(value.photons_per_light);
                for depth in value.photon_trace_depth {
                    self.writer.write_bit_long(depth);
                }
                self.writer.write_bit(value.final_gathering_enabled);
                self.writer.write_bit_long(value.final_gathering_ray_count);
                for state in value.final_gathering_sample_radius_state {
                    self.writer.write_bit(state);
                }
                for radius in value.final_gathering_sample_radius {
                    self.writer.write_bit_double(radius);
                }
                self.writer.write_bit_double(value.light_luminance_scale);
                self.writer.write_bit_short(value.diagnostics_mode);
                self.writer.write_bit_short(value.diagnostics_grid_mode);
                self.writer.write_bit_double(value.diagnostics_grid_size);
                self.writer.write_bit_short(value.diagnostics_photon_mode);
                self.writer.write_bit_short(value.diagnostics_bsp_mode);
                self.writer.write_bit(value.export_mi_enabled);
                self.writer.write_variable_text(&value.description);
                self.writer.write_bit_long(value.tile_size);
                self.writer.write_bit_short(value.tile_order);
                self.writer.write_bit_long(value.memory_limit);
                self.writer.write_bit(value.diagnostics_samples_mode);
                self.writer.write_bit_double(value.energy_multiplier);
            }
            ClassObjectData::RapidRtRenderSettings(value) => {
                self.write_render_settings_data(&value.base, true);
                self.writer.write_bit_long(value.version);
                self.writer.write_bit_long(value.render_target);
                self.writer.write_bit_long(value.render_level);
                self.writer.write_bit_long(value.render_time);
                self.writer.write_bit_long(value.lighting_model);
                self.writer.write_bit_long(value.filter_type);
                self.writer.write_bit_double(value.filter_width);
                self.writer.write_bit_double(value.filter_height);
                self.writer.write_bit(value.base.has_predefined);
            }
            ClassObjectData::GradientBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.color_top as i32);
                self.writer.write_bit_long(value.color_middle as i32);
                self.writer.write_bit_long(value.color_bottom as i32);
                self.writer.write_bit_double(value.horizon);
                self.writer.write_bit_double(value.height);
                self.writer.write_bit_double(value.rotation);
            }
            ClassObjectData::GroundPlaneBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.color_sky_zenith as i32);
                self.writer.write_bit_long(value.color_sky_horizon as i32);
                self.writer
                    .write_bit_long(value.color_underground_horizon as i32);
                self.writer
                    .write_bit_long(value.color_underground_azimuth as i32);
                self.writer.write_bit_long(value.color_near as i32);
                self.writer.write_bit_long(value.color_far as i32);
            }
            ClassObjectData::IblBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit(value.enabled);
                self.writer.write_variable_text(&value.name);
                self.writer.write_bit_double(value.rotation);
                self.writer.write_bit(value.display_image);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.secondary_background.value(),
                );
            }
            ClassObjectData::ImageBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_variable_text(&value.filename);
                self.writer.write_bit(value.fit_to_screen);
                self.writer.write_bit(value.maintain_aspect_ratio);
                self.writer.write_bit(value.use_tiling);
                self.writer.write_2bit_double(value.offset);
                self.writer.write_2bit_double(value.scale);
            }
            ClassObjectData::SkyLightBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.sun.value());
            }
            ClassObjectData::SolidBackground(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.color as i32);
            }
            ClassObjectData::RenderEntry(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_variable_text(&value.image_filename);
                self.writer.write_variable_text(&value.preset_name);
                self.writer.write_variable_text(&value.view_name);
                self.writer.write_bit_long(value.width);
                self.writer.write_bit_long(value.height);
                self.writer.write_bit_short(value.start_year);
                self.writer.write_bit_short(value.start_month);
                self.writer.write_bit_short(value.start_day);
                self.writer.write_bit_short(value.start_hour);
                self.writer.write_bit_short(value.start_minute);
                self.writer.write_bit_short(value.start_second);
                self.writer.write_bit_short(value.start_millisecond);
                self.writer.write_bit_short(value.end_year);
                self.writer.write_bit_short(value.end_month);
                self.writer.write_bit_short(value.end_day);
                self.writer.write_bit_short(value.end_hour);
                self.writer.write_bit_short(value.end_minute);
                self.writer.write_bit_short(value.end_second);
                self.writer.write_bit_short(value.end_millisecond);
                self.writer.write_bit_double(value.render_time);
                self.writer.write_bit_long(value.memory_amount);
                self.writer.write_bit_long(value.material_count);
                self.writer.write_bit_long(value.light_count);
                self.writer.write_bit_long(value.triangle_count);
                self.writer.write_bit_long(value.display_index);
            }
            ClassObjectData::RenderEnvironment(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit(value.fog_enabled);
                self.writer.write_bit(value.fog_background_enabled);
                for component in value.fog_color {
                    self.writer.write_byte(component);
                }
                self.writer.write_bit_double(value.fog_density_near);
                self.writer.write_bit_double(value.fog_density_far);
                self.writer.write_bit_double(value.fog_distance_near);
                self.writer.write_bit_double(value.fog_distance_far);
                self.writer.write_bit(value.environment_image_enabled);
                self.writer
                    .write_variable_text(&value.environment_image_filename);
            }
            ClassObjectData::RenderGlobal(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.procedure);
                self.writer.write_bit_long(value.destination);
                self.writer.write_bit(value.save_enabled);
                self.writer.write_variable_text(&value.save_filename);
                self.writer.write_bit_long(value.image_width);
                self.writer.write_bit_long(value.image_height);
                self.writer.write_bit(value.predefined_presets_first);
                self.writer.write_bit(value.high_level_info);
            }
            ClassObjectData::MotionPath(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.camera_path.value());
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.target_path.value());
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.view.value());
                self.writer.write_bit_short(value.frames);
                self.writer.write_bit_short(value.frame_rate);
                self.writer.write_bit(value.corner_deceleration);
            }
            ClassObjectData::CurvePath(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.entity.value());
            }
            ClassObjectData::PointPath(value) => {
                self.writer.write_bit_short(value.class_version);
                self.writer.write_3bit_double(value.point);
            }
            ClassObjectData::TvDeviceProperties(value) => {
                self.writer.write_bit_long(value.flags as i32);
                self.writer.write_bit_short(value.max_regen_threads);
                self.writer.write_bit_long(value.use_lut_palette);
                self.writer.write_bit_long_long(value.alternate_highlight);
                self.writer
                    .write_bit_long_long(value.alternate_highlight_color);
                self.writer.write_bit_long_long(value.geometry_shader_usage);
                self.writer.write_bit_long(value.blending_mode);
                self.writer.write_bit_double(value.antialiasing_level);
                self.writer.write_bit_double(value.reserved_double);
            }
            ClassObjectData::PointCloudDefinition(value)
            | ClassObjectData::PointCloudDefinitionEx(value) => {
                self.write_point_cloud_definition(value);
            }
            ClassObjectData::PointCloudDefinitionReactor(value)
            | ClassObjectData::PointCloudDefinitionReactorEx(value) => {
                self.writer.write_bit_long(value.class_version);
            }
            ClassObjectData::PointCloudColorMap(value) => {
                self.writer.write_bit_short(value.class_version);
                self.writer
                    .write_variable_text(&value.default_intensity_scheme);
                self.writer
                    .write_variable_text(&value.default_elevation_scheme);
                self.writer
                    .write_variable_text(&value.default_classification_scheme);
                self.write_point_cloud_ramps(&value.color_ramps);
                self.write_point_cloud_ramps(&value.classification_color_ramps);
            }
            ClassObjectData::NavisworksModelDefinition(value) => {
                self.writer.write_bit_short(value.flags);
                self.writer.write_variable_text(&value.path);
                self.writer.write_bit(value.status);
                self.writer.write_3bit_double(value.extents_min);
                self.writer.write_3bit_double(value.extents_max);
                self.writer.write_bit(value.host_drawing_visibility);
            }
            ClassObjectData::ContextDataManager(value) => {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.object_context.value());
                self.writer.write_bit_long(value.sub_managers.len() as i32);
                for manager in &value.sub_managers {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, manager.handle.value());
                    self.writer.write_bit_long(manager.entries.len() as i32);
                    for entry in &manager.entries {
                        self.writer
                            .write_handle(DwgReferenceType::SoftOwnership, entry.item.value());
                        self.writer.write_variable_text(&entry.name);
                    }
                }
            }
            ClassObjectData::SunStudy(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_variable_text(&value.setup_name);
                self.writer.write_variable_text(&value.description);
                self.writer.write_bit_long(value.output_type);
                if value.output_type == 0 {
                    self.writer.write_bit(value.use_subset);
                    self.writer.write_variable_text(&value.sheet_set_name);
                    self.writer.write_variable_text(&value.sheet_subset_name);
                }
                self.writer.write_bit(value.select_dates_from_calendar);
                self.writer.write_bit_long(value.dates.len() as i32);
                for date in &value.dates {
                    self.writer.write_bit_long(date.julian_day);
                    self.writer.write_bit_long(date.milliseconds);
                }
                self.writer.write_bit(value.select_range_of_dates);
                if value.select_range_of_dates {
                    self.writer.write_bit_long(value.start_time);
                    self.writer.write_bit_long(value.end_time);
                    self.writer.write_bit_long(value.interval);
                }
                self.writer.write_bit_long(value.hours.len() as i32);
                for hour in &value.hours {
                    self.writer.write_bit(*hour);
                }
                self.writer.write_bit_long(value.shade_plot_type);
                self.writer.write_bit_long(value.viewport_count);
                self.writer.write_bit_long(value.rows);
                self.writer.write_bit_long(value.columns);
                self.writer.write_bit_double(value.spacing);
                self.writer.write_bit(value.lock_viewports);
                self.writer.write_bit(value.label_viewports);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.page_setup_wizard.value(),
                );
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.view.value());
                self.writer
                    .write_handle(DwgReferenceType::SoftOwnership, value.visual_style.value());
                self.writer
                    .write_handle(DwgReferenceType::SoftOwnership, value.text_style.value());
            }
            ClassObjectData::DataTable(value) => {
                self.writer.write_bit_short(value.flags);
                self.writer.write_bit_long(value.columns.len() as i32);
                self.writer.write_bit_long(value.row_count);
                self.writer.write_variable_text(&value.name);
                for column in &value.columns {
                    self.writer.write_bit_long(column.value_type);
                    self.writer.write_variable_text(&column.name);
                    for row in &column.rows {
                        self.writer.write_bit_long(row.integer);
                        self.writer.write_bit_double(row.real);
                        self.writer.write_variable_text(&row.text);
                    }
                }
            }
            ClassObjectData::DataLink(value) => {
                self.writer.write_variable_text(&value.data_adapter);
                self.writer.write_variable_text(&value.description);
                self.writer.write_variable_text(&value.tooltip);
                self.writer.write_variable_text(&value.connection_string);
                self.writer.write_bit_long(value.option);
                self.writer.write_bit_long(value.update_option);
                self.writer.write_bit_long(value.flags);
                self.writer.write_bit_short(value.year);
                self.writer.write_bit_short(value.month);
                self.writer.write_bit_short(value.day);
                self.writer.write_bit_short(value.hour);
                self.writer.write_bit_short(value.minute);
                self.writer.write_bit_short(value.second);
                self.writer.write_bit_short(value.millisecond);
                self.writer.write_bit_short(value.path_option);
                self.writer.write_bit_long(value.status_flags);
                self.writer.write_variable_text(&value.update_status);
                self.writer.write_bit_long(value.custom_data.len() as i32);
                for item in &value.custom_data {
                    self.writer
                        .write_handle(DwgReferenceType::HardOwnership, item.target.value());
                    self.writer.write_variable_text(&item.value);
                }
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, value.hard_owner.value());
            }
            ClassObjectData::PersistentSubentityManager(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_bit_long(value.reserved_zero);
                self.writer.write_bit_long(value.reserved_two);
                self.writer.write_bit_long(value.associated_step_count);
                self.writer.write_bit_long(value.associated_subentity_count);
                self.writer.write_bit_long(value.steps.len() as i32);
                for step in &value.steps {
                    self.writer.write_bit_long(*step);
                }
                self.writer.write_bit_long(value.subentities.len() as i32);
                for subentity in &value.subentities {
                    self.writer.write_bit_long(*subentity);
                }
            }
            ClassObjectData::GeoMapImage(value) => {
                self.writer.write_bit_long(value.class_version);
                self.writer.write_raw_double(value.origin.x);
                self.writer.write_raw_double(value.origin.y);
                self.writer.write_raw_double(value.origin.z);
                self.writer.write_2raw_double(value.image_size);
                self.writer.write_bit_short(value.display_properties);
                self.writer.write_bit(value.clipping_enabled);
                self.writer.write_byte(value.brightness);
                self.writer.write_byte(value.contrast);
                self.writer.write_byte(value.fade);
            }
            ClassObjectData::DetailViewStyle(value) => {
                self.writer.write_bit_short(value.base.class_version);
                self.writer.write_variable_text(&value.base.description);
                self.writer.write_bit(value.base.modified_for_recompute);
                if self.version.r2018_plus(self.dxf_version) {
                    self.writer.write_variable_text(&value.base.display_name);
                    self.writer.write_bit_long(value.base.flags);
                }
                self.writer.write_bit_short(value.class_version);
                self.writer.write_bit_long(value.flags);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.identifier_style.value(),
                );
                self.writer.write_cm_color(&value.identifier_color);
                self.writer.write_bit_double(value.identifier_height);
                self.writer
                    .write_variable_text(&value.identifier_excluded_characters);
                self.writer.write_bit_double(value.identifier_offset);
                self.writer.write_byte(value.identifier_placement);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.arrow_symbol.value());
                self.writer.write_cm_color(&value.arrow_symbol_color);
                self.writer.write_bit_double(value.arrow_symbol_size);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.boundary_linetype.value(),
                );
                self.writer.write_bit_long(value.boundary_lineweight);
                self.writer.write_cm_color(&value.boundary_color);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.view_label_text_style.value(),
                );
                self.writer.write_cm_color(&value.view_label_text_color);
                self.writer.write_bit_double(value.view_label_text_height);
                self.writer.write_bit_long(value.view_label_attachment);
                self.writer.write_bit_double(value.view_label_offset);
                self.writer.write_bit_long(value.view_label_alignment);
                self.writer.write_variable_text(&value.view_label_pattern);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.connection_linetype.value(),
                );
                self.writer.write_bit_long(value.connection_lineweight);
                self.writer.write_cm_color(&value.connection_color);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.border_linetype.value());
                self.writer.write_bit_long(value.border_lineweight);
                self.writer.write_cm_color(&value.border_color);
                self.writer.write_byte(value.model_edge);
            }
            ClassObjectData::SectionViewStyle(value) => {
                self.writer.write_bit_short(value.base.class_version);
                self.writer.write_variable_text(&value.base.description);
                self.writer.write_bit(value.base.modified_for_recompute);
                if self.version.r2018_plus(self.dxf_version) {
                    self.writer.write_variable_text(&value.base.display_name);
                    self.writer.write_bit_long(value.base.flags);
                }
                self.writer.write_bit_short(value.class_version);
                self.writer.write_bit_long(value.flags);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.identifier_style.value(),
                );
                self.writer.write_cm_color(&value.identifier_color);
                self.writer.write_bit_double(value.identifier_height);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.arrow_start_symbol.value(),
                );
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.arrow_end_symbol.value(),
                );
                self.writer.write_cm_color(&value.arrow_symbol_color);
                self.writer.write_bit_double(value.arrow_symbol_size);
                self.writer
                    .write_variable_text(&value.identifier_excluded_characters);
                self.writer
                    .write_bit_double(value.arrow_symbol_extension_length);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.plane_linetype.value());
                self.writer.write_bit_long(value.plane_lineweight);
                self.writer.write_cm_color(&value.plane_color);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.bend_linetype.value());
                self.writer.write_bit_long(value.bend_lineweight);
                self.writer.write_cm_color(&value.bend_color);
                self.writer.write_bit_double(value.bend_line_length);
                self.writer.write_bit_double(value.end_line_length);
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.view_label_text_style.value(),
                );
                self.writer.write_cm_color(&value.view_label_text_color);
                self.writer.write_bit_double(value.view_label_text_height);
                self.writer.write_bit_long(value.view_label_attachment);
                self.writer.write_bit_double(value.view_label_offset);
                self.writer.write_bit_long(value.view_label_alignment);
                self.writer.write_variable_text(&value.view_label_pattern);
                self.writer.write_cm_color(&value.hatch_color);
                self.writer.write_cm_color(&value.hatch_background_color);
                self.writer.write_variable_text(&value.hatch_pattern);
                self.writer.write_bit_double(value.hatch_scale);
                self.writer.write_bit_long(value.hatch_transparency);
                self.writer.write_bit(value.reserved_flags[0]);
                self.writer.write_bit(value.reserved_flags[1]);
                self.writer.write_bit_long(value.identifier_position);
                self.writer.write_bit_double(value.identifier_offset);
                self.writer.write_bit_long(value.arrow_position);
                self.writer.write_bit_double(value.end_line_overshoot);
                self.writer.write_bit_long(value.hatch_angles.len() as i32);
                for angle in &value.hatch_angles {
                    self.writer.write_bit_double(*angle);
                }
            }
            ClassObjectData::AcMeCommandHistory(value) => {
                self.writer.write_bit_short(value.class_version);
            }
            ClassObjectData::AcMeScope(value) => {
                self.writer.write_bit_short(value.class_version);
            }
            ClassObjectData::AcMeStateManager(value) => {
                self.writer.write_bit_short(value.class_version);
            }
            ClassObjectData::CsacDocumentOptions(value) => {
                self.writer.write_bit_short(value.class_version);
            }
        }
    }
}
