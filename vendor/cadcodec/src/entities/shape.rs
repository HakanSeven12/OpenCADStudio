//! Shape entity implementation.
//!
//! The Shape entity represents a reference to a shape defined in an
//! external .shx shape file. Shapes are similar to blocks but are
//! defined in compiled shape files.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

// ============================================================================
// Shape Entity
// ============================================================================

/// Shape entity.
///
/// Represents a reference to a shape defined in an external .shx shape file.
/// Each shape has an insertion point, size, rotation, and can be scaled
/// and obliqued.
///
/// # DXF Information
/// - Entity type: SHAPE
/// - Subclass marker: AcDbShape
/// - Object type: 0x21 (33)
///
/// # Shape Files
/// Shape files (.shx) are compiled from shape definition files (.shp).
/// They contain vector glyph definitions identified by number.
/// The TextStyle table entry with IsShapeFile flag links to the .shx file.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::Shape;
/// use acadrust::types::Vector3;
///
/// let mut shape = Shape::new();
/// shape.insertion_point = Vector3::new(100.0, 50.0, 0.0);
/// shape.size = 5.0;
/// shape.shape_number = 132; // The shape number from the .shx file
/// shape.shape_name = "ARROW".to_string();
/// shape.style_name = "MYSHAPES".to_string(); // TextStyle name
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Shape {
    /// Common entity data.
    pub common: EntityCommon,

    /// Insertion point in WCS.
    /// DXF codes: 10, 20, 30
    pub insertion_point: Vector3,

    /// Shape size (height multiplier).
    /// DXF code: 40
    pub size: f64,

    /// Shape name (from the .shx file).
    /// DXF code: 2
    pub shape_name: String,

    /// Shape number in the .shx file.
    /// This is the identifier used to look up the shape in the file.
    pub shape_number: i32,

    /// Rotation angle in radians.
    /// DXF code: 50
    pub rotation: f64,

    /// Relative X scale factor (width factor).
    /// DXF code: 41
    pub relative_x_scale: f64,

    /// Oblique angle in radians.
    /// DXF code: 51
    pub oblique_angle: f64,

    /// Extrusion direction (normal vector).
    /// DXF codes: 210, 220, 230
    pub normal: Vector3,

    /// Thickness (extrusion in Z direction).
    /// DXF code: 39
    pub thickness: f64,

    /// Text style name (references TextStyle with IsShapeFile).
    /// DXF code: 7 (in C# code)
    pub style_name: String,

    /// Text style handle (for resolving the .shx file).
    pub style_handle: Option<Handle>,
}

impl Shape {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "SHAPE";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbShape";

    /// Object type code.
    pub const OBJECT_TYPE: i16 = 0x21; // 33

    /// Creates a new shape with default values.
    pub fn new() -> Self {
        Shape {
            common: EntityCommon::default(),
            insertion_point: Vector3::ZERO,
            size: 1.0,
            shape_name: String::new(),
            shape_number: 0,
            rotation: 0.0,
            relative_x_scale: 1.0,
            oblique_angle: 0.0,
            normal: Vector3::UNIT_Z,
            thickness: 0.0,
            style_name: String::new(),
            style_handle: None,
        }
    }

    /// Creates a shape at the specified location with a name.
    ///
    /// # Arguments
    /// * `insertion_point` - Location in world coordinates
    /// * `shape_name` - Name of the shape in the .shx file
    /// * `size` - Height/size multiplier
    pub fn with_name(insertion_point: Vector3, shape_name: &str, size: f64) -> Self {
        Shape {
            insertion_point,
            shape_name: shape_name.to_string(),
            size,
            ..Self::new()
        }
    }

    /// Creates a shape at the specified location with a number.
    ///
    /// # Arguments
    /// * `insertion_point` - Location in world coordinates
    /// * `shape_number` - Number of the shape in the .shx file
    /// * `size` - Height/size multiplier
    pub fn with_number(insertion_point: Vector3, shape_number: i32, size: f64) -> Self {
        Shape {
            insertion_point,
            shape_number,
            size,
            ..Self::new()
        }
    }

    /// Creates a shape with full parameters.
    ///
    /// # Arguments
    /// * `insertion_point` - Location in world coordinates
    /// * `shape_name` - Name of the shape
    /// * `style_name` - Name of the TextStyle that references the .shx file
    /// * `size` - Height/size multiplier
    /// * `rotation` - Rotation angle in radians
    pub fn with_style(
        insertion_point: Vector3,
        shape_name: &str,
        style_name: &str,
        size: f64,
        rotation: f64,
    ) -> Self {
        Shape {
            insertion_point,
            shape_name: shape_name.to_string(),
            style_name: style_name.to_string(),
            size,
            rotation,
            ..Self::new()
        }
    }

    /// Sets the rotation angle in degrees.
    pub fn set_rotation_degrees(&mut self, degrees: f64) {
        self.rotation = degrees.to_radians();
    }

    /// Gets the rotation angle in degrees.
    pub fn rotation_degrees(&self) -> f64 {
        self.rotation.to_degrees()
    }

    /// Sets the oblique angle in degrees.
    pub fn set_oblique_degrees(&mut self, degrees: f64) {
        self.oblique_angle = degrees.to_radians();
    }

    /// Gets the oblique angle in degrees.
    pub fn oblique_degrees(&self) -> f64 {
        self.oblique_angle.to_degrees()
    }

    /// Returns the effective width (size * relative_x_scale).
    pub fn width(&self) -> f64 {
        self.size * self.relative_x_scale
    }

    /// Returns the effective height (same as size).
    pub fn height(&self) -> f64 {
        self.size
    }

    /// Returns the direction vector based on rotation.
    pub fn direction(&self) -> Vector3 {
        Vector3::new(self.rotation.cos(), self.rotation.sin(), 0.0)
    }

    /// Rotates the shape by an additional angle (in radians).
    pub fn rotate(&mut self, angle: f64) {
        self.rotation += angle;
        // Normalize to 0..2PI
        while self.rotation < 0.0 {
            self.rotation += std::f64::consts::TAU;
        }
        while self.rotation >= std::f64::consts::TAU {
            self.rotation -= std::f64::consts::TAU;
        }
    }

    /// Scales the shape uniformly.
    pub fn scale(&mut self, factor: f64) {
        self.size *= factor;
    }

    /// Scales the shape non-uniformly.
    pub fn scale_xy(&mut self, x_factor: f64, y_factor: f64) {
        self.size *= y_factor;
        self.relative_x_scale *= x_factor / y_factor;
    }

    /// Sets the normal vector (extrusion direction).
    pub fn set_normal(&mut self, normal: Vector3) {
        self.normal = normal.normalize();
    }

    /// Returns true if the shape has thickness (3D extrusion).
    pub fn has_thickness(&self) -> bool {
        self.thickness.abs() > 1e-10
    }

    /// Returns true if the shape has a custom extrusion direction.
    pub fn has_custom_normal(&self) -> bool {
        (self.normal.x.abs() > 1e-10)
            || (self.normal.y.abs() > 1e-10)
            || ((self.normal.z - 1.0).abs() > 1e-10)
    }

    /// Returns true if the shape is oblique.
    pub fn is_oblique(&self) -> bool {
        self.oblique_angle.abs() > 1e-10
    }

    /// Returns true if the shape has non-uniform scaling.
    pub fn is_scaled(&self) -> bool {
        (self.relative_x_scale - 1.0).abs() > 1e-10
    }

    /// Mirrors the shape horizontally.
    pub fn mirror_x(&mut self) {
        self.relative_x_scale = -self.relative_x_scale;
        self.rotation = std::f64::consts::PI - self.rotation;
    }

    /// Mirrors the shape vertically.
    pub fn mirror_y(&mut self) {
        self.rotation = -self.rotation;
    }

    /// Calculates an approximate bounding box.
    /// Note: Since shape geometry is defined in the .shx file, this is an estimate.
    pub fn approximate_bounds(&self) -> (Vector3, Vector3) {
        let half_width = self.width() / 2.0;
        let half_height = self.height() / 2.0;

        // Conservative estimate assuming shape centered on insertion point
        let min = Vector3::new(
            self.insertion_point.x - half_width,
            self.insertion_point.y - half_height,
            self.insertion_point.z,
        );
        let max = Vector3::new(
            self.insertion_point.x + half_width,
            self.insertion_point.y + half_height,
            self.insertion_point.z + self.thickness,
        );

        (min, max)
    }

    /// Returns a transformation matrix for this shape (as a flat array).
    /// [m11, m12, m13, m21, m22, m23, m31, m32, m33, tx, ty, tz]
    pub fn transform_matrix(&self) -> [f64; 12] {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();
        let tan_o = self.oblique_angle.tan();

        let sx = self.relative_x_scale * self.size;
        let sy = self.size;

        [
            cos_r * sx,
            -sin_r * sy + cos_r * tan_o * sy,
            0.0,
            sin_r * sx,
            cos_r * sy + sin_r * tan_o * sy,
            0.0,
            0.0,
            0.0,
            self.thickness.max(1.0),
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        ]
    }
}

impl Default for Shape {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Shape {
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
        let (min, max) = self.approximate_bounds();
        BoundingBox3D::new(min, max).ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_shape(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        Self::ENTITY_NAME
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_shape(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_shape(self, transform);
    }
}

// ============================================================================
// Shape-related utilities
// ============================================================================

/// Common shape names found in standard AutoCAD shape files.
pub mod standard_shapes {
    /// Box shape (standard).
    pub const BOX: &str = "BOX";
    /// Circle shape.
    pub const CIRC: &str = "CIRC";
    /// Hexagon shape.
    pub const HEX: &str = "HEX";
    /// Cross shape.
    pub const CROSS: &str = "CROSS";
    /// Arrow shape.
    pub const ARROW: &str = "ARROW";
    /// Plus sign shape.
    pub const PLUS: &str = "PLUS";
    /// Diamond shape.
    pub const DIAMOND: &str = "DIAMOND";
    /// Triangle shape.
    pub const TRIANGLE: &str = "TRIANGLE";
    /// Square shape.
    pub const SQUARE: &str = "SQUARE";
}

/// Standard GDT (Geometric Dimensioning and Tolerancing) shape numbers.
/// These are from the GDT.SHX file.
pub mod gdt_shapes {
    /// Straightness symbol.
    pub const STRAIGHTNESS: i32 = 110;
    /// Flatness symbol.
    pub const FLATNESS: i32 = 111;
    /// Circularity symbol.
    pub const CIRCULARITY: i32 = 112;
    /// Cylindricity symbol.
    pub const CYLINDRICITY: i32 = 113;
    /// Position symbol.
    pub const POSITION: i32 = 114;
    /// Concentricity symbol.
    pub const CONCENTRICITY: i32 = 115;
    /// Symmetry symbol.
    pub const SYMMETRY: i32 = 116;
    /// Parallelism symbol.
    pub const PARALLELISM: i32 = 117;
    /// Perpendicularity symbol.
    pub const PERPENDICULARITY: i32 = 118;
    /// Angularity symbol.
    pub const ANGULARITY: i32 = 119;
    /// Line profile symbol.
    pub const LINE_PROFILE: i32 = 120;
    /// Surface profile symbol.
    pub const SURFACE_PROFILE: i32 = 121;
    /// Circular runout symbol.
    pub const CIRCULAR_RUNOUT: i32 = 122;
    /// Total runout symbol.
    pub const TOTAL_RUNOUT: i32 = 123;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_creation() {
        let shape = Shape::new();
        assert_eq!(shape.insertion_point, Vector3::ZERO);
        assert_eq!(shape.size, 1.0);
        assert_eq!(shape.rotation, 0.0);
        assert_eq!(shape.relative_x_scale, 1.0);
        assert_eq!(shape.oblique_angle, 0.0);
        assert!(shape.shape_name.is_empty());
    }

    #[test]
    fn test_with_name() {
        let shape = Shape::with_name(Vector3::new(10.0, 20.0, 0.0), "ARROW", 5.0);
        assert_eq!(shape.insertion_point.x, 10.0);
        assert_eq!(shape.shape_name, "ARROW");
        assert_eq!(shape.size, 5.0);
    }

    #[test]
    fn test_with_number() {
        let shape = Shape::with_number(Vector3::new(10.0, 20.0, 0.0), 132, 5.0);
        assert_eq!(shape.shape_number, 132);
        assert_eq!(shape.size, 5.0);
    }

    #[test]
    fn test_with_style() {
        let shape = Shape::with_style(
            Vector3::new(0.0, 0.0, 0.0),
            "ARROW",
            "MYSHAPES",
            3.0,
            std::f64::consts::FRAC_PI_2,
        );
        assert_eq!(shape.shape_name, "ARROW");
        assert_eq!(shape.style_name, "MYSHAPES");
        assert_eq!(shape.size, 3.0);
        assert!((shape.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    }

    #[test]
    fn test_rotation_degrees() {
        let mut shape = Shape::new();
        shape.set_rotation_degrees(45.0);
        assert!((shape.rotation_degrees() - 45.0).abs() < 1e-10);
    }

    #[test]
    fn test_oblique_degrees() {
        let mut shape = Shape::new();
        shape.set_oblique_degrees(15.0);
        assert!((shape.oblique_degrees() - 15.0).abs() < 1e-10);
    }

    #[test]
    fn test_width_height() {
        let mut shape = Shape::new();
        shape.size = 10.0;
        shape.relative_x_scale = 0.5;

        assert_eq!(shape.height(), 10.0);
        assert_eq!(shape.width(), 5.0);
    }

    #[test]
    fn test_direction() {
        let mut shape = Shape::new();
        shape.rotation = 0.0;
        let dir = shape.direction();
        assert!((dir.x - 1.0).abs() < 1e-10);
        assert!(dir.y.abs() < 1e-10);

        shape.rotation = std::f64::consts::FRAC_PI_2;
        let dir = shape.direction();
        assert!(dir.x.abs() < 1e-10);
        assert!((dir.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_rotate() {
        let mut shape = Shape::new();
        shape.rotate(std::f64::consts::PI);
        assert!((shape.rotation - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_scale() {
        let mut shape = Shape::new();
        shape.size = 10.0;
        shape.scale(2.0);
        assert_eq!(shape.size, 20.0);
    }

    #[test]
    fn test_scale_xy() {
        let mut shape = Shape::new();
        shape.size = 10.0;
        shape.relative_x_scale = 1.0;

        shape.scale_xy(2.0, 0.5);

        assert_eq!(shape.size, 5.0);
        assert_eq!(shape.relative_x_scale, 4.0);
    }

    #[test]
    fn test_has_thickness() {
        let mut shape = Shape::new();
        assert!(!shape.has_thickness());

        shape.thickness = 5.0;
        assert!(shape.has_thickness());
    }

    #[test]
    fn test_has_custom_normal() {
        let mut shape = Shape::new();
        assert!(!shape.has_custom_normal());

        shape.normal = Vector3::new(1.0, 0.0, 0.0);
        assert!(shape.has_custom_normal());
    }

    #[test]
    fn test_is_oblique() {
        let mut shape = Shape::new();
        assert!(!shape.is_oblique());

        shape.oblique_angle = 0.25;
        assert!(shape.is_oblique());
    }

    #[test]
    fn test_is_scaled() {
        let mut shape = Shape::new();
        assert!(!shape.is_scaled());

        shape.relative_x_scale = 1.5;
        assert!(shape.is_scaled());
    }

    #[test]
    fn test_translate() {
        let mut shape = Shape::new();
        shape.translate(Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(shape.insertion_point, Vector3::new(10.0, 20.0, 30.0));
    }

    #[test]
    fn test_entity_type() {
        let shape = Shape::new();
        assert_eq!(shape.entity_type(), "SHAPE");
    }

    #[test]
    fn test_approximate_bounds() {
        let mut shape = Shape::new();
        shape.insertion_point = Vector3::new(10.0, 10.0, 0.0);
        shape.size = 10.0;
        shape.relative_x_scale = 1.0;

        let (min, max) = shape.approximate_bounds();
        assert_eq!(min.x, 5.0);
        assert_eq!(min.y, 5.0);
        assert_eq!(max.x, 15.0);
        assert_eq!(max.y, 15.0);
    }

    #[test]
    fn test_transform_matrix() {
        let mut shape = Shape::new();
        shape.insertion_point = Vector3::new(10.0, 20.0, 0.0);
        shape.size = 2.0;
        shape.relative_x_scale = 1.0;
        shape.rotation = 0.0;

        let matrix = shape.transform_matrix();
        // tx, ty at indices 9, 10
        assert_eq!(matrix[9], 10.0);
        assert_eq!(matrix[10], 20.0);
    }

    #[test]
    fn test_standard_shapes() {
        assert_eq!(standard_shapes::ARROW, "ARROW");
        assert_eq!(standard_shapes::BOX, "BOX");
    }

    #[test]
    fn test_gdt_shapes() {
        assert_eq!(gdt_shapes::POSITION, 114);
        assert_eq!(gdt_shapes::PERPENDICULARITY, 118);
    }

    #[test]
    fn test_default() {
        let shape = Shape::default();
        assert_eq!(shape.size, 1.0);
    }
}
