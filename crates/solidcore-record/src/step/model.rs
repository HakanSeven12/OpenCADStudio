//! The neutral intermediate: plain-data B-rep bodies, in millimetres and degrees.
//!
//! Every placement is already resolved to an orthonormal [`Frame`] and every length
//! is converted to millimetres, so a kernel builder only has to construct each curve,
//! surface and face as given. Indices are into the owning [`Body`]'s arrays.

/// A point or vector.
pub type P3 = [f64; 3];

/// A right-handed orthonormal frame (`AXIS2_PLACEMENT_3D` resolved per ISO 10303-42).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Location.
    pub origin: P3,
    /// Unit X (`ref_direction` made perpendicular to Z).
    pub x: P3,
    /// Unit Y = Z x X.
    pub y: P3,
    /// Unit Z (`axis`).
    pub z: P3,
}

/// A non-uniform rational B-spline in STEP's knot form.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineCurve {
    /// Degree, 1 to 32.
    pub degree: u32,
    /// Control points (mm).
    pub points: Vec<P3>,
    /// Weights, one per point, when rational.
    pub weights: Option<Vec<f64>>,
    /// Distinct knot values, increasing.
    pub knots: Vec<f64>,
    /// Multiplicity of each distinct knot; they sum to `points + degree + 1`.
    pub mults: Vec<u32>,
}

/// A curve in its STEP parameterisation.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// `LINE`: `origin + t * magnitude * dir` (dir unit).
    Line {
        /// Point at t = 0.
        origin: P3,
        /// Unit direction.
        dir: P3,
        /// Length per unit parameter (mm).
        magnitude: f64,
    },
    /// `CIRCLE`: `c + r (cos t x + sin t y)`, t in degrees.
    Circle {
        /// Centre and axes.
        frame: Frame,
        /// Radius (mm).
        radius: f64,
    },
    /// `ELLIPSE`: `c + a cos t x + b sin t y`, t in degrees.
    Ellipse {
        /// Centre and axes.
        frame: Frame,
        /// Semi axis along X (mm).
        semi1: f64,
        /// Semi axis along Y (mm).
        semi2: f64,
    },
    /// `B_SPLINE_CURVE_WITH_KNOTS` (rational or not), also `POLYLINE` (degree 1) and
    /// `QUASI_UNIFORM_CURVE` / `UNIFORM_CURVE` / `BEZIER_CURVE` with their implied knots.
    BSpline(BSplineCurve),
}

/// A B-spline surface in STEP's knot form. Points are row major: `points[i][j]` with
/// `i` along U.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineSurface {
    /// U degree.
    pub u_degree: u32,
    /// V degree.
    pub v_degree: u32,
    /// Control net, `nu` rows of `nv` points.
    pub points: Vec<Vec<P3>>,
    /// Weights in the same layout, when rational.
    pub weights: Option<Vec<Vec<f64>>>,
    /// Distinct U knots.
    pub u_knots: Vec<f64>,
    /// U multiplicities.
    pub u_mults: Vec<u32>,
    /// Distinct V knots.
    pub v_knots: Vec<f64>,
    /// V multiplicities.
    pub v_mults: Vec<u32>,
}

/// A surface in its STEP parameterisation (angles in degrees).
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// `PLANE`.
    Plane {
        /// Position; the plane is z = 0 in it.
        frame: Frame,
    },
    /// `CYLINDRICAL_SURFACE`.
    Cylinder {
        /// Axis frame.
        frame: Frame,
        /// Radius (mm).
        radius: f64,
    },
    /// `CONICAL_SURFACE`: radius `radius` at the frame origin.
    Cone {
        /// Axis frame.
        frame: Frame,
        /// Radius at the origin (mm).
        radius: f64,
        /// Half angle (degrees, 0 < a < 90).
        semi_angle_deg: f64,
    },
    /// `SPHERICAL_SURFACE`.
    Sphere {
        /// Centre frame.
        frame: Frame,
        /// Radius (mm).
        radius: f64,
    },
    /// `TOROIDAL_SURFACE`.
    Torus {
        /// Centre frame (Z the axis).
        frame: Frame,
        /// Major radius (mm).
        major: f64,
        /// Minor radius (mm).
        minor: f64,
    },
    /// `SURFACE_OF_REVOLUTION`.
    Revolution {
        /// Swept curve (the generatrix).
        curve: Box<Curve>,
        /// A point on the axis.
        axis_origin: P3,
        /// Unit axis direction.
        axis_dir: P3,
    },
    /// `SURFACE_OF_LINEAR_EXTRUSION`.
    Extrusion {
        /// Swept curve.
        curve: Box<Curve>,
        /// Unit extrusion direction.
        dir: P3,
        /// Length per unit parameter (mm).
        magnitude: f64,
    },
    /// `B_SPLINE_SURFACE_WITH_KNOTS`, rational or not.
    BSpline(BSplineSurface),
}

/// A topological vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// `#id` of the `VERTEX_POINT` (or of the point, for poly loops).
    pub step_id: u64,
    /// Position (mm).
    pub point: P3,
}

/// A topological edge with its bounded curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// `#id` of the `EDGE_CURVE` (0 for an edge made from a poly loop).
    pub step_id: u64,
    /// Index of the start vertex.
    pub start: usize,
    /// Index of the end vertex.
    pub end: usize,
    /// The edge geometry (for a `SURFACE_CURVE` / `SEAM_CURVE` / `INTERSECTION_CURVE`,
    /// its 3D curve).
    pub curve: Curve,
    /// `same_sense`: the curve runs from start to end.
    pub same_sense: bool,
    /// The curve parameter at the start vertex.
    pub t_start: f64,
    /// The curve parameter at the end vertex. For a closed curve the pair spans one
    /// period in the curve's direction: `t_end > t_start` when `same_sense`, else
    /// `t_end < t_start`.
    pub t_end: f64,
}

/// An edge used by a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrientedEdge {
    /// Index into [`Body::edges`].
    pub edge: usize,
    /// `orientation`: the loop runs the edge from start to end.
    pub forward: bool,
}

/// A face boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    /// `EDGE_LOOP` (also a `POLY_LOOP`, as line edges).
    Edges(Vec<OrientedEdge>),
    /// `VERTEX_LOOP`: a single vertex (index), e.g. a cone apex.
    Vertex(usize),
}

/// A face's loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// From a `FACE_OUTER_BOUND`.
    pub outer: bool,
    /// The bound's `orientation`: false reverses the loop.
    pub orientation: bool,
    /// The loop.
    pub bound: Bound,
}

/// A face.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// `#id` of the face.
    pub step_id: u64,
    /// Geometry. For a `FACE` of a faceted B-rep, the plane through its polygon.
    pub surface: Surface,
    /// `same_sense`: the face normal is the surface normal (after any
    /// `ORIENTED_FACE` reversal).
    pub same_sense: bool,
    /// Bounds in file order.
    pub loops: Vec<Loop>,
}

/// A shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    /// `#id` of the shell.
    pub step_id: u64,
    /// `CLOSED_SHELL` (else `OPEN_SHELL`).
    pub closed: bool,
    /// False for an `ORIENTED_CLOSED_SHELL` with orientation `.F.` (a void).
    pub orientation: bool,
    /// Faces, indices into [`Body::faces`].
    pub faces: Vec<usize>,
}

/// What kind of representation item a body came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// `MANIFOLD_SOLID_BREP`.
    ManifoldSolid,
    /// `BREP_WITH_VOIDS`.
    SolidWithVoids,
    /// `FACETED_BREP`.
    Faceted,
    /// `SHELL_BASED_SURFACE_MODEL`.
    ShellModel,
}

impl BodyKind {
    /// The STEP entity name.
    pub fn step_name(self) -> &'static str {
        match self {
            BodyKind::ManifoldSolid => "MANIFOLD_SOLID_BREP",
            BodyKind::SolidWithVoids => "BREP_WITH_VOIDS",
            BodyKind::Faceted => "FACETED_BREP",
            BodyKind::ShellModel => "SHELL_BASED_SURFACE_MODEL",
        }
    }
}

/// One B-rep body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// `#id` of the representation item.
    pub step_id: u64,
    /// The representation item's name, when it has one.
    pub name: Option<String>,
    /// Its kind.
    pub kind: BodyKind,
    /// Length unit of its representation context, in millimetres (already applied).
    pub unit_mm: f64,
    /// The context's distance uncertainty (mm), if it states one.
    pub uncertainty_mm: Option<f64>,
    /// Shells; the first is the outer one of a solid.
    pub shells: Vec<Shell>,
    /// Faces.
    pub faces: Vec<Face>,
    /// Edges.
    pub edges: Vec<Edge>,
    /// Vertices.
    pub vertices: Vec<Vertex>,
}

/// A non-fatal import diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Entity the diagnostic is about, when it has one.
    pub step_id: Option<u64>,
    /// Human-readable description.
    pub message: String,
}

/// A body that could not be mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// `#id` of the representation item.
    pub step_id: u64,
    /// Its entity name.
    pub kind: String,
    /// Why, e.g. `unsupported: OFFSET_SURFACE`.
    pub reason: String,
}

/// A 2D pcurve keyed by `(face step id, loop index, oriented-edge index)`.
pub type PcurveKey = (u64, usize, usize);

/// A curve in a surface's parameter space: the curve of a `PCURVE`, in the model's
/// parameter units of its surface (see [`Pcurve::curve`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Curve2d {
    /// `LINE`: `origin + t * dir`, `t` the file's line parameter.
    Line {
        /// Point at t = 0.
        origin: [f64; 2],
        /// Change per unit parameter (the file's direction times its magnitude).
        dir: [f64; 2],
    },
    /// `CIRCLE` / `ELLIPSE`: `centre + cos t * axis1 + sin t * axis2`, t in degrees. The
    /// axes are scaled to the surface's parameter units, so they need be neither
    /// perpendicular nor of equal length.
    Conic {
        /// Centre.
        centre: [f64; 2],
        /// The point at t = 0 relative to the centre.
        axis1: [f64; 2],
        /// The point at t = 90 degrees relative to the centre.
        axis2: [f64; 2],
    },
    /// `B_SPLINE_CURVE_WITH_KNOTS` (rational or not), `POLYLINE` and the implied-knot
    /// forms, as for [`Curve::BSpline`]. Each point's third coordinate is 0.
    BSpline(BSplineCurve),
}

/// One `PCURVE` of an edge's `SURFACE_CURVE`, `SEAM_CURVE`, `INTERSECTION_CURVE` or
/// `BOUNDED_SURFACE_CURVE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcurve {
    /// `#id` of the surface the curve lies in (`basis_surface`).
    pub surface: u64,
    /// The curve in that surface's parameters: a plane's u and v in mm; a cylinder's or
    /// cone's u in degrees and v in mm; a sphere's or torus's u and v in degrees; a
    /// B-spline surface's in its knot values.
    pub curve: Curve2d,
    /// The parameter values of a `TRIMMED_CURVE` around the curve, when it gives them
    /// (degrees for a conic), in file order.
    pub trim: Option<(f64, f64)>,
}

/// The `PCURVE`s of a file, for builders that make UV trim curves from them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilePcurves {
    /// By `EDGE_CURVE` `#id` ([`Edge::step_id`]): its pcurves, in file order.
    pub edges: std::collections::BTreeMap<u64, Vec<Pcurve>>,
    /// By face `#id` ([`Face::step_id`]): the `#id` of the face's surface.
    pub face_surfaces: std::collections::BTreeMap<u64, u64>,
    /// Pcurves that could not be mapped. Each leaves out only that pcurve; the bodies
    /// and [`Model::diagnostics`] are unaffected.
    pub diagnostics: Vec<Diagnostic>,
}

/// Everything imported from one file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    /// Part 21 header metadata.
    pub header: crate::step::p21::Header,
    /// `FILE_SCHEMA` names.
    pub schemas: Vec<String>,
    /// Bodies in entity id order. Includes a one-face open-shell
    /// [`BodyKind::ShellModel`] body for every face that no shell references.
    pub bodies: Vec<Body>,
    /// Bodies that could not be mapped, with the reason.
    pub skipped: Vec<Skipped>,
    /// Non-fatal diagnostics: missing/multiple unit contexts and ignored assembly
    /// instancing. An empty list means the model was mapped without ambiguity.
    pub diagnostics: Vec<Diagnostic>,
    /// Number of `MAPPED_ITEM`s and `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`s (assembly
    /// instancing), which are not applied: each body is imported once, in the
    /// coordinates of the representation that defines it.
    pub assembly_instances: usize,
    /// Surface-space trim curves, keyed by face/loop/edge use. Importers that carry
    /// pcurves populate this; exporters that do not need them leave it empty.
    pub pcurves: std::collections::BTreeMap<PcurveKey, Vec<Vec<[f64; 2]>>>,
    /// The file's own `PCURVE`s, keyed by edge and face (filled by the importer; an
    /// exporter ignores it).
    pub file_pcurves: FilePcurves,
}
