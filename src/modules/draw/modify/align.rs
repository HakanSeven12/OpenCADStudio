// ALIGN command — align selected objects using 1 or 2 point pairs.
//
// Workflow:
//   1. Select objects (Enter to finish selection)
//   2. First source point → first destination point
//   3. Second source point → second destination point (Enter to skip = translate only)
//   4. Enter = apply (scale = optional: Y/N prompt after 2nd pair)
//
// With 1 pair:  pure translation (src1 → dst1)
// With 2 pairs: translate + rotate (+ optional uniform scale to fit)

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, EntityTransform};
use crate::scene::model::wire_model::WireModel;

pub struct AlignCommand {
    state: AlignState,
    handles: Vec<Handle>,
    src1: Option<DVec3>,
    dst1: Option<DVec3>,
    src2: Option<DVec3>,
    dst2: Option<DVec3>,
}

#[derive(PartialEq)]
enum AlignState {
    Gathering,
    Src1,
    Dst1,
    Src2,
    Dst2,
    AskScale,
}

impl AlignCommand {
    pub fn with_selection(handles: Vec<Handle>) -> Self {
        let state = if handles.is_empty() {
            AlignState::Gathering
        } else {
            AlignState::Src1
        };

        Self {
            state,
            handles,
            src1: None,
            dst1: None,
            src2: None,
            dst2: None,
        }
    }
}

impl CadCommand for AlignCommand {
    fn name(&self) -> &'static str {
        "ALIGN"
    }

    fn prompt(&self) -> String {
        match self.state {
            AlignState::Gathering => t!(
                "ALIGN  Select objects (%{count} selected, Enter when done):",
                count = self.handles.len()
            )
            .into_owned(),
            AlignState::Src1 => t!("ALIGN  Specify 1st source point:").into_owned(),
            AlignState::Dst1 => t!("ALIGN  Specify 1st destination point:").into_owned(),
            AlignState::Src2 => {
                t!("ALIGN  Specify 2nd source point (Enter = translate only):").into_owned()
            }
            AlignState::Dst2 => t!("ALIGN  Specify 2nd destination point:").into_owned(),
            AlignState::AskScale => {
                t!(
                    "ALIGN  Scale objects based on alignment points? [Yes / No]:"
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;

        match self.state {
            AlignState::AskScale => vec![
                CmdOption::new(t!("Yes").as_ref(), "Y"),
                CmdOption::new(t!("No").as_ref(), "N"),
            ],
            _ => vec![],
        }
    }

    fn is_selection_gathering(&self) -> bool {
        self.state == AlignState::Gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.state {
            AlignState::Gathering => CmdResult::NeedPoint,
            AlignState::Src1 => {
                self.src1 = Some(pt);
                self.state = AlignState::Dst1;
                CmdResult::NeedPoint
            }
            AlignState::Dst1 => {
                self.dst1 = Some(pt);
                self.state = AlignState::Src2;
                CmdResult::NeedPoint
            }
            AlignState::Src2 => {
                self.src2 = Some(pt);
                self.state = AlignState::Dst2;
                CmdResult::NeedPoint
            }
            AlignState::Dst2 => {
                self.dst2 = Some(pt);
                self.state = AlignState::AskScale;
                CmdResult::NeedPoint
            }
            AlignState::AskScale => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.state {
            AlignState::Gathering => {
                if self.handles.is_empty() {
                    return CmdResult::Cancel;
                }

                self.state = AlignState::Src1;
                CmdResult::NeedPoint
            }

            AlignState::Src2 => {
                // One alignment pair only: translation.
                match (self.src1, self.dst1) {
                    (Some(s), Some(d)) => {
                        let delta = d - s;

                        CmdResult::TransformSelected(
                            self.handles.clone(),
                            EntityTransform::Translate(delta),
                        )
                    }
                    _ => CmdResult::Cancel,
                }
            }

            // Default option shown as <No>.
            AlignState::AskScale => self.compute_align(false),

            _ => CmdResult::Cancel,
        }
    }

    fn wants_text_input(&self) -> bool {
        self.state == AlignState::AskScale
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.state != AlignState::AskScale {
            return None;
        }

        match text.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" | "scale" => {
                Some(self.compute_align(true))
            }

            "n" | "no" | "don't scale" | "dont scale" | "noscale" => {
                Some(self.compute_align(false))
            }

            _ => Some(CmdResult::NeedPoint),
        }
    }
    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
    fn line(a: DVec3, b: DVec3, name: &str) -> WireModel {
        WireModel::solid(
            name.into(),
            vec![
                [a.x as f32, a.y as f32, a.z as f32],
                [b.x as f32, b.y as f32, b.z as f32],
            ],
            WireModel::CYAN,
            false,
        )
    }

    let mut out = Vec::new();

    // Keep the first completed alignment pair visible while defining
    // the second pair and while choosing the scale option.
    if let (Some(src1), Some(dst1)) = (self.src1, self.dst1) {
        match self.state {
            AlignState::Src2
            | AlignState::Dst2
            | AlignState::AskScale => {
                out.push(line(src1, dst1, "align_pair_1"));
            }
            _ => {}
        }
    }

        match self.state {
            // First source has been picked: stretch its reference line
            // to the cursor until the first destination is chosen.
            AlignState::Dst1 => {
                if let Some(src1) = self.src1 {
                    out.push(line(src1, pt, "align_pair_1_preview"));
                }
            }

            // Second source has been picked: stretch the second reference
            // line to the cursor until its destination is chosen.
            AlignState::Dst2 => {
                if let Some(src2) = self.src2 {
                    out.push(line(src2, pt, "align_pair_2_preview"));
                }
            }

            // Once both pairs are complete, keep both visible while
            // waiting for the Scale / No Scale decision.
            AlignState::AskScale => {
                if let (Some(src2), Some(dst2)) = (self.src2, self.dst2) {
                    out.push(line(src2, dst2, "align_pair_2"));
                }
            }

            _ => {}
        }

        out
    }
}

impl AlignCommand {
    fn compute_align(&self, with_scale: bool) -> CmdResult {
        let (s1, d1, s2, d2) = match (self.src1, self.dst1, self.src2, self.dst2) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => return CmdResult::Cancel,
        };

        // Build transform: move s1→d1, rotate so s2-s1 aligns with d2-d1 (in the world XY plane)
        let src_vec = s2 - s1;
        let dst_vec = d2 - d1;

        let src_len = src_vec.length();
        let dst_len = dst_vec.length();

        if src_len < 1e-6 || dst_len < 1e-6 {
            // Degenerate: just translate
            let delta = d1 - s1;
            return CmdResult::TransformSelected(
                self.handles.clone(),
                EntityTransform::Translate(delta),
            );
        }

        // Angle from src_vec to dst_vec in the world XY plane
        let src_angle = src_vec.y.atan2(src_vec.x);
        let dst_angle = dst_vec.y.atan2(dst_vec.x);
        let angle = dst_angle - src_angle;

        let scale_factor = if with_scale { dst_len / src_len } else { 1.0 };

        // Apply: translate to origin (s1), scale, rotate, translate to d1
        // We use the EntityTransform enum — it doesn't support composed transforms directly.
        // Return a special align result that carries the full matrix.
        let _ = (angle, scale_factor, with_scale);

        // Compose via AlignTransform CmdResult
        CmdResult::AlignSelected {
            handles: self.handles.clone(),
            src1: s1,
            dst1: d1,
            angle_rad: angle,
            scale: scale_factor,
        }
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ALIGN"] });  // AlignCommand
