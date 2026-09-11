//! Tolerance (Feature Control Frame) entity implementation.
//!
//! The Tolerance entity represents a geometric dimensioning and tolerancing
//! (GD&T) feature control frame annotation.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

// ============================================================================
// Tolerance Entity
// ============================================================================

/// Tolerance entity (Feature Control Frame).
///
/// Represents a geometric dimensioning and tolerancing (GD&T) feature
/// control frame annotation used in technical drawings.
///
/// # DXF Information
/// - Entity type: TOLERANCE
/// - Subclass marker: AcDbFcf
/// - Object type code: 0x2E (46)
///
/// # Text Format
///
/// The tolerance text uses a special format for GDT symbols:
/// - `{\Fgdt;X}` - Font switch to GDT font with symbol X
///   - `j` - Perpendicularity
///   - `n` - Angularity
///   - `s` - Symmetry
///   - `p` - Position
/// - `%%v` - Special character/separator
/// - `^J` - Line separator (newline within frame)
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::Tolerance;
/// use acadrust::types::Vector3;
///
/// let mut tol = Tolerance::new();
/// tol.insertion_point = Vector3::new(10.0, 10.0, 0.0);
/// tol.text = "{\\Fgdt;p}%%v0.5%%v%%v%%v%%v".to_string();
/// tol.dimension_style_name = "Standard".to_string();
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tolerance {
    /// Common entity data.
    pub common: EntityCommon,

    /// Insertion point in WCS.
    /// DXF codes: 10, 20, 30
    pub insertion_point: Vector3,

    /// X-axis direction vector in WCS.
    /// DXF codes: 11, 21, 31
    pub direction: Vector3,

    /// Extrusion/normal vector.
    /// DXF codes: 210, 220, 230
    /// Default: (0, 0, 1)
    pub normal: Vector3,

    /// Tolerance text string (feature control frame content).
    /// DXF code: 1
    /// Uses special GDT font formatting.
    pub text: String,

    /// Dimension style name.
    /// DXF code: 3 (name reference)
    pub dimension_style_name: String,

    /// Dimension style handle (for DWG).
    pub dimension_style_handle: Option<Handle>,

    /// Text height (from dimension style or override).
    pub text_height: f64,

    /// Dimension gap (from dimension style).
    pub dimension_gap: f64,

    /// Undocumented R13/R14 value preserved for DWG round-trip.
    pub dwg_unknown_short: i16,
}

impl Tolerance {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "TOLERANCE";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbFcf";

    /// Object type code (DWG).
    pub const OBJECT_TYPE: u16 = 0x2E; // 46

    /// Creates a new tolerance entity with default values.
    pub fn new() -> Self {
        Tolerance {
            common: EntityCommon::default(),
            insertion_point: Vector3::ZERO,
            direction: Vector3::UNIT_X,
            normal: Vector3::UNIT_Z,
            text: String::new(),
            dimension_style_name: "Standard".to_string(),
            dimension_style_handle: None,
            text_height: 0.18,
            dimension_gap: 0.09,
            dwg_unknown_short: 0,
        }
    }

    /// Creates a tolerance with insertion point and text.
    pub fn with_text(insertion_point: Vector3, text: impl Into<String>) -> Self {
        Tolerance {
            insertion_point,
            text: text.into(),
            ..Self::new()
        }
    }

    /// Creates a tolerance with full parameters.
    pub fn new_full(
        insertion_point: Vector3,
        text: impl Into<String>,
        direction: Vector3,
        style: impl Into<String>,
    ) -> Self {
        Tolerance {
            insertion_point,
            text: text.into(),
            direction: direction.normalize(),
            dimension_style_name: style.into(),
            ..Self::new()
        }
    }

    /// Sets the direction to point toward the given location.
    pub fn point_toward(&mut self, target: Vector3) {
        let dir = target - self.insertion_point;
        if dir.length() > 1e-10 {
            self.direction = dir.normalize();
        }
    }

    /// Rotates the tolerance frame around its normal.
    pub fn rotate(&mut self, angle: f64) {
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let n = self.normal.normalize();

        // Rodrigues' rotation formula for direction vector
        let d = self.direction;
        let dot = n.x * d.x + n.y * d.y + n.z * d.z;
        let cross = Vector3::new(
            n.y * d.z - n.z * d.y,
            n.z * d.x - n.x * d.z,
            n.x * d.y - n.y * d.x,
        );

        self.direction = Vector3::new(
            d.x * cos_a + cross.x * sin_a + n.x * dot * (1.0 - cos_a),
            d.y * cos_a + cross.y * sin_a + n.y * dot * (1.0 - cos_a),
            d.z * cos_a + cross.z * sin_a + n.z * dot * (1.0 - cos_a),
        );
    }

    /// Returns the rotation angle around the normal (in radians).
    pub fn rotation_angle(&self) -> f64 {
        let n = self.normal.normalize();
        let d = self.direction.normalize();

        // Project direction onto the plane perpendicular to normal
        let dot = n.x * d.x + n.y * d.y + n.z * d.z;
        let projected = Vector3::new(d.x - n.x * dot, d.y - n.y * dot, d.z - n.z * dot);

        if projected.length() < 1e-10 {
            return 0.0;
        }

        // Calculate angle from X axis in the plane
        let x_axis = if (n.z.abs() - 1.0).abs() < 1e-10 {
            Vector3::UNIT_X
        } else {
            Vector3::new(-n.y, n.x, 0.0).normalize()
        };

        let y_axis = Vector3::new(
            n.y * x_axis.z - n.z * x_axis.y,
            n.z * x_axis.x - n.x * x_axis.z,
            n.x * x_axis.y - n.y * x_axis.x,
        );

        let px = projected.x * x_axis.x + projected.y * x_axis.y + projected.z * x_axis.z;
        let py = projected.x * y_axis.x + projected.y * y_axis.y + projected.z * y_axis.z;

        py.atan2(px)
    }

    /// Parses the tolerance text and returns the lines.
    pub fn text_lines(&self) -> Vec<&str> {
        self.text.split("^J").collect()
    }

    /// Returns the number of lines in the tolerance frame.
    pub fn line_count(&self) -> usize {
        self.text.matches("^J").count() + 1
    }

    /// Creates a simple position tolerance string.
    ///
    /// # Arguments
    /// * `tolerance` - The tolerance value (e.g., 0.5)
    /// * `datum_a` - Optional first datum reference
    /// * `datum_b` - Optional second datum reference
    /// * `datum_c` - Optional third datum reference
    pub fn position_tolerance(
        tolerance: f64,
        datum_a: Option<&str>,
        datum_b: Option<&str>,
        datum_c: Option<&str>,
    ) -> String {
        let mut text = format!("{{\\Fgdt;p}}%%v{}%%v", tolerance);

        if let Some(a) = datum_a {
            text.push_str(a);
        }
        text.push_str("%%v");

        if let Some(b) = datum_b {
            text.push_str(b);
        }
        text.push_str("%%v");

        if let Some(c) = datum_c {
            text.push_str(c);
        }
        text.push_str("%%v");

        text
    }

    /// Creates a flatness tolerance string.
    pub fn flatness_tolerance(tolerance: f64) -> String {
        format!("{{\\Fgdt;c}}%%v{}%%v%%v%%v%%v", tolerance)
    }

    /// Creates a perpendicularity tolerance string.
    pub fn perpendicularity_tolerance(tolerance: f64, datum: &str) -> String {
        format!("{{\\Fgdt;j}}%%v{}%%v{}%%v%%v%%v", tolerance, datum)
    }

    /// Creates a parallelism tolerance string.
    pub fn parallelism_tolerance(tolerance: f64, datum: &str) -> String {
        format!("{{\\Fgdt;h}}%%v{}%%v{}%%v%%v%%v", tolerance, datum)
    }

    /// Creates a concentricity tolerance string.
    pub fn concentricity_tolerance(tolerance: f64, datum: &str) -> String {
        format!("{{\\Fgdt;u}}%%v{}%%v{}%%v%%v%%v", tolerance, datum)
    }

    /// Creates a symmetry tolerance string.
    pub fn symmetry_tolerance(tolerance: f64, datum: &str) -> String {
        format!("{{\\Fgdt;i}}%%v{}%%v{}%%v%%v%%v", tolerance, datum)
    }

    /// Creates a runout tolerance string.
    pub fn runout_tolerance(tolerance: f64, datum: &str, total: bool) -> String {
        let symbol = if total { "t" } else { "r" };
        format!("{{\\Fgdt;{}}}%%v{}%%v{}%%v%%v%%v", symbol, tolerance, datum)
    }

    /// Creates a cylindricity tolerance string.
    pub fn cylindricity_tolerance(tolerance: f64) -> String {
        format!("{{\\Fgdt;e}}%%v{}%%v%%v%%v%%v", tolerance)
    }

    /// Creates a straightness tolerance string.
    pub fn straightness_tolerance(tolerance: f64) -> String {
        format!("{{\\Fgdt;a}}%%v{}%%v%%v%%v%%v", tolerance)
    }

    /// Creates a circularity/roundness tolerance string.
    pub fn circularity_tolerance(tolerance: f64) -> String {
        format!("{{\\Fgdt;g}}%%v{}%%v%%v%%v%%v", tolerance)
    }

    /// Creates a profile of a line tolerance string.
    pub fn line_profile_tolerance(tolerance: f64, datum: Option<&str>) -> String {
        let datum_str = datum.unwrap_or("");
        format!("{{\\Fgdt;k}}%%v{}%%v{}%%v%%v%%v", tolerance, datum_str)
    }

    /// Creates a profile of a surface tolerance string.
    pub fn surface_profile_tolerance(tolerance: f64, datum: Option<&str>) -> String {
        let datum_str = datum.unwrap_or("");
        format!("{{\\Fgdt;d}}%%v{}%%v{}%%v%%v%%v", tolerance, datum_str)
    }

    /// Creates a multi-line tolerance by joining lines with ^J.
    pub fn multi_line(lines: &[&str]) -> String {
        lines.join("^J")
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Tolerance {
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
        // Approximate bounding box - would need font metrics for accuracy
        let half_width = self.text.len() as f64 * self.text_height * 0.6;
        let half_height = self.text_height * self.line_count() as f64;

        let min = Vector3::new(
            self.insertion_point.x - half_width * 0.1,
            self.insertion_point.y - half_height * 0.5,
            self.insertion_point.z,
        );
        let max = Vector3::new(
            self.insertion_point.x + half_width,
            self.insertion_point.y + half_height * 0.5,
            self.insertion_point.z,
        );

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_tolerance(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        Self::ENTITY_NAME
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_tolerance(self, transform);
    }
}

// ============================================================================
// GDT Symbol Constants
// ============================================================================

/// GDT (Geometric Dimensioning and Tolerancing) symbol codes.
///
/// These are used with the `{\Fgdt;X}` format in tolerance text.
pub mod gdt_symbols {
    /// Straightness symbol.
    pub const STRAIGHTNESS: char = 'a';
    /// Flatness symbol.
    pub const FLATNESS: char = 'c';
    /// Circularity/Roundness symbol.
    pub const CIRCULARITY: char = 'g';
    /// Cylindricity symbol.
    pub const CYLINDRICITY: char = 'e';
    /// Profile of a line symbol.
    pub const LINE_PROFILE: char = 'k';
    /// Profile of a surface symbol.
    pub const SURFACE_PROFILE: char = 'd';
    /// Parallelism symbol.
    pub const PARALLELISM: char = 'h';
    /// Perpendicularity symbol.
    pub const PERPENDICULARITY: char = 'j';
    /// Angularity symbol.
    pub const ANGULARITY: char = 'n';
    /// Position symbol.
    pub const POSITION: char = 'p';
    /// Concentricity symbol.
    pub const CONCENTRICITY: char = 'u';
    /// Symmetry symbol.
    pub const SYMMETRY: char = 'i';
    /// Circular runout symbol.
    pub const CIRCULAR_RUNOUT: char = 'r';
    /// Total runout symbol.
    pub const TOTAL_RUNOUT: char = 't';
    /// Diameter symbol.
    pub const DIAMETER: char = 'n';
    /// MMC (Maximum Material Condition) symbol.
    pub const MMC: char = 'm';
    /// LMC (Least Material Condition) symbol.
    pub const LMC: char = 'l';
    /// RFS (Regardless of Feature Size) symbol.
    pub const RFS: char = 's';
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tolerance_creation() {
        let tol = Tolerance::new();
        assert_eq!(tol.insertion_point, Vector3::ZERO);
        assert_eq!(tol.direction, Vector3::UNIT_X);
        assert_eq!(tol.normal, Vector3::UNIT_Z);
        assert!(tol.text.is_empty());
        assert_eq!(tol.dimension_style_name, "Standard");
    }

    #[test]
    fn test_tolerance_with_text() {
        let tol = Tolerance::with_text(Vector3::new(10.0, 20.0, 0.0), "test");
        assert_eq!(tol.insertion_point.x, 10.0);
        assert_eq!(tol.insertion_point.y, 20.0);
        assert_eq!(tol.text, "test");
    }

    #[test]
    fn test_tolerance_full() {
        let tol = Tolerance::new_full(
            Vector3::new(5.0, 5.0, 0.0),
            "tolerance text",
            Vector3::UNIT_Y,
            "DIMSTYLE1",
        );
        assert_eq!(tol.insertion_point.x, 5.0);
        assert_eq!(tol.text, "tolerance text");
        assert!((tol.direction.y - 1.0).abs() < 1e-10);
        assert_eq!(tol.dimension_style_name, "DIMSTYLE1");
    }

    #[test]
    fn test_tolerance_translate() {
        let mut tol = Tolerance::with_text(Vector3::new(0.0, 0.0, 0.0), "test");
        tol.translate(Vector3::new(5.0, 10.0, 0.0));
        assert_eq!(tol.insertion_point.x, 5.0);
        assert_eq!(tol.insertion_point.y, 10.0);
    }

    #[test]
    fn test_tolerance_entity_type() {
        let tol = Tolerance::new();
        assert_eq!(tol.entity_type(), "TOLERANCE");
    }

    #[test]
    fn test_position_tolerance() {
        let text = Tolerance::position_tolerance(0.5, Some("A"), Some("B"), None);
        assert!(text.contains("p"));
        assert!(text.contains("0.5"));
        assert!(text.contains("A"));
        assert!(text.contains("B"));
    }

    #[test]
    fn test_flatness_tolerance() {
        let text = Tolerance::flatness_tolerance(0.1);
        assert!(text.contains("c"));
        assert!(text.contains("0.1"));
    }

    #[test]
    fn test_perpendicularity_tolerance() {
        let text = Tolerance::perpendicularity_tolerance(0.05, "A");
        assert!(text.contains("j"));
        assert!(text.contains("0.05"));
        assert!(text.contains("A"));
    }

    #[test]
    fn test_multi_line() {
        let text = Tolerance::multi_line(&["line1", "line2", "line3"]);
        assert_eq!(text, "line1^Jline2^Jline3");
    }

    #[test]
    fn test_text_lines() {
        let mut tol = Tolerance::new();
        tol.text = "line1^Jline2^Jline3".to_string();
        let lines = tol.text_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
        assert_eq!(lines[2], "line3");
    }

    #[test]
    fn test_line_count() {
        let mut tol = Tolerance::new();
        tol.text = "line1".to_string();
        assert_eq!(tol.line_count(), 1);

        tol.text = "line1^Jline2".to_string();
        assert_eq!(tol.line_count(), 2);

        tol.text = "line1^Jline2^Jline3".to_string();
        assert_eq!(tol.line_count(), 3);
    }

    #[test]
    fn test_point_toward() {
        let mut tol = Tolerance::new();
        tol.insertion_point = Vector3::new(0.0, 0.0, 0.0);
        tol.point_toward(Vector3::new(0.0, 10.0, 0.0));
        assert!((tol.direction.y - 1.0).abs() < 1e-10);
        assert!(tol.direction.x.abs() < 1e-10);
    }

    #[test]
    fn test_bounding_box() {
        let tol = Tolerance::with_text(Vector3::new(10.0, 10.0, 0.0), "test text");
        let bb = tol.bounding_box();
        assert!(bb.min.x <= 10.0);
        assert!(bb.max.x >= 10.0);
    }

    #[test]
    fn test_cylindricity_tolerance() {
        let text = Tolerance::cylindricity_tolerance(0.02);
        assert!(text.contains("e"));
        assert!(text.contains("0.02"));
    }

    #[test]
    fn test_runout_tolerance() {
        let circular = Tolerance::runout_tolerance(0.1, "A", false);
        assert!(circular.contains("r"));

        let total = Tolerance::runout_tolerance(0.1, "A", true);
        assert!(total.contains("t"));
    }
}
