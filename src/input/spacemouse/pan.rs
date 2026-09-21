//! Planar input in device space: pushes and tilts have the same meaning.
//! The SDK's camera matrices have already combined orbit, pan, and zoom, so
//! they cannot reliably recover these independent physical inputs.
use glam::{DVec2, DVec3};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Pan {
    axes: [i16; 6],
    received: Option<Instant>,
    frame_time: Option<f64>,
}
impl Pan {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// SpaceMouse USB uses split translation/rotation reports; Wireless also
    /// sends a combined report. Ignore buttons, battery, and truncated reports.
    pub fn report(&mut self, report: &[u8]) -> bool {
        let (start, count) = match (report.first(), report.len()) {
            (Some(1), 13..) => (0, 6),
            (Some(1), 7) => (0, 3),
            (Some(2), 7..) => (3, 3),
            _ => return false,
        };
        for i in 0..count {
            self.axes[start + i] = i16::from_le_bytes([report[1 + i * 2], report[2 + i * 2]]);
        }
        self.received = Some(Instant::now());
        if !self.moving(true) {
            self.frame_time = None;
        }
        true
    }
    pub fn moving(&self, zoom: bool) -> bool {
        direction(self.axes) != DVec2::ZERO || (zoom && response(self.axes[2] as f64) != 0.)
    }

    pub fn frame(&mut self, milliseconds: f64, zoom: bool) -> DVec3 {
        if !milliseconds.is_finite()
            || self
                .received
                .is_none_or(|t| t.elapsed() > Duration::from_millis(350))
        {
            self.clear();
            return DVec3::ZERO;
        }
        let dt = self.frame_time.map_or(1. / 60., |old| {
            ((milliseconds - old) / 1000.).clamp(0., 0.05)
        });
        self.frame_time = Some(milliseconds);
        direction(self.axes).extend(if zoom {
            response(self.axes[2] as f64)
        } else {
            0.
        }) * dt
    }
}

fn response(v: f64) -> f64 {
    let n = (v.abs() / 350.).min(1.);
    if n <= 0.08 {
        0.
    } else {
        v.signum() * ((n - 0.08) / 0.92).powf(1.5)
    }
}

fn direction(axes: [i16; 6]) -> DVec2 {
    fn strongest(push: f64, tilt: f64) -> f64 {
        // A hand often pushes and tilts together. Do not double the speed.
        if push.abs() >= tilt.abs() {
            push
        } else {
            tilt
        }
    }
    let right = strongest(response(axes[0] as f64), response(-(axes[4] as f64)));
    let up = strongest(response(-(axes[1] as f64)), response(-(axes[3] as f64)));
    DVec2::new(right, up).clamp_length_max(1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(axes: [i16; 6]) -> Vec<u8> {
        std::iter::once(1)
            .chain(axes.into_iter().flat_map(i16::to_le_bytes))
            .collect()
    }
    #[test]
    fn pushes_and_tilts_pan_in_the_same_screen_directions() {
        for (push, tilt, expected) in [
            ([350, 0, 0, 0, 0, 0], [0, 0, 0, 0, -350, 0], DVec2::X),
            ([-350, 0, 0, 0, 0, 0], [0, 0, 0, 0, 350, 0], -DVec2::X),
            ([0, -350, 0, 0, 0, 0], [0, 0, 0, -350, 0, 0], DVec2::Y),
            ([0, 350, 0, 0, 0, 0], [0, 0, 0, 350, 0, 0], -DVec2::Y),
        ] {
            assert_eq!(direction(push), expected);
            assert_eq!(direction(tilt), expected);
        }
        assert_eq!(direction([350, 0, 0, 0, -350, 0]), DVec2::X);
    }
    #[test]
    fn twist_vertical_pressure_and_sensor_noise_do_not_pan() {
        assert_eq!(direction([0, 0, 350, 0, 0, -350]), DVec2::ZERO);
        assert_eq!(direction([10, -10, 350, 20, -20, 350]), DVec2::ZERO);
    }
    #[test]
    fn held_input_continues_and_release_or_disconnect_stops_it() {
        let mut pan = Pan::default();
        assert!(pan.report(&report([350, 0, 0, 0, 0, 0])));
        for frame in 0..20 {
            assert!(pan.frame(frame as f64 * 16., false).x > 0.);
        }
        assert!(pan.report(&report([0; 6])));
        assert!(!pan.moving(false));
        assert_eq!(pan.frame(400., false), DVec3::ZERO);
        pan.report(&report([350, 0, 0, 0, 0, 0]));
        pan.received = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(pan.frame(500., false), DVec3::ZERO);
        assert!(!pan.moving(false));
    }
    #[test]
    fn split_and_combined_reports_agree_and_other_reports_are_ignored() {
        let mut pan = Pan::default();
        let full = report([1, 2, 3, 4, 5, 6]);
        assert!(pan.report(&full[..7]));
        let mut rotation = vec![2];
        rotation.extend_from_slice(&full[7..]);
        assert!(pan.report(&rotation));
        assert_eq!(pan.axes, [1, 2, 3, 4, 5, 6]);
        assert!(!pan.report(&[1, 2, 3]));
        assert!(!pan.report(&[23; 13]));
        assert_eq!(pan.axes, [1, 2, 3, 4, 5, 6]);
    }
}
