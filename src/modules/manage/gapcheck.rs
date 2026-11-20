// GAPCHECK — find small openings between the loose ends of selected curves
// and step through them. Read-only: nothing in the drawing changes.
//
//   GAPCHECK  Maximum gap distance <1>:
//   GAPCHECK  <n> gap(s) up to <d> [Next/Previous/Distance/eXit]:
//
// A loose end is an end of an open curve that no other end of the selection
// meets. Each loose end is paired with the nearest other loose end within the
// distance; the two ends of one open curve count too.

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::WireModel;
use document::{EntityType, Handle};
use glam::DVec3;
use std::sync::atomic::{AtomicU64, Ordering};

/// Ends closer than this meet; they are not a gap.
const COINCIDENT: f64 = 1e-6;

/// The distance asked last, offered as the default next time.
static LAST_DISTANCE: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000); // 1.0

#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    pub a: (Handle, DVec3),
    pub b: (Handle, DVec3),
}

impl Gap {
    pub fn distance(&self) -> f64 {
        self.a.1.distance(self.b.1)
    }
}

/// The two ends of every open, bounded curve among `entities`.
pub fn curve_ends<'a>(
    entities: impl IntoIterator<Item = (Handle, &'a EntityType)>,
) -> Vec<(Handle, DVec3)> {
    let mut ends = Vec::new();
    for (handle, entity) in entities {
        let Some(curve) = crate::entities::curve::entity_curve(entity) else {
            continue;
        };
        if curve.extent() != ocs_geom::geom2d::Extent::Bounded || curve.is_closed() {
            continue;
        }
        for t in [0.0, 1.0] {
            ends.push((handle, DVec3::from_array(curve.point_at(t))));
        }
    }
    ends
}

/// Gaps between loose ends no more than `max` apart, nearest first per end.
pub fn find_gaps(ends: &[(Handle, DVec3)], max: f64) -> Vec<Gap> {
    // ponytail: O(n²) over the selected ends; a grid if selections of
    // tens of thousands of curves get slow.
    let loose: Vec<_> = (0..ends.len())
        .filter(|&i| {
            !(0..ends.len()).any(|j| j != i && ends[i].1.distance(ends[j].1) <= COINCIDENT)
        })
        .collect();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut paired = vec![false; ends.len()];
    for &i in &loose {
        if paired[i] {
            continue;
        }
        let nearest = loose
            .iter()
            .copied()
            .filter(|&j| j != i && !paired[j])
            .map(|j| (j, ends[i].1.distance(ends[j].1)))
            .filter(|&(_, d)| d <= max)
            .min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((j, _)) = nearest {
            paired[i] = true;
            paired[j] = true;
            gaps.push(Gap {
                a: ends[i],
                b: ends[j],
            });
        }
    }
    gaps.sort_by(|x, y| x.distance().total_cmp(&y.distance()));
    gaps
}

pub struct GapCheckCommand {
    ends: Vec<(Handle, DVec3)>,
    max: f64,
    gaps: Vec<Gap>,
    current: Option<usize>,
    asking_distance: bool,
}

impl GapCheckCommand {
    pub fn new(ends: Vec<(Handle, DVec3)>) -> Self {
        Self {
            ends,
            max: f64::from_bits(LAST_DISTANCE.load(Ordering::Relaxed)),
            gaps: Vec::new(),
            current: None,
            asking_distance: true,
        }
    }

    fn scan(&mut self, max: f64) -> CmdResult {
        if !(max.is_finite() && max > 0.0) {
            return CmdResult::ReportError("Requires a positive distance.".into());
        }
        LAST_DISTANCE.store(max.to_bits(), Ordering::Relaxed);
        self.max = max;
        self.gaps = find_gaps(&self.ends, max);
        self.current = None;
        self.asking_distance = false;
        if self.gaps.is_empty() {
            return CmdResult::ReportMeasurement(format!("No gaps up to {max}."));
        }
        self.show(true)
    }

    fn show(&mut self, forward: bool) -> CmdResult {
        let count = self.gaps.len();
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
        let gap = &self.gaps[next];
        let mid = (gap.a.1 + gap.b.1) * 0.5;
        // Enough room around the gap to see the two curves leave it.
        let margin = (self.max * 5.0).max(gap.distance() * 5.0);
        CmdResult::Show {
            p1: mid - DVec3::splat(margin),
            p2: mid + DVec3::splat(margin),
        }
    }

    fn marker(&self, gap: &Gap, current: bool) -> Vec<WireModel> {
        let color = if current {
            WireModel::HOVER
        } else {
            WireModel::CYAN
        };
        let r = self.max * if current { 1.0 } else { 0.5 };
        let square = |c: DVec3| {
            let pts = [(-r, -r), (r, -r), (r, r), (-r, r), (-r, -r)]
                .map(|(x, y)| [c.x + x, c.y + y, c.z]);
            WireModel::solid_f64("gapcheck".into(), pts.to_vec(), color, false)
        };
        vec![
            square(gap.a.1),
            square(gap.b.1),
            WireModel::solid_f64(
                "gapcheck".into(),
                vec![gap.a.1.to_array(), gap.b.1.to_array()],
                color,
                false,
            ),
        ]
    }
}

impl CadCommand for GapCheckCommand {
    fn name(&self) -> &'static str {
        "GAPCHECK"
    }

    fn prompt(&self) -> String {
        if self.asking_distance {
            return format!("GAPCHECK  Maximum gap distance <{}>:", self.max);
        }
        let position = match self.current {
            Some(index) => {
                let gap = &self.gaps[index];
                format!(
                    " {}/{} gap {:.6} between {:X} and {:X}",
                    index + 1,
                    self.gaps.len(),
                    gap.distance(),
                    gap.a.0.value(),
                    gap.b.0.value()
                )
            }
            None => String::new(),
        };
        format!(
            "GAPCHECK  {} gap(s) up to {}{position} [Next/Previous/Distance/eXit]:",
            self.gaps.len(),
            self.max
        )
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.asking_distance {
            return Vec::new();
        }
        vec![
            CmdOption::new("Next", "N"),
            CmdOption::new("Previous", "P"),
            CmdOption::new("Distance", "D"),
            CmdOption::new("eXit", "X"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if self.asking_distance {
            return Some(match text {
                "" => self.scan(self.max),
                _ => match text.parse::<f64>() {
                    Ok(max) => self.scan(max),
                    Err(_) => CmdResult::ReportError("Requires a positive distance.".into()),
                },
            });
        }
        match text.trim_start_matches('_').to_ascii_uppercase().as_str() {
            "" | "N" | "NEXT" => Some(self.show(true)),
            "P" | "PREVIOUS" => Some(self.show(false)),
            "D" | "DISTANCE" => {
                self.asking_distance = true;
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
        if self.asking_distance {
            return self.scan(self.max);
        }
        self.show(true)
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.asking_distance && !self.gaps.is_empty() {
            self.asking_distance = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        self.gaps
            .iter()
            .enumerate()
            .flat_map(|(i, gap)| self.marker(gap, self.current == Some(i)))
            .collect()
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GAPCHECK"]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn line(h: u64, a: [f64; 2], b: [f64; 2]) -> [(Handle, DVec3); 2] {
        [
            (Handle::new(h), DVec3::new(a[0], a[1], 0.0)),
            (Handle::new(h), DVec3::new(b[0], b[1], 0.0)),
        ]
    }

    #[test]
    fn the_issue_rectangle_has_one_gap_only_above_the_threshold() {
        let ends: Vec<_> = [
            line(1, [0.0, 0.0], [100.0, 0.0]),
            line(2, [100.0, 0.0], [100.0, 50.0]),
            line(3, [100.0, 50.0], [0.0, 50.0]),
            line(4, [0.0, 50.0], [0.0, 0.1]),
        ]
        .concat();
        let gaps = find_gaps(&ends, 0.2);
        assert_eq!(gaps.len(), 1);
        assert!((gaps[0].distance() - 0.1).abs() < 1e-12);
        assert!(find_gaps(&ends, 0.05).is_empty());
    }
}
