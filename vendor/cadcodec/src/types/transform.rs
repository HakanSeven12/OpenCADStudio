//! Transformation types for geometric operations
//!
//! Provides transform matrices and operations for rotating, scaling,
//! and translating CAD entities.

use crate::types::{Vector2, Vector3};
use std::ops::Mul;

/// 3x3 matrix for 2D transformations and rotations
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Matrix3 {
    /// Matrix elements stored in row-major order
    pub m: [[f64; 3]; 3],
}

impl Matrix3 {
    /// Create identity matrix
    pub fn identity() -> Self {
        Self {
            m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Create zero matrix
    pub fn zero() -> Self {
        Self { m: [[0.0; 3]; 3] }
    }

    /// Create matrix from rows
    pub fn from_rows(row0: [f64; 3], row1: [f64; 3], row2: [f64; 3]) -> Self {
        Self {
            m: [row0, row1, row2],
        }
    }

    /// Create rotation matrix around Z axis
    pub fn rotation_z(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m: [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Create scaling matrix
    pub fn scaling(sx: f64, sy: f64, sz: f64) -> Self {
        Self {
            m: [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, sz]],
        }
    }

    /// Create arbitrary axis matrix for OCS to WCS conversion
    ///
    /// Implements the AutoCAD arbitrary axis algorithm
    pub fn arbitrary_axis(normal: Vector3) -> Self {
        const ARBITRARY_AXIS_THRESHOLD: f64 = 1.0 / 64.0;

        let normal = normal.normalize();

        // Choose reference axis (Ax) based on normal direction
        let ax = if normal.x.abs() < ARBITRARY_AXIS_THRESHOLD
            && normal.y.abs() < ARBITRARY_AXIS_THRESHOLD
        {
            Vector3::new(0.0, 1.0, 0.0) // Use Y axis
        } else {
            Vector3::new(0.0, 0.0, 1.0) // Use Z axis
        };

        // Calculate X direction (Ax × N normalized)
        let x_dir = ax.cross(&normal).normalize();

        // Calculate Y direction (N × X normalized)
        let y_dir = normal.cross(&x_dir).normalize();

        Self {
            m: [
                [x_dir.x, y_dir.x, normal.x],
                [x_dir.y, y_dir.y, normal.y],
                [x_dir.z, y_dir.z, normal.z],
            ],
        }
    }

    /// Transpose the matrix
    pub fn transpose(&self) -> Self {
        Self {
            m: [
                [self.m[0][0], self.m[1][0], self.m[2][0]],
                [self.m[0][1], self.m[1][1], self.m[2][1]],
                [self.m[0][2], self.m[1][2], self.m[2][2]],
            ],
        }
    }

    /// Calculate determinant
    pub fn determinant(&self) -> f64 {
        self.m[0][0] * (self.m[1][1] * self.m[2][2] - self.m[1][2] * self.m[2][1])
            - self.m[0][1] * (self.m[1][0] * self.m[2][2] - self.m[1][2] * self.m[2][0])
            + self.m[0][2] * (self.m[1][0] * self.m[2][1] - self.m[1][1] * self.m[2][0])
    }

    /// Invert the matrix (returns None if singular)
    pub fn inverse(&self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;

        Some(Self {
            m: [
                [
                    (self.m[1][1] * self.m[2][2] - self.m[1][2] * self.m[2][1]) * inv_det,
                    (self.m[0][2] * self.m[2][1] - self.m[0][1] * self.m[2][2]) * inv_det,
                    (self.m[0][1] * self.m[1][2] - self.m[0][2] * self.m[1][1]) * inv_det,
                ],
                [
                    (self.m[1][2] * self.m[2][0] - self.m[1][0] * self.m[2][2]) * inv_det,
                    (self.m[0][0] * self.m[2][2] - self.m[0][2] * self.m[2][0]) * inv_det,
                    (self.m[0][2] * self.m[1][0] - self.m[0][0] * self.m[1][2]) * inv_det,
                ],
                [
                    (self.m[1][0] * self.m[2][1] - self.m[1][1] * self.m[2][0]) * inv_det,
                    (self.m[0][1] * self.m[2][0] - self.m[0][0] * self.m[2][1]) * inv_det,
                    (self.m[0][0] * self.m[1][1] - self.m[0][1] * self.m[1][0]) * inv_det,
                ],
            ],
        })
    }

    /// Transform a Vector3
    pub fn transform_point(&self, v: Vector3) -> Vector3 {
        Vector3::new(
            self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z,
            self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z,
            self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z,
        )
    }
}

impl Mul for Matrix3 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut result = Self::zero();
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    result.m[i][j] += self.m[i][k] * rhs.m[k][j];
                }
            }
        }
        result
    }
}

impl Mul<Vector3> for Matrix3 {
    type Output = Vector3;

    fn mul(self, v: Vector3) -> Self::Output {
        self.transform_point(v)
    }
}

impl Default for Matrix3 {
    fn default() -> Self {
        Self::identity()
    }
}

/// 4x4 transformation matrix for 3D operations
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Matrix4 {
    /// Matrix elements stored in row-major order
    pub m: [[f64; 4]; 4],
}

impl Matrix4 {
    /// Create identity matrix
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create zero matrix
    pub fn zero() -> Self {
        Self { m: [[0.0; 4]; 4] }
    }

    /// Create translation matrix
    pub fn translation(tx: f64, ty: f64, tz: f64) -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, tx],
                [0.0, 1.0, 0.0, ty],
                [0.0, 0.0, 1.0, tz],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create scaling matrix
    pub fn scaling(sx: f64, sy: f64, sz: f64) -> Self {
        Self {
            m: [
                [sx, 0.0, 0.0, 0.0],
                [0.0, sy, 0.0, 0.0],
                [0.0, 0.0, sz, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create scaling matrix with origin
    pub fn scaling_with_origin(sx: f64, sy: f64, sz: f64, origin: Vector3) -> Self {
        // Translate to origin, scale, translate back
        Self::translation(origin.x, origin.y, origin.z)
            * Self::scaling(sx, sy, sz)
            * Self::translation(-origin.x, -origin.y, -origin.z)
    }

    /// Create rotation matrix around X axis
    pub fn rotation_x(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, cos, -sin, 0.0],
                [0.0, sin, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create rotation matrix around Y axis
    pub fn rotation_y(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m: [
                [cos, 0.0, sin, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [-sin, 0.0, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create rotation matrix around Z axis
    pub fn rotation_z(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m: [
                [cos, -sin, 0.0, 0.0],
                [sin, cos, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create rotation matrix around arbitrary axis (Rodrigues' rotation formula)
    pub fn rotation(axis: Vector3, angle: f64) -> Self {
        let axis = axis.normalize();
        let cos = angle.cos();
        let sin = angle.sin();
        let one_minus_cos = 1.0 - cos;

        let x = axis.x;
        let y = axis.y;
        let z = axis.z;

        Self {
            m: [
                [
                    cos + x * x * one_minus_cos,
                    x * y * one_minus_cos - z * sin,
                    x * z * one_minus_cos + y * sin,
                    0.0,
                ],
                [
                    y * x * one_minus_cos + z * sin,
                    cos + y * y * one_minus_cos,
                    y * z * one_minus_cos - x * sin,
                    0.0,
                ],
                [
                    z * x * one_minus_cos - y * sin,
                    z * y * one_minus_cos + x * sin,
                    cos + z * z * one_minus_cos,
                    0.0,
                ],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Create matrix from Matrix3 (3x3 rotation/scale portion)
    pub fn from_matrix3(m3: Matrix3) -> Self {
        Self {
            m: [
                [m3.m[0][0], m3.m[0][1], m3.m[0][2], 0.0],
                [m3.m[1][0], m3.m[1][1], m3.m[1][2], 0.0],
                [m3.m[2][0], m3.m[2][1], m3.m[2][2], 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Get the 3x3 rotation/scale portion
    pub fn to_matrix3(&self) -> Matrix3 {
        Matrix3 {
            m: [
                [self.m[0][0], self.m[0][1], self.m[0][2]],
                [self.m[1][0], self.m[1][1], self.m[1][2]],
                [self.m[2][0], self.m[2][1], self.m[2][2]],
            ],
        }
    }

    /// Transpose the matrix
    pub fn transpose(&self) -> Self {
        let mut result = Self::zero();
        for i in 0..4 {
            for j in 0..4 {
                result.m[i][j] = self.m[j][i];
            }
        }
        result
    }

    /// Transform a point (applies full transformation including translation)
    pub fn transform_point(&self, v: Vector3) -> Vector3 {
        let w = self.m[3][0] * v.x + self.m[3][1] * v.y + self.m[3][2] * v.z + self.m[3][3];
        let w = if w.abs() < 1e-10 { 1.0 } else { w };

        Vector3::new(
            (self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z + self.m[0][3]) / w,
            (self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z + self.m[1][3]) / w,
            (self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z + self.m[2][3]) / w,
        )
    }

    /// Transform a direction vector (ignores translation)
    pub fn transform_direction(&self, v: Vector3) -> Vector3 {
        Vector3::new(
            self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z,
            self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z,
            self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z,
        )
    }
}

impl Mul for Matrix4 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut result = Self::zero();
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    result.m[i][j] += self.m[i][k] * rhs.m[k][j];
                }
            }
        }
        result
    }
}

impl Default for Matrix4 {
    fn default() -> Self {
        Self::identity()
    }
}

/// Transform structure combining rotation, scaling, and translation
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Transform {
    /// The 4x4 transformation matrix
    pub matrix: Matrix4,
}

impl Transform {
    /// Create identity transform
    pub fn identity() -> Self {
        Self {
            matrix: Matrix4::identity(),
        }
    }

    /// Create transform from matrix
    pub fn from_matrix(matrix: Matrix4) -> Self {
        Self { matrix }
    }

    /// Create rotation transform around arbitrary axis
    pub fn from_rotation(axis: Vector3, angle: f64) -> Self {
        Self {
            matrix: Matrix4::rotation(axis, angle),
        }
    }

    /// Create translation transform
    pub fn from_translation(translation: Vector3) -> Self {
        Self {
            matrix: Matrix4::translation(translation.x, translation.y, translation.z),
        }
    }

    /// Create uniform scaling transform
    pub fn from_scale(scale: f64) -> Self {
        Self {
            matrix: Matrix4::scaling(scale, scale, scale),
        }
    }

    /// Create non-uniform scaling transform
    pub fn from_scaling(scale: Vector3) -> Self {
        Self {
            matrix: Matrix4::scaling(scale.x, scale.y, scale.z),
        }
    }

    /// Create scaling transform with origin
    pub fn from_scaling_with_origin(scale: Vector3, origin: Vector3) -> Self {
        Self {
            matrix: Matrix4::scaling_with_origin(scale.x, scale.y, scale.z, origin),
        }
    }

    /// Apply transform to a point
    pub fn apply(&self, point: Vector3) -> Vector3 {
        self.matrix.transform_point(point)
    }

    /// Apply only the rotation portion
    pub fn apply_rotation(&self, direction: Vector3) -> Vector3 {
        self.matrix.transform_direction(direction)
    }

    /// Combine with another transform (this transform applied first)
    pub fn then(&self, other: &Transform) -> Transform {
        Transform {
            matrix: other.matrix * self.matrix,
        }
    }

    /// Combine with another transform (other transform applied first)
    pub fn compose(&self, other: &Transform) -> Transform {
        Transform {
            matrix: self.matrix * other.matrix,
        }
    }

    /// Create a mirror transform across the YZ plane (negate X)
    pub fn from_mirror_x() -> Self {
        Self {
            matrix: Matrix4::scaling(-1.0, 1.0, 1.0),
        }
    }

    /// Create a mirror transform across the XZ plane (negate Y)
    pub fn from_mirror_y() -> Self {
        Self {
            matrix: Matrix4::scaling(1.0, -1.0, 1.0),
        }
    }

    /// Create a mirror transform across the XY plane (negate Z)
    pub fn from_mirror_z() -> Self {
        Self {
            matrix: Matrix4::scaling(1.0, 1.0, -1.0),
        }
    }

    /// Create a mirror transform across an arbitrary plane defined by a point and normal
    ///
    /// The plane passes through `point` with the given `normal` direction.
    /// Uses the Householder reflection: P' = P - 2(n·P + d)n
    pub fn from_mirror_plane(point: Vector3, normal: Vector3) -> Self {
        let n = normal.normalize();
        let d = -(n.x * point.x + n.y * point.y + n.z * point.z);

        Self {
            matrix: Matrix4 {
                m: [
                    [
                        1.0 - 2.0 * n.x * n.x,
                        -2.0 * n.x * n.y,
                        -2.0 * n.x * n.z,
                        -2.0 * n.x * d,
                    ],
                    [
                        -2.0 * n.y * n.x,
                        1.0 - 2.0 * n.y * n.y,
                        -2.0 * n.y * n.z,
                        -2.0 * n.y * d,
                    ],
                    [
                        -2.0 * n.z * n.x,
                        -2.0 * n.z * n.y,
                        1.0 - 2.0 * n.z * n.z,
                        -2.0 * n.z * d,
                    ],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        }
    }

    /// Create a mirror transform across a line defined by two points (in the XY plane)
    ///
    /// The mirror line passes through `p1` and `p2`.
    pub fn from_mirror_line(p1: Vector3, p2: Vector3) -> Self {
        let dir = (p2 - p1).normalize();
        // Normal to the line in the XY plane
        let normal = Vector3::new(-dir.y, dir.x, 0.0);
        Self::from_mirror_plane(p1, normal)
    }

    /// Check if transform is identity
    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Mul for Transform {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.compose(&rhs)
    }
}

/// Helper for 2D rotation (utility function)
pub fn rotate_point_2d(point: Vector2, center: Vector2, angle: f64) -> Vector2 {
    let cos = angle.cos();
    let sin = angle.sin();
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    Vector2::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    )
}

/// Helper to check if angle is effectively zero
pub fn is_zero_angle(angle: f64) -> bool {
    angle.abs() < 1e-10
}

/// Normalize an angle to the range [0, 2π)
pub fn normalize_angle(angle: f64) -> f64 {
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut a = angle % two_pi;
    if a < 0.0 {
        a += two_pi;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn test_matrix3_identity() {
        let m = Matrix3::identity();
        let v = Vector3::new(1.0, 2.0, 3.0);
        let result = m * v;
        assert!((result.x - v.x).abs() < 1e-10);
        assert!((result.y - v.y).abs() < 1e-10);
        assert!((result.z - v.z).abs() < 1e-10);
    }

    #[test]
    fn test_matrix3_rotation_z() {
        let m = Matrix3::rotation_z(PI / 2.0);
        let v = Vector3::new(1.0, 0.0, 0.0);
        let result = m * v;
        assert!(result.x.abs() < 1e-10);
        assert!((result.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_matrix4_translation() {
        let m = Matrix4::translation(1.0, 2.0, 3.0);
        let v = Vector3::new(0.0, 0.0, 0.0);
        let result = m.transform_point(v);
        assert!((result.x - 1.0).abs() < 1e-10);
        assert!((result.y - 2.0).abs() < 1e-10);
        assert!((result.z - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_matrix4_rotation() {
        let m = Matrix4::rotation(Vector3::new(0.0, 0.0, 1.0), PI / 2.0);
        let v = Vector3::new(1.0, 0.0, 0.0);
        let result = m.transform_point(v);
        assert!(result.x.abs() < 1e-10);
        assert!((result.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_transform_composition() {
        let t1 = Transform::from_translation(Vector3::new(1.0, 0.0, 0.0));
        let t2 = Transform::from_translation(Vector3::new(0.0, 1.0, 0.0));
        let combined = t1.then(&t2);

        let origin = Vector3::new(0.0, 0.0, 0.0);
        let result = combined.apply(origin);

        assert!((result.x - 1.0).abs() < 1e-10);
        assert!((result.y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_arbitrary_axis() {
        // Test with standard Z normal
        let m = Matrix3::arbitrary_axis(Vector3::new(0.0, 0.0, 1.0));
        let det = m.determinant();
        assert!((det - 1.0).abs() < 1e-10); // Should be orthonormal
    }

    #[test]
    fn test_mirror_x() {
        let t = Transform::from_mirror_x();
        let p = Vector3::new(3.0, 5.0, 7.0);
        let r = t.apply(p);
        assert!((r.x - (-3.0)).abs() < 1e-10);
        assert!((r.y - 5.0).abs() < 1e-10);
        assert!((r.z - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_y() {
        let t = Transform::from_mirror_y();
        let p = Vector3::new(3.0, 5.0, 7.0);
        let r = t.apply(p);
        assert!((r.x - 3.0).abs() < 1e-10);
        assert!((r.y - (-5.0)).abs() < 1e-10);
        assert!((r.z - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_z() {
        let t = Transform::from_mirror_z();
        let p = Vector3::new(3.0, 5.0, 7.0);
        let r = t.apply(p);
        assert!((r.x - 3.0).abs() < 1e-10);
        assert!((r.y - 5.0).abs() < 1e-10);
        assert!((r.z - (-7.0)).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_plane_through_origin() {
        // Mirror across YZ plane through origin = same as mirror_x
        let t = Transform::from_mirror_plane(Vector3::ZERO, Vector3::UNIT_X);
        let p = Vector3::new(3.0, 5.0, 7.0);
        let r = t.apply(p);
        assert!((r.x - (-3.0)).abs() < 1e-10);
        assert!((r.y - 5.0).abs() < 1e-10);
        assert!((r.z - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_plane_offset() {
        // Mirror across plane x=2 (normal X, point (2,0,0))
        let t = Transform::from_mirror_plane(Vector3::new(2.0, 0.0, 0.0), Vector3::UNIT_X);
        let p = Vector3::new(5.0, 3.0, 1.0);
        let r = t.apply(p);
        assert!((r.x - (-1.0)).abs() < 1e-10); // 2 - (5-2) = -1
        assert!((r.y - 3.0).abs() < 1e-10);
        assert!((r.z - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_line() {
        // Mirror across the X axis (line from (0,0,0) to (1,0,0))
        let t =
            Transform::from_mirror_line(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        let p = Vector3::new(3.0, 5.0, 0.0);
        let r = t.apply(p);
        assert!((r.x - 3.0).abs() < 1e-10);
        assert!((r.y - (-5.0)).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_line_diagonal() {
        // Mirror across the line y=x (45-degree line through origin)
        let t = Transform::from_mirror_line(Vector3::ZERO, Vector3::new(1.0, 1.0, 0.0));
        let p = Vector3::new(3.0, 0.0, 0.0);
        let r = t.apply(p);
        // Point (3,0) mirrored across y=x should give (0,3)
        assert!((r.x - 0.0).abs() < 1e-10);
        assert!((r.y - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_mirror_is_involution() {
        // Applying a mirror twice should return the original point
        let t =
            Transform::from_mirror_plane(Vector3::new(1.0, 2.0, 3.0), Vector3::new(1.0, 1.0, 0.0));
        let p = Vector3::new(7.0, -2.0, 4.0);
        let r = t.apply(t.apply(p));
        assert!((r.x - p.x).abs() < 1e-10);
        assert!((r.y - p.y).abs() < 1e-10);
        assert!((r.z - p.z).abs() < 1e-10);
    }

    #[test]
    fn test_normalize_angle() {
        assert!((normalize_angle(0.0)).abs() < 1e-10);
        assert!((normalize_angle(PI) - PI).abs() < 1e-10);
        assert!((normalize_angle(-PI / 2.0) - 3.0 * PI / 2.0).abs() < 1e-10);
        assert!((normalize_angle(3.0 * PI) - PI).abs() < 1e-10);
    }
}
