//! Scale object implementation.
//!
//! Defines scales used for dimension annotations and viewport scaling.

use crate::types::Handle;

// ============================================================================
// Scale
// ============================================================================

/// Scale object.
///
/// Defines a named scale for dimension annotations and viewport scaling.
/// Scales are stored in the ACAD_SCALELIST dictionary.
///
/// # DXF Information
/// - Object type: SCALE
/// - Subclass marker: AcDbScale
/// - DXF codes:
///   - 300: Scale name
///   - 140: Paper units value
///   - 141: Drawing units value
///   - 290: Is unit scale flag
///
/// # Example
///
/// ```ignore
/// use acadrust::objects::Scale;
///
/// let scale = Scale::new("1:50", 1.0, 50.0);
/// assert!((scale.factor() - 0.02).abs() < 1e-10);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scale {
    /// Object handle.
    pub handle: Handle,

    /// Owner handle (SCALELIST dictionary).
    pub owner_handle: Handle,

    /// Scale name.
    /// DXF code: 300
    pub name: String,

    /// Paper units value.
    /// The number of paper units for this scale.
    /// DXF code: 140
    pub paper_units: f64,

    /// Drawing units value.
    /// The number of drawing units for this scale.
    /// DXF code: 141
    pub drawing_units: f64,

    /// Whether this is a unit scale (1:1).
    /// DXF code: 290
    pub is_unit_scale: bool,

    /// Whether this scale is temporary (internal use).
    pub is_temporary: bool,
}

impl Scale {
    /// Object type name.
    pub const OBJECT_NAME: &'static str = "SCALE";

    /// Subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbScale";

    /// Creates a new Scale with the given name and values.
    pub fn new(name: &str, paper_units: f64, drawing_units: f64) -> Self {
        let is_unit_scale = (paper_units - drawing_units).abs() < 1e-10;

        Scale {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            name: name.to_string(),
            paper_units,
            drawing_units,
            is_unit_scale,
            is_temporary: false,
        }
    }

    /// Creates a unit scale (1:1).
    pub fn unit_scale() -> Self {
        Scale {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            name: "1:1".to_string(),
            paper_units: 1.0,
            drawing_units: 1.0,
            is_unit_scale: true,
            is_temporary: false,
        }
    }

    /// Creates a scale from a ratio (paper:drawing).
    ///
    /// For example, `from_ratio("1:50", 1, 50)` creates a 1:50 scale.
    pub fn from_ratio(name: &str, paper: i32, drawing: i32) -> Self {
        Self::new(name, paper as f64, drawing as f64)
    }

    /// Calculates the scale factor (paper_units / drawing_units).
    ///
    /// For a 1:50 scale, the factor is 0.02 (1/50).
    /// For a 2:1 scale (enlargement), the factor is 2.0.
    pub fn factor(&self) -> f64 {
        if self.drawing_units.abs() < 1e-10 {
            1.0
        } else {
            self.paper_units / self.drawing_units
        }
    }

    /// Calculates the inverse scale factor (drawing_units / paper_units).
    ///
    /// For a 1:50 scale, the inverse factor is 50.
    /// For a 2:1 scale (enlargement), the inverse factor is 0.5.
    pub fn inverse_factor(&self) -> f64 {
        if self.paper_units.abs() < 1e-10 {
            1.0
        } else {
            self.drawing_units / self.paper_units
        }
    }

    /// Returns true if this is a reduction scale (factor < 1).
    pub fn is_reduction(&self) -> bool {
        self.factor() < 1.0 - 1e-10
    }

    /// Returns true if this is an enlargement scale (factor > 1).
    pub fn is_enlargement(&self) -> bool {
        self.factor() > 1.0 + 1e-10
    }

    /// Returns the ratio as a string (e.g., "1:50" or "2:1").
    pub fn ratio_string(&self) -> String {
        if self.paper_units >= self.drawing_units {
            let ratio = (self.paper_units / self.drawing_units).round() as i64;
            format!("{}:1", ratio)
        } else {
            let ratio = (self.drawing_units / self.paper_units).round() as i64;
            format!("1:{}", ratio)
        }
    }

    // ========================================================================
    // Standard Scales (Metric - Architectural)
    // ========================================================================

    /// 1:1 scale (full size).
    pub fn scale_1_1() -> Self {
        Self::new("1:1", 1.0, 1.0)
    }

    /// 1:2 scale (half size).
    pub fn scale_1_2() -> Self {
        Self::new("1:2", 1.0, 2.0)
    }

    /// 1:4 scale.
    pub fn scale_1_4() -> Self {
        Self::new("1:4", 1.0, 4.0)
    }

    /// 1:5 scale.
    pub fn scale_1_5() -> Self {
        Self::new("1:5", 1.0, 5.0)
    }

    /// 1:8 scale.
    pub fn scale_1_8() -> Self {
        Self::new("1:8", 1.0, 8.0)
    }

    /// 1:10 scale.
    pub fn scale_1_10() -> Self {
        Self::new("1:10", 1.0, 10.0)
    }

    /// 1:16 scale.
    pub fn scale_1_16() -> Self {
        Self::new("1:16", 1.0, 16.0)
    }

    /// 1:20 scale.
    pub fn scale_1_20() -> Self {
        Self::new("1:20", 1.0, 20.0)
    }

    /// 1:30 scale.
    pub fn scale_1_30() -> Self {
        Self::new("1:30", 1.0, 30.0)
    }

    /// 1:40 scale.
    pub fn scale_1_40() -> Self {
        Self::new("1:40", 1.0, 40.0)
    }

    /// 1:50 scale.
    pub fn scale_1_50() -> Self {
        Self::new("1:50", 1.0, 50.0)
    }

    /// 1:100 scale.
    pub fn scale_1_100() -> Self {
        Self::new("1:100", 1.0, 100.0)
    }

    /// 1:128 scale.
    pub fn scale_1_128() -> Self {
        Self::new("1:128", 1.0, 128.0)
    }

    /// 2:1 scale (enlarged).
    pub fn scale_2_1() -> Self {
        Self::new("2:1", 2.0, 1.0)
    }

    /// 4:1 scale (enlarged).
    pub fn scale_4_1() -> Self {
        Self::new("4:1", 4.0, 1.0)
    }

    /// 8:1 scale (enlarged).
    pub fn scale_8_1() -> Self {
        Self::new("8:1", 8.0, 1.0)
    }

    /// 10:1 scale (enlarged).
    pub fn scale_10_1() -> Self {
        Self::new("10:1", 10.0, 1.0)
    }

    /// 100:1 scale (enlarged).
    pub fn scale_100_1() -> Self {
        Self::new("100:1", 100.0, 1.0)
    }

    // ========================================================================
    // Standard Scales (Imperial - Architectural)
    // ========================================================================

    /// 1" = 1' scale (1:12).
    pub fn scale_1in_1ft() -> Self {
        Self::new("1\" = 1'", 1.0, 12.0)
    }

    /// 1/2" = 1' scale (1:24).
    pub fn scale_half_in_1ft() -> Self {
        Self::new("1/2\" = 1'", 0.5, 12.0)
    }

    /// 1/4" = 1' scale (1:48).
    pub fn scale_quarter_in_1ft() -> Self {
        Self::new("1/4\" = 1'", 0.25, 12.0)
    }

    /// 1/8" = 1' scale (1:96).
    pub fn scale_eighth_in_1ft() -> Self {
        Self::new("1/8\" = 1'", 0.125, 12.0)
    }

    /// 3/4" = 1' scale (1:16).
    pub fn scale_3_4in_1ft() -> Self {
        Self::new("3/4\" = 1'", 0.75, 12.0)
    }

    /// 3/8" = 1' scale (1:32).
    pub fn scale_3_8in_1ft() -> Self {
        Self::new("3/8\" = 1'", 0.375, 12.0)
    }

    /// 3/16" = 1' scale (1:64).
    pub fn scale_3_16in_1ft() -> Self {
        Self::new("3/16\" = 1'", 0.1875, 12.0)
    }

    /// 3/32" = 1' scale (1:128).
    pub fn scale_3_32in_1ft() -> Self {
        Self::new("3/32\" = 1'", 0.09375, 12.0)
    }

    /// 1 1/2" = 1' scale (1:8).
    pub fn scale_1_5in_1ft() -> Self {
        Self::new("1 1/2\" = 1'", 1.5, 12.0)
    }

    /// 3" = 1' scale (1:4).
    pub fn scale_3in_1ft() -> Self {
        Self::new("3\" = 1'", 3.0, 12.0)
    }

    /// 6" = 1' scale (1:2, half size).
    pub fn scale_6in_1ft() -> Self {
        Self::new("6\" = 1'", 6.0, 12.0)
    }

    /// Returns all standard metric scales.
    pub fn standard_metric_scales() -> Vec<Scale> {
        vec![
            Self::scale_1_1(),
            Self::scale_1_2(),
            Self::scale_1_4(),
            Self::scale_1_5(),
            Self::scale_1_8(),
            Self::scale_1_10(),
            Self::scale_1_16(),
            Self::scale_1_20(),
            Self::scale_1_30(),
            Self::scale_1_40(),
            Self::scale_1_50(),
            Self::scale_1_100(),
            Self::scale_1_128(),
            Self::scale_2_1(),
            Self::scale_4_1(),
            Self::scale_8_1(),
            Self::scale_10_1(),
            Self::scale_100_1(),
        ]
    }

    /// Returns all standard imperial/architectural scales.
    pub fn standard_imperial_scales() -> Vec<Scale> {
        vec![
            Self::scale_1_1(),
            Self::scale_6in_1ft(),
            Self::scale_3in_1ft(),
            Self::scale_1_5in_1ft(),
            Self::scale_1in_1ft(),
            Self::scale_3_4in_1ft(),
            Self::scale_half_in_1ft(),
            Self::scale_3_8in_1ft(),
            Self::scale_quarter_in_1ft(),
            Self::scale_3_16in_1ft(),
            Self::scale_eighth_in_1ft(),
            Self::scale_3_32in_1ft(),
        ]
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::unit_scale()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_creation() {
        let scale = Scale::new("1:50", 1.0, 50.0);
        assert_eq!(scale.name, "1:50");
        assert_eq!(scale.paper_units, 1.0);
        assert_eq!(scale.drawing_units, 50.0);
        assert!(!scale.is_unit_scale);
    }

    #[test]
    fn test_unit_scale() {
        let scale = Scale::unit_scale();
        assert_eq!(scale.name, "1:1");
        assert!(scale.is_unit_scale);
        assert!((scale.factor() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_from_ratio() {
        let scale = Scale::from_ratio("1:25", 1, 25);
        assert_eq!(scale.paper_units, 1.0);
        assert_eq!(scale.drawing_units, 25.0);
    }

    #[test]
    fn test_factor() {
        let scale_1_50 = Scale::new("1:50", 1.0, 50.0);
        assert!((scale_1_50.factor() - 0.02).abs() < 1e-10);

        let scale_2_1 = Scale::new("2:1", 2.0, 1.0);
        assert!((scale_2_1.factor() - 2.0).abs() < 1e-10);

        let scale_1_1 = Scale::unit_scale();
        assert!((scale_1_1.factor() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_inverse_factor() {
        let scale_1_50 = Scale::new("1:50", 1.0, 50.0);
        assert!((scale_1_50.inverse_factor() - 50.0).abs() < 1e-10);

        let scale_2_1 = Scale::new("2:1", 2.0, 1.0);
        assert!((scale_2_1.inverse_factor() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_is_reduction() {
        let scale = Scale::new("1:50", 1.0, 50.0);
        assert!(scale.is_reduction());
        assert!(!scale.is_enlargement());
    }

    #[test]
    fn test_is_enlargement() {
        let scale = Scale::new("5:1", 5.0, 1.0);
        assert!(scale.is_enlargement());
        assert!(!scale.is_reduction());
    }

    #[test]
    fn test_unit_is_neither() {
        let scale = Scale::unit_scale();
        assert!(!scale.is_reduction());
        assert!(!scale.is_enlargement());
    }

    #[test]
    fn test_ratio_string() {
        let scale_1_50 = Scale::new("1:50", 1.0, 50.0);
        assert_eq!(scale_1_50.ratio_string(), "1:50");

        let scale_2_1 = Scale::new("2:1", 2.0, 1.0);
        assert_eq!(scale_2_1.ratio_string(), "2:1");

        let scale_1_1 = Scale::unit_scale();
        assert_eq!(scale_1_1.ratio_string(), "1:1");
    }

    #[test]
    fn test_zero_drawing_units_factor() {
        let scale = Scale::new("Invalid", 1.0, 0.0);
        assert!((scale.factor() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_zero_paper_units_inverse() {
        let scale = Scale::new("Invalid", 0.0, 1.0);
        assert!((scale.inverse_factor() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_standard_metric_scales() {
        let scales = Scale::standard_metric_scales();
        assert!(scales.len() >= 15);

        // Check 1:1
        assert!(scales.iter().any(|s| s.name == "1:1"));

        // Check 1:50
        assert!(scales.iter().any(|s| s.name == "1:50"));
    }

    #[test]
    fn test_standard_imperial_scales() {
        let scales = Scale::standard_imperial_scales();
        assert!(scales.len() >= 10);

        // Check 1/4" = 1'
        assert!(scales.iter().any(|s| s.name == "1/4\" = 1'"));
    }

    #[test]
    fn test_scale_1_1() {
        let scale = Scale::scale_1_1();
        assert!(scale.is_unit_scale || (scale.factor() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_scale_1_50() {
        let scale = Scale::scale_1_50();
        assert!((scale.factor() - 0.02).abs() < 1e-10);
    }

    #[test]
    fn test_scale_1_100() {
        let scale = Scale::scale_1_100();
        assert!((scale.factor() - 0.01).abs() < 1e-10);
    }

    #[test]
    fn test_scale_2_1() {
        let scale = Scale::scale_2_1();
        assert!((scale.factor() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_imperial_scale_1in_1ft() {
        let scale = Scale::scale_1in_1ft();
        // 1:12
        assert!((scale.factor() - (1.0 / 12.0)).abs() < 1e-10);
    }

    #[test]
    fn test_imperial_scale_quarter_in() {
        let scale = Scale::scale_quarter_in_1ft();
        // 1/4:12 = 1:48
        assert!((scale.factor() - (0.25 / 12.0)).abs() < 1e-10);
    }

    #[test]
    fn test_default() {
        let scale = Scale::default();
        assert_eq!(scale.name, "1:1");
        assert!(scale.is_unit_scale);
    }

    #[test]
    fn test_temporary_flag() {
        let mut scale = Scale::new("Temp", 1.0, 5.0);
        assert!(!scale.is_temporary);

        scale.is_temporary = true;
        assert!(scale.is_temporary);
    }

    #[test]
    fn test_clone() {
        let scale = Scale::new("1:25", 1.0, 25.0);
        let cloned = scale.clone();

        assert_eq!(scale.name, cloned.name);
        assert_eq!(scale.paper_units, cloned.paper_units);
        assert_eq!(scale.drawing_units, cloned.drawing_units);
    }
}
