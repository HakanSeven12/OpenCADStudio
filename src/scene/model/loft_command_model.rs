//! LOFT command/history integration. Geometry and validation live in the kernel.

use super::sweep_model::embedded_path;

use crate::scene::model::sweep_model::ExtrudeMode;
use document::entities::{EmbeddedEntity, Point, Surface, SurfaceData, SurfaceKind};
use document::objects::{SolidHistoryLoft, SolidHistoryLoftParameters, SolidHistoryNodeBase};
use document::{EntityType, Handle};

pub(crate) fn embedded_section(entity: &EntityType) -> Option<EmbeddedEntity> {
    match entity {
        // A region as the curve round it, as the reference records it.
        EntityType::Region(region) => Some(
            super::sweep_model::region_boundary_entity(entity).map_or_else(
                || EmbeddedEntity::Region(region.clone()),
                |(curve, _)| curve,
            ),
        ),
        EntityType::Point(point) => Some(EmbeddedEntity::Point(point.clone())),
        _ => embedded_path(entity),
    }
}

pub fn is_section(entity: &EntityType) -> bool {
    embedded_section(entity)
        .is_some_and(|entity| ocs_solid::loft_section_geometry(&[entity]).is_ok())
}

pub fn is_guide_or_path(entity: &EntityType) -> bool {
    embedded_path(entity).is_some_and(|entity| ocs_solid::loft_path_geometry(&entity).is_ok())
}

pub fn record(
    sections: &[LoftSectionSelection],
    guides: &[Handle],
    path: Option<Handle>,
    available: &[(Handle, EntityType)],
    mode: ExtrudeMode,
    options: LoftOptions,
) -> Result<SolidHistoryLoft, String> {
    let find = |handle| {
        available
            .iter()
            .find(|(candidate, _)| *candidate == handle)
            .map(|(_, entity)| entity)
            .ok_or_else(|| "A selected loft source no longer exists.".to_string())
    };
    let mut cross_sections = Vec::new();
    let mut section_counts = Vec::new();
    let mut surface = mode == ExtrudeMode::Surface;
    for section in sections {
        let members = match section {
            LoftSectionSelection::Entity(handle) => {
                vec![embedded_section(find(*handle)?).ok_or("Unsupported loft section.")?]
            }
            LoftSectionSelection::Point(point) => vec![EmbeddedEntity::Point(Point::from_coords(
                point.x, point.y, point.z,
            ))],
            LoftSectionSelection::Join(handles) => handles
                .iter()
                .map(|handle| {
                    embedded_section(find(*handle)?)
                        .ok_or_else(|| "Unsupported joined loft edge.".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        if let ocs_solid::LoftSection::Profile { closed, .. } =
            ocs_solid::loft_section_geometry(&members)?
        {
            surface |= !closed;
        }
        section_counts.push(members.len());
        cross_sections.extend(members);
    }
    let guides = guides
        .iter()
        .map(|handle| {
            embedded_path(find(*handle)?).ok_or_else(|| "Unsupported loft guide.".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let path_entity = path
        .map(|handle| {
            embedded_path(find(handle)?).ok_or_else(|| "Unsupported loft path.".to_string())
        })
        .transpose()?;
    Ok(SolidHistoryLoft {
        base: SolidHistoryNodeBase::new(1),
        operation_major: 1,
        cross_sections,
        guides,
        parameters: Some(SolidHistoryLoftParameters {
            path_entity,
            normals: options.normals,
            start_draft_angle: options.start_draft_angle,
            end_draft_angle: options.end_draft_angle,
            start_magnitude: options.start_magnitude,
            end_magnitude: options.end_magnitude,
            start_continuity: options.start_continuity,
            end_continuity: options.end_continuity,
            start_bulge: options.start_bulge,
            end_bulge: options.end_bulge,
            closed: options.closed,
            periodic: options.periodic,
            surface,
            align_direction: options.align_direction,
            section_counts,
        }),
        ..SolidHistoryLoft::default()
    })
}

pub fn build_body(
    sections: &[LoftSectionSelection],
    guides: &[Handle],
    path: Option<Handle>,
    available: &[(Handle, EntityType)],
    mode: ExtrudeMode,
    options: LoftOptions,
) -> Result<ocs_solid::model::Brep, String> {
    ocs_solid::rebuild_loft_with_options(&record(
        sections, guides, path, available, mode, options,
    )?)
}

pub fn surface_entity(record: &SolidHistoryLoft) -> EntityType {
    let settings = record
        .parameters
        .clone()
        .unwrap_or_else(|| SolidHistoryLoftParameters {
            normals: 0,
            ..Default::default()
        });
    let mut surface = Surface::new(SurfaceKind::Lofted);
    surface.surface_data = SurfaceData::Lofted {
        loft_transform: record.base.transform,
        cross_section_entities: record.cross_sections.clone(),
        guide_entities: record.guides.clone(),
        path_entity: settings.path_entity,
        plane_normal_lofting_type: settings.normals,
        start_draft_angle: settings.start_draft_angle,
        end_draft_angle: settings.end_draft_angle,
        start_draft_magnitude: settings.start_magnitude,
        end_draft_magnitude: settings.end_magnitude,
        arc_length_parameterization: true,
        no_twist: true,
        align_direction: settings.align_direction,
        simple_surfaces: true,
        closed_surfaces: settings.closed,
        solid: false,
        ruled_surface: settings.normals == 0,
        virtual_guide: false,
        cross_sections: Vec::new(),
        guide_curves: Vec::new(),
        path_curve: None,
    };
    EntityType::Surface(Box::new(surface))
}

/// Construction parameters shared by the LOFT command and its history.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoftOptions {
    /// Ruled, Smooth, First normal, Last normal, Ends normal, All normal,
    /// or Use draft angles, respectively.
    pub normals: i32,
    pub start_draft_angle: f64,
    pub end_draft_angle: f64,
    pub start_magnitude: f64,
    pub end_magnitude: f64,
    /// Point-end continuity: G0 (0) or G1 (1).
    pub start_continuity: i32,
    pub end_continuity: i32,
    /// Positive point-end bulge factors; independent of draft magnitudes.
    pub start_bulge: f64,
    pub end_bulge: f64,
    pub closed: bool,
    pub periodic: bool,
    pub align_direction: bool,
}

/// Ordered cross-sections used by LOFT creation, validation and preview.
#[derive(Clone, Debug, PartialEq)]
pub enum LoftSectionSelection {
    Entity(Handle),
    Point(glam::DVec3),
    /// Connected source edges forming one exact cross-section.
    Join(Vec<Handle>),
}
