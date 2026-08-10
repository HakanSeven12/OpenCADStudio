// EXTRUDE / REVOLVE: turning a drawn profile into a solid.
//
// The profile comes from `entities::curve`, which is where every entity's
// geometry is already defined once, so a circle, an arc-bulged polyline and a
// closed spline all arrive as the same thing: a plane plus a chain of curves
// in that plane's coordinates. The kernel sweeps that chain into analytic
// surfaces — a straight run into a plane, an arc into a cylinder, a profile
// turned about an axis into a cone, a sphere or a torus — so the result can
// be written back out as ACIS rather than as facets.
//
// A spline profile has no analytic side wall, so it is refused rather than
// approximated into a surface nobody asked for. That is the kernel's answer
// and this module passes it on.

use acadrust::kernel::brep::{self, Body};
use acadrust::kernel::geom2d::Curve;
use acadrust::kernel::space::{PlanarCurve, Plane, Vec3};
use acadrust::EntityType;

use crate::entities::curve::entity_curve;

/// A drawn profile, as the kernel wants it: the plane it lies in and the
/// chain of pieces closing a loop in that plane.
pub struct Profile {
    pub plane: Plane,
    pub pieces: Vec<Curve>,
}

/// The closed profile an entity describes, or `None` when it does not
/// describe one.
///
/// A single closed curve — a circle, an ellipse, a closed polyline — is what
/// a sweep needs. The kernel wants it as a chain of at least three pieces,
/// which a polyline already is and which a circle is not, so a round profile
/// is handed over as the arcs it is made of rather than as one curve with
/// nowhere for the sweep to start.
pub fn profile_of(entity: &EntityType) -> Option<Profile> {
    let planar: PlanarCurve = entity_curve(entity)?;
    if !planar.curve.is_closed() {
        return None;
    }
    let pieces = match &planar.curve {
        // A polyline is already a chain. Its own segments carry the bulges,
        // so an arc stays an arc rather than becoming a run of chords.
        Curve::Polyline(_) => planar.curve.segments(),
        // Everything else closed on itself is cut into quarters, which is
        // both the fewest pieces a chain may have and the fewest that leave
        // each one unambiguous about which way round it goes.
        Curve::Circle(circle) => quarters(circle.centre, circle.radius),
        other => split_evenly(other, 4),
    };
    (pieces.len() >= 3).then_some(Profile {
        plane: planar.plane,
        pieces,
    })
}

/// A circle as four arcs.
fn quarters(centre: [f64; 2], radius: f64) -> Vec<Curve> {
    use acadrust::kernel::geom2d::Arc;
    use std::f64::consts::FRAC_PI_2;
    (0..4)
        .map(|quarter| {
            let start = FRAC_PI_2 * quarter as f64;
            Curve::Arc(Arc {
                centre,
                radius,
                start_angle: start,
                end_angle: start + FRAC_PI_2,
            })
        })
        .collect()
}

/// Any other closed curve as `count` straight pieces between points on it.
///
/// The honest fallback: an ellipse or a spline has no analytic sweep, so the
/// kernel would refuse the exact form anyway. Chords at least say plainly
/// what they are.
fn split_evenly(curve: &Curve, count: usize) -> Vec<Curve> {
    use acadrust::kernel::geom2d::Line;
    (0..count)
        .map(|step| Curve::Line(Line {
            start: curve.point_at(step as f64 / count as f64),
            end: curve.point_at((step + 1) as f64 / count as f64),
        }))
        .collect()
}

/// EXTRUDE: drag the profile `height` along its own plane's normal.
///
/// `None` for a profile that does not close, encloses nothing, or holds a
/// piece with no analytic side wall.
pub fn extruded(entity: &EntityType, height: f64) -> Option<Body> {
    let profile = profile_of(entity)?;
    let normal = profile.plane.normal()?;
    brep::extrude(
        profile.plane,
        &profile.pieces,
        [normal[0] * height, normal[1] * height, normal[2] * height],
    )
}

/// REVOLVE: turn the profile about the axis from `from` to `to` by `angle`
/// radians.
///
/// The axis has to lie in the profile's plane — a profile and an axis that do
/// not share one sweep into surfaces with no analytic form, and the kernel
/// refuses rather than approximating them.
pub fn revolved(
    entity: &EntityType,
    from: [f64; 3],
    to: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let profile = profile_of(entity)?;
    brep::revolve(
        profile.plane,
        &profile.pieces,
        from,
        [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
        angle,
    )
}

// ── SWEEP and LOFT ──────────────────────────────────────────────────────────
//
// Neither keeps a B-rep — both have only ever produced a mesh — so both are
// built from point lists rather than from topology. A profile becomes the
// points its own curve tessellates to, which is the same source EXTRUDE and
// REVOLVE read, so a circle stays round here too.

use crate::entities::curve::curve_points;
use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

/// The points a profile entity traces, and whether it closes.
fn outline(entity: &EntityType) -> Option<(Vec<[f64; 3]>, bool)> {
    let planar = entity_curve(entity)?;
    let closed = planar.curve.is_closed();
    let mut points = curve_points(&planar);
    // A closed curve tessellates back to its own start. Carrying the repeat
    // would put a zero-width quad in every strip below.
    if closed && points.len() > 1 {
        let first = points[0];
        let last = points[points.len() - 1];
        if Vec3::from(first).distance(Vec3::from(last)) < 1e-9 {
            points.pop();
        }
    }
    (points.len() >= 2).then_some((points, closed))
}

/// SWEEP: drag a profile along a path.
///
/// The path contributes its direction and length, not its shape — which is
/// what SWEEP has always done here, and what makes it an extrusion along an
/// arbitrary vector rather than along a curve.
pub fn swept(profile: &EntityType, path: &EntityType, color: [f32; 4]) -> Option<MeshLodSet> {
    let (points, closed) = outline(profile)?;
    let along = {
        let track = curve_points(&entity_curve(path)?);
        let (from, to) = (Vec3::from(*track.first()?), Vec3::from(*track.last()?));
        to - from
    };
    if along.length() < 1e-12 {
        return None;
    }
    let moved: Vec<[f64; 3]> = points
        .iter()
        .map(|point| (Vec3::from(*point) + along).to_array())
        .collect();
    let mut mesh = Ribbon::default();
    mesh.band(&points, &moved, closed);
    if closed {
        // An open profile sweeps into a sheet with nothing to cap.
        mesh.cap(&points, true);
        mesh.cap(&moved, false);
    }
    mesh.finish(color)
}

/// LOFT: rule a surface through a run of profiles.
///
/// Consecutive profiles are joined by a band each, and the two ends are
/// capped when they close. Profiles with different point counts are resampled
/// onto the finer of the two, so a circle lofted to a square does not twist.
pub fn lofted(profiles: &[EntityType], color: [f32; 4]) -> Option<MeshLodSet> {
    let sections: Vec<(Vec<[f64; 3]>, bool)> = profiles.iter().filter_map(outline).collect();
    if sections.len() < 2 {
        return None;
    }
    let mut mesh = Ribbon::default();
    for pair in sections.windows(2) {
        let count = pair[0].0.len().max(pair[1].0.len());
        let closed = pair[0].1 && pair[1].1;
        let lower = resampled(&pair[0].0, count, pair[0].1);
        let upper = resampled(&pair[1].0, count, pair[1].1);
        mesh.band(&lower, &upper, closed);
    }
    if sections.first()?.1 {
        mesh.cap(&sections.first()?.0, true);
    }
    if sections.last()?.1 {
        mesh.cap(&sections.last()?.0, false);
    }
    mesh.finish(color)
}

/// A ring walked in `count` even steps along its own length.
///
/// Even by distance rather than by index: two profiles given at different
/// densities line up where they are, so a band between them does not twist
/// wherever one of them happened to be sampled more finely.
fn resampled(points: &[[f64; 3]], count: usize, closed: bool) -> Vec<[f64; 3]> {
    let mut ring: Vec<Vec3> = points.iter().map(|point| Vec3::from(*point)).collect();
    if closed {
        ring.push(ring[0]);
    }
    let mut walked = vec![0.0];
    for pair in ring.windows(2) {
        walked.push(walked[walked.len() - 1] + pair[0].distance(pair[1]));
    }
    let total = *walked.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return points.to_vec();
    }
    let steps = if closed { count } else { count.max(2) - 1 };
    (0..if closed { count } else { count.max(2) })
        .map(|step| {
            let want = total * step as f64 / steps as f64;
            let at = walked
                .iter()
                .rposition(|reached| *reached <= want)
                .unwrap_or(0)
                .min(ring.len() - 2);
            let span = walked[at + 1] - walked[at];
            let along = if span > 0.0 { (want - walked[at]) / span } else { 0.0 };
            ring[at].lerp(ring[at + 1], along).to_array()
        })
        .collect()
}

/// Triangles being gathered from bands and caps.
#[derive(Default)]
struct Ribbon {
    positions: Vec<[f64; 3]>,
    normals: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl Ribbon {
    /// A strip of quads between two rings of the same length.
    fn band(&mut self, lower: &[[f64; 3]], upper: &[[f64; 3]], closed: bool) {
        let count = lower.len().min(upper.len());
        if count < 2 {
            return;
        }
        let spans = if closed { count } else { count - 1 };
        for step in 0..spans {
            let next = (step + 1) % count;
            self.quad(lower[step], lower[next], upper[next], upper[step]);
        }
    }

    /// A flat lid over a closed ring, fanned from its middle.
    ///
    /// A fan rather than a proper triangulation: a lofted section can be
    /// concave and a fan would then cover ground outside it, but every
    /// profile these commands accept is a single closed curve, and the middle
    /// of one is inside it.
    fn cap(&mut self, ring: &[[f64; 3]], downward: bool) {
        if ring.len() < 3 {
            return;
        }
        let mut middle = Vec3::new(0.0, 0.0, 0.0);
        for point in ring {
            middle = middle + Vec3::from(*point);
        }
        let middle = (middle / ring.len() as f64).to_array();
        for step in 0..ring.len() {
            let next = (step + 1) % ring.len();
            if downward {
                self.triangle(middle, ring[next], ring[step]);
            } else {
                self.triangle(middle, ring[step], ring[next]);
            }
        }
    }

    fn quad(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) {
        self.triangle(a, b, c);
        self.triangle(a, c, d);
    }

    fn triangle(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
        let Some(normal) = (Vec3::from(b) - Vec3::from(a))
            .cross(Vec3::from(c) - Vec3::from(a))
            .normalize()
        else {
            // Collapsed: no normal, and nothing to draw.
            return;
        };
        let base = self.positions.len() as u32;
        for corner in [a, b, c] {
            self.positions.push(corner);
            self.normals.push(normal.to_array());
        }
        self.triangles.push([base, base + 1, base + 2]);
    }

    /// The gathered triangles as the renderer's mesh, or `None` for none.
    fn finish(self, color: [f32; 4]) -> Option<MeshLodSet> {
        if self.triangles.is_empty() {
            return None;
        }
        // The renderer holds each position as a coarse float plus a fine
        // correction, so a profile at survey coordinates keeps its last
        // millimetres instead of losing them to f32.
        let mut verts = Vec::with_capacity(self.positions.len());
        let mut verts_low = Vec::with_capacity(self.positions.len());
        for point in &self.positions {
            let high = [point[0] as f32, point[1] as f32, point[2] as f32];
            verts.push(high);
            verts_low.push([
                (point[0] - high[0] as f64) as f32,
                (point[1] - high[1] as f64) as f32,
                (point[2] - high[2] as f64) as f32,
            ]);
        }
        Some(MeshLodSet::from_single(MeshModel {
            name: String::new(),
            verts,
            verts_low,
            normals: self
                .normals
                .iter()
                .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32])
                .collect(),
            indices: self.triangles.iter().flatten().copied().collect(),
            triangle_material_handles: Vec::new(),
            triangle_colors: Vec::new(),
            color,
            selected: false,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Circle, LwPolyline, LwVertex};
    use acadrust::types::{Vector2, Vector3};

    fn square(size: f64) -> EntityType {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = true;
        EntityType::LwPolyline(polyline)
    }

    fn volume(body: &Body) -> f64 {
        crate::scene::model::solid_model::volume(body)
    }

    #[test]
    fn a_closed_polyline_extrudes_into_a_prism() {
        let solid = extruded(&square(10.0), 4.0).expect("a prism");
        assert!((volume(&solid) - 400.0).abs() < 1e-6, "{}", volume(&solid));
    }

    #[test]
    fn a_circle_extrudes_into_a_cylinder() {
        // Cut into quarter arcs on the way, so the wall is four cylinder
        // patches rather than a run of flat chords — the volume says which.
        let mut circle = Circle::new();
        circle.center = Vector3::new(0.0, 0.0, 0.0);
        circle.radius = 5.0;
        let solid = extruded(&EntityType::Circle(circle), 3.0).expect("a cylinder");
        let expected = std::f64::consts::PI * 25.0 * 3.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
        assert!(got <= expected * 1.000_001, "{got} vs {expected}");
    }

    #[test]
    fn a_square_revolved_about_its_own_edge_is_a_tube() {
        // The axis runs up the square's left side, so what it sweeps is a
        // solid cylinder of radius ten and height ten.
        let solid = revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            std::f64::consts::TAU,
        );
        // The profile is in the XY plane and the axis lies in it, so this is
        // a revolution about the y axis: radius ten, height ten.
        let solid = solid.expect("a cylinder");
        let expected = std::f64::consts::PI * 100.0 * 10.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
    }

    #[test]
    fn an_open_profile_is_refused() {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = false;
        assert!(extruded(&EntityType::LwPolyline(polyline), 4.0).is_none());
    }

    #[test]
    fn an_axis_off_the_profiles_plane_is_refused() {
        assert!(revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            std::f64::consts::TAU
        )
        .is_none());
    }
}
