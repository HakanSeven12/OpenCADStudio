// ATTDEF command — define a block attribute (AttributeDefinition entity).
//
// Workflow (command-line only):
//   1. Text: Enter attribute tag    (required, no spaces)
//   2. Text: Enter attribute prompt (optional — press Enter to use tag)
//   3. Text: Enter default value    (optional — press Enter for blank)
//   4. Point: Click insertion point

use acadrust::entities::AttributeDefinition;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

enum Step {
    Tag,
    Prompt {
        tag: String,
    },
    Default {
        tag: String,
        prompt: String,
    },
    Insertion {
        tag: String,
        prompt: String,
        default: String,
    },
}

pub struct AttdefCommand {
    step: Step,
    height: f64,
    text_style: String,
    width_factor: f64,
    oblique_angle: f64,
    plane: WorkingPlane,
}

impl AttdefCommand {
    pub fn with_text_defaults(
        height: f64,
        text_style: String,
        width_factor: f64,
        oblique_angle: f64,
    ) -> Self {
        Self {
            step: Step::Tag,
            height,
            text_style,
            width_factor,
            oblique_angle,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for AttdefCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ATTDEF"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Tag => t!("ATTDEF  Enter attribute tag (no spaces):").into_owned(),
            Step::Prompt { tag } => t!(
                "ATTDEF  Enter prompt for '%{tag}' (Enter=use tag):",
                tag = tag
            )
            .into_owned(),
            Step::Default { tag, .. } => t!(
                "ATTDEF  Enter default value for '%{tag}' (Enter=blank):",
                tag = tag
            )
            .into_owned(),
            Step::Insertion { tag, .. } => t!(
                "ATTDEF  Specify insertion point for '%{tag}':",
                tag = tag
            )
            .into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Insertion { .. })
    }

    fn wants_text_with_spaces(&self) -> bool {
        // Tag must be a single token (so disallow spaces there); the
        // Prompt and Default value steps are free-form text.
        matches!(self.step, Step::Prompt { .. } | Step::Default { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match &self.step {
            Step::Tag => {
                let tag = text.trim().replace(' ', "_");
                if tag.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = Step::Prompt { tag };
                Some(CmdResult::NeedPoint)
            }
            Step::Prompt { tag } => {
                let tag = tag.clone();
                let prompt = if text.trim().is_empty() {
                    tag.clone()
                } else {
                    text.trim().to_string()
                };
                self.step = Step::Default { tag, prompt };
                Some(CmdResult::NeedPoint)
            }
            Step::Default { tag, prompt } => {
                let tag = tag.clone();
                let prompt = prompt.clone();
                let default = text.trim().to_string();
                self.step = Step::Insertion {
                    tag,
                    prompt,
                    default,
                };
                Some(CmdResult::NeedPoint)
            }
            Step::Insertion { .. } => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Step::Insertion {
            tag,
            prompt,
            default,
        } = &self.step
        {
            let point = self.plane.to_local(pt);
            let mut attdef = AttributeDefinition {
                tag: tag.clone(),
                prompt: prompt.clone(),
                default_value: default.clone(),
                insertion_point: Vector3::new(point.x, point.y, point.z),
                height: self.height,
                text_style: self.text_style.clone(),
                width_factor: self.width_factor,
                oblique_angle: self.oblique_angle,
                ..Default::default()
            };
            attdef.common.layer = "0".into();
            CmdResult::CommitAndExit(
                self.plane
                    .place_entity(EntityType::AttributeDefinition(attdef)),
            )
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            Step::Tag => CmdResult::Cancel,
            // Treat Enter as empty text input for prompt/default steps.
            Step::Prompt { tag } => {
                let tag = tag.clone();
                self.step = Step::Default {
                    tag: tag.clone(),
                    prompt: tag,
                };
                CmdResult::NeedPoint
            }
            Step::Default { tag, prompt } => {
                let (tag, prompt) = (tag.clone(), prompt.clone());
                self.step = Step::Insertion {
                    tag,
                    prompt,
                    default: String::new(),
                };
                CmdResult::NeedPoint
            }
            Step::Insertion { .. } => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.step, Step::Insertion { .. }) {
            return None;
        }
        // Show a small cross at the insertion point.
        let d = 0.15;
        let points = [
            pt - self.plane.x * d,
            pt + self.plane.x * d,
            DVec3::splat(f64::NAN),
            pt - self.plane.y * d,
            pt + self.plane.y * d,
        ];
        Some(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "attdef_preview".into(),
            points: points
                .iter()
                .map(|point| point.as_vec3().to_array())
                .collect(),
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        })
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ATTDEF"] });  // AttdefCommand
