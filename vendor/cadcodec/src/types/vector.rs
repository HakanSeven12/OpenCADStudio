//! Vector types for geometric operations

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// 2D vector
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vector2 {
    pub x: f64,
    pub y: f64,
}

impl Vector2 {
    /// Create a new 2D vector
    pub const fn new(x: f64, y: f64) -> Self {
        Vector2 { x, y }
    }

    /// Zero vector
    pub const ZERO: Vector2 = Vector2::new(0.0, 0.0);

    /// Unit X vector
    pub const UNIT_X: Vector2 = Vector2::new(1.0, 0.0);

    /// Unit Y vector
    pub const UNIT_Y: Vector2 = Vector2::new(0.0, 1.0);

    /// Calculate the length (magnitude) of the vector
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Calculate the squared length (avoids sqrt for performance)
    pub fn length_squared(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Normalize the vector (make it unit length)
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0 {
            Vector2::new(self.x / len, self.y / len)
        } else {
            *self
        }
    }

    /// Dot product
    pub fn dot(&self, other: &Vector2) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Cross product (returns scalar for 2D)
    pub fn cross(&self, other: &Vector2) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// Distance to another point
    pub fn distance(&self, other: &Vector2) -> f64 {
        (*self - *other).length()
    }
}

impl Default for Vector2 {
    fn default() -> Self {
        Vector2::ZERO
    }
}

impl Add for Vector2 {
    type Output = Vector2;
    fn add(self, other: Vector2) -> Vector2 {
        Vector2::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Vector2 {
    type Output = Vector2;
    fn sub(self, other: Vector2) -> Vector2 {
        Vector2::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Vector2 {
    type Output = Vector2;
    fn mul(self, scalar: f64) -> Vector2 {
        Vector2::new(self.x * scalar, self.y * scalar)
    }
}

impl Div<f64> for Vector2 {
    type Output = Vector2;
    fn div(self, scalar: f64) -> Vector2 {
        Vector2::new(self.x / scalar, self.y / scalar)
    }
}

impl Neg for Vector2 {
    type Output = Vector2;
    fn neg(self) -> Vector2 {
        Vector2::new(-self.x, -self.y)
    }
}

impl fmt::Display for Vector2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// 3D vector
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    /// Create a new 3D vector
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    /// Create a zero vector
    pub const fn zero() -> Self {
        Vector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Zero vector
    pub const ZERO: Vector3 = Vector3::new(0.0, 0.0, 0.0);

    /// Unit X vector
    pub const UNIT_X: Vector3 = Vector3::new(1.0, 0.0, 0.0);

    /// Unit Y vector
    pub const UNIT_Y: Vector3 = Vector3::new(0.0, 1.0, 0.0);

    /// Unit Z vector
    pub const UNIT_Z: Vector3 = Vector3::new(0.0, 0.0, 1.0);

    /// Calculate the length (magnitude) of the vector
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Calculate the squared length (avoids sqrt for performance)
    pub fn length_squared(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Normalize the vector (make it unit length)
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0 {
            Vector3::new(self.x / len, self.y / len, self.z / len)
        } else {
            *self
        }
    }

    /// Dot product
    pub fn dot(&self, other: &Vector3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product
    pub fn cross(&self, other: &Vector3) -> Vector3 {
        Vector3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Distance to another point
    pub fn distance(&self, other: &Vector3) -> f64 {
        (*self - *other).length()
    }
}

impl Default for Vector3 {
    fn default() -> Self {
        Vector3::ZERO
    }
}

impl Add for Vector3 {
    type Output = Vector3;
    fn add(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vector3 {
    type Output = Vector3;
    fn sub(self, other: Vector3) -> Vector3 {
        Vector3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vector3 {
    type Output = Vector3;
    fn mul(self, scalar: f64) -> Vector3 {
        Vector3::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

impl Div<f64> for Vector3 {
    type Output = Vector3;
    fn div(self, scalar: f64) -> Vector3 {
        Vector3::new(self.x / scalar, self.y / scalar, self.z / scalar)
    }
}

impl Neg for Vector3 {
    type Output = Vector3;
    fn neg(self) -> Vector3 {
        Vector3::new(-self.x, -self.y, -self.z)
    }
}

impl fmt::Display for Vector3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector2_creation() {
        let v = Vector2::new(3.0, 4.0);
        assert_eq!(v.x, 3.0);
        assert_eq!(v.y, 4.0);
    }

    #[test]
    fn test_vector2_length() {
        let v = Vector2::new(3.0, 4.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.length_squared(), 25.0);
    }

    #[test]
    fn test_vector2_normalize() {
        let v = Vector2::new(3.0, 4.0);
        let n = v.normalize();
        assert!((n.length() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_vector2_operations() {
        let v1 = Vector2::new(1.0, 2.0);
        let v2 = Vector2::new(3.0, 4.0);

        let sum = v1 + v2;
        assert_eq!(sum, Vector2::new(4.0, 6.0));

        let diff = v2 - v1;
        assert_eq!(diff, Vector2::new(2.0, 2.0));

        let scaled = v1 * 2.0;
        assert_eq!(scaled, Vector2::new(2.0, 4.0));
    }

    #[test]
    fn test_vector2_dot() {
        let v1 = Vector2::new(1.0, 2.0);
        let v2 = Vector2::new(3.0, 4.0);
        assert_eq!(v1.dot(&v2), 11.0);
    }

    #[test]
    fn test_vector3_creation() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_vector3_cross() {
        let v1 = Vector3::UNIT_X;
        let v2 = Vector3::UNIT_Y;
        let cross = v1.cross(&v2);
        assert_eq!(cross, Vector3::UNIT_Z);
    }

    #[test]
    fn test_vector3_operations() {
        let v1 = Vector3::new(1.0, 2.0, 3.0);
        let v2 = Vector3::new(4.0, 5.0, 6.0);

        let sum = v1 + v2;
        assert_eq!(sum, Vector3::new(5.0, 7.0, 9.0));

        let neg = -v1;
        assert_eq!(neg, Vector3::new(-1.0, -2.0, -3.0));
    }
}
