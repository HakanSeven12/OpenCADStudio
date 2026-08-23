// Circle creation commands.

use acadrust::types::Vector3;
use acadrust::{Circle, EntityType};
use cadkernel::geom2d::{
    fillets_between, Circle as KernelCircle, Curve as KernelCurve, Line as KernelLine,
    Tolerance,
};
use crate::t;

use crate::command::{CadCommand, CmdResult, DynField, TangentObject, WorkingPlane};
use crate::modules::draw::defaults;
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

const TAU: f64 = std::f64::consts::TAU;

// ── Per-method SVG icons ───────────────────────────────────────────────────

const ICON_CR: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_cr.svg"
));
const ICON_CD: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_cd.svg"
));
const ICON_2P: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_2p.svg"
));
const ICON_3P: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_3p.svg"
));
const ICON_TTR: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_ttr.svg"
));
const ICON_TTT: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/circle/circle_ttt.svg"
));

// ── Dropdown metadata (used by ribbon.rs) ─────────────────────────────────

pub const DROPDOWN_ID: &str = "CIRCLE";

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("CIRCLE", "Center, Radius", ICON_CR),
    ("CIRCLE_CD", "Center, Diameter", ICON_CD),
    ("CIRCLE_2P", "2-Point", ICON_2P),
    ("CIRCLE_3P", "3-Point", ICON_3P),
    ("CIRCLE_TTR", "Tan, Tan, Radius", ICON_TTR),
    ("CIRCLE_TTT", "Tan, Tan, Tan", ICON_TTT),
];

/// Default icon — shown until first use (falls back to Center, Radius).
pub const ICON: IconKind = ICON_CR;

// ── Shared geometry ────────────────────────────────────────────────────────

fn circle_wire(center: DVec3, radius: f64, plane: WorkingPlane) -> WireModel {
    let center = plane.to_local(center);
    let points = KernelCurve::Circle(KernelCircle {
        centre: [center.x, center.y],
        radius,
    })
        .tessellate_angle(TAU / 64.0)
        .into_iter()
        .map(|point| plane.to_world(DVec3::new(point[0], point[1], center.z)).to_array())
        .collect();
    WireModel::solid_f64("rubber_band".into(), points, WireModel::CYAN, false)
}

fn make_circle(center: DVec3, radius: f64, plane: WorkingPlane) -> EntityType {
    let center = plane.to_local(center);
    plane.place_entity(EntityType::Circle(Circle {
        center: Vector3::new(center.x, center.y, center.z),
        radius,
        ..Default::default()
    }))
}

fn circumcircle(
    a: DVec3,
    b: DVec3,
    c: DVec3,
    plane: WorkingPlane,
) -> Option<(DVec3, f64)> {
    let (a, b, c) = (plane.to_local(a), plane.to_local(b), plane.to_local(c));
    let circle = cadkernel::geom2d::arc_through_points(
        [a.x, a.y],
        [b.x, b.y],
        [c.x, c.y],
    )?;
    let center = plane.to_world(DVec3::new(circle.centre[0], circle.centre[1], a.z));
    Some((center, circle.radius))
}

fn plane_distance(a: DVec3, b: DVec3, plane: WorkingPlane) -> f64 {
    let d = plane.vector_to_local(b - a);
    d.x.hypot(d.y)
}

fn tangent_object_local(object: TangentObject, plane: WorkingPlane) -> TangentObject {
    match object {
        TangentObject::Line { p1, p2 } => TangentObject::Line {
            p1: plane.to_local(p1),
            p2: plane.to_local(p2),
        },
        TangentObject::Circle { center, radius } => TangentObject::Circle {
            center: plane.to_local(center),
            radius,
        },
    }
}

// ── Command: Center, Radius ────────────────────────────────────────────────

pub struct CircleCommand {
    step: StepCR,
    default_r: f64,
    plane: WorkingPlane,
}
enum StepCR {
    Center,
    Radius(DVec3),
    Diameter(DVec3),
}

impl CircleCommand {
    pub fn new() -> Self {
        Self {
            step: StepCR::Center,
            default_r: defaults::get_circle_radius(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for CircleCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE"
    }
    fn prompt(&self) -> String {
        match &self.step {
            StepCR::Center => t!("CIRCLE  Specify center point:").into_owned(),
            StepCR::Radius(c) => {
                let r = format!("{:.4}", self.default_r);
                let cx = format!("{:.3}", c.x);
                let cy = format!("{:.3}", c.y);
                t!(
                    "CIRCLE  Specify radius or type value  <%{r}>  [center (%{cx},%{cy})]:",
                    r = r,
                    cx = cx,
                    cy = cy
                )
                .into_owned()
            }
            StepCR::Diameter(_) => crate::tf!(
                "CIRCLE  Specify diameter  <{:.4}>:",
                self.default_r * 2.0
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            StepCR::Center => vec![
                CmdOption::new("3P", "3P"),
                CmdOption::new("2P", "2P"),
                CmdOption::new("Ttr", "TTR"),
                CmdOption::new("Ttt", "TTT"),
            ],
            StepCR::Radius(_) => vec![
                CmdOption::new(t!("Diameter").as_ref(), "D"),
            ],
            StepCR::Diameter(_) => vec![],
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !matches!(self.step, StepCR::Diameter(_))
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            StepCR::Center => {
                self.step = StepCR::Radius(pt);
                CmdResult::NeedPoint
            }
            StepCR::Radius(c) => {
                let r = plane_distance(*c, pt, self.plane);
                if r <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                defaults::set_circle_radius(r);
                CmdResult::CommitAndExit(make_circle(*c, r, self.plane))
            }
            StepCR::Diameter(c) => {
                let radius = plane_distance(*c, pt, self.plane);
                if radius <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                defaults::set_circle_radius(radius);
                CmdResult::CommitAndExit(make_circle(*c, radius, self.plane))
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            StepCR::Radius(center) => {
                CmdResult::CommitAndExit(make_circle(center, self.default_r, self.plane))
            }
            StepCR::Diameter(center) => {
                CmdResult::CommitAndExit(make_circle(center, self.default_r, self.plane))
            }
            StepCR::Center => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            StepCR::Radius(center) => Some(circle_wire(
                center,
                plane_distance(center, pt, self.plane),
                self.plane,
            )),
            StepCR::Diameter(center) => Some(circle_wire(
                center,
                plane_distance(center, pt, self.plane),
                self.plane,
            )),
            StepCR::Center => None,
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // At the centre step, keyword options switch construction method by
        // handing off to the dedicated variant command. (#304)
        if matches!(self.step, StepCR::Center) {
            return match text.trim().to_uppercase().as_str() {
                "3P" => Some(CmdResult::Dispatch("CIRCLE_3P".into())),
                "2P" => Some(CmdResult::Dispatch("CIRCLE_2P".into())),
                "T" | "TTR" => Some(CmdResult::Dispatch("CIRCLE_TTR".into())),
                "TTT" => Some(CmdResult::Dispatch("CIRCLE_TTT".into())),
                _ => None,
            };
        }
        if let StepCR::Radius(center) = &self.step {
            if matches!(text.trim().to_uppercase().as_str(), "D" | "DIAMETER") {
                self.step = StepCR::Diameter(*center);
                return Some(CmdResult::NeedPoint);
            }
        }
        if let StepCR::Radius(c) = &self.step {
            let r: f64 = text.trim().replace(',', ".").parse().ok()?;
            if r > 0.0 {
                defaults::set_circle_radius(r);
                return Some(CmdResult::CommitAndExit(make_circle(*c, r, self.plane)));
            }
        }
        if let StepCR::Diameter(c) = &self.step {
            let diameter: f64 = text.trim().replace(',', ".").parse().ok()?;
            if diameter > 0.0 {
                defaults::set_circle_diam(diameter);
                return Some(CmdResult::CommitAndExit(make_circle(
                    *c,
                    diameter * 0.5,
                    self.plane,
                )));
            }
        }
        None
    }
    fn dyn_field(&self) -> DynField {
        match self.step {
            StepCR::Center => DynField::Point,
            StepCR::Radius(_) | StepCR::Diameter(_) => DynField::Distance,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match self.step {
            StepCR::Center => None,
            StepCR::Radius(c) => Some(DynSpec {
                anchor: DynAnchor::Point(c),
                fields: vec![DynFieldSpec::new(DynRole::Radius)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
            StepCR::Diameter(c) => Some(DynSpec {
                anchor: DynAnchor::Point(c),
                fields: vec![DynFieldSpec::new(DynRole::Diameter)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
        }
    }
}

// ── Command: Center, Diameter ──────────────────────────────────────────────

pub struct CircleCDCommand {
    step: StepCR,
    default_d: f64,
    plane: WorkingPlane,
}

impl CircleCDCommand {
    pub fn new() -> Self {
        Self {
            step: StepCR::Center,
            default_d: defaults::get_circle_diam(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for CircleCDCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE_CD"
    }
    fn prompt(&self) -> String {
        match &self.step {
            StepCR::Center => t!("CIRCLE CD  Specify center point:").into_owned(),
            StepCR::Diameter(c) => crate::tf!(
                "CIRCLE CD  Specify diameter or type value  <{:.4}>  [center ({:.3},{:.3})]:",
                self.default_d, c.x, c.y
            )
            .into_owned(),
            StepCR::Radius(_) => unreachable!(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            StepCR::Center => {
                self.step = StepCR::Diameter(pt);
                CmdResult::NeedPoint
            }
            StepCR::Diameter(c) => {
                let radius = plane_distance(*c, pt, self.plane);
                if radius <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                defaults::set_circle_radius(radius);
                CmdResult::CommitAndExit(make_circle(*c, radius, self.plane))
            }
            StepCR::Radius(_) => unreachable!(),
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        if let StepCR::Diameter(c) = &self.step {
            let c = *c;
            let d = self.default_d;
            return CmdResult::CommitAndExit(make_circle(c, d / 2.0, self.plane));
        }
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if let StepCR::Diameter(c) = &self.step {
            Some(circle_wire(
                *c,
                plane_distance(*c, pt, self.plane),
                self.plane,
            ))
        } else {
            None
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let StepCR::Diameter(c) = &self.step {
            let d: f64 = text.trim().replace(',', ".").parse().ok()?;
            if d > 0.0 {
                defaults::set_circle_diam(d);
                return Some(CmdResult::CommitAndExit(make_circle(*c, d / 2.0, self.plane)));
            }
        }
        None
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match self.step {
            StepCR::Center => None,
            StepCR::Diameter(c) => Some(DynSpec {
                anchor: DynAnchor::Point(c),
                fields: vec![DynFieldSpec::new(DynRole::Diameter)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
            StepCR::Radius(_) => None,
        }
    }
}

// ── Command: 2-Point ──────────────────────────────────────────────────────

pub struct Circle2PCommand {
    p1: Option<DVec3>,
    plane: WorkingPlane,
}

impl Circle2PCommand {
    pub fn new() -> Self {
        Self {
            p1: None,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for Circle2PCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE_2P"
    }
    fn prompt(&self) -> String {
        if self.p1.is_none() {
            crate::t!("CIRCLE 2P  Specify first end of diameter:").into_owned()
        } else {
            crate::t!("CIRCLE 2P  Specify second end of diameter:").into_owned()
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.p1 {
            None => {
                self.p1 = Some(pt);
                CmdResult::NeedPoint
            }
            Some(p1) => {
                let center = (p1 + pt) * 0.5;
                let radius = plane_distance(p1, pt, self.plane) / 2.0;
                if radius <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                defaults::set_circle_radius(radius);
                CmdResult::CommitAndExit(make_circle(center, radius, self.plane))
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let p1 = self.p1?;
        let center = (p1 + pt) * 0.5;
        let radius = plane_distance(p1, pt, self.plane) / 2.0;
        Some(circle_wire(center, radius, self.plane))
    }
}

// ── Command: 3-Point ──────────────────────────────────────────────────────

pub struct Circle3PCommand {
    pts: Vec<DVec3>,
    plane: WorkingPlane,
}

impl Circle3PCommand {
    pub fn new() -> Self {
        Self {
            pts: Vec::new(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for Circle3PCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE_3P"
    }
    fn prompt(&self) -> String {
        match self.pts.len() {
            0 => crate::t!("CIRCLE 3P  Specify first point:").into_owned(),
            1 => crate::t!("CIRCLE 3P  Specify second point:").into_owned(),
            _ => crate::t!("CIRCLE 3P  Specify third point:").into_owned(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.pts.push(pt);
        if self.pts.len() < 3 {
            return CmdResult::NeedPoint;
        }
        let (a, b, c) = (self.pts[0], self.pts[1], self.pts[2]);
        match circumcircle(a, b, c, self.plane) {
            Some((center, radius)) => {
                defaults::set_circle_radius(radius);
                CmdResult::CommitAndExit(make_circle(center, radius, self.plane))
            }
            None => {
                self.pts.pop();
                CmdResult::NeedPoint
            } // collinear — ask again
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.pts.len() {
            0 => None,
            1 => {
                // Show circle preview with p1→cursor as diameter (same as 2P).
                let p1 = self.pts[0];
                let center = (p1 + pt) * 0.5;
                let radius = plane_distance(p1, pt, self.plane) / 2.0;
                Some(circle_wire(center, radius, self.plane))
            }
            _ => {
                // Show circumcircle preview if non-collinear, else polyline.
                let (a, b) = (self.pts[0], self.pts[1]);
                if let Some((center, radius)) = circumcircle(a, b, pt, self.plane) {
                    Some(circle_wire(center, radius, self.plane))
                } else {
                    Some(WireModel::solid_f64(
                        "rubber_band".into(),
                        vec![[a.x, a.y, a.z], [b.x, b.y, b.z], [pt.x, pt.y, pt.z]],
                        WireModel::CYAN,
                        false,
                    ))
                }
            }
        }
    }
}

// ── 2-D geometry for TTR/TTT ────────────────────────────────────

#[derive(Clone, Copy)]
struct Line2D {
    a: f64,
    b: f64,
    c: f64,
} // ax + by + c = 0, a²+b²=1

impl Line2D {
    fn from_obj(p1: DVec3, p2: DVec3) -> Self {
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-9 {
            return Self {
                a: 1.0,
                b: 0.0,
                c: -p1.x,
            };
        }
        let a = -dy / len;
        let b = dx / len;
        Self {
            a,
            b,
            c: -(a * p1.x + b * p1.y),
        }
    }
}

fn solve_quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-9 {
        if b.abs() < 1e-9 {
            return vec![];
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec![];
    }
    let sq = disc.sqrt();
    vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
}

fn best_of(candidates: &[DVec3], hint: DVec3) -> Option<DVec3> {
    candidates.iter().copied().min_by(|a, b| {
        a.distance(hint)
            .partial_cmp(&b.distance(hint))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn best_circle_of(candidates: &[(DVec3, f64)], hint: DVec3) -> Option<(DVec3, f64)> {
    candidates
        .iter()
        .copied()
        .filter(|&(_, r)| r > 1e-4)
        .min_by(|(a, _), (b, _)| {
            a.distance(hint)
                .partial_cmp(&b.distance(hint))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn tangent_curve(object: TangentObject) -> KernelCurve {
    match object {
        TangentObject::Line { p1, p2 } => KernelCurve::Line(KernelLine {
            start: [p1.x, p1.y],
            end: [p2.x, p2.y],
        }),
        TangentObject::Circle { center, radius } => KernelCurve::Circle(KernelCircle {
            centre: [center.x, center.y],
            radius,
        }),
    }
}

fn ttr_candidates(first: TangentObject, second: TangentObject, radius: f64) -> Vec<DVec3> {
    let first = tangent_curve(first);
    let second = tangent_curve(second);
    fillets_between(&first, &second, radius, Tolerance::default())
        .into_iter()
        .map(|fillet| DVec3::new(fillet.centre[0], fillet.centre[1], 0.0))
        .collect()
}

/// Unified TTT solver: circle tangent to three objects. Returns all (center, radius) candidates.
fn ttt_candidates(
    obj1: TangentObject,
    obj2: TangentObject,
    obj3: TangentObject,
) -> Vec<(DVec3, f64)> {
    let objs = [obj1, obj2, obj3];
    let mut results = Vec::new();
    let sign_combos: [[f64; 3]; 8] = [
        [-1., -1., -1.],
        [-1., -1., 1.],
        [-1., 1., -1.],
        [-1., 1., 1.],
        [1., -1., -1.],
        [1., -1., 1.],
        [1., 1., -1.],
        [1., 1., 1.],
    ];
    for eps in &sign_combos {
        for (center, r) in ttt_solve_sign(&objs, eps) {
            if r > 1e-4 {
                results.push((center, r));
            }
        }
    }
    results
}

// LinEq: lx*cx + ly*cy + lr*r = k
struct LinEq {
    lx: f64,
    ly: f64,
    lr: f64,
    k: f64,
}

fn ttt_solve_sign(objs: &[TangentObject; 3], eps: &[f64; 3]) -> Vec<(DVec3, f64)> {
    let mut lin_eqs: Vec<LinEq> = Vec::new();
    let mut circle_idx: Vec<usize> = Vec::new();

    for (i, &obj) in objs.iter().enumerate() {
        match obj {
            TangentObject::Line { p1, p2 } => {
                let l = Line2D::from_obj(p1, p2);
                lin_eqs.push(LinEq {
                    lx: l.a,
                    ly: l.b,
                    lr: -eps[i],
                    k: -l.c,
                });
            }
            TangentObject::Circle { .. } => {
                circle_idx.push(i);
            }
        }
    }

    // Circle-pair differences → additional linear equations
    for j in 1..circle_idx.len() {
        let i0 = circle_idx[0];
        let i1 = circle_idx[j];
        if let (
            TangentObject::Circle {
                center: p0,
                radius: r0,
            },
            TangentObject::Circle {
                center: p1,
                radius: r1,
            },
        ) = (objs[i0], objs[i1])
        {
            let lx = 2.0 * (p1.x - p0.x);
            let ly = 2.0 * (p1.y - p0.y);
            let lr = -2.0 * (r0 * eps[i0] - r1 * eps[i1]);
            let k = (r0 * r0 - r1 * r1) + p1.x * p1.x + p1.y * p1.y - p0.x * p0.x - p0.y * p0.y;
            lin_eqs.push(LinEq { lx, ly, lr, k });
        }
    }

    if lin_eqs.len() < 2 {
        return vec![];
    }

    let e0 = &lin_eqs[0];
    let e1 = &lin_eqs[1];
    let det = e0.lx * e1.ly - e1.lx * e0.ly;
    if det.abs() < 1e-9 {
        // Recover the concentric-circle case from its radius constraint.
        let (pin, other) = if e0.lx.hypot(e0.ly) < 1e-9 {
            (e0, e1)
        } else {
            (e1, e0)
        };
        if pin.lx.hypot(pin.ly) >= 1e-9
            || pin.lr.abs() < 1e-9
            || other.lx.hypot(other.ly) < 1e-9
        {
            return vec![];
        }
        let r = pin.k / pin.lr;
        if r <= 1e-4 {
            return vec![];
        }
        let Some(&ci) = circle_idx.first() else {
            return vec![];
        };
        let TangentObject::Circle {
            center: cp,
            radius: cr,
        } = objs[ci]
        else {
            return vec![];
        };
        let rho = (r + eps[ci] * cr).abs();
        // The center line: other.lx·cx + other.ly·cy = m.
        let m = other.k - other.lr * r;
        let l2 = other.lx * other.lx + other.ly * other.ly;
        let l = l2.sqrt();
        let bx = other.lx * m / l2;
        let by = other.ly * m / l2;
        let dx = -other.ly / l;
        let dy = other.lx / l;
        let p = bx - cp.x;
        let q = by - cp.y;
        return solve_quadratic(1.0, 2.0 * (p * dx + q * dy), p * p + q * q - rho * rho)
            .into_iter()
            .map(|t| (DVec3::new(bx + dx * t, by + dy * t, 0.0), r))
            .collect();
    }

    // cx = a_cx + b_cx*r,  cy = a_cy + b_cy*r
    let a_cx = (e0.k * e1.ly - e1.k * e0.ly) / det;
    let b_cx = -(e0.lr * e1.ly - e1.lr * e0.ly) / det;
    let a_cy = (e0.lx * e1.k - e1.lx * e0.k) / det;
    let b_cy = -(e0.lx * e1.lr - e1.lx * e0.lr) / det;

    let r_vals: Vec<f64> = if lin_eqs.len() >= 3 {
        let e2 = &lin_eqs[2];
        let r_coeff = b_cx * e2.lx + b_cy * e2.ly + e2.lr;
        let r_const = e2.k - a_cx * e2.lx - a_cy * e2.ly;
        if r_coeff.abs() < 1e-9 {
            vec![]
        } else {
            vec![r_const / r_coeff]
        }
    } else if !circle_idx.is_empty() {
        let ci = circle_idx[0];
        if let TangentObject::Circle {
            center: cp,
            radius: cr,
        } = objs[ci]
        {
            let p = a_cx - cp.x;
            let q = a_cy - cp.y;
            let e = eps[ci];
            let a_q = b_cx * b_cx + b_cy * b_cy - e * e;
            let b_q = 2.0 * (p * b_cx + q * b_cy - cr * e);
            let c_q = p * p + q * q - cr * cr;
            solve_quadratic(a_q, b_q, c_q)
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    r_vals
        .into_iter()
        .filter_map(|r| {
            if r <= 1e-4 {
                return None;
            }
            let cx = a_cx + b_cx * r;
            let cy = a_cy + b_cy * r;
            Some((DVec3::new(cx, cy, 0.0), r))
        })
        .collect()
}

// ── Command: Tan, Tan, Radius ──────────────────────────────────────────────

pub struct CircleTTRCommand {
    step: StepTTR,
    default_r: f64,
    plane: WorkingPlane,
}

enum StepTTR {
    First,
    Second {
        obj1: TangentObject,
        hit1: DVec3,
    },
    Radius {
        obj1: TangentObject,
        obj2: TangentObject,
        hit1: DVec3,
        hit2: DVec3,
    },
}

impl CircleTTRCommand {
    pub fn new() -> Self {
        Self {
            step: StepTTR::First,
            default_r: defaults::get_circle_radius(),
            plane: WorkingPlane::default(),
        }
    }

    fn result_for_radius(&self, radius: f64) -> CmdResult {
        if radius <= 1.0e-9 {
            return CmdResult::NeedPoint;
        }
        let StepTTR::Radius {
            obj1,
            obj2,
            hit1,
            hit2,
        } = &self.step
        else {
            return CmdResult::NeedPoint;
        };
        let hint = (*hit1 + *hit2) * 0.5;
        let candidates = ttr_candidates(*obj1, *obj2, radius);
        let Some(center) = best_of(&candidates, hint) else {
            return CmdResult::NeedPoint;
        };
        defaults::set_circle_radius(radius);
        CmdResult::CommitAndExit(make_circle(
            self.plane.to_world(center),
            radius,
            self.plane,
        ))
    }

    fn radius_from_point(&self, point: DVec3) -> Option<f64> {
        let StepTTR::Radius { hit2, .. } = &self.step else {
            return None;
        };
        let point = self.plane.to_local(point);
        Some((point.x - hit2.x).hypot(point.y - hit2.y))
    }
}

impl CadCommand for CircleTTRCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE_TTR"
    }

    fn needs_tangent_pick(&self) -> bool {
        matches!(self.step, StepTTR::First | StepTTR::Second { .. })
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, StepTTR::Radius { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.step, StepTTR::Radius { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn prompt(&self) -> String {
        match &self.step {
            StepTTR::First => crate::t!("CIRCLE TTR  Select first tangent object:").into_owned(),
            StepTTR::Second { .. } => crate::t!("CIRCLE TTR  Select second tangent object:").into_owned(),
            StepTTR::Radius { .. } => format!(
                "{} <{:.4}>",
                crate::t!("CIRCLE TTR  Specify radius:"),
                self.default_r
            ),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.radius_from_point(pt)
            .map(|radius| self.result_for_radius(radius))
            .unwrap_or(CmdResult::NeedPoint)
    }

    fn on_tangent_point(&mut self, obj: TangentObject, hit: DVec3) -> CmdResult {
        let obj = tangent_object_local(obj, self.plane);
        let hit = self.plane.to_local(hit);
        match &self.step {
            StepTTR::First => {
                self.step = StepTTR::Second {
                    obj1: obj,
                    hit1: hit,
                };
                CmdResult::NeedPoint
            }
            StepTTR::Second { obj1, hit1 } => {
                let (o1, h1) = (*obj1, *hit1);
                self.step = StepTTR::Radius {
                    obj1: o1,
                    obj2: obj,
                    hit1: h1,
                    hit2: hit,
                };
                CmdResult::NeedPoint
            }
            StepTTR::Radius { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let r: f64 = text.trim().replace(',', ".").parse().ok()?;
        if r <= 0.0 {
            return Some(CmdResult::NeedPoint);
        }
        matches!(self.step, StepTTR::Radius { .. }).then(|| self.result_for_radius(r))
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.step, StepTTR::Radius { .. }) {
            self.result_for_radius(self.default_r)
        } else {
            CmdResult::Cancel
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let radius = self.radius_from_point(pt)?;
        let StepTTR::Radius {
            obj1,
            obj2,
            hit1,
            hit2,
        } = &self.step
        else {
            return None;
        };
        let center = best_of(&ttr_candidates(*obj1, *obj2, radius), (*hit1 + *hit2) * 0.5)?;
        Some(circle_wire(
            self.plane.to_world(center),
            radius,
            self.plane,
        ))
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        let StepTTR::Radius { hit2, .. } = &self.step else {
            return None;
        };
        Some(DynSpec {
            anchor: DynAnchor::Point(self.plane.to_world(*hit2)),
            fields: vec![DynFieldSpec::new(DynRole::Radius)],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        self.radius_from_point(cursor)
    }
}

// ── Command: Tan, Tan, Tan ────────────────────────────────────────────────

pub struct CircleTTTCommand {
    objs: Vec<TangentObject>,
    hits: Vec<DVec3>,
    plane: WorkingPlane,
}

impl CircleTTTCommand {
    pub fn new() -> Self {
        Self {
            objs: Vec::new(),
            hits: Vec::new(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for CircleTTTCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CIRCLE_TTT"
    }

    fn needs_tangent_pick(&self) -> bool {
        self.objs.len() < 3
    }

    fn prompt(&self) -> String {
        match self.objs.len() {
            0 => crate::t!("CIRCLE TTT  Select first tangent object:").into_owned(),
            1 => crate::t!("CIRCLE TTT  Select second tangent object:").into_owned(),
            _ => crate::t!("CIRCLE TTT  Select third tangent object:").into_owned(),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_tangent_point(&mut self, obj: TangentObject, hit: DVec3) -> CmdResult {
        self.objs.push(tangent_object_local(obj, self.plane));
        self.hits.push(self.plane.to_local(hit));
        if self.objs.len() < 3 {
            return CmdResult::NeedPoint;
        }
        let hint = self.hits.iter().fold(DVec3::ZERO, |a, &b| a + b) / 3.0;
        let candidates = ttt_candidates(self.objs[0], self.objs[1], self.objs[2]);
        match best_circle_of(&candidates, hint) {
            Some((center, r)) => {
                defaults::set_circle_radius(r);
                CmdResult::CommitAndExit(make_circle(
                    self.plane.to_world(center),
                    r,
                    self.plane,
                ))
            }
            None => {
                self.objs.pop();
                self.hits.pop();
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE_2P"] });
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE_3P"] });
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE_CD"] });
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE"] });
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE_TTR"] });
inventory::submit!(crate::command::CommandRegistration { names: &["CIRCLE_TTT"] });

#[cfg(test)]
mod tests {
    use super::*;

    fn has_candidate(cands: &[DVec3], want: DVec3) -> bool {
        cands.iter().any(|c| c.distance(want) < 1e-6)
    }

    /// #814: include circles that enclose both tangent objects.
    #[test]
    fn ttr_two_circles_offers_the_enclosing_solution() {
        let c1 = TangentObject::Circle {
            center: DVec3::new(-3.0, 0.0, 0.0),
            radius: 1.0,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(3.0, 0.0, 0.0),
            radius: 1.0,
        };
        let cands = ttr_candidates(c1, c2, 5.0);
        // |P - Ci| = 5 - 1 = 4 puts both small circles inside the new one.
        let y = 7.0f64.sqrt();
        assert!(
            has_candidate(&cands, DVec3::new(0.0, y, 0.0))
                && has_candidate(&cands, DVec3::new(0.0, -y, 0.0)),
            "no enclosing candidate, got {cands:?}"
        );
        // The externally tangent family is still produced.
        assert!(
            cands.iter().any(|c| {
                (c.distance(DVec3::new(-3.0, 0.0, 0.0)) - 6.0).abs() < 1e-6
                    && (c.distance(DVec3::new(3.0, 0.0, 0.0)) - 6.0).abs() < 1e-6
            }),
            "external tangency lost, got {cands:?}"
        );
    }

    /// Include line-circle solutions that enclose the circle.
    #[test]
    fn ttr_line_and_circle_offers_the_enclosing_solution() {
        let line = TangentObject::Line {
            p1: DVec3::ZERO,
            p2: DVec3::new(10.0, 0.0, 0.0),
        };
        let circle = TangentObject::Circle {
            center: DVec3::new(0.0, 2.0, 0.0),
            radius: 1.0,
        };
        let cands = ttr_candidates(line, circle, 5.0);
        // Tangent to y = 0 at distance 5, and |P - (0,2)| = 5 - 1 = 4.
        let x = 7.0f64.sqrt();
        assert!(
            has_candidate(&cands, DVec3::new(x, 5.0, 0.0))
                && has_candidate(&cands, DVec3::new(-x, 5.0, 0.0)),
            "no enclosing candidate, got {cands:?}"
        );
    }

    /// #318: solve a concentric ring with a line through its center.
    #[test]
    fn ttt_concentric_ring_with_line() {
        let inner = TangentObject::Circle { center: DVec3::ZERO, radius: 3.5 };
        let outer = TangentObject::Circle { center: DVec3::ZERO, radius: 5.0 };
        let dir = DVec3::new(0.35, 1.0, 0.0).normalize();
        let line = TangentObject::Line { p1: DVec3::ZERO, p2: dir * 10.0 };
        for order in [
            [inner, outer, line],
            [line, inner, outer],
            [outer, line, inner],
        ] {
            let cands = ttt_candidates(order[0], order[1], order[2]);
            let ok = cands.iter().any(|&(c, r)| {
                let d = (c.x * c.x + c.y * c.y).sqrt();
                (r - 0.75).abs() < 1e-6
                    && (d - 4.25).abs() < 1e-6
                    && (c.x * dir.y - c.y * dir.x).abs() - 0.75 < 1e-6
            });
            assert!(ok, "no annulus candidate, got {cands:?}");
        }
    }
}
