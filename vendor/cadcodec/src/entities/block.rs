//! Block entity - marks the beginning of a block definition

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Block entity - marks the beginning of a block definition
///
/// A Block entity is the beginning marker for a block definition in the BLOCKS section.
/// It contains the block's base point and other properties.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Block {
    pub common: EntityCommon,
    /// Block name (should match the BlockRecord name)
    pub name: String,
    /// Base point (insertion point) for the block
    pub base_point: Vector3,
    /// Block description
    pub description: String,
    /// X-ref path name (for external references)
    pub xref_path: String,
}

impl Block {
    /// Create a new block entity
    pub fn new(name: impl Into<String>, base_point: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            name: name.into(),
            base_point,
            description: String::new(),
            xref_path: String::new(),
        }
    }

    /// Builder: Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Builder: Set the xref path
    pub fn with_xref_path(mut self, xref_path: impl Into<String>) -> Self {
        self.xref_path = xref_path.into();
        self
    }
}

/// BlockEnd entity - marks the end of a block definition
///
/// A BlockEnd entity is the ending marker for a block definition in the BLOCKS section.
/// It has no additional properties beyond the common entity properties.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEnd {
    pub common: EntityCommon,
}

impl BlockEnd {
    /// Create a new block end entity
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
        }
    }
}

impl Default for BlockEnd {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Entity trait for Block
impl Entity for Block {
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
        BoundingBox3D::from_point(self.base_point)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_block(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "BLOCK"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_block(self, transform);
    }
}

// Implement Entity trait for BlockEnd
impl Entity for BlockEnd {
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
        // BlockEnd has no geometry, return a zero-size box at origin
        BoundingBox3D::from_point(Vector3::new(0.0, 0.0, 0.0))
    }

    fn translate(&mut self, _offset: Vector3) {
        super::translate::translate_block_end(self, _offset);
    }

    fn entity_type(&self) -> &'static str {
        "ENDBLK"
    }

    fn apply_transform(&mut self, _transform: &crate::types::Transform) {
        super::transform::transform_block_end(self, _transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_creation() {
        let block = Block::new("MyBlock", Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(block.name, "MyBlock");
        assert_eq!(block.base_point, Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(block.description, "");
    }

    #[test]
    fn test_block_with_description() {
        let block =
            Block::new("MyBlock", Vector3::new(0.0, 0.0, 0.0)).with_description("Test block");
        assert_eq!(block.description, "Test block");
    }

    #[test]
    fn test_block_end_creation() {
        let block_end = BlockEnd::new();
        assert!(block_end.common.handle.is_null());
    }
}
