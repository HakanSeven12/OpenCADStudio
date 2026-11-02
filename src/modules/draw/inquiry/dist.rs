// DIST command — measure distance and angle between two picked points.

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::entities::common::{format_length, unit_context, without_negative_zero};
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
pub struct DistCommand {
    // Keep the picked point in full f64 precision. Downcasting to f32 here
    // loses ~0.03–0.06 units at survey-scale coordinates (e.g. eastings near
    // 5e5), which made snapped-endpoint measurements read off by that much.
    first: Option<DVec3>,
    plane: WorkingPlane,
}
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/measure.svg"));
impl DistCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for DistCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIST"
    }

    fn prompt(&self) -> String {
        if self.first.is_none() {
            t!("DIST  Specify first point:").into_owned()
        } else {
            t!("DIST  Specify second point:").into_owned()
        }
    }

    fn has_measurement_readout(&self) -> bool {
        true
    }

    fn measurement_readout(&self, cursor: DVec3) -> Option<String> {
        let distance = self.plane.vector_to_local(cursor - self.first?).length();
        distance
            .is_finite()
            .then(|| crate::tr!("measurement", "distance", value = format_length(distance)))
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(p1) = self.first {
            let m = measure(self.plane, p1, pt);
            let (dist, dx, dy, dz) = (m.distance, m.delta.x, m.delta.y, m.delta.z);
            let angle_xy = m.angle_xy.to_degrees();
            let angle_z = m.angle_z.to_degrees();

            let aprec = unit_context().auprec.max(0) as usize;
            let dist_s = format_length(dist);
            let angle_xy_s = without_negative_zero(format!("{angle_xy:.aprec$}"));
            let angle_z_s = without_negative_zero(format!("{angle_z:.aprec$}"));
            let dx_s = format_length(dx);
            let dy_s = format_length(dy);
            let dz_s = format_length(dz);
            let msg = t!(
                "Distance = %{dist},  Angle in XY Plane = %{angle_xy}°,  Angle from XY Plane = %{angle_z}°\n  Delta X = %{dx},  Delta Y = %{dy},  Delta Z = %{dz}",
                dist = dist_s,
                angle_xy = angle_xy_s,
                angle_z = angle_z_s,
                dx = dx_s,
                dy = dy_s,
                dz = dz_s,
            )
            .into_owned();
            CmdResult::Measurement(msg)
        } else {
            self.first = Some(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        Some(rubber_band(self.first?, pt))
    }
}

/// What DIST reports for two picked points, worked out in the working plane.
pub struct PointPair {
    /// `p2 - p1` in working-plane coordinates.
    pub delta: DVec3,
    pub distance: f64,
    /// Direction in the XY plane, radians in `[0, TAU)`: counted from
    /// ANGBASE the way ANGDIR runs, like every other direction the drawing
    /// shows.
    pub angle_xy: f64,
    /// Elevation from the XY plane toward Z, radians.
    pub angle_z: f64,
}

/// The distance and angles between `p1` and `p2` (WCS points) as DIST
/// measures them. Shared with QuickCalc's distance and angle picks.
pub fn measure(plane: WorkingPlane, p1: DVec3, p2: DVec3) -> PointPair {
    let delta = plane.vector_to_local(p2 - p1);
    let ctx = unit_context();
    let sign = if ctx.angdir_cw { -1.0 } else { 1.0 };
    let angle_xy =
        (sign * (delta.y.atan2(delta.x) - ctx.angbase)).rem_euclid(std::f64::consts::TAU);
    let angle_z = delta.z.atan2(delta.x.hypot(delta.y));
    PointPair {
        delta,
        distance: delta.length(),
        angle_xy,
        angle_z,
    }
}

/// The rubber band from the first picked point to the cursor.
///
/// Built with [`WireModel::solid_f64`] so it carries the double-single
/// residual: the renderer works relative to the eye and a plain f32 vertex is
/// quantised to ~0.5 m at UTM-scale coordinates, which left the preview line
/// visibly offset from the snapped points (and from the readout).
pub fn rubber_band(p1: DVec3, pt: DVec3) -> WireModel {
    WireModel::solid_f64(
        "dist_preview".into(),
        vec![p1.to_array(), pt.to_array()],
        WireModel::CYAN,
        false,
    )
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIST"] }); // DistCommand

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CmdResult;
    use crate::entities::common::{set_unit_context, UnitContext};

    #[test]
    fn dist_rubber_band_keeps_full_precision_at_utm_scale() {
        // UTM-scale picks quantise to ~0.5 m in absolute f32; the rubber band
        // must reconstruct its endpoints through the double-single pair so the
        // preview stays on the points reported by the measurement.
        let p1 = DVec3::new(1_649_905.55, 4_945_694.94, 12.0);
        let pt = DVec3::new(1_649_906.73, 4_945_695.38, 12.0);
        let wire = rubber_band(p1, pt);
        for (index, expected) in [p1, pt].into_iter().enumerate() {
            let high = wire.points[index];
            let low = wire.points_low[index];
            let actual = DVec3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            );
            assert!(
                actual.distance(expected) < 1e-6,
                "endpoint {index}: {actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn dist_respects_precision_zero() {
        let mut cmd = DistCommand::new();
        set_unit_context(UnitContext {
            lunits: 2,
            luprec: 0,
            aunits: 0,
            auprec: 0,
            angbase: 0.0,
            angdir_cw: false,
            dimzin: 0,
        });

        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let res = cmd.on_point(DVec3::new(10.0, 0.0, 0.0));
        match res {
            CmdResult::Measurement(msg) => {
                assert!(
                    msg.contains("Distance = 10"),
                    "Expected 'Distance = 10' in '{msg}'"
                );
                assert!(
                    msg.contains("Delta X = 10"),
                    "Expected 'Delta X = 10' in '{msg}'"
                );
                assert!(
                    msg.contains("Delta Y = 0"),
                    "Expected 'Delta Y = 0' in '{msg}'"
                );
                assert!(
                    msg.contains("Delta Z = 0"),
                    "Expected 'Delta Z = 0' in '{msg}'"
                );
                assert!(
                    msg.contains("Angle in XY Plane = 0°"),
                    "Expected 'Angle in XY Plane = 0°' in '{msg}'"
                );
            }
            _ => panic!("Expected CmdResult::Measurement"),
        }
    }

    #[test]
    fn dist_respects_precision_four() {
        let mut cmd = DistCommand::new();
        set_unit_context(UnitContext {
            lunits: 2,
            luprec: 4,
            aunits: 0,
            auprec: 2,
            angbase: 0.0,
            angdir_cw: false,
            dimzin: 0,
        });

        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let res = cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        match res {
            CmdResult::Measurement(msg) => {
                assert!(
                    msg.contains("Distance = 11.1803"),
                    "Expected 'Distance = 11.1803' in '{msg}'"
                );
                assert!(
                    msg.contains("Delta X = 10.0000"),
                    "Expected 'Delta X = 10.0000' in '{msg}'"
                );
                assert!(
                    msg.contains("Delta Y = 5.0000"),
                    "Expected 'Delta Y = 5.0000' in '{msg}'"
                );
                assert!(
                    msg.contains("Angle in XY Plane = 26.57°"),
                    "Expected 'Angle in XY Plane = 26.57°' in '{msg}'"
                );
            }
            _ => panic!("Expected CmdResult::Measurement"),
        }
    }

    #[test]
    fn dist_respects_architectural_units() {
        let mut cmd = DistCommand::new();
        set_unit_context(UnitContext {
            lunits: 4, // Architectural
            luprec: 4,
            aunits: 0,
            auprec: 0,
            angbase: 0.0,
            angdir_cw: false,
            dimzin: 0,
        });

        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let res = cmd.on_point(DVec3::new(18.0, 0.0, 0.0));
        match res {
            CmdResult::Measurement(msg) => {
                // 18 inches = 1'-6"
                assert!(msg.contains("1'-6\""), "Expected 1'-6\" in '{msg}'");
            }
            _ => panic!("Expected CmdResult::Measurement"),
        }
    }
}

#[cfg(test)]
mod measurement_tests {
    use super::*;
    #[test]
    fn live_distance_keeps_f64_and_unit_settings() {
        let previous = unit_context();
        let mut ctx = previous;
        ctx.lunits = 2;
        ctx.luprec = 4;
        crate::entities::common::set_unit_context(ctx);
        let mut command = DistCommand::new();
        let first = DVec3::new(500000.001, 600000.002, 0.0);
        assert!(command.measurement_readout(first).is_none());
        command.on_point(first);
        assert!(command
            .measurement_readout(first + DVec3::X * 0.001)
            .unwrap()
            .contains("0.0010"));
        ctx.lunits = 4;
        crate::entities::common::set_unit_context(ctx);
        assert!(command
            .measurement_readout(first + DVec3::X * 18.0)
            .unwrap()
            .contains("1'-6\""));
        crate::entities::common::set_unit_context(previous);
    }
}
