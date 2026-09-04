//! SWEEP command data mapped to the shared kernel history reconstruction.

use acadrust::entities::{Surface, SurfaceData, SurfaceKind, SurfaceSweepOptions};
use acadrust::objects::{SolidHistoryNodeBase, SolidHistorySweep};
use acadrust::types::Vector3;
use acadrust::EntityType;
use cadkernel::brep::Body;

use crate::command::{ExtrudeMode, SweepOptions};
use super::sweep_model::{embedded_path, embedded_revolve_profile};

fn embedded_sweep_profile(entity: &EntityType) -> Option<(acadrust::entities::EmbeddedEntity, [f64; 16])> {
    if let EntityType::Region(region) = entity {
        Some((acadrust::entities::EmbeddedEntity::Region(region.clone()), glam::DMat4::IDENTITY.to_cols_array()))
    } else {
        embedded_revolve_profile(entity)
    }
}

pub fn is_sweep_profile(entity: &EntityType) -> bool {
    embedded_sweep_profile(entity).is_some_and(|(profile, transform)| {
        cadkernel::acis::sweep_profile_geometry(&profile, transform).is_ok()
    })
}

pub fn is_sweep_path(entity: &EntityType) -> bool {
    match embedded_path(entity) {
        Some(acadrust::entities::EmbeddedEntity::Spline(value)) => {
            value.degree > 0 && (value.control_points.len() > value.degree as usize
                || value.fit_points.len() >= 2)
        }
        Some(_) => crate::entities::curve::entity_curve(entity)
            .is_some_and(|curve| curve.curve.length().is_finite() && curve.curve.length() > 1e-9),
        None => false,
    }
}

/// All selected profiles use one base point, preserving their relative offsets.
pub fn sweep_selection_options(profiles: &[EntityType], mut options: SweepOptions) -> Option<SweepOptions> {
    if options.base_point.is_some() {
        return Some(options);
    }
    let geometry = profiles.iter().map(|profile| {
        let (entity, transform) = embedded_sweep_profile(profile)?;
        let (plane, wires, _) = cadkernel::acis::sweep_profile_geometry(&entity, transform).ok()?;
        Some((plane, wires))
    }).collect::<Option<Vec<_>>>()?;
    options.base_point = Some(glam::DVec3::from_array(
        cadkernel::brep::sweep_profile_group_base(&geometry)?,
    ));
    Some(options)
}

pub fn sweep_record(profile: &EntityType, path: &EntityType, options: SweepOptions) -> Option<SolidHistorySweep> {
    let (sweep_entity, sweep_entity_transform) = embedded_sweep_profile(profile)?;
    let (plane, wires, _) = cadkernel::acis::sweep_profile_geometry(&sweep_entity, sweep_entity_transform).ok()?;
    let base_point = match options.base_point {
        Some(point) => point.to_array(),
        None => cadkernel::brep::sweep_profile_base(plane, &wires)?,
    };
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    Some(SolidHistorySweep {
        base,
        operation_major: 1,
        sweep_entity: Some(sweep_entity),
        path_entity: Some(embedded_path(path)?),
        scale_factor: options.scale,
        twist_angle: options.twist_angle,
        align_option: u8::from(options.align),
        has_align_start: true,
        bank: options.bank,
        sweep_entity_transform,
        path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        reference_point: Vector3::new(base_point[0], base_point[1], base_point[2]),
        ..SolidHistorySweep::default()
    })
}

pub fn swept_with_options(profile: &EntityType, path: &EntityType, mode: ExtrudeMode, options: SweepOptions) -> Option<Body> {
    let record = sweep_record(profile, path, options)?;
    cadkernel::acis::rebuild_sweep_with_mode(&record, mode == ExtrudeMode::Surface).ok()
}

/// Preserve native construction parameters alongside the sheet's saved B-rep.
pub fn swept_surface_entity(record: &SolidHistorySweep) -> EntityType {
    let mut surface = Surface::new(SurfaceKind::Swept);
    if let Ok(point) = cadkernel::acis::sweep_history_reference_point(record) {
        surface.point_of_reference = Vector3::new(point[0], point[1], point[2]);
    }
    surface.surface_data = SurfaceData::Swept {
        class_version: 0,
        sweep_entity: record.sweep_entity.clone(),
        path_entity: record.path_entity.clone(),
        sweep_transform: glam::DMat4::IDENTITY.to_cols_array(),
        path_transform: glam::DMat4::IDENTITY.to_cols_array(),
        options: SurfaceSweepOptions {
            draft_angle: record.draft_angle,
            twist_angle: record.twist_angle,
            scale_factor: record.scale_factor,
            align_angle: record.align_angle,
            sweep_entity_transform: record.sweep_entity_transform,
            path_entity_transform: record.path_entity_transform,
            sweep_alignment_flags: record.align_option as i16,
            align_start: record.has_align_start,
            bank: record.bank,
            base_point_set: true,
            reference_vector: record.reference_point,
            ..SurfaceSweepOptions::default()
        },
    };
    EntityType::Surface(surface)
}
