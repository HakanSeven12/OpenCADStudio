//! Drawing-view border entity (`AcDbViewBorder`, DXF class "DRAWINGVIEW").
//!
//! The border of a Model-Documentation drawing view. Its paper-space rectangle,
//! view scale and the reference to the view's *active* viewport (the entity
//! carrying the real camera) are decoded and encoded as native semantic fields.
//!
//! The border itself is a non-plotting aid — it is not drawn — but its
//! rectangle gives each view's true paper placement, and its viewport link is
//! the last hop of the section-mark viewing-direction chain.

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// A Model-Documentation drawing-view border (`AcDbViewBorder`).
///
/// All coordinates are layout paper-space. Verified: `max − min` equals the
/// view's template-viewport size, `center` is exactly the rectangle midpoint
/// (stored redundantly in the record), and `scale` equals the template/active
/// viewport height ratio.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewBorder {
    /// Common entity data (handle, layer, color, …).
    pub common: EntityCommon,
    /// `AcDbViewBorder` version (group 70).
    pub version: i16,
    /// Border rectangle minimum corner.
    pub min: [f64; 2],
    /// Border rectangle maximum corner.
    pub max: [f64; 2],
    /// View centre point (the rectangle midpoint).
    pub center: [f64; 2],
    /// View scale value (group 40).
    pub scale: f64,
    /// View rotation angle in radians (second group 40).
    pub rotation_angle: f64,
    /// The view's *active* viewport entity (the border's first object-specific
    /// handle reference) — carries the real camera (`view_direction`, twist).
    pub active_viewport: Handle,
    /// Associated `SCALE` object (group 340).
    pub scale_handle: Handle,
}

impl ViewBorder {
    /// Create an empty view border.
    pub fn new() -> Self {
        ViewBorder {
            common: EntityCommon::new(),
            version: 0,
            min: [0.0; 2],
            max: [0.0; 2],
            center: [0.0; 2],
            scale: 1.0,
            rotation_angle: 0.0,
            active_viewport: Handle::NULL,
            scale_handle: Handle::NULL,
        }
    }
}

impl Default for ViewBorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for ViewBorder {
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
        BoundingBox3D {
            min: Vector3::new(self.min[0], self.min[1], 0.0),
            max: Vector3::new(self.max[0], self.max[1], 0.0),
        }
    }
    fn translate(&mut self, offset: Vector3) {
        self.min[0] += offset.x;
        self.min[1] += offset.y;
        self.max[0] += offset.x;
        self.max[1] += offset.y;
        self.center[0] += offset.x;
        self.center[1] += offset.y;
    }
    fn entity_type(&self) -> &'static str {
        "DRAWINGVIEW"
    }
    fn apply_transform(&mut self, transform: &Transform) {
        let corners = [
            Vector3::new(self.min[0], self.min[1], 0.0),
            Vector3::new(self.max[0], self.min[1], 0.0),
            Vector3::new(self.max[0], self.max[1], 0.0),
            Vector3::new(self.min[0], self.max[1], 0.0),
        ]
        .map(|point| transform.apply(point));
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for point in corners {
            min[0] = min[0].min(point.x);
            min[1] = min[1].min(point.y);
            max[0] = max[0].max(point.x);
            max[1] = max[1].max(point.y);
        }
        let center = transform.apply(Vector3::new(self.center[0], self.center[1], 0.0));
        self.min = min;
        self.max = max;
        self.center = [center.x, center.y];
    }
}
