//! Text style table entry

use super::TableEntry;
use crate::types::Handle;

/// Text generation flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextGenerationFlags {
    /// Text is backward (mirrored in X)
    pub backward: bool,
    /// Text is upside down (mirrored in Y)
    pub upside_down: bool,
}

impl TextGenerationFlags {
    /// Create default flags
    pub fn new() -> Self {
        TextGenerationFlags {
            backward: false,
            upside_down: false,
        }
    }
}

impl Default for TextGenerationFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// A text style table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle {
    /// Unique handle
    pub handle: Handle,
    /// Style name
    pub name: String,
    /// Text generation flags
    pub flags: TextGenerationFlags,
    /// Fixed text height (0 = variable)
    pub height: f64,
    /// Width factor
    pub width_factor: f64,
    /// Oblique angle in radians
    pub oblique_angle: f64,
    /// Last height used (code 42, default 2.5)
    pub last_height: f64,
    /// Primary font file name
    pub font_file: String,
    /// Big font file name (for Asian languages)
    pub big_font_file: String,
    /// True Type font name
    pub true_type_font: String,
    /// Whether this style is xref-dependent
    pub xref_dependent: bool,
    /// Whether this STYLE record names an SHX shape file.
    pub is_shape_file: bool,
    /// Whether glyphs are drawn vertically.
    pub is_vertical: bool,
    /// Annotative: entities using this style scale with the annotation scale.
    /// Persisted as XDATA under the `AcadAnnotative` application:
    /// `AnnotativeData { 1 <flag> }`.
    pub annotative: bool,
}

impl TextStyle {
    /// Create a new text style
    pub fn new(name: impl Into<String>) -> Self {
        TextStyle {
            handle: Handle::NULL,
            name: name.into(),
            flags: TextGenerationFlags::new(),
            height: 0.0,
            width_factor: 1.0,
            oblique_angle: 0.0,
            last_height: 2.5,
            font_file: "txt".to_string(),
            big_font_file: String::new(),
            true_type_font: String::new(),
            xref_dependent: false,
            is_shape_file: false,
            is_vertical: false,
            annotative: false,
        }
    }

    /// Create the standard "Standard" text style
    pub fn standard() -> Self {
        TextStyle {
            handle: Handle::NULL,
            name: "Standard".to_string(),
            flags: TextGenerationFlags::new(),
            height: 0.0,
            width_factor: 1.0,
            oblique_angle: 0.0,
            last_height: 2.5,
            font_file: "txt".to_string(),
            big_font_file: String::new(),
            true_type_font: String::new(),
            xref_dependent: false,
            is_shape_file: false,
            is_vertical: false,
            annotative: false,
        }
    }

    /// Create a text style with a TrueType font
    pub fn with_truetype(name: impl Into<String>, font: impl Into<String>) -> Self {
        TextStyle {
            true_type_font: font.into(),
            ..Self::new(name)
        }
    }

    /// Get the effective last height (returns last_height, or default 2.5 if 0)
    pub fn effective_last_height(&self) -> f64 {
        if self.last_height > 0.0 {
            self.last_height
        } else {
            2.5
        }
    }

    /// Set the text as backward (mirrored in X)
    pub fn set_backward(&mut self, backward: bool) {
        self.flags.backward = backward;
    }

    /// Set the text as upside down (mirrored in Y)
    pub fn set_upside_down(&mut self, upside_down: bool) {
        self.flags.upside_down = upside_down;
    }

    /// Check if text is backward
    pub fn is_backward(&self) -> bool {
        self.flags.backward
    }

    /// Check if text is upside down
    pub fn is_upside_down(&self) -> bool {
        self.flags.upside_down
    }

    /// Check if this style has a fixed height
    pub fn has_fixed_height(&self) -> bool {
        self.height > 0.0
    }
}

impl TableEntry for TextStyle {
    fn handle(&self) -> Handle {
        self.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.handle = handle;
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn is_standard(&self) -> bool {
        self.name == "Standard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_textstyle_creation() {
        let style = TextStyle::new("MyStyle");
        assert_eq!(style.name, "MyStyle");
        assert_eq!(style.width_factor, 1.0);
        assert!(!style.has_fixed_height());
    }

    #[test]
    fn test_textstyle_standard() {
        let style = TextStyle::standard();
        assert_eq!(style.name, "Standard");
        assert!(style.is_standard());
    }

    #[test]
    fn test_textstyle_flags() {
        let mut style = TextStyle::new("Test");
        assert!(!style.is_backward());
        assert!(!style.is_upside_down());

        style.set_backward(true);
        assert!(style.is_backward());

        style.set_upside_down(true);
        assert!(style.is_upside_down());
    }
}
