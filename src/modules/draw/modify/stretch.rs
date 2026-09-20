// Stretch tool — ribbon definition + interactive command.
//
// Command:  STRETCH (SS)
//   Workflow:
//     1. Pick first corner of the crossing window (right-to-left = crossing).
//     2. Pick second corner.
//     3. Pick base point.
//     4. Pick new point → stretches only vertices inside the crossing window.
//
//   Entity behaviour:
//     Line        : move start if inside, move end if inside, move both if both inside.
//     LwPolyline  : move each vertex independently.
//     Polyline/P2D: move each vertex independently.
//     Arc / Circle: move the whole entity if its center is inside the window.
//     Insert      : move the whole entity if its insertion point is inside.
//     All others  : move the whole entity if any point is inside.

use crate::t;
use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "STRETCH",
        label: "Stretch",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/stretch.svg")),
        event: ModuleEvent::Command("STRETCH".to_string()),
    }
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    /// Waiting for the first corner of another crossing window.
    WindowCorner1,
    /// Waiting for the opposite corner.
    WindowCorner2(DVec3),
    /// Selection is complete; waiting for the displacement base point.
    Base,
    /// Waiting for the displacement target.
    Target { base: DVec3 },
}

pub struct StretchCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    /// Independent crossing windows accumulated during the selection stage.
    windows: Vec<(DVec3, DVec3)>,
    step: Step,
}

impl StretchCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            windows: Vec::new(),
            step: Step::WindowCorner1,
        }
    }

    /// Continue gathering crossing windows after the host has resolved the
    /// entities touched by the latest window.
    pub fn with_windows(
        handles: Vec<Handle>,
        wire_models: Vec<WireModel>,
        windows: Vec<(DVec3, DVec3)>,
    ) -> Self {
        Self {
            handles,
            wire_models,
            windows,
            step: Step::WindowCorner1,
        }
    }
    /// A window enclosing every vertex of the current selection, or `None`
    /// when nothing is selected. Padded so a vertex exactly on the boundary
    /// counts as inside rather than depending on the comparison's edge.
    fn selection_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        let mut any = false;
        for wire in &self.wire_models {
            for index in 0..wire.points.len() {
                let point = wire_point(wire, index);
                if !point.is_finite() {
                    continue;
                }
                min = min.min(point);
                max = max.max(point);
                any = true;
            }
        }
        if !any {
            return None;
        }
        let pad = (max - min).length().max(1.0) * 1e-6;
        Some((min - DVec3::splat(pad), max + DVec3::splat(pad)))
    }
}

/// A wire vertex rebuilt from its double-single halves.
fn wire_point(wire: &WireModel, index: usize) -> DVec3 {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    DVec3::new(
        high[0] as f64 + low[0] as f64,
        high[1] as f64 + low[1] as f64,
        high[2] as f64 + low[2] as f64,
    )
}

impl CadCommand for StretchCommand {
    fn name(&self) -> &'static str {
        "STRETCH"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::WindowCorner1 => {
                if self.windows.is_empty() && self.handles.is_empty() {
                    t!("STRETCH  Specify first corner of crossing window:").into_owned()
                } else {
                    t!(
                        "STRETCH  Specify first corner of another crossing window, or press Enter to continue:"
                    )
                    .into_owned()
                }
            }
            Step::WindowCorner2(_) => t!("STRETCH  Specify opposite corner:").into_owned(),
            Step::Base => t!("STRETCH  Specify base point:").into_owned(),
            Step::Target { base } => {
                let bx = format!("{:.3}", base.x);
                let bz = format!("{:.3}", base.z);
                t!(
                    "STRETCH  Specify new point  [base %{bx},%{bz}]:",
                    bx = bx,
                    bz = bz
                )
                .into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::WindowCorner1 => {
                self.step = Step::WindowCorner2(pt);
                CmdResult::NeedPoint
            }

            Step::WindowCorner2(c1) => {
                let win_min = c1.min(pt);
                let win_max = c1.max(pt);

                let mut windows = self.windows.clone();
                windows.push((win_min, win_max));

                // Hand the accumulated selection back to the host. The host resolves
                // the entities touched by this window and relaunches STRETCH still in
                // the selection stage, so another crossing window can be drawn.
                CmdResult::StretchWindow {
                    handles: self.handles.clone(),
                    windows,
                }
            }

            Step::Base => {
                self.step = Step::Target { base: pt };
                CmdResult::NeedPoint
            }

            Step::Target { base } => {
                let delta = pt - *base;

                CmdResult::StretchEntities {
                    handles: self.handles.clone(),
                    windows: self.windows.clone(),
                    delta,
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if let Step::WindowCorner1 = self.step {
            // Preserve the existing preselection behaviour: if STRETCH started
            // from a selected set and no explicit crossing window was drawn,
            // treat its complete bounds as the stretch window.
            if self.windows.is_empty() {
                if let Some((win_min, win_max)) = self.selection_bounds() {
                    self.windows.push((win_min, win_max));
                }
            }

            if !self.handles.is_empty() && !self.windows.is_empty() {
                self.step = Step::Base;
                return CmdResult::NeedPoint;
            }
        }

        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn window_corner_pick(&self) -> bool {
        // The two crossing-window corners are free points; Ortho/Polar must not
        // pin the opposite corner to an axis or the window becomes a line (#291).
        matches!(self.step, Step::WindowCorner1 | Step::WindowCorner2(_))
    }

    fn window_first_corner(&self) -> Option<DVec3> {
        // Expose the first corner so the host draws a filled crossing marquee to
        // the cursor, matching a normal box selection instead of a bare outline.
        match &self.step {
            Step::WindowCorner2(c1) => Some(*c1),
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.step {
            // The crossing-window rectangle is drawn as a filled selection
            // marquee by the host (via window_first_corner) so it matches a
            // normal box selection — nothing to draw here. (#291)
            Step::WindowCorner2(_) => vec![],
            Step::Target { base } => {
                let delta = pt - *base;

                let windows: Vec<_> = self
                    .windows
                    .iter()
                    .map(|(win_min, win_max)| (win_min.as_vec3(), win_max.as_vec3()))
                    .collect();

                let mut out: Vec<WireModel> = self
                    .wire_models
                    .iter()
                    .map(|wire| wire.stretched_windows(&windows, delta.as_vec3()))
                    .collect();

                out.push(WireModel::solid(
                    "rubber_band".into(),
                    vec![
                        [base.x as f32, base.y as f32, base.z as f32],
                        [pt.x as f32, pt.y as f32, pt.z as f32],
                    ],
                    WireModel::CYAN,
                    false,
                ));

                out
            }
            _ => vec![],
        }
    }
}
