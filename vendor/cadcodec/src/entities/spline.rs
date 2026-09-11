//! Spline entity (NURBS curve)

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Spline flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SplineFlags {
    /// Is the spline closed?
    pub closed: bool,
    /// Is the spline periodic?
    pub periodic: bool,
    /// Is the spline rational?
    pub rational: bool,
    /// Is the spline planar?
    pub planar: bool,
    /// Is the spline linear?
    pub linear: bool,
}

impl SplineFlags {
    /// Create default spline flags
    pub fn new() -> Self {
        SplineFlags {
            closed: false,
            periodic: false,
            rational: false,
            planar: false,
            linear: false,
        }
    }
}

impl Default for SplineFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// A spline entity (NURBS curve)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Spline {
    /// Common entity data
    pub common: EntityCommon,
    /// Degree of the spline (typically 3 for cubic)
    pub degree: i32,
    /// Spline flags
    pub flags: SplineFlags,
    /// Knot values
    pub knots: Vec<f64>,
    /// Control points
    pub control_points: Vec<Vector3>,
    /// Weights (for rational splines)
    pub weights: Vec<f64>,
    /// Fit points (if available)
    pub fit_points: Vec<Vector3>,
    /// Normal vector
    pub normal: Vector3,
    /// Knot tolerance (DXF 42).
    pub knot_tolerance: f64,
    /// Control-point tolerance (DXF 43).
    pub control_tolerance: f64,
    /// Fit tolerance (DXF 44).
    pub fit_tolerance: f64,
    /// Start tangent vector (DXF 12/22/32); zero when unset.
    pub begin_tangent: Vector3,
    /// End tangent vector (DXF 13/23/33); zero when unset.
    pub end_tangent: Vector3,
    /// Knot parameterization method (R2013+ DWG): 0=Chord, 1=SquareRoot,
    /// 2=Uniform, 15=Custom. Zero for splines saved before R2013.
    pub knot_parameterization: i32,
    /// Show the control-vertex frame (R2013+ DWG flag).
    pub cv_frame_visible: bool,
    /// Complete R2013+ spline flag word, including flags not otherwise
    /// represented by this API.
    pub dwg_flags1: i32,
}

impl Spline {
    /// Create a new spline
    pub fn new() -> Self {
        Spline {
            common: EntityCommon::new(),
            degree: 3,
            flags: SplineFlags::new(),
            knots: Vec::new(),
            control_points: Vec::new(),
            weights: Vec::new(),
            fit_points: Vec::new(),
            normal: Vector3::UNIT_Z,
            knot_tolerance: 0.0,
            control_tolerance: 0.0,
            fit_tolerance: 0.0,
            begin_tangent: Vector3::ZERO,
            end_tangent: Vector3::ZERO,
            knot_parameterization: 0,
            cv_frame_visible: false,
            dwg_flags1: 0,
        }
    }

    /// Create a spline from control points
    pub fn from_control_points(degree: i32, control_points: Vec<Vector3>) -> Self {
        let knots = Self::generate_clamped_knots(degree as usize, control_points.len());
        Spline {
            degree,
            control_points,
            knots,
            ..Self::new()
        }
    }

    /// Create a spline from fit points
    pub fn from_fit_points(fit_points: Vec<Vector3>) -> Self {
        Spline {
            fit_points,
            ..Self::new()
        }
    }

    /// Generate a clamped uniform knot vector for the given degree and
    /// number of control points.
    ///
    /// The result has `n + p + 1` elements: `p+1` zeros, evenly-spaced
    /// internal knots, and `p+1` ones.
    pub fn generate_clamped_knots(degree: usize, num_control_points: usize) -> Vec<f64> {
        if num_control_points == 0 {
            return Vec::new();
        }
        let n = num_control_points;
        let p = degree;
        let m = n + p + 1;
        let mut kv = Vec::with_capacity(m);
        for _ in 0..=p {
            kv.push(0.0);
        }
        let internal = m - 2 * (p + 1);
        for i in 1..=internal {
            kv.push(i as f64 / (internal + 1) as f64);
        }
        for _ in 0..=p {
            kv.push(1.0);
        }
        kv
    }

    /// Get the number of control points
    pub fn control_point_count(&self) -> usize {
        self.control_points.len()
    }

    /// Get the number of knots
    pub fn knot_count(&self) -> usize {
        self.knots.len()
    }

    /// Add a control point
    pub fn add_control_point(&mut self, point: Vector3) {
        self.control_points.push(point);
    }

    /// Add a knot value
    pub fn add_knot(&mut self, knot: f64) {
        self.knots.push(knot);
    }
}

impl Default for Spline {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Spline {
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
        if self.control_points.is_empty() {
            if self.fit_points.is_empty() {
                return BoundingBox3D::from_point(Vector3::ZERO);
            }
            return BoundingBox3D::from_points(&self.fit_points).unwrap();
        }
        BoundingBox3D::from_points(&self.control_points).unwrap()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_spline(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "SPLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_spline(self, transform);
    }
}
