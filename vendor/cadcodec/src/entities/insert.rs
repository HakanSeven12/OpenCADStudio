//! Insert entity (block reference)

use crate::entities::{Arc, Ellipse};
use crate::entities::{AttributeEntity, Entity, EntityCommon, EntityType};
use crate::types::{
    BoundingBox3D, Color, Handle, LineWeight, Matrix3, Matrix4, Transform, Transparency, Vector3,
};

/// Minimum absolute value accepted for scale factors (avoids degenerate geometry).
const SCALE_EPSILON: f64 = 1e-12;

/// Insert entity - a reference to a block definition
///
/// An Insert entity places an instance of a block at a specified location
/// with optional scaling, rotation, and array properties.
///
/// When the block contains attribute definitions, the insert holds a
/// collection of [`AttributeEntity`] instances with the concrete values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Insert {
    pub common: EntityCommon,
    /// Block name (references a BlockRecord)
    pub block_name: String,
    /// Insertion point (in WCS)
    pub insert_point: Vector3,
    /// X scale factor (must not be zero)
    x_scale: f64,
    /// Y scale factor (must not be zero)
    y_scale: f64,
    /// Z scale factor (must not be zero)
    z_scale: f64,
    /// Rotation angle in radians
    pub rotation: f64,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Column count (for array inserts / MINSERT)
    pub column_count: u16,
    /// Row count (for array inserts / MINSERT)
    pub row_count: u16,
    /// Column spacing (for array inserts)
    pub column_spacing: f64,
    /// Row spacing (for array inserts)
    pub row_spacing: f64,
    /// Preserve the fixed DWG MINSERT type even for a 1×1 array.
    dwg_minsert: bool,
    /// Attribute entities attached to this insert
    pub attributes: Vec<AttributeEntity>,
    /// Model-documentation viewport referenced by
    /// `AcDbViewRepBlockReference`; `None` for ordinary INSERT entities.
    pub view_rep_handle: Option<Handle>,
    /// SEQEND handle terminating the attribute sequence.
    ///
    /// Older DWG versions require this structural record to remain adjacent
    /// to the INSERT/ATTRIB handle chain. Preserve its source handle when the
    /// insert came from a file; newly created inserts leave it unset.
    pub seqend_handle: Option<Handle>,
}

impl Insert {
    /// Create a new insert entity
    pub fn new(block_name: impl Into<String>, insert_point: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            block_name: block_name.into(),
            insert_point,
            x_scale: 1.0,
            y_scale: 1.0,
            z_scale: 1.0,
            rotation: 0.0,
            normal: Vector3::new(0.0, 0.0, 1.0),
            column_count: 1,
            row_count: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
            dwg_minsert: false,
            attributes: Vec::new(),
            view_rep_handle: None,
            seqend_handle: None,
        }
    }

    // ── Scale accessors with zero-guard ─────────────────────────

    /// Get X scale factor
    pub fn x_scale(&self) -> f64 {
        self.x_scale
    }

    /// Set X scale factor. Rejects zero (uses [`SCALE_EPSILON`] instead).
    pub fn set_x_scale(&mut self, value: f64) {
        self.x_scale = if value.abs() < SCALE_EPSILON {
            SCALE_EPSILON
        } else {
            value
        };
    }

    /// Get Y scale factor
    pub fn y_scale(&self) -> f64 {
        self.y_scale
    }

    /// Set Y scale factor. Rejects zero (uses [`SCALE_EPSILON`] instead).
    pub fn set_y_scale(&mut self, value: f64) {
        self.y_scale = if value.abs() < SCALE_EPSILON {
            SCALE_EPSILON
        } else {
            value
        };
    }

    /// Get Z scale factor
    pub fn z_scale(&self) -> f64 {
        self.z_scale
    }

    /// Set Z scale factor. Rejects zero (uses [`SCALE_EPSILON`] instead).
    pub fn set_z_scale(&mut self, value: f64) {
        self.z_scale = if value.abs() < SCALE_EPSILON {
            SCALE_EPSILON
        } else {
            value
        };
    }

    // ── Builder helpers ─────────────────────────────────────────

    /// Builder: Set the scale factors
    pub fn with_scale(mut self, x: f64, y: f64, z: f64) -> Self {
        self.set_x_scale(x);
        self.set_y_scale(y);
        self.set_z_scale(z);
        self
    }

    /// Builder: Set uniform scale
    pub fn with_uniform_scale(mut self, scale: f64) -> Self {
        self.set_x_scale(scale);
        self.set_y_scale(scale);
        self.set_z_scale(scale);
        self
    }

    /// Builder: Set the rotation angle
    pub fn with_rotation(mut self, angle: f64) -> Self {
        self.rotation = angle;
        self
    }

    /// Builder: Set the normal vector
    pub fn with_normal(mut self, normal: Vector3) -> Self {
        self.normal = normal;
        self
    }

    /// Builder: Set array properties
    pub fn with_array(
        mut self,
        columns: u16,
        rows: u16,
        col_spacing: f64,
        row_spacing: f64,
    ) -> Self {
        self.column_count = columns;
        self.row_count = rows;
        self.column_spacing = col_spacing;
        self.row_spacing = row_spacing;
        self.dwg_minsert = true;
        self
    }

    /// Preserve this block reference as the DWG MINSERT fixed type.
    pub fn mark_as_minsert(&mut self) {
        self.dwg_minsert = true;
    }

    // ── Queries ─────────────────────────────────────────────────

    /// True when the insert has attribute entities attached.
    ///
    /// Corresponds to DXF group code 66.
    pub fn has_attributes(&self) -> bool {
        !self.attributes.is_empty()
    }

    /// Check if this is an array insert (MINSERT)
    pub fn is_array(&self) -> bool {
        self.column_count > 1 || self.row_count > 1
    }

    /// True when DXF object type is MINSERT (array insert).
    pub fn is_minsert(&self) -> bool {
        self.dwg_minsert || self.is_array()
    }

    /// Get the total number of instances in the array
    pub fn instance_count(&self) -> usize {
        (self.column_count as usize) * (self.row_count as usize)
    }

    /// Get all insertion points for array instances
    pub fn array_points(&self) -> Vec<Vector3> {
        let mut points = Vec::with_capacity(self.instance_count());

        for row in 0..self.row_count {
            for col in 0..self.column_count {
                let offset_x = col as f64 * self.column_spacing;
                let offset_y = row as f64 * self.row_spacing;

                // Apply rotation to the offset
                let cos_r = self.rotation.cos();
                let sin_r = self.rotation.sin();
                let rotated_x = offset_x * cos_r - offset_y * sin_r;
                let rotated_y = offset_x * sin_r + offset_y * cos_r;

                let point = self.insert_point + Vector3::new(rotated_x, rotated_y, 0.0);
                points.push(point);
            }
        }

        points
    }

    /// Check if the insert has uniform scale
    pub fn has_uniform_scale(&self) -> bool {
        (self.x_scale - self.y_scale).abs() < 1e-10 && (self.y_scale - self.z_scale).abs() < 1e-10
    }

    /// Get the uniform scale factor (if uniform)
    pub fn uniform_scale(&self) -> Option<f64> {
        if self.has_uniform_scale() {
            Some(self.x_scale)
        } else {
            None
        }
    }

    /// Return the DXF subclass marker name.
    ///
    /// `AcDbBlockReference` for normal inserts, or a conceptual "MINSERT"
    /// distinction when array counts exceed 1.
    pub fn subclass_marker(&self) -> &'static str {
        if self.is_minsert() {
            "AcDbMInsertBlock"
        } else {
            "AcDbBlockReference"
        }
    }

    // ── Transform helpers ─────────────

    /// Build the full OCS → WCS transform that positions the block
    /// contents into world space.
    ///
    /// Order: **Scale → Rotate(Z) → ArbitraryAxis(Normal) → Translate**
    pub fn get_transform(&self) -> Transform {
        let ocs = Matrix4::from_matrix3(Matrix3::arbitrary_axis(self.normal));
        let translation = Matrix4::translation(
            self.insert_point.x,
            self.insert_point.y,
            self.insert_point.z,
        );
        let rotation = Matrix4::rotation_z(self.rotation);
        let scale = Matrix4::scaling(self.x_scale, self.y_scale, self.z_scale);

        // Combined: world = OCS * translate * rotate * scale
        Transform::from_matrix(ocs * translation * rotation * scale)
    }

    /// Transform a normal vector correctly for non-uniform scaling.
    ///
    /// Uses the inverse-transpose of the 3×3 rotation/scale portion of
    /// the transform, which is the geometrically correct approach for
    /// normals under non-uniform scale. Falls back to the original
    /// normal if the matrix is singular.
    fn transform_normal(transform: &Transform, normal: Vector3) -> Vector3 {
        let m4 = transform.matrix;
        let upper3x3 = Matrix3::from_rows(
            [m4.m[0][0], m4.m[0][1], m4.m[0][2]],
            [m4.m[1][0], m4.m[1][1], m4.m[1][2]],
            [m4.m[2][0], m4.m[2][1], m4.m[2][2]],
        );
        // Inverse-transpose is the correct normal transform
        if let Some(inv) = upper3x3.inverse() {
            let inv_t = inv.transpose();
            let transformed = inv_t.transform_point(normal);
            let len = transformed.length();
            if len < 1e-10 {
                normal
            } else {
                transformed * (1.0 / len)
            }
        } else {
            normal // fallback for singular matrix
        }
    }

    /// Resolve ByBlock / layer-"0" property inheritance for an exploded entity.
    ///
    /// AutoCAD convention:
    /// - Entities on layer `"0"` in the block inherit the Insert's layer.
    /// - `Color::ByBlock` inherits the Insert's color.
    /// - `LineWeight::ByBlock` inherits the Insert's line weight.
    /// - Empty/ByLayer linetype with ByBlock semantic: not changed (already
    ///   resolved to ByLayer at the entity level).
    fn resolve_properties(&self, common: &mut EntityCommon) {
        // Layer "0" → inherit insert's layer
        if common.layer == "0" {
            common.layer = self.common.layer.clone();
        }
        // Color ByBlock → inherit insert's color
        if common.color == Color::ByBlock {
            common.color = self.common.color;
        }
        // LineWeight ByBlock → inherit insert's line weight
        if common.line_weight == LineWeight::ByBlock {
            common.line_weight = self.common.line_weight;
        }
    }

    // ── Explode ─────────────────────────────────────────────────

    /// Explode the block reference into individual entities.
    ///
    /// Returns clones (or converted equivalents) of the entities stored in
    /// the referenced block, with the insert's transform applied so they
    /// are positioned in world space.
    ///
    /// `block_entities` should be the `entities` vec from the corresponding
    /// [`BlockRecord`](crate::tables::BlockRecord).  Block/BlockEnd markers
    /// and attribute definitions are skipped.
    ///
    /// **Property inheritance** (AutoCAD convention):
    /// - Entities on layer `"0"` → inherit the Insert's layer.
    /// - `Color::ByBlock` → inherit the Insert's color.
    /// - `LineWeight::ByBlock` → inherit the Insert's line weight.
    ///
    /// **MINSERT arrays**: When `column_count > 1` or `row_count > 1`,
    /// a full copy of every entity is produced at each array position.
    ///
    /// **Entity-specific handling**:
    /// * **Arc** — with uniform XY scale the arc is reconstructed from
    ///   transformed vertices.  With non-uniform XY scale the arc is
    ///   converted to an [`Ellipse`] (elliptical arc).
    /// * **Circle** — converted to an [`Ellipse`] with `ratio = 1.0`
    ///   so that non-uniform scales produce a correct ellipse.
    /// * All other entities are cloned and
    ///   [`apply_transform`](Entity::apply_transform) is called.
    pub fn explode(&self, block_entities: &[EntityType]) -> Vec<EntityType> {
        let transforms = self.array_transforms();
        let mut result = Vec::new();

        for transform in &transforms {
            for entity in block_entities {
                if let Some(exploded) = self.explode_single(entity, transform) {
                    result.push(exploded);
                }
            }
        }

        result
    }

    /// Build a transform for each array cell (single transform when not MINSERT).
    fn array_transforms(&self) -> Vec<Transform> {
        if !self.is_minsert() {
            return vec![self.get_transform()];
        }

        let ocs = Matrix4::from_matrix3(Matrix3::arbitrary_axis(self.normal));
        let rotation = Matrix4::rotation_z(self.rotation);
        let scale = Matrix4::scaling(self.x_scale, self.y_scale, self.z_scale);

        let mut transforms = Vec::with_capacity(self.instance_count());
        for row in 0..self.row_count {
            for col in 0..self.column_count {
                let offset_x = col as f64 * self.column_spacing;
                let offset_y = row as f64 * self.row_spacing;
                let cell_pt = Vector3::new(
                    self.insert_point.x + offset_x,
                    self.insert_point.y + offset_y,
                    self.insert_point.z,
                );
                let translation = Matrix4::translation(cell_pt.x, cell_pt.y, cell_pt.z);
                transforms.push(Transform::from_matrix(ocs * translation * rotation * scale));
            }
        }
        transforms
    }

    /// Explode a single entity with the given transform, returning `None`
    /// for structural entities that should be skipped.
    fn explode_single(&self, entity: &EntityType, transform: &Transform) -> Option<EntityType> {
        match entity {
            // Skip structural / meta entities
            EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::AttributeDefinition(_) => {
                None
            }

            // Arc handling — uniform XY keeps Arc, non-uniform → Ellipse
            EntityType::Arc(arc) => {
                let sx = self.x_scale.abs();
                let sy = self.y_scale.abs();
                let is_uniform_xy = (sx - sy).abs() < 1e-10;

                if is_uniform_xy {
                    Some(self.explode_arc_uniform(arc, transform))
                } else {
                    Some(self.explode_arc_to_ellipse(arc, transform))
                }
            }

            // Circle → Ellipse so non-uniform scale works
            EntityType::Circle(circle) => {
                // CIRCLE stores `center` in OCS; ELLIPSE is a WCS entity —
                // convert on the way in (identity for the Z-up normal).
                let ocs_to_wcs = Matrix3::arbitrary_axis(circle.normal);
                let mut ellipse = Ellipse {
                    common: circle.common.clone(),
                    center: ocs_to_wcs * circle.center,
                    major_axis: ocs_to_wcs * (Vector3::UNIT_X * circle.radius),
                    minor_axis_ratio: 1.0,
                    start_parameter: 0.0,
                    end_parameter: std::f64::consts::TAU,
                    normal: circle.normal,
                };
                Self::apply_full_ellipse_transform(&mut ellipse, transform);
                self.resolve_properties(&mut ellipse.common);
                Some(EntityType::Ellipse(ellipse))
            }

            // Default path — clone + transform
            _ => {
                let mut cloned = entity.clone();
                cloned.as_entity_mut().apply_transform(transform);
                self.resolve_properties(cloned.common_mut());
                Some(cloned)
            }
        }
    }

    /// Explode an arc with uniform XY scale — keeps it as an Arc.
    fn explode_arc_uniform(&self, arc: &Arc, transform: &Transform) -> EntityType {
        // DXF arcs store `center` in OCS (arbitrary-axis algorithm with the
        // arc's normal). Convert everything to WCS, apply the INSERT transform
        // in WCS, then project back into the new arc's OCS so the renderer
        // (which always treats `center` as OCS) reproduces the correct WCS
        // position regardless of normal direction.
        let ocs_to_wcs = Matrix3::arbitrary_axis(arc.normal);
        let center_wcs = ocs_to_wcs * arc.center;
        let wcs_start = center_wcs
            + ocs_to_wcs
                * Vector3::new(
                    arc.radius * arc.start_angle.cos(),
                    arc.radius * arc.start_angle.sin(),
                    0.0,
                );
        let wcs_end = center_wcs
            + ocs_to_wcs
                * Vector3::new(
                    arc.radius * arc.end_angle.cos(),
                    arc.radius * arc.end_angle.sin(),
                    0.0,
                );

        let new_center_wcs = transform.apply(center_wcs);
        let new_start_wcs = transform.apply(wcs_start);
        let new_end_wcs = transform.apply(wcs_end);

        let new_radius = (new_start_wcs - new_center_wcs).length();
        let mut new_normal = Self::transform_normal(transform, arc.normal);

        // Handedness correction: a transform with negative upper-3×3 determinant
        // (e.g. odd-count mirror in the INSERT scale) flips the OCS plane's
        // orientation. Arcs are always CCW around their normal, so to preserve
        // the visual sweep we flip the normal — the new OCS basis is then the
        // mirror of the old one and CCW around the flipped normal traces the
        // mirrored geometry.
        if Self::upper3x3_determinant(transform) < 0.0 {
            new_normal = new_normal * -1.0;
        }

        // Convert transformed WCS offsets back to OCS angles & center for the
        // new normal so to_truck()'s arc_pt (which applies the new OCS) gives
        // correct WCS points.
        let new_ocs_to_wcs = Matrix3::arbitrary_axis(new_normal);
        let new_wcs_to_ocs = new_ocs_to_wcs.transpose();
        let new_center_ocs = new_wcs_to_ocs * new_center_wcs;
        let ds_ocs = new_wcs_to_ocs * (new_start_wcs - new_center_wcs);
        let de_ocs = new_wcs_to_ocs * (new_end_wcs - new_center_wcs);
        let new_start_angle = ds_ocs.y.atan2(ds_ocs.x);
        let new_end_angle = de_ocs.y.atan2(de_ocs.x);

        let mut new_arc = Arc::from_center_radius_angles(
            new_center_ocs,
            new_radius,
            new_start_angle,
            new_end_angle,
        );
        new_arc.normal = new_normal;
        new_arc.common = arc.common.clone();
        self.resolve_properties(&mut new_arc.common);
        EntityType::Arc(new_arc)
    }

    /// Determinant of the upper-3×3 portion of a transform's matrix.
    /// Negative ⇒ the transform reverses handedness (a reflection).
    fn upper3x3_determinant(transform: &Transform) -> f64 {
        let m = transform.matrix.m;
        Matrix3::from_rows(
            [m[0][0], m[0][1], m[0][2]],
            [m[1][0], m[1][1], m[1][2]],
            [m[2][0], m[2][1], m[2][2]],
        )
        .determinant()
    }

    /// Explode an arc with non-uniform XY scale — converts to an elliptical arc (Ellipse).
    fn explode_arc_to_ellipse(&self, arc: &Arc, transform: &Transform) -> EntityType {
        // ARC stores `center` in OCS; ELLIPSE is a WCS entity. Convert on the
        // way in (identity for the common Z-up normal). The arc's angles are
        // measured against the OCS X axis, which becomes the ellipse's major
        // axis below — so the parameters carry over unchanged.
        let ocs_to_wcs = Matrix3::arbitrary_axis(arc.normal);
        let mut ellipse = Ellipse {
            common: arc.common.clone(),
            center: ocs_to_wcs * arc.center,
            major_axis: ocs_to_wcs * (Vector3::UNIT_X * arc.radius),
            minor_axis_ratio: 1.0,
            start_parameter: arc.start_angle,
            end_parameter: arc.end_angle,
            normal: arc.normal,
        };
        Self::apply_full_ellipse_transform(&mut ellipse, transform);
        self.resolve_properties(&mut ellipse.common);
        EntityType::Ellipse(ellipse)
    }

    /// Apply transform to an Ellipse with correct minor_axis_ratio recalculation.
    ///
    /// Unlike the default Ellipse::apply_transform (which leaves minor_axis_ratio
    /// unchanged), this properly computes the new ratio by transforming both
    /// major and minor axis directions independently. The new normal is derived
    /// from `new_major × new_minor`, which automatically encodes any handedness
    /// flip introduced by a reflective transform (det < 0), preserving the
    /// original sweep direction.
    ///
    /// ELLIPSE is one of the few WCS entities in DXF: `center` (code 10) and
    /// `major_axis` (code 11) are world coordinates, NOT OCS — so the transform
    /// applies to the stored values directly and the results are stored back
    /// as-is. (An earlier revision round-tripped them through the
    /// arbitrary-axis OCS, which made files with a non-Z-up result — e.g. a
    /// mirrored explode — read wrong in other CAD applications.)
    fn apply_full_ellipse_transform(ellipse: &mut Ellipse, transform: &Transform) {
        let center_wcs = ellipse.center;
        let major_wcs = ellipse.major_axis;

        // Original minor axis vector (WCS, perpendicular to major in the ellipse plane).
        let original_minor_dir = ellipse.normal.cross(&major_wcs).normalize();
        let original_minor_len = major_wcs.length() * ellipse.minor_axis_ratio;
        let original_minor = original_minor_dir * original_minor_len;

        // Transform center in WCS
        let new_center_wcs = transform.apply(center_wcs);

        // Transform both axes through the 3×3 portion (direction + scale, no translation)
        let new_major_wcs = transform.apply_rotation(major_wcs);
        let new_minor_wcs = transform.apply_rotation(original_minor);

        let new_major_len = new_major_wcs.length();
        let new_minor_len = new_minor_wcs.length();

        // DXF convention: major_axis must be the longer axis.
        // If the minor became longer, swap them.
        let (final_major_wcs, final_minor_wcs, swapped) = if new_minor_len > new_major_len + 1e-12 {
            (new_minor_wcs, new_major_wcs, true)
        } else {
            (new_major_wcs, new_minor_wcs, false)
        };

        // Derive normal from major × minor so handedness follows the geometry:
        // a reflective transform automatically produces a flipped normal, and the
        // CCW parameter sweep (around the new normal) traces the mirrored shape.
        let cross = final_major_wcs.cross(&final_minor_wcs);
        let cross_len = cross.length();
        let new_normal = if cross_len > 1e-12 {
            cross * (1.0 / cross_len)
        } else {
            Self::transform_normal(transform, ellipse.normal)
        };

        ellipse.center = new_center_wcs;
        ellipse.major_axis = final_major_wcs;
        ellipse.minor_axis_ratio = if final_major_wcs.length() > 1e-12 {
            final_minor_wcs.length() / final_major_wcs.length()
        } else {
            1.0
        };
        ellipse.normal = new_normal;

        if swapped {
            // When axes swap, parameters need to shift by π/2
            ellipse.start_parameter -= std::f64::consts::FRAC_PI_2;
            ellipse.end_parameter -= std::f64::consts::FRAC_PI_2;
        }
    }

    /// Convenience wrapper that looks up the block in a
    /// [`CadDocument`](crate::document::CadDocument) by `block_name` and
    /// calls [`explode`](Self::explode).
    ///
    /// Returns an empty `Vec` when the block record is not found.
    pub fn explode_from_document(
        &self,
        document: &crate::document::CadDocument,
    ) -> Vec<EntityType> {
        match document.block_records.get(&self.block_name) {
            Some(br) => {
                let entities: Vec<EntityType> = br
                    .entity_handles
                    .iter()
                    .filter_map(|h| {
                        document
                            .entity_index
                            .get(h)
                            .map(|&idx| document.entities[idx].as_ref().clone())
                    })
                    .collect();
                self.explode(&entities)
            }
            None => Vec::new(),
        }
    }
}

impl Entity for Insert {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, weight: LineWeight) {
        self.common.line_weight = weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        // For now, return a bounding box at the insertion point
        // In a full implementation, this would need to reference the block definition
        BoundingBox3D::from_point(self.insert_point)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_insert(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        if self.is_minsert() {
            "MINSERT"
        } else {
            "INSERT"
        }
    }

    fn apply_transform(&mut self, transform: &Transform) {
        super::transform::transform_insert(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{AttributeDefinition, Block, BlockEnd, Circle, Line};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    /// Helper – approximate equality for f64
    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Helper – approximate equality for Vector3
    fn approx_vec(a: Vector3, b: Vector3) -> bool {
        approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
    }

    // ── basic explode ───────────────────────────────────────────

    #[test]
    fn explode_empty_block_returns_empty() {
        let insert = Insert::new("TestBlock", Vector3::ZERO);
        let result = insert.explode(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn explode_skips_block_markers_and_attdefs() {
        let block_entities = vec![
            EntityType::Block(Block::new("TestBlock", Vector3::ZERO)),
            EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )),
            EntityType::AttributeDefinition(AttributeDefinition::new(
                "TAG".into(),
                "prompt".into(),
                "default".into(),
            )),
            EntityType::BlockEnd(BlockEnd::new()),
        ];

        let insert = Insert::new("TestBlock", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        // Only the Line should survive
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], EntityType::Line(_)));
    }

    // ── identity insert ─────────────────────────────────────────

    #[test]
    fn explode_identity_insert_preserves_line() {
        let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 1);
        if let EntityType::Line(l) = &result[0] {
            assert!(approx_vec(l.start, Vector3::new(0.0, 0.0, 0.0)));
            assert!(approx_vec(l.end, Vector3::new(10.0, 0.0, 0.0)));
        } else {
            panic!("expected Line");
        }
    }

    // ── translation ─────────────────────────────────────────────

    #[test]
    fn explode_with_translation() {
        let line = Line::from_points(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::new(10.0, 20.0, 0.0));
        let result = insert.explode(&block_entities);

        if let EntityType::Line(l) = &result[0] {
            assert!(approx_vec(l.start, Vector3::new(10.0, 20.0, 0.0)));
            assert!(approx_vec(l.end, Vector3::new(15.0, 20.0, 0.0)));
        } else {
            panic!("expected Line");
        }
    }

    // ── uniform scale ───────────────────────────────────────────

    #[test]
    fn explode_with_uniform_scale() {
        let line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::ZERO).with_uniform_scale(3.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Line(l) = &result[0] {
            assert!(approx_vec(l.end, Vector3::new(3.0, 0.0, 0.0)));
        } else {
            panic!("expected Line");
        }
    }

    // ── rotation ────────────────────────────────────────────────

    #[test]
    fn explode_with_90_degree_rotation() {
        let line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::ZERO).with_rotation(FRAC_PI_2);
        let result = insert.explode(&block_entities);

        if let EntityType::Line(l) = &result[0] {
            assert!(approx_vec(l.end, Vector3::new(0.0, 1.0, 0.0)));
        } else {
            panic!("expected Line");
        }
    }

    // ── combined scale + translation ────────────────────────────

    #[test]
    fn explode_with_scale_and_translation() {
        let line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 1.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::new(5.0, 5.0, 0.0)).with_scale(2.0, 3.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Line(l) = &result[0] {
            assert!(approx_vec(l.start, Vector3::new(5.0, 5.0, 0.0)));
            assert!(approx_vec(l.end, Vector3::new(7.0, 8.0, 0.0)));
        } else {
            panic!("expected Line");
        }
    }

    // ── Circle → Ellipse conversion ─────────────────────────────

    #[test]
    fn explode_circle_becomes_ellipse() {
        let circle = Circle::from_center_radius(Vector3::ZERO, 5.0);
        let block_entities = vec![EntityType::Circle(circle)];

        let insert = Insert::new("B", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], EntityType::Ellipse(_)));

        if let EntityType::Ellipse(e) = &result[0] {
            assert!(approx(e.start_parameter, 0.0));
            assert!(approx(e.end_parameter, TAU));
        } else {
            panic!("expected Ellipse");
        }
    }

    #[test]
    fn explode_circle_with_non_uniform_scale() {
        let circle = Circle::from_center_radius(Vector3::ZERO, 1.0);
        let block_entities = vec![EntityType::Circle(circle)];

        // Scale X by 2, Y by 1 → ellipse with major=2, minor=1
        let insert = Insert::new("B", Vector3::ZERO).with_scale(2.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Ellipse(e) = &result[0] {
            let major_len = e.major_axis.length();
            let minor_len = major_len * e.minor_axis_ratio;
            assert!(approx(major_len, 2.0));
            assert!(approx(minor_len, 1.0));
            assert!(approx(e.minor_axis_ratio, 0.5));
        } else {
            panic!("expected Ellipse");
        }
    }

    #[test]
    fn explode_circle_non_uniform_scale_minor_becomes_major() {
        let circle = Circle::from_center_radius(Vector3::ZERO, 1.0);
        let block_entities = vec![EntityType::Circle(circle)];

        // Scale X by 1, Y by 3 → original minor direction (Y) becomes major
        let insert = Insert::new("B", Vector3::ZERO).with_scale(1.0, 3.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Ellipse(e) = &result[0] {
            let major_len = e.major_axis.length();
            let minor_len = major_len * e.minor_axis_ratio;
            // Major should be 3 (the larger), minor should be 1
            assert!(approx(major_len, 3.0));
            assert!(approx(minor_len, 1.0));
        } else {
            panic!("expected Ellipse");
        }
    }

    // ── Arc reconstruction ──────────────────────────────────────

    #[test]
    fn explode_arc_identity() {
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 5.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc.clone())];

        let insert = Insert::new("B", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 1);
        if let EntityType::Arc(a) = &result[0] {
            assert!(approx(a.radius, 5.0));
            assert!(approx(a.start_angle, 0.0));
            assert!(approx(a.end_angle, FRAC_PI_2));
            assert!(approx_vec(a.center, Vector3::ZERO));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_with_translation() {
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 10.0, 0.0, PI);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::new(100.0, 200.0, 0.0));
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            assert!(approx_vec(a.center, Vector3::new(100.0, 200.0, 0.0)));
            assert!(approx(a.radius, 10.0));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_with_uniform_scale() {
        let arc = Arc::from_center_radius_angles(Vector3::new(1.0, 1.0, 0.0), 2.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::ZERO).with_uniform_scale(3.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            assert!(approx(a.radius, 6.0)); // 2 * 3
            assert!(approx_vec(a.center, Vector3::new(3.0, 3.0, 0.0)));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_non_uniform_becomes_ellipse() {
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 5.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        // Non-uniform scale → arc must become an elliptical arc
        let insert = Insert::new("B", Vector3::ZERO).with_scale(2.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 1);
        if let EntityType::Ellipse(e) = &result[0] {
            // Major axis should be 10 (5*2), minor 5 (5*1)
            let major_len = e.major_axis.length();
            let minor_len = major_len * e.minor_axis_ratio;
            assert!(approx(major_len, 10.0));
            assert!(approx(minor_len, 5.0));
            // It's a partial ellipse (not full)
            assert!(!e.is_full());
        } else {
            panic!("expected Ellipse for non-uniform arc");
        }
    }

    // ── multiple entities ───────────────────────────────────────

    #[test]
    fn explode_multiple_entities() {
        let block_entities = vec![
            EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )),
            EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, 1.0)),
            EntityType::Arc(Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, PI)),
        ];

        let insert = Insert::new("B", Vector3::new(10.0, 0.0, 0.0));
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], EntityType::Line(_)));
        assert!(matches!(result[1], EntityType::Ellipse(_))); // circle → ellipse
        assert!(matches!(result[2], EntityType::Arc(_))); // uniform scale → stays Arc
    }

    // ── Layer "0" inheritance ───────────────────────────────────

    #[test]
    fn explode_layer_zero_inherits_insert_layer() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = "0".to_string(); // layer "0" in block
        let block_entities = vec![EntityType::Line(line)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.layer = "Walls".to_string();
        let result = insert.explode(&block_entities);

        assert_eq!(result[0].common().layer, "Walls");
    }

    #[test]
    fn explode_named_layer_preserved() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = "Hidden".to_string();
        let block_entities = vec![EntityType::Line(line)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.layer = "Walls".to_string();
        let result = insert.explode(&block_entities);

        // Named layer stays — NOT replaced by insert's layer
        assert_eq!(result[0].common().layer, "Hidden");
    }

    // ── ByBlock property resolution ─────────────────────────────

    #[test]
    fn explode_byblock_color_inherits_insert_color() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.color = Color::ByBlock;
        let block_entities = vec![EntityType::Line(line)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.color = Color::from_index(1); // Red
        let result = insert.explode(&block_entities);

        assert_eq!(result[0].common().color, Color::from_index(1));
    }

    #[test]
    fn explode_byblock_lineweight_inherits_insert_lineweight() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.line_weight = LineWeight::ByBlock;
        let block_entities = vec![EntityType::Line(line)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.line_weight = LineWeight::from_value(50); // 0.50mm
        let result = insert.explode(&block_entities);

        assert_eq!(result[0].common().line_weight, LineWeight::from_value(50));
    }

    #[test]
    fn explode_non_byblock_color_preserved() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.color = Color::from_index(3); // Green
        let block_entities = vec![EntityType::Line(line)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.color = Color::from_index(1); // Red
        let result = insert.explode(&block_entities);

        // Green stays — NOT replaced
        assert_eq!(result[0].common().color, Color::from_index(3));
    }

    // ── MINSERT array ───────────────────────────────────────────

    #[test]
    fn explode_minsert_produces_array_copies() {
        let line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        let block_entities = vec![EntityType::Line(line)];

        // 2 columns × 3 rows = 6 copies
        let insert = Insert::new("B", Vector3::new(0.0, 0.0, 0.0)).with_array(2, 3, 10.0, 20.0);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 6);
        // All should be Lines
        for e in &result {
            assert!(matches!(e, EntityType::Line(_)));
        }
    }

    #[test]
    fn explode_minsert_positions_correct() {
        let line = Line::from_points(Vector3::ZERO, Vector3::ZERO); // zero-length line at origin
        let block_entities = vec![EntityType::Line(line)];

        // 2 columns, 1 row, spacing 10
        let insert = Insert::new("B", Vector3::new(5.0, 0.0, 0.0)).with_array(2, 1, 10.0, 0.0);
        let result = insert.explode(&block_entities);

        assert_eq!(result.len(), 2);
        if let EntityType::Line(l0) = &result[0] {
            assert!(approx_vec(l0.start, Vector3::new(5.0, 0.0, 0.0)));
        }
        if let EntityType::Line(l1) = &result[1] {
            // Second column: insert_point.x + column_spacing = 5 + 10 = 15
            assert!(approx_vec(l1.start, Vector3::new(15.0, 0.0, 0.0)));
        }
    }

    // ── Arc + Circle property inheritance ───────────────────────

    #[test]
    fn explode_arc_layer_zero_inherits() {
        let mut arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, PI);
        arc.common.layer = "0".to_string();
        let block_entities = vec![EntityType::Arc(arc)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.layer = "Pipes".to_string();
        let result = insert.explode(&block_entities);

        assert_eq!(result[0].common().layer, "Pipes");
    }

    #[test]
    fn explode_circle_byblock_color_inherits() {
        let mut circle = Circle::from_center_radius(Vector3::ZERO, 1.0);
        circle.common.color = Color::ByBlock;
        let block_entities = vec![EntityType::Circle(circle)];

        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.common.color = Color::from_index(5); // Blue
        let result = insert.explode(&block_entities);

        if let EntityType::Ellipse(e) = &result[0] {
            assert_eq!(e.common.color, Color::from_index(5));
        } else {
            panic!("expected Ellipse");
        }
    }

    #[test]
    fn explode_preserves_entity_layer() {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = "MyLayer".to_string();
        let block_entities = vec![EntityType::Line(line)];

        let insert = Insert::new("B", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        assert_eq!(result[0].common().layer, "MyLayer");
    }

    #[test]
    fn explode_arc_preserves_layer() {
        let mut arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, PI);
        arc.common.layer = "ArcLayer".to_string();
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::ZERO);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            assert_eq!(a.common.layer, "ArcLayer");
        } else {
            panic!("expected Arc");
        }
    }

    // ── Mirrored INSERT arc handedness ──────────────────────────
    //
    // The visual sweep direction of an arc inside a mirrored block must match
    // the mirror of the original sweep. acadrust encodes this by emitting a
    // flipped normal so that the CCW (around-normal) parameterization traces
    // the mirrored geometry.

    /// Reconstruct an arc's three sample points (start, midpoint, end) in WCS,
    /// using the renderer's OCS-axis convention. `arc.center` is interpreted as
    /// OCS coordinates (per DXF arbitrary-axis algorithm).
    fn arc_sample_points(arc: &Arc) -> (Vector3, Vector3, Vector3) {
        let basis = Matrix3::arbitrary_axis(arc.normal);
        let center_wcs = basis * arc.center;
        let ccw_end = if arc.end_angle >= arc.start_angle {
            arc.end_angle
        } else {
            arc.end_angle + TAU
        };
        let mid = arc.start_angle + (ccw_end - arc.start_angle) * 0.5;
        let pt = |a: f64| {
            center_wcs + basis * Vector3::new(arc.radius * a.cos(), arc.radius * a.sin(), 0.0)
        };
        (pt(arc.start_angle), pt(mid), pt(arc.end_angle))
    }

    #[test]
    fn explode_arc_mirror_x_preserves_sweep_direction() {
        // Original arc traces Q1 from (r,0) through (~0.707r, ~0.707r) to (0,r).
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        // Mirror-X (x_scale = -1) should produce a CCW arc through Q2.
        let insert = Insert::new("B", Vector3::ZERO).with_scale(-1.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            let (start, mid, end) = arc_sample_points(a);
            // Endpoints land at the mirrored positions.
            assert!(approx_vec(start, Vector3::new(-1.0, 0.0, 0.0)));
            assert!(approx_vec(end, Vector3::new(0.0, 1.0, 0.0)));
            // Crucially, the midpoint is in Q2 (mirror of the original Q1 midpoint),
            // not Q4 (which is what you would see if the sweep went the wrong way).
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(mid, Vector3::new(-inv_sqrt2, inv_sqrt2, 0.0)));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_mirror_y_preserves_sweep_direction() {
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        // Mirror-Y (y_scale = -1) → mirrored arc should trace Q4.
        let insert = Insert::new("B", Vector3::ZERO).with_scale(1.0, -1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            let (start, mid, end) = arc_sample_points(a);
            assert!(approx_vec(start, Vector3::new(1.0, 0.0, 0.0)));
            assert!(approx_vec(end, Vector3::new(0.0, -1.0, 0.0)));
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(mid, Vector3::new(inv_sqrt2, -inv_sqrt2, 0.0)));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_double_mirror_is_rotation() {
        // x_scale = -1, y_scale = -1 has det = +1 (a 180° rotation), so the
        // normal must NOT be flipped and the arc should land in Q3.
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::ZERO).with_scale(-1.0, -1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            // Normal stays (0,0,1).
            assert!(approx_vec(a.normal, Vector3::new(0.0, 0.0, 1.0)));
            let (start, mid, end) = arc_sample_points(a);
            assert!(approx_vec(start, Vector3::new(-1.0, 0.0, 0.0)));
            assert!(approx_vec(end, Vector3::new(0.0, -1.0, 0.0)));
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(mid, Vector3::new(-inv_sqrt2, -inv_sqrt2, 0.0)));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_mirror_x_offset_center_position_preserved() {
        // Regression: when the normal flips, `arc.center` (stored in OCS) must
        // be projected into the new OCS so the renderer reconstructs the WCS
        // position correctly. Earlier the center was stored as WCS, which made
        // the renderer apply the new OCS basis a second time and flip the X
        // back — placing the arc at the mirror of where it should be.
        let arc = Arc::from_center_radius_angles(Vector3::new(5.0, 3.0, 0.0), 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::ZERO).with_scale(-1.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            // Original arc centered at (5,3,0) sweeps Q1-of-center: start (6,3) → end (5,4).
            // Mirror-X: centered at (-5,3,0), start (-6,3) → end (-5,4), midpoint upper-left.
            assert!(approx_vec(a.normal, Vector3::new(0.0, 0.0, -1.0)));
            let (start, mid, end) = arc_sample_points(a);
            assert!(approx_vec(start, Vector3::new(-6.0, 3.0, 0.0)));
            assert!(approx_vec(end, Vector3::new(-5.0, 4.0, 0.0)));
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(
                mid,
                Vector3::new(-5.0 - inv_sqrt2, 3.0 + inv_sqrt2, 0.0)
            ));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_mirror_x_with_translation_position_preserved() {
        // Same regression but with both mirror and INSERT translation —
        // catches any leftover confusion between WCS/OCS center.
        let arc = Arc::from_center_radius_angles(Vector3::new(2.0, 0.0, 0.0), 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::new(10.0, 20.0, 0.0)).with_scale(-1.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            let (start, mid, end) = arc_sample_points(a);
            // The block arc at (2,0,0) gets mirrored to (-2,0,0) then translated to (8,20,0).
            // Sweep: start (3,0)→mirror→(−3,0)→translate→(7,20).
            //        end   (2,1)→mirror→(−2,1)→translate→(8,21).
            assert!(approx_vec(start, Vector3::new(7.0, 20.0, 0.0)));
            assert!(approx_vec(end, Vector3::new(8.0, 21.0, 0.0)));
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(
                mid,
                Vector3::new(8.0 - inv_sqrt2, 20.0 + inv_sqrt2, 0.0)
            ));
        } else {
            panic!("expected Arc");
        }
    }

    #[test]
    fn explode_arc_mirror_flips_normal() {
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        let insert = Insert::new("B", Vector3::ZERO).with_scale(-1.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Arc(a) = &result[0] {
            // A single-axis mirror should flip the arc normal so the renderer's
            // CCW sweep matches the mirrored geometry.
            assert!(approx_vec(a.normal, Vector3::new(0.0, 0.0, -1.0)));
        } else {
            panic!("expected Arc");
        }
    }

    // ── Mirrored INSERT ellipse handedness ──────────────────────

    /// Reconstruct an ellipse's three sample points at t = start, mid, end in WCS.
    /// ELLIPSE is a WCS entity: `center` and `major_axis` are world coordinates.
    fn ellipse_sample_points(e: &Ellipse) -> (Vector3, Vector3, Vector3) {
        let center_wcs = e.center;
        let major_wcs = e.major_axis;
        let major_len = major_wcs.length();
        let u = major_wcs * (1.0 / major_len.max(1e-12));
        let v = e.normal.cross(&u);
        let minor_len = major_len * e.minor_axis_ratio;
        let mut t1 = e.end_parameter;
        if t1 <= e.start_parameter {
            t1 += TAU;
        }
        let pt = |t: f64| center_wcs + u * (major_len * t.cos()) + v * (minor_len * t.sin());
        let mid = e.start_parameter + (t1 - e.start_parameter) * 0.5;
        (pt(e.start_parameter), pt(mid), pt(t1))
    }

    #[test]
    fn explode_arc_mirror_x_nonuniform_to_ellipse() {
        // Non-uniform XY scale with mirror: arc → ellipse, sweep preserved.
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        let block_entities = vec![EntityType::Arc(arc)];

        // Mirror-X plus stretch X by 2: total X factor = -2, Y factor = 1.
        let insert = Insert::new("B", Vector3::ZERO).with_scale(-2.0, 1.0, 1.0);
        let result = insert.explode(&block_entities);

        if let EntityType::Ellipse(e) = &result[0] {
            let (start, mid, end) = ellipse_sample_points(e);
            // Apply the transform manually to the original arc samples to derive expectations:
            //  start (1,0) → (-2,0)
            //  mid   (~0.707, ~0.707) → (~-1.414, ~0.707)
            //  end   (0,1) → (0,1)
            let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
            assert!(approx_vec(start, Vector3::new(-2.0, 0.0, 0.0)));
            assert!(approx_vec(
                mid,
                Vector3::new(-2.0 * inv_sqrt2, inv_sqrt2, 0.0)
            ));
            assert!(approx_vec(end, Vector3::new(0.0, 1.0, 0.0)));
        } else {
            panic!("expected Ellipse for non-uniform mirrored scale");
        }
    }
}
