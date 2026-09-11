//! Seqend entity — end-of-sequence marker for polyline vertices and insert attributes

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// Marker entity signaling the end of a vertex or attribute sequence.
///
/// Seqend has no geometry or entity-specific data. It only carries
/// the inherited common entity fields (handle, owner, layer, etc.).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Seqend {
    /// Common entity data
    pub common: EntityCommon,
}

impl Seqend {
    /// Create a new Seqend marker
    pub fn new() -> Self {
        Seqend {
            common: EntityCommon::new(),
        }
    }
}

impl Default for Seqend {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Seqend {
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
        BoundingBox3D::from_point(Vector3::ZERO)
    }
    fn translate(&mut self, _offset: Vector3) {
        super::translate::translate_seqend(self, _offset);
    }
    fn entity_type(&self) -> &'static str {
        "SEQEND"
    }
    fn apply_transform(&mut self, _transform: &Transform) {
        super::transform::transform_seqend(self, _transform);
    }
}
