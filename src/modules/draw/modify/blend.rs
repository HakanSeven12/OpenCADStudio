//! BLEND (BLE) — create a smooth spline between two open curve endpoints.
//!
//! The endpoint nearest each pick is used. Tangent continuity creates a cubic
//! Bezier (G1); curvature continuity creates a quintic Bezier whose endpoint
//! first and second derivatives reproduce the source curves' tangent and
//! geometric curvature (G2).

use acadrust::entities::{EntityCommon, Spline};
use acadrust::kernel::space::curve as space_curve;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::t;
use acadrust::kernel::space::NurbsCurve3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::entities::common::BulgeArc;
use crate::scene::model::wire_model::WireModel;
use crate::scene::view::transform::{ocs_axes, ocs_point_to_wcs};

use super::entity_index::ModifyEntityIndex;

const EPS: f64 = 1.0e-10;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Continuity {
    Tangent,
    Curvature,
}

impl Continuity {
    fn label(self) -> &'static str {
        match self {
            Self::Tangent => "Tangent",
            Self::Curvature => "Curvature",
        }
    }
}

#[derive(Clone, Copy)]
struct EndpointFrame {
    point: DVec3,
    /// Direction from the source curve interior towards its selected endpoint.
    outward: DVec3,
    /// Geometric curvature vector at the endpoint.
    curvature: DVec3,
}

#[derive(Clone, Copy)]
enum BlendStep {
    First,
    Continuity {
        resume: Option<(Handle, EndpointFrame)>,
    },
    Second {
        handle: Handle,
        endpoint: EndpointFrame,
    },
}

pub struct BlendCommand {
    continuity: Continuity,
    step: BlendStep,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
}

impl BlendCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let entity_index = ModifyEntityIndex::build(&all_entities);
        Self {
            continuity: Continuity::Curvature,
            step: BlendStep::First,
            all_entities,
            entity_index,
        }
    }

    fn entity(&self, handle: Handle) -> Option<&EntityType> {
        self.entity_index.get(&self.all_entities, handle)
    }

    fn blend_for(
        &self,
        first_handle: Handle,
        first: EndpointFrame,
        second_handle: Handle,
        click: DVec3,
    ) -> Option<Spline> {
        if first_handle == second_handle {
            return None;
        }
        let second_entity = self.entity(second_handle)?;
        let second = endpoint_frame(second_entity, click)?;
        let common = blend_common(self.entity(first_handle)?.common());
        build_blend(first, second, self.continuity, common)
    }
}

impl CadCommand for BlendCommand {
    fn name(&self) -> &'static str {
        "BLEND"
    }

    fn prompt(&self) -> String {
        match self.step {
            BlendStep::First => {
                let c = self.continuity.label();
                t!(
                    "BLEND  Select first open curve  [Continuity=%{c}]:",
                    c = c
                )
                .into_owned()
            }
            BlendStep::Continuity { .. } => {
                let c = self.continuity.label();
                t!(
                    "BLEND  Enter continuity [Tangent/Curvature] <%{c}>:",
                    c = c
                )
                .into_owned()
            }
            BlendStep::Second { .. } => {
                let c = self.continuity.label();
                t!(
                    "BLEND  Select second open curve  [Continuity=%{c}]:",
                    c = c
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            BlendStep::First | BlendStep::Second { .. } => {
                vec![CmdOption::new(t!("Continuity").as_ref(), "C")]
            }
            BlendStep::Continuity { .. } => vec![
                CmdOption::new(t!("Tangent").as_ref(), "T"),
                CmdOption::new(t!("Curvature").as_ref(), "C"),
            ],
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, BlendStep::Continuity { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_ascii_uppercase();
        match self.step {
            BlendStep::First => {
                if upper == "C" || upper == "CONTINUITY" {
                    self.step = BlendStep::Continuity { resume: None };
                    Some(CmdResult::NeedPoint)
                } else {
                    None
                }
            }
            BlendStep::Second { handle, endpoint } => {
                if upper == "C" || upper == "CONTINUITY" {
                    self.step = BlendStep::Continuity {
                        resume: Some((handle, endpoint)),
                    };
                    Some(CmdResult::NeedPoint)
                } else {
                    None
                }
            }
            BlendStep::Continuity { resume } => {
                if upper.is_empty() {
                    self.step = resume
                        .map(|(handle, endpoint)| BlendStep::Second { handle, endpoint })
                        .unwrap_or(BlendStep::First);
                    return Some(CmdResult::NeedPoint);
                }
                match upper.as_str() {
                    "T" | "TANGENT" => self.continuity = Continuity::Tangent,
                    "C" | "CURVATURE" => self.continuity = Continuity::Curvature,
                    _ => return Some(CmdResult::NeedPoint),
                }
                self.step = resume
                    .map(|(handle, endpoint)| BlendStep::Second { handle, endpoint })
                    .unwrap_or(BlendStep::First);
                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        !matches!(self.step, BlendStep::Continuity { .. })
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, click: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            BlendStep::First => {
                let Some(endpoint) = self.entity(handle).and_then(|e| endpoint_frame(e, click))
                else {
                    return CmdResult::NeedPoint;
                };
                self.step = BlendStep::Second { handle, endpoint };
                CmdResult::NeedPoint
            }
            BlendStep::Second {
                handle: first_handle,
                endpoint: first,
            } => self
                .blend_for(first_handle, first, handle, click)
                .map(EntityType::Spline)
                .map(CmdResult::CommitAndExit)
                .unwrap_or(CmdResult::NeedPoint),
            BlendStep::Continuity { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, click: DVec3) -> Vec<WireModel> {
        let BlendStep::Second {
            handle: first_handle,
            endpoint: first,
        } = self.step
        else {
            return Vec::new();
        };
        let Some(spline) = self.blend_for(first_handle, first, handle, click) else {
            return Vec::new();
        };
        vec![WireModel::solid_f64(
            "blend_preview".into(),
            sample_bezier(&spline.control_points, 64),
            WireModel::CYAN,
            false,
        )]
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if let BlendStep::Continuity { resume } = self.step {
            self.step = resume
                .map(|(handle, endpoint)| BlendStep::Second { handle, endpoint })
                .unwrap_or(BlendStep::First);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }
}

fn blend_common(source: &EntityCommon) -> EntityCommon {
    let mut common = EntityCommon::new();
    common.layer = source.layer.clone();
    common.color = source.color;
    common.line_weight = source.line_weight;
    common.linetype = source.linetype.clone();
    common.linetype_handle = source.linetype_handle;
    common.linetype_scale = source.linetype_scale;
    common.transparency = source.transparency;
    common
}

fn build_blend(
    first: EndpointFrame,
    second: EndpointFrame,
    continuity: Continuity,
    common: EntityCommon,
) -> Option<Spline> {
    let chord = second.point - first.point;
    let length = chord.length();
    if length <= EPS || !length.is_finite() {
        return None;
    }
    let t0 = first.outward.try_normalize()?;
    // The source's outward direction at the second end points away from the
    // blend. Reverse it to get the blend's arrival direction.
    let t1 = -second.outward.try_normalize()?;

    let (degree, points) = match continuity {
        Continuity::Tangent => {
            let handle = length / 3.0;
            (
                3,
                vec![
                    first.point,
                    first.point + handle * t0,
                    second.point - handle * t1,
                    second.point,
                ],
            )
        }
        Continuity::Curvature => {
            // Smaller endpoint speeds prevent a tight source curve joined over
            // a long gap from creating enormous inner control points. Changing
            // speed does not change the matched geometric curvature.
            let speed0 = endpoint_speed(length, first.curvature.length());
            let speed1 = endpoint_speed(length, second.curvature.length());
            let p0 = first.point;
            let p1 = p0 + (speed0 / 5.0) * t0;
            let p2 = 2.0 * p1 - p0 + (speed0 * speed0 / 20.0) * first.curvature;
            let p5 = second.point;
            let p4 = p5 - (speed1 / 5.0) * t1;
            let p3 = 2.0 * p4 - p5 + (speed1 * speed1 / 20.0) * second.curvature;
            (5, vec![p0, p1, p2, p3, p4, p5])
        }
    };

    let mut spline = Spline::new();
    spline.common = common;
    spline.degree = degree;
    spline.knots = Spline::generate_clamped_knots(degree as usize, points.len());
    spline.control_points = points
        .into_iter()
        .map(|p| Vector3::new(p.x, p.y, p.z))
        .collect();
    Some(spline)
}

fn endpoint_speed(chord: f64, curvature: f64) -> f64 {
    if curvature <= EPS {
        chord
    } else {
        chord.min((2.0 * chord / curvature).sqrt())
    }
}

fn endpoint_frame(entity: &EntityType, click: DVec3) -> Option<EndpointFrame> {
    match entity {
        EntityType::Line(line) => {
            let start = to_dvec(line.start);
            let end = to_dvec(line.end);
            segment_endpoint(start, end, click)
        }
        EntityType::Arc(arc) => arc_endpoint(arc, click),
        EntityType::LwPolyline(poly) if !poly.is_closed => {
            let n = poly.vertices.len();
            if n < 2 {
                return None;
            }
            let normal = to_dvec(poly.normal).normalize_or_zero();
            let selected_start = click.distance_squared(lw_point(poly, 0))
                <= click.distance_squared(lw_point(poly, n - 1));
            let segment = if selected_start { 0 } else { n - 2 };
            planar_segment_endpoint(
                [
                    poly.vertices[segment].location.x,
                    poly.vertices[segment].location.y,
                ],
                [
                    poly.vertices[segment + 1].location.x,
                    poly.vertices[segment + 1].location.y,
                ],
                poly.vertices[segment].bulge,
                poly.elevation,
                normal,
                selected_start,
            )
        }
        EntityType::Polyline2D(poly) if !poly.is_closed() => {
            let filtered = crate::entities::polyline::drawn_vertices2d(poly);
            let vertices = filtered.as_deref().unwrap_or(&poly.vertices);
            let n = vertices.len();
            if n < 2 {
                return None;
            }
            let normal = to_dvec(poly.normal).normalize_or_zero();
            let first = ocs_to_dvec(
                vertices[0].location.x,
                vertices[0].location.y,
                poly.elevation,
                normal,
            );
            let last = ocs_to_dvec(
                vertices[n - 1].location.x,
                vertices[n - 1].location.y,
                poly.elevation,
                normal,
            );
            let selected_start = click.distance_squared(first) <= click.distance_squared(last);
            let segment = if selected_start { 0 } else { n - 2 };
            planar_segment_endpoint(
                [vertices[segment].location.x, vertices[segment].location.y],
                [
                    vertices[segment + 1].location.x,
                    vertices[segment + 1].location.y,
                ],
                vertices[segment].bulge,
                poly.elevation,
                normal,
                selected_start,
            )
        }
        EntityType::Polyline(poly) if !poly.is_closed() => {
            let points: Vec<_> = poly.vertices.iter().map(|v| to_dvec(v.location)).collect();
            polyline_endpoint(&points, click)
        }
        EntityType::Polyline3D(poly) if !poly.is_closed() => {
            let points: Vec<_> = poly.vertices.iter().map(|v| to_dvec(v.position)).collect();
            polyline_endpoint(&points, click)
        }
        EntityType::Spline(spline) if !spline.flags.closed && !spline.flags.periodic => {
            spline_endpoint(spline, click)
        }
        _ => None,
    }
}

fn segment_endpoint(start: DVec3, end: DVec3, click: DVec3) -> Option<EndpointFrame> {
    let direction = (end - start).try_normalize()?;
    if click.distance_squared(start) <= click.distance_squared(end) {
        Some(EndpointFrame {
            point: start,
            outward: -direction,
            curvature: DVec3::ZERO,
        })
    } else {
        Some(EndpointFrame {
            point: end,
            outward: direction,
            curvature: DVec3::ZERO,
        })
    }
}

fn polyline_endpoint(points: &[DVec3], click: DVec3) -> Option<EndpointFrame> {
    if points.len() < 2 {
        return None;
    }
    if click.distance_squared(points[0]) <= click.distance_squared(points[points.len() - 1]) {
        segment_endpoint(points[0], points[1], points[0])
    } else {
        segment_endpoint(
            points[points.len() - 2],
            points[points.len() - 1],
            points[points.len() - 1],
        )
    }
}

fn arc_endpoint(arc: &acadrust::entities::Arc, click: DVec3) -> Option<EndpointFrame> {
    if arc.radius <= EPS {
        return None;
    }
    let normal_tuple = (arc.normal.x, arc.normal.y, arc.normal.z);
    let normal = to_dvec(arc.normal).try_normalize()?;
    let (ax, ay) = ocs_axes(normal_tuple);
    let axis_x = DVec3::new(ax.0, ax.1, ax.2);
    let axis_y = DVec3::new(ay.0, ay.1, ay.2);
    let center = tuple_to_dvec(ocs_point_to_wcs(
        (arc.center.x, arc.center.y, arc.center.z),
        normal_tuple,
    ));
    let at = |angle: f64| center + arc.radius * (angle.cos() * axis_x + angle.sin() * axis_y);
    let start = at(arc.start_angle);
    let end = at(arc.end_angle);
    let (point, angle, at_start) = if click.distance_squared(start) <= click.distance_squared(end) {
        (start, arc.start_angle, true)
    } else {
        (end, arc.end_angle, false)
    };
    let tangent = normal
        .cross(point - center)
        .try_normalize()
        .or_else(|| (-angle.sin() * axis_x + angle.cos() * axis_y).try_normalize())?;
    Some(EndpointFrame {
        point,
        outward: if at_start { -tangent } else { tangent },
        curvature: (center - point) / (arc.radius * arc.radius),
    })
}

fn lw_point(poly: &acadrust::entities::LwPolyline, index: usize) -> DVec3 {
    let normal = to_dvec(poly.normal).normalize_or_zero();
    let p = poly.vertices[index].location;
    ocs_to_dvec(p.x, p.y, poly.elevation, normal)
}

fn planar_segment_endpoint(
    p0: [f64; 2],
    p1: [f64; 2],
    bulge: f64,
    elevation: f64,
    normal: DVec3,
    selected_start: bool,
) -> Option<EndpointFrame> {
    if normal.length_squared() <= EPS {
        return None;
    }
    let start = ocs_to_dvec(p0[0], p0[1], elevation, normal);
    let end = ocs_to_dvec(p1[0], p1[1], elevation, normal);
    let Some(arc) = BulgeArc::from_bulge(p0, p1, bulge) else {
        return segment_endpoint(start, end, if selected_start { start } else { end });
    };
    let center = ocs_to_dvec(arc.center[0], arc.center[1], elevation, normal);
    let point = if selected_start { start } else { end };
    let traversal_tangent = (normal.cross(point - center) * bulge.signum()).try_normalize()?;
    Some(EndpointFrame {
        point,
        outward: if selected_start {
            -traversal_tangent
        } else {
            traversal_tangent
        },
        curvature: (center - point) / (arc.radius * arc.radius),
    })
}

fn spline_endpoint(spline: &Spline, click: DVec3) -> Option<EndpointFrame> {
    if spline.control_points.len() >= 2 {
        return spline_control_endpoint(spline, click);
    }
    let points: Vec<_> = spline.fit_points.iter().copied().map(to_dvec).collect();
    if points.len() < 2 {
        return None;
    }
    let at_start =
        click.distance_squared(points[0]) <= click.distance_squared(points[points.len() - 1]);
    let index = if at_start { 0 } else { points.len() - 1 };
    let tangent = if at_start {
        let stored = to_dvec(spline.begin_tangent);
        if stored.length_squared() > EPS {
            stored
        } else {
            points[1] - points[0]
        }
    } else {
        let stored = to_dvec(spline.end_tangent);
        if stored.length_squared() > EPS {
            stored
        } else {
            points[index] - points[index - 1]
        }
    }
    .try_normalize()?;
    let curvature = if points.len() >= 3 {
        if at_start {
            circumcircle_curvature(points[0], points[1], points[2])
        } else {
            circumcircle_curvature(points[index], points[index - 1], points[index - 2])
        }
    } else {
        DVec3::ZERO
    };
    Some(EndpointFrame {
        point: points[index],
        outward: if at_start { -tangent } else { tangent },
        curvature,
    })
}

fn spline_control_endpoint(spline: &Spline, click: DVec3) -> Option<EndpointFrame> {
    let count = spline.control_points.len();
    let degree = usize::try_from(spline.degree).ok()?;
    if degree == 0 || count <= degree {
        return None;
    }
    // The kernel's space curve holds the rational and the polynomial case
    // alike — weights absent means polynomial — so there is nothing here to
    // tell apart, and a malformed knot vector is replaced with a clamped
    // uniform one rather than refused.
    let controls: Vec<[f64; 3]> = spline
        .control_points
        .iter()
        .map(|point| [point.x, point.y, point.z])
        .collect();
    let weights = (spline.weights.len() == count).then(|| {
        spline
            .weights
            .iter()
            .map(|weight| if weight.abs() <= EPS { 1.0 } else { *weight })
            .collect()
    });
    let curve = NurbsCurve3::new(degree, controls, spline.knots.clone(), weights)?;

    let (start, end) = curve.domain();
    let at_start = click.distance_squared(point_to_dvec(curve.point_at_knot(start)))
        <= click.distance_squared(point_to_dvec(curve.point_at_knot(end)));
    let parameter = if at_start { start } else { end };
    curve_frame(
        point_to_dvec(curve.point_at_knot(parameter)),
        point_to_dvec(curve.tangent_at_knot(parameter)),
        point_to_dvec(curve.acceleration_at_knot(parameter)),
        at_start,
    )
}

fn curve_frame(
    point: DVec3,
    first_derivative: DVec3,
    second_derivative: DVec3,
    at_start: bool,
) -> Option<EndpointFrame> {
    let speed_squared = first_derivative.length_squared();
    if speed_squared <= EPS {
        return None;
    }
    let tangent = first_derivative / speed_squared.sqrt();
    let normal_second = second_derivative - tangent * second_derivative.dot(tangent);
    Some(EndpointFrame {
        point,
        outward: if at_start { -tangent } else { tangent },
        curvature: normal_second / speed_squared,
    })
}

/// The curvature vector at `point` of the circle through the three points.
fn circumcircle_curvature(point: DVec3, next: DVec3, third: DVec3) -> DVec3 {
    let curvature = space_curve::curvature_through(
        [point.x, point.y, point.z],
        [next.x, next.y, next.z],
        [third.x, third.y, third.z],
    );
    DVec3::new(curvature[0], curvature[1], curvature[2])
}

/// A Bézier control polygon sampled into a polyline.
///
/// De Casteljau, from the kernel. What was here summed Bernstein terms with
/// binomial coefficients out of a lookup table that had entries for degrees
/// three and five and returned one for everything else — correct only for
/// the two the blend happens to build today, and quietly wrong the moment a
/// third was added.
fn sample_bezier(control_points: &[Vector3], segments: usize) -> Vec<[f64; 3]> {
    let control: Vec<[f64; 3]> = control_points
        .iter()
        .map(|p| [p.x, p.y, p.z])
        .collect();
    space_curve::bezier_points(&control, segments)
}

fn ocs_to_dvec(x: f64, y: f64, z: f64, normal: DVec3) -> DVec3 {
    tuple_to_dvec(ocs_point_to_wcs((x, y, z), (normal.x, normal.y, normal.z)))
}

fn tuple_to_dvec(point: (f64, f64, f64)) -> DVec3 {
    DVec3::new(point.0, point.1, point.2)
}

fn to_dvec(point: Vector3) -> DVec3 {
    DVec3::new(point.x, point.y, point.z)
}

fn point_to_dvec(point: [f64; 3]) -> DVec3 {
    DVec3::new(point[0], point[1], point[2])
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["BLEND", "BLE"]
});
