// TORIENT tool — interactive command.
//
// Command:  TORIENT
//   Requires at least one entity selected before starting.
//   Step 1: Wait for an angle (numerical input), Enter (Most Readable), or pick first point.
//   Step 2: If first point picked, pick second point to define angle vector.

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, DynField};
use crate::scene::model::wire_model::WireModel;
use acadrust::EntityType;

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    AngleOrFirstPoint,
    SecondPoint { first_point: DVec3 },
}

pub struct TorientCommand {
    entities: Vec<(Handle, EntityType)>,
    view_twist: f64,
    step: Step,
}

impl TorientCommand {
    pub fn new(entities: Vec<(Handle, EntityType)>, view_twist: f64) -> Self {
        Self {
            entities,
            view_twist,
            step: Step::AngleOrFirstPoint,
        }
    }

    fn commit_angle(&self, new_angle_rad: Option<f64>) -> CmdResult {
        let mut replacements = Vec::new();

        for (handle, entity) in &self.entities {
            let mut new_entity = entity.clone();
            let mut changed = false;

            match &mut new_entity {
                EntityType::Text(text) => {
                    let angle = new_angle_rad.unwrap_or_else(|| most_readable_angle(text.rotation, self.view_twist));
                    text.rotation = angle;
                    changed = true;
                }
                EntityType::MText(mtext) => {
                    let angle = new_angle_rad.unwrap_or_else(|| most_readable_angle(mtext.rotation as f64, self.view_twist));
                    mtext.rotation = angle as f64;
                    changed = true;
                }
                EntityType::AttributeDefinition(attdef) => {
                    let angle = new_angle_rad.unwrap_or_else(|| most_readable_angle(attdef.rotation, self.view_twist));
                    attdef.rotation = angle;
                    changed = true;
                }
                EntityType::Insert(insert) => {
                    let mut block_changed = false;
                    for attr in &mut insert.attributes {
                        let angle = new_angle_rad.unwrap_or_else(|| most_readable_angle(attr.rotation, self.view_twist));
                        attr.rotation = angle;
                        block_changed = true;
                    }
                    if block_changed {
                        changed = true;
                    }
                }
                _ => {}
            }

            if changed {
                replacements.push((*handle, vec![new_entity]));
            }
        }

        if replacements.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::ReplaceMany(replacements, vec![])
        }
    }
}

fn most_readable_angle(wcs_angle_rad: f64, view_twist: f64) -> f64 {
    let two_pi = 2.0 * std::f64::consts::PI;
    
    // Calculate the angle as it appears on screen
    let mut screen_angle = (wcs_angle_rad - view_twist) % two_pi;
    if screen_angle < 0.0 {
        screen_angle += two_pi;
    }

    // "Most Readable" logic: if upside down on screen (angle > 90 and <= 270 deg)
    let half_pi = std::f64::consts::PI / 2.0;
    let three_half_pi = 3.0 * std::f64::consts::PI / 2.0;

    let mut final_wcs_angle = wcs_angle_rad;
    if screen_angle > half_pi + 1e-6 && screen_angle <= three_half_pi + 1e-6 {
        final_wcs_angle += std::f64::consts::PI;
    }
    
    // Normalize final WCS angle
    final_wcs_angle %= two_pi;
    if final_wcs_angle < 0.0 {
        final_wcs_angle += two_pi;
    }
    final_wcs_angle
}

impl CadCommand for TorientCommand {
    fn name(&self) -> &'static str {
        "TORIENT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::AngleOrFirstPoint => t!("TORIENT  New absolute rotation <Most Readable>:").into_owned(),
            Step::SecondPoint { .. } => t!("TORIENT  Specify second point:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::AngleOrFirstPoint => {
                self.step = Step::SecondPoint { first_point: pt };
                CmdResult::NeedPoint
            }
            Step::SecondPoint { first_point } => {
                let dx = pt.x - first_point.x;
                let dy = pt.y - first_point.y;
                let angle = dy.atan2(dx);
                self.commit_angle(Some(angle))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::AngleOrFirstPoint => self.commit_angle(None),
            Step::SecondPoint { .. } => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Step::AngleOrFirstPoint = self.step {
            let angle = crate::entities::common::parse_typed_angle(text)?;
            Some(self.commit_angle(Some(angle)))
        } else {
            None
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let pt = pt.as_vec3();
        if let Step::SecondPoint { first_point } = self.step {
            let first_point = first_point.as_vec3();
            vec![WireModel::solid(
                "rubber_band".into(),
                vec![[first_point.x, first_point.y, first_point.z], [pt.x, pt.y, pt.z]],
                WireModel::CYAN,
                false,
            )]
        } else {
            vec![]
        }
    }

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::AngleOrFirstPoint => DynField::Angle,
            Step::SecondPoint { .. } => DynField::Point,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        None
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.step, Step::AngleOrFirstPoint)
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TORIENT"] });
