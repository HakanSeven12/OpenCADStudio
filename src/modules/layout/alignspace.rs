// ALIGNSPACE — pan (one point) or pan, zoom and rotate (two points) a layout
// viewport's view so model points land on chosen sheet points:
//   First alignment point in model space:          (inside the viewport)
//   Second alignment point in model space <none>:
//   First alignment point in paper space:          (on the sheet)
//   Second alignment point in paper space:
// The host switches spaces between the two halves; each half hands its
// points to `ALIGNSPACE_MODEL` / `ALIGNSPACE_PAPER x y z …`.

use crate::command::{CadCommand, CmdResult};
use crate::t;
use glam::DVec3;

pub struct AlignSpacePointsCommand {
    /// `ALIGNSPACE_MODEL` or `ALIGNSPACE_PAPER`.
    name: &'static str,
    /// The model half asks for one or two points; the paper half for as
    /// many as the model half got.
    count: usize,
    points: Vec<DVec3>,
}

impl AlignSpacePointsCommand {
    pub fn model() -> Self {
        Self {
            name: "ALIGNSPACE_MODEL",
            count: 2,
            points: Vec::new(),
        }
    }

    pub fn paper(count: usize) -> Self {
        Self {
            name: "ALIGNSPACE_PAPER",
            count,
            points: Vec::new(),
        }
    }

    fn finish(&self) -> CmdResult {
        let points: Vec<String> = self
            .points
            .iter()
            .map(|p| format!("{} {} {}", p.x, p.y, p.z))
            .collect();
        CmdResult::Dispatch(format!("{} {}", self.name, points.join(" ")))
    }
}

impl CadCommand for AlignSpacePointsCommand {
    fn name(&self) -> &'static str {
        "ALIGNSPACE"
    }

    fn prompt(&self) -> String {
        let prompt = match (self.name, self.points.len()) {
            ("ALIGNSPACE_MODEL", 0) => "ALIGNSPACE  Specify first alignment point in model space:",
            ("ALIGNSPACE_MODEL", _) => {
                "ALIGNSPACE  Specify second alignment point in model space <none>:"
            }
            (_, 0) => "ALIGNSPACE  Specify first alignment point in paper space:",
            _ => "ALIGNSPACE  Specify second alignment point in paper space:",
        };
        t!(prompt).into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.points.push(point);
        if self.points.len() < self.count {
            return CmdResult::NeedPoint;
        }
        self.finish()
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter after one model point aligns by that point alone.
        if self.name == "ALIGNSPACE_MODEL" && self.points.len() == 1 {
            return self.finish();
        }
        CmdResult::Cancel
    }
}
