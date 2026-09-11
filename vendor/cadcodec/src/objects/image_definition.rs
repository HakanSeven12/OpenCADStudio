//! ImageDefinition object - Raster image definition

use crate::types::Handle;

/// Resolution unit for image definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResolutionUnit {
    /// No units specified
    #[default]
    None = 0,
    /// Centimeters
    Centimeters = 2,
    /// Inches
    Inches = 5,
}

impl ResolutionUnit {
    /// Create from DXF code value
    pub fn from_code(code: i32) -> Self {
        match code {
            2 => ResolutionUnit::Centimeters,
            5 => ResolutionUnit::Inches,
            _ => ResolutionUnit::None,
        }
    }

    /// Convert to DXF code value
    pub fn to_code(self) -> i32 {
        self as i32
    }

    /// Check if units are specified
    pub fn has_units(&self) -> bool {
        !matches!(self, ResolutionUnit::None)
    }
}

/// Image definition reactor
///
/// Links an image definition to an image entity that references it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDefinitionReactor {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle (usually the ImageDefinition)
    pub owner: Handle,
    /// Associated image entity handle
    pub image_handle: Handle,
}

impl ImageDefinitionReactor {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "IMAGEDEF_REACTOR";

    /// Create a new reactor
    pub fn new(image_handle: Handle) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            image_handle,
        }
    }
}

/// Image definition object
///
/// Defines a raster image that can be inserted into the drawing as an
/// image entity. Contains the path to the image file and its properties.
///
/// # DXF Object Type
/// IMAGEDEF
///
/// # Example
/// ```ignore
/// use acadrust::objects::ImageDefinition;
///
/// let mut img_def = ImageDefinition::new("C:\\Images\\photo.jpg");
/// img_def.size_in_pixels = (1024, 768);
/// img_def.pixel_size = (0.01, 0.01); // 0.01 units per pixel
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDefinition {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Image file path (DXF code 1)
    pub file_name: String,
    /// Whether the image file is loaded (DXF code 280)
    pub is_loaded: bool,
    /// Image size in pixels (width, height) (DXF codes 10, 20)
    pub size_in_pixels: (u32, u32),
    /// Pixel size in AutoCAD units (width, height) (DXF codes 11, 21)
    pub pixel_size: (f64, f64),
    /// Resolution unit (DXF code 281)
    pub resolution_unit: ResolutionUnit,
    /// Academic year (internal field, DXF code 290)
    /// Note: This appears in DXF but purpose is unclear
    pub class_version: i32,
}

impl ImageDefinition {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "IMAGEDEF";

    /// Default class version
    pub const DEFAULT_CLASS_VERSION: i32 = 0;

    /// Create a new image definition
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            file_name: file_name.into(),
            is_loaded: false,
            size_in_pixels: (0, 0),
            pixel_size: (0.0, 0.0),
            resolution_unit: ResolutionUnit::None,
            class_version: Self::DEFAULT_CLASS_VERSION,
        }
    }

    /// Create with known dimensions
    pub fn with_dimensions(file_name: impl Into<String>, width_px: u32, height_px: u32) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            file_name: file_name.into(),
            is_loaded: false,
            size_in_pixels: (width_px, height_px),
            pixel_size: (1.0, 1.0),
            resolution_unit: ResolutionUnit::None,
            class_version: Self::DEFAULT_CLASS_VERSION,
        }
    }

    /// Get image width in pixels
    pub fn width_pixels(&self) -> u32 {
        self.size_in_pixels.0
    }

    /// Get image height in pixels
    pub fn height_pixels(&self) -> u32 {
        self.size_in_pixels.1
    }

    /// Set image size in pixels
    pub fn set_size_pixels(&mut self, width: u32, height: u32) {
        self.size_in_pixels = (width, height);
    }

    /// Get the pixel width in drawing units
    pub fn pixel_width(&self) -> f64 {
        self.pixel_size.0
    }

    /// Get the pixel height in drawing units
    pub fn pixel_height(&self) -> f64 {
        self.pixel_size.1
    }

    /// Set the pixel size in drawing units
    pub fn set_pixel_size(&mut self, width: f64, height: f64) {
        self.pixel_size = (width, height);
    }

    /// Calculate the image width in drawing units
    pub fn width_units(&self) -> f64 {
        self.size_in_pixels.0 as f64 * self.pixel_size.0
    }

    /// Calculate the image height in drawing units
    pub fn height_units(&self) -> f64 {
        self.size_in_pixels.1 as f64 * self.pixel_size.1
    }

    /// Get the aspect ratio (width/height)
    pub fn aspect_ratio(&self) -> Option<f64> {
        if self.size_in_pixels.1 == 0 {
            None
        } else {
            Some(self.size_in_pixels.0 as f64 / self.size_in_pixels.1 as f64)
        }
    }

    /// Set resolution from DPI
    pub fn set_resolution_dpi(&mut self, dpi: f64) {
        // 1 inch = 25.4 mm, pixel size in inches
        let pixel_size = 1.0 / dpi;
        self.pixel_size = (pixel_size, pixel_size);
        self.resolution_unit = ResolutionUnit::Inches;
    }

    /// Set resolution from pixels per centimeter
    pub fn set_resolution_ppcm(&mut self, ppcm: f64) {
        let pixel_size = 1.0 / ppcm;
        self.pixel_size = (pixel_size, pixel_size);
        self.resolution_unit = ResolutionUnit::Centimeters;
    }

    /// Get resolution in DPI (if using inches)
    pub fn resolution_dpi(&self) -> Option<f64> {
        match self.resolution_unit {
            ResolutionUnit::Inches if self.pixel_size.0 > 0.0 => Some(1.0 / self.pixel_size.0),
            _ => None,
        }
    }

    /// Check if the file path is relative
    pub fn is_relative_path(&self) -> bool {
        !self.is_absolute_path()
    }

    /// Check if the file path is absolute
    pub fn is_absolute_path(&self) -> bool {
        // Windows: starts with drive letter or UNC
        // Unix: starts with /
        let path = &self.file_name;
        if path.len() >= 2 {
            let bytes = path.as_bytes();
            // Check for drive letter (C:)
            if bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
                return true;
            }
            // Check for UNC path (\\)
            if bytes[0] == b'\\' && bytes[1] == b'\\' {
                return true;
            }
        }
        // Check for Unix absolute path
        path.starts_with('/')
    }

    /// Get just the file name without path
    pub fn file_name_only(&self) -> &str {
        // Find the last path separator
        let path = &self.file_name;
        let last_sep = path.rfind(['/', '\\']);
        match last_sep {
            Some(pos) => &path[pos + 1..],
            None => path,
        }
    }

    /// Get the file extension (without dot)
    pub fn file_extension(&self) -> Option<&str> {
        let name = self.file_name_only();
        let dot_pos = name.rfind('.')?;
        Some(&name[dot_pos + 1..])
    }

    /// Check if this appears to be a supported image format
    pub fn is_supported_format(&self) -> bool {
        match self.file_extension().map(|s| s.to_lowercase()).as_deref() {
            Some("bmp") | Some("jpg") | Some("jpeg") | Some("png") | Some("tif") | Some("tiff")
            | Some("gif") | Some("pcx") | Some("tga") => true,
            _ => false,
        }
    }

    /// Mark the image as loaded
    pub fn set_loaded(&mut self, loaded: bool) {
        self.is_loaded = loaded;
    }

    /// Builder: Set as loaded
    pub fn loaded(mut self) -> Self {
        self.is_loaded = true;
        self
    }

    /// Builder: Set pixel size
    pub fn with_pixel_size(mut self, width: f64, height: f64) -> Self {
        self.pixel_size = (width, height);
        self
    }

    /// Builder: Set size in pixels
    pub fn with_size_pixels(mut self, width: u32, height: u32) -> Self {
        self.size_in_pixels = (width, height);
        self
    }

    /// Builder: Set resolution unit
    pub fn with_resolution_unit(mut self, unit: ResolutionUnit) -> Self {
        self.resolution_unit = unit;
        self
    }
}

impl Default for ImageDefinition {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_definition_creation() {
        let img_def = ImageDefinition::new("C:\\Images\\test.jpg");
        assert_eq!(img_def.file_name, "C:\\Images\\test.jpg");
        assert!(!img_def.is_loaded);
        assert_eq!(img_def.size_in_pixels, (0, 0));
    }

    #[test]
    fn test_image_definition_with_dimensions() {
        let img_def = ImageDefinition::with_dimensions("photo.png", 1920, 1080);
        assert_eq!(img_def.width_pixels(), 1920);
        assert_eq!(img_def.height_pixels(), 1080);
    }

    #[test]
    fn test_image_definition_units() {
        let mut img_def = ImageDefinition::with_dimensions("test.jpg", 100, 50);
        img_def.set_pixel_size(0.1, 0.1);

        assert!((img_def.width_units() - 10.0).abs() < 1e-10);
        assert!((img_def.height_units() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_image_definition_aspect_ratio() {
        let img_def = ImageDefinition::with_dimensions("test.jpg", 1600, 900);
        let ratio = img_def.aspect_ratio().unwrap();
        assert!((ratio - 16.0 / 9.0).abs() < 1e-10);
    }

    #[test]
    fn test_image_definition_dpi() {
        let mut img_def = ImageDefinition::new("test.jpg");
        img_def.set_resolution_dpi(72.0);

        assert_eq!(img_def.resolution_unit, ResolutionUnit::Inches);
        let dpi = img_def.resolution_dpi().unwrap();
        assert!((dpi - 72.0).abs() < 1e-10);
    }

    #[test]
    fn test_image_definition_ppcm() {
        let mut img_def = ImageDefinition::new("test.jpg");
        img_def.set_resolution_ppcm(100.0);

        assert_eq!(img_def.resolution_unit, ResolutionUnit::Centimeters);
        assert!((img_def.pixel_size.0 - 0.01).abs() < 1e-10);
    }

    #[test]
    fn test_resolution_unit() {
        assert_eq!(ResolutionUnit::from_code(0), ResolutionUnit::None);
        assert_eq!(ResolutionUnit::from_code(2), ResolutionUnit::Centimeters);
        assert_eq!(ResolutionUnit::from_code(5), ResolutionUnit::Inches);

        assert!(!ResolutionUnit::None.has_units());
        assert!(ResolutionUnit::Inches.has_units());
    }

    #[test]
    fn test_file_path_analysis() {
        let img_def = ImageDefinition::new("C:\\Images\\photo.jpg");
        assert!(img_def.is_absolute_path());
        assert!(!img_def.is_relative_path());
        assert_eq!(img_def.file_name_only(), "photo.jpg");
        assert_eq!(img_def.file_extension(), Some("jpg"));
        assert!(img_def.is_supported_format());
    }

    #[test]
    fn test_relative_path() {
        let img_def = ImageDefinition::new("images/photo.png");
        assert!(!img_def.is_absolute_path());
        assert!(img_def.is_relative_path());
        assert_eq!(img_def.file_name_only(), "photo.png");
    }

    #[test]
    fn test_unix_path() {
        let img_def = ImageDefinition::new("/home/user/images/photo.tiff");
        assert!(img_def.is_absolute_path());
        assert_eq!(img_def.file_name_only(), "photo.tiff");
        assert_eq!(img_def.file_extension(), Some("tiff"));
    }

    #[test]
    fn test_unc_path() {
        let img_def = ImageDefinition::new("\\\\server\\share\\photo.bmp");
        assert!(img_def.is_absolute_path());
        assert_eq!(img_def.file_name_only(), "photo.bmp");
    }

    #[test]
    fn test_supported_formats() {
        assert!(ImageDefinition::new("test.jpg").is_supported_format());
        assert!(ImageDefinition::new("test.jpeg").is_supported_format());
        assert!(ImageDefinition::new("test.png").is_supported_format());
        assert!(ImageDefinition::new("test.bmp").is_supported_format());
        assert!(ImageDefinition::new("test.tif").is_supported_format());
        assert!(ImageDefinition::new("test.tiff").is_supported_format());
        assert!(ImageDefinition::new("test.gif").is_supported_format());

        assert!(!ImageDefinition::new("test.pdf").is_supported_format());
        assert!(!ImageDefinition::new("test.doc").is_supported_format());
    }

    #[test]
    fn test_builder_pattern() {
        let img_def = ImageDefinition::new("test.jpg")
            .with_size_pixels(800, 600)
            .with_pixel_size(0.05, 0.05)
            .with_resolution_unit(ResolutionUnit::Centimeters)
            .loaded();

        assert_eq!(img_def.size_in_pixels, (800, 600));
        assert_eq!(img_def.pixel_size, (0.05, 0.05));
        assert_eq!(img_def.resolution_unit, ResolutionUnit::Centimeters);
        assert!(img_def.is_loaded);
    }

    #[test]
    fn test_image_definition_reactor() {
        let reactor = ImageDefinitionReactor::new(Handle::from(0xABC));
        assert_eq!(reactor.image_handle, Handle::from(0xABC));
        assert_eq!(ImageDefinitionReactor::OBJECT_TYPE, "IMAGEDEF_REACTOR");
    }
}
