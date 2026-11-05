// COMPARE — show what differs from another drawing and step through it.
//
//   COMPARE  <added> added, <removed> removed, <modified> modified
//   [Next/Previous/Export/eXit]:
//
// Export asks for a CSV report path (Enter takes the one proposed).

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::compare::Comparison;
use crate::scene::WireModel;
use glam::DVec3;

pub struct CompareCommand {
    comparison: Comparison,
    /// The difference shown last; `None` before the first step.
    current: Option<usize>,
    /// The two drawings, named in the report.
    names: (String, String),
    /// Proposed report path; `Some` while asking for one.
    report: std::path::PathBuf,
    asking_report: bool,
}

impl CompareCommand {
    pub fn new(comparison: Comparison, names: (String, String), report: std::path::PathBuf) -> Self {
        Self {
            comparison,
            current: None,
            names,
            report,
            asking_report: false,
        }
    }

    fn write_report(&mut self, path: &str) -> CmdResult {
        self.asking_report = false;
        let path = match path.trim().trim_matches('"') {
            "" => self.report.clone(),
            typed => std::path::PathBuf::from(typed),
        };
        let csv = self.comparison.csv(&self.names.0, &self.names.1);
        match std::fs::write(&path, csv) {
            Ok(()) => {
                self.report = path.clone();
                CmdResult::ReportMeasurement(
                    crate::tf!("Comparison report written to {}", path.display()).into_owned(),
                )
            }
            Err(error) => CmdResult::ReportError(crate::tf!("COMPARE: {error}").into_owned()),
        }
    }

    fn show(&mut self, forward: bool) -> CmdResult {
        let count = self.comparison.differences.len();
        if count == 0 {
            return CmdResult::NeedPoint;
        }
        let next = match (self.current, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
        };
        self.current = Some(next);
        let (min, max) = self.comparison.differences[next].bounds;
        // A margin around the difference so it is seen in its context.
        let margin = ((max - min).length() * 0.5).max(1.0);
        CmdResult::Show {
            p1: min - DVec3::splat(margin),
            p2: max + DVec3::splat(margin),
        }
    }
}

impl CadCommand for CompareCommand {
    fn name(&self) -> &'static str {
        "COMPARE"
    }

    fn prompt(&self) -> String {
        if self.asking_report {
            let path = self.report.display();
            return crate::tf!("COMPARE  Report file <{path}>:").into_owned();
        }
        let comparison = &self.comparison;
        let (added, removed, modified) = (comparison.added, comparison.removed, comparison.modified);
        let position = match self.current {
            Some(index) => {
                let d = &comparison.differences[index];
                format!(
                    " {}/{} {} {}",
                    index + 1,
                    comparison.differences.len(),
                    crate::t!(d.change.name()),
                    d.object
                )
            }
            None => String::new(),
        };
        crate::tf!(
            "COMPARE  {added} added, {removed} removed, {modified} modified{position} [Next/Previous/Export/eXit]:"
        )
        .into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.asking_report {
            return Vec::new();
        }
        vec![
            CmdOption::new("Next", "N"),
            CmdOption::new("Previous", "P"),
            CmdOption::new("Export", "E"),
            CmdOption::new("eXit", "X"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.asking_report {
            return Some(self.write_report(text));
        }
        match text
            .trim()
            .trim_start_matches('_')
            .to_ascii_uppercase()
            .as_str()
        {
            "" | "N" | "NEXT" => Some(self.show(true)),
            "P" | "PREVIOUS" => Some(self.show(false)),
            "E" | "EXPORT" => {
                self.asking_report = true;
                Some(CmdResult::NeedPoint)
            }
            "X" | "EXIT" => Some(CmdResult::Cancel),
            _ => None,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.asking_report {
            return self.write_report("");
        }
        self.show(true)
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.asking_report {
            self.asking_report = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        // ponytail: the overlay is cloned per frame; cache it in the scene if
        // comparisons of very large drawings stutter.
        self.comparison.overlay.clone()
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["COMPARE"]
});
