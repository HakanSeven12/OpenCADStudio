//! Interactive front-end for the SNAPANG system variable.
//!
//! Bare `SNAPANG` asks for the angle the way an angle prompt does in other
//! CAD programs: a typed number, or two points whose direction becomes the
//! angle (a rubber band runs from the first point). Enter keeps the current
//! angle. The value is set through the inline `SNAPANG <value>` handler via
//! [`CmdResult::Dispatch`], so range handling and persistence stay in one
//! place.

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::modules::draw::inquiry::dist::rubber_band;
use crate::scene::model::wire_model::WireModel;
use crate::t;
use glam::DVec3;

pub struct SnapAngCommand {
    /// The angle in effect when the command started, offered as the default.
    current: f32,
    /// The first point of the direction, once picked.
    first: Option<DVec3>,
    plane: WorkingPlane,
}

impl SnapAngCommand {
    pub fn new(current: f32) -> Self {
        Self {
            current,
            first: None,
            plane: WorkingPlane::default(),
        }
    }
}

/// The snap angle the direction from `from` to `to` gives: degrees
/// counter-clockwise from the X axis of `plane`, in `[0, 360)`, the way
/// SNAPANG rotates the crosshair. `None` when the points coincide in the
/// plane.
pub fn angle_between(plane: WorkingPlane, from: DVec3, to: DVec3) -> Option<f64> {
    plane
        .angle(from, to)
        .map(|angle| angle.to_degrees().rem_euclid(360.0))
}

fn rejection() -> CmdResult {
    CmdResult::ReportError(t!("Requires valid numeric angle or two points.").into_owned())
}

impl CadCommand for SnapAngCommand {
    fn name(&self) -> &'static str {
        "SNAPANG"
    }

    fn prompt(&self) -> String {
        if self.first.is_some() {
            return t!("Specify second point:").into_owned();
        }
        let name = "SNAPANG";
        let current = self.current;
        crate::tf!("Enter new value for {name} <{current}>:").into_owned()
    }

    /// The first step reads a typed angle; once a point is picked, the
    /// second is an ordinary point prompt (coordinates, direct distance).
    fn input_kind(&self) -> InputKind {
        if self.first.is_none() {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        Some(rubber_band(self.first?, pt))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        // A typed coordinate is a point: the point pipeline takes it.
        if self.first.is_some() || crate::entities::coord_parse::parse_coord(t).is_some() {
            return None;
        }
        if t.is_empty() {
            return Some(CmdResult::Done);
        }
        match t.parse::<f64>() {
            Ok(angle) if angle.is_finite() => Some(CmdResult::Dispatch(format!("SNAPANG {t}"))),
            _ => Some(rejection()),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(first) = self.first else {
            self.first = Some(pt);
            return CmdResult::NeedPoint;
        };
        match angle_between(self.plane, first, pt) {
            Some(angle) => CmdResult::Dispatch(format!("SNAPANG {angle}")),
            None => rejection(),
        }
    }

    /// Enter keeps the current angle.
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatched(result: CmdResult) -> String {
        match result {
            CmdResult::Dispatch(command) => command,
            _ => panic!("expected the value to be set"),
        }
    }

    #[test]
    fn two_points_give_the_direction_between_them() {
        let mut cmd = SnapAngCommand::new(0.0);
        assert!(matches!(
            cmd.on_point(DVec3::new(10.0, 10.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let command = dispatched(cmd.on_point(DVec3::new(20.0, 20.0, 0.0)));
        let angle: f64 = command.strip_prefix("SNAPANG ").unwrap().parse().unwrap();
        assert!((angle - 45.0).abs() < 1e-9, "{command}");
    }

    #[test]
    fn a_direction_to_the_lower_left_is_counted_full_circle() {
        let mut cmd = SnapAngCommand::new(0.0);
        cmd.on_point(DVec3::ZERO);
        let command = dispatched(cmd.on_point(DVec3::new(-1.0, -1.0, 0.0)));
        let angle: f64 = command.strip_prefix("SNAPANG ").unwrap().parse().unwrap();
        assert!((angle - 225.0).abs() < 1e-9, "{command}");
    }

    #[test]
    fn the_angle_is_measured_in_the_active_ucs() {
        // A UCS turned 30° about Z: a line along its X axis is 0°.
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let plane = WorkingPlane {
            origin: DVec3::new(5.0, 5.0, 0.0),
            x: DVec3::new(cos, sin, 0.0),
            y: DVec3::new(-sin, cos, 0.0),
            z: DVec3::Z,
        };
        let along_x = angle_between(plane, DVec3::ZERO, DVec3::new(cos, sin, 0.0)).unwrap();
        assert!(along_x.abs() < 1e-9 || (along_x - 360.0).abs() < 1e-9);
        let world_y = angle_between(plane, DVec3::ZERO, DVec3::Y).unwrap();
        assert!((world_y - 60.0).abs() < 1e-9, "{world_y}");
    }

    #[test]
    fn coincident_points_ask_for_the_second_point_again() {
        let mut cmd = SnapAngCommand::new(0.0);
        cmd.on_point(DVec3::new(3.0, 4.0, 0.0));
        assert!(matches!(
            cmd.on_point(DVec3::new(3.0, 4.0, 0.0)),
            CmdResult::ReportError(_)
        ));
        assert_eq!(cmd.first, Some(DVec3::new(3.0, 4.0, 0.0)));
        assert!(matches!(
            cmd.on_point(DVec3::new(3.0, 5.0, 0.0)),
            CmdResult::Dispatch(_)
        ));
    }

    #[test]
    fn a_typed_number_is_the_angle_and_a_coordinate_is_a_point() {
        let mut cmd = SnapAngCommand::new(15.0);
        assert!(matches!(cmd.input_kind(), InputKind::SingleToken));
        assert_eq!(dispatched(cmd.on_text_input("30").unwrap()), "SNAPANG 30");

        let mut cmd = SnapAngCommand::new(15.0);
        assert!(cmd.on_text_input("10,20").is_none());
        assert!(cmd.on_text_input("@5<30").is_none());
        cmd.on_point(DVec3::new(10.0, 20.0, 0.0));
        assert!(matches!(cmd.input_kind(), InputKind::Point));
        assert!(
            cmd.on_text_input("40").is_none(),
            "a distance at the point prompt"
        );
    }

    #[test]
    fn text_that_is_neither_is_refused_and_enter_keeps_the_angle() {
        let mut cmd = SnapAngCommand::new(15.0);
        assert!(matches!(
            cmd.on_text_input("ABC"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(cmd.first.is_none());
        assert!(matches!(cmd.on_enter(), CmdResult::Done));
        assert!(cmd.prompt().contains("<15>"), "{}", cmd.prompt());
    }
}
