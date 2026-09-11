//! Centralized `apply_transform` implementations for all entity types.
//!
//! Each entity's transformation logic is defined here as a standalone
//! function, and [`EntityType`] exposes a convenience dispatch method.

use super::*;
use crate::entities::solid3d::AcisData;
use crate::types::{Matrix3, Transform, Vector2, Vector3};

/// Compose `transform` into an ACIS body's placement (the SAT `transform`
/// record) so the solid's actual geometry moves/rotates/scales, instead of
/// only nudging a reference point. ACIS keeps geometry in body-local space
/// and a `transform` record places it into the world as `world = scale·(p·M)
/// + T`; we fold the incoming world transform into that placement and bake
/// the result into a single 3×3 (scale = 1), which the tessellator applies
/// verbatim. Binary SAB is decoded, composed, and rewritten as text SAT
/// (the SAB writer is not lossless for this edit). A parse failure leaves
/// the data unchanged.
pub(crate) fn compose_acis_placement(acis: &mut AcisData, transform: &Transform) {
    let Some(mut doc) = acis.parse() else {
        return;
    };
    let (m, t, s) = doc.placement();
    // Existing placement as a column-vector linear map L (world = L·p + T):
    //   world = s·(p·M) + T  ⇒  L[i][j] = s·M[j][i].
    let mut l = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            l[i][j] = s * m[j][i];
        }
    }
    // Incoming transform: rotation/scale R and translation tr (column form).
    let mm = &transform.matrix.m;
    let r = [
        [mm[0][0], mm[0][1], mm[0][2]],
        [mm[1][0], mm[1][1], mm[1][2]],
        [mm[2][0], mm[2][1], mm[2][2]],
    ];
    let tr_in = [mm[0][3], mm[1][3], mm[2][3]];
    // L' = R·L
    let mut lp = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            lp[i][j] = r[i][0] * l[0][j] + r[i][1] * l[1][j] + r[i][2] * l[2][j];
        }
    }
    // T' = R·T + tr_in
    let tp = [
        r[0][0] * t[0] + r[0][1] * t[1] + r[0][2] * t[2] + tr_in[0],
        r[1][0] * t[0] + r[1][1] * t[1] + r[1][2] * t[2] + tr_in[1],
        r[2][0] * t[0] + r[2][1] * t[1] + r[2][2] * t[2] + tr_in[2],
    ];
    // Re-encode with scale = 1 and M_out[r][c] = L'[c][r], so that
    // `p·M_out` reproduces `L'·p`.
    let m_out = [
        [lp[0][0], lp[1][0], lp[2][0]],
        [lp[0][1], lp[1][1], lp[2][1]],
        [lp[0][2], lp[1][2], lp[2][2]],
    ];
    doc.set_placement(m_out, tp, 1.0);
    *acis = AcisData::from_sat(&doc.to_sat_string());
}

/// True when `transform` reverses orientation (negative upper-3×3
/// determinant — an odd number of mirrors). Plane-curve direction data
/// (polyline bulges, hatch boundary arc flags) must flip with it.
pub(crate) fn is_reflecting(transform: &Transform) -> bool {
    let m = transform.matrix.m;
    Matrix3::from_rows(
        [m[0][0], m[0][1], m[0][2]],
        [m[1][0], m[1][1], m[1][2]],
        [m[2][0], m[2][1], m[2][2]],
    )
    .determinant()
        < 0.0
}

// ── Point ────────────────────────────────────────────────────────────────────

/// OCS→WCS and WCS→(new)OCS mappers for a planar entity whose stored
/// coordinates live in the OCS of `old_normal`. Mirrors `transform_insert`:
/// map to world, transform there, map back into the transformed normal's
/// frame. Both are the identity for the default +Z normal.
fn ocs_pair(old_normal: Vector3, new_normal: Vector3) -> (Matrix3, Matrix3) {
    (
        Matrix3::arbitrary_axis(old_normal),
        Matrix3::arbitrary_axis(new_normal).transpose(),
    )
}

pub(crate) fn transform_point(e: &mut Point, transform: &Transform) {
    e.location = transform.apply(e.location);
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── Line ─────────────────────────────────────────────────────────────────────

pub(crate) fn transform_line(e: &mut Line, transform: &Transform) {
    e.start = transform.apply(e.start);
    e.end = transform.apply(e.end);
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── Circle ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_circle(e: &mut Circle, transform: &Transform) {
    // `center` is stored in OCS — transform in world, store back in the new
    // normal's OCS (see `ocs_pair`).
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.center = new_t * transform.apply(ocs * e.center);

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.radius *= scale_factor;

    e.normal = new_normal;
}

// ── Arc ──────────────────────────────────────────────────────────────────────

pub(crate) fn transform_arc(e: &mut Arc, transform: &Transform) {
    // The start/end angles are measured in the arc's plane, so a rotation or
    // reflection must move them too — transform the actual endpoints and
    // recompute the angles in the new plane. (Translating alone leaves the
    // angles unchanged, as before.)
    let ocs = Matrix3::arbitrary_axis(e.normal);
    // `center` is stored in OCS — lift it (and the endpoints derived from it)
    // to world before transforming.
    let center_w = ocs * e.center;
    let endpoint = |ang: f64| {
        let local = Vector3::new(e.radius * ang.cos(), e.radius * ang.sin(), 0.0);
        center_w + ocs * local
    };
    let ws = transform.apply(endpoint(e.start_angle));
    let we = transform.apply(endpoint(e.end_angle));

    let new_center = transform.apply(center_w);
    let scale_factor = transform
        .apply_rotation(Vector3::new(1.0, 0.0, 0.0))
        .length();
    let new_normal = transform.apply_rotation(e.normal).normalize();

    let new_ocs_t = Matrix3::arbitrary_axis(new_normal).transpose();
    let angle_of = |w: Vector3| {
        let l = new_ocs_t * (w - new_center);
        l.y.atan2(l.x)
    };
    let (a_start, a_end) = (angle_of(ws), angle_of(we));

    // An arc is stored CCW from start→end. A reflection reverses orientation,
    // so the reflected CCW arc traces CW — swap the endpoints to keep it CCW.
    if is_reflecting(transform) {
        e.start_angle = a_end;
        e.end_angle = a_start;
    } else {
        e.start_angle = a_start;
        e.end_angle = a_end;
    }
    // Store the world-space centre back in the NEW normal's OCS.
    e.center = new_ocs_t * new_center;
    e.radius *= scale_factor;
    e.normal = new_normal;
}

// ── Ellipse ──────────────────────────────────────────────────────────────────

pub(crate) fn transform_ellipse(e: &mut Ellipse, transform: &Transform) {
    e.center = transform.apply(e.center);
    e.major_axis = transform.apply_rotation(e.major_axis);
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── Polyline (3D heavy) ──────────────────────────────────────────────────────

pub(crate) fn transform_polyline(e: &mut Polyline, transform: &Transform) {
    for vertex in &mut e.vertices {
        vertex.location = transform.apply(vertex.location);
    }
}

// ── Polyline2D ───────────────────────────────────────────────────────────────

pub(crate) fn transform_polyline2d(e: &mut Polyline2D, transform: &Transform) {
    let flip = is_reflecting(transform);
    // Vertices are stored in OCS — transform in world, store back in the new
    // normal's OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    for vertex in &mut e.vertices {
        vertex.location = new_t * transform.apply(ocs * vertex.location);
        if flip {
            // Bulge encodes the arc's side/direction in the plane; a
            // reflection reverses it.
            vertex.bulge = -vertex.bulge;
        }
    }
    e.normal = new_normal;
}

// ── Polyline3D ───────────────────────────────────────────────────────────────

pub(crate) fn transform_polyline3d(e: &mut Polyline3D, transform: &Transform) {
    for v in &mut e.vertices {
        v.position = transform.apply(v.position);
    }
}

// ── LwPolyline ───────────────────────────────────────────────────────────────

pub(crate) fn transform_lwpolyline(e: &mut LwPolyline, transform: &Transform) {
    let flip = is_reflecting(transform);
    // Vertices + elevation are stored in OCS — transform in world, store back
    // in the new normal's OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    let mut new_elevation = e.elevation;
    for vertex in &mut e.vertices {
        let pt3d = Vector3::new(vertex.location.x, vertex.location.y, e.elevation);
        let l = new_t * transform.apply(ocs * pt3d);
        vertex.location.x = l.x;
        vertex.location.y = l.y;
        new_elevation = l.z;
        if flip {
            // Bulge encodes the arc's side/direction in the plane; a
            // reflection reverses it. Without this, exploding a mirrored
            // INSERT bows every bulged segment to the wrong side.
            vertex.bulge = -vertex.bulge;
        }
    }
    if e.vertices.is_empty() {
        let l = new_t * transform.apply(ocs * Vector3::new(0.0, 0.0, e.elevation));
        new_elevation = l.z;
    }
    e.elevation = new_elevation;
    e.normal = new_normal;
}

// ── Text ─────────────────────────────────────────────────────────────────────

/// Carry a stored orientation angle through a transform.
///
/// TEXT, MTEXT, SHAPE and the ATTRIB pair keep which way they face as a single
/// angle rather than as geometry, so transforming their points alone leaves the
/// glyphs pointing the old way — ROTATE put the text in the right place and
/// still drew it horizontal. Turn the angle into the direction it stands for,
/// send that through the transform, and read the angle back. `atan2` ignores
/// any scale the transform also carries, and a reflection lands on the mirrored
/// angle, which is what the mirror paths used to work out separately.
///
/// A transform that flattens the direction away leaves the angle alone rather
/// than snapping it to zero through `atan2(0, 0)`.
pub(crate) fn rotate_stored_angle(rotation: &mut f64, transform: &Transform) {
    let direction = Vector3::new(rotation.cos(), rotation.sin(), 0.0);
    let rotated = transform.apply_rotation(direction);
    if rotated.x != 0.0 || rotated.y != 0.0 {
        *rotation = rotated.y.atan2(rotated.x);
    }
}

pub(crate) fn transform_text(e: &mut Text, transform: &Transform) {
    // Insertion / alignment points are stored in OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.insertion_point = new_t * transform.apply(ocs * e.insertion_point);
    if let Some(ref mut align) = e.alignment_point {
        *align = new_t * transform.apply(ocs * *align);
    }
    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.height *= scale_factor;
    e.normal = new_normal;
    rotate_stored_angle(&mut e.rotation, transform);
}

// ── MText ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_mtext(e: &mut MText, transform: &Transform) {
    e.insertion_point = transform.apply(e.insertion_point);
    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.height *= scale_factor;
    e.rectangle_width *= scale_factor;
    if let Some(ref mut h) = e.rectangle_height {
        *h *= scale_factor;
    }
    e.normal = transform.apply_rotation(e.normal).normalize();
    rotate_stored_angle(&mut e.rotation, transform);
}

// ── Spline ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_spline(e: &mut Spline, transform: &Transform) {
    for point in &mut e.control_points {
        *point = transform.apply(*point);
    }
    for point in &mut e.fit_points {
        *point = transform.apply(*point);
    }
    e.begin_tangent = transform.apply_rotation(e.begin_tangent);
    e.end_tangent = transform.apply_rotation(e.end_tangent);

    let axes = [Vector3::UNIT_X, Vector3::UNIT_Y, Vector3::UNIT_Z]
        .map(|axis| transform.apply_rotation(axis));
    let max_scale = axes.iter().map(|axis| axis.length()).fold(0.0, f64::max);
    let orthogonal = [(0, 1), (0, 2), (1, 2)]
        .into_iter()
        .all(|(left, right)| axes[left].dot(&axes[right]).abs() <= 1e-12 * max_scale.powi(2));
    // Exact for orthogonal axes; conservative for sheared transforms.
    let distance_scale = if orthogonal {
        max_scale
    } else {
        axes.iter()
            .map(|axis| axis.length_squared())
            .sum::<f64>()
            .sqrt()
    };
    e.control_tolerance *= distance_scale;
    e.fit_tolerance *= distance_scale;
    e.normal = transform_normal(transform, e.normal);
}

// ── Helix ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_helix(e: &mut Helix, transform: &Transform) {
    let old_axis = e.axis_vector.normalize();
    let old_base = e.axis_base_point;
    let old_start = e.start_point;
    let transformed_axis = transform.apply_rotation(old_axis);
    let transformed_radial = transform.apply_rotation(old_start - old_base);
    let transformed_base = transform.apply(old_base);
    let transformed_start = transform.apply(old_start);

    transform_spline(&mut e.spline, transform);
    e.axis_base_point = transformed_base;
    e.start_point = transformed_start;
    e.axis_vector = transformed_axis.normalize();
    e.radius = transformed_radial.length();
    e.turn_height *= transformed_axis.length();
    if is_reflecting(transform) {
        e.handedness = !e.handedness;
    }
}

// ── Dimension ────────────────────────────────────────────────────────────────

// Dimension uses the default Entity trait implementation (extract translation).

// ── Hatch ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_hatch(e: &mut Hatch, transform: &Transform) {
    let old_normal = e.normal;
    let old_elevation = e.elevation;

    let new_normal = transform.apply_rotation(old_normal).normalize();

    let old_ocs_to_wcs = Matrix3::arbitrary_axis(old_normal);
    let new_wcs_to_ocs = Matrix3::arbitrary_axis(new_normal).transpose();

    let origin_wcs = old_ocs_to_wcs * Vector3::new(0.0, 0.0, old_elevation);
    let new_origin_wcs = transform.apply(origin_wcs);
    let new_origin_ocs = new_wcs_to_ocs * new_origin_wcs;
    let new_elevation = new_origin_ocs.z;

    let ocs_x_wcs = old_ocs_to_wcs * Vector3::UNIT_X;
    let ocs_y_wcs = old_ocs_to_wcs * Vector3::UNIT_Y;

    let trans_ocs_x_wcs = transform.apply_rotation(ocs_x_wcs);
    let trans_ocs_y_wcs = transform.apply_rotation(ocs_y_wcs);

    let scale_x = trans_ocs_x_wcs.length();
    let scale_y = trans_ocs_y_wcs.length();

    let is_uniform =
        (scale_x - scale_y).abs() < 1e-10 && trans_ocs_x_wcs.dot(&trans_ocs_y_wcs).abs() < 1e-10;

    let x_in_new_ocs = new_wcs_to_ocs * trans_ocs_x_wcs;
    let y_in_new_ocs = new_wcs_to_ocs * trans_ocs_y_wcs;
    let is_flipped = (x_in_new_ocs.x * y_in_new_ocs.y - x_in_new_ocs.y * y_in_new_ocs.x) < 0.0;

    let transform_ocs_point = |p: Vector2| -> Vector2 {
        let p_wcs = old_ocs_to_wcs * Vector3::new(p.x, p.y, old_elevation);
        let p_new_wcs = transform.apply(p_wcs);
        let p_new_ocs = new_wcs_to_ocs * p_new_wcs;
        Vector2::new(p_new_ocs.x, p_new_ocs.y)
    };

    let transform_ocs_angle = |angle_rad: f64| -> f64 {
        let p_ocs = Vector2::new(angle_rad.cos(), angle_rad.sin());
        let p_wcs_dir = old_ocs_to_wcs * Vector3::new(p_ocs.x, p_ocs.y, 0.0);
        let transformed_wcs_dir = transform.apply_rotation(p_wcs_dir);
        let transformed_ocs_dir = new_wcs_to_ocs * transformed_wcs_dir;
        let mut new_angle_rad = transformed_ocs_dir.y.atan2(transformed_ocs_dir.x);
        if new_angle_rad < 0.0 {
            new_angle_rad += 2.0 * std::f64::consts::PI;
        }
        new_angle_rad
    };

    for path in &mut e.paths {
        for edge in &mut path.edges {
            match edge {
                BoundaryEdge::Line(line) => {
                    line.start = transform_ocs_point(line.start);
                    line.end = transform_ocs_point(line.end);
                }
                BoundaryEdge::CircularArc(arc) => {
                    // Clockwise angles are mirrored and their forward sweep is
                    // stored without normalization. Transform the geometric start
                    // angle and preserve that sweep.
                    let to_true = |a: f64, ccw: bool| if ccw { a } else { -a };
                    let norm = |a: f64| a.rem_euclid(2.0 * std::f64::consts::PI);
                    let stored_sweep = arc.end_angle - arc.start_angle;
                    let true_start = to_true(arc.start_angle, arc.counter_clockwise);
                    let new_ccw = arc.counter_clockwise ^ is_flipped;
                    let center = transform_ocs_point(arc.center);

                    if is_uniform {
                        let new_true_start = transform_ocs_angle(true_start);
                        let new_start = norm(to_true(new_true_start, new_ccw));
                        arc.center = center;
                        arc.radius *= scale_x;
                        arc.counter_clockwise = new_ccw;
                        arc.start_angle = new_start;
                        arc.end_angle = new_start + stored_sweep;
                    } else {
                        let major_axis_wcs = trans_ocs_x_wcs * arc.radius;
                        let major_axis_ocs_3d = new_wcs_to_ocs * major_axis_wcs;
                        let major_axis_endpoint =
                            Vector2::new(major_axis_ocs_3d.x, major_axis_ocs_3d.y);

                        // The ellipse parameter equals the circle's TRUE angle:
                        // the transformed axes carry the deformation, so the
                        // parameter is unchanged — only re-mirrored for the new
                        // direction flag, with the stored sweep carried over.
                        let mut start = norm(to_true(true_start, new_ccw));
                        let mut ratio = scale_y / scale_x;
                        let mut major = major_axis_endpoint;
                        if ratio > 1.0 {
                            let len = major.length() * ratio;
                            major = Vector2::new(-major.y, major.x).normalize() * len;
                            ratio = 1.0 / ratio;
                            // Major axis rotated +90°: true parameter shifts by
                            // -π/2; stored space mirrors that for CW edges.
                            let shift = if new_ccw {
                                -std::f64::consts::FRAC_PI_2
                            } else {
                                std::f64::consts::FRAC_PI_2
                            };
                            start = norm(start + shift);
                        }
                        *edge = BoundaryEdge::EllipticArc(EllipticArcEdge {
                            center,
                            major_axis_endpoint: major,
                            minor_axis_ratio: ratio,
                            start_angle: start,
                            end_angle: start + stored_sweep,
                            counter_clockwise: new_ccw,
                        });
                    }
                }
                BoundaryEdge::EllipticArc(ellipse) => {
                    let center = transform_ocs_point(ellipse.center);

                    let old_major_wcs = old_ocs_to_wcs
                        * Vector3::new(
                            ellipse.major_axis_endpoint.x,
                            ellipse.major_axis_endpoint.y,
                            0.0,
                        );
                    let old_major_len = ellipse.major_axis_endpoint.length();
                    let old_minor_len = old_major_len * ellipse.minor_axis_ratio;
                    let old_minor_ocs_dir = Vector2::new(
                        -ellipse.major_axis_endpoint.y,
                        ellipse.major_axis_endpoint.x,
                    )
                    .normalize();
                    let old_minor_wcs = old_ocs_to_wcs
                        * Vector3::new(
                            old_minor_ocs_dir.x * old_minor_len,
                            old_minor_ocs_dir.y * old_minor_len,
                            0.0,
                        );

                    let new_major_wcs = transform.apply_rotation(old_major_wcs);
                    let new_minor_wcs = transform.apply_rotation(old_minor_wcs);

                    let new_major_ocs_3d = new_wcs_to_ocs * new_major_wcs;
                    let new_minor_ocs_3d = new_wcs_to_ocs * new_minor_wcs;

                    let new_major_ocs = Vector2::new(new_major_ocs_3d.x, new_major_ocs_3d.y);
                    let new_minor_ocs = Vector2::new(new_minor_ocs_3d.x, new_minor_ocs_3d.y);

                    let new_major_len = new_major_ocs.length();
                    let new_minor_len = new_minor_ocs.length();

                    ellipse.center = center;
                    // Same stored-angle convention as CircularArc: CW edges
                    // keep mirrored parameters, and the stored sweep
                    // (end - start ≥ 0, end may exceed 2π to encode a wrap)
                    // must survive untouched. Transform only the start
                    // parameter and carry the sweep over.
                    let norm = |a: f64| a.rem_euclid(2.0 * std::f64::consts::PI);
                    let stored_sweep = ellipse.end_angle - ellipse.start_angle;
                    if is_flipped {
                        ellipse.counter_clockwise = !ellipse.counter_clockwise;
                        ellipse.start_angle = norm(-ellipse.start_angle);
                    }

                    if new_minor_len > new_major_len + 1e-12 {
                        ellipse.major_axis_endpoint = new_minor_ocs;
                        ellipse.minor_axis_ratio = new_major_len / new_minor_len;
                        // True parameter shifts by -π/2 (major rotated +90°);
                        // stored space mirrors that for CW edges.
                        let shift = if ellipse.counter_clockwise {
                            -std::f64::consts::FRAC_PI_2
                        } else {
                            std::f64::consts::FRAC_PI_2
                        };
                        ellipse.start_angle = norm(ellipse.start_angle + shift);
                        ellipse.end_angle = ellipse.start_angle + stored_sweep;
                    } else {
                        ellipse.major_axis_endpoint = new_major_ocs;
                        ellipse.minor_axis_ratio = if new_major_len > 1e-12 {
                            new_minor_len / new_major_len
                        } else {
                            1.0
                        };
                        ellipse.end_angle = ellipse.start_angle + stored_sweep;
                    }
                }
                BoundaryEdge::Spline(spline) => {
                    for cp in &mut spline.control_points {
                        let p_wcs = old_ocs_to_wcs * Vector3::new(cp.x, cp.y, old_elevation);
                        let p_new_wcs = transform.apply(p_wcs);
                        let p_new_ocs = new_wcs_to_ocs * p_new_wcs;
                        cp.x = p_new_ocs.x;
                        cp.y = p_new_ocs.y;
                    }
                    for fp in &mut spline.fit_points {
                        *fp = transform_ocs_point(*fp);
                    }
                    let trans_dir = |d: Vector2| -> Vector2 {
                        let d_wcs = old_ocs_to_wcs * Vector3::new(d.x, d.y, 0.0);
                        let d_new_wcs = transform.apply_rotation(d_wcs);
                        let d_new_ocs = new_wcs_to_ocs * d_new_wcs;
                        Vector2::new(d_new_ocs.x, d_new_ocs.y)
                    };
                    spline.start_tangent = trans_dir(spline.start_tangent);
                    spline.end_tangent = trans_dir(spline.end_tangent);
                }
                BoundaryEdge::Polyline(poly) => {
                    for v in &mut poly.vertices {
                        let t = transform_ocs_point(Vector2::new(v.x, v.y));
                        v.x = t.x;
                        v.y = t.y;
                        if is_flipped {
                            v.z = -v.z;
                        }
                    }
                }
            }
        }
    }

    for seed in &mut e.seed_points {
        *seed = transform_ocs_point(*seed);
    }

    let p_dir = Vector2::new(e.pattern_angle.cos(), e.pattern_angle.sin());
    let p_wcs_dir = old_ocs_to_wcs * Vector3::new(p_dir.x, p_dir.y, 0.0);
    let transformed_p_wcs_dir = transform.apply_rotation(p_wcs_dir);
    let transformed_p_ocs_dir = new_wcs_to_ocs * transformed_p_wcs_dir;
    e.pattern_angle = transformed_p_ocs_dir.y.atan2(transformed_p_ocs_dir.x);
    e.pattern_scale *= scale_x;

    for line in &mut e.pattern.lines {
        let l_dir = Vector2::new(line.angle.cos(), line.angle.sin());
        let l_wcs_dir = old_ocs_to_wcs * Vector3::new(l_dir.x, l_dir.y, 0.0);
        let transformed_l_wcs_dir = transform.apply_rotation(l_wcs_dir);
        let transformed_l_ocs_dir = new_wcs_to_ocs * transformed_l_wcs_dir;
        line.angle = transformed_l_ocs_dir.y.atan2(transformed_l_ocs_dir.x);

        line.base_point = transform_ocs_point(line.base_point);

        let off_wcs = old_ocs_to_wcs * Vector3::new(line.offset.x, line.offset.y, 0.0);
        let transformed_off_wcs = transform.apply_rotation(off_wcs);
        let transformed_off_ocs = new_wcs_to_ocs * transformed_off_wcs;
        line.offset = Vector2::new(transformed_off_ocs.x, transformed_off_ocs.y);

        for dash in &mut line.dash_lengths {
            *dash *= scale_x;
        }
    }

    if e.gradient_color.enabled {
        let g_dir = Vector2::new(e.gradient_color.angle.cos(), e.gradient_color.angle.sin());
        let g_wcs_dir = old_ocs_to_wcs * Vector3::new(g_dir.x, g_dir.y, 0.0);
        let transformed_g_wcs_dir = transform.apply_rotation(g_wcs_dir);
        let transformed_g_ocs_dir = new_wcs_to_ocs * transformed_g_wcs_dir;
        e.gradient_color.angle = transformed_g_ocs_dir.y.atan2(transformed_g_ocs_dir.x);
    }

    e.normal = new_normal;
    e.elevation = new_elevation;
}

// ── Solid ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_solid(e: &mut Solid, transform: &Transform) {
    // Corners are stored in OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.first_corner = new_t * transform.apply(ocs * e.first_corner);
    e.second_corner = new_t * transform.apply(ocs * e.second_corner);
    e.third_corner = new_t * transform.apply(ocs * e.third_corner);
    e.fourth_corner = new_t * transform.apply(ocs * e.fourth_corner);
    e.normal = new_normal;
}

// ── Face3D ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_face3d(e: &mut Face3D, transform: &Transform) {
    e.first_corner = transform.apply(e.first_corner);
    e.second_corner = transform.apply(e.second_corner);
    e.third_corner = transform.apply(e.third_corner);
    e.fourth_corner = transform.apply(e.fourth_corner);
}

// ── Insert ───────────────────────────────────────────────────────────────────

/// Minimum absolute value accepted for insert scale factors.
const SCALE_EPSILON: f64 = 1e-12;

/// Transform a normal vector using inverse-transpose of the upper 3×3
/// matrix, which is the geometrically correct approach for normals under
/// non-uniform scale. Falls back to the original normal if the matrix is
/// singular.
pub(crate) fn transform_normal(transform: &Transform, normal: Vector3) -> Vector3 {
    let m4 = transform.matrix;
    let upper3x3 = Matrix3::from_rows(
        [m4.m[0][0], m4.m[0][1], m4.m[0][2]],
        [m4.m[1][0], m4.m[1][1], m4.m[1][2]],
        [m4.m[2][0], m4.m[2][1], m4.m[2][2]],
    );
    if let Some(inv) = upper3x3.inverse() {
        let inv_t = inv.transpose();
        let transformed = inv_t.transform_point(normal);
        let len = transformed.length();
        if len < 1e-10 {
            normal
        } else {
            transformed * (1.0 / len)
        }
    } else {
        normal
    }
}

pub(crate) fn transform_insert(e: &mut Insert, transform: &Transform) {
    let new_normal = transform_normal(transform, e.normal);

    // `insert_point` is stored in the OCS defined by `normal`, not in world
    // space. Convert it to world, apply the transform there, then map it back
    // into the (possibly new) OCS — otherwise a world-space move/rotate of a
    // block whose extrusion direction isn't +Z lands on the wrong axes.
    // For a +Z normal the OCS is the identity, so this is a no-op.
    let ocs_old = Matrix3::arbitrary_axis(e.normal);
    let world_pos = ocs_old * e.insert_point;
    let new_world_pos = transform.apply(world_pos);
    let ocs_new = Matrix3::arbitrary_axis(new_normal);
    let new_position = ocs_new.transpose() * new_world_pos;

    let trans_ow = Matrix3::arbitrary_axis(e.normal) * Matrix3::rotation_z(e.rotation);

    let trans_wo_base = Matrix3::arbitrary_axis(new_normal);
    let trans_wo = trans_wo_base.transpose();

    let m4 = transform.matrix;
    let transformation = Matrix3::from_rows(
        [m4.m[0][0], m4.m[0][1], m4.m[0][2]],
        [m4.m[1][0], m4.m[1][1], m4.m[1][2]],
        [m4.m[2][0], m4.m[2][1], m4.m[2][2]],
    );

    let v = trans_wo * (transformation * (trans_ow * Vector3::UNIT_X));
    let new_rotation = v.y.atan2(v.x);

    let trans_wo_rot = Matrix3::rotation_z(new_rotation).transpose() * trans_wo;
    let s = trans_wo_rot
        * (transformation * (trans_ow * Vector3::new(e.x_scale(), e.y_scale(), e.z_scale())));

    let clamp = |val: f64| -> f64 {
        if val.abs() < SCALE_EPSILON {
            SCALE_EPSILON
        } else {
            val
        }
    };

    e.normal = new_normal;
    e.insert_point = new_position;
    e.set_x_scale(clamp(s.x));
    e.set_y_scale(clamp(s.y));
    e.set_z_scale(clamp(s.z));
    e.rotation = new_rotation;

    for att in &mut e.attributes {
        att.apply_transform(transform);
    }
}

// ── Block ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_block(e: &mut Block, transform: &Transform) {
    e.base_point = transform.apply(e.base_point);
}

// ── BlockEnd ─────────────────────────────────────────────────────────────────

pub(crate) fn transform_block_end(_e: &mut BlockEnd, _transform: &Transform) {
    // No geometry
}

// ── Ray ──────────────────────────────────────────────────────────────────────

pub(crate) fn transform_ray(e: &mut Ray, transform: &Transform) {
    e.base_point = transform.apply(e.base_point);
    e.direction = transform.apply_rotation(e.direction).normalize();
}

// ── XLine ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_xline(e: &mut XLine, transform: &Transform) {
    e.base_point = transform.apply(e.base_point);
    e.direction = transform.apply_rotation(e.direction).normalize();
}

// ── Viewport ─────────────────────────────────────────────────────────────────

pub(crate) fn transform_viewport(e: &mut Viewport, transform: &Transform) {
    e.center = transform.apply(e.center);

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.width *= scale_factor;
    e.height *= scale_factor;

    e.view_direction = transform.apply_rotation(e.view_direction).normalize();
    e.ucs_x_axis = transform.apply_rotation(e.ucs_x_axis).normalize();
    e.ucs_y_axis = transform.apply_rotation(e.ucs_y_axis).normalize();

    e.ucs_origin = transform.apply(e.ucs_origin);
    e.view_target = transform.apply(e.view_target);
}

// ── AttributeDefinition ──────────────────────────────────────────────────────

pub(crate) fn transform_attribute_definition(e: &mut AttributeDefinition, transform: &Transform) {
    // Insertion / alignment points are stored in OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.insertion_point = new_t * transform.apply(ocs * e.insertion_point);
    e.alignment_point = new_t * transform.apply(ocs * e.alignment_point);
    e.normal = new_normal;

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.height *= scale_factor;
    rotate_stored_angle(&mut e.rotation, transform);
}

// ── AttributeEntity ──────────────────────────────────────────────────────────

pub(crate) fn transform_attribute_entity(e: &mut AttributeEntity, transform: &Transform) {
    // Insertion / alignment points are stored in OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.insertion_point = new_t * transform.apply(ocs * e.insertion_point);
    e.alignment_point = new_t * transform.apply(ocs * e.alignment_point);
    e.normal = new_normal;

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.height *= scale_factor;
    rotate_stored_angle(&mut e.rotation, transform);
}

// ── Leader ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_leader(e: &mut Leader, transform: &Transform) {
    for vertex in &mut e.vertices {
        *vertex = transform.apply(*vertex);
    }
    e.block_offset = transform.apply(e.block_offset);
    e.annotation_offset = transform.apply(e.annotation_offset);
    e.horizontal_direction = transform.apply_rotation(e.horizontal_direction).normalize();
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── MultiLeader ──────────────────────────────────────────────────────────────

pub(crate) fn transform_multileader(e: &mut MultiLeader, transform: &Transform) {
    e.context.content_base_point = transform.apply(e.context.content_base_point);
    e.context.text_location = transform.apply(e.context.text_location);
    e.context.block_content_location = transform.apply(e.context.block_content_location);
    e.context.base_point = transform.apply(e.context.base_point);

    e.context.text_normal = transform.apply_rotation(e.context.text_normal).normalize();
    e.context.text_direction = transform
        .apply_rotation(e.context.text_direction)
        .normalize();
    e.context.base_direction = transform
        .apply_rotation(e.context.base_direction)
        .normalize();
    e.context.base_vertical = transform
        .apply_rotation(e.context.base_vertical)
        .normalize();

    for root in &mut e.context.leader_roots {
        root.connection_point = transform.apply(root.connection_point);
        for bp in &mut root.break_points {
            bp.start_point = transform.apply(bp.start_point);
            bp.end_point = transform.apply(bp.end_point);
        }
        for line in &mut root.lines {
            for point in &mut line.points {
                *point = transform.apply(*point);
            }
            for bp in &mut line.break_points {
                bp.start_point = transform.apply(bp.start_point);
                bp.end_point = transform.apply(bp.end_point);
            }
        }
    }

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.arrowhead_size *= scale_factor;
    e.text_height *= scale_factor;
    e.dogleg_length *= scale_factor;
    e.context.arrowhead_size *= scale_factor;
    e.context.text_height *= scale_factor;
    e.context.landing_gap *= scale_factor;
}

// ── MLine ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_mline(e: &mut MLine, transform: &Transform) {
    e.start_point = transform.apply(e.start_point);
    for vertex in &mut e.vertices {
        vertex.position = transform.apply(vertex.position);
        vertex.direction = transform.apply_rotation(vertex.direction).normalize();
        vertex.miter = transform.apply_rotation(vertex.miter).normalize();
    }
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── Mesh ─────────────────────────────────────────────────────────────────────

pub(crate) fn transform_mesh(e: &mut Mesh, transform: &Transform) {
    for vertex in &mut e.vertices {
        *vertex = transform.apply(*vertex);
    }
}

// ── RasterImage ──────────────────────────────────────────────────────────────

pub(crate) fn transform_raster_image(e: &mut RasterImage, transform: &Transform) {
    e.insertion_point = transform.apply(e.insertion_point);
    e.u_vector = transform.apply_rotation(e.u_vector);
    e.v_vector = transform.apply_rotation(e.v_vector);

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.u_vector = e.u_vector * scale_factor;
    e.v_vector = e.v_vector * scale_factor;
}

// ── Solid3D ──────────────────────────────────────────────────────────────────

pub(crate) fn transform_solid3d(e: &mut Solid3D, transform: &Transform) {
    e.point_of_reference = transform.apply(e.point_of_reference);
    compose_acis_placement(&mut e.acis_data, transform);
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = transform.apply(*pt);
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = transform.apply(silhouette.target);
        silhouette.view_direction = transform
            .apply_rotation(silhouette.view_direction)
            .normalize();
        silhouette.up_vector = transform.apply_rotation(silhouette.up_vector).normalize();
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = transform.apply(*pt);
            }
        }
    }
}

// ── Region ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_region(e: &mut Region, transform: &Transform) {
    e.point_of_reference = transform.apply(e.point_of_reference);
    compose_acis_placement(&mut e.acis_data, transform);
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = transform.apply(*pt);
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = transform.apply(silhouette.target);
        silhouette.view_direction = transform
            .apply_rotation(silhouette.view_direction)
            .normalize();
        silhouette.up_vector = transform.apply_rotation(silhouette.up_vector).normalize();
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = transform.apply(*pt);
            }
        }
    }
}

// ── Body ─────────────────────────────────────────────────────────────────────

pub(crate) fn transform_body(e: &mut Body, transform: &Transform) {
    e.point_of_reference = transform.apply(e.point_of_reference);
    compose_acis_placement(&mut e.acis_data, transform);
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = transform.apply(*pt);
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = transform.apply(silhouette.target);
        silhouette.view_direction = transform
            .apply_rotation(silhouette.view_direction)
            .normalize();
        silhouette.up_vector = transform.apply_rotation(silhouette.up_vector).normalize();
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = transform.apply(*pt);
            }
        }
    }
}

/// Keep embedded loft curves in their local frame while moving their placement.
pub(crate) fn transform_loft_placement(e: &mut crate::entities::Surface, transform: &Transform) {
    if let crate::entities::SurfaceData::Lofted { loft_transform, .. } = &mut e.surface_data {
        let previous = *loft_transform;
        *loft_transform = std::array::from_fn(|index| {
            let row = index % 4;
            let column = index / 4;
            (0..4)
                .map(|inner| transform.matrix.m[row][inner] * previous[column * 4 + inner])
                .sum()
        });
    }
}

pub(crate) fn transform_surface(e: &mut crate::entities::Surface, transform: &Transform) {
    transform_loft_placement(e, transform);
    e.point_of_reference = transform.apply(e.point_of_reference);
    compose_acis_placement(&mut e.acis_data, transform);
    for wire in &mut e.wires {
        for pt in &mut wire.points {
            *pt = transform.apply(*pt);
        }
    }
    for silhouette in &mut e.silhouettes {
        silhouette.target = transform.apply(silhouette.target);
        silhouette.view_direction = transform
            .apply_rotation(silhouette.view_direction)
            .normalize();
        silhouette.up_vector = transform.apply_rotation(silhouette.up_vector).normalize();
        for wire in &mut silhouette.wires {
            for pt in &mut wire.points {
                *pt = transform.apply(*pt);
            }
        }
    }
}

// ── Table ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_table(e: &mut Table, transform: &Transform) {
    e.insertion_point = transform.apply(e.insertion_point);
    e.horizontal_direction = transform.apply_rotation(e.horizontal_direction).normalize();
    e.normal = transform.apply_rotation(e.normal).normalize();

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    for col in &mut e.columns {
        col.width *= scale_factor;
    }
    for row in &mut e.rows {
        row.height *= scale_factor;
    }
}

// ── Tolerance ────────────────────────────────────────────────────────────────

pub(crate) fn transform_tolerance(e: &mut Tolerance, transform: &Transform) {
    e.insertion_point = transform.apply(e.insertion_point);
    e.direction = transform.apply_rotation(e.direction).normalize();
    e.normal = transform.apply_rotation(e.normal).normalize();

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.text_height *= scale_factor;
    e.dimension_gap *= scale_factor;
}

// ── PolyfaceMesh ─────────────────────────────────────────────────────────────

pub(crate) fn transform_polyface_mesh(e: &mut PolyfaceMesh, transform: &Transform) {
    for v in &mut e.vertices {
        v.location = transform.apply(v.location);
    }
    e.normal = transform.apply_rotation(e.normal).normalize();
}

// ── Wipeout ──────────────────────────────────────────────────────────────────

pub(crate) fn transform_wipeout(e: &mut Wipeout, transform: &Transform) {
    // `u_vector` / `v_vector` carry both the image direction AND its size, so
    // the linear part of the transform (rotation + scale, via `apply_rotation`)
    // already scales them. Multiplying again by the scale factor double-scaled
    // the mask (a 2× insert grew the wipeout 4×).
    e.insertion_point = transform.apply(e.insertion_point);
    e.u_vector = transform.apply_rotation(e.u_vector);
    e.v_vector = transform.apply_rotation(e.v_vector);
}

// ── Shape ────────────────────────────────────────────────────────────────────

pub(crate) fn transform_shape(e: &mut Shape, transform: &Transform) {
    // Insertion point is stored in OCS.
    let new_normal = transform.apply_rotation(e.normal).normalize();
    let (ocs, new_t) = ocs_pair(e.normal, new_normal);
    e.insertion_point = new_t * transform.apply(ocs * e.insertion_point);
    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.size *= scale_factor;
    e.normal = new_normal;
    rotate_stored_angle(&mut e.rotation, transform);
}

// ── Underlay ─────────────────────────────────────────────────────────────────

pub(crate) fn transform_underlay(e: &mut Underlay, transform: &Transform) {
    e.insertion_point = transform.apply(e.insertion_point);
    e.normal = transform.apply_rotation(e.normal).normalize();

    let unit_x = Vector3::new(1.0, 0.0, 0.0);
    let transformed_unit = transform.apply_rotation(unit_x);
    let scale_factor = transformed_unit.length();
    e.x_scale *= scale_factor;
    e.y_scale *= scale_factor;
    e.z_scale *= scale_factor;
}

// ── Seqend ───────────────────────────────────────────────────────────────────

pub(crate) fn transform_seqend(_e: &mut Seqend, _transform: &Transform) {
    // No geometry
}

// ── Ole2Frame ────────────────────────────────────────────────────────────────

pub(crate) fn transform_ole2frame(e: &mut Ole2Frame, transform: &Transform) {
    e.upper_left_corner = transform.apply(e.upper_left_corner);
    e.lower_right_corner = transform.apply(e.lower_right_corner);
}

// ── PolygonMesh ──────────────────────────────────────────────────────────────

pub(crate) fn transform_polygon_mesh(e: &mut PolygonMeshEntity, transform: &Transform) {
    for v in &mut e.vertices {
        v.location = transform.apply(v.location);
    }
}

// ── UnknownEntity ────────────────────────────────────────────────────────────

pub(crate) fn transform_unknown(_e: &mut UnknownEntity, _transform: &Transform) {
    // No geometry
}

// ── EntityType dispatch ──────────────────────────────────────────────────────

impl EntityType {
    /// Apply a general transform to this entity.
    ///
    /// Dispatches to the appropriate per-entity implementation.
    /// This is equivalent to calling `entity.as_entity_mut().apply_transform(t)`.
    pub fn apply_transform(&mut self, transform: &Transform) {
        match self {
            EntityType::Point(e) => transform_point(e, transform),
            EntityType::Line(e) => transform_line(e, transform),
            EntityType::Circle(e) => transform_circle(e, transform),
            EntityType::Arc(e) => transform_arc(e, transform),
            EntityType::Ellipse(e) => transform_ellipse(e, transform),
            EntityType::Polyline(e) => transform_polyline(e, transform),
            EntityType::Polyline2D(e) => transform_polyline2d(e, transform),
            EntityType::Polyline3D(e) => transform_polyline3d(e, transform),
            EntityType::LwPolyline(e) => transform_lwpolyline(e, transform),
            EntityType::Text(e) => transform_text(e, transform),
            EntityType::MText(e) => transform_mtext(e, transform),
            EntityType::Spline(e) => transform_spline(e, transform),
            EntityType::Helix(e) => transform_helix(e, transform),
            EntityType::Dimension(e) => e.apply_transform(transform),
            EntityType::Hatch(e) => transform_hatch(e, transform),
            EntityType::Solid(e) => transform_solid(e, transform),
            EntityType::Face3D(e) => transform_face3d(e, transform),
            EntityType::Insert(e) => transform_insert(e, transform),
            EntityType::Block(e) => transform_block(e, transform),
            EntityType::BlockEnd(e) => transform_block_end(e, transform),
            EntityType::Ray(e) => transform_ray(e, transform),
            EntityType::XLine(e) => transform_xline(e, transform),
            EntityType::Viewport(e) => transform_viewport(e, transform),
            EntityType::AttributeDefinition(e) => transform_attribute_definition(e, transform),
            EntityType::AttributeEntity(e) => transform_attribute_entity(e, transform),
            EntityType::Leader(e) => transform_leader(e, transform),
            EntityType::MultiLeader(e) => transform_multileader(e, transform),
            EntityType::MLine(e) => transform_mline(e, transform),
            EntityType::Mesh(e) => transform_mesh(e, transform),
            EntityType::RasterImage(e) => transform_raster_image(e, transform),
            EntityType::Solid3D(e) => transform_solid3d(e, transform),
            EntityType::Region(e) => transform_region(e, transform),
            EntityType::Body(e) => transform_body(e, transform),
            EntityType::Surface(e) => transform_surface(e, transform),
            EntityType::Table(e) => transform_table(e, transform),
            EntityType::Tolerance(e) => transform_tolerance(e, transform),
            EntityType::PolyfaceMesh(e) => transform_polyface_mesh(e, transform),
            EntityType::Wipeout(e) => transform_wipeout(e, transform),
            EntityType::Shape(e) => transform_shape(e, transform),
            EntityType::Underlay(e) => transform_underlay(e, transform),
            EntityType::Seqend(e) => transform_seqend(e, transform),
            EntityType::Ole2Frame(e) => transform_ole2frame(e, transform),
            EntityType::PolygonMesh(e) => transform_polygon_mesh(e, transform),
            // Lights are glyph-only; the photometric frame is not transformed
            // (keeps the preserved raw record valid for write-back).
            EntityType::Light(_) => {}
            // Anchored to their drawing view (see translate).
            EntityType::SectionSymbol(_) | EntityType::ViewBorder(_) => {}
            EntityType::Extended(_) => {}
            EntityType::Unknown(e) => transform_unknown(e, transform),
        }
    }

    /// Apply rotation around an axis.
    pub fn apply_rotation(&mut self, axis: Vector3, angle: f64) {
        self.apply_transform(&Transform::from_rotation(axis, angle));
    }

    /// Apply uniform scaling.
    pub fn apply_scaling(&mut self, scale: f64) {
        self.apply_transform(&Transform::from_scale(scale));
    }

    /// Apply non-uniform scaling.
    pub fn apply_scaling_xyz(&mut self, scale: Vector3) {
        self.apply_transform(&Transform::from_scaling(scale));
    }

    /// Apply scaling with a specific origin point.
    pub fn apply_scaling_with_origin(&mut self, scale: Vector3, origin: Vector3) {
        self.apply_transform(&Transform::from_scaling_with_origin(scale, origin));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Vector3;

    #[test]
    fn test_transform_line() {
        let mut line = Line::from_points(Vector3::new(1.0, 0.0, 0.0), Vector3::new(2.0, 0.0, 0.0));
        let t = Transform::from_scale(2.0);
        transform_line(&mut line, &t);
        assert!((line.start.x - 2.0).abs() < 1e-10);
        assert!((line.end.x - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_transform_circle_scales_radius() {
        let mut circle = Circle::new();
        circle.center = Vector3::ZERO;
        circle.radius = 5.0;
        let t = Transform::from_scale(3.0);
        transform_circle(&mut circle, &t);
        assert!((circle.radius - 15.0).abs() < 1e-10);
    }

    #[test]
    fn test_transform_entity_type_dispatch() {
        let mut entity = EntityType::Circle({
            let mut c = Circle::new();
            c.radius = 5.0;
            c
        });
        entity.apply_transform(&Transform::from_scale(2.0));
        if let EntityType::Circle(c) = &entity {
            assert!((c.radius - 10.0).abs() < 1e-10);
        } else {
            panic!("Expected Circle");
        }
    }

    // Mirrored boundary arcs must remain continuous with adjacent lines.
    #[test]
    fn test_mirror_hatch_arc_edge_stays_continuous() {
        use crate::entities::hatch::{BoundaryEdge, BoundaryPath, CircularArcEdge, LineEdge};
        use crate::types::Vector2;

        // Path: line from (2,0)→... then a CCW half-circle r=1 centered at
        // (1,0) from angle 0 to π (i.e. (2,0) → (0,0)), then line back.
        let mut path = BoundaryPath::new();
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(0.0, -1.0),
            end: Vector2::new(2.0, 0.0),
        }));
        path.edges.push(BoundaryEdge::CircularArc(CircularArcEdge {
            center: Vector2::new(1.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
            counter_clockwise: true,
        }));
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(0.0, 0.0),
            end: Vector2::new(0.0, -1.0),
        }));
        let mut hatch = Hatch::new();
        hatch.paths.push(path);

        // Mirror across the Y axis (x → -x): det < 0, handedness flips.
        let t = Transform::from_scaling(Vector3::new(-1.0, 1.0, 1.0));
        transform_hatch(&mut hatch, &t);

        let edges = &hatch.paths[0].edges;
        let (l1, arc, l2) = match (&edges[0], &edges[1], &edges[2]) {
            (BoundaryEdge::Line(a), BoundaryEdge::CircularArc(b), BoundaryEdge::Line(c)) => {
                (a, b, c)
            }
            _ => panic!("edge kinds changed"),
        };
        // Stored-angle convention: true point of a CW edge is at -θ.
        let pt = |theta: f64, ccw: bool| {
            let a = if ccw { theta } else { -theta };
            (
                arc.center.x + arc.radius * a.cos(),
                arc.center.y + arc.radius * a.sin(),
            )
        };
        let (sx, sy) = pt(arc.start_angle, arc.counter_clockwise);
        let (ex, ey) = pt(arc.end_angle, arc.counter_clockwise);
        assert!(
            !arc.counter_clockwise,
            "mirror must flip the direction flag"
        );
        assert!(
            (sx - l1.end.x).abs() < 1e-9 && (sy - l1.end.y).abs() < 1e-9,
            "arc start {:?} must meet previous line end {:?}",
            (sx, sy),
            (l1.end.x, l1.end.y)
        );
        assert!(
            (ex - l2.start.x).abs() < 1e-9 && (ey - l2.start.y).abs() < 1e-9,
            "arc end {:?} must meet next line start {:?}",
            (ex, ey),
            (l2.start.x, l2.start.y)
        );
        // Midpoint sanity: the half-circle bulges DOWN after a y-axis mirror?
        // Original bulges up (+y); x-mirror keeps +y bulge at mirrored x.
        let mid_a = {
            let s = if arc.counter_clockwise {
                arc.start_angle
            } else {
                -arc.start_angle
            };
            let e = if arc.counter_clockwise {
                arc.end_angle
            } else {
                -arc.end_angle
            };
            let mut sweep = e - s;
            if arc.counter_clockwise && sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            if !arc.counter_clockwise && sweep >= 0.0 {
                sweep -= std::f64::consts::TAU;
            }
            s + sweep / 2.0
        };
        let (mx, my) = (
            arc.center.x + arc.radius * mid_a.cos(),
            arc.center.y + arc.radius * mid_a.sin(),
        );
        assert!(
            (mx - (-1.0)).abs() < 1e-9 && (my - 1.0).abs() < 1e-9,
            "arc midpoint {:?} must be the mirror of (1,1) → (-1,1)",
            (mx, my)
        );
    }

    // Wrapped boundary sweeps stay unnormalized through transforms.
    #[test]
    fn test_hatch_arc_wrap_sweep_survives_transform() {
        use crate::entities::hatch::{BoundaryEdge, BoundaryPath, CircularArcEdge};
        use crate::types::Vector2;

        let mk = || {
            let mut path = BoundaryPath::new();
            path.edges.push(BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(10.0, 5.0),
                radius: 61.3,
                start_angle: 5.80985,
                end_angle: 6.63571, // > 2π: wrap-encoded short arc
                counter_clockwise: false,
            }));
            let mut h = Hatch::new();
            h.paths.push(path);
            h
        };

        // Pure translation: angles must be untouched.
        let mut h = mk();
        transform_hatch(
            &mut h,
            &Transform::from_translation(Vector3::new(3.0, -2.0, 0.0)),
        );
        let BoundaryEdge::CircularArc(a) = &h.paths[0].edges[0] else {
            panic!()
        };
        assert!(
            (a.start_angle - 5.80985).abs() < 1e-9,
            "start changed: {}",
            a.start_angle
        );
        assert!(
            (a.end_angle - 6.63571).abs() < 1e-9,
            "end changed: {}",
            a.end_angle
        );
        assert!(!a.counter_clockwise);

        // Mirror: sweep magnitude must survive (0.82586), direction flag flips.
        let mut h = mk();
        transform_hatch(
            &mut h,
            &Transform::from_scaling(Vector3::new(-1.0, 1.0, 1.0)),
        );
        let BoundaryEdge::CircularArc(a) = &h.paths[0].edges[0] else {
            panic!()
        };
        assert!(a.counter_clockwise, "mirror must flip the flag");
        let sweep = a.end_angle - a.start_angle;
        assert!(
            (sweep - 0.82586).abs() < 1e-9,
            "sweep must stay 0.82586, got {sweep}"
        );
    }

    // Exploding a mirrored INSERT routes plain entities through
    // apply_transform → transform_lwpolyline. The bulge encodes which side
    // the arc bows to; a reflection must negate it or every bulged segment
    // bows the wrong way after EXPLODE.
    #[test]
    fn test_reflecting_transform_negates_lwpolyline_bulge() {
        let mut lw = LwPolyline::new();
        let mut v0 = LwVertex::new(crate::types::Vector2::new(0.0, 0.0));
        v0.bulge = 0.5;
        let mut v1 = LwVertex::new(crate::types::Vector2::new(10.0, 0.0));
        v1.bulge = -0.3;
        lw.add_vertex(v0);
        lw.add_vertex(v1);

        // Mirror across Y (x → -x): reflection, bulges must negate.
        let mut a = lw.clone();
        transform_lwpolyline(
            &mut a,
            &Transform::from_scaling(Vector3::new(-1.0, 1.0, 1.0)),
        );
        assert!((a.vertices[0].bulge - (-0.5)).abs() < 1e-12);
        assert!((a.vertices[1].bulge - 0.3).abs() < 1e-12);

        // Pure rotation (det > 0): bulges untouched.
        let mut b = lw.clone();
        transform_lwpolyline(
            &mut b,
            &Transform::from_rotation(Vector3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2),
        );
        assert!((b.vertices[0].bulge - 0.5).abs() < 1e-12);
        assert!((b.vertices[1].bulge - (-0.3)).abs() < 1e-12);
    }
}
