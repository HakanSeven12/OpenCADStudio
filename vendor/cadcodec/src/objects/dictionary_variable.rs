//! DictionaryVariable object implementation.
//!
//! A simple object that stores a named value in a dictionary.

use crate::types::Handle;

// ============================================================================
// DictionaryVariable
// ============================================================================

/// Dictionary variable object.
///
/// A simple object that stores a named value in a dictionary.
/// Used for storing various system and application variables.
///
/// # DXF Information
/// - Object type: DICTIONARYVAR
/// - Subclass marker: DictionaryVariables
/// - DXF codes:
///   - 280: Object schema number
///   - 1: Value string
///
/// # Example
///
/// ```ignore
/// use acadrust::objects::DictionaryVariable;
///
/// let var = DictionaryVariable::new("CTAB", "Model");
/// assert_eq!(var.value, "Model");
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryVariable {
    /// Object handle.
    pub handle: Handle,

    /// Owner handle (the dictionary containing this variable).
    pub owner_handle: Handle,

    /// Object schema number.
    /// DXF code: 280
    pub schema_number: i16,

    /// The variable value.
    /// DXF code: 1
    pub value: String,

    /// Name (optional, typically stored as dictionary key).
    pub name: String,
}

impl DictionaryVariable {
    /// Object type name.
    pub const OBJECT_NAME: &'static str = "DICTIONARYVAR";

    /// Subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "DictionaryVariables";

    /// Creates a new DictionaryVariable with the given name and value.
    pub fn new(name: &str, value: &str) -> Self {
        DictionaryVariable {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            schema_number: 0,
            value: value.to_string(),
            name: name.to_string(),
        }
    }

    /// Creates a DictionaryVariable with just a value (no name).
    pub fn with_value(value: &str) -> Self {
        DictionaryVariable {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            schema_number: 0,
            value: value.to_string(),
            name: String::new(),
        }
    }

    /// Sets the value.
    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
    }

    /// Returns true if the value is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Attempts to parse the value as an integer.
    pub fn as_i64(&self) -> Option<i64> {
        self.value.parse().ok()
    }

    /// Attempts to parse the value as a float.
    pub fn as_f64(&self) -> Option<f64> {
        self.value.parse().ok()
    }

    /// Attempts to parse the value as a boolean.
    ///
    /// Recognizes "0", "1", "true", "false", "yes", "no".
    pub fn as_bool(&self) -> Option<bool> {
        match self.value.to_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        }
    }

    /// Creates a variable with an integer value.
    pub fn from_i64(name: &str, value: i64) -> Self {
        Self::new(name, &value.to_string())
    }

    /// Creates a variable with a float value.
    pub fn from_f64(name: &str, value: f64) -> Self {
        Self::new(name, &value.to_string())
    }

    /// Creates a variable with a boolean value.
    pub fn from_bool(name: &str, value: bool) -> Self {
        Self::new(name, if value { "1" } else { "0" })
    }

    // ========================================================================
    // Common System Variables
    // ========================================================================

    /// CTAB - Current tab/layout name.
    pub fn ctab(layout_name: &str) -> Self {
        Self::new("CTAB", layout_name)
    }

    /// CLAYER - Current layer name.
    pub fn clayer(layer_name: &str) -> Self {
        Self::new("CLAYER", layer_name)
    }

    /// DIMSTYLE - Current dimension style name.
    pub fn dimstyle(style_name: &str) -> Self {
        Self::new("DIMSTYLE", style_name)
    }

    /// TEXTSTYLE - Current text style name.
    pub fn textstyle(style_name: &str) -> Self {
        Self::new("TEXTSTYLE", style_name)
    }

    /// CELTSCALE - Current entity linetype scale.
    pub fn celtscale(scale: f64) -> Self {
        Self::from_f64("CELTSCALE", scale)
    }

    /// CECOLOR - Current entity color.
    pub fn cecolor(color_index: i32) -> Self {
        Self::from_i64("CECOLOR", color_index as i64)
    }
}

impl Default for DictionaryVariable {
    fn default() -> Self {
        Self::with_value("")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionaryvariable_creation() {
        let var = DictionaryVariable::new("CTAB", "Model");
        assert_eq!(var.name, "CTAB");
        assert_eq!(var.value, "Model");
    }

    #[test]
    fn test_with_value() {
        let var = DictionaryVariable::with_value("test_value");
        assert!(var.name.is_empty());
        assert_eq!(var.value, "test_value");
    }

    #[test]
    fn test_set_value() {
        let mut var = DictionaryVariable::new("VAR", "old");
        var.set_value("new");
        assert_eq!(var.value, "new");
    }

    #[test]
    fn test_is_empty() {
        let empty = DictionaryVariable::with_value("");
        assert!(empty.is_empty());

        let full = DictionaryVariable::with_value("value");
        assert!(!full.is_empty());
    }

    #[test]
    fn test_as_i64() {
        let var = DictionaryVariable::with_value("42");
        assert_eq!(var.as_i64(), Some(42));

        let invalid = DictionaryVariable::with_value("not_a_number");
        assert_eq!(invalid.as_i64(), None);
    }

    #[test]
    fn test_as_f64() {
        let var = DictionaryVariable::with_value("3.14159");
        let result = var.as_f64();
        assert!(result.is_some());
        assert!((result.unwrap() - 3.14159).abs() < 1e-5);

        let invalid = DictionaryVariable::with_value("not_a_number");
        assert!(invalid.as_f64().is_none());
    }

    #[test]
    fn test_as_bool() {
        let var1 = DictionaryVariable::with_value("1");
        assert_eq!(var1.as_bool(), Some(true));

        let var_true = DictionaryVariable::with_value("true");
        assert_eq!(var_true.as_bool(), Some(true));

        let var_yes = DictionaryVariable::with_value("YES");
        assert_eq!(var_yes.as_bool(), Some(true));

        let var0 = DictionaryVariable::with_value("0");
        assert_eq!(var0.as_bool(), Some(false));

        let var_false = DictionaryVariable::with_value("false");
        assert_eq!(var_false.as_bool(), Some(false));

        let var_no = DictionaryVariable::with_value("No");
        assert_eq!(var_no.as_bool(), Some(false));

        let invalid = DictionaryVariable::with_value("maybe");
        assert!(invalid.as_bool().is_none());
    }

    #[test]
    fn test_from_i64() {
        let var = DictionaryVariable::from_i64("COUNT", 100);
        assert_eq!(var.name, "COUNT");
        assert_eq!(var.value, "100");
        assert_eq!(var.as_i64(), Some(100));
    }

    #[test]
    fn test_from_f64() {
        let var = DictionaryVariable::from_f64("SCALE", 2.5);
        assert_eq!(var.name, "SCALE");
        assert_eq!(var.as_f64(), Some(2.5));
    }

    #[test]
    fn test_from_bool() {
        let var_true = DictionaryVariable::from_bool("ENABLED", true);
        assert_eq!(var_true.value, "1");

        let var_false = DictionaryVariable::from_bool("DISABLED", false);
        assert_eq!(var_false.value, "0");
    }

    #[test]
    fn test_ctab() {
        let var = DictionaryVariable::ctab("Layout1");
        assert_eq!(var.name, "CTAB");
        assert_eq!(var.value, "Layout1");
    }

    #[test]
    fn test_clayer() {
        let var = DictionaryVariable::clayer("MyLayer");
        assert_eq!(var.name, "CLAYER");
        assert_eq!(var.value, "MyLayer");
    }

    #[test]
    fn test_dimstyle() {
        let var = DictionaryVariable::dimstyle("Standard");
        assert_eq!(var.name, "DIMSTYLE");
        assert_eq!(var.value, "Standard");
    }

    #[test]
    fn test_textstyle() {
        let var = DictionaryVariable::textstyle("Arial");
        assert_eq!(var.name, "TEXTSTYLE");
        assert_eq!(var.value, "Arial");
    }

    #[test]
    fn test_celtscale() {
        let var = DictionaryVariable::celtscale(1.5);
        assert_eq!(var.name, "CELTSCALE");
        assert_eq!(var.as_f64(), Some(1.5));
    }

    #[test]
    fn test_cecolor() {
        let var = DictionaryVariable::cecolor(256);
        assert_eq!(var.name, "CECOLOR");
        assert_eq!(var.as_i64(), Some(256));
    }

    #[test]
    fn test_default() {
        let var = DictionaryVariable::default();
        assert!(var.is_empty());
        assert!(var.name.is_empty());
    }

    #[test]
    fn test_schema_number() {
        let mut var = DictionaryVariable::new("VAR", "value");
        assert_eq!(var.schema_number, 0);
        var.schema_number = 1;
        assert_eq!(var.schema_number, 1);
    }

    #[test]
    fn test_clone() {
        let var = DictionaryVariable::new("CTAB", "Model");
        let cloned = var.clone();

        assert_eq!(var.name, cloned.name);
        assert_eq!(var.value, cloned.value);
    }

    #[test]
    fn test_constants() {
        assert_eq!(DictionaryVariable::OBJECT_NAME, "DICTIONARYVAR");
        assert_eq!(DictionaryVariable::SUBCLASS_MARKER, "DictionaryVariables");
    }
}
