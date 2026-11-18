//! Mass properties as MASSPROP reports them: the volume, the centroid, the moments and
//! products of inertia about the origin, the principal moments and directions about the
//! centroid, and the radii of gyration. From Solidcore's measurement of a body, or from a
//! closed triangle mesh.

/// A body's mass properties (unit density).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub centroid: [f64; 3],
    /// Ixx, Iyy, Izz about the origin.
    pub moment_of_inertia: [f64; 3],
    /// The principal directions, one per row, by increasing moment.
    pub principal_directions: [f64; 9],
    /// The principal moments about the centroid, increasing.
    pub principal_moments: [f64; 3],
    /// Ixy, Iyz, Izx about the origin.
    pub product_of_inertia: [f64; 3],
    pub radii_of_gyration: [f64; 3],
}

impl MassProperties {
    /// The properties of `volume` with its `centroid` and its inertia tensor about the
    /// centroid (row-major, off-diagonal terms the negated products).
    pub fn from_central(volume: f64, centroid: [f64; 3], central: [[f64; 3]; 3]) -> Option<Self> {
        if !(volume.is_finite() && volume > 0.0) {
            return None;
        }
        let (principal_moments, principal_directions) = principal_axes(central);
        let c2: f64 = centroid.iter().map(|c| c * c).sum();
        let mut origin = central;
        for row in 0..3 {
            for column in 0..3 {
                origin[row][column] += volume * (if row == column { c2 } else { 0.0 } - centroid[row] * centroid[column]);
            }
        }
        let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
        let product_of_inertia = [origin[0][1], origin[1][2], origin[2][0]];
        let radii_of_gyration = moment_of_inertia.map(|value| (value.max(0.0) / volume).sqrt());
        let out = Self {
            volume,
            centroid,
            moment_of_inertia,
            principal_directions,
            principal_moments,
            product_of_inertia,
            radii_of_gyration,
        };
        out.is_finite().then_some(out)
    }

    /// Solidcore's measurement of a body.
    pub fn from_estimate(estimate: &solidcore_ops::measure::MassPropertiesEstimate) -> Self {
        Self::from_central(estimate.volume, estimate.centroid, estimate.tensor).unwrap_or(Self {
            volume: estimate.volume,
            centroid: estimate.centroid,
            ..Self::default()
        })
    }

    /// The properties of a closed, consistently wound triangle mesh, about its own middle so
    /// survey coordinates stay well conditioned.
    pub fn of_mesh(positions: &[[f64; 3]], triangles: &[[usize; 3]]) -> Option<Self> {
        positions.first()?;
        let count = positions.len() as f64;
        let reference = [0, 1, 2].map(|k| positions.iter().map(|p| p[k]).sum::<f64>() / count);
        let mut signed_volume = 0.0;
        let mut centroid_numerator = [0.0; 3];
        let mut squared = [0.0; 3];
        let mut products = [0.0; 3];
        for triangle in triangles {
            let corner = |i: usize| -> Option<[f64; 3]> {
                let p = positions.get(triangle[i])?;
                Some([p[0] - reference[0], p[1] - reference[1], p[2] - reference[2]])
            };
            let points = [corner(0)?, corner(1)?, corner(2)?];
            if points.iter().flatten().any(|v| !v.is_finite()) {
                return None;
            }
            let [a, b, c] = points;
            let tetra = (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
            signed_volume += tetra;
            for axis in 0..3 {
                let [a, b, c] = points.map(|p| p[axis]);
                centroid_numerator[axis] += tetra * (a + b + c) / 4.0;
                squared[axis] += tetra * (a * a + b * b + c * c + a * b + a * c + b * c) / 10.0;
            }
            for (index, (first, second)) in [(0usize, 1usize), (1, 2), (2, 0)].into_iter().enumerate() {
                let diagonal: f64 = points.iter().map(|p| p[first] * p[second]).sum();
                let all = points.iter().map(|p| p[first]).sum::<f64>() * points.iter().map(|p| p[second]).sum::<f64>();
                products[index] += tetra * (diagonal + all) / 20.0;
            }
        }
        if signed_volume == 0.0 || !signed_volume.is_finite() {
            return None;
        }
        let local = centroid_numerator.map(|v| v / signed_volume);
        let orientation = signed_volume.signum();
        let volume = signed_volume.abs();
        let square = squared.map(|v| v * orientation);
        let products = products.map(|v| -v * orientation);
        let moment = [square[1] + square[2], square[0] + square[2], square[0] + square[1]];
        let central = [
            [
                moment[0] - volume * (local[1].powi(2) + local[2].powi(2)),
                products[0] + volume * local[0] * local[1],
                products[2] + volume * local[0] * local[2],
            ],
            [
                products[0] + volume * local[0] * local[1],
                moment[1] - volume * (local[0].powi(2) + local[2].powi(2)),
                products[1] + volume * local[1] * local[2],
            ],
            [
                products[2] + volume * local[0] * local[2],
                products[1] + volume * local[1] * local[2],
                moment[2] - volume * (local[0].powi(2) + local[1].powi(2)),
            ],
        ];
        Self::from_central(volume, [0, 1, 2].map(|k| reference[k] + local[k]), central)
    }

    fn is_finite(&self) -> bool {
        self.centroid
            .iter()
            .chain(&self.moment_of_inertia)
            .chain(&self.principal_moments)
            .chain(&self.product_of_inertia)
            .chain(&self.radii_of_gyration)
            .all(|v| v.is_finite())
    }

    /// The same body moved by `delta`.
    pub fn translated(mut self, delta: [f64; 3]) -> Self {
        let old = self.centroid;
        let square_change = |axis: usize| delta[axis] * (2.0 * old[axis] + delta[axis]);
        self.moment_of_inertia[0] += self.volume * (square_change(1) + square_change(2));
        self.moment_of_inertia[1] += self.volume * (square_change(0) + square_change(2));
        self.moment_of_inertia[2] += self.volume * (square_change(0) + square_change(1));
        let product_change = |first: usize, second: usize| old[first] * delta[second] + old[second] * delta[first] + delta[first] * delta[second];
        self.product_of_inertia[0] -= self.volume * product_change(0, 1);
        self.product_of_inertia[1] -= self.volume * product_change(1, 2);
        self.product_of_inertia[2] -= self.volume * product_change(2, 0);
        self.radii_of_gyration = self.moment_of_inertia.map(|m| (m.max(0.0) / self.volume).sqrt());
        self.centroid = [0, 1, 2].map(|k| old[k] + delta[k]);
        self
    }
}

/// The area and area-weighted centroid of a mesh's triangles.
pub fn surface_properties(positions: &[[f64; 3]], triangles: &[[usize; 3]]) -> Option<(f64, [f64; 3])> {
    let reference = *positions.first()?;
    let mut moment = [0.0; 3];
    let mut area = 0.0;
    for triangle in triangles {
        let corner = |i: usize| -> Option<[f64; 3]> {
            let p = positions.get(triangle[i])?;
            Some([p[0] - reference[0], p[1] - reference[1], p[2] - reference[2]])
        };
        let [a, b, c] = [corner(0)?, corner(1)?, corner(2)?];
        let (u, v) = ([0, 1, 2].map(|k| b[k] - a[k]), [0, 1, 2].map(|k| c[k] - a[k]));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let piece = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
        if !piece.is_finite() {
            return None;
        }
        for k in 0..3 {
            moment[k] += (a[k] + b[k] + c[k]) * piece / 3.0;
        }
        area += piece;
    }
    (area > 0.0).then(|| (area, [0, 1, 2].map(|k| reference[k] + moment[k] / area)))
}

/// The eigenvalues of a symmetric 3×3 (increasing) and their unit eigenvectors as rows, by
/// Jacobi rotations; the world axes for an isotropic tensor.
fn principal_axes(mut matrix: [[f64; 3]; 3]) -> ([f64; 3], [f64; 9]) {
    let mut vectors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..24 {
        let (p, q) = [(0usize, 1usize), (0, 2), (1, 2)]
            .into_iter()
            .max_by(|&(ap, aq), &(bp, bq)| matrix[ap][aq].abs().total_cmp(&matrix[bp][bq].abs()))
            .unwrap_or((0, 1));
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
    let mut axes: Vec<(f64, [f64; 3])> = (0..3)
        .map(|axis| (matrix[axis][axis].max(0.0), [vectors[0][axis], vectors[1][axis], vectors[2][axis]]))
        .collect();
    axes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let moments = [axes[0].0, axes[1].0, axes[2].0];
    let directions = if (moments[2] - moments[0]).abs() <= moments[2].abs().max(1.0) * 1e-10 {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    } else {
        let [a, b, c] = [axes[0].1, axes[1].1, axes[2].1];
        [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]
    };
    (moments, directions)
}
