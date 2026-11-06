// Ellipse tool — ribbon dropdown + all OpenCADStudio ellipse creation methods.
//
// Commands:
//   ELLIPSE      — Center, Axes  (center → major endpoint → minor distance)
//   ELLIPSE_AXIS — Axis, End     (axis endpoint 1 → endpoint 2 → minor distance)
//   ELLIPSE_ARC  — Ellipse Arc   (shape as above, then start/end parametric angles)

use crate::t;
use document::types::Vector3;
use document::{Ellipse, EntityType};

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::wire_model::{TangentGeom, WireModel};
use glam::DVec3;

fn parse_num(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse().ok()
}

const TAU: f64 = std::f64::consts::TAU;

/// Minimum swept angle before the previewed arc may flip CW/CCW — filters the
/// per-frame cursor jitter that otherwise reverses the sweep on tiny moves.
const DIR_TOL: f64 = 0.1745; // ~10°

// ── Icons ─────────────────────────────────────────────────────────────────

const ICON_CTR: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/ellipse/ellipse_ctr.svg"
));
const ICON_AXIS: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/ellipse/ellipse_axis.svg"
));
const ICON_ARC: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/ellipse/ellipse_arc.svg"
));

// ── Dropdown metadata ─────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "ELLIPSE";

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("ELLIPSE", "Center, Axes", ICON_CTR),
    ("ELLIPSE_AXIS", "Axis, End", ICON_AXIS),
    ("ELLIPSE_ARC", "Ellipse Arc", ICON_ARC),
];

pub const ICON: IconKind = ICON_CTR;

// ── Shared helpers ────────────────────────────────────────────────────────

/// Preview wire for a full or partial ellipse.
fn ellipse_wire(
    center: DVec3,
    major: DVec3, // vector from center to major-axis endpoint
    ratio: f64,   // minor/major
    t_start: f64,
    t_end: f64,
    plane: WorkingPlane,
) -> WireModel {
    let r_major = major.length();
    if r_major < 1e-9 {
        return WireModel::solid("rubber_band".into(), vec![], WireModel::CYAN, false);
    }
    let major_dir = major / r_major;
    let v = plane.z.cross(major_dir).normalize_or(plane.y);
    let segs = 64u32;
    // Unwrap t_end so the arc goes counter-clockwise.
    let t_e = if t_end <= t_start { t_end + TAU } else { t_end };
    let pts: Vec<[f64; 3]> = (0..=segs)
        .map(|i| {
            let t = t_start + (t_e - t_start) * (i as f64 / segs as f64);
            let p = center + t.cos() * r_major * major_dir + t.sin() * r_major * ratio * v;
            [p.x, p.y, p.z]
        })
        .collect();
    let mut wire = WireModel::solid_f64("rubber_band".into(), pts, WireModel::CYAN, false);
    wire.tangent_geoms.push(TangentGeom::PlanarEllipse {
        center: [center.x, center.y, center.z],
        major_axis: [major.x, major.y, major.z],
        normal: plane.z.to_array(),
        minor_axis_ratio: ratio,
        start_param: t_start,
        end_param: t_e,
    });
    wire
}

/// Convert a world point to the parametric angle on the ellipse.
fn param_angle(center: DVec3, major_dir: DVec3, v: DVec3, pt: DVec3, ratio: f64) -> f64 {
    let d = pt - center;
    let u_proj = d.dot(major_dir);
    let v_proj = d.dot(v);
    // Inverse-map: on ellipse x=cos(t)*r_major, y=sin(t)*r_major*ratio
    // → t = atan2(v_proj / (r_major*ratio), u_proj / r_major) but we normalise
    v_proj.atan2(u_proj * ratio).rem_euclid(TAU)
}

/// Build the final entity: a true ellipse, or under PELLIPSE 1 its
/// polyline approximation.
fn make_ellipse(
    center: DVec3,
    major: DVec3,
    ratio: f64,
    t_start: f64,
    t_end: f64,
    plane: WorkingPlane,
) -> EntityType {
    if crate::modules::draw::defaults::get_pellipse() {
        if let Some(polyline) = ellipse_polyline(center, major, ratio, t_start, t_end, plane) {
            return polyline;
        }
    }
    EntityType::Ellipse(Ellipse {
        center: Vector3::new(center.x, center.y, center.z),
        major_axis: Vector3::new(major.x, major.y, major.z),
        minor_axis_ratio: ratio,
        start_parameter: t_start,
        end_parameter: t_end,
        normal: Vector3::new(plane.z.x, plane.z.y, plane.z.z),
        ..Default::default()
    })
}

/// PELLIPSE 1: two tangent arcs per eighth of the ellipse (16 for a full
/// one), fitted through on-curve points with the exact tangents there. An
/// elliptical arc becomes an open polyline over its sweep.
fn ellipse_polyline(
    center: DVec3,
    major: DVec3,
    ratio: f64,
    t_start: f64,
    t_end: f64,
    plane: WorkingPlane,
) -> Option<EntityType> {
    use document::entities::{LwPolyline, LwVertex};
    use document::types::Vector2;
    let r_major = major.length();
    if r_major < 1e-9 {
        return None;
    }
    let v = plane.z.cross(major / r_major).normalize_or(plane.y) * r_major * ratio;
    let sweep = (t_end - t_start).rem_euclid(TAU);
    let closed = sweep < 1e-9;
    let sweep = if closed { TAU } else { sweep };
    let spans = (sweep / (TAU / 8.0) - 1e-9).ceil().max(1.0) as usize;
    let count = if closed { spans } else { spans + 1 };
    let (mut points, mut tangents) = (Vec::with_capacity(count), Vec::with_capacity(count));
    for i in 0..count {
        let t = t_start + sweep * i as f64 / spans as f64;
        let point = plane.to_local(center + t.cos() * major + t.sin() * v);
        let tangent = plane.vector_to_local(-t.sin() * major + t.cos() * v);
        points.push([point.x, point.y]);
        tangents.push(Some([tangent.x, tangent.y]));
    }
    let fitted = kernel::geom2d::fit_arc_chain(&points, closed, &tangents)?;
    let mut polyline = LwPolyline::new();
    polyline.is_closed = closed;
    polyline.elevation = plane.to_local(center).z;
    polyline.vertices = fitted
        .into_iter()
        .map(|fitted| {
            let mut vertex = LwVertex::new(Vector2::new(fitted.point[0], fitted.point[1]));
            vertex.bulge = fitted.bulge;
            vertex
        })
        .collect();
    Some(plane.place_entity(EntityType::LwPolyline(polyline)))
}

// ── 1. Center mode ────────────────────────────────────────────────────────
//   Step 1: center   Step 2: major-axis endpoint   Step 3: minor-axis point

enum CtrStep {
    Center,
    MajorAxis { center: DVec3 },
    MinorRatio { center: DVec3, major: DVec3 },
}

pub struct EllipseCommand {
    step: CtrStep,
    plane: WorkingPlane,
}

impl EllipseCommand {
    pub fn new() -> Self {
        Self {
            step: CtrStep::Center,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for EllipseCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ELLIPSE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            CtrStep::Center => t!("ELLIPSE  Specify center:").into_owned(),
            CtrStep::MajorAxis { .. } => t!("ELLIPSE  Specify major axis endpoint:").into_owned(),
            CtrStep::MinorRatio { major, .. } => crate::tf!(
                "ELLIPSE  Specify minor axis point or type half-length  [major r={:.3}]:",
                major.length()
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            CtrStep::Center => vec![CmdOption::new("Arc", "ARC"), CmdOption::new("Axis", "AXIS")],
            _ => vec![],
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, CtrStep::Center)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            CtrStep::Center => {
                self.step = CtrStep::MajorAxis { center: pt };
                CmdResult::NeedPoint
            }
            CtrStep::MajorAxis { center } => {
                let center = *center;
                self.step = CtrStep::MinorRatio {
                    center,
                    major: pt - center,
                };
                CmdResult::NeedPoint
            }
            CtrStep::MinorRatio { center, major } => {
                let (center, major) = (*center, *major);
                let ratio = minor_ratio(center, major, pt);
                CmdResult::CommitAndExit(make_ellipse(
                    center, major, ratio, 0.0, TAU, self.plane,
                ))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // At the centre step, keyword options switch construction method by
        // handing off to the dedicated variant command.
        if matches!(self.step, CtrStep::Center) {
            return match text.trim().to_uppercase().as_str() {
                "A" | "ARC" => Some(CmdResult::Dispatch("ELLIPSE_ARC".into())),
                "AXIS" => Some(CmdResult::Dispatch("ELLIPSE_AXIS".into())),
                _ => None,
            };
        }
        if let CtrStep::MinorRatio { center, major } = &self.step {
            let r_minor = parse_num(text)?;
            if r_minor > 0.0 {
                let ratio = (r_minor / major.length()).clamp(1e-6, 1.0);
                return Some(CmdResult::CommitAndExit(make_ellipse(
                    *center, *major, ratio, 0.0, TAU, self.plane,
                )));
            }
        }
        None
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match &self.step {
            CtrStep::MajorAxis { center } => Some(line_wire(*center, pt)),
            CtrStep::MinorRatio { center, major } => {
                let ratio = minor_ratio(*center, *major, pt).max(0.001);
                Some(ellipse_wire(*center, *major, ratio, 0.0, TAU, self.plane))
            }
            _ => None,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match &self.step {
            // Center + major axis endpoint: ordinary point picks (legacy polar
            // anchored at the previous point).
            CtrStep::Center | CtrStep::MajorAxis { .. } => None,
            // Minor axis: half-length measured square to the major axis. Show
            // the perpendicular drop from the cursor onto the major axis.
            CtrStep::MinorRatio { center, major } => Some(DynSpec {
                anchor: DynAnchor::Point(*center),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::Perp,
                ref_point: Some(*center + *major),
            }),
        }
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if let CtrStep::MinorRatio { center, major } = &self.step {
            Some(minor_ratio(*center, *major, cursor) * major.length())
        } else {
            None
        }
    }
}

// ── 2. Axis, End mode ─────────────────────────────────────────────────────
//   Step 1: axis endpoint 1   Step 2: axis endpoint 2   Step 3: minor-axis point

enum AxisStep {
    Pt1,
    Pt2 { p1: DVec3 },
    MinorRatio { center: DVec3, major: DVec3 },
}

pub struct EllipseAxisCommand {
    step: AxisStep,
    plane: WorkingPlane,
}

impl EllipseAxisCommand {
    pub fn new() -> Self {
        Self {
            step: AxisStep::Pt1,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for EllipseAxisCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ELLIPSE_AXIS"
    }

    fn prompt(&self) -> String {
        match &self.step {
            AxisStep::Pt1 => {
                t!("ELLIPSE (Axis)  Specify first endpoint of major axis:").into_owned()
            }
            AxisStep::Pt2 { .. } => {
                t!("ELLIPSE (Axis)  Specify second endpoint of major axis:").into_owned()
            }
            AxisStep::MinorRatio { major, .. } => crate::tf!(
                "ELLIPSE (Axis)  Specify minor axis point or type half-length  [major r={:.3}]:",
                major.length()
            )
            .into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            AxisStep::Pt1 => {
                self.step = AxisStep::Pt2 { p1: pt };
                CmdResult::NeedPoint
            }
            AxisStep::Pt2 { p1 } => {
                let center = (*p1 + pt) * 0.5;
                let major = pt - center; // half-vector
                self.step = AxisStep::MinorRatio { center, major };
                CmdResult::NeedPoint
            }
            AxisStep::MinorRatio { center, major } => {
                let (center, major) = (*center, *major);
                let ratio = minor_ratio(center, major, pt);
                CmdResult::CommitAndExit(make_ellipse(
                    center, major, ratio, 0.0, TAU, self.plane,
                ))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let AxisStep::MinorRatio { center, major } = &self.step {
            let r_minor = parse_num(text)?;
            if r_minor > 0.0 {
                let ratio = (r_minor / major.length()).clamp(1e-6, 1.0);
                return Some(CmdResult::CommitAndExit(make_ellipse(
                    *center, *major, ratio, 0.0, TAU, self.plane,
                )));
            }
        }
        None
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match &self.step {
            AxisStep::Pt1 => None,
            AxisStep::Pt2 { p1 } => Some(line_wire(*p1, pt)),
            AxisStep::MinorRatio { center, major } => {
                let ratio = minor_ratio(*center, *major, pt).max(0.001);
                Some(ellipse_wire(*center, *major, ratio, 0.0, TAU, self.plane))
            }
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match &self.step {
            // Endpoints define the full major axis — legacy polar (anchored at
            // the previous point) is right.
            AxisStep::Pt1 | AxisStep::Pt2 { .. } => None,
            // Minor half-length, square to the major axis.
            AxisStep::MinorRatio { center, major } => Some(DynSpec {
                anchor: DynAnchor::Point(*center),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::Perp,
                ref_point: Some(*center + *major),
            }),
        }
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if let AxisStep::MinorRatio { center, major } = &self.step {
            Some(minor_ratio(*center, *major, cursor) * major.length())
        } else {
            None
        }
    }
}

// ── 3. Ellipse Arc mode ───────────────────────────────────────────────────
//   Same shape steps as Center mode, then: start parameter, end parameter.

enum ArcStep {
    Center,
    MajorAxis {
        center: DVec3,
    },
    MinorRatio {
        center: DVec3,
        major: DVec3,
    },
    StartAngle {
        center: DVec3,
        major: DVec3,
        ratio: f64,
    },
    EndAngle {
        center: DVec3,
        major: DVec3,
        ratio: f64,
        t_start: f64,
    },
}

pub struct EllipseArcCommand {
    step: ArcStep,
    prev_pt: Option<DVec3>,
    cw: bool,
    plane: WorkingPlane,
}

impl EllipseArcCommand {
    pub fn new() -> Self {
        Self {
            step: ArcStep::Center,
            prev_pt: None,
            cw: false,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for EllipseArcCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ELLIPSE_ARC"
    }

    fn prompt(&self) -> String {
        match &self.step {
            ArcStep::Center => t!("ELLIPSE ARC  Specify center:").into_owned(),
            ArcStep::MajorAxis { .. } => {
                t!("ELLIPSE ARC  Specify major axis endpoint:").into_owned()
            }
            ArcStep::MinorRatio { major, .. } => {
                let r = format!("{:.3}", major.length());
                t!(
                    "ELLIPSE ARC  Specify minor axis point or type half-length  [major r=%{r}]:",
                    r = r
                )
                .into_owned()
            }
            ArcStep::StartAngle { .. } => {
                t!("ELLIPSE ARC  Specify start angle point or type degrees:").into_owned()
            }
            ArcStep::EndAngle { t_start, .. } => {
                let sa = format!("{:.1}°", t_start.to_degrees());
                t!(
                    "ELLIPSE ARC  Specify end angle point or type degrees  [start=%{sa}]:",
                    sa = sa
                )
                .into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            ArcStep::Center => {
                self.step = ArcStep::MajorAxis { center: pt };
                CmdResult::NeedPoint
            }
            ArcStep::MajorAxis { center } => {
                let center = *center;
                self.step = ArcStep::MinorRatio {
                    center,
                    major: pt - center,
                };
                CmdResult::NeedPoint
            }
            ArcStep::MinorRatio { center, major } => {
                let (center, major) = (*center, *major);
                let ratio = minor_ratio(center, major, pt);
                self.step = ArcStep::StartAngle {
                    center,
                    major,
                    ratio,
                };
                CmdResult::NeedPoint
            }
            ArcStep::StartAngle {
                center,
                major,
                ratio,
            } => {
                let (center, major, ratio) = (*center, *major, *ratio);
                let t_start = angle_from_point(center, major, ratio, pt, self.plane);
                self.prev_pt = None; // reset direction tracking for end-angle step
                self.step = ArcStep::EndAngle {
                    center,
                    major,
                    ratio,
                    t_start,
                };
                CmdResult::NeedPoint
            }
            ArcStep::EndAngle {
                center,
                major,
                ratio,
                t_start,
            } => {
                let (center, major, ratio, t_start) = (*center, *major, *ratio, *t_start);
                let t_end = angle_from_point(center, major, ratio, pt, self.plane);
                let entity = if self.cw {
                    make_ellipse(center, major, ratio, t_end, t_start, self.plane)
                } else {
                    make_ellipse(center, major, ratio, t_start, t_end, self.plane)
                };
                CmdResult::CommitAndExit(entity)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let angle_step = matches!(
            &self.step,
            ArcStep::StartAngle { .. } | ArcStep::EndAngle { .. }
        );
        let val = if angle_step {
            crate::entities::common::parse_typed_angle(text)?
        } else {
            parse_num(text)?
        };
        match &self.step {
            ArcStep::MinorRatio { center, major } => {
                if val > 0.0 {
                    let ratio = (val / major.length()).clamp(1e-6, 1.0);
                    let (c, m) = (*center, *major);
                    self.step = ArcStep::StartAngle {
                        center: c,
                        major: m,
                        ratio,
                    };
                    return Some(CmdResult::NeedPoint);
                }
            }
            ArcStep::StartAngle {
                center,
                major,
                ratio,
            } => {
                let t_start = val;
                let (c, m, r) = (*center, *major, *ratio);
                self.prev_pt = None;
                self.step = ArcStep::EndAngle {
                    center: c,
                    major: m,
                    ratio: r,
                    t_start,
                };
                return Some(CmdResult::NeedPoint);
            }
            ArcStep::EndAngle {
                center,
                major,
                ratio,
                t_start,
            } => {
                // Positive is CCW; negative is CW.
                let t_end = val;
                return Some(CmdResult::CommitAndExit(make_ellipse(
                    *center, *major, *ratio, *t_start, t_end, self.plane,
                )));
            }
            _ => {}
        }
        None
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match &self.step {
            ArcStep::MajorAxis { center } => Some(line_wire(*center, pt)),
            ArcStep::MinorRatio { center, major } => {
                let ratio = minor_ratio(*center, *major, pt).max(0.001);
                Some(ellipse_wire(*center, *major, ratio, 0.0, TAU, self.plane))
            }
            ArcStep::StartAngle { center, .. } => {
                // Only the start angle is being chosen here — show a line from
                // the centre to the cursor to indicate that angle, not a
                // (misleading) full arc preview.
                Some(line_wire(*center, pt))
            }
            ArcStep::EndAngle {
                center,
                major,
                ratio,
                t_start,
            } => {
                // Detect sweep direction from the change in PARAMETRIC angle
                // (the visual sweep along the ellipse), not the geometric angle
                // about the centre — on a flat ellipse a large visible move can
                // be a tiny centre angle, which made the direction stick. A
                // tolerance ignores jitter; the reference advances only on a
                // clear move so slow sweeps accumulate.
                if let Some(prev) = self.prev_pt {
                    let t_prev = angle_from_point(*center, *major, *ratio, prev, self.plane);
                    let t_cur = angle_from_point(*center, *major, *ratio, pt, self.plane);
                    let mut d = t_cur - t_prev;
                    while d > std::f64::consts::PI {
                        d -= TAU;
                    }
                    while d <= -std::f64::consts::PI {
                        d += TAU;
                    }
                    if d.abs() > DIR_TOL {
                        self.cw = d < 0.0;
                        self.prev_pt = Some(pt);
                    }
                } else {
                    self.prev_pt = Some(pt);
                }
                let t_end = angle_from_point(*center, *major, *ratio, pt, self.plane);
                Some(if self.cw {
                    ellipse_wire(*center, *major, *ratio, t_end, *t_start, self.plane)
                } else {
                    ellipse_wire(*center, *major, *ratio, *t_start, t_end, self.plane)
                })
            }
            _ => None,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        match &self.step {
            ArcStep::Center | ArcStep::MajorAxis { .. } => None,
            // Minor half-length, square to the major axis.
            ArcStep::MinorRatio { center, major } => Some(DynSpec {
                anchor: DynAnchor::Point(*center),
                fields: vec![DynFieldSpec::new(DynRole::Distance)],
                guide: DynGuide::Perp,
                ref_point: Some(*center + *major),
            }),
            // Start / end sweep angles measured at the centre (the last point
            // is the previous pick, so anchor the angle arc at the centre).
            ArcStep::StartAngle { center, .. } | ArcStep::EndAngle { center, .. } => {
                Some(DynSpec {
                    anchor: DynAnchor::Point(*center),
                    fields: vec![DynFieldSpec::new(DynRole::Angle)],
                    guide: DynGuide::Polar,
                    ref_point: None,
                })
            }
        }
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if let ArcStep::MinorRatio { center, major } = &self.step {
            Some(minor_ratio(*center, *major, cursor) * major.length())
        } else {
            None
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────

fn minor_ratio(center: DVec3, major: DVec3, pt: DVec3) -> f64 {
    let r_major = major.length();
    if r_major < 1e-9 {
        return 0.5;
    }
    let major_dir = major / r_major;
    let to_pt = pt - center;
    let perp = to_pt - major_dir * to_pt.dot(major_dir);
    let r_minor = perp.length().max(1e-6);
    (r_minor / r_major).clamp(1e-6, 1.0)
}

fn angle_from_point(
    center: DVec3,
    major: DVec3,
    ratio: f64,
    pt: DVec3,
    plane: WorkingPlane,
) -> f64 {
    let r_major = major.length();
    if r_major < 1e-9 {
        return 0.0;
    }
    let major_dir = major / r_major;
    let v = plane.z.cross(major_dir).normalize_or(plane.y);
    param_angle(center, major_dir, v, pt, ratio)
}

fn line_wire(from: DVec3, to: DVec3) -> WireModel {
    WireModel::solid_f64(
        "rubber_band".into(),
        vec![[from.x, from.y, from.z], [to.x, to.y, to.z]],
        WireModel::CYAN,
        false,
    )
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["ELLIPSE_ARC"]
}); // EllipseArcCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["ELLIPSE_AXIS"]
}); // EllipseAxisCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["ELLIPSE"]
}); // EllipseCommand

#[cfg(test)]
mod tests {
    use super::*;

    /// PELLIPSE 1: a full ellipse is a closed 16-arc polyline, an elliptical
    /// arc an open one; every vertex lies on the ellipse.
    #[test]
    fn pellipse_polyline_has_sixteen_arcs_on_the_ellipse() {
        let plane = WorkingPlane::default();
        let center = DVec3::new(5.0, 2.0, 1.5);
        let major = DVec3::new(4.0, 0.0, 0.0);
        let on_ellipse = |entity: &EntityType| {
            let EntityType::LwPolyline(p) = entity else {
                panic!("expected a polyline");
            };
            // Even vertices are the samples; odd ones the arc joints between.
            for (i, vertex) in p.vertices.iter().enumerate() {
                let x = (vertex.location.x - center.x) / 4.0;
                let y = (vertex.location.y - center.y) / 2.0;
                let error = (x * x + y * y - 1.0).abs();
                assert!(error < if i % 2 == 0 { 1e-9 } else { 0.03 }, "{i}: {error}");
            }
            assert!((p.elevation - center.z).abs() < 1e-9);
            p.clone()
        };
        let full = ellipse_polyline(center, major, 0.5, 0.0, TAU, plane).unwrap();
        let full = on_ellipse(&full);
        assert!(full.is_closed);
        assert_eq!(full.vertices.len(), 16);
        assert!(full.vertices.iter().all(|v| v.bulge > 0.0));

        let arc = ellipse_polyline(center, major, 0.5, 0.0, TAU / 4.0, plane).unwrap();
        let arc = on_ellipse(&arc);
        assert!(!arc.is_closed);
        assert_eq!(arc.vertices.len(), 5);
    }
}
