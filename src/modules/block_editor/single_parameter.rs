use crate::command::{CadCommand, CmdResult};
use codec::Handle;
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Basepoint,
    Lookup,
}
impl Kind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "BASE" | "BASEPOINT" | "B" => Some(Self::Basepoint),
            "LOOKUP" | "K" => Some(Self::Lookup),
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
}
impl SingleParameterCommand {
    pub(crate) fn new(document: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document,
            definition,
            kind,
            location: None,
            grips: None,
        }
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
        if self.asking_grips() {
            return crate::t!("Enter number of grips [0/1] <1>:").into_owned();
        }
        crate::tf!(
            "BPARAMETER {}  Specify parameter location:",
            crate::t!(self.kind.name())
        )
        .into_owned()
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !self.asking_grips() {
            return None;
        }
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
        Some(self.take_grips(count))
    }
    fn on_point(&mut self, location: DVec3) -> CmdResult {
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
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.asking_grips() {
            return self.take_grips(1);
        }
        CmdResult::Cancel
    }
}
