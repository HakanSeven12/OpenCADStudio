// DIM's Distribute option — space parallel linear/aligned dimensions evenly
// between the outermost two (Equal), or step them away from a base dimension
// by an offset distance (Offset). The method chosen last is the next default.

use std::sync::atomic::{AtomicBool, Ordering};

use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, SelectionEntity};

/// Offset was the method chosen last (session-wide, like the other DIM
/// defaults).
static OFFSET_METHOD: AtomicBool = AtomicBool::new(false);

enum Step {
    Method,
    Base,
    Gather,
}

pub struct DimDistributeCommand {
    step: Step,
    offset_method: bool,
    /// The Offset distance is being asked.
    offset_pending: bool,
    offset: f64,
    base: Option<Handle>,
    picked: Option<EntityType>,
    dimensions: Vec<Handle>,
}

fn spaced(entity: &EntityType) -> bool {
    matches!(
        crate::entities::dimension::as_dimension(entity),
        Some(codec::entities::Dimension::Linear(_) | codec::entities::Dimension::Aligned(_))
    )
}

impl DimDistributeCommand {
    pub fn new(offset: f64) -> Self {
        Self {
            step: Step::Method,
            offset_method: OFFSET_METHOD.load(Ordering::Relaxed),
            offset_pending: false,
            offset,
            base: None,
            picked: None,
            dimensions: Vec::new(),
        }
    }

    fn choose(&mut self, offset: bool) {
        self.offset_method = offset;
        OFFSET_METHOD.store(offset, Ordering::Relaxed);
        self.step = if offset { Step::Base } else { Step::Gather };
    }

    fn finish(&mut self) -> CmdResult {
        CmdResult::DistributeDimensions {
            base: self.base,
            dimensions: std::mem::take(&mut self.dimensions),
            offset: self.offset_method.then_some(self.offset),
        }
    }
}

impl CadCommand for DimDistributeCommand {
    fn name(&self) -> &'static str {
        "DIM"
    }

    fn prompt(&self) -> String {
        if self.offset_pending {
            return crate::tr!(
                "dim-cmd",
                "offset-distance",
                offset = format!("{:.6}", self.offset)
            );
        }
        match self.step {
            Step::Method if self.offset_method => crate::tr!("dim-cmd", "distribute-method-offset"),
            Step::Method => crate::tr!("dim-cmd", "distribute-method-equal"),
            Step::Base => crate::tr!("dim-cmd", "distribute-base"),
            Step::Gather if self.offset_method => crate::tr!("dim-cmd", "distribute-others-offset"),
            Step::Gather => crate::tr!("dim-cmd", "distribute-others"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.offset_pending {
            return Vec::new();
        }
        match self.step {
            Step::Method => vec![
                CmdOption::new("Equal", "E"),
                CmdOption::new("Offset", "O"),
            ],
            Step::Base => vec![CmdOption::new("Offset", "O")],
            Step::Gather if self.offset_method => vec![CmdOption::new("Offset", "O")],
            Step::Gather => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        if self.offset_pending || matches!(self.step, Step::Method) {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Base) && !self.offset_pending
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::Base)
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !matches!(self.step, Step::Base) {
            return CmdResult::NeedPoint;
        }
        match self.picked.take() {
            Some(entity) if !handle.is_null() && spaced(&entity) => {
                self.base = Some(handle);
                self.step = Step::Gather;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError(crate::tr!("dim-cmd", "no-object-selected")),
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Gather) && !self.offset_pending
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(spaced)
    }

    fn selection_entities_exclude_locked(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.dimensions = entities
            .into_iter()
            .filter(|selected| Some(selected.handle) != self.base && spaced(&selected.entity))
            .map(|selected| selected.handle)
            .collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if self.offset_pending {
            let Ok(value) = word.parse::<f64>() else {
                return Some(CmdResult::ReportError(
                    crate::t!("Requires numeric distance or two points.").into_owned(),
                ));
            };
            if !value.is_finite() || value < 0.0 {
                return Some(CmdResult::ReportError(
                    crate::t!("Value must be positive.").into_owned(),
                ));
            }
            self.offset = value;
            self.offset_pending = false;
            return Some(CmdResult::NeedPoint);
        }
        match (&self.step, word.as_str()) {
            (Step::Method, "E" | "EQUAL") => self.choose(false),
            (Step::Method, "O" | "OFFSET") => self.choose(true),
            (Step::Base, "O" | "OFFSET") => self.offset_pending = true,
            (Step::Gather, "O" | "OFFSET") if self.offset_method => self.offset_pending = true,
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.offset_pending {
            self.offset_pending = false;
            return CmdResult::NeedPoint;
        }
        match self.step {
            Step::Method => {
                let offset = self.offset_method;
                self.choose(offset);
                CmdResult::NeedPoint
            }
            // Nothing picked: the base is asked again.
            Step::Base => CmdResult::ReportError(crate::tr!("dim-cmd", "no-object-selected")),
            Step::Gather => self.finish(),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
