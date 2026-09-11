//! Wipeout entity implementation.
//!
//! The Wipeout entity represents a blank/masking area that hides
//! objects behind it in the drawing.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector2, Vector3};

use bitflags::bitflags;

// ============================================================================
// Flags and Enums
// ============================================================================

bitflags! {
    /// Display flags for wipeout and raster image entities.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct WipeoutDisplayFlags: i16 {
        /// No flags.
        const NONE = 0;
        /// Show the wipeout/image.
        const SHOW_IMAGE = 1;
        /// Show when not aligned with screen.
        const SHOW_NOT_ALIGNED = 2;
        /// Use clipping boundary.
        const USE_CLIPPING_BOUNDARY = 4;
        /// Transparency is on.
        const TRANSPARENCY_ON = 8;
        /// Default flags for wipeout.
        const DEFAULT = Self::SHOW_IMAGE.bits() | Self::SHOW_NOT_ALIGNED.bits() | Self::USE_CLIPPING_BOUNDARY.bits();
    }
}

/// Clipping boundary type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum WipeoutClipType {
    /// Rectangular clipping (2 vertices: opposite corners).
    #[default]
    Rectangular = 1,
    /// Polygonal clipping (3+ vertices).
    Polygonal = 2,
}

impl From<i16> for WipeoutClipType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Rectangular,
            2 => Self::Polygonal,
            _ => Self::Rectangular,
        }
    }
}

/// Clipping mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum WipeoutClipMode {
    /// Show inside the boundary, clip outside.
    #[default]
    Outside = 0,
    /// Show outside the boundary, clip inside (inverted).
    Inside = 1,
}

impl From<u8> for WipeoutClipMode {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Inside,
            _ => Self::Outside,
        }
    }
}

// ============================================================================
// Wipeout Entity
// ============================================================================

/// Wipeout entity.
///
/// Represents a blank/masking area that hides objects behind it.
/// The wipeout appears as an opaque white (or background color) area
/// with optional visible frame.
///
/// # DXF Information
/// - Entity type: WIPEOUT
/// - Subclass marker: AcDbRasterImage
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::Wipeout;
/// use acadrust::types::{Vector2, Vector3};
///
/// // Create a rectangular wipeout
/// let wipeout = Wipeout::rectangular(
///     Vector3::new(10.0, 10.0, 0.0),
///     50.0,
///     30.0,
/// );
///
/// // Create a polygonal wipeout
/// let mut wipeout = Wipeout::new();
/// wipeout.set_polygon(&[
///     Vector2::new(0.0, 0.0),
///     Vector2::new(100.0, 0.0),
///     Vector2::new(100.0, 50.0),
///     Vector2::new(50.0, 75.0),
///     Vector2::new(0.0, 50.0),
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Wipeout {
    /// Common entity data.
    pub common: EntityCommon,

    /// Class version number.
    /// DXF code: 90
    pub class_version: i32,

    /// Insertion point in WCS.
    /// DXF codes: 10, 20, 30
    pub insertion_point: Vector3,

    /// U-vector (horizontal direction with pixel width scale).
    /// DXF codes: 11, 21, 31
    pub u_vector: Vector3,

    /// V-vector (vertical direction with pixel height scale).
    /// DXF codes: 12, 22, 32
    pub v_vector: Vector3,

    /// Size in pixels (for internal representation).
    /// DXF codes: 13, 23
    pub size: Vector2,

    /// Display flags.
    /// DXF code: 70
    pub flags: WipeoutDisplayFlags,

    /// Whether clipping is enabled.
    /// DXF code: 280
    pub clipping_enabled: bool,

    /// Brightness (0-100).
    /// DXF code: 281
    pub brightness: u8,

    /// Contrast (0-100).
    /// DXF code: 282
    pub contrast: u8,

    /// Fade (0-100).
    /// DXF code: 283
    pub fade: u8,

    /// Clipping mode (inside/outside).
    /// DXF code: 290 (R2010+)
    pub clip_mode: WipeoutClipMode,

    /// Clipping boundary type.
    /// DXF code: 71
    pub clip_type: WipeoutClipType,

    /// Clipping boundary vertices (in image pixel space).
    /// DXF codes: 14, 24 (repeated)
    pub clip_boundary_vertices: Vec<Vector2>,

    /// Image definition handle (internal, usually None for wipeout).
    pub definition_handle: Option<Handle>,

    /// Image definition reactor handle (internal).
    pub definition_reactor_handle: Option<Handle>,
}

impl Wipeout {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "WIPEOUT";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbRasterImage";

    /// Creates a new wipeout with default values.
    pub fn new() -> Self {
        Wipeout {
            common: EntityCommon::default(),
            class_version: 0,
            insertion_point: Vector3::ZERO,
            u_vector: Vector3::UNIT_X,
            v_vector: Vector3::UNIT_Y,
            size: Vector2::new(1.0, 1.0),
            flags: WipeoutDisplayFlags::DEFAULT,
            clipping_enabled: true,
            brightness: 50,
            contrast: 50,
            fade: 0,
            clip_mode: WipeoutClipMode::Outside,
            clip_type: WipeoutClipType::Rectangular,
            clip_boundary_vertices: vec![Vector2::new(-0.5, -0.5), Vector2::new(0.5, 0.5)],
            definition_handle: None,
            definition_reactor_handle: None,
        }
    }

    /// Creates a rectangular wipeout at the given location.
    ///
    /// # Arguments
    /// * `insertion_point` - Lower-left corner in world coordinates
    /// * `width` - Width in world units
    /// * `height` - Height in world units
    pub fn rectangular(insertion_point: Vector3, width: f64, height: f64) -> Self {
        Wipeout {
            insertion_point,
            u_vector: Vector3::new(width, 0.0, 0.0),
            v_vector: Vector3::new(0.0, height, 0.0),
            size: Vector2::new(1.0, 1.0),
            clip_type: WipeoutClipType::Rectangular,
            clip_boundary_vertices: vec![Vector2::new(-0.5, -0.5), Vector2::new(0.5, 0.5)],
            ..Self::new()
        }
    }

    /// Creates a wipeout from corner points.
    pub fn from_corners(corner1: Vector3, corner2: Vector3) -> Self {
        let min_x = corner1.x.min(corner2.x);
        let min_y = corner1.y.min(corner2.y);
        let max_x = corner1.x.max(corner2.x);
        let max_y = corner1.y.max(corner2.y);

        Self::rectangular(
            Vector3::new(min_x, min_y, corner1.z),
            max_x - min_x,
            max_y - min_y,
        )
    }

    /// Creates a polygonal wipeout from world coordinate vertices.
    pub fn polygonal(vertices: &[Vector2], z: f64) -> Self {
        if vertices.len() < 3 {
            return Self::new();
        }

        // Calculate bounding box
        let mut min_x = vertices[0].x;
        let mut min_y = vertices[0].y;
        let mut max_x = vertices[0].x;
        let mut max_y = vertices[0].y;

        for v in vertices.iter().skip(1) {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }

        let width = max_x - min_x;
        let height = max_y - min_y;

        // Clip coordinates are centred on the image. X grows right while Y
        // grows down in raster space, hence the vertical inversion.
        let normalized: Vec<Vector2> = vertices
            .iter()
            .map(|v| {
                Vector2::new(
                    if width > 0.0 {
                        (v.x - min_x) / width - 0.5
                    } else {
                        0.0
                    },
                    if height > 0.0 {
                        0.5 - (v.y - min_y) / height
                    } else {
                        0.0
                    },
                )
            })
            .collect();

        Wipeout {
            insertion_point: Vector3::new(min_x, min_y, z),
            u_vector: Vector3::new(width, 0.0, 0.0),
            v_vector: Vector3::new(0.0, height, 0.0),
            size: Vector2::new(1.0, 1.0),
            clip_type: WipeoutClipType::Polygonal,
            clip_boundary_vertices: normalized,
            ..Self::new()
        }
    }

    /// Sets a rectangular clipping boundary in normalized coordinates.
    pub fn set_rectangular(&mut self, lower_left: Vector2, upper_right: Vector2) {
        self.clip_type = WipeoutClipType::Rectangular;
        self.clip_boundary_vertices = vec![lower_left, upper_right];
    }

    /// Sets a polygonal clipping boundary in normalized coordinates.
    pub fn set_polygon(&mut self, vertices: &[Vector2]) {
        if vertices.len() >= 3 {
            self.clip_type = WipeoutClipType::Polygonal;
            self.clip_boundary_vertices = vertices.to_vec();
        }
    }

    /// Gets the clipping boundary vertices in world coordinates.
    pub fn world_boundary_vertices(&self) -> Vec<Vector3> {
        self.clip_boundary_vertices
            .iter()
            .map(|v| {
                self.insertion_point
                    + self.u_vector * (v.x + self.size.x * 0.5)
                    + self.v_vector * (self.size.y * 0.5 - v.y)
            })
            .collect()
    }

    /// Returns the width in world units.
    pub fn width(&self) -> f64 {
        self.u_vector.length() * self.size.x.abs()
    }

    /// Returns the height in world units.
    pub fn height(&self) -> f64 {
        self.v_vector.length() * self.size.y.abs()
    }

    /// Returns the area in world units.
    pub fn area(&self) -> f64 {
        if self.clip_type == WipeoutClipType::Rectangular {
            self.u_vector.cross(&self.v_vector).length() * self.size.x.abs() * self.size.y.abs()
        } else {
            // Calculate polygon area using shoelace formula
            self.polygon_area()
        }
    }

    /// Calculates the polygon area in world units.
    fn polygon_area(&self) -> f64 {
        let verts = &self.clip_boundary_vertices;
        if verts.len() < 3 {
            return 0.0;
        }

        let mut area = 0.0;
        for i in 0..verts.len() {
            let j = (i + 1) % verts.len();
            area += verts[i].x * verts[j].y;
            area -= verts[j].x * verts[i].y;
        }

        // Clip vertices are measured in pixels. Convert pixel area to world
        // area using the parallelogram spanned by the per-pixel U/V vectors.
        (area / 2.0).abs() * self.u_vector.cross(&self.v_vector).length()
    }

    /// Sets the size of the wipeout (width and height).
    pub fn set_size(&mut self, width: f64, height: f64) {
        let u_dir = if self.u_vector.length() > 1e-10 {
            self.u_vector.normalize()
        } else {
            Vector3::UNIT_X
        };
        let v_dir = if self.v_vector.length() > 1e-10 {
            self.v_vector.normalize()
        } else {
            Vector3::UNIT_Y
        };

        let pixel_width = self.size.x.abs().max(f64::EPSILON);
        let pixel_height = self.size.y.abs().max(f64::EPSILON);
        self.u_vector = u_dir * (width / pixel_width);
        self.v_vector = v_dir * (height / pixel_height);
    }

    /// Rotates the wipeout around its insertion point.
    pub fn rotate(&mut self, angle: f64) {
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let u = self.u_vector;
        let v = self.v_vector;

        self.u_vector = Vector3::new(u.x * cos_a - u.y * sin_a, u.x * sin_a + u.y * cos_a, u.z);
        self.v_vector = Vector3::new(v.x * cos_a - v.y * sin_a, v.x * sin_a + v.y * cos_a, v.z);
    }

    /// Scales the wipeout uniformly.
    pub fn scale(&mut self, factor: f64) {
        self.u_vector = self.u_vector * factor;
        self.v_vector = self.v_vector * factor;
    }

    /// Returns true if the wipeout is rectangular.
    pub fn is_rectangular(&self) -> bool {
        self.clip_type == WipeoutClipType::Rectangular
    }

    /// Returns true if the wipeout is polygonal.
    pub fn is_polygonal(&self) -> bool {
        self.clip_type == WipeoutClipType::Polygonal
    }

    /// Returns the number of clip boundary vertices.
    pub fn vertex_count(&self) -> usize {
        self.clip_boundary_vertices.len()
    }

    /// Sets the frame visibility (via flags).
    pub fn set_frame_visible(&mut self, visible: bool) {
        if visible {
            self.flags |= WipeoutDisplayFlags::SHOW_IMAGE;
        } else {
            self.flags -= WipeoutDisplayFlags::SHOW_IMAGE;
        }
    }

    /// Returns true if the frame is visible.
    pub fn is_frame_visible(&self) -> bool {
        self.flags.contains(WipeoutDisplayFlags::SHOW_IMAGE)
    }

    /// Returns the center point in world coordinates.
    pub fn center(&self) -> Vector3 {
        self.insertion_point
            + self.u_vector * (self.size.x * 0.5)
            + self.v_vector * (self.size.y * 0.5)
    }

    /// Returns the four corners in world coordinates (for rectangular).
    pub fn corners(&self) -> [Vector3; 4] {
        let u = self.u_vector * self.size.x;
        let v = self.v_vector * self.size.y;
        [
            self.insertion_point,
            self.insertion_point + u,
            self.insertion_point + u + v,
            self.insertion_point + v,
        ]
    }

    /// Checks if a point is inside the wipeout boundary.
    pub fn contains_point(&self, point: Vector3) -> bool {
        let local = point - self.insertion_point;
        let uu = self.u_vector.dot(&self.u_vector);
        let uv = self.u_vector.dot(&self.v_vector);
        let vv = self.v_vector.dot(&self.v_vector);
        let det = uu * vv - uv * uv;
        if !det.is_finite() || det.abs() <= f64::EPSILON * uu.max(vv).powi(2) {
            return false;
        }
        let lu = local.dot(&self.u_vector);
        let lv = local.dot(&self.v_vector);
        let u = (lu * vv - lv * uv) / det;
        let v = (lv * uu - lu * uv) / det;
        let clip_point = Vector2::new(u - self.size.x * 0.5, self.size.y * 0.5 - v);

        if self.clip_type == WipeoutClipType::Rectangular {
            let min = &self.clip_boundary_vertices[0];
            let max = &self.clip_boundary_vertices[1];
            clip_point.x >= min.x.min(max.x)
                && clip_point.x <= min.x.max(max.x)
                && clip_point.y >= min.y.min(max.y)
                && clip_point.y <= min.y.max(max.y)
        } else {
            // Point-in-polygon test using ray casting
            self.point_in_polygon(clip_point)
        }
    }

    /// Ray casting point-in-polygon test.
    fn point_in_polygon(&self, point: Vector2) -> bool {
        let verts = &self.clip_boundary_vertices;
        if verts.len() < 3 {
            return false;
        }

        let mut inside = false;
        let mut j = verts.len() - 1;

        for i in 0..verts.len() {
            if ((verts[i].y > point.y) != (verts[j].y > point.y))
                && (point.x
                    < (verts[j].x - verts[i].x) * (point.y - verts[i].y)
                        / (verts[j].y - verts[i].y)
                        + verts[i].x)
            {
                inside = !inside;
            }
            j = i;
        }

        inside
    }
}

impl Default for Wipeout {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Wipeout {
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
        let corners = self.corners();
        let mut min = corners[0];
        let mut max = corners[0];

        for c in &corners[1..] {
            min.x = min.x.min(c.x);
            min.y = min.y.min(c.y);
            min.z = min.z.min(c.z);
            max.x = max.x.max(c.x);
            max.y = max.y.max(c.y);
            max.z = max.z.max(c.z);
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_wipeout(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        Self::ENTITY_NAME
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_wipeout(self, transform);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wipeout_creation() {
        let wipeout = Wipeout::new();
        assert_eq!(wipeout.insertion_point, Vector3::ZERO);
        assert!(wipeout.clipping_enabled);
        assert_eq!(wipeout.brightness, 50);
        assert_eq!(wipeout.contrast, 50);
        assert_eq!(wipeout.fade, 0);
    }

    #[test]
    fn test_rectangular_wipeout() {
        let wipeout = Wipeout::rectangular(Vector3::new(10.0, 20.0, 0.0), 100.0, 50.0);
        assert_eq!(wipeout.insertion_point.x, 10.0);
        assert_eq!(wipeout.insertion_point.y, 20.0);
        assert!((wipeout.width() - 100.0).abs() < 1e-10);
        assert!((wipeout.height() - 50.0).abs() < 1e-10);
        assert!(wipeout.is_rectangular());
    }

    #[test]
    fn test_from_corners() {
        let wipeout =
            Wipeout::from_corners(Vector3::new(10.0, 20.0, 0.0), Vector3::new(60.0, 70.0, 0.0));
        assert!((wipeout.width() - 50.0).abs() < 1e-10);
        assert!((wipeout.height() - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_polygonal_wipeout() {
        let vertices = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(100.0, 100.0),
            Vector2::new(0.0, 100.0),
        ];
        let wipeout = Wipeout::polygonal(&vertices, 0.0);
        assert!(wipeout.is_polygonal());
        assert_eq!(wipeout.vertex_count(), 4);
    }

    #[test]
    fn test_world_boundary_vertices() {
        let wipeout = Wipeout::rectangular(Vector3::new(10.0, 10.0, 0.0), 50.0, 30.0);
        let verts = wipeout.world_boundary_vertices();
        assert_eq!(verts.len(), 2);
        assert!((verts[0].x - 10.0).abs() < 1e-10);
        assert!((verts[1].x - 60.0).abs() < 1e-10);
    }

    #[test]
    fn test_area_rectangular() {
        let wipeout = Wipeout::rectangular(Vector3::ZERO, 10.0, 5.0);
        assert!((wipeout.area() - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_center() {
        let wipeout = Wipeout::rectangular(Vector3::new(0.0, 0.0, 0.0), 10.0, 10.0);
        let center = wipeout.center();
        assert!((center.x - 5.0).abs() < 1e-10);
        assert!((center.y - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_corners() {
        let wipeout = Wipeout::rectangular(Vector3::new(0.0, 0.0, 0.0), 10.0, 20.0);
        let corners = wipeout.corners();
        assert_eq!(corners[0], Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(corners[1], Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(corners[2], Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(corners[3], Vector3::new(0.0, 20.0, 0.0));
    }

    #[test]
    fn test_rotate() {
        let mut wipeout = Wipeout::rectangular(Vector3::ZERO, 10.0, 0.0);
        wipeout.u_vector = Vector3::new(10.0, 0.0, 0.0);
        wipeout.v_vector = Vector3::new(0.0, 10.0, 0.0);

        wipeout.rotate(std::f64::consts::FRAC_PI_2); // 90 degrees

        assert!((wipeout.u_vector.y - 10.0).abs() < 1e-10);
        assert!((wipeout.v_vector.x + 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_scale() {
        let mut wipeout = Wipeout::rectangular(Vector3::ZERO, 10.0, 5.0);
        wipeout.scale(2.0);

        assert!((wipeout.width() - 20.0).abs() < 1e-10);
        assert!((wipeout.height() - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_translate() {
        let mut wipeout = Wipeout::rectangular(Vector3::ZERO, 10.0, 10.0);
        wipeout.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(wipeout.insertion_point.x, 5.0);
        assert_eq!(wipeout.insertion_point.y, 5.0);
    }

    #[test]
    fn test_bounding_box() {
        let wipeout = Wipeout::rectangular(Vector3::new(10.0, 20.0, 0.0), 30.0, 40.0);
        let bb = wipeout.bounding_box();

        assert!((bb.min.x - 10.0).abs() < 1e-10);
        assert!((bb.min.y - 20.0).abs() < 1e-10);
        assert!((bb.max.x - 40.0).abs() < 1e-10);
        assert!((bb.max.y - 60.0).abs() < 1e-10);
    }

    #[test]
    fn test_entity_type() {
        let wipeout = Wipeout::new();
        assert_eq!(wipeout.entity_type(), "WIPEOUT");
    }

    #[test]
    fn test_set_size() {
        let mut wipeout = Wipeout::rectangular(Vector3::ZERO, 10.0, 10.0);
        wipeout.set_size(50.0, 25.0);

        assert!((wipeout.width() - 50.0).abs() < 1e-10);
        assert!((wipeout.height() - 25.0).abs() < 1e-10);
    }

    #[test]
    fn test_frame_visibility() {
        let mut wipeout = Wipeout::new();
        assert!(wipeout.is_frame_visible());

        wipeout.set_frame_visible(false);
        assert!(!wipeout.is_frame_visible());

        wipeout.set_frame_visible(true);
        assert!(wipeout.is_frame_visible());
    }

    #[test]
    fn test_display_flags() {
        let wipeout = Wipeout::new();
        assert!(wipeout.flags.contains(WipeoutDisplayFlags::SHOW_IMAGE));
        assert!(wipeout
            .flags
            .contains(WipeoutDisplayFlags::SHOW_NOT_ALIGNED));
        assert!(wipeout
            .flags
            .contains(WipeoutDisplayFlags::USE_CLIPPING_BOUNDARY));
    }

    #[test]
    fn test_clip_type() {
        let mut wipeout = Wipeout::new();
        assert!(wipeout.is_rectangular());
        assert!(!wipeout.is_polygonal());

        wipeout.set_polygon(&[
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.5, 1.0),
        ]);
        assert!(!wipeout.is_rectangular());
        assert!(wipeout.is_polygonal());
    }

    #[test]
    fn test_contains_point_rectangular() {
        let wipeout = Wipeout::rectangular(Vector3::new(10.0, 10.0, 0.0), 20.0, 20.0);

        // Inside
        assert!(wipeout.contains_point(Vector3::new(20.0, 20.0, 0.0)));
        // Outside
        assert!(!wipeout.contains_point(Vector3::new(0.0, 0.0, 0.0)));
        assert!(!wipeout.contains_point(Vector3::new(50.0, 50.0, 0.0)));
    }
}
