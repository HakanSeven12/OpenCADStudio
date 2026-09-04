// Profile-based kernel solid commands stored as exact ACIS data.

use std::sync::{Mutex, OnceLock};

use acadrust::{entities::Solid3D, EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, ExtrudeExtent, ExtrudeMode, LoftOptions,
    LoftSectionSelection, SelectionEntity, SweepOptions, WorkingPlane,
};
use crate::scene::WireModel;
use crate::scene::model::presspull_model::{PresspullTarget, PresspullTargetKind};
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
                body.map(|body| preview_body_wires(&body, self.color, 0))
                    .unwrap_or_default()
            })
            .collect()
    }
}

fn preview_body_wires(
    body: &cadkernel::brep::Body,
    color: [f32; 4],
    isolines: usize,
) -> Vec<WireModel> {
    // This overlay only consumes curves. Do not triangulate the body's faces
    // on every cursor event just to discard the resulting surface mesh.
    let wireframe = cadkernel::brep::mesh::tessellate_wireframe(
        body,
        cadkernel::brep::mesh::TessellationTolerance::new(
            cadkernel::tessellation::DEFAULT_ANGLE,
            1e-9,
        )
        .with_isolines(isolines),
    );
    let mut wires = wireframe
        .edges
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
        .collect::<Vec<_>>();
    wires.extend(
        wireframe.isolines
            .into_iter()
            .filter(|line| line.positions.len() >= 2)
            .map(|line| {
                WireModel::solid_f64(
                    "REVOLVE-PREVIEW-ISOLINE".to_owned(),
                    line.positions,
                    color,
                    false,
                )
            }),
    );
    wires
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
    step: PresspullStep,
    targets: Vec<PresspullTarget>,
    ctrl: bool,
    shift: bool,
    isolines: usize,
    target_generation: u64,
    preview_key: Option<(u64, u64)>,
    preview_cache: Vec<WireModel>,
    working_plane: WorkingPlane,
    hover_key: Option<(u64, Option<Handle>, [u64; 3], bool)>,
    hover_cache: Vec<WireModel>,
    color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresspullStep {
    Pick,
    Multiple,
    Height,
}

impl PresspullCommand {
    pub fn new(color: [f32; 4]) -> Self {
        Self {
            step: PresspullStep::Pick,
            targets: Vec::new(),
            ctrl: false,
            shift: false,
            isolines: 0,
            target_generation: 0,
            preview_key: None,
            preview_cache: Vec::new(),
            working_plane: WorkingPlane::default(),
            hover_key: None,
            hover_cache: Vec::new(),
            color,
        }
    }

    pub fn set_isolines(&mut self, isolines: usize) {
        if self.isolines != isolines {
            self.isolines = isolines;
            self.invalidate_preview();
        }
    }

    pub fn set_preselection(&mut self, targets: Vec<PresspullTarget>) {
        self.targets.clear();
        self.step = PresspullStep::Pick;
        self.invalidate_preview();
        for target in targets {
            self.on_presspull_target(target);
        }
    }

    fn invalidate_preview(&mut self) {
        self.target_generation = self.target_generation.wrapping_add(1);
        self.preview_key = None;
        self.preview_cache.clear();
        self.hover_key = None;
        self.hover_cache.clear();
    }

    fn has_target(&self, target: &PresspullTarget) -> bool {
        self.targets.iter().any(|selected| match (&selected.kind, &target.kind) {
            (
                PresspullTargetKind::Profile { source: Some(first), .. },
                PresspullTargetKind::Profile { source: Some(second), .. },
            ) => first == second,
            (
                PresspullTargetKind::Face { handle: first, face: first_face, .. },
                PresspullTargetKind::Face { handle: second, face: second_face, .. },
            ) => first == second && first_face == second_face,
            (
                PresspullTargetKind::Profile { source: None, entity: first, owner: first_owner },
                PresspullTargetKind::Profile { source: None, entity: second, owner: second_owner },
            ) => first_owner == second_owner && first == second,
            _ => false,
        })
    }

    fn pick(&self, handle: Option<Handle>, point: DVec3) -> CmdResult {
        if !point.is_finite() {
            return CmdResult::NeedPoint;
        }
        CmdResult::PresspullPick {
            handle: handle.filter(|handle| !handle.is_null()),
            point,
            offset: self.ctrl,
            multiple: self.step == PresspullStep::Multiple || self.shift,
        }
    }

    fn height(&self, point: DVec3) -> Option<f64> {
        let target = self.targets.last()?;
        let distance = (point - target.anchor).dot(target.direction);
        distance.is_finite().then_some(distance)
    }

    fn finish(&self, distance: f64) -> CmdResult {
        if self.targets.is_empty() || !distance.is_finite() || distance.abs() <= 1e-6 {
            return CmdResult::NeedPoint;
        }
        CmdResult::PresspullApply {
            targets: self.targets.clone(),
            distance,
            color: self.color,
        }
    }

    fn undo_selection(&mut self) -> CmdResult {
        if self.targets.pop().is_some() {
            self.invalidate_preview();
        }
        if self.targets.is_empty() {
            self.step = PresspullStep::Pick;
        }
        CmdResult::NeedPoint
    }
}

impl CadCommand for PresspullCommand {
    fn name(&self) -> &'static str {
        "PRESSPULL"
    }

    fn prompt(&self) -> String {
        match self.step {
            PresspullStep::Pick => {
                t!("PRESSPULL  Select object or bounded area (Enter to finish):").into_owned()
            }
            PresspullStep::Multiple => {
                t!("PRESSPULL  Select additional objects or bounded areas [Undo] (Enter for height):").into_owned()
            }
            PresspullStep::Height => {
                t!("PRESSPULL  Specify signed extrusion height or [Multiple/Undo]:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            PresspullStep::Pick => vec![CmdOption::enter("Done")],
            PresspullStep::Multiple => {
                vec![CmdOption::new("Undo", "UNDO"), CmdOption::enter("Height")]
            }
            PresspullStep::Height => {
                vec![CmdOption::new("Multiple", "MULTIPLE"), CmdOption::new("Undo", "UNDO")]
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.step != PresspullStep::Height || self.shift
    }

    fn entity_pick_accepts_points(&self) -> bool {
        true
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        if self.working_plane.origin != plane.origin || self.working_plane.x != plane.x
            || self.working_plane.y != plane.y
        {
            self.hover_key = None;
            self.hover_cache.clear();
        }
        self.working_plane = plane;
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn entity_pick_deferred_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    fn on_deferred_entity_hover(
        &mut self, scene: &crate::scene::Scene, handle: Option<Handle>, point: DVec3,
    ) -> Vec<WireModel> {
        if !self.needs_entity_pick() {
            return Vec::new();
        }
        let key = (scene.geometry_epoch, handle, point.to_array().map(f64::to_bits), self.ctrl);
        if self.hover_key == Some(key) {
            return self.hover_cache.clone();
        }
        self.hover_key = Some(key);
        self.hover_cache.clear();
        let Ok(target) = crate::scene::model::presspull_model::resolve_target(
            scene, handle, point, self.working_plane, self.ctrl,
        ) else { return Vec::new(); };
        let geometry = match target.kind {
            PresspullTargetKind::Profile { entity, .. } =>
                crate::scene::model::presspull_model::profile_geometry(&entity)
                    .map(|(plane, loops, _)| (plane, loops)),
            PresspullTargetKind::Face { body, face, .. } =>
                cadkernel::brep::planar_face_profile(&body, face)
                    .map(|profile| (profile.plane, profile.loops)),
        };
        if let Some((plane, loops)) = geometry {
            self.hover_cache = loops.into_iter().flatten().filter_map(|curve| {
                let points = curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE)
                    .into_iter().map(|point| plane.point_at(point)).collect::<Vec<_>>();
                (points.len() >= 2).then(|| {
                    let mut wire = WireModel::solid_f64(
                        "PRESSPULL-BOUNDARY-HOVER".to_owned(), points, [1.0, 0.65, 0.1, 1.0], false,
                    );
                    wire.line_weight_px = 2.0;
                    wire
                })
            }).collect();
        }
        self.hover_cache.clone()
    }

    fn on_entity_pick(&mut self, handle: acadrust::Handle, point: DVec3) -> CmdResult {
        if !self.needs_entity_pick() {
            return CmdResult::NeedPoint;
        }
        self.pick(Some(handle), point)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.needs_entity_pick() {
            return self.pick(None, point);
        }
        self.height(point).map_or(CmdResult::NeedPoint, |height| self.finish(height))
    }

    fn on_presspull_target(&mut self, mut target: PresspullTarget) {
        let Some(direction) = target.direction.try_normalize() else {
            return;
        };
        if !target.anchor.is_finite() || self.has_target(&target) {
            return;
        }
        target.direction = direction;
        self.targets.push(target);
        if self.step != PresspullStep::Multiple {
            self.step = PresspullStep::Height;
        }
        self.invalidate_preview();
    }

    fn on_presspull_applied(&mut self, success: bool) {
        if success {
            self.targets.clear();
            self.step = PresspullStep::Pick;
            self.invalidate_preview();
        } else if !self.targets.is_empty() {
            self.step = PresspullStep::Height;
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.needs_entity_pick()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return Some(self.on_enter());
        }
        match text.trim_start_matches('_').to_ascii_uppercase().as_str() {
            "M" | "MULTIPLE" if !self.targets.is_empty() => {
                self.step = PresspullStep::Multiple;
                return Some(CmdResult::NeedPoint);
            }
            "U" | "UNDO" => return Some(self.undo_selection()),
            _ => {}
        }
        // The shared picker resolves coordinates and hexadecimal handles.
        // Do not consume either as a distance while selecting targets.
        if self.needs_entity_pick() || text.starts_with("0x") || text.starts_with("0X") {
            return None;
        }
        crate::entities::common::parse_typed_length(text).map(|height| self.finish(height))
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step == PresspullStep::Multiple && !self.targets.is_empty() {
            self.step = PresspullStep::Height;
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        (!self.targets.is_empty()).then(|| self.undo_selection())
    }

    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        if self.needs_entity_pick() {
            return None;
        }
        let target = self.targets.last()?;
        Some((target.anchor, target.direction))
    }

    fn resolved_anchor(&self) -> Option<DVec3> {
        self.targets.last().map(|target| target.anchor)
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        let (anchor, _) = self.cursor_axis()?;
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
        if self.needs_entity_pick() {
            return None;
        }
        self.height(cursor)
    }

    fn dyn_commit_as_text(&self) -> bool {
        !self.needs_entity_pick()
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        if self.needs_entity_pick() {
            return Vec::new();
        }
        let Some(height) = self.height(cursor).filter(|height| height.abs() > 1e-6) else {
            return Vec::new();
        };
        let key = (self.target_generation, height.to_bits());
        if self.preview_key != Some(key) {
            self.preview_cache = self.targets.iter().flat_map(|target| {
                crate::scene::model::presspull_model::preview_wires(
                    target,
                    height,
                    self.color,
                    self.isolines,
                )
            }).collect();
            self.preview_key = Some(key);
        }
        self.preview_cache.clone()
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
    mode_return: RevolveStep,
    angle: f64,
    start_angle: f64,
    reverse: bool,
    isolines: usize,
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
    pub fn new(color: [f32; 4], isolines: usize) -> Self {
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
            mode_return: RevolveStep::Pick,
            angle: defaults.angle,
            start_angle: defaults.start_angle,
            reverse: false,
            isolines,
            color,
        }
    }

    pub fn set_preselection(&mut self, profiles: Vec<(Handle, EntityType)>) {
        let selected_handles = profiles.iter().map(|(handle, _)| *handle).collect::<Vec<_>>();
        self.preview_profiles = profiles
            .into_iter()
            .filter(|(_, entity)| {
                crate::scene::model::sweep_model::extrusion_profile_of(entity).is_some()
            })
            .collect();
        self.handles = if self.preview_profiles.is_empty() {
            Vec::new()
        } else {
            selected_handles
        };
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
        let cursor_direction = cursor - anchor;
        let radial = (cursor_direction - axis * cursor_direction.dot(axis)).try_normalize()?;
        let mut angle = axis
            .dot(reference.cross(radial))
            .atan2(reference.dot(radial));
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
                body.map(|body| preview_body_wires(&body, self.color, self.isolines))
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
            RevolveStep::AxisStart => t!("REVOLVE  Specify axis start point or [Object/X/Y/Z/Mode]:").into_owned(),
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
                CmdOption::new("Mode", "MODE"),
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
                self.mode_return = RevolveStep::Pick;
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
                self.step = self.mode_return;
                Some(CmdResult::NeedPoint)
            }
            RevolveStep::AxisStart => {
                if "OBJECT".starts_with(&upper) {
                    self.step = RevolveStep::AxisObject;
                    Some(CmdResult::NeedPoint)
                } else if "MODE".starts_with(&upper) {
                    self.mode_return = RevolveStep::AxisStart;
                    self.step = RevolveStep::Mode;
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
                self.step = self.mode_return;
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
        let selected_handles = entities.iter().map(|entry| entry.handle).collect::<Vec<_>>();
        self.preview_profiles = entities
            .into_iter()
            .filter(|entry| {
                crate::scene::model::sweep_model::extrusion_profile_of(&entry.entity).is_some()
            })
            .map(|entry| (entry.handle, entry.entity))
            .collect();
        self.handles = if self.preview_profiles.is_empty() {
            Vec::new()
        } else {
            selected_handles
        };
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
    fn cursor_plane(&self) -> Option<(DVec3, DVec3)> {
        if !matches!(self.step, RevolveStep::Angle | RevolveStep::StartAngle) {
            return None;
        }
        Some((
            (self.axis_end - self.axis_start).try_normalize()?,
            self.angle_anchor()?,
        ))
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
    profiles: Vec<(Handle, EntityType)>,
    injected_path: Option<EntityType>,
    hover_path: Option<(Handle, EntityType)>,
    preview_key: Option<(Handle, ExtrudeMode, SweepOptions)>,
    preview_cache: Vec<WireModel>,
    mode: ExtrudeMode,
    options: SweepOptions,
    reference_start: Option<DVec3>,
    reference_length: f64,
    new_length_start: Option<DVec3>,
    isolines: usize,
    color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SweepStep {
    PickProfiles,
    Mode,
    PickPath,
    Alignment,
    BasePoint,
    Scale,
    ReferenceLength,
    ReferenceEnd,
    NewLength,
    NewLengthStart,
    NewLengthEnd,
    Twist,
}

impl SweepCommand {
    pub fn new(color: [f32; 4], isolines: usize) -> Self {
        Self {
            step: SweepStep::PickProfiles,
            profiles: Vec::new(),
            injected_path: None,
            hover_path: None,
            preview_key: None,
            preview_cache: Vec::new(),
            mode: ExtrudeMode::Solid,
            options: SweepOptions::default(),
            reference_start: None,
            reference_length: 1.0,
            new_length_start: None,
            isolines,
            color,
        }
    }

    pub fn set_preselection(&mut self, profiles: Vec<(Handle, EntityType)>) {
        self.set_profiles(profiles);
        if !self.profiles.is_empty() {
            self.step = SweepStep::PickPath;
        }
    }

    fn set_profiles(&mut self, profiles: Vec<(Handle, EntityType)>) {
        self.profiles.clear();
        for (handle, entity) in profiles {
            if !handle.is_null()
                && !self.profiles.iter().any(|(selected, _)| *selected == handle)
                && crate::scene::model::sweep_model::is_sweep_profile(&entity)
            {
                self.profiles.push((handle, entity));
            }
        }
        self.preview_key = None;
        self.preview_cache.clear();
    }

    fn contains_profile(&self, handle: Handle) -> bool {
        self.profiles.iter().any(|(selected, _)| *selected == handle)
    }

    fn selection_options(&self) -> Option<SweepOptions> {
        let profiles = self.profiles.iter().map(|(_, entity)| entity.clone()).collect::<Vec<_>>();
        crate::scene::model::sweep_model::sweep_selection_options(&profiles, self.options)
    }

    fn finish(&self, path_handle: Handle) -> CmdResult {
        CmdResult::SweepEntities {
            handles: self.profiles.iter().map(|(handle, _)| *handle).collect(),
            path_handle,
            mode: self.mode,
            options: self.options,
            color: self.color,
        }
    }

    fn set_scale(&mut self, scale: f64) -> bool {
        if !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        self.options.scale = scale;
        self.step = SweepStep::PickPath;
        true
    }

    fn set_reference_length(&mut self, length: f64) -> bool {
        if !length.is_finite() || length <= 1e-12 {
            return false;
        }
        self.reference_length = length;
        self.step = SweepStep::NewLength;
        true
    }

    fn length_anchor(&self) -> DVec3 {
        self.reference_start
            .or_else(|| self.selection_options().and_then(|options| options.base_point))
            .unwrap_or(DVec3::ZERO)
    }
}

impl CadCommand for SweepCommand {
    fn name(&self) -> &'static str {
        "SWEEP"
    }
    fn prompt(&self) -> String {
        match self.step {
            SweepStep::PickProfiles => t!("SWEEP  Select objects to sweep or [Mode] (Enter to finish):").into_owned(),
            SweepStep::Mode => format!(
                "{} <{}>:",
                t!("SWEEP  Creation mode [Solid/Surface]"),
                if self.mode == ExtrudeMode::Solid { "Solid" } else { "Surface" },
            ),
            SweepStep::PickPath => t!("SWEEP  Select sweep path or [Alignment/Base point/Scale/Twist]:").into_owned(),
            SweepStep::Alignment => format!(
                "{} <{}>:",
                t!("SWEEP  Align sweep object perpendicular to path [Yes/No]"),
                if self.options.align { "Yes" } else { "No" },
            ),
            SweepStep::BasePoint => t!("SWEEP  Specify base point:").into_owned(),
            SweepStep::Scale => format!(
                "{} <{}>:", t!("SWEEP  Enter scale factor or [Reference]"), self.options.scale,
            ),
            SweepStep::ReferenceLength => format!(
                "{} <{}>:",
                t!("SWEEP  Specify reference length or first point"),
                crate::entities::common::format_length(self.reference_length),
            ),
            SweepStep::ReferenceEnd => t!("SWEEP  Specify second reference point:").into_owned(),
            SweepStep::NewLength => format!(
                "{} <{}>:",
                t!("SWEEP  Specify new length or [Points]"),
                crate::entities::common::format_length(self.reference_length * self.options.scale),
            ),
            SweepStep::NewLengthStart => t!("SWEEP  Specify first point of new length:").into_owned(),
            SweepStep::NewLengthEnd => t!("SWEEP  Specify second point of new length:").into_owned(),
            SweepStep::Twist => format!(
                "{} <{}>:",
                t!("SWEEP  Specify twist angle or [Bank]"),
                crate::entities::common::format_angle(self.options.twist_angle),
            ),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            SweepStep::PickProfiles => vec![CmdOption::new("Mode", "MODE"), CmdOption::enter("Done")],
            SweepStep::Mode => vec![CmdOption::new("Solid", "SOLID"), CmdOption::new("Surface", "SURFACE")],
            SweepStep::PickPath => vec![
                CmdOption::new("Alignment", "ALIGNMENT"),
                CmdOption::new("Base point", "BASE"),
                CmdOption::new("Scale", "SCALE"),
                CmdOption::new("Twist", "TWIST"),
            ],
            SweepStep::Alignment => vec![CmdOption::new("Yes", "YES"), CmdOption::new("No", "NO")],
            SweepStep::Scale => vec![CmdOption::new("Reference", "REFERENCE")],
            SweepStep::NewLength => vec![CmdOption::new("Points", "POINTS")],
            SweepStep::Twist => vec![CmdOption::new("Bank", "BANK")],
            _ => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == SweepStep::PickPath
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.step == SweepStep::PickPath
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.step != SweepStep::PickPath || handle.is_null() || self.contains_profile(handle) {
            self.injected_path = None;
            return CmdResult::NeedPoint;
        }
        let path = self.injected_path.take().or_else(|| {
            self.hover_path.as_ref()
                .filter(|(hovered, _)| *hovered == handle)
                .map(|(_, entity)| entity.clone())
        });
        if path.as_ref().is_some_and(crate::scene::model::sweep_model::is_sweep_path) {
            self.finish(handle)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !point.is_finite() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            SweepStep::BasePoint => {
                self.options.base_point = Some(point);
                self.step = SweepStep::PickPath;
            }
            SweepStep::ReferenceLength => {
                self.reference_start = Some(point);
                self.step = SweepStep::ReferenceEnd;
            }
            SweepStep::ReferenceEnd => {
                if let Some(start) = self.reference_start {
                    self.set_reference_length(point.distance(start));
                }
            }
            SweepStep::NewLength => {
                self.set_scale(point.distance(self.length_anchor()) / self.reference_length);
            }
            SweepStep::NewLengthStart => {
                self.new_length_start = Some(point);
                self.step = SweepStep::NewLengthEnd;
            }
            SweepStep::NewLengthEnd => {
                if let Some(start) = self.new_length_start {
                    self.set_scale(point.distance(start) / self.reference_length);
                }
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step,
            SweepStep::PickProfiles | SweepStep::Mode | SweepStep::PickPath
                | SweepStep::Alignment | SweepStep::Scale | SweepStep::ReferenceLength
                | SweepStep::NewLength | SweepStep::Twist
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, SweepStep::PickPath | SweepStep::ReferenceLength | SweepStep::NewLength)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        if value.is_empty() {
            return None;
        }
        let keyword = value.to_ascii_uppercase();
        match self.step {
            SweepStep::PickProfiles if "MODE".starts_with(&keyword) => {
                self.step = SweepStep::Mode;
            }
            SweepStep::Mode => {
                if "SOLID".starts_with(&keyword) {
                    self.mode = ExtrudeMode::Solid;
                } else if "SURFACE".starts_with(&keyword) {
                    self.mode = ExtrudeMode::Surface;
                } else {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = SweepStep::PickProfiles;
            }
            SweepStep::PickPath => {
                self.step = if "ALIGNMENT".starts_with(&keyword) {
                    SweepStep::Alignment
                } else if "BASE".starts_with(&keyword) || "BASE POINT".starts_with(&keyword) {
                    SweepStep::BasePoint
                } else if "SCALE".starts_with(&keyword) {
                    SweepStep::Scale
                } else if "TWIST".starts_with(&keyword) {
                    SweepStep::Twist
                } else {
                    // Unconsumed text can be an object handle from automation.
                    return None;
                };
            }
            SweepStep::Alignment => {
                if "YES".starts_with(&keyword) {
                    self.options.align = true;
                } else if "NO".starts_with(&keyword) {
                    self.options.align = false;
                } else {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = SweepStep::PickPath;
            }
            SweepStep::Scale => {
                if "REFERENCE".starts_with(&keyword) {
                    self.reference_start = None;
                    self.new_length_start = None;
                    self.step = SweepStep::ReferenceLength;
                } else if let Ok(scale) = value.parse::<f64>() {
                    self.set_scale(scale);
                }
            }
            SweepStep::ReferenceLength => {
                if let Some(length) = crate::entities::common::parse_typed_length(value) {
                    self.set_reference_length(length);
                } else {
                    return None;
                }
            }
            SweepStep::NewLength => {
                if "POINTS".starts_with(&keyword) {
                    self.step = SweepStep::NewLengthStart;
                } else if let Some(length) = crate::entities::common::parse_typed_length(value) {
                    self.set_scale(length / self.reference_length);
                } else {
                    return None;
                }
            }
            SweepStep::Twist => {
                if "BANK".starts_with(&keyword) {
                    self.options.bank = true;
                    self.step = SweepStep::PickPath;
                } else if let Some(angle) = crate::entities::common::parse_angle(value) {
                    if angle.is_finite() {
                        self.options.twist_angle = angle;
                        self.options.bank = false;
                        self.step = SweepStep::PickPath;
                    }
                }
            }
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            SweepStep::PickProfiles if !self.profiles.is_empty() => {
                self.step = SweepStep::PickPath;
            }
            SweepStep::Mode => self.step = SweepStep::PickProfiles,
            SweepStep::Alignment | SweepStep::Scale | SweepStep::Twist
                | SweepStep::NewLength => self.step = SweepStep::PickPath,
            SweepStep::ReferenceLength => self.step = SweepStep::NewLength,
            _ => return CmdResult::Cancel,
        }
        CmdResult::NeedPoint
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == SweepStep::PickProfiles
    }

    fn selection_forces_add(&self) -> bool {
        self.step == SweepStep::PickProfiles
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        if self.step == SweepStep::PickProfiles {
            self.set_profiles(entities.into_iter().map(|entry| (entry.handle, entry.entity)).collect());
        }
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.step == SweepStep::PickPath
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        if self.step == SweepStep::PickPath {
            self.injected_path = Some(entity);
        }
    }

    fn wants_hover_entity(&self, handle: Handle) -> bool {
        self.step == SweepStep::PickPath
            && !handle.is_null()
            && !self.contains_profile(handle)
            && self.hover_path.as_ref().is_none_or(|(hovered, _)| *hovered != handle)
    }

    fn inject_hover_entity(&mut self, handle: Handle, entity: EntityType) {
        self.hover_path = Some((handle, entity));
        self.preview_key = None;
    }

    fn on_hover_entity(&mut self, handle: Handle, _point: DVec3) -> Vec<WireModel> {
        if self.step != SweepStep::PickPath || handle.is_null() || self.contains_profile(handle) {
            return Vec::new();
        }
        let key = (handle, self.mode, self.options);
        if self.preview_key == Some(key) {
            return self.preview_cache.clone();
        }
        self.preview_cache.clear();
        let Some((hovered, path)) = self.hover_path.as_ref() else {
            return Vec::new();
        };
        if *hovered != handle {
            return Vec::new();
        }
        self.preview_key = Some(key);
        if !crate::scene::model::sweep_model::is_sweep_path(path) {
            return Vec::new();
        }
        let Some(options) = self.selection_options() else {
            return Vec::new();
        };
        self.preview_cache = self.profiles.iter().flat_map(|(_, profile)| {
            crate::scene::model::sweep_model::swept_with_options(profile, path, self.mode, options)
                .map(|body| preview_body_wires(&body, self.color, self.isolines))
                .unwrap_or_default()
        }).collect();
        self.preview_cache.clone()
    }
}

// ── LOFT command ───────────────────────────────────────────────────────────

pub struct LoftCommand {
    state: LoftState,
    undo: Vec<LoftState>,
    available: Vec<(Handle, EntityType)>,
    injected_entity: Option<EntityType>,
    entity_revision: u64,
    preview_key: Option<LoftPreviewKey>,
    preview_cache: Vec<WireModel>,
    preview_error: Option<String>,
    notice: Option<String>,
    isolines: usize,
    color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoftStep {
    Sections,
    Join,
    Point,
    Mode,
    Options,
    Guides,
    Path,
    Settings,
    Normals,
    StartDraft,
    EndDraft,
    StartMagnitude,
    EndMagnitude,
    StartContinuity,
    EndContinuity,
    StartBulge,
    EndBulge,
    Closed,
    AlignDirection,
}

#[derive(Clone)]
struct LoftState {
    step: LoftStep,
    return_step: LoftStep,
    sections: Vec<LoftSectionSelection>,
    join: Vec<Handle>,
    guides: Vec<Handle>,
    path: Option<Handle>,
    mode: ExtrudeMode,
    options: LoftOptions,
}

#[derive(Clone, PartialEq)]
struct LoftPreviewKey {
    sections: Vec<LoftSectionSelection>,
    guides: Vec<Handle>,
    path: Option<Handle>,
    mode: ExtrudeMode,
    options: LoftOptions,
    entity_revision: u64,
}

impl LoftCommand {
    pub fn new(
        color: [f32; 4],
        isolines: usize,
        mode: ExtrudeMode,
        seed: Vec<(Handle, EntityType)>,
        available: Vec<(Handle, EntityType)>,
    ) -> Self {
        let mut command = Self {
            state: LoftState {
                step: LoftStep::Sections,
                return_step: LoftStep::Sections,
                sections: Vec::new(),
                join: Vec::new(),
                guides: Vec::new(),
                path: None,
                mode,
                options: LoftOptions::default(),
            },
            undo: Vec::new(),
            available,
            injected_entity: None,
            entity_revision: 0,
            preview_key: None,
            preview_cache: Vec::new(),
            preview_error: None,
            notice: None,
            isolines,
            color,
        };
        // Preserve supplied selection order; a loft is not an unordered set.
        for (handle, entity) in seed {
            if !handle.is_null()
                && !command.contains_section(handle)
                && crate::scene::model::loft_command_model::is_section(&entity)
            {
                command.store_entity(handle, entity);
                command.state.sections.push(LoftSectionSelection::Entity(handle));
            }
        }
        let invalid_point = command.state.sections.iter().enumerate().find_map(|(index, section)| {
            (command.is_point_section(section)
                && ((index > 0 && index + 1 < command.state.sections.len())
                    || (index > 0 && command.is_point_section(&command.state.sections[index - 1]))))
                .then_some(index)
        });
        if let Some(index) = invalid_point {
            command.state.sections.truncate(index);
            command.notice = Some(t!("Point cross-sections are allowed only at the first or last end, with a curve between point ends.").into_owned());
        } else if command.state.sections.len() >= 2 {
            command.state.step = LoftStep::Options;
        }
        command
    }

    fn store_entity(&mut self, handle: Handle, entity: EntityType) {
        if let Some((_, stored)) = self.available.iter_mut().find(|(id, _)| *id == handle) {
            *stored = entity;
        } else {
            self.available.push((handle, entity));
        }
        self.entity_revision = self.entity_revision.wrapping_add(1);
    }

    fn contains_section(&self, handle: Handle) -> bool {
        self.state.sections.iter().any(|section| match section {
            LoftSectionSelection::Entity(id) => *id == handle,
            LoftSectionSelection::Join(handles) => handles.contains(&handle),
            LoftSectionSelection::Point(_) => false,
        })
    }

    fn is_point_section(&self, section: &LoftSectionSelection) -> bool {
        match section {
            LoftSectionSelection::Point(_) => true,
            LoftSectionSelection::Entity(handle) => self.available.iter().any(|(id, entity)| {
                id == handle && matches!(entity, EntityType::Point(_))
            }),
            LoftSectionSelection::Join(_) => false,
        }
    }

    fn point_ends(&self) -> (bool, bool) {
        (
            self.state.sections.first().is_some_and(|section| self.is_point_section(section)),
            self.state.sections.last().is_some_and(|section| self.is_point_section(section)),
        )
    }

    fn point_setting_next(&self, step: LoftStep) -> LoftStep {
        match step {
            LoftStep::StartContinuity if self.point_ends().1 => LoftStep::EndContinuity,
            LoftStep::StartBulge if self.point_ends().1 => LoftStep::EndBulge,
            _ => LoftStep::Options,
        }
    }

    fn remember(&mut self) {
        self.undo.push(self.state.clone());
        self.notice = None;
    }

    fn enter_step(&mut self, step: LoftStep) {
        self.remember();
        self.state.step = step;
    }

    fn invalid(&mut self, message: String) -> CmdResult {
        self.notice = Some(message);
        CmdResult::NeedPoint
    }

    fn can_close(&self) -> bool {
        self.state.sections.len() >= 3
            && !self.state.sections.iter().any(|section| self.is_point_section(section))
    }

    fn key(&self) -> LoftPreviewKey {
        LoftPreviewKey {
            sections: self.state.sections.clone(),
            guides: self.state.guides.clone(),
            path: self.state.path,
            mode: self.state.mode,
            options: self.state.options,
            entity_revision: self.entity_revision,
        }
    }

    fn cached_preview(&mut self, key: LoftPreviewKey) -> Vec<WireModel> {
        if self.preview_key.as_ref() == Some(&key) {
            return self.preview_cache.clone();
        }
        self.preview_cache.clear();
        self.preview_error = None;
        if key.sections.len() >= 2 {
            match crate::scene::model::loft_command_model::build_body(
                &key.sections,
                &key.guides,
                key.path,
                &self.available,
                key.mode,
                key.options,
            ) {
                Ok(body) => {
                    // Only edges/isolines: do not triangulate faces on mouse movement.
                    self.preview_cache = preview_body_wires(&body, self.color, self.isolines);
                }
                Err(error) => self.preview_error = Some(error),
            }
        }
        self.preview_key = Some(key);
        self.preview_cache.clone()
    }

    fn finish(&mut self) -> CmdResult {
        if self.state.sections.len() < 2 {
            return self.invalid(t!("LOFT requires at least two cross-sections.").into_owned());
        }
        if self.state.sections.iter().enumerate().any(|(index, section)| {
            self.is_point_section(section)
                && ((index > 0 && index + 1 < self.state.sections.len())
                    || (index > 0 && self.is_point_section(&self.state.sections[index - 1])))
        }) {
            return self.invalid(t!("Point cross-sections are allowed only at the first or last end, with a curve between point ends.").into_owned());
        }
        if self.state.options.closed && !self.can_close() {
            return self.invalid(t!("A closed loft requires at least three curve cross-sections and no point ends.").into_owned());
        }
        let key = self.key();
        self.cached_preview(key);
        if let Some(error) = self.preview_error.clone() {
            return self.invalid(error);
        }
        CmdResult::LoftEntities {
            sections: self.state.sections.clone(),
            guides: self.state.guides.clone(),
            path: self.state.path,
            mode: self.state.mode,
            options: self.state.options,
            color: self.color,
        }
    }

    fn undo_selection(&mut self) -> CmdResult {
        if let Some(state) = self.undo.pop() {
            self.state = state;
        } else if !self.state.sections.is_empty() {
            self.state.sections.pop();
            self.state.step = LoftStep::Sections;
            self.state.options.closed = false;
        }
        self.notice = None;
        CmdResult::NeedPoint
    }

    fn add_section(&mut self, handle: Handle) -> CmdResult {
        if self.state.sections.len() >= 2 && self.point_ends().1 {
            return self.invalid(t!("A point must remain the last cross-section; use Undo to change the end.").into_owned());
        }
        if self.contains_section(handle) {
            return self.invalid(t!("That cross-section is already selected.").into_owned());
        }
        if self.available.iter().find(|(id, _)| *id == handle)
            .is_none_or(|(_, entity)| !crate::scene::model::loft_command_model::is_section(entity))
        {
            return self.invalid(t!("Select a supported planar curve or region cross-section.").into_owned());
        }
        let section = LoftSectionSelection::Entity(handle);
        let point = self.is_point_section(&section);
        if point && self.point_ends().1 {
            return self.invalid(t!("Add a curve cross-section between point ends.").into_owned());
        }
        self.remember();
        self.state.sections.push(section);
        if point {
            self.state.options.closed = false;
            if self.state.sections.len() >= 2 {
                self.state.step = LoftStep::Options;
            }
        }
        CmdResult::NeedPoint
    }

    fn set_numeric_setting(&mut self, text: &str) -> bool {
        let angle = matches!(self.state.step, LoftStep::StartDraft | LoftStep::EndDraft);
        let bulge = matches!(self.state.step, LoftStep::StartBulge | LoftStep::EndBulge);
        let number = if angle {
            crate::entities::common::parse_angle(text)
        } else {
            text.trim().replace(',', ".").parse::<f64>().ok()
        };
        let Some(number) = number.filter(|number| number.is_finite()) else {
            return false;
        };
        if (angle && !(0.0..=std::f64::consts::PI).contains(&number))
            || (!angle && number < 0.0)
            || (bulge && number < 0.0)
        {
            return false;
        }
        self.remember();
        self.state.step = match self.state.step {
            LoftStep::StartDraft => {
                self.state.options.start_draft_angle = number;
                LoftStep::EndDraft
            }
            LoftStep::EndDraft => {
                self.state.options.end_draft_angle = number;
                LoftStep::Settings
            }
            LoftStep::StartMagnitude => {
                self.state.options.start_magnitude = number;
                LoftStep::EndMagnitude
            }
            LoftStep::EndMagnitude => {
                self.state.options.end_magnitude = number;
                LoftStep::Settings
            }
            LoftStep::StartBulge => {
                self.state.options.start_bulge = number;
                self.point_setting_next(LoftStep::StartBulge)
            }
            LoftStep::EndBulge => {
                self.state.options.end_bulge = number;
                LoftStep::Options
            }
            _ => return false,
        };
        true
    }

    fn keyword(text: &str, choices: &[&str]) -> Option<usize> {
        let keyword = text.trim().trim_start_matches('_').replace([' ', '-'], "").to_ascii_uppercase();
        let keyword = match keyword.as_str() {
            "CROSSSECTIONSONLY" => "CROSSSECTIONS",
            "BULGEMAGNITUDE" => "BULGE",
            _ => keyword.as_str(),
        };
        if keyword.is_empty() {
            return None;
        }
        if let Some(index) = choices.iter().position(|choice| *choice == keyword) {
            return Some(index);
        }
        let mut matches = choices.iter().enumerate().filter(|(_, choice)| choice.starts_with(keyword));
        let index = matches.next()?.0;
        matches.next().is_none().then_some(index)
    }

    fn option_prompt(&self) -> String {
        match self.state.step {
            LoftStep::Sections => format!(
                "{} ({}):", t!("LOFT  Select cross-sections in order or [Point/Join/Mode/Undo] (Enter to finish)"),
                self.state.sections.len(),
            ),
            LoftStep::Join => format!(
                "{} ({}):", t!("LOFT  Select connected edges for one cross-section or [Undo] (Enter to finish)"),
                self.state.join.len(),
            ),
            LoftStep::Point => t!("LOFT  Specify point end:").into_owned(),
            LoftStep::Mode => format!("{} <{}>:", t!("LOFT  Creation mode [Solid/Surface]"),
                if self.state.mode == ExtrudeMode::Solid { "Solid" } else { "Surface" }),
            LoftStep::Options => {
                let (start, end) = self.point_ends();
                if start || end {
                    t!("LOFT  Enter an option [Guides/Path/Cross sections only/Settings/Continuity/Bulge magnitude/Mode/Undo] <Cross sections only>:").into_owned()
                } else {
                    t!("LOFT  Enter an option [Guides/Path/Cross sections only/Settings/Mode/Undo] <Cross sections only>:").into_owned()
                }
            }
            LoftStep::Guides => format!("{} ({}):", t!("LOFT  Select guide curves or [Undo] (Enter to finish)"), self.state.guides.len()),
            LoftStep::Path => t!("LOFT  Select path or [Undo]:").into_owned(),
            LoftStep::Settings => {
                let mut choices = vec![t!("Normals").into_owned()];
                if self.state.options.normals == 6 {
                    choices.push(t!("Draft angles").into_owned());
                    choices.push(t!("Magnitudes").into_owned());
                }
                if self.can_close() {
                    choices.push(t!("Closed").into_owned());
                }
                choices.push(t!("Align direction").into_owned());
                choices.push(t!("Done").into_owned());
                format!("{} [{}]:", t!("LOFT  Settings"), choices.join("/"))
            }
            LoftStep::Normals => t!("LOFT  Surface normals [Ruled/Smooth/First normal/Last normal/Ends normal/All normal/Use draft angles]:").into_owned(),
            LoftStep::StartDraft => format!("{} <{}>:", t!("LOFT  Start draft angle"), crate::entities::common::format_angle(self.state.options.start_draft_angle)),
            LoftStep::EndDraft => format!("{} <{}>:", t!("LOFT  End draft angle"), crate::entities::common::format_angle(self.state.options.end_draft_angle)),
            LoftStep::StartMagnitude => format!("{} <{}>:", t!("LOFT  Start magnitude"), self.state.options.start_magnitude),
            LoftStep::EndMagnitude => format!("{} <{}>:", t!("LOFT  End magnitude"), self.state.options.end_magnitude),
            LoftStep::StartContinuity => format!("{} <G{}>:", t!("LOFT  Start point continuity [G0/G1]"), self.state.options.start_continuity),
            LoftStep::EndContinuity => format!("{} <G{}>:", t!("LOFT  End point continuity [G0/G1]"), self.state.options.end_continuity),
            LoftStep::StartBulge => format!("{} <{:.4}>:", t!("LOFT  Start point bulge magnitude"), self.state.options.start_bulge),
            LoftStep::EndBulge => format!("{} <{:.4}>:", t!("LOFT  End point bulge magnitude"), self.state.options.end_bulge),
            LoftStep::Closed => format!("{} <{}>:", t!("LOFT  Closed [Yes/No]"), if self.state.options.closed { "Yes" } else { "No" }),
            LoftStep::AlignDirection => format!("{} <{}>:", t!("LOFT  Align cross-section directions [Yes/No]"), if self.state.options.align_direction { "Yes" } else { "No" }),
        }
    }
}

impl CadCommand for LoftCommand {
    fn name(&self) -> &'static str {
        "LOFT"
    }
    fn prompt(&self) -> String {
        let prompt = self.option_prompt();
        match &self.notice {
            Some(notice) => format!("{notice}\n{prompt}"),
            None => prompt,
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        let mut options = match self.state.step {
            LoftStep::Sections => vec![CmdOption::new("Point", "POINT"), CmdOption::new("Join", "JOIN"), CmdOption::new("Mode", "MODE"), CmdOption::enter("Done")],
            LoftStep::Join | LoftStep::Guides => vec![CmdOption::enter("Done")],
            LoftStep::Mode => vec![CmdOption::new("Solid", "SOLID"), CmdOption::new("Surface", "SURFACE")],
            LoftStep::Options => {
                let mut choices = vec![CmdOption::new("Guides", "GUIDES"), CmdOption::new("Path", "PATH"), CmdOption::new("Cross sections only", "CROSSSECTIONS"), CmdOption::new("Settings", "SETTINGS")];
                let (start, end) = self.point_ends();
                if start || end {
                    choices.push(CmdOption::new("Continuity", "CONTINUITY"));
                    choices.push(CmdOption::new("Bulge magnitude", "BULGE"));
                }
                choices.push(CmdOption::new("Mode", "MODE"));
                choices
            }
            LoftStep::Settings => {
                let mut choices = vec![CmdOption::new("Normals", "NORMALS")];
                if self.state.options.normals == 6 {
                    choices.push(CmdOption::new("Draft angles", "DRAFTANGLES"));
                    choices.push(CmdOption::new("Magnitudes", "MAGNITUDES"));
                }
                if self.can_close() {
                    choices.push(CmdOption::new("Closed", "CLOSED"));
                }
                choices.push(CmdOption::new("Align direction", "ALIGNDIRECTION"));
                choices.push(CmdOption::enter("Done"));
                choices
            }
            LoftStep::Normals => vec![
                CmdOption::new("Ruled", "RULED"), CmdOption::new("Smooth", "SMOOTH"),
                CmdOption::new("First normal", "FIRSTNORMAL"), CmdOption::new("Last normal", "LASTNORMAL"),
                CmdOption::new("Ends normal", "ENDSNORMAL"), CmdOption::new("All normal", "ALLNORMAL"),
                CmdOption::new("Use draft angles", "USEDRAFTANGLES"),
            ],
            LoftStep::Closed | LoftStep::AlignDirection => vec![CmdOption::new("Yes", "YES"), CmdOption::new("No", "NO")],
            LoftStep::StartContinuity | LoftStep::EndContinuity => vec![CmdOption::new("G0", "G0"), CmdOption::new("G1", "G1")],
            _ => Vec::new(),
        };
        options.push(CmdOption::new("Undo", "UNDO"));
        options
    }
    fn needs_entity_pick(&self) -> bool {
        matches!(self.state.step, LoftStep::Sections | LoftStep::Join | LoftStep::Guides | LoftStep::Path)
    }
    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }
    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.injected_entity = Some(entity);
    }
    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !self.needs_entity_pick() || handle.is_null() {
            self.injected_entity = None;
            return CmdResult::NeedPoint;
        }
        if let Some(entity) = self.injected_entity.take() {
            self.store_entity(handle, entity);
        }
        if self.state.step == LoftStep::Sections {
            return self.add_section(handle);
        }
        if self.contains_section(handle) {
            return self.invalid(t!("A cross-section cannot also be a guide, path, or joined edge.").into_owned());
        }
        if self.available.iter().find(|(id, _)| *id == handle)
            .is_none_or(|(_, entity)| !crate::scene::model::loft_command_model::is_guide_or_path(entity))
        {
            return self.invalid(t!("Select a supported guide or path curve.").into_owned());
        }
        match self.state.step {
            LoftStep::Join => {
                if self.state.join.contains(&handle) {
                    return self.invalid(t!("That edge is already selected.").into_owned());
                }
                self.remember();
                self.state.join.push(handle);
            }
            LoftStep::Guides => {
                if self.state.guides.contains(&handle) {
                    return self.invalid(t!("That guide is already selected.").into_owned());
                }
                self.remember();
                self.state.guides.push(handle);
            }
            LoftStep::Path => {
                self.remember();
                self.state.path = Some(handle);
                self.state.guides.clear();
                return self.finish();
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }
    fn wants_text_input(&self) -> bool {
        true
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.state.step == LoftStep::Point
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.state.step != LoftStep::Point || !point.is_finite() {
            return CmdResult::NeedPoint;
        }
        if self.point_ends().1 {
            return self.invalid(t!("Add a curve cross-section between point ends.").into_owned());
        }
        self.remember();
        self.state.sections.push(LoftSectionSelection::Point(point));
        self.state.options.closed = false;
        self.state.step = if self.state.sections.len() >= 2 { LoftStep::Options } else { LoftStep::Sections };
        CmdResult::NeedPoint
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if text.trim().is_empty() {
            return Some(self.on_enter());
        }
        if Self::keyword(text, &["UNDO"]).is_some() {
            return Some(self.undo_selection());
        }
        // Leave hexadecimal object identifiers to the shared pick-input router.
        // Treating them as unknown options would consume scripted section,
        // guide and path selections before on_entity_pick can receive them.
        if self.needs_entity_pick()
            && u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).is_ok()
        {
            return None;
        }
        match self.state.step {
            LoftStep::Sections => match Self::keyword(text, &["POINT", "JOIN", "MODE"]) {
                Some(0) => {
                    if self.point_ends().1 {
                        return Some(self.invalid(t!("Select a curve cross-section before another point end.").into_owned()));
                    }
                    self.enter_step(LoftStep::Point);
                }
                Some(1) => {
                    if self.state.sections.len() >= 2 && self.point_ends().1 {
                        return Some(self.invalid(t!("A point must remain the last cross-section; use Undo to change the end.").into_owned()));
                    }
                    self.enter_step(LoftStep::Join);
                    self.state.join.clear();
                }
                Some(2) => {
                    self.enter_step(LoftStep::Mode);
                    self.state.return_step = LoftStep::Sections;
                }
                _ => return Some(self.invalid(t!("Select a cross-section or enter Point, Join, Mode, or Undo.").into_owned())),
            },
            LoftStep::Mode => match Self::keyword(text, &["SOLID", "SURFACE"]) {
                Some(mode) => {
                    self.remember();
                    self.state.mode = if mode == 0 { ExtrudeMode::Solid } else { ExtrudeMode::Surface };
                    self.state.step = self.state.return_step;
                }
                _ => return Some(self.invalid(t!("Enter Solid or Surface.").into_owned())),
            },
            LoftStep::Options => match Self::keyword(text, &["GUIDES", "PATH", "CROSSSECTIONS", "SETTINGS", "MODE", "CONTINUITY", "BULGE"]) {
                Some(0) => {
                    self.enter_step(LoftStep::Guides);
                    self.state.path = None;
                }
                Some(1) => {
                    self.enter_step(LoftStep::Path);
                    self.state.guides.clear();
                }
                Some(2) => {
                    self.remember();
                    self.state.guides.clear();
                    self.state.path = None;
                    return Some(self.finish());
                }
                Some(3) => self.enter_step(LoftStep::Settings),
                Some(4) => {
                    self.enter_step(LoftStep::Mode);
                    self.state.return_step = LoftStep::Options;
                }
                Some(5) | Some(6) if !self.point_ends().0 && !self.point_ends().1 => {
                    return Some(self.invalid(t!("Continuity and bulge magnitude require a point cross-section at an end.").into_owned()));
                }
                Some(5) => self.enter_step(if self.point_ends().0 { LoftStep::StartContinuity } else { LoftStep::EndContinuity }),
                Some(6) => self.enter_step(if self.point_ends().0 { LoftStep::StartBulge } else { LoftStep::EndBulge }),
                _ => return Some(self.invalid(t!("Enter one of the listed loft options.").into_owned())),
            },
            LoftStep::Settings => match Self::keyword(text, &["NORMALS", "DRAFTANGLES", "MAGNITUDES", "CLOSED", "ALIGNDIRECTION", "DONE"]) {
                Some(0) => self.enter_step(LoftStep::Normals),
                Some(1) | Some(2) if self.state.options.normals != 6 => {
                    return Some(self.invalid(t!("Choose Use draft angles under Normals before editing draft angles or magnitudes.").into_owned()));
                }
                Some(1) => self.enter_step(LoftStep::StartDraft),
                Some(2) => self.enter_step(LoftStep::StartMagnitude),
                Some(3) if self.can_close() => self.enter_step(LoftStep::Closed),
                Some(3) => return Some(self.invalid(t!("Closed is available for at least three curve cross-sections without point ends.").into_owned())),
                Some(4) => self.enter_step(LoftStep::AlignDirection),
                Some(5) => self.enter_step(LoftStep::Options),
                _ => return Some(self.invalid(t!("Enter a listed loft setting.").into_owned())),
            },
            LoftStep::Normals => {
                let selected = Self::keyword(text, &["RULED", "SMOOTH", "FIRSTNORMAL", "LASTNORMAL", "ENDSNORMAL", "ALLNORMAL", "USEDRAFTANGLES"])
                    .or_else(|| text.trim().parse::<usize>().ok().filter(|value| *value <= 6));
                let Some(selected) = selected else {
                    return Some(self.invalid(t!("Select one of the seven surface normal options.").into_owned()));
                };
                self.remember();
                self.state.options.normals = selected as i32;
                self.state.step = LoftStep::Settings;
            }
            LoftStep::StartDraft | LoftStep::EndDraft | LoftStep::StartMagnitude | LoftStep::EndMagnitude => {
                if !self.set_numeric_setting(text) {
                    return Some(self.invalid(t!("Enter a finite draft angle from 0 to 180 degrees or a non-negative magnitude.").into_owned()));
                }
            }
            LoftStep::StartContinuity | LoftStep::EndContinuity => {
                let continuity = match text.trim().trim_start_matches('_').to_ascii_uppercase().as_str() {
                    "G0" => 0,
                    "G1" => 1,
                    _ => return Some(self.invalid(t!("Enter G0 or G1.").into_owned())),
                };
                let step = self.state.step;
                self.remember();
                if step == LoftStep::StartContinuity {
                    self.state.options.start_continuity = continuity;
                } else {
                    self.state.options.end_continuity = continuity;
                }
                self.state.step = self.point_setting_next(step);
            }
            LoftStep::StartBulge | LoftStep::EndBulge => {
                if !self.set_numeric_setting(text) {
                    return Some(self.invalid(t!("Enter a finite, non-negative bulge magnitude.").into_owned()));
                }
            }
            LoftStep::Closed | LoftStep::AlignDirection => {
                let Some(selected) = Self::keyword(text, &["YES", "NO"]) else {
                    return Some(self.invalid(t!("Enter Yes or No.").into_owned()));
                };
                if self.state.step == LoftStep::Closed && selected == 0 && !self.can_close() {
                    return Some(self.invalid(t!("A closed loft requires at least three curve cross-sections and no point ends.").into_owned()));
                }
                self.remember();
                if self.state.step == LoftStep::Closed {
                    self.state.options.closed = selected == 0;
                } else {
                    self.state.options.align_direction = selected == 0;
                }
                self.state.step = LoftStep::Settings;
            }
            LoftStep::Point => return None,
            LoftStep::Join | LoftStep::Guides | LoftStep::Path => {
                return Some(self.invalid(t!("Select a curve or use Undo.").into_owned()));
            }
        }
        Some(CmdResult::NeedPoint)
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.state.step {
            LoftStep::Sections if self.state.sections.len() >= 2 => self.enter_step(LoftStep::Options),
            LoftStep::Sections => return self.invalid(t!("LOFT requires at least two cross-sections; select another section or a point end.").into_owned()),
            LoftStep::Join if !self.state.join.is_empty() => {
                self.remember();
                self.state.sections.push(LoftSectionSelection::Join(std::mem::take(&mut self.state.join)));
                self.state.step = LoftStep::Sections;
            }
            LoftStep::Join => return self.invalid(t!("Select connected edges for the cross-section.").into_owned()),
            LoftStep::Options => return self.finish(),
            LoftStep::Guides if !self.state.guides.is_empty() => return self.finish(),
            LoftStep::Guides => return self.invalid(t!("Select at least one guide curve.").into_owned()),
            LoftStep::Mode => self.enter_step(self.state.return_step),
            LoftStep::Settings => self.enter_step(LoftStep::Options),
            LoftStep::Normals | LoftStep::Closed | LoftStep::AlignDirection | LoftStep::EndDraft | LoftStep::EndMagnitude => self.enter_step(LoftStep::Settings),
            LoftStep::StartDraft => self.enter_step(LoftStep::EndDraft),
            LoftStep::StartMagnitude => self.enter_step(LoftStep::EndMagnitude),
            LoftStep::StartContinuity | LoftStep::EndContinuity | LoftStep::StartBulge | LoftStep::EndBulge => self.enter_step(self.point_setting_next(self.state.step)),
            LoftStep::Path | LoftStep::Point => return self.invalid(t!("Specify the requested path or point, or use Undo.").into_owned()),
        }
        CmdResult::NeedPoint
    }
    fn on_undo_step(&mut self) -> Option<CmdResult> {
        Some(self.undo_selection())
    }
    fn wants_hover_entity(&self, handle: Handle) -> bool {
        self.needs_entity_pick() && !handle.is_null()
            && !self.available.iter().any(|(id, _)| *id == handle)
    }
    fn inject_hover_entity(&mut self, handle: Handle, entity: EntityType) {
        self.store_entity(handle, entity);
    }
    fn on_hover_entity(&mut self, handle: Handle, _point: DVec3) -> Vec<WireModel> {
        let mut key = self.key();
        if !handle.is_null() && !self.contains_section(handle) {
            if let Some((_, entity)) = self.available.iter().find(|(id, _)| *id == handle) {
                match self.state.step {
                    LoftStep::Sections if crate::scene::model::loft_command_model::is_section(entity) => {
                        let section = LoftSectionSelection::Entity(handle);
                        let point = self.is_point_section(&section);
                        if !(self.point_ends().1 && (point || self.state.sections.len() >= 2)) {
                            key.sections.push(section);
                            if point {
                                key.options.closed = false;
                            }
                        }
                    }
                    LoftStep::Guides if !key.guides.contains(&handle)
                        && crate::scene::model::loft_command_model::is_guide_or_path(entity) => key.guides.push(handle),
                    LoftStep::Path if crate::scene::model::loft_command_model::is_guide_or_path(entity) => {
                        key.path = Some(handle);
                        key.guides.clear();
                    }
                    _ => {}
                }
            }
        }
        self.cached_preview(key)
    }
    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let mut key = self.key();
        if self.state.step == LoftStep::Point && point.is_finite()
            && !key.sections.is_empty()
            && !self.point_ends().1
        {
            key.sections.push(LoftSectionSelection::Point(point));
            key.options.closed = false;
        }
        self.cached_preview(key)
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

#[cfg(test)]
mod revolve_tests {
    use super::*;

    #[test]
    fn cursor_angle_is_measured_about_the_model_axis() {
        let profile = EntityType::Line(acadrust::entities::Line::from_coords(
            2.0, 0.0, 0.0, 2.0, 4.0, 0.0,
        ));
        let mut command = RevolveCommand::new([1.0; 4], 0);
        command.set_preselection(vec![(Handle::new(1), profile)]);
        command.axis_start = DVec3::ZERO;
        command.axis_end = DVec3::Y;
        command.step = RevolveStep::Angle;

        let anchor = command.angle_anchor().unwrap();
        let quarter = anchor - DVec3::Z * 2.0;
        let angle = command.cursor_angle(quarter, false).unwrap();
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

        let displaced_along_axis = quarter + DVec3::Y * 100.0;
        let angle = command.cursor_angle(displaced_along_axis, false).unwrap();
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

        let (normal, origin) = command.cursor_plane().unwrap();
        assert!((normal - DVec3::Y).length_squared() < 1e-24);
        assert!((origin - anchor).length_squared() < 1e-24);
    }
}
inventory::submit!(crate::command::CommandRegistration { names: &["SWEEP"] });
