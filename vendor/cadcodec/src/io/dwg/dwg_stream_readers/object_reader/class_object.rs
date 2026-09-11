//! Readers for native class-registered non-graphical objects.

use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::objects::*;
use crate::types::{DxfVersion, Handle, Vector3};

const MAX_ITEMS: i32 = 100_000;

fn count(value: i32) -> usize {
    value.max(0).min(MAX_ITEMS) as usize
}

fn read_render_settings(
    reader: &mut DwgMergedReader,
    version: DwgVersion,
    dxf_version: DxfVersion,
    rapid_rt: bool,
) -> RenderSettings {
    let stored_version = reader.read_bit_long();
    RenderSettings {
        class_version: if rapid_rt && dxf_version == DxfVersion::AC1027 {
            stored_version + 1
        } else if !rapid_rt && version.r2013_plus(dxf_version) {
            stored_version - 1
        } else {
            stored_version
        },
        name: reader.read_variable_text(),
        fog_enabled: reader.read_bit(),
        fog_background_enabled: reader.read_bit(),
        backfaces_enabled: reader.read_bit(),
        environment_image_enabled: reader.read_bit(),
        environment_image_filename: reader.read_variable_text(),
        description: reader.read_variable_text(),
        display_index: reader.read_bit_long(),
        has_predefined: if !rapid_rt && version.r2013_plus(dxf_version) {
            reader.read_bit()
        } else {
            false
        },
    }
}

fn read_point_cloud_ramps(reader: &mut DwgMergedReader) -> Vec<PointCloudColorRamp> {
    let mut result = Vec::new();
    for _ in 0..count(reader.read_bit_long()) {
        let class_version = reader.read_bit_short();
        let mut color_schemes = Vec::new();
        for _ in 0..count(reader.read_bit_long()) {
            color_schemes.push(reader.read_variable_text());
        }
        result.push(PointCloudColorRamp {
            class_version,
            color_schemes,
        });
    }
    result
}

fn read_point_cloud_definition(reader: &mut DwgMergedReader) -> PointCloudDefinition {
    PointCloudDefinition {
        class_version: reader.read_bit_long(),
        source_filename: reader.read_variable_text(),
        is_loaded: reader.read_bit(),
        point_count: reader.read_bit_long_long(),
        extents_min: reader.read_3bit_double(),
        extents_max: reader.read_3bit_double(),
    }
}

fn read_view_rep_handle(reader: &mut DwgMergedReader) -> Handle {
    Handle::from(reader.read_handle())
}

fn read_view_rep_guid(reader: &mut DwgMergedReader) -> ViewRepGuid {
    let data1 = reader.read_bit_long();
    let data2 = reader.read_bit_short();
    let data3 = reader.read_bit_short();
    let mut data4 = [0; 8];
    for item in &mut data4 {
        *item = reader.read_byte();
    }
    ViewRepGuid {
        data1,
        data2,
        data3,
        data4,
    }
}

fn read_view_rep_sketch_geometry(
    reader: &mut DwgMergedReader,
    type_code: i32,
) -> ViewRepSketchGeometry {
    match type_code {
        19 | 23 => ViewRepSketchGeometry::Line {
            type_code,
            first: reader.read_3bit_double(),
            second: reader.read_3bit_double(),
        },
        11 => ViewRepSketchGeometry::Circle {
            type_code,
            center: reader.read_3bit_double(),
            normal: reader.read_3bit_double(),
            direction: reader.read_3bit_double(),
            radius: reader.read_bit_double(),
            start_parameter: reader.read_bit_double(),
            end_parameter: reader.read_bit_double(),
            reserved: reader.read_bit_double(),
        },
        42 => {
            let flags = [reader.read_bit(), reader.read_bit()];
            let degree = reader.read_bit_short() as i32;
            let tolerance = reader.read_bit_double();
            let knot_header = [
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
            ];
            let mut knots = Vec::with_capacity(count(knot_header[0]));
            for _ in 0..count(knot_header[0]) {
                knots.push(reader.read_bit_double());
            }
            let weight_header = [
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
            ];
            let mut weights = Vec::with_capacity(count(weight_header[0]));
            for _ in 0..count(weight_header[0]) {
                weights.push(reader.read_bit_double());
            }
            let point_header = [
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
            ];
            let mut control_points = Vec::with_capacity(count(point_header[0]));
            for _ in 0..count(point_header[0]) {
                control_points.push(reader.read_3bit_double());
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
    }
}

fn read_view_rep(reader: &mut DwgMergedReader) -> ViewRep {
    let header_values = [
        reader.read_bit_long(),
        reader.read_bit_long(),
        reader.read_bit_long(),
        reader.read_bit_long(),
        reader.read_bit_long(),
    ];
    let name = reader.read_variable_text();
    let scale = reader.read_bit_long();
    let header_status = reader.read_bit_long();
    let description = reader.read_variable_text();
    let source_id = reader.read_bit_long_long();
    let source_enabled = reader.read_bit();
    let source_version = reader.read_bit_long();
    let model_id = reader.read_bit_long_long();
    let guid = read_view_rep_guid(reader);
    let marker = reader.read_byte();
    let mut transform = [0.0; 16];
    for item in &mut transform {
        *item = reader.read_bit_double();
    }
    let transform_version = reader.read_bit_long();
    let database_id = reader.read_bit_long_long();
    let geometry_version = reader.read_bit_long();
    let geometry_marker = reader.read_bit_long();
    let mut sketches = Vec::new();
    for _ in 0..count(reader.read_bit_long()) {
        let id = reader.read_bit_long();
        let sketch_version = reader.read_bit_long();
        let mut references = Vec::new();
        for _ in 0..count(reader.read_bit_long()) {
            references.push(ViewRepSketchReference {
                object: read_view_rep_handle(reader),
                flag: reader.read_bit(),
            });
        }
        let reserved = reader.read_bit_long();
        let enabled = reader.read_bit();
        let type_code = reader.read_bit_long();
        let geometry = read_view_rep_sketch_geometry(reader, type_code);
        let final_flag = reader.read_bit();
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
    let related_objects = [read_view_rep_handle(reader), read_view_rep_handle(reader)];
    let source_manager = read_view_rep_handle(reader);
    let owned_objects = [read_view_rep_handle(reader), read_view_rep_handle(reader)];
    let optional_objects = [read_view_rep_handle(reader), read_view_rep_handle(reader)];
    let position = reader.read_2raw_double();
    let rotation = reader.read_bit_double();
    let orientation = read_view_rep_handle(reader);
    let is_active = reader.read_bit();
    let projection = reader.read_bit_short();
    let linked_views = [read_view_rep_handle(reader), read_view_rep_handle(reader)];
    let mut section_sketches = Vec::new();
    for _ in 0..count(reader.read_bit_long()) {
        let class_name = reader.read_variable_text();
        let path_count = count(reader.read_bit_short() as i32);
        let mut objects = Vec::with_capacity(path_count.saturating_add(1));
        for _ in 0..=path_count {
            objects.push(read_view_rep_handle(reader));
        }
        section_sketches.push(ViewRepObjectPath {
            class_name,
            objects,
        });
    }
    let action_mode = reader.read_bit_long();
    let action = if action_mode != 0 {
        Some(read_view_rep_handle(reader))
    } else {
        None
    };
    let has_parent = reader.read_bit();
    let parent = read_view_rep_handle(reader);
    let tail_version = reader.read_bit_long();
    let tail_state = reader.read_bit_long();
    let tail_id = reader.read_bit_long_long();
    let path_count = reader.read_bit_long();
    let path_version = reader.read_bit_long();
    let path_id = reader.read_bit_long_long();
    let has_block_path = reader.read_bit();
    let block_path = if has_block_path {
        let class_name = reader.read_variable_text();
        let block_path_version = reader.read_bit_long();
        let mut entries = Vec::new();
        for _ in 0..count(reader.read_bit_long()) {
            entries.push(ViewRepBlockPathEntry {
                flag: reader.read_byte(),
                kind: reader.read_byte(),
                object: read_view_rep_handle(reader),
            });
        }
        Some(ViewRepBlockPath {
            class_name,
            version: block_path_version,
            entries,
        })
    } else {
        None
    };
    let style = read_view_rep_handle(reader);
    ViewRep {
        header_values,
        name,
        scale,
        header_status,
        description,
        source_id,
        source_enabled,
        source_version,
        model_id,
        guid,
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
    }
}

pub fn read_class_object_data(
    reader: &mut DwgMergedReader,
    dxf_name: &str,
    version: DwgVersion,
    dxf_version: DxfVersion,
) -> Option<ClassObjectData> {
    let name = dxf_name.to_uppercase();
    let data = match name.as_str() {
        "ACDBVIEWREPMODELSPACESOURCE" => {
            let enabled = reader.read_bit();
            let header_values = [
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
            ];
            let mut transform = [0.0; 16];
            for item in &mut transform {
                *item = reader.read_bit_double();
            }
            let source_version = reader.read_bit_long();
            let source_status = reader.read_bit_long();
            let model = Handle::from(reader.read_main_handle());
            let guid = read_view_rep_guid(reader);
            let references = [read_view_rep_handle(reader), read_view_rep_handle(reader)];
            let tail_values = [reader.read_bit_long(), reader.read_bit_long()];
            let orientation = read_view_rep_handle(reader);
            ClassObjectData::ViewRepModelSpaceSource(ViewRepModelSpaceSource {
                enabled,
                header_values,
                transform,
                source_version,
                source_status,
                model,
                guid,
                references,
                tail_values,
                orientation,
            })
        }
        "ACDBVIEWREP" => ClassObjectData::ViewRep(read_view_rep(reader)),
        "ACDBVIEWREPSOURCEMGR" => ClassObjectData::ViewRepSourceManager(ViewRepSourceManager {
            has_source: reader.read_bit(),
            source: Handle::from(reader.read_handle()),
            status: reader.read_bit_long(),
        }),
        "ACDBVIEWREPSTANDARD" => ClassObjectData::ViewRepStandard(ViewRepStandard {
            values: [
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
                reader.read_bit_long(),
            ],
        }),
        "ACDBVIEWREPORIENTATIONDEF" => ClassObjectData::ViewRepOrientationDefinition,
        "ACDBVIEWREPORIENTATION" => ClassObjectData::ViewRepOrientation(ViewRepOrientation {
            camera: reader.read_3bit_double(),
            target: reader.read_3bit_double(),
            normal: reader.read_3bit_double(),
        }),
        "ACDBVIEWREPSECTIONDEFINITION" => {
            ClassObjectData::ViewRepSectionDefinition(ViewRepSectionDefinition {
                version: reader.read_bit_long(),
                section_depth: reader.read_bit_double(),
                flags: [reader.read_bit_long(), reader.read_bit_long()],
            })
        }
        "ACDBSYMODELSPACEVIEWSELSET" => {
            let version = reader.read_bit_long();
            let mut entities = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                entities.push(Handle::from(reader.read_handle()));
            }
            ClassObjectData::ViewRepModelSpaceViewSelectionSet(ViewRepModelSpaceViewSelectionSet {
                version,
                entities,
            })
        }
        "SPATIAL_INDEX" => {
            let last_updated_julian_day = reader.read_bit_long();
            let last_updated_milliseconds = reader.read_bit_long();
            let min_corner = crate::types::Vector3::new(
                reader.read_bit_double(),
                reader.read_bit_double(),
                reader.read_bit_double(),
            );
            let max_corner = crate::types::Vector3::new(
                reader.read_bit_double(),
                reader.read_bit_double(),
                reader.read_bit_double(),
            );
            let mut indexed_objects = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                // AcDbSpatialIndex stores its indexed-entity IDs inline in
                // the main data stream, not in the trailing object-handle
                // stream. The latter contains only the ordinary common-object
                // owner/reactor references.
                indexed_objects.push(Handle::from(reader.read_main_handle()));
            }
            let binary_size = count(reader.read_bit_long());
            // Autodesk documents the trailing binary block as an internal,
            // ignorable acceleration cache. Consume it to keep the stream
            // aligned; the semantic index is the extents + ordered object IDs.
            let _cached_acceleration_data = reader.read_bytes(binary_size);
            ClassObjectData::SpatialIndex(SpatialIndex {
                last_updated_julian_day,
                last_updated_milliseconds,
                min_corner,
                max_corner,
                indexed_objects,
            })
        }
        "LAYERFILTER" => {
            let mut names = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                names.push(reader.read_variable_text());
            }
            ClassObjectData::LayerFilter(LayerFilter { names })
        }
        "PARTIAL_VIEWING_INDEX" => {
            let num_entries = count(reader.read_bit_long());
            let has_entries = num_entries != 0 && reader.read_bit();
            let mut entries = Vec::with_capacity(num_entries);
            for _ in 0..num_entries {
                entries.push(PartialViewingIndexEntry {
                    extents_min: reader.read_3bit_double(),
                    extents_max: reader.read_3bit_double(),
                    object: Handle::from(reader.read_handle()),
                });
            }
            ClassObjectData::PartialViewingIndex(PartialViewingIndex {
                has_entries,
                entries,
            })
        }
        "VBA_PROJECT" => {
            if !version.r2000_plus() {
                return None;
            }
            let size = count(reader.read_bit_long());
            ClassObjectData::VbaProject(VbaProject {
                storage: crate::compound_file::StructuredStoragePayload::decode(
                    &reader.read_bytes(size),
                ),
            })
        }
        "SECTION_MANAGER" => {
            let is_live = reader.read_bit();
            let mut sections = Vec::new();
            for _ in 0..count(reader.read_bit_short() as i32) {
                sections.push(Handle::from(reader.read_handle()));
            }
            ClassObjectData::SectionManager(SectionManager { is_live, sections })
        }
        "SECTION_SETTINGS" => {
            let current_type = reader.read_bit_long();
            let mut types = Vec::new();
            for _ in 0..count(reader.read_bit_long()).min(4) {
                let section_type = reader.read_bit_long();
                let generation = reader.read_bit_long();
                let mut sources = Vec::new();
                for _ in 0..count(reader.read_bit_long()) {
                    sources.push(Handle::from(reader.read_handle()));
                }
                let destination_block = Handle::from(reader.read_handle());
                let destination_file = reader.read_variable_text();
                let mut geometry = Vec::new();
                for _ in 0..count(reader.read_bit_long()) {
                    geometry.push(SectionGeometrySettings {
                        geometry_count: reader.read_bit_long(),
                        index: reader.read_bit_long(),
                        flags: reader.read_bit_long(),
                        color: reader.read_cm_color(),
                        layer: reader.read_variable_text(),
                        linetype: reader.read_variable_text(),
                        linetype_scale: reader.read_bit_double(),
                        plot_style: reader.read_variable_text(),
                        lineweight: if version.r2000_plus() {
                            reader.read_bit_long()
                        } else {
                            0
                        },
                        face_transparency: reader.read_bit_short(),
                        edge_transparency: reader.read_bit_short(),
                        hatch_type: reader.read_bit_short(),
                        hatch_pattern: reader.read_variable_text(),
                        hatch_angle: reader.read_bit_double(),
                        hatch_spacing: reader.read_bit_double(),
                        hatch_scale: reader.read_bit_double(),
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
            let class_version = reader.read_bit_long();
            let mut lights = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                lights.push(LightListEntry {
                    handle: Handle::from(reader.read_handle()),
                    name: reader.read_variable_text(),
                });
            }
            ClassObjectData::LightList(LightList {
                class_version,
                lights,
            })
        }
        "SUN" => ClassObjectData::Sun(Sun {
            class_version: reader.read_bit_long(),
            is_on: reader.read_bit(),
            color: reader.read_cm_color(),
            intensity: reader.read_bit_double(),
            has_shadow: reader.read_bit(),
            julian_day: reader.read_bit_long(),
            milliseconds: reader.read_bit_long(),
            is_daylight_savings_on: reader.read_bit(),
            shadow_type: reader.read_bit_long(),
            shadow_map_size: reader.read_bit_short(),
            shadow_softness: reader.read_byte(),
        }),
        "RENDERSETTINGS" => ClassObjectData::RenderSettings(read_render_settings(
            reader,
            version,
            dxf_version,
            false,
        )),
        "MENTALRAYRENDERSETTINGS" => {
            let base = read_render_settings(reader, version, dxf_version, false);
            ClassObjectData::MentalRayRenderSettings(MentalRayRenderSettings {
                base,
                version: reader.read_bit_long(),
                sampling_min: reader.read_bit_long(),
                sampling_max: reader.read_bit_long(),
                sampling_filter: reader.read_bit_short(),
                sampling_filter_width: reader.read_bit_double(),
                sampling_filter_height: reader.read_bit_double(),
                sampling_contrast: [
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                    reader.read_bit_double(),
                ],
                shadow_mode: reader.read_bit_short(),
                shadow_maps_enabled: reader.read_bit(),
                ray_tracing_enabled: reader.read_bit(),
                ray_trace_depth: [
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                ],
                global_illumination_enabled: reader.read_bit(),
                global_illumination_sample_count: reader.read_bit_long(),
                global_illumination_sample_radius_enabled: reader.read_bit(),
                global_illumination_sample_radius: reader.read_bit_double(),
                photons_per_light: reader.read_bit_long(),
                photon_trace_depth: [
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                ],
                final_gathering_enabled: reader.read_bit(),
                final_gathering_ray_count: reader.read_bit_long(),
                final_gathering_sample_radius_state: [
                    reader.read_bit(),
                    reader.read_bit(),
                    reader.read_bit(),
                ],
                final_gathering_sample_radius: [reader.read_bit_double(), reader.read_bit_double()],
                light_luminance_scale: reader.read_bit_double(),
                diagnostics_mode: reader.read_bit_short(),
                diagnostics_grid_mode: reader.read_bit_short(),
                diagnostics_grid_size: reader.read_bit_double(),
                diagnostics_photon_mode: reader.read_bit_short(),
                diagnostics_bsp_mode: reader.read_bit_short(),
                export_mi_enabled: reader.read_bit(),
                description: reader.read_variable_text(),
                tile_size: reader.read_bit_long(),
                tile_order: reader.read_bit_short(),
                memory_limit: reader.read_bit_long(),
                diagnostics_samples_mode: reader.read_bit(),
                energy_multiplier: reader.read_bit_double(),
            })
        }
        "RAPIDRTRENDERSETTINGS" => {
            let mut base = read_render_settings(reader, version, dxf_version, true);
            let rapid_version = reader.read_bit_long();
            let render_target = reader.read_bit_long();
            let render_level = reader.read_bit_long();
            let render_time = reader.read_bit_long();
            let lighting_model = reader.read_bit_long();
            let filter_type = reader.read_bit_long();
            let filter_width = reader.read_bit_double();
            let filter_height = reader.read_bit_double();
            base.has_predefined = reader.read_bit();
            ClassObjectData::RapidRtRenderSettings(RapidRtRenderSettings {
                base,
                version: rapid_version,
                render_target,
                render_level,
                render_time,
                lighting_model,
                filter_type,
                filter_width,
                filter_height,
            })
        }
        "GRADIENT_BACKGROUND" => ClassObjectData::GradientBackground(GradientBackground {
            class_version: reader.read_bit_long(),
            color_top: reader.read_bit_long() as u32,
            color_middle: reader.read_bit_long() as u32,
            color_bottom: reader.read_bit_long() as u32,
            horizon: reader.read_bit_double(),
            height: reader.read_bit_double(),
            rotation: reader.read_bit_double(),
        }),
        "GROUND_PLANE_BACKGROUND" => {
            ClassObjectData::GroundPlaneBackground(GroundPlaneBackground {
                class_version: reader.read_bit_long(),
                color_sky_zenith: reader.read_bit_long() as u32,
                color_sky_horizon: reader.read_bit_long() as u32,
                color_underground_horizon: reader.read_bit_long() as u32,
                color_underground_azimuth: reader.read_bit_long() as u32,
                color_near: reader.read_bit_long() as u32,
                color_far: reader.read_bit_long() as u32,
            })
        }
        "RAPIDRTRENDERENVIRONMENT" | "IBL_BACKGROUND" => {
            ClassObjectData::IblBackground(IblBackground {
                class_version: reader.read_bit_long(),
                enabled: reader.read_bit(),
                name: reader.read_variable_text(),
                rotation: reader.read_bit_double(),
                display_image: reader.read_bit(),
                secondary_background: Handle::from(reader.read_handle()),
            })
        }
        "IMAGE_BACKGROUND" => ClassObjectData::ImageBackground(ImageBackground {
            class_version: reader.read_bit_long(),
            filename: reader.read_variable_text(),
            fit_to_screen: reader.read_bit(),
            maintain_aspect_ratio: reader.read_bit(),
            use_tiling: reader.read_bit(),
            offset: reader.read_2bit_double(),
            scale: reader.read_2bit_double(),
        }),
        "SKYLIGHT_BACKGROUND" => ClassObjectData::SkyLightBackground(SkyLightBackground {
            class_version: reader.read_bit_long(),
            sun: Handle::from(reader.read_handle()),
        }),
        "SOLID_BACKGROUND" => ClassObjectData::SolidBackground(SolidBackground {
            class_version: reader.read_bit_long(),
            color: reader.read_bit_long() as u32,
        }),
        "RENDERENTRY" => ClassObjectData::RenderEntry(RenderEntry {
            class_version: reader.read_bit_long(),
            image_filename: reader.read_variable_text(),
            preset_name: reader.read_variable_text(),
            view_name: reader.read_variable_text(),
            width: reader.read_bit_long(),
            height: reader.read_bit_long(),
            start_year: reader.read_bit_short(),
            start_month: reader.read_bit_short(),
            start_day: reader.read_bit_short(),
            start_hour: reader.read_bit_short(),
            start_minute: reader.read_bit_short(),
            start_second: reader.read_bit_short(),
            start_millisecond: reader.read_bit_short(),
            end_year: reader.read_bit_short(),
            end_month: reader.read_bit_short(),
            end_day: reader.read_bit_short(),
            end_hour: reader.read_bit_short(),
            end_minute: reader.read_bit_short(),
            end_second: reader.read_bit_short(),
            end_millisecond: reader.read_bit_short(),
            render_time: reader.read_bit_double(),
            memory_amount: reader.read_bit_long(),
            material_count: reader.read_bit_long(),
            light_count: reader.read_bit_long(),
            triangle_count: reader.read_bit_long(),
            display_index: reader.read_bit_long(),
        }),
        "RENDERENVIRONMENT" => ClassObjectData::RenderEnvironment(RenderEnvironment {
            class_version: reader.read_bit_long(),
            fog_enabled: reader.read_bit(),
            fog_background_enabled: reader.read_bit(),
            fog_color: [reader.read_byte(), reader.read_byte(), reader.read_byte()],
            fog_density_near: reader.read_bit_double(),
            fog_density_far: reader.read_bit_double(),
            fog_distance_near: reader.read_bit_double(),
            fog_distance_far: reader.read_bit_double(),
            environment_image_enabled: reader.read_bit(),
            environment_image_filename: reader.read_variable_text(),
        }),
        "RENDERGLOBAL" => ClassObjectData::RenderGlobal(RenderGlobal {
            class_version: reader.read_bit_long(),
            procedure: reader.read_bit_long(),
            destination: reader.read_bit_long(),
            save_enabled: reader.read_bit(),
            save_filename: reader.read_variable_text(),
            image_width: reader.read_bit_long(),
            image_height: reader.read_bit_long(),
            predefined_presets_first: reader.read_bit(),
            high_level_info: reader.read_bit(),
        }),
        "ACDBMOTIONPATH" | "MOTIONPATH" => ClassObjectData::MotionPath(MotionPath {
            class_version: reader.read_bit_long(),
            camera_path: Handle::from(reader.read_handle()),
            target_path: Handle::from(reader.read_handle()),
            view: Handle::from(reader.read_handle()),
            frames: reader.read_bit_short(),
            frame_rate: reader.read_bit_short(),
            corner_deceleration: reader.read_bit(),
        }),
        "ACDBCURVEPATH" | "CURVEPATH" => ClassObjectData::CurvePath(CurvePath {
            class_version: reader.read_bit_long(),
            entity: Handle::from(reader.read_handle()),
        }),
        "ACDBPOINTPATH" | "POINTPATH" => ClassObjectData::PointPath(PointPath {
            class_version: reader.read_bit_short(),
            point: reader.read_3bit_double(),
        }),
        "TVDEVICEPROPERTIES" => ClassObjectData::TvDeviceProperties(TvDeviceProperties {
            flags: reader.read_bit_long() as u32,
            max_regen_threads: reader.read_bit_short(),
            use_lut_palette: reader.read_bit_long(),
            alternate_highlight: reader.read_bit_long_long(),
            alternate_highlight_color: reader.read_bit_long_long(),
            geometry_shader_usage: reader.read_bit_long_long(),
            blending_mode: reader.read_bit_long(),
            antialiasing_level: reader.read_bit_double(),
            reserved_double: reader.read_bit_double(),
        }),
        "ACDBPOINTCLOUDDEF" | "POINTCLOUDDEF" => {
            ClassObjectData::PointCloudDefinition(read_point_cloud_definition(reader))
        }
        "ACDBPOINTCLOUDDEFEX" | "POINTCLOUDDEFEX" => {
            ClassObjectData::PointCloudDefinitionEx(read_point_cloud_definition(reader))
        }
        "ACDBPOINTCLOUDDEF_REACTOR" | "POINTCLOUDDEF_REACTOR" => {
            ClassObjectData::PointCloudDefinitionReactor(PointCloudDefinitionReactor {
                class_version: reader.read_bit_long(),
            })
        }
        "ACDBPOINTCLOUDDEF_REACTOR_EX" | "POINTCLOUDDEF_REACTOR_EX" => {
            ClassObjectData::PointCloudDefinitionReactorEx(PointCloudDefinitionReactor {
                class_version: reader.read_bit_long(),
            })
        }
        "ACDBPOINTCLOUDCOLORMAP" | "POINTCLOUDCOLORMAP" => {
            ClassObjectData::PointCloudColorMap(PointCloudColorMap {
                class_version: reader.read_bit_short(),
                default_intensity_scheme: reader.read_variable_text(),
                default_elevation_scheme: reader.read_variable_text(),
                default_classification_scheme: reader.read_variable_text(),
                color_ramps: read_point_cloud_ramps(reader),
                classification_color_ramps: read_point_cloud_ramps(reader),
            })
        }
        "NAVISWORKSMODELDEF" | "COORDINATION_MODEL_DEFINITION" => {
            ClassObjectData::NavisworksModelDefinition(NavisworksModelDefinition {
                flags: reader.read_bit_short(),
                path: reader.read_variable_text(),
                status: reader.read_bit(),
                extents_min: reader.read_3bit_double(),
                extents_max: reader.read_3bit_double(),
                host_drawing_visibility: reader.read_bit(),
            })
        }
        "CONTEXTDATAMANAGER" => {
            let object_context = Handle::from(reader.read_handle());
            let mut sub_managers = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                let handle = Handle::from(reader.read_handle());
                let mut entries = Vec::new();
                for _ in 0..count(reader.read_bit_long()) {
                    entries.push(ContextDataEntry {
                        item: Handle::from(reader.read_handle()),
                        name: reader.read_variable_text(),
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
            let class_version = reader.read_bit_long();
            let setup_name = reader.read_variable_text();
            let description = reader.read_variable_text();
            let output_type = reader.read_bit_long();
            let (use_subset, sheet_set_name, sheet_subset_name) = if output_type == 0 {
                (
                    reader.read_bit(),
                    reader.read_variable_text(),
                    reader.read_variable_text(),
                )
            } else {
                (false, String::new(), String::new())
            };
            let select_dates_from_calendar = reader.read_bit();
            let mut dates = Vec::new();
            for _ in 0..count(reader.read_bit_long()).min(10_000) {
                dates.push(SunStudyDate {
                    julian_day: reader.read_bit_long(),
                    milliseconds: reader.read_bit_long(),
                });
            }
            let select_range_of_dates = reader.read_bit();
            let (start_time, end_time, interval) = if select_range_of_dates {
                (
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                    reader.read_bit_long(),
                )
            } else {
                (0, 0, 0)
            };
            let mut hours = Vec::new();
            for _ in 0..count(reader.read_bit_long()).min(10_000) {
                hours.push(reader.read_bit());
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
                shade_plot_type: reader.read_bit_long(),
                viewport_count: reader.read_bit_long(),
                rows: reader.read_bit_long(),
                columns: reader.read_bit_long(),
                spacing: reader.read_bit_double(),
                lock_viewports: reader.read_bit(),
                label_viewports: reader.read_bit(),
                page_setup_wizard: Handle::from(reader.read_handle()),
                view: Handle::from(reader.read_handle()),
                visual_style: Handle::from(reader.read_handle()),
                text_style: Handle::from(reader.read_handle()),
            })
        }
        "DATATABLE" => {
            let flags = reader.read_bit_short();
            let column_count = count(reader.read_bit_long());
            let row_count = reader.read_bit_long();
            let name = reader.read_variable_text();
            let mut columns = Vec::with_capacity(column_count);
            for _ in 0..column_count {
                let value_type = reader.read_bit_long();
                let column_name = reader.read_variable_text();
                let mut rows = Vec::new();
                for _ in 0..count(row_count) {
                    rows.push(DataTableValue {
                        integer: reader.read_bit_long(),
                        real: reader.read_bit_double(),
                        text: reader.read_variable_text(),
                    });
                }
                columns.push(DataTableColumn {
                    value_type,
                    name: column_name,
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
            let data_adapter = reader.read_variable_text();
            let description = reader.read_variable_text();
            let tooltip = reader.read_variable_text();
            let connection_string = reader.read_variable_text();
            let option = reader.read_bit_long();
            let update_option = reader.read_bit_long();
            let flags = reader.read_bit_long();
            let year = reader.read_bit_short();
            let month = reader.read_bit_short();
            let day = reader.read_bit_short();
            let hour = reader.read_bit_short();
            let minute = reader.read_bit_short();
            let second = reader.read_bit_short();
            let millisecond = reader.read_bit_short();
            let path_option = reader.read_bit_short();
            let status_flags = reader.read_bit_long();
            let update_status = reader.read_variable_text();
            let mut custom_data = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                custom_data.push(DataLinkCustomData {
                    target: Handle::from(reader.read_handle()),
                    value: reader.read_variable_text(),
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
                hard_owner: Handle::from(reader.read_handle()),
            })
        }
        "ACDBPERSSUBENTMANAGER" | "PERSUBENTMGR" => {
            let class_version = reader.read_bit_long();
            let reserved_zero = reader.read_bit_long();
            let reserved_two = reader.read_bit_long();
            let associated_step_count = reader.read_bit_long();
            let associated_subentity_count = reader.read_bit_long();
            let mut steps = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                steps.push(reader.read_bit_long());
            }
            let mut subentities = Vec::new();
            for _ in 0..count(reader.read_bit_long()) {
                subentities.push(reader.read_bit_long());
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
        "GEOMAPIMAGE" => ClassObjectData::GeoMapImage(GeoMapImage {
            class_version: reader.read_bit_long(),
            origin: Vector3::new(
                reader.read_raw_double(),
                reader.read_raw_double(),
                reader.read_raw_double(),
            ),
            image_size: reader.read_2raw_double(),
            display_properties: reader.read_bit_short(),
            clipping_enabled: reader.read_bit(),
            brightness: reader.read_byte(),
            contrast: reader.read_byte(),
            fade: reader.read_byte(),
        }),
        "ACDBDETAILVIEWSTYLE" | "DETAILVIEWSTYLE" => {
            let base = ModelDocViewStyle {
                class_version: reader.read_bit_short(),
                description: reader.read_variable_text(),
                modified_for_recompute: reader.read_bit(),
                display_name: if version.r2018_plus(dxf_version) {
                    reader.read_variable_text()
                } else {
                    String::new()
                },
                flags: if version.r2018_plus(dxf_version) {
                    reader.read_bit_long()
                } else {
                    0
                },
            };
            ClassObjectData::DetailViewStyle(DetailViewStyle {
                base,
                class_version: reader.read_bit_short(),
                flags: reader.read_bit_long(),
                identifier_style: Handle::from(reader.read_handle()),
                identifier_color: reader.read_cm_color(),
                identifier_height: reader.read_bit_double(),
                identifier_excluded_characters: reader.read_variable_text(),
                identifier_offset: reader.read_bit_double(),
                identifier_placement: reader.read_byte(),
                arrow_symbol: Handle::from(reader.read_handle()),
                arrow_symbol_color: reader.read_cm_color(),
                arrow_symbol_size: reader.read_bit_double(),
                boundary_linetype: Handle::from(reader.read_handle()),
                boundary_lineweight: reader.read_bit_long(),
                boundary_color: reader.read_cm_color(),
                view_label_text_style: Handle::from(reader.read_handle()),
                view_label_text_color: reader.read_cm_color(),
                view_label_text_height: reader.read_bit_double(),
                view_label_attachment: reader.read_bit_long(),
                view_label_offset: reader.read_bit_double(),
                view_label_alignment: reader.read_bit_long(),
                view_label_pattern: reader.read_variable_text(),
                connection_linetype: Handle::from(reader.read_handle()),
                connection_lineweight: reader.read_bit_long(),
                connection_color: reader.read_cm_color(),
                border_linetype: Handle::from(reader.read_handle()),
                border_lineweight: reader.read_bit_long(),
                border_color: reader.read_cm_color(),
                model_edge: reader.read_byte(),
            })
        }
        "ACDBSECTIONVIEWSTYLE" | "SECTIONVIEWSTYLE" => {
            let base = ModelDocViewStyle {
                class_version: reader.read_bit_short(),
                description: reader.read_variable_text(),
                modified_for_recompute: reader.read_bit(),
                display_name: if version.r2018_plus(dxf_version) {
                    reader.read_variable_text()
                } else {
                    String::new()
                },
                flags: if version.r2018_plus(dxf_version) {
                    reader.read_bit_long()
                } else {
                    0
                },
            };
            ClassObjectData::SectionViewStyle(SectionViewStyle {
                base,
                class_version: reader.read_bit_short(),
                flags: reader.read_bit_long(),
                identifier_style: Handle::from(reader.read_handle()),
                identifier_color: reader.read_cm_color(),
                identifier_height: reader.read_bit_double(),
                arrow_start_symbol: Handle::from(reader.read_handle()),
                arrow_end_symbol: Handle::from(reader.read_handle()),
                arrow_symbol_color: reader.read_cm_color(),
                arrow_symbol_size: reader.read_bit_double(),
                identifier_excluded_characters: reader.read_variable_text(),
                arrow_symbol_extension_length: reader.read_bit_double(),
                plane_linetype: Handle::from(reader.read_handle()),
                plane_lineweight: reader.read_bit_long(),
                plane_color: reader.read_cm_color(),
                bend_linetype: Handle::from(reader.read_handle()),
                bend_lineweight: reader.read_bit_long(),
                bend_color: reader.read_cm_color(),
                bend_line_length: reader.read_bit_double(),
                end_line_length: reader.read_bit_double(),
                view_label_text_style: Handle::from(reader.read_handle()),
                view_label_text_color: reader.read_cm_color(),
                view_label_text_height: reader.read_bit_double(),
                view_label_attachment: reader.read_bit_long(),
                view_label_offset: reader.read_bit_double(),
                view_label_alignment: reader.read_bit_long(),
                view_label_pattern: reader.read_variable_text(),
                hatch_color: reader.read_cm_color(),
                hatch_background_color: reader.read_cm_color(),
                hatch_pattern: reader.read_variable_text(),
                hatch_scale: reader.read_bit_double(),
                hatch_transparency: reader.read_bit_long(),
                reserved_flags: [reader.read_bit(), reader.read_bit()],
                identifier_position: reader.read_bit_long(),
                identifier_offset: reader.read_bit_double(),
                arrow_position: reader.read_bit_long(),
                end_line_overshoot: reader.read_bit_double(),
                hatch_angles: {
                    let mut angles = Vec::new();
                    for _ in 0..count(reader.read_bit_long()) {
                        angles.push(reader.read_bit_double());
                    }
                    angles
                },
            })
        }
        "ACMECOMMANDHISTORY" => ClassObjectData::AcMeCommandHistory(AcMeCommandHistory {
            class_version: reader.read_bit_short(),
        }),
        "ACMESCOPE" => ClassObjectData::AcMeScope(AcMeScope {
            class_version: reader.read_bit_short(),
        }),
        "ACMESTATEMGR" => ClassObjectData::AcMeStateManager(AcMeStateManager {
            class_version: reader.read_bit_short(),
        }),
        "CSACDOCUMENTOPTIONS" => ClassObjectData::CsacDocumentOptions(CsacDocumentOptions {
            class_version: reader.read_bit_short(),
            raw_dwg_data: Some(reader.raw_merged_data()),
            raw_dwg_handle_bits: reader.get_handle_bits(),
            raw_dwg_version: Some(dxf_version),
        }),
        _ => return None,
    };
    Some(data)
}
