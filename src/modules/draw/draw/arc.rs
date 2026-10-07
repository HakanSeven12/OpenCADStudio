// Arc creation commands.

use codec::types::Vector3;
use codec::{Arc as CadArc, EntityType};
use crate::t;
use kernel::geom2d::{self, Curve as KernelCurve};

use crate::command::{CadCommand, CmdResult, TangentObject, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::wire_model::{TangentGeom, WireModel};
use super::apollonius::{self, Circle2D};
use glam::{DVec2, DVec3};

const TAU: f64 = std::f64::consts::TAU;

// ── Per-method SVG icons ───────────────────────────────────────────────────

const ICON_CSE: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_cse.svg"));
const ICON_3P: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_3p.svg"));
const ICON_SCE: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_sce.svg"));
const ICON_SCA: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_sca.svg"));
const ICON_SCL: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_scl.svg"));
const ICON_SEA: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_sea.svg"));
const ICON_SER: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_ser.svg"));
const ICON_SED: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_sed.svg"));
const ICON_CSA: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_csa.svg"));
const ICON_CSL: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_csl.svg"));
const ICON_CONT: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/arc/arc_cont.svg"));

// ── Dropdown metadata ──────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "ARC";

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("ARC_3P", "3-Point", ICON_3P),
    ("ARC_SCE", "Start, Center, End", ICON_SCE),
    ("ARC_SCA", "Start, Center, Angle", ICON_SCA),
    ("ARC_SCL", "Start, Center, Length", ICON_SCL),
    ("ARC_SEA", "Start, End, Angle", ICON_SEA),
    ("ARC_SED", "Start, End, Direction", ICON_SED),
    ("ARC_SER", "Start, End, Radius", ICON_SER),
    ("ARC_CSE", "Center, Start, End", ICON_CSE),
    ("ARC_CSA", "Center, Start, Angle", ICON_CSA),
    ("ARC_CSL", "Center, Start, Length", ICON_CSL),
    ("ARC_CONT", "Continue", ICON_CONT),
];

/// Default icon — falls back to 3-Point before first use.
pub const ICON: IconKind = ICON_3P;

// ── Shared math helpers ────────────────────────────────────────────────────

/// Angle in radians from `center` to `pt`.
fn angle_xy(center: DVec3, pt: DVec3, plane: WorkingPlane) -> f64 {
    plane.angle(center, pt).unwrap_or(0.0)
}

fn arc_preview(
    center: DVec3,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    plane: WorkingPlane,
) -> Option<WireModel> {
    let center_local = plane.to_local(center);
    let arc = geom2d::bounded_arc(
        [center_local.x, center_local.y],
        radius,
        start_angle,
        end_angle,
    )?;
    let points = KernelCurve::Arc(arc)
        .tessellate_angle(TAU / 64.0)
        .into_iter()
        .map(|point| plane.to_world(DVec3::new(point[0], point[1], center_local.z)).to_array())
        .collect();
    let mut wire = WireModel::solid_f64(
        "rubber_band".into(),
        points,
        WireModel::CYAN,
        false,
    );
    wire.tangent_geoms.push(TangentGeom::Arc {
        center: [center.x, center.y, center.z],
        axis_x: plane.x.to_array(),
        axis_y: plane.y.to_array(),
        radius: arc.radius,
        start_angle: arc.start_angle,
        end_angle: arc.end_angle,
    });
    Some(wire)
}

fn make_arc(
    center: DVec3,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    plane: WorkingPlane,
) -> Option<EntityType> {
    let center = plane.to_local(center);
    let arc = geom2d::bounded_arc(
        [center.x, center.y],
        radius,
        start_angle,
        end_angle,
    )?;
    Some(plane.place_entity(EntityType::Arc(CadArc {
        center: Vector3::new(center.x, center.y, center.z),
        radius: arc.radius,
        start_angle: arc.start_angle,
        end_angle: arc.end_angle,
        ..Default::default()
    })))
}

fn arc_result(
    center: DVec3,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    plane: WorkingPlane,
) -> CmdResult {
    make_arc(center, radius, start_angle, end_angle, plane)
        .map(CmdResult::CommitAndExit)
        .unwrap_or(CmdResult::NeedPoint)
}

fn line_wire(a: DVec3, b: DVec3) -> WireModel {
    WireModel::solid_f64(
        "rubber_band".into(),
        vec![[a.x, a.y, a.z], [b.x, b.y, b.z]],
        WireModel::CYAN,
        false,
    )
}

fn arc_through_points(
    a: DVec3,
    b: DVec3,
    c: DVec3,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let (a, b, c) = (plane.to_local(a), plane.to_local(b), plane.to_local(c));
    let arc = geom2d::arc_through_points([a.x, a.y], [b.x, b.y], [c.x, c.y])?;
    let center = plane.to_world(DVec3::new(arc.centre[0], arc.centre[1], a.z));
    Some((center, arc.radius, arc.start_angle, arc.end_angle))
}

/// Arc center+radius from two endpoints and a cursor (sagitta / bow-toward-cursor).
fn arc_from_sagitta(
    s: DVec3,
    e: DVec3,
    cursor: DVec3,
    flip_direction: bool,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let (s, e, cursor) = (plane.to_local(s), plane.to_local(e), plane.to_local(cursor));
    let chord_vec = e - s;
    let chord_len = (chord_vec.x * chord_vec.x + chord_vec.y * chord_vec.y).sqrt();
    if chord_len < 1e-6 {
        return None;
    }
    let unit_chord = DVec3::new(chord_vec.x / chord_len, chord_vec.y / chord_len, 0.0);
    let perp = DVec3::new(-unit_chord.y, unit_chord.x, 0.0);
    let mid = (s + e) * 0.5;
    let h = (cursor - mid).dot(perp); // signed sagitta
    if h.abs() < 1e-3 {
        return None;
    }
    let sagitta = if flip_direction { -h } else { h };
    let arc = geom2d::arc_from_sagitta([s.x, s.y], [e.x, e.y], sagitta)?;
    let center = plane.to_world(DVec3::new(arc.centre[0], arc.centre[1], s.z));
    Some((center, arc.radius, arc.start_angle, arc.end_angle))
}

/// Builds an arc from endpoints and a signed angle.
fn arc_from_endpoints_angle(
    s: DVec3,
    e: DVec3,
    included: f64,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let (s, e) = (plane.to_local(s), plane.to_local(e));
    let arc = geom2d::arc_from_endpoints_angle([s.x, s.y], [e.x, e.y], included)?;
    let center = plane.to_world(DVec3::new(arc.centre[0], arc.centre[1], s.z));
    Some((center, arc.radius, arc.start_angle, arc.end_angle))
}

/// Arc center+radius from start, end, and a radius-magnitude point (dist = dist(pt, start)).
fn arc_from_se_radius(
    s: DVec3,
    e: DVec3,
    radius_pt: DVec3,
    clockwise: bool,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let radius = plane.to_local(s).distance(plane.to_local(radius_pt));
    arc_from_endpoints_radius(s, e, radius, clockwise, plane)
}

fn arc_from_endpoints_radius(
    s: DVec3,
    e: DVec3,
    signed_radius: f64,
    clockwise: bool,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let (s, e) = if clockwise { (e, s) } else { (s, e) };
    let (s, e) = (plane.to_local(s), plane.to_local(e));
    let arc = geom2d::arc_from_endpoints_radius([s.x, s.y], [e.x, e.y], signed_radius)?;
    let center = plane.to_world(DVec3::new(arc.centre[0], arc.centre[1], s.z));
    Some((center, arc.radius, arc.start_angle, arc.end_angle))
}

/// Builds an arc tangent to the preceding entity.
fn arc_continue(
    s: DVec3,
    t: DVec3,
    e: DVec3,
    flip: bool,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    let (s, e) = (plane.to_local(s), plane.to_local(e));
    let tangent = plane.vector_to_local(t);
    let arc = geom2d::arc_from_start_tangent(
        [s.x, s.y],
        [tangent.x, tangent.y],
        [e.x, e.y],
        flip,
    )?;
    let center = plane.to_world(DVec3::new(arc.centre[0], arc.centre[1], s.z));
    Some((center, arc.radius, arc.start_angle, arc.end_angle))
}

/// Returns the last curve endpoint and outgoing tangent.
pub fn continue_anchor(entity: &EntityType, last: Option<DVec3>) -> Option<(DVec3, DVec3)> {
    if !matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline2D(_)
    ) {
        return None;
    }
    let curve = crate::entities::curve::entity_curve(entity)?;
    let start = DVec3::from_array(curve.point_at(0.0));
    let end = DVec3::from_array(curve.point_at(1.0));
    let use_end = last.map_or(true, |point| point.distance(end) <= point.distance(start));
    let (point, tangent) = if use_end {
        (end, DVec3::from_array(curve.tangent_at(1.0)))
    } else {
        (start, -DVec3::from_array(curve.tangent_at(0.0)))
    };
    let tangent = tangent.normalize_or_zero();
    (tangent.length_squared() > 1.0e-12).then_some((point, tangent))
}

/// Compute end_angle from a chord-length pick (SCL / CSL semantics).
/// Positive length selects the minor arc; negative length selects the major.
fn end_angle_from_chord_len(start_angle: f64, chord: f64, r: f64) -> Option<f64> {
    geom2d::arc_sweep_from_chord(r, chord).map(|sweep| start_angle + sweep)
}

/// An input point or a fluid tangent snap constraint for arc construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArcPick {
    Point(DVec3),
    Tangent {
        target: TangentObject,
        hit: DVec3,
    },
}

impl ArcPick {
    pub fn point(&self) -> DVec3 {
        match *self {
            ArcPick::Point(pt) => pt,
            ArcPick::Tangent { hit, .. } => hit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseGeom {
    pub center: DVec3,
    pub major_axis: DVec3,
    pub normal: DVec3,
    pub minor_axis_ratio: f64,
}

impl EllipseGeom {
    pub fn new(center: DVec3, major_axis: DVec3, normal: DVec3, minor_axis_ratio: f64) -> Self {
        Self {
            center,
            major_axis,
            normal,
            minor_axis_ratio,
        }
    }

    pub fn a(&self) -> f64 {
        self.major_axis.length()
    }

    pub fn b(&self) -> f64 {
        (self.a() * self.minor_axis_ratio).max(1e-9)
    }

    pub fn u_hat(&self) -> DVec3 {
        let a = self.a();
        if a > 1e-9 {
            self.major_axis / a
        } else {
            DVec3::X
        }
    }

    pub fn n_hat(&self) -> DVec3 {
        self.normal.normalize_or_zero()
    }

    pub fn v_hat(&self) -> DVec3 {
        self.n_hat().cross(self.u_hat()).normalize_or_zero()
    }

    pub fn param_at(&self, pt: DVec3) -> f64 {
        let d = pt - self.center;
        let u_proj = d.dot(self.u_hat());
        let v_proj = d.dot(self.v_hat());
        let a = self.a().max(1e-9);
        let b = self.b();
        (v_proj / b).atan2(u_proj / a)
    }

    pub fn point_at(&self, t: f64) -> DVec3 {
        self.center + self.u_hat() * (self.a() * t.cos()) + self.v_hat() * (self.b() * t.sin())
    }

    pub fn normal_at(&self, t: f64) -> DVec3 {
        let a = self.a();
        let b = self.b();
        (self.u_hat() * (b * t.cos()) + self.v_hat() * (a * t.sin())).normalize_or_zero()
    }

    pub fn osculating_circle_at(&self, t: f64) -> (DVec3, f64) {
        let a = self.a();
        let b = self.b();
        let pt = self.point_at(t);
        let n = self.normal_at(t);
        let num = (a * a * t.sin() * t.sin() + b * b * t.cos() * t.cos()).powf(1.5);
        let rho = (num / (a * b)).clamp(1e-3, 1e7);
        let osc_center = pt - n * rho;
        (osc_center, rho)
    }

    pub fn project_point(&self, pt: DVec3) -> DVec3 {
        let t = self.param_at(pt);
        self.point_at(t)
    }
}

fn fluid_anchor_point(pick: ArcPick, cursor: DVec3, plane: WorkingPlane) -> DVec3 {
    match pick {
        ArcPick::Point(pt) => pt,
        ArcPick::Tangent { target, hit } => match target {
            TangentObject::Circle { center, radius } => {
                let loc_center = plane.to_local(center);
                let loc_pt = plane.to_local(cursor);
                let d = (DVec2::new(loc_pt.x, loc_pt.y) - DVec2::new(loc_center.x, loc_center.y)).length();
                if d > 1e-6 {
                    let dir = (DVec2::new(loc_pt.x, loc_pt.y) - DVec2::new(loc_center.x, loc_center.y)).normalize();
                    plane.to_world(DVec3::new(loc_center.x + dir.x * radius, loc_center.y + dir.y * radius, loc_center.z))
                } else {
                    hit
                }
            }
            TangentObject::Ellipse {
                center,
                major_axis,
                normal,
                minor_axis_ratio,
            } => {
                let ell = EllipseGeom::new(center, major_axis, normal, minor_axis_ratio);
                ell.project_point(cursor)
            }
            _ => hit,
        },
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ellipse2D {
    pub center: DVec2,
    pub major_dir: DVec2,
    pub minor_dir: DVec2,
    pub a: f64,
    pub b: f64,
}

impl Ellipse2D {
    pub fn new(center: DVec2, major_axis: DVec2, normal_z: f64, minor_ratio: f64) -> Self {
        let a = major_axis.length().max(1e-9);
        let b = (a * minor_ratio).max(1e-9);
        let major_dir = major_axis / a;
        let sign = if normal_z < 0.0 { -1.0 } else { 1.0 };
        let minor_dir = DVec2::new(-major_dir.y, major_dir.x) * sign;
        Self {
            center,
            major_dir,
            minor_dir,
            a,
            b,
        }
    }

    pub fn point_at(&self, t: f64) -> DVec2 {
        self.center + self.major_dir * (self.a * t.cos()) + self.minor_dir * (self.b * t.sin())
    }

    pub fn normal_at(&self, t: f64) -> DVec2 {
        (self.major_dir * (self.b * t.cos()) + self.minor_dir * (self.a * t.sin())).normalize_or_zero()
    }

    pub fn param_at(&self, pt: DVec2) -> f64 {
        let d = pt - self.center;
        let u_proj = d.dot(self.major_dir);
        let v_proj = d.dot(self.minor_dir);
        (v_proj / self.b).atan2(u_proj / self.a)
    }

    pub fn project_point(&self, pt: DVec2) -> DVec2 {
        let t = self.param_at(pt);
        self.point_at(t)
    }

    /// Returns all points on the ellipse whose normal lines pass through `center`.
    /// For any such point P, (P - center) is collinear with the ellipse normal at P.
    pub fn normal_points_for_center(&self, center: DVec2) -> Vec<DVec2> {
        let d = center - self.center;
        let uc = d.dot(self.major_dir);
        let vc = d.dot(self.minor_dir);
        let a = self.a;
        let b = self.b;
        let a2_b2 = a * a - b * b;

        // Normal equation: g(t) = (a^2 - b^2) * sin(t)*cos(t) - a*uc*sin(t) + b*vc*cos(t) = 0
        let g = |t: f64| -> f64 {
            0.5 * a2_b2 * (2.0 * t).sin() - a * uc * t.sin() + b * vc * t.cos()
        };
        let g_prime = |t: f64| -> f64 {
            a2_b2 * (2.0 * t).cos() - a * uc * t.cos() - b * vc * t.sin()
        };

        let n_slices = 16;
        let dt = std::f64::consts::TAU / (n_slices as f64);
        let mut roots = Vec::with_capacity(4);

        for i in 0..n_slices {
            let t0 = i as f64 * dt;
            let t1 = (i + 1) as f64 * dt;
            let g0 = g(t0);
            let g1 = g(t1);

            if g0.abs() < 1e-12 {
                let norm = t0.rem_euclid(std::f64::consts::TAU);
                if !roots.iter().any(|&r: &f64| (r - norm).abs() < 1e-4) {
                    roots.push(norm);
                }
                continue;
            }

            if g0.signum() != g1.signum() {
                let mut lo = t0;
                let mut hi = t1;
                for _ in 0..8 {
                    let mid = 0.5 * (lo + hi);
                    let gm = g(mid);
                    if gm.signum() == g0.signum() {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let mut root = 0.5 * (lo + hi);
                for _ in 0..6 {
                    let val = g(root);
                    let der = g_prime(root);
                    if der.abs() > 1e-12 {
                        let step = val / der;
                        root -= step;
                        if step.abs() < 1e-12 {
                            break;
                        }
                    }
                }
                let norm = root.rem_euclid(std::f64::consts::TAU);
                if !roots.iter().any(|&r: &f64| (r - norm).abs() < 1e-4) {
                    roots.push(norm);
                }
            }
        }

        // Also check Newton iteration starting from radial guess
        let t_guess = (vc / b).atan2(uc / a);
        let mut root_guess = t_guess;
        for _ in 0..6 {
            let val = g(root_guess);
            let der = g_prime(root_guess);
            if der.abs() > 1e-12 {
                let step = val / der;
                root_guess -= step;
                if step.abs() < 1e-12 {
                    break;
                }
            }
        }
        if g(root_guess).abs() < 1e-6 {
            let norm = root_guess.rem_euclid(std::f64::consts::TAU);
            if !roots.iter().any(|&r: &f64| (r - norm).abs() < 1e-4) {
                roots.push(norm);
            }
        }

        roots.into_iter().map(|t| self.point_at(t)).collect()
    }
}

fn target_to_ellipse2d(target: TangentObject, plane: WorkingPlane) -> Option<Ellipse2D> {
    match target {
        TangentObject::Ellipse {
            center,
            major_axis,
            normal,
            minor_axis_ratio,
        } => {
            let loc_center = plane.to_local(center);
            let loc_major = plane.vector_to_local(major_axis);
            let loc_normal = plane.vector_to_local(normal);
            Some(Ellipse2D::new(
                DVec2::new(loc_center.x, loc_center.y),
                DVec2::new(loc_major.x, loc_major.y),
                loc_normal.z,
                minor_axis_ratio,
            ))
        }
        _ => None,
    }
}

fn target_to_circle2d_pure(target: TangentObject, plane: WorkingPlane) -> Option<Circle2D> {
    match target {
        TangentObject::Circle { center, radius } => {
            let loc = plane.to_local(center);
            Some(Circle2D {
                center: DVec2::new(loc.x, loc.y),
                radius,
            })
        }
        _ => None,
    }
}

/// Solves PPC (Point, Point, Curve) where Curve is an Ellipse.
/// Scans the full perimeter t ∈ [0, 2π) to find all valid tangent circles passing through p1 and p2,
/// and selects the one closest to `hint`.
pub fn solve_ellipse_ppc(
    ell: &Ellipse2D,
    p1: DVec2,
    p2: DVec2,
    hint: DVec2,
) -> Option<DVec2> {
    let d_vec = p2 - p1;
    let d_len = d_vec.length();
    if d_len < 1e-9 {
        return None;
    }
    let m = (p1 + p2) * 0.5;
    let n_bisect = DVec2::new(-d_vec.y, d_vec.x) / d_len;

    let eval_t = |t: f64| -> Option<(f64, DVec2, f64, DVec2)> {
        let pt = ell.point_at(t);
        let n_hat = ell.normal_at(t);
        let delta = m - pt;
        let det = n_hat.y * n_bisect.x - n_hat.x * n_bisect.y;
        if det.abs() < 1e-6 {
            return None;
        }
        let lambda = (-delta.x * n_bisect.y + delta.y * n_bisect.x) / det;
        let center = pt + n_hat * lambda;
        let r_p = (center - p1).length();
        let r_t = lambda.abs();
        let res = r_t - r_p;
        Some((res, pt, lambda.abs(), center))
    };

    let n_steps = 144;
    let dt = std::f64::consts::TAU / (n_steps as f64);
    let mut candidates = Vec::new();

    for i in 0..n_steps {
        let t_a = i as f64 * dt;
        let t_b = (i + 1) as f64 * dt;

        let Some((res_a, _, _, _)) = eval_t(t_a) else { continue; };
        let Some((res_b, _, _, _)) = eval_t(t_b) else { continue; };

        let n_hat_a = ell.normal_at(t_a);
        let det_a = n_hat_a.y * n_bisect.x - n_hat_a.x * n_bisect.y;
        let n_hat_b = ell.normal_at(t_b);
        let det_b = n_hat_b.y * n_bisect.x - n_hat_b.x * n_bisect.y;
        if det_a.signum() != det_b.signum() {
            continue;
        }

        if res_a.signum() != res_b.signum() {
            let mut lo = t_a;
            let mut hi = t_b;
            let mut best_t = (lo + hi) * 0.5;
            for _ in 0..16 {
                let mid = (lo + hi) * 0.5;
                let Some((res_m, _, _, _)) = eval_t(mid) else { break; };
                best_t = mid;
                if res_m.abs() < 1e-8 {
                    break;
                }
                if res_m.signum() == res_a.signum() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if let Some((res, pt, r, center)) = eval_t(best_t) {
                if res.abs() < 1e-3 && r.is_finite() && r > 1e-4 {
                    candidates.push((pt, r, center));
                }
            }
        }
    }

    candidates
        .into_iter()
        .min_by(|(pt_a, r_a, _), (pt_b, r_b, _)| {
            let dist_a = (*pt_a - hint).length_squared() + if *r_a > 50000.0 { 1e8 } else { 0.0 };
            let dist_b = (*pt_b - hint).length_squared() + if *r_b > 50000.0 { 1e8 } else { 0.0 };
            dist_a.total_cmp(&dist_b)
        })
        .map(|(pt, _, _)| pt)
}

/// Solves CCP (Curve, Curve, Point) where Curve 1 is an Ellipse and Curve 2 is a Circle.
pub fn solve_ellipse_circle_ccp(
    ell: &Ellipse2D,
    c2: Circle2D,
    p: DVec2,
    hint_ell: DVec2,
    hint_c2: DVec2,
) -> Option<(DVec2, DVec2)> {
    let n_steps = 144;
    let dt = std::f64::consts::TAU / (n_steps as f64);
    let mut candidates = Vec::new();

    let eval_t = |t: f64| -> Option<(DVec2, DVec2, f64, f64, f64)> {
        let pt = ell.point_at(t);
        let n_hat = ell.normal_at(t);
        let w = pt - p;
        let w_dot_n = w.dot(n_hat);
        if w_dot_n.abs() < 1e-6 {
            return None;
        }
        let lambda = -w.length_squared() / (2.0 * w_dot_n);
        let r = lambda.abs();
        if r < 1e-4 || r > 1e7 {
            return None;
        }
        let center = pt + n_hat * lambda;
        let d2 = (center - c2.center).length();
        let res_ext = d2 - (r + c2.radius);
        let res_int = d2 - (r - c2.radius).abs();
        Some((pt, center, r, res_ext, res_int))
    };

    let mut check_mode = |is_ext: bool| {
        for i in 0..n_steps {
            let t_a = i as f64 * dt;
            let t_b = (i + 1) as f64 * dt;

            let Some((_, _, _, res_ext_a, res_int_a)) = eval_t(t_a) else { continue; };
            let Some((_, _, _, res_ext_b, res_int_b)) = eval_t(t_b) else { continue; };

            let pt_a = ell.point_at(t_a);
            let n_hat_a = ell.normal_at(t_a);
            let w_a = pt_a - p;
            let pt_b = ell.point_at(t_b);
            let n_hat_b = ell.normal_at(t_b);
            let w_b = pt_b - p;
            if (w_a.dot(n_hat_a)).signum() != (w_b.dot(n_hat_b)).signum() {
                continue;
            }

            let (ra, rb) = if is_ext { (res_ext_a, res_ext_b) } else { (res_int_a, res_int_b) };
            if ra.signum() != rb.signum() {
                let mut lo = t_a;
                let mut hi = t_b;
                let mut best_t = (lo + hi) * 0.5;
                for _ in 0..16 {
                    let mid = (lo + hi) * 0.5;
                    let Some((_, _, _, res_ext_m, res_int_m)) = eval_t(mid) else { break; };
                    let rm = if is_ext { res_ext_m } else { res_int_m };
                    best_t = mid;
                    if rm.abs() < 1e-8 {
                        break;
                    }
                    if rm.signum() == ra.signum() {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                if let Some((pt1, center, r, res_ext_final, res_int_final)) = eval_t(best_t) {
                    let res_final = if is_ext { res_ext_final } else { res_int_final };
                    if res_final.abs() < 1e-3 {
                        let d2_vec = center - c2.center;
                        let d2_len = d2_vec.length();
                        if d2_len > 1e-9 {
                            let dir = d2_vec / d2_len;
                            let pt2 = if is_ext {
                                c2.center + dir * c2.radius
                            } else if r > c2.radius {
                                c2.center - dir * c2.radius
                            } else {
                                c2.center + dir * c2.radius
                            };
                            candidates.push((pt1, pt2, r));
                        }
                    }
                }
            }
        }
    };

    check_mode(true);
    check_mode(false);

    candidates
        .into_iter()
        .min_by(|(pt1_a, pt2_a, r_a), (pt1_b, pt2_b, r_b)| {
            let dist_a = (*pt1_a - hint_ell).length_squared() + (*pt2_a - hint_c2).length_squared() + if *r_a > 50000.0 { 1e8 } else { 0.0 };
            let dist_b = (*pt1_b - hint_ell).length_squared() + (*pt2_b - hint_c2).length_squared() + if *r_b > 50000.0 { 1e8 } else { 0.0 };
            dist_a.total_cmp(&dist_b)
        })
        .map(|(pt1, pt2, _)| (pt1, pt2))
}

/// Solves CCP (Curve, Curve, Point) where both Curves are Ellipses.
pub fn solve_ellipse_ellipse_ccp(
    ell1: &Ellipse2D,
    ell2: &Ellipse2D,
    p: DVec2,
    hint1: DVec2,
    hint2: DVec2,
) -> Option<(DVec2, DVec2)> {
    let n_steps = 144;
    let dt = std::f64::consts::TAU / (n_steps as f64);
    let mut candidates = Vec::new();

    let eval_t = |t: f64| -> Option<(DVec2, DVec2, f64, f64)> {
        let pt1 = ell1.point_at(t);
        let n_hat = ell1.normal_at(t);
        let w = pt1 - p;
        let w_dot_n = w.dot(n_hat);
        if w_dot_n.abs() < 1e-6 {
            return None;
        }
        let lambda = -w.length_squared() / (2.0 * w_dot_n);
        let r = lambda.abs();
        if r < 1e-4 || r > 1e7 {
            return None;
        }
        let center = pt1 + n_hat * lambda;
        let normal_pts = ell2.normal_points_for_center(center);
        let pt2 = normal_pts
            .into_iter()
            .min_by(|a, b| (*a - hint2).length_squared().total_cmp(&(*b - hint2).length_squared()))?;
        let d2 = (center - pt2).length();
        let res = d2 - r;
        Some((pt1, pt2, r, res))
    };

    for i in 0..n_steps {
        let t_a = i as f64 * dt;
        let t_b = (i + 1) as f64 * dt;

        let Some((_, _, _, res_a)) = eval_t(t_a) else { continue; };
        let Some((_, _, _, res_b)) = eval_t(t_b) else { continue; };

        let pt_a = ell1.point_at(t_a);
        let n_hat_a = ell1.normal_at(t_a);
        let w_a = pt_a - p;
        let pt_b = ell1.point_at(t_b);
        let n_hat_b = ell1.normal_at(t_b);
        let w_b = pt_b - p;
        if (w_a.dot(n_hat_a)).signum() != (w_b.dot(n_hat_b)).signum() {
            continue;
        }

        if res_a.signum() != res_b.signum() {
            let mut lo = t_a;
            let mut hi = t_b;
            let mut best_t = (lo + hi) * 0.5;
            for _ in 0..16 {
                let mid = (lo + hi) * 0.5;
                let Some((_, _, _, res_m)) = eval_t(mid) else { break; };
                best_t = mid;
                if res_m.abs() < 1e-8 {
                    break;
                }
                if res_m.signum() == res_a.signum() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if let Some((pt1, pt2, r, res_final)) = eval_t(best_t) {
                if res_final.abs() < 1e-3 {
                    candidates.push((pt1, pt2, r));
                }
            }
        }
    }

    candidates
        .into_iter()
        .min_by(|(pt1_a, pt2_a, r_a), (pt1_b, pt2_b, r_b)| {
            let dist_a = (*pt1_a - hint1).length_squared() + (*pt2_a - hint2).length_squared() + if *r_a > 50000.0 { 1e8 } else { 0.0 };
            let dist_b = (*pt1_b - hint1).length_squared() + (*pt2_b - hint2).length_squared() + if *r_b > 50000.0 { 1e8 } else { 0.0 };
            dist_a.total_cmp(&dist_b)
        })
        .map(|(pt1, pt2, _)| (pt1, pt2))
}

fn solve_tangent_ccp(
    t1: TangentObject,
    h1: DVec3,
    t2: TangentObject,
    h2: DVec3,
    mid: DVec3,
    plane: WorkingPlane,
) -> Option<(DVec3, DVec3)> {
    let z = plane.to_local(mid).z;
    let p = DVec2::new(plane.to_local(mid).x, plane.to_local(mid).y);
    let hint1 = DVec2::new(plane.to_local(h1).x, plane.to_local(h1).y);
    let hint2 = DVec2::new(plane.to_local(h2).x, plane.to_local(h2).y);

    let ell1 = target_to_ellipse2d(t1, plane);
    let ell2 = target_to_ellipse2d(t2, plane);
    let c1 = target_to_circle2d_pure(t1, plane);
    let c2 = target_to_circle2d_pure(t2, plane);

    if let (Some(ell1), Some(c2)) = (ell1, c2) {
        let (w1, w2) = solve_ellipse_circle_ccp(&ell1, c2, p, hint1, hint2)?;
        return Some((
            plane.to_world(DVec3::new(w1.x, w1.y, z)),
            plane.to_world(DVec3::new(w2.x, w2.y, z)),
        ));
    }

    if let (Some(c1), Some(ell2)) = (c1, ell2) {
        let (w2, w1) = solve_ellipse_circle_ccp(&ell2, c1, p, hint2, hint1)?;
        return Some((
            plane.to_world(DVec3::new(w1.x, w1.y, z)),
            plane.to_world(DVec3::new(w2.x, w2.y, z)),
        ));
    }

    if let (Some(ell1), Some(ell2)) = (ell1, ell2) {
        let (w1, w2) = solve_ellipse_ellipse_ccp(&ell1, &ell2, p, hint1, hint2)?;
        return Some((
            plane.to_world(DVec3::new(w1.x, w1.y, z)),
            plane.to_world(DVec3::new(w2.x, w2.y, z)),
        ));
    }

    if let (Some(c1), Some(c2)) = (c1, c2) {
        let sols = apollonius::solve_ccp(c1, c2, p);
        if let Some(best) = apollonius::best_ccp(&sols, hint1, hint2) {
            let w1 = plane.to_world(DVec3::new(best.tangent1.x, best.tangent1.y, z));
            let tan2 = best.tangent2.unwrap_or(best.tangent1);
            let w2 = plane.to_world(DVec3::new(tan2.x, tan2.y, z));
            return Some((w1, w2));
        }
    }

    None
}

fn solve_tangent_ppc(
    t1: TangentObject,
    h1: DVec3,
    p1: DVec3,
    p2: DVec3,
    plane: WorkingPlane,
) -> Option<DVec3> {
    let z = plane.to_local(p1).z;
    let pt1 = DVec2::new(plane.to_local(p1).x, plane.to_local(p1).y);
    let pt2 = DVec2::new(plane.to_local(p2).x, plane.to_local(p2).y);
    let hint1 = DVec2::new(plane.to_local(h1).x, plane.to_local(h1).y);

    if let Some(ell) = target_to_ellipse2d(t1, plane) {
        if let Some(w1_2d) = solve_ellipse_ppc(&ell, pt1, pt2, hint1) {
            return Some(plane.to_world(DVec3::new(w1_2d.x, w1_2d.y, z)));
        }
        return None;
    }

    if let Some(c1) = target_to_circle2d_pure(t1, plane) {
        let sols = apollonius::solve_ppc(c1, pt1, pt2);
        if let Some(best) = apollonius::best_ppc(&sols, hint1) {
            return Some(plane.to_world(DVec3::new(best.tangent1.x, best.tangent1.y, z)));
        }
    }

    None
}

/// Solves an arc from 3 inputs, solving fluid tangencies (Apollonius CCP / PPC)
/// when one or more inputs are Tangent constraints.
pub(crate) fn solve_arc_3_inputs(
    p1: ArcPick,
    p2: ArcPick,
    p3: ArcPick,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64)> {
    match (p1, p2, p3) {
        // Case 1: Two tangents and one point (Apollonius CCP)
        // (Tangent, Point, Tangent) -> bridging arc through middle point!
        (
            ArcPick::Tangent { target: t1, hit: h1 },
            ArcPick::Point(mid),
            ArcPick::Tangent { target: t2, hit: h2 },
        ) => {
            if let Some((w1, w2)) = solve_tangent_ccp(t1, h1, t2, h2, mid, plane) {
                return arc_through_points(w1, mid, w2, plane);
            }
        }
        // (Tangent, Tangent, Point)
        (
            ArcPick::Tangent { target: t1, hit: h1 },
            ArcPick::Tangent { target: t2, hit: h2 },
            ArcPick::Point(end),
        ) => {
            if let Some((w1, w2)) = solve_tangent_ccp(t1, h1, t2, h2, end, plane) {
                return arc_through_points(w1, end, w2, plane);
            }
        }
        // (Point, Tangent, Tangent)
        (
            ArcPick::Point(start),
            ArcPick::Tangent { target: t1, hit: h1 },
            ArcPick::Tangent { target: t2, hit: h2 },
        ) => {
            if let Some((w1, w2)) = solve_tangent_ccp(t1, h1, t2, h2, start, plane) {
                return arc_through_points(w1, start, w2, plane);
            }
        }

        // Case 2: One tangent and two points (Apollonius PPC)
        // (Tangent, Point, Point)
        (
            ArcPick::Tangent { target: t1, hit: h1 },
            ArcPick::Point(mid),
            ArcPick::Point(end),
        ) => {
            if let Some(w1) = solve_tangent_ppc(t1, h1, mid, end, plane) {
                return arc_through_points(w1, mid, end, plane);
            }
        }
        // (Point, Point, Tangent)
        (
            ArcPick::Point(start),
            ArcPick::Point(mid),
            ArcPick::Tangent { target: t1, hit: h1 },
        ) => {
            if let Some(w1) = solve_tangent_ppc(t1, h1, start, mid, plane) {
                return arc_through_points(start, mid, w1, plane);
            }
        }
        // (Point, Tangent, Point)
        (
            ArcPick::Point(start),
            ArcPick::Tangent { target: t1, hit: h1 },
            ArcPick::Point(end),
        ) => {
            if let Some(w1) = solve_tangent_ppc(t1, h1, start, end, plane) {
                return arc_through_points(start, w1, end, plane);
            }
        }

        _ => {}
    }

    let has_tangent = matches!(p1, ArcPick::Tangent { .. })
        || matches!(p2, ArcPick::Tangent { .. })
        || matches!(p3, ArcPick::Tangent { .. });
    if has_tangent {
        None
    } else {
        arc_through_points(p1.point(), p2.point(), p3.point(), plane)
    }
}

// ── Command 1: Center, Start, End ─────────────────────────────────────────

pub struct ArcCSECommand {
    step: u8,
    c: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcCSECommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            c: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcCSECommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_CSE"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC  Specify center:").into_owned(),
            1 => t!("ARC  Specify start point:").into_owned(),
            _ => {
                let cx = format!("{:.2}", self.c.x);
                let cy = format!("{:.2}", self.c.y);
                let r = format!("{:.3}", self.r);
                t!(
                    "ARC  Specify end point  [c=(%{cx},%{cy}) r=%{r}]:",
                    cx = cx,
                    cy = cy,
                    r = r
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        // Alternate construction methods, offered only at the first (center)
        // step. Each keyword hands off to the dedicated variant command.
        if self.step == 0 {
            vec![
                CmdOption::new("SCE", "SCE"),
                CmdOption::new("SCA", "SCA"),
                CmdOption::new("SEA", "SEA"),
                CmdOption::new("SER", "SER"),
                CmdOption::new("CSA", "CSA"),
                CmdOption::new("3P", "3P"),
            ]
        } else {
            vec![]
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == 0
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.c = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.r = self.c.distance(pt);
                self.sa = angle_xy(self.c, pt, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // At the center step, keyword options switch construction method by
        // handing off to the dedicated variant command.
        if self.step == 0 {
            return match text.trim().to_uppercase().as_str() {
                "SCE" => Some(CmdResult::Dispatch("ARC_SCE".into())),
                "SCA" => Some(CmdResult::Dispatch("ARC_SCA".into())),
                "SEA" => Some(CmdResult::Dispatch("ARC_SEA".into())),
                "SER" => Some(CmdResult::Dispatch("ARC_SER".into())),
                "CSA" => Some(CmdResult::Dispatch("ARC_CSA".into())),
                "3P" => Some(CmdResult::Dispatch("ARC_3P".into())),
                _ => None,
            };
        }
        None
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.c, pt)),
            2 => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}

// ── ARC: compatible prompt flow ───────────────────────────────────────────
//
// The plain ARC command follows the prompts of commercial solutions, so every keyword a user
// knows is there at the same step:
//   Specify start point of arc or [Center]
//   Specify second point of arc or [Center/End]
//   Specify end point of arc
//   … after Center: Specify end point of arc or [Angle/chord Length]
//   … after End:    Specify center point of arc or [Angle/Direction/Radius]
// Enter at the start prompt continues tangent from the last line/arc.
// Holding Ctrl flips the sweep direction where it is not fixed by the picks.

#[derive(Clone, Copy, Debug, PartialEq)]
enum ArcStep {
    Start,
    Second { s: ArcPick },
    End3 { s: ArcPick, m: ArcPick },
    CenterFirst,
    StartAfterCenter { c: DVec3 },
    CenterAfterStart { s: DVec3 },
    /// Start and centre known: end point [Angle / chord Length].
    EndSC { s: DVec3, c: DVec3 },
    AngleSC { s: DVec3, c: DVec3 },
    LengthSC { s: DVec3, c: DVec3 },
    EndAfterStart { s: DVec3 },
    /// Start and end known: centre point [Angle / Direction / Radius].
    CenterSE { s: DVec3, e: DVec3 },
    AngleSE { s: DVec3, e: DVec3 },
    DirectionSE { s: DVec3, e: DVec3 },
    RadiusSE { s: DVec3, e: DVec3 },
}

pub struct ArcCommand {
    step: ArcStep,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcCommand {
    pub fn new() -> Self {
        Self {
            step: ArcStep::Start,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }

    /// Arc about `c` from `s` towards the direction of `e` (Ctrl flips).
    fn sc_end(&self, s: DVec3, c: DVec3, e: DVec3) -> Option<(DVec3, f64, f64, f64)> {
        let r = c.distance(s);
        if r < 1e-9 {
            return None;
        }
        let sa = angle_xy(c, s, self.plane);
        let ea = angle_xy(c, e, self.plane);
        Some(if self.cw { (c, r, ea, sa) } else { (c, r, sa, ea) })
    }

    /// Arc about `c` from `s` sweeping the signed `span` (radians, CCW > 0).
    fn sc_angle(&self, s: DVec3, c: DVec3, span: f64) -> Option<(DVec3, f64, f64, f64)> {
        let r = c.distance(s);
        if r < 1e-9 || span.abs() < 1e-9 {
            return None;
        }
        let sa = angle_xy(c, s, self.plane);
        let ea = sa + span;
        Some(if span < 0.0 { (c, r, ea, sa) } else { (c, r, sa, ea) })
    }

    /// Arc about `c` from `s` with chord `len` (Ctrl flips).
    fn sc_length(&self, s: DVec3, c: DVec3, len: f64) -> Option<(DVec3, f64, f64, f64)> {
        let r = c.distance(s);
        if r < 1e-9 || len == 0.0 {
            return None;
        }
        let sa = angle_xy(c, s, self.plane);
        let ea = end_angle_from_chord_len(sa, len, r)?;
        Some(if self.cw { (c, r, ea, sa) } else { (c, r, sa, ea) })
    }

    /// Sweep from `s` to the cursor about `c`, going the way Ctrl says.
    fn cursor_span(&self, s: DVec3, c: DVec3, cursor: DVec3) -> f64 {
        let span = (angle_xy(c, cursor, self.plane) - angle_xy(c, s, self.plane)).rem_euclid(TAU);
        if self.cw {
            span - TAU
        } else {
            span
        }
    }

    fn typed_number(text: &str) -> Option<f64> {
        crate::entities::common::parse_typed_length(text.trim())
    }

    fn typed_angle(text: &str) -> Option<f64> {
        crate::entities::common::parse_typed_angle(text.trim())
    }

    fn commit(&self, arc: Option<(DVec3, f64, f64, f64)>) -> CmdResult {
        match arc {
            Some((c, r, sa, ea)) => arc_result(c, r, sa, ea, self.plane),
            None => CmdResult::NeedPoint,
        }
    }

    fn preview(&self, arc: Option<(DVec3, f64, f64, f64)>, fallback: WireModel) -> Option<WireModel> {
        match arc {
            Some((c, r, sa, ea)) => arc_preview(c, r, sa, ea, self.plane).or(Some(fallback)),
            None => Some(fallback),
        }
    }
}

impl CadCommand for ArcCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC"
    }

    fn prompt(&self) -> String {
        match self.step {
            ArcStep::Start => t!("ARC  Specify start point of arc:"),
            ArcStep::Second { .. } => t!("ARC  Specify second point of arc:"),
            ArcStep::End3 { .. } | ArcStep::EndSC { .. } | ArcStep::EndAfterStart { .. } => {
                t!("ARC  Specify end point of arc:")
            }
            ArcStep::CenterFirst | ArcStep::CenterAfterStart { .. } | ArcStep::CenterSE { .. } => {
                t!("ARC  Specify center point of arc:")
            }
            ArcStep::StartAfterCenter { .. } => t!("ARC  Specify start point of arc:"),
            ArcStep::AngleSC { .. } | ArcStep::AngleSE { .. } => t!("ARC  Specify included angle:"),
            ArcStep::LengthSC { .. } => t!("ARC  Specify length of chord:"),
            ArcStep::DirectionSE { .. } => {
                t!("ARC  Specify tangent direction for the start point of arc:")
            }
            ArcStep::RadiusSE { .. } => t!("ARC  Specify radius of arc:"),
        }
        .into_owned()
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            ArcStep::Start => vec![CmdOption::new("Center", "C")],
            ArcStep::Second { .. } => vec![CmdOption::new("Center", "C"), CmdOption::new("End", "E")],
            ArcStep::EndSC { .. } => vec![
                CmdOption::new("Angle", "A"),
                CmdOption::new("chord Length", "L"),
            ],
            ArcStep::CenterSE { .. } => vec![
                CmdOption::new("Angle", "A"),
                CmdOption::new("Direction", "D"),
                CmdOption::new("Radius", "R"),
            ],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(
            self.step,
            ArcStep::End3 { .. }
                | ArcStep::CenterFirst
                | ArcStep::StartAfterCenter { .. }
                | ArcStep::CenterAfterStart { .. }
                | ArcStep::EndAfterStart { .. }
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            ArcStep::Start | ArcStep::Second { .. } | ArcStep::EndSC { .. } | ArcStep::CenterSE { .. }
        )
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.on_point_with_tangent(pt, None).unwrap_or(CmdResult::NeedPoint)
    }

    fn on_point_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Option<CmdResult> {
        let pick = match tangent {
            Some(target) => ArcPick::Tangent { target, hit: pt },
            None => ArcPick::Point(pt),
        };
        match self.step {
            ArcStep::Start => {
                self.step = ArcStep::Second { s: pick };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::Second { s } => {
                self.step = ArcStep::End3 { s, m: pick };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::End3 { s, m } => match solve_arc_3_inputs(s, m, pick, self.plane) {
                Some((c, r, sa, ea)) => Some(arc_result(c, r, sa, ea, self.plane)),
                None => Some(CmdResult::NeedPoint),
            },
            ArcStep::CenterFirst => {
                self.step = ArcStep::StartAfterCenter { c: pt };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::StartAfterCenter { c } => {
                self.step = ArcStep::EndSC { s: pt, c };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::CenterAfterStart { s } => {
                self.step = ArcStep::EndSC { s, c: pt };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::EndSC { s, c } => Some(self.commit(self.sc_end(s, c, pt))),
            // At a typed prompt a pick reads as commercial solutions do: the cursor's
            // direction from the centre (angle) or its distance (length,
            // radius), the cursor itself for a direction / sagitta.
            ArcStep::AngleSC { s, c } => {
                let span = self.cursor_span(s, c, pt);
                Some(self.commit(self.sc_angle(s, c, span)))
            }
            ArcStep::LengthSC { s, c } => Some(self.commit(self.sc_length(s, c, s.distance(pt)))),
            ArcStep::EndAfterStart { s } => {
                self.step = ArcStep::CenterSE { s, e: pt };
                Some(CmdResult::NeedPoint)
            }
            ArcStep::CenterSE { s, e } => Some(self.commit(self.sc_end(s, pt, e))),
            ArcStep::AngleSE { s, e } => {
                Some(self.commit(arc_from_sagitta(s, e, pt, self.cw, self.plane)))
            }
            ArcStep::DirectionSE { s, e } => {
                Some(self.commit(arc_continue(s, pt - s, e, self.cw, self.plane)))
            }
            ArcStep::RadiusSE { s, e } => {
                Some(self.commit(arc_from_se_radius(s, e, pt, self.cw, self.plane)))
            }
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        match self.step {
            ArcStep::Start => match upper.as_str() {
                "C" | "CE" | "CENTER" | "CENTRE" => {
                    self.step = ArcStep::CenterFirst;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            ArcStep::Second { s } => match upper.as_str() {
                "C" | "CE" | "CENTER" | "CENTRE" => {
                    self.step = ArcStep::CenterAfterStart { s: s.point() };
                    Some(CmdResult::NeedPoint)
                }
                "E" | "END" => {
                    self.step = ArcStep::EndAfterStart { s: s.point() };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            ArcStep::EndSC { s, c } => match upper.as_str() {
                "A" | "ANGLE" => {
                    self.step = ArcStep::AngleSC { s, c };
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LENGTH" => {
                    self.step = ArcStep::LengthSC { s, c };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            ArcStep::CenterSE { s, e } => match upper.as_str() {
                "A" | "ANGLE" => {
                    self.step = ArcStep::AngleSE { s, e };
                    Some(CmdResult::NeedPoint)
                }
                "D" | "DIRECTION" => {
                    self.step = ArcStep::DirectionSE { s, e };
                    Some(CmdResult::NeedPoint)
                }
                "R" | "RADIUS" => {
                    self.step = ArcStep::RadiusSE { s, e };
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            ArcStep::AngleSC { s, c } => {
                let mut span = Self::typed_angle(text)?;
                if self.cw {
                    span = -span;
                }
                Some(self.commit(self.sc_angle(s, c, span)))
            }
            ArcStep::LengthSC { s, c } => {
                let len = Self::typed_number(text)?;
                Some(self.commit(self.sc_length(s, c, len)))
            }
            ArcStep::AngleSE { s, e } => {
                let mut included = Self::typed_angle(text)?;
                if self.cw {
                    included = -included;
                }
                Some(self.commit(arc_from_endpoints_angle(s, e, included, self.plane)))
            }
            ArcStep::DirectionSE { s, e } => {
                let angle = Self::typed_angle(text)?;
                let tangent = self.plane.x * angle.cos() + self.plane.y * angle.sin();
                Some(self.commit(arc_continue(s, tangent, e, self.cw, self.plane)))
            }
            ArcStep::RadiusSE { s, e } => {
                let r = Self::typed_number(text)?;
                Some(self.commit(arc_from_endpoints_radius(s, e, r, self.cw, self.plane)))
            }
            _ => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Enter at the start prompt: continue tangent from the last
            // line or arc, as commercial solutions do.
            ArcStep::Start => CmdResult::Dispatch("ARC_CONT".into()),
            _ => CmdResult::Cancel,
        }
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn dyn_field(&self) -> crate::command::DynField {
        use crate::command::DynField;
        match self.step {
            ArcStep::AngleSC { .. } | ArcStep::AngleSE { .. } | ArcStep::DirectionSE { .. } => {
                DynField::Angle
            }
            ArcStep::LengthSC { .. } | ArcStep::RadiusSE { .. } => DynField::Distance,
            _ => DynField::Point,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match self.step {
            ArcStep::AngleSC { s, c } => Some(DynSpec {
                anchor: DynAnchor::Point(c),
                fields: vec![DynFieldSpec::new(DynRole::Angle)],
                guide: DynGuide::Polar,
                ref_point: Some(s),
            }),
            ArcStep::LengthSC { s, .. } => Some(DynSpec {
                anchor: DynAnchor::Point(s),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::Radius,
                ref_point: None,
            }),
            ArcStep::AngleSE { s, e } => Some(DynSpec {
                anchor: DynAnchor::Point((s + e) * 0.5),
                fields: vec![DynFieldSpec::new(DynRole::Angle)],
                guide: DynGuide::None,
                ref_point: Some(s),
            }),
            ArcStep::DirectionSE { s, .. } => Some(DynSpec {
                anchor: DynAnchor::Point(s),
                fields: vec![DynFieldSpec::new(DynRole::Angle)],
                guide: DynGuide::Polar,
                ref_point: Some(s + self.plane.x),
            }),
            ArcStep::RadiusSE { s, .. } => Some(DynSpec {
                anchor: DynAnchor::Point(s),
                fields: vec![DynFieldSpec::new(DynRole::Radius)],
                guide: DynGuide::None,
                ref_point: None,
            }),
            _ => None,
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(
            self.step,
            ArcStep::AngleSC { .. }
                | ArcStep::LengthSC { .. }
                | ArcStep::AngleSE { .. }
                | ArcStep::DirectionSE { .. }
                | ArcStep::RadiusSE { .. }
        )
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        match self.step {
            ArcStep::AngleSC { s, c } => Some(crate::command::dyn_display_angle_deg(
                (angle_xy(c, cursor, self.plane) - angle_xy(c, s, self.plane)) as f32,
            ) as f64),
            ArcStep::LengthSC { s, .. } => Some(s.distance(cursor)),
            ArcStep::RadiusSE { s, e } => {
                arc_from_se_radius(s, e, cursor, self.cw, self.plane).map(|(_, r, _, _)| r)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.on_preview_wires_with_tangent(pt, None).into_iter().next()
    }

    fn on_preview_wires_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Vec<WireModel> {
        match self.step {
            ArcStep::Start | ArcStep::CenterFirst => Vec::new(),
            ArcStep::Second { s } => {
                let start = fluid_anchor_point(s, pt, self.plane);
                vec![line_wire(start, pt)]
            }
            ArcStep::EndAfterStart { s } | ArcStep::CenterAfterStart { s } => {
                vec![line_wire(s, pt)]
            }
            ArcStep::StartAfterCenter { c } => vec![line_wire(c, pt)],
            ArcStep::End3 { s, m } => {
                let p3 = match tangent {
                    Some(target) => ArcPick::Tangent { target, hit: pt },
                    None => ArcPick::Point(pt),
                };
                let fallback = WireModel::solid_f64(
                    "rubber_band".into(),
                    vec![
                        [s.point().x, s.point().y, s.point().z],
                        [m.point().x, m.point().y, m.point().z],
                        [pt.x, pt.y, pt.z],
                    ],
                    WireModel::CYAN,
                    false,
                );
                self.preview(solve_arc_3_inputs(s, m, p3, self.plane), fallback).into_iter().collect()
            }
            ArcStep::EndSC { s, c } => self.preview(self.sc_end(s, c, pt), line_wire(c, pt)).into_iter().collect(),
            ArcStep::AngleSC { s, c } => {
                let span = self.cursor_span(s, c, pt);
                self.preview(self.sc_angle(s, c, span), line_wire(c, pt)).into_iter().collect()
            }
            ArcStep::LengthSC { s, c } => {
                self.preview(self.sc_length(s, c, s.distance(pt)), line_wire(s, pt)).into_iter().collect()
            }
            ArcStep::CenterSE { s, e } => self.preview(self.sc_end(s, pt, e), line_wire(s, e)).into_iter().collect(),
            ArcStep::AngleSE { s, e } => {
                self.preview(arc_from_sagitta(s, e, pt, self.cw, self.plane), line_wire(s, e)).into_iter().collect()
            }
            ArcStep::DirectionSE { s, e } => {
                self.preview(arc_continue(s, pt - s, e, self.cw, self.plane), line_wire(s, e)).into_iter().collect()
            }
            ArcStep::RadiusSE { s, e } => {
                self.preview(arc_from_se_radius(s, e, pt, self.cw, self.plane), line_wire(s, e)).into_iter().collect()
            }
        }
    }
}

// ── Command 2: 3-Point  (ARC_3P) ──────────────────────────────────────────

pub struct Arc3PCommand {
    picks: Vec<ArcPick>,
    plane: WorkingPlane,
}

impl Arc3PCommand {
    pub fn new() -> Self {
        Self {
            picks: Vec::new(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for Arc3PCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ARC_3P"
    }
    fn prompt(&self) -> String {
        match self.picks.len() {
            0 => t!("ARC 3P  Specify start point:").into_owned(),
            1 => t!("ARC 3P  Specify second point on arc:").into_owned(),
            _ => t!("ARC 3P  Specify end point:").into_owned(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.on_point_with_tangent(pt, None).unwrap_or(CmdResult::NeedPoint)
    }

    fn on_point_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Option<CmdResult> {
        let pick = match tangent {
            Some(target) => ArcPick::Tangent { target, hit: pt },
            None => ArcPick::Point(pt),
        };
        self.picks.push(pick);
        if self.picks.len() < 3 {
            return Some(CmdResult::NeedPoint);
        }
        let (p1, p2, p3) = (self.picks[0], self.picks[1], self.picks[2]);
        match solve_arc_3_inputs(p1, p2, p3, self.plane) {
            None => {
                self.picks.pop();
                Some(CmdResult::NeedPoint)
            } // collinear or no solution — retry
            Some((center, radius, start, end)) => {
                Some(arc_result(center, radius, start, end, self.plane))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.picks.is_empty() {
            CmdResult::Dispatch("ARC_CONT".into())
        } else {
            CmdResult::Cancel
        }
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.on_preview_wires_with_tangent(pt, None).into_iter().next()
    }

    fn on_preview_wires_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Vec<WireModel> {
        match self.picks.len() {
            0 => Vec::new(),
            1 => {
                let start = fluid_anchor_point(self.picks[0], pt, self.plane);
                vec![line_wire(start, pt)]
            }
            _ => {
                let (p1, p2) = (self.picks[0], self.picks[1]);
                let p3 = match tangent {
                    Some(target) => ArcPick::Tangent { target, hit: pt },
                    None => ArcPick::Point(pt),
                };
                if let Some((center, radius, start, end)) =
                    solve_arc_3_inputs(p1, p2, p3, self.plane)
                {
                    arc_preview(center, radius, start, end, self.plane).into_iter().collect()
                } else {
                    vec![WireModel::solid_f64(
                        "rubber_band".into(),
                        vec![
                            [p1.point().x, p1.point().y, p1.point().z],
                            [p2.point().x, p2.point().y, p2.point().z],
                            [pt.x, pt.y, pt.z],
                        ],
                        WireModel::CYAN,
                        false,
                    )]
                }
            }
        }
    }

    fn resolved_anchor(&self) -> Option<DVec3> {
        self.picks.last().map(|p| p.point())
    }
}

// ── Command 3: Start, Center, End  (ARC_SCE) ──────────────────────────────

pub struct ArcSCECommand {
    step: u8,
    s: DVec3,
    c: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcSCECommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            c: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSCECommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SCE"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SCE  Specify start point:").into_owned(),
            1 => t!("ARC SCE  Specify center:").into_owned(),
            _ => {
                let r = format!("{:.3}", self.r);
                t!("ARC SCE  Specify end point  [r=%{r}]:", r = r).into_owned()
            }
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.c = pt;
                self.r = pt.distance(self.s);
                self.sa = angle_xy(pt, self.s, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}

// ── Command 4: Start, Center, Angle  (ARC_SCA) ────────────────────────────
// Interactive: cursor direction from center defines span.  Typing: degrees of span.

pub struct ArcSCACommand {
    step: u8,
    s: DVec3,
    c: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcSCACommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            c: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSCACommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SCA"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SCA  Specify start point:").into_owned(),
            1 => t!("ARC SCA  Specify center:").into_owned(),
            _ => {
                let sa = format!("{:.1}°", self.sa.to_degrees());
                t!(
                    "ARC SCA  Click end direction or type arc span in degrees  [start=%{sa}]:",
                    sa = sa
                )
                .into_owned()
            }
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.c = pt;
                self.r = pt.distance(self.s);
                self.sa = angle_xy(pt, self.s, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step == 2 {
            let mut span = crate::entities::common::parse_typed_angle(text)?;
            if self.cw {
                span = -span;
            }
            let ea = self.sa + span;
            return Some(if span < 0.0 {
                arc_result(self.c, self.r, ea, self.sa, self.plane)
            } else {
                arc_result(self.c, self.r, self.sa, ea, self.plane)
            });
        }
        None
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        // Included angle (span) at the centre. Typed value is the span, handled
        // by on_text_input; the box previews the live span via dyn_live_value.
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.c),
            fields: vec![DynFieldSpec::new(DynRole::Angle)],
            guide: DynGuide::Polar,
            ref_point: Some(
                self.c + self.plane.x * self.sa.cos() + self.plane.y * self.sa.sin(),
            ),
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        (self.step == 2).then(|| {
            crate::command::dyn_display_angle_deg(
                (angle_xy(self.c, cursor, self.plane) - self.sa) as f32,
            ) as f64
        })
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}

// ── Command 5: Start, Center, Length  (ARC_SCL) ───────────────────────────
// "Length" = chord length from start to end of arc.
// Interactive: cursor distance from start_pt drives the chord length.

pub struct ArcSCLCommand {
    step: u8,
    s: DVec3,
    c: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcSCLCommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            c: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSCLCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SCL"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SCL  Specify start point:").into_owned(),
            1 => t!("ARC SCL  Specify center:").into_owned(),
            _ => {
                let r = format!("{:.3}", self.r);
                t!(
                    "ARC SCL  Click chord end or type chord length  [r=%{r}]:",
                    r = r
                )
                .into_owned()
            }
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.c = pt;
                self.r = pt.distance(self.s);
                self.sa = angle_xy(pt, self.s, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let chord = self.s.distance(pt);
                let Some(ea) = end_angle_from_chord_len(self.sa, chord, self.r) else {
                    return CmdResult::NeedPoint;
                };
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step == 2 {
            let chord: f64 = text.trim().replace(',', ".").parse().ok()?;
            if chord != 0.0 {
                let ea = end_angle_from_chord_len(self.sa, chord, self.r)?;
                return Some(if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                });
            }
        }
        None
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        // Chord length from the start point (typed → on_text_input).
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.s),
            fields: vec![DynFieldSpec::new(DynRole::Distance)],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        (self.step == 2).then(|| self.s.distance(cursor))
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                let chord = self.s.distance(pt);
                let ea = end_angle_from_chord_len(self.sa, chord, self.r)?;
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}

// ── Command 6: Start, End, Angle  (ARC_SEA) ───────────────────────────────
// Interactive: cursor distance from chord defines sagitta → arc shape.

pub struct ArcSEACommand {
    step: u8,
    s: DVec3,
    e: DVec3,
    ctrl: bool,
    plane: WorkingPlane,
}

impl ArcSEACommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            e: DVec3::ZERO,
            ctrl: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSEACommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SEA"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SEA  Specify start point:").into_owned(),
            1 => t!("ARC SEA  Specify end point:").into_owned(),
            _ => t!("ARC SEA  Specify angle (move cursor perpendicular to chord):").into_owned(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.e = pt;
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => match arc_from_sagitta(self.s, self.e, pt, self.ctrl, self.plane) {
                Some((center, radius, sa, ea)) => {
                    arc_result(center, radius, sa, ea, self.plane)
                }
                None => CmdResult::NeedPoint,
            },
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step != 2 {
            return None;
        }
        let mut included = crate::entities::common::parse_typed_angle(text)?;
        if self.ctrl {
            included = -included;
        }
        let (center, radius, sa, ea) =
            arc_from_endpoints_angle(self.s, self.e, included, self.plane)?;
        Some(arc_result(center, radius, sa, ea, self.plane))
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point((self.s + self.e) * 0.5),
            fields: vec![DynFieldSpec::new(DynRole::Angle)],
            guide: DynGuide::None,
            ref_point: Some(self.s),
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                if let Some((center, radius, sa, ea)) =
                    arc_from_sagitta(self.s, self.e, pt, self.ctrl, self.plane)
                {
                    arc_preview(center, radius, sa, ea, self.plane)
                } else {
                    Some(line_wire(self.s, self.e))
                }
            }
            _ => None,
        }
    }
}

// ── Command 7: Start, End, Radius  (ARC_SER) ──────────────────────────────
// Interactive: radius = distance(cursor, start_point).

pub struct ArcSERCommand {
    step: u8,
    s: DVec3,
    e: DVec3,
    ctrl: bool,
    plane: WorkingPlane,
}

impl ArcSERCommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            e: DVec3::ZERO,
            ctrl: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSERCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SER"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SER  Specify start point:").into_owned(),
            1 => t!("ARC SER  Specify end point:").into_owned(),
            _ => t!("ARC SER  Click radius point or type radius value:").into_owned(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.e = pt;
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => match arc_from_se_radius(self.s, self.e, pt, self.ctrl, self.plane) {
                Some((center, radius, sa, ea)) => {
                    arc_result(center, radius, sa, ea, self.plane)
                }
                None => CmdResult::NeedPoint,
            },
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step == 2 {
            let r: f64 = text.trim().replace(',', ".").parse().ok()?;
            let (center, radius, sa, ea) =
                arc_from_endpoints_radius(self.s, self.e, r, self.ctrl, self.plane)?;
            return Some(arc_result(center, radius, sa, ea, self.plane));
        }
        None
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        // Radius value (typed → on_text_input); the preview arc is the guide.
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.s),
            fields: vec![DynFieldSpec::new(DynRole::Radius)],
            guide: DynGuide::None,
            ref_point: None,
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if self.step != 2 {
            return None;
        }
        arc_from_se_radius(self.s, self.e, cursor, self.ctrl, self.plane)
            .map(|(_, r, _, _)| r)
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                if let Some((center, radius, sa, ea)) =
                    arc_from_se_radius(self.s, self.e, pt, self.ctrl, self.plane)
                {
                    arc_preview(center, radius, sa, ea, self.plane)
                } else {
                    Some(line_wire(self.s, self.e))
                }
            }
            _ => None,
        }
    }
}

// ── Command 8: Start, End, Direction  (ARC_SED) ───────────────────────────
// Interactive: cursor position defines tangent direction at start (cursor − start).

pub struct ArcSEDCommand {
    step: u8,
    s: DVec3,
    e: DVec3,
    ctrl: bool,
    plane: WorkingPlane,
}

impl ArcSEDCommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            s: DVec3::ZERO,
            e: DVec3::ZERO,
            ctrl: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcSEDCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_SED"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC SED  Specify start point:").into_owned(),
            1 => t!("ARC SED  Specify end point:").into_owned(),
            _ => t!("ARC SED  Specify tangent direction at start:").into_owned(),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.s = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.e = pt;
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => match arc_continue(self.s, pt - self.s, self.e, self.ctrl, self.plane) {
                Some((center, radius, sa, ea)) => {
                    arc_result(center, radius, sa, ea, self.plane)
                }
                None => CmdResult::NeedPoint,
            },
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step != 2 {
            return None;
        }
        let angle = crate::entities::common::parse_typed_angle(text)?;
        let tangent = self.plane.x * angle.cos() + self.plane.y * angle.sin();
        let (center, radius, sa, ea) =
            arc_continue(self.s, tangent, self.e, self.ctrl, self.plane)?;
        Some(arc_result(center, radius, sa, ea, self.plane))
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.s),
            fields: vec![DynFieldSpec::new(DynRole::Angle)],
            guide: DynGuide::Polar,
            ref_point: Some(self.s + self.plane.x),
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.s, pt)),
            2 => {
                if let Some((center, radius, sa, ea)) =
                    arc_continue(self.s, pt - self.s, self.e, self.ctrl, self.plane)
                {
                    arc_preview(center, radius, sa, ea, self.plane)
                } else {
                    Some(line_wire(self.s, self.e))
                }
            }
            _ => None,
        }
    }
}

// ── Command 9: Center, Start, Angle  (ARC_CSA) ────────────────────────────
// Interactive: angle direction indicated by cursor position relative to center.

pub struct ArcCSACommand {
    step: u8,
    c: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcCSACommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            c: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcCSACommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_CSA"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC CSA  Specify center:").into_owned(),
            1 => t!("ARC CSA  Specify start point:").into_owned(),
            _ => {
                let sa = format!("{:.1}°", self.sa.to_degrees());
                t!(
                    "ARC CSA  Click end direction or type arc span in degrees  [start=%{sa}]:",
                    sa = sa
                )
                .into_owned()
            }
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.c = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.r = self.c.distance(pt);
                self.sa = angle_xy(self.c, pt, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step == 2 {
            let mut span = crate::entities::common::parse_typed_angle(text)?;
            if self.cw {
                span = -span;
            }
            let ea = self.sa + span;
            return Some(if span < 0.0 {
                arc_result(self.c, self.r, ea, self.sa, self.plane)
            } else {
                arc_result(self.c, self.r, self.sa, ea, self.plane)
            });
        }
        None
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.c),
            fields: vec![DynFieldSpec::new(DynRole::Angle)],
            guide: DynGuide::Polar,
            ref_point: Some(
                self.c + self.plane.x * self.sa.cos() + self.plane.y * self.sa.sin(),
            ),
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        (self.step == 2).then(|| {
            crate::command::dyn_display_angle_deg(
                (angle_xy(self.c, cursor, self.plane) - self.sa) as f32,
            ) as f64
        })
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.c, pt)),
            2 => {
                let ea = angle_xy(self.c, pt, self.plane);
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}

// ── Command 10: Center, Start, Length  (ARC_CSL) ──────────────────────────
// "Length" = chord from start to end.  Interactive: dist(cursor, start_pt) = chord.

pub struct ArcCSLCommand {
    step: u8,
    c: DVec3,
    s: DVec3,
    r: f64,
    sa: f64,
    cw: bool,
    plane: WorkingPlane,
}

impl ArcCSLCommand {
    pub fn new() -> Self {
        Self {
            step: 0,
            c: DVec3::ZERO,
            s: DVec3::ZERO,
            r: 0.0,
            sa: 0.0,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArcCSLCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.cw = ctrl;
    }

    fn name(&self) -> &'static str {
        "ARC_CSL"
    }
    fn prompt(&self) -> String {
        match self.step {
            0 => t!("ARC CSL  Specify center:").into_owned(),
            1 => t!("ARC CSL  Specify start point:").into_owned(),
            _ => {
                let r = format!("{:.3}", self.r);
                t!(
                    "ARC CSL  Click chord end or type chord length  [r=%{r}]:",
                    r = r
                )
                .into_owned()
            }
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            0 => {
                self.c = pt;
                self.step = 1;
                CmdResult::NeedPoint
            }
            1 => {
                self.s = pt;
                self.r = self.c.distance(pt);
                self.sa = angle_xy(self.c, pt, self.plane);
                self.step = 2;
                CmdResult::NeedPoint
            }
            _ => {
                let chord = self.s.distance(pt);
                let Some(ea) = end_angle_from_chord_len(self.sa, chord, self.r) else {
                    return CmdResult::NeedPoint;
                };
                if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                }
            }
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn enter_accepts_default_start(&self) -> bool {
        false
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step == 2 {
            let chord: f64 = text.trim().replace(',', ".").parse().ok()?;
            if chord != 0.0 {
                let ea = end_angle_from_chord_len(self.sa, chord, self.r)?;
                return Some(if self.cw {
                    arc_result(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_result(self.c, self.r, self.sa, ea, self.plane)
                });
            }
        }
        None
    }
    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        // Chord length from the start point (typed → on_text_input).
        (self.step == 2).then(|| DynSpec {
            anchor: DynAnchor::Point(self.s),
            fields: vec![DynFieldSpec::new(DynRole::Distance)],
            guide: DynGuide::Radius,
            ref_point: None,
        })
    }
    fn dyn_commit_as_text(&self) -> bool {
        self.step == 2
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        (self.step == 2).then(|| self.s.distance(cursor))
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            1 => Some(line_wire(self.c, pt)),
            2 => {
                let chord = self.s.distance(pt);
                let ea = end_angle_from_chord_len(self.sa, chord, self.r)?;
                if self.cw {
                    arc_preview(self.c, self.r, ea, self.sa, self.plane)
                } else {
                    arc_preview(self.c, self.r, self.sa, ea, self.plane)
                }
            }
            _ => None,
        }
    }
}


/// Solves an arc continuing from start point `s` with initial tangent `tangent`
/// that touches `target` tangentially.
///
/// Returns `(center, radius, start_angle, end_angle, tangent_pt_on_target)`.
pub fn solve_tangent_continue(
    s: DVec3,
    tangent: DVec3,
    target: TangentObject,
    hit: DVec3,
    flip: bool,
    plane: WorkingPlane,
) -> Option<(DVec3, f64, f64, f64, DVec3)> {
    let s_loc = plane.to_local(s);
    let t_loc = plane.vector_to_local(tangent);
    let hit_loc = plane.to_local(hit);

    let s2 = DVec2::new(s_loc.x, s_loc.y);
    let t2 = DVec2::new(t_loc.x, t_loc.y).normalize_or_zero();
    let hit2 = DVec2::new(hit_loc.x, hit_loc.y);

    if t2.length_squared() < 1e-12 {
        return None;
    }
    let n2 = DVec2::new(-t2.y, t2.x);

    match target {
        TangentObject::Circle { center, radius } => {
            let c_loc = plane.to_local(center);
            let c2 = DVec2::new(c_loc.x, c_loc.y);
            let r2 = radius.abs();
            if r2 < 1e-6 {
                return None;
            }
            let d = s2 - c2;
            let d_len_sq = d.length_squared();
            let d_dot_n = d.dot(n2);

            let mut candidates = Vec::new();
            for sigma in [1.0, -1.0] {
                let denom = 2.0 * (d_dot_n - sigma * r2);
                if denom.abs() > 1e-9 {
                    let r = (r2 * r2 - d_len_sq) / denom;
                    if r.abs() > 1e-4 && r.abs() < 1e8 {
                        let center2 = s2 + n2 * r;
                        let v = center2 - c2;
                        let dist = v.length();
                        if dist > 1e-9 {
                            let u = v / dist;
                            let p1 = c2 + u * r2;
                            let p2 = c2 - u * r2;
                            let (best_p, err) = if ((p1 - center2).length() - r.abs()).abs()
                                <= ((p2 - center2).length() - r.abs())
                            {
                                (p1, ((p1 - center2).length() - r.abs()).abs())
                            } else {
                                (p2, ((p2 - center2).length() - r.abs()).abs())
                            };
                            if err < 1e-3 {
                                candidates.push(best_p);
                            }
                        }
                    }
                }
            }

            candidates
                .into_iter()
                .filter_map(|best_p| {
                    let tgt_world = plane.to_world(DVec3::new(best_p.x, best_p.y, s_loc.z));
                    let res = arc_continue(s, tangent, tgt_world, flip, plane)?;
                    let score = (best_p - hit2).length_squared();
                    Some((score, res, tgt_world))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, (c, r, sa, ea), tgt_world)| (c, r, sa, ea, tgt_world))
        }

        TangentObject::Line { p1, p2 } => {
            let p1_loc = plane.to_local(p1);
            let p2_loc = plane.to_local(p2);
            let p1_2 = DVec2::new(p1_loc.x, p1_loc.y);
            let p2_2 = DVec2::new(p2_loc.x, p2_loc.y);
            let line_vec = p2_2 - p1_2;
            let line_len = line_vec.length();
            if line_len < 1e-9 {
                return None;
            }
            let v_line = line_vec / line_len;
            let n_line = DVec2::new(-v_line.y, v_line.x);
            let d0 = (s2 - p1_2).dot(n_line);
            let k = n2.dot(n_line);

            let mut r_candidates = Vec::new();
            if (1.0 - k).abs() > 1e-9 {
                r_candidates.push(d0 / (1.0 - k));
            }
            if (1.0 + k).abs() > 1e-9 {
                r_candidates.push(-d0 / (1.0 + k));
            }
            if (1.0 - k * k).abs() <= 1e-9 && k.abs() > 1e-9 {
                r_candidates.push(-d0 / (2.0 * k));
            }

            let mut candidates = Vec::new();
            for r in r_candidates {
                if r.abs() > 1e-4 && r.abs() < 1e8 {
                    let center2 = s2 + n2 * r;
                    let p_tgt2 = center2 - n_line * ((center2 - p1_2).dot(n_line));
                    let err = ((p_tgt2 - center2).length() - r.abs()).abs();
                    if err < 1e-3 {
                        candidates.push(p_tgt2);
                    }
                }
            }

            candidates
                .into_iter()
                .filter_map(|best_p| {
                    let tgt_world = plane.to_world(DVec3::new(best_p.x, best_p.y, s_loc.z));
                    let res = arc_continue(s, tangent, tgt_world, flip, plane)?;
                    let seg_t = (best_p - p1_2).dot(v_line);
                    let outside_dist = if seg_t < 0.0 {
                        -seg_t
                    } else if seg_t > line_len {
                        seg_t - line_len
                    } else {
                        0.0
                    };
                    let score =
                        (best_p - hit2).length_squared() + outside_dist * outside_dist * 100.0;
                    Some((score, res, tgt_world))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, (c, r, sa, ea), tgt_world)| (c, r, sa, ea, tgt_world))
        }

        TangentObject::Ellipse { .. } => {
            let ell = target_to_ellipse2d(target, plane)?;

            let eval_t = |t: f64| -> Option<(f64, DVec2, f64, DVec2)> {
                let pt = ell.point_at(t);
                let n_hat = ell.normal_at(t);
                let delta = pt - s2;
                let det = n2.y * n_hat.x - n2.x * n_hat.y;
                if det.abs() < 1e-6 {
                    return None;
                }
                let r = (-delta.x * n_hat.y + delta.y * n_hat.x) / det;
                let lambda = (n2.x * delta.y - n2.y * delta.x) / det;
                let center = s2 + n2 * r;
                let res = r.abs() - lambda.abs();
                Some((res, pt, r.abs(), center))
            };

            let n_steps = 144;
            let dt = std::f64::consts::TAU / (n_steps as f64);
            let mut candidates = Vec::new();

            for i in 0..n_steps {
                let t_a = i as f64 * dt;
                let t_b = (i + 1) as f64 * dt;

                let Some((res_a, _, _, _)) = eval_t(t_a) else {
                    continue;
                };
                let Some((res_b, _, _, _)) = eval_t(t_b) else {
                    continue;
                };

                let n_hat_a = ell.normal_at(t_a);
                let det_a = n2.y * n_hat_a.x - n2.x * n_hat_a.y;
                let n_hat_b = ell.normal_at(t_b);
                let det_b = n2.y * n_hat_b.x - n2.x * n_hat_b.y;
                if det_a.signum() != det_b.signum() {
                    continue;
                }

                if res_a.signum() != res_b.signum() {
                    let mut lo = t_a;
                    let mut hi = t_b;
                    let mut best_t = (lo + hi) * 0.5;
                    for _ in 0..16 {
                        let mid = (lo + hi) * 0.5;
                        let Some((res_m, _, _, _)) = eval_t(mid) else {
                            break;
                        };
                        best_t = mid;
                        if res_m.abs() < 1e-8 {
                            break;
                        }
                        if res_m.signum() == res_a.signum() {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    if let Some((res, pt, r, _center)) = eval_t(best_t) {
                        if res.abs() < 1e-3 && r.is_finite() && r > 1e-4 && r < 1e8 {
                            candidates.push(pt);
                        }
                    }
                }
            }

            candidates
                .into_iter()
                .filter_map(|best_p| {
                    let tgt_world = plane.to_world(DVec3::new(best_p.x, best_p.y, s_loc.z));
                    let res = arc_continue(s, tangent, tgt_world, flip, plane)?;
                    let score = (best_p - hit2).length_squared();
                    Some((score, res, tgt_world))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, (c, r, sa, ea), tgt_world)| (c, r, sa, ea, tgt_world))
        }
    }
}

// ── Command 11: Continue  (ARC_CONT) ──────────────────────────────────────
pub struct ArcContCommand {
    s: DVec3,
    tangent: DVec3,
    /// Live Ctrl state (set via `set_ctrl`): flips the arc to the other way.
    ctrl: bool,
    plane: WorkingPlane,
    resolved_end: Option<DVec3>,
}

impl ArcContCommand {
    pub fn new(s: DVec3, tangent: DVec3) -> Self {
        Self {
            s,
            tangent,
            ctrl: false,
            plane: WorkingPlane::default(),
            resolved_end: None,
        }
    }
}

impl CadCommand for ArcContCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ARC_CONT"
    }
    fn prompt(&self) -> String {
        t!("ARC Continue  Specify end point  [Ctrl = flip direction]:").into_owned()
    }
    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.on_point_with_tangent(pt, None).unwrap_or(CmdResult::NeedPoint)
    }
    fn on_point_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Option<CmdResult> {
        if let Some(target) = tangent {
            if let Some((center, radius, sa, ea, tgt_pt)) =
                solve_tangent_continue(self.s, self.tangent, target, pt, self.ctrl, self.plane)
            {
                self.resolved_end = Some(tgt_pt);
                return Some(arc_result(center, radius, sa, ea, self.plane));
            }
        }
        match arc_continue(self.s, self.tangent, pt, self.ctrl, self.plane) {
            Some((center, radius, sa, ea)) => {
                self.resolved_end = Some(pt);
                Some(arc_result(center, radius, sa, ea, self.plane))
            }
            None => Some(CmdResult::NeedPoint),
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.on_preview_wires_with_tangent(pt, None).into_iter().next()
    }
    fn on_preview_wires_with_tangent(
        &mut self,
        pt: DVec3,
        tangent: Option<TangentObject>,
    ) -> Vec<WireModel> {
        if let Some(target) = tangent {
            if let Some((center, radius, sa, ea, _tgt_pt)) =
                solve_tangent_continue(self.s, self.tangent, target, pt, self.ctrl, self.plane)
            {
                return arc_preview(center, radius, sa, ea, self.plane).into_iter().collect();
            }
        }
        match arc_continue(self.s, self.tangent, pt, self.ctrl, self.plane) {
            Some((center, radius, sa, ea)) => {
                arc_preview(center, radius, sa, ea, self.plane).into_iter().collect()
            }
            None => vec![line_wire(self.s, pt)],
        }
    }
    fn resolved_anchor(&self) -> Option<DVec3> {
        self.resolved_end
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ARC", "ARC_3P"] });  // Arc3PCommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_CSA"] });  // ArcCSACommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_CSL"] });  // ArcCSLCommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_CSE"] });  // ArcCSECommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SCA"] });  // ArcSCACommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SCE"] });  // ArcSCECommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SCL"] });  // ArcSCLCommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SEA"] });  // ArcSEACommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SED"] });  // ArcSEDCommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_SER"] });  // ArcSERCommand
inventory::submit!(crate::command::CommandRegistration { names: &["ARC_CONT"] });  // ArcContCommand

#[cfg(test)]
mod acad_flow_tests {
    use super::*;

    fn keywords(cmd: &ArcCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    fn p(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn committed_arc(res: CmdResult) -> CadArc {
        match res {
            CmdResult::CommitAndExit(EntityType::Arc(a)) => a,
            _ => panic!("expected a committed arc"),
        }
    }

    #[test]
    fn prompts_follow_compatible_keywords() {
        let mut cmd = ArcCommand::new();
        assert_eq!(keywords(&cmd), ["C"]);
        cmd.on_point(p(0.0, 0.0));
        assert_eq!(keywords(&cmd), ["C", "E"]);
        assert!(matches!(cmd.on_text_input("C"), Some(CmdResult::NeedPoint)));
        assert!(keywords(&cmd).is_empty(), "centre pick has no keywords");
        cmd.on_point(p(5.0, 0.0));
        assert_eq!(keywords(&cmd), ["A", "L"]);

        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_text_input("E");
        cmd.on_point(p(10.0, 0.0));
        assert_eq!(keywords(&cmd), ["A", "D", "R"]);
    }

    #[test]
    fn three_points_make_an_arc() {
        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_point(p(5.0, 5.0));
        let arc = committed_arc(cmd.on_point(p(10.0, 0.0)));
        assert!((arc.radius - 5.0).abs() < 1e-9);
        assert!((arc.center.x - 5.0).abs() < 1e-9 && arc.center.y.abs() < 1e-9);
    }

    #[test]
    fn center_first_then_start_and_end() {
        let mut cmd = ArcCommand::new();
        cmd.on_text_input("C");
        cmd.on_point(p(5.0, 0.0));
        cmd.on_point(p(10.0, 0.0));
        assert_eq!(keywords(&cmd), ["A", "L"]);
        let arc = committed_arc(cmd.on_point(p(5.0, 10.0)));
        assert!((arc.radius - 5.0).abs() < 1e-9);
        assert!(arc.start_angle.abs() < 1e-9);
        assert!((arc.end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn start_center_angle_and_chord_length_typed() {
        let mut cmd = ArcCommand::new();
        cmd.on_point(p(10.0, 0.0));
        cmd.on_text_input("C");
        cmd.on_point(p(5.0, 0.0));
        cmd.on_text_input("A");
        let arc = committed_arc(cmd.on_text_input("90").expect("angle consumed"));
        assert!((arc.end_angle - arc.start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);

        let mut cmd = ArcCommand::new();
        cmd.on_point(p(10.0, 0.0));
        cmd.on_text_input("C");
        cmd.on_point(p(5.0, 0.0));
        cmd.on_text_input("L");
        // Chord 10 on radius 5: a semicircle.
        let arc = committed_arc(cmd.on_text_input("10").expect("length consumed"));
        assert!((arc.end_angle - arc.start_angle - std::f64::consts::PI).abs() < 1e-9);
    }

    #[test]
    fn start_end_then_radius_direction_and_angle() {
        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_text_input("E");
        cmd.on_point(p(10.0, 0.0));
        cmd.on_text_input("R");
        let arc = committed_arc(cmd.on_text_input("5").expect("radius consumed"));
        assert!((arc.radius - 5.0).abs() < 1e-9);

        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_text_input("E");
        cmd.on_point(p(10.0, 0.0));
        cmd.on_text_input("D");
        // Tangent straight up at the start: a semicircle over the chord.
        let arc = committed_arc(cmd.on_text_input("90").expect("direction consumed"));
        assert!((arc.radius - 5.0).abs() < 1e-6);

        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_text_input("E");
        cmd.on_point(p(10.0, 0.0));
        cmd.on_text_input("A");
        let arc = committed_arc(cmd.on_text_input("180").expect("angle consumed"));
        assert!((arc.radius - 5.0).abs() < 1e-6);

        // Centre pick after start / end.
        let mut cmd = ArcCommand::new();
        cmd.on_point(p(0.0, 0.0));
        cmd.on_text_input("E");
        cmd.on_point(p(10.0, 0.0));
        let arc = committed_arc(cmd.on_point(p(5.0, 0.0)));
        assert!((arc.radius - 5.0).abs() < 1e-9);
    }

    #[test]
    fn enter_at_start_continues_from_last_entity() {
        let mut cmd = ArcCommand::new();
        assert!(matches!(cmd.on_enter(), CmdResult::Dispatch(c) if c == "ARC_CONT"));
    }

    #[test]
    fn arc_3p_fluid_tangent_between_two_circles() {
        let mut cmd = Arc3PCommand::new();
        let c1 = TangentObject::Circle {
            center: DVec3::new(-20.0, 0.0, 0.0),
            radius: 10.0,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(20.0, 0.0, 0.0),
            radius: 10.0,
        };
        // Pick 1: Tangent on circle 1
        let r1 = cmd.on_point_with_tangent(DVec3::new(-20.0, 10.0, 0.0), Some(c1));
        assert!(matches!(r1, Some(CmdResult::NeedPoint)));

        // Pick 2: Middle pass-through point
        let r2 = cmd.on_point_with_tangent(DVec3::new(0.0, 10.0, 0.0), None);
        assert!(matches!(r2, Some(CmdResult::NeedPoint)));

        // Live preview with tangent on circle 2
        let preview = cmd.on_preview_wires_with_tangent(DVec3::new(20.0, 10.0, 0.0), Some(c2));
        assert!(!preview.is_empty(), "Live preview should generate arc wire");

        // Pick 3: Tangent on circle 2
        let r3 = cmd.on_point_with_tangent(DVec3::new(20.0, 10.0, 0.0), Some(c2));
        let arc = committed_arc(r3.expect("command completed"));
        // The arc must pass through (0, 10)
        let center = DVec3::new(arc.center.x, arc.center.y, arc.center.z);
        let dist_to_mid = (center - DVec3::new(0.0, 10.0, 0.0)).length();
        assert!((dist_to_mid - arc.radius).abs() < 1e-3);
        // The arc must be tangent to c1: |center - c1.center| = |arc.radius +- 10|
        let dist_to_c1 = (center - DVec3::new(-20.0, 0.0, 0.0)).length();
        assert!((dist_to_c1 - (arc.radius + 10.0)).abs() < 1e-3 || (dist_to_c1 - (arc.radius - 10.0).abs()).abs() < 1e-3);
    }

    #[test]
    fn arc_3p_fluid_tangent_to_one_circle_and_two_points() {
        let mut cmd = Arc3PCommand::new();
        let c1 = TangentObject::Circle {
            center: DVec3::new(0.0, 0.0, 0.0),
            radius: 10.0,
        };
        // Pick 1: Point 1
        cmd.on_point_with_tangent(DVec3::new(0.0, 20.0, 0.0), None);
        // Pick 2: Point 2
        cmd.on_point_with_tangent(DVec3::new(20.0, 20.0, 0.0), None);
        // Pick 3: Tangent on circle 1
        let r3 = cmd.on_point_with_tangent(DVec3::new(0.0, 10.0, 0.0), Some(c1));
        let arc = committed_arc(r3.expect("command completed"));
        let center = DVec3::new(arc.center.x, arc.center.y, arc.center.z);
        // Passes through p1 and p2
        assert!(((center - DVec3::new(0.0, 20.0, 0.0)).length() - arc.radius).abs() < 1e-3);
        assert!(((center - DVec3::new(20.0, 20.0, 0.0)).length() - arc.radius).abs() < 1e-3);
    }

    #[test]
    fn arc_3p_fluid_tangent_ellipse_and_circle() {
        let mut cmd = Arc3PCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
        };

        // Pick 1: Tangent on Ellipse 1
        cmd.on_point_with_tangent(DVec3::new(-30.0, 10.0, 0.0), Some(e1));
        // Pick 2: Middle point
        cmd.on_point_with_tangent(DVec3::new(0.0, 15.0, 0.0), None);
        // Live preview with tangent on circle 2
        let preview = cmd.on_preview_wires_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        assert!(!preview.is_empty());
        // Pick 3: Tangent on circle 2
        let r3 = cmd.on_point_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        let arc = committed_arc(r3.expect("command completed"));

        // Verify the arc's start point lies exactly on the ellipse!
        let arc_start = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );

        // One of the endpoints must lie on the ellipse (x/20)^2 + (y/10)^2 = 1 (relative to center -30,0)
        let check_on_ellipse = |pt: DVec3| {
            let dx = pt.x - (-30.0);
            let dy = pt.y - 0.0;
            ((dx / 20.0).powi(2) + (dy / 10.0).powi(2) - 1.0).abs()
        };
        let d_start = check_on_ellipse(arc_start);
        let d_end = check_on_ellipse(arc_end);
        assert!(d_start < 1e-3 || d_end < 1e-3, "One arc endpoint must lie on the ellipse: d_start={d_start}, d_end={d_end}");
    }

    #[test]
    fn arc_3p_fluid_tangent_ellipse_two_points_wrong_click_point() {
        let mut cmd = Arc3PCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(0.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        // Point 1 and Point 2
        cmd.on_point_with_tangent(DVec3::new(-30.0, 30.0, 0.0), None);
        cmd.on_point_with_tangent(DVec3::new(30.0, 30.0, 0.0), None);

        // Pick 3: Tangent on Ellipse 1 at the "wrong" place (e.g. bottom vertex (0, -10))
        // The real tangent solution closest to the points is near the top (0, 10).
        let r3 = cmd.on_point_with_tangent(DVec3::new(0.0, -10.0, 0.0), Some(e1));
        let arc = committed_arc(r3.expect("command should find valid tangent arc"));

        // Verify the arc endpoints or contact point is tangent to the ellipse
        let center = DVec3::new(arc.center.x, arc.center.y, arc.center.z);
        // Must pass through p1 and p2
        assert!(((center - DVec3::new(-30.0, 30.0, 0.0)).length() - arc.radius).abs() < 1e-3);
        assert!(((center - DVec3::new(30.0, 30.0, 0.0)).length() - arc.radius).abs() < 1e-3);

        // One of the arc endpoints must touch the ellipse (x/20)^2 + (y/10)^2 = 1
        let arc_start = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );
        let check_on_ellipse = |pt: DVec3| {
            ((pt.x / 20.0).powi(2) + (pt.y / 10.0).powi(2) - 1.0).abs()
        };
        let d_start = check_on_ellipse(arc_start);
        let d_end = check_on_ellipse(arc_end);
        assert!(d_start < 1e-3 || d_end < 1e-3, "Arc must terminate exactly on ellipse: d_start={d_start}, d_end={d_end}");
    }

    #[test]
    fn arc_3p_fluid_tangent_ellipse_and_circle_arbitrary_click() {
        let mut cmd = Arc3PCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
        };

        // Pick 1: User clicks on the FAR side of the ellipse (-50, 0)
        cmd.on_point_with_tangent(DVec3::new(-50.0, 0.0, 0.0), Some(e1));
        // Pick 2: Middle point (0, 15)
        cmd.on_point_with_tangent(DVec3::new(0.0, 15.0, 0.0), None);
        // Pick 3: Circle (30, 10)
        let r3 = cmd.on_point_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        let arc = committed_arc(r3.expect("must find closest valid tangent arc"));

        let arc_start = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );
        let check_on_ellipse = |pt: DVec3| {
            let dx = pt.x - (-30.0);
            let dy = pt.y - 0.0;
            ((dx / 20.0).powi(2) + (dy / 10.0).powi(2) - 1.0).abs()
        };
        let d_start = check_on_ellipse(arc_start);
        let d_end = check_on_ellipse(arc_end);
        assert!(d_start < 1e-3 || d_end < 1e-3, "Arc must terminate exactly on ellipse: d_start={d_start}, d_end={d_end}");
    }

    #[test]
    fn arc_3p_fluid_tangent_two_tangents_then_point_all_endpoints_tangent() {
        let mut cmd = Arc3PCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let c2 = TangentObject::Circle {
            center: DVec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
        };

        // Pick 1: Tangent on Ellipse 1
        cmd.on_point_with_tangent(DVec3::new(-30.0, 10.0, 0.0), Some(e1));
        // Pick 2: Tangent on Circle 2
        cmd.on_point_with_tangent(DVec3::new(30.0, 10.0, 0.0), Some(c2));
        // Pick 3: Point in between (0, 15)
        let r3 = cmd.on_point_with_tangent(DVec3::new(0.0, 15.0, 0.0), None);
        let arc = committed_arc(r3.expect("command completed"));

        let arc_start = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );

        let check_on_ellipse = |pt: DVec3| {
            let dx = pt.x - (-30.0);
            let dy = pt.y - 0.0;
            ((dx / 20.0).powi(2) + (dy / 10.0).powi(2) - 1.0).abs()
        };
        let check_on_circle = |pt: DVec3| {
            ((pt - DVec3::new(30.0, 0.0, 0.0)).length() - 10.0).abs()
        };

        // BOTH endpoints must be tangent: one on ellipse 1, one on circle 2!
        let start_on_e1 = check_on_ellipse(arc_start) < 1e-3;
        let end_on_e1 = check_on_ellipse(arc_end) < 1e-3;
        let start_on_c2 = check_on_circle(arc_start) < 1e-3;
        let end_on_c2 = check_on_circle(arc_end) < 1e-3;

        assert!(
            (start_on_e1 && end_on_c2) || (start_on_c2 && end_on_e1),
            "Both arc endpoints must terminate tangentially on the respective curves! start: {arc_start:?}, end: {arc_end:?}"
        );
    }

    #[test]
    fn arc_3p_fluid_tangent_two_ellipses() {
        let mut cmd = Arc3PCommand::new();
        let e1 = TangentObject::Ellipse {
            center: DVec3::new(-30.0, 0.0, 0.0),
            major_axis: DVec3::new(20.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };
        let e2 = TangentObject::Ellipse {
            center: DVec3::new(30.0, 0.0, 0.0),
            major_axis: DVec3::new(15.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.6,
        };

        // Pick 1: Tangent on Ellipse 1
        cmd.on_point_with_tangent(DVec3::new(-30.0, 10.0, 0.0), Some(e1));
        // Pick 2: Tangent on Ellipse 2
        cmd.on_point_with_tangent(DVec3::new(30.0, 9.0, 0.0), Some(e2));
        // Pick 3: Point in between (0, 15)
        let r3 = cmd.on_point_with_tangent(DVec3::new(0.0, 15.0, 0.0), None);
        let arc = committed_arc(r3.expect("command completed for two ellipses"));

        let arc_start = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );

        let check_on_e1 = |pt: DVec3| {
            let dx = pt.x - (-30.0);
            let dy = pt.y - 0.0;
            ((dx / 20.0).powi(2) + (dy / 10.0).powi(2) - 1.0).abs()
        };
        let check_on_e2 = |pt: DVec3| {
            let dx = pt.x - 30.0;
            let dy = pt.y - 0.0;
            ((dx / 15.0).powi(2) + (dy / 9.0).powi(2) - 1.0).abs()
        };

        let start_on_e1 = check_on_e1(arc_start) < 1e-3;
        let end_on_e1 = check_on_e1(arc_end) < 1e-3;
        let start_on_e2 = check_on_e2(arc_start) < 1e-3;
        let end_on_e2 = check_on_e2(arc_end) < 1e-3;

        assert!(
            (start_on_e1 && end_on_e2) || (start_on_e2 && end_on_e1),
            "Both endpoints must terminate tangentially on the respective ellipses! start: {arc_start:?}, end: {arc_end:?}"
        );
    }

    #[test]
    fn arc_cont_tangent_to_circle() {
        let s = DVec3::new(0.0, 0.0, 0.0);
        let tangent = DVec3::new(1.0, 0.0, 0.0);
        let mut cmd = ArcContCommand::new(s, tangent);

        let circle = TangentObject::Circle {
            center: DVec3::new(10.0, 10.0, 0.0),
            radius: 4.0,
        };

        let hit = DVec3::new(10.0, 6.0, 0.0);
        let result = cmd.on_point_with_tangent(hit, Some(circle));
        let arc = committed_arc(result.expect("arc cont command completed with tangent circle"));

        let center = DVec3::new(arc.center.x, arc.center.y, arc.center.z);
        let start_pt = DVec3::new(
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let end_pt = DVec3::new(
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
            0.0,
        );
        assert!(start_pt.distance(s) < 1e-3 || end_pt.distance(s) < 1e-3);

        let resolved = cmd.resolved_anchor().expect("resolved anchor must exist");
        let circle_center = DVec3::new(10.0, 10.0, 0.0);
        assert!((resolved.distance(circle_center) - 4.0).abs() < 1e-3, "resolved point must be on circle circumference");

        let center_dist = center.distance(circle_center);
        let is_ext_tangent = (center_dist - (arc.radius + 4.0)).abs() < 1e-3;
        let is_int_tangent = (center_dist - (arc.radius - 4.0).abs()).abs() < 1e-3;
        assert!(is_ext_tangent || is_int_tangent, "Arc must be tangent to circle: center_dist={center_dist}, arc.radius={}", arc.radius);
    }

    #[test]
    fn arc_cont_tangent_to_line() {
        let s = DVec3::new(0.0, 0.0, 0.0);
        let tangent = DVec3::new(1.0, 0.0, 0.0);
        let mut cmd = ArcContCommand::new(s, tangent);

        let line = TangentObject::Line {
            p1: DVec3::new(5.0, 10.0, 0.0),
            p2: DVec3::new(15.0, 10.0, 0.0),
        };

        let hit = DVec3::new(10.0, 10.0, 0.0);
        let result = cmd.on_point_with_tangent(hit, Some(line));
        let arc = committed_arc(result.expect("arc cont completed with tangent line"));

        let center = DVec3::new(arc.center.x, arc.center.y, arc.center.z);
        assert!(((center.y - 10.0).abs() - arc.radius).abs() < 1e-3);

        let resolved = cmd.resolved_anchor().expect("resolved anchor must exist");
        assert!((resolved.y - 10.0).abs() < 1e-3, "resolved point must lie on the line");
    }

    #[test]
    fn arc_cont_tangent_to_ellipse() {
        let s = DVec3::new(0.0, 0.0, 0.0);
        let tangent = DVec3::new(1.0, 0.0, 0.0);
        let mut cmd = ArcContCommand::new(s, tangent);

        let ell = TangentObject::Ellipse {
            center: DVec3::new(20.0, 10.0, 0.0),
            major_axis: DVec3::new(10.0, 0.0, 0.0),
            normal: DVec3::Z,
            minor_axis_ratio: 0.5,
        };

        let hit = DVec3::new(15.0, 10.0, 0.0);
        let result = cmd.on_point_with_tangent(hit, Some(ell));
        let _arc = committed_arc(result.expect("arc cont completed with tangent ellipse"));

        let resolved = cmd.resolved_anchor().expect("resolved anchor must exist");
        let dx = (resolved.x - 20.0) / 10.0;
        let dy = (resolved.y - 10.0) / 5.0;
        assert!((dx * dx + dy * dy - 1.0).abs() < 1e-3, "resolved point must lie on ellipse");
    }
}

