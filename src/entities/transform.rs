// Geometric frames and transforms applied to entities.
//
// Shared by entity code, the scene and interactive commands.

use document::EntityType;
use glam::DVec3;

// ── Working plane ─────────────────────────────────────────────────────────

/// Full-precision coordinate frame used by interactive commands.
///
/// Points delivered to commands remain WCS points. Commands that construct
/// planar geometry use this frame for their local calculations, then convert
/// the result back to WCS for storage and preview.
#[derive(Clone, Copy, Debug)]
pub struct WorkingPlane {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Default for WorkingPlane {
    fn default() -> Self {
        Self {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Y,
            z: DVec3::Z,
        }
    }
}

impl WorkingPlane {
    pub fn is_identity(self) -> bool {
        self.origin.abs_diff_eq(DVec3::ZERO, 1e-12)
            && self.x.abs_diff_eq(DVec3::X, 1e-12)
            && self.y.abs_diff_eq(DVec3::Y, 1e-12)
            && self.z.abs_diff_eq(DVec3::Z, 1e-12)
    }

    pub fn new(origin: DVec3, x: DVec3, y: DVec3) -> Self {
        let x = x.normalize_or(DVec3::X);
        let raw_y = y.normalize_or(DVec3::Y);
        let fallback = if x.dot(DVec3::Z).abs() < 0.999 {
            DVec3::Z
        } else {
            DVec3::Y
        };
        let z = x.cross(raw_y).normalize_or(x.cross(fallback).normalize());
        let y = z.cross(x).normalize();
        Self { origin, x, y, z }
    }

    pub fn to_world(self, point: DVec3) -> DVec3 {
        self.origin + self.x * point.x + self.y * point.y + self.z * point.z
    }

    pub fn to_local(self, point: DVec3) -> DVec3 {
        let delta = point - self.origin;
        DVec3::new(delta.dot(self.x), delta.dot(self.y), delta.dot(self.z))
    }

    pub fn vector_to_world(self, vector: DVec3) -> DVec3 {
        self.x * vector.x + self.y * vector.y + self.z * vector.z
    }

    pub fn vector_to_local(self, vector: DVec3) -> DVec3 {
        DVec3::new(vector.dot(self.x), vector.dot(self.y), vector.dot(self.z))
    }

    pub fn angle(self, from: DVec3, to: DVec3) -> Option<f64> {
        let direction = self.vector_to_local(to - from);
        (direction.x.hypot(direction.y) > f64::EPSILON).then(|| direction.y.atan2(direction.x))
    }

    pub fn to_world_transform(self) -> document::types::Transform {
        use document::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.y.x, self.z.x, self.origin.x],
                [self.x.y, self.y.y, self.z.y, self.origin.y],
                [self.x.z, self.y.z, self.z.z, self.origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    pub fn to_local_transform(self) -> document::types::Transform {
        use document::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.x.y, self.x.z, -self.origin.dot(self.x)],
                [self.y.x, self.y.y, self.y.z, -self.origin.dot(self.y)],
                [self.z.x, self.z.y, self.z.z, -self.origin.dot(self.z)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    pub fn place_entity(self, mut entity: EntityType) -> EntityType {
        crate::scene::view::dispatch::apply_transform(
            &mut entity,
            &EntityTransform::Affine(self.to_world_transform()),
        );
        entity
    }
}

// ── Transform ─────────────────────────────────────────────────────────────

/// A geometric transformation applied to existing entities.
#[derive(Clone)]
pub enum EntityTransform {
    /// Move every point by the given world-space delta.
    Translate(DVec3),
    /// Rotate around the axis through `center`.
    Rotate {
        center: DVec3,
        axis: DVec3,
        angle_rad: f64,
    },
    /// Uniform scale from `center` by `factor`.
    Scale { center: DVec3, factor: f64 },
    /// Mirror through the plane containing `p1`→`p2` and the working normal.
    Mirror {
        p1: DVec3,
        p2: DVec3,
        working_normal: DVec3,
    },
    /// General affine transform. Used when a complete UCS basis must be baked
    /// into block-local geometry instead of stored as drawing UCS state.
    Affine(document::types::Transform),
}

#[cfg(test)]
mod working_plane_tests {
    use super::*;

    #[test]
    fn translated_and_rotated_frame_round_trips_points_and_vectors() {
        let plane = WorkingPlane::new(
            DVec3::new(125_000.25, -42_000.5, 810.75),
            DVec3::new(0.0, 1.0, 0.0),
            DVec3::new(0.0, 0.0, 1.0),
        );
        let local = DVec3::new(12.5, -3.25, 7.75);
        let vector = DVec3::new(-2.0, 5.0, 1.5);

        assert!(plane
            .to_local(plane.to_world(local))
            .abs_diff_eq(local, 1e-9));
        assert!(plane
            .vector_to_local(plane.vector_to_world(vector))
            .abs_diff_eq(vector, 1e-12));
    }
}
