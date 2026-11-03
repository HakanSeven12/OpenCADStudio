//! Two-point authoring gesture: parameter position followed by label position.
use crate::command::{CadCommand, CmdResult, InputKind};
use crate::scene::WireModel;
use codec::Handle;
use glam::DVec3;

/// Values answered at the placement prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub chain: Option<bool>,
    pub palette: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Chain,
    Palette,
}

impl Asking {
    fn question(self) -> &'static str {
        match self {
            Self::Chain => {
                "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
            }
            Self::Palette => "Display property in Properties palette? [Yes/No] <Yes>:",
        }
    }
    /// The creation default the question's Enter answer stands for.
    fn default_answer(self) -> bool {
        self == Self::Palette
    }
    fn is_default(self, value: bool) -> bool {
        value == self.default_answer()
    }
}

pub struct PointParameterCommand {
    document_id: u64,
    definition: Handle,
    location: Option<DVec3>,
    asking: Option<Asking>,
    options: Options,
}

impl PointParameterCommand {
    pub fn new(document_id: u64, definition: Handle) -> Self {
        Self {
            document_id,
            definition,
            location: None,
            asking: None,
            options: Options::default(),
        }
    }
}

impl PointParameterCommand {
    fn accept(&mut self, text: Option<&str>) -> CmdResult {
        let asking = self.asking.expect("an option question is open");
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
            }
        }
        self.asking = None;
        CmdResult::NeedPoint
    }
}

impl CadCommand for PointParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }

    fn prompt(&self) -> String {
        if let Some(asking) = self.asking {
            return crate::t!(asking.question()).into_owned();
        }
        crate::t!(if self.location.is_some() {
            "BPARAMETER  Specify point parameter label location:"
        } else {
            "BPARAMETER  Specify parameter location or [Chain/Palette]:"
        })
        .into_owned()
    }

    fn input_kind(&self) -> InputKind {
        if self.asking.is_some() {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.asking.is_none() && self.location.is_none()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.asking.is_some() {
            return Some(self.accept(Some(text)));
        }
        if self.location.is_some() {
            return None;
        }
        let asking = match text.trim().to_ascii_uppercase().as_str() {
            "C" | "CHAIN" => Asking::Chain,
            "P" | "PALETTE" => Asking::Palette,
            _ => {
                return Some(CmdResult::ReportError(
                    crate::t!("Point or option keyword required.").into_owned(),
                ))
            }
        };
        match asking {
            Asking::Chain => self.options.chain = None,
            Asking::Palette => self.options.palette = None,
        }
        self.asking = Some(asking);
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.asking.is_some() {
            return CmdResult::NeedPoint;
        }
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        if let Some(location) = self.location {
            CmdResult::PlaceBlockPointParameter {
                document_id: self.document_id,
                definition: self.definition,
                location,
                label: point,
                options: self.options.clone(),
            }
        } else {
            self.location = Some(point);
            CmdResult::NeedPoint
        }
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        self.location
            .filter(|_| point.is_finite())
            .map(|location| {
                vec![WireModel::solid_f64(
                    "preview_point_parameter".into(),
                    vec![location.to_array(), point.to_array()],
                    WireModel::CYAN,
                    false,
                )]
            })
            .unwrap_or_default()
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.asking.is_some() {
            return self.accept(None);
        }
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> PointParameterCommand {
        PointParameterCommand::new(0, Handle::new(1))
    }

    fn placed(result: CmdResult) -> CmdResult {
        match result {
            CmdResult::PlaceBlockPointParameter { .. } => result,
            _ => panic!("expected a placement"),
        }
    }

    fn place(command: &mut PointParameterCommand) -> CmdResult {
        assert!(
            matches!(
                command.on_point(DVec3::new(2., 2., 0.)),
                CmdResult::NeedPoint
            ),
            "the location is the first pick"
        );
        placed(command.on_point(DVec3::new(3., 3., 0.)))
    }

    #[test]
    fn a_point_location_prompt_lists_the_options_it_accepts() {
        assert_eq!(
            command().prompt(),
            "BPARAMETER  Specify parameter location or [Chain/Palette]:"
        );
    }

    #[test]
    fn a_chain_answer_asks_whether_associated_actions_run() {
        let mut command = command();
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
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options {
                    chain: Some(true),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn an_empty_chain_answer_keeps_actions_unchained() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options { chain: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn a_palette_answer_lists_in_the_palette_by_default() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Display property in Properties palette? [Yes/No] <Yes>:"
        );
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options { palette: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn a_palette_answer_can_hide_the_property() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("PALETTE"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options {
                    palette: Some(false),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn a_yes_or_no_answer_outside_yes_or_no_is_refused() {
        let mut command = command();
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
        let Some(CmdResult::ReportError(error)) = command().on_text_input("Z") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn the_label_location_step_takes_a_point_without_options() {
        let mut command = command();
        assert!(matches!(
            command.on_point(DVec3::new(2., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER  Specify point parameter label location:"
        );
        assert!(command.on_text_input("C").is_none());
    }

    #[test]
    fn reopening_an_option_keeps_the_default_it_shows() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options { chain: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn options_can_be_answered_in_any_order() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options {
                    chain: Some(true),
                    palette: Some(false),
                },
                ..
            }
        ));
    }

    #[test]
    fn reopening_the_palette_keeps_the_default_it_shows() {
        let mut command = command();
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
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockPointParameter {
                options: Options { palette: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn a_click_while_an_option_is_open_keeps_the_question() {
        let mut command = command();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }
}
