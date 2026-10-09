//! Associative extruded, revolved and lofted surfaces (SURFACEASSOCIATIVITY):
//! the sources and values each surface was made from, kept with the surface
//! so it follows edits of its sources and the drawing's associative network
//! describes it on save.

use codec::xdata::XDataValue;
use codec::{EntityType, Handle};
use glam::DVec3;

/// The application name under which a surface keeps its sources.
pub const SURFACE_SOURCES_APP: &str = "OCS_SURFACE_SOURCES";

/// What an associative surface was made from.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceSources {
    /// EXTRUDE: the profile, the height along its normal (or the length of a
    /// given direction) and the taper angle.
    Extrude { profile: Handle, height: f64, direction: Option<DVec3>, taper: f64 },
    /// REVOLVE: the profile, the axis (an axis object or two points), the
    /// angle and the start angle.
    Revolve { profile: Handle, axis: Option<Handle>, start: DVec3, end: DVec3, angle: f64, start_angle: f64 },
    /// LOFT: the cross sections in order.
    Loft { sections: Vec<Handle> },
}

impl SurfaceSources {
    /// Every entity the surface reads.
    pub fn handles(&self) -> Vec<Handle> {
        match self {
            SurfaceSources::Extrude { profile, .. } => vec![*profile],
            SurfaceSources::Revolve { profile, axis, .. } => std::iter::once(*profile).chain(*axis).collect(),
            SurfaceSources::Loft { sections } => sections.clone(),
        }
    }
}

/// Keep the sources with the surface.
pub fn link_surface_sources(document: &mut codec::CadDocument, surface: Handle, sources: &SurfaceSources) {
    if surface.is_null() {
        return;
    }
    let point = |p: DVec3| XDataValue::Point3D(codec::types::Vector3::new(p.x, p.y, p.z));
    let values = match sources {
        SurfaceSources::Extrude { profile, height, direction, taper } => {
            let mut values = vec![
                XDataValue::String("EXTRUDE".into()),
                XDataValue::Handle(*profile),
                XDataValue::Real(*height),
                XDataValue::Real(*taper),
            ];
            values.extend(direction.map(point));
            values
        }
        SurfaceSources::Revolve { profile, axis, start, end, angle, start_angle } => {
            let mut values = vec![
                XDataValue::String("REVOLVE".into()),
                XDataValue::Handle(*profile),
                point(*start),
                point(*end),
                XDataValue::Real(*angle),
                XDataValue::Real(*start_angle),
            ];
            values.extend(axis.map(XDataValue::Handle));
            values
        }
        SurfaceSources::Loft { sections } => std::iter::once(XDataValue::String("LOFT".into()))
            .chain(sections.iter().map(|handle| XDataValue::Handle(*handle)))
            .collect(),
    };
    crate::scene::view::dispatch::set_entity_xdata(document, surface, SURFACE_SOURCES_APP, Some(values));
}

/// The sources an associative surface keeps.
pub fn surface_sources(entity: &EntityType) -> Option<SurfaceSources> {
    let record = entity
        .common()
        .extended_data
        .records()
        .iter()
        .find(|record| record.application_name == SURFACE_SOURCES_APP)?;
    let mut kind = None;
    let (mut handles, mut reals, mut points) = (Vec::new(), Vec::new(), Vec::new());
    for value in &record.values {
        match value {
            XDataValue::String(text) if kind.is_none() => kind = Some(text.as_str()),
            XDataValue::Handle(handle) => handles.push(*handle),
            XDataValue::Real(value) => reals.push(*value),
            XDataValue::Point3D(p) => points.push(DVec3::new(p.x, p.y, p.z)),
            _ => {}
        }
    }
    match kind? {
        "EXTRUDE" => Some(SurfaceSources::Extrude {
            profile: *handles.first()?,
            height: *reals.first()?,
            taper: *reals.get(1)?,
            direction: points.first().copied(),
        }),
        "REVOLVE" => Some(SurfaceSources::Revolve {
            profile: *handles.first()?,
            axis: handles.get(1).copied(),
            start: *points.first()?,
            end: *points.get(1)?,
            angle: *reals.first()?,
            start_angle: *reals.get(1)?,
        }),
        "LOFT" => (handles.len() >= 2).then_some(SurfaceSources::Loft { sections: handles }),
        _ => None,
    }
}

/// The two ends of an axis object (a line).
pub fn axis_points(entity: &EntityType) -> Option<(DVec3, DVec3)> {
    match entity {
        EntityType::Line(line) => Some((
            DVec3::new(line.start.x, line.start.y, line.start.z),
            DVec3::new(line.end.x, line.end.y, line.end.z),
        )),
        _ => None,
    }
}
