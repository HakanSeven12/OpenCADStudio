use acadrust::Handle;
use glam::DVec3;

use crate::command::{
    CadCommand, CmdResult, DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec,
};

#[derive(Clone, Copy)]
pub enum EdgeOperation {
    Fillet,
    Chamfer,
}

pub struct SolidEdgeCommand {
    operation: EdgeOperation,
    target: Option<Handle>,
    picked: Option<(Handle, DVec3)>,
    default_value: f64,
}

impl SolidEdgeCommand {
    pub fn new(operation: EdgeOperation, target: Option<Handle>) -> Self {
        Self {
            operation,
            target,
            picked: None,
            default_value: 1.0,
        }
    }

    fn finish(&self, value: f64) -> CmdResult {
        let Some((handle, pick)) = self.picked else {
            return CmdResult::Cancel;
        };
        CmdResult::SolidEdgeBlend {
            handle,
            pick,
            value,
            fillet: matches!(self.operation, EdgeOperation::Fillet),
        }
    }
}

impl CadCommand for SolidEdgeCommand {
    fn name(&self) -> &'static str {
        match self.operation {
            EdgeOperation::Fillet => "SOLIDFILLET",
            EdgeOperation::Chamfer => "SOLIDCHAMFER",
        }
    }

    fn prompt(&self) -> String {
        if self.picked.is_none() {
            return crate::t!("Select a solid edge:").into_owned();
        }
        match self.operation {
            EdgeOperation::Fillet => {
                crate::tf!("Specify fillet radius <{:.3}>:", self.default_value).into_owned()
            }
            EdgeOperation::Chamfer => {
                crate::tf!("Specify chamfer distance <{:.3}>:", self.default_value).into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.picked.is_none()
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() || self.target.is_some_and(|target| target != handle) {
            return CmdResult::NeedPoint;
        }
        self.picked = Some((handle, point));
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some((_, pick)) = self.picked else {
            return CmdResult::NeedPoint;
        };
        let value = point.distance(pick);
        if value > 0.0 && value.is_finite() {
            self.finish(value)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.picked.is_none() {
            return None;
        }
        let value = text.trim().parse::<f64>().ok()?;
        (value > 0.0 && value.is_finite()).then(|| self.finish(value))
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.picked.is_some() {
            self.finish(self.default_value)
        } else {
            CmdResult::Cancel
        }
    }

    fn wants_text_input(&self) -> bool {
        self.picked.is_some()
    }

    fn dyn_commit_as_text(&self) -> bool {
        self.picked.is_some()
    }

    fn dyn_spec(&self) -> Option<DynSpec> {
        let (_, pick) = self.picked?;
        Some(DynSpec {
            anchor: DynAnchor::Point(pick),
            fields: vec![DynFieldSpec::new(match self.operation {
                EdgeOperation::Fillet => DynRole::Radius,
                EdgeOperation::Chamfer => DynRole::Distance,
            })],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        self.picked.map(|(_, pick)| cursor.distance(pick))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["SOLIDFILLET", "SOLIDCHAMFER"]
});
