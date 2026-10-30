//! Eye-plane-guarded world → clip / screen projection.
//!
//! A perspective view's clip W is the point's depth in front of the eye
//! (`-z_view`). At or behind the eye plane `W <= 0`, and dividing by it
//! mirrors the point onto the opposite side of the screen — often right under
//! the cursor, where a pick or snap then takes it for real geometry. Every CPU
//! projection that decides a hit therefore goes through these helpers instead
//! of `Mat4::project_point3`, which divides by a negative W without complaint
//! (#46 for overlays, #64 for picking and snapping).

use glam::{DVec3, Mat4, Vec3};
use iced::{Point, Rectangle};

/// Smallest clip W treated as in front of the eye. At `W = 0` the NDC
/// division explodes; just past it the projected point is millions of pixels
/// away. The epsilon matches the overlay's guard so a point is rejected the
/// same way everywhere.
const MIN_CLIP_W: f32 = 1e-7;

/// NDC of `world` under a relative-to-eye view-projection, or `None` when the
/// point sits on/behind the perspective eye plane or is not finite.
pub(crate) fn project_ndc(view_rot: Mat4, eye: DVec3, world: DVec3) -> Option<Vec3> {
    let clip = view_rot * (world - eye).as_vec3().extend(1.0);
    if !clip.is_finite() || clip.w <= MIN_CLIP_W {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    ndc.is_finite().then_some(ndc)
}

/// Viewport-local pixel position of `world`, with the same eye-plane guard.
pub(crate) fn project_screen(
    view_rot: Mat4,
    eye: DVec3,
    bounds: Rectangle,
    world: DVec3,
) -> Option<Point> {
    let ndc = project_ndc(view_rot, eye, world)?;
    let screen = Point::new(
        (ndc.x + 1.0) * 0.5 * bounds.width,
        (1.0 - ndc.y) * 0.5 * bounds.height,
    );
    (screen.x.is_finite() && screen.y.is_finite()).then_some(screen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perspective(eye: DVec3) -> (Mat4, DVec3) {
        let view = glam::camera::rh::proj::directx::perspective(
            45.0_f32.to_radians(),
            4.0 / 3.0,
            0.1,
            2000.0,
        );
        (view, eye)
    }

    #[test]
    fn front_points_project_and_behind_points_do_not() {
        let bounds = Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let (view, eye) = perspective(DVec3::ZERO);
        let front = project_screen(view, eye, bounds, DVec3::new(0.0, 0.0, -100.0))
            .expect("in front of the eye");
        assert!((front.x - 400.0).abs() < 1e-3 && (front.y - 300.0).abs() < 1e-3);
        assert!(project_screen(view, eye, bounds, DVec3::new(0.0, 0.0, 100.0)).is_none());
        assert!(project_screen(view, eye, bounds, DVec3::new(0.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn orthographic_projection_never_has_an_eye_plane() {
        let bounds = Rectangle::with_size(iced::Size::new(800.0, 600.0));
        let view = glam::camera::rh::proj::directx::orthographic(-1.0, 1.0, -1.0, 1.0, 0.0, 10.0);
        let eye = DVec3::new(3.0, 4.0, 5.0);
        // Any point has W = 1 here, including ones behind the eye position.
        for world in [
            eye + DVec3::new(0.0, 0.0, 100.0),
            eye - DVec3::new(0.0, 0.0, 100.0),
        ] {
            assert!(project_screen(view, eye, bounds, world).is_some());
        }
    }
}
