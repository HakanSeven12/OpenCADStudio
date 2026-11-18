//! Turning a solid into triangles.
//!
//! Nothing draws a B-rep directly. A renderer wants positions, normals and
//! indices, and so does anything measuring volume or exporting to a mesh
//! format — so this is the last step out of the kernel for most callers.
//!
//! # In parameter space, then lifted
//!
//! Each face is triangulated in its own `(u, v)`. Topological boundaries and
//! holes are constrained edges, then every vertex is mapped through the
//! surface. Curved faces are refined until their normal change is within the
//! requested angle.
//!
//! Doing it the other way round, triangulating in space, would mean deciding
//! what "inside the boundary" means on a curved patch, which is the question
//! parameter space already answers.
//!
//! # Orientation
//!
//! Every triangle comes out wound so its normal points out of the solid. A
//! face whose sense disagrees with its surface has its triangles reversed;
//! getting that wrong lights a solid inside out, and no amount of shading
//! afterwards recovers it.

use super::topology::{Body, EdgeKey, FaceKey};
use ocs_geom::geom2d::{constrained::ConstrainedMesh, triangulate};
use ocs_geom::space::Vec3;
use std::collections::{HashMap, HashSet};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, TAU};

type ParameterMap<T> = rustc_hash::FxHashMap<[u64; 2], T>;

/// A triangulated solid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Positions, in world coordinates.
    pub positions: Vec<[f64; 3]>,
    /// One outward unit normal per position.
    pub normals: Vec<[f64; 3]>,
    /// Three indices per triangle.
    pub triangles: Vec<[usize; 3]>,
}

impl Mesh {
    /// Volume and centroid of a closed, consistently wound mesh. Curved-body
    /// accuracy follows the tessellation tolerance. Empty or degenerate meshes
    /// return `None`; a local reference keeps survey coordinates well conditioned.
    pub fn mass_properties(&self) -> Option<(f64, [f64; 3])> {
        let properties = self.inertial_properties()?;
        Some((properties.volume, properties.centroid))
    }

    /// Volume, centroid and inertia of a closed, consistently wound mesh.
    /// A local reference keeps the integration stable at survey coordinates.
    pub fn inertial_properties(&self) -> Option<super::MassProperties> {
        // Measured about the middle of the mesh: a mesh a hair short of
        // closed — faces meshed apart along their shared edges — reads its
        // gap once per unit of distance to the reference, and the first
        // vertex, wherever the faces happened to start, is as far as the
        // part is long.
        let count = self.positions.len() as f64;
        let reference = self
            .positions
            .iter()
            .fold(Vec3::new(0.0, 0.0, 0.0), |sum, point| sum + Vec3::from(*point))
            / count;
        self.positions.first()?;
        let mut signed_volume = 0.0;
        let mut centroid_numerator = [0.0; 3];
        let mut squared_integrals = [0.0; 3];
        let mut product_integrals = [0.0; 3];
        for triangle in &self.triangles {
            let a = Vec3::from(*self.positions.get(triangle[0])?) - reference;
            let b = Vec3::from(*self.positions.get(triangle[1])?) - reference;
            let c = Vec3::from(*self.positions.get(triangle[2])?) - reference;
            if [a, b, c]
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .any(|value| !value.is_finite())
            {
                return None;
            }
            let points = [a.to_array(), b.to_array(), c.to_array()];
            let tetra = a.dot(b.cross(c)) / 6.0;
            signed_volume += tetra;
            for axis in 0..3 {
                let [a, b, c] = points.map(|point| point[axis]);
                centroid_numerator[axis] += tetra * (a + b + c) / 4.0;
                squared_integrals[axis] +=
                    tetra * (a * a + b * b + c * c + a * b + a * c + b * c) / 10.0;
            }
            for (index, (first, second)) in
                [(0usize, 1usize), (1, 2), (2, 0)].into_iter().enumerate()
            {
                let diagonal = points
                    .iter()
                    .map(|point| point[first] * point[second])
                    .sum::<f64>();
                let all = points.iter().map(|point| point[first]).sum::<f64>()
                    * points.iter().map(|point| point[second]).sum::<f64>();
                product_integrals[index] += tetra * (diagonal + all) / 20.0;
            }
        }
        if signed_volume == 0.0 || !signed_volume.is_finite() {
            return None;
        }
        let local_centroid = centroid_numerator.map(|value| value / signed_volume);
        let orientation = signed_volume.signum();
        let volume = signed_volume.abs();
        let square = squared_integrals.map(|value| value * orientation);
        let products = product_integrals.map(|value| -value * orientation);
        let moment = [
            square[1] + square[2],
            square[0] + square[2],
            square[0] + square[1],
        ];
        let central = [
            [
                moment[0] - volume * (local_centroid[1].powi(2) + local_centroid[2].powi(2)),
                products[0] + volume * local_centroid[0] * local_centroid[1],
                products[2] + volume * local_centroid[0] * local_centroid[2],
            ],
            [
                products[0] + volume * local_centroid[0] * local_centroid[1],
                moment[1] - volume * (local_centroid[0].powi(2) + local_centroid[2].powi(2)),
                products[1] + volume * local_centroid[1] * local_centroid[2],
            ],
            [
                products[2] + volume * local_centroid[0] * local_centroid[2],
                products[1] + volume * local_centroid[1] * local_centroid[2],
                moment[2] - volume * (local_centroid[0].powi(2) + local_centroid[1].powi(2)),
            ],
        ];
        let (principal_moments, principal_directions) = principal_axes(central);
        let centroid = (reference + Vec3::from(local_centroid)).to_array();
        let c = Vec3::from(centroid);
        let c2 = c.dot(c);
        let mut origin = central;
        for row in 0..3 {
            for column in 0..3 {
                origin[row][column] += volume
                    * (if row == column { c2 } else { 0.0 } - centroid[row] * centroid[column]);
            }
        }
        let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
        let product_of_inertia = [origin[0][1], origin[1][2], origin[2][0]];
        let radii_of_gyration = moment_of_inertia.map(|value| (value.max(0.0) / volume).sqrt());
        centroid
            .iter()
            .chain(moment_of_inertia.iter())
            .chain(principal_moments.iter())
            .chain(product_of_inertia.iter())
            .chain(radii_of_gyration.iter())
            .all(|value| value.is_finite())
            .then_some(super::MassProperties {
                volume,
                centroid,
                moment_of_inertia,
                principal_directions,
                principal_moments,
                product_of_inertia,
                radii_of_gyration,
            })
    }

    /// Area of all triangles in the mesh.
    pub fn surface_area(&self) -> Option<f64> {
        self.surface_properties().map(|(area, _)| area)
    }

    /// Area and area-weighted centroid of all triangles in the mesh.
    pub fn surface_properties(&self) -> Option<(f64, [f64; 3])> {
        let reference = Vec3::from(*self.positions.first()?);
        let mut moment = Vec3::ZERO;
        self.triangles
            .iter()
            .try_fold(0.0, |area, triangle| {
                let a = Vec3::from(*self.positions.get(triangle[0])?) - reference;
                let b = Vec3::from(*self.positions.get(triangle[1])?) - reference;
                let c = Vec3::from(*self.positions.get(triangle[2])?) - reference;
                let triangle_area = (b - a).cross(c - a).length() * 0.5;
                if !triangle_area.is_finite() {
                    return None;
                }
                moment = moment + (a + b + c) * (triangle_area / 3.0);
                Some(area + triangle_area)
            })
            .filter(|area| *area > 0.0)
            .map(|area| (area, (reference + moment / area).to_array()))
    }

    /// How many triangles it holds.
    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// Adds another mesh's triangles, keeping both.
    pub fn absorb(&mut self, other: Mesh) {
        let offset = self.positions.len();
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.triangles.extend(
            other
                .triangles
                .into_iter()
                .map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]),
        );
    }
}

pub(super) fn principal_axes(mut matrix: [[f64; 3]; 3]) -> ([f64; 3], [f64; 9]) {
    let mut vectors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..24 {
        let (p, q) = [(0usize, 1usize), (0, 2), (1, 2)]
            .into_iter()
            .max_by(|&(ap, aq), &(bp, bq)| matrix[ap][aq].abs().total_cmp(&matrix[bp][bq].abs()))
            .unwrap();
        if matrix[p][q].abs() <= 1e-12 {
            break;
        }
        let angle = 0.5 * (2.0 * matrix[p][q]).atan2(matrix[q][q] - matrix[p][p]);
        let (sine, cosine) = angle.sin_cos();
        for row in 0..3 {
            let (mp, mq) = (matrix[row][p], matrix[row][q]);
            matrix[row][p] = cosine * mp - sine * mq;
            matrix[row][q] = sine * mp + cosine * mq;
        }
        for column in 0..3 {
            let (mp, mq) = (matrix[p][column], matrix[q][column]);
            matrix[p][column] = cosine * mp - sine * mq;
            matrix[q][column] = sine * mp + cosine * mq;
        }
        for row in 0..3 {
            let (vp, vq) = (vectors[row][p], vectors[row][q]);
            vectors[row][p] = cosine * vp - sine * vq;
            vectors[row][q] = sine * vp + cosine * vq;
        }
    }
    let mut axes = (0..3)
        .map(|axis| {
            (
                matrix[axis][axis].max(0.0),
                [vectors[0][axis], vectors[1][axis], vectors[2][axis]],
            )
        })
        .collect::<Vec<_>>();
    axes.sort_by(|left, right| left.0.total_cmp(&right.0));
    let moments = [axes[0].0, axes[1].0, axes[2].0];
    let directions = if (moments[2] - moments[0]).abs() <= moments[2].abs().max(1.0) * 1e-10 {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    } else {
        [
            axes[0].1[0],
            axes[0].1[1],
            axes[0].1[2],
            axes[1].1[0],
            axes[1].1[1],
            axes[1].1[2],
            axes[2].1[0],
            axes[2].1[1],
            axes[2].1[2],
        ]
    };
    (moments, directions)
}

/// One tolerance policy for a body's faces and edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessellationTolerance {
    pub angle: f64,
    pub linear: f64,
    chordal: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: bool,
    solid_isolines: bool,
    triangle_budget: Option<usize>,
}

impl TessellationTolerance {
    pub fn new(angle: f64, linear: f64) -> Self {
        Self {
            angle: ocs_geom::tessellation::angle(angle),
            linear: finite_positive(linear, 1e-9),
            chordal: None,
            isolines: [0; 2],
            planar_isolines: false,
            solid_isolines: false,
            triangle_budget: None,
        }
    }

    /// Stop refining once the body has `triangles` triangles, for callers
    /// that would rather show a body incomplete than build a mesh far larger
    /// than they can use. The face being refined when the budget runs out,
    /// and every face after it, is left out and [`BodyMesh::over_budget`] is
    /// set; the work done stays near the budget. Without it (the default)
    /// there is no limit.
    pub fn with_triangle_budget(mut self, triangles: usize) -> Self {
        self.triangle_budget = Some(triangles);
        self
    }

    pub fn with_chordal_deflection(mut self, deflection: f64) -> Self {
        self.chordal = (deflection.is_finite() && deflection > 0.0).then_some(deflection);
        self
    }

    pub fn with_isolines(mut self, count: usize) -> Self {
        self.isolines = [count; 2];
        self
    }

    /// Use independent construction-line counts for the two surface
    /// parameter axes. The first count fixes U while the second fixes V.
    pub fn with_uv_isolines(mut self, u_count: usize, v_count: usize) -> Self {
        self.isolines = [u_count, v_count];
        self
    }

    /// Construction lines spaced as a solid draws them, `count` being the
    /// drawing's isoline setting (planes get none, other curved surfaces
    /// evenly spaced lines).
    /// Round the axis, a face going all the way round gets `count` lines a
    /// whole turn over `count` apart from the surface's zero angle, and a face
    /// covering part of a turn is cut into `count` times that part (rounded,
    /// plus one) equal pieces. A sphere's latitude span is cut into `count`
    /// equal pieces; cylinders and cones get no lines across their axis.
    pub fn with_solid_isolines(mut self, count: usize) -> Self {
        self.isolines = [count; 2];
        self.solid_isolines = true;
        self
    }

    /// Include parameter-space construction lines on planar faces.
    ///
    /// Planar isolines stay opt-in because solid-body display normally needs
    /// only the boundary edges, while editable sheet surfaces use the interior
    /// grid to expose their construction parameters.
    pub fn with_planar_isolines(mut self, enabled: bool) -> Self {
        self.planar_isolines = enabled;
        self
    }
}

/// Tessellation of one topological edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeMesh {
    pub edge: EdgeKey,
    pub parameters: Vec<f64>,
    pub positions: Vec<[f64; 3]>,
}

/// All display geometry derived from one body under one tolerance policy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyMesh {
    pub mesh: Mesh,
    pub triangle_faces: Vec<FaceKey>,
    /// Feature edges used by a conventional shaded-mesh wireframe.
    pub edges: Vec<EdgeMesh>,
    /// Every non-parameter-seam topological edge used by CAD drawing views.
    pub drawing_edges: Vec<EdgeMesh>,
    pub isolines: Vec<FacePolyline>,
    pub precision: f64,
    pub missing_faces: Vec<FaceKey>,
    /// The tolerance's triangle budget ran out: the faces from the one being
    /// refined then on are in `missing_faces`.
    pub over_budget: bool,
    analytic_cones: Vec<AnalyticConeFace>,
    /// Some face was filled with its boundary chords split where they cross
    /// (see `crossing_face`): its error no longer falls as the square of the
    /// angle, so no extrapolation from it holds.
    pub(super) crossed: bool,
}

/// Display-only body curves, without a face triangle mesh or silhouette data.
///
/// Failed edge schedules are omitted. `missing_faces` lists faces whose
/// requested isolines could not be generated; an empty list does not certify
/// that the body's faces can be triangulated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyWireframe {
    pub edges: Vec<EdgeMesh>,
    pub isolines: Vec<FacePolyline>,
    pub missing_faces: Vec<FaceKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FacePolyline {
    pub face: FaceKey,
    pub positions: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceMesh {
    pub mesh: Mesh,
    pub edges: Vec<Vec<[f64; 3]>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SilhouetteSource {
    sides: Vec<SilhouetteSide>,
    triangles: Vec<SilhouetteTriangle>,
    cones: Vec<ConeSilhouette>,
    precision: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct AnalyticConeFace {
    face: FaceKey,
    cone: ConeSilhouette,
}

#[derive(Debug, Clone, PartialEq)]
struct ConeSilhouette {
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    slope: f64,
    v_range: [f64; 2],
}

#[derive(Debug, Clone, PartialEq)]
struct SilhouetteSide {
    positions: [[f64; 3]; 2],
    normals: [[f64; 3]; 2],
}

#[derive(Debug, Clone, PartialEq)]
struct SilhouetteTriangle {
    positions: [[f64; 3]; 3],
    normals: [[f64; 3]; 3],
}

impl BodyMesh {
    pub fn silhouette_source(&self) -> SilhouetteSource {
        let mut groups = HashMap::new();
        let mut next = 0_u64;
        let cone_faces: HashSet<_> = self.analytic_cones.iter().map(|cone| cone.face).collect();
        let mut excluded_groups = HashSet::new();
        let triangle_groups = self
            .triangle_faces
            .iter()
            .map(|face| {
                let group = *groups.entry(*face).or_insert_with(|| {
                    let group = next;
                    next += 1;
                    group
                });
                if cone_faces.contains(face) {
                    excluded_groups.insert(group);
                }
                group
            })
            .collect::<Vec<_>>();
        let mut source = silhouette_source(
            &self.mesh,
            &triangle_groups,
            &excluded_groups,
            self.precision,
        );
        source.cones = self
            .analytic_cones
            .iter()
            .map(|cone| cone.cone.clone())
            .collect();
        source
    }
}

impl SurfaceMesh {
    pub fn silhouette_source(&self, precision: f64) -> SilhouetteSource {
        silhouette_source(
            &self.mesh,
            &vec![0; self.mesh.triangles.len()],
            &HashSet::new(),
            precision,
        )
    }
}

/// View-dependent smooth-face lines from the same triangles as the surface.
pub fn silhouette(source: &SilhouetteSource, view_direction: [f64; 3]) -> Vec<[f64; 3]> {
    let Some(view) = Vec3::from(view_direction).normalize() else {
        return Vec::new();
    };
    let seed = if view.x.abs() < 0.8 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let tangent = view.cross(seed).normalize().unwrap_or(seed);
    let bitangent = view.cross(tangent);
    // Keep a contour from landing exactly on a mesh vertex.
    let contour_view = (view + tangent * 1e-7 + bitangent * 6.180_339_887e-8)
        .normalize()
        .unwrap_or(view);
    let mut out = Vec::new();
    for side in &source.sides {
        let signs = side
            .normals
            .map(|normal| Vec3::from(normal).dot(contour_view).signum());
        if signs[0] != signs[1] {
            out.extend(side.positions);
        }
    }
    for triangle in &source.triangles {
        let values = triangle
            .normals
            .map(|normal| Vec3::from(normal).dot(contour_view));
        let mut crossings = Vec::with_capacity(2);
        for [from, to] in [[0, 1], [1, 2], [2, 0]] {
            if values[from].is_sign_positive() == values[to].is_sign_positive() {
                continue;
            }
            let t = values[from] / (values[from] - values[to]);
            crossings.push(
                (Vec3::from(triangle.positions[from])
                    + (Vec3::from(triangle.positions[to]) - Vec3::from(triangle.positions[from]))
                        * t)
                    .to_array(),
            );
        }
        if let [a, b] = crossings.as_slice() {
            out.extend([*a, *b]);
        }
    }
    for cone in &source.cones {
        append_cone_silhouette(&mut out, cone, contour_view);
    }
    out
}

fn append_cone_silhouette(out: &mut Vec<[f64; 3]>, cone: &ConeSilhouette, view: Vec3) {
    let x_axis = Vec3::from(cone.x_axis);
    let y_axis = Vec3::from(cone.y_axis);
    let axis = Vec3::from(cone.axis);
    let sin_coefficient = -x_axis.cross(axis).dot(view);
    let cos_coefficient = y_axis.cross(axis).dot(view);
    let constant = (x_axis.cross(y_axis) * cone.slope).dot(view);
    let amplitude = sin_coefficient.hypot(cos_coefficient);
    let tolerance = amplitude.max(constant.abs()).max(1.0) * 1e-12;
    if !amplitude.is_finite() || amplitude <= tolerance || constant.abs() > amplitude + tolerance {
        return;
    }
    let phase = sin_coefficient.atan2(cos_coefficient);
    let offset = (-constant / amplitude).clamp(-1.0, 1.0).acos();
    let count = if offset.abs() <= tolerance { 1 } else { 2 };
    for parameter in [phase - offset, phase + offset].into_iter().take(count) {
        let point_at = |v: f64| {
            let radial = cone.radius - cone.slope * v;
            (Vec3::from(cone.origin)
                + x_axis * (radial * parameter.cos())
                + y_axis * (radial * parameter.sin())
                + axis * v)
                .to_array()
        };
        out.extend(cone.v_range.map(point_at));
    }
}

fn silhouette_source(
    mesh: &Mesh,
    triangle_groups: &[u64],
    excluded_groups: &HashSet<u64>,
    precision: f64,
) -> SilhouetteSource {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct Point([i64; 3]);
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct Side {
        group: u64,
        a: Point,
        b: Point,
    }
    let precision = precision.max(1e-12);
    let origin = mesh.positions.first().copied().unwrap_or([0.0; 3]);
    let point_key = |point: [f64; 3]| {
        Point([0, 1, 2].map(|axis| ((point[axis] - origin[axis]) / precision).round() as i64))
    };
    let mut open: HashMap<Side, ([f64; 3], [[f64; 3]; 2])> = HashMap::new();
    let mut sides = Vec::new();
    let mut triangles = Vec::new();
    for (index, triangle) in mesh.triangles.iter().enumerate() {
        let Some(group) = triangle_groups.get(index).copied() else {
            continue;
        };
        if excluded_groups.contains(&group) {
            continue;
        }
        let positions = triangle.map(|vertex| mesh.positions[vertex]);
        let Some(normal) = (Vec3::from(positions[1]) - Vec3::from(positions[0]))
            .cross(Vec3::from(positions[2]) - Vec3::from(positions[0]))
            .normalize()
        else {
            continue;
        };
        let normals = triangle.map(|vertex| {
            mesh.normals
                .get(vertex)
                .and_then(|normal| Vec3::from(*normal).normalize())
                .unwrap_or(normal)
                .to_array()
        });
        let smooth = [[0, 1], [1, 2], [2, 0]]
            .into_iter()
            .any(|[a, b]| (Vec3::from(normals[a]) - Vec3::from(normals[b])).length() > 1e-10);
        if smooth {
            triangles.push(SilhouetteTriangle { positions, normals });
            continue;
        }
        for [from, to] in [[0, 1], [1, 2], [2, 0]] {
            let mut a = point_key(positions[from]);
            let mut b = point_key(positions[to]);
            let mut segment = [positions[from], positions[to]];
            if a.0 > b.0 {
                std::mem::swap(&mut a, &mut b);
                segment.swap(0, 1);
            }
            let key = Side { group, a, b };
            if let Some((other, previous)) = open.remove(&key) {
                sides.push(SilhouetteSide {
                    positions: previous,
                    normals: [other, normal.to_array()],
                });
            } else {
                open.insert(key, (normal.to_array(), segment));
            }
        }
    }
    SilhouetteSource {
        sides,
        triangles,
        cones: Vec::new(),
        precision,
    }
}

fn silhouette_precision(mesh: &Mesh) -> f64 {
    let scale = mesh
        .positions
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (scale * f64::EPSILON * 1024.0).max(1e-12)
}

pub fn transform_silhouette(
    source: &SilhouetteSource,
    placement: &super::place::Placement,
) -> Option<SilhouetteSource> {
    transform_silhouette_affine(
        source,
        [placement.x_axis, placement.y_axis, placement.z_axis],
        placement.origin,
    )
}

/// Moves a silhouette source by an affine map whose vectors are columns.
pub fn transform_silhouette_affine(
    source: &SilhouetteSource,
    vectors: [[f64; 3]; 3],
    origin: [f64; 3],
) -> Option<SilhouetteSource> {
    let vectors = vectors.map(Vec3::from);
    if origin.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let lengths = vectors.map(Vec3::length);
    if lengths
        .iter()
        .any(|length| !length.is_finite() || *length <= 0.0)
    {
        return None;
    }
    let unit = [0, 1, 2].map(|axis| vectors[axis] / lengths[axis]);
    let determinant = unit[0].dot(unit[1].cross(unit[2]));
    if !determinant.is_finite() || determinant.abs() <= 1e-12 {
        return None;
    }
    let normal_vectors = [
        unit[1].cross(unit[2]) / (determinant * lengths[0]),
        unit[2].cross(unit[0]) / (determinant * lengths[1]),
        unit[0].cross(unit[1]) / (determinant * lengths[2]),
    ];
    let stretch = vectors
        .iter()
        .map(|vector| vector.dot(*vector))
        .sum::<f64>()
        .sqrt();
    let mut out = source.clone();
    for side in &mut out.sides {
        for point in &mut side.positions {
            let moved = Vec3::from(origin)
                + vectors[0] * point[0]
                + vectors[1] * point[1]
                + vectors[2] * point[2];
            *point = moved.to_array();
        }
        for normal in &mut side.normals {
            let moved = normal_vectors[0] * normal[0]
                + normal_vectors[1] * normal[1]
                + normal_vectors[2] * normal[2];
            *normal = moved.normalize()?.to_array();
        }
    }
    for triangle in &mut out.triangles {
        for point in &mut triangle.positions {
            let moved = Vec3::from(origin)
                + vectors[0] * point[0]
                + vectors[1] * point[1]
                + vectors[2] * point[2];
            *point = moved.to_array();
        }
        for normal in &mut triangle.normals {
            let moved = normal_vectors[0] * normal[0]
                + normal_vectors[1] * normal[1]
                + normal_vectors[2] * normal[2];
            *normal = moved.normalize()?.to_array();
        }
    }
    for cone in &mut out.cones {
        let moved_origin = Vec3::from(origin)
            + vectors[0] * cone.origin[0]
            + vectors[1] * cone.origin[1]
            + vectors[2] * cone.origin[2];
        cone.origin = moved_origin.to_array();
        for vector in [&mut cone.x_axis, &mut cone.y_axis, &mut cone.axis] {
            *vector = (vectors[0] * vector[0] + vectors[1] * vector[1] + vectors[2] * vector[2])
                .to_array();
        }
    }
    out.precision *= stretch;
    Some(out)
}

impl BodyMesh {
    pub fn is_complete(&self) -> bool {
        self.missing_faces.is_empty() && !self.mesh.is_empty()
    }
}

pub fn sweep_surface(
    profile: &ocs_geom::space::PlanarCurve,
    path: &ocs_geom::space::PlanarCurve,
    max_angle: f64,
) -> Option<SurfaceMesh> {
    if path.curve.is_closed() {
        return None;
    }
    let closed = profile.curve.is_closed();
    let outline = open_samples(curve_samples(profile, max_angle), closed);
    let track = open_samples(curve_samples(path, max_angle), false);
    if outline.len() < 2 || track.len() < 2 {
        return None;
    }
    let origin = Vec3::from(track[0]);
    let sections: Vec<Vec<[f64; 3]>> = track
        .iter()
        .map(|point| {
            let shift = Vec3::from(*point) - origin;
            outline
                .iter()
                .map(|position| (Vec3::from(*position) + shift).to_array())
                .collect()
        })
        .collect();
    surface_from_sections(&sections, closed, Some(&profile.plane))
}

pub fn loft_surface(profiles: &[ocs_geom::space::PlanarCurve], max_angle: f64) -> Option<SurfaceMesh> {
    if profiles.len() < 2 {
        return None;
    }
    let closed = profiles.iter().all(|profile| profile.curve.is_closed());
    let sampled: Vec<Vec<[f64; 3]>> = profiles
        .iter()
        .map(|profile| {
            Some(open_samples(
                curve_samples(profile, max_angle),
                profile.curve.is_closed(),
            ))
        })
        .collect::<Option<_>>()?;
    let count = sampled.iter().map(Vec::len).max()?;
    if count < 2 || sampled.iter().any(|ring| ring.len() < 2) {
        return None;
    }
    let sections: Vec<Vec<[f64; 3]>> = sampled
        .iter()
        .map(|ring| resample_ring(ring, count, closed))
        .collect();
    let mut surface = surface_from_sections(&sections, closed, None)?;
    if closed {
        cap_ring(
            &mut surface.mesh,
            &profiles.first()?.plane,
            sections.first()?,
            true,
        );
        cap_ring(
            &mut surface.mesh,
            &profiles.last()?.plane,
            sections.last()?,
            false,
        );
    }
    Some(surface)
}

fn surface_from_sections(
    sections: &[Vec<[f64; 3]>],
    closed: bool,
    cap_plane: Option<&ocs_geom::space::Plane>,
) -> Option<SurfaceMesh> {
    let mut out = SurfaceMesh::default();
    for pair in sections.windows(2) {
        band(&mut out.mesh, &pair[0], &pair[1], closed);
    }
    let mut first_section = sections.first()?.clone();
    let mut last_section = sections.last()?.clone();
    if closed {
        first_section.push(first_section[0]);
        last_section.push(last_section[0]);
    }
    out.edges.extend([first_section, last_section]);
    if closed {
        if let Some(plane) = cap_plane {
            cap_ring(&mut out.mesh, plane, sections.first()?, true);
            let shift = Vec3::from(sections.last()?.first().copied()?)
                - Vec3::from(sections.first()?.first().copied()?);
            let far = ocs_geom::space::Plane::from_axes(
                (Vec3::from(plane.origin) + shift).to_array(),
                plane.x_axis,
                plane.y_axis,
            );
            cap_ring(&mut out.mesh, &far, sections.last()?, false);
        }
    } else {
        let first: Vec<[f64; 3]> = sections
            .iter()
            .filter_map(|ring| ring.first().copied())
            .collect();
        let last: Vec<[f64; 3]> = sections
            .iter()
            .filter_map(|ring| ring.last().copied())
            .collect();
        out.edges.extend([first, last]);
    }
    out.edges.retain(|edge| edge.len() >= 2);
    Some(out)
}

fn open_samples(mut points: Vec<[f64; 3]>, closed: bool) -> Vec<[f64; 3]> {
    if closed && points.len() > 1 && distance3(points[0], points[points.len() - 1]) <= 1e-9 {
        points.pop();
    }
    points
}

fn curve_samples(curve: &ocs_geom::space::PlanarCurve, max_angle: f64) -> Vec<[f64; 3]> {
    curve.tessellate_angle(max_angle)
}

fn resample_ring(points: &[[f64; 3]], count: usize, closed: bool) -> Vec<[f64; 3]> {
    let mut chain: Vec<Vec3> = points.iter().copied().map(Vec3::from).collect();
    if closed {
        chain.push(chain[0]);
    }
    let mut walked = vec![0.0];
    for pair in chain.windows(2) {
        walked.push(walked[walked.len() - 1] + pair[0].distance(pair[1]));
    }
    let total = *walked.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return points.to_vec();
    }
    let divisor = if closed {
        count
    } else {
        count.saturating_sub(1).max(1)
    };
    (0..count)
        .map(|step| {
            let wanted = total * step as f64 / divisor as f64;
            let at = walked
                .iter()
                .rposition(|reached| *reached <= wanted)
                .unwrap_or(0)
                .min(chain.len() - 2);
            let span = walked[at + 1] - walked[at];
            let unit = if span > 0.0 {
                (wanted - walked[at]) / span
            } else {
                0.0
            };
            chain[at].lerp(chain[at + 1], unit).to_array()
        })
        .collect()
}

fn band(mesh: &mut Mesh, lower: &[[f64; 3]], upper: &[[f64; 3]], closed: bool) {
    let count = lower.len().min(upper.len());
    let spans = if closed {
        count
    } else {
        count.saturating_sub(1)
    };
    for index in 0..spans {
        let next = (index + 1) % count;
        emit_points(mesh, lower[index], lower[next], upper[next]);
        emit_points(mesh, lower[index], upper[next], upper[index]);
    }
}

fn cap_ring(mesh: &mut Mesh, plane: &ocs_geom::space::Plane, ring: &[[f64; 3]], reverse: bool) {
    let parameters: Vec<[f64; 2]> = ring
        .iter()
        .filter_map(|point| plane.project(*point))
        .collect();
    if parameters.len() != ring.len() {
        return;
    }
    let (points, triangles) = triangulate(&parameters, &[]);
    for triangle in triangles {
        let mut positions = [
            plane.point_at(points[triangle[0]]),
            plane.point_at(points[triangle[1]]),
            plane.point_at(points[triangle[2]]),
        ];
        if reverse {
            positions.swap(1, 2);
        }
        emit_points(mesh, positions[0], positions[1], positions[2]);
    }
}

fn emit_points(mesh: &mut Mesh, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    let Some(normal) = (Vec3::from(b) - Vec3::from(a))
        .cross(Vec3::from(c) - Vec3::from(a))
        .normalize()
    else {
        return;
    };
    let base = mesh.positions.len();
    mesh.positions.extend([a, b, c]);
    mesh.normals.extend([normal.to_array(); 3]);
    mesh.triangles.push([base, base + 1, base + 2]);
}

/// A planar face with no inside to draw: two boundary curves a file's fit
/// apart — a spline lying along the arc it was fitted to — cross and recross
/// each other, so no triangulation holds, and there is nothing to show.
fn hairline_face(body: &Body, face: FaceKey) -> bool {
    super::split::planar_measure(body, face).is_some_and(|(area, perimeter)| {
        let half = 0.5 * perimeter;
        area.abs() <= 1e-3 * half * half
    })
}

/// Tessellates faces and edges from one shared sample schedule.
pub fn tessellate(body: &Body, tolerance: TessellationTolerance) -> BodyMesh {
    let _normals = SplineNormals::open();
    let _fold = FoldDeviation::set(tolerance.chordal.unwrap_or(tolerance.linear));
    let schedules = body_edge_schedules(body, tolerance);
    let mut out = BodyMesh::default();
    for face_key in body.face_keys() {
        let max_angle = face_chordal_angle(body, face_key, tolerance.angle, tolerance.chordal);
        let limit = FaceLimit::set(
            tolerance
                .triangle_budget
                .map(|budget| budget.saturating_sub(out.mesh.triangles.len())),
        );
        if limit.exhausted() {
            out.over_budget = true;
            out.missing_faces.push(face_key);
            continue;
        }
        let mut face = scheduled_face(body, face_key, max_angle, tolerance.linear, &schedules);
        // A face the budget stopped is not retried finer.
        if face.is_none() && !limit.reached() {
            face = finer_face(body, face_key, max_angle, tolerance.linear).or_else(|| {
                let mesh =
                    crossing_face(body, face_key, max_angle, tolerance.linear, &schedules)?;
                out.crossed = true;
                Some(mesh)
            });
        }
        out.over_budget |= limit.reached();
        match face {
            Some(mesh) => {
                if let Some(cone) = analytic_cone_face(body, face_key, &mesh) {
                    out.analytic_cones.push(cone);
                }
                out.triangle_faces
                    .extend(std::iter::repeat(face_key).take(mesh.triangles.len()));
                out.mesh.absorb(mesh);
                if tolerance.isolines.iter().any(|count| *count > 0) {
                    match face_isolines(
                        body,
                        face_key,
                        max_angle,
                        tolerance.linear,
                        tolerance.isolines,
                        tolerance.planar_isolines,
                        tolerance.solid_isolines,
                        &schedules,
                    ) {
                        Some(lines) => out.isolines.extend(lines),
                        None => out.missing_faces.push(face_key),
                    }
                }
            }
            None if hairline_face(body, face_key) => {}
            None => out.missing_faces.push(face_key),
        }
    }
    out.drawing_edges = scheduled_edges(body, &schedules);
    out.edges = out
        .drawing_edges
        .iter()
        .filter(|edge| {
            schedules
                .get(&edge.edge)
                .is_none_or(|schedule| !smooth_scheduled_edge(body, edge.edge, schedule))
        })
        .cloned()
        .collect();
    out.missing_faces.dedup();
    out.precision = silhouette_precision(&out.mesh);
    out
}

std::thread_local! {
    /// While a [`tessellate`] with a triangle budget fills a face: the
    /// triangles it may still add, and whether a fill ran into that limit.
    static FACE_LIMIT: std::cell::Cell<(Option<usize>, bool)> =
        const { std::cell::Cell::new((None, false)) };
}

/// The triangle limit of the face being filled, for as long as it lives.
struct FaceLimit((Option<usize>, bool));

impl FaceLimit {
    fn set(limit: Option<usize>) -> Self {
        Self(FACE_LIMIT.with(|cell| cell.replace((limit, false))))
    }

    /// The budget is spent before the face starts.
    fn exhausted(&self) -> bool {
        FACE_LIMIT.with(|cell| cell.get().0 == Some(0))
    }

    /// A fill of this face stopped at the limit.
    fn reached(&self) -> bool {
        FACE_LIMIT.with(|cell| cell.get().1)
    }
}

impl Drop for FaceLimit {
    fn drop(&mut self) {
        FACE_LIMIT.with(|cell| cell.set(self.0));
    }
}

/// Whether `triangles` on the face being filled would pass its budget
/// limit, noting it when they would; never without a budget.
fn face_limit_exceeded(triangles: usize) -> bool {
    FACE_LIMIT.with(|cell| {
        let (limit, _) = cell.get();
        let exceeded = limit.is_some_and(|limit| triangles > limit);
        if exceeded {
            cell.set((limit, true));
        }
        exceeded
    })
}

/// Tessellates display edges and optional face isolines without triangulating.
///
/// Uses the same edge samples, seam filtering, face parameter bounds and
/// tolerance policy as [`tessellate`]. This is intended for wireframe-only
/// consumers such as interactive previews: no face mesh is built or validated,
/// and valid isolines remain available even when triangulation would fail.
/// See [`BodyWireframe`] for partial-output failure semantics.
pub fn tessellate_wireframe(body: &Body, tolerance: TessellationTolerance) -> BodyWireframe {
    let schedules = body_edge_schedules(body, tolerance);
    let mut out = BodyWireframe::default();
    if tolerance.isolines.iter().any(|count| *count > 0) {
        for face_key in body.face_keys() {
            let max_angle = face_chordal_angle(body, face_key, tolerance.angle, tolerance.chordal);
            match face_isolines(
                body,
                face_key,
                max_angle,
                tolerance.linear,
                tolerance.isolines,
                tolerance.planar_isolines,
                tolerance.solid_isolines,
                &schedules,
            ) {
                Some(lines) => out.isolines.extend(lines),
                None => out.missing_faces.push(face_key),
            }
        }
    }
    out.edges = visible_scheduled_edges(body, &schedules);
    out
}

fn body_edge_schedules(
    body: &Body,
    tolerance: TessellationTolerance,
) -> HashMap<EdgeKey, Vec<super::place::EdgeSample>> {
    edge_schedules(body, body.edge_keys(), tolerance)
}

/// One face's triangles, from the same shared edge schedules [`tessellate`]
/// uses — each edge's depends only on it and its own faces — so the face
/// comes out as it does there, without meshing the rest of the body.
pub(super) fn face_triangles(
    body: &Body,
    face: FaceKey,
    tolerance: TessellationTolerance,
) -> Option<Mesh> {
    let _normals = SplineNormals::open();
    let edges: HashSet<EdgeKey> = body
        .face_coedges(face)
        .iter()
        .filter_map(|coedge| Some(body.coedges.get(*coedge)?.edge))
        .collect();
    let schedules = edge_schedules(body, edges.into_iter(), tolerance);
    let max_angle = face_chordal_angle(body, face, tolerance.angle, tolerance.chordal);
    scheduled_face(body, face, max_angle, tolerance.linear, &schedules)
        .or_else(|| finer_face(body, face, max_angle, tolerance.linear))
        .or_else(|| crossing_face(body, face, max_angle, tolerance.linear, &schedules))
}

/// A face whose boundary rings cross as chords where their curves only
/// touch, filled with the crossings split (see
/// [`ocs_geom::geom2d::constrained::splitting_crossings`]).
fn crossing_face(
    body: &Body,
    face: FaceKey,
    max_angle: f64,
    tolerance: f64,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> Option<Mesh> {
    ocs_geom::geom2d::constrained::splitting_crossings(|| {
        scheduled_face(body, face, max_angle, tolerance, schedules)
    })
}

/// Whether one face meshes as [`tessellate`] would mesh it — triangles, or
/// a hairline sliver it leaves out without counting it missing — remembered
/// for a face exactly like one asked about before: the faces a boolean
/// leaves untouched come back unchanged from one operation to the next, and
/// a part cut a hundred times was meshed whole a hundred times over.
pub(super) fn face_meshes(body: &Body, face: FaceKey, tolerance: TessellationTolerance) -> bool {
    use std::hash::{Hash, Hasher};
    thread_local! {
        static MESHED: std::cell::RefCell<HashMap<u64, bool>> =
            std::cell::RefCell::new(HashMap::new());
    }
    let meshes_now = || {
        face_triangles(body, face, tolerance).is_some_and(|mesh| !mesh.triangles.is_empty())
            || hairline_face(body, face)
    };
    // A chordal limit reads the neighbours' curves too; asked fresh.
    if tolerance.chordal.is_some() {
        return meshes_now();
    }
    // The face's surface and boundary, and the surfaces and parameter
    // curves across each of its edges, which its edges' schedules read too.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let Some(node) = body.faces.get(face) else {
        return false;
    };
    let surface = body.surfaces.get(node.surface);
    let mut written = format!("{tolerance:?}{:?}{surface:?}", node.forward);
    for coedge in body.face_coedges(face) {
        let Some(use_) = body.coedges.get(coedge) else {
            return false;
        };
        let Some(edge) = body.edges.get(use_.edge) else {
            return false;
        };
        let ends = [edge.start, edge.end].map(|vertex| body.vertices.get(vertex).map(|v| v.point));
        written += &format!(
            "|{}{:?}{:?}{:?}{:?}{:?}",
            use_.forward,
            use_.pcurve,
            body.curves.get(edge.curve),
            [edge.start_parameter, edge.end_parameter],
            ends,
            edge.coedges.len()
        );
        for other in &edge.coedges {
            let across = body.coedges.get(*other).map(|other| {
                let face = body.loops.get(other.owner).and_then(|ring| body.faces.get(ring.owner));
                let surface = face.map(|face| (face.forward, body.surfaces.get(face.surface)));
                (other.forward, &other.pcurve, surface)
            });
            written += &format!("{across:?}");
        }
    }
    written.hash(&mut hasher);
    let key = hasher.finish();
    if let Some(known) = MESHED.with(|cache| cache.borrow().get(&key).copied()) {
        return known;
    }
    let meshes = meshes_now();
    MESHED.with(|cache| {
        let mut cache = cache.borrow_mut();
        // ponytail: whole cache dropped when full; an LRU if hit rates matter.
        if cache.len() >= 1 << 14 {
            cache.clear();
        }
        cache.insert(key, meshes);
    });
    meshes
}

fn edge_schedules(
    body: &Body,
    edges: impl Iterator<Item = EdgeKey>,
    tolerance: TessellationTolerance,
) -> HashMap<EdgeKey, Vec<super::place::EdgeSample>> {
    edges
        .filter_map(|edge| {
            // The faces fill against their own limit and cannot split a
            // boundary segment, so an edge is sampled at least as finely as
            // every face it bounds needs (#1538: a cylinder notched by
            // ellipses tightens its face below its circles').
            let max_angle = body.edges.get(edge)?.coedges.iter().fold(
                edge_chordal_angle(body, edge, tolerance.angle, tolerance.chordal),
                |angle, coedge| {
                    body.coedges
                        .get(*coedge)
                        .and_then(|coedge| body.loops.get(coedge.owner))
                        .map_or(angle, |ring| {
                            angle.min(face_chordal_angle(
                                body,
                                ring.owner,
                                tolerance.angle,
                                tolerance.chordal,
                            ))
                        })
                },
            );
            let mut samples = shared_edge_samples(body, edge, max_angle, tolerance.linear)?;
            for sample in &mut samples {
                sample.position =
                    shared_surface_position(body, edge, sample.position, tolerance.linear)
                        .unwrap_or(sample.position);
            }
            // An edge ends at its vertices. Projected onto its faces, an end
            // lands wherever those surfaces put it, and the surfaces of a
            // file fitted to 4e-3 disagree by that much from one edge to the
            // next round a vertex, so the face's boundary no longer closed
            // (#1538).
            {
                let node = body.edges.get(edge)?;
                samples.first_mut()?.position = body.vertices.get(node.start)?.point;
                samples.last_mut()?.position = body.vertices.get(node.end)?.point;
            }
            Some((edge, samples))
        })
        .collect()
}

fn visible_scheduled_edges(
    body: &Body,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> Vec<EdgeMesh> {
    scheduled_edges(body, schedules)
        .into_iter()
        .filter(|edge| {
            schedules
                .get(&edge.edge)
                .is_none_or(|schedule| !smooth_scheduled_edge(body, edge.edge, schedule))
        })
        .collect()
}

fn scheduled_edges(
    body: &Body,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> Vec<EdgeMesh> {
    body.edge_keys()
        .filter(|edge| !topological_parameter_seam(body, *edge))
        .filter_map(|edge| {
            let schedule = schedules.get(&edge)?;
            (schedule.len() >= 2).then(|| EdgeMesh {
                edge,
                parameters: schedule.iter().map(|sample| sample.parameter).collect(),
                positions: schedule.iter().map(|sample| sample.position).collect(),
            })
        })
        .collect()
}

fn smooth_scheduled_edge(
    body: &Body,
    edge_key: EdgeKey,
    schedule: &[super::place::EdgeSample],
) -> bool {
    let Some(edge) = body.edges.get(edge_key) else {
        return false;
    };
    let [first, second] = edge.coedges.as_slice() else {
        return false;
    };
    // An edge dividing one plane was put there on purpose — an imprint, a
    // split — and is drawn; only a smooth change of surface is hidden.
    let plane = |coedge: &super::topology::CoedgeKey| {
        let face = body.faces.get(body.loops.get(body.coedges.get(*coedge)?.owner)?.owner)?;
        match body.surfaces.get(face.surface)? {
            super::geometry::Surface::Plane(plane) => Some(*plane),
            _ => None,
        }
    };
    if let (Some(one), Some(two)) = (plane(first), plane(second)) {
        let parallel = match (one.normal(), two.normal()) {
            (Some(a), Some(b)) => Vec3::from(a).dot(Vec3::from(b)).abs() >= 1.0 - 1e-12,
            _ => false,
        };
        if one == two || (parallel && two.distance_to(one.origin).is_some_and(|gap| gap.abs() <= 1e-9)) {
            return false;
        }
    }
    schedule.iter().all(|sample| {
        let normals =
            [first, second].map(|coedge| outward_normal_at_edge(body, *coedge, sample.position));
        let [Some(first), Some(second)] = normals else {
            return false;
        };
        first.dot(second) >= 1.0 - 1e-8
    })
}

fn outward_normal_at_edge(
    body: &Body,
    coedge_key: super::topology::CoedgeKey,
    position: [f64; 3],
) -> Option<Vec3> {
    let coedge = body.coedges.get(coedge_key)?;
    let face = body.faces.get(body.loops.get(coedge.owner)?.owner)?;
    let surface = body.surfaces.get(face.surface)?;
    let (u, v) = surface.parameters_at(position)?;
    let normal = Vec3::from(surface_display_normal(surface, [u, v])?);
    Some(if face.forward { normal } else { -normal })
}

fn shared_surface_position(
    body: &Body,
    edge_key: EdgeKey,
    position: [f64; 3],
    tolerance: f64,
) -> Option<[f64; 3]> {
    let edge = body.edges.get(edge_key)?;
    let surfaces: Option<Vec<&super::geometry::Surface>> = edge
        .coedges
        .iter()
        .map(|coedge| {
            let (surface, _) = coedge_geometry(body, *coedge)?;
            Some(surface)
        })
        .collect();
    surface_consensus_position(&surfaces?, position, tolerance)
}

fn surface_consensus_position(
    surfaces: &[&super::geometry::Surface],
    position: [f64; 3],
    tolerance: f64,
) -> Option<[f64; 3]> {
    let projected: Option<Vec<[f64; 3]>> = surfaces
        .iter()
        .map(|surface| {
            let (u, v) = surface.parameters_at(position)?;
            let point = surface.point_at(u, v);
            point
                .iter()
                .all(|coordinate| coordinate.is_finite())
                .then_some(point)
        })
        .collect();
    let projected = projected?;
    if projected.is_empty() {
        return None;
    }
    let scale = projected
        .iter()
        .flatten()
        .map(|coordinate| coordinate.abs())
        .fold(1.0, f64::max);
    let agreement = tolerance.max(f64::EPSILON * 1024.0 * scale);
    for first in 0..projected.len() {
        for second in first + 1..projected.len() {
            if distance3(projected[first], projected[second]) > agreement {
                return None;
            }
        }
    }
    let mut average = [0.0; 3];
    for point in &projected {
        for axis in 0..3 {
            average[axis] += point[axis];
        }
    }
    Some(average.map(|coordinate| coordinate / projected.len() as f64))
}

fn topological_parameter_seam(body: &Body, edge: EdgeKey) -> bool {
    let Some(edge) = body.edges.get(edge) else {
        return false;
    };
    edge.coedges.iter().enumerate().any(|(index, first)| {
        let Some(first) = body.coedges.get(*first) else {
            return false;
        };
        edge.coedges.iter().skip(index + 1).any(|second| {
            body.coedges.get(*second).is_some_and(|second| {
                first.owner == second.owner && first.forward != second.forward
            })
        })
    })
}

fn analytic_cone_face(body: &Body, face: FaceKey, mesh: &Mesh) -> Option<AnalyticConeFace> {
    let node = body.faces.get(face)?;
    let super::geometry::Surface::Cone(surface) = body.surfaces.get(node.surface)? else {
        return None;
    };
    let full_revolution = node.loops.iter().any(|ring| {
        let Some(ring) = body.loops.get(*ring) else {
            return false;
        };
        ring.coedges.iter().enumerate().any(|(index, first)| {
            let Some(first) = body.coedges.get(*first) else {
                return false;
            };
            ring.coedges.iter().skip(index + 1).any(|second| {
                body.coedges.get(*second).is_some_and(|second| {
                    first.edge == second.edge && first.forward != second.forward
                })
            })
        })
    });
    if !full_revolution {
        return None;
    }
    let geometry = super::geometry::Surface::Cone(*surface);
    let mut v_range = [f64::INFINITY, f64::NEG_INFINITY];
    for point in &mesh.positions {
        let (_, v) = geometry.parameters_at(*point)?;
        v_range[0] = v_range[0].min(v);
        v_range[1] = v_range[1].max(v);
    }
    if !v_range.iter().all(|value| value.is_finite()) || v_range[1] <= v_range[0] {
        return None;
    }
    Some(AnalyticConeFace {
        face,
        cone: ConeSilhouette {
            origin: surface.base.origin,
            x_axis: surface.base.x_axis,
            y_axis: surface.base.y_axis,
            axis: surface.base.normal()?,
            radius: surface.radius,
            slope: surface.half_angle.tan(),
            v_range,
        },
    })
}

fn face_isolines(
    body: &Body,
    face: FaceKey,
    max_angle: f64,
    tolerance: f64,
    counts: [usize; 2],
    planar_isolines: bool,
    solid_isolines: bool,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> Option<Vec<FacePolyline>> {
    let Some(node) = body.faces.get(face) else {
        return None;
    };
    let Some(surface) = body.surfaces.get(node.surface) else {
        return None;
    };
    if matches!(surface, super::geometry::Surface::Plane(_)) && !planar_isolines {
        return Some(Vec::new());
    }
    let parameters = if let Some(domain) = whole_surface_domain(body, face, surface) {
        vec![domain_ring(domain)]
    } else {
        face_rings(body, face, surface, schedules, tolerance)?
            .iter()
            .map(|ring| ring.iter().map(|point| point.parameters).collect())
            .collect()
    };
    let Some(bounds) = parameter_bounds(&parameters) else {
        return None;
    };
    let mut out = Vec::new();
    for fixed_axis in 0..2 {
        let count = counts[fixed_axis];
        if count == 0 {
            continue;
        }
        let varying_axis = 1 - fixed_axis;
        let fixed_span = bounds[fixed_axis][1] - bounds[fixed_axis][0];
        if fixed_span <= 0.0 {
            continue;
        }
        let values = if solid_isolines {
            solid_isoline_values(surface, fixed_axis, bounds[fixed_axis], count)
        } else {
            (0..count)
                .map(|index| {
                    bounds[fixed_axis][0]
                        + fixed_span * (index as f64 + 1.0) / (count as f64 + 1.0)
                })
                .collect()
        };
        for fixed in values {
            for interval in line_intervals(&parameters, fixed_axis, fixed) {
                let mut points = Vec::new();
                let mut budget = PCURVE_REFINE_BUDGET;
                if !sample_isoline(
                    surface,
                    fixed_axis,
                    fixed,
                    interval[0],
                    interval[1],
                    max_angle,
                    0,
                    &mut points,
                    &mut budget,
                ) {
                    return None;
                }
                let mut end = [0.0; 2];
                end[fixed_axis] = fixed;
                end[varying_axis] = interval[1];
                points.push(surface.point_at(end[0], end[1]));
                // A solid's line at its surface's zero angle may lie on the
                // face's seam, which is not drawn: it is kept there.
                if points.len() >= 2
                    && (solid_isolines
                        || !polyline_on_face_boundary(body, face, &points, tolerance, schedules))
                {
                    out.push(FacePolyline {
                        face,
                        positions: points,
                    });
                }
            }
        }
    }
    Some(out)
}

/// Where a solid draws its `count` construction lines fixing `axis` of a
/// face spanning `range` (see [`TessellationTolerance::with_solid_isolines`]).
fn solid_isoline_values(
    surface: &super::geometry::Surface,
    axis: usize,
    range: [f64; 2],
    count: usize,
) -> Vec<f64> {
    use super::geometry::Surface;
    let span = range[1] - range[0];
    let pieces = |parts: usize| -> Vec<f64> {
        (1..parts)
            .map(|index| range[0] + span * index as f64 / parts as f64)
            .collect()
    };
    match (surface, axis) {
        (Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_), 0)
        | (Surface::Torus(_), _) => {
            let turn = std::f64::consts::TAU;
            if span >= turn - 1e-9 {
                (0..count)
                    .map(|index| {
                        range[0] + (turn * index as f64 / count as f64 - range[0]).rem_euclid(turn)
                    })
                    .collect()
            } else {
                pieces((count as f64 * span / turn + 0.5).floor() as usize + 1)
            }
        }
        (Surface::Sphere(_), _) => pieces(count),
        (Surface::Cylinder(_) | Surface::Cone(_), _) => Vec::new(),
        _ => (0..count)
            .map(|index| range[0] + span * (index as f64 + 1.0) / (count as f64 + 1.0))
            .collect(),
    }
}

fn polyline_on_face_boundary(
    body: &Body,
    face_key: FaceKey,
    points: &[[f64; 3]],
    tolerance: f64,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> bool {
    let Some(face) = body.faces.get(face_key) else {
        return false;
    };
    face.loops
        .iter()
        .filter_map(|loop_key| body.loops.get(*loop_key))
        .flat_map(|ring| &ring.coedges)
        .filter_map(|coedge_key| body.coedges.get(*coedge_key).map(|coedge| coedge.edge))
        .any(|edge_key| {
            schedules.get(&edge_key).is_some_and(|schedule| {
                schedule.len() >= 2
                    && points.iter().all(|point| {
                        schedule.windows(2).any(|segment| {
                            Vec3::from(*point).distance_to_segment(
                                Vec3::from(segment[0].position),
                                Vec3::from(segment[1].position),
                            ) <= tolerance
                        })
                    })
            })
        })
}

fn parameter_bounds(rings: &[Vec<[f64; 2]>]) -> Option<[[f64; 2]; 2]> {
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for point in rings.iter().flatten() {
        for axis in 0..2 {
            bounds[axis][0] = bounds[axis][0].min(point[axis]);
            bounds[axis][1] = bounds[axis][1].max(point[axis]);
        }
    }
    bounds
        .iter()
        .all(|range| range[0].is_finite() && range[1].is_finite())
        .then_some(bounds)
}

fn line_intervals(rings: &[Vec<[f64; 2]>], fixed_axis: usize, fixed: f64) -> Vec<[f64; 2]> {
    let varying_axis = 1 - fixed_axis;
    let mut crossings = Vec::new();
    for ring in rings {
        for index in 0..ring.len() {
            let a = ring[index];
            let b = ring[(index + 1) % ring.len()];
            let crosses = (a[fixed_axis] <= fixed && b[fixed_axis] > fixed)
                || (b[fixed_axis] <= fixed && a[fixed_axis] > fixed);
            if !crosses {
                continue;
            }
            let unit = (fixed - a[fixed_axis]) / (b[fixed_axis] - a[fixed_axis]);
            crossings.push(a[varying_axis] + (b[varying_axis] - a[varying_axis]) * unit);
        }
    }
    crossings.sort_by(f64::total_cmp);
    crossings.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
    crossings
        .chunks_exact(2)
        .filter_map(|pair| (pair[1] > pair[0]).then_some([pair[0], pair[1]]))
        .collect()
}

fn sample_isoline(
    surface: &super::geometry::Surface,
    fixed_axis: usize,
    fixed: f64,
    from: f64,
    to: f64,
    max_angle: f64,
    depth: u32,
    points: &mut Vec<[f64; 3]>,
    budget: &mut usize,
) -> bool {
    let Some(left) = budget.checked_sub(1) else {
        return false;
    };
    *budget = left;
    let at = |value: f64| {
        let mut parameters = [0.0; 2];
        parameters[fixed_axis] = fixed;
        parameters[1 - fixed_axis] = value;
        surface.point_at(parameters[0], parameters[1])
    };
    let middle = 0.5 * (from + to);
    let directions = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0].map(|unit| {
        let mut parameters = [0.0; 2];
        parameters[fixed_axis] = fixed;
        parameters[1 - fixed_axis] = from + (to - from) * unit;
        surface
            .tangents_at(parameters[0], parameters[1])
            .map(|tangents| {
                if fixed_axis == 0 {
                    tangents.1
                } else {
                    tangents.0
                }
            })
            .unwrap_or([0.0; 3])
    });
    let split = angle_exceeds(
        ocs_geom::tessellation::max_direction_angle(&directions),
        max_angle,
    );
    if split {
        if depth >= MAX_DEPTH {
            return false;
        }
        if !sample_isoline(
            surface,
            fixed_axis,
            fixed,
            from,
            middle,
            max_angle,
            depth + 1,
            points,
            budget,
        ) || !sample_isoline(
            surface,
            fixed_axis,
            fixed,
            middle,
            to,
            max_angle,
            depth + 1,
            points,
            budget,
        ) {
            return false;
        }
    } else {
        points.push(at(from));
    }
    true
}

fn finite_positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn angle_for_chordal_radius(cap: f64, deflection: Option<f64>, radius: f64) -> f64 {
    let Some(deflection) = deflection else {
        return cap;
    };
    if !radius.is_finite() || radius <= f64::MIN_POSITIVE {
        return cap;
    }
    let ratio = (deflection / radius).clamp(0.0, 1.0);
    let chord_angle = (2.0 * (1.0 - ratio).acos()).max(f64::EPSILON);
    cap.min(ocs_geom::tessellation::angle(chord_angle))
}

fn curve_chordal_radius(curve: &super::geometry::Curve3) -> f64 {
    match curve {
        super::geometry::Curve3::Circle(value) => value.radius.abs(),
        super::geometry::Curve3::Ellipse(value) => {
            let major = value.major_radius.abs().max(value.minor_radius.abs());
            let minor = value.major_radius.abs().min(value.minor_radius.abs());
            if minor > f64::MIN_POSITIVE {
                major * major / minor
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

fn surface_chordal_radius(surface: &super::geometry::Surface) -> f64 {
    match surface {
        super::geometry::Surface::Cylinder(value) => value.radius.abs(),
        super::geometry::Surface::Cone(value) => value.radius.abs(),
        super::geometry::Surface::Sphere(value) => value.radius.abs(),
        super::geometry::Surface::Torus(value) => {
            value.major_radius.abs() + value.minor_radius.abs()
        }
        _ => 0.0,
    }
}

fn edge_chordal_angle(body: &Body, edge: EdgeKey, cap: f64, deflection: Option<f64>) -> f64 {
    let radius = body
        .edges
        .get(edge)
        .and_then(|edge| body.curves.get(edge.curve))
        .map_or(0.0, curve_chordal_radius);
    angle_for_chordal_radius(cap, deflection, radius)
}

fn face_chordal_angle(body: &Body, face: FaceKey, cap: f64, deflection: Option<f64>) -> f64 {
    let Some(node) = body.faces.get(face) else {
        return cap;
    };
    let mut radius = body
        .surfaces
        .get(node.surface)
        .map_or(0.0, surface_chordal_radius);
    for coedge in body.face_coedges(face) {
        let candidate = body
            .coedges
            .get(coedge)
            .and_then(|coedge| body.edges.get(coedge.edge))
            .and_then(|edge| body.curves.get(edge.curve))
            .map_or(0.0, curve_chordal_radius);
        if candidate.is_finite() {
            radius = radius.max(candidate);
        }
    }
    angle_for_chordal_radius(cap, deflection, radius)
}

fn angle_exceeds(value: f64, limit: f64) -> bool {
    !value.is_finite()
        || value > limit + f64::EPSILON * 64.0 * value.abs().max(limit.abs()).max(1.0)
}

fn shared_edge_samples(
    body: &Body,
    edge_key: EdgeKey,
    max_angle: f64,
    tolerance: f64,
) -> Option<Vec<super::place::EdgeSample>> {
    let edge = body.edges.get(edge_key)?;
    let directions = edge
        .coedges
        .iter()
        .map(|coedge| {
            let (_, pcurve) = coedge_geometry(body, *coedge)?;
            Some((
                *coedge,
                if pcurve.is_some() {
                    pcurve_edge_forward(body, edge_key, *coedge)?
                } else {
                    true
                },
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let has_nurbs_pcurve = edge.coedges.iter().any(|coedge| {
        coedge_geometry(body, *coedge).is_some_and(|(surface, pcurve)| {
            surface.is_spline() && pcurve.is_some()
        })
    });
    if has_nurbs_pcurve {
        if let Some(samples) =
            edge_samples_from_pcurves(body, edge_key, max_angle, tolerance, &directions)
        {
            return Some(samples);
        }
    }
    let mut samples = vec![super::place::EdgeSample {
        parameter: edge.start_parameter,
        position: body.vertices.get(edge.start)?.point,
    }];
    let mut budget = PCURVE_REFINE_BUDGET;
    if refine_edge(
        body,
        edge_key,
        edge.start_parameter,
        edge.end_parameter,
        ocs_geom::tessellation::angle(max_angle),
        tolerance.max(1e-12),
        0,
        &mut samples,
        &directions,
        &mut budget,
        normals_follow_circle(body, edge_key),
    )
    .is_some()
    {
        samples.push(super::place::EdgeSample {
            parameter: edge.end_parameter,
            position: body.vertices.get(edge.end)?.point,
        });
        return Some(samples);
    }
    edge_samples_from_pcurves(body, edge_key, max_angle, tolerance, &directions)
}

fn edge_samples_from_pcurves(
    body: &Body,
    edge_key: EdgeKey,
    max_angle: f64,
    tolerance: f64,
    directions: &HashMap<super::topology::CoedgeKey, bool>,
) -> Option<Vec<super::place::EdgeSample>> {
    let edge = body.edges.get(edge_key)?;
    for coedge_key in &edge.coedges {
        let Some((surface, Some(pcurve))) = coedge_geometry(body, *coedge_key) else {
            continue;
        };
        if !surface.is_spline() {
            continue;
        }
        let mut samples = Vec::new();
        let mut breaks = vec![0.0, 1.0];
        if let ocs_geom::geom2d::Curve::Nurbs(curve) = pcurve {
            let (from, to) = curve.domain();
            let span = to - from;
            if span.is_finite() && span > 0.0 {
                breaks.extend(
                    curve
                        .knots()
                        .iter()
                        .filter(|knot| **knot > from && **knot < to)
                        .map(|knot| (*knot - from) / span),
                );
            }
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|a, b| parameter_value_near(*a, *b));
        // A pcurve that disagrees with its edge keeps every piece turning,
        // however small, and halving it down to MAX_DEPTH is billions of
        // evaluations. Past the budget the pcurve is given up on and the
        // edge's own curve is sampled instead.
        let mut budget = PCURVE_REFINE_BUDGET;
        let resolved = breaks.windows(2).all(|pair| {
            refine_pcurve_edge(
                body,
                edge_key,
                *coedge_key,
                pair[0],
                pair[1],
                ocs_geom::tessellation::angle(max_angle),
                tolerance.max(1e-12),
                0,
                &mut samples,
                directions,
                &mut budget,
            )
            .is_some()
        });
        if resolved {
            let start = body.vertices.get(edge.start)?.point;
            if let Some(first) = samples.first_mut() {
                first.position = start;
            }
            samples.push(super::place::EdgeSample {
                parameter: edge.end_parameter,
                position: body.vertices.get(edge.end)?.point,
            });
            return Some(samples);
        }
    }
    None
}

fn refine_pcurve_edge(
    body: &Body,
    edge_key: EdgeKey,
    source_coedge: super::topology::CoedgeKey,
    from: f64,
    to: f64,
    max_angle: f64,
    tolerance: f64,
    depth: u32,
    samples: &mut Vec<super::place::EdgeSample>,
    coedge_directions: &HashMap<super::topology::CoedgeKey, bool>,
    budget: &mut usize,
) -> Option<()> {
    *budget = budget.checked_sub(1)?;
    let edge = body.edges.get(edge_key)?;
    let (source_surface, Some(source_pcurve)) = coedge_geometry(body, source_coedge)? else {
        return None;
    };
    let source_forward = *coedge_directions.get(&source_coedge)?;
    let source_parameter = |parameter: f64| {
        if source_forward {
            parameter
        } else {
            1.0 - parameter
        }
    };
    let units = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
        .map(|unit| from + (to - from) * unit);
    let positions = units.map(|parameter| {
        let uv = source_pcurve.point_at(source_parameter(parameter));
        source_surface.point_at(uv[0], uv[1])
    });
    let mut directions = Vec::with_capacity(units.len());
    for parameter in units {
        let step = 1e-5;
        let before = source_pcurve.point_at(source_parameter((parameter - step).max(from)));
        let after = source_pcurve.point_at(source_parameter((parameter + step).min(to)));
        let uv = source_pcurve.point_at(source_parameter(parameter));
        let (along_u, along_v) = source_surface.tangents_at(uv[0], uv[1])?;
        directions.push(
            (Vec3::from(along_u) * (after[0] - before[0])
                + Vec3::from(along_v) * (after[1] - before[1]))
                .to_array(),
        );
    }
    let mut split = angle_exceeds(
        ocs_geom::tessellation::max_direction_angle(&directions),
        max_angle,
    );
    for coedge_key in &edge.coedges {
        let (surface, pcurve) = coedge_geometry(body, *coedge_key)?;
        let mut normals = Vec::with_capacity(units.len());
        for (parameter, position) in units.iter().zip(positions) {
            let uv = if *coedge_key == source_coedge {
                source_pcurve.point_at(source_parameter(*parameter))
            } else if let Some((u, v)) = surface.parameters_at(position) {
                [u, v]
            } else {
                let pcurve_forward = *coedge_directions.get(coedge_key)?;
                let preferred = if pcurve_forward {
                    *parameter
                } else {
                    1.0 - *parameter
                };
                let pcurve = pcurve?;
                let direct = pcurve.point_at(preferred);
                if distance3(position, surface.point_at(direct[0], direct[1])) <= tolerance {
                    direct
                } else {
                    closest_pcurve_parameters(surface, pcurve, position, preferred, tolerance)?.1
                }
            };
            if distance3(position, surface.point_at(uv[0], uv[1])) > tolerance {
                return None;
            }
            let Some(normal) = surface_display_normal(surface, uv) else {
                return None;
            };
            normals.push(normal);
        }
        split |= angle_exceeds(
            ocs_geom::tessellation::max_direction_angle(&normals),
            max_angle,
        );
    }
    if split {
        if depth >= MAX_DEPTH {
            if distance3(positions[0], positions[positions.len() - 1]) <= tolerance {
                samples.push(super::place::EdgeSample {
                    parameter: edge.start_parameter
                        + (edge.end_parameter - edge.start_parameter) * from,
                    position: positions[0],
                });
                return Some(());
            }
            return None;
        }
        let middle = 0.5 * (from + to);
        refine_pcurve_edge(
            body,
            edge_key,
            source_coedge,
            from,
            middle,
            max_angle,
            tolerance,
            depth + 1,
            samples,
            coedge_directions,
            budget,
        )?;
        refine_pcurve_edge(
            body,
            edge_key,
            source_coedge,
            middle,
            to,
            max_angle,
            tolerance,
            depth + 1,
            samples,
            coedge_directions,
            budget,
        )?;
    } else {
        samples.push(super::place::EdgeSample {
            parameter: edge.start_parameter + (edge.end_parameter - edge.start_parameter) * from,
            position: positions[0],
        });
    }
    Some(())
}

fn pcurve_edge_forward(
    body: &Body,
    edge_key: EdgeKey,
    coedge_key: super::topology::CoedgeKey,
) -> Option<bool> {
    let edge = body.edges.get(edge_key)?;
    let curve = body.curves.get(edge.curve)?;
    let (surface, Some(pcurve)) = coedge_geometry(body, coedge_key)? else {
        return None;
    };
    let mut forward = 0.0;
    let mut reversed = 0.0;
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let edge_parameter =
            edge.start_parameter + (edge.end_parameter - edge.start_parameter) * parameter;
        let position = curve.point_at(edge_parameter);
        let direct = pcurve.point_at(parameter);
        let reverse = pcurve.point_at(1.0 - parameter);
        forward += distance3(position, surface.point_at(direct[0], direct[1]));
        reversed += distance3(position, surface.point_at(reverse[0], reverse[1]));
    }
    if forward.is_finite() && reversed.is_finite() {
        Some(forward <= reversed)
    } else {
        body.coedges.get(coedge_key).map(|coedge| coedge.forward)
    }
}

fn refine_edge(
    body: &Body,
    edge_key: EdgeKey,
    from: f64,
    to: f64,
    max_angle: f64,
    tolerance: f64,
    depth: u32,
    samples: &mut Vec<super::place::EdgeSample>,
    coedge_directions: &HashMap<super::topology::CoedgeKey, bool>,
    budget: &mut usize,
    normals_follow: bool,
) -> Option<()> {
    *budget = budget.checked_sub(1)?;
    let edge = body.edges.get(edge_key)?;
    let curve = body.curves.get(edge.curve)?;
    let middle = 0.5 * (from + to);
    let curved = Vec3::from(curve.point_at(middle));
    let directions = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
        .map(|unit| curve.tangent_at(from + (to - from) * unit));
    let mut split = angle_exceeds(
        ocs_geom::tessellation::max_direction_angle(&directions),
        max_angle,
    );
    let curve_turns = split;
    let mut folded = false;
    // Where the faces' normals turn no faster than the curve, the curve's
    // own turn decides alone.
    let faces = if normals_follow { &[][..] } else { &edge.coedges[..] };
    'coedges: for coedge_key in faces {
        let Some((surface, pcurve)) = coedge_geometry(body, *coedge_key) else {
            continue;
        };
        let pcurve_forward = *coedge_directions.get(coedge_key)?;
        let mut normals = Vec::with_capacity(9);
        for unit in [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0] {
            let parameter = from + (to - from) * unit;
            let on_curve = curve.point_at(parameter);
            let (uv, exact) = if let Some(pcurve) = pcurve {
                let mut preferred = (parameter - edge.start_parameter)
                    / (edge.end_parameter - edge.start_parameter);
                if !pcurve_forward {
                    preferred = 1.0 - preferred;
                }
                let direct = pcurve.point_at(preferred);
                let direct_distance = distance3(on_curve, surface.point_at(direct[0], direct[1]));
                if direct_distance <= tolerance {
                    (direct, true)
                } else if let Some((_, closest, _)) =
                    closest_pcurve_parameters(surface, pcurve, on_curve, preferred, tolerance)
                        .filter(|(_, _, deviation)| *deviation <= tolerance)
                {
                    (closest, true)
                } else if let Some((u, v)) = surface.parameters_at(on_curve) {
                    ([u, v], false)
                } else {
                    continue 'coedges;
                }
            } else if let Some((u, v)) = surface.parameters_at(on_curve) {
                ([u, v], false)
            } else {
                continue 'coedges;
            };
            if exact
                && Vec3::from(surface.point_at(uv[0], uv[1])).distance(Vec3::from(on_curve))
                    > tolerance
            {
                continue 'coedges;
            }
            let Some(normal) = surface_display_normal(surface, uv) else {
                continue 'coedges;
            };
            normals.push(normal);
        }
        folded |= normals
            .windows(2)
            .any(|pair| ocs_geom::tessellation::max_direction_angle(pair) > NORMAL_REVERSAL);
        split |= angle_exceeds(
            ocs_geom::tessellation::max_direction_angle(&normals),
            max_angle,
        );
    }
    if split && !curve_turns && folded {
        // Only a face's normal asks for the split, and it reverses between
        // two neighbouring samples: a fold, where it turns at any scale and
        // halving would run to MAX_DEPTH and lose the edge. While the chord
        // stays within the deflection of the curve, the piece is kept; the
        // faces' own refinement settles a fold the same way.
        let (start, end) = (curve.point_at(from), curve.point_at(to));
        let chord_middle = [0, 1, 2].map(|axis| 0.5 * (start[axis] + end[axis]));
        if distance3(curved.to_array(), chord_middle) <= fold_deviation(tolerance) {
            return Some(());
        }
    }
    if split {
        if depth >= MAX_DEPTH {
            return None;
        }
        refine_edge(
            body,
            edge_key,
            from,
            middle,
            max_angle,
            tolerance,
            depth + 1,
            samples,
            coedge_directions,
            budget,
            normals_follow,
        )?;
        samples.push(super::place::EdgeSample {
            parameter: middle,
            position: curved.to_array(),
        });
        refine_edge(
            body,
            edge_key,
            middle,
            to,
            max_angle,
            tolerance,
            depth + 1,
            samples,
            coedge_directions,
            budget,
            normals_follow,
        )?;
    }
    Some(())
}

/// Whether a circular edge's faces turn their normals along it no faster
/// than the circle turns its tangent: a plane it lies in, a sphere, and a
/// cylinder or cone round the circle's own axis. There the normals can never
/// ask for a finer division than the tangent does, and checking them at every
/// step of every circle was most of a perforated plate's meshing.
fn normals_follow_circle(body: &Body, edge_key: EdgeKey) -> bool {
    use super::geometry::{Curve3, Surface};
    let Some(edge) = body.edges.get(edge_key) else {
        return false;
    };
    let Some(Curve3::Circle(circle)) = body.curves.get(edge.curve) else {
        return false;
    };
    let (Some(normal), centre) = (circle.plane.normal(), Vec3::from(circle.plane.origin)) else {
        return false;
    };
    let normal = Vec3::from(normal);
    let size = circle.radius.max(1.0);
    let round_axis = |frame: &ocs_geom::space::Plane| {
        frame.normal().is_some_and(|axis| {
            let axis = Vec3::from(axis);
            let off = centre - Vec3::from(frame.origin);
            axis.cross(normal).length() <= 1e-12 && axis.cross(off).length() <= 1e-9 * size
        })
    };
    !edge.coedges.is_empty()
        && edge.coedges.iter().all(|coedge| {
            coedge_geometry(body, *coedge).is_some_and(|(surface, _)| match surface {
                Surface::Plane(plane) => plane
                    .normal()
                    .is_some_and(|axis| Vec3::from(axis).cross(normal).length() <= 1e-12),
                Surface::Sphere(_) => true,
                Surface::Cylinder(cylinder) => round_axis(&cylinder.base),
                Surface::Cone(cone) => round_axis(&cone.base),
                _ => false,
            })
        })
}

fn coedge_geometry<'a>(
    body: &'a Body,
    coedge_key: super::topology::CoedgeKey,
) -> Option<(
    &'a super::geometry::Surface,
    Option<&'a ocs_geom::geom2d::Curve>,
)> {
    let coedge = body.coedges.get(coedge_key)?;
    let face = body.faces.get(body.loops.get(coedge.owner)?.owner)?;
    let surface = body.surfaces.get(face.surface)?;
    Some((surface, coedge.pcurve.as_ref()))
}

#[derive(Clone)]
struct BoundaryPoint {
    parameters: [f64; 2],
    position: [f64; 3],
}

fn scheduled_face(
    body: &Body,
    face: FaceKey,
    max_angle: f64,
    tolerance: f64,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
) -> Option<Mesh> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    if let Some(domain) = whole_surface_domain(body, face, surface) {
        return fill_whole_surface(body, face, surface, domain, max_angle, tolerance);
    }
    if let Some(band) = scheduled_band(body, face, surface, schedules, tolerance)
        .or_else(|| scheduled_periodic_band(body, face, surface, schedules, tolerance))
        .or_else(|| scheduled_winding_band(body, face, surface, schedules, tolerance))
        .or_else(|| scheduled_singular_band(body, face, surface, schedules, tolerance))
    {
        if !band.strip
            && band.holes.is_empty()
            && matches!(
                surface,
                super::geometry::Surface::Cone(_) | super::geometry::Surface::Torus(_)
            )
            && rim_runs_one_way(&band)
        {
            return fill_scheduled_singular_cap(body, face, &band, max_angle);
        }
        let zipped = |rim: &[BoundaryPoint]| {
            periods(surface)[band.varying]
                .is_none_or(|period| is_monotonic_periodic_rim(rim, band.varying, period))
        };
        if band.strip
            && band.holes.is_empty()
            && (band.structured
                || (!surface.is_spline()
                    && periods(surface)[1 - band.varying].is_none()
                    && zipped(&band.low)
                    && zipped(&band.high)))
        {
            return fill_scheduled_band(body, face, &band, max_angle, tolerance);
        }
        let mut rings = vec![band.ring_with_seams(surface, max_angle)?];
        rings.extend(band.holes);
        let rings = align_rings(rings, periods(surface));
        return fill_scheduled(body, face, surface, &rings, max_angle, tolerance);
    }
    // No band: the search above has already run, so go straight to the
    // rings [`face_rings`] would fall back to.
    let rings = unbanded_face_rings(body, face, surface, schedules, tolerance)?;
    fill_scheduled(body, face, surface, &rings, max_angle, tolerance)
}

fn face_rings(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<Vec<Vec<BoundaryPoint>>> {
    if let Some(band) = scheduled_band(body, face, surface, schedules, tolerance)
        .or_else(|| scheduled_periodic_band(body, face, surface, schedules, tolerance))
        .or_else(|| scheduled_winding_band(body, face, surface, schedules, tolerance))
        .or_else(|| scheduled_singular_band(body, face, surface, schedules, tolerance))
    {
        let mut rings = vec![band.ring()];
        rings.extend(band.holes);
        return Some(align_rings(rings, periods(surface)));
    }
    unbanded_face_rings(body, face, surface, schedules, tolerance)
}

/// A face's boundary rings from its edge schedules, for a face no band
/// search covers.
fn unbanded_face_rings(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<Vec<Vec<BoundaryPoint>>> {
    if let Some(rings) = scheduled_rings(body, face, surface, schedules, tolerance) {
        if rings.iter().any(|ring| boundary_area(ring)) {
            return Some(align_rings(rings, periods(surface)));
        }
    }
    None
}

fn scheduled_rings(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<Vec<Vec<BoundaryPoint>>> {
    let node = body.faces.get(face)?;
    let mut rings = Vec::with_capacity(node.loops.len());
    for loop_key in &node.loops {
        let points = scheduled_loop(body, *loop_key, surface, schedules, tolerance)?;
        if points.len() >= 3 {
            rings.push(points);
        }
    }
    (!rings.is_empty()).then_some(rings)
}

fn scheduled_loop(
    body: &Body,
    loop_key: super::topology::LoopKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<Vec<BoundaryPoint>> {
    let ring = body.loops.get(loop_key)?;
    let mut uses = Vec::with_capacity(ring.coedges.len());
    for coedge_key in &ring.coedges {
        let coedge = body.coedges.get(*coedge_key)?;
        let cancels = uses
            .last()
            .and_then(|previous| body.coedges.get(*previous))
            .is_some_and(|previous| {
                previous.edge == coedge.edge
                    && previous.forward != coedge.forward
                    && previous.pcurve.is_none()
                    && coedge.pcurve.is_none()
            });
        if cancels {
            uses.pop();
        } else {
            uses.push(*coedge_key);
        }
    }
    while uses.len() >= 2 {
        let first = body.coedges.get(uses[0])?;
        let last = body.coedges.get(*uses.last()?)?;
        if first.edge != last.edge
            || first.forward == last.forward
            || first.pcurve.is_some()
            || last.pcurve.is_some()
        {
            break;
        }
        uses.remove(0);
        uses.pop();
    }
    // A loop that was nothing but a slit — one edge run there and back —
    // cancels to nothing and bounds no area. It is skipped, not failed: a
    // `None` here would take the whole face with it.
    if uses.is_empty() {
        return Some(Vec::new());
    }
    let mut pieces = Vec::with_capacity(uses.len());
    for coedge_key in uses {
        let coedge = body.coedges.get(coedge_key)?;
        let mut samples = schedules.get(&coedge.edge)?.clone();
        if !coedge.forward {
            samples.reverse();
        }
        if samples.len() >= 2 {
            pieces.push((samples, coedge.pcurve.as_ref()));
        }
    }
    chain_samples(surface, pieces, tolerance)
}

fn chain_samples(
    surface: &super::geometry::Surface,
    pieces: Vec<(Vec<super::place::EdgeSample>, Option<&ocs_geom::geom2d::Curve>)>,
    tolerance: f64,
) -> Option<Vec<BoundaryPoint>> {
    let surface_periods = periods(surface);
    // Neighbouring edges meet at their shared vertex, but a file's edges are
    // often only fitted to within its own tolerance and may end a hair apart
    // (5e-5 on a 100-unit solid, #1538). The joins accept a gap that small
    // next to the ring rather than the fitting tolerance alone.
    let join = {
        let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for sample in pieces.iter().flat_map(|(samples, _)| samples) {
            for axis in 0..3 {
                low[axis] = low[axis].min(sample.position[axis]);
                high[axis] = high[axis].max(sample.position[axis]);
            }
        }
        let size = distance3(low, high);
        tolerance.max(if size.is_finite() { size * 1e-5 } else { 0.0 })
    };
    let mut pieces = pieces.into_iter();
    let (first, pcurve) = pieces.next()?;
    let (mut points, mut explicit) = parameterize_samples(surface, &first, pcurve, tolerance)?;
    let mut all_explicit = explicit;
    let mut shifted = [0.0; 2];
    unwrap_boundary(&mut points, surface_periods);
    let mut pieces = pieces.peekable();
    while let Some((samples, pcurve)) = pieces.next() {
        let head = points.last()?.position;
        let (mut next, next_explicit) = parameterize_samples(surface, &samples, pcurve, tolerance)?;
        unwrap_boundary(&mut next, surface_periods);
        // Explicit pcurves share one face parameter space. In particular,
        // opposite sides of a periodic trimmed patch may deliberately differ
        // by one full period while meeting at an axis singularity; folding
        // that difference away erases the patch's parameter-space area.
        if !explicit || !next_explicit {
            let raw = next.first()?.parameters;
            align_parameters(&mut next, &points, surface_periods, pieces.peek().is_none());
            // Where an explicit run joins one placed by projection, the
            // turn it was moved by carries on through the rest of the run.
            let moved = next.first()?.parameters;
            shifted = [moved[0] - raw[0], moved[1] - raw[1]];
        } else {
            for point in &mut next {
                point.parameters[0] += shifted[0];
                point.parameters[1] += shifted[1];
            }
            if let Some(shift) = ordinary_period_jump(
                surface,
                points.last()?.parameters,
                next[0].parameters,
                surface_periods,
            ) {
                // Two explicit pcurves on either side of an ordinary seam
                // point that merely name it from different turns: the jump
                // is not a pole's, so it is folded rather than kept as a
                // boundary segment spanning a whole period (#1538).
                for point in &mut next {
                    point.parameters[0] += shift[0];
                    point.parameters[1] += shift[1];
                }
                shifted = [shifted[0] + shift[0], shifted[1] + shift[1]];
            }
        }
        if distance3(head, next[0].position) > join {
            return None;
        }
        let skip = if explicit
            && next_explicit
            && !same_corner(points.last()?.parameters, next[0].parameters)
        {
            0
        } else {
            1
        };
        points.extend_from_slice(&next[skip..]);
        explicit = next_explicit;
        all_explicit &= next_explicit;
    }
    if distance3(points.first()?.position, points.last()?.position) > join {
        return None;
    }
    let first = points.first()?.parameters;
    let last = points.last()?.parameters;
    if all_explicit && !same_corner(first, last) {
        return Some(points);
    }
    let mut closing = last;
    let mut winds_period = false;
    for axis in 0..2 {
        let Some(period) = surface_periods[axis] else {
            continue;
        };
        let traversal = last[axis] - first[axis];
        if traversal.abs() < period * 0.5 {
            continue;
        }
        let target = first[axis] + traversal.signum() * period;
        let mut candidate = closing;
        candidate[axis] = target;
        if distance3(
            points.last()?.position,
            surface.point_at(candidate[0], candidate[1]),
        ) <= tolerance
        {
            closing = candidate;
            winds_period = true;
        }
    }
    if !winds_period {
        points.pop();
    } else {
        points.last_mut()?.parameters = closing;
    }
    Some(points)
}

fn boundary_area(points: &[BoundaryPoint]) -> bool {
    let parameters: Vec<[f64; 2]> = points.iter().map(|point| point.parameters).collect();
    let area = boundary_area_value(points);
    let spans = [0, 1].map(|axis| {
        let low = parameters
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min);
        let high = parameters
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        high - low
    });
    area.is_finite() && area > f64::EPSILON * 64.0 * spans[0] * spans[1]
}

fn boundary_area_value(points: &[BoundaryPoint]) -> f64 {
    ocs_geom::geom2d::signed_area(
        &points
            .iter()
            .map(|point| point.parameters)
            .collect::<Vec<_>>(),
    )
    .abs()
}

fn align_rings(
    mut rings: Vec<Vec<BoundaryPoint>>,
    periods: [Option<f64>; 2],
) -> Vec<Vec<BoundaryPoint>> {
    let Some(outer) = rings
        .iter()
        .max_by(|a, b| boundary_area_value(a).total_cmp(&boundary_area_value(b)))
        .map(|ring| boundary_centroid(ring))
    else {
        return rings;
    };
    for ring in &mut rings {
        let centre = boundary_centroid(ring);
        for axis in 0..2 {
            if let Some(period) = periods[axis] {
                let shift = period * ((outer[axis] - centre[axis]) / period).round();
                for point in ring.iter_mut() {
                    point.parameters[axis] += shift;
                }
            }
        }
    }
    rings
}

fn boundary_centroid(points: &[BoundaryPoint]) -> [f64; 2] {
    let count = points.len().max(1) as f64;
    points.iter().fold([0.0; 2], |sum, point| {
        [
            sum[0] + point.parameters[0] / count,
            sum[1] + point.parameters[1] / count,
        ]
    })
}

fn parameterize(
    surface: &super::geometry::Surface,
    positions: &[[f64; 3]],
) -> Option<Vec<BoundaryPoint>> {
    let periods = periods(surface);
    let mut parameters: Vec<[f64; 2]> = positions
        .iter()
        .map(|position| match surface.parameters_at(*position) {
            Some((u, v)) => Some([u, v]),
            None => singular_parameters(surface, *position),
        })
        .collect::<Option<_>>()?;
    for axis in 0..2 {
        if periods[axis].is_some() {
            for index in 0..parameters.len() {
                if parameters[index][axis].is_finite() {
                    continue;
                }
                parameters[index][axis] = (1..parameters.len())
                    .flat_map(|offset| {
                        [
                            index.checked_sub(offset),
                            (index + offset < parameters.len()).then_some(index + offset),
                        ]
                    })
                    .flatten()
                    .find_map(|near| {
                        parameters[near][axis]
                            .is_finite()
                            .then_some(parameters[near][axis])
                    })?;
            }
        }
    }
    let mut previous: Option<[f64; 2]> = None;
    positions
        .iter()
        .zip(parameters)
        .map(|(position, mut parameters)| {
            if let Some(last) = previous {
                for axis in 0..2 {
                    if let Some(period) = periods[axis] {
                        parameters[axis] = unwound(parameters[axis], last[axis], period);
                    }
                }
            }
            previous = Some(parameters);
            Some(BoundaryPoint {
                parameters,
                position: *position,
            })
        })
        .collect()
}

fn singular_parameters(surface: &super::geometry::Surface, position: [f64; 3]) -> Option<[f64; 2]> {
    let super::geometry::Surface::Sphere(sphere) = surface else {
        return None;
    };
    let normal = Vec3::from(sphere.frame.normal()?);
    let height = (Vec3::from(position) - Vec3::from(sphere.frame.origin)).dot(normal);
    Some([f64::NAN, if height >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 }])
}

fn closest_pcurve_parameters(
    surface: &super::geometry::Surface,
    pcurve: &ocs_geom::geom2d::Curve,
    position: [f64; 3],
    preferred: f64,
    tolerance: f64,
) -> Option<(f64, [f64; 2], f64)> {
    const COARSE: usize = 32;
    let distance_at = |parameter: f64| {
        let uv = pcurve.point_at(parameter);
        let distance = distance3(position, surface.point_at(uv[0], uv[1]));
        distance.is_finite().then_some((uv, distance * distance))
    };
    let mut parameter = preferred.clamp(0.0, 1.0);
    let mut projected = None;
    for _ in 0..12 {
        let (uv, squared) = distance_at(parameter)?;
        if projected.is_none_or(|(_, _, best): (f64, [f64; 2], f64)| squared < best) {
            projected = Some((parameter, uv, squared));
        }
        if squared.sqrt() <= tolerance {
            return Some((parameter, uv, squared.sqrt()));
        }
        let uv_tangent = pcurve.tangent_at(parameter);
        let (along_u, along_v) = surface.tangents_at(uv[0], uv[1])?;
        let tangent = Vec3::from(along_u) * uv_tangent[0] + Vec3::from(along_v) * uv_tangent[1];
        let length2 = tangent.dot(tangent);
        if !length2.is_finite() || length2 <= f64::MIN_POSITIVE {
            break;
        }
        let point = Vec3::from(surface.point_at(uv[0], uv[1]));
        let correction = ((point - Vec3::from(position)).dot(tangent) / length2).clamp(-0.25, 0.25);
        let next = (parameter - correction).clamp(0.0, 1.0);
        if parameter_value_near(next, parameter) {
            break;
        }
        parameter = next;
    }
    let mut candidates: Vec<f64> = (0..=COARSE)
        .map(|index| index as f64 / COARSE as f64)
        .collect();
    if let ocs_geom::geom2d::Curve::Nurbs(curve) = pcurve {
        let (from, to) = curve.domain();
        let span = to - from;
        if span.is_finite() && span > 0.0 {
            candidates.extend(
                curve
                    .knots()
                    .iter()
                    .filter(|knot| **knot >= from && **knot <= to)
                    .map(|knot| (*knot - from) / span),
            );
        }
    }
    candidates.sort_by(f64::total_cmp);
    candidates.dedup_by(|a, b| parameter_value_near(*a, *b));
    let midpoints: Vec<f64> = candidates
        .windows(2)
        .map(|pair| 0.5 * (pair[0] + pair[1]))
        .collect();
    candidates.extend(midpoints);
    candidates.sort_by(f64::total_cmp);
    let mut best = projected;
    let mut best_index = 0;
    for (index, parameter) in candidates.iter().copied().enumerate() {
        let (uv, distance) = distance_at(parameter)?;
        let replace =
            best.is_none_or(|(best_parameter, _, best_distance): (f64, [f64; 2], f64)| {
                distance < best_distance
                    || (parameter_value_near(distance, best_distance)
                        && (parameter - preferred).abs() < (best_parameter - preferred).abs())
            });
        if replace {
            best = Some((parameter, uv, distance));
            best_index = index;
        }
    }
    let mut low = candidates[best_index.saturating_sub(1)];
    let mut high = candidates[(best_index + 1).min(candidates.len() - 1)];
    for _ in 0..56 {
        let left = low + (high - low) / 3.0;
        let right = high - (high - low) / 3.0;
        if distance_at(left)?.1 <= distance_at(right)?.1 {
            high = right;
        } else {
            low = left;
        }
    }
    for parameter in [low, 0.5 * (low + high), high] {
        let (uv, distance) = distance_at(parameter)?;
        let replace = best.is_none_or(|(best_parameter, _, best_distance)| {
            distance < best_distance
                || (parameter_value_near(distance, best_distance)
                    && (parameter - preferred).abs() < (best_parameter - preferred).abs())
        });
        if replace {
            best = Some((parameter, uv, distance));
        }
    }
    best.map(|(parameter, uv, squared)| (parameter, uv, squared.sqrt()))
}

fn parameterize_samples(
    surface: &super::geometry::Surface,
    samples: &[super::place::EdgeSample],
    pcurve: Option<&ocs_geom::geom2d::Curve>,
    tolerance: f64,
) -> Option<(Vec<BoundaryPoint>, bool)> {
    if let Some(pcurve) = pcurve {
        let last = samples.len().checked_sub(1)?;
        let first_parameter = samples.first()?.parameter;
        let parameter_span = samples.last()?.parameter - first_parameter;
        let mut forward_deviation = 0.0;
        let mut reversed_deviation = 0.0;
        for index in [0, last / 4, last / 2, last * 3 / 4, last] {
            let preferred = if parameter_span.abs() > f64::EPSILON {
                (samples[index].parameter - first_parameter) / parameter_span
            } else {
                0.0
            };
            let direct = pcurve.point_at(preferred);
            let reverse = pcurve.point_at(1.0 - preferred);
            forward_deviation += distance3(
                samples[index].position,
                surface.point_at(direct[0], direct[1]),
            );
            reversed_deviation += distance3(
                samples[index].position,
                surface.point_at(reverse[0], reverse[1]),
            );
        }
        let pcurve_forward = forward_deviation <= reversed_deviation;
        let mapped: Option<Vec<(f64, BoundaryPoint)>> = samples
            .iter()
            .map(|sample| {
                let mut preferred = if parameter_span.abs() > f64::EPSILON {
                    (sample.parameter - first_parameter) / parameter_span
                } else {
                    0.0
                };
                if !pcurve_forward {
                    preferred = 1.0 - preferred;
                }
                let direct = pcurve.point_at(preferred);
                let direct_deviation =
                    distance3(sample.position, surface.point_at(direct[0], direct[1]));
                let (parameter, parameters, deviation) = if direct_deviation <= tolerance {
                    (preferred, direct, direct_deviation)
                } else {
                    closest_pcurve_parameters(
                        surface,
                        pcurve,
                        sample.position,
                        preferred,
                        tolerance,
                    )?
                };
                (deviation <= tolerance).then_some((
                    parameter,
                    BoundaryPoint {
                        parameters,
                        position: sample.position,
                    },
                ))
            })
            .collect();
        if let Some(mut mapped) = mapped {
            if mapped.len() > 3
                && distance3(
                    surface.point_at(pcurve.point_at(0.0)[0], pcurve.point_at(0.0)[1]),
                    surface.point_at(pcurve.point_at(1.0)[0], pcurve.point_at(1.0)[1]),
                ) <= tolerance
                && mapped[1].0 > mapped[mapped.len() - 2].0
            {
                let first = pcurve.point_at(1.0);
                let end = pcurve.point_at(0.0);
                mapped[0] = (
                    1.0,
                    BoundaryPoint {
                        parameters: first,
                        position: samples[0].position,
                    },
                );
                mapped[last] = (
                    0.0,
                    BoundaryPoint {
                        parameters: end,
                        position: samples[last].position,
                    },
                );
            }
            return Some((mapped.into_iter().map(|(_, point)| point).collect(), true));
        }
    }
    let positions: Vec<[f64; 3]> = samples.iter().map(|sample| sample.position).collect();
    parameterize(surface, &positions).map(|points| (points, false))
}

/// The shift that brings `to` onto `from` when the two differ by whole
/// periods on one axis at a point where that axis is not singular — moving
/// along it moves the point, so the two parameters are one ordinary place.
/// `None` for a real parameter jump or one at a pole (where the patch may
/// legitimately span a period between its sides).
fn ordinary_period_jump(
    surface: &super::geometry::Surface,
    from: [f64; 2],
    to: [f64; 2],
    periods: [Option<f64>; 2],
) -> Option<[f64; 2]> {
    for axis in 0..2 {
        let Some(period) = periods[axis] else {
            continue;
        };
        let other = 1 - axis;
        let turns = ((from[axis] - to[axis]) / period).round();
        if turns == 0.0
            || !parameter_value_near(to[axis] + turns * period, from[axis])
            || !parameter_value_near(to[other], from[other])
        {
            continue;
        }
        let mut nudged = from;
        nudged[axis] += period * 0.01;
        let here = surface.point_at(from[0], from[1]);
        let there = surface.point_at(nudged[0], nudged[1]);
        let singular = distance3(here, there) <= 1e-9 * (1.0 + distance3(here, [0.0; 3]));
        if singular {
            return None;
        }
        let mut shift = [0.0; 2];
        shift[axis] = turns * period;
        return Some(shift);
    }
    None
}

fn align_parameters(
    points: &mut [BoundaryPoint],
    chain: &[BoundaryPoint],
    periods: [Option<f64>; 2],
    closes_ring: bool,
) {
    let Some(first) = points.first() else {
        return;
    };
    let previous = chain
        .last()
        .map(|point| point.parameters)
        .unwrap_or(first.parameters);
    let last = points
        .last()
        .map(|point| point.parameters)
        .unwrap_or(first.parameters);
    let mut best = (f64::INFINITY, [0.0; 2]);
    for across in period_shifts(periods[0]) {
        for along in period_shifts(periods[1]) {
            let shift = [across, along];
            let moved_first = [first.parameters[0] + across, first.parameters[1] + along];
            let moved_last = [last[0] + across, last[1] + along];
            if !closes_ring
                && parameter_near(moved_first, previous)
                && chain
                    .first()
                    .is_some_and(|first| parameter_near(moved_last, first.parameters))
            {
                continue;
            }
            let gap = (moved_first[0] - previous[0]).hypot(moved_first[1] - previous[1]);
            if gap < best.0 {
                best = (gap, shift);
            }
        }
    }
    for point in points {
        point.parameters[0] += best.1[0];
        point.parameters[1] += best.1[1];
    }
}

fn period_shifts(period: Option<f64>) -> impl Iterator<Item = f64> {
    let period = period.filter(|value| value.is_finite() && *value > 0.0);
    let turns = if period.is_some() { 2 } else { 0 };
    (-turns..=turns).map(move |turn| f64::from(turn) * period.unwrap_or(0.0))
}

/// Whether two explicit pcurve ends name one corner. Their pcurves are fitted
/// separately and meet only to rounding (3e-13 on a 134-unit domain, #1538);
/// ends that name one place from different turns, as at a pole, differ by far
/// more.
fn same_corner(a: [f64; 2], b: [f64; 2]) -> bool {
    let scale = a.into_iter().chain(b).map(f64::abs).fold(1.0, f64::max);
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9 * scale
}

fn parameter_near(a: [f64; 2], b: [f64; 2]) -> bool {
    let scale = a.into_iter().chain(b).map(f64::abs).fold(1.0, f64::max);
    (a[0] - b[0]).hypot(a[1] - b[1]) <= f64::EPSILON * 64.0 * scale
}

struct BoundaryBand {
    low: Vec<BoundaryPoint>,
    high: Vec<BoundaryPoint>,
    holes: Vec<Vec<BoundaryPoint>>,
    varying: usize,
    strip: bool,
    structured: bool,
}

impl BoundaryBand {
    fn ring(&self) -> Vec<BoundaryPoint> {
        let mut ring = self.low.clone();
        ring.extend(self.high.iter().rev().cloned());
        ring
    }

    fn ring_with_seams(
        &self,
        surface: &super::geometry::Surface,
        max_angle: f64,
    ) -> Option<Vec<BoundaryPoint>> {
        let mut ring = self.low.clone();
        let end = parameter_seam(
            surface,
            self.varying,
            self.low.last()?,
            self.high.last()?,
            max_angle,
        )?;
        if end.len() > 2 {
            ring.extend_from_slice(&end[1..end.len() - 1]);
        }
        ring.extend(self.high.iter().rev().cloned());
        let start = parameter_seam(
            surface,
            self.varying,
            self.high.first()?,
            self.low.first()?,
            max_angle,
        )?;
        if start.len() > 2 {
            ring.extend_from_slice(&start[1..start.len() - 1]);
        }
        Some(ring)
    }
}

fn scheduled_band(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<BoundaryBand> {
    let node = body.faces.get(face)?;
    let surface_periods = periods(surface);
    let mut candidates = Vec::new();
    let mut refined_side = false;
    let mut candidate_loops = Vec::new();
    for loop_key in &node.loops {
        let ring = body.loops.get(*loop_key)?;
        if ring.coedges.is_empty() {
            continue;
        }
        let mut loop_has_candidate = false;
        let mut loop_has_refined_side = false;
        for coedge_key in &ring.coedges {
            let coedge = body.coedges.get(*coedge_key)?;
            let mut samples = schedules.get(&coedge.edge)?.clone();
            if !coedge.forward {
                samples.reverse();
            }
            let (mut points, _) =
                parameterize_samples(surface, &samples, coedge.pcurve.as_ref(), tolerance)?;
            unwrap_boundary(&mut points, surface_periods);
            let varying = (0..2)
                .filter(|axis| surface_periods[*axis].is_some())
                .filter(|axis| {
                    let period = surface_periods[*axis].unwrap();
                    points.len() > 2
                        && (points.last().unwrap().parameters[*axis]
                            - points.first().unwrap().parameters[*axis])
                            .abs()
                            >= period * (1.0 - 1e-9)
                        && is_isoparametric_rim(surface, &points, *axis, tolerance)
                })
                .max_by(|a, b| {
                    parameter_range(&points, *a).total_cmp(&parameter_range(&points, *b))
                });
            // A seam, the face meeting itself along a meridian down to a
            // pole, bounds nothing: the band closes across it. A spline
            // surface's seams keep to the general mesher, which reads one
            // the same however the file wrote it.
            let seam = !surface.is_spline()
                && body.edges.get(coedge.edge).is_some_and(|edge| {
                    edge.coedges.len() == 2
                        && edge.coedges.iter().all(|other| {
                            body.coedges.get(*other).is_some_and(|other| other.owner == *loop_key)
                        })
                });
            if let Some(varying) = varying {
                candidates.push((varying, points));
                loop_has_candidate = true;
            } else if !seam {
                loop_has_refined_side |= samples.len() > 2;
            }
        }
        if !loop_has_candidate {
            let points = scheduled_loop(body, *loop_key, surface, schedules, tolerance)?;
            let varying = (0..2)
                .filter_map(|axis| surface_periods[axis].map(|period| (axis, period)))
                .filter(|(axis, period)| {
                    points
                        .first()
                        .zip(points.last())
                        .is_some_and(|(first, last)| {
                            (last.parameters[*axis] - first.parameters[*axis]).abs()
                                >= *period * (1.0 - 1e-9)
                        })
                        && is_isoparametric_rim(surface, &points, *axis, tolerance)
                })
                .map(|(axis, _)| axis)
                .max_by(|a, b| {
                    parameter_range(&points, *a).total_cmp(&parameter_range(&points, *b))
                });
            if let Some(varying) = varying {
                candidates.push((varying, points));
                candidate_loops.push(*loop_key);
                continue;
            }
        }
        if loop_has_candidate {
            candidate_loops.push(*loop_key);
            refined_side |= loop_has_refined_side;
        }
    }
    if !(1..=2).contains(&candidates.len()) || refined_side {
        return None;
    }
    let varying = candidates[0].0;
    if candidates.iter().any(|(axis, _)| *axis != varying) {
        return None;
    }
    let mut rims: Vec<_> = candidates.into_iter().map(|(_, rim)| rim).collect();
    let period = surface_periods[varying]?;
    let fixed = 1 - varying;
    let mut holes = Vec::new();
    let mut winding_rim = None;
    let mut singular_loops = 0;
    for loop_key in &node.loops {
        if candidate_loops.contains(loop_key) {
            continue;
        }
        if body.loops.get(*loop_key)?.coedges.is_empty() {
            singular_loops += 1;
            if rims.len() != 1 || singular_loops > 1 {
                return None;
            }
            continue;
        }
        let ring = scheduled_loop(body, *loop_key, surface, schedules, tolerance)?;
        if ring.len() >= 3 {
            let traversal = ring.last()?.parameters[varying] - ring.first()?.parameters[varying];
            // A loop round the surface travels a whole turn, a hole none: a
            // traced loop's pieces, a fit apart where they meet, fall a hair
            // short of the turn.
            if rims.len() == 1 && winding_rim.is_none() && traversal.abs() >= period * 0.5 {
                winding_rim = Some(ring);
            } else {
                holes.push(ring);
            }
        }
    }
    let structured = winding_rim
        .as_ref()
        .is_none_or(|rim| is_isoparametric_rim(surface, rim, varying, tolerance));
    if let Some(rim) = winding_rim {
        rims.push(rim);
    }
    if rims.len() == 2
        && holes.is_empty()
        && surface_periods[fixed].is_none()
        && rims
            .iter()
            .any(|rim| !is_monotonic_periodic_rim(rim, varying, period))
    {
        return cut_winding_band(surface, &rims, varying, period);
    }
    let traversal: Vec<f64> = rims
        .iter()
        .map(|rim| rim.last().unwrap().parameters[varying] - rim[0].parameters[varying])
        .collect();
    for rim in &mut rims {
        rim.sort_by(|a, b| a.parameters[varying].total_cmp(&b.parameters[varying]));
    }
    let base = rims[0][0].parameters[varying];
    for rim in rims.iter_mut().skip(1) {
        for point in rim.iter_mut() {
            point.parameters[varying] =
                base + (point.parameters[varying] - base).rem_euclid(period);
        }
        rim.sort_by(|a, b| a.parameters[varying].total_cmp(&b.parameters[varying]));
        rim.dedup_by(|a, b| parameter_value_near(a.parameters[varying], b.parameters[varying]));
        let mut closing = rim.first()?.clone();
        closing.parameters[varying] += period;
        rim.push(closing);
    }
    if rims.len() == 2
        && (!holes.is_empty() || !structured)
        && parameter_range(&rims[0], varying) >= period * (1.0 - 1e-9)
    {
        fit_periodic_band(surface, &mut rims, &mut holes, varying, period)?;
    }
    let mut bounds: Vec<f64> = rims
        .iter()
        .map(|rim| average_parameter(rim, fixed))
        .collect();
    if structured {
        for (rim, value) in rims.iter().zip(&bounds) {
            if rim.iter().any(|point| {
                let mut parameters = point.parameters;
                parameters[fixed] = *value;
                distance3(
                    point.position,
                    surface.point_at(parameters[0], parameters[1]),
                ) > tolerance
            }) {
                return None;
            }
        }
    }
    if rims.len() == 2 {
        if let Some(fixed_period) = surface_periods[fixed] {
            let outward = if node.forward {
                traversal[0]
            } else {
                -traversal[0]
            };
            let positive = (varying == 0 && outward > 0.0) || (varying == 1 && outward < 0.0);
            let delta = if positive {
                (bounds[1] - bounds[0]).rem_euclid(fixed_period)
            } else {
                -(bounds[0] - bounds[1]).rem_euclid(fixed_period)
            };
            let target = bounds[0] + delta;
            let shift = target - bounds[1];
            bounds[1] = target;
            for point in &mut rims[1] {
                point.parameters[fixed] += shift;
            }
        }
    }
    let strip = rims.len() == 2;
    if !strip {
        let target = if let super::geometry::Surface::Cone(cone) = surface {
            let apex = cone.radius / cone.half_angle.tan();
            boundary_collapsed(surface, varying, base, period, fixed, apex, tolerance)
                .then_some(apex)?
        } else if let super::geometry::Surface::Torus(torus) = surface {
            if varying != 0 || torus.minor_radius.abs() <= f64::EPSILON {
                return None;
            }
            let ratio = -torus.major_radius / torus.minor_radius;
            if !ratio.is_finite() || ratio.abs() > 1.0 {
                return None;
            }
            let near = bounds[0];
            [ratio.acos(), -ratio.acos()]
                .map(|value| unwound(value, near, TAU))
                .into_iter()
                .filter(|value| {
                    boundary_collapsed(surface, varying, base, period, fixed, *value, tolerance)
                })
                .min_by(|a, b| (a - near).abs().total_cmp(&(b - near).abs()))?
        } else {
            let domain = surface_domain(surface)?;
            let candidates = [domain[fixed][0], domain[fixed][1]];
            let collapsed = candidates.map(|value| {
                boundary_collapsed(surface, varying, base, period, fixed, value, tolerance)
            });
            match collapsed {
                [true, false] => candidates[0],
                [false, true] => candidates[1],
                [true, true] => {
                    let outward = if node.forward {
                        traversal[0]
                    } else {
                        -traversal[0]
                    };
                    if (varying == 0 && outward > 0.0) || (varying == 1 && outward < 0.0) {
                        candidates[1]
                    } else {
                        candidates[0]
                    }
                }
                _ => return None,
            }
        };
        let position = surface.point_at(
            if varying == 0 { base } else { target },
            if varying == 0 { target } else { base },
        );
        let mut singular = vec![
            BoundaryPoint {
                parameters: [0.0; 2],
                position,
            },
            BoundaryPoint {
                parameters: [0.0; 2],
                position,
            },
        ];
        singular[0].parameters[varying] = base;
        singular[1].parameters[varying] = base + period;
        singular[0].parameters[fixed] = target;
        singular[1].parameters[fixed] = target;
        rims.push(singular);
        bounds.push(target);
    }
    let low_index = usize::from(bounds[1] < bounds[0]);
    let high_index = 1 - low_index;
    let mut low = rims.remove(low_index);
    let mut high = rims.remove(if high_index > low_index {
        high_index - 1
    } else {
        high_index
    });
    let low_fixed = bounds[low_index];
    let high_fixed = bounds[high_index];
    if structured {
        for point in &mut low {
            point.parameters[fixed] = low_fixed;
        }
        for point in &mut high {
            point.parameters[fixed] = high_fixed;
        }
    }
    Some(BoundaryBand {
        low,
        high,
        holes,
        varying,
        strip,
        structured,
    })
}

/// Two rims winding round a periodic surface, one of which doubles back
/// along the way (a notch cut into a cylinder's edge), cut at one value of
/// the varying parameter that each crosses exactly once. Sorting such a rim
/// by that parameter, as a plain band does, tears the notch apart; cut
/// there, the two rims and the seam between them bound an ordinary region.
fn cut_winding_band(
    surface: &super::geometry::Surface,
    rims: &[Vec<BoundaryPoint>],
    varying: usize,
    period: f64,
) -> Option<BoundaryBand> {
    let rims: Vec<Vec<BoundaryPoint>> = rims
        .iter()
        .map(|rim| {
            let mut rim = rim.clone();
            if rim.last()?.parameters[varying] < rim.first()?.parameters[varying] {
                rim.reverse();
            }
            Some(rim)
        })
        .collect::<Option<_>>()?;
    let turn = |value: f64, seam: f64| ((value - seam) / period).floor();
    let crossings = |rim: &[BoundaryPoint], seam: f64| -> f64 {
        rim.windows(2)
            .map(|pair| {
                (turn(pair[1].parameters[varying], seam) - turn(pair[0].parameters[varying], seam))
                    .abs()
            })
            .sum()
    };
    // Seams through the middle of the longest steps first: well clear of any
    // vertex, and of a notch's sides, which do not step along at all.
    let mut steps: Vec<(f64, f64)> = rims
        .iter()
        .flat_map(|rim| rim.windows(2))
        .map(|pair| {
            let (a, b) = (pair[0].parameters[varying], pair[1].parameters[varying]);
            ((b - a).abs(), 0.5 * (a + b))
        })
        .collect();
    steps.sort_by(|a, b| b.0.total_cmp(&a.0));
    let seam = steps
        .into_iter()
        .map(|(_, middle)| middle)
        .find(|seam| rims.iter().all(|rim| crossings(rim, *seam) == 1.0))?;
    let mut cut: Vec<Vec<BoundaryPoint>> = rims
        .iter()
        .map(|rim| {
            let index = rim.windows(2).position(|pair| {
                turn(pair[0].parameters[varying], seam) != turn(pair[1].parameters[varying], seam)
            })?;
            let (a, b) = (&rim[index], &rim[index + 1]);
            let line = seam + turn(b.parameters[varying], seam) * period;
            let t = (line - a.parameters[varying])
                / (b.parameters[varying] - a.parameters[varying]);
            let mut parameters = [0, 1].map(|axis| {
                a.parameters[axis] + (b.parameters[axis] - a.parameters[axis]) * t
            });
            parameters[varying] = line;
            let start = BoundaryPoint {
                parameters,
                position: surface.point_at(parameters[0], parameters[1]),
            };
            let mut out = vec![start.clone()];
            out.extend(rim[index + 1..].iter().cloned());
            out.extend(rim[1..=index].iter().map(|point| {
                let mut point = point.clone();
                point.parameters[varying] += period;
                point
            }));
            let mut end = start;
            end.parameters[varying] += period;
            out.push(end);
            let shift = seam - line;
            for point in &mut out {
                point.parameters[varying] += shift;
            }
            Some(out)
        })
        .collect::<Option<_>>()?;
    let fixed = 1 - varying;
    let first_is_low = average_parameter(&cut[0], fixed) < average_parameter(&cut[1], fixed);
    let (low, high) = if first_is_low {
        (cut.remove(0), cut.remove(0))
    } else {
        (cut.remove(1), cut.remove(0))
    };
    Some(BoundaryBand {
        low,
        high,
        holes: Vec::new(),
        varying,
        strip: true,
        structured: false,
    })
}

fn unwrap_boundary(points: &mut [BoundaryPoint], periods: [Option<f64>; 2]) {
    for index in 1..points.len() {
        for axis in 0..2 {
            if let Some(period) = periods[axis] {
                points[index].parameters[axis] = unwound(
                    points[index].parameters[axis],
                    points[index - 1].parameters[axis],
                    period,
                );
            }
        }
    }
}

fn parameter_range(points: &[BoundaryPoint], axis: usize) -> f64 {
    let low = points
        .iter()
        .map(|point| point.parameters[axis])
        .fold(f64::INFINITY, f64::min);
    let high = points
        .iter()
        .map(|point| point.parameters[axis])
        .fold(f64::NEG_INFINITY, f64::max);
    high - low
}

fn average_parameter(points: &[BoundaryPoint], axis: usize) -> f64 {
    points
        .iter()
        .map(|point| point.parameters[axis])
        .sum::<f64>()
        / points.len() as f64
}

fn scheduled_periodic_band(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<BoundaryBand> {
    let node = body.faces.get(face)?;
    let surface_periods = periods(surface);
    let rings: Vec<Vec<BoundaryPoint>> = node
        .loops
        .iter()
        .map(|loop_key| scheduled_loop(body, *loop_key, surface, schedules, tolerance))
        .collect::<Option<_>>()?;
    for varying in 0..2 {
        let period = surface_periods[varying]?;
        let fixed = 1 - varying;
        let fixed_period = surface_periods[fixed]?;
        let full: Vec<usize> = rings
            .iter()
            .enumerate()
            .filter_map(|(index, ring)| {
                let traversal =
                    ring.last()?.parameters[varying] - ring.first()?.parameters[varying];
                (traversal.abs() >= period * (1.0 - 1e-9)).then_some(index)
            })
            .collect();
        if full.len() != 2 {
            continue;
        }
        let mut rims = vec![rings[full[0]].clone(), rings[full[1]].clone()];
        let mut holes: Vec<Vec<BoundaryPoint>> = rings
            .iter()
            .enumerate()
            .filter_map(|(index, ring)| (!full.contains(&index)).then_some(ring.clone()))
            .collect();
        let traversal = rims[0].last()?.parameters[varying] - rims[0].first()?.parameters[varying];
        let base = rims[0].first()?.parameters[varying];
        let shift = period * ((base - rims[1].first()?.parameters[varying]) / period).round();
        for point in &mut rims[1] {
            point.parameters[varying] += shift;
        }
        fit_periodic_band(surface, &mut rims, &mut holes, varying, period)?;
        let mut bounds = [
            average_parameter(&rims[0], fixed),
            average_parameter(&rims[1], fixed),
        ];
        let outward = if node.forward { traversal } else { -traversal };
        let positive = (varying == 0 && outward > 0.0) || (varying == 1 && outward < 0.0);
        let delta = if positive {
            (bounds[1] - bounds[0]).rem_euclid(fixed_period)
        } else {
            -(bounds[0] - bounds[1]).rem_euclid(fixed_period)
        };
        let target = bounds[0] + delta;
        let fixed_shift = target - bounds[1];
        bounds[1] = target;
        for point in &mut rims[1] {
            point.parameters[fixed] += fixed_shift;
        }
        let low_index = usize::from(bounds[1] < bounds[0]);
        let high_index = 1 - low_index;
        let low = rims.remove(low_index);
        let high = rims.remove(if high_index > low_index {
            high_index - 1
        } else {
            high_index
        });
        return Some(BoundaryBand {
            low,
            high,
            holes,
            varying,
            strip: true,
            structured: false,
        });
    }
    None
}

fn scheduled_singular_band(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<BoundaryBand> {
    let super::geometry::Surface::Cone(cone) = surface else {
        return None;
    };
    let node = body.faces.get(face)?;
    let mut singular_loops = 0;
    let mut boundary_loop = None;
    for loop_key in &node.loops {
        if body.loops.get(*loop_key)?.coedges.is_empty() {
            singular_loops += 1;
        } else if boundary_loop.replace(*loop_key).is_some() {
            return None;
        }
    }
    // A face cut from a whole cone may reach the apex along its seam
    // instead: the seam runs there and back, which the loop drops as a slit,
    // and what is left is the rim alone, as with an apex loop.
    let apex_seam = || -> Option<bool> {
        let apex = surface.point_at(0.0, cone.radius / cone.half_angle.tan());
        let ring = body.loops.get(boundary_loop?)?;
        Some(ring.coedges.iter().any(|coedge| {
            let Some(edge) = body.coedges.get(*coedge).and_then(|c| body.edges.get(c.edge))
            else {
                return false;
            };
            edge.coedges.len() == 2
                && edge.coedges.iter().all(|c| ring.coedges.contains(c))
                && [edge.start, edge.end].iter().any(|vertex| {
                    body.vertices
                        .get(*vertex)
                        .is_some_and(|vertex| distance3(vertex.point, apex) <= tolerance * 10.0)
                })
        }))
    };
    if singular_loops > 1 || (singular_loops == 0 && apex_seam() != Some(true)) {
        return None;
    }
    let varying = 0;
    let fixed = 1;
    let period = periods(surface)[varying]?;
    let mut rim = scheduled_loop(body, boundary_loop?, surface, schedules, tolerance)?;
    let traversal = rim.last()?.parameters[varying] - rim.first()?.parameters[varying];
    // A turn to within where an edge's end parameter puts its corner: an
    // exact curve's end read back off its vertex is a few ulps of the
    // radius out, which is more than 1e-9 of a turn on a small cone.
    if (traversal.abs() - period).abs() > period * 1e-6 {
        return None;
    }
    let increasing = traversal > 0.0;
    let varying_scale = rim
        .iter()
        .map(|point| point.parameters[varying].abs())
        .fold(period.max(1.0), f64::max);
    let varying_epsilon = f64::EPSILON * 128.0 * varying_scale;
    let monotonic = !rim.windows(2).any(|pair| {
        let delta = pair[1].parameters[varying] - pair[0].parameters[varying];
        if increasing {
            delta < -varying_epsilon
        } else {
            delta > varying_epsilon
        }
    });
    // A rim that doubles back a little — a cut climbing the cone near
    // upright — keeps its own order, turned to run with the parameter; the
    // face is then filled as a ring through the apex rather than as a fan
    // (see `scheduled_face`).
    if monotonic {
        rim.sort_by(|a, b| a.parameters[varying].total_cmp(&b.parameters[varying]));
    } else if !increasing {
        rim.reverse();
    }
    let base = rim.first()?.parameters[varying];
    let apex = cone.radius / cone.half_angle.tan();
    if !apex.is_finite()
        || !boundary_collapsed(surface, varying, base, period, fixed, apex, tolerance)
    {
        return None;
    }
    let parameter_scale = rim
        .iter()
        .map(|point| point.parameters[fixed].abs())
        .fold(apex.abs().max(1.0), f64::max);
    let epsilon = f64::EPSILON * 128.0 * parameter_scale;
    let crosses_apex = rim
        .iter()
        .map(|point| point.parameters[fixed] - apex)
        .fold((false, false), |(below, above), delta| {
            (below || delta < -epsilon, above || delta > epsilon)
        });
    if crosses_apex == (true, true) {
        return None;
    }
    let position = surface.point_at(base, apex);
    let singular = vec![
        BoundaryPoint {
            parameters: [base, apex],
            position,
        },
        BoundaryPoint {
            parameters: [base + period, apex],
            position,
        },
    ];
    let rim_fixed = average_parameter(&rim, fixed);
    let (low, high) = if rim_fixed < apex {
        (rim, singular)
    } else {
        (singular, rim)
    };
    Some(BoundaryBand {
        low,
        high,
        holes: Vec::new(),
        varying,
        strip: false,
        structured: false,
    })
}

fn scheduled_winding_band(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    schedules: &HashMap<EdgeKey, Vec<super::place::EdgeSample>>,
    tolerance: f64,
) -> Option<BoundaryBand> {
    let node = body.faces.get(face)?;
    if node.loops.len() < 2 {
        return None;
    }
    let mut rings: Vec<Vec<BoundaryPoint>> = node
        .loops
        .iter()
        .map(|loop_key| {
            (!body.loops.get(*loop_key)?.coedges.is_empty())
                .then(|| scheduled_loop(body, *loop_key, surface, schedules, tolerance))?
        })
        .collect::<Option<_>>()?;
    // A slit loop cancels to no points and bounds no area: skip it, as the
    // other scheduled meshers do, rather than measure a ring with no ends.
    rings.retain(|ring| ring.len() >= 3);
    let surface_periods = periods(surface);
    for varying in 0..2 {
        let Some(period) = surface_periods[varying] else {
            continue;
        };
        if surface_periods[1 - varying].is_some() {
            continue;
        }
        // Two rims winding round once each, and anything else a hole between
        // them that winds round not at all.
        let traversal = |ring: &[BoundaryPoint]| {
            ring.last().unwrap().parameters[varying] - ring.first().unwrap().parameters[varying]
        };
        let (rims, holes): (Vec<_>, Vec<_>) = rings
            .iter()
            .cloned()
            .partition(|ring| traversal(ring).abs() >= period * 0.5);
        if rims.len() != 2
            || rims.iter().any(|ring| !is_monotonic_periodic_rim(ring, varying, period))
        {
            continue;
        }
        let traversals = [0, 1].map(|index| traversal(&rims[index]));
        if traversals[0].is_sign_positive() == traversals[1].is_sign_positive() {
            continue;
        }
        let mut rims = rims;
        let mut holes = holes;
        fit_periodic_band(surface, &mut rims, &mut holes, varying, period)?;
        let first_is_low = separated_rim_order(&rims[0], &rims[1], varying)?;
        let (low, high) = if first_is_low {
            (rims.remove(0), rims.remove(0))
        } else {
            (rims.remove(1), rims.remove(0))
        };
        return Some(BoundaryBand {
            low,
            high,
            holes,
            varying,
            strip: true,
            structured: false,
        });
    }
    None
}

fn is_monotonic_periodic_rim(rim: &[BoundaryPoint], varying: usize, period: f64) -> bool {
    let Some((first, last)) = rim.first().zip(rim.last()) else {
        return false;
    };
    let traversal = last.parameters[varying] - first.parameters[varying];
    if traversal.abs() < period * (1.0 - 1e-9) || traversal.abs() > period * (1.0 + 1e-9) {
        return false;
    }
    let increasing = traversal > 0.0;
    let scale = rim
        .iter()
        .map(|point| point.parameters[varying].abs())
        .fold(period.max(1.0), f64::max);
    // A rim stepping straight across at one value meets the next curve a
    // rounding off that value: two curves evaluated at one corner.
    let epsilon = (f64::EPSILON * 128.0 * scale).max(period * 1e-9);
    rim.windows(2).all(|pair| {
        let delta = pair[1].parameters[varying] - pair[0].parameters[varying];
        if increasing {
            delta >= -epsilon
        } else {
            delta <= epsilon
        }
    })
}

fn separated_rim_order(
    first: &[BoundaryPoint],
    second: &[BoundaryPoint],
    varying: usize,
) -> Option<bool> {
    let fixed = 1 - varying;
    let mut values: Vec<f64> = first
        .iter()
        .chain(second)
        .map(|point| point.parameters[varying])
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| parameter_value_near(*a, *b));
    let scale = first
        .iter()
        .chain(second)
        .map(|point| point.parameters[fixed].abs())
        .fold(1.0, f64::max);
    let epsilon = f64::EPSILON * 128.0 * scale;
    let mut order = None;
    for value in values {
        let delta = rim_fixed_at(second, varying, value)? - rim_fixed_at(first, varying, value)?;
        if delta.abs() <= epsilon {
            return None;
        }
        let current = delta > 0.0;
        if order.is_some_and(|order| order != current) {
            return None;
        }
        order = Some(current);
    }
    order
}

fn rim_fixed_at(rim: &[BoundaryPoint], varying: usize, value: f64) -> Option<f64> {
    let fixed = 1 - varying;
    for pair in rim.windows(2) {
        let from = pair[0].parameters[varying];
        let to = pair[1].parameters[varying];
        if value < from && !parameter_value_near(value, from)
            || value > to && !parameter_value_near(value, to)
        {
            continue;
        }
        let span = to - from;
        if parameter_value_near(span, 0.0) {
            return Some(0.5 * (pair[0].parameters[fixed] + pair[1].parameters[fixed]));
        }
        let unit = ((value - from) / span).clamp(0.0, 1.0);
        return Some(
            pair[0].parameters[fixed]
                + (pair[1].parameters[fixed] - pair[0].parameters[fixed]) * unit,
        );
    }
    None
}

fn parameter_seam(
    surface: &super::geometry::Surface,
    fixed_axis: usize,
    from: &BoundaryPoint,
    to: &BoundaryPoint,
    max_angle: f64,
) -> Option<Vec<BoundaryPoint>> {
    let fixed = 0.5 * (from.parameters[fixed_axis] + to.parameters[fixed_axis]);
    let varying_axis = 1 - fixed_axis;
    let mut points = Vec::new();
    let mut budget = PCURVE_REFINE_BUDGET;
    if !sample_parameter_seam(
        surface,
        fixed_axis,
        fixed,
        from.parameters[varying_axis],
        to.parameters[varying_axis],
        max_angle,
        0,
        &mut points,
        &mut budget,
    ) {
        return None;
    }
    // The rims may end a rounding apart across the seam. Leaning the seam
    // from one end to the other keeps it straight; holding it at the mean
    // value would notch the ring there and leave a sliver to triangulate.
    let (low, high) = (from.parameters[varying_axis], to.parameters[varying_axis]);
    for point in &mut points {
        let unit = if high != low {
            (point.parameters[varying_axis] - low) / (high - low)
        } else {
            0.0
        };
        let (start, end) = (from.parameters[fixed_axis], to.parameters[fixed_axis]);
        point.parameters[fixed_axis] = start + (end - start) * unit;
        point.position = surface.point_at(point.parameters[0], point.parameters[1]);
    }
    points.push(to.clone());
    Some(points)
}

fn sample_parameter_seam(
    surface: &super::geometry::Surface,
    fixed_axis: usize,
    fixed: f64,
    from: f64,
    to: f64,
    max_angle: f64,
    depth: u32,
    points: &mut Vec<BoundaryPoint>,
    budget: &mut usize,
) -> bool {
    let Some(left) = budget.checked_sub(1) else {
        return false;
    };
    *budget = left;
    let parameters = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0].map(|unit| {
        let mut parameters = [0.0; 2];
        parameters[fixed_axis] = fixed;
        parameters[1 - fixed_axis] = from + (to - from) * unit;
        parameters
    });
    if surface_path_angle(surface, &parameters)
        .map(|angle| angle_exceeds(angle, max_angle))
        .unwrap_or(true)
    {
        if depth >= MAX_DEPTH {
            return false;
        }
        let middle = 0.5 * (from + to);
        return sample_parameter_seam(
            surface,
            fixed_axis,
            fixed,
            from,
            middle,
            max_angle,
            depth + 1,
            points,
            budget,
        ) && sample_parameter_seam(
            surface,
            fixed_axis,
            fixed,
            middle,
            to,
            max_angle,
            depth + 1,
            points,
            budget,
        );
    }
    let mut parameters = [0.0; 2];
    parameters[fixed_axis] = fixed;
    parameters[1 - fixed_axis] = from;
    points.push(BoundaryPoint {
        parameters,
        position: surface.point_at(parameters[0], parameters[1]),
    });
    true
}

fn fit_periodic_band(
    surface: &super::geometry::Surface,
    rims: &mut [Vec<BoundaryPoint>],
    holes: &mut [Vec<BoundaryPoint>],
    varying: usize,
    period: f64,
) -> Option<()> {
    let first = rims.first()?;
    let choice = first.iter().find_map(|point| {
        let seam = point.parameters[varying];
        let centre = seam + period * 0.5;
        let shifts: Vec<f64> = holes
            .iter()
            .map(|hole| {
                let middle = average_parameter(hole, varying);
                period * ((centre - middle) / period).round()
            })
            .collect();
        let fits = holes.iter().zip(&shifts).all(|(hole, shift)| {
            let low = hole
                .iter()
                .map(|point| point.parameters[varying] + shift)
                .fold(f64::INFINITY, f64::min);
            let high = hole
                .iter()
                .map(|point| point.parameters[varying] + shift)
                .fold(f64::NEG_INFINITY, f64::max);
            let scale = seam.abs().max((seam + period).abs()).max(1.0);
            let epsilon = f64::EPSILON * 128.0 * scale;
            low > seam + epsilon && high < seam + period - epsilon
        });
        fits.then_some((seam, shifts))
    })?;
    for (hole, shift) in holes.iter_mut().zip(choice.1) {
        for point in hole {
            point.parameters[varying] += shift;
        }
    }
    for rim in rims {
        rotate_periodic_rim(surface, rim, varying, choice.0, period)?;
    }
    Some(())
}

fn rotate_periodic_rim(
    surface: &super::geometry::Surface,
    rim: &mut Vec<BoundaryPoint>,
    varying: usize,
    seam: f64,
    period: f64,
) -> Option<()> {
    let scale = seam.abs().max((seam + period).abs()).max(1.0);
    // As loose as `is_monotonic_periodic_rim`: a corner two curves place a
    // rounding apart is on the seam from either side.
    let epsilon = (f64::EPSILON * 128.0 * scale).max(period * 1e-9);
    // Rotated round rather than sorted: a rim may step straight across the
    // band at one value (a notch cut into a tube's end), and sorting by that
    // value would scramble or merge the step's two ends.
    if rim.last()?.parameters[varying] < rim.first()?.parameters[varying] {
        rim.reverse();
    }
    if rim.len() > 1 {
        rim.pop();
    }
    for point in rim.iter_mut() {
        let mut value = seam + (point.parameters[varying] - seam).rem_euclid(period);
        if (value - seam - period).abs() <= epsilon {
            value = seam;
        }
        point.parameters[varying] = value;
    }
    if let Some(wrap) = (1..rim.len()).find(|index| {
        rim[*index].parameters[varying] < rim[index - 1].parameters[varying] - period * 0.5
    }) {
        rim.rotate_left(wrap);
    }
    rim.dedup_by(|a, b| {
        (0..2).all(|axis| {
            let scale = a.parameters[axis].abs().max(b.parameters[axis].abs()).max(1.0);
            (a.parameters[axis] - b.parameters[axis]).abs() <= f64::EPSILON * 128.0 * scale
        })
    });
    // Both rims must start on the seam itself: one whose samples straddle
    // it would otherwise start a step past it, and the seam joining the two
    // rims ran slantwise across the face (#1538's torus).
    let (first, last) = (rim.first()?, rim.last()?);
    if first.parameters[varying] - seam > epsilon {
        let before = last.parameters[varying] - period;
        let t = (seam - before) / (first.parameters[varying] - before);
        let fixed = 1 - varying;
        let mut parameters = [0.0; 2];
        parameters[varying] = seam;
        parameters[fixed] =
            last.parameters[fixed] + (first.parameters[fixed] - last.parameters[fixed]) * t;
        rim.insert(
            0,
            BoundaryPoint {
                parameters,
                position: surface.point_at(parameters[0], parameters[1]),
            },
        );
    } else {
        // A corner a rounding past the seam is on it, and starts there
        // exactly: left where it lies, the rim also ends that rounding past
        // the other rim, and the two no longer span one turn (#2308's brace).
        rim[0].parameters[varying] = seam;
    }
    let mut closing = rim.first()?.clone();
    closing.parameters[varying] += period;
    rim.push(closing);
    Some(())
}

fn is_isoparametric_rim(
    surface: &super::geometry::Surface,
    points: &[BoundaryPoint],
    varying: usize,
    tolerance: f64,
) -> bool {
    let fixed = 1 - varying;
    let value = average_parameter(points, fixed);
    points.iter().all(|point| {
        let mut parameters = point.parameters;
        parameters[fixed] = value;
        distance3(
            point.position,
            surface.point_at(parameters[0], parameters[1]),
        ) <= tolerance
    })
}

fn surface_domain(surface: &super::geometry::Surface) -> Option<[[f64; 2]; 2]> {
    use super::geometry::Surface;
    match surface {
        Surface::Sphere(_) => Some([[0.0, TAU], [-FRAC_PI_2, FRAC_PI_2]]),
        Surface::Cone(surface) if surface.half_angle.tan().abs() > 1e-15 => {
            let apex = surface.radius / surface.half_angle.tan();
            Some([[0.0, TAU], [0.0_f64.min(apex), 0.0_f64.max(apex)]])
        }
        Surface::Torus(_) => Some([[0.0, TAU], [0.0, TAU]]),
        Surface::Nurbs(surface) | Surface::Offset(crate::OffsetSurface { approximation: surface, .. }) => {
            let ((u0, u1), (v0, v1)) = surface.domain();
            Some([[u0, u1], [v0, v1]])
        }
        _ => None,
    }
}

fn whole_surface_domain(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
) -> Option<[[f64; 2]; 2]> {
    let node = body.faces.get(face)?;
    let domain = surface_domain(surface)?;
    let valid_domain = domain
        .iter()
        .all(|span| span[0].is_finite() && span[1].is_finite() && span[1] > span[0]);
    if !valid_domain {
        return None;
    }
    let bounded_domain = surface.is_spline()
        && node.loops.as_slice().first().is_some_and(|loop_key| {
            let Some(ring) = body.loops.get(*loop_key) else {
                return false;
            };
            if node.loops.len() != 1 || ring.coedges.len() != 4 {
                return false;
            }
            let mut sides = [0_u8; 4];
            for coedge_key in &ring.coedges {
                let Some(curve) = body
                    .coedges
                    .get(*coedge_key)
                    .and_then(|coedge| coedge.pcurve.as_ref())
                else {
                    return false;
                };
                let Some(side) = curve.rectangle_side(domain) else {
                    return false;
                };
                sides[side] += 1;
            }
            sides == [1; 4]
        });
    if bounded_domain {
        return Some(domain);
    }
    // Every loop a slit — one edge walked there and back — encloses nothing,
    // so the face is the whole surface. A ball touching two tangent faces
    // carries one slit per contact (#1563).
    let seam_loop = matches!(
        surface,
        super::geometry::Surface::Sphere(_) | super::geometry::Surface::Torus(_)
    ) && !node.loops.is_empty()
        && node.loops.iter().all(|loop_key| {
            let Some(ring) = body.loops.get(*loop_key) else {
                return false;
            };
            !ring.coedges.is_empty()
                && ring.coedges.iter().all(|coedge_key| {
                    let Some(coedge) = body.coedges.get(*coedge_key) else {
                        return false;
                    };
                    if coedge.pcurve.is_some() {
                        // An explicit pcurve bounds a trimmed periodic patch; it
                        // is not merely an arbitrary cut across the whole surface.
                        return false;
                    }
                    let matching: Vec<_> = ring
                        .coedges
                        .iter()
                        .filter_map(|candidate| body.coedges.get(*candidate))
                        .filter(|candidate| candidate.edge == coedge.edge)
                        .collect();
                    matching.len() == 2 && matching[0].forward != matching[1].forward
                })
        });
    // A torus cut once round by one of its own circles (a closed edge along
    // a constant u or v) is not divided: the face is the whole torus.
    let cut_once = matches!(surface, super::geometry::Surface::Torus(_))
        && node.loops.len() == 1
        && body.loops.get(node.loops[0]).is_some_and(|ring| {
            let [coedge_key] = ring.coedges.as_slice() else {
                return false;
            };
            let Some(edge) = body.coedges.get(*coedge_key).and_then(|c| body.edges.get(c.edge)) else {
                return false;
            };
            let Some(curve) = body.curves.get(edge.curve) else {
                return false;
            };
            if edge.start != edge.end {
                return false;
            }
            let (t0, t1) = (edge.start_parameter, edge.end_parameter);
            let params: Option<Vec<(f64, f64)>> = (0..16)
                .map(|k| surface.parameters_at(curve.point_at(t0 + (t1 - t0) * k as f64 / 16.0)))
                .collect();
            let Some(params) = params else {
                return false;
            };
            let tau = std::f64::consts::TAU;
            let steady = |pick: fn(&(f64, f64)) -> f64| {
                let first = pick(&params[0]);
                params.iter().all(|p| {
                    let d = (pick(p) - first).rem_euclid(tau);
                    d.min(tau - d) < 1e-6
                })
            };
            steady(|p| p.0) || steady(|p| p.1)
        });
    if !node.loops.is_empty() && !seam_loop && !cut_once {
        return None;
    }
    let closed = match surface {
        super::geometry::Surface::Sphere(_) | super::geometry::Surface::Torus(_) => true,
        super::geometry::Surface::Nurbs(_) | super::geometry::Surface::Offset(_) => {
            surface.spline().is_some_and(|s| s.periodicity() == [true, true])
        }
        _ => false,
    };
    if !closed {
        return None;
    }
    Some(domain)
}

fn domain_ring(domain: [[f64; 2]; 2]) -> Vec<[f64; 2]> {
    vec![
        [domain[0][0], domain[1][0]],
        [domain[0][1], domain[1][0]],
        [domain[0][1], domain[1][1]],
        [domain[0][0], domain[1][1]],
    ]
}

fn boundary_collapsed(
    surface: &super::geometry::Surface,
    varying: usize,
    from: f64,
    period: f64,
    fixed: usize,
    value: f64,
    tolerance: f64,
) -> bool {
    let mut first = [0.0; 2];
    first[varying] = from;
    first[fixed] = value;
    let origin = surface.point_at(first[0], first[1]);
    (1..=4).all(|step| {
        let mut parameters = first;
        parameters[varying] = from + period * step as f64 / 4.0;
        distance3(origin, surface.point_at(parameters[0], parameters[1])) <= tolerance
    })
}

fn periods(surface: &super::geometry::Surface) -> [Option<f64>; 2] {
    use super::geometry::Surface;
    match surface {
        Surface::Plane(_) => [None, None],
        Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => [Some(TAU), None],
        Surface::Torus(_) => [Some(TAU), Some(TAU)],
        Surface::Nurbs(surface) | Surface::Offset(crate::OffsetSurface { approximation: surface, .. }) => {
            let ((u0, u1), (v0, v1)) = surface.domain();
            let periodic = surface.periodicity();
            [
                periodic[0].then_some(u1 - u0),
                periodic[1].then_some(v1 - v0),
            ]
        }
    }
}

fn unwound(value: f64, previous: f64, period: f64) -> f64 {
    value + period * ((previous - value) / period).round()
}

fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    Vec3::from(a).distance(Vec3::from(b))
}

/// Recursion guard; tolerance normally stops first.
const MAX_DEPTH: u32 = 32;
/// Most pieces one edge or parameter line may split into before its walk is
/// abandoned. Every walk splits both halves of a span that still turns too
/// far, so a span whose angle never settles (one that measures NaN, say)
/// would otherwise split until the depth cap: 2^32 calls, or 2^50 once the
/// floating-point spacing runs out under the face cap.
const PCURVE_REFINE_BUDGET: usize = 1 << 14;
const MAX_FACE_DEPTH: u32 = 128;
const MAX_FACE_PASSES: usize = 128;
const MAX_FACE_ADDITIONS: usize = 262_144;

/// Triangulates a whole body.
///
/// `max_angle` is the largest change of direction, in radians.
pub fn body(body: &Body, max_angle: f64, tolerance: f64) -> Mesh {
    tessellate(body, TessellationTolerance::new(max_angle, tolerance)).mesh
}

/// Triangulates one face.
///
/// `None` when its canonical edge schedule cannot be mapped to the surface.
pub fn face(body: &Body, face: FaceKey, max_angle: f64, tolerance: f64) -> Option<Mesh> {
    let schedules: HashMap<EdgeKey, Vec<super::place::EdgeSample>> = body
        .edge_keys()
        .filter_map(|edge| Some((edge, shared_edge_samples(body, edge, max_angle, tolerance)?)))
        .collect();
    scheduled_face(body, face, max_angle, tolerance, &schedules)
        .or_else(|| finer_face(body, face, max_angle, tolerance))
        .or_else(|| crossing_face(body, face, max_angle, tolerance, &schedules))
}

/// A face whose boundary, sampled as its neighbours need, crosses itself:
/// two of its curves run so close that their chords cross although the
/// curves do not (a spline beside the arc it leaves at a shallow angle).
/// Its own edges sampled finer then; the seam with the neighbours can open
/// by at most the tolerance, which a hole the size of the face cannot beat.
///
/// The 0.01 retry samples a hundred times as finely, and filling the face costs far more than
/// linearly in its samples (a face of about 10 000 edge samples took 12-18 s to fail again), so
/// it is only tried while it stays small: see [`finest_retry_affordable`].
fn finer_face(body: &Body, face: FaceKey, max_angle: f64, tolerance: f64) -> Option<Mesh> {
    let node = body.faces.get(face)?;
    for factor in [0.1, 0.01] {
        let schedules: HashMap<EdgeKey, Vec<super::place::EdgeSample>> = node
            .loops
            .iter()
            .filter_map(|ring| body.loops.get(*ring))
            .flat_map(|ring| ring.coedges.iter())
            .filter_map(|coedge| body.coedges.get(*coedge))
            .filter_map(|coedge| {
                Some((
                    coedge.edge,
                    shared_edge_samples(body, coedge.edge, max_angle * factor, tolerance * factor)?,
                ))
            })
            .collect();
        if let Some(mesh) = scheduled_face(body, face, max_angle, tolerance, &schedules) {
            return Some(mesh);
        }
        let points: usize = schedules.values().map(Vec::len).sum();
        if !finest_retry_affordable(points) {
            return None;
        }
    }
    None
}

/// The most edge samples the finest retry of [`finer_face`] may use on one face.
const FINEST_RETRY_MAX_POINTS: usize = 4_000;

/// Whether the 0.01 retry may follow a failed 0.1 one that sampled `points`: sample counts grow
/// about tenfold per step, so it is predicted from them rather than paid for first.
fn finest_retry_affordable(points: usize) -> bool {
    points.saturating_mul(10) <= FINEST_RETRY_MAX_POINTS
}

fn fill_scheduled_band(
    body: &Body,
    face: FaceKey,
    band: &BoundaryBand,
    max_angle: f64,
    tolerance: f64,
) -> Option<Mesh> {
    let mut pins = band.low.clone();
    pins.extend(band.high.iter().cloned());
    let mut boundary_segments: Vec<[[f64; 2]; 2]> = band
        .low
        .windows(2)
        .chain(band.high.windows(2))
        .map(|pair| [pair[0].parameters, pair[1].parameters])
        .collect();
    if !band.strip {
        boundary_segments.extend([
            [band.low.first()?.parameters, band.high.first()?.parameters],
            [band.low.last()?.parameters, band.high.last()?.parameters],
        ]);
    }
    let mut lower = 0;
    let mut upper = 0;
    let mut triangles = Vec::new();
    let fixed_axis = 1 - band.varying;
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    let fixed_values = if band.structured {
        let fixed_from = band.low.first()?.parameters[fixed_axis];
        let fixed_to = band.high.first()?.parameters[fixed_axis];
        let mut varying_values: Vec<f64> = band
            .low
            .iter()
            .chain(&band.high)
            .map(|point| point.parameters[band.varying])
            .collect();
        varying_values.sort_by(f64::total_cmp);
        varying_values.dedup_by(|a, b| parameter_value_near(*a, *b));
        let mut probes = varying_values.clone();
        probes.extend(
            varying_values
                .windows(2)
                .map(|pair| 0.5 * (pair[0] + pair[1])),
        );
        let mut values = Vec::new();
        let mut budget = MAX_FACE_ADDITIONS;
        for varying in probes {
            surface_span_breaks(
                surface,
                fixed_axis,
                varying,
                fixed_from,
                fixed_to,
                max_angle,
                0,
                &mut values,
                &mut budget,
            )?;
        }
        values.push(fixed_to);
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| parameter_value_near(*a, *b));
        values
    } else {
        Vec::new()
    };
    let base_triangles = band
        .low
        .len()
        .saturating_add(band.high.len())
        .saturating_sub(2);
    let band_triangles = fixed_values
        .len()
        .saturating_sub(1)
        .saturating_mul(base_triangles)
        .saturating_mul(2);
    if band_triangles > MAX_FACE_ADDITIONS || face_limit_exceeded(band_triangles) {
        return None;
    }
    while lower + 1 < band.low.len() || upper + 1 < band.high.len() {
        let take_lower = upper + 1 >= band.high.len()
            || (lower + 1 < band.low.len()
                && band.low[lower + 1].parameters[band.varying]
                    <= band.high[upper + 1].parameters[band.varying]);
        let mut corners = if take_lower {
            let corners = [
                band.low[lower].parameters,
                band.low[lower + 1].parameters,
                band.high[upper].parameters,
            ];
            lower += 1;
            corners
        } else {
            let corners = [
                band.low[lower].parameters,
                band.high[upper + 1].parameters,
                band.high[upper].parameters,
            ];
            upper += 1;
            corners
        };
        if band.varying == 1 {
            corners.swap(1, 2);
        }
        if band.structured {
            triangles.extend(subdivide_band_triangle(corners, fixed_axis, &fixed_values)?);
        } else {
            triangles.push(corners);
        }
    }
    let mut mesh = Mesh::default();
    for corners in triangles {
        if !refine_scheduled(
            &mut mesh,
            body,
            face,
            corners,
            max_angle,
            0,
            &pins,
            &boundary_segments,
            tolerance,
            None,
        ) {
            return None;
        }
    }
    (!mesh.triangles.is_empty()).then_some(mesh)
}

fn surface_span_breaks(
    surface: &super::geometry::Surface,
    fixed_axis: usize,
    varying: f64,
    from: f64,
    to: f64,
    max_angle: f64,
    depth: u32,
    values: &mut Vec<f64>,
    budget: &mut usize,
) -> Option<()> {
    *budget = budget.checked_sub(1)?;
    let parameters = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0].map(|unit| {
        let mut parameters = [0.0; 2];
        parameters[fixed_axis] = from + (to - from) * unit;
        parameters[1 - fixed_axis] = varying;
        parameters
    });
    // The parameter line's own turning counts as well as the normal's: a
    // flat spline patch whose isolines curve (a planar face swept along an
    // arc) has a constant normal, and would otherwise be cut by chords.
    if !angle_exceeds(surface_path_angle(surface, &parameters)?, max_angle) {
        values.push(from);
        return Some(());
    }
    // Across a fold the normal turns at any scale, and every probe line that
    // crosses it would pile breaks up there, each one a grid line through
    // the whole face. A span the normal reverses on between two neighbouring
    // samples, or where the parametrisation folds, is kept while it stays
    // within the deflection; a smooth span turns gradually and is split.
    let reverses = || {
        let normals: Option<Vec<[f64; 3]>> = parameters
            .iter()
            .map(|uv| surface_display_normal(surface, *uv))
            .collect();
        normals.is_some_and(|normals| {
            normals
                .windows(2)
                .any(|pair| ocs_geom::tessellation::max_direction_angle(pair) > NORMAL_REVERSAL)
        })
    };
    if (reverses() || parametrization_folds(surface, &parameters))
        && span_deviation(surface, &parameters) <= fold_deviation(-1.0)
    {
        values.push(from);
        return Some(());
    }
    if depth >= MAX_FACE_DEPTH {
        return None;
    }
    let middle = 0.5 * (from + to);
    surface_span_breaks(
        surface,
        fixed_axis,
        varying,
        from,
        middle,
        max_angle,
        depth + 1,
        values,
        budget,
    )?;
    surface_span_breaks(
        surface,
        fixed_axis,
        varying,
        middle,
        to,
        max_angle,
        depth + 1,
        values,
        budget,
    )
}

fn surface_span_depth(
    surface: &super::geometry::Surface,
    fixed_axis: usize,
    varying: f64,
    from: f64,
    to: f64,
    max_angle: f64,
    depth: u32,
    budget: &mut usize,
) -> Option<u32> {
    *budget = budget.checked_sub(1)?;
    let parameters = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0].map(|unit| {
        let mut parameters = [0.0; 2];
        parameters[fixed_axis] = from + (to - from) * unit;
        parameters[1 - fixed_axis] = varying;
        parameters
    });
    if !angle_exceeds(surface_normal_angle(surface, &parameters)?, max_angle) {
        return Some(depth);
    }
    if depth >= MAX_FACE_DEPTH {
        return None;
    }
    let middle = 0.5 * (from + to);
    Some(
        surface_span_depth(
            surface,
            fixed_axis,
            varying,
            from,
            middle,
            max_angle,
            depth + 1,
            budget,
        )?
        .max(surface_span_depth(
            surface,
            fixed_axis,
            varying,
            middle,
            to,
            max_angle,
            depth + 1,
            budget,
        )?),
    )
}

fn subdivide_band_triangle(
    corners: [[f64; 2]; 3],
    fixed_axis: usize,
    fixed_values: &[f64],
) -> Option<Vec<[[f64; 2]; 3]>> {
    if fixed_values.len() <= 2 {
        return Some(vec![corners]);
    }
    let lone = if parameter_value_near(corners[1][fixed_axis], corners[2][fixed_axis]) {
        0
    } else if parameter_value_near(corners[2][fixed_axis], corners[0][fixed_axis]) {
        1
    } else if parameter_value_near(corners[0][fixed_axis], corners[1][fixed_axis]) {
        2
    } else {
        return None;
    };
    let corners = [
        corners[lone],
        corners[(lone + 1) % 3],
        corners[(lone + 2) % 3],
    ];
    let from = corners[0][fixed_axis];
    let to = corners[1][fixed_axis];
    if parameter_value_near(from, to) {
        return None;
    }
    let mut units: Vec<f64> = fixed_values
        .iter()
        .map(|value| ((value - from) / (to - from)).clamp(0.0, 1.0))
        .collect();
    units.sort_by(f64::total_cmp);
    units.dedup_by(|a, b| parameter_value_near(*a, *b));
    if units
        .first()
        .is_none_or(|unit| !parameter_value_near(*unit, 0.0))
        || units
            .last()
            .is_none_or(|unit| !parameter_value_near(*unit, 1.0))
    {
        return None;
    }
    let at = |to: [f64; 2], unit: f64| {
        [
            corners[0][0] + (to[0] - corners[0][0]) * unit,
            corners[0][1] + (to[1] - corners[0][1]) * unit,
        ]
    };
    let mut triangles = Vec::with_capacity(units.len().saturating_mul(2).saturating_sub(3));
    let mut left = corners[0];
    let mut right = corners[0];
    for (step, unit) in units.into_iter().skip(1).enumerate() {
        let next_left = at(corners[1], unit);
        let next_right = at(corners[2], unit);
        if step == 0 {
            triangles.push([corners[0], next_left, next_right]);
        } else {
            triangles.push([left, next_left, next_right]);
            triangles.push([left, next_right, right]);
        }
        left = next_left;
        right = next_right;
    }
    Some(triangles)
}

fn parameter_value_near(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * 128.0 * a.abs().max(b.abs()).max(1.0)
}

/// Whether the longer rim of a band never steps back along the varying
/// parameter — what filling it column by column towards a singular rim needs.
fn rim_runs_one_way(band: &BoundaryBand) -> bool {
    let rim = if band.low.len() > band.high.len() { &band.low } else { &band.high };
    let scale = rim
        .iter()
        .map(|point| point.parameters[band.varying].abs())
        .fold(1.0, f64::max);
    let epsilon = f64::EPSILON * 128.0 * scale;
    let step = |pair: &[BoundaryPoint]| {
        pair[1].parameters[band.varying] - pair[0].parameters[band.varying]
    };
    rim.windows(2).all(|pair| step(pair) >= -epsilon)
        || rim.windows(2).all(|pair| step(pair) <= epsilon)
}

fn fill_scheduled_singular_cap(
    body: &Body,
    face: FaceKey,
    band: &BoundaryBand,
    max_angle: f64,
) -> Option<Mesh> {
    let (rim, singular) = if band.low.len() > band.high.len() {
        (&band.low, &band.high)
    } else {
        (&band.high, &band.low)
    };
    let apex = singular.first()?;
    let fixed = 1 - band.varying;
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    let mut budget = MAX_FACE_ADDITIONS;
    let mut radial_depth = rim.iter().try_fold(0, |depth, point| {
        Some(depth.max(surface_span_depth(
            surface,
            fixed,
            point.parameters[band.varying],
            apex.parameters[fixed],
            point.parameters[fixed],
            max_angle,
            0,
            &mut budget,
        )?))
    })?;
    let triangles = loop {
        let steps = 1usize.checked_shl(radial_depth)?;
        let cap_triangles = steps
            .saturating_mul(rim.len().saturating_sub(1))
            .saturating_mul(2);
        if cap_triangles > MAX_FACE_ADDITIONS || face_limit_exceeded(cap_triangles) {
            return None;
        }
        let triangles = singular_cap_triangles(rim, apex, band.varying, steps);
        if triangles.iter().all(|corners| {
            triangle_within_angle(surface, *corners, max_angle, fixed, apex.parameters[fixed])
        }) {
            break triangles;
        }
        if radial_depth >= MAX_FACE_DEPTH {
            return None;
        }
        radial_depth += 1;
    };
    let mut pins = rim.clone();
    pins.extend(rim.iter().map(|point| {
        let mut singular = apex.clone();
        singular.parameters[band.varying] = point.parameters[band.varying];
        singular
    }));
    let mut mesh = Mesh::default();
    for corners in triangles {
        emit_scheduled(&mut mesh, body, face, corners, &pins);
    }
    (!mesh.triangles.is_empty()).then_some(mesh)
}

fn singular_cap_triangles(
    rim: &[BoundaryPoint],
    apex: &BoundaryPoint,
    varying: usize,
    steps: usize,
) -> Vec<[[f64; 2]; 3]> {
    let fixed = 1 - varying;
    let mut triangles = Vec::with_capacity(
        rim.len()
            .saturating_sub(1)
            .saturating_mul(steps.saturating_mul(2).saturating_sub(1)),
    );
    for pair in rim.windows(2) {
        let mut previous = [apex.parameters; 2];
        previous[0][varying] = pair[0].parameters[varying];
        previous[1][varying] = pair[1].parameters[varying];
        for step in 1..=steps {
            let unit = step as f64 / steps as f64;
            let mut current = [pair[0].parameters, pair[1].parameters];
            for (index, point) in current.iter_mut().enumerate() {
                point[fixed] = apex.parameters[fixed]
                    + (pair[index].parameters[fixed] - apex.parameters[fixed]) * unit;
            }
            let rim_fixed = 0.5 * (pair[0].parameters[fixed] + pair[1].parameters[fixed]);
            if apex.parameters[fixed] < rim_fixed {
                triangles.push([current[1], current[0], previous[0]]);
                if step > 1 {
                    triangles.push([current[1], previous[0], previous[1]]);
                }
            } else {
                triangles.push([current[0], current[1], previous[1]]);
                if step > 1 {
                    triangles.push([current[0], previous[1], previous[0]]);
                }
            }
            previous = current;
        }
    }
    triangles
}

fn triangle_within_angle(
    surface: &super::geometry::Surface,
    corners: [[f64; 2]; 3],
    max_angle: f64,
    _singular_axis: usize,
    _singular_value: f64,
) -> bool {
    if surface_triangle_angle(surface, corners).is_none_or(|angle| angle_exceeds(angle, max_angle))
    {
        return false;
    }
    [[0, 1], [1, 2], [2, 0]].into_iter().all(|[from, to]| {
        let parameters = [0.0, 0.25, 0.5, 0.75, 1.0].map(|unit| {
            [
                corners[from][0] + (corners[to][0] - corners[from][0]) * unit,
                corners[from][1] + (corners[to][1] - corners[from][1]) * unit,
            ]
        });
        let angle = surface_normal_angle(surface, &parameters);
        angle.is_some_and(|angle| !angle_exceeds(angle, max_angle))
    })
}

fn fill_whole_surface(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    domain: [[f64; 2]; 2],
    max_angle: f64,
    tolerance: f64,
) -> Option<Mesh> {
    let analytic = matches!(
        surface,
        super::geometry::Surface::Sphere(_) | super::geometry::Surface::Torus(_)
    );
    let step = (max_angle / std::f64::consts::SQRT_2).max(f64::EPSILON);
    let u_cells = if analytic {
        ((domain[0][1] - domain[0][0]) / step).ceil() as usize
    } else {
        4
    };
    let v_cells = if analytic {
        ((domain[1][1] - domain[1][0]) / step).ceil() as usize
    } else if matches!(surface, super::geometry::Surface::Sphere(_)) {
        2
    } else {
        4
    };
    let cells = u_cells.saturating_mul(v_cells).saturating_mul(2);
    if cells > MAX_FACE_ADDITIONS || face_limit_exceeded(cells) {
        return None;
    }
    // A square parameter grid needlessly refines the nearly straight direction
    // of anisotropic NURBS patches whenever the curved direction is split.
    // Seed each direction from its own normal variation, then retain the usual
    // triangle-level verification below for variation between the probes.
    let uniform_grid = || {
        [
            (0..=u_cells)
                .map(|index| {
                    domain[0][0] + (domain[0][1] - domain[0][0]) * index as f64 / u_cells as f64
                })
                .collect(),
            (0..=v_cells)
                .map(|index| {
                    domain[1][0] + (domain[1][1] - domain[1][0]) * index as f64 / v_cells as f64
                })
                .collect(),
        ]
    };
    let [u_values, v_values] = if analytic {
        uniform_grid()
    } else {
        surface_grid_values(surface, domain, max_angle)
            .filter(|[u, v]| {
                u.len()
                    .saturating_sub(1)
                    .saturating_mul(v.len().saturating_sub(1))
                    .saturating_mul(2)
                    <= MAX_FACE_ADDITIONS
            })
            // Singular normals can prevent directional seeding; preserve the
            // existing recursive path for those patches.
            .unwrap_or_else(uniform_grid)
    };
    let mut mesh = Mesh::default();
    for v_span in v_values.windows(2) {
        let [v0, v1] = [v_span[0], v_span[1]];
        for u_span in u_values.windows(2) {
            let [u0, u1] = [u_span[0], u_span[1]];
            for corners in [
                [[u0, v0], [u1, v0], [u0, v1]],
                [[u1, v0], [u1, v1], [u0, v1]],
            ] {
                if analytic {
                    emit_scheduled(&mut mesh, body, face, corners, &[]);
                    continue;
                }
                if !refine_scheduled(
                    &mut mesh,
                    body,
                    face,
                    corners,
                    max_angle,
                    0,
                    &[],
                    &[],
                    tolerance,
                    None,
                ) {
                    return None;
                }
            }
        }
    }
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// Triangulates a face while preserving all boundary rings as constraints.
fn fill_scheduled(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    rings: &[Vec<BoundaryPoint>],
    max_angle: f64,
    tolerance: f64,
) -> Option<Mesh> {
    let parameters: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|ring| ring.iter().map(|point| point.parameters).collect())
        .collect();
    let Some(mut domain) = ConstrainedMesh::new(&parameters) else {
        return fill_crossing_planar_ring(body, face, surface, rings, max_angle, tolerance);
    };
    let additions = seed_surface_grid(&mut domain, surface, &parameters, max_angle)?;
    let pins: Vec<BoundaryPoint> = rings.iter().flatten().cloned().collect();
    let flat = matches!(surface, super::geometry::Surface::Plane(_));
    let mut additions = additions;
    let mut complete = None;
    let mut normal_cache = ParameterMap::default();
    for _ in 0..MAX_FACE_PASSES {
        let triangles = domain.triangles();
        if triangles.is_empty() {
            return None;
        }
        if flat {
            complete = Some(triangles);
            break;
        }
        let mut candidates = Vec::new();
        let mut candidates_within_tolerance = true;
        for triangle in &triangles {
            let refinement =
                triangle_refinement(surface, triangle, max_angle, tolerance, &mut normal_cache)?;
            match refinement {
                TriangleRefinement::Complete => {}
                TriangleRefinement::Boundary => {
                    return None;
                }
                TriangleRefinement::Interior(parameters) => {
                    candidates_within_tolerance &=
                        triangle_within_tolerance(surface, triangle.parameters, tolerance);
                    candidates.push((parameters, triangle.parameters));
                }
            }
        }
        if candidates.is_empty() {
            complete = Some(triangles);
            break;
        }
        let mut unique = std::collections::HashSet::with_capacity(candidates.len());
        candidates.retain(|(parameters, _)| unique.insert(parameters.map(f64::to_bits)));
        let mut inserted = 0;
        for (parameters, corners) in candidates {
            // Each added point adds about two triangles.
            if additions >= MAX_FACE_ADDITIONS || face_limit_exceeded(additions.saturating_mul(2)) {
                return None;
            }
            match domain.insert(parameters) {
                Some(true) => {
                    additions += 1;
                    inserted += 1;
                }
                Some(false) => {
                    let centre = [
                        (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
                        (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
                    ];
                    let mut added = !parameter_near(centre, parameters) && domain.insert(centre)?;
                    for weights in [[0.2, 0.3, 0.5], [0.2, 0.5, 0.3], [0.5, 0.2, 0.3]] {
                        if added {
                            break;
                        }
                        let off_centre = [0, 1].map(|axis| {
                            corners[0][axis] * weights[0]
                                + corners[1][axis] * weights[1]
                                + corners[2][axis] * weights[2]
                        });
                        added = domain.insert(off_centre)?;
                    }
                    if added {
                        additions += 1;
                        inserted += 1;
                    }
                }
                None => return None,
            }
        }
        if inserted == 0 {
            if candidates_within_tolerance {
                complete = Some(triangles);
                break;
            }
            return None;
        }
    }
    let triangles = complete?;

    let mut mesh = Mesh::default();
    // The boundary's own points first, so a corner on the boundary is found
    // by its key rather than by searching every pin: a face with a hundred
    // holes has thousands. The first pin at a parameter wins, as the search.
    let mut point_cache: ParameterMap<[f64; 3]> = ParameterMap::default();
    for pin in pins.iter().rev() {
        point_cache.insert(parameter_key(pin.parameters), pin.position);
    }
    for triangle in triangles {
        if parameter_triangle_degenerate(triangle.parameters) {
            continue;
        }
        emit_scheduled_cached(
            &mut mesh,
            body,
            face,
            triangle.parameters,
            &pins,
            &normal_cache,
            &mut point_cache,
        );
    }
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// A planar face whose only loop meets itself once: two of its edges cross, as
/// in a "bow-tie" quadrilateral, or one of its corners lies on an edge it does
/// not bound (to rounding, often just across it). The constrained triangulation
/// refuses such a boundary, so the face would be missing. Its region is the
/// even-odd fill of the loop, which is exactly the two simple loops the
/// boundary splits into at that contact. Each is meshed on its own, with the
/// loop's own points (plus the crossing point), so nothing is drawn outside the
/// boundary.
///
/// Reached only after the triangulation has refused the face. Anything else is
/// left as before: a curved surface, more than one loop, a hairline face, a
/// loop that runs along itself, more than one contact, or two pieces one
/// inside the other (where the even-odd fill is not their union).
fn fill_crossing_planar_ring(
    body: &Body,
    face: FaceKey,
    surface: &super::geometry::Surface,
    rings: &[Vec<BoundaryPoint>],
    max_angle: f64,
    tolerance: f64,
) -> Option<Mesh> {
    // A loop whose lobes cancel to a sliver is a hairline face, which the mesh
    // leaves out on purpose (`hairline_face`); it stays so.
    if !matches!(surface, super::geometry::Surface::Plane(_))
        || rings.len() != 1
        || hairline_face(body, face)
    {
        return None;
    }
    let pieces = split_at_crossings(surface, &rings[0])?;
    let mut mesh = Mesh::default();
    for piece in pieces {
        mesh.absorb(fill_scheduled(
            body,
            face,
            surface,
            std::slice::from_ref(&piece),
            max_angle,
            tolerance,
        )?);
    }
    (!mesh.triangles.is_empty()).then_some(mesh)
}

/// The two simple loops a loop splits into at its one self-contact: a corner
/// lying on a segment it does not bound (split at that corner, no new point),
/// or else one proper crossing (split at the crossing point). `None` when there
/// is no such contact, more than one (their pieces can border an even-odd
/// hole), a degenerate piece, or one piece inside the other (then the inner one
/// is a hole of the even-odd fill, not part of it).
fn split_at_crossings(
    surface: &super::geometry::Surface,
    ring: &[BoundaryPoint],
) -> Option<Vec<Vec<BoundaryPoint>>> {
    let mut ring = ring.to_vec();
    if ring.len() > 1 && same_corner(ring[0].parameters, ring[ring.len() - 1].parameters) {
        ring.pop();
    }
    let n = ring.len();
    let eps = 1e-8 * ring_size(&ring);
    let (touching, crossing) = (touches(&ring, eps), crossings(&ring, eps));
    let (inner, outer) = if let ([(i, k)], []) = (touching.as_slice(), crossing.as_slice()) {
        let (i, k) = (*i, *k);
        // A corner on a segment it does not bound pinches the loop there: with
        // that segment rotated to the end, the loop is the corner's two sides.
        let rotated: Vec<BoundaryPoint> = ring[i + 1..].iter().chain(&ring[..=i]).cloned().collect();
        let k = (k + n - (i + 1)) % n;
        (rotated[..=k].to_vec(), rotated[k..].to_vec())
    } else if let ([], [(i, j, at)]) = (touching.as_slice(), crossing.as_slice()) {
        let (i, j, at) = (*i, *j, *at);
        let crossing = BoundaryPoint {
            parameters: at,
            position: surface.point_at(at[0], at[1]),
        };
        let mut inner = vec![crossing.clone()];
        inner.extend_from_slice(&ring[i + 1..=j]);
        let mut outer = ring[..=i].to_vec();
        outer.push(crossing);
        outer.extend_from_slice(&ring[j + 1..]);
        (inner, outer)
    } else {
        return None;
    };
    let simple = |piece: &[BoundaryPoint]| {
        let eps = 1e-8 * ring_size(piece);
        piece.len() >= 3 && crossings(piece, eps).is_empty() && touches(piece, eps).is_empty()
    };
    // A two-point piece is a needle: the loop running out along the segment the
    // corner touches and straight back. It has no area, so the face is the
    // other piece alone.
    let pieces: Vec<Vec<BoundaryPoint>> = [inner, outer].into_iter().filter(|piece| piece.len() >= 3).collect();
    if pieces.is_empty() || !pieces.iter().all(|piece| simple(piece)) {
        return None;
    }
    let nested = pieces.iter().enumerate().any(|(a, piece)| {
        pieces.iter().enumerate().any(|(b, other)| {
            a != b
                && piece.iter().any(|point| {
                    !other
                        .iter()
                        .any(|corner| same_corner(corner.parameters, point.parameters))
                        && inside_ring(other, point.parameters)
                })
        })
    });
    (!nested).then_some(pieces)
}

/// Every pair of non-adjacent segments of a closed loop that cross properly
/// (each strictly separates the other's ends), with where they cross. A pair
/// with an end within `eps` of the other segment is a corner touching it, not
/// a crossing, and is left to [`touches`].
fn crossings(ring: &[BoundaryPoint], eps: f64) -> Vec<(usize, usize, [f64; 2])> {
    let n = ring.len();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut out = Vec::new();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b) = (ring[i].parameters, ring[i + 1].parameters);
            let (c, d) = (ring[j].parameters, ring[(j + 1) % n].parameters);
            if [(a, c, d), (b, c, d), (c, a, b), (d, a, b)]
                .iter()
                .any(|(p, s, e)| distance_to_segment(*p, *s, *e) <= eps)
            {
                continue;
            }
            let (o1, o2) = (cross(a, b, c), cross(a, b, d));
            let (o3, o4) = (cross(c, d, a), cross(c, d, b));
            if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
                let t = o3 / (o3 - o4);
                out.push((i, j, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]));
            }
        }
    }
    out
}

/// Every segment `i` and corner `k` that does not bound it but lies on it,
/// within `eps` and away from its ends.
fn touches(ring: &[BoundaryPoint], eps: f64) -> Vec<(usize, usize)> {
    let n = ring.len();
    let distance = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (ring[i].parameters, ring[(i + 1) % n].parameters);
        for (k, corner) in ring.iter().enumerate() {
            if k == i || k == (i + 1) % n {
                continue;
            }
            let p = corner.parameters;
            if distance(p, a) > eps && distance(p, b) > eps && distance_to_segment(p, a, b) <= eps {
                out.push((i, k));
            }
        }
    }
    out
}

fn distance_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if length2 > 0.0 {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - ab[0] * t).hypot(p[1] - a[1] - ab[1] * t)
}

/// The diagonal of a loop's parameter box.
fn ring_size(ring: &[BoundaryPoint]) -> f64 {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for point in ring {
        for axis in 0..2 {
            low[axis] = low[axis].min(point.parameters[axis]);
            high[axis] = high[axis].max(point.parameters[axis]);
        }
    }
    (high[0] - low[0]).hypot(high[1] - low[1])
}

/// Whether `point` lies inside a closed loop (even-odd rule).
fn inside_ring(ring: &[BoundaryPoint], point: [f64; 2]) -> bool {
    let mut inside = false;
    for k in 0..ring.len() {
        let (a, b) = (ring[k].parameters, ring[(k + 1) % ring.len()].parameters);
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let x = a[0] + (point[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if point[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn surface_grid_values(
    surface: &super::geometry::Surface,
    bounds: [[f64; 2]; 2],
    max_angle: f64,
) -> Option<[Vec<f64>; 2]> {
    let nurbs = surface.spline();
    let values = [0, 1].map(|axis| {
        let other = 1 - axis;
        // An analytic surface has no knots to probe at, and its bounds and
        // their middle can all sit where the normal stands still along the
        // axis (a torus's top and bottom circles), leaving the grid with no
        // lines at all (#1538). Eighths across the other axis cannot.
        let mut probes = nurbs.map_or_else(
            || {
                (1..8)
                    .map(|step| {
                        bounds[other][0] + (bounds[other][1] - bounds[other][0]) * step as f64 / 8.0
                    })
                    .collect()
            },
            |nurbs| {
                let knots = nurbs.knots();
                if other == 0 {
                    knots.0.to_vec()
                } else {
                    knots.1.to_vec()
                }
            },
        );
        probes.retain(|value| *value >= bounds[other][0] && *value <= bounds[other][1]);
        probes.extend(bounds[other]);
        probes.sort_by(f64::total_cmp);
        probes.dedup_by(|a, b| parameter_value_near(*a, *b));
        probes.extend(
            probes
                .windows(2)
                .map(|pair| 0.5 * (pair[0] + pair[1]))
                .collect::<Vec<_>>(),
        );
        let mut values = Vec::new();
        let axis_angle = match surface {
            super::geometry::Surface::Cylinder(_) | super::geometry::Surface::Cone(_)
                if axis == 0 =>
            {
                max_angle
            }
            _ => max_angle * FRAC_1_SQRT_2,
        };
        let mut budget = MAX_FACE_ADDITIONS;
        for probe in probes {
            surface_span_breaks(
                surface,
                axis,
                probe,
                bounds[axis][0],
                bounds[axis][1],
                axis_angle,
                0,
                &mut values,
                &mut budget,
            )?;
        }
        if let Some(nurbs) = nurbs {
            let knots = nurbs.knots();
            let axis_knots = if axis == 0 { knots.0 } else { knots.1 };
            let ((u0, u1), (v0, v1)) = nurbs.domain();
            let axis_domain = if axis == 0 { (u0, u1) } else { (v0, v1) };
            let period = nurbs.periodicity()[axis].then_some(axis_domain.1 - axis_domain.0);
            for knot in axis_knots {
                for turn in if period.is_some() { -2..=2 } else { 0..=0 } {
                    let value = *knot + f64::from(turn) * period.unwrap_or(0.0);
                    if value >= bounds[axis][0] && value <= bounds[axis][1] {
                        values.push(value);
                    }
                }
            }
        }
        values.push(bounds[axis][1]);
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| parameter_value_near(*a, *b));
        Some(values)
    });
    let [Some(u_values), Some(v_values)] = values else {
        return None;
    };
    Some([u_values, v_values])
}

fn seed_surface_grid(
    domain: &mut ConstrainedMesh,
    surface: &super::geometry::Surface,
    rings: &[Vec<[f64; 2]>],
    max_angle: f64,
) -> Option<usize> {
    if matches!(surface, super::geometry::Surface::Plane(_)) {
        return Some(0);
    }
    let bounds = parameter_bounds(rings)?;
    let Some([u_values, v_values]) = surface_grid_values(surface, bounds, max_angle) else {
        return Some(0);
    };
    if u_values.len().saturating_mul(v_values.len()) > MAX_FACE_ADDITIONS {
        return None;
    }
    let mut inserted = 0;
    for (fixed_axis, values) in [u_values, v_values].into_iter().enumerate() {
        // A knot a rounding away from the boundary (a seam the rims put at
        // 0.4999999999 beside a knot at 0.5) would only cut slivers.
        let [low, high] = bounds[fixed_axis];
        let near = (high - low) * 1e-6;
        for fixed in values.iter().skip(1).take(values.len().saturating_sub(2)) {
            if *fixed - low <= near || high - *fixed <= near {
                continue;
            }
            for interval in line_intervals(rings, fixed_axis, *fixed) {
                let span = interval[1] - interval[0];
                let inset = span * 1e-9;
                if !span.is_finite() || span <= 0.0 || inset <= 0.0 {
                    continue;
                }
                let mut from = [0.0; 2];
                let mut to = [0.0; 2];
                from[fixed_axis] = *fixed;
                to[fixed_axis] = *fixed;
                from[1 - fixed_axis] = interval[0] + inset;
                to[1 - fixed_axis] = interval[1] - inset;
                if from[1 - fixed_axis] >= to[1 - fixed_axis] {
                    continue;
                }
                inserted += domain.constrain(from, to)?;
                if inserted > MAX_FACE_ADDITIONS {
                    return None;
                }
            }
        }
    }
    Some(inserted)
}

enum TriangleRefinement {
    Complete,
    Boundary,
    Interior([f64; 2]),
}

fn parameter_triangle_degenerate(corners: [[f64; 2]; 3]) -> bool {
    let ranges = [0, 1].map(|axis| {
        let low = corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min);
        let high = corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        high - low
    });
    let coordinate_scale = corners
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if ranges
        .into_iter()
        .any(|range| range <= f64::EPSILON * 128.0 * coordinate_scale)
    {
        return true;
    }
    let ab = [corners[1][0] - corners[0][0], corners[1][1] - corners[0][1]];
    let ac = [corners[2][0] - corners[0][0], corners[2][1] - corners[0][1]];
    let scale = ab.into_iter().chain(ac).map(f64::abs).fold(0.0, f64::max);
    (ab[0] * ac[1] - ab[1] * ac[0]).abs() <= f64::EPSILON * 128.0 * scale * scale
}

/// The largest distance between the surface along a straight parameter chord
/// and the straight chord in space between its ends.
fn chord_deviation(surface: &super::geometry::Surface, ends: [[f64; 2]; 2]) -> f64 {
    let [from, to] = ends.map(|uv| surface.point_at(uv[0], uv[1]));
    [0.25, 0.5, 0.75]
        .into_iter()
        .map(|unit| {
            let on_surface = surface.point_at(
                ends[0][0] + (ends[1][0] - ends[0][0]) * unit,
                ends[0][1] + (ends[1][1] - ends[0][1]) * unit,
            );
            let on_chord = [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * unit);
            distance3(on_surface, on_chord)
        })
        .fold(0.0, f64::max)
}

/// The largest distance between the surface over a parameter triangle and
/// the flat triangle in space between its corners.
fn flat_triangle_deviation(surface: &super::geometry::Surface, corners: [[f64; 2]; 3]) -> f64 {
    let positions = corners.map(|uv| surface.point_at(uv[0], uv[1]));
    [
        [0.5, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [1.0 / 3.0; 3],
        [0.5, 0.25, 0.25],
        [0.25, 0.5, 0.25],
        [0.25, 0.25, 0.5],
    ]
    .into_iter()
    .map(|weights| {
        let at = |values: [f64; 3]| {
            values[0] * weights[0] + values[1] * weights[1] + values[2] * weights[2]
        };
        let on_surface =
            surface.point_at(at(corners.map(|uv| uv[0])), at(corners.map(|uv| uv[1])));
        let flat = [0, 1, 2].map(|axis| at(positions.map(|point| point[axis])));
        distance3(on_surface, flat)
    })
    .fold(0.0, f64::max)
}

fn triangle_refinement(
    surface: &super::geometry::Surface,
    triangle: &ocs_geom::geom2d::constrained::ConstrainedTriangle,
    max_angle: f64,
    tolerance: f64,
    normal_cache: &mut ParameterMap<Option<[f64; 3]>>,
) -> Option<TriangleRefinement> {
    let corners = triangle.parameters;
    if parameter_triangle_degenerate(corners) {
        return Some(TriangleRefinement::Complete);
    }
    let edge_vertices = [[0, 1], [1, 2], [2, 0]];
    let mut edge_angles = [0.0; 3];
    for (index, [from, to]) in edge_vertices.into_iter().enumerate() {
        let parameters = [0.0, 0.25, 0.5, 0.75, 1.0].map(|unit| {
            [
                corners[from][0] + (corners[to][0] - corners[from][0]) * unit,
                corners[from][1] + (corners[to][1] - corners[from][1]) * unit,
            ]
        });
        edge_angles[index] = surface_normal_angle_cached(surface, &parameters, normal_cache)?;
    }
    // A constrained edge is a boundary sample pair (or a grid line), which
    // no insertion may split: the neighbouring face shares those samples.
    // The edge sampler measures the curve's own turn, this the surface
    // normal's along the straight parameter chord, and the two differ by a
    // hair — 0.171 against 0.170 on a torus rim failed a whole face (#1563).
    // Within twice the limit it stands; beyond that the ring is broken --
    // unless the chord still lies within the caller's chordal deflection of
    // the surface: then the normal turns across it (a crease, or a bend
    // tighter than the boundary samples) but the chord is where the surface
    // is. The triangle on it is then settled by its own deviation, since the
    // chord cannot be split and its turn would refine it without end.
    let deflection = fold_deviation(tolerance);
    let turning_chords = (0..3)
        .filter(|index| {
            let [from, to] = edge_vertices[*index];
            angle_exceeds(edge_angles[*index], max_angle * 2.0)
                && triangle.constraints[*index]
                && distance3(
                    surface.point_at(corners[from][0], corners[from][1]),
                    surface.point_at(corners[to][0], corners[to][1]),
                ) > tolerance
        })
        .collect::<Vec<_>>();
    if !turning_chords.is_empty()
        && turning_chords.iter().all(|index| {
            let [from, to] = edge_vertices[*index];
            chord_deviation(surface, [corners[from], corners[to]]) <= deflection
        })
        && flat_triangle_deviation(surface, corners) <= deflection
    {
        return Some(TriangleRefinement::Complete);
    }
    if (0..3).any(|index| {
        let [from, to] = edge_vertices[index];
        angle_exceeds(edge_angles[index], max_angle * 2.0)
            && triangle.constraints[index]
            && chord_deviation(surface, [corners[from], corners[to]]) > deflection
            && distance3(
                surface.point_at(corners[from][0], corners[from][1]),
                surface.point_at(corners[to][0], corners[to][1]),
            ) > tolerance
            && !folded_chord_settled(
                surface,
                [corners[from], corners[to]],
                edge_angles[index],
                tolerance,
            )
    }) {
        return Some(TriangleRefinement::Boundary);
    }
    if let Some((index, [from, to])) = edge_vertices
        .into_iter()
        .enumerate()
        .filter(|(index, [from, to])| {
            !triangle.constraints[*index]
                && angle_exceeds(edge_angles[*index], max_angle)
                && distance3(
                    surface.point_at(corners[*from][0], corners[*from][1]),
                    surface.point_at(corners[*to][0], corners[*to][1]),
                ) > tolerance
        })
        .max_by(|(a, _), (b, _)| edge_angles[*a].total_cmp(&edge_angles[*b]))
    {
        if folded_piece_settled(surface, corners, edge_angles[index], tolerance) {
            return Some(TriangleRefinement::Complete);
        }
        let middle = [
            0.5 * (corners[from][0] + corners[to][0]),
            0.5 * (corners[from][1] + corners[to][1]),
        ];
        if parameter_near(middle, corners[3 - from - to]) {
            return Some(TriangleRefinement::Complete);
        }
        return Some(TriangleRefinement::Interior(middle));
    }
    // Within tolerance in space the triangle is complete, as the edge test
    // above already treats its edges. Near a pole -- a surface edge collapsed
    // to a point -- the normal turns around the apex at any scale, so without
    // this floor the triangles there are split at their centroids until
    // MAX_FACE_ADDITIONS and the face is lost.
    if edge_vertices.iter().all(|[from, to]| {
        distance3(
            surface.point_at(corners[*from][0], corners[*from][1]),
            surface.point_at(corners[*to][0], corners[*to][1]),
        ) <= tolerance
    }) {
        return Some(TriangleRefinement::Complete);
    }
    match surface_triangle_angle_cached(surface, corners, normal_cache) {
        Some(angle) if !angle_exceeds(angle, max_angle) => Some(TriangleRefinement::Complete),
        Some(angle) if folded_piece_settled(surface, corners, angle, tolerance) => {
            Some(TriangleRefinement::Complete)
        }
        Some(_) => Some(TriangleRefinement::Interior([
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        ])),
        None => None,
    }
}

fn refine_scheduled(
    mesh: &mut Mesh,
    body: &Body,
    face: FaceKey,
    corners: [[f64; 2]; 3],
    max_angle: f64,
    depth: u32,
    pins: &[BoundaryPoint],
    boundary_segments: &[[[f64; 2]; 2]],
    tolerance: f64,
    split_axis: Option<usize>,
) -> bool {
    // Recursive patches need the same growth bound as constrained faces.
    // A folded or singular surface can otherwise keep emitting triangles
    // around a normal discontinuity until memory is exhausted.
    if mesh.triangles.len() >= MAX_FACE_ADDITIONS || face_limit_exceeded(mesh.triangles.len()) {
        return false;
    }
    let Some(node) = body.faces.get(face) else {
        return false;
    };
    let Some(surface) = body.surfaces.get(node.surface) else {
        return false;
    };
    if parameter_triangle_degenerate(corners) {
        return true;
    }
    let edge_vertices = [[0, 1], [1, 2], [2, 0]];
    let edge_angles = edge_vertices.map(|[from, to]| {
        let parameters = [0.0, 0.25, 0.5, 0.75, 1.0].map(|unit| {
            [
                corners[from][0] + (corners[to][0] - corners[from][0]) * unit,
                corners[from][1] + (corners[to][1] - corners[from][1]) * unit,
            ]
        });
        surface_normal_angle(surface, &parameters).unwrap_or(std::f64::consts::PI)
    });
    let boundary_edges = edge_vertices.map(|[from, to]| {
        boundary_segments.iter().any(|segment| {
            (segment[0] == corners[from] && segment[1] == corners[to])
                || (segment[1] == corners[from] && segment[0] == corners[to])
        })
    });
    let split_edge = edge_vertices
        .into_iter()
        .enumerate()
        .filter(|(index, _)| {
            angle_exceeds(edge_angles[*index], max_angle) && !boundary_edges[*index] && {
                let [from, to] = edge_vertices[*index];
                distance3(
                    surface.point_at(corners[from][0], corners[from][1]),
                    surface.point_at(corners[to][0], corners[to][1]),
                ) > tolerance
            }
        })
        .max_by(|(a, _), (b, _)| edge_angles[*a].total_cmp(&edge_angles[*b]));
    if let Some((index, [from, to])) = split_edge {
        if triangle_within_tolerance(surface, corners, tolerance)
            || folded_piece_settled(surface, corners, edge_angles[index], tolerance)
        {
            emit_scheduled(mesh, body, face, corners, pins);
            return true;
        }
        let opposite = 3 - from - to;
        let unit = if let Some(axis) = split_axis {
            let delta = corners[to][axis] - corners[from][axis];
            if delta != 0.0 {
                let projected = (corners[opposite][axis] - corners[from][axis]) / delta;
                if projected > 1e-9 && projected < 1.0 - 1e-9 {
                    projected
                } else {
                    0.5
                }
            } else {
                0.5
            }
        } else {
            0.5
        };
        let middle = [
            corners[from][0] + (corners[to][0] - corners[from][0]) * unit,
            corners[from][1] + (corners[to][1] - corners[from][1]) * unit,
        ];
        if parameter_near(middle, corners[opposite]) {
            return true;
        }
        if depth >= MAX_FACE_DEPTH {
            return false;
        }
        return [
            [corners[from], middle, corners[opposite]],
            [middle, corners[to], corners[opposite]],
        ]
        .into_iter()
        .all(|part| {
            refine_scheduled(
                mesh,
                body,
                face,
                part,
                max_angle,
                depth + 1,
                pins,
                boundary_segments,
                tolerance,
                split_axis,
            )
        });
    }
    if edge_angles.into_iter().enumerate().any(|(index, angle)| {
        if !angle_exceeds(angle, max_angle) {
            return false;
        }
        let [from, to] = edge_vertices[index];
        boundary_edges[index]
            && distance3(
                surface.point_at(corners[from][0], corners[from][1]),
                surface.point_at(corners[to][0], corners[to][1]),
            ) > tolerance
            && !folded_chord_settled(surface, [corners[from], corners[to]], angle, tolerance)
    }) {
        return false;
    }
    let triangle_angle = surface_triangle_angle(surface, corners);
    let split = triangle_angle
        .map(|angle| angle_exceeds(angle, max_angle))
        .unwrap_or(true);
    if split {
        let turn = triangle_angle.unwrap_or(std::f64::consts::PI);
        if triangle_within_tolerance(surface, corners, tolerance)
            || folded_piece_settled(surface, corners, turn, tolerance)
        {
            emit_scheduled(mesh, body, face, corners, pins);
            return true;
        }
        if depth >= MAX_FACE_DEPTH {
            return false;
        }
        let middle = [
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        ];
        for part in [
            [corners[0], corners[1], middle],
            [corners[1], corners[2], middle],
            [corners[2], corners[0], middle],
        ] {
            if !refine_scheduled(
                mesh,
                body,
                face,
                part,
                max_angle,
                depth + 1,
                pins,
                boundary_segments,
                tolerance,
                split_axis,
            ) {
                return false;
            }
        }
        return true;
    }
    emit_scheduled(mesh, body, face, corners, pins);
    true
}

/// A normal reversing by more than this within one triangle, or between two
/// neighbouring samples of an edge piece or a seed span, marks a fold: there
/// it turns at any scale, so halving by the angle limit only stops at the
/// linear floor. The fold stops below still require the piece to lie within
/// the caller's deflection.
const NORMAL_REVERSAL: f64 = std::f64::consts::FRAC_PI_2;

/// How far a flat triangle strays from its surface: the largest distance
/// between the surface at the parameter midpoints of the edges and at the
/// centroid, and the flat triangle at the same places.
fn piece_deviation(surface: &super::geometry::Surface, corners: [[f64; 2]; 3]) -> f64 {
    [&[0, 1][..], &[1, 2], &[2, 0], &[0, 1, 2]]
        .into_iter()
        .map(|indices| {
            let points: Vec<[f64; 2]> = indices.iter().map(|index| corners[*index]).collect();
            mean_deviation(surface, &points)
        })
        .fold(0.0, f64::max)
}

/// A triangle's corners, edge midpoints and centroid in (u, v).
fn piece_samples(corners: [[f64; 2]; 3]) -> [[f64; 2]; 7] {
    let mean = |indices: &[usize]| {
        let n = indices.len() as f64;
        [0, 1].map(|axis| indices.iter().map(|i| corners[*i][axis]).sum::<f64>() / n)
    };
    [
        corners[0],
        corners[1],
        corners[2],
        mean(&[0, 1]),
        mean(&[1, 2]),
        mean(&[2, 0]),
        mean(&[0, 1, 2]),
    ]
}

/// How far the surface strays from the straight chord between the first
/// and last of `parameters`, evenly spaced along a parameter line.
fn span_deviation(surface: &super::geometry::Surface, parameters: &[[f64; 2]]) -> f64 {
    let points: Vec<[f64; 3]> = parameters
        .iter()
        .map(|uv| surface.point_at(uv[0], uv[1]))
        .collect();
    let (first, last) = (points[0], points[points.len() - 1]);
    let steps = (points.len() - 1) as f64;
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let unit = index as f64 / steps;
            let chord = [0, 1, 2].map(|axis| first[axis] + (last[axis] - first[axis]) * unit);
            distance3(*point, chord)
        })
        .fold(0.0, f64::max)
}

/// The distance between the surface at the mean of `corners` in (u, v) and
/// the mean of the surface points at those corners.
fn mean_deviation(surface: &super::geometry::Surface, corners: &[[f64; 2]]) -> f64 {
    let n = corners.len() as f64;
    let uv = [0, 1].map(|axis| corners.iter().map(|uv| uv[axis]).sum::<f64>() / n);
    let points: Vec<[f64; 3]> = corners
        .iter()
        .map(|uv| surface.point_at(uv[0], uv[1]))
        .collect();
    let flat = [0, 1, 2].map(|axis| points.iter().map(|point| point[axis]).sum::<f64>() / n);
    distance3(surface.point_at(uv[0], uv[1]), flat)
}

std::thread_local! {
    /// While a [`tessellate`] runs: its chordal deflection, or its linear
    /// tolerance where none is set -- how far a folded piece may stray.
    static FOLD_DEVIATION: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// The fold deviation of one [`tessellate`] for as long as it lives.
struct FoldDeviation(Option<f64>);

impl FoldDeviation {
    fn set(deviation: f64) -> Self {
        Self(FOLD_DEVIATION.with(|cell| cell.replace(Some(deviation))))
    }
}

impl Drop for FoldDeviation {
    fn drop(&mut self) {
        FOLD_DEVIATION.with(|cell| cell.set(self.0));
    }
}

/// How far a folded piece, a boundary chord, or a triangle on one may stray
/// from its surface: the running tessellation's chordal deflection, else the
/// `linear` tolerance at hand.
fn fold_deviation(linear: f64) -> f64 {
    FOLD_DEVIATION.with(|cell| cell.get()).unwrap_or(linear)
}

/// Whether a piece the normal test would split again is settled anyway: the
/// normal reverses over it (`turn` past [`NORMAL_REVERSAL`]) or the
/// parametrisation folds inside it ([`parametrization_folds`]), so it turns
/// at any scale and halving only runs to the linear floor, while the flat
/// piece already lies within the caller's chordal deflection of the surface. A
/// folded patch -- one whose parametrisation turns over inside the face --
/// then meshes by its shape instead of by millimetre triangles along the
/// fold, and a tighter deflection still meshes it finer.
fn folded_piece_settled(
    surface: &super::geometry::Surface,
    corners: [[f64; 2]; 3],
    turn: f64,
    tolerance: f64,
) -> bool {
    (turn > NORMAL_REVERSAL || parametrization_folds(surface, &piece_samples(corners)))
        && piece_deviation(surface, corners) <= fold_deviation(tolerance)
}

/// Below this sine of the angle between the parameter directions, the
/// parametrisation is folding over: `Su` and `Sv` are within about half a
/// degree of parallel, and the normal there swings by whatever the last
/// digits of their cross product say rather than by the surface's shape.
const FOLDING_PARAMETRISATION: f64 = 1e-2;

/// Whether the parametrisation folds over at one of the piece's corners,
/// edge midpoints or centroid: the flanks of a fold, where the normal turns
/// by less than [`NORMAL_REVERSAL`] within a piece yet still at any scale. A
/// regular surface never comes near it; a pole, where one derivative
/// vanishes, is not counted.
fn parametrization_folds(surface: &super::geometry::Surface, samples: &[[f64; 2]]) -> bool {
    samples.iter().any(|uv| {
        let Some((du, dv)) = surface_frame(surface, *uv) else {
            return false;
        };
        let (du, dv) = (Vec3::from(du), Vec3::from(dv));
        let lengths = du.length() * dv.length();
        lengths > 0.0 && du.cross(dv).length() < FOLDING_PARAMETRISATION * lengths
    })
}

/// The same stop for a boundary chord `[from, to]` of a fill: the edge
/// sampler keeps a folded edge piece whole, so the fill must not take its
/// chord for a broken boundary while the surface stays within the deflection.
fn folded_chord_settled(
    surface: &super::geometry::Surface,
    chord: [[f64; 2]; 2],
    turn: f64,
    tolerance: f64,
) -> bool {
    turn > NORMAL_REVERSAL && mean_deviation(surface, &chord) <= fold_deviation(tolerance)
}

fn triangle_within_tolerance(
    surface: &super::geometry::Surface,
    corners: [[f64; 2]; 3],
    tolerance: f64,
) -> bool {
    let positions = corners.map(|uv| surface.point_at(uv[0], uv[1]));
    [[0, 1], [1, 2], [2, 0]]
        .into_iter()
        .all(|[from, to]| distance3(positions[from], positions[to]) <= tolerance)
}

fn surface_triangle_angle(
    surface: &super::geometry::Surface,
    corners: [[f64; 2]; 3],
) -> Option<f64> {
    let parameters = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [1.0 / 3.0; 3],
        [0.5, 0.25, 0.25],
        [0.25, 0.5, 0.25],
        [0.25, 0.25, 0.5],
    ]
    .map(|weights| {
        [
            corners[0][0] * weights[0] + corners[1][0] * weights[1] + corners[2][0] * weights[2],
            corners[0][1] * weights[0] + corners[1][1] * weights[1] + corners[2][1] * weights[2],
        ]
    });
    surface_normal_angle(surface, &parameters)
}

fn surface_triangle_angle_cached(
    surface: &super::geometry::Surface,
    corners: [[f64; 2]; 3],
    cache: &mut ParameterMap<Option<[f64; 3]>>,
) -> Option<f64> {
    let parameters = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [1.0 / 3.0; 3],
        [0.5, 0.25, 0.25],
        [0.25, 0.5, 0.25],
        [0.25, 0.25, 0.5],
    ]
    .map(|weights| {
        [
            corners[0][0] * weights[0] + corners[1][0] * weights[1] + corners[2][0] * weights[2],
            corners[0][1] * weights[0] + corners[1][1] * weights[1] + corners[2][1] * weights[2],
        ]
    });
    surface_normal_angle_cached(surface, &parameters, cache)
}

type NormalsBySurface = HashMap<(usize, u64, u64), Option<[f64; 3]>>;
type FramesBySurface = HashMap<(usize, u64, u64), Option<([f64; 3], [f64; 3])>>;

std::thread_local! {
    /// Spline normals already found while one body is meshed, by surface
    /// and parameters: the quality tests ask again for the corners and edge
    /// points neighbouring triangles share, a million normals for a column.
    /// Open only for the length of a [`tessellate`] — a surface's address
    /// names it there and nowhere else.
    static SPLINE_NORMALS: std::cell::RefCell<Option<NormalsBySurface>> =
        const { std::cell::RefCell::new(None) };
    /// The tangents beside them, for the same reason.
    static SPLINE_FRAMES: std::cell::RefCell<Option<FramesBySurface>> =
        const { std::cell::RefCell::new(None) };
}

/// Keeps spline normals for as long as it lives, unless an outer one does.
struct SplineNormals(bool);

impl SplineNormals {
    fn open() -> Self {
        SPLINE_NORMALS.with(|normals| {
            let mut normals = normals.borrow_mut();
            let opened = normals.is_none();
            if opened {
                *normals = Some(HashMap::new());
                SPLINE_FRAMES.with(|frames| *frames.borrow_mut() = Some(HashMap::new()));
            }
            Self(opened)
        })
    }
}

impl Drop for SplineNormals {
    fn drop(&mut self) {
        if self.0 {
            SPLINE_NORMALS.with(|normals| *normals.borrow_mut() = None);
            SPLINE_FRAMES.with(|frames| *frames.borrow_mut() = None);
        }
    }
}

/// A collapsed spline boundary has no differential normal at the pole.
/// Retain its parameter-dependent one-sided limit for display, without
/// accepting singularities in the interior of a surface.
fn surface_display_normal(surface: &super::geometry::Surface, uv: [f64; 2]) -> Option<[f64; 3]> {
    if !surface.is_spline() {
        return display_normal(surface, uv);
    }
    let key = (surface as *const _ as usize, uv[0].to_bits(), uv[1].to_bits());
    let known = SPLINE_NORMALS
        .with(|normals| normals.borrow().as_ref().and_then(|normals| normals.get(&key).copied()));
    if let Some(normal) = known {
        return normal;
    }
    let normal = display_normal(surface, uv);
    SPLINE_NORMALS.with(|normals| {
        if let Some(normals) = normals.borrow_mut().as_mut() {
            normals.insert(key, normal);
        }
    });
    normal
}

/// [`Surface::tangents_at`](super::geometry::Surface::tangents_at), kept
/// for a spline as its normals are.
fn surface_frame(
    surface: &super::geometry::Surface,
    uv: [f64; 2],
) -> Option<([f64; 3], [f64; 3])> {
    if !surface.is_spline() {
        return surface.tangents_at(uv[0], uv[1]);
    }
    let key = (surface as *const _ as usize, uv[0].to_bits(), uv[1].to_bits());
    let known = SPLINE_FRAMES
        .with(|frames| frames.borrow().as_ref().and_then(|frames| frames.get(&key).copied()));
    if let Some(frame) = known {
        return frame;
    }
    let frame = surface.tangents_at(uv[0], uv[1]);
    SPLINE_FRAMES.with(|frames| {
        if let Some(frames) = frames.borrow_mut().as_mut() {
            frames.insert(key, frame);
        }
    });
    frame
}

fn display_normal(surface: &super::geometry::Surface, uv: [f64; 2]) -> Option<[f64; 3]> {
    let direct = surface.normal_at(uv[0], uv[1]);
    let Some(nurbs) = surface.spline() else {
        return direct;
    };
    let ((u0, u1), (v0, v1)) = nurbs.domain();
    let on_u_boundary = parameter_value_near(uv[0], u0) || parameter_value_near(uv[0], u1);
    let on_v_boundary = parameter_value_near(uv[1], v0) || parameter_value_near(uv[1], v1);
    if !on_u_boundary && !on_v_boundary {
        return direct;
    }
    // At a collapsed row, cancellation can leave a tiny, nonzero tangent
    // with an arbitrary direction. Treat that like the exact zero tangent.
    let collapsed = surface.tangents_at(uv[0], uv[1]).is_some_and(|(du, dv)| {
        let u_length = Vec3::from(du).length() * (u1 - u0);
        let v_length = Vec3::from(dv).length() * (v1 - v0);
        (on_v_boundary && u_length <= v_length * 1e-12)
            || (on_u_boundary && v_length <= u_length * 1e-12)
    });
    if direct.is_some() && !collapsed {
        return direct;
    }
    let mut inward = uv;
    for (axis, (low, high)) in [(u0, u1), (v0, v1)].into_iter().enumerate() {
        let span = high - low;
        if !span.is_finite() || span <= 0.0 {
            continue;
        }
        let step = span * 1e-6;
        let value = if parameter_value_near(uv[axis], low) {
            low + step
        } else if parameter_value_near(uv[axis], high) {
            high - step
        } else {
            continue;
        };
        inward[axis] = value;
    }
    // At a corner, moving only along the collapsed boundary still leaves us
    // on the pole. A tiny cancellation residual there can look like a valid
    // normal and depend on which endpoint is the pole. Move every boundary
    // coordinate inward before evaluating the one-sided limit.
    if inward != uv {
        surface.normal_at(inward[0], inward[1])
    } else {
        None
    }
}

fn surface_normal_angle(
    surface: &super::geometry::Surface,
    parameters: &[[f64; 2]],
) -> Option<f64> {
    let normals = parameters
        .iter()
        .map(|uv| surface_display_normal(surface, *uv))
        .collect::<Option<Vec<_>>>()?;
    Some(ocs_geom::tessellation::max_direction_angle(&normals))
}

fn surface_normal_angle_cached(
    surface: &super::geometry::Surface,
    parameters: &[[f64; 2]],
    cache: &mut ParameterMap<Option<[f64; 3]>>,
) -> Option<f64> {
    let normals = parameters
        .iter()
        .map(|uv| {
            let key = uv.map(f64::to_bits);
            if let Some(normal) = cache.get(&key) {
                return *normal;
            }
            let normal = surface_display_normal(surface, *uv);
            cache.insert(key, normal);
            normal
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ocs_geom::tessellation::max_direction_angle(&normals))
}

fn surface_path_angle(surface: &super::geometry::Surface, parameters: &[[f64; 2]]) -> Option<f64> {
    let positions: Vec<_> = parameters
        .iter()
        .map(|uv| surface.point_at(uv[0], uv[1]))
        .collect();
    let position_scale = positions
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let path_span = positions
        .iter()
        .map(|point| distance3(positions[0], *point))
        .fold(0.0, f64::max);
    if path_span <= f64::EPSILON * 1024.0 * position_scale {
        return Some(0.0);
    }
    let (first, last) = parameters.first().zip(parameters.last())?;
    let delta = [last[0] - first[0], last[1] - first[1]];
    let length = delta[0].hypot(delta[1]);
    let frames = parameters
        .iter()
        .map(|uv| surface_frame(surface, *uv))
        .collect::<Option<Vec<_>>>()?;
    let normals = parameters
        .iter()
        .map(|uv| surface_display_normal(surface, *uv))
        .collect::<Option<Vec<_>>>()?;
    let mut largest = ocs_geom::tessellation::max_direction_angle(&normals);
    if length <= f64::MIN_POSITIVE {
        return Some(largest);
    }
    let direction = [delta[0] / length, delta[1] / length];
    let tangent_scale = frames
        .iter()
        .flat_map(|frame| [frame.0, frame.1])
        .map(|tangent| tangent.iter().map(|value| value * value).sum::<f64>())
        .fold(0.0, f64::max);
    let tangent_cutoff = tangent_scale * 1e-28;
    let directions: Vec<_> = frames
        .into_iter()
        .map(|tangents| {
            (Vec3::from(tangents.0) * direction[0] + Vec3::from(tangents.1) * direction[1])
                .to_array()
        })
        .filter(|tangent| {
            tangent.iter().all(|value| value.is_finite())
                && tangent.iter().map(|value| value * value).sum::<f64>() > tangent_cutoff
        })
        .collect();
    largest = largest.max(ocs_geom::tessellation::max_direction_angle(&directions));
    Some(largest)
}

fn emit_scheduled(
    mesh: &mut Mesh,
    body: &Body,
    face: FaceKey,
    corners: [[f64; 2]; 3],
    pins: &[BoundaryPoint],
) {
    emit_scheduled_with_cache(mesh, body, face, corners, pins, None, None);
}

fn emit_scheduled_cached(
    mesh: &mut Mesh,
    body: &Body,
    face: FaceKey,
    corners: [[f64; 2]; 3],
    pins: &[BoundaryPoint],
    cache: &ParameterMap<Option<[f64; 3]>>,
    point_cache: &mut ParameterMap<[f64; 3]>,
) {
    emit_scheduled_with_cache(
        mesh,
        body,
        face,
        corners,
        pins,
        Some(cache),
        Some(point_cache),
    );
}

fn emit_scheduled_with_cache(
    mesh: &mut Mesh,
    body: &Body,
    face: FaceKey,
    corners: [[f64; 2]; 3],
    pins: &[BoundaryPoint],
    cache: Option<&ParameterMap<Option<[f64; 3]>>>,
    mut point_cache: Option<&mut ParameterMap<[f64; 3]>>,
) {
    let Some(node) = body.faces.get(face) else {
        return;
    };
    let Some(surface) = body.surfaces.get(node.surface) else {
        return;
    };
    let points: Vec<Vec3> = corners
        .iter()
        .map(|parameters| {
            Vec3::from(canonical_point_cached(
                surface,
                *parameters,
                pins,
                point_cache.as_deref_mut(),
            ))
        })
        .collect();
    let Some(normal) = (points[1] - points[0])
        .cross(points[2] - points[0])
        .normalize()
    else {
        return;
    };
    let normals = corners.map(|parameters| {
        let stored = cache
            .and_then(|cache| cache.get(&parameters.map(f64::to_bits)))
            .copied()
            .flatten()
            .or_else(|| surface_display_normal(surface, parameters));
        let normal = stored
            .and_then(|normal| Vec3::from(normal).normalize())
            .unwrap_or(normal);
        if node.forward {
            normal
        } else {
            -normal
        }
    });
    let base = mesh.positions.len();
    let outward = normals[0];
    let order = if normal.dot(outward) >= 0.0 {
        [0, 1, 2]
    } else {
        [0, 2, 1]
    };
    for step in order {
        mesh.positions.push(points[step].to_array());
        mesh.normals.push(normals[step].to_array());
    }
    mesh.triangles.push([base, base + 1, base + 2]);
}

fn canonical_point_cached(
    surface: &super::geometry::Surface,
    parameters: [f64; 2],
    pins: &[BoundaryPoint],
    cache: Option<&mut ParameterMap<[f64; 3]>>,
) -> [f64; 3] {
    // A cache holds the pins already (see `fill_scheduled`).
    let key = parameter_key(parameters);
    if let Some(cache) = cache {
        if let Some(point) = cache.get(&key) {
            return *point;
        }
        let point = surface.point_at(parameters[0], parameters[1]);
        cache.insert(key, point);
        return point;
    }
    if let Some(pin) = pins.iter().find(|pin| pin.parameters == parameters) {
        return pin.position;
    }
    surface.point_at(parameters[0], parameters[1])
}

/// A parameter pair as a map key; `-0.0` and `0.0` are one parameter.
fn parameter_key(parameters: [f64; 2]) -> [u64; 2] {
    parameters.map(|value| (value + 0.0).to_bits())
}

/// The band a tube covers, as a ring in `(u, v)`.
///
/// A cylinder or cone face can be bounded by two rims and nothing else: it
/// wraps the whole way round, so there is no seam cutting it open and no ring
/// for its boundary to trace. Each rim is a closed circle, shared with the
/// disc that caps it, and projects to a line spanning a full turn — two of
/// those do not join up.
///
/// What they do say is where the band starts and stops, which with a full
/// turn of `u` is the whole region. `None` for anything else: a face bounded
/// by arcs and generators traces a proper ring and goes the ordinary way.
/// The default angular threshold, for a caller with no opinion.
pub fn default_angle() -> f64 {
    ocs_geom::tessellation::DEFAULT_ANGLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Cylinder, Surface, Torus};
    use crate::make::cuboid;
    use ocs_geom::geom2d::{Curve, Line};
    use ocs_geom::space::Plane;

    const TOL: f64 = 1e-9;

    /// A spline cone: the v = 0 edge of the net collapsed to one apex point,
    /// the v = 1 edge a quadratic ring around it.
    fn spline_cone() -> Surface {
        let apex = [0.0, 0.0, 1.0];
        let ring = [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
        ];
        let net = ring.iter().map(|rim| vec![apex, *rim]).collect();
        Surface::Nurbs(ocs_geom::space::NurbsSurface3::new(2, 1, net, vec![], vec![], None).unwrap())
    }

    fn refine(corners: [[f64; 2]; 3]) -> TriangleRefinement {
        let triangle = ocs_geom::geom2d::constrained::ConstrainedTriangle {
            parameters: corners,
            constraints: [false; 3],
        };
        let mut cache = ParameterMap::default();
        triangle_refinement(
            &spline_cone(),
            &triangle,
            15f64.to_radians(),
            1e-3,
            &mut cache,
        )
        .unwrap()
    }

    #[test]
    fn a_triangle_at_a_pole_within_tolerance_is_complete() {
        // At the apex the normal turns with u however small the triangle,
        // so only its size in space can settle it.
        let at_pole = refine([[0.2, 0.0], [0.8, 0.0], [0.5, 1e-7]]);
        assert!(matches!(at_pole, TriangleRefinement::Complete));
    }

    #[test]
    fn a_triangle_away_from_the_pole_is_still_refined() {
        let away = refine([[0.2, 0.4], [0.8, 0.4], [0.5, 0.6]]);
        assert!(matches!(away, TriangleRefinement::Interior(_)));
    }

    /// A degree-1 spline ridge, 2 wide and 1 deep: two flat halves meeting
    /// at `u = 0.5` (x = 1), 0.3 high there, so the normal turns by about
    /// 33 degrees across the crease.
    fn creased_sheet() -> Surface {
        let net = [[0.0, 0.0], [1.0, 0.3], [2.0, 0.0]]
            .iter()
            .map(|[x, z]| vec![[*x, 0.0, *z], [*x, 1.0, *z]])
            .collect();
        Surface::Nurbs(ocs_geom::space::NurbsSurface3::new(1, 1, net, vec![], vec![], None).unwrap())
    }

    /// A triangle whose first side is a boundary chord 0.02 long across the
    /// crease (about 3e-3 below the ridge), refined with a 15 degree limit.
    fn refine_across_crease(tolerance: f64) -> TriangleRefinement {
        let triangle = ocs_geom::geom2d::constrained::ConstrainedTriangle {
            parameters: [[0.495, 0.5], [0.505, 0.5], [0.5, 0.505]],
            constraints: [true, false, false],
        };
        let mut cache = ParameterMap::default();
        triangle_refinement(
            &creased_sheet(),
            &triangle,
            15f64.to_radians(),
            tolerance,
            &mut cache,
        )
        .unwrap()
    }

    #[test]
    fn a_boundary_chord_across_a_crease_within_the_deflection_is_kept() {
        // The normal turns by more than twice the limit along the chord, which
        // refused the triangle and lost the whole face; the chord is 3e-3
        // from the surface, inside a 5e-3 deflection.
        assert!(matches!(
            refine_across_crease(5e-3),
            TriangleRefinement::Complete
        ));
        let _deflection = FoldDeviation::set(5e-3);
        assert!(matches!(
            refine_across_crease(1e-3),
            TriangleRefinement::Complete
        ));
    }

    #[test]
    fn a_boundary_chord_across_a_crease_beyond_the_deflection_is_still_refused() {
        // A tighter deflection than the chord's 3e-3 keeps today's refusal.
        assert!(matches!(
            refine_across_crease(1e-3),
            TriangleRefinement::Boundary
        ));
        let _deflection = FoldDeviation::set(1e-3);
        assert!(matches!(
            refine_across_crease(1e-3),
            TriangleRefinement::Boundary
        ));
    }

    #[test]
    fn the_finest_retry_is_not_tried_on_a_face_that_already_needs_hundreds_of_samples() {
        // On a real corridor solid the 0.1 retry of three faces that still failed sampled 841,
        // 1 100 and 1 179 edge points; the 0.01 retry then sampled ten times as many and took
        // 12-18 s per face to fail again, which was 44.5 s of a 44.6 s tessellation.
        for points in [401, 841, 1_100, 1_179, usize::MAX] {
            assert!(!finest_retry_affordable(points), "{points} samples");
        }
        // A small face keeps the retry the audit that added it relied on.
        for points in [0, 12, 48, 400] {
            assert!(finest_retry_affordable(points), "{points} samples");
        }
    }

    #[test]
    fn mesh_mass_properties_preserve_translation_and_orientation() {
        for origin in [[0.0; 3], [1e9, -2e9, 3e9]] {
            let solid = cuboid(origin, [2.0, 3.0, 4.0]).unwrap();
            let mut mesh = self::body(&solid, default_angle(), TOL);
            for _ in 0..2 {
                let properties = mesh.inertial_properties().unwrap();
                assert!((properties.volume - 24.0).abs() < 1e-9);
                for axis in 0..3 {
                    assert!(
                        (properties.centroid[axis] - origin[axis] - [1.0, 1.5, 2.0][axis]).abs()
                            < 1e-6
                    );
                    assert!(
                        (properties.principal_moments[axis] - [26.0, 40.0, 50.0][axis]).abs()
                            < 1e-8
                    );
                }
                assert!((mesh.surface_area().unwrap() - 52.0).abs() < 1e-9);
                for triangle in &mut mesh.triangles {
                    triangle.swap(1, 2);
                }
            }
        }
        assert!(Mesh::default().mass_properties().is_none());
        assert!(Mesh {
            positions: vec![[0.0; 3]],
            triangles: vec![[0, 0, 0]],
            ..Default::default()
        }
        .mass_properties()
        .is_none());
    }

    #[test]
    fn a_box_meshes_into_two_triangles_a_side() {
        let solid = cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        assert_eq!(mesh.len(), 12, "six faces, two triangles each");
        assert_eq!(mesh.positions.len(), 36);
    }

    fn budget_tolerance() -> TessellationTolerance {
        TessellationTolerance::new(ocs_geom::tessellation::DEFAULT_ANGLE, 1e-3)
            .with_chordal_deflection(5e-3)
    }

    #[test]
    fn a_triangle_budget_stops_the_mesh_and_says_so() {
        let torus = crate::make::torus([0.0; 3], 5.0, 1.0).unwrap();
        let whole = tessellate(&torus, budget_tolerance());
        assert!(!whole.over_budget && whole.missing_faces.is_empty());
        let budgeted = tessellate(&torus, budget_tolerance().with_triangle_budget(1_000));
        assert!(budgeted.over_budget);
        assert!(!budgeted.missing_faces.is_empty());
        assert!(
            budgeted.mesh.triangles.len() <= 1_000,
            "{} triangles",
            budgeted.mesh.triangles.len()
        );
    }

    #[test]
    fn a_mesh_within_its_budget_is_the_mesh_without_one() {
        // Triangle count and a hash of every vertex, normal and index of the
        // default tessellation, measured on 003eee3 before budgets existed.
        type Fingerprint = fn(&Mesh) -> (usize, u64);
        let bodies: [(Body, Fingerprint, (usize, u64)); 2] = [
            // The cylinder's flat caps are cocircular points, so their
            // triangulation is not unique and the tie breaks per platform:
            // it gets the triangulation-independent fingerprint.
            (
                crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap(),
                shape_fingerprint,
                (508, 6_030_817_377_226_346_326),
            ),
            (
                crate::make::torus([0.0; 3], 5.0, 1.0).unwrap(),
                quantized_fingerprint,
                (23_762, 3_222_715_392_039_835_452),
            ),
        ];
        for (body, fingerprint, before) in bodies {
            let plain = tessellate(&body, budget_tolerance());
            assert_eq!(fingerprint(&plain.mesh), before);
            for budget in [usize::MAX, 2 * plain.mesh.triangles.len()] {
                let budgeted = tessellate(&body, budget_tolerance().with_triangle_budget(budget));
                assert_eq!(budgeted, plain);
            }
        }
    }

    #[test]
    fn a_bad_pcurve_does_not_remove_an_otherwise_valid_edge_schedule() {
        let mut solid = cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let edge_key = solid.edge_keys().next().unwrap();
        let edge = solid.edges.get(edge_key).unwrap().clone();
        for coedge_key in &edge.coedges {
            solid.coedges.get_mut(*coedge_key).unwrap().pcurve = Some(Curve::Line(Line {
                start: [100.0, 100.0],
                end: [101.0, 101.0],
            }));
        }

        let samples = shared_edge_samples(&solid, edge_key, default_angle(), TOL).unwrap();
        assert_eq!(
            samples.first().unwrap().position,
            solid.vertices.get(edge.start).unwrap().point
        );
        assert_eq!(
            samples.last().unwrap().position,
            solid.vertices.get(edge.end).unwrap().point
        );
    }

    #[test]
    fn rendered_boundary_follows_the_surface_when_the_trim_curve_is_inexact() {
        let torus = Surface::Torus(Torus {
            frame: Plane::XY,
            major_radius: 12.65,
            minor_radius: -2.0,
        });
        let position = surface_consensus_position(&[&torus], [10.95, 0.0, 0.0], TOL).unwrap();
        let parameters = torus.parameters_at(position).unwrap();
        let pins = [BoundaryPoint {
            parameters: [parameters.0, parameters.1],
            position,
        }];
        let snapped = canonical_point_cached(&torus, pins[0].parameters, &pins, None);
        assert!((snapped[0] - 10.65).abs() < TOL, "{snapped:?}");
        assert!(snapped[1].abs() < TOL && snapped[2].abs() < TOL);
    }

    #[test]
    fn displayed_edge_uses_the_shared_surface_intersection() {
        let cylinder = Surface::Cylinder(Cylinder {
            base: Plane::XY,
            radius: 10.65,
        });
        let torus = Surface::Torus(Torus {
            frame: Plane::XY,
            major_radius: 12.65,
            minor_radius: -2.0,
        });
        let displayed =
            surface_consensus_position(&[&cylinder, &torus], [10.95, 0.0, 0.0], TOL).unwrap();
        assert!((displayed[0] - 10.65).abs() < TOL, "{displayed:?}");
        assert!(displayed[1].abs() < TOL && displayed[2].abs() < TOL);
    }

    #[test]
    fn adjacent_faces_keep_one_canonical_boundary_position() {
        let first = Surface::Plane(Plane::XY);
        let second =
            Surface::Plane(Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap());
        let position =
            surface_consensus_position(&[&first, &second], [1.0, 4e-7, 4e-7], 1e-6).unwrap();
        let canonical = |surface: &Surface| {
            let parameters = surface.parameters_at(position).unwrap();
            canonical_point_cached(
                surface,
                [parameters.0, parameters.1],
                &[BoundaryPoint {
                    parameters: [parameters.0, parameters.1],
                    position,
                }],
                None,
            )
        };

        assert_eq!(canonical(&first), canonical(&second));
        assert!(first.distance_to(position).abs() <= 1e-6);
        assert!(second.distance_to(position).abs() <= 1e-6);
    }

    #[test]
    fn an_edge_dividing_one_plane_is_drawn_and_a_tangent_one_is_not() {
        let mut solid = cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let edge_key = solid.edge_keys().next().unwrap();
        let edge = solid.edges.get(edge_key).unwrap().clone();
        let faces: Vec<_> = edge
            .coedges
            .iter()
            .map(|coedge| {
                let coedge = solid.coedges.get(*coedge).unwrap();
                solid.loops.get(coedge.owner).unwrap().owner
            })
            .collect();
        let reference = solid.faces.get(faces[0]).unwrap().clone();
        let adjacent = solid.faces.get_mut(faces[1]).unwrap();
        adjacent.surface = reference.surface;
        adjacent.forward = reference.forward;
        let schedule = [
            crate::place::EdgeSample {
                parameter: edge.start_parameter,
                position: solid.vertices.get(edge.start).unwrap().point,
            },
            crate::place::EdgeSample {
                parameter: edge.end_parameter,
                position: solid.vertices.get(edge.end).unwrap().point,
            },
        ];
        assert!(!smooth_scheduled_edge(&solid, edge_key, &schedule));

        // A round meets the faces it joins tangentially: those edges stay hidden.
        let block = cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let corner = block
            .edge_keys()
            .find(|key| {
                let (start, end) = block.edge_endpoints(*key).unwrap();
                start[2] == 10.0 && end[2] == 10.0 && start[1] == 10.0 && end[1] == 10.0
            })
            .unwrap();
        let rounded = crate::fillet_edges(&block, &[corner], 2.0).unwrap();
        let tessellation = tessellate(&rounded, TessellationTolerance::new(default_angle(), TOL));
        assert_eq!(tessellation.edges.len(), rounded.edges.len() - 2);
    }

    #[test]
    fn planar_isolines_are_opt_in_and_counted_per_axis() {
        let solid = cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let tolerance = TessellationTolerance::new(default_angle(), TOL);

        assert!(tessellate_wireframe(&solid, tolerance.with_isolines(2))
            .isolines
            .is_empty());
        assert_eq!(
            tessellate_wireframe(
                &solid,
                tolerance.with_uv_isolines(1, 0).with_planar_isolines(true),
            )
            .isolines
            .len(),
            6,
        );
        assert_eq!(
            tessellate_wireframe(
                &solid,
                tolerance.with_uv_isolines(0, 2).with_planar_isolines(true),
            )
            .isolines
            .len(),
            12,
        );
    }

    #[test]
    fn cylinder_isolines_are_not_mistaken_for_the_parameter_seam() {
        let solid = crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let tolerance = TessellationTolerance::new(default_angle(), TOL)
            .with_uv_isolines(4, 0);
        let isolines = tessellate_wireframe(&solid, tolerance).isolines;

        assert_eq!(isolines.len(), 4);
        assert!(isolines.iter().all(|isoline| {
            let first = isoline.positions.first().unwrap();
            let last = isoline.positions.last().unwrap();
            (last[2] - first[2]).abs() > 9.9
        }));
    }

    #[test]
    fn the_triangles_cover_the_boxs_own_area() {
        let solid = cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        let area: f64 = mesh
            .triangles
            .iter()
            .map(|t| {
                let a = Vec3::from(mesh.positions[t[0]]);
                let b = Vec3::from(mesh.positions[t[1]]);
                let c = Vec3::from(mesh.positions[t[2]]);
                (b - a).cross(c - a).length() * 0.5
            })
            .sum();
        // 2·(2·3 + 3·4 + 2·4)
        assert!((area - 52.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn every_normal_points_out_of_the_solid() {
        // The one that matters for anything drawn: a face wound the wrong way
        // lights the solid inside out and no shading afterwards recovers it.
        let solid = cuboid([0.0; 3], [4.0, 6.0, 8.0]).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        let centre = Vec3::new(2.0, 3.0, 4.0);
        for triangle in &mesh.triangles {
            let corner = Vec3::from(mesh.positions[triangle[0]]);
            let normal = Vec3::from(mesh.normals[triangle[0]]);
            assert!(
                normal.dot(corner - centre) > 0.0,
                "a triangle faced inwards at {corner:?}"
            );
        }
    }

    #[test]
    fn a_wall_is_sampled_as_finely_as_the_tolerance_asks() {
        // The subdivision cap used to decide this instead of the tolerance. A
        // tube starts as one rectangle spanning a whole turn, so five levels
        // of halving reached 32 segments and stopped — however small the angle
        // asked for. The wall then stayed visibly coarser than the rim drawn
        // around it, which is a mismatch no shading hides.
        let solid = crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let wall = solid
            .faces
            .iter()
            .find(|(_, face)| {
                matches!(
                    solid.surfaces.get(face.surface),
                    Some(crate::geometry::Surface::Cylinder(_))
                )
            })
            .map(|(key, _)| key)
            .unwrap();

        // A fiftieth of a per cent of the radius: about fifty sides, which is
        // what the edge sampling in a drawing asks for.
        let mesh = face(&solid, wall, 5.0 * 0.002, 1e-9).expect("a drawn wall");
        // Read the count back off the mesh: how many distinct angles the
        // triangle corners land on around the axis.
        let mut angles: Vec<i64> = mesh
            .positions
            .iter()
            .map(|p| (p[1].atan2(p[0]) * 1e6) as i64)
            .collect();
        angles.sort_unstable();
        angles.dedup();
        assert!(angles.len() >= 48, "only {} sides", angles.len());
    }

    /// A cylinder wall on its two rims alone, the seam between them taken
    /// away.
    fn seamless_tube() -> (Body, FaceKey) {
        let mut solid = crate::make::cylinder([0.0; 3], 3.0, 6.0).unwrap();
        let wall = solid
            .faces
            .iter()
            .find(|(_, face)| {
                matches!(
                    solid.surfaces.get(face.surface),
                    Some(crate::Surface::Cylinder(_))
                )
            })
            .map(|(key, _)| key)
            .unwrap();

        // Take the seam away, leaving the wall on its two rims alone.
        let ring = solid.faces.get(wall).unwrap().loops[0];
        let kept: Vec<_> = solid
            .loops
            .get(ring)
            .unwrap()
            .coedges
            .iter()
            .copied()
            .filter(|coedge| {
                let edge = solid.coedges.get(*coedge).unwrap().edge;
                let node = solid.edges.get(edge).unwrap();
                node.start == node.end
            })
            .collect();
        assert_eq!(kept.len(), 2, "two rims");
        let face = solid.faces.get_mut(wall).unwrap();
        face.loops = Vec::new();
        for coedge in kept {
            let owner = solid.loops.insert(crate::topology::Loop {
                coedges: vec![coedge],
                owner: wall,
                provenance: crate::Provenance::Synthesized,
            });
            solid.coedges.get_mut(coedge).unwrap().owner = owner;
            solid.faces.get_mut(wall).unwrap().loops.push(owner);
        }
        (solid, wall)
    }

    #[test]
    fn a_tube_with_no_seam_still_knows_the_band_it_covers() {
        // A cylinder wall bounded by a rim at each end and nothing else: it
        // wraps the whole way round, so no seam cuts it open and its boundary
        // traces no ring in (u, v). Files carry solids shaped that way, and
        // the face was dropped for want of a ring to fill.
        let (solid, wall) = seamless_tube();
        let mesh =
            crate::mesh::face(&solid, wall, default_angle(), 1e-9).expect("a drawn wall");
        let area: f64 = mesh
            .triangles
            .iter()
            .map(|t| {
                let at = |i: usize| Vec3::from(mesh.positions[t[i]]);
                (at(1) - at(0)).cross(at(2) - at(0)).length() * 0.5
            })
            .sum();
        let expected = TAU * 3.0 * 6.0;
        assert!(
            (area - expected).abs() < 0.02 * expected,
            "{area} vs {expected}"
        );
    }

    #[test]
    fn a_tube_whose_rims_start_a_rounding_apart_still_meshes() {
        // The tube of a scaffold brace (OpenCADStudio#2308). Its rims are
        // fitted intersection curves, on the wall at their corner and a hair
        // off it in between, so neither reads as a circle of the surface.
        // Both corners sit at one angle round the axis but land a rounding
        // apart, each placed by its own curve. Rotated to a common seam, one
        // rim started that rounding past it and ended as far beyond the
        // other, and the wall was dropped. Which rim leads depends on the
        // loop order, so the turn is tried both ways.
        for turn in [3e-11_f64, -3e-11] {
            let (mut solid, wall) = seamless_tube();
            let rims: Vec<EdgeKey> = solid
                .faces
                .get(wall)
                .unwrap()
                .loops
                .iter()
                .map(|ring| {
                    let coedge = solid.loops.get(*ring).unwrap().coedges[0];
                    solid.coedges.get(coedge).unwrap().edge
                })
                .collect();
            for rim in rims {
                let edge = solid.edges.get(rim).unwrap().clone();
                let Some(crate::Curve3::Circle(circle)) = solid.curves.get(edge.curve) else {
                    panic!("a circular rim");
                };
                let origin = circle.plane.origin;
                let start = if origin[2] > 0.0 { turn } else { 0.0 };
                let plane =
                    Plane::orthonormal(origin, [start.cos(), start.sin(), 0.0], [0.0, 0.0, 1.0])
                        .unwrap();
                let fitted = crate::Curve3::Ellipse(crate::geometry::Ellipse3 {
                    plane,
                    major_radius: 3.0,
                    minor_radius: 3.0 - 2e-6,
                });
                solid.vertices.get_mut(edge.start).unwrap().point = fitted.point_at(0.0);
                *solid.curves.get_mut(edge.curve).unwrap() = fitted;
            }

            let mesh = crate::mesh::face(&solid, wall, default_angle(), 1e-9)
                .unwrap_or_else(|| panic!("no wall with the top rim turned {turn:e}"));
            let mut area = 0.0;
            for t in &mesh.triangles {
                let at = |i: usize| Vec3::from(mesh.positions[t[i]]);
                let normal = (at(1) - at(0)).cross(at(2) - at(0));
                let centre = (at(0) + at(1) + at(2)) * (1.0 / 3.0);
                let out = Vec3::new(centre.x, centre.y, 0.0);
                assert!(normal.dot(out) > 0.0, "a triangle facing the axis");
                area += normal.length() * 0.5;
            }
            let expected = TAU * 3.0 * 6.0;
            assert!(
                (area - expected).abs() < 0.02 * expected,
                "{area} vs {expected} with the top rim turned {turn:e}"
            );
        }
    }

    #[test]
    fn a_hole_stays_a_hole_however_its_loop_was_listed() {
        // A plate with a hole through it. Nothing says the outer loop comes
        // first — a face lifted from a file lists its loops however the file
        // did — so the ring that bounds the face is chosen by area. Taking
        // the first on trust fills the hole and empties the metal, which is a
        // picture that looks deliberate.
        // A ring of square section: its two flat faces are annuli, each
        // bounded by an outer rim with an inner one cut out of it.
        use ocs_geom::geom2d::{Curve as Curve2, Line};
        let corners = [[4.0, 0.0], [7.0, 0.0], [7.0, 2.0], [4.0, 2.0]];
        let profile: Vec<Curve2> = (0..4)
            .map(|index| {
                Curve2::Line(Line {
                    start: corners[index],
                    end: corners[(index + 1) % 4],
                })
            })
            .collect();
        let plane =
            ocs_geom::space::Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap();
        let drilled =
            crate::revolve(plane, &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).expect("a ring");

        let holed = drilled
            .faces
            .iter()
            .find(|(_, face)| face.loops.len() == 2)
            .map(|(key, _)| key)
            .expect("a face with a hole in it");

        let area = |body: &Body, face| {
            crate::mesh::face(body, face, default_angle(), 1e-9)
                .map(|mesh| {
                    mesh.triangles
                        .iter()
                        .map(|t| {
                            let at = |i: usize| Vec3::from(mesh.positions[t[i]]);
                            (at(1) - at(0)).cross(at(2) - at(0)).length() * 0.5
                        })
                        .sum::<f64>()
                })
                .unwrap_or(0.0)
        };
        let expected = std::f64::consts::PI * (49.0 - 16.0);
        let drawn = area(&drilled, holed);
        assert!(
            (drawn - expected).abs() < 0.02 * expected,
            "{drawn} vs {expected}"
        );

        // And the same face with its loops listed the other way round has to
        // come out identical.
        let mut swapped = drilled.clone();
        swapped.faces.get_mut(holed).unwrap().loops.swap(0, 1);
        let other = area(&swapped, holed);
        assert!((drawn - other).abs() < 1e-9, "{drawn} vs {other}");
    }

    #[test]
    fn the_winding_agrees_with_the_normal() {
        let solid = cuboid([0.0; 3], [4.0, 6.0, 8.0]).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        for triangle in &mesh.triangles {
            let a = Vec3::from(mesh.positions[triangle[0]]);
            let b = Vec3::from(mesh.positions[triangle[1]]);
            let c = Vec3::from(mesh.positions[triangle[2]]);
            let wound = (b - a).cross(c - a).normalize().unwrap();
            let stored = Vec3::from(mesh.normals[triangle[0]]);
            assert!(
                wound.dot(stored) > 0.9,
                "the winding and the normal disagree"
            );
        }
    }

    #[test]
    fn reversed_parameter_triangles_are_emitted_outward() {
        let solid = cuboid([0.0; 3], [4.0, 6.0, 8.0]).unwrap();
        let face = solid.face_keys().next().unwrap();
        let mut mesh = Mesh::default();
        emit_scheduled_with_cache(
            &mut mesh,
            &solid,
            face,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0]],
            &[],
            None,
            None,
        );
        let triangle = mesh.triangles[0];
        let a = Vec3::from(mesh.positions[triangle[0]]);
        let b = Vec3::from(mesh.positions[triangle[1]]);
        let c = Vec3::from(mesh.positions[triangle[2]]);
        let wound = (b - a).cross(c - a).normalize().unwrap();
        let stored = Vec3::from(mesh.normals[triangle[0]]);
        assert!(wound.dot(stored) > 0.9);
    }

    #[test]
    fn every_vertex_is_on_the_solid_it_came_from() {
        let solid = cuboid([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        for position in &mesh.positions {
            let on_a_face = solid.face_keys().any(|face| {
                solid
                    .faces
                    .get(face)
                    .and_then(|node| solid.surfaces.get(node.surface))
                    .is_some_and(|surface| surface.contains(*position, 1e-9))
            });
            assert!(on_a_face, "{position:?} is not on the solid");
        }
    }

    #[test]
    fn a_flat_face_is_never_split_however_fine_the_angle() {
        // A plane's triangles are exact, so refining them would only cost
        // vertices.
        let solid = cuboid([0.0; 3], [10.0; 3]).unwrap();
        let coarse = self::body(&solid, 1.0, TOL).len();
        let fine = self::body(&solid, default_angle() * 0.25, TOL).len();
        assert_eq!(coarse, fine, "a plane does not curve");
        assert_eq!(fine, 12);
    }

    #[test]
    fn a_cylinder_wall_is_split_until_it_follows_its_surface() {
        // Triangulating the boundary alone would give the wall two triangles
        // whatever the tolerance, and a cylinder would render as a flat
        // ribbon.
        let solid = crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let coarse = self::body(&solid, 2.0, TOL).len();
        let fine = self::body(&solid, 0.01, TOL).len();
        assert!(fine > coarse * 4, "{coarse} then {fine}");
    }

    #[test]
    fn a_cylinders_mesh_stays_on_the_cylinder() {
        let solid = crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let mesh = self::body(&solid, default_angle(), TOL);
        assert!(!mesh.is_empty());
        for position in &mesh.positions {
            let radius = (position[0] * position[0] + position[1] * position[1]).sqrt();
            let on_wall = (radius - 5.0).abs() < 0.05;
            let on_cap = radius <= 5.0 + 1e-6
                && (position[2].abs() < 1e-9 || (position[2] - 10.0).abs() < 1e-9);
            assert!(on_wall || on_cap, "{position:?} is off the cylinder");
        }
    }

    #[test]
    fn a_body_with_nothing_in_it_meshes_to_nothing() {
        let mesh = self::body(&Body::new(), default_angle(), TOL);
        assert!(mesh.is_empty());
    }

    #[test]
    fn two_meshes_join_without_their_indices_colliding() {
        let solid = cuboid([0.0; 3], [1.0; 3]).unwrap();
        let mut one = self::body(&solid, default_angle(), TOL);
        let other = self::body(&solid, default_angle(), TOL);
        let counts = (one.len(), other.len());
        one.absorb(other);
        assert_eq!(one.len(), counts.0 + counts.1);
        for triangle in &one.triangles {
            assert!(triangle.iter().all(|index| *index < one.positions.len()));
        }
    }

    #[test]
    fn a_solid_at_survey_coordinates_meshes_where_it_is() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let solid = cuboid(origin, [0.5, 0.5, 0.5]).unwrap();
        let mesh = self::body(&solid, default_angle(), 1e-6);
        assert_eq!(mesh.len(), 12);
        for position in &mesh.positions {
            assert!(
                (position[0] - origin[0]).abs() <= 0.5 + 1e-6,
                "{position:?}"
            );
        }
    }

    #[test]
    fn a_boolean_result_meshes_too() {
        let a = cuboid([0.0; 3], [10.0; 3]).unwrap();
        let b = cuboid([5.0; 3], [10.0; 3]).unwrap();
        let joined =
            crate::boolean::combine(a, b, crate::boolean::Operation::Union, TOL)
                .unwrap();
        let mesh = self::body(&joined, default_angle(), TOL);
        assert!(!mesh.is_empty());
        // An imprinted face is no longer a rectangle, so it takes more than
        // two triangles — but never fewer.
        assert!(
            mesh.len() >= joined.faces.len() * 2,
            "{} faces gave only {} triangles",
            joined.faces.len(),
            mesh.len()
        );
        for triangle in &mesh.triangles {
            assert!(triangle.iter().all(|i| *i < mesh.positions.len()));
        }
    }

    /// Faces on degree-1 bilinear patches `[p00, p10, p01, p11]`; see
    /// [`spline_sheet`].
    fn bilinear_sheet(patches: &[[[f64; 3]; 4]], pcurves: bool) -> Body {
        let nets: Vec<_> = patches
            .iter()
            .map(|[p00, p10, p01, p11]| (1, vec![vec![*p00, *p01], vec![*p10, *p11]]))
            .collect();
        spline_sheet(&nets, pcurves)
    }

    /// Faces on B-spline patches `(degree, net)`, the net indexed `[u][v]`,
    /// each bounded by its four sides (`v = 0`, `u = 1`, `v = 1`, `u = 0`),
    /// which must be straight lines between the net's corners; corners and
    /// sides two patches share are one vertex and one edge. With `pcurves`
    /// every side carries its straight line in the patch's (u, v).
    fn spline_sheet(patches: &[(usize, Vec<Vec<[f64; 3]>>)], pcurves: bool) -> Body {
        use crate::{Coedge, Face, Loop, Lump, Provenance, Shell};
        let mut body = Body::new();
        let lump = body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        body.roots.push(lump);
        let shell = body.shells.insert(Shell {
            faces: Vec::new(),
            owner: lump,
            provenance: Provenance::Synthesized,
        });
        body.lumps.get_mut(lump).unwrap().shells.push(shell);
        for (degree, net) in patches {
            let (last_u, last_v) = (net.len() - 1, net[0].len() - 1);
            let [p00, p10, p01, p11] = [
                net[0][0],
                net[last_u][0],
                net[0][last_v],
                net[last_u][last_v],
            ];
            let surface = body.surfaces.insert(Surface::Nurbs(
                ocs_geom::space::NurbsSurface3::new(
                    *degree,
                    *degree,
                    net.clone(),
                    vec![],
                    vec![],
                    None,
                )
                .unwrap(),
            ));
            let face = body.faces.insert(Face {
                surface,
                forward: true,
                loops: Vec::new(),
                owner: shell,
                provenance: Provenance::Synthesized,
            });
            body.shells.get_mut(shell).unwrap().faces.push(face);
            let ring = body.loops.insert(Loop {
                coedges: Vec::new(),
                owner: face,
                provenance: Provenance::Synthesized,
            });
            body.faces.get_mut(face).unwrap().loops.push(ring);
            let sides = [
                (p00, p10, [0.0, 0.0], [1.0, 0.0]),
                (p10, p11, [1.0, 0.0], [1.0, 1.0]),
                (p11, p01, [1.0, 1.0], [0.0, 1.0]),
                (p01, p00, [0.0, 1.0], [0.0, 0.0]),
            ];
            for (from, to, uv_from, uv_to) in sides {
                let (edge, forward) = sheet_edge(&mut body, from, to);
                let pcurve = pcurves.then(|| {
                    Curve::Line(Line {
                        start: uv_from,
                        end: uv_to,
                    })
                });
                let coedge = body.coedges.insert(Coedge {
                    edge,
                    forward,
                    pcurve,
                    owner: ring,
                    provenance: Provenance::Synthesized,
                });
                body.edges.get_mut(edge).unwrap().coedges.push(coedge);
                body.loops.get_mut(ring).unwrap().coedges.push(coedge);
            }
        }
        body
    }

    /// The straight edge from `from` to `to`, reusing the vertices and the edge
    /// an earlier side already made; `false` when it runs the other way.
    fn sheet_edge(body: &mut Body, from: [f64; 3], to: [f64; 3]) -> (EdgeKey, bool) {
        use crate::{Curve3, Edge, Line3, Provenance, Vertex};
        let mut vertex = |point: [f64; 3]| {
            let known = body.vertices.iter().find(|(_, v)| v.point == point);
            match known.map(|(key, _)| key) {
                Some(key) => key,
                None => body.vertices.insert(Vertex {
                    point,
                    provenance: Provenance::Synthesized,
                }),
            }
        };
        let (start, end) = (vertex(from), vertex(to));
        let shared = body
            .edges
            .iter()
            .find(|(_, edge)| edge.start == end && edge.end == start);
        if let Some((key, _)) = shared {
            return (key, false);
        }
        let curve = body.curves.insert(Curve3::Line(Line3 {
            origin: from,
            direction: [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
        }));
        let edge = body.edges.insert(Edge {
            curve,
            start_parameter: 0.0,
            end_parameter: 1.0,
            start,
            end,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        (edge, true)
    }

    /// A bow-tie strip, about 11 long and 0.1 to 0.3 wide, almost flat: its
    /// long sides run in opposite directions, so the patch turns over along
    /// the curve `v = 1.1 u - 0.5` (from `u = 5/11` on `v = 0` to `v = 0.6`
    /// on `u = 1`, metres long in space, off every dyadic sample) and its
    /// normal reverses across it. The far corner sits 1e-5 off the plane, so
    /// the normal turns through the fold within a fraction of a millimetre.
    const BOW_TIE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [0.0, 0.1, 0.0],
        [5.0, 0.0, 0.0],
        [-6.0, 0.3, 1e-5],
    ];

    fn fold_tolerance() -> TessellationTolerance {
        TessellationTolerance::new(ocs_geom::tessellation::DEFAULT_ANGLE, 1e-3)
            .with_chordal_deflection(5e-3)
    }

    #[test]
    fn a_folded_patch_meshes_by_its_shape_not_by_millimetres_along_the_fold() {
        // Its sides carry pcurves, so they are sampled; the fill then met a
        // normal that reverses at any scale and halved the triangles along
        // the fold down to the linear floor.
        let body = bilinear_sheet(&[BOW_TIE], true);
        let mesh = tessellate(&body, fold_tolerance());
        assert!(
            mesh.missing_faces.is_empty() && mesh.mesh.triangles.len() < 1_000,
            "{} triangles, {} faces missing",
            mesh.mesh.triangles.len(),
            mesh.missing_faces.len()
        );
    }

    #[test]
    fn a_line_edge_between_folded_patches_is_sampled() {
        // No pcurves on the sides: the edge sampler halved the
        // shared side by the faces' normals, which reverse on it, until
        // MAX_DEPTH and dropped the edge, so both faces were lost.
        // The second patch is flat and borders the bow-tie's `u = 1` side,
        // which the fold crosses at `v = 0.6`.
        let [_, p10, _, p11] = BOW_TIE;
        let second = [
            p11,
            p10,
            [p11[0], p11[1] + 0.3, p11[2]],
            [p10[0], p10[1] + 0.3, p10[2]],
        ];
        let body = bilinear_sheet(&[BOW_TIE, second], false);
        let mesh = tessellate(&body, fold_tolerance());
        assert!(
            mesh.missing_faces.is_empty() && mesh.mesh.triangles.len() < 2_000,
            "{} triangles, {} faces missing",
            mesh.mesh.triangles.len(),
            mesh.missing_faces.len()
        );
    }

    #[test]
    fn a_tighter_chordal_deflection_still_meshes_a_folded_patch_finer() {
        // The fold stop accepts a piece only within the caller's deflection.
        let body = bilinear_sheet(&[BOW_TIE], true);
        let count = |chordal: f64| {
            let tolerance = TessellationTolerance::new(ocs_geom::tessellation::DEFAULT_ANGLE, 1e-4)
                .with_chordal_deflection(chordal);
            let mesh = tessellate(&body, tolerance);
            assert!(mesh.missing_faces.is_empty());
            mesh.mesh.triangles.len()
        };
        let (fine, coarse) = (count(1e-4), count(5e-3));
        assert!(fine > coarse, "{fine} against {coarse} triangles");
    }

    /// Coordinates are hashed on a grid of this step, so the last-bit
    /// differences that `sin`, `cos`, `atan2` and `powf` show between
    /// platforms do not reach the hash. It is far coarser than that noise
    /// (about 1e-14 at these sizes), so a value sits near a rounding boundary
    /// only about once in 1e8, and far finer than any tolerance the tests mesh
    /// with (1e-4 and up).
    const FINGERPRINT_STEP: f64 = 1e-6;

    /// Triangle count and a hash of every vertex and normal, quantized to
    /// [`FINGERPRINT_STEP`], and every index, exactly.
    fn quantized_fingerprint(mesh: &Mesh) -> (usize, u64) {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |word: u64| {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        for point in mesh.positions.iter().chain(&mesh.normals) {
            point
                .iter()
                .for_each(|value| eat((value / FINGERPRINT_STEP).round() as i64 as u64));
        }
        for triangle in &mesh.triangles {
            triangle.iter().for_each(|index| eat(*index as u64));
        }
        (mesh.triangles.len(), hash)
    }

    /// For a mesh whose triangulation is not unique: triangle count, then a
    /// hash of the total area and the sorted set of distinct vertex positions,
    /// both quantized to [`FINGERPRINT_STEP`]. It still changes when the
    /// count, the covered area or the vertices change, but not when a
    /// cocircular tie is triangulated the other way.
    fn shape_fingerprint(mesh: &Mesh) -> (usize, u64) {
        let area: f64 = mesh
            .triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| mesh.positions[index]);
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let cross = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt()
            })
            .sum();
        let mut vertices: Vec<[i64; 3]> = mesh
            .positions
            .iter()
            .map(|point| point.map(|value| (value / FINGERPRINT_STEP).round() as i64))
            .collect();
        vertices.sort_unstable();
        vertices.dedup();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |word: u64| {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        eat((area / FINGERPRINT_STEP).round() as i64 as u64);
        for vertex in vertices {
            vertex.iter().for_each(|value| eat(*value as u64));
        }
        (mesh.triangles.len(), hash)
    }

    fn mesh_fingerprint(body: &Body) -> (usize, u64) {
        quantized_fingerprint(&tessellate(body, fold_tolerance()).mesh)
    }

    fn shape_of(body: &Body) -> (usize, u64) {
        shape_fingerprint(&tessellate(body, fold_tolerance()).mesh)
    }

    #[test]
    fn smooth_surfaces_mesh_exactly_as_before_the_fold_stop() {
        // A smooth surface never reverses its normal inside a piece, so the
        // fold stop leaves their meshes identical, vertex for vertex.
        let cylinder = crate::make::cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let torus = crate::make::torus([0.0; 3], 5.0, 1.0).unwrap();
        let saddle = [
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [0.0, 5.0, 0.0],
            [5.0, 5.0, 2.0],
        ];
        let saddle = bilinear_sheet(&[saddle], true);
        // A bicubic dome: straight rims, the inner control points raised.
        let dome: Vec<Vec<[f64; 3]>> = (0..4)
            .map(|u| {
                (0..4)
                    .map(|v| {
                        let inner = (1..3).contains(&u) && (1..3).contains(&v);
                        [
                            u as f64 * 5.0 / 3.0,
                            v as f64 * 5.0 / 3.0,
                            if inner { 1.5 } else { 0.0 },
                        ]
                    })
                    .collect()
            })
            .collect();
        let dome = spline_sheet(&[(3, dome)], true);
        // A ribbon twisted by about 170 degrees along its length: its normal
        // turns past a right angle along each long side, but smoothly, so
        // the edges there are still sampled by the angle limit.
        let ribbon = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 10.0],
            [-1.0, 0.2, 10.0],
        ];
        let ribbon = bilinear_sheet(&[ribbon], false);
        assert_eq!(
            [
                shape_of(&cylinder),
                mesh_fingerprint(&torus),
                mesh_fingerprint(&saddle),
                mesh_fingerprint(&dome),
                mesh_fingerprint(&ribbon)
            ],
            SMOOTH_MESHES
        );
    }

    /// Cylinder, torus, saddle, dome and ribbon, as meshed on upstream 003eee3
    /// before the fold stop, hashed by [`quantized_fingerprint`]. The cylinder
    /// is hashed by [`shape_fingerprint`] instead: its flat caps are cocircular
    /// points, so their triangulation is not unique and the tie breaks per
    /// platform.
    const SMOOTH_MESHES: [(usize, u64); 5] = [
        (508, 6_030_817_377_226_346_326),
        (23_762, 3_222_715_392_039_835_452),
        (98, 10_157_540_559_343_846_255),
        (984, 12_070_472_126_132_433_869),
        (537, 5_912_244_891_144_382_325),
    ];
}

#[cfg(test)]
mod crossing_planar_ring_tests {
    use super::*;
    use crate::geometry::{Curve3, Line3, Surface};
    use crate::{Coedge, Edge, Face, Loop, Lump, Provenance, Shell, Vertex};
    use ocs_geom::space::Plane;

    /// One planar face in z = 0 bounded by straight edges through `corners`,
    /// in order, closed back to the first.
    fn planar_sheet(corners: &[[f64; 2]]) -> Body {
        let made = Provenance::Synthesized;
        let mut body = Body::new();
        let lump = body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: made.clone(),
        });
        body.roots.push(lump);
        let shell = body.shells.insert(Shell {
            faces: Vec::new(),
            owner: lump,
            provenance: made.clone(),
        });
        body.lumps.get_mut(lump).unwrap().shells.push(shell);
        let surface = body.surfaces.insert(Surface::Plane(Plane {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }));
        let face = body.faces.insert(Face {
            surface,
            forward: true,
            loops: Vec::new(),
            owner: shell,
            provenance: made.clone(),
        });
        body.shells.get_mut(shell).unwrap().faces.push(face);
        let ring = body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: made.clone(),
        });
        body.faces.get_mut(face).unwrap().loops.push(ring);
        let vertices: Vec<_> = corners
            .iter()
            .map(|c| {
                body.vertices.insert(Vertex {
                    point: [c[0], c[1], 0.0],
                    provenance: made.clone(),
                })
            })
            .collect();
        let n = corners.len();
        for i in 0..n {
            let (a, b) = (corners[i], corners[(i + 1) % n]);
            let curve = body.curves.insert(Curve3::Line(Line3 {
                origin: [a[0], a[1], 0.0],
                direction: [b[0] - a[0], b[1] - a[1], 0.0],
            }));
            let edge = body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: 1.0,
                start: vertices[i],
                end: vertices[(i + 1) % n],
                coedges: Vec::new(),
                provenance: made.clone(),
            });
            let coedge = body.coedges.insert(Coedge {
                edge,
                forward: true,
                pcurve: None,
                owner: ring,
                provenance: made.clone(),
            });
            body.edges.get_mut(edge).unwrap().coedges.push(coedge);
            body.loops.get_mut(ring).unwrap().coedges.push(coedge);
        }
        body
    }

    /// The constrained fill of the sheet's face over its corner loop: the step
    /// a refused boundary used to leave the face missing at.
    fn fill(corners: &[[f64; 2]]) -> Option<Mesh> {
        let body = planar_sheet(corners);
        let face = body.face_keys().next().unwrap();
        let surface = body.surfaces.get(body.faces.get(face).unwrap().surface).unwrap();
        let ring: Vec<BoundaryPoint> = corners
            .iter()
            .map(|c| BoundaryPoint {
                parameters: *c,
                position: [c[0], c[1], 0.0],
            })
            .collect();
        fill_scheduled(&body, face, surface, &[ring], ocs_geom::tessellation::DEFAULT_ANGLE, 1e-3)
    }

    fn in_triangle(p: [f64; 2], t: [[f64; 2]; 3]) -> bool {
        let side = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let s = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
        s.iter().all(|v| *v >= -1e-9) || s.iter().all(|v| *v <= 1e-9)
    }

    /// Every triangle's centre lies in one of `lobes`.
    fn within(mesh: &Mesh, lobes: &[[[f64; 2]; 3]]) -> bool {
        mesh.triangles.iter().all(|t| {
            let [a, b, c] = t.map(|i| mesh.positions[i]);
            let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            lobes.iter().any(|lobe| in_triangle(centre, *lobe))
        })
    }

    fn area(mesh: &Mesh) -> f64 {
        mesh.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| Vec3::from(mesh.positions[i]));
                0.5 * (b - a).cross(c - a).length()
            })
            .sum()
    }

    #[test]
    fn a_bow_tie_planar_face_is_meshed_as_its_two_lobes() {
        // Edges (0,0)-(4,4) and (4,0)-(0,2) cross at (4/3, 4/3): a right lobe of
        // area 48/9 and a left one of 12/9; nothing between them.
        let mesh = fill(&[[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 2.0]]).expect("drawn");
        assert!((area(&mesh) - 60.0 / 9.0).abs() < 1e-9, "area {}", area(&mesh));
        let x = [4.0 / 3.0, 4.0 / 3.0];
        assert!(within(&mesh, &[[x, [4.0, 4.0], [4.0, 0.0]], [x, [0.0, 2.0], [0.0, 0.0]]]));
    }

    #[test]
    fn a_planar_face_whose_corner_lies_on_another_edge_is_meshed_as_its_two_sides() {
        // The corner (2, -1e-10) lies on the bottom edge (0,0)-(4,0), just
        // across it: a right triangle of area 3 and a left quadrilateral of 3.5.
        let corners = [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [2.0, -1e-10], [1.0, 2.0], [0.0, 3.0]];
        let mesh = fill(&corners).expect("drawn");
        assert!((area(&mesh) - 6.5).abs() < 1e-6, "area {}", area(&mesh));
        let c = [2.0, 0.0];
        assert!(within(
            &mesh,
            &[[c, [4.0, 0.0], [4.0, 3.0]], [[0.0, 0.0], c, [1.0, 2.0]], [[0.0, 0.0], [1.0, 2.0], [0.0, 3.0]]]
        ));
    }

    #[test]
    fn a_planar_face_with_a_needle_back_along_an_edge_is_meshed_without_it() {
        // Up the right edge to (4,4) and straight back down to (4+1e-10, 2): a
        // needle with no area. The face is (0,0) (4,0) (4,2) (2,3) (0,3).
        let corners = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [4.0 + 1e-10, 2.0], [2.0, 3.0], [0.0, 3.0]];
        let mesh = fill(&corners).expect("drawn");
        assert!((area(&mesh) - 11.0).abs() < 1e-6, "area {}", area(&mesh));
        assert!(within(
            &mesh,
            &[[[0.0, 0.0], [4.0, 0.0], [4.0, 2.0]], [[0.0, 0.0], [4.0, 2.0], [2.0, 3.0]], [[0.0, 0.0], [2.0, 3.0], [0.0, 3.0]]]
        ));
    }

    #[test]
    fn faces_the_triangulation_accepts_or_cannot_split_are_as_before() {
        // A plain square: two triangles, area 4.
        let square = fill(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]).expect("drawn");
        assert_eq!(square.triangles.len(), 2);
        assert!((area(&square) - 4.0).abs() < 1e-9);
        // A star with five crossings: more than one contact stays refused.
        let star: Vec<[f64; 2]> = (0..5)
            .map(|k| {
                let angle = std::f64::consts::FRAC_PI_2 + k as f64 * 4.0 * std::f64::consts::PI / 5.0;
                [angle.cos(), angle.sin()]
            })
            .collect();
        assert!(fill(&star).is_none());
        // A symmetric bow-tie: its lobes cancel, so it is a hairline face by the
        // mesh's own rule, and it stays left out as before.
        let symmetric = [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]];
        let sheet = planar_sheet(&symmetric);
        assert!(hairline_face(&sheet, sheet.face_keys().next().unwrap()));
        assert!(fill(&symmetric).is_none());
    }
}
