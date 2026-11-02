//! Authoring shared by Linear, Polar and XY parameters: Linear and Polar take a
//! label pick, XY asks for its grip count and labels itself at the endpoint.
use crate::command::{CadCommand, CmdResult};
use crate::scene::{
    block_distance_parameter::{self, Kind},
    WireModel,
};
use codec::{types::Vector3, Handle};
use glam::DVec3;

pub struct DistanceParameterCommand {
    document_id: u64,
    definition: Handle,
    kind: Kind,
    base: Option<DVec3>,
    end: Option<DVec3>,
    grips: Option<usize>,
}

impl DistanceParameterCommand {
    pub(crate) fn new(document_id: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document_id,
            definition,
            kind,
            base: None,
            end: None,
            grips: None,
        }
    }
    fn asking_grips(&self) -> bool {
        self.kind == Kind::XY && self.end.is_some() && self.grips.is_none()
    }
    fn finish(&self, label: DVec3, grips: Option<usize>) -> CmdResult {
        CmdResult::PlaceBlockDistanceParameter {
            document_id: self.document_id,
            definition: self.definition,
            kind: self.kind,
            base: self.base.unwrap(),
            end: self.end.unwrap(),
            label,
            grips,
        }
    }
    fn commit_grips(&self, grips: usize) -> CmdResult {
        let end = self.end.unwrap();
        self.finish(end, Some(grips))
    }
}

impl CadCommand for DistanceParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self) -> String {
        let step = if self.base.is_none() {
            "Specify parameter base point:"
        } else if self.end.is_none() {
            "Specify parameter endpoint:"
        } else if self.asking_grips() {
            "Specify number of grips [0/1/2/4] <1>:"
        } else {
            "Specify parameter label location:"
        };
        crate::tf!(
            "BPARAMETER {}  {}",
            crate::t!(self.kind.name()),
            crate::t!(step)
        )
        .into_owned()
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.asking_grips() {
            return CmdResult::ReportError(crate::t!("Requires an integer value.").into_owned());
        }
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        let Some(base) = self.base else {
            self.base = Some(point);
            return CmdResult::NeedPoint;
        };
        let vector = |p: DVec3| Vector3::new(p.x, p.y, p.z);
        if let Some(error) = block_distance_parameter::points_error(
            self.kind,
            vector(base),
            vector(self.end.unwrap_or(point)),
            vector(point),
        ) {
            return CmdResult::ReportError(crate::t!(error).into_owned());
        }
        if self.end.is_none() {
            self.end = Some(point);
            return CmdResult::NeedPoint;
        }
        self.finish(point, None)
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !self.asking_grips() {
            return None;
        }
        let answer = text.trim();
        let grips = match answer {
            "" => 1,
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "4" => 4,
            _ => {
                // Which refusal an answer gets depends on its form, not its
                // value: digits are a count this list happens not to offer,
                // anything else is no count at all. The widest integer type
                // keeps a long typed answer a count rather than a non-number.
                let refusal = if answer.parse::<i128>().is_ok() {
                    "Enter the number of grips: 0, 1, 2 or 4."
                } else {
                    "Requires an integer value."
                };
                return Some(CmdResult::ReportError(crate::t!(refusal).into_owned()));
            }
        };
        self.grips = Some(grips);
        Some(self.commit_grips(grips))
    }
    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let Some(base) = self.base.filter(|_| point.is_finite()) else {
            return vec![];
        };
        let end = self.end.unwrap_or(point);
        let mut lines = vec![[base, end]];
        if self.kind == Kind::XY {
            lines = vec![
                [base, DVec3::new(end.x, base.y, base.z)],
                [base, DVec3::new(base.x, end.y, base.z)],
            ];
        }
        if self.end.is_some() && !self.asking_grips() {
            if self.kind == Kind::XY {
                let x = DVec3::new(base.x, point.y, base.z);
                let y = DVec3::new(point.x, base.y, base.z);
                lines.extend([
                    [base, x],
                    [x, DVec3::new(end.x, point.y, base.z)],
                    [base, y],
                    [y, DVec3::new(point.x, end.y, base.z)],
                ]);
            } else {
                let d = end - base;
                let n = DVec3::new(-d.y, d.x, 0.0).normalize_or_zero();
                let offset = n * (point - base).dot(n);
                lines = vec![
                    [base, base + offset],
                    [base + offset, end + offset],
                    [end + offset, end],
                ];
            }
        }
        lines
            .into_iter()
            .map(|line| {
                WireModel::solid_f64(
                    "preview_distance_parameter".into(),
                    line.into_iter().map(|point| point.to_array()).collect(),
                    WireModel::CYAN,
                    false,
                )
            })
            .collect()
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.asking_grips() {
            self.grips = Some(1);
            return self.commit_grips(1);
        }
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_grip_count() -> DistanceParameterCommand {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY);
        for point in [DVec3::ZERO, DVec3::new(6., 2., 0.)] {
            assert!(
                matches!(command.on_point(point), CmdResult::NeedPoint),
                "the base point and the endpoint are picks"
            );
        }
        assert!(command.asking_grips(), "the XY command now wants a count");
        command
    }

    /// A picked point where a count is wanted is not a count at all, so the
    /// refusal says that rather than reciting the list.
    #[test]
    fn a_pick_at_the_grip_count_prompt_is_refused_as_not_an_integer() {
        let mut command = at_grip_count();
        assert!(matches!(
            command.on_point(DVec3::new(9., 6., 0.)),
            CmdResult::ReportError(reason) if reason == "Requires an integer value."
        ));
        assert!(matches!(
            command.on_text_input("1,8"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires an integer value."
        ));
        assert!(matches!(
            command.on_text_input("2.5"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires an integer value."
        ));
    }

    /// A whole number the option list does not offer stays a list problem.
    #[test]
    fn a_count_outside_the_list_is_refused_with_the_list() {
        for count in ["3", "-1", "8", "+2", "04", "99999999999999999999"] {
            let mut command = at_grip_count();
            assert!(
                matches!(
                    command.on_text_input(count),
                    Some(CmdResult::ReportError(reason))
                        if reason == "Enter the number of grips: 0, 1, 2 or 4."
                ),
                "{count}"
            );
        }
    }
}
