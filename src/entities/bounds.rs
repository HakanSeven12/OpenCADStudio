//! Boxes of entities and block references, measured through their block
//! definitions so a reference counts what it draws.

use crate::display::wire_model::WireModel;

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
                let (lo, hi) = entity_bounds(entity);
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
    let base = crate::entities::block_ref::block_base_point(doc, name);
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

pub(crate) fn entity_bounds(e: &document::EntityType) -> ([f64; 3], [f64; 3]) {
    if let document::EntityType::Region(region) = e {
        if region.wires.is_empty() {
            if let Some(bounds) = crate::entities::solid3d::region_record(region)
                .and_then(|body| ocs_solid::model::aabb(&body))
            {
                return (bounds.min, bounds.max);
            }
        }
    }
    if let document::EntityType::Line(line) = e {
        if let Some(association) =
            document::entities::CenterMarkAssociation::read(&line.common.extended_data)
        {
            return crate::entities::centermark::mark_bounds(&association);
        }
    }
    if let document::EntityType::Solid(solid) = e {
        if let Some(bounds) = crate::entities::solid::wcs_bounds(solid) {
            return (bounds.min, bounds.max);
        }
    }
    if let document::EntityType::Spline(spline) = e {
        if let Some(bounds) = crate::entities::spline::fit_geometry_bounds(spline) {
            return (bounds.min, bounds.max);
        }
    }
    if let document::EntityType::Ellipse(ellipse) = e {
        if let Some(bounds) = ellipse_bounds(ellipse) {
            return bounds;
        }
    }
    let bounds = e.as_entity().bounding_box();
    (
        [bounds.min.x, bounds.min.y, bounds.min.z],
        [bounds.max.x, bounds.max.y, bounds.max.z],
    )
}

/// Tight WCS box of an ellipse or elliptical arc: the swept endpoints plus
/// every axis-extreme parameter inside the sweep. The generic
/// `bounding_box()` uses the major radius on both axes.
pub(crate) fn ellipse_bounds(el: &document::entities::Ellipse) -> Option<([f64; 3], [f64; 3])> {
    let (c, a, n) = (el.center, el.major_axis, el.normal);
    let r = el.minor_axis_ratio;
    // A stored normal need not be unit length; the minor axis must be.
    let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    if !(len > 0.0) {
        return None;
    }
    let n = document::types::Vector3::new(n.x / len, n.y / len, n.z / len);
    // Minor axis = normal x major, scaled by the ratio.
    let b = [
        (n.y * a.z - n.z * a.y) * r,
        (n.z * a.x - n.x * a.z) * r,
        (n.x * a.y - n.y * a.x) * r,
    ];
    let a = [a.x, a.y, a.z];
    if !(a.iter().chain(b.iter()).all(|v| v.is_finite())) || a == [0.0; 3] {
        return None;
    }
    let tau = std::f64::consts::TAU;
    let start = el.start_parameter;
    let mut sweep = (el.end_parameter - start).rem_euclid(tau);
    if sweep <= 1e-12 {
        sweep = tau;
    }
    let mut params = vec![start, start + sweep];
    for k in 0..3 {
        // d/dt (a cos t + b sin t) = 0  ->  t = atan2(b, a) (+ pi)
        let t0 = b[k].atan2(a[k]);
        for t in [t0, t0 + std::f64::consts::PI] {
            let off = (t - start).rem_euclid(tau);
            if off <= sweep {
                params.push(start + off);
            }
        }
    }
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let cc = [c.x, c.y, c.z];
    for t in params {
        let (ct, st) = (t.cos(), t.sin());
        for k in 0..3 {
            let v = cc[k] + a[k] * ct + b[k] * st;
            min[k] = min[k].min(v);
            max[k] = max[k].max(v);
        }
    }
    Some((min, max))
}

/// [`entity_bounds`] with the document, for entities whose extent is not
/// stored on the entity: an underlay spans its page (or its clip), and a block
/// reference spans what its definition draws once the reference transform is
/// applied to it.
pub(crate) fn entity_bounds_in(
    document: &document::Document,
    e: &document::EntityType,
) -> ([f64; 3], [f64; 3]) {
    if let document::EntityType::Underlay(underlay) = e {
        if let Some(bounds) = crate::entities::underlay::world_bounds(underlay, document) {
            return bounds;
        }
    }
    if let document::EntityType::Insert(insert) = e {
        if let Some(bounds) = crate::entities::bounds::insert_world_bounds(document, insert) {
            return (
                [bounds.min.x, bounds.min.y, bounds.min.z],
                [bounds.max.x, bounds.max.y, bounds.max.z],
            );
        }
    }
    entity_bounds(e)
}

/// [`entity_aabb`] with the document (see [`entity_bounds_in`]).
pub(crate) fn entity_aabb_in(document: &document::Document, e: &document::EntityType) -> [f32; 4] {
    if let document::EntityType::Underlay(underlay) = e {
        if let Some((min, max)) = crate::entities::underlay::world_bounds(underlay, document) {
            return [min[0] as f32, min[1] as f32, max[0] as f32, max[1] as f32];
        }
    }
    entity_aabb(e)
}

pub(crate) fn entity_aabb(e: &document::EntityType) -> [f32; 4] {
    let (min, max) = entity_bounds(e);
    let min_x = min[0] as f32;
    let min_y = min[1] as f32;
    let max_x = max[0] as f32;
    let max_y = max[1] as f32;
    // The all-zero box is bounding_box()'s Default — returned by entities with
    // no usable box (unimplemented) — so treat it as UNBOUNDED (never
    // pre-rejected). A genuinely zero-size box *away* from the origin (e.g. a
    // POINT) is a valid, cullable position: keep it, otherwise point-heavy
    // drawings fill the always-checked set and stall hit-testing.
    if min_x == 0.0 && min_y == 0.0 && max_x == 0.0 && max_y == 0.0 {
        return WireModel::UNBOUNDED_AABB;
    }
    [min_x, min_y, max_x, max_y]
}

pub(crate) fn civil_drawn_cache(blob: &[u8]) -> bool {
    // An 8-byte header, then records of `u32 size, u32 type, payload`.
    blob.get(12..16).is_some_and(|t| u32::from_le_bytes([t[0], t[1], t[2], t[3]]) != 36)
}
