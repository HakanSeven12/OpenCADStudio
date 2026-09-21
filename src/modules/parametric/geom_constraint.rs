use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};

/// Command-line front end for the standard geometric-constraint family.
/// Each choice is handed to the existing focused command so ribbon and typed
/// entry use one implementation.
pub struct GeomConstraintCommand;

impl GeomConstraintCommand {
    pub fn new() -> Self {
        Self
    }

    fn dispatch(keyword: &str) -> Option<&'static str> {
        Some(match keyword {
            "H" | "HORIZONTAL" => "GCHORIZONTAL",
            "V" | "VERTICAL" => "VCONSTRAINT",
            "P" | "PERPENDICULAR" => "GCPERPENDICULAR",
            "PA" | "PARALLEL" => "PCONSTRAINT",
            "T" | "TANGENT" => "TCONSTRAINT",
            "SM" | "SMOOTH" => "GCSMOOTH",
            "C" | "COINCIDENT" => "GCCOINCIDENT",
            "CON" | "CONCENTRIC" => "GCCONCENTRIC",
            "COL" | "COLLINEAR" | "COLINEAR" => "LCONSTRAINT",
            "SY" | "SYMMETRIC" => "SYCONSTRAINT",
            "E" | "EQUAL" => "ECONSTRAINT",
            "F" | "FIX" => "FXCONSTRAINT",
            _ => return None,
        })
    }
}

impl CadCommand for GeomConstraintCommand {
    fn name(&self) -> &'static str {
        "GEOMCONSTRAINT"
    }

    fn prompt(&self) -> String {
        "GEOMCONSTRAINT  Enter constraint type [Horizontal/Vertical/Perpendicular/Parallel/Tangent/Smooth/Coincident/Concentric/Collinear/Symmetric/Equal/Fix] <Coincident>:".to_string()
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Horizontal", "H"),
            CmdOption::new("Vertical", "V"),
            CmdOption::new("Perpendicular", "P"),
            CmdOption::new("Parallel", "PA"),
            CmdOption::new("Tangent", "T"),
            CmdOption::new("Smooth", "SM"),
            CmdOption::new("Coincident", "C"),
            CmdOption::new("Concentric", "CON"),
            CmdOption::new("Collinear", "COL"),
            CmdOption::new("Symmetric", "SY"),
            CmdOption::new("Equal", "E"),
            CmdOption::new("Fix", "F"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text
            .trim()
            .trim_start_matches('_')
            .to_ascii_uppercase();
        Self::dispatch(&keyword).map(|command| CmdResult::Dispatch(command.to_string()))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("GCCOINCIDENT".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GEOMCONSTRAINT"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooser_dispatches_an_existing_constraint_command() {
        let mut command = GeomConstraintCommand::new();
        assert!(matches!(
            command.on_text_input("Parallel"),
            Some(CmdResult::Dispatch(found)) if found == "PCONSTRAINT"
        ));
        assert!(matches!(
            command.on_enter(),
            CmdResult::Dispatch(found) if found == "GCCOINCIDENT"
        ));
    }
}
