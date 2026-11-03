//! Authoring shared by Linear, Polar and XY parameters: Linear and Polar take a
//! label pick, XY asks for its grip count and labels itself at the endpoint.
use crate::command::{CadCommand, CmdResult, InputKind};
use crate::scene::{
    block_distance_parameter::{self, Kind},
    WireModel,
};
use codec::{types::Vector3, Handle};
use glam::DVec3;

/// Values answered at the base-point prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub name: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
    pub distance_label: Option<String>,
    pub angle_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Name,
    Chain,
    Description,
    Palette,
    DistanceLabel,
    AngleLabel,
}

impl Asking {
    /// The creation default the question's Enter answer stands for.
    fn default_answer(self) -> bool {
        self == Self::Palette
    }
    fn is_default(self, value: bool) -> bool {
        value == self.default_answer()
    }
}

pub struct DistanceParameterCommand {
    document_id: u64,
    definition: Handle,
    kind: Kind,
    base: Option<DVec3>,
    end: Option<DVec3>,
    grips: Option<usize>,
    asking: Option<Asking>,
    options: Options,
    name_default: String,
    distance_label_default: String,
    angle_label_default: String,
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
            asking: None,
            options: Options::default(),
            name_default: String::new(),
            distance_label_default: String::new(),
            angle_label_default: String::new(),
        }
    }
    pub(crate) fn with_name(mut self, name: String) -> Self {
        self.name_default = name;
        self
    }
    pub(crate) fn with_labels(mut self, distance: String, angle: String) -> Self {
        self.distance_label_default = distance;
        self.angle_label_default = angle;
        self
    }
    fn accept(&mut self, text: Option<&str>) -> CmdResult {
        let asking = self.asking.expect("an option question is open");
        if matches!(asking, Asking::DistanceLabel | Asking::AngleLabel) {
            let value = text.unwrap_or("").trim();
            match asking {
                Asking::DistanceLabel => {
                    if !value.is_empty() && value != self.distance_label_default {
                        self.options.distance_label = Some(value.to_owned());
                    }
                    self.asking = Some(Asking::AngleLabel);
                }
                Asking::AngleLabel => {
                    let distance = self
                        .options
                        .distance_label
                        .as_deref()
                        .unwrap_or(&self.distance_label_default);
                    if value.eq_ignore_ascii_case(distance) {
                        return CmdResult::ReportError(
                            crate::t!("The distance and angle labels must differ.").into_owned(),
                        );
                    }
                    if !value.is_empty() && value != self.angle_label_default {
                        self.options.angle_label = Some(value.to_owned());
                    }
                    self.asking = None;
                }
                _ => unreachable!(),
            }
            return CmdResult::NeedPoint;
        }
        if asking == Asking::Name {
            let value = text.unwrap_or("").trim();
            if !value.is_empty() && !value.eq_ignore_ascii_case(&self.name_default) {
                self.options.name = Some(value.to_owned());
            }
            self.asking = None;
            return CmdResult::NeedPoint;
        }
        if asking == Asking::Description {
            let value = text.unwrap_or("");
            if !value.is_empty() {
                self.options.description = Some(value.to_owned());
            }
            self.asking = None;
            return CmdResult::NeedPoint;
        }
        let value = match text.map(str::trim) {
            None | Some("") => asking.default_answer(),
            Some(text) => match text.to_ascii_uppercase().as_str() {
                "Y" | "YES" => true,
                "N" | "NO" => false,
                _ => return CmdResult::ReportError(crate::t!("Enter Yes or No.").into_owned()),
            },
        };
        if !asking.is_default(value) {
            match asking {
                Asking::Chain => self.options.chain = Some(value),
                Asking::Palette => self.options.palette = Some(value),
                Asking::Name | Asking::Description | Asking::DistanceLabel | Asking::AngleLabel => {
                    unreachable!()
                }
            }
        }
        self.asking = None;
        CmdResult::NeedPoint
    }
    fn asking_grips(&self) -> bool {
        self.kind == Kind::XY && self.end.is_some() && self.grips.is_none()
    }
    /// The question the open option asks, carrying the defaults it offers.
    fn question(&self, asking: Asking) -> String {
        match asking {
            Asking::Name => crate::tf!("Enter parameter name <{}>:", self.name_default).into_owned(),
            Asking::Chain => crate::t!(
                "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
            )
            .into_owned(),
            Asking::Description => crate::t!("Enter property description:").into_owned(),
            Asking::Palette => {
                crate::t!("Display property in Properties palette? [Yes/No] <Yes>:").into_owned()
            }
            Asking::DistanceLabel => crate::tf!(
                "Enter distance property label <{}>:",
                self.distance_label_default
            )
            .into_owned(),
            Asking::AngleLabel => crate::tf!(
                "Enter angle property label <{}>:",
                self.angle_label_default
            )
            .into_owned(),
        }
    }
    /// The measured properties that only a Polar parameter offers: its own
    /// name and its distance and angle labels.
    fn accepts_label(&self) -> bool {
        self.kind == Kind::Polar
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
            options: self.options.clone(),
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
        if let Some(asking) = self.asking {
            return self.question(asking);
        }
        let step = if self.base.is_none() {
            if self.kind == Kind::Linear {
                "Specify start point or [Chain/Description/Palette]:"
            } else if self.accepts_label() {
                "Specify base point or [Name/Label/Chain/Description/Palette]:"
            } else {
                "Specify base point or [Chain/Description/Palette]:"
            }
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
    fn input_kind(&self) -> InputKind {
        match self.asking {
            Some(
                Asking::Name | Asking::Description | Asking::DistanceLabel | Asking::AngleLabel,
            ) => InputKind::FreeText,
            Some(_) => InputKind::SingleToken,
            None => InputKind::Point,
        }
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.asking.is_none() && self.base.is_none()
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.asking.is_some() {
            return CmdResult::NeedPoint;
        }
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
        if self.asking.is_some() {
            return Some(self.accept(Some(text)));
        }
        if self.base.is_none() {
            let asking = match text.trim().to_ascii_uppercase().as_str() {
                "N" | "NAME" if self.accepts_label() => Asking::Name,
                "C" | "CHAIN" => Asking::Chain,
                "D" | "DESCRIPTION" => Asking::Description,
                "P" | "PALETTE" => Asking::Palette,
                "L" | "LABEL" if self.accepts_label() => Asking::DistanceLabel,
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Point or option keyword required.").into_owned(),
                    ))
                }
            };
            match asking {
                Asking::Name => self.options.name = None,
                Asking::Chain => self.options.chain = None,
                Asking::Description => self.options.description = None,
                Asking::Palette => self.options.palette = None,
                Asking::DistanceLabel => {
                    self.options.distance_label = None;
                    self.options.angle_label = None;
                }
                Asking::AngleLabel => unreachable!(),
            }
            self.asking = Some(asking);
            return Some(CmdResult::NeedPoint);
        }
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
        if self.asking.is_some() {
            return self.accept(None);
        }
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

    fn linear() -> DistanceParameterCommand {
        DistanceParameterCommand::new(0, Handle::new(1), Kind::Linear)
    }

    #[test]
    fn a_distance_base_prompt_lists_the_options_it_accepts() {
        for (kind, prompt) in [
            (
                Kind::Linear,
                "BPARAMETER Linear  Specify start point or [Chain/Description/Palette]:",
            ),
            (
                Kind::Polar,
                "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette]:",
            ),
            (
                Kind::XY,
                "BPARAMETER XY  Specify base point or [Chain/Description/Palette]:",
            ),
        ] {
            assert_eq!(
                DistanceParameterCommand::new(0, Handle::new(1), kind).prompt(),
                prompt
            );
        }
    }

    fn polar() -> DistanceParameterCommand {
        DistanceParameterCommand::new(0, Handle::new(1), Kind::Polar)
            .with_name("Polar1".into())
            .with_labels("Distance2".into(), "Angle2".into())
    }

    #[test]
    fn a_name_answer_asks_for_the_parameter_name() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <Polar1>:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("  P9  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette]:"
        );
        assert_eq!(command.options.name.as_deref(), Some("P9"));
    }

    #[test]
    fn an_empty_name_answer_keeps_the_default() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("Name"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.name, None);
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Polar1"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.name, None);
    }

    #[test]
    fn a_name_keyword_is_refused_on_types_without_a_name_question() {
        for kind in [Kind::Linear, Kind::XY] {
            let mut command = DistanceParameterCommand::new(0, Handle::new(1), kind);
            assert!(
                matches!(
                    command.on_text_input("N"),
                    Some(CmdResult::ReportError(reason))
                        if reason == "Point or option keyword required."
                ),
                "{}",
                kind.name()
            );
        }
    }

    #[test]
    fn the_name_travels_with_the_placement() {
        let mut command = polar();
        for answer in ["N", "P9"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(1., 1., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(1., 1., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.name.as_deref() == Some("P9")
        ));
    }

    #[test]
    fn a_label_answer_asks_both_property_labels_in_turn() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert!(matches!(
            command.on_text_input("Dist9"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette]:"
        );
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
        assert_eq!(command.options.angle_label.as_deref(), Some("C"));
    }

    #[test]
    fn an_empty_label_answer_keeps_the_default() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn a_label_question_takes_typed_text() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.input_kind(), InputKind::FreeText);
    }

    #[test]
    fn a_pick_at_a_label_question_keeps_the_question() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_point(DVec3::ONE), CmdResult::NeedPoint));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert!(matches!(
            command.on_text_input("Dist9"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_point(DVec3::ONE), CmdResult::NeedPoint));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
    }

    #[test]
    fn an_angle_label_equal_to_the_distance_label_is_refused() {
        let mut command = polar();
        for answer in ["L", "Dist9"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_text_input("dist9"),
            Some(CmdResult::ReportError(reason))
                if reason == "The distance and angle labels must differ."
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert_eq!(command.options.angle_label, None);
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.angle_label.as_deref(), Some("C"));
    }

    #[test]
    fn an_angle_label_equal_to_the_offered_default_is_refused() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(
            command.on_text_input("Distance2"),
            Some(CmdResult::ReportError(reason))
                if reason == "The distance and angle labels must differ."
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
    }

    #[test]
    fn a_label_answer_is_trimmed() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("  Dist9  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
        assert!(matches!(
            command.on_text_input("   "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn reopening_the_label_option_clears_both_answers() {
        let mut command = polar();
        for answer in ["L", "Dist9", "C"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn a_label_keyword_is_refused_on_types_without_labels() {
        for kind in [Kind::Linear, Kind::XY] {
            let mut command = DistanceParameterCommand::new(0, Handle::new(1), kind);
            assert!(
                matches!(
                    command.on_text_input("L"),
                    Some(CmdResult::ReportError(reason))
                        if reason == "Point or option keyword required."
                ),
                "{}",
                kind.name()
            );
        }
    }

    #[test]
    fn the_labels_travel_with_the_placement() {
        let mut command = polar();
        for answer in ["L", "Dist9", "C"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(1., 1., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(1., 1., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.distance_label.as_deref() == Some("Dist9")
                    && options.angle_label.as_deref() == Some("C")
        ));
    }

    #[test]
    fn a_chain_answer_asks_whether_associated_actions_run() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.chain, Some(true));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Linear  Specify start point or [Chain/Description/Palette]:"
        );
    }

    #[test]
    fn an_empty_chain_answer_keeps_actions_unchained() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.chain, None);
    }

    #[test]
    fn a_description_answer_takes_its_text_as_written() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter property description:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("  ramp  length  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.description.as_deref(),
            Some("  ramp  length  ")
        );
    }

    #[test]
    fn a_palette_answer_lists_in_the_palette_by_default() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Display property in Properties palette? [Yes/No] <Yes>:"
        );
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.palette, None);
    }

    #[test]
    fn a_palette_answer_can_hide_the_property() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("PALETTE"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.palette, Some(false));
    }

    #[test]
    fn a_yes_or_no_answer_outside_yes_or_no_is_refused() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        let Some(CmdResult::ReportError(error)) = command.on_text_input("maybe") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Enter Yes or No.");
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }

    #[test]
    fn text_that_is_not_an_option_is_refused_with_the_prompt() {
        let Some(CmdResult::ReportError(error)) = linear().on_text_input("Z") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn options_can_be_answered_in_any_order() {
        let mut command = linear();
        for answer in ["P", "No", "D", "audit note", "CHAIN", "Yes"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        assert_eq!(command.options.chain, Some(true));
        assert_eq!(command.options.description.as_deref(), Some("audit note"));
        assert_eq!(command.options.palette, Some(false));
    }

    #[test]
    fn a_click_while_an_option_is_open_keeps_the_question() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert!(
            command.base.is_none(),
            "the click did not become the base point"
        );
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }

    #[test]
    fn a_keyword_after_the_base_point_is_not_an_option() {
        let mut command = linear();
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert!(command.on_text_input("C").is_none());
    }

    #[test]
    fn reopening_an_option_keeps_the_default_it_shows() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.palette, None);
    }

    #[test]
    fn the_answers_travel_with_the_placement() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("ramp"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(2., 2., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(2., 2., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.chain == Some(true)
                    && options.description.as_deref() == Some("ramp")
                    && options.palette == Some(false)
        ));
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
