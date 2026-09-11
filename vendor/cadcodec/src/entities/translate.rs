//! Centralized translation implementations for all entity types.
//!
//! Each entity's `translate` logic is defined here as a standalone function,
//! and [`EntityType`] exposes a convenience dispatch method.

use super::*;
use crate::types::{Matrix3, Transform, Vector3};

// ── Point ────────────────────────────────────────────────────────────────────

/// Map a WCS displacement into a planar entity's OCS frame. ARC / CIRCLE /
/// TEXT / SOLID / LWPOLYLINE / ... store their coordinates in OCS, so adding
/// a raw world offset moves a mirrored entity (normal 0,0,-1) the wrong way.
/// `arbitrary_axis` of the default +Z normal is the identity, so ordinary
/// entities are unaffected.
fn ocs_offset(normal: Vector3, offset: Vector3) -> Vector3 {
    Matrix3::arbitrary_axis(normal).transpose() * offset
}

pub(crate) fn translate_point(e: &mut Point, offset: Vector3) {
    e.location = e.location + offset;
}

// ── Line ─────────────────────────────────────────────────────────────────────

pub(crate) fn translate_line(e: &mut Line, offset: Vector3) {
    e.start = e.start + offset;
    e.end = e.end + offset;
}

// ── Circle ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_circle(e: &mut Circle, offset: Vector3) {
    e.center = e.center + ocs_offset(e.normal, offset);
}

// ── Arc ──────────────────────────────────────────────────────────────────────

pub(crate) fn translate_arc(e: &mut Arc, offset: Vector3) {
    e.center = e.center + ocs_offset(e.normal, offset);
}

// ── Ellipse ──────────────────────────────────────────────────────────────────

pub(crate) fn translate_ellipse(e: &mut Ellipse, offset: Vector3) {
    e.center = e.center + offset;
}

// ── Polyline (3D heavy) ──────────────────────────────────────────────────────

pub(crate) fn translate_polyline(e: &mut Polyline, offset: Vector3) {
    for vertex in &mut e.vertices {
        vertex.location = vertex.location + offset;
    }
}

// ── Polyline2D ───────────────────────────────────────────────────────────────

pub(crate) fn translate_polyline2d(e: &mut Polyline2D, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    for vertex in &mut e.vertices {
        vertex.location = vertex.location + offset;
    }
}

// ── Polyline3D ───────────────────────────────────────────────────────────────

pub(crate) fn translate_polyline3d(e: &mut Polyline3D, offset: Vector3) {
    for v in &mut e.vertices {
        v.position = v.position + offset;
    }
}

// ── LwPolyline ───────────────────────────────────────────────────────────────

pub(crate) fn translate_lwpolyline(e: &mut LwPolyline, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    for vertex in &mut e.vertices {
        vertex.location.x += offset.x;
        vertex.location.y += offset.y;
    }
    e.elevation += offset.z;
}

// ── Text ─────────────────────────────────────────────────────────────────────

pub(crate) fn translate_text(e: &mut Text, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    e.insertion_point = e.insertion_point + offset;
    if let Some(ref mut align) = e.alignment_point {
        *align = *align + offset;
    }
}

// ── MText ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_mtext(e: &mut MText, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── Spline ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_spline(e: &mut Spline, offset: Vector3) {
    for point in &mut e.control_points {
        *point = *point + offset;
    }
    for point in &mut e.fit_points {
        *point = *point + offset;
    }
}

pub(crate) fn translate_helix(e: &mut Helix, offset: Vector3) {
    translate_spline(&mut e.spline, offset);
    e.axis_base_point = e.axis_base_point + offset;
    e.start_point = e.start_point + offset;
    // axis_vector is a direction — unchanged by translation.
}

// ── Dimension ────────────────────────────────────────────────────────────────

pub(crate) fn translate_dimension(e: &mut Dimension, offset: Vector3) {
    e.set_definition_point(e.definition_point() + offset);
    match e {
        Dimension::Aligned(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.first_point = d.first_point + offset;
            d.second_point = d.second_point + offset;
        }
        Dimension::Linear(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.first_point = d.first_point + offset;
            d.second_point = d.second_point + offset;
        }
        Dimension::Radius(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.angle_vertex = d.angle_vertex + offset;
        }
        Dimension::Diameter(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.angle_vertex = d.angle_vertex + offset;
        }
        Dimension::Angular2Ln(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.dimension_arc = d.dimension_arc + offset;
            d.angle_vertex = d.angle_vertex + offset;
            d.first_point = d.first_point + offset;
            d.second_point = d.second_point + offset;
        }
        Dimension::Angular3Pt(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.angle_vertex = d.angle_vertex + offset;
            d.first_point = d.first_point + offset;
            d.second_point = d.second_point + offset;
        }
        Dimension::Ordinate(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.feature_location = d.feature_location + offset;
            d.leader_endpoint = d.leader_endpoint + offset;
        }
        Dimension::Arc(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.first_extension_point = d.first_extension_point + offset;
            d.second_extension_point = d.second_extension_point + offset;
            d.center_point = d.center_point + offset;
            d.first_leader_point = d.first_leader_point + offset;
            d.second_leader_point = d.second_leader_point + offset;
        }
        Dimension::LargeRadial(d) => {
            d.base.text_middle_point = d.base.text_middle_point + offset;
            d.chord_point = d.chord_point + offset;
            d.override_center = d.override_center + offset;
            d.jog_point = d.jog_point + offset;
        }
    }
}

// ── Hatch ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_hatch(e: &mut Hatch, offset: Vector3) {
    let ocs_to_wcs = Matrix3::arbitrary_axis(e.normal);
    let wcs_to_ocs = ocs_to_wcs.transpose();
    let offset_ocs = wcs_to_ocs * offset;

    e.elevation += offset_ocs.z;

    for path in &mut e.paths {
        for edge in &mut path.edges {
            match edge {
                BoundaryEdge::Line(line) => {
                    line.start.x += offset_ocs.x;
                    line.start.y += offset_ocs.y;
                    line.end.x += offset_ocs.x;
                    line.end.y += offset_ocs.y;
                }
                BoundaryEdge::CircularArc(arc) => {
                    arc.center.x += offset_ocs.x;
                    arc.center.y += offset_ocs.y;
                }
                BoundaryEdge::EllipticArc(ellipse) => {
                    ellipse.center.x += offset_ocs.x;
                    ellipse.center.y += offset_ocs.y;
                }
                BoundaryEdge::Spline(spline) => {
                    for cp in &mut spline.control_points {
                        cp.x += offset_ocs.x;
                        cp.y += offset_ocs.y;
                    }
                    for fp in &mut spline.fit_points {
                        fp.x += offset_ocs.x;
                        fp.y += offset_ocs.y;
                    }
                }
                BoundaryEdge::Polyline(poly) => {
                    for v in &mut poly.vertices {
                        v.x += offset_ocs.x;
                        v.y += offset_ocs.y;
                    }
                }
            }
        }
    }

    for seed in &mut e.seed_points {
        seed.x += offset_ocs.x;
        seed.y += offset_ocs.y;
    }
}

// ── Solid ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_solid(e: &mut Solid, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    e.first_corner = e.first_corner + offset;
    e.second_corner = e.second_corner + offset;
    e.third_corner = e.third_corner + offset;
    e.fourth_corner = e.fourth_corner + offset;
}

// ── Face3D ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_face3d(e: &mut Face3D, offset: Vector3) {
    e.first_corner = e.first_corner + offset;
    e.second_corner = e.second_corner + offset;
    e.third_corner = e.third_corner + offset;
    e.fourth_corner = e.fourth_corner + offset;
}

// ── Insert ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_insert(e: &mut Insert, offset: Vector3) {
    // `insert_point` lives in the OCS defined by `normal`, so a world-space
    // offset can't be added to it directly — convert to world, add, convert
    // back. For a +Z normal the OCS is the identity and this is a plain add.
    let ocs = Matrix3::arbitrary_axis(e.normal);
    let world = ocs * e.insert_point + offset;
    e.insert_point = ocs.transpose() * world;

    // Attributes are positioned in world space beside the block; move them by
    // the same world offset so they follow the INSERT (mirrors transform_insert
    // — without this a MOVE left the attribute text behind).
    for att in &mut e.attributes {
        translate_attribute_entity(att, offset);
    }
}

// ── Block ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_block(e: &mut Block, offset: Vector3) {
    e.base_point = e.base_point + offset;
}

// ── BlockEnd ─────────────────────────────────────────────────────────────────

pub(crate) fn translate_block_end(_e: &mut BlockEnd, _offset: Vector3) {
    // No geometry
}

// ── Ray ──────────────────────────────────────────────────────────────────────

pub(crate) fn translate_ray(e: &mut Ray, offset: Vector3) {
    e.base_point = e.base_point + offset;
}

// ── XLine ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_xline(e: &mut XLine, offset: Vector3) {
    e.base_point = e.base_point + offset;
}

// ── Viewport ─────────────────────────────────────────────────────────────────

pub(crate) fn translate_viewport(e: &mut Viewport, offset: Vector3) {
    e.center = e.center + offset;
}

// ── AttributeDefinition ──────────────────────────────────────────────────────

pub(crate) fn translate_attribute_definition(e: &mut AttributeDefinition, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    e.insertion_point = e.insertion_point + offset;
    e.alignment_point = e.alignment_point + offset;
}

// ── AttributeEntity ──────────────────────────────────────────────────────────

pub(crate) fn translate_attribute_entity(e: &mut AttributeEntity, offset: Vector3) {
    let offset = ocs_offset(e.normal, offset);
    e.insertion_point = e.insertion_point + offset;
    e.alignment_point = e.alignment_point + offset;
}

// ── Leader ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_leader(e: &mut Leader, offset: Vector3) {
    for vertex in &mut e.vertices {
        *vertex = *vertex + offset;
    }
    e.block_offset = e.block_offset + offset;
    e.annotation_offset = e.annotation_offset + offset;
}

// ── MultiLeader ──────────────────────────────────────────────────────────────

pub(crate) fn translate_multileader(e: &mut MultiLeader, offset: Vector3) {
    // Move every anchor the render reads. The text / block content / base
    // points were previously skipped, so a MOVE shifted the leader line but
    // left the label (and dogleg break points) behind.
    e.context.content_base_point = e.context.content_base_point + offset;
    e.context.text_location = e.context.text_location + offset;
    e.context.block_content_location = e.context.block_content_location + offset;
    e.context.base_point = e.context.base_point + offset;

    for root in &mut e.context.leader_roots {
        root.connection_point = root.connection_point + offset;
        for bp in &mut root.break_points {
            bp.start_point = bp.start_point + offset;
            bp.end_point = bp.end_point + offset;
        }
        for line in &mut root.lines {
            for pt in &mut line.points {
                *pt = *pt + offset;
            }
            for bp in &mut line.break_points {
                bp.start_point = bp.start_point + offset;
                bp.end_point = bp.end_point + offset;
            }
        }
    }
}

// ── MLine ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_mline(e: &mut MLine, offset: Vector3) {
    e.start_point = e.start_point + offset;
    for vertex in &mut e.vertices {
        vertex.position = vertex.position + offset;
    }
}

// ── Mesh ─────────────────────────────────────────────────────────────────────

pub(crate) fn translate_mesh(e: &mut Mesh, offset: Vector3) {
    for vertex in &mut e.vertices {
        *vertex = *vertex + offset;
    }
}

// ── RasterImage ──────────────────────────────────────────────────────────────

pub(crate) fn translate_raster_image(e: &mut RasterImage, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── Solid3D ──────────────────────────────────────────────────────────────────

pub(crate) fn translate_solid3d(e: &mut Solid3D, offset: Vector3) {
    e.point_of_reference = e.point_of_reference + offset;
    // The body geometry lives in the ACIS placement, so move it there too —
    // otherwise the solid keeps rendering at its original location.
    super::transform::compose_acis_placement(
        &mut e.acis_data,
        &Transform::from_translation(offset),
    );

    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = *pt + offset;
        }
    }

    for silhouette in &mut e.silhouettes {
        silhouette.target = silhouette.target + offset;
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = *pt + offset;
            }
        }
    }
}

// ── Region ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_region(e: &mut Region, offset: Vector3) {
    e.point_of_reference = e.point_of_reference + offset;
    super::transform::compose_acis_placement(
        &mut e.acis_data,
        &Transform::from_translation(offset),
    );
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = *pt + offset;
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = silhouette.target + offset;
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = *pt + offset;
            }
        }
    }
}

// ── Body ─────────────────────────────────────────────────────────────────────

pub(crate) fn translate_body(e: &mut Body, offset: Vector3) {
    e.point_of_reference = e.point_of_reference + offset;
    super::transform::compose_acis_placement(
        &mut e.acis_data,
        &Transform::from_translation(offset),
    );
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = *pt + offset;
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = silhouette.target + offset;
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = *pt + offset;
            }
        }
    }
}

pub(crate) fn translate_surface(e: &mut crate::entities::Surface, offset: Vector3) {
    super::transform::transform_loft_placement(e, &Transform::from_translation(offset));
    e.point_of_reference = e.point_of_reference + offset;
    super::transform::compose_acis_placement(
        &mut e.acis_data,
        &Transform::from_translation(offset),
    );
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = *pt + offset;
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = silhouette.target + offset;
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = *pt + offset;
            }
        }
    }
}

// ── Table ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_table(e: &mut Table, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── Tolerance ────────────────────────────────────────────────────────────────

pub(crate) fn translate_tolerance(e: &mut Tolerance, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── PolyfaceMesh ─────────────────────────────────────────────────────────────

pub(crate) fn translate_polyface_mesh(e: &mut PolyfaceMesh, offset: Vector3) {
    for v in &mut e.vertices {
        v.location = v.location + offset;
    }
}

// ── Wipeout ──────────────────────────────────────────────────────────────────

pub(crate) fn translate_wipeout(e: &mut Wipeout, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── Shape ────────────────────────────────────────────────────────────────────

pub(crate) fn translate_shape(e: &mut Shape, offset: Vector3) {
    e.insertion_point = e.insertion_point + ocs_offset(e.normal, offset);
}

// ── Underlay ─────────────────────────────────────────────────────────────────

pub(crate) fn translate_underlay(e: &mut Underlay, offset: Vector3) {
    e.insertion_point = e.insertion_point + offset;
}

// ── Seqend ───────────────────────────────────────────────────────────────────

pub(crate) fn translate_seqend(_e: &mut Seqend, _offset: Vector3) {
    // No geometry
}

// ── Ole2Frame ────────────────────────────────────────────────────────────────

pub(crate) fn translate_ole2frame(e: &mut Ole2Frame, offset: Vector3) {
    e.upper_left_corner = e.upper_left_corner + offset;
    e.lower_right_corner = e.lower_right_corner + offset;
}

// ── PolygonMesh ──────────────────────────────────────────────────────────────

pub(crate) fn translate_polygon_mesh(e: &mut PolygonMeshEntity, offset: Vector3) {
    for v in &mut e.vertices {
        v.location = v.location + offset;
    }
}

// ── UnknownEntity ────────────────────────────────────────────────────────────

pub(crate) fn translate_unknown(_e: &mut UnknownEntity, _offset: Vector3) {
    // No geometry
}

// ── EntityType dispatch ──────────────────────────────────────────────────────

impl EntityType {
    /// Translate this entity by the given offset vector.
    ///
    /// Dispatches to the appropriate per-entity implementation.
    /// This is equivalent to calling `entity.as_entity_mut().translate(offset)`.
    pub fn translate(&mut self, offset: Vector3) {
        match self {
            EntityType::Point(e) => translate_point(e, offset),
            EntityType::Line(e) => translate_line(e, offset),
            EntityType::Circle(e) => translate_circle(e, offset),
            EntityType::Arc(e) => translate_arc(e, offset),
            EntityType::Ellipse(e) => translate_ellipse(e, offset),
            EntityType::Polyline(e) => translate_polyline(e, offset),
            EntityType::Polyline2D(e) => translate_polyline2d(e, offset),
            EntityType::Polyline3D(e) => translate_polyline3d(e, offset),
            EntityType::LwPolyline(e) => translate_lwpolyline(e, offset),
            EntityType::Text(e) => translate_text(e, offset),
            EntityType::MText(e) => translate_mtext(e, offset),
            EntityType::Spline(e) => translate_spline(e, offset),
            EntityType::Helix(e) => translate_helix(e, offset),
            EntityType::Dimension(e) => translate_dimension(e, offset),
            EntityType::Hatch(e) => translate_hatch(e, offset),
            EntityType::Solid(e) => translate_solid(e, offset),
            EntityType::Face3D(e) => translate_face3d(e, offset),
            EntityType::Insert(e) => translate_insert(e, offset),
            EntityType::Block(e) => translate_block(e, offset),
            EntityType::BlockEnd(e) => translate_block_end(e, offset),
            EntityType::Ray(e) => translate_ray(e, offset),
            EntityType::XLine(e) => translate_xline(e, offset),
            EntityType::Viewport(e) => translate_viewport(e, offset),
            EntityType::AttributeDefinition(e) => translate_attribute_definition(e, offset),
            EntityType::AttributeEntity(e) => translate_attribute_entity(e, offset),
            EntityType::Leader(e) => translate_leader(e, offset),
            EntityType::MultiLeader(e) => translate_multileader(e, offset),
            EntityType::MLine(e) => translate_mline(e, offset),
            EntityType::Mesh(e) => translate_mesh(e, offset),
            EntityType::RasterImage(e) => translate_raster_image(e, offset),
            EntityType::Solid3D(e) => translate_solid3d(e, offset),
            EntityType::Region(e) => translate_region(e, offset),
            EntityType::Body(e) => translate_body(e, offset),
            EntityType::Surface(e) => translate_surface(e, offset),
            EntityType::Table(e) => translate_table(e, offset),
            EntityType::Tolerance(e) => translate_tolerance(e, offset),
            EntityType::PolyfaceMesh(e) => translate_polyface_mesh(e, offset),
            EntityType::Wipeout(e) => translate_wipeout(e, offset),
            EntityType::Shape(e) => translate_shape(e, offset),
            EntityType::Underlay(e) => translate_underlay(e, offset),
            EntityType::Seqend(e) => translate_seqend(e, offset),
            EntityType::Ole2Frame(e) => translate_ole2frame(e, offset),
            EntityType::PolygonMesh(e) => translate_polygon_mesh(e, offset),
            EntityType::Light(e) => {
                e.position = Vector3::new(
                    e.position.x + offset.x,
                    e.position.y + offset.y,
                    e.position.z + offset.z,
                );
                e.target = Vector3::new(
                    e.target.x + offset.x,
                    e.target.y + offset.y,
                    e.target.z + offset.z,
                );
            }
            // Section marks / view borders are anchored to their drawing
            // view; the preserved raw record is re-emitted verbatim, so a
            // display-only move would silently revert on save.
            EntityType::SectionSymbol(_) | EntityType::ViewBorder(_) => {}
            EntityType::Extended(e) => e.translate(offset),
            EntityType::Unknown(e) => translate_unknown(e, offset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Vector3;

    #[test]
    fn test_translate_line() {
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        translate_line(&mut line, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(line.start, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(line.end, Vector3::new(15.0, 5.0, 0.0));
    }

    #[test]
    fn test_translate_circle() {
        let mut circle = Circle::new();
        circle.center = Vector3::new(1.0, 2.0, 3.0);
        translate_circle(&mut circle, Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(circle.center, Vector3::new(11.0, 22.0, 33.0));
    }

    #[test]
    fn test_translate_entity_type_dispatch() {
        let mut entity = EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
        ));
        entity.translate(Vector3::new(5.0, 5.0, 0.0));
        if let EntityType::Line(line) = &entity {
            assert_eq!(line.start, Vector3::new(5.0, 5.0, 0.0));
            assert_eq!(line.end, Vector3::new(15.0, 5.0, 0.0));
        } else {
            panic!("Expected Line");
        }
    }

    #[test]
    fn test_translate_solid3d() {
        let mut solid = Solid3D::new();
        solid.point_of_reference = Vector3::new(1.0, 2.0, 3.0);
        translate_solid3d(&mut solid, Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(solid.point_of_reference, Vector3::new(11.0, 2.0, 3.0));
    }

    #[test]
    fn test_translate_insert_moves_attributes() {
        let mut ins = Insert::new("B", Vector3::new(10.0, 10.0, 0.0));
        let mut att = AttributeEntity::new("T".into(), "V".into());
        att.insertion_point = Vector3::new(12.0, 10.0, 0.0);
        att.alignment_point = Vector3::new(12.0, 10.0, 0.0);
        ins.attributes.push(att);

        translate_insert(&mut ins, Vector3::new(5.0, 0.0, 0.0));

        assert_eq!(ins.insert_point, Vector3::new(15.0, 10.0, 0.0));
        // The attribute must follow the block (a MOVE used to leave it behind).
        assert_eq!(
            ins.attributes[0].insertion_point,
            Vector3::new(17.0, 10.0, 0.0)
        );
        assert_eq!(
            ins.attributes[0].alignment_point,
            Vector3::new(17.0, 10.0, 0.0)
        );
    }

    #[test]
    fn test_translate_multileader_moves_text() {
        let mut ml = MultiLeader::default();
        ml.context.text_location = Vector3::new(5.0, 5.0, 0.0);
        ml.context.base_point = Vector3::new(5.0, 5.0, 0.0);
        ml.context.block_content_location = Vector3::new(5.0, 5.0, 0.0);
        ml.context.content_base_point = Vector3::new(5.0, 5.0, 0.0);

        translate_multileader(&mut ml, Vector3::new(3.0, 0.0, 0.0));

        // Every anchor moves, not just the leader line (the label used to stay).
        assert_eq!(ml.context.text_location, Vector3::new(8.0, 5.0, 0.0));
        assert_eq!(ml.context.base_point, Vector3::new(8.0, 5.0, 0.0));
        assert_eq!(
            ml.context.block_content_location,
            Vector3::new(8.0, 5.0, 0.0)
        );
        assert_eq!(ml.context.content_base_point, Vector3::new(8.0, 5.0, 0.0));
    }
}
