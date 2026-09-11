//! Viewport table entry

use super::TableEntry;
use crate::entities::{GridFlags, ViewportRenderMode};
use crate::types::{Color, Handle, Vector2, Vector3};

/// A viewport table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VPort {
    /// Unique handle
    pub handle: Handle,
    /// Viewport name
    pub name: String,
    /// Lower-left corner
    pub lower_left: Vector2,
    /// Upper-right corner
    pub upper_right: Vector2,
    /// View center point
    pub view_center: Vector2,
    /// Snap base point
    pub snap_base: Vector2,
    /// Snap spacing
    pub snap_spacing: Vector2,
    /// Grid spacing
    pub grid_spacing: Vector2,
    /// View direction
    pub view_direction: Vector3,
    /// View target
    pub view_target: Vector3,
    /// View height
    pub view_height: f64,
    /// Aspect ratio
    pub aspect_ratio: f64,
    /// Lens length
    pub lens_length: f64,
    /// View twist angle
    pub view_twist: f64,
    /// Front clipping plane distance
    pub front_clip: f64,
    /// Back clipping plane distance
    pub back_clip: f64,
    /// UCS follow mode
    pub ucsfollow: bool,
    /// Circle zoom percent
    pub circle_zoom: i16,
    /// Fast zoom enabled
    pub fast_zoom: bool,
    /// Grid on/off
    pub grid_on: bool,
    /// Snap on/off
    pub snap_on: bool,
    /// Snap style (isometric)
    pub snap_style: bool,
    /// Snap isometric pair
    pub snap_isopair: i16,
    /// Snap rotation angle
    pub snap_rotation: f64,
    /// Visual style / render mode (DXF code 281)
    pub render_mode: ViewportRenderMode,
    #[cfg_attr(feature = "serde", serde(default))]
    pub perspective: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub back_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clip_at_eye: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucsicon_lower: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucsicon_origin: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub use_default_lights: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub default_lighting_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub brightness: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub contrast: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ambient_color: Color,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_at_origin: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_per_viewport: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_origin: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_x_axis: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_y_axis: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_elevation: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_ortho_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub grid_flags: GridFlags,
    #[cfg_attr(feature = "serde", serde(default))]
    pub grid_major: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_reference: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_resolved: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_dependent: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub background_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub visual_style_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub sun_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub named_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_ucs_handle: Handle,
}

impl VPort {
    /// Create a new viewport
    pub fn new(name: impl Into<String>) -> Self {
        VPort {
            handle: Handle::NULL,
            name: name.into(),
            lower_left: Vector2::ZERO,
            upper_right: Vector2::new(1.0, 1.0),
            view_center: Vector2::ZERO,
            snap_base: Vector2::ZERO,
            snap_spacing: Vector2::new(0.5, 0.5),
            grid_spacing: Vector2::new(10.0, 10.0),
            view_direction: Vector3::UNIT_Z,
            view_target: Vector3::ZERO,
            view_height: 10.0,
            aspect_ratio: 1.0,
            lens_length: 50.0,
            view_twist: 0.0,
            front_clip: 0.0,
            back_clip: 0.0,
            ucsfollow: false,
            circle_zoom: 100,
            fast_zoom: true,
            grid_on: false,
            snap_on: false,
            snap_style: false,
            snap_isopair: 0,
            snap_rotation: 0.0,
            render_mode: ViewportRenderMode::Wireframe2D,
            perspective: false,
            front_clipping: false,
            back_clipping: false,
            front_clip_at_eye: false,
            ucsicon_lower: true,
            ucsicon_origin: true,
            use_default_lights: true,
            default_lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient_color: Color::from_index(250),
            ucs_at_origin: false,
            ucs_per_viewport: true,
            ucs_origin: Vector3::ZERO,
            ucs_x_axis: Vector3::UNIT_X,
            ucs_y_axis: Vector3::UNIT_Y,
            ucs_elevation: 0.0,
            ucs_ortho_type: 0,
            grid_flags: GridFlags::from_bits(2),
            grid_major: 5,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
            background_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            sun_handle: Handle::NULL,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
        }
    }

    /// Create the standard "*Active" viewport
    pub fn active() -> Self {
        Self::new("*Active")
    }
}

impl TableEntry for VPort {
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
        self.name == "*Active"
    }
}
