// COMPARE — show what differs from another drawing and step through it.
//
//   COMPARE  <added> only here (green), <removed> only in the other drawing
//   (red) [Next/Previous/eXit]:

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::compare::Comparison;
use crate::scene::WireModel;
use glam::DVec3;

pub struct CompareCommand {
    comparison: Comparison,
    /// The difference shown last; `None` before the first step.
    current: Option<usize>,
}

impl CompareCommand {
    pub fn new(comparison: Comparison) -> Self {
        Self {
            comparison,
            current: None,
        }
    }

    fn show(&mut self, forward: bool) -> CmdResult {
        let count = self.comparison.spots.len();
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
        let (min, max) = self.comparison.spots[next];
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
        let (added, removed) = (self.comparison.added, self.comparison.removed);
        let position = match self.current {
            Some(index) => format!(" {}/{}", index + 1, self.comparison.spots.len()),
            None => String::new(),
        };
        crate::tf!(
            "COMPARE  {added} only here (green), {removed} only in the other drawing (red){position} [Next/Previous/eXit]:"
        )
        .into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Next", "N"),
            CmdOption::new("Previous", "P"),
            CmdOption::new("eXit", "X"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text
            .trim()
            .trim_start_matches('_')
            .to_ascii_uppercase()
            .as_str()
        {
            "" | "N" | "NEXT" => Some(self.show(true)),
            "P" | "PREVIOUS" => Some(self.show(false)),
            "X" | "EXIT" => Some(CmdResult::Cancel),
            _ => None,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.show(true)
    }

    fn on_escape(&mut self) -> CmdResult {
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
