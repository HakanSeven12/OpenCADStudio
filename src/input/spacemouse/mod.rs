//! SpaceMouse navigation data and the thread-safe bridge to the platform driver.
//!
//! No SDK types escape this module. Driver callbacks operate on a snapshot;
//! only the app thread applies a proposed view to a drawing.

// Snapshot/ABI support is intentionally dormant until another platform has an adapter.
#![cfg_attr(not(windows), allow(dead_code))]

use glam::{DMat4, DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
mod navlib;
mod pan;
#[cfg(windows)]
mod raw_input;
#[cfg(windows)]
mod worker;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NavigationMode {
    #[default]
    Auto,
    PanOnly,
    PanZoom,
    Full3D,
}

impl NavigationMode {
    pub const ALL: [Self; 4] = [Self::Auto, Self::PanOnly, Self::PanZoom, Self::Full3D];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Follow context",
            Self::PanOnly => "Pan only",
            Self::PanZoom => "Pan and zoom",
            Self::Full3D => "3D navigation",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Auto => "Pan and zoom on paper sheets; full navigation in model views.",
            Self::PanOnly => {
                "Push or tilt to pan. Twist and vertical pressure are ignored; zoom and rotation stay fixed."
            }
            Self::PanZoom => {
                "Push or tilt to pan; lift or press to zoom. Twist is ignored and rotation stays fixed."
            }
            Self::Full3D => "Pan, zoom, and orbit model views. Paper sheets keep rotation locked.",
        }
    }
    pub fn allows_rotation(self, sheet: bool) -> bool {
        !sheet && matches!(self, Self::Auto | Self::Full3D)
    }
    pub fn allows_zoom(self) -> bool {
        self != Self::PanOnly
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub enabled: bool,
    pub mode: NavigationMode,
    pub pan_speed: u16,
    pub pan_reversed: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: NavigationMode::Auto,
            pan_speed: 100,
            pan_reversed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Ready,
    Disconnected,
    Unavailable(String),
    Unsupported,
}
impl Status {
    pub fn label(&self) -> &str {
        match self {
            Self::Starting => "Connecting…",
            Self::Ready => "Ready",
            Self::Disconnected => "No SpaceMouse connected",
            Self::Unavailable(_) => "3DxWare unavailable",
            Self::Unsupported => "Available in the Windows desktop app",
        }
    }
}

/// Stable target plus camera revision. Late callbacks must never move another
/// document, pane, layout, or a camera already changed by another input source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub tab: u64,
    pub layout: String,
    pub viewport: Option<u64>,
    pub tile: usize,
    pub generation: u64,
}
impl Target {
    pub fn same_context(&self, other: &Self) -> bool {
        self.tab == other.tab
            && self.layout == other.layout
            && self.viewport == other.viewport
            && self.tile == other.tile
    }
}

#[derive(Clone, Debug)]
pub struct View {
    pub target: Target,
    pub eye: DVec3,
    pub rotation: DQuat,
    pub distance: f64,
    pub fov: f64,
    pub perspective: bool,
    pub aspect: f64,
    pub model_bounds: [f64; 6],
    pub selection_bounds: Option<[f64; 6]>,
    pub pointer: DVec3,
    pub pivot: DVec3,
    pub pivot_visible: bool,
    pub rotate: bool,
    pub zoom: bool,
    pub motion_enabled: bool,
    pub units_to_meters: f64,
    pub pan_speed: f64,
}
impl View {
    fn planar(&self) -> bool {
        !self.rotate
    }
    pub fn affine(&self) -> [f64; 16] {
        DMat4::from_rotation_translation(self.rotation, self.eye).to_cols_array()
    }
    pub fn half_height(&self) -> f64 {
        self.distance * (self.fov * 0.5).tan()
    }
    pub fn extents(&self) -> [f64; 6] {
        let h = self.half_height();
        let w = h * self.aspect;
        [
            -w,
            -h,
            -self.distance * 1000.0,
            w,
            h,
            self.distance * 1000.0,
        ]
    }
    /// Validate before accepting driver data. A pan-only view projects the
    /// translation onto its original screen plane, including in perspective.
    pub fn set_affine(&mut self, values: [f64; 16]) -> bool {
        if !values.iter().all(|v| v.is_finite()) {
            return false;
        }
        let matrix = DMat4::from_cols_array(&values);
        let x = matrix.x_axis.truncate();
        let y = matrix.y_axis.truncate();
        let z = matrix.z_axis.truncate();
        if (x.length_squared() - 1.0).abs() > 0.01
            || (y.length_squared() - 1.0).abs() > 0.01
            || (z.length_squared() - 1.0).abs() > 0.01
            || x.dot(y).abs() > 0.01
            || x.cross(y).dot(z) < 0.99
            || matrix.w_axis.w != 1.0
            || matrix.x_axis.w != 0.0
            || matrix.y_axis.w != 0.0
            || matrix.z_axis.w != 0.0
        {
            return false;
        }
        let proposed_eye = matrix.w_axis.truncate();
        if self.zoom {
            self.eye = proposed_eye;
        } else {
            let normal = self.rotation * DVec3::Z;
            let delta = proposed_eye - self.eye;
            self.eye += delta - normal * delta.dot(normal);
        }
        if self.rotate {
            self.rotation = DQuat::from_mat4(&matrix).normalize();
        }
        true
    }
    pub fn set_extents(&mut self, extents: [f64; 6]) -> bool {
        if !extents.iter().all(|v| v.is_finite()) {
            return false;
        }
        let distance = (extents[4] - extents[1]) * 0.5 / (self.fov * 0.5).tan();
        if !(1e-6..=1e15).contains(&distance) {
            return false;
        }
        if self.zoom {
            // Orthographic zoom changes scale while retaining the view center.
            self.eye += self.rotation * DVec3::Z * (distance - self.distance);
            self.distance = distance;
        }
        true
    }

    /// Honor the driver's Lock Horizon option in our Z-up world. Keep the
    /// proposed eye and viewing direction away from the poles. A pitch through
    /// a pole must stop there: rebuilding world-up on the far side would reverse
    /// screen-right and turn the view through 180 degrees.
    fn lock_horizon(&mut self, previous: DQuat, orbit_about_pivot: bool) {
        if !self.rotate {
            return;
        }
        // Leave enough room for the app camera's f32 quaternion round-trip.
        const POLE_MARGIN: f64 = 1e-4;
        let proposed_normal = self.rotation * DVec3::Z;
        let mut normal = proposed_normal;
        let previous_right = (previous * DVec3::X)
            .truncate()
            .extend(0.)
            .try_normalize()
            .unwrap_or(DVec3::X);
        let right = DVec3::Z.cross(normal);
        let crossed_pole =
            right.dot(previous_right) < 0. && (self.rotation * DVec3::X).dot(previous_right) > 0.;
        let right = if crossed_pole || right.length_squared() < POLE_MARGIN.powi(2) {
            let horizontal_normal = previous_right.cross(DVec3::Z);
            normal = horizontal_normal * POLE_MARGIN
                + DVec3::Z * normal.z.signum() * (1. - POLE_MARGIN.powi(2)).sqrt();
            // Apply the rejected pitch to the eye around the SDK's orbit pivot
            // too. Correcting orientation alone lets a held orbit carry the eye
            // over the pole and shifts the object across the screen.
            if orbit_about_pivot {
                let correction = DQuat::from_rotation_arc(proposed_normal, normal);
                self.eye = self.pivot + correction * (self.eye - self.pivot);
            }
            previous_right
        } else {
            right.normalize()
        };
        self.rotation =
            DQuat::from_mat3(&glam::DMat3::from_cols(right, normal.cross(right), normal))
                .normalize();
    }
}

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon: Option<&'static [u8]>,
}

#[derive(Default)]
struct Bridge {
    view: Option<View>,
    pending_view: Option<View>,
    actions: Vec<Action>,
    commands: VecDeque<(Target, String)>,
    focus: bool,
    moving: bool,
    in_transaction: bool,
    frame_time: Option<f64>,
    reset_motion: bool,
    driver_settings_changed: bool,
    lock_horizon: bool,
    orbit_about_pivot: bool,
    pan: pan::Pan,
    status: Option<Status>,
    ever_connected: bool,
    wake: Option<iced::futures::channel::mpsc::Sender<()>>,
    #[cfg(windows)]
    control: Option<worker::Wake>,
}
impl Bridge {
    fn wake(&mut self) {
        if let Some(wake) = &mut self.wake {
            let _ = wake.try_send(());
        }
    }
}

#[derive(Clone, Default)]
pub struct Service(Arc<Mutex<Bridge>>);
impl Hash for Service {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}
impl Service {
    pub fn status(&self) -> Status {
        if !cfg!(any(windows, test)) {
            return Status::Unsupported;
        }
        self.0
            .lock()
            .unwrap()
            .status
            .clone()
            .unwrap_or(Status::Starting)
    }
    pub fn visible(&self) -> bool {
        self.0.lock().unwrap().ever_connected
    }
    pub fn moving(&self) -> bool {
        let bridge = self.0.lock().unwrap();
        if let Some(view) = bridge.view.as_ref().filter(|v| v.planar()) {
            bridge.pan.moving(view.zoom)
        } else {
            bridge.moving
        }
    }
    pub fn frame(&self, milliseconds: f64) {
        let mut bridge = self.0.lock().unwrap();
        if bridge.focus
            && bridge
                .view
                .as_ref()
                .is_some_and(|v| v.planar() && v.motion_enabled)
        {
            let zoom = bridge.view.as_ref().unwrap().zoom;
            let delta = bridge.pan.frame(milliseconds, zoom);
            if delta != DVec3::ZERO {
                let view = bridge.view.as_mut().unwrap();
                view.eye -= view.rotation
                    * delta.truncate().extend(0.)
                    * view.half_height()
                    * 2.
                    * view.pan_speed;
                // Lift/pressure owns zoom in both projections. Keep the center
                // fixed and leave push/tilt exclusively responsible for pan.
                let distance = (view.distance * (delta.z * 2.).exp()).clamp(1e-6, 1e15);
                view.eye += view.rotation * DVec3::Z * (distance - view.distance);
                view.distance = distance;
                bridge.pending_view = bridge.view.clone();
            }
            // Also wake after a stale report stops motion, so rendering can idle.
            bridge.wake();
            return;
        }
        if bridge.focus && bridge.moving {
            bridge.frame_time = Some(milliseconds);
            #[cfg(windows)]
            if let Some(wake) = &bridge.control {
                wake.signal();
            }
        }
    }
    #[cfg(test)]
    pub fn test_connect(&self) {
        self.0.lock().unwrap().status = Some(Status::Ready);
    }
    #[cfg(test)]
    pub fn test_view(&self) -> Option<View> {
        self.0.lock().unwrap().view.clone()
    }
    #[cfg(test)]
    pub fn test_pan_report(&self, axes: [i16; 6]) {
        let report: Vec<_> = std::iter::once(1)
            .chain(axes.into_iter().flat_map(i16::to_le_bytes))
            .collect();
        self.0.lock().unwrap().pan.report(&report);
    }
    #[cfg(test)]
    pub fn test_propose(&self, change: impl FnOnce(&mut View)) {
        let mut bridge = self.0.lock().unwrap();
        let mut view = bridge.view.clone().expect("focused drawing snapshot");
        change(&mut view);
        bridge.pending_view = Some(view);
        bridge.moving = true;
    }
    pub fn set_actions(&self, actions: Vec<Action>) {
        self.0.lock().unwrap().actions = actions;
    }
    /// Rebase callbacks that arrived while the UI applied the previous frame.
    /// They still belong to the same gesture, despite the scene revision bump.
    pub fn acknowledge(&self, previous: &Target, applied: &Target) {
        let mut bridge = self.0.lock().unwrap();
        if previous.same_context(applied) {
            let Bridge {
                view, pending_view, ..
            } = &mut *bridge;
            for view in [view, pending_view].into_iter().flatten() {
                if &view.target == previous {
                    view.target = applied.clone();
                }
            }
        }
    }
    pub fn sync(&self, view: Option<View>, focus: bool) {
        let mut bridge = self.0.lock().unwrap();
        let context_changed = match (&bridge.view, &view) {
            (Some(old), Some(new)) => !old.target.same_context(&new.target),
            (None, None) => false,
            _ => true,
        };
        let changed = bridge.view.as_ref().map(|v| &v.target) != view.as_ref().map(|v| &v.target)
            || bridge.focus != focus
            || bridge
                .view
                .as_ref()
                .map(|v| (v.rotate, v.zoom, v.motion_enabled))
                != view.as_ref().map(|v| (v.rotate, v.zoom, v.motion_enabled));
        if changed {
            bridge.reset_motion = true;
            bridge.frame_time = None;
            bridge.pan.clear();
            bridge.pending_view = None;
            if context_changed || bridge.focus != focus {
                bridge.commands.clear();
                bridge.moving = false;
            }
            bridge.in_transaction = false;
            bridge.view = view;
        } else if bridge.pending_view.is_none() {
            bridge.view = view;
        }
        bridge.focus = focus;
        if changed {
            #[cfg(windows)]
            if let Some(wake) = &bridge.control {
                wake.signal();
            }
        }
    }
    pub fn drain(&self) -> (Option<View>, Vec<(Target, String)>) {
        let mut bridge = self.0.lock().unwrap();
        let view = if bridge.in_transaction {
            None
        } else {
            bridge.pending_view.take()
        };
        (view, bridge.commands.drain(..).collect())
    }
    pub fn subscription(&self) -> iced::Subscription<()> {
        #[cfg(windows)]
        {
            iced::Subscription::run_with(self.clone(), navlib::subscription)
        }
        #[cfg(not(windows))]
        {
            iced::Subscription::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn view() -> View {
        View {
            target: Target {
                tab: 1,
                layout: "Model".into(),
                viewport: None,
                tile: 0,
                generation: 0,
            },
            eye: DVec3::new(0., 0., 10.),
            rotation: DQuat::IDENTITY,
            distance: 10.,
            fov: 1.,
            perspective: false,
            aspect: 1.,
            model_bounds: [-1., -1., -1., 1., 1., 1.],
            selection_bounds: None,
            pointer: DVec3::ZERO,
            pivot: DVec3::ZERO,
            pivot_visible: false,
            rotate: false,
            zoom: false,
            motion_enabled: true,
            units_to_meters: 1.,
            pan_speed: 1.,
        }
    }
    #[test]
    fn pan_only_preserves_depth_rotation_and_zoom() {
        let mut v = view();
        v.rotation = DQuat::from_rotation_x(0.7);
        let initial = v.clone();
        let normal = initial.rotation * DVec3::Z;
        let proposed = DMat4::from_rotation_translation(
            DQuat::from_rotation_y(0.4),
            v.eye + DVec3::new(4., 5., 6.),
        );
        assert!(v.set_affine(proposed.to_cols_array()));
        assert!((v.eye - initial.eye).dot(normal).abs() < 1e-10);
        assert_eq!(v.rotation, initial.rotation);
        assert!(v.set_extents([-20., -20., -20., 20., 20., 20.]));
        assert_eq!(v.distance, initial.distance);
    }
    #[test]
    fn invalid_driver_matrices_do_not_corrupt_the_camera() {
        let mut v = view();
        let eye = v.eye;
        assert!(!v.set_affine([f64::NAN; 16]));
        assert!(!v.set_affine([0.; 16]));
        assert!(!v.set_extents([0.; 6]));
        assert_eq!(v.eye, eye);
    }
    #[test]
    fn context_and_focus_changes_discard_queued_input() {
        let service = Service::default();
        let v = view();
        service.sync(Some(v.clone()), true);
        service.0.lock().unwrap().pending_view = Some(v.clone());
        service.sync(Some(v.clone()), false);
        assert!(service.drain().0.is_none());
        service.0.lock().unwrap().pending_view = Some(v.clone());
        let mut other = v;
        other.target.tab = 2;
        service.sync(Some(other), true);
        assert!(service.drain().0.is_none());
    }
    #[test]
    fn paper_cannot_rotate_in_any_mode() {
        for mode in NavigationMode::ALL {
            assert!(!mode.allows_rotation(true));
        }
        assert!(NavigationMode::Auto.allows_rotation(false));
        assert!(!NavigationMode::PanOnly.allows_rotation(false));
    }
    #[test]
    fn navigation_frames_are_drained_atomically() {
        let service = Service::default();
        service.sync(Some(view()), true);
        service.test_propose(|v| v.eye.x += 2.);
        service.0.lock().unwrap().in_transaction = true;
        assert!(service.drain().0.is_none());
        service.0.lock().unwrap().in_transaction = false;
        assert_eq!(service.drain().0.unwrap().eye.x, 2.);
        assert!(service.drain().0.is_none());
    }
    #[test]
    fn camera_updates_retain_buttons_but_document_changes_drop_them() {
        let service = Service::default();
        let mut v = view();
        service.sync(Some(v.clone()), true);
        service
            .0
            .lock()
            .unwrap()
            .commands
            .push_back((v.target.clone(), "UNDO".into()));
        v.target.generation += 1;
        service.sync(Some(v.clone()), true);
        assert_eq!(service.drain().1.len(), 1);
        service
            .0
            .lock()
            .unwrap()
            .commands
            .push_back((v.target.clone(), "UNDO".into()));
        v.target.viewport = Some(123);
        service.sync(Some(v), true);
        assert!(service.drain().1.is_empty());
    }
    #[test]
    fn applying_a_frame_retains_newer_callbacks_in_the_same_gesture() {
        let service = Service::default();
        let mut applied = view();
        service.sync(Some(applied.clone()), true);
        service.0.lock().unwrap().reset_motion = false;
        service.test_propose(|v| v.eye.x = 1.);
        let first = service.drain().0.unwrap();
        service.test_propose(|v| v.eye.x = 2.);
        service.0.lock().unwrap().in_transaction = true;
        applied.eye = first.eye;
        applied.target.generation += 1;
        service.acknowledge(&first.target, &applied.target);
        service.sync(Some(applied.clone()), true);
        assert!(service.drain().0.is_none());
        assert!(!service.0.lock().unwrap().reset_motion);
        service.0.lock().unwrap().in_transaction = false;
        let second = service.drain().0.unwrap();
        assert_eq!(second.eye.x, 2.);
        assert_eq!(second.target, applied.target);
        // A subsequent wheel/mouse camera revision must reset the SDK model.
        applied.target.generation += 1;
        service.sync(Some(applied), true);
        assert!(service.0.lock().unwrap().reset_motion);
    }
}
