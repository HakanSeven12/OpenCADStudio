//! Model files IMPORT reads: ACIS text (SAT), STEP and IGES, as the drawing
//! objects they become.

use document::entities::{Entity as _, Insert};
use document::types::{Color, Matrix4, Transform, Vector3};
use document::EntityType;

use crate::scene::convert::acis_import::{painted_body_entity, sat_entities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    Sat,
    Step,
    Iges,
}

/// The objects a file brings in.
pub struct Imported {
    /// Block definitions, each before any block that inserts it.
    pub blocks: Vec<(String, Vec<EntityType>)>,
    /// The objects for the current space.
    pub entities: Vec<EntityType>,
}

/// File units over drawing units; one where the drawing is unitless.
fn ratio(file_mm: f64, drawing_mm: Option<f64>) -> f64 {
    drawing_mm.filter(|mm| *mm > 0.0).map_or(1.0, |mm| file_mm / mm)
}

/// A block name from a product's, with the characters names cannot hold
/// replaced.
fn block_name(name: &str) -> String {
    crate::scene::sanitized_table_name(name, "Part")
}

/// A STEP presentation colour as the drawing colour it imports as: one of
/// the standard index colours red, yellow, green, cyan, blue, magenta,
/// white or the dark grey 250 (which black also becomes) when it is close
/// to it, otherwise the true colour. Measured on the 8-bit channels:
/// white is an intensity (channel mean) of at least 0.93 with an HSV
/// saturation of at most 0.2; grey 250 an intensity from 0.16 to 0.39 with
/// a saturation below 0.2; the six hues a value of at least 0.82, a
/// saturation of at least 0.75 and a hue within 7.5° of theirs (the hue
/// angle of the colour's projection across the grey axis).
pub fn step_colour(rgb: crate::io::step_import::Rgb) -> Color {
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
    let true_colour = Color::Rgb {
        r: r as u8,
        g: g as u8,
        b: b as u8,
    };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == 0.0 {
        return Color::Index(250);
    }
    let saturation = (max - min) / max;
    let intensity = (r + g + b) / 3.0 / 255.0;
    if saturation <= 0.2 && intensity >= 0.93 {
        return Color::Index(7);
    }
    if saturation < 0.2 && (0.16..=0.39).contains(&intensity) {
        return Color::Index(250);
    }
    if saturation < 0.75 || max / 255.0 < 0.82 {
        return true_colour;
    }
    let hue = (3f64.sqrt() / 2.0 * (g - b))
        .atan2(r - (g + b) / 2.0)
        .to_degrees();
    let sector = (hue / 60.0).round();
    if (hue - 60.0 * sector).abs() > 7.5 {
        return true_colour;
    }
    // Red, yellow, green, cyan, blue, magenta.
    Color::Index([1, 2, 3, 4, 5, 6][sector.rem_euclid(6.0) as usize])
}

/// An insert of `block` placed by `placement` (columns: axes, origin).
fn placed_insert(block: &str, placement: &[[f64; 4]; 3]) -> EntityType {
    let mut insert = Insert::new(block, Vector3::ZERO);
    let mut m = Matrix4::identity();
    for r in 0..3 {
        m.m[r] = placement[r];
    }
    insert.apply_transform(&Transform::from_matrix(m));
    EntityType::Insert(insert)
}

/// STEP: a block per product — its solids, and inserts of its components —
/// with each top product inserted at the origin, scaled from the file's
/// units to the drawing's. The solids take their colour from the block.
fn step_blocks(model: crate::io::step_import::StepModel, scale: f64, stem: &str) -> Imported {
    // Unique names, in product order.
    let mut names: Vec<String> = Vec::new();
    for product in &model.products {
        let base = match product.numbered {
            Some(id) => block_name(&format!("{stem}-{id}")),
            None => block_name(&product.name),
        };
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|taken| taken.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names.push(name);
    }
    // Children before parents. A link that closes a loop in the file's
    // assembly (a product that would contain itself) is dropped, so no
    // block ends up inserting itself.
    let mut order = Vec::new();
    let mut dropped: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut state = vec![0u8; model.products.len()];
    for &root in &model.roots {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        // Each product being walked and the next of its children to visit.
        let mut stack = vec![(root, 0usize)];
        while let Some((k, next)) = stack.last_mut() {
            let k = *k;
            match model.products[k].children.get(*next) {
                Some(&(child, _)) => {
                    let link = (k, *next);
                    *next += 1;
                    match state[child] {
                        0 => {
                            state[child] = 1;
                            stack.push((child, 0));
                        }
                        1 => {
                            dropped.insert(link);
                        }
                        _ => {}
                    }
                }
                None => {
                    state[k] = 2;
                    order.push(k);
                    stack.pop();
                }
            }
        }
    }
    let blocks = order
        .iter()
        .map(|&k| {
            let product = &model.products[k];
            let mut entities: Vec<EntityType> = product
                .bodies
                .iter()
                .filter_map(|body| {
                    let faces: Vec<Option<Color>> = body.faces.iter().map(|c| c.map(step_colour)).collect();
                    let colour = body.colour.map(step_colour);
                    let mut entity = painted_body_entity(&body.body, colour, &faces, false)?;
                    if product.scale != 1.0 {
                        entity.apply_transform(&Transform::from_scale(product.scale));
                    }
                    Some(entity)
                })
                .collect();
            entities.extend(
                product
                    .children
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| !dropped.contains(&(k, *n)))
                    .map(|(_, (child, placement))| {
                        let mut insert = placed_insert(&names[*child], placement);
                        insert.common_mut().color = Color::ByBlock;
                        insert
                    }),
            );
            (names[k].clone(), entities)
        })
        .collect();
    let identity = [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, scale, 0.0]];
    // Solids placed straight into the drawing keep the colour by block
    // they have in a block.
    let mut direct: Vec<EntityType> = model
        .direct
        .iter()
        .filter_map(|body| {
            let faces: Vec<Option<Color>> = body.faces.iter().map(|c| c.map(step_colour)).collect();
            let colour = body.colour.map(step_colour);
            let mut entity = painted_body_entity(&body.body, colour, &faces, false)?;
            if scale != 1.0 {
                entity.apply_transform(&Transform::from_scale(scale));
            }
            Some(entity)
        })
        .collect();
    direct.extend(model.roots.iter().map(|&k| placed_insert(&names[k], &identity)));
    Imported { blocks, entities: direct }
}

/// `None` when the file is not of `format` or holds nothing it can read.
/// `stem` names the blocks of a STEP file without products.
pub fn read(
    format: ModelFormat,
    bytes: &[u8],
    drawing_mm: Option<f64>,
    stem: &str,
) -> Option<Imported> {
    let imported = match format {
        ModelFormat::Sat => Imported {
            blocks: Vec::new(),
            entities: sat_entities(&String::from_utf8_lossy(bytes))?,
        },
        ModelFormat::Step => {
            let model = crate::io::step_import::read(bytes)?;
            // A file that names no length unit is read in drawing units.
            let scale = model.unit_mm.map_or(1.0, |mm| ratio(mm, drawing_mm));
            step_blocks(model, scale, stem)
        }
        ModelFormat::Iges => {
            // Solids, then the surfaces as surfaces however flat, then the
            // free curves as splines; faces and edges carry the object's
            // colour, every object its level's layer.
            let model = crate::io::iges_import::read(bytes, drawing_mm)?;
            let solids = model.bodies.iter().map(|body| (body, false));
            let sheets = model.sheets.iter().map(|body| (body, true));
            let mut entities: Vec<EntityType> = solids
                .chain(sheets)
                .filter_map(|((body, color, layer), sheet)| {
                    let mut entity = painted_body_entity(body, Some(*color), &[], sheet)?;
                    entity.common_mut().layer = layer.clone();
                    Some(entity)
                })
                .collect();
            for (curve, color, layer) in &model.curves {
                let mut spline = document::entities::Spline::new();
                crate::entities::spline::replace_with_nurbs(&mut spline, curve);
                spline.common.color = *color;
                spline.common.layer = layer.clone();
                entities.push(EntityType::Spline(spline));
            }
            Imported {
                blocks: Vec::new(),
                entities,
            }
        }
    };
    let solids = imported
        .blocks
        .iter()
        .flat_map(|(_, entities)| entities)
        .chain(&imported.entities)
        .any(|entity| !matches!(entity, EntityType::Insert(_)));
    solids.then_some(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Band edges measured on imported STEP colours (8-bit channels).
    #[test]
    fn step_colour_bands() {
        let index = |c: [u8; 3]| match step_colour(c.map(|v| f64::from(v) / 255.0)) {
            Color::Index(i) => i,
            _ => 0,
        };
        let cases: [([u8; 3], u8); 14] = [
            ([255, 36, 0], 1),
            ([255, 37, 0], 0),
            ([210, 0, 0], 1),
            ([209, 0, 0], 0),
            ([255, 63, 63], 1),
            ([255, 64, 64], 0),
            ([0, 0, 0], 250),
            ([1, 1, 1], 0),
            ([102, 99, 96], 250),
            ([42, 40, 40], 0),
            ([90, 90, 72], 0),
            ([255, 255, 204], 7),
            ([240, 240, 233], 7),
            ([237, 237, 237], 0),
        ];
        for (c, want) in cases {
            assert_eq!(index(c), want, "{c:?}");
        }
    }
}
