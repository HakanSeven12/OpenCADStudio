//! Quick dimension creation from a selected set of drawing entities.

use acadrust::entities::{Dimension, DimensionDiameter, DimensionLinear, DimensionOrdinate, DimensionRadius};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::{characteristic_points, intersect, Curve, SnapKind, Tolerance};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, DimensionAssociationSource,
    EntityTransform, SelectionEntity, WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/qdim.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "QDIM",
        label: "Quick Dim",
        icon: ICON,
        event: ModuleEvent::Command("QDIM".to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step { Gathering, Place, Datum, Edit, Settings }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Continuous, Staggered, Baseline, Ordinate, Radius, Diameter }

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapPriority { Endpoints, Intersections }

#[derive(Clone)]
struct SourceCurve { handle: Handle, curve: Curve }

#[derive(Clone)]
struct Candidate { point: DVec3, sources: Vec<DimensionAssociationSource> }

pub struct QdimCommand {
    step: Step,
    mode: Mode,
    snap_priority: SnapPriority,
    plane: WorkingPlane,
    selection: Vec<SelectionEntity>,
    datum: Option<DVec3>,
    excluded: Vec<DVec3>,
    dim_spacing: f64,
}

impl QdimCommand {
    pub fn new(selection: Vec<SelectionEntity>, dim_spacing: f64, snap_priority: u8) -> Self {
        let step = if selection.is_empty() { Step::Gathering } else { Step::Place };
        Self {
            step,
            mode: Mode::Continuous,
            snap_priority: if snap_priority == 1 { SnapPriority::Intersections } else { SnapPriority::Endpoints },
            plane: WorkingPlane::default(),
            selection,
            datum: None,
            excluded: Vec::new(),
            dim_spacing: dim_spacing.abs().max(1e-9),
        }
    }

    fn local_sources(&self) -> Vec<SourceCurve> {
        let transform = EntityTransform::Affine(self.plane.to_local_transform());
        self.selection.iter().filter_map(|selected| {
            let mut entity = selected.entity.clone();
            crate::scene::view::dispatch::apply_transform(&mut entity, &transform);
            crate::entities::curve::entity_curve_xy(&entity)
                .map(|curve| SourceCurve { handle: selected.handle, curve })
        }).collect()
    }

    fn candidates(&self) -> Vec<Candidate> {
        let curves = self.local_sources();
        let mut candidates = if self.snap_priority == SnapPriority::Intersections {
            let mut hits = Vec::new();
            for first in 0..curves.len() {
                for second in first + 1..curves.len() {
                    for crossing in intersect(&curves[first].curve, &curves[second].curve, Tolerance::default()) {
                        hits.push(Candidate {
                            point: DVec3::new(crossing.point[0], crossing.point[1], 0.0),
                            // An intersection depends on two source curves. The
                            // current association record stores one source per
                            // extension origin, so do not bind it to an
                            // unrelated endpoint marker.
                            sources: Vec::new(),
                        });
                    }
                }
            }
            hits
        } else { Vec::new() };

        if candidates.is_empty() {
            for source in &curves {
                for point in characteristic_points(&source.curve).into_iter().filter(|point| point.kind == SnapKind::Endpoint) {
                    candidates.push(Candidate {
                        point: DVec3::new(point.point[0], point.point[1], 0.0),
                        sources: vec![DimensionAssociationSource::inferred(source.handle)],
                    });
                }
            }
        }

        for selected in &self.selection {
            if let EntityType::Dimension(dimension) = &selected.entity {
                for point in dimension_reference_points(dimension) {
                    candidates.push(Candidate {
                        point: self.plane.to_local(point),
                        sources: vec![DimensionAssociationSource::inferred(selected.handle)],
                    });
                }
            }
        }

        let tolerance = candidate_tolerance(candidates.iter().map(|candidate| candidate.point));
        let mut unique: Vec<Candidate> = Vec::new();
        for candidate in candidates {
            if let Some(existing) = unique.iter_mut().find(|existing| existing.point.distance(candidate.point) <= tolerance) {
                for source in candidate.sources {
                    if !existing.sources.iter().any(|current| current.handle == source.handle) {
                        existing.sources.push(source);
                    }
                }
            } else {
                unique.push(candidate);
            }
        }
        unique.retain(|candidate| !self.excluded.iter().any(|point| point.distance(candidate.point) <= tolerance));
        unique
    }

    fn radial_sources(&self) -> Vec<(Handle, Curve)> {
        self.local_sources().into_iter()
            .filter(|source| matches!(source.curve, Curve::Circle(_) | Curve::Arc(_)))
            .map(|source| (source.handle, source.curve)).collect()
    }

    fn orientation(&self, points: &[Candidate], place: DVec3) -> bool {
        let min_x = points.iter().map(|point| point.point.x).fold(f64::INFINITY, f64::min);
        let max_x = points.iter().map(|point| point.point.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = points.iter().map(|point| point.point.y).fold(f64::INFINITY, f64::min);
        let max_y = points.iter().map(|point| point.point.y).fold(f64::NEG_INFINITY, f64::max);
        let outside_y = (place.y - max_y).max(min_y - place.y).max(0.0);
        let outside_x = (place.x - max_x).max(min_x - place.x).max(0.0);
        if (outside_x - outside_y).abs() > 1e-9 { outside_y >= outside_x } else { max_x - min_x >= max_y - min_y }
    }

    fn build_dimensions(&self, place_world: DVec3) -> Vec<(EntityType, DimensionAssociationInput)> {
        let place = self.plane.to_local(place_world);
        if matches!(self.mode, Mode::Radius | Mode::Diameter) {
            return self.build_radial_dimensions(place);
        }
        let mut points = self.candidates();
        if points.is_empty() { return Vec::new(); }
        let horizontal = self.orientation(&points, place);
        points.sort_by(|first, second| if horizontal { first.point.x.total_cmp(&second.point.x) } else { first.point.y.total_cmp(&second.point.y) });
        let tolerance = candidate_tolerance(points.iter().map(|point| point.point));
        points.dedup_by(|first, second| {
            let delta = if horizontal { first.point.x - second.point.x } else { first.point.y - second.point.y };
            delta.abs() <= tolerance
        });

        match self.mode {
            Mode::Continuous | Mode::Staggered => points.windows(2).enumerate().map(|(index, pair)| {
                let offset = if self.mode == Mode::Staggered { index as f64 * self.dim_spacing } else { 0.0 };
                let direction = if horizontal {
                    if place.y >= (pair[0].point.y + pair[1].point.y) * 0.5 { 1.0 } else { -1.0 }
                } else if place.x >= (pair[0].point.x + pair[1].point.x) * 0.5 { 1.0 } else { -1.0 };
                let shifted = if horizontal { DVec3::new(place.x, place.y + direction * offset, 0.0) } else { DVec3::new(place.x + direction * offset, place.y, 0.0) };
                self.linear_dimension(&pair[0], &pair[1], shifted, horizontal)
            }).collect(),
            Mode::Baseline => {
                let datum = self.datum.unwrap_or(points[0].point);
                let datum_candidate = Candidate {
                    point: datum,
                    sources: points.iter().min_by(|first, second| first.point.distance(datum).total_cmp(&second.point.distance(datum)))
                        .map(|point| point.sources.clone()).unwrap_or_default(),
                };
                points.iter().filter(|point| point.point.distance(datum) > tolerance)
                    .map(|point| self.linear_dimension(&datum_candidate, point, place, horizontal)).collect()
            }
            Mode::Ordinate => points.iter().map(|point| self.ordinate_dimension(point, place, horizontal)).collect(),
            Mode::Radius | Mode::Diameter => Vec::new(),
        }
    }

    fn linear_dimension(&self, first: &Candidate, second: &Candidate, place: DVec3, horizontal: bool) -> (EntityType, DimensionAssociationInput) {
        let mut dimension = DimensionLinear::new(v3(first.point), v3(second.point));
        dimension.rotation = if horizontal { 0.0 } else { std::f64::consts::FRAC_PI_2 };
        let definition = if horizontal { DVec3::new(second.point.x, place.y, 0.0) } else { DVec3::new(place.x, second.point.y, 0.0) };
        let text = if horizontal { DVec3::new((first.point.x + second.point.x) * 0.5, place.y, 0.0) } else { DVec3::new(place.x, (first.point.y + second.point.y) * 0.5, 0.0) };
        dimension.definition_point = v3(definition);
        dimension.base.definition_point = v3(definition);
        dimension.base.text_middle_point = v3(text);
        dimension.base.insertion_point = v3(text);
        dimension.base.actual_measurement = dimension.measurement();
        let association = DimensionAssociationInput::Explicit(vec![first.sources.first().copied(), second.sources.first().copied()]);
        (self.plane.place_entity(EntityType::Dimension(Dimension::Linear(dimension))), association)
    }

    fn ordinate_dimension(&self, point: &Candidate, place: DVec3, horizontal: bool) -> (EntityType, DimensionAssociationInput) {
        let datum = self.datum.unwrap_or(DVec3::ZERO);
        let leader = if horizontal { DVec3::new(point.point.x, place.y, 0.0) } else { DVec3::new(place.x, point.point.y, 0.0) };
        let mut dimension = DimensionOrdinate::new(v3(point.point), v3(leader), horizontal);
        dimension.definition_point = v3(datum);
        dimension.base.definition_point = v3(datum);
        dimension.base.text_middle_point = v3(leader);
        dimension.base.insertion_point = v3(leader);
        dimension.refresh_measurement();
        (self.plane.place_entity(EntityType::Dimension(Dimension::Ordinate(dimension))), DimensionAssociationInput::Explicit(vec![point.sources.first().copied()]))
    }

    fn build_radial_dimensions(&self, place: DVec3) -> Vec<(EntityType, DimensionAssociationInput)> {
        self.radial_sources().into_iter().enumerate().map(|(index, (handle, curve))| {
            let (center, radius) = match curve { Curve::Circle(circle) => (circle.centre, circle.radius), Curve::Arc(arc) => (arc.centre, arc.radius), _ => unreachable!() };
            let center = DVec3::new(center[0], center[1], 0.0);
            let mut direction = place - center;
            direction.z = 0.0;
            direction = if direction.length_squared() <= f64::EPSILON { DVec3::X } else { direction.normalize() };
            let chord = center + direction * radius;
            let text = place + DVec3::new(0.0, index as f64 * self.dim_spacing, 0.0);
            let association = DimensionAssociationInput::Explicit(vec![Some(DimensionAssociationSource::inferred(handle))]);
            let entity = if self.mode == Mode::Radius {
                let mut dimension = DimensionRadius::new(v3(center), v3(chord));
                dimension.base.definition_point = v3(chord);
                dimension.base.text_middle_point = v3(text);
                dimension.base.insertion_point = v3(text);
                dimension.base.text_user_positioned = true;
                dimension.leader_length = chord.distance(text);
                dimension.base.actual_measurement = dimension.measurement();
                EntityType::Dimension(Dimension::Radius(dimension))
            } else {
                let opposite = center - direction * radius;
                let mut dimension = DimensionDiameter::new(v3(chord), v3(opposite));
                dimension.base.definition_point = v3(opposite);
                dimension.base.text_middle_point = v3(text);
                dimension.base.insertion_point = v3(text);
                dimension.base.text_user_positioned = true;
                dimension.leader_length = chord.distance(text);
                dimension.base.actual_measurement = dimension.measurement();
                EntityType::Dimension(Dimension::Diameter(dimension))
            };
            (self.plane.place_entity(entity), association)
        }).collect()
    }

    fn remove_candidate(&mut self, point_world: DVec3) {
        let point = self.plane.to_local(point_world);
        let all = self.candidates();
        let Some(nearest) = all.iter().min_by(|first, second| first.point.distance(point).total_cmp(&second.point.distance(point))) else { return; };
        self.excluded.push(nearest.point);
    }

    fn preview_dimensions(&self, point: DVec3) -> Vec<WireModel> {
        self.build_dimensions(point).into_iter().enumerate().flat_map(|(index, (entity, _))| preview_entity(index, &entity)).collect()
    }
}

impl CadCommand for QdimCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) { self.plane = plane; }
    fn name(&self) -> &'static str { "QDIM" }

    fn prompt(&self) -> String {
        match self.step {
            Step::Gathering => t!("QDIM  Select geometry to dimension (Enter when done):").into_owned(),
            Step::Place => t!("QDIM  Specify dimension line position or choose an option:").into_owned(),
            Step::Datum => t!("QDIM  Specify datum point:").into_owned(),
            Step::Edit => t!("QDIM  Select a point to remove (Enter when done):").into_owned(),
            Step::Settings => t!("QDIM  Choose extension origin priority [Endpoints/Intersections]:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Place => vec![
                CmdOption::new("Continuous", "CONTINUOUS"), CmdOption::new("Staggered", "STAGGERED"),
                CmdOption::new("Baseline", "BASELINE"), CmdOption::new("Ordinate", "ORDINATE"),
                CmdOption::new("Radius", "RADIUS"), CmdOption::new("Diameter", "DIAMETER"),
                CmdOption::new("Datum point", "DATUM"), CmdOption::new("Edit", "EDIT"),
                CmdOption::new("Settings", "SETTINGS"),
            ],
            Step::Settings => vec![CmdOption::new("Endpoints", "ENDPOINTS"), CmdOption::new("Intersections", "INTERSECTIONS")],
            _ => Vec::new(),
        }
    }

    fn is_selection_gathering(&self) -> bool { self.step == Step::Gathering }
    fn selection_forces_add(&self) -> bool { self.step == Step::Gathering }
    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult { CmdResult::NeedPoint }
    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) { self.selection = entities; }
    fn wants_text_input(&self) -> bool { matches!(self.step, Step::Place | Step::Settings) }
    fn point_step_accepts_keywords(&self) -> bool { self.step == Step::Place }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        if self.step == Step::Settings {
            match keyword.as_str() {
                "E" | "ENDPOINT" | "ENDPOINTS" => { self.snap_priority = SnapPriority::Endpoints; self.step = Step::Place; return Some(CmdResult::SetQuickDimensionSnapPriority(0)); }
                "I" | "INTERSECTION" | "INTERSECTIONS" => { self.snap_priority = SnapPriority::Intersections; self.step = Step::Place; return Some(CmdResult::SetQuickDimensionSnapPriority(1)); }
                _ => return Some(CmdResult::NeedPoint),
            }
        }
        match keyword.as_str() {
            "C" | "CONTINUOUS" => self.mode = Mode::Continuous,
            "S" | "STAGGERED" => self.mode = Mode::Staggered,
            "B" | "BASELINE" => self.mode = Mode::Baseline,
            "O" | "ORDINATE" => self.mode = Mode::Ordinate,
            "R" | "RADIUS" => self.mode = Mode::Radius,
            "DI" | "DIAMETER" => self.mode = Mode::Diameter,
            "DA" | "DATUM" | "DATUMPOINT" => { self.step = Step::Datum; return Some(CmdResult::NeedPoint); }
            "E" | "EDIT" => { self.step = Step::Edit; return Some(CmdResult::NeedPoint); }
            "SE" | "SETTINGS" => { self.step = Step::Settings; return Some(CmdResult::NeedPoint); }
            _ => return Some(CmdResult::NeedPoint),
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Gathering | Step::Settings => CmdResult::NeedPoint,
            Step::Datum => { self.datum = Some(self.plane.to_local(point)); self.step = Step::Place; CmdResult::NeedPoint }
            Step::Edit => { self.remove_candidate(point); CmdResult::NeedPoint }
            Step::Place => {
                let dimensions = self.build_dimensions(point);
                if dimensions.is_empty() { CmdResult::NeedPoint } else { CmdResult::CommitDimensionsAndExit(dimensions) }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Gathering if !self.selection.is_empty() => { self.step = Step::Place; CmdResult::NeedPoint }
            Step::Edit | Step::Datum | Step::Settings => { self.step = Step::Place; CmdResult::NeedPoint }
            _ => CmdResult::Cancel,
        }
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        if self.step == Step::Place { return self.preview_dimensions(point); }
        if self.step == Step::Edit {
            let size = self.dim_spacing * 0.12;
            return self.candidates().into_iter().enumerate().flat_map(|(index, candidate)| {
                let point = self.plane.to_world(candidate.point);
                [
                    WireModel::solid_f64(format!("qdim_edit_{index}_x"), vec![
                        [point.x - size, point.y, point.z], [point.x + size, point.y, point.z],
                    ], WireModel::CYAN, false),
                    WireModel::solid_f64(format!("qdim_edit_{index}_y"), vec![
                        [point.x, point.y - size, point.z], [point.x, point.y + size, point.z],
                    ], WireModel::CYAN, false),
                ]
            }).collect();
        }
        Vec::new()
    }
}

fn v3(point: DVec3) -> Vector3 { Vector3::new(point.x, point.y, point.z) }
fn d3(point: Vector3) -> DVec3 { DVec3::new(point.x, point.y, point.z) }

fn candidate_tolerance(points: impl Iterator<Item = DVec3>) -> f64 {
    let magnitude = points.map(|point| point.x.abs().max(point.y.abs()).max(point.z.abs())).fold(1.0_f64, f64::max);
    Tolerance::default().linear().max(magnitude * f64::EPSILON * 64.0)
}

fn dimension_reference_points(dimension: &Dimension) -> Vec<DVec3> {
    match dimension {
        Dimension::Linear(value) => vec![d3(value.first_point), d3(value.second_point)],
        Dimension::Aligned(value) => vec![d3(value.first_point), d3(value.second_point)],
        Dimension::Radius(value) => vec![d3(value.angle_vertex), d3(value.definition_point)],
        Dimension::Diameter(value) => vec![d3(value.angle_vertex), d3(value.definition_point)],
        Dimension::Ordinate(value) => vec![d3(value.feature_location)],
        Dimension::Angular2Ln(value) => vec![d3(value.first_point), d3(value.second_point), d3(value.angle_vertex), d3(value.definition_point)],
        Dimension::Angular3Pt(value) => vec![d3(value.angle_vertex), d3(value.first_point), d3(value.second_point)],
        Dimension::Arc(value) => vec![d3(value.center_point), d3(value.first_extension_point), d3(value.second_extension_point)],
        Dimension::LargeRadial(value) => vec![d3(value.definition_point), d3(value.chord_point)],
    }
}

fn preview_entity(index: usize, entity: &EntityType) -> Vec<WireModel> {
    let EntityType::Dimension(dimension) = entity else { return Vec::new(); };
    let mut lines: Vec<Vec<[f64; 3]>> = Vec::new();
    match dimension {
        Dimension::Linear(value) => {
            let first = d3(value.first_point); let second = d3(value.second_point); let definition = d3(value.definition_point);
            let horizontal = value.rotation.cos().abs() >= value.rotation.sin().abs();
            let first_line = if horizontal { DVec3::new(first.x, definition.y, definition.z) } else { DVec3::new(definition.x, first.y, definition.z) };
            let second_line = if horizontal { DVec3::new(second.x, definition.y, definition.z) } else { DVec3::new(definition.x, second.y, definition.z) };
            lines.push(vec![first.to_array(), first_line.to_array()]);
            lines.push(vec![second.to_array(), second_line.to_array()]);
            lines.push(vec![first_line.to_array(), second_line.to_array()]);
        }
        Dimension::Ordinate(value) => lines.push(vec![d3(value.feature_location).to_array(), d3(value.leader_endpoint).to_array()]),
        Dimension::Radius(value) => lines.push(vec![d3(value.angle_vertex).to_array(), d3(value.definition_point).to_array(), d3(value.base.text_middle_point).to_array()]),
        Dimension::Diameter(value) => lines.push(vec![d3(value.definition_point).to_array(), d3(value.angle_vertex).to_array(), d3(value.base.text_middle_point).to_array()]),
        _ => {}
    }
    lines.into_iter().enumerate().map(|(part, points)| WireModel::solid_f64(format!("qdim_preview_{index}_{part}"), points, WireModel::CYAN, false)).collect()
}

inventory::submit!(crate::command::CommandRegistration { names: &["QDIM"] });
