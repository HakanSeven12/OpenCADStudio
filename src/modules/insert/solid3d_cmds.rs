// Profile-based kernel solid commands stored as exact ACIS data.

use std::sync::{Mutex, OnceLock};

use acadrust::{entities::Solid3D, EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, ExtrudeExtent, ExtrudeMode, SelectionEntity, WorkingPlane,
};
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
        self.step == ExtrudeStep::Path
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
        // A bare Enter finalizes the current source selection through
        // `on_enter`. Treating the empty string as an option prefix makes it
        // match every keyword and silently advances to the wrong branch.
        if value.is_empty() {
            return None;
        }
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
    fn is_selection_gathering(&self) -> bool {
        self.step == ExtrudeStep::Pick
    }
    fn selection_forces_add(&self) -> bool {
        self.step == ExtrudeStep::Pick
    }
    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        if self.step != ExtrudeStep::Pick {
            return;
        }
        self.handles = entities.iter().map(|entry| entry.handle).collect();
        self.preview_profiles = entities
            .iter()
            .map(|entry| (entry.handle, entry.entity.clone()))
            .collect();
        if let Some(curve) = entities
            .iter()
            .find_map(|entry| crate::entities::curve::entity_curve(&entry.entity))
        {
            self.anchor = DVec3::from_array(curve.plane.origin);
            self.profile_direction = curve.plane.normal().map(DVec3::from_array);
        } else {
            self.profile_direction = None;
        }
    }
    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
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
    handles: Vec<Handle>,
    preview_profiles: Vec<(Handle, EntityType)>,
    injected_axis: Option<EntityType>,
    axis_start: DVec3,
    axis_end: DVec3,
    working_plane: WorkingPlane,
    mode: ExtrudeMode,
    angle: f64,
    start_angle: f64,
    reverse: bool,
    color: [f32; 4],
}

#[derive(Clone, Copy)]
struct RevolveDefaults {
    mode: ExtrudeMode,
    angle: f64,
    start_angle: f64,
}

fn revolve_defaults() -> &'static Mutex<RevolveDefaults> {
    static DEFAULTS: OnceLock<Mutex<RevolveDefaults>> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        Mutex::new(RevolveDefaults {
            mode: ExtrudeMode::Solid,
            angle: std::f64::consts::TAU,
            start_angle: 0.0,
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RevolveStep {
    Pick,
    Mode,
    AxisStart,
    AxisEnd,
    AxisObject,
    Angle,
    StartAngle,
    Expression,
}

impl RevolveCommand {
    pub fn new(color: [f32; 4]) -> Self {
        let defaults = *revolve_defaults().lock().unwrap_or_else(|error| error.into_inner());
        Self {
            step: RevolveStep::Pick,
            handles: Vec::new(),
            preview_profiles: Vec::new(),
            injected_axis: None,
            axis_start: DVec3::ZERO,
            axis_end: DVec3::new(0.0, 0.0, 1.0),
            working_plane: WorkingPlane::default(),
            mode: defaults.mode,
            angle: defaults.angle,
            start_angle: defaults.start_angle,
            reverse: false,
            color,
        }
    }

    pub fn set_preselection(&mut self, profiles: Vec<(Handle, EntityType)>) {
        self.handles = profiles.iter().map(|(handle, _)| *handle).collect();
        self.preview_profiles = profiles;
        if !self.handles.is_empty() {
            self.step = RevolveStep::AxisStart;
        }
    }

    fn profile_reference(&self) -> Option<DVec3> {
        let axis = (self.axis_end - self.axis_start).try_normalize()?;
        self.preview_profiles
            .iter()
            .filter_map(|(_, entity)| {
                crate::scene::model::sweep_model::extrusion_profile_of(entity)
            })
            .flat_map(|(profile, _)| {
                profile.pieces.into_iter().flat_map(move |piece| {
                    [0.0, 0.5, 1.0].into_iter().map(move |parameter| {
                        DVec3::from_array(profile.plane.point_at(piece.point_at(parameter)))
                    })
                })
            })
            .filter(|point| point.is_finite())
            .max_by(|first, second| {
                let radial_length = |point: DVec3| {
                    let delta = point - self.axis_start;
                    (delta - axis * delta.dot(axis)).length_squared()
                };
                radial_length(*first).total_cmp(&radial_length(*second))
            })
    }

    fn angle_anchor(&self) -> Option<DVec3> {
        let axis = (self.axis_end - self.axis_start).try_normalize()?;
        let reference = self.profile_reference()?;
        Some(self.axis_start + axis * (reference - self.axis_start).dot(axis))
    }

    fn cursor_angle(&self, cursor: DVec3, full_at_zero: bool) -> Option<f64> {
        let axis = (self.axis_end - self.axis_start).try_normalize()?;
        let anchor = self.angle_anchor()?;
        let reference = (self.profile_reference()? - anchor).try_normalize()?;
        let radial = (cursor - (self.axis_start
            + axis * (cursor - self.axis_start).dot(axis)))
            .try_normalize()?;
        let mut angle = axis.dot(reference.cross(radial)).atan2(reference.dot(radial));
        if angle < 0.0 {
            angle += std::f64::consts::TAU;
        }
        if full_at_zero && angle.abs() <= 1e-9 {
            angle = std::f64::consts::TAU;
        }
        angle.is_finite().then_some(angle)
    }

    fn signed_angle(&self, angle: f64) -> f64 {
        angle * if self.reverse { -1.0 } else { 1.0 }
    }

    fn preview_wires(&self, cursor: DVec3) -> Vec<WireModel> {
        let (angle, start_angle) = match self.step {
            RevolveStep::Angle => (
                self.cursor_angle(cursor, true)
                    .map(|angle| self.signed_angle(angle)),
                Some(self.start_angle),
            ),
            RevolveStep::StartAngle => (
                Some(self.signed_angle(self.angle)),
                self.cursor_angle(cursor, false),
            ),
            _ => return Vec::new(),
        };
        let (Some(angle), Some(start_angle)) = (angle, start_angle) else {
            return Vec::new();
        };
        let from = self.axis_start.to_array();
        let to = self.axis_end.to_array();
        self.preview_profiles
            .iter()
            .flat_map(|(_, entity)| {
                let Some((_, closed)) =
                    crate::scene::model::sweep_model::extrusion_profile_of(entity)
                else {
                    return Vec::new();
                };
                let creates_surface = self.mode == ExtrudeMode::Surface || !closed;
                let body = if creates_surface {
                    crate::scene::model::sweep_model::revolved_surface(
                        entity,
                        from,
                        to,
                        angle,
                        start_angle,
                    )
                } else {
                    crate::scene::model::sweep_model::revolved(
                        entity,
                        from,
                        to,
                        angle,
                        start_angle,
                    )
                };
                body.map(|body| preview_body_wires(&body, self.color))
                    .unwrap_or_default()
            })
            .collect()
    }

    fn set_axis(&mut self, direction: DVec3) -> CmdResult {
        let Some(direction) = direction.try_normalize() else {
            return CmdResult::NeedPoint;
        };
        self.axis_start = self.working_plane.origin;
        self.axis_end = self.axis_start + direction;
        self.step = RevolveStep::Angle;
        CmdResult::NeedPoint
    }

    fn finish(&self, angle: f64) -> CmdResult {
        let angle = self.signed_angle(angle);
        if angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= std::f64::consts::TAU + 1e-9 {
            let mut defaults = revolve_defaults().lock().unwrap_or_else(|error| error.into_inner());
            defaults.mode = self.mode;
            defaults.angle = angle.abs();
            defaults.start_angle = self.start_angle;
        }
        CmdResult::RevolveEntities {
            handles: self.handles.clone(),
            axis_start: self.axis_start,
            axis_end: self.axis_end,
            angle,
            start_angle: self.start_angle,
            mode: self.mode,
            color: self.color,
        }
    }
}

fn revolve_axis(entity: &EntityType) -> Option<(DVec3, DVec3)> {
    let point = |value: &acadrust::types::Vector3| DVec3::new(value.x, value.y, value.z);
    let (start, direction) = match entity {
        EntityType::Line(line) => (point(&line.start), point(&line.end) - point(&line.start)),
        EntityType::Ray(ray) => (point(&ray.base_point), point(&ray.direction)),
        EntityType::XLine(line) => (point(&line.base_point), point(&line.direction)),
        EntityType::Polyline3D(line) if line.vertices.len() == 2 => {
            let start = point(&line.vertices[0].position);
            (start, point(&line.vertices[1].position) - start)
        }
        _ => return None,
    };
    let direction = direction.try_normalize()?;
    Some((start, start + direction))
}

impl CadCommand for RevolveCommand {
    fn name(&self) -> &'static str {
        "REVOLVE"
    }
    fn prompt(&self) -> String {
        match self.step {
            RevolveStep::Pick => t!("REVOLVE  Select objects to revolve or [Mode] (Enter to finish):").into_owned(),
            RevolveStep::Mode => format!(
                "{} <{}>:",
                t!("REVOLVE  Creation mode [Solid/Surface]"),
                if self.mode == ExtrudeMode::Solid { "Solid" } else { "Surface" }
            ),
            RevolveStep::AxisStart => t!("REVOLVE  Specify axis start point or [Object/X/Y/Z]:").into_owned(),
            RevolveStep::AxisEnd => t!("REVOLVE  Axis end point:").into_owned(),
            RevolveStep::AxisObject => t!("REVOLVE  Select line, ray, or construction line for axis:").into_owned(),
            RevolveStep::Angle => format!(
                "{} <{}>:",
                t!("REVOLVE  Specify angle of revolution or [Start angle/Reverse/Expression]"),
                crate::entities::common::format_angle(self.angle)
            ),
            RevolveStep::StartAngle => format!(
                "{} <{}>:",
                t!("REVOLVE  Specify start angle"),
                crate::entities::common::format_angle(self.start_angle)
            ),
            RevolveStep::Expression => t!("REVOLVE  Enter angle expression:").into_owned(),
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            RevolveStep::Pick => vec![CmdOption::new("Mode", "MODE"), CmdOption::enter("Done")],
            RevolveStep::Mode => vec![
                CmdOption::new("Solid", "SOLID"),
                CmdOption::new("Surface", "SURFACE"),
            ],
            RevolveStep::AxisStart => vec![
                CmdOption::new("Object", "OBJECT"),
                CmdOption::new("X", "X"),
                CmdOption::new("Y", "Y"),
                CmdOption::new("Z", "Z"),
            ],
            RevolveStep::Angle => vec![
                CmdOption::new("Start angle", "START"),
                CmdOption::new("Reverse", "REVERSE"),
                CmdOption::new("Expression", "EXPRESSION"),
            ],
            _ => Vec::new(),
        }
    }
    fn needs_entity_pick(&self) -> bool {
        self.step == RevolveStep::AxisObject
    }
    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            RevolveStep::AxisObject => {
                let Some((start, end)) = self.injected_axis.take().as_ref().and_then(revolve_axis)
                else {
                    return CmdResult::NeedPoint;
                };
                self.axis_start = start;
                self.axis_end = end;
                self.step = RevolveStep::Angle;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            RevolveStep::AxisStart => {
                self.axis_start = pt;
                self.step = RevolveStep::AxisEnd;
                CmdResult::NeedPoint
            }
            RevolveStep::AxisEnd => {
                if (pt - self.axis_start).length_squared() <= 1e-12 {
                    CmdResult::NeedPoint
                } else {
                    self.axis_end = pt;
                    self.step = RevolveStep::Angle;
                    CmdResult::NeedPoint
                }
            }
            RevolveStep::Angle => self
                .cursor_angle(pt, true)
                .map(|angle| self.finish(angle))
                .unwrap_or(CmdResult::NeedPoint),
            RevolveStep::StartAngle => {
                let Some(angle) = self.cursor_angle(pt, false) else {
                    return CmdResult::NeedPoint;
                };
                self.start_angle = angle;
                self.step = RevolveStep::Angle;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }
    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            RevolveStep::Pick
                | RevolveStep::Mode
                | RevolveStep::AxisStart
                | RevolveStep::Angle
                | RevolveStep::StartAngle
                | RevolveStep::Expression
        )
    }
    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, RevolveStep::AxisStart | RevolveStep::Angle)
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        if value.is_empty() {
            return None;
        }
        let upper = value.to_ascii_uppercase();
        match self.step {
            RevolveStep::Pick if "MODE".starts_with(&upper) => {
                self.step = RevolveStep::Mode;
                Some(CmdResult::NeedPoint)
            }
            RevolveStep::Mode => {
                if "SOLID".starts_with(&upper) {
                    self.mode = ExtrudeMode::Solid;
                } else if "SURFACE".starts_with(&upper) {
                    self.mode = ExtrudeMode::Surface;
                } else {
                    return Some(CmdResult::NeedPoint);
                }
                revolve_defaults()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .mode = self.mode;
                self.step = RevolveStep::Pick;
                Some(CmdResult::NeedPoint)
            }
            RevolveStep::AxisStart => {
                if "OBJECT".starts_with(&upper) {
                    self.step = RevolveStep::AxisObject;
                    Some(CmdResult::NeedPoint)
                } else if upper == "X" {
                    Some(self.set_axis(self.working_plane.x))
                } else if upper == "Y" {
                    Some(self.set_axis(self.working_plane.y))
                } else if upper == "Z" {
                    Some(self.set_axis(self.working_plane.z))
                } else {
                    None
                }
            }
            RevolveStep::Angle => {
                if "START ANGLE".starts_with(&upper) || "START".starts_with(&upper) {
                    self.step = RevolveStep::StartAngle;
                    return Some(CmdResult::NeedPoint);
                }
                if "REVERSE".starts_with(&upper) {
                    self.reverse = !self.reverse;
                    return Some(CmdResult::NeedPoint);
                }
                if "EXPRESSION".starts_with(&upper) {
                    self.step = RevolveStep::Expression;
                    return Some(CmdResult::NeedPoint);
                }
                crate::entities::common::parse_angle(value)
                    .filter(|angle| {
                        angle.is_finite()
                            && angle.abs() > 1e-9
                            && angle.abs() <= std::f64::consts::TAU + 1e-9
                    })
                    .map(|angle| self.finish(angle))
            }
            RevolveStep::StartAngle => {
                let angle = crate::entities::common::parse_angle(value)?;
                if !angle.is_finite() {
                    return Some(CmdResult::NeedPoint);
                }
                self.start_angle = angle;
                self.step = RevolveStep::Angle;
                Some(CmdResult::NeedPoint)
            }
            RevolveStep::Expression => {
                let angle = crate::app::expr_eval::eval_number(value)
                    .and_then(|value| crate::entities::common::parse_angle(&value.to_string()))?;
                if !angle.is_finite()
                    || angle.abs() <= 1e-9
                    || angle.abs() > std::f64::consts::TAU + 1e-9
                {
                    return Some(CmdResult::NeedPoint);
                }
                Some(self.finish(angle))
            }
            _ => None,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            RevolveStep::Pick if !self.handles.is_empty() => {
                self.step = RevolveStep::AxisStart;
                CmdResult::NeedPoint
            }
            RevolveStep::Mode => {
                self.step = RevolveStep::Pick;
                CmdResult::NeedPoint
            }
            RevolveStep::Angle => self.finish(self.angle),
            RevolveStep::StartAngle => {
                self.step = RevolveStep::Angle;
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.working_plane = plane;
    }
    fn is_selection_gathering(&self) -> bool {
        self.step == RevolveStep::Pick
    }
    fn selection_forces_add(&self) -> bool {
        self.step == RevolveStep::Pick
    }
    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        if self.step != RevolveStep::Pick {
            return;
        }
        self.handles = entities.iter().map(|entry| entry.handle).collect();
        self.preview_profiles = entities
            .into_iter()
            .map(|entry| (entry.handle, entry.entity))
            .collect();
    }
    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn inject_before_entity_pick(&self) -> bool {
        self.step == RevolveStep::AxisObject
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        if self.step == RevolveStep::AxisObject {
            self.injected_axis = Some(entity);
        }
    }
    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        self.preview_wires(cursor)
    }
    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.step, RevolveStep::Angle | RevolveStep::StartAngle) {
            crate::command::DynField::Angle
        } else {
            crate::command::DynField::Point
        }
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        matches!(self.step, RevolveStep::Angle | RevolveStep::StartAngle).then(|| {
            crate::command::DynSpec {
                anchor: crate::command::DynAnchor::Point(
                    self.angle_anchor().unwrap_or(self.axis_start),
                ),
                fields: vec![crate::command::DynFieldSpec::new(crate::command::DynRole::Angle)],
                guide: crate::command::DynGuide::None,
                ref_point: self.profile_reference(),
            }
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.step, RevolveStep::Angle | RevolveStep::StartAngle)
    }
    fn dyn_auto_sign_angle(&self) -> bool {
        false
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        let full_at_zero = self.step == RevolveStep::Angle;
        self.cursor_angle(cursor, full_at_zero).map(|angle| {
            let angle = if self.step == RevolveStep::Angle {
                self.signed_angle(angle)
            } else {
                angle
            };
            crate::command::dyn_display_angle_deg(angle as f32) as f64
        })
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

pub fn empty_revolved_surface(
    profile: &EntityType,
    axis_start: DVec3,
    axis_end: DVec3,
    angle: f64,
    start_angle: f64,
) -> EntityType {
    use acadrust::entities::{Surface, SurfaceData, SurfaceKind};
    use acadrust::types::Vector3;

    let mut surface = Surface::new(SurfaceKind::Revolved);
    let axis = (axis_end - axis_start).normalize_or(DVec3::Z);
    surface.point_of_reference = Vector3::new(axis_start.x, axis_start.y, axis_start.z);
    if let SurfaceData::Revolved {
        revolve_entity,
        axis_point,
        axis_vector,
        revolve_angle,
        start_angle: stored_start,
        entity_transform,
        solid,
        ..
    } = &mut surface.surface_data
    {
        if let Some((embedded, transform)) =
            crate::scene::model::sweep_model::embedded_revolve_profile(profile)
        {
            *revolve_entity = Some(embedded);
            *entity_transform = transform;
        }
        *axis_point = Vector3::new(axis_start.x, axis_start.y, axis_start.z);
        *axis_vector = Vector3::new(axis.x, axis.y, axis.z);
        *revolve_angle = angle;
        *stored_start = start_angle;
        *solid = false;
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
