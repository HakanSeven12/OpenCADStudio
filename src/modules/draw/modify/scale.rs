// Scale tool — ribbon definition + interactive command.
//
// Command:  SCALE (SC)
//   Requires at least one entity selected.
//   Step 1: pick base (scale center)
//   Step 2: specify scale factor — drag for a live preview (factor = cursor
//           distance from base) or type a factor. A live ghost of the
//           selection tracks the cursor from the first move onward.
//
//   Reference scaling: type `R` at step 2 to define the factor as
//   new-length / reference-length. The reference length may be typed or
//   measured between two points; the new length may be typed or measured
//   from the scale base.

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, DynField, EntityTransform};
use crate::modules::draw::defaults;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SCALE",
        label: "Scale",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/scale.svg")),
        event: ModuleEvent::Command("SCALE".to_string()),
    }
}

enum Step {
    Base,
    /// Default flow: factor is the cursor distance from `base`.
    Factor { base: DVec3 },
    /// Reference flow: waiting for the first of two independent points, or a
    /// typed reference length.
    RefFirst { base: DVec3 },
    /// Reference flow: measuring the reference length from `first`.
    RefSecond { base: DVec3, first: DVec3 },
    /// Reference flow: factor is `cursor_dist / ref_dist` from `base`.
    RefNew { base: DVec3, ref_dist: f64 },
}

pub struct ScaleCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: Step,
    default_factor: f64,
}

impl ScaleCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: Step::Base,
            default_factor: defaults::get_scale_factor(),
        }
    }

    /// Commit a uniform scale about `base` and end the command.
    fn commit(&self, base: DVec3, factor: f64) -> CmdResult {
        defaults::set_scale_factor(factor);
        CmdResult::TransformSelected(
            self.handles.clone(),
            EntityTransform::Scale {
                center: base,
                factor,
            },
        )
    }
}

impl CadCommand for ScaleCommand {
    fn name(&self) -> &'static str {
        "SCALE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Base => t!(
                "SCALE  Specify base point  [%{count} objects]:",
                count = self.handles.len()
            )
            .into_owned(),
            Step::Factor { .. } => {
                let f = format!("{:.4}", self.default_factor);
                t!("SCALE  Specify scale factor  <%{f}>:", f = f).into_owned()
            }
            Step::RefFirst { .. } => {
                t!("SCALE  Specify first reference point or type reference length:").into_owned()
            }
            Step::RefSecond { .. } => {
                t!("SCALE  Specify second reference point:").into_owned()
            }
            Step::RefNew { ref_dist, .. } => {
                let d = format!("{:.3}", ref_dist);
                t!(
                    "SCALE  Specify new length from base or type a length  [ref=%{d}]:",
                    d = d
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match &self.step {
            // The reference-scaling keyword is only offered at the factor step.
            Step::Factor { .. } => vec![CmdOption::new(t!("Reference").as_ref(), "R")],
            _ => vec![],
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Base => {
                self.step = Step::Factor { base: pt };
                CmdResult::NeedPoint
            }
            Step::Factor { base } => {
                let base = *base;
                let factor = base.distance(pt);
                if factor <= f64::EPSILON {
                    return CmdResult::NeedPoint;
                }
                self.commit(base, factor)
            }
            Step::RefFirst { base } => {
                let base = *base;
                self.step = Step::RefSecond { base, first: pt };
                CmdResult::NeedPoint
            }
            Step::RefSecond { base, first } => {
                let base = *base;
                let ref_dist = first.distance(pt);
                if ref_dist <= f64::EPSILON {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::RefNew { base, ref_dist };
                CmdResult::NeedPoint
            }
            Step::RefNew { base, ref_dist } => {
                let base = *base;
                let new_dist = base.distance(pt);
                if new_dist <= f64::EPSILON {
                    return CmdResult::NeedPoint;
                }
                self.commit(base, new_dist / *ref_dist)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter at the factor step accepts the stored default factor.
        if let Step::Factor { base } = &self.step {
            let base = *base;
            return self.commit(base, self.default_factor);
        }
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            Step::Factor { base } => {
                let base = *base;
                // `R` / `Reference` switches to reference scaling.
                let low = t.to_ascii_lowercase();
                if low == "r" || low == "reference" {
                    self.step = Step::RefFirst { base };
                    return Some(CmdResult::NeedPoint);
                }
                let factor: f64 = t.replace(',', ".").parse().ok()?;
                (factor.is_finite() && factor > 0.0).then(|| self.commit(base, factor))
            }
            Step::RefFirst { base } => {
                let base = *base;
                let ref_dist: f64 = t.replace(',', ".").parse().ok()?;
                if ref_dist.is_finite() && ref_dist > 0.0 {
                    self.step = Step::RefNew { base, ref_dist };
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
            Step::RefSecond { .. } => None,
            Step::RefNew { base, ref_dist } => {
                let (base, ref_dist) = (*base, *ref_dist);
                let new_len: f64 = t.replace(',', ".").parse().ok()?;
                let factor = new_len / ref_dist;
                (factor.is_finite() && factor > 0.0).then(|| self.commit(base, factor))
            }
            Step::Base => None,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let (base, factor): (DVec3, f32) = match &self.step {
            // Default flow: scale live by cursor distance from the base.
            Step::Factor { base } => (*base, base.distance(pt).max(1e-6) as f32),
            // Reference flow, new-length step: factor = cursor_dist / ref_dist.
            Step::RefNew { base, ref_dist } => {
                (*base, (base.distance(pt) / ref_dist) as f32)
            }
            // The reference length is measured between two points independent
            // of the scale base.
            Step::RefSecond { first, .. } => {
                return vec![WireModel::solid(
                    "rubber_band".into(),
                    vec![
                        [first.x as f32, first.y as f32, first.z as f32],
                        [pt.x as f32, pt.y as f32, pt.z as f32],
                    ],
                    WireModel::CYAN,
                    false,
                )];
            }
            Step::Base | Step::RefFirst { .. } => return vec![],
        };
        let mut out: Vec<WireModel> = self
            .wire_models
            .iter()
            .map(|w| w.scaled(base.as_vec3(), factor))
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

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::Factor { .. } => DynField::Scalar,
            Step::RefFirst { .. } | Step::RefNew { .. } => DynField::Distance,
            _ => DynField::Point,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match self.step {
            Step::Factor { base } => Some(DynSpec {
                anchor: DynAnchor::Point(base),
                fields: vec![DynFieldSpec::new(DynRole::Factor)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
            Step::RefFirst { base } => Some(DynSpec {
                anchor: DynAnchor::Point(base),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::None,
                ref_point: None,
            }),
            Step::RefNew { base, .. } => Some(DynSpec {
                anchor: DynAnchor::Point(base),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
            _ => None,
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(
            self.step,
            Step::Factor { .. } | Step::RefFirst { .. } | Step::RefNew { .. }
        )
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        match self.step {
            Step::Factor { base } | Step::RefNew { base, .. } => Some(base.distance(cursor)),
            _ => None,
        }
    }
}
