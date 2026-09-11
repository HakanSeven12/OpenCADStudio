//! View table entry

use super::TableEntry;
use crate::entities::ViewportRenderMode;
use crate::types::{Color, Handle, Vector3};

/// A view table entry
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct View {
    /// Unique handle
    pub handle: Handle,
    /// View name
    pub name: String,
    /// View center point
    pub center: Vector3,
    /// View height
    pub height: f64,
    /// View width
    pub width: f64,
    /// View direction (from target)
    pub direction: Vector3,
    /// View target point
    pub target: Vector3,
    /// Lens length
    pub lens_length: f64,
    /// Front clipping plane offset
    pub front_clip: f64,
    /// Back clipping plane offset
    pub back_clip: f64,
    /// Twist angle
    pub twist_angle: f64,
    /// Perspective projection flag (VIEWMODE bit 0). True for views created by
    /// the CAMERA command; used to draw the camera display glyph.
    pub perspective: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub back_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clip_at_eye: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub render_mode: ViewportRenderMode,
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
    pub paper_space: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_associated: bool,
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
    pub camera_plottable: bool,
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
    pub live_section_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub visual_style_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub sun_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub named_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_ucs_handle: Handle,
}

impl View {
    /// Create a new view
    pub fn new(name: impl Into<String>) -> Self {
        View {
            handle: Handle::NULL,
            name: name.into(),
            center: Vector3::ZERO,
            height: 1.0,
            width: 1.0,
            direction: Vector3::UNIT_Z,
            target: Vector3::ZERO,
            lens_length: 50.0,
            front_clip: 0.0,
            back_clip: 0.0,
            twist_angle: 0.0,
            perspective: false,
            front_clipping: false,
            back_clipping: false,
            front_clip_at_eye: false,
            render_mode: ViewportRenderMode::Wireframe2D,
            use_default_lights: true,
            default_lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient_color: Color::from_index(250),
            paper_space: false,
            ucs_associated: false,
            ucs_origin: Vector3::ZERO,
            ucs_x_axis: Vector3::UNIT_X,
            ucs_y_axis: Vector3::UNIT_Y,
            ucs_elevation: 0.0,
            ucs_ortho_type: 0,
            camera_plottable: false,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
            background_handle: Handle::NULL,
            live_section_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            sun_handle: Handle::NULL,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
        }
    }
}

impl TableEntry for View {
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
