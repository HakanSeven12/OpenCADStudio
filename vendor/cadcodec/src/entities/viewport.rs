//! Viewport entity - Paper space viewport for model space views

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Viewport status flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewportStatusFlags {
    /// Viewport is on (visible)
    pub is_on: bool,
    /// Perspective mode active
    pub perspective: bool,
    /// Front clipping on
    pub front_clipping: bool,
    /// Back clipping on
    pub back_clipping: bool,
    /// UCS follow mode
    pub ucs_follow: bool,
    /// Front clip not at eye
    pub front_clip_not_at_eye: bool,
    /// UCS icon visibility
    pub ucs_icon_visible: bool,
    /// UCS icon at origin
    pub ucs_icon_at_origin: bool,
    /// Fast zoom enabled
    pub fast_zoom: bool,
    /// Snap mode on
    pub snap_on: bool,
    /// Grid mode on
    pub grid_on: bool,
    /// Isometric snap style
    pub isometric_snap: bool,
    /// Hide plot
    pub hide_plot: bool,
    /// kIsoPairTop
    pub iso_pair_top: bool,
    /// kIsoPairRight
    pub iso_pair_right: bool,
    /// Viewport locked
    pub locked: bool,
}

impl ViewportStatusFlags {
    /// Create default viewport status (on, visible)
    pub fn default_on() -> Self {
        Self {
            is_on: true,
            ..Default::default()
        }
    }

    /// Create from the DWG/DXF viewport status bit-coded flags (group 90).
    /// The low bits run perspective(0x1) … iso_pair_right(0x2000); the two high
    /// bits are viewport-locked(0x4000) and viewport-on/visible(0x8000).
    pub fn from_bits(bits: i32) -> Self {
        Self {
            perspective: (bits & (1 << 0)) != 0,
            front_clipping: (bits & (1 << 1)) != 0,
            back_clipping: (bits & (1 << 2)) != 0,
            ucs_follow: (bits & (1 << 3)) != 0,
            front_clip_not_at_eye: (bits & (1 << 4)) != 0,
            ucs_icon_visible: (bits & (1 << 5)) != 0,
            ucs_icon_at_origin: (bits & (1 << 6)) != 0,
            fast_zoom: (bits & (1 << 7)) != 0,
            snap_on: (bits & (1 << 8)) != 0,
            grid_on: (bits & (1 << 9)) != 0,
            isometric_snap: (bits & (1 << 10)) != 0,
            hide_plot: (bits & (1 << 11)) != 0,
            iso_pair_top: (bits & (1 << 12)) != 0,
            iso_pair_right: (bits & (1 << 13)) != 0,
            locked: (bits & (1 << 14)) != 0,
            is_on: (bits & (1 << 15)) != 0,
        }
    }

    /// Convert to the DWG/DXF viewport status bit-coded flags (group 90).
    pub fn to_bits(&self) -> i32 {
        let mut bits = 0;
        if self.perspective {
            bits |= 1 << 0;
        }
        if self.front_clipping {
            bits |= 1 << 1;
        }
        if self.back_clipping {
            bits |= 1 << 2;
        }
        if self.ucs_follow {
            bits |= 1 << 3;
        }
        if self.front_clip_not_at_eye {
            bits |= 1 << 4;
        }
        if self.ucs_icon_visible {
            bits |= 1 << 5;
        }
        if self.ucs_icon_at_origin {
            bits |= 1 << 6;
        }
        if self.fast_zoom {
            bits |= 1 << 7;
        }
        if self.snap_on {
            bits |= 1 << 8;
        }
        if self.grid_on {
            bits |= 1 << 9;
        }
        if self.isometric_snap {
            bits |= 1 << 10;
        }
        if self.hide_plot {
            bits |= 1 << 11;
        }
        if self.iso_pair_top {
            bits |= 1 << 12;
        }
        if self.iso_pair_right {
            bits |= 1 << 13;
        }
        if self.locked {
            bits |= 1 << 14;
        }
        if self.is_on {
            bits |= 1 << 15;
        }
        bits
    }
}

/// Render mode for viewport display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ViewportRenderMode {
    /// 2D Wireframe
    #[default]
    Wireframe2D = 0,
    /// 3D Wireframe
    Wireframe3D = 1,
    /// Hidden line removal
    HiddenLine = 2,
    /// Flat shaded
    FlatShaded = 3,
    /// Gouraud shaded
    GouraudShaded = 4,
    /// Flat shaded with edges
    FlatShadedWithEdges = 5,
    /// Gouraud shaded with edges
    GouraudShadedWithEdges = 6,
}

impl ViewportRenderMode {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => ViewportRenderMode::Wireframe3D,
            2 => ViewportRenderMode::HiddenLine,
            3 => ViewportRenderMode::FlatShaded,
            4 => ViewportRenderMode::GouraudShaded,
            5 => ViewportRenderMode::FlatShadedWithEdges,
            6 => ViewportRenderMode::GouraudShadedWithEdges,
            _ => ViewportRenderMode::Wireframe2D,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Grid display flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridFlags {
    /// Grid beyond limits
    pub beyond_limits: bool,
    /// Adaptive grid
    pub adaptive: bool,
    /// Allow subdivision
    pub subdivision: bool,
    /// Follow dynamic UCS
    pub follow_dynamic: bool,
}

impl GridFlags {
    /// Create from bits
    pub fn from_bits(bits: i16) -> Self {
        Self {
            beyond_limits: (bits & 1) != 0,
            adaptive: (bits & 2) != 0,
            subdivision: (bits & 4) != 0,
            follow_dynamic: (bits & 8) != 0,
        }
    }

    /// Convert to bits
    pub fn to_bits(&self) -> i16 {
        let mut bits = 0;
        if self.beyond_limits {
            bits |= 1;
        }
        if self.adaptive {
            bits |= 2;
        }
        if self.subdivision {
            bits |= 4;
        }
        if self.follow_dynamic {
            bits |= 8;
        }
        bits
    }
}

/// Viewport entity - defines a view of model space within paper space
///
/// Viewports are used in paper space layouts to create windows into model space.
/// They define the position, size, and viewing parameters for displaying
/// model space content.
///
/// # DXF Entity Type
/// VIEWPORT
///
/// # Example
/// ```ignore
/// use acadrust::entities::Viewport;
/// use acadrust::types::Vector3;
///
/// let mut viewport = Viewport::new();
/// viewport.center = Vector3::new(5.0, 5.0, 0.0);
/// viewport.width = 10.0;
/// viewport.height = 8.0;
/// viewport.view_target = Vector3::new(0.0, 0.0, 0.0);
/// viewport.view_direction = Vector3::new(0.0, 0.0, 1.0);
/// viewport.custom_scale = 0.5; // 1:2 scale
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Viewport {
    /// Common entity properties
    pub common: EntityCommon,
    /// Center point in paper space
    pub center: Vector3,
    /// Width in paper space units
    pub width: f64,
    /// Height in paper space units
    pub height: f64,
    /// Viewport status flags
    pub status: ViewportStatusFlags,
    /// Viewport ID (unique within the drawing)
    pub id: i16,
    /// View center point (DCS - Display Coordinate System)
    pub view_center: Vector3,
    /// Snap base point
    pub snap_base: Vector3,
    /// Snap spacing
    pub snap_spacing: Vector3,
    /// Grid spacing
    pub grid_spacing: Vector3,
    /// View direction vector (WCS)
    pub view_direction: Vector3,
    /// View target point (WCS)
    pub view_target: Vector3,
    /// Lens length (mm)
    pub lens_length: f64,
    /// Front clipping plane Z value
    pub front_clip_z: f64,
    /// Back clipping plane Z value
    pub back_clip_z: f64,
    /// View height (in model space units)
    pub view_height: f64,
    /// Snap angle
    pub snap_angle: f64,
    /// View twist angle
    pub twist_angle: f64,
    /// Circle zoom percent (1-20000)
    pub circle_sides: i16,
    /// Frozen layer handles
    pub frozen_layers: Vec<Handle>,
    /// Clip-boundary entity handle (H 340). Non-NULL when the viewport is
    /// clipped by a non-rectangular boundary; NULL for a plain rectangular
    /// viewport.
    pub clip_boundary_handle: Handle,
    /// Render mode
    pub render_mode: ViewportRenderMode,
    /// Plot style sheet assigned directly to the viewport.
    pub style_sheet: String,
    /// Place the UCS icon at the UCS origin.
    pub ucs_at_origin: bool,
    /// UCS per viewport flag
    pub ucs_per_viewport: bool,
    /// UCS icon visibility
    pub ucs_icon_visible: bool,
    /// UCS origin
    pub ucs_origin: Vector3,
    /// UCS X axis
    pub ucs_x_axis: Vector3,
    /// UCS Y axis
    pub ucs_y_axis: Vector3,
    /// UCS handle
    pub ucs_handle: Handle,
    /// Base UCS handle
    pub base_ucs_handle: Handle,
    /// UCS orthographic type
    pub ucs_ortho_type: i16,
    /// Elevation
    pub elevation: f64,
    /// Shade plot mode
    pub shade_plot_mode: i16,
    /// Grid flags
    pub grid_flags: GridFlags,
    /// Grid major frequency
    pub grid_major: i16,
    /// Background handle
    pub background_handle: Handle,
    /// Shade plot handle
    pub shade_plot_handle: Handle,
    /// Visual style handle
    pub visual_style_handle: Handle,
    /// Sun object owned by this viewport.
    pub sun_handle: Handle,
    /// Default lighting on
    pub default_lighting: bool,
    /// Default lighting type
    pub default_lighting_type: i16,
    /// View brightness
    pub brightness: f64,
    /// View contrast
    pub contrast: f64,
    /// Ambient light color.
    pub ambient_color: Color,
    /// Custom scale factor
    pub custom_scale: f64,
}

impl Viewport {
    /// Create a new viewport with default settings
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            center: Vector3::ZERO,
            width: 297.0,  // A4 width in mm
            height: 210.0, // A4 height in mm
            status: ViewportStatusFlags::default_on(),
            id: 0,
            view_center: Vector3::ZERO,
            snap_base: Vector3::ZERO,
            snap_spacing: Vector3::new(10.0, 10.0, 0.0),
            grid_spacing: Vector3::new(10.0, 10.0, 0.0),
            view_direction: Vector3::UNIT_Z,
            view_target: Vector3::ZERO,
            lens_length: 50.0,
            front_clip_z: 0.0,
            back_clip_z: 0.0,
            view_height: 210.0,
            snap_angle: 0.0,
            twist_angle: 0.0,
            circle_sides: 1000,
            frozen_layers: Vec::new(),
            clip_boundary_handle: Handle::NULL,
            render_mode: ViewportRenderMode::Wireframe2D,
            style_sheet: String::new(),
            ucs_at_origin: false,
            ucs_per_viewport: false,
            ucs_icon_visible: true,
            ucs_origin: Vector3::ZERO,
            ucs_x_axis: Vector3::UNIT_X,
            ucs_y_axis: Vector3::UNIT_Y,
            ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
            ucs_ortho_type: 0,
            elevation: 0.0,
            shade_plot_mode: 0,
            grid_flags: GridFlags::default(),
            grid_major: 5,
            background_handle: Handle::NULL,
            shade_plot_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            sun_handle: Handle::NULL,
            default_lighting: true,
            default_lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient_color: Color::from_index(0),
            custom_scale: 1.0,
        }
    }

    /// Create a viewport with specific size and position
    pub fn with_size(center: Vector3, width: f64, height: f64) -> Self {
        let mut vp = Self::new();
        vp.center = center;
        vp.width = width;
        vp.height = height;
        vp
    }

    /// Create a viewport for model space (ID 1)
    pub fn model_space() -> Self {
        let mut vp = Self::new();
        vp.id = 1;
        vp
    }

    /// Set the view target (what the viewport is looking at)
    pub fn set_view_target(&mut self, target: Vector3) {
        self.view_target = target;
    }

    /// Set the view direction
    pub fn set_view_direction(&mut self, direction: Vector3) {
        let normalized = direction.normalize();
        if normalized.length_squared() > 0.0 {
            self.view_direction = normalized;
        } else {
            self.view_direction = Vector3::UNIT_Z;
        }
    }

    /// Set the view height (zoom level in model space)
    pub fn set_view_height(&mut self, height: f64) {
        self.view_height = height;
    }

    /// Calculate the scale factor (paper space / model space)
    pub fn scale(&self) -> f64 {
        if self.view_height.abs() > 1e-10 {
            self.height / self.view_height
        } else {
            1.0
        }
    }

    /// Set the scale factor
    pub fn set_scale(&mut self, scale: f64) {
        if scale.abs() > 1e-10 {
            self.view_height = self.height / scale;
            self.custom_scale = scale;
        }
    }

    /// Lock the viewport (prevent zoom/pan)
    pub fn lock(&mut self) {
        self.status.locked = true;
    }

    /// Unlock the viewport
    pub fn unlock(&mut self) {
        self.status.locked = false;
    }

    /// Check if viewport is locked
    pub fn is_locked(&self) -> bool {
        self.status.locked
    }

    /// Turn viewport on
    pub fn turn_on(&mut self) {
        self.status.is_on = true;
    }

    /// Turn viewport off
    pub fn turn_off(&mut self) {
        self.status.is_on = false;
    }

    /// Check if viewport is on
    pub fn is_on(&self) -> bool {
        self.status.is_on
    }

    /// Add a frozen layer by handle
    pub fn freeze_layer(&mut self, layer_handle: Handle) {
        if !self.frozen_layers.contains(&layer_handle) {
            self.frozen_layers.push(layer_handle);
        }
    }

    /// Remove a frozen layer
    pub fn thaw_layer(&mut self, layer_handle: Handle) {
        self.frozen_layers.retain(|h| *h != layer_handle);
    }

    /// Set the render mode
    pub fn set_render_mode(&mut self, mode: ViewportRenderMode) {
        self.render_mode = mode;
    }

    /// Get the paper space bounds of this viewport
    pub fn paper_bounds(&self) -> BoundingBox3D {
        let half_w = self.width / 2.0;
        let half_h = self.height / 2.0;
        BoundingBox3D::new(
            Vector3::new(self.center.x - half_w, self.center.y - half_h, 0.0),
            Vector3::new(self.center.x + half_w, self.center.y + half_h, 0.0),
        )
    }

    /// Set a standard view (top, front, right, isometric, etc.)
    pub fn set_standard_view(&mut self, view: StandardView) {
        match view {
            StandardView::Top => {
                self.view_direction = Vector3::UNIT_Z;
                self.twist_angle = 0.0;
            }
            StandardView::Bottom => {
                self.view_direction = -Vector3::UNIT_Z;
                self.twist_angle = 0.0;
            }
            StandardView::Front => {
                self.view_direction = -Vector3::UNIT_Y;
                self.twist_angle = 0.0;
            }
            StandardView::Back => {
                self.view_direction = Vector3::UNIT_Y;
                self.twist_angle = 0.0;
            }
            StandardView::Left => {
                self.view_direction = -Vector3::UNIT_X;
                self.twist_angle = 0.0;
            }
            StandardView::Right => {
                self.view_direction = Vector3::UNIT_X;
                self.twist_angle = 0.0;
            }
            StandardView::SWIsometric => {
                self.view_direction = Vector3::new(-1.0, -1.0, 1.0).normalize();
                self.twist_angle = 0.0;
            }
            StandardView::SEIsometric => {
                self.view_direction = Vector3::new(1.0, -1.0, 1.0).normalize();
                self.twist_angle = 0.0;
            }
            StandardView::NEIsometric => {
                self.view_direction = Vector3::new(1.0, 1.0, 1.0).normalize();
                self.twist_angle = 0.0;
            }
            StandardView::NWIsometric => {
                self.view_direction = Vector3::new(-1.0, 1.0, 1.0).normalize();
                self.twist_angle = 0.0;
            }
        }
    }

    /// Builder: Set center
    pub fn with_center(mut self, center: Vector3) -> Self {
        self.center = center;
        self
    }

    /// Builder: Set view target
    pub fn with_view_target(mut self, target: Vector3) -> Self {
        self.view_target = target;
        self
    }

    /// Builder: Set view direction
    pub fn with_view_direction(mut self, direction: Vector3) -> Self {
        let normalized = direction.normalize();
        if normalized.length_squared() > 0.0 {
            self.view_direction = normalized;
        } else {
            self.view_direction = Vector3::UNIT_Z;
        }
        self
    }

    /// Builder: Set scale
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.set_scale(scale);
        self
    }

    /// Builder: Set locked
    pub fn with_locked(mut self) -> Self {
        self.status.locked = true;
        self
    }
}

/// Standard view directions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StandardView {
    /// Top view (looking down Z)
    Top,
    /// Bottom view (looking up Z)
    Bottom,
    /// Front view (looking into -Y)
    Front,
    /// Back view (looking into +Y)
    Back,
    /// Left view (looking into -X)
    Left,
    /// Right view (looking into +X)
    Right,
    /// SW Isometric
    SWIsometric,
    /// SE Isometric
    SEIsometric,
    /// NE Isometric
    NEIsometric,
    /// NW Isometric
    NWIsometric,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Viewport {
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
        self.paper_bounds()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_viewport(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "VIEWPORT"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_viewport(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_creation() {
        let vp = Viewport::new();
        assert!(vp.is_on());
        assert!(!vp.is_locked());
        assert_eq!(vp.view_direction, Vector3::UNIT_Z);
    }

    #[test]
    fn test_viewport_with_size() {
        let vp = Viewport::with_size(Vector3::new(100.0, 100.0, 0.0), 200.0, 150.0);
        assert_eq!(vp.center, Vector3::new(100.0, 100.0, 0.0));
        assert_eq!(vp.width, 200.0);
        assert_eq!(vp.height, 150.0);
    }

    #[test]
    fn test_viewport_scale() {
        let mut vp = Viewport::new();
        vp.height = 100.0;
        vp.view_height = 200.0;

        assert!((vp.scale() - 0.5).abs() < 1e-10);

        vp.set_scale(0.25);
        assert!((vp.view_height - 400.0).abs() < 1e-10);
    }

    #[test]
    fn test_viewport_lock() {
        let mut vp = Viewport::new();
        assert!(!vp.is_locked());

        vp.lock();
        assert!(vp.is_locked());

        vp.unlock();
        assert!(!vp.is_locked());
    }

    #[test]
    fn test_viewport_frozen_layers() {
        let mut vp = Viewport::new();
        let layer1 = Handle::new(100);
        let layer2 = Handle::new(200);

        vp.freeze_layer(layer1);
        vp.freeze_layer(layer2);
        assert_eq!(vp.frozen_layers.len(), 2);

        vp.thaw_layer(layer1);
        assert_eq!(vp.frozen_layers.len(), 1);
        assert!(vp.frozen_layers.contains(&layer2));
    }

    #[test]
    fn test_viewport_standard_views() {
        let mut vp = Viewport::new();

        vp.set_standard_view(StandardView::Front);
        assert_eq!(vp.view_direction, -Vector3::UNIT_Y);

        vp.set_standard_view(StandardView::Top);
        assert_eq!(vp.view_direction, Vector3::UNIT_Z);

        vp.set_standard_view(StandardView::NEIsometric);
        // Check it's roughly (1, 1, 1) normalized
        let expected = Vector3::new(1.0, 1.0, 1.0).normalize();
        assert!((vp.view_direction.x - expected.x).abs() < 1e-10);
        assert!((vp.view_direction.y - expected.y).abs() < 1e-10);
        assert!((vp.view_direction.z - expected.z).abs() < 1e-10);
    }

    #[test]
    fn test_viewport_paper_bounds() {
        let vp = Viewport::with_size(Vector3::new(100.0, 100.0, 0.0), 200.0, 100.0);

        let bounds = vp.paper_bounds();
        assert_eq!(bounds.min.x, 0.0);
        assert_eq!(bounds.min.y, 50.0);
        assert_eq!(bounds.max.x, 200.0);
        assert_eq!(bounds.max.y, 150.0);
    }

    #[test]
    fn test_viewport_status_flags() {
        // Spec layout (group 90): bit 15 = viewport on, bit 14 = locked,
        // bit 0 = perspective. 0x8001 = on + perspective.
        let flags = ViewportStatusFlags::from_bits(0x8001);
        assert!(flags.is_on);
        assert!(flags.perspective);
        assert!(!flags.locked);
        assert_eq!(flags.to_bits(), 0x8001);

        // Locked, off viewport: bit 14 only.
        let locked = ViewportStatusFlags::from_bits(0x4000);
        assert!(locked.locked);
        assert!(!locked.is_on);
        assert_eq!(locked.to_bits(), 0x4000);
    }

    #[test]
    fn test_viewport_render_mode() {
        let mut vp = Viewport::new();
        vp.set_render_mode(ViewportRenderMode::HiddenLine);
        assert_eq!(vp.render_mode, ViewportRenderMode::HiddenLine);
    }

    #[test]
    fn test_viewport_translate() {
        let mut vp = Viewport::with_size(Vector3::new(0.0, 0.0, 0.0), 100.0, 100.0);

        vp.translate(Vector3::new(50.0, 50.0, 0.0));
        assert_eq!(vp.center, Vector3::new(50.0, 50.0, 0.0));
    }

    #[test]
    fn test_viewport_builder() {
        let vp = Viewport::new()
            .with_center(Vector3::new(100.0, 100.0, 0.0))
            .with_view_target(Vector3::new(50.0, 50.0, 0.0))
            .with_scale(0.5)
            .with_locked();

        assert_eq!(vp.center, Vector3::new(100.0, 100.0, 0.0));
        assert_eq!(vp.view_target, Vector3::new(50.0, 50.0, 0.0));
        assert!(vp.is_locked());
    }
}
