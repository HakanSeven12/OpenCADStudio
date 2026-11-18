//! ACIS text (SAT) files and Solidcore records as drawing objects.
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
    Some(if planar {
        Class::Region
    } else {
        Class::Surface
    })
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
fn paint_record(
    document: &mut document::entities::acis::SatDocument,
    owner: usize,
    color: document::types::Color,
) {
    use document::entities::acis::{SatPointer, SatRecord, SatToken};
    let mut chain: Vec<(&str, i64)> = Vec::new();
    if let document::types::Color::Rgb { r, g, b } = color {
        chain.push((
            "truecolor-adesk-attrib",
            i64::from(packed_true_colour(r, g, b)),
        ));
    }
    chain.push(("color-adesk-attrib", attribute_index(color)));
    let Some(existing) = document.record(owner).map(|record| record.attribute) else {
        return;
    };
    let first = document.record_count() as i32;
    let count = chain.len() as i32;
    for (k, (kind, value)) in chain.into_iter().enumerate() {
        let at = first + k as i32;
        let next = if k as i32 + 1 < count {
            SatPointer::new(at + 1)
        } else {
            existing
        };
        let previous = if k == 0 {
            SatPointer::NULL
        } else {
            SatPointer::new(at - 1)
        };
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

/// Gives the faces and edges of `body`, written into `document` by
/// [`solid_to_sat`](super::acis_export::solid_to_sat), colours of their own, by record index.
pub fn paint_body(
    document: &mut document::entities::acis::SatDocument,
    body: &ocs_solid::model::Brep,
    faces: &std::collections::HashMap<u32, document::types::Color>,
    edges: &std::collections::HashMap<u32, document::types::Color>,
) {
    // The writer puts faces and edges in the record's own order.
    let records = |kind: &str| -> Vec<usize> {
        (0..document.record_count())
            .filter(|&at| document.record(at).is_some_and(|r| r.entity_type == kind))
            .collect()
    };
    let face_records = records("face");
    let edge_records = records("edge");
    for (index, at) in (0..body.faces.len() as u32).zip(face_records) {
        if let Some(color) = faces.get(&index) {
            paint_record(document, at, *color);
        }
    }
    for (index, at) in (0..body.edges.len() as u32).zip(edge_records) {
        if let Some(color) = edges.get(&index) {
            paint_record(document, at, *color);
        }
    }
}

/// The colours a read body's faces and edges carry of their own, by record index, found
/// through the SAT records each was read from.
pub fn body_colors(
    document: &document::entities::acis::SatDocument,
    body: &ocs_solid::Lifted,
) -> (
    std::collections::HashMap<u32, document::types::Color>,
    std::collections::HashMap<u32, document::types::Color>,
) {
    let mut owned = std::collections::HashMap::new();
    // A true colour outranks the index beside it, so it is read last.
    for kind in ["color-adesk-attrib", "truecolor-adesk-attrib"] {
        for record in document.records.iter().filter(|record| record.entity_type == kind) {
            let (Some(owner), Some(value)) = (
                record.token_pointer(2).map(|pointer| pointer.0),
                record.token(3).and_then(|token| token.as_integer()),
            ) else {
                continue;
            };
            let color = if kind == "truecolor-adesk-attrib" {
                document::types::Color::from_true_color_value(((value & 0x00FF_FFFF) | 0x0100_0000) as i32)
            } else if (1..=255).contains(&value) {
                document::types::Color::from_index(value as i16)
            } else {
                continue;
            };
            owned.insert(owner, color);
        }
    }
    let faces = body
        .face_records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((index as u32, *owned.get(&(*record as i32))?)))
        .collect();
    let edges = body
        .edge_records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((index as u32, *owned.get(&((*record)? as i32))?)))
        .collect();
    (faces, edges)
}

/// A record as its drawing object in `colour` (by block when `None`), with the faces given a
/// colour of their own carrying it, and the edges bounding those faces carrying it too. A
/// `sheet` is a SURFACE however flat it is.
pub fn painted_body_entity(
    body: &ocs_solid::model::Brep,
    colour: Option<document::types::Color>,
    faces: &[Option<document::types::Color>],
    sheet: bool,
) -> Option<EntityType> {
    let mut document = super::acis_export::solid_to_sat(body)?;
    let base = colour.unwrap_or(document::types::Color::ByBlock);
    let own: std::collections::HashMap<u32, document::types::Color> = faces
        .iter()
        .enumerate()
        .filter_map(|(index, color)| Some((index as u32, (*color)?)))
        .collect();
    let bounding: std::collections::HashMap<u32, document::types::Color> = ocs_solid::model::edge_faces(body)
        .into_iter()
        .enumerate()
        .filter_map(|(edge, faces)| Some((edge as u32, faces.iter().find_map(|f| own.get(f).copied())?)))
        .collect();
    paint_body(&mut document, body, &own, &bounding);
    let mut entity = if sheet {
        entity_as(&document, Class::Surface)?
    } else {
        entity(&document)?
    };
    entity.common_mut().color = base;
    Some(entity)
}
