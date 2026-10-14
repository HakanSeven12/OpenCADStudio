// TXTEXP — replace text with the outlines of its characters.
//
// Each glyph contour of a TEXT or MTEXT becomes a polyline on the text's
// layer and with its properties: a lightweight polyline when the text lies
// flat, a 3D polyline when it is tilted.

use codec::entities::{LwPolyline, LwVertex, Polyline3D, Vertex3DPolyline};
use codec::types::Vector2;
use codec::{CadDocument, EntityType, Handle};

use crate::scene::convert::acad_to_render::{convert, RenderObject};

/// The outlines replacing each explodable text of `selected`, as
/// (text, polylines) pairs ready for `apply_explode_replacements`.
pub fn plan_text_explode(
    selected: &[(Handle, &EntityType)],
    document: &CadDocument,
) -> Vec<(Handle, Vec<EntityType>)> {
    selected
        .iter()
        .filter_map(|(handle, entity)| {
            let outlines = text_outlines(entity, document);
            (!outlines.is_empty()).then_some((*handle, outlines))
        })
        .collect()
}

/// The character outlines of a TEXT or MTEXT; empty for anything else.
pub fn text_outlines(entity: &EntityType, document: &CadDocument) -> Vec<EntityType> {
    let (common, elevation) = match entity {
        EntityType::Text(text) => (&text.common, text.insertion_point.z),
        EntityType::MText(text) => (&text.common, text.insertion_point.z),
        _ => return Vec::new(),
    };
    let Some(RenderObject::Text(groups)) = convert(entity, document).map(|render| render.object)
    else {
        return Vec::new();
    };
    let mut common = common.clone();
    common.handle = Handle::NULL;
    let mut outlines = Vec::new();
    for group in &groups {
        for stroke in group.strokes.iter().filter(|stroke| stroke.len() >= 2) {
            let mut points: Vec<[f64; 3]> = stroke
                .iter()
                .map(|&[x, y]| {
                    let (x, y) = (x as f64, y as f64);
                    match group.plane {
                        Some(plane) => std::array::from_fn(|axis| {
                            plane.origin[axis] + plane.x_axis[axis] * x + plane.y_axis[axis] * y
                        }),
                        None => [group.origin[0] + x, group.origin[1] + y, elevation],
                    }
                })
                .collect();
            let closed = points.len() > 2 && {
                let (first, last) = (points[0], points[points.len() - 1]);
                (0..3).all(|axis| (first[axis] - last[axis]).abs() <= 1e-9)
            };
            if closed {
                points.pop();
            }
            let z = points[0][2];
            if points.iter().all(|point| (point[2] - z).abs() <= 1e-9) {
                let mut polyline = LwPolyline::new();
                polyline.common = common.clone();
                polyline.is_closed = closed;
                polyline.elevation = z;
                polyline.vertices = points
                    .iter()
                    .map(|point| LwVertex::new(Vector2::new(point[0], point[1])))
                    .collect();
                outlines.push(EntityType::LwPolyline(polyline));
            } else {
                let mut polyline = Polyline3D::new();
                polyline.common = common.clone();
                polyline.flags.closed = closed;
                polyline.vertices = points
                    .iter()
                    .map(|point| Vertex3DPolyline::from_xyz(point[0], point[1], point[2]))
                    .collect();
                outlines.push(EntityType::Polyline3D(polyline));
            }
        }
    }
    outlines
}
