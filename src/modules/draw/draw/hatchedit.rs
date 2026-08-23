// HATCHEDIT — edit an existing hatch entity's pattern, scale, or angle.
//
// Workflow:
//   1. Pick or pre-select a Hatch entity.
//   2. Enter options:
//        P <name>     — change pattern (ANSI31, SOLID, etc.)
//        S <value>    — change scale
//        A <degrees>  — change angle
//      Press Enter to apply changes.

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, HatchEditOperation};

enum HatcheditStep {
    PickHatch,
    EditOptions {
        handle: Handle,
        name: String,
        scale: f32,
        angle: f32,
    },
}

pub struct HatcheditCommand {
    step: HatcheditStep,
    origin: Option<(f64, f64)>,
    disassociate: bool,
    style: Option<acadrust::entities::HatchStyleType>,
    annotative: Option<bool>,
    annotative_current: bool,
}

impl HatcheditCommand {
    pub fn new() -> Self {
        Self {
            step: HatcheditStep::PickHatch,
            origin: None,
            disassociate: false,
            style: None,
            annotative: None,
            annotative_current: false,
        }
    }

    pub fn with_handle(
        handle: Handle,
        name: String,
        scale: f32,
        angle: f32,
        annotative: bool,
    ) -> Self {
        Self {
            step: HatcheditStep::EditOptions {
                handle,
                name,
                scale,
                angle,
            },
            origin: None,
            disassociate: false,
            style: None,
            annotative: None,
            annotative_current: annotative,
        }
    }

    fn apply_result(&self, operation: HatchEditOperation) -> Option<CmdResult> {
        let HatcheditStep::EditOptions {
            handle,
            name,
            scale,
            angle,
        } = &self.step
        else {
            return None;
        };
        Some(CmdResult::HatcheditApply {
            handle: *handle,
            name: name.clone(),
            scale: *scale,
            angle: *angle,
            operation,
        })
    }

    fn update_operation(&self) -> HatchEditOperation {
        HatchEditOperation::Update {
            origin: self.origin,
            disassociate: self.disassociate,
            style: self.style,
            annotative: self.annotative,
        }
    }
}

impl CadCommand for HatcheditCommand {
    fn name(&self) -> &'static str {
        "HATCHEDIT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            HatcheditStep::PickHatch => t!("HATCHEDIT  Select hatch:").into_owned(),
            HatcheditStep::EditOptions {
                name, scale, angle, ..
            } => {
                let scale = format!("{scale:.4}");
                let angle = format!("{angle:.1}");
                t!(
                    "HATCHEDIT  Pattern:%{name}  Scale:%{scale}  Angle:%{angle}  [P pattern / S scale / A angle / O x,y / D disassociate / Y style / N annotative / R recreate / E separate / + handles / - handles | Enter]:",
                    name = name,
                    scale = scale,
                    angle = angle
                )
                .into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, HatcheditStep::PickHatch)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Actual hatch model retrieval happens in commands.rs dispatch.
        // Store handle; name/scale/angle filled in by dispatch.
        self.step = HatcheditStep::EditOptions {
            handle,
            name: String::new(),
            scale: 1.0,
            angle: 0.0,
        };
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, HatcheditStep::EditOptions { .. })
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        if !matches!(self.step, HatcheditStep::EditOptions { .. }) {
            return Vec::new();
        }
        vec![
            crate::command::CmdOption::new("Disassociate", "D"),
            crate::command::CmdOption::new("Annotative", "N"),
            crate::command::CmdOption::new("Recreate boundary", "R"),
            crate::command::CmdOption::new("Separate hatches", "E"),
            crate::command::CmdOption::new("Draw front", "F"),
            crate::command::CmdOption::new("Draw back", "B"),
            crate::command::CmdOption::enter("Apply"),
        ]
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let (_handle, name, scale, angle) = match &mut self.step {
            HatcheditStep::EditOptions {
                handle,
                name,
                scale,
                angle,
            } => (*handle, name, scale, angle),
            _ => return None,
        };

        let text = text.trim().to_uppercase();

        if text.is_empty() {
            return self.apply_result(self.update_operation());
        }

        if text == "ANNOTATIVE" {
            self.annotative = Some(!self.annotative.unwrap_or(self.annotative_current));
            return Some(CmdResult::NeedPoint);
        }
        if text == "SEPARATE" {
            return self.apply_result(HatchEditOperation::Separate);
        }

        // Parse option: P/S/A followed by value
        if let Some(rest) = text.strip_prefix('P') {
            let n = rest.trim().to_string();
            if !n.is_empty() {
                *name = n;
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('S') {
            if let Ok(v) = rest.trim().replace(',', ".").parse::<f32>() {
                if v > 0.0 {
                    *scale = v;
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('A') {
            if let Ok(v) = rest.trim().replace(',', ".").parse::<f32>() {
                *angle = v;
            }
            return Some(CmdResult::NeedPoint);
        }

        if let Some(rest) = text.strip_prefix('O') {
            let values: Vec<_> = rest
                .trim()
                .split([',', ';', ' '])
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.replace(',', ".").parse::<f64>().ok())
                .collect();
            if values.len() >= 2 {
                self.origin = Some((values[0], values[1]));
            }
            return Some(CmdResult::NeedPoint);
        }
        if text == "D" || text == "DISASSOCIATE" {
            self.disassociate = true;
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('Y') {
            self.style = match rest.trim() {
                "NORMAL" | "N" => Some(acadrust::entities::HatchStyleType::Normal),
                "OUTER" | "O" => Some(acadrust::entities::HatchStyleType::Outer),
                "IGNORE" | "I" => Some(acadrust::entities::HatchStyleType::Ignore),
                _ => self.style,
            };
            return Some(CmdResult::NeedPoint);
        }
        if text == "N" || text == "ANNOTATIVE" {
            self.annotative = Some(!self.annotative.unwrap_or(self.annotative_current));
            return Some(CmdResult::NeedPoint);
        }
        if text == "R" || text == "RECREATE" {
            return self.apply_result(HatchEditOperation::RecreateBoundary);
        }
        if text == "E" || text == "SEPARATE" {
            return self.apply_result(HatchEditOperation::Separate);
        }
        if text == "F" || text == "FRONT" {
            return self.apply_result(HatchEditOperation::DrawOrderFront);
        }
        if text == "B" || text == "BACK" {
            return self.apply_result(HatchEditOperation::DrawOrderBack);
        }
        let parse_handles = |source: &str| {
            source
                .split([',', ';', ' '])
                .filter(|part| !part.is_empty())
                .filter_map(|part| {
                    u64::from_str_radix(part.trim_start_matches("0X"), 16)
                        .ok()
                        .map(Handle::new)
                })
                .collect::<Vec<_>>()
        };
        if let Some(rest) = text.strip_prefix('+') {
            return self.apply_result(HatchEditOperation::AddBoundaries(parse_handles(rest)));
        }
        if let Some(rest) = text.strip_prefix('-') {
            return self.apply_result(HatchEditOperation::RemoveBoundaries(parse_handles(rest)));
        }

        // Unrecognized — stay and re-prompt
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        // Enter without text → apply current settings
        self.apply_result(self.update_operation()).unwrap_or(CmdResult::Cancel)
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["HATCHEDIT"] });  // HatcheditCommand
