//! Interactive placement of the supported Visibility action parameter.

use crate::command::{CadCommand, CmdOption, CmdResult};
use codec::Handle;
use glam::DVec3;

pub struct VisibilityParameterCommand {
    document_id: u64,
    definition: Handle,
    choosing_type: bool,
}

impl VisibilityParameterCommand {
    pub fn new(document_id: u64, definition: Handle, choosing_type: bool) -> Self {
        Self {
            document_id,
            definition,
            choosing_type,
        }
    }
}

impl CadCommand for VisibilityParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }

    fn prompt(&self) -> String {
        if self.choosing_type {
            crate::t!(
                "BPARAMETER  Enter parameter type [Alignment/Base/pOint/Linear/Polar/Xy/Rotation/Flip/Visibility/lookup]:"
            )
            .into_owned()
        } else {
            crate::t!("BPARAMETER  Specify visibility parameter location:").into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.choosing_type {
            vec![
                CmdOption::new(crate::t!("Alignment").as_ref(), "A"),
                CmdOption::new(crate::t!("Base").as_ref(), "B"),
                CmdOption::new(crate::t!("Point").as_ref(), "O"),
                CmdOption::new(crate::t!("Linear").as_ref(), "L"),
                CmdOption::new(crate::t!("Polar").as_ref(), "P"),
                CmdOption::new(crate::t!("XY").as_ref(), "X"),
                CmdOption::new(crate::t!("Rotation").as_ref(), "R"),
                CmdOption::new(crate::t!("Flip").as_ref(), "F"),
                CmdOption::new(crate::t!("Visibility").as_ref(), "V"),
                CmdOption::new(crate::t!("Lookup").as_ref(), "LOOKUP"),
            ]
        } else {
            vec![]
        }
    }

    fn wants_text_input(&self) -> bool {
        self.choosing_type
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !self.choosing_type {
            return None;
        }
        Some(
            match text
                .trim()
                .trim_start_matches('_')
                .to_ascii_uppercase()
                .as_str()
            {
                "L" | "LINEAR" => CmdResult::Dispatch("BPARAMETER LINEAR".into()),
                "P" | "PO" | "POLAR" => CmdResult::Dispatch("BPARAMETER POLAR".into()),
                "X" | "XY" => CmdResult::Dispatch("BPARAMETER XY".into()),
                "R" | "ROTATION" => CmdResult::Dispatch("BPARAMETER ROTATION".into()),
                "A" | "ALIGNMENT" => CmdResult::Dispatch("BPARAMETER ALIGNMENT".into()),
                "F" | "FLIP" => CmdResult::Dispatch("BPARAMETER FLIP".into()),
                "LOOKUP" => CmdResult::Dispatch("BPARAMETER LOOKUP".into()),
                "B" | "BASE" | "BASEPOINT" => CmdResult::Dispatch("BPARAMETER BASEPOINT".into()),
                "O" | "POINT" => CmdResult::Dispatch("BPARAMETER POINT".into()),
                "V" | "VISIBILITY" => {
                    self.choosing_type = false;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError(crate::t!("Invalid option keyword.").into_owned()),
            },
        )
    }

    fn on_point(&mut self, location: DVec3) -> CmdResult {
        if self.choosing_type {
            return CmdResult::ReportError(crate::t!("Invalid option keyword.").into_owned());
        }
        if !location.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        CmdResult::PlaceBlockVisibilityParameter {
            document_id: self.document_id,
            definition: self.definition,
            location,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.choosing_type {
            self.on_text_input("").unwrap()
        } else {
            CmdResult::Cancel
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["BPARAMETER"]
});
