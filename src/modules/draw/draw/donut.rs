// DONUT creates a ring as a closed, constant-width LwPolyline.

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::EntityType;
use cadkernel::geom2d::{
    Circle as KernelCircle, Curve as KernelCurve, Vec2 as KernelVec2,
};
use glam::DVec3;

use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::draw::defaults;
use crate::scene::model::wire_model::WireModel;

const TAU: f64 = std::f64::consts::TAU;

pub struct DonutCommand {
    state: DonutState,
    inner_r: f64,
    outer_r: f64,
    plane: WorkingPlane,
}

#[derive(Clone, Copy)]
enum DonutState {
    AskInner,
    AskInnerSecond(DVec3),
    AskOuter,
    AskOuterSecond(DVec3),
    PlaceCenter,
}

impl DonutCommand {
    pub fn new() -> Self {
        let mut inner_diameter = defaults::get_donut_inner_diameter();
        let mut outer_diameter = defaults::get_donut_outer_diameter();
        if inner_diameter > outer_diameter {
            std::mem::swap(&mut inner_diameter, &mut outer_diameter);
        }
        Self {
            state: DonutState::AskInner,
            inner_r: inner_diameter * 0.5,
            outer_r: outer_diameter * 0.5,
            plane: WorkingPlane::default(),
        }
    }

    fn set_inner_diameter(&mut self, diameter: f64) {
        self.inner_r = diameter * 0.5;
        self.state = DonutState::AskOuter;
    }

    fn set_outer_diameter(&mut self, diameter: f64) {
        self.outer_r = diameter * 0.5;
        if self.inner_r > self.outer_r {
            std::mem::swap(&mut self.inner_r, &mut self.outer_r);
        }
        defaults::set_donut_inner_diameter(self.inner_r * 2.0);
        defaults::set_donut_outer_diameter(self.outer_r * 2.0);
        self.state = DonutState::PlaceCenter;
    }

    fn point_distance(&self, first: DVec3, second: DVec3) -> f64 {
        let first = self.plane.to_local(first);
        let second = self.plane.to_local(second);
        KernelVec2::new(first.x, first.y).distance(KernelVec2::new(second.x, second.y))
    }

    fn project_to_plane(&self, point: DVec3) -> DVec3 {
        let local = self.plane.to_local(point);
        self.plane.to_world(DVec3::new(local.x, local.y, 0.0))
    }
}

fn prompt_with_default(prompt: &str, value: f64) -> String {
    match prompt.char_indices().next_back() {
        Some((index, suffix @ (':' | '：'))) => {
            format!("{} <{value:.4}>{suffix}", &prompt[..index])
        }
        _ => format!("{prompt} <{value:.4}>"),
    }
}

impl CadCommand for DonutCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DONUT"
    }

    fn prompt(&self) -> String {
        match self.state {
            DonutState::AskInner => t!("DONUT  Specify inside diameter <0>:")
                .replace("<0>", &format!("<{:.4}>", self.inner_r * 2.0)),
            DonutState::AskInnerSecond(_) => {
                t!("DONUT  Specify second point for inside diameter:").into_owned()
            }
            DonutState::AskOuter => prompt_with_default(
                t!("DONUT  Specify outside diameter:").as_ref(),
                self.outer_r * 2.0,
            ),
            DonutState::AskOuterSecond(_) => {
                t!("DONUT  Specify second point for outside diameter:").into_owned()
            }
            DonutState::PlaceCenter => {
                t!("DONUT  Specify center of donut (Enter to exit):").into_owned()
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.state, DonutState::AskInner | DonutState::AskOuter)
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.state, DonutState::AskInner | DonutState::AskOuter) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let val: f64 = text
            .trim()
            .replace(',', ".")
            .parse()
            .ok()
            .filter(|v: &f64| v.is_finite())?;
        match self.state {
            DonutState::AskInner => {
                if val < 0.0 {
                    return Some(CmdResult::NeedPoint);
                }
                self.set_inner_diameter(val);
                Some(CmdResult::NeedPoint)
            }
            DonutState::AskOuter => {
                if val <= 0.0 {
                    return Some(CmdResult::NeedPoint);
                }
                self.set_outer_diameter(val);
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.state {
            DonutState::AskInner => {
                self.state = DonutState::AskInnerSecond(self.project_to_plane(pt));
                CmdResult::NeedPoint
            }
            DonutState::AskInnerSecond(first) => {
                let diameter = self.point_distance(first, pt);
                if diameter <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                self.set_inner_diameter(diameter);
                CmdResult::NeedPoint
            }
            DonutState::AskOuter => {
                self.state = DonutState::AskOuterSecond(self.project_to_plane(pt));
                CmdResult::NeedPoint
            }
            DonutState::AskOuterSecond(first) => {
                let diameter = self.point_distance(first, pt);
                if diameter <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                self.set_outer_diameter(diameter);
                CmdResult::NeedPoint
            }
            DonutState::PlaceCenter => {
                let center = self.plane.to_local(pt);
                let entity = make_donut(center.x, center.y, center.z, self.inner_r, self.outer_r);
                // Keep command active so user can place more donuts.
                CmdResult::CommitEntity(self.plane.place_entity(entity))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.state {
            DonutState::AskInner => {
                self.state = DonutState::AskOuter;
                CmdResult::NeedPoint
            }
            DonutState::AskOuter => {
                self.set_outer_diameter(self.outer_r * 2.0);
                CmdResult::NeedPoint
            }
            DonutState::PlaceCenter => CmdResult::Cancel,
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.state {
            DonutState::AskInnerSecond(first) | DonutState::AskOuterSecond(first) => {
                let point = self.project_to_plane(pt);
                Some(WireModel::solid_f64(
                    "rubber_band".into(),
                    vec![first.to_array(), point.to_array()],
                    WireModel::CYAN,
                    false,
                ))
            }
            DonutState::PlaceCenter => Some(donut_wire(
                pt,
                self.inner_r,
                self.outer_r,
                self.plane,
            )),
            _ => None,
        }
    }
}

fn donut_wire(
    center: DVec3,
    inner_r: f64,
    outer_r: f64,
    plane: WorkingPlane,
) -> WireModel {
    let local_center = plane.to_local(center);
    let mut points = Vec::new();
    for radius in [outer_r, inner_r] {
        if radius <= 1.0e-9 {
            continue;
        }
        if !points.is_empty() {
            points.push([f64::NAN; 3]);
        }
        points.extend(
            KernelCurve::Circle(KernelCircle {
                centre: [local_center.x, local_center.y],
                radius,
            })
            .tessellate_angle(TAU / 64.0)
            .into_iter()
            .map(|point| {
                plane
                    .to_world(DVec3::new(point[0], point[1], local_center.z))
                    .to_array()
            }),
        );
    }
    WireModel::solid_f64("rubber_band".into(), points, WireModel::CYAN, false)
}

fn make_donut(cx: f64, cy: f64, elevation: f64, inner_r: f64, outer_r: f64) -> EntityType {
    use acadrust::types::Vector2;
    let r_avg = (inner_r + outer_r) / 2.0;
    let width = outer_r - inner_r;

    let mut p = LwPolyline::new();
    p.is_closed = true;
    p.constant_width = width;
    p.elevation = elevation;

    // Segment widths inherit the polyline's constant width.
    let mut v0 = LwVertex::new(Vector2::new(cx - r_avg, cy));
    v0.bulge = 1.0;

    let mut v1 = LwVertex::new(Vector2::new(cx + r_avg, cy));
    v1.bulge = 1.0;

    p.vertices = vec![v0, v1];
    EntityType::LwPolyline(p)
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DONUT"] });  // DonutCommand
