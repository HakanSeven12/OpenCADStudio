use crate::command::{CadCommand, CmdResult, InputKind};
use codec::Handle;
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Name,
    Description,
    Palette,
}

/// Values answered at the placement prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub name: Option<String>,
    pub description: Option<String>,
    pub palette: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Basepoint,
    Lookup,
}
impl Kind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "BASE" | "BASEPOINT" | "B" => Some(Self::Basepoint),
            "LOOKUP" => Some(Self::Lookup),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        if self == Self::Basepoint {
            "Basepoint"
        } else {
            "Lookup"
        }
    }
}
inventory::submit!(crate::command::CommandRegistration {
    names: &["BLOOKUPTABLE"]
});

pub struct SingleParameterCommand {
    document: u64,
    definition: Handle,
    kind: Kind,
    location: Option<DVec3>,
    grips: Option<usize>,
    default_name: String,
    asking: Option<Asking>,
    options: Options,
}
impl SingleParameterCommand {
    pub(crate) fn new(document: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document,
            definition,
            kind,
            location: None,
            grips: None,
            default_name: String::new(),
            asking: None,
            options: Options::default(),
        }
    }
    pub(crate) fn with_default_name(mut self, name: String) -> Self {
        self.default_name = name;
        self
    }
    /// A Lookup parameter takes its location first and then its grip count;
    /// a Basepoint is placed by its location alone.
    fn asking_grips(&self) -> bool {
        self.kind == Kind::Lookup && self.location.is_some() && self.grips.is_none()
    }
    fn place(&self) -> CmdResult {
        CmdResult::PlaceBlockSingleParameter {
            document_id: self.document,
            definition: self.definition,
            kind: self.kind,
            location: self.location.unwrap(),
            grips: self.grips,
            options: self.options.clone(),
        }
    }
    fn take_grips(&mut self, count: usize) -> CmdResult {
        self.grips = Some(count);
        self.place()
    }
}
impl CadCommand for SingleParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self) -> String {
        match self.asking {
            Some(Asking::Name) => {
                return crate::tf!("Enter parameter name <{}>:", self.default_name).into_owned()
            }
            Some(Asking::Description) => {
                return crate::t!("Enter property description:").into_owned()
            }
            Some(Asking::Palette) => {
                return crate::t!("Display property in Properties palette? [Yes/No] <Yes>:")
                    .into_owned()
            }
            None => {}
        }
        if self.asking_grips() {
            return crate::t!("Enter number of grips [0/1] <1>:").into_owned();
        }
        if self.kind == Kind::Lookup {
            return crate::tf!(
                "BPARAMETER {}  Specify parameter location or [Name/Description/Palette]:",
                crate::t!(self.kind.name())
            )
            .into_owned();
        }
        crate::tf!(
            "BPARAMETER {}  Specify parameter location:",
            crate::t!(self.kind.name())
        )
        .into_owned()
    }
    fn input_kind(&self) -> InputKind {
        match self.asking {
            Some(Asking::Name | Asking::Description) => InputKind::FreeText,
            Some(Asking::Palette) => InputKind::SingleToken,
            None if self.asking_grips() => InputKind::SingleToken,
            None => InputKind::Point,
        }
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.asking.is_none() && !self.asking_grips() && self.kind == Kind::Lookup
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Some(asking) = self.asking {
            match asking {
                Asking::Name => {
                    let name = text.trim();
                    if !name.is_empty() {
                        self.options.name = Some(name.to_owned());
                    }
                }
                Asking::Description => {
                    if !text.is_empty() {
                        self.options.description = Some(text.to_owned());
                    }
                }
                Asking::Palette => match text.trim().to_ascii_uppercase().as_str() {
                    "" => {}
                    "Y" | "YES" => self.options.palette = Some(true),
                    "N" | "NO" => self.options.palette = Some(false),
                    _ => {
                        return Some(CmdResult::ReportError(
                            crate::t!("Enter Yes or No.").into_owned(),
                        ))
                    }
                },
            }
            self.asking = None;
            return Some(CmdResult::NeedPoint);
        }
        if self.asking_grips() {
            let count = match text.trim() {
                "" => 1,
                "0" => 0,
                "1" => 1,
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Enter the number of grips: 0 or 1.").into_owned(),
                    ))
                }
            };
            return Some(self.take_grips(count));
        }
        if self.kind == Kind::Lookup {
            self.asking = match text.trim().to_ascii_uppercase().as_str() {
                "N" | "NAME" => {
                    self.options.name = None;
                    Some(Asking::Name)
                }
                "D" | "DESCRIPTION" => {
                    self.options.description = None;
                    Some(Asking::Description)
                }
                "P" | "PALETTE" => {
                    self.options.palette = None;
                    Some(Asking::Palette)
                }
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Point or option keyword required.").into_owned(),
                    ))
                }
            };
            return Some(CmdResult::NeedPoint);
        }
        None
    }
    fn on_point(&mut self, location: DVec3) -> CmdResult {
        if self.asking.is_some() {
            return CmdResult::NeedPoint;
        }
        if self.asking_grips() {
            return CmdResult::ReportError(
                crate::t!("Enter the number of grips: 0 or 1.").into_owned(),
            );
        }
        if !location.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        if self.kind == Kind::Lookup {
            self.location = Some(location);
            return CmdResult::NeedPoint;
        }
        CmdResult::PlaceBlockSingleParameter {
            document_id: self.document,
            definition: self.definition,
            kind: self.kind,
            location,
            grips: None,
            options: self.options.clone(),
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.asking.is_some() {
            self.asking = None;
            return CmdResult::NeedPoint;
        }
        if self.asking_grips() {
            return self.take_grips(1);
        }
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup() -> SingleParameterCommand {
        SingleParameterCommand::new(0, Handle::new(1), Kind::Lookup)
            .with_default_name("Lookup1".to_string())
    }

    fn placed(result: CmdResult) -> CmdResult {
        match result {
            CmdResult::PlaceBlockSingleParameter { .. } => result,
            _ => panic!("expected a placement"),
        }
    }

    fn place(command: &mut SingleParameterCommand) -> CmdResult {
        assert!(
            matches!(
                command.on_point(DVec3::new(2., 2., 0.)),
                CmdResult::NeedPoint
            ),
            "the location is the first pick"
        );
        placed(command.on_enter())
    }

    #[test]
    fn a_lookup_location_prompt_lists_the_options_it_accepts() {
        assert_eq!(
            lookup().prompt(),
            "BPARAMETER Lookup  Specify parameter location or [Name/Description/Palette]:"
        );
    }

    #[test]
    fn a_name_answer_asks_for_the_next_free_name_and_returns_to_the_location_prompt() {
        let mut command = lookup();
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <Lookup1>:");
        assert!(matches!(
            command.on_text_input("Grip9"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Lookup  Specify parameter location or [Name/Description/Palette]:"
        );
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter {
                options: Options { name: Some(ref name), .. },
                ..
            } if name == "Grip9"
        ));
    }

    #[test]
    fn an_empty_name_answer_keeps_the_default_name() {
        let mut command = lookup();
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input(""),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter {
                options: Options { name: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn a_description_answer_takes_its_text_as_written() {
        let mut command = lookup();
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter property description:");
        assert!(matches!(
            command.on_text_input("audit note"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter {
                options: Options { description: Some(ref text), .. },
                ..
            } if text == "audit note"
        ));
    }

    #[test]
    fn a_palette_answer_is_a_yes_or_no() {
        let mut command = lookup();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Display property in Properties palette? [Yes/No] <Yes>:"
        );
        assert!(matches!(
            command.on_text_input("Maybe"),
            Some(CmdResult::ReportError(reason)) if reason == "Enter Yes or No."
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter {
                options: Options {
                    palette: Some(false),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn a_palette_answer_accepts_its_default() {
        let mut command = lookup();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input(""),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter {
                options: Options { palette: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn text_that_is_not_an_option_is_refused_with_the_prompt() {
        assert!(matches!(
            lookup().on_text_input("ALL"),
            Some(CmdResult::ReportError(reason)) if reason == "Point or option keyword required."
        ));
    }

    #[test]
    fn the_grip_count_step_takes_typed_text() {
        let mut command = lookup();
        assert!(matches!(
            command.on_point(DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert_eq!(command.input_kind(), InputKind::SingleToken);
    }

    #[test]
    fn reopening_a_question_keeps_the_default_it_shows() {
        let mut command = lookup();
        assert!(matches!(command.on_text_input("N"), Some(CmdResult::NeedPoint)));
        assert!(matches!(
            command.on_text_input("Grip9"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_text_input("N"), Some(CmdResult::NeedPoint)));
        assert_eq!(command.prompt(), "Enter parameter name <Lookup1>:");
        assert!(matches!(command.on_text_input(""), Some(CmdResult::NeedPoint)));
        assert!(matches!(
            place(&mut command),
            CmdResult::PlaceBlockSingleParameter { options: Options { name: None, .. }, .. }
        ));
    }

    #[test]
    fn a_basepoint_prompt_keeps_its_single_instruction() {
        let mut command = SingleParameterCommand::new(0, Handle::new(1), Kind::Basepoint);
        assert_eq!(
            command.prompt(),
            "BPARAMETER Basepoint  Specify parameter location:"
        );
        assert!(command.on_text_input("N").is_none());
    }
}
