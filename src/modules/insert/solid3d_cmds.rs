// 2D→3D modelling commands — EXTRUDE, REVOLVE, SWEEP, LOFT.
//
// Each picks profile/path entities and emits a `CmdResult` whose handler
// builds a solid and inserts its MeshModel into scene.meshes under a
// minimal placeholder Solid3D entity (empty ACIS — saving will not restore
// the mesh). The standalone primitives (BOX/CYLINDER/CONE/SPHERE/WEDGE/TORUS)
// live in the Model tab (`modules::model::primitive_cmd`).

use acadrust::{entities::Solid3D, EntityType};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};

// BOX / SPHERE / CYLINDER (and CONE / WEDGE / TORUS) now live in the Model tab
// (`modules::model::primitive_cmd`), which builds them as B-reps cached
// for the Design-group boolean tools. EXTRUDE / REVOLVE / SWEEP / LOFT remain
// here as 2D→3D operations.

// ── EXTRUDE command ────────────────────────────────────────────────────────

pub struct ExtrudeCommand {
    step: ExtrudeStep,
    pub target_handle: acadrust::Handle,
    color: [f32; 4],
}

#[derive(PartialEq)]
enum ExtrudeStep {
    Pick,
    Height,
}

impl ExtrudeCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            step: ExtrudeStep::Pick,
            target_handle: acadrust::Handle::NULL,
            color,
        }
    }
}

impl CadCommand for ExtrudeCommand {
    fn name(&self) -> &'static str {
        "EXTRUDE"
    }
    fn prompt(&self) -> String {
        match self.step {
            ExtrudeStep::Pick => t!("EXTRUDE  Select closed profile (Circle, LwPolyline…):")
                .into_owned(),
            ExtrudeStep::Height => t!("EXTRUDE  Height:").into_owned(),
        }
    }
    fn needs_entity_pick(&self) -> bool {
        self.step == ExtrudeStep::Pick
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target_handle = handle;
        self.step = ExtrudeStep::Height;
        CmdResult::NeedPoint
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step == ExtrudeStep::Height {
            return CmdResult::ExtrudeEntity {
                handle: self.target_handle,
                height: pt.y.abs().max(1e-4),
                color: self.color,
            };
        }
        CmdResult::NeedPoint
    }
    fn wants_text_input(&self) -> bool {
        self.step == ExtrudeStep::Height
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        crate::entities::common::parse_typed_length(text)
            .filter(|&h| h.abs() > 1e-6)
            .map(|h| CmdResult::ExtrudeEntity {
                handle: self.target_handle,
                height: h.abs(),
                color: self.color,
            })
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── REVOLVE command ────────────────────────────────────────────────────────

pub struct RevolveCommand {
    step: RevolveStep,
    target_handle: acadrust::Handle,
    axis_start: DVec3,
    axis_end: DVec3,
    color: [f32; 4],
}

#[derive(PartialEq)]
enum RevolveStep {
    Pick,
    AxisStart,
    AxisEnd,
    Angle,
}

impl RevolveCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            step: RevolveStep::Pick,
            target_handle: acadrust::Handle::NULL,
            axis_start: DVec3::ZERO,
            axis_end: DVec3::new(0.0, 0.0, 1.0),
            color,
        }
    }
}

impl CadCommand for RevolveCommand {
    fn name(&self) -> &'static str {
        "REVOLVE"
    }
    fn prompt(&self) -> String {
        match self.step {
            RevolveStep::Pick => t!("REVOLVE  Select profile:").into_owned(),
            RevolveStep::AxisStart => t!("REVOLVE  Axis start point:").into_owned(),
            RevolveStep::AxisEnd => t!("REVOLVE  Axis end point:").into_owned(),
            RevolveStep::Angle => t!("REVOLVE  Angle of revolution <360>:").into_owned(),
        }
    }
    fn needs_entity_pick(&self) -> bool {
        self.step == RevolveStep::Pick
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target_handle = handle;
        self.step = RevolveStep::AxisStart;
        CmdResult::NeedPoint
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            RevolveStep::AxisStart => {
                self.axis_start = pt;
                self.step = RevolveStep::AxisEnd;
                CmdResult::NeedPoint
            }
            RevolveStep::AxisEnd => {
                self.axis_end = pt;
                self.step = RevolveStep::Angle;
                CmdResult::NeedPoint
            }
            RevolveStep::Angle => self.make_revolve(360.0),
            _ => CmdResult::NeedPoint,
        }
    }
    fn wants_text_input(&self) -> bool {
        self.step == RevolveStep::Angle
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let angle = if text.trim().is_empty() {
            360.0f32
        } else {
            text.trim()
                .parse::<f32>()
                .ok()
                .filter(|&a| a.abs() > 1e-3)?
        };
        Some(self.make_revolve(angle.abs()))
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.step == RevolveStep::Angle {
            self.make_revolve(360.0)
        } else {
            CmdResult::Cancel
        }
    }
}

impl RevolveCommand {
    fn make_revolve(&self, angle_deg: f32) -> CmdResult {
        CmdResult::RevolveEntity {
            handle: self.target_handle,
            axis_start: self.axis_start,
            axis_end: self.axis_end,
            angle_deg,
            color: self.color,
        }
    }
}

// ── SWEEP command ──────────────────────────────────────────────────────────

pub struct SweepCommand {
    step: SweepStep,
    profile_handle: acadrust::Handle,
    color: [f32; 4],
}

#[derive(PartialEq)]
enum SweepStep {
    PickProfile,
    PickPath,
}

impl SweepCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            step: SweepStep::PickProfile,
            profile_handle: acadrust::Handle::NULL,
            color,
        }
    }
}

impl CadCommand for SweepCommand {
    fn name(&self) -> &'static str {
        "SWEEP"
    }
    fn prompt(&self) -> String {
        match self.step {
            SweepStep::PickProfile => t!("SWEEP  Select profile to sweep:").into_owned(),
            SweepStep::PickPath => {
                t!("SWEEP  Select path (Line, Arc, LwPolyline):").into_owned()
            }
        }
    }
    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            SweepStep::PickProfile => {
                self.profile_handle = handle;
                self.step = SweepStep::PickPath;
                CmdResult::NeedPoint
            }
            SweepStep::PickPath => CmdResult::SweepEntity {
                profile_handle: self.profile_handle,
                path_handle: handle,
                color: self.color,
            },
        }
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── LOFT command ───────────────────────────────────────────────────────────

pub struct LoftCommand {
    profiles: Vec<acadrust::Handle>,
    color: [f32; 4],
}

impl LoftCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            profiles: Vec::new(),
            color,
        }
    }
}

impl CadCommand for LoftCommand {
    fn name(&self) -> &'static str {
        "LOFT"
    }
    fn prompt(&self) -> String {
        if self.profiles.is_empty() {
            t!("LOFT  Select first cross-section:").into_owned()
        } else {
            t!(
                "LOFT  Select next cross-section (%{count} selected, Enter to finish):",
                count = self.profiles.len()
            )
            .into_owned()
        }
    }
    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Avoid duplicate picks.
        if !self.profiles.contains(&handle) {
            self.profiles.push(handle);
        }
        CmdResult::NeedPoint
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn wants_text_input(&self) -> bool {
        self.profiles.len() >= 2
    }
    fn on_text_input(&mut self, _text: &str) -> Option<CmdResult> {
        None
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.profiles.len() < 2 {
            CmdResult::Cancel
        } else {
            CmdResult::LoftEntities {
                handles: self.profiles.clone(),
                color: self.color,
            }
        }
    }
}

// ── Placeholder Solid3D entity construction ────────────────────────────────

/// Create a minimal Solid3D entity with empty ACIS data (placeholder only).
pub fn empty_solid3d() -> EntityType {
    EntityType::Solid3D(Solid3D::new())
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["EXTRUDE"] });  // ExtrudeCommand
inventory::submit!(crate::command::CommandRegistration { names: &["LOFT"] });  // LoftCommand
inventory::submit!(crate::command::CommandRegistration { names: &["REVOLVE"] });  // RevolveCommand
inventory::submit!(crate::command::CommandRegistration { names: &["SWEEP"] });  // SweepCommand
