// Profile-based kernel solid commands stored as exact ACIS data.

use std::sync::{Mutex, OnceLock};

use acadrust::{entities::Solid3D, EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, ExtrudeExtent, ExtrudeMode};
use crate::scene::WireModel;
use crate::t;

// ── EXTRUDE command ────────────────────────────────────────────────────────

pub struct ExtrudeCommand {
    command_name: &'static str,
    step: ExtrudeStep,
    handles: Vec<Handle>,
    preview_profiles: Vec<(Handle, EntityType)>,
    injected_profile: Option<EntityType>,
    anchor: DVec3,
    profile_direction: Option<DVec3>,
    direction_start: Option<DVec3>,
    mode: ExtrudeMode,
    taper_angle: f64,
    last_height: Option<f64>,
    taper_return: ExtrudeStep,
    color: [f32; 4],
}

#[derive(Clone, Copy)]
struct ExtrudeDefaults {
    mode: ExtrudeMode,
    height: Option<f64>,
    taper_angle: f64,
}

fn extrude_defaults() -> &'static Mutex<ExtrudeDefaults> {
    static DEFAULTS: OnceLock<Mutex<ExtrudeDefaults>> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        Mutex::new(ExtrudeDefaults {
            mode: ExtrudeMode::Solid,
            height: None,
            taper_angle: 0.0,
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtrudeStep {
    Pick,
    Mode,
    Height,
    DirectionStart,
    DirectionEnd,
    Path,
    Taper,
    Expression,
    TaperExpression,
}

impl ExtrudeCommand {
    pub fn new_named(name: &str, color: [f32; 4]) -> Self {
        let defaults = *extrude_defaults().lock().unwrap_or_else(|error| error.into_inner());
        Self {
            command_name: match name {
                "THICKEN" => "THICKEN",
                _ => "EXTRUDE",
            },
            step: ExtrudeStep::Pick,
            handles: Vec::new(),
            preview_profiles: Vec::new(),
            injected_profile: None,
            anchor: DVec3::ZERO,
            profile_direction: None,
            direction_start: None,
            mode: defaults.mode,
            taper_angle: defaults.taper_angle,
            last_height: defaults.height,
            taper_return: ExtrudeStep::Height,
            color,
        }
    }

    pub fn set_preselection(
        &mut self,
        profiles: Vec<(Handle, EntityType)>,
        anchor: DVec3,
        direction: Option<DVec3>,
    ) {
        self.handles = profiles.iter().map(|(handle, _)| *handle).collect();
        self.preview_profiles = profiles;
        self.anchor = anchor;
        self.profile_direction = direction.and_then(DVec3::try_normalize);
        if !self.handles.is_empty() {
            self.step = ExtrudeStep::Height;
        }
    }

    fn finish(&self, extent: ExtrudeExtent) -> CmdResult {
        if let Some(height) = match extent {
            ExtrudeExtent::Height(height) => Some(height),
            ExtrudeExtent::Direction(direction) => Some(direction.length()),
            ExtrudeExtent::Path(_) => None,
        } {
            extrude_defaults()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .height = Some(height);
        }
        CmdResult::ExtrudeEntities {
            handles: self.handles.clone(),
            extent,
            mode: self.mode,
            taper_angle: self.taper_angle,
            color: self.color,
        }
    }

    fn preview_wires(&self, cursor: DVec3) -> Vec<WireModel> {
        if self.step != ExtrudeStep::Height {
            return Vec::new();
        }
        let Some(axis) = self.profile_direction else {
            return Vec::new();
        };
        let height = (cursor - self.anchor).dot(axis);
        if !height.is_finite() || height.abs() <= 1e-6 {
            return Vec::new();
        }

        self.preview_profiles
            .iter()
            .flat_map(|(_, entity)| {
                let Some((profile, closed)) =
                    crate::scene::model::sweep_model::extrusion_profile_of(entity)
                else {
                    return Vec::new();
                };
                let Some(normal) = profile.plane.normal() else {
                    return Vec::new();
                };
                let direction = [normal[0] * height, normal[1] * height, normal[2] * height];
                let creates_surface = self.mode == ExtrudeMode::Surface || !closed;
                let body = if creates_surface {
                    crate::scene::model::sweep_model::extruded_surface(
                        entity,
                        direction,
                        self.taper_angle,
                    )
                } else {
                    crate::scene::model::sweep_model::extruded_direction(
                        entity,
                        direction,
                        self.taper_angle,
                    )
                };
                body.map(|body| preview_body_wires(&body, self.color))
                    .unwrap_or_default()
            })
            .collect()
    }
}

fn preview_body_wires(body: &cadkernel::brep::Body, color: [f32; 4]) -> Vec<WireModel> {
    let mesh = cadkernel::brep::mesh::tessellate(
        body,
        cadkernel::brep::mesh::TessellationTolerance::new(
            cadkernel::tessellation::DEFAULT_ANGLE,
            1e-9,
        ),
    );
    mesh.edges
        .into_iter()
        .filter(|edge| edge.positions.len() >= 2)
        .map(|edge| {
            WireModel::solid_f64(
                "EXTRUDE-PREVIEW".to_owned(),
                edge.positions,
                color,
                false,
            )
        })
        .collect()
}

impl CadCommand for ExtrudeCommand {
    fn name(&self) -> &'static str {
        self.command_name
    }
    fn prompt(&self) -> String {
        match self.step {
            ExtrudeStep::Pick => t!("EXTRUDE  Select objects to extrude or [Mode] (Enter to finish):").into_owned(),
            ExtrudeStep::Mode => format!(
                "{} <{}>:",
                t!("EXTRUDE  Creation mode [Solid/Surface]"),
                if self.mode == ExtrudeMode::Solid { "Solid" } else { "Surface" }
            ),
            ExtrudeStep::Height => {
                let base = t!("EXTRUDE  Specify height or [Direction/Path/Taper angle/Expression]");
                self.last_height.map_or_else(
                    || format!("{base}:"),
                    |height| format!("{base} <{}>:", crate::entities::common::format_length(height)),
                )
            }
            ExtrudeStep::DirectionStart => t!("EXTRUDE  Start point of direction:").into_owned(),
            ExtrudeStep::DirectionEnd => t!("EXTRUDE  End point of direction:").into_owned(),
            ExtrudeStep::Path => t!("EXTRUDE  Select extrusion path or [Taper angle]:").into_owned(),
            ExtrudeStep::Taper => format!(
                "{} <{}>:",
                t!("EXTRUDE  Specify taper angle or [Expression]"),
                crate::entities::common::format_angle(self.taper_angle)
            ),
            ExtrudeStep::Expression => t!("EXTRUDE  Enter height expression:").into_owned(),
            ExtrudeStep::TaperExpression => t!("EXTRUDE  Enter taper angle expression:").into_owned(),
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            ExtrudeStep::Pick => vec![CmdOption::new("Mode", "MODE"), CmdOption::enter("Done")],
            ExtrudeStep::Mode => vec![
                CmdOption::new("Solid", "SOLID"),
                CmdOption::new("Surface", "SURFACE"),
            ],
            ExtrudeStep::Height => vec![
                CmdOption::new("Direction", "DIRECTION"),
                CmdOption::new("Path", "PATH"),
                CmdOption::new("Taper angle", "TAPER"),
                CmdOption::new("Expression", "EXPRESSION"),
            ],
            ExtrudeStep::Path => vec![CmdOption::new("Taper angle", "TAPER")],
            ExtrudeStep::Taper => vec![CmdOption::new("Expression", "EXPRESSION")],
            _ => Vec::new(),
        }
    }
    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, ExtrudeStep::Pick | ExtrudeStep::Path)
    }
    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }
    fn set_entity_pick_direction(&mut self, direction: Option<DVec3>) {
        if self.step == ExtrudeStep::Pick {
            self.profile_direction = direction.and_then(DVec3::try_normalize);
        }
    }
    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            ExtrudeStep::Pick => {
                if !self.handles.contains(&handle) {
                    self.handles.push(handle);
                    self.anchor = point;
                    if let Some(entity) = self.injected_profile.take() {
                        self.preview_profiles.push((handle, entity));
                    }
                } else {
                    self.injected_profile = None;
                }
                CmdResult::NeedPoint
            }
            ExtrudeStep::Path if !self.handles.contains(&handle) => {
                self.finish(ExtrudeExtent::Path(handle))
            }
            _ => CmdResult::NeedPoint,
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            ExtrudeStep::Height => {
                let height = self
                    .profile_direction
                    .map(|direction| (pt - self.anchor).dot(direction))
                    .unwrap_or_else(|| pt.distance(self.anchor));
                if height.is_finite() && height.abs() > 1e-6 {
                    self.finish(ExtrudeExtent::Height(height))
                } else {
                    CmdResult::NeedPoint
                }
            }
            ExtrudeStep::DirectionStart => {
                self.direction_start = Some(pt);
                self.step = ExtrudeStep::DirectionEnd;
                CmdResult::NeedPoint
            }
            ExtrudeStep::DirectionEnd => {
                let direction = pt - self.direction_start.unwrap_or(pt);
                if direction.is_finite() && direction.length_squared() > 1e-12 {
                    self.finish(ExtrudeExtent::Direction(direction))
                } else {
                    CmdResult::NeedPoint
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }
    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            ExtrudeStep::Pick
                | ExtrudeStep::Mode
                | ExtrudeStep::Height
                | ExtrudeStep::Path
                | ExtrudeStep::Taper
                | ExtrudeStep::Expression
                | ExtrudeStep::TaperExpression
        )
    }
    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, ExtrudeStep::Height | ExtrudeStep::Path)
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        match self.step {
            ExtrudeStep::Pick if "MODE".starts_with(&value.to_ascii_uppercase()) => {
                self.step = ExtrudeStep::Mode;
                Some(CmdResult::NeedPoint)
            }
            ExtrudeStep::Mode => {
                let upper = value.to_ascii_uppercase();
                if "SOLID".starts_with(&upper) {
                    self.mode = ExtrudeMode::Solid;
                } else if "SURFACE".starts_with(&upper) {
                    self.mode = ExtrudeMode::Surface;
                } else {
                    return Some(CmdResult::NeedPoint);
                }
                extrude_defaults()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .mode = self.mode;
                self.step = ExtrudeStep::Pick;
                Some(CmdResult::NeedPoint)
            }
            ExtrudeStep::Height => {
                let upper = value.to_ascii_uppercase();
                if "DIRECTION".starts_with(&upper) {
                    self.step = ExtrudeStep::DirectionStart;
                    return Some(CmdResult::NeedPoint);
                }
                if "PATH".starts_with(&upper) {
                    self.step = ExtrudeStep::Path;
                    return Some(CmdResult::NeedPoint);
                }
                if "TAPER".starts_with(&upper) || "TAPER ANGLE".starts_with(&upper) {
                    self.taper_return = ExtrudeStep::Height;
                    self.step = ExtrudeStep::Taper;
                    return Some(CmdResult::NeedPoint);
                }
                if "EXPRESSION".starts_with(&upper) {
                    self.step = ExtrudeStep::Expression;
                    return Some(CmdResult::NeedPoint);
                }
                crate::entities::common::parse_typed_length(value)
                    .filter(|height| height.is_finite() && height.abs() > 1e-6)
                    .map(|height| self.finish(ExtrudeExtent::Height(height)))
            }
            ExtrudeStep::Path => {
                let upper = value.to_ascii_uppercase();
                if "TAPER".starts_with(&upper) || "TAPER ANGLE".starts_with(&upper) {
                    self.taper_return = ExtrudeStep::Path;
                    self.step = ExtrudeStep::Taper;
                }
                Some(CmdResult::NeedPoint)
            }
            ExtrudeStep::Taper => {
                if "EXPRESSION".starts_with(&value.to_ascii_uppercase()) {
                    self.step = ExtrudeStep::TaperExpression;
                    return Some(CmdResult::NeedPoint);
                }
                let angle = crate::entities::common::parse_angle(value)?;
                if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 {
                    return Some(CmdResult::NeedPoint);
                }
                self.taper_angle = angle;
                extrude_defaults()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .taper_angle = angle;
                self.step = self.taper_return;
                Some(CmdResult::NeedPoint)
            }
            ExtrudeStep::Expression => crate::app::expr_eval::eval_number(value)
                .filter(|height| height.is_finite() && height.abs() > 1e-6)
                .map(|height| self.finish(ExtrudeExtent::Height(height))),
            ExtrudeStep::TaperExpression => {
                let angle = crate::app::expr_eval::eval_number(value)
                    .and_then(|value| crate::entities::common::parse_angle(&value.to_string()))?;
                if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 {
                    return Some(CmdResult::NeedPoint);
                }
                self.taper_angle = angle;
                extrude_defaults()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .taper_angle = angle;
                self.step = self.taper_return;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            ExtrudeStep::Pick if !self.handles.is_empty() => {
                self.step = ExtrudeStep::Height;
                CmdResult::NeedPoint
            }
            ExtrudeStep::Mode => {
                self.step = ExtrudeStep::Pick;
                CmdResult::NeedPoint
            }
            ExtrudeStep::Height => self
                .last_height
                .filter(|height| height.is_finite() && height.abs() > 1e-6)
                .map(|height| self.finish(ExtrudeExtent::Height(height)))
                .unwrap_or(CmdResult::Cancel),
            ExtrudeStep::Taper => {
                self.step = self.taper_return;
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }
    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        (self.step == ExtrudeStep::Height)
            .then_some((self.anchor, self.profile_direction?))
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
        Some((cursor - self.anchor).dot(self.profile_direction?))
    }
    fn inject_before_entity_pick(&self) -> bool {
        self.step == ExtrudeStep::Pick
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        if self.step == ExtrudeStep::Pick {
            self.injected_profile = Some(entity);
        }
    }
    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        self.preview_wires(cursor)
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

pub fn empty_extruded_surface(direction: DVec3, taper_angle: f64) -> EntityType {
    use acadrust::entities::{Surface, SurfaceData, SurfaceKind};
    use acadrust::types::Vector3;

    let mut surface = Surface::new(SurfaceKind::Extruded);
    if let SurfaceData::Extruded {
        options,
        sweep_vector,
        ..
    } = &mut surface.surface_data
    {
        options.draft_angle = taper_angle;
        options.is_solid = false;
        *sweep_vector = Vector3::new(direction.x, direction.y, direction.z);
    }
    EntityType::Surface(surface)
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["EXTRUDE", "THICKEN", "PRESSPULL"]
});
inventory::submit!(crate::command::CommandRegistration { names: &["LOFT"] });
inventory::submit!(crate::command::CommandRegistration { names: &["REVOLVE"] });
inventory::submit!(crate::command::CommandRegistration { names: &["SWEEP"] });
