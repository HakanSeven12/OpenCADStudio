//! User Coordinate System table entry

use super::TableEntry;
use crate::types::{Handle, Vector3};

/// A User Coordinate System (UCS) table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ucs {
    /// Unique handle
    pub handle: Handle,
    /// UCS name
    pub name: String,
    /// Origin point
    pub origin: Vector3,
    /// X-axis direction
    pub x_axis: Vector3,
    /// Y-axis direction
    pub y_axis: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub elevation: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ortho_view_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ortho_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub named_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_reference: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_resolved: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_dependent: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_handle: Handle,
}

impl Ucs {
    /// Create a new UCS
    pub fn new(name: impl Into<String>) -> Self {
        Ucs {
            handle: Handle::NULL,
            name: name.into(),
            origin: Vector3::ZERO,
            x_axis: Vector3::UNIT_X,
            y_axis: Vector3::UNIT_Y,
            elevation: 0.0,
            ortho_view_type: 0,
            ortho_type: 0,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
        }
    }

    /// Create a UCS with specific origin and axes
    pub fn from_origin_axes(
        name: impl Into<String>,
        origin: Vector3,
        x_axis: Vector3,
        y_axis: Vector3,
    ) -> Self {
        Ucs {
            handle: Handle::NULL,
            name: name.into(),
            origin,
            x_axis,
            y_axis,
            elevation: 0.0,
            ortho_view_type: 0,
            ortho_type: 0,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
        }
    }

    /// Get the Z-axis direction (cross product of X and Y)
    pub fn z_axis(&self) -> Vector3 {
        self.x_axis.cross(&self.y_axis)
    }
}

impl TableEntry for Ucs {
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
}
