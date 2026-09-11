//! Line type table entry

use super::TableEntry;
use crate::types::Handle;

/// Line type element (dash, dot, space)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineTypeElement {
    /// Length of the element (positive = dash, negative = space, 0 = dot)
    pub length: f64,
    /// Complex data for shape or text display (codes 74-75, 9, 44-46, 50, 340)
    pub complex: Option<LineTypeComplexData>,
}

impl LineTypeElement {
    /// Create a dash element
    pub fn dash(length: f64) -> Self {
        LineTypeElement {
            length: length.abs(),
            complex: None,
        }
    }

    /// Create a space element
    pub fn space(length: f64) -> Self {
        LineTypeElement {
            length: -length.abs(),
            complex: None,
        }
    }

    /// Create a dot element
    pub fn dot() -> Self {
        LineTypeElement {
            length: 0.0,
            complex: None,
        }
    }

    /// Check if this is a dash
    pub fn is_dash(&self) -> bool {
        self.length > 0.0
    }

    /// Check if this is a space
    pub fn is_space(&self) -> bool {
        self.length < 0.0
    }

    /// Check if this is a dot
    pub fn is_dot(&self) -> bool {
        self.length == 0.0
    }
}

/// A line type table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineType {
    /// Unique handle
    pub handle: Handle,
    /// Line type name
    pub name: String,
    /// Description
    pub description: String,
    /// Pattern elements
    pub elements: Vec<LineTypeElement>,
    /// Total pattern length
    pub pattern_length: f64,
    /// Alignment (always 'A' for AutoCAD)
    pub alignment: char,
    /// Whether this linetype is externally dependent on an xref
    pub xref_dependent: bool,
}

impl LineType {
    /// Create a new line type
    pub fn new(name: impl Into<String>) -> Self {
        LineType {
            handle: Handle::NULL,
            name: name.into(),
            description: String::new(),
            elements: Vec::new(),
            pattern_length: 0.0,
            alignment: 'A',
            xref_dependent: false,
        }
    }

    /// Create the standard "Continuous" line type
    pub fn continuous() -> Self {
        LineType {
            handle: Handle::NULL,
            name: "Continuous".to_string(),
            description: "Solid line".to_string(),
            elements: Vec::new(),
            pattern_length: 0.0,
            alignment: 'A',
            xref_dependent: false,
        }
    }

    /// Create the standard "ByLayer" line type
    pub fn by_layer() -> Self {
        LineType {
            handle: Handle::NULL,
            name: "ByLayer".to_string(),
            description: String::new(),
            elements: Vec::new(),
            pattern_length: 0.0,
            alignment: 'A',
            xref_dependent: false,
        }
    }

    /// Create the standard "ByBlock" line type
    pub fn by_block() -> Self {
        LineType {
            handle: Handle::NULL,
            name: "ByBlock".to_string(),
            description: String::new(),
            elements: Vec::new(),
            pattern_length: 0.0,
            alignment: 'A',
            xref_dependent: false,
        }
    }

    /// Create a dashed line type
    pub fn dashed() -> Self {
        let mut lt = LineType::new("Dashed");
        lt.description = "__ __ __ __ __ __".to_string();
        lt.add_element(LineTypeElement::dash(0.5));
        lt.add_element(LineTypeElement::space(0.25));
        lt.pattern_length = 0.75;
        lt
    }

    /// Create a dotted line type
    pub fn dotted() -> Self {
        let mut lt = LineType::new("Dotted");
        lt.description = ". . . . . . . .".to_string();
        lt.add_element(LineTypeElement::dot());
        lt.add_element(LineTypeElement::space(0.25));
        lt.pattern_length = 0.25;
        lt
    }

    /// Add an element to the pattern
    pub fn add_element(&mut self, element: LineTypeElement) {
        self.elements.push(element);
    }

    /// Get the number of elements
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Check if this is a continuous line type
    pub fn is_continuous(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns `true` if any element carries an embedded shape or text glyph.
    pub fn is_complex(&self) -> bool {
        self.elements.iter().any(|e| e.complex.is_some())
    }
}

impl TableEntry for LineType {
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
        matches!(self.name.as_str(), "Continuous" | "ByLayer" | "ByBlock")
    }
}

/// The kind of content a complex linetype segment renders.
/// This replaces the raw flag bits so the model describes **what** the segment
/// is rather than how DWG/DXF encodes it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LineTypeComplexContent {
    /// An embedded shape glyph from a .SHP file.
    Shape { shape_number: i16 },
    /// An embedded text string drawn along the segment.
    Text { text: String },
}

/// Complex linetype data for segments that display a shape or text instead of
/// a dash/dot. Parsed from DXF codes 9, 44-46, 50, 74-75, 340 and DWG segment
/// text area per OpenDesign spec §20.4.58.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineTypeComplexData {
    /// What this segment renders — shape glyph or text string.
    pub content: LineTypeComplexContent,
    /// Text style / shape file handle (DXF 340 / DWG shape-file hard pointer).
    pub style_handle: Handle,
    /// Shape/text scale factor.
    pub scale: f64,
    /// Rotation angle in radians (DXF code 50 is degrees on disk; converted on
    /// read/write). Relative to the line tangent unless `rotation_absolute`.
    pub rotation: f64,
    /// Rotation is world-absolute rather than relative to the line tangent.
    pub absolute_rotation: bool,
    /// Offset from the element's position on the line (DXF 44, 45):
    /// `[along-line, perpendicular]` in drawing units.
    pub offset: [f64; 2],
}

impl Default for LineTypeComplexData {
    fn default() -> Self {
        Self {
            content: LineTypeComplexContent::Shape { shape_number: 0 },
            style_handle: Handle::NULL,
            scale: 1.0,
            rotation: 0.0,
            absolute_rotation: false,
            offset: [0.0, 0.0],
        }
    }
}

impl LineTypeComplexData {
    /// Returns `true` when this element carries an embedded shape glyph.
    #[inline]
    pub fn is_shape(&self) -> bool {
        matches!(self.content, LineTypeComplexContent::Shape { .. })
    }

    /// Returns `true` when this element carries an embedded text string.
    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(self.content, LineTypeComplexContent::Text { .. })
    }

    /// Returns `true` when the rotation is world-absolute.
    #[inline]
    pub fn is_absolute_rotation(&self) -> bool {
        self.absolute_rotation
    }

    /// Shape number, if this is a shape element.
    #[inline]
    pub fn shape_number(&self) -> Option<i16> {
        match self.content {
            LineTypeComplexContent::Shape { shape_number } => Some(shape_number),
            LineTypeComplexContent::Text { .. } => None,
        }
    }

    /// Text string, if this is a text element.
    #[inline]
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            LineTypeComplexContent::Text { text } => Some(text),
            LineTypeComplexContent::Shape { .. } => None,
        }
    }

    // -- helpers for incremental DXF parsing --

    /// Ensure the content variant is `Shape`, switching if necessary.
    pub(crate) fn ensure_shape(&mut self) {
        if !matches!(self.content, LineTypeComplexContent::Shape { .. }) {
            self.content = LineTypeComplexContent::Shape { shape_number: 0 };
        }
    }

    /// Ensure the content variant is `Text`, switching if necessary.
    pub(crate) fn ensure_text(&mut self) {
        if !matches!(self.content, LineTypeComplexContent::Text { .. }) {
            self.content = LineTypeComplexContent::Text {
                text: String::new(),
            };
        }
    }

    /// Set the shape number (used by DXF reader for code 74).
    pub(crate) fn set_shape_number(&mut self, n: i16) {
        self.ensure_shape();
        if let LineTypeComplexContent::Shape { shape_number } = &mut self.content {
            *shape_number = n;
        }
    }

    /// Set the text string (used by DXF reader for code 9).
    pub(crate) fn set_text(&mut self, text: String) {
        self.ensure_text();
        if let LineTypeComplexContent::Text { text: t } = &mut self.content {
            *t = text;
        }
    }

    /// Classify content from DXF element-type flags (code 75).
    pub(crate) fn apply_dxf_flags(&mut self, flags: i16) {
        self.absolute_rotation = flags & 0x01 != 0;
        // DXF: 0x04 = shape, 0x02 = text
        if flags & 0x04 != 0 {
            self.ensure_shape();
        } else if flags & 0x02 != 0 {
            self.ensure_text();
        }
    }
}

impl LineTypeElement {
    /// Return mutable reference to complex data, initializing if absent.
    pub fn complex_mut(&mut self) -> &mut LineTypeComplexData {
        self.complex
            .get_or_insert_with(LineTypeComplexData::default)
    }

    /// Check if this element displays a shape.
    pub fn is_shape(&self) -> bool {
        self.complex.as_ref().map_or(false, |c| c.is_shape())
    }

    /// Check if this element displays text.
    pub fn is_text(&self) -> bool {
        self.complex.as_ref().map_or(false, |c| c.is_text())
    }

    /// Check if the rotation is absolute (bit 0 of flags).
    pub fn is_absolute_rotation(&self) -> bool {
        self.complex
            .as_ref()
            .map_or(false, |c| c.is_absolute_rotation())
    }

    /// Check if this element has complex data.
    pub fn is_complex(&self) -> bool {
        self.complex.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complex_data_shape() {
        let c = LineTypeComplexData {
            content: LineTypeComplexContent::Shape { shape_number: 42 },
            style_handle: Handle::new(100),
            scale: 2.0,
            rotation: 45.0,
            absolute_rotation: false,
            offset: [1.0, 2.0],
        };
        assert!(c.is_shape());
        assert!(!c.is_text());
        assert!(!c.is_absolute_rotation());
        assert_eq!(c.shape_number(), Some(42));
        assert_eq!(c.text(), None);
    }

    #[test]
    fn test_complex_data_text() {
        let c = LineTypeComplexData {
            content: LineTypeComplexContent::Text {
                text: "X".to_string(),
            },
            style_handle: Handle::new(50),
            scale: 1.0,
            rotation: 0.0,
            absolute_rotation: false,
            offset: [0.0, 0.5],
        };
        assert!(!c.is_shape());
        assert!(c.is_text());
        assert!(!c.is_absolute_rotation());
        assert_eq!(c.shape_number(), None);
        assert_eq!(c.text(), Some("X"));
    }

    #[test]
    fn test_complex_data_absolute_rotation() {
        let c = LineTypeComplexData {
            content: LineTypeComplexContent::Shape { shape_number: 1 },
            style_handle: Handle::NULL,
            scale: 1.0,
            rotation: 90.0,
            absolute_rotation: true,
            offset: [0.0, 0.0],
        };
        assert!(c.is_shape());
        assert!(c.is_absolute_rotation());
    }

    #[test]
    fn test_element_complex_methods() {
        let mut elem = LineTypeElement::dash(1.0);
        assert!(!elem.is_complex());
        assert!(!elem.is_shape());
        assert!(!elem.is_text());

        elem.complex = Some(LineTypeComplexData {
            content: LineTypeComplexContent::Shape { shape_number: 5 },
            ..Default::default()
        });
        assert!(elem.is_complex());
        assert!(elem.is_shape());
        assert!(!elem.is_text());
    }

    #[test]
    fn test_linetype_is_complex() {
        let mut lt = LineType::continuous();
        assert!(!lt.is_complex());

        let mut elem = LineTypeElement::dot();
        elem.complex = Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text {
                text: "A".to_string(),
            },
            ..Default::default()
        });
        lt.elements.push(elem);
        assert!(lt.is_complex());
    }

    #[test]
    fn test_complex_default() {
        let c = LineTypeComplexData::default();
        assert!(c.is_shape());
        assert_eq!(c.shape_number(), Some(0));
        assert!(c.style_handle.is_null());
        assert!(!c.is_text());
        assert!(!c.is_absolute_rotation());
    }

    #[test]
    fn test_dxf_flag_helpers() {
        let mut c = LineTypeComplexData::default();
        c.apply_dxf_flags(0x04);
        assert!(c.is_shape());
        assert!(!c.is_text());
        c.apply_dxf_flags(0x02 | 0x01);
        assert!(c.is_text());
        assert!(c.is_absolute_rotation());
    }

    #[test]
    fn test_ensure_shape_switches_content() {
        let mut c = LineTypeComplexData {
            content: LineTypeComplexContent::Text {
                text: "hi".to_string(),
            },
            ..Default::default()
        };
        c.ensure_shape();
        assert!(c.is_shape());
    }

    #[test]
    fn test_ensure_text_switches_content() {
        let mut c = LineTypeComplexData::default();
        c.ensure_text();
        assert!(c.is_text());
        assert_eq!(c.text(), Some(""));
    }

    #[test]
    fn test_element_constructors_no_complex() {
        assert!(!LineTypeElement::dash(1.0).is_complex());
        assert!(!LineTypeElement::space(0.5).is_complex());
        assert!(!LineTypeElement::dot().is_complex());
    }
}
