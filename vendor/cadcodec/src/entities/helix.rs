//! Helix entity (a 3D spiral curve, AcDbHelix).
//!
//! A helix is defined as a spline (the evaluated curve geometry) plus the
//! parametric fields that generated it: an axis, a start point, a radius, a
//! turn count and a turn height. It derives from the spline entity in the DWG
//! format, so the wire record is the full spline record followed by the helix
//! parameters — this type embeds a [`Spline`] to reuse that geometry directly.

use super::spline::Spline;
use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Constraint that is held fixed while the other helix parameters vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HelixConstraint {
    /// Turn height is held constant (DXF 280 = 0).
    TurnHeight,
    /// Number of turns is held constant (DXF 280 = 1).
    Turns,
    /// Overall height is held constant (DXF 280 = 2).
    Height,
}

impl HelixConstraint {
    /// Decode the DXF 280 constraint code.
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => HelixConstraint::Turns,
            2 => HelixConstraint::Height,
            _ => HelixConstraint::TurnHeight,
        }
    }

    /// Encode to the DXF 280 constraint code.
    pub fn to_code(self) -> u8 {
        match self {
            HelixConstraint::TurnHeight => 0,
            HelixConstraint::Turns => 1,
            HelixConstraint::Height => 2,
        }
    }
}

impl Default for HelixConstraint {
    fn default() -> Self {
        HelixConstraint::TurnHeight
    }
}

/// A helix entity (AcDbHelix): a spline curve plus its generating parameters.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Helix {
    /// Common entity data.
    pub common: EntityCommon,
    /// The evaluated curve geometry (AcDbSpline part of the record).
    pub spline: Spline,
    /// Major release number (DXF 90).
    pub major_version: i32,
    /// Maintenance release number (DXF 91).
    pub maintenance_version: i32,
    /// Axis base point (DXF 10).
    pub axis_base_point: Vector3,
    /// Start point on the first turn (DXF 11).
    pub start_point: Vector3,
    /// Axis direction vector (DXF 12).
    pub axis_vector: Vector3,
    /// Radius of the first turn (DXF 40).
    pub radius: f64,
    /// Number of turns (DXF 41).
    pub turns: f64,
    /// Height of one turn (DXF 42).
    pub turn_height: f64,
    /// Handedness: `false` = left, `true` = right (DXF 290).
    pub handedness: bool,
    /// Which parameter is held constant (DXF 280).
    pub constraint: HelixConstraint,
}

impl Helix {
    /// Create a new, empty helix.
    pub fn new() -> Self {
        Helix {
            common: EntityCommon::new(),
            spline: Spline::new(),
            major_version: 29,
            maintenance_version: 0,
            axis_base_point: Vector3::ZERO,
            start_point: Vector3::ZERO,
            axis_vector: Vector3::UNIT_Z,
            radius: 1.0,
            turns: 3.0,
            turn_height: 1.0,
            handedness: true,
            constraint: HelixConstraint::TurnHeight,
        }
    }
}

impl Default for Helix {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Helix {
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
        self.spline.bounding_box()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_helix(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "HELIX"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_helix(self, transform);
    }
}
