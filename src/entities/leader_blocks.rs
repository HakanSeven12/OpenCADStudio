//! The predefined content blocks a multileader style offers as its source
//! block, and the blocks that carry them in a drawing.
//!
//! Each is created the first time it is picked, centred on its insertion
//! point, with the attributes the leader fills in.

use document::entities::attribute_definition::{HorizontalAlignment, VerticalAlignment};
use document::entities::{Arc, AttributeDefinition, Circle, Line, LwPolyline, LwVertex};
use document::types::{Color, LineWeight, Transparency, Vector3};
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
    // The attributes take the drawing's current text style.
    let text_style = doc.header.current_text_style_name.clone();
    let entities = geometry(name, &text_style)?;
    crate::app::define_block_raw_doc(doc, name, Vector3::ZERO, &entities);
    doc.block_records.get(name).map(|record| record.handle)
}

/// The predefined blocks, sized in drawing units whatever the drawing's
/// units: a half-unit callout circle, quarter-unit tags and attributes
/// 0.09375 high, centred on their baseline.
fn geometry(name: &str, text_style: &str) -> Option<Vec<EntityType>> {
    let line = |x1: f64, y1: f64, x2: f64, y2: f64| {
        EntityType::Line(Line::from_points(
            Vector3::new(x1, y1, 0.0),
            Vector3::new(x2, y2, 0.0),
        ))
    };
    let circle = |r: f64| EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, r));
    let arc = |cx: f64, start: f64, end: f64| {
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(cx, 0.0, 0.0),
            0.125,
            start,
            end,
        ))
    };
    let closed = |points: &[(f64, f64)]| {
        let mut p = LwPolyline::new();
        for &(x, y) in points {
            p.vertices.push(LwVertex::from_coords(x, y));
        }
        p.is_closed = true;
        EntityType::LwPolyline(p)
    };
    let attribute = |tag: &str, prompt: &str, y: f64| {
        let mut a =
            AttributeDefinition::new(tag.to_string(), prompt.to_string(), tag.to_string());
        a.insertion_point = Vector3::new(0.0, y, 0.0);
        a.alignment_point = Vector3::new(0.0, y, 0.0);
        a.height = 0.09375;
        a.text_style = text_style.to_string();
        a.set_alignment(HorizontalAlignment::Center, VerticalAlignment::Baseline);
        EntityType::AttributeDefinition(a)
    };
    let tag = || attribute("TAGNUMBER", "Enter tag number", -0.046875);
    let (half_pi, three_half_pi) = (std::f64::consts::FRAC_PI_2, 3.0 * std::f64::consts::FRAC_PI_2);
    let (hx, hy) = (0.125 / 3f64.sqrt() * 2.0, 0.125);
    let tx = 0.25 * 3f64.sqrt() / 2.0;
    let mut entities = match name {
        "_DetailCallout" => vec![
            circle(0.25),
            line(0.25, 0.0, -0.25, 0.0),
            attribute("VIEWNUMBER", "Enter view number", 1.0 / 48.0),
            attribute("SHEETNUMBER", "Enter sheet number", -0.15625),
        ],
        "_TagSlot" => vec![
            arc(-0.125, half_pi, three_half_pi),
            arc(0.125, three_half_pi, half_pi),
            line(-0.125, 0.125, 0.125, 0.125),
            line(-0.125, -0.125, 0.125, -0.125),
            tag(),
        ],
        "_TagCircle" => vec![circle(0.125), tag()],
        "_TagBox" => vec![
            closed(&[(-0.125, 0.125), (0.125, 0.125), (0.125, -0.125), (-0.125, -0.125)]),
            tag(),
        ],
        "_TagHexagon" => vec![
            closed(&[
                (-hx, 0.0),
                (-hx / 2.0, hy),
                (hx / 2.0, hy),
                (hx, 0.0),
                (hx / 2.0, -hy),
                (-hx / 2.0, -hy),
            ]),
            tag(),
        ],
        "_TagTriangle" => vec![closed(&[(-tx, -0.125), (0.0, 0.25), (tx, -0.125)]), tag()],
        _ => return None,
    };
    // The content takes the leader's block colour and transparency; curves
    // also its linetype, and all but polylines its lineweight. Attributes
    // keep the layer's linetype and lineweight.
    for entity in &mut entities {
        let attribute = matches!(entity, EntityType::AttributeDefinition(_));
        let polyline = matches!(entity, EntityType::LwPolyline(_));
        let common = entity.common_mut();
        common.color = Color::ByBlock;
        common.transparency = Transparency::BY_BLOCK;
        if !attribute {
            common.linetype = "ByBlock".to_string();
        }
        if !attribute && !polyline {
            common.line_weight = LineWeight::ByBlock;
        }
    }
    Some(entities)
}
