// DIVIDE and MEASURE place points or block references along a curve.
use acadrust::entities::{Insert, Point as PointEnt};
use acadrust::types::Vector3;
use acadrust::{Entity, EntityType, Handle};
use cadkernel::space::PlanarCurve;
use glam::DVec3;
use crate::command::{CadCommand, CmdOption, CmdResult, CurveMarker, WorkingPlane};
use crate::entities::curve::{entity_curve, entity_spatial_measurement};
use crate::t;
const MEASURE: bool = true;

#[derive(Clone, Copy, PartialEq)]
enum Step { Pick, Amount, BlockName, Align }

pub struct MeasurementMarkerCommand {
    target: Option<Handle>,
    pick_point: DVec3,
    step: Step,
    blocks: Vec<String>,
    block: Option<String>,
    align: bool,
    plane: WorkingPlane,
    valid_pick: bool,
    value_origin: Option<DVec3>,
}

pub type MeasureCommand = MeasurementMarkerCommand;

impl MeasurementMarkerCommand {
    pub fn new() -> Self {
        Self { target: None, pick_point: DVec3::ZERO, step: Step::Pick,
            blocks: Vec::new(), block: None, align: true,
            plane: WorkingPlane::default(), valid_pick: false, value_origin: None }
    }
    pub fn with_blocks(mut self, blocks: Vec<String>) -> Self {
        self.blocks = blocks;
        self
    }
    fn marker(&self) -> Option<CurveMarker> {
        self.block.as_ref().map(|name| CurveMarker {
            block: name.clone(), align: self.align, plane: self.plane,
        })
    }
}

impl CadCommand for MeasurementMarkerCommand {
    fn name(&self) -> &'static str { if MEASURE { "MEASURE" } else { "DIVIDE" } }
    fn prompt(&self) -> String {
        if self.value_origin.is_some() { return "MEASURE  Specify second point:".into(); }
        let prompt = match self.step {
            Step::Pick if MEASURE => "Select object to measure:",
            Step::Pick => "Select object to divide:",
            Step::BlockName => "Enter name of block to insert:",
            Step::Align => "Align block with object? [Yes/No] <Yes>:",
            Step::Amount if MEASURE && self.block.is_none() => "Specify length of segment or [Block]:",
            Step::Amount if MEASURE => "Specify length of segment:",
            Step::Amount if self.block.is_none() => "Enter number of segments or [Block]:",
            Step::Amount => "Enter number of segments:",
        };
        format!("{}  {}", self.name(), prompt)
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.value_origin.is_some() { return vec![]; }
        match self.step {
            Step::Amount if self.block.is_none() => vec![CmdOption::new("Block", "B")],
            Step::Align => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            _ => vec![],
        }
    }
    fn set_working_plane(&mut self, plane: WorkingPlane) { self.plane = plane; }
    fn needs_entity_pick(&self) -> bool { self.step == Step::Pick }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.valid_pick = measurable(&entity).is_some();
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() || !self.valid_pick { return CmdResult::NeedPoint; }
        self.target = Some(handle);
        self.pick_point = pt;
        self.step = Step::Amount;
        CmdResult::NeedPoint
    }
    fn wants_text_input(&self) -> bool { self.step != Step::Pick }
    fn dyn_field(&self) -> crate::command::DynField {
        if self.step == Step::Amount && MEASURE { crate::command::DynField::Distance }
        else if self.step == Step::Amount { crate::command::DynField::Scalar }
        else { crate::command::DynField::Point }
    }
    fn dyn_commit_as_text(&self) -> bool { self.step == Step::Amount }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        match self.step {
            Step::BlockName => {
                self.block = Some(self.blocks.iter().find(|name| name.eq_ignore_ascii_case(text))?.clone());
                self.step = Step::Align;
            }
            Step::Align => {
                self.align = match text.to_ascii_uppercase().as_str() {
                    "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return None,
                };
                self.step = Step::Amount;
            }
            Step::Amount => {
                if self.block.is_none() && self.value_origin.is_none() && matches!(text.to_ascii_uppercase().as_str(), "B" | "BLOCK") {
                    self.step = Step::BlockName;
                } else {
                    let segment_length = text.replace(',', ".").parse::<f64>().ok()
                        .filter(|d| d.is_finite() && *d > 0.0)?;
                    return Some(CmdResult::MeasureEntity { handle: self.target?, segment_length,
                        pick_point: self.pick_point, marker: self.marker() });
                }
            }
            Step::Pick => return None,
        }
        Some(CmdResult::NeedPoint)
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if !MEASURE || self.step != Step::Amount || !cursor.is_finite() { return None; }
        let origin = self.value_origin?;
        let distance = cadkernel::space::Vec3::from(cursor.to_array()).distance(origin.to_array().into());
        (distance.is_finite() && distance > 0.0).then_some(distance)
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if MEASURE && self.step == Step::Amount && pt.is_finite() {
            if self.value_origin.is_none() { self.value_origin = Some(pt); }
            else if let Some(value) = self.dyn_live_value(pt) {
                return self.on_text_input(&value.to_string()).unwrap_or(CmdResult::NeedPoint);
            }
        }
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.step == Step::Align {
            self.align = true;
            self.step = Step::Amount;
            CmdResult::NeedPoint
        } else { CmdResult::Cancel }
    }
}


pub struct DivideCommand {
    target: Option<Handle>,
    waiting_for_n: bool,
}

impl DivideCommand {
    pub fn new() -> Self {
        Self {
            target: None,
            waiting_for_n: false,
        }
    }
}

impl CadCommand for DivideCommand {
    fn name(&self) -> &'static str {
        "DIVIDE"
    }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            t!("DIVIDE  Select object to divide:").into_owned()
        } else {
            t!("DIVIDE  Enter number of segments:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        self.waiting_for_n = true;
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        self.waiting_for_n
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if self.waiting_for_n {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let n: usize = text.trim().parse().ok().filter(|&n| n >= 2)?;
        let handle = self.target?;
        self.waiting_for_n = false;
        Some(CmdResult::DivideEntity { handle, n })
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


pub fn divide_entity(entity: &EntityType, n: usize) -> Vec<EntityType> {
    if n < 2 {
        return vec![];
    }
    let Some(curve) = entity_curve(entity) else {
        return vec![];
    };
    let total = curve.length();
    if !total.is_finite() || total <= 1e-10 { return vec![]; }
    let step = total / n as f64;
    (1..n)
        .map(|k| make_point(curve.point_at_distance(step * k as f64)))
        .collect()
}

pub fn measure_entity(entity: &EntityType, segment_length: f64, pick_point: DVec3,
    marker: Option<&CurveMarker>) -> Vec<EntityType> {
    if !segment_length.is_finite() || segment_length <= 0.0 {
        return vec![];
    }
    let Some((curve, total)) = measurable(entity) else {
        return vec![];
    };
    let mut pts = Vec::new();
    let first = DVec3::from_array(curve.point_at_distance(0.0));
    let last = DVec3::from_array(curve.point_at_distance(total));
    let reverse = !curve.is_closed() && pick_point.distance_squared(last) < pick_point.distance_squared(first);
    // A complete final interval includes the end point. Index multiplication
    // avoids cumulative drift when many intervals are placed on a long curve.
    let count = (total / segment_length).floor() as usize;
    for index in 1..=count {
        let walked = segment_length * index as f64;
        pts.push(make_marker(&curve, if reverse { total - walked } else { walked }, marker));
    }
    pts
}

fn make_marker(curve: &MeasuredCurve, distance: f64, marker: Option<&CurveMarker>) -> EntityType {
    let pos = curve.point_at_distance(distance);
    let Some(marker) = marker else {
        let mut point = PointEnt::new();
        point.location = Vector3::new(pos[0], pos[1], pos[2]);
        return EntityType::Point(point);
    };
    let local = marker.plane.to_local(DVec3::from_array(pos));
    let mut insert = Insert::new(marker.block.clone(), Vector3::new(local.x, local.y, local.z));
    if marker.align {
        let tangent = DVec3::from_array(curve.tangent_at(curve.parameter_at_distance(distance)));
        let tangent = marker.plane.vector_to_local(tangent);
        insert.rotation = tangent.y.atan2(tangent.x);
    }
    insert.apply_transform(&marker.plane.to_world_transform());
    EntityType::Insert(insert)
}

fn make_point(pos: [f64; 3]) -> EntityType {
    let mut p = PointEnt::new();
    p.location = Vector3::new(pos[0], pos[1], pos[2]);
    EntityType::Point(p)
}

/// The entity's curve and its length, or `None` for anything that cannot be
/// walked along — a hatch, a block, an unbounded ray.
enum MeasuredCurve {
    Planar(PlanarCurve),
    Spatial(cadkernel::space::ArcLengthCurve3),
}
impl MeasuredCurve {
    fn is_closed(&self) -> bool {
        match self { Self::Planar(curve) => curve.is_closed(), Self::Spatial(curve) => curve.is_closed() }
    }
    fn point_at_distance(&self, distance: f64) -> [f64; 3] {
        match self { Self::Planar(curve) => curve.point_at_distance(distance), Self::Spatial(curve) => curve.point_at_distance(distance) }
    }
    fn parameter_at_distance(&self, distance: f64) -> f64 {
        match self { Self::Planar(curve) => curve.parameter_at_distance(distance), Self::Spatial(curve) => curve.parameter_at_distance(distance) }
    }
    fn tangent_at(&self, parameter: f64) -> [f64; 3] {
        match self { Self::Planar(curve) => curve.tangent_at(parameter), Self::Spatial(curve) => curve.tangent_at(parameter) }
    }
}

fn measurable(entity: &EntityType) -> Option<(MeasuredCurve, f64)> {
    let (curve, total) = if let Some(curve) = entity_curve(entity) {
        let total = curve.length();
        (MeasuredCurve::Planar(curve), total)
    } else {
        let curve = entity_spatial_measurement(entity)?;
        let total = curve.length();
        (MeasuredCurve::Spatial(curve), total)
    };
    (total.is_finite() && total > 1e-10).then_some((curve, total))
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIVIDE"] });
inventory::submit!(crate::command::CommandRegistration { names: &["MEASURE"] });
