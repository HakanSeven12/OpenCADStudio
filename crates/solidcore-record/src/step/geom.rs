//! STEP geometry (ISO 10303-42) as plain data: the curves and surfaces a file defines, exactly
//! as it defines them, with lengths in millimetres and angles in radians. Nothing is
//! approximated, bounded, normalised or turned into another kind on the way in: a circle stays a
//! circle with its placement, an unbounded cylinder stays unbounded, a B-spline keeps its own
//! knots and form, and a direction keeps the ratios the file writes. Every curve and surface keeps
//! STEP's own parameterisation, so a 2D curve in a surface's parameter space means the same thing
//! here as in the file.
//!
//! Entities the file shares (one surface under many edges' pcurves) are shared here too, through
//! `Arc`, so reading is linear in the file however often an entity is referenced.
//!
//! The record computes no geometric quantity (docs/layers.md §1, §4): a direction is not made a
//! unit vector, a placement's reference direction is not made orthogonal to its axis (nor chosen
//! when the file omits it), its y axis is not formed, a vector's length is not taken, and nothing
//! is evaluated. Those are the kernel's, on its own types, when it builds from this data (#2578,
//! slice 4). The formulas on the variants below say what the data means, in ISO 10303-42's
//! terms; they are not computed here.
//!
//! Origin: SolidCraft's `crates/step/src/geom.rs` at `a5b71ca2` (MIT OR Apache-2.0).
//!
//! Deviations from SolidCraft: [`Placement`] keeps `axis` and `ref_direction` as the file gives
//! them (each optional, neither normalised; was: both unit and orthogonal, a missing reference
//! direction chosen perpendicular to the axis), and [`Axis1::direction`] likewise. A `VECTOR` is
//! a [`Vector`] of the written orientation, magnitude and unit factors (was: a unit direction
//! and the vector's length), in [`Curve::Line`] and [`Surface::Extrusion`]. Removed, to come back
//! as kernel primitives in slice 4: `Placement::y`, `Placement::at` and the vector helpers `add`,
//! `sub`, `scale`, `dot`, `cross`, `norm`. Doc comments on every public item and field.

use std::sync::Arc;

/// A point or vector: `[x, y, z]` (z = 0 in a 2D parameter space).
pub type P3 = [f64; 3];

/// `AXIS2_PLACEMENT_3D`, or `AXIS2_PLACEMENT_2D` (no `axis`): an origin and the directions of
/// its local Z and X axes, as the file writes them.
///
/// ISO 10303-42 makes the local axes from these: Z is `axis` (+Z when absent), X is
/// `ref_direction` made orthogonal to Z (a chosen perpendicular when absent), Y is Z × X. That
/// construction, with its checks (a zero or parallel reference direction), is the kernel's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The location, in millimetres (or the parameter space's units).
    pub origin: P3,
    /// Local Z as the file gives it (not normalised); `None` when the file omits it and for an
    /// `AXIS2_PLACEMENT_2D`, which ISO 10303-42 defines as +Z.
    pub axis: Option<P3>,
    /// Local X as the file gives it (not normalised, not made orthogonal to `axis`); `None` when
    /// the file omits it.
    pub ref_direction: Option<P3>,
}

/// `AXIS1_PLACEMENT`: an origin and a direction, as the file writes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis1 {
    /// The location, in millimetres.
    pub origin: P3,
    /// The direction as the file gives it (not normalised); `None` when the file omits it, which
    /// ISO 10303-42 defines as +Z.
    pub direction: Option<P3>,
}

/// `VECTOR`: an orientation and a magnitude, as the file writes them, and the unit factors of
/// the space they are in.
///
/// In millimetres and radians, component `i` of the vector is
/// `scale[i] · magnitude · orientation[i] / |orientation|`. The record does not form it; the
/// kernel does (slice 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    /// The direction ratios as written (not normalised; never all zero).
    pub orientation: P3,
    /// The magnitude as written, in file units (positive).
    pub magnitude: f64,
    /// The unit factor of each component: the length unit in model space, the parameter's unit
    /// (length, angle, or 1) in a surface's parameter space.
    pub scale: P3,
}

/// A STEP LOGICAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logical {
    /// `.T.`
    True,
    /// `.F.`
    False,
    /// `.U.`
    Unknown,
}

/// `b_spline_curve_form`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveForm {
    /// `.POLYLINE_FORM.`
    PolylineForm,
    /// `.CIRCULAR_ARC.`
    CircularArc,
    /// `.ELLIPTIC_ARC.`
    EllipticArc,
    /// `.PARABOLIC_ARC.`
    ParabolicArc,
    /// `.HYPERBOLIC_ARC.`
    HyperbolicArc,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// `b_spline_surface_form`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceForm {
    /// `.PLANE_SURF.`
    PlaneSurf,
    /// `.CYLINDRICAL_SURF.`
    CylindricalSurf,
    /// `.CONICAL_SURF.`
    ConicalSurf,
    /// `.SPHERICAL_SURF.`
    SphericalSurf,
    /// `.TOROIDAL_SURF.`
    ToroidalSurf,
    /// `.SURF_OF_REVOLUTION.`
    SurfOfRevolution,
    /// `.RULED_SURF.`
    RuledSurf,
    /// `.GENERALISED_CONE.`
    GeneralisedCone,
    /// `.QUADRIC_SURF.`
    QuadricSurf,
    /// `.SURF_OF_LINEAR_EXTRUSION.`
    SurfOfLinearExtrusion,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// `knot_type`. A B-spline given without knots (`BEZIER_CURVE`, `UNIFORM_CURVE`,
/// `QUASI_UNIFORM_CURVE` and their surfaces) has them laid out on reading, and keeps the
/// matching type here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnotSpec {
    /// `.UNIFORM_KNOTS.`
    UniformKnots,
    /// `.QUASI_UNIFORM_KNOTS.`
    QuasiUniformKnots,
    /// `.PIECEWISE_BEZIER_KNOTS.`
    PiecewiseBezierKnots,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// A B-spline curve: distinct knots with their multiplicities, as STEP writes them.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineCurve {
    /// The degree (1 to `read::MAX_DEGREE`).
    pub degree: usize,
    /// The control points.
    pub points: Vec<P3>,
    /// Present for a rational curve: one positive weight per point.
    pub weights: Option<Vec<f64>>,
    /// The distinct knots, increasing.
    pub knots: Vec<f64>,
    /// Each knot's multiplicity (1 to degree + 1); they add up to points + degree + 1.
    pub multiplicities: Vec<usize>,
    /// The curve form.
    pub form: CurveForm,
    /// `closed_curve`.
    pub closed: Logical,
    /// `self_intersect`.
    pub self_intersect: Logical,
    /// The knot type.
    pub knot_spec: KnotSpec,
}

/// A B-spline surface; `points[i][j]` runs along u with `i`, along v with `j`.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineSurface {
    /// The degree in u.
    pub u_degree: usize,
    /// The degree in v.
    pub v_degree: usize,
    /// The control net, rows along u, every row the same length.
    pub points: Vec<Vec<P3>>,
    /// Present for a rational surface: one positive weight per control point.
    pub weights: Option<Vec<Vec<f64>>>,
    /// The distinct u knots, increasing.
    pub u_knots: Vec<f64>,
    /// Each u knot's multiplicity.
    pub u_multiplicities: Vec<usize>,
    /// The distinct v knots, increasing.
    pub v_knots: Vec<f64>,
    /// Each v knot's multiplicity.
    pub v_multiplicities: Vec<usize>,
    /// The surface form.
    pub form: SurfaceForm,
    /// `u_closed`.
    pub u_closed: Logical,
    /// `v_closed`.
    pub v_closed: Logical,
    /// `self_intersect`.
    pub self_intersect: Logical,
    /// The knot type.
    pub knot_spec: KnotSpec,
}

/// One end of a `TRIMMED_CURVE`: a point on the basis curve or its parameter (a trim lists one
/// or both).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trim {
    /// A `CARTESIAN_POINT`.
    Point(P3),
    /// A `PARAMETER_VALUE` (radians on a circle or an ellipse).
    Parameter(f64),
}

/// `trimming_preference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimPreference {
    /// `.CARTESIAN.`
    Cartesian,
    /// `.PARAMETER.`
    Parameter,
    /// `.UNSPECIFIED.`, or anything else.
    Unspecified,
}

/// `preferred_surface_curve_representation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferredRep {
    /// `.CURVE_3D.`, or anything else.
    Curve3d,
    /// `.PCURVE_S1.`
    PcurveS1,
    /// `.PCURVE_S2.`
    PcurveS2,
}

/// Which `SURFACE_CURVE` subtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceCurveKind {
    /// `SURFACE_CURVE`.
    Surface,
    /// `SEAM_CURVE`.
    Seam,
    /// `INTERSECTION_CURVE`.
    Intersection,
    /// `BOUNDED_SURFACE_CURVE`.
    BoundedSurface,
}

/// `PCURVE`: a 2D curve in the parameter space of `surface`. Its coordinates are the surface's
/// own parameters (lengths in mm, angles in radians).
#[derive(Clone, Debug, PartialEq)]
pub struct Pcurve {
    /// The surface.
    pub surface: Arc<Surface>,
    /// A 2D curve: every point and placement has z = 0 and no placement has an axis.
    pub curve: Arc<Curve>,
}

/// An item of a `SURFACE_CURVE`'s `associated_geometry`.
#[derive(Clone, Debug, PartialEq)]
pub enum Associated {
    /// A `PCURVE`.
    Pcurve(Pcurve),
    /// A surface.
    Surface(Arc<Surface>),
}

/// A STEP curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `LINE`: `origin + t·v`, with `v` the [`Vector`].
    Line {
        /// `pnt`.
        origin: P3,
        /// `dir`.
        vector: Vector,
    },
    /// `CIRCLE`. Parameter: the angle from the placement's X axis.
    Circle {
        /// The placement.
        position: Placement,
        /// The radius (positive).
        radius: f64,
    },
    /// `ELLIPSE`: `origin + a·cos t·X + b·sin t·Y`.
    Ellipse {
        /// The placement.
        position: Placement,
        /// `a`, along X (positive).
        semi_axis_1: f64,
        /// `b`, along Y (positive).
        semi_axis_2: f64,
    },
    /// `HYPERBOLA`: `origin + a·cosh t·X + b·sinh t·Y`.
    Hyperbola {
        /// The placement.
        position: Placement,
        /// `a` (positive).
        semi_axis: f64,
        /// `b` (positive).
        semi_imag_axis: f64,
    },
    /// `PARABOLA`: `origin + f·t²·X + 2f·t·Y`.
    Parabola {
        /// The placement.
        position: Placement,
        /// `f` (not zero).
        focal_dist: f64,
    },
    /// `B_SPLINE_CURVE` and its subtypes.
    BSpline(Box<BSplineCurve>),
    /// `POLYLINE`: segment `i` (from 0) is the parameters `[i, i + 1]`.
    Polyline(Vec<P3>),
    /// `TRIMMED_CURVE`.
    Trimmed {
        /// `basis_curve`.
        basis: Arc<Curve>,
        /// `trim_1` (one or two items).
        trim_1: Vec<Trim>,
        /// `trim_2` (one or two items).
        trim_2: Vec<Trim>,
        /// `sense_agreement`.
        sense: bool,
        /// `master_representation`.
        preference: TrimPreference,
    },
    /// `SURFACE_CURVE`, `SEAM_CURVE`, `INTERSECTION_CURVE`, `BOUNDED_SURFACE_CURVE`.
    SurfaceCurve {
        /// Which subtype.
        kind: SurfaceCurveKind,
        /// `curve_3d`.
        curve_3d: Arc<Curve>,
        /// `associated_geometry` (at most two items).
        associated: Vec<Associated>,
        /// `master_representation`.
        master: PreferredRep,
    },
    /// `OFFSET_CURVE_3D`.
    OffsetCurve3d {
        /// `basis_curve`.
        basis: Arc<Curve>,
        /// `distance`, in millimetres.
        distance: f64,
        /// `self_intersect`.
        self_intersect: Logical,
        /// `ref_direction` as the file gives it (not normalised).
        ref_direction: P3,
    },
}

/// A STEP surface.
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// `PLANE`: `origin + u·X + v·Y`.
    Plane {
        /// The placement.
        position: Placement,
    },
    /// `CYLINDRICAL_SURFACE`: `origin + r·(cos u·X + sin u·Y) + v·Z`.
    Cylinder {
        /// The placement.
        position: Placement,
        /// `r` (positive).
        radius: f64,
    },
    /// `CONICAL_SURFACE`: `origin + (r + v·tan a)·(cos u·X + sin u·Y) + v·Z`.
    Cone {
        /// The placement.
        position: Placement,
        /// `r` (not negative).
        radius: f64,
        /// `a`, in radians, its size in (0, π/2).
        semi_angle: f64,
    },
    /// `SPHERICAL_SURFACE`: `origin + r·cos v·(cos u·X + sin u·Y) + r·sin v·Z`.
    Sphere {
        /// The placement.
        position: Placement,
        /// `r` (positive).
        radius: f64,
    },
    /// `TOROIDAL_SURFACE`: `origin + (R + r·cos v)·(cos u·X + sin u·Y) + r·sin v·Z`.
    Torus {
        /// The placement.
        position: Placement,
        /// `R` (may be negative: some writers use that for a spindle torus).
        major_radius: f64,
        /// `r` (positive).
        minor_radius: f64,
    },
    /// `DEGENERATE_TOROIDAL_SURFACE`: a torus whose minor radius reaches the axis;
    /// `select_outer` keeps the outer part.
    DegenerateTorus {
        /// The placement.
        position: Placement,
        /// `R`.
        major_radius: f64,
        /// `r` (positive).
        minor_radius: f64,
        /// `select_outer`.
        select_outer: bool,
    },
    /// `B_SPLINE_SURFACE` and its subtypes.
    BSpline(Box<BSplineSurface>),
    /// `SURFACE_OF_REVOLUTION`: the profile at `v` turned by the angle `u` about the axis.
    Revolution {
        /// `swept_curve`.
        profile: Arc<Curve>,
        /// `axis_position`.
        axis: Axis1,
    },
    /// `SURFACE_OF_LINEAR_EXTRUSION`: `profile(u) + v·w`, with `w` the [`Vector`].
    Extrusion {
        /// `swept_curve`.
        profile: Arc<Curve>,
        /// `extrusion_axis`.
        vector: Vector,
    },
    /// `OFFSET_SURFACE`.
    Offset {
        /// `basis_surface`.
        basis: Arc<Surface>,
        /// `distance`, in millimetres.
        distance: f64,
        /// `self_intersect`.
        self_intersect: Logical,
    },
    /// `RECTANGULAR_TRIMMED_SURFACE`, its bounds in the basis surface's parameter units.
    RectangularTrimmed {
        /// `basis_surface`.
        basis: Arc<Surface>,
        /// `u1`.
        u1: f64,
        /// `u2`.
        u2: f64,
        /// `v1`.
        v1: f64,
        /// `v2`.
        v2: f64,
        /// `usense`.
        u_sense: bool,
        /// `vsense`.
        v_sense: bool,
    },
}
