//! Dimension entity types

use crate::entities::EntityCommon;
use crate::types::{Matrix3, Transform, Vector3};

/// Dimension type flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DimensionType {
    /// Rotated, horizontal, or vertical linear dimension
    Linear = 0,
    /// Aligned dimension
    Aligned = 1,
    /// Angular 2 lines dimension
    Angular = 2,
    /// Diameter dimension
    Diameter = 3,
    /// Radius dimension
    Radius = 4,
    /// Angular 3 points dimension
    Angular3Point = 5,
    /// Ordinate dimension
    Ordinate = 6,
    /// Arc-length dimension
    ArcLength = 8,
    /// Jogged / large-radius radial dimension
    LargeRadial = 9,
}

/// Attachment point type for dimension text
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AttachmentPointType {
    TopLeft = 1,
    TopCenter = 2,
    TopRight = 3,
    MiddleLeft = 4,
    MiddleCenter = 5,
    MiddleRight = 6,
    BottomLeft = 7,
    BottomCenter = 8,
    BottomRight = 9,
}

/// Base dimension entity
///
/// All dimension types share common properties and behavior.
/// Specific dimension types extend this base with additional properties.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionBase {
    pub common: EntityCommon,
    /// Definition point for the dimension line (in WCS)
    pub definition_point: Vector3,
    /// Middle point of dimension text (in WCS)
    pub text_middle_point: Vector3,
    /// Insertion point for clones of a dimension (in OCS)
    pub insertion_point: Vector3,
    /// Dimension type
    pub dimension_type: DimensionType,
    /// Attachment point
    pub attachment_point: AttachmentPointType,
    /// Dimension text explicitly entered by the user
    pub text: String,
    /// User text override (alternative to text field)
    pub user_text: Option<String>,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Rotation angle of dimension text
    pub text_rotation: f64,
    /// Horizontal direction for the dimension entity
    pub horizontal_direction: f64,
    /// Dimension style name
    pub style_name: String,
    /// Actual measurement (computed)
    pub actual_measurement: f64,
    /// Version number
    pub version: u8,
    /// Block name that contains the dimension geometry
    pub block_name: String,
    /// Line spacing factor
    pub line_spacing_factor: f64,
    /// Line spacing style (1 = at least, 2 = exact).
    pub line_spacing_style: i16,
    /// Scale applied when inserting the anonymous dimension block.
    pub insertion_scale: Vector3,
    /// Rotation applied when inserting the anonymous dimension block.
    pub insertion_rotation: f64,
    /// Undocumented R2007+ dimension state bit preserved for round-trip.
    pub dwg_unknown_bit: bool,
    /// Flip the first dimension arrow.
    pub flip_arrow1: bool,
    /// Flip the second dimension arrow.
    pub flip_arrow2: bool,
    /// Complete DWG dimension flag byte. Bit zero mirrors
    /// [`text_user_positioned`](Self::text_user_positioned).
    pub dwg_flags_byte: u8,
    /// Dimension text was positioned at a user-defined location rather than at
    /// the style's default (DXF group 70, bit 0x80). When false the text
    /// follows the dimension style (DIMTAD etc.); when true `text_middle_point`
    /// is an explicit override.
    pub text_user_positioned: bool,
}

impl DimensionBase {
    /// Create a new dimension base
    pub fn new(dim_type: DimensionType) -> Self {
        Self {
            common: EntityCommon::default(),
            definition_point: Vector3::new(0.0, 0.0, 0.0),
            text_middle_point: Vector3::new(0.0, 0.0, 0.0),
            insertion_point: Vector3::new(0.0, 0.0, 0.0),
            dimension_type: dim_type,
            attachment_point: AttachmentPointType::MiddleCenter,
            text: String::new(),
            user_text: None,
            normal: Vector3::new(0.0, 0.0, 1.0),
            text_rotation: 0.0,
            horizontal_direction: 0.0,
            style_name: "Standard".to_string(),
            actual_measurement: 0.0,
            version: 0,
            block_name: String::new(),
            line_spacing_factor: 1.0,
            line_spacing_style: 1,
            insertion_scale: Vector3::new(1.0, 1.0, 1.0),
            insertion_rotation: 0.0,
            dwg_unknown_bit: false,
            flip_arrow1: false,
            flip_arrow2: false,
            dwg_flags_byte: 0,
            text_user_positioned: false,
        }
    }

    /// Builder: Set the text override
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.set_text_override(Some(text.into()));
        self
    }

    /// Returns the effective user text override.
    pub fn text_override(&self) -> Option<&str> {
        self.user_text
            .as_deref()
            .or_else(|| (!self.text.is_empty()).then_some(self.text.as_str()))
    }

    /// Sets or clears the user text override.
    pub fn set_text_override(&mut self, text: Option<String>) {
        self.text = text.clone().unwrap_or_default();
        self.user_text = text;
    }

    /// Builder: Set the style name
    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style_name = style.into();
        self
    }

    /// Builder: Set the normal vector
    pub fn with_normal(mut self, normal: Vector3) -> Self {
        self.normal = normal;
        self
    }

    /// Check if this is an angular dimension
    pub fn is_angular(&self) -> bool {
        matches!(
            self.dimension_type,
            DimensionType::Angular | DimensionType::Angular3Point
        )
    }
}

impl Default for DimensionBase {
    fn default() -> Self {
        Self::new(DimensionType::Linear)
    }
}

/// Aligned dimension entity
///
/// Measures the distance between two points along a line parallel to those points.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAligned {
    pub base: DimensionBase,
    /// First definition point (in WCS)
    pub first_point: Vector3,
    /// Second definition point (in WCS)
    pub second_point: Vector3,
    /// Definition point on dimension line
    pub definition_point: Vector3,
    /// Extension line rotation (optional)
    pub ext_line_rotation: f64,
}

impl DimensionAligned {
    /// Create a new aligned dimension
    pub fn new(first_point: Vector3, second_point: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Aligned);

        // Calculate measurement
        base.actual_measurement = first_point.distance(&second_point);

        Self {
            base,
            first_point,
            second_point,
            definition_point: Vector3::ZERO,
            ext_line_rotation: 0.0,
        }
    }

    /// Get the measurement value
    pub fn measurement(&self) -> f64 {
        self.first_point.distance(&self.second_point)
    }

    /// Set the offset distance from the second point
    pub fn set_offset(&mut self, offset: f64) {
        let dir = self.second_point - self.first_point;
        let perpendicular = Vector3::new(-dir.y, dir.x, 0.0).normalize();
        self.definition_point = self.second_point + perpendicular * offset;
    }

    /// Get the offset distance
    pub fn offset(&self) -> f64 {
        self.second_point.distance(&self.definition_point)
    }
}

impl Default for DimensionAligned {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Aligned),
            first_point: Vector3::ZERO,
            second_point: Vector3::ZERO,
            definition_point: Vector3::ZERO,
            ext_line_rotation: 0.0,
        }
    }
}

/// Linear dimension entity
///
/// Measures the horizontal or vertical distance between two points,
/// or the distance along a rotated axis.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLinear {
    pub base: DimensionBase,
    /// First definition point (in WCS)
    pub first_point: Vector3,
    /// Second definition point (in WCS)
    pub second_point: Vector3,
    /// Definition point on dimension line
    pub definition_point: Vector3,
    /// Rotation angle of the dimension line
    pub rotation: f64,
    /// Extension line rotation
    pub ext_line_rotation: f64,
}

impl DimensionLinear {
    /// Create a new linear dimension
    pub fn new(first_point: Vector3, second_point: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Linear);
        base.actual_measurement = first_point.distance(&second_point);

        Self {
            base,
            first_point,
            second_point,
            definition_point: Vector3::ZERO,
            rotation: 0.0,
            ext_line_rotation: 0.0,
        }
    }

    /// Create a horizontal linear dimension
    pub fn horizontal(first_point: Vector3, second_point: Vector3) -> Self {
        Self::new(first_point, second_point)
    }

    /// Create a vertical linear dimension
    pub fn vertical(first_point: Vector3, second_point: Vector3) -> Self {
        let mut dim = Self::new(first_point, second_point);
        dim.rotation = std::f64::consts::FRAC_PI_2; // 90 degrees
        dim
    }

    /// Create a rotated linear dimension
    pub fn rotated(first_point: Vector3, second_point: Vector3, angle: f64) -> Self {
        let mut dim = Self::new(first_point, second_point);
        dim.rotation = angle;
        dim
    }

    /// Get the measurement value (projected onto rotation axis)
    pub fn measurement(&self) -> f64 {
        let diff = self.second_point - self.first_point;
        let projected = diff.x * self.rotation.cos() + diff.y * self.rotation.sin();
        if projected.is_finite() {
            projected.abs()
        } else {
            0.0
        }
    }

    /// Set the offset distance
    pub fn set_offset(&mut self, offset: f64) {
        let axis_y = Vector3::new(-self.rotation.sin(), self.rotation.cos(), 0.0);
        self.definition_point = self.second_point + axis_y * offset;
    }
}

impl Default for DimensionLinear {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Linear),
            first_point: Vector3::ZERO,
            second_point: Vector3::ZERO,
            definition_point: Vector3::ZERO,
            rotation: 0.0,
            ext_line_rotation: 0.0,
        }
    }
}

/// Radius dimension entity
///
/// Measures the radius of a circle or arc.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionRadius {
    pub base: DimensionBase,
    /// Definition point (point on arc/circle) - in WCS
    pub definition_point: Vector3,
    /// Center point of the arc/circle (in WCS)
    pub angle_vertex: Vector3,
    /// Leader length
    pub leader_length: f64,
}

impl DimensionRadius {
    /// Create a new radius dimension
    pub fn new(center: Vector3, point_on_arc: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Radius);
        base.actual_measurement = center.distance(&point_on_arc);

        Self {
            base,
            definition_point: point_on_arc,
            angle_vertex: center,
            leader_length: 0.0,
        }
    }

    /// Get the radius measurement
    pub fn measurement(&self) -> f64 {
        self.definition_point.distance(&self.angle_vertex)
    }
}

impl Default for DimensionRadius {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Radius),
            definition_point: Vector3::ZERO,
            angle_vertex: Vector3::ZERO,
            leader_length: 0.0,
        }
    }
}

/// Diameter dimension entity
///
/// Measures the diameter of a circle or arc.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionDiameter {
    pub base: DimensionBase,
    /// Chord point opposite `angle_vertex`, in WCS.
    pub definition_point: Vector3,
    /// First chord point, in WCS.
    pub angle_vertex: Vector3,
    /// Leader length
    pub leader_length: f64,
}

impl DimensionDiameter {
    /// Create a new diameter dimension
    pub fn new(chord_point: Vector3, far_chord_point: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Diameter);
        base.definition_point = far_chord_point;
        base.actual_measurement = chord_point.distance(&far_chord_point);

        Self {
            base,
            definition_point: far_chord_point,
            angle_vertex: chord_point,
            leader_length: 0.0,
        }
    }

    /// Get the diameter measurement
    pub fn measurement(&self) -> f64 {
        self.definition_point.distance(&self.angle_vertex)
    }

    /// Get the center point
    pub fn center(&self) -> Vector3 {
        (self.angle_vertex + self.definition_point) * 0.5
    }
}

impl Default for DimensionDiameter {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Diameter),
            definition_point: Vector3::ZERO,
            angle_vertex: Vector3::ZERO,
            leader_length: 0.0,
        }
    }
}

/// Angular 2-line dimension entity
///
/// Measures the angle between two lines.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAngular2Ln {
    pub base: DimensionBase,
    /// Dimension arc location (group 16).
    pub dimension_arc: Vector3,
    /// First point (line 1 start) - in WCS
    pub first_point: Vector3,
    /// First line end (group 14).
    pub second_point: Vector3,
    /// Second line start (group 15).
    pub angle_vertex: Vector3,
    /// Second line end (group 10).
    pub definition_point: Vector3,
}

impl DimensionAngular2Ln {
    /// Create a new angular 2-line dimension
    pub fn new(vertex: Vector3, first_point: Vector3, second_point: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Angular);

        // Calculate angle between the two lines
        let v1 = (first_point - vertex).normalize();
        let v2 = (second_point - vertex).normalize();
        let angle = v1.dot(&v2).acos();
        base.actual_measurement = angle.to_degrees();

        Self {
            base,
            dimension_arc: Vector3::ZERO,
            first_point: vertex,
            second_point: first_point,
            angle_vertex: vertex,
            definition_point: second_point,
        }
    }

    /// Get the angle measurement in radians
    pub fn measurement_radians(&self) -> f64 {
        let first_direction = self.second_point - self.first_point;
        let second_direction = self.definition_point - self.angle_vertex;
        let Some(vertex) = line_intersection(
            self.first_point,
            first_direction,
            self.angle_vertex,
            second_direction,
        ) else {
            return minor_angle_radians(first_direction, second_direction);
        };
        selected_line_sector_radians(
            vertex,
            first_direction,
            second_direction,
            self.dimension_arc,
            self.base.normal,
        )
        .unwrap_or_else(|| minor_angle_radians(first_direction, second_direction))
    }

    /// Get the angle measurement in degrees
    pub fn measurement_degrees(&self) -> f64 {
        self.measurement_radians().to_degrees()
    }
}

impl Default for DimensionAngular2Ln {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Angular),
            dimension_arc: Vector3::ZERO,
            first_point: Vector3::ZERO,
            second_point: Vector3::ZERO,
            angle_vertex: Vector3::ZERO,
            definition_point: Vector3::ZERO,
        }
    }
}

/// Type alias for backward compatibility
pub type DimensionAngular2Line = DimensionAngular2Ln;

/// Angular 3-point dimension entity
///
/// Measures the angle defined by three points.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionAngular3Pt {
    pub base: DimensionBase,
    /// Definition point (arc location) - in WCS
    pub definition_point: Vector3,
    /// First point on first line - in WCS
    pub first_point: Vector3,
    /// Second point on second line - in WCS
    pub second_point: Vector3,
    /// Angle vertex - in WCS
    pub angle_vertex: Vector3,
}

impl DimensionAngular3Pt {
    /// Create a new angular 3-point dimension
    pub fn new(vertex: Vector3, first_point: Vector3, second_point: Vector3) -> Self {
        let mut base = DimensionBase::new(DimensionType::Angular3Point);

        // Calculate angle
        let v1 = (first_point - vertex).normalize();
        let v2 = (second_point - vertex).normalize();
        let angle = v1.dot(&v2).acos();
        base.actual_measurement = angle.to_degrees();

        Self {
            base,
            definition_point: Vector3::ZERO,
            angle_vertex: vertex,
            first_point,
            second_point,
        }
    }

    /// Get the angle measurement in radians
    pub fn measurement_radians(&self) -> f64 {
        let first_direction = self.first_point - self.angle_vertex;
        let second_direction = self.second_point - self.angle_vertex;
        selected_line_sector_radians(
            self.angle_vertex,
            first_direction,
            second_direction,
            self.definition_point,
            self.base.normal,
        )
        .unwrap_or_else(|| minor_angle_radians(first_direction, second_direction))
    }

    /// Get the angle measurement in degrees
    pub fn measurement_degrees(&self) -> f64 {
        self.measurement_radians().to_degrees()
    }
}

impl Default for DimensionAngular3Pt {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Angular3Point),
            definition_point: Vector3::ZERO,
            first_point: Vector3::ZERO,
            second_point: Vector3::ZERO,
            angle_vertex: Vector3::ZERO,
        }
    }
}

/// Type alias for backward compatibility
pub type DimensionAngular3Point = DimensionAngular3Pt;

fn minor_angle_radians(first: Vector3, second: Vector3) -> f64 {
    let first_length = first.length();
    let second_length = second.length();
    if first_length <= f64::EPSILON || second_length <= f64::EPSILON {
        return 0.0;
    }
    (first.dot(&second) / (first_length * second_length))
        .clamp(-1.0, 1.0)
        .acos()
}

fn line_intersection(
    first_origin: Vector3,
    first_direction: Vector3,
    second_origin: Vector3,
    second_direction: Vector3,
) -> Option<Vector3> {
    let cross = first_direction.cross(&second_direction);
    let denominator = cross.length_squared();
    let direction_scale = first_direction.length_squared() * second_direction.length_squared();
    if !denominator.is_finite() || denominator <= direction_scale * 1.0e-24 {
        return None;
    }
    let offset = second_origin - first_origin;
    let first_parameter = offset.cross(&second_direction).dot(&cross) / denominator;
    let second_parameter = offset.cross(&first_direction).dot(&cross) / denominator;
    let first_point = first_origin + first_direction * first_parameter;
    let second_point = second_origin + second_direction * second_parameter;
    let scale = first_point.length().max(second_point.length()).max(1.0);
    (first_point.distance(&second_point) <= scale * 1.0e-9)
        .then_some((first_point + second_point) * 0.5)
}

fn selected_line_sector_radians(
    vertex: Vector3,
    first_direction: Vector3,
    second_direction: Vector3,
    arc_point: Vector3,
    preferred_normal: Vector3,
) -> Option<f64> {
    if first_direction.length_squared() <= f64::EPSILON
        || second_direction.length_squared() <= f64::EPSILON
    {
        return None;
    }
    let arc_direction = arc_point - vertex;
    if arc_direction.length_squared() <= f64::EPSILON {
        return None;
    }

    let cross = first_direction.cross(&second_direction);
    let preferred_scale =
        preferred_normal.length() * first_direction.length().max(second_direction.length());
    let preferred_is_normal = preferred_normal.length_squared() > f64::EPSILON
        && preferred_normal.dot(&first_direction).abs() <= preferred_scale * 1.0e-9
        && preferred_normal.dot(&second_direction).abs() <= preferred_scale * 1.0e-9;
    let normal = if preferred_is_normal {
        preferred_normal.normalize()
    } else if cross.length_squared() > f64::EPSILON {
        cross.normalize()
    } else {
        return None;
    };
    let axis_x = first_direction.normalize();
    let axis_y = normal.cross(&axis_x).normalize();
    let normalize_angle = |vector: Vector3| {
        vector
            .dot(&axis_y)
            .atan2(vector.dot(&axis_x))
            .rem_euclid(std::f64::consts::TAU)
    };
    let first = normalize_angle(first_direction);
    let second = normalize_angle(second_direction);
    let selected = normalize_angle(arc_direction);
    let mut boundaries = [
        first,
        (first + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU),
        second,
        (second + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU),
    ];
    boundaries.sort_by(f64::total_cmp);

    for index in 0..boundaries.len() {
        let start = boundaries[index];
        let end = if index + 1 < boundaries.len() {
            boundaries[index + 1]
        } else {
            boundaries[0] + std::f64::consts::TAU
        };
        let selected = if selected + 1.0e-12 < start {
            selected + std::f64::consts::TAU
        } else {
            selected
        };
        if selected >= start - 1.0e-12 && selected <= end + 1.0e-12 {
            return Some((end - start).clamp(0.0, std::f64::consts::PI));
        }
    }
    None
}

/// Ordinate dimension entity
///
/// Measures the X or Y ordinate of a point.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionOrdinate {
    pub base: DimensionBase,
    /// Definition point (origin) - in WCS
    pub definition_point: Vector3,
    /// Feature location point (in WCS)
    pub feature_location: Vector3,
    /// Leader endpoint (in WCS)
    pub leader_endpoint: Vector3,
    /// True if this is an X-ordinate, false for Y-ordinate
    pub is_ordinate_type_x: bool,
}

impl DimensionOrdinate {
    /// Create a new ordinate dimension
    pub fn new(feature_location: Vector3, leader_endpoint: Vector3, is_x_type: bool) -> Self {
        let mut value = Self {
            base: DimensionBase::new(DimensionType::Ordinate),
            definition_point: Vector3::ZERO,
            feature_location,
            leader_endpoint,
            is_ordinate_type_x: is_x_type,
        };
        value.refresh_measurement();
        value
    }

    /// Create a new X-ordinate dimension
    pub fn x_ordinate(feature_location: Vector3, leader_endpoint: Vector3) -> Self {
        Self::new(feature_location, leader_endpoint, true)
    }

    /// Create a new Y-ordinate dimension
    pub fn y_ordinate(feature_location: Vector3, leader_endpoint: Vector3) -> Self {
        Self::new(feature_location, leader_endpoint, false)
    }

    /// Get the ordinate measurement
    pub fn measurement(&self) -> f64 {
        let delta = self.feature_location - self.definition_point;
        let wcs_to_ocs = Matrix3::arbitrary_axis(self.base.normal).transpose();
        let delta = wcs_to_ocs * delta;
        let angle = -self.base.horizontal_direction;
        let (sin, cos) = angle.sin_cos();
        let projected = if self.is_ordinate_type_x {
            delta.x * cos + delta.y * sin
        } else {
            -delta.x * sin + delta.y * cos
        };
        projected.abs()
    }

    /// Recompute the cached measurement.
    pub fn refresh_measurement(&mut self) {
        self.base.actual_measurement = self.measurement();
    }

    /// Return the dimension-local X and Y axes in world coordinates.
    pub fn local_axes(&self) -> (Vector3, Vector3) {
        let basis = Matrix3::arbitrary_axis(self.base.normal);
        let angle = -self.base.horizontal_direction;
        let (sin, cos) = angle.sin_cos();
        (
            basis * Vector3::new(cos, sin, 0.0),
            basis * Vector3::new(-sin, cos, 0.0),
        )
    }

    /// Derive the visible ordinate leader.
    pub fn leader_polyline(
        &self,
        dogleg_length: f64,
        extension_offset: f64,
        fixed_extension_length: Option<f64>,
    ) -> [Vector3; 4] {
        let (x_axis, y_axis) = self.local_axes();
        let (main_axis, landing_axis) = if self.is_ordinate_type_x {
            (y_axis, x_axis)
        } else {
            (x_axis, y_axis)
        };
        let delta = self.leader_endpoint - self.feature_location;
        let signed = |value: f64| if value < 0.0 { -1.0 } else { 1.0 };
        let main_direction = main_axis * signed(delta.dot(&main_axis));
        let landing_direction = landing_axis * signed(delta.dot(&landing_axis));
        let main_distance = delta.dot(&main_direction).abs();
        let dogleg = dogleg_length.max(0.0);
        let first_leg = (main_distance - 2.0 * dogleg).max(dogleg);
        let elbow = self.feature_location + main_direction * first_leg;
        let landing_start = self.leader_endpoint - landing_direction * dogleg;
        let extension_start = if let Some(length) = fixed_extension_length {
            elbow - main_direction * length.max(0.0)
        } else {
            self.feature_location + main_direction * extension_offset.max(0.0)
        };
        [extension_start, elbow, landing_start, self.leader_endpoint]
    }
}

impl Default for DimensionOrdinate {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::Ordinate),
            definition_point: Vector3::ZERO,
            feature_location: Vector3::ZERO,
            leader_endpoint: Vector3::ZERO,
            is_ordinate_type_x: true,
        }
    }
}

/// Arc-length dimension entity.
///
/// Measures all or part of a circular arc and optionally carries a leader.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionArc {
    pub base: DimensionBase,
    pub definition_point: Vector3,
    pub first_extension_point: Vector3,
    pub second_extension_point: Vector3,
    pub center_point: Vector3,
    pub is_partial: bool,
    pub arc_start_parameter: f64,
    pub arc_end_parameter: f64,
    pub has_leader: bool,
    pub first_leader_point: Vector3,
    pub second_leader_point: Vector3,
}

impl DimensionArc {
    pub fn measurement(&self) -> f64 {
        let radius = self.center_point.distance(&self.first_extension_point);
        let raw_sweep = self.arc_end_parameter - self.arc_start_parameter;
        let mut sweep = raw_sweep.rem_euclid(std::f64::consts::TAU);
        // A complete turn is congruent to zero after normalization. Preserve
        // that explicit full-circle intent while still treating identical
        // start/end parameters as an empty measurement.
        if sweep <= 1.0e-12 && raw_sweep.abs() > 1.0e-12 {
            sweep = std::f64::consts::TAU;
        }
        if sweep <= 1.0e-12 && raw_sweep.abs() <= 1.0e-12 {
            let start = (self.first_extension_point.y - self.center_point.y)
                .atan2(self.first_extension_point.x - self.center_point.x);
            let end = (self.second_extension_point.y - self.center_point.y)
                .atan2(self.second_extension_point.x - self.center_point.x);
            sweep = (end - start).rem_euclid(std::f64::consts::TAU);
        }
        radius * sweep
    }
}

impl Default for DimensionArc {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::ArcLength),
            definition_point: Vector3::ZERO,
            first_extension_point: Vector3::ZERO,
            second_extension_point: Vector3::ZERO,
            center_point: Vector3::ZERO,
            is_partial: false,
            arc_start_parameter: 0.0,
            arc_end_parameter: 0.0,
            has_leader: false,
            first_leader_point: Vector3::ZERO,
            second_leader_point: Vector3::ZERO,
        }
    }
}

/// Jogged radial dimension used when the true circle center lies outside the
/// useful drawing area.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimensionLargeRadial {
    pub base: DimensionBase,
    pub definition_point: Vector3,
    pub chord_point: Vector3,
    pub jog_angle: f64,
    pub override_center: Vector3,
    pub jog_point: Vector3,
}

impl DimensionLargeRadial {
    pub fn measurement(&self) -> f64 {
        self.definition_point.distance(&self.chord_point)
    }
}

impl Default for DimensionLargeRadial {
    fn default() -> Self {
        Self {
            base: DimensionBase::new(DimensionType::LargeRadial),
            definition_point: Vector3::ZERO,
            chord_point: Vector3::ZERO,
            jog_angle: 0.0,
            override_center: Vector3::ZERO,
            jog_point: Vector3::ZERO,
        }
    }
}

/// Unified dimension enum for all dimension types
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Dimension {
    Aligned(DimensionAligned),
    Linear(DimensionLinear),
    Radius(DimensionRadius),
    Diameter(DimensionDiameter),
    Angular2Ln(DimensionAngular2Ln),
    Angular3Pt(DimensionAngular3Pt),
    Ordinate(DimensionOrdinate),
    Arc(DimensionArc),
    LargeRadial(DimensionLargeRadial),
}

impl Dimension {
    /// Get the base dimension data
    pub fn base(&self) -> &DimensionBase {
        match self {
            Dimension::Aligned(d) => &d.base,
            Dimension::Linear(d) => &d.base,
            Dimension::Radius(d) => &d.base,
            Dimension::Diameter(d) => &d.base,
            Dimension::Angular2Ln(d) => &d.base,
            Dimension::Angular3Pt(d) => &d.base,
            Dimension::Ordinate(d) => &d.base,
            Dimension::Arc(d) => &d.base,
            Dimension::LargeRadial(d) => &d.base,
        }
    }

    /// Get mutable base dimension data
    pub fn base_mut(&mut self) -> &mut DimensionBase {
        match self {
            Dimension::Aligned(d) => &mut d.base,
            Dimension::Linear(d) => &mut d.base,
            Dimension::Radius(d) => &mut d.base,
            Dimension::Diameter(d) => &mut d.base,
            Dimension::Angular2Ln(d) => &mut d.base,
            Dimension::Angular3Pt(d) => &mut d.base,
            Dimension::Ordinate(d) => &mut d.base,
            Dimension::Arc(d) => &mut d.base,
            Dimension::LargeRadial(d) => &mut d.base,
        }
    }

    /// Get the authoritative definition point for this dimension subtype.
    pub fn definition_point(&self) -> Vector3 {
        match self {
            Dimension::Aligned(d) => d.definition_point,
            Dimension::Linear(d) => d.definition_point,
            Dimension::Radius(d) => d.definition_point,
            Dimension::Diameter(d) => d.definition_point,
            Dimension::Angular2Ln(d) => d.definition_point,
            Dimension::Angular3Pt(d) => d.definition_point,
            Dimension::Ordinate(d) => d.definition_point,
            Dimension::Arc(d) => d.definition_point,
            Dimension::LargeRadial(d) => d.definition_point,
        }
    }

    /// Update the subtype and compatibility copy of the definition point.
    pub fn set_definition_point(&mut self, point: Vector3) {
        match self {
            Dimension::Aligned(d) => d.definition_point = point,
            Dimension::Linear(d) => d.definition_point = point,
            Dimension::Radius(d) => d.definition_point = point,
            Dimension::Diameter(d) => d.definition_point = point,
            Dimension::Angular2Ln(d) => d.definition_point = point,
            Dimension::Angular3Pt(d) => d.definition_point = point,
            Dimension::Ordinate(d) => d.definition_point = point,
            Dimension::Arc(d) => d.definition_point = point,
            Dimension::LargeRadial(d) => d.definition_point = point,
        }
        self.base_mut().definition_point = point;
    }

    /// Get the measurement value
    pub fn measurement(&self) -> f64 {
        match self {
            Dimension::Aligned(d) => d.measurement(),
            Dimension::Linear(d) => d.measurement(),
            Dimension::Radius(d) => d.measurement(),
            Dimension::Diameter(d) => d.measurement(),
            Dimension::Angular2Ln(d) => d.measurement_degrees(),
            Dimension::Angular3Pt(d) => d.measurement_degrees(),
            Dimension::Ordinate(d) => d.measurement(),
            Dimension::Arc(d) => d.measurement(),
            Dimension::LargeRadial(d) => d.measurement(),
        }
    }
}

impl super::Entity for Dimension {
    fn handle(&self) -> crate::types::Handle {
        self.base().common.handle
    }

    fn set_handle(&mut self, handle: crate::types::Handle) {
        self.base_mut().common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.base().common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.base_mut().common.layer = layer;
    }

    fn color(&self) -> crate::types::Color {
        self.base().common.color
    }

    fn set_color(&mut self, color: crate::types::Color) {
        self.base_mut().common.color = color;
    }

    fn line_weight(&self) -> crate::types::LineWeight {
        self.base().common.line_weight
    }

    fn set_line_weight(&mut self, weight: crate::types::LineWeight) {
        self.base_mut().common.line_weight = weight;
    }

    fn transparency(&self) -> crate::types::Transparency {
        self.base().common.transparency
    }

    fn set_transparency(&mut self, transparency: crate::types::Transparency) {
        self.base_mut().common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.base().common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.base_mut().common.invisible = invisible;
    }

    fn bounding_box(&self) -> crate::types::BoundingBox3D {
        use crate::types::BoundingBox3D;
        match self {
            Dimension::Aligned(d) => {
                BoundingBox3D::from_points(&[d.first_point, d.second_point, d.definition_point])
                    .unwrap_or_default()
            }
            Dimension::Linear(d) => {
                BoundingBox3D::from_points(&[d.first_point, d.second_point, d.definition_point])
                    .unwrap_or_default()
            }
            Dimension::Radius(d) => {
                BoundingBox3D::from_points(&[d.angle_vertex, d.definition_point])
                    .unwrap_or_default()
            }
            Dimension::Diameter(d) => {
                BoundingBox3D::from_points(&[d.angle_vertex, d.definition_point])
                    .unwrap_or_default()
            }
            Dimension::Angular2Ln(d) => BoundingBox3D::from_points(&[
                d.dimension_arc,
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
            ])
            .unwrap_or_default(),
            Dimension::Angular3Pt(d) => BoundingBox3D::from_points(&[
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
            ])
            .unwrap_or_default(),
            Dimension::Ordinate(d) => {
                BoundingBox3D::from_points(&[d.feature_location, d.leader_endpoint])
                    .unwrap_or_default()
            }
            Dimension::Arc(d) => BoundingBox3D::from_points(&[
                d.definition_point,
                d.first_extension_point,
                d.second_extension_point,
                d.center_point,
                d.first_leader_point,
                d.second_leader_point,
            ])
            .unwrap_or_default(),
            Dimension::LargeRadial(d) => BoundingBox3D::from_points(&[
                d.definition_point,
                d.chord_point,
                d.override_center,
                d.jog_point,
            ])
            .unwrap_or_default(),
        }
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_dimension(self, offset);
    }

    fn apply_transform(&mut self, transform: &Transform) {
        let Dimension::Ordinate(d) = self else {
            let translated = transform.apply(Vector3::ZERO);
            self.translate(translated);
            return;
        };

        let old_normal = d.base.normal;
        let old_basis = Matrix3::arbitrary_axis(old_normal);
        let old_axis_angle = -d.base.horizontal_direction;
        let old_axis = old_basis * Vector3::new(old_axis_angle.cos(), old_axis_angle.sin(), 0.0);
        let old_text_angle = old_axis_angle + d.base.text_rotation;
        let old_text_axis =
            old_basis * Vector3::new(old_text_angle.cos(), old_text_angle.sin(), 0.0);

        d.definition_point = transform.apply(d.definition_point);
        d.feature_location = transform.apply(d.feature_location);
        d.leader_endpoint = transform.apply(d.leader_endpoint);
        d.base.definition_point = d.definition_point;
        d.base.text_middle_point = transform.apply(d.base.text_middle_point);
        d.base.insertion_point = transform.apply(d.base.insertion_point);

        let transformed_normal = transform.apply_rotation(old_normal);
        if transformed_normal.length() > 1e-12 {
            d.base.normal = transformed_normal.normalize();
        }
        let new_wcs_to_ocs = Matrix3::arbitrary_axis(d.base.normal).transpose();
        let transformed_axis = new_wcs_to_ocs * transform.apply_rotation(old_axis);
        let mut new_axis_angle = old_axis_angle;
        if transformed_axis.length() > 1e-12 {
            new_axis_angle = transformed_axis.y.atan2(transformed_axis.x);
            d.base.horizontal_direction = -new_axis_angle;
        }
        let transformed_text_axis = new_wcs_to_ocs * transform.apply_rotation(old_text_axis);
        if transformed_text_axis.length() > 1e-12 {
            let relative = transformed_text_axis.y.atan2(transformed_text_axis.x) - new_axis_angle;
            d.base.text_rotation = relative.sin().atan2(relative.cos());
        }
        d.refresh_measurement();
    }

    fn entity_type(&self) -> &'static str {
        match self {
            Dimension::Aligned(_) => "DIMENSION_ALIGNED",
            Dimension::Linear(_) => "DIMENSION_LINEAR",
            Dimension::Radius(_) => "DIMENSION_RADIUS",
            Dimension::Diameter(_) => "DIMENSION_DIAMETER",
            Dimension::Angular2Ln(_) => "DIMENSION_ANGULAR_2LINE",
            Dimension::Angular3Pt(_) => "DIMENSION_ANGULAR_3POINT",
            Dimension::Ordinate(_) => "DIMENSION_ORDINATE",
            Dimension::Arc(_) => "ARC_DIMENSION",
            Dimension::LargeRadial(_) => "LARGE_RADIAL_DIMENSION",
        }
    }
}
