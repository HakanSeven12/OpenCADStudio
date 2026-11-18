//! Whether an inward offset leaves room: the test before hollowing a solid.
//!
//! Adapted from Solidcore's `offset/wall_room.rs` (NVIDIA, Apache-2.0),
//! written on this kernel's face meshes. An inward offset of `wall` from the
//! faces of a solid leaves a cavity when some point inside the solid lies
//! farther than `wall` (and a margin) from every offset face. A point counts
//! only as the centre of a ball inside the solid, reached through material:
//! the balls start at face samples and step inward along the normal, then
//! climb from ball to ball away from the nearest offset face. A step of `h`
//! from a sample ends at a ball's centre when no face is nearer than `h`.
//! Distances are to the meshes less their deviation from the surfaces, so a
//! witness found is there.

use super::near::{closest_on_triangle, FaceSoups};
use super::topology::{Body, FaceKey};
use ocs_geom::space::Vec3;

/// Samples per face, and in all.
const PER_FACE: usize = 25;
const MAX_SAMPLES: usize = 2048;
/// The most climbing steps from one sample.
const STEPS: usize = 48;

/// What [`shell_wall_room`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WallRoom {
    /// A point inside the solid farther than the wall from every offset
    /// face: the offset leaves a cavity around it.
    Room([f64; 3]),
    /// Every face is open: nothing is offset.
    NoWalls,
    /// No such point was found; nothing is claimed.
    NotShown,
}

/// Whether offsetting every face of the closed solid `body` but `open`
/// inward by `wall` leaves a cavity.
pub fn shell_wall_room(body: &Body, open: &[FaceKey], wall: f64) -> WallRoom {
    if !(wall.is_finite() && wall > 0.0) {
        return WallRoom::NotShown;
    }
    let tolerance = super::operation_tolerance(&[body]);
    let soups = FaceSoups::of(body, tolerance);
    let all: Vec<FaceKey> = body.face_keys().collect();
    let walls: Vec<FaceKey> = all.iter().copied().filter(|face| !open.contains(face)).collect();
    if walls.is_empty() {
        return WallRoom::NoWalls;
    }
    // The least distance from `point` to `faces`, each measured on its mesh
    // less how far the mesh strays, and the nearest mesh point.
    let nearest = |faces: &[FaceKey], point: Vec3| -> (f64, Option<Vec3>) {
        let mut best = (f64::INFINITY, None);
        for face in faces {
            let (Some(triangles), Some(deviation)) = (soups.triangles(*face), soups.deviation(*face)) else {
                return (0.0, None);
            };
            for triangle in triangles {
                let at = closest_on_triangle(point, triangle);
                let gap = (at - point).length() - deviation;
                if gap < best.0 {
                    best = (gap, Some(at));
                }
            }
        }
        best
    };
    let margin = (10.0 * tolerance).max(1e-3 * wall);
    let need = wall + margin;
    let mut samples: Vec<(Vec3, Vec3)> = Vec::new();
    for face in &walls {
        let (Some(node), Some(triangles)) = (body.faces.get(*face), soups.triangles(*face)) else {
            return WallRoom::NotShown;
        };
        let Some(surface) = body.surfaces.get(node.surface) else {
            return WallRoom::NotShown;
        };
        let inward = if node.forward { -1.0 } else { 1.0 };
        let stride = (triangles.len() / PER_FACE).max(1);
        for triangle in triangles.iter().step_by(stride).take(PER_FACE) {
            if samples.len() >= MAX_SAMPLES {
                break;
            }
            let centre = (triangle[0] + triangle[1] + triangle[2]) * (1.0 / 3.0);
            let Some((u, v)) = surface.parameters_at(centre.to_array()) else {
                continue;
            };
            let Some(normal) = surface.normal_at(u, v) else {
                continue;
            };
            samples.push((Vec3::from(surface.point_at(u, v)), Vec3::from(normal) * inward));
        }
    }
    // The sample's own face is as far as the step, less what its mesh may
    // stray by: a ball is taken as touching it within that slack.
    let slack = 2.0 * all.iter().filter_map(|face| soups.deviation(*face)).fold(0.0, f64::max) + 1e-9 * need;
    // A step of `h` inward from a sample ends at a ball's centre when no
    // face is nearer to it than `h`.
    let ball = |sample: Vec3, normal: Vec3, h: f64| {
        let centre = sample + normal * h;
        let (radius, _) = nearest(&all, centre);
        (radius.is_finite() && radius + slack >= h, centre, radius)
    };
    // First the direct step of the whole wall from each sample.
    for &(sample, normal) in &samples {
        let (inside, centre, _) = ball(sample, normal, need + slack);
        if inside && nearest(&walls, centre).0 >= need {
            return WallRoom::Room(centre.to_array());
        }
    }
    // Then climbing from a small ball at each, away from the nearest offset
    // face by most of the ball's radius, so staying inside the body, while
    // the distance from the offset faces grows.
    for &(sample, normal) in &samples {
        let (inside, mut centre, mut radius) = ball(sample, normal, margin.max(1e3 * tolerance));
        if !inside {
            continue;
        }
        for _ in 0..STEPS {
            let (gap, nearest_point) = nearest(&walls, centre);
            if gap >= need {
                return WallRoom::Room(centre.to_array());
            }
            let Some(from) = nearest_point.filter(|_| gap > 0.0 && gap.is_finite()) else {
                break;
            };
            let away = centre - from;
            let next = centre + away * (0.9 * radius / away.length());
            let (room, _) = nearest(&all, next);
            if !(room > 0.0 && room.is_finite()) || nearest(&walls, next).0 <= gap * (1.0 + 1e-9) {
                break;
            }
            centre = next;
            radius = room;
        }
    }
    WallRoom::NotShown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make;

    #[test]
    fn a_wall_thinner_than_half_the_block_leaves_room() {
        let block = make::cuboid([0.0; 3], [10.0, 6.0, 4.0]).unwrap();
        let top = crate::face_at_point(&block, [5.0, 3.0, 4.0]).unwrap();
        assert!(matches!(shell_wall_room(&block, &[top], 1.0), WallRoom::Room(_)));
        // Open at the top, the cavity is reached up to 4 from the floor and
        // 3 from the long sides: a wall of 3 leaves nothing.
        assert_eq!(shell_wall_room(&block, &[top], 3.2), WallRoom::NotShown);
        assert_eq!(shell_wall_room(&block, &[], 2.1), WallRoom::NotShown);
        let all: Vec<FaceKey> = block.face_keys().collect();
        assert_eq!(shell_wall_room(&block, &all, 1.0), WallRoom::NoWalls);
    }

    #[test]
    fn a_ball_has_room_inside_its_radius_only() {
        let ball = make::sphere([1.0, 2.0, 3.0], 2.0).unwrap();
        assert!(matches!(shell_wall_room(&ball, &[], 1.5), WallRoom::Room(_)));
        assert_eq!(shell_wall_room(&ball, &[], 2.1), WallRoom::NotShown);
    }
}
