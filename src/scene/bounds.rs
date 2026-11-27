//! Boxes of entities and block references, measured through their block
//! definitions so a reference counts what it draws.

/// Copy/paste anchor: the lower-left corner of the bounding box that encloses
/// every copied entity. Standard clipboard behaviour puts this corner under the
/// cursor at paste time, so the whole selection drops down-and-right of the pick.
///
/// Unions each entity's own bounding box (O(entities)) rather than averaging
/// every tessellated wire vertex, which cost O(total geometry) and stalled a
/// whole-drawing copy. The per-entity `min` corners give the exact enclosing
/// box's lower-left.
///
/// A block reference's own `bounding_box()` is only its insertion point, which
/// can lie far from what it draws (a block drawn in survey coordinates and
/// inserted at their negative), so references are measured through their block
/// definition instead.
pub(crate) fn entities_lower_left_by_bbox(
    doc: &document::Document,
    handles: &[document::Handle],
) -> glam::DVec3 {
    let mut blocks = rustc_hash::FxHashMap::default();
    let mut min = glam::DVec3::splat(f64::INFINITY);
    let mut any = false;
    for &h in handles {
        let Some(e) = doc.get_entity(h) else { continue };
        let bb = match e {
            document::EntityType::Insert(insert) => insert_bounds(doc, insert, &mut blocks, 0)
                .unwrap_or_else(|| e.as_entity().bounding_box()),
            _ => e.as_entity().bounding_box(),
        };
        let lo = glam::DVec3::new(bb.min.x, bb.min.y, bb.min.z);
        if lo.x.is_finite() && lo.y.is_finite() && lo.z.is_finite() {
            min = min.min(lo);
            any = true;
        }
    }
    if any {
        min
    } else {
        glam::DVec3::ZERO
    }
}

type BlockBounds = rustc_hash::FxHashMap<String, Option<document::types::BoundingBox3D>>;

/// Box enclosing every listed entity, measured through block definitions so a
/// reference contributes what it draws and not just its insertion point.
pub(crate) fn entities_bounds(
    doc: &document::Document,
    handles: &[document::Handle],
) -> Option<document::types::BoundingBox3D> {
    let mut blocks = rustc_hash::FxHashMap::default();
    let mut out: Option<document::types::BoundingBox3D> = None;
    for &handle in handles {
        let Some(entity) = doc.get_entity(handle) else {
            continue;
        };
        let bb = match entity {
            document::EntityType::Insert(insert) => {
                insert_bounds(doc, insert, &mut blocks, 0).unwrap_or_default()
            }
            _ => {
                let (lo, hi) = crate::scene::convert::tess::entity_bounds(entity);
                document::types::BoundingBox3D::new(
                    document::types::Vector3::new(lo[0], lo[1], lo[2]),
                    document::types::Vector3::new(hi[0], hi[1], hi[2]),
                )
            }
        };
        if is_default_box(&bb) {
            continue;
        }
        include(&mut out, bb.min);
        include(&mut out, bb.max);
    }
    out
}

/// Nesting deeper than this is treated as a cycle.
const MAX_BLOCK_NESTING: usize = 32;

/// World box of a block reference: its block's box (relative to the block's
/// base point) carried through the reference's transform for every MINSERT
/// cell, plus its attributes. `None` when nothing in it can be measured.
fn insert_bounds(
    doc: &document::Document,
    insert: &document::entities::Insert,
    blocks: &mut BlockBounds,
    depth: usize,
) -> Option<document::types::BoundingBox3D> {
    let mut out = None;
    if let Some(local) = block_bounds(doc, &insert.block_name, blocks, depth) {
        // MINSERT cells differ only in their insertion point and the grid is
        // affine, so the four corner cells span the whole array.
        let cells = if insert.is_minsert() {
            let points = insert.array_points();
            let columns = (insert.column_count as usize).max(1);
            [
                0,
                columns - 1,
                points.len().saturating_sub(columns),
                points.len().saturating_sub(1),
            ]
            .iter()
            .filter_map(|&k| points.get(k).copied())
            .collect::<Vec<_>>()
        } else {
            vec![insert.insert_point]
        };
        let mut placed = insert.clone();
        for cell in cells {
            placed.insert_point = cell;
            let transform = placed.get_transform();
            for corner in box_corners(&local) {
                include(&mut out, transform.apply(corner));
            }
        }
    }
    for attribute in &insert.attributes {
        let bb = document::Entity::bounding_box(attribute);
        if !is_default_box(&bb) {
            include(&mut out, bb.min);
            include(&mut out, bb.max);
        }
    }
    out
}

/// World box of one block reference, measured through its definition.
pub(crate) fn insert_world_bounds(
    doc: &document::Document,
    insert: &document::entities::Insert,
) -> Option<document::types::BoundingBox3D> {
    insert_bounds(doc, insert, &mut BlockBounds::default(), 0)
}

/// Box of the named block definition's contents, relative to its base point.
pub(crate) fn block_definition_bounds(
    doc: &document::Document,
    name: &str,
) -> Option<document::types::BoundingBox3D> {
    block_bounds(doc, name, &mut BlockBounds::default(), 0)
}

/// Box of a block definition's contents, relative to its base point. Attribute
/// definitions are left out: a reference draws its attributes instead.
fn block_bounds(
    doc: &document::Document,
    name: &str,
    blocks: &mut BlockBounds,
    depth: usize,
) -> Option<document::types::BoundingBox3D> {
    if let Some(known) = blocks.get(name) {
        return *known;
    }
    if depth >= MAX_BLOCK_NESTING {
        return None;
    }
    // Placeholder while measuring, so a block that contains itself ends.
    blocks.insert(name.to_string(), None);
    let record = doc.block_records.get(name)?;
    // The one place the base-point convention lives (dynamic blocks' BASEPOINT
    // parameter included).
    let base = crate::scene::render_graph::block_base_point(doc, name);
    let mut out = None;
    for &h in &record.entity_handles {
        let Some(e) = doc.get_entity(h) else { continue };
        let bb = match e {
            document::EntityType::AttributeDefinition(_) => continue,
            document::EntityType::Insert(nested) => {
                match insert_bounds(doc, nested, blocks, depth + 1) {
                    Some(bb) => bb,
                    None => continue,
                }
            }
            _ => e.as_entity().bounding_box(),
        };
        if is_default_box(&bb) {
            continue;
        }
        include(&mut out, bb.min - base);
        include(&mut out, bb.max - base);
    }
    blocks.insert(name.to_string(), out);
    out
}

/// The all-zero box is `bounding_box()`'s default for entities that do not
/// measure themselves: it says nothing about where they are.
fn is_default_box(bb: &document::types::BoundingBox3D) -> bool {
    bb.min == document::types::Vector3::ZERO && bb.max == document::types::Vector3::ZERO
}

fn include(out: &mut Option<document::types::BoundingBox3D>, point: document::types::Vector3) {
    if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
        return;
    }
    match out {
        Some(bb) => bb.expand_to_include(point),
        None => *out = Some(document::types::BoundingBox3D::from_point(point)),
    }
}

fn box_corners(bb: &document::types::BoundingBox3D) -> [document::types::Vector3; 8] {
    let (a, b) = (bb.min, bb.max);
    let v = document::types::Vector3::new;
    [
        v(a.x, a.y, a.z),
        v(b.x, a.y, a.z),
        v(a.x, b.y, a.z),
        v(b.x, b.y, a.z),
        v(a.x, a.y, b.z),
        v(b.x, a.y, b.z),
        v(a.x, b.y, b.z),
        v(b.x, b.y, b.z),
    ]
}
