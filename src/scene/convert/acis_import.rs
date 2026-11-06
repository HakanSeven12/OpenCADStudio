//! ACIS text (SAT) files and kernel bodies as drawing objects.
//!
//! Each body of a file becomes one object: a closed solid a 3DSOLID, a sheet
//! whose faces all lie on planes a REGION, any other sheet a SURFACE.

use document::entities::acis::{SatDocument, SatFace, Sidedness};
use document::entities::{AcisData, Region, Solid3D, Surface, SurfaceKind};
use document::EntityType;

/// What a body restores as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Solid,
    Region,
    Surface,
}

fn class(document: &SatDocument) -> Option<Class> {
    let faces: Vec<SatFace<'_>> = document
        .records
        .iter()
        .filter_map(SatFace::from_record)
        .collect();
    if faces.is_empty() {
        return None;
    }
    if faces
        .iter()
        .all(|face| face.sidedness() != Sidedness::Double)
    {
        return Some(Class::Solid);
    }
    let planar = faces.iter().all(|face| {
        face.surface()
            .index()
            .and_then(|at| document.record(at))
            .is_some_and(|surface| surface.entity_type == "plane-surface")
    });
    Some(if planar { Class::Region } else { Class::Surface })
}

/// One body's document as its drawing object.
fn entity(document: &SatDocument) -> Option<EntityType> {
    entity_as(document, class(document)?)
}

fn entity_as(document: &SatDocument, class: Class) -> Option<EntityType> {
    Some(match class {
        Class::Solid => {
            let mut solid = Solid3D::new();
            solid.set_sat_document(document);
            EntityType::Solid3D(solid)
        }
        Class::Region => {
            let mut region = Region::new();
            region.set_sat_document(document);
            EntityType::Region(region)
        }
        Class::Surface => {
            let mut surface = Surface::new(SurfaceKind::Generic);
            surface.acis_data = AcisData::from_sat(&document.to_sat_string());
            EntityType::Surface(Box::new(surface))
        }
    })
}

/// The objects of an ACIS text file, one per body with faces; `None` when
/// the text is no ACIS data.
pub fn sat_entities(text: &str) -> Option<Vec<EntityType>> {
    let document = SatDocument::parse(text).ok()?;
    Some(document.split_bodies().iter().filter_map(entity).collect())
}

/// The value of an ACIS colour attribute: the index (0 by block), or the
/// index nearest a true colour.
fn attribute_index(color: document::types::Color) -> i64 {
    match color {
        document::types::Color::Index(index) => i64::from(index),
        document::types::Color::Rgb { r, g, b } => i64::from(document::types::nearest_aci(r, g, b)),
        _ => 0,
    }
}

/// A true colour packed as a 32-bit colour value: the true-colour flag
/// byte, then red, green and blue.
pub(crate) fn packed_true_colour(r: u8, g: u8, b: u8) -> i32 {
    ((0xC2u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)) as i32
}

/// Hangs colour attributes on a record: a true colour first, then its
/// index, as imported models carry them.
fn paint_record(document: &mut document::entities::acis::SatDocument, owner: usize, color: document::types::Color) {
    use document::entities::acis::{SatPointer, SatRecord, SatToken};
    let mut chain: Vec<(&str, i64)> = Vec::new();
    if let document::types::Color::Rgb { r, g, b } = color {
        chain.push(("truecolor-adesk-attrib", i64::from(packed_true_colour(r, g, b))));
    }
    chain.push(("color-adesk-attrib", attribute_index(color)));
    let Some(existing) = document.record(owner).map(|record| record.attribute) else {
        return;
    };
    let first = document.record_count() as i32;
    let count = chain.len() as i32;
    for (k, (kind, value)) in chain.into_iter().enumerate() {
        let at = first + k as i32;
        let next = if k as i32 + 1 < count { SatPointer::new(at + 1) } else { existing };
        let previous = if k == 0 { SatPointer::NULL } else { SatPointer::new(at - 1) };
        let mut record = SatRecord::new(-1, kind);
        record.tokens = vec![
            SatToken::Pointer(next),
            SatToken::Pointer(previous),
            SatToken::Pointer(SatPointer::new(owner as i32)),
            SatToken::Integer(value),
        ];
        document.add_record(record);
    }
    if let Some(record) = document.record_mut(owner) {
        record.attribute = SatPointer::new(first);
    }
}

/// A kernel body as its drawing object in `colour` (by block when `None`),
/// with the faces given a colour of their own carrying it, and the edges
/// bounding those faces carrying it too. A `sheet` is a SURFACE however
/// flat it is.
pub fn painted_body_entity(
    body: &kernel::brep::Body,
    colour: Option<document::types::Color>,
    faces: &[Option<document::types::Color>],
    sheet: bool,
) -> Option<EntityType> {
    let mut document = super::acis_export::solid_to_sat(body)?;
    let base = colour.unwrap_or(document::types::Color::ByBlock);
    let own: std::collections::HashMap<kernel::brep::FaceKey, document::types::Color> = body
        .faces
        .keys()
        .zip(faces)
        .filter_map(|(key, color)| Some((key, (*color)?)))
        .collect();
    // The writer puts faces and edges in the body's own order.
    let face_records: Vec<usize> = (0..document.record_count())
        .filter(|&at| document.record(at).is_some_and(|r| r.entity_type == "face"))
        .collect();
    let edge_records: Vec<usize> = (0..document.record_count())
        .filter(|&at| document.record(at).is_some_and(|r| r.entity_type == "edge"))
        .collect();
    for (key, at) in body.faces.keys().zip(face_records) {
        if let Some(color) = own.get(&key) {
            paint_record(&mut document, at, *color);
        }
    }
    for ((_, edge), at) in body.edges.iter().zip(edge_records) {
        let bounding = edge.coedges.iter().find_map(|coedge| {
            let ring = body.coedges.get(*coedge)?.owner;
            own.get(&body.loops.get(ring)?.owner).copied()
        });
        if let Some(color) = bounding {
            paint_record(&mut document, at, color);
        }
    }
    let mut entity = if sheet {
        entity_as(&document, Class::Surface)?
    } else {
        entity(&document)?
    };
    entity.common_mut().color = base;
    Some(entity)
}
