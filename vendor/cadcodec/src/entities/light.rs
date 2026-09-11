//! Light entity (point / spot / distant light source).
//!
//! Includes attenuation, shadow and optional photometric/IES data.

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LightPhotometricData {
    pub has_web_file: bool,
    pub web_file: String,
    pub physical_intensity_method: i16,
    pub physical_intensity: f64,
    pub illuminance_distance: f64,
    pub lamp_color_type: i16,
    pub lamp_color_temperature: f64,
    pub lamp_color_preset: i16,
    pub web_rotation: Vector3,
    pub extended_light_shape: i16,
    pub extended_light_length: f64,
    pub extended_light_width: f64,
    pub extended_light_radius: f64,
    pub web_file_type: i16,
    pub web_symmetry: i16,
    pub has_target_grip: i16,
    pub web_flux: f64,
    pub web_angles: [f64; 5],
    pub glyph_display_type: i16,
}

impl Default for LightPhotometricData {
    fn default() -> Self {
        Self {
            has_web_file: false,
            web_file: String::new(),
            physical_intensity_method: 0,
            physical_intensity: 0.0,
            illuminance_distance: 0.0,
            lamp_color_type: 0,
            lamp_color_temperature: 0.0,
            lamp_color_preset: 0,
            web_rotation: Vector3::new(1.0, 1.0, 1.0),
            extended_light_shape: 0,
            extended_light_length: 0.0,
            extended_light_width: 0.0,
            extended_light_radius: 0.0,
            web_file_type: 0,
            web_symmetry: 0,
            has_target_grip: 0,
            web_flux: 0.0,
            web_angles: [0.0; 5],
            glyph_display_type: 0,
        }
    }
}

/// A light source entity (`AcDbLight`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Light {
    /// Common entity data (handle, layer, color, …).
    pub common: EntityCommon,
    /// Light name (DXF 1), e.g. `"Spotlight1"`.
    pub name: String,
    pub class_version: i32,
    /// Light type (DXF 70): 1 = distant, 2 = point, 3 = spot.
    pub light_type: i32,
    /// Light source position (DXF 10).
    pub position: Vector3,
    /// Aim / target point (DXF 11) — meaningful for spot and distant lights.
    pub target: Vector3,
    pub status: bool,
    pub light_color: Color,
    pub plot_glyph: bool,
    pub intensity: f64,
    pub attenuation_type: i32,
    pub use_attenuation_limits: bool,
    pub attenuation_start_limit: f64,
    pub attenuation_end_limit: f64,
    pub hotspot_angle: f64,
    pub falloff_angle: f64,
    pub cast_shadows: bool,
    pub shadow_type: i32,
    pub shadow_map_size: i16,
    pub shadow_map_softness: u8,
    /// True when the file's LIGHTINGUNITS enables the photometric tail.
    pub photometric_mode: bool,
    /// Present when the photometric tail's has-data bit is set.
    pub photometric_data: Option<LightPhotometricData>,
}

impl Light {
    /// Create a new point light at the origin.
    pub fn new() -> Self {
        Light {
            common: EntityCommon::new(),
            name: String::new(),
            class_version: 1,
            light_type: 2,
            position: Vector3::ZERO,
            target: Vector3::ZERO,
            status: true,
            light_color: Color::WHITE,
            plot_glyph: false,
            intensity: 1.0,
            attenuation_type: 0,
            use_attenuation_limits: false,
            attenuation_start_limit: 0.0,
            attenuation_end_limit: 0.0,
            hotspot_angle: 0.0,
            falloff_angle: 0.0,
            cast_shadows: false,
            shadow_type: 0,
            shadow_map_size: 0,
            shadow_map_softness: 0,
            photometric_mode: false,
            photometric_data: None,
        }
    }

    /// True for a spot (cone) light.
    pub fn is_spot(&self) -> bool {
        self.light_type == 3
    }

    /// True for a distant (parallel-ray) light.
    pub fn is_distant(&self) -> bool {
        self.light_type == 1
    }
}

impl Default for Light {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Light {
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
        BoundingBox3D::from_point(self.position)
    }
    fn translate(&mut self, offset: Vector3) {
        self.position = Vector3::new(
            self.position.x + offset.x,
            self.position.y + offset.y,
            self.position.z + offset.z,
        );
        self.target = Vector3::new(
            self.target.x + offset.x,
            self.target.y + offset.y,
            self.target.z + offset.z,
        );
    }
    fn entity_type(&self) -> &'static str {
        "LIGHT"
    }
    fn apply_transform(&mut self, transform: &Transform) {
        self.position = transform.apply(self.position);
        self.target = transform.apply(self.target);
    }
}
