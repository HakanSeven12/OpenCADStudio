// Profile-based kernel solid commands stored as exact ACIS data.

use acadrust::{entities::Solid3D, EntityType};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::t;

// ── EXTRUDE command ────────────────────────────────────────────────────────

pub struct ExtrudeCommand {
    command_name: &'static str,
    step: ExtrudeStep,
    pub target_handle: acadrust::Handle,
    anchor: DVec3,
    direction: Option<DVec3>,
    color: [f32; 4],
}

#[derive(PartialEq)]
enum ExtrudeStep {
    Pick,
    Height,
}

impl ExtrudeCommand {
    pub fn new_named(name: &str, color: [f32; 4]) -> Self {
        Self {
            command_name: match name {
                "THICKEN" => "THICKEN",
                _ => "EXTRUDE",
            },
            step: ExtrudeStep::Pick,
            target_handle: acadrust::Handle::NULL,
            anchor: DVec3::ZERO,
            direction: None,
            color,
        }
    }
}

impl CadCommand for ExtrudeCommand {
    fn name(&self) -> &'static str {
        self.command_name
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
    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }
    fn set_entity_pick_direction(&mut self, direction: Option<DVec3>) {
        self.direction = direction.and_then(DVec3::try_normalize);
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target_handle = handle;
        self.anchor = point;
        self.step = ExtrudeStep::Height;
        CmdResult::NeedPoint
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step == ExtrudeStep::Height {
            let height = self
                .direction
                .map(|direction| (pt - self.anchor).dot(direction))
                .unwrap_or_else(|| pt.distance(self.anchor));
            if !height.is_finite() || height.abs() <= 1e-6 {
                return CmdResult::NeedPoint;
            }
            return CmdResult::ExtrudeEntity {
                handle: self.target_handle,
                height,
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
                height: h,
                color: self.color,
            })
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        (self.step == ExtrudeStep::Height).then_some((self.anchor, self.direction?))
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        (self.step == ExtrudeStep::Height).then_some(crate::command::DynSpec {
            anchor: crate::command::DynAnchor::Point(self.anchor),
            fields: vec![crate::command::DynFieldSpec::new(
                crate::command::DynRole::Distance,
            )],
            guide: crate::command::DynGuide::Radius,
            ref_point: None,
        })
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        Some((cursor - self.anchor).dot(self.direction?))
    }
}

// ── PRESSPULL command ─────────────────────────────────────────────────────

pub struct PresspullCommand {
    picked: Option<(acadrust::Handle, DVec3)>,
    direction: Option<DVec3>,
    color: [f32; 4],
}

impl PresspullCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            picked: None,
            direction: None,
            color,
        }
    }
}

impl CadCommand for PresspullCommand {
    fn name(&self) -> &'static str {
        "PRESSPULL"
    }

    fn prompt(&self) -> String {
        if self.picked.is_none() {
            t!("PRESSPULL  Select a closed profile or planar solid face:").into_owned()
        } else {
            t!("PRESSPULL  Signed distance:").into_owned()
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

    fn set_entity_pick_direction(&mut self, direction: Option<DVec3>) {
        self.direction = direction.and_then(|value| value.try_normalize());
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: acadrust::Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.picked = Some((handle, point));
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some((handle, pick)) = self.picked else {
            return CmdResult::NeedPoint;
        };
        let distance = self
            .direction
            .map(|direction| (point - pick).dot(direction))
            .unwrap_or_else(|| point.distance(pick));
        if !distance.is_finite() || distance.abs() <= 1e-6 {
            return CmdResult::NeedPoint;
        }
        CmdResult::PresspullEntity {
            handle,
            pick,
            distance,
            drag: Some(point),
            color: self.color,
        }
    }

    fn wants_text_input(&self) -> bool {
        self.picked.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let (handle, pick) = self.picked?;
        crate::entities::common::parse_typed_length(text)
            .filter(|distance| distance.abs() > 1e-6)
            .map(|distance| CmdResult::PresspullEntity {
                handle,
                pick,
                distance,
                drag: None,
                color: self.color,
            })
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        Some((self.picked?.1, self.direction?))
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        let (_, anchor) = self.picked?;
        Some(crate::command::DynSpec {
            anchor: crate::command::DynAnchor::Point(anchor),
            fields: vec![crate::command::DynFieldSpec::new(
                crate::command::DynRole::Distance,
            )],
            guide: crate::command::DynGuide::Radius,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        let (_, anchor) = self.picked?;
        Some((cursor - anchor).dot(self.direction?))
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
        Some(self.make_revolve(angle))
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

// ── Solid3D entity construction ────────────────────────────────────────────

pub fn empty_solid3d() -> EntityType {
    EntityType::Solid3D(Solid3D::new())
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["EXTRUDE", "THICKEN", "PRESSPULL"]
});
inventory::submit!(crate::command::CommandRegistration { names: &["LOFT"] });
inventory::submit!(crate::command::CommandRegistration { names: &["REVOLVE"] });
inventory::submit!(crate::command::CommandRegistration { names: &["SWEEP"] });
