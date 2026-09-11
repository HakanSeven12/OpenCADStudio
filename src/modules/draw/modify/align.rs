// ALIGN command — rigid 3D placement using one, two, or three point pairs.
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
    src3: Option<DVec3>,
    dst3: Option<DVec3>,
}

#[derive(PartialEq)]
enum AlignState {
    Gathering,
    Src1,
    Dst1,
    Src2,
    Dst2,
    Src3,
    Dst3,
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
            src3: None,
            dst3: None,
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
            AlignState::Src3 => "ALIGN  Specify 3rd source point or <continue>:".into(),
            AlignState::Dst3 => "ALIGN  Specify 3rd destination point:".into(),
            AlignState::AskScale => {
                t!(
                    "ALIGN  Scale objects based on alignment points? [Yes / No] <No>:"
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
        if !pt.is_finite() { return CmdResult::NeedPoint; }
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
                let pair = [self.src1.unwrap().to_array(), pt.to_array()];
                if cadkernel::space::align_point_pairs(&pair, &pair, false).is_none() {
                    return CmdResult::NeedPoint;
                }
                self.src2 = Some(pt);
                self.state = AlignState::Dst2;
                CmdResult::NeedPoint
            }
            AlignState::Dst2 => {
                let pair = [self.dst1.unwrap().to_array(), pt.to_array()];
                if cadkernel::space::align_point_pairs(&pair, &pair, false).is_none() {
                    return CmdResult::NeedPoint;
                }
                self.dst2 = Some(pt);
                self.state = AlignState::Src3;
                CmdResult::NeedPoint
            }
            AlignState::Src3 => {
                let frame = [self.src1.unwrap().to_array(), self.src2.unwrap().to_array(), pt.to_array()];
                if cadkernel::space::align_point_pairs(&frame, &frame, false).is_none() {
                    return CmdResult::NeedPoint;
                }
                self.src3 = Some(pt);
                self.state = AlignState::Dst3;
                CmdResult::NeedPoint
            }
            AlignState::Dst3 => {
                self.dst3 = Some(pt);
                self.compute_align(false)
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
            AlignState::Src3 => {
                self.state = AlignState::AskScale;
                CmdResult::NeedPoint
            }
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
            | AlignState::Src3
            | AlignState::Dst3
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
            AlignState::Src3 | AlignState::Dst3 | AlignState::AskScale => {
                if let (Some(src2), Some(dst2)) = (self.src2, self.dst2) {
                    out.push(line(src2, dst2, "align_pair_2"));
                }
                if self.state == AlignState::Dst3 {
                    if let Some(src3) = self.src3 {
                        out.push(line(src3, pt, "align_pair_3_preview"));
                    }
                }
            }

            _ => {}
        }

        out
    }
}

impl AlignCommand {
    fn compute_align(&self, with_scale: bool) -> CmdResult {
        let (Some(s1), Some(d1), Some(s2), Some(d2)) = (self.src1, self.dst1, self.src2, self.dst2)
            else { return CmdResult::NeedPoint; };
        let mut source = vec![s1.to_array(), s2.to_array()];
        let mut target = vec![d1.to_array(), d2.to_array()];
        if let (Some(s3), Some(d3)) = (self.src3, self.dst3) {
            source.push(s3.to_array());
            target.push(d3.to_array());
        }
        let Some(matrix) = cadkernel::space::align_point_pairs(&source, &target, with_scale)
            else { return CmdResult::NeedPoint; };
        CmdResult::TransformSelected(self.handles.clone(), EntityTransform::Affine(
            acadrust::types::Transform::from_matrix(acadrust::types::Matrix4 { m: matrix }),
        ))
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["ALIGN"] });  // AlignCommand
