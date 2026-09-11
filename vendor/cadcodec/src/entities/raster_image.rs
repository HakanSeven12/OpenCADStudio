//! RasterImage entity implementation.
//!
//! The RasterImage entity displays an external raster image file
//! (BMP, JPEG, PNG, TIFF, etc.) within a drawing.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector2, Vector3};

use bitflags::bitflags;

// ============================================================================
// Enums
// ============================================================================

/// Clipping mode for raster images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum ClipMode {
    /// Clip the outside (show inside the boundary).
    #[default]
    Outside = 0,
    /// Clip the inside (show outside the boundary, inverted).
    Inside = 1,
}

impl From<u8> for ClipMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Outside,
            1 => Self::Inside,
            _ => Self::Outside,
        }
    }
}

/// Clipping boundary type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum ClipType {
    /// Rectangular clipping (two opposite corners).
    #[default]
    Rectangular = 1,
    /// Polygonal clipping (three or more vertices).
    Polygonal = 2,
}

impl From<i16> for ClipType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Rectangular,
            2 => Self::Polygonal,
            _ => Self::Rectangular,
        }
    }
}

/// Resolution units for image definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum ResolutionUnit {
    /// No units.
    #[default]
    None = 0,
    /// Centimeters.
    Centimeters = 2,
    /// Inches.
    Inches = 5,
}

impl From<u8> for ResolutionUnit {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            2 => Self::Centimeters,
            5 => Self::Inches,
            _ => Self::None,
        }
    }
}

/// Image display quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i32)]
pub enum ImageDisplayQuality {
    /// Draft quality (faster).
    Draft = 0,
    /// High quality.
    #[default]
    High = 1,
}

impl From<i32> for ImageDisplayQuality {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Draft,
            1 => Self::High,
            _ => Self::High,
        }
    }
}

// ============================================================================
// Bitflags
// ============================================================================

bitflags! {
    /// Image display flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct ImageDisplayFlags: i16 {
        /// No flags.
        const NONE = 0;
        /// Show image.
        const SHOW_IMAGE = 1;
        /// Show image when not aligned with screen.
        const SHOW_NOT_ALIGNED = 2;
        /// Use clipping boundary.
        const USE_CLIPPING_BOUNDARY = 4;
        /// Transparency is on.
        const TRANSPARENCY_ON = 8;
    }
}

// ============================================================================
// Clipping Boundary
// ============================================================================

/// Clipping boundary for a raster image.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClipBoundary {
    /// Clipping type.
    pub clip_type: ClipType,
    /// Clipping mode.
    pub clip_mode: ClipMode,
    /// Boundary vertices (in image pixel space).
    /// For rectangular: exactly 2 vertices (opposite corners).
    /// For polygonal: 3 or more vertices.
    pub vertices: Vec<Vector2>,
}

impl ClipBoundary {
    /// Creates a rectangular clipping boundary.
    pub fn rectangular(corner1: Vector2, corner2: Vector2) -> Self {
        Self {
            clip_type: ClipType::Rectangular,
            clip_mode: ClipMode::Outside,
            vertices: vec![corner1, corner2],
        }
    }

    /// Creates a polygonal clipping boundary.
    pub fn polygonal(vertices: Vec<Vector2>) -> Self {
        Self {
            clip_type: ClipType::Polygonal,
            clip_mode: ClipMode::Outside,
            vertices,
        }
    }

    /// Creates a default boundary that shows the entire image.
    pub fn full_image(width: f64, height: f64) -> Self {
        Self::rectangular(
            Vector2::new(-0.5, -0.5),
            Vector2::new(width - 0.5, height - 0.5),
        )
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns true if this is a rectangular boundary.
    pub fn is_rectangular(&self) -> bool {
        self.clip_type == ClipType::Rectangular
    }

    /// Returns true if this is a polygonal boundary.
    pub fn is_polygonal(&self) -> bool {
        self.clip_type == ClipType::Polygonal
    }

    /// Returns the bounding rectangle of the clipping boundary.
    pub fn bounding_rect(&self) -> Option<(Vector2, Vector2)> {
        if self.vertices.is_empty() {
            return None;
        }

        let mut min = self.vertices[0];
        let mut max = self.vertices[0];

        for v in &self.vertices[1..] {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
        }

        Some((min, max))
    }
}

impl Default for ClipBoundary {
    fn default() -> Self {
        Self {
            clip_type: ClipType::Rectangular,
            clip_mode: ClipMode::Outside,
            vertices: Vec::new(),
        }
    }
}

// ============================================================================
// RasterImage Entity
// ============================================================================

/// RasterImage (IMAGE) entity.
///
/// Displays an external raster image file within the drawing. The actual
/// image data is stored in an ImageDefinition object; this entity references
/// that definition and specifies the placement, size, and display properties.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::RasterImage;
/// use acadrust::types::Vector3;
///
/// // Create a raster image at a specific location
/// let mut image = RasterImage::new(
///     "photo.jpg",
///     Vector3::new(0.0, 0.0, 0.0),  // insertion point
///     1920.0,  // pixel width
///     1080.0,  // pixel height
/// );
///
/// // Scale to 10 units wide (maintaining aspect ratio)
/// image.set_width(10.0);
///
/// // Adjust display properties
/// image.brightness = 60;
/// image.contrast = 50;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RasterImage {
    /// Common entity data.
    pub common: EntityCommon,

    /// Class version.
    pub class_version: i32,

    /// Insertion point in WCS.
    pub insertion_point: Vector3,

    /// U-vector of a single pixel (visual width direction).
    /// Length determines horizontal pixel size in world units.
    pub u_vector: Vector3,

    /// V-vector of a single pixel (visual height direction).
    /// Length determines vertical pixel size in world units.
    pub v_vector: Vector3,

    /// Image size in pixels (width, height).
    pub size: Vector2,

    /// Display flags.
    pub flags: ImageDisplayFlags,

    /// Whether clipping is enabled.
    pub clipping_enabled: bool,

    /// Brightness (0-100).
    pub brightness: u8,

    /// Contrast (0-100).
    pub contrast: u8,

    /// Fade (0-100).
    pub fade: u8,

    /// Clipping boundary.
    pub clip_boundary: ClipBoundary,

    /// Handle to the ImageDefinition object.
    pub definition_handle: Option<Handle>,

    /// Handle to the ImageDefinitionReactor object (internal).
    pub definition_reactor_handle: Option<Handle>,

    /// Image file path (stored in ImageDefinition, cached here for convenience).
    pub file_path: String,
}

impl RasterImage {
    /// Creates a new raster image.
    pub fn new(
        file_path: &str,
        insertion_point: Vector3,
        width_pixels: f64,
        height_pixels: f64,
    ) -> Self {
        let size = Vector2::new(width_pixels, height_pixels);

        Self {
            common: EntityCommon::default(),
            class_version: 0,
            insertion_point,
            u_vector: Vector3::new(1.0, 0.0, 0.0),
            v_vector: Vector3::new(0.0, 1.0, 0.0),
            size,
            flags: ImageDisplayFlags::SHOW_IMAGE | ImageDisplayFlags::USE_CLIPPING_BOUNDARY,
            clipping_enabled: false,
            brightness: 50,
            contrast: 50,
            fade: 0,
            clip_boundary: ClipBoundary::full_image(width_pixels, height_pixels),
            definition_handle: None,
            definition_reactor_handle: None,
            file_path: file_path.to_string(),
        }
    }

    /// Creates a raster image with specified world size.
    pub fn with_size(
        file_path: &str,
        insertion_point: Vector3,
        width_pixels: f64,
        height_pixels: f64,
        world_width: f64,
        world_height: f64,
    ) -> Self {
        let mut image = Self::new(file_path, insertion_point, width_pixels, height_pixels);
        image.set_size(world_width, world_height);
        image
    }

    /// Returns the width in world units.
    pub fn width(&self) -> f64 {
        self.u_vector.length() * self.size.x
    }

    /// Returns the height in world units.
    pub fn height(&self) -> f64 {
        self.v_vector.length() * self.size.y
    }

    /// Returns the aspect ratio (width / height).
    pub fn aspect_ratio(&self) -> f64 {
        if self.size.y == 0.0 {
            1.0
        } else {
            self.size.x / self.size.y
        }
    }

    /// Sets the width in world units (adjusts u_vector scale).
    pub fn set_width(&mut self, width: f64) {
        let pixel_size = width / self.size.x;
        self.u_vector = self.u_vector.normalize() * pixel_size;
    }

    /// Sets the height in world units (adjusts v_vector scale).
    pub fn set_height(&mut self, height: f64) {
        let pixel_size = height / self.size.y;
        self.v_vector = self.v_vector.normalize() * pixel_size;
    }

    /// Sets both width and height.
    pub fn set_size(&mut self, width: f64, height: f64) {
        self.set_width(width);
        self.set_height(height);
    }

    /// Sets the width and maintains aspect ratio.
    pub fn set_width_keep_aspect(&mut self, width: f64) {
        let ratio = self.aspect_ratio();
        self.set_width(width);
        self.set_height(width / ratio);
    }

    /// Sets the height and maintains aspect ratio.
    pub fn set_height_keep_aspect(&mut self, height: f64) {
        let ratio = self.aspect_ratio();
        self.set_height(height);
        self.set_width(height * ratio);
    }

    /// Returns the pixel size in world units.
    pub fn pixel_size(&self) -> (f64, f64) {
        (self.u_vector.length(), self.v_vector.length())
    }

    /// Sets uniform pixel size.
    pub fn set_pixel_size(&mut self, size: f64) {
        self.u_vector = self.u_vector.normalize() * size;
        self.v_vector = self.v_vector.normalize() * size;
    }

    /// Returns true if the image is shown.
    pub fn is_visible(&self) -> bool {
        self.flags.contains(ImageDisplayFlags::SHOW_IMAGE)
    }

    /// Sets whether the image is shown.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            self.flags |= ImageDisplayFlags::SHOW_IMAGE;
        } else {
            self.flags &= !ImageDisplayFlags::SHOW_IMAGE;
        }
    }

    /// Returns true if transparency is on.
    pub fn is_transparent(&self) -> bool {
        self.flags.contains(ImageDisplayFlags::TRANSPARENCY_ON)
    }

    /// Sets whether transparency is on.
    pub fn set_transparent(&mut self, transparent: bool) {
        if transparent {
            self.flags |= ImageDisplayFlags::TRANSPARENCY_ON;
        } else {
            self.flags &= !ImageDisplayFlags::TRANSPARENCY_ON;
        }
    }

    /// Returns the four corners of the image in world coordinates.
    pub fn corners(&self) -> [Vector3; 4] {
        let origin = self.insertion_point;
        let u = self.u_vector * self.size.x;
        let v = self.v_vector * self.size.y;

        [
            origin,         // bottom-left
            origin + u,     // bottom-right
            origin + u + v, // top-right
            origin + v,     // top-left
        ]
    }

    /// Returns the center of the image in world coordinates.
    pub fn center(&self) -> Vector3 {
        let u = self.u_vector * self.size.x;
        let v = self.v_vector * self.size.y;
        self.insertion_point + (u + v) * 0.5
    }

    /// Returns the bounding box.
    pub fn bounding_box(&self) -> (Vector3, Vector3) {
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

        (min, max)
    }

    /// Translates the image by the given offset.
    pub fn translate(&mut self, offset: Vector3) {
        self.insertion_point = self.insertion_point + offset;
    }

    /// Rotates the image around the Z axis (in radians).
    pub fn rotate(&mut self, angle: f64) {
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // Rotate u_vector
        let u = self.u_vector;
        self.u_vector = Vector3::new(u.x * cos_a - u.y * sin_a, u.x * sin_a + u.y * cos_a, u.z);

        // Rotate v_vector
        let v = self.v_vector;
        self.v_vector = Vector3::new(v.x * cos_a - v.y * sin_a, v.x * sin_a + v.y * cos_a, v.z);
    }

    /// Sets a rectangular clipping region.
    pub fn set_clip_rect(&mut self, corner1: Vector2, corner2: Vector2) {
        self.clip_boundary = ClipBoundary::rectangular(corner1, corner2);
        self.clipping_enabled = true;
    }

    /// Sets a polygonal clipping region.
    pub fn set_clip_polygon(&mut self, vertices: Vec<Vector2>) {
        self.clip_boundary = ClipBoundary::polygonal(vertices);
        self.clipping_enabled = true;
    }

    /// Clears the clipping region (shows full image).
    pub fn clear_clip(&mut self) {
        self.clip_boundary = ClipBoundary::full_image(self.size.x, self.size.y);
        self.clipping_enabled = false;
    }

    /// Returns the file name (without path).
    pub fn file_name(&self) -> &str {
        self.file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.file_path)
    }

    /// Returns the file extension.
    pub fn file_extension(&self) -> Option<&str> {
        self.file_path.rsplit('.').next()
    }
}

impl Default for RasterImage {
    fn default() -> Self {
        Self::new("", Vector3::ZERO, 1.0, 1.0)
    }
}

impl Entity for RasterImage {
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

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
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
        super::translate::translate_raster_image(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "IMAGE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_raster_image(self, transform);
    }
}

// ============================================================================
// ImageDefinition (Object)
// ============================================================================

/// Image definition object.
///
/// Stores the actual image file reference and properties.
/// Multiple RasterImage entities can reference the same ImageDefinition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDefinition {
    /// Object handle.
    pub handle: Handle,

    /// Class version.
    pub class_version: i32,

    /// Image file path.
    pub file_path: String,

    /// Image size in pixels.
    pub size: Vector2,

    /// Default pixel size in AutoCAD units.
    pub pixel_size: Vector2,

    /// Whether the image file is loaded.
    pub is_loaded: bool,

    /// Resolution units.
    pub resolution_unit: ResolutionUnit,
}

impl ImageDefinition {
    /// Creates a new image definition.
    pub fn new(file_path: &str, width_pixels: f64, height_pixels: f64) -> Self {
        Self {
            handle: Handle::NULL,
            class_version: 0,
            file_path: file_path.to_string(),
            size: Vector2::new(width_pixels, height_pixels),
            pixel_size: Vector2::new(1.0, 1.0),
            is_loaded: true,
            resolution_unit: ResolutionUnit::None,
        }
    }

    /// Returns the aspect ratio.
    pub fn aspect_ratio(&self) -> f64 {
        if self.size.y == 0.0 {
            1.0
        } else {
            self.size.x / self.size.y
        }
    }

    /// Returns the image width in pixels.
    pub fn width(&self) -> f64 {
        self.size.x
    }

    /// Returns the image height in pixels.
    pub fn height(&self) -> f64 {
        self.size.y
    }

    /// Returns the file name (without path).
    pub fn file_name(&self) -> &str {
        self.file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.file_path)
    }

    /// Sets the resolution in DPI (dots per inch).
    pub fn set_dpi(&mut self, dpi: f64) {
        self.resolution_unit = ResolutionUnit::Inches;
        self.pixel_size = Vector2::new(1.0 / dpi, 1.0 / dpi);
    }

    /// Returns the resolution in DPI (if resolution unit is inches).
    pub fn dpi(&self) -> Option<f64> {
        match self.resolution_unit {
            ResolutionUnit::Inches if self.pixel_size.x > 0.0 => Some(1.0 / self.pixel_size.x),
            _ => None,
        }
    }
}

impl Default for ImageDefinition {
    fn default() -> Self {
        Self::new("", 1.0, 1.0)
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for RasterImage entities.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RasterImageBuilder {
    image: RasterImage,
}

impl RasterImageBuilder {
    /// Creates a new builder.
    pub fn new(file_path: &str, width_pixels: f64, height_pixels: f64) -> Self {
        Self {
            image: RasterImage::new(file_path, Vector3::ZERO, width_pixels, height_pixels),
        }
    }

    /// Sets the insertion point.
    pub fn at(mut self, point: Vector3) -> Self {
        self.image.insertion_point = point;
        self
    }

    /// Sets the world size.
    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.image.set_size(width, height);
        self
    }

    /// Sets the world width (maintains aspect ratio).
    pub fn width(mut self, width: f64) -> Self {
        self.image.set_width_keep_aspect(width);
        self
    }

    /// Sets the world height (maintains aspect ratio).
    pub fn height(mut self, height: f64) -> Self {
        self.image.set_height_keep_aspect(height);
        self
    }

    /// Sets the rotation angle in radians.
    pub fn rotation(mut self, angle: f64) -> Self {
        self.image.rotate(angle);
        self
    }

    /// Sets the brightness (0-100).
    pub fn brightness(mut self, brightness: u8) -> Self {
        self.image.brightness = brightness.min(100);
        self
    }

    /// Sets the contrast (0-100).
    pub fn contrast(mut self, contrast: u8) -> Self {
        self.image.contrast = contrast.min(100);
        self
    }

    /// Sets the fade (0-100).
    pub fn fade(mut self, fade: u8) -> Self {
        self.image.fade = fade.min(100);
        self
    }

    /// Enables transparency.
    pub fn transparent(mut self) -> Self {
        self.image.set_transparent(true);
        self
    }

    /// Sets a rectangular clip region.
    pub fn clip_rect(mut self, corner1: Vector2, corner2: Vector2) -> Self {
        self.image.set_clip_rect(corner1, corner2);
        self
    }

    /// Builds the RasterImage.
    pub fn build(self) -> RasterImage {
        self.image
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn test_raster_image_creation() {
        let image = RasterImage::new("test.jpg", Vector3::ZERO, 1920.0, 1080.0);

        assert_eq!(image.file_path, "test.jpg");
        assert_eq!(image.size, Vector2::new(1920.0, 1080.0));
        assert!(image.is_visible());
    }

    #[test]
    fn test_raster_image_size() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 50.0);

        // Default: 1 pixel = 1 unit
        assert!((image.width() - 100.0).abs() < 1e-10);
        assert!((image.height() - 50.0).abs() < 1e-10);

        // Scale to 10 units wide
        image.set_width(10.0);
        assert!((image.width() - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_raster_image_aspect_ratio() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 1920.0, 1080.0);
        let aspect = image.aspect_ratio();

        image.set_width_keep_aspect(19.2);
        assert!((image.width() - 19.2).abs() < 1e-10);
        assert!((image.height() - 10.8).abs() < 0.01);
        assert!((image.aspect_ratio() - aspect).abs() < 1e-10);
    }

    #[test]
    fn test_raster_image_corners() {
        let mut image = RasterImage::new("test.jpg", Vector3::new(10.0, 20.0, 0.0), 100.0, 50.0);
        image.set_size(10.0, 5.0);

        let corners = image.corners();
        assert_eq!(corners[0], Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(corners[1], Vector3::new(20.0, 20.0, 0.0));
        assert_eq!(corners[2], Vector3::new(20.0, 25.0, 0.0));
        assert_eq!(corners[3], Vector3::new(10.0, 25.0, 0.0));
    }

    #[test]
    fn test_raster_image_center() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);
        image.set_size(10.0, 10.0);

        let center = image.center();
        assert_eq!(center, Vector3::new(5.0, 5.0, 0.0));
    }

    #[test]
    fn test_raster_image_translate() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);
        image.translate(Vector3::new(5.0, 10.0, 0.0));

        assert_eq!(image.insertion_point, Vector3::new(5.0, 10.0, 0.0));
    }

    #[test]
    fn test_raster_image_rotate() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);
        image.rotate(PI / 2.0); // 90 degrees

        // U-vector should now point up
        assert!((image.u_vector.x).abs() < 1e-10);
        assert!((image.u_vector.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_raster_image_visibility() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);
        assert!(image.is_visible());

        image.set_visible(false);
        assert!(!image.is_visible());

        image.set_visible(true);
        assert!(image.is_visible());
    }

    #[test]
    fn test_raster_image_transparency() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);
        assert!(!image.is_transparent());

        image.set_transparent(true);
        assert!(image.is_transparent());
    }

    #[test]
    fn test_raster_image_clipping() {
        let mut image = RasterImage::new("test.jpg", Vector3::ZERO, 100.0, 100.0);

        // Set rectangular clip
        image.set_clip_rect(Vector2::new(10.0, 10.0), Vector2::new(90.0, 90.0));
        assert!(image.clipping_enabled);
        assert!(image.clip_boundary.is_rectangular());

        // Set polygonal clip
        image.set_clip_polygon(vec![
            Vector2::new(50.0, 0.0),
            Vector2::new(100.0, 50.0),
            Vector2::new(50.0, 100.0),
            Vector2::new(0.0, 50.0),
        ]);
        assert!(image.clip_boundary.is_polygonal());
        assert_eq!(image.clip_boundary.vertex_count(), 4);

        // Clear clip
        image.clear_clip();
        assert!(!image.clipping_enabled);
    }

    #[test]
    fn test_clip_boundary_rectangular() {
        let boundary = ClipBoundary::rectangular(Vector2::new(0.0, 0.0), Vector2::new(100.0, 50.0));

        assert!(boundary.is_rectangular());
        assert!(!boundary.is_polygonal());
        assert_eq!(boundary.vertex_count(), 2);

        let rect = boundary.bounding_rect().unwrap();
        assert_eq!(rect.0, Vector2::new(0.0, 0.0));
        assert_eq!(rect.1, Vector2::new(100.0, 50.0));
    }

    #[test]
    fn test_clip_boundary_full_image() {
        let boundary = ClipBoundary::full_image(100.0, 50.0);
        assert_eq!(boundary.vertices[0], Vector2::new(-0.5, -0.5));
        assert_eq!(boundary.vertices[1], Vector2::new(99.5, 49.5));
    }

    #[test]
    fn test_raster_image_file_name() {
        let image = RasterImage::new("C:/images/photo.jpg", Vector3::ZERO, 100.0, 100.0);
        assert_eq!(image.file_name(), "photo.jpg");
        assert_eq!(image.file_extension(), Some("jpg"));
    }

    #[test]
    fn test_image_definition() {
        let mut def = ImageDefinition::new("photo.png", 1920.0, 1080.0);

        assert_eq!(def.file_name(), "photo.png");
        assert_eq!(def.width(), 1920.0);
        assert_eq!(def.height(), 1080.0);
        assert!((def.aspect_ratio() - 1.7777777).abs() < 0.0001);

        def.set_dpi(300.0);
        assert_eq!(def.resolution_unit, ResolutionUnit::Inches);
        assert!((def.dpi().unwrap() - 300.0).abs() < 1e-10);
    }

    #[test]
    fn test_raster_image_builder() {
        let image = RasterImageBuilder::new("photo.jpg", 1920.0, 1080.0)
            .at(Vector3::new(10.0, 20.0, 0.0))
            .width(19.2)
            .brightness(70)
            .contrast(60)
            .transparent()
            .build();

        assert_eq!(image.insertion_point, Vector3::new(10.0, 20.0, 0.0));
        assert!((image.width() - 19.2).abs() < 0.01);
        assert_eq!(image.brightness, 70);
        assert_eq!(image.contrast, 60);
        assert!(image.is_transparent());
    }

    #[test]
    fn test_display_flags() {
        let flags = ImageDisplayFlags::SHOW_IMAGE | ImageDisplayFlags::TRANSPARENCY_ON;
        assert!(flags.contains(ImageDisplayFlags::SHOW_IMAGE));
        assert!(flags.contains(ImageDisplayFlags::TRANSPARENCY_ON));
        assert!(!flags.contains(ImageDisplayFlags::USE_CLIPPING_BOUNDARY));
    }

    #[test]
    fn test_clip_mode() {
        assert_eq!(ClipMode::from(0), ClipMode::Outside);
        assert_eq!(ClipMode::from(1), ClipMode::Inside);
    }

    #[test]
    fn test_clip_type() {
        assert_eq!(ClipType::from(1), ClipType::Rectangular);
        assert_eq!(ClipType::from(2), ClipType::Polygonal);
    }

    #[test]
    fn test_resolution_unit() {
        assert_eq!(ResolutionUnit::from(0), ResolutionUnit::None);
        assert_eq!(ResolutionUnit::from(2), ResolutionUnit::Centimeters);
        assert_eq!(ResolutionUnit::from(5), ResolutionUnit::Inches);
    }
}
