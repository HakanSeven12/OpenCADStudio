//! The predefined content blocks a multileader style offers as its source
//! block, and the blocks that carry them in a drawing.
//!
//! Each is created the first time it is picked, centred on its insertion
//! point at unit size, with the attributes the leader fills in.

use document::entities::attribute_definition::{HorizontalAlignment, VerticalAlignment};
use document::entities::{AttributeDefinition, Circle, Line, LwPolyline, LwVertex};
use document::types::{Color, LineWeight, Vector3};
use document::{Document, EntityType, Handle};

/// Name shown, block name. The first is the default source block.
pub(crate) const STANDARD: &[(&str, &str)] = &[
    ("Detail Callout", "_DetailCallout"),
    ("Slot", "_TagSlot"),
    ("Circle", "_TagCircle"),
    ("Box", "_TagBox"),
    ("Hexagon", "_TagHexagon"),
    ("Triangle", "_TagTriangle"),
];

/// Whether `name` is one of the predefined content blocks.
pub(crate) fn is_standard_block(name: &str) -> bool {
    STANDARD
        .iter()
        .any(|(_, block)| block.eq_ignore_ascii_case(name))
}

/// What the source block picker shows for `handle`: a predefined block's
/// name, a user block's own name, or the default when the style has none.
pub(crate) fn label(doc: &Document, handle: Option<Handle>) -> String {
    let Some(record) = handle
        .filter(|h| !h.is_null())
        .and_then(|h| doc.block_records.iter().find(|b| b.handle == h))
    else {
        return STANDARD[0].0.to_string();
    };
    STANDARD
        .iter()
        .find(|(_, block)| block.eq_ignore_ascii_case(&record.name))
        .map(|(shown, _)| shown.to_string())
        .unwrap_or_else(|| record.name.clone())
}

/// The handle a picker value selects, creating a predefined block when the
/// drawing does not have it yet. `None` for an unknown name.
pub(crate) fn resolve(doc: &mut Document, value: &str) -> Option<Handle> {
    let name = STANDARD
        .iter()
        .find(|(shown, _)| *shown == value)
        .map(|(_, block)| *block)
        .unwrap_or(value);
    if let Some(record) = doc.block_records.get(name) {
        return Some(record.handle);
    }
    let entities = geometry(name)?;
    crate::app::define_block_raw_doc(doc, name, Vector3::ZERO, &entities);
    doc.block_records.get(name).map(|record| record.handle)
}

fn geometry(name: &str) -> Option<Vec<EntityType>> {
    let line = |x1: f64, y1: f64, x2: f64, y2: f64| {
        EntityType::Line(Line::from_points(
            Vector3::new(x1, y1, 0.0),
            Vector3::new(x2, y2, 0.0),
        ))
    };
    let closed = |points: &[(f64, f64, f64)]| {
        let mut p = LwPolyline::new();
        for &(x, y, bulge) in points {
            let mut v = LwVertex::from_coords(x, y);
            v.bulge = bulge;
            p.vertices.push(v);
        }
        p.is_closed = true;
        EntityType::LwPolyline(p)
    };
    let attribute = |tag: &str, y: f64, height: f64| {
        let mut a = AttributeDefinition::new(tag.to_string(), tag.to_string(), String::new());
        a.insertion_point = Vector3::new(0.0, y, 0.0);
        a.alignment_point = Vector3::new(0.0, y, 0.0);
        a.height = height;
        a.set_alignment(HorizontalAlignment::Center, VerticalAlignment::Middle);
        EntityType::AttributeDefinition(a)
    };
    let tag = || attribute("TAGNUMBER", 0.0, 0.5);
    let regular = |sides: usize, start: f64| {
        let points: Vec<(f64, f64, f64)> = (0..sides)
            .map(|i| {
                let a = (start + 360.0 * i as f64 / sides as f64).to_radians();
                (a.cos(), a.sin(), 0.0)
            })
            .collect();
        closed(&points)
    };
    let mut entities = match name {
        "_DetailCallout" => vec![
            EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, 1.0)),
            line(-1.0, 0.0, 1.0, 0.0),
            attribute("VIEWNUMBER", 0.5, 0.4),
            attribute("SHEETNUMBER", -0.5, 0.4),
        ],
        "_TagSlot" => vec![
            closed(&[
                (0.5, 0.5, 0.0),
                (-0.5, 0.5, 1.0),
                (-0.5, -0.5, 0.0),
                (0.5, -0.5, 1.0),
            ]),
            tag(),
        ],
        "_TagCircle" => vec![
            EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, 1.0)),
            tag(),
        ],
        "_TagBox" => vec![
            closed(&[
                (-1.0, -1.0, 0.0),
                (1.0, -1.0, 0.0),
                (1.0, 1.0, 0.0),
                (-1.0, 1.0, 0.0),
            ]),
            tag(),
        ],
        "_TagHexagon" => vec![regular(6, 0.0), tag()],
        "_TagTriangle" => vec![regular(3, 90.0), tag()],
        _ => return None,
    };
    // The content takes the leader's block colour and lineweight.
    for entity in &mut entities {
        let common = entity.common_mut();
        common.color = Color::ByBlock;
        common.line_weight = LineWeight::ByBlock;
    }
    Some(entities)
}
