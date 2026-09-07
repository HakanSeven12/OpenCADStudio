use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, SelectionEntity};

/// Which boolean operation to apply to selected solids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub fn from_id(id: &str) -> Option<BoolOp> {
        Some(match id {
            "UNION" => BoolOp::Union,
            "SUBTRACT" => BoolOp::Subtract,
            "INTERSECT" => BoolOp::Intersect,
            _ => return None,
        })
    }
}

enum SubtractStep {
    Bases,
    Cutters,
    ConvertMeshes,
}

pub struct SubtractCommand {
    step: SubtractStep,
    bases: Vec<Handle>,
    cutters: Vec<Handle>,
    selected: Vec<Handle>,
    bases_have_mesh: bool,
    selected_has_mesh: bool,
}

impl SubtractCommand {
    pub fn new(bases: Vec<Handle>, bases_have_mesh: bool) -> Self {
        let step = if bases.is_empty() {
            SubtractStep::Bases
        } else {
            SubtractStep::Cutters
        };
        Self {
            step,
            bases,
            cutters: Vec::new(),
            selected: Vec::new(),
            bases_have_mesh,
            selected_has_mesh: false,
        }
    }

    fn finish(&mut self, convert_meshes: bool) -> CmdResult {
        CmdResult::SolidSubtract {
            bases: std::mem::take(&mut self.bases),
            cutters: std::mem::take(&mut self.cutters),
            convert_meshes,
        }
    }
}

impl CadCommand for SubtractCommand {
    fn name(&self) -> &'static str {
        "SUBTRACT"
    }

    fn prompt(&self) -> String {
        match self.step {
            SubtractStep::Bases => crate::t!("SUBTRACT  Select base Solids, Regions, Surfaces, or Meshes, then press Enter:")
                .into_owned(),
            SubtractStep::Cutters => {
                crate::t!("SUBTRACT  Select Solids, Regions, Surfaces, or Meshes to subtract, then press Enter:").into_owned()
            }
            SubtractStep::ConvertMeshes => crate::t!(
                "SUBTRACT  Convert selected closed Mesh objects to solids? [Yes/No] <Yes>:"
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        if matches!(self.step, SubtractStep::ConvertMeshes) {
            vec![
                crate::command::CmdOption::new(crate::t!("Yes").as_ref(), "Y"),
                crate::command::CmdOption::new(crate::t!("No").as_ref(), "N"),
            ]
        } else {
            Vec::new()
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.step, SubtractStep::ConvertMeshes) {
            return self.finish(true);
        }
        if self.selected.is_empty() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            SubtractStep::Bases => {
                self.bases = std::mem::take(&mut self.selected);
                self.bases_have_mesh = self.selected_has_mesh;
                self.selected_has_mesh = false;
                self.step = SubtractStep::Cutters;
                CmdResult::DeselectAndContinue
            }
            SubtractStep::Cutters => {
                self.cutters = std::mem::take(&mut self.selected);
                if self.bases_have_mesh || self.selected_has_mesh {
                    self.step = SubtractStep::ConvertMeshes;
                    CmdResult::NeedPoint
                } else {
                    self.finish(false)
                }
            }
            SubtractStep::ConvertMeshes => unreachable!(),
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, SubtractStep::ConvertMeshes)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !matches!(self.step, SubtractStep::ConvertMeshes) {
            return None;
        }
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => self.finish(true),
            "n" | "no" => self.finish(false),
            _ => CmdResult::NeedPoint,
        })
    }

    fn is_selection_gathering(&self) -> bool {
        !matches!(self.step, SubtractStep::ConvertMeshes)
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.selected_has_mesh = entities.iter().any(|entity| {
            matches!(
                entity.entity,
                EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_)
            )
        });
        self.selected = entities
            .into_iter()
            .filter_map(|entity| {
                matches!(
                    entity.entity,
                    EntityType::Solid3D(_)
                        | EntityType::Region(_)
                        | EntityType::Surface(_)
                        | EntityType::Mesh(_)
                        | EntityType::PolygonMesh(_)
                        | EntityType::PolyfaceMesh(_)
                )
                .then_some(entity.handle)
            })
            .collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["UNION", "SUBTRACT", "INTERSECT"]
});
