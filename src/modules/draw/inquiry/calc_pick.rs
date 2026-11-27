// QuickCalc picks: a point, a distance or an angle taken from the drawing
// and handed back to the QuickCalc pallet as a calculator value.

use glam::DVec3;

use super::dist::{measure, rubber_band};
use crate::calc::{AngleMode, Value};
use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::entities::common::format_length;
use crate::scene::model::wire_model::WireModel;

/// What the pallet asked the drawing for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Point,
    Distance,
    Angle,
}

impl PickKind {
    /// Points the pick takes.
    pub fn steps(self) -> u8 {
        match self {
            PickKind::Point => 1,
            PickKind::Distance | PickKind::Angle => 2,
        }
    }
}

pub struct CalcPickCommand {
    kind: PickKind,
    /// The first point, full f64 (see `DistCommand::first`).
    first: Option<DVec3>,
    plane: WorkingPlane,
    angle_mode: AngleMode,
}

impl CalcPickCommand {
    pub fn new(kind: PickKind, angle_mode: AngleMode) -> Self {
        Self {
            kind,
            first: None,
            plane: WorkingPlane::default(),
            angle_mode,
        }
    }
}

impl CadCommand for CalcPickCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "QUICKCALC"
    }

    fn prompt(&self) -> String {
        match (self.kind, self.first) {
            (PickKind::Point, _) => crate::t!("QuickCalc  Specify a point:").into_owned(),
            (_, None) => crate::t!("QuickCalc  Specify first point:").into_owned(),
            (_, Some(_)) => crate::t!("QuickCalc  Specify second point:").into_owned(),
        }
    }

    fn quick_calc_step(&self) -> Option<u8> {
        Some(if self.first.is_some() { 2 } else { 1 })
    }

    fn has_measurement_readout(&self) -> bool {
        self.kind == PickKind::Distance
    }

    fn measurement_readout(&self, cursor: DVec3) -> Option<String> {
        let distance = measure(self.plane, self.first?, cursor).distance;
        distance
            .is_finite()
            .then(|| crate::tr!("measurement", "distance", value = format_length(distance)))
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let value = match (self.kind, self.first) {
            (PickKind::Point, _) => Value::Point(self.plane.to_local(pt)),
            (_, None) => {
                self.first = Some(pt);
                return CmdResult::NeedPoint;
            }
            (PickKind::Distance, Some(p1)) => Value::Number(measure(self.plane, p1, pt).distance),
            (PickKind::Angle, Some(p1)) => Value::Number(
                self.angle_mode
                    .radians_to_mode(measure(self.plane, p1, pt).angle_xy),
            ),
        };
        CmdResult::QuickCalcPick(value)
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        Some(rubber_band(self.first?, pt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::common::{set_unit_context, unit_context, UnitContext};

    fn pick(mut cmd: CalcPickCommand, points: &[DVec3]) -> Value {
        let (last, rest) = points.split_last().unwrap();
        for p in rest {
            assert!(matches!(cmd.on_point(*p), CmdResult::NeedPoint));
            assert_eq!(cmd.quick_calc_step(), Some(2));
        }
        match cmd.on_point(*last) {
            CmdResult::QuickCalcPick(v) => v,
            _ => panic!("expected a QuickCalc pick"),
        }
    }

    fn number(v: Value) -> f64 {
        match v {
            Value::Number(n) => n,
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn point_distance_and_angle() {
        let previous = unit_context();
        set_unit_context(UnitContext {
            angbase: 0.0,
            angdir_cw: false,
            ..previous
        });
        let new = |kind, mode| CalcPickCommand::new(kind, mode);
        let (a, b) = (DVec3::ZERO, DVec3::new(0.0, 3600.0, 0.0));
        let point = pick(new(PickKind::Point, AngleMode::Degrees), &[b]);
        let distance = pick(new(PickKind::Distance, AngleMode::Degrees), &[a, b]);
        let degrees = pick(new(PickKind::Angle, AngleMode::Degrees), &[a, b]);
        let radians = pick(new(PickKind::Angle, AngleMode::Radians), &[a, b]);
        set_unit_context(previous);
        assert_eq!(point, Value::Point(b));
        assert_eq!(distance, Value::Number(3600.0));
        assert!((number(degrees) - 90.0).abs() < 1e-9);
        assert!((number(radians) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn follows_the_ucs_angbase_and_angdir() {
        let mut cmd = CalcPickCommand::new(PickKind::Point, AngleMode::Degrees);
        cmd.set_working_plane(WorkingPlane::new(
            DVec3::new(100.0, 0.0, 0.0),
            DVec3::X,
            DVec3::Y,
        ));
        assert_eq!(
            pick(cmd, &[DVec3::new(150.0, 20.0, 0.0)]),
            Value::Point(DVec3::new(50.0, 20.0, 0.0))
        );
        // Pointing east is 90° clockwise from a north ANGBASE.
        let previous = unit_context();
        set_unit_context(UnitContext {
            angbase: std::f64::consts::FRAC_PI_2,
            angdir_cw: true,
            ..previous
        });
        let angle = pick(
            CalcPickCommand::new(PickKind::Angle, AngleMode::Degrees),
            &[DVec3::ZERO, DVec3::X],
        );
        set_unit_context(previous);
        assert!((number(angle) - 90.0).abs() < 1e-9);
    }
}
