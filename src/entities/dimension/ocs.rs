// OCS/WCS projection and precision-origin shifts.

use super::*;

pub(crate) fn vec3_local(v: Vector3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// WCS point used as the local frame origin while one dimension is
/// tessellated.
///
/// Dimension geometry is assembled in `Vec3` (f32). On a survey drawing at
/// UTM-scale coordinates (eastings ~1.65e6, northings ~4.95e6) an absolute
/// f32 has an ulp of ~0.5 m, so the dimensional lines, extension lines and
/// arrows snap to a half-metre grid and visibly drift away from the points
/// they measure. Shifting the dimension's definition data to a nearby origin
/// first keeps every f32 operation exact to well under a millimetre; the
/// origin is folded back into the double-single wire buffers on output.
pub(crate) fn dimension_precision_origin(dim: &Dimension) -> Vector3 {
    let point = match dim {
        Dimension::Aligned(d) => d.first_point,
        Dimension::Linear(d) => d.first_point,
        Dimension::Radius(d) => d.angle_vertex,
        Dimension::Diameter(d) => d.angle_vertex,
        Dimension::Angular2Ln(d) => d.angle_vertex,
        Dimension::Angular3Pt(d) => d.angle_vertex,
        Dimension::Ordinate(d) => d.feature_location,
        Dimension::Arc(d) => d.center_point,
        Dimension::LargeRadial(d) => d.chord_point,
    };
    if point.x.is_finite() && point.y.is_finite() && point.z.is_finite() {
        point
    } else {
        {
            let base = dim.base();
            if base.definition_point.x.is_finite() {
                base.definition_point
            } else {
                Vector3::ZERO
            }
        }
    }
}

pub(crate) fn point_from_origin(point: Vector3, origin: Vector3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

/// Same, but keeps the all-zero "not stored" sentinel some base fields use
/// (text placement and the legacy insertion point). Shifting a zero there
/// would turn "automatic" into a bogus explicit point at the far origin.
pub(crate) fn stored_point_from_origin(point: Vector3, origin: Vector3) -> Vector3 {
    if point == Vector3::ZERO {
        point
    } else {
        point_from_origin(point, origin)
    }
}

/// `dim` with every definition point moved into the local frame at `origin`.
/// Measurements, angles and vectors are translation-invariant, so the shifted
/// dimension reports the same values while its `Vec3` geometry stays small.
pub(crate) fn shift_dimension(dim: &Dimension, origin: Vector3) -> Dimension {
    let mut shifted = dim.clone();
    let base = shifted.base_mut();
    base.definition_point = point_from_origin(base.definition_point, origin);
    base.text_middle_point = stored_point_from_origin(base.text_middle_point, origin);
    base.insertion_point = stored_point_from_origin(base.insertion_point, origin);
    match &mut shifted {
        Dimension::Aligned(d) => {
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Linear(d) => {
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Radius(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Diameter(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Angular2Ln(d) => {
            d.dimension_arc = point_from_origin(d.dimension_arc, origin);
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
            d.definition_point = point_from_origin(d.definition_point, origin);
        }
        Dimension::Angular3Pt(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.first_point = point_from_origin(d.first_point, origin);
            d.second_point = point_from_origin(d.second_point, origin);
            d.angle_vertex = point_from_origin(d.angle_vertex, origin);
        }
        Dimension::Ordinate(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.feature_location = point_from_origin(d.feature_location, origin);
            d.leader_endpoint = point_from_origin(d.leader_endpoint, origin);
        }
        Dimension::Arc(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.first_extension_point = point_from_origin(d.first_extension_point, origin);
            d.second_extension_point = point_from_origin(d.second_extension_point, origin);
            d.center_point = point_from_origin(d.center_point, origin);
            d.first_leader_point = point_from_origin(d.first_leader_point, origin);
            d.second_leader_point = point_from_origin(d.second_leader_point, origin);
        }
        Dimension::LargeRadial(d) => {
            d.definition_point = point_from_origin(d.definition_point, origin);
            d.chord_point = point_from_origin(d.chord_point, origin);
            d.override_center = point_from_origin(d.override_center, origin);
            d.jog_point = point_from_origin(d.jog_point, origin);
        }
    }
    shifted
}

/// Move one wire built in the local frame back to world space, splitting each
/// vertex into the double-single pair the renderer reconstructs relative to
/// the eye. Leaves `snap_pts` alone: those are absolute source points, not
/// locally-computed geometry.
pub(crate) fn translate_wire_from_origin(wire: &mut WireModel, origin: Vector3) {
    let translate = |high: &mut Vec<[f32; 3]>, low: &mut Vec<[f32; 3]>| {
        if high.is_empty() {
            return;
        }
        let old_low = std::mem::take(low);
        low.reserve(high.len());
        for (index, point) in high.iter_mut().enumerate() {
            if !point[0].is_finite() {
                low.push([0.0; 3]);
                continue;
            }
            let residual = old_low.get(index).copied().unwrap_or([0.0; 3]);
            let x = point[0] as f64 + residual[0] as f64 + origin.x;
            let y = point[1] as f64 + residual[1] as f64 + origin.y;
            let z = point[2] as f64 + residual[2] as f64 + origin.z;
            let (hx, lx) = WireModel::split_ds(x);
            let (hy, ly) = WireModel::split_ds(y);
            let (hz, lz) = WireModel::split_ds(z);
            *point = [hx, hy, hz];
            low.push([lx, ly, lz]);
        }
    };
    let geom = wire.geom_mut();
    translate(&mut geom.points, &mut geom.points_low);
    translate(&mut geom.fill_tris, &mut geom.fill_tris_low);
    translate(&mut geom.pick_tris, &mut geom.pick_tris_low);
    for point in &mut wire.key_vertices {
        point[0] += origin.x;
        point[1] += origin.y;
        point[2] += origin.z;
    }
}

/// Style-driven text anchor on the dimension line: a point on the line through
/// `defpt` parallel to (`ax`,`ay`), slid along it per DIMJUST and lifted by
/// `perp_off` toward the side the dimension line sits on.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_on_dim_line(
    first: Vector3,
    second: Vector3,
    defpt: Vector3,
    ax: f64,
    ay: f64,
    dimjust: i16,
    perp_off: f64,
    text_w: f64,
    gap: f64,
    arrow: f64,
    dimtix: bool,
    dimatfit: i16,
    tad: i16,
    horizontal_outside: bool,
) -> Vector3 {
    // The perpendicular "up" side: the perpendicular of the dimension line
    // normalised so the text reads left-to-right (or bottom-to-top when
    // vertical). DIMTAD "above" (1) and JIS (3) sit on this side.
    let (mut nx, mut ny) = (ax, ay);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let px = -ny;
    let py = nx;
    // DIMTAD side:
    //   1 (above) / 3 (JIS) / 0 (centred) → the text-up side, independent of the
    //     object — keying it off the object flipped "above" to "below" whenever
    //     the dimension ran the opposite way (#144);
    //   4 (below)   → the opposite side;
    //   2 (outside) → the side farthest from the defining points (the object).
    let perp_sign = match tad {
        4 => -1.0,
        2 => {
            let off = (defpt.x - first.x) * px + (defpt.y - first.y) * py;
            if off >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 1.0,
    };
    // Along-axis positions of the extension points relative to the dim line.
    let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    // DIMJUST: 0=centred, 1/3=near first ext, 2/4=near second ext. Beside
    // an extension line (1/2) the text starts an arrow and two gaps from it,
    // towards the other one; `text_w` already carries a gap each side.
    let inward = if t2 >= t1 { 1.0 } else { -1.0 };
    let beside = arrow + gap + (text_w * 0.5).max(0.0);
    let mut along = match dimjust {
        1 => t1 + inward * beside,
        2 => t2 - inward * beside,
        3 => t1,
        4 => t2,
        _ => (t1 + t2) * 0.5,
    };
    // DIMATFIT / DIMTIX fit: move text according to the selected priority when
    // the combined text-and-arrow envelope cannot fit.
    if dimjust == 0 && !dimtix && text_w > 0.0 {
        let lo = t1.min(t2);
        let hi = t1.max(t2);
        let span = hi - lo;
        let insufficient = text_w + 2.0 * arrow > span;
        let text_outside = insufficient
            && match dimatfit {
                0 | 2 => true,
                1 | 3 => text_w > span,
                _ => text_w > span,
            };
        if text_outside {
            // Past the second extension line (the side picked last), the
            // two-arrow stub the dimension line grows outside it, then the
            // gap that `text_w` already carries.
            along = t2 + inward * (2.0 * arrow + text_w * 0.5);
            if horizontal_outside && ay.abs() > 1e-3 {
                // Horizontal text beside a hook: the line runs out two arrow
                // lengths, the hook turns one arrow toward the text-up side
                // and the text sits against it, centred on the hook.
                let elbow_along = t2 + inward * 2.0 * arrow;
                let side = if px >= 0.0 { 1.0 } else { -1.0 };
                let elbow_x = defpt.x + ax * elbow_along;
                let elbow_y = defpt.y + ay * elbow_along;
                let vertical = if tad == 0 { 0.0 } else { perp_off * perp_sign };
                return Vector3::new(
                    elbow_x + side * (arrow + text_w * 0.5),
                    elbow_y + vertical,
                    defpt.z,
                );
            }
        }
    }
    let bx = defpt.x + ax * along;
    let by = defpt.y + ay * along;
    Vector3::new(
        bx + px * perp_off * perp_sign,
        by + py * perp_off * perp_sign,
        defpt.z,
    )
}

pub(crate) fn text_on_single_arrow_dim_line(
    first: Vector3,
    second: Vector3,
    defpt: Vector3,
    ax: f64,
    ay: f64,
    perp_off: f64,
    text_width: f64,
    arrow: f64,
    dimtix: bool,
    dimatfit: i16,
    tad: i16,
) -> Vector3 {
    let (mut nx, mut ny) = (ax, ay);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let (perpendicular_x, perpendicular_y) = (-ny, nx);
    let perpendicular_sign = match tad {
        4 => -1.0,
        2 => {
            let offset =
                (defpt.x - first.x) * perpendicular_x + (defpt.y - first.y) * perpendicular_y;
            if offset >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 1.0,
    };
    let first_along = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
    let second_along = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
    let low = first_along.min(second_along);
    let high = first_along.max(second_along);
    let available = high - low;
    let insufficient = text_width + arrow > available;
    let text_outside = !dimtix
        && insufficient
        && match dimatfit {
            0 | 2 => true,
            1 | 3 => text_width > available,
            _ => text_width > available,
        };
    let along = if text_outside {
        let outward = if second_along >= first_along {
            1.0
        } else {
            -1.0
        };
        second_along + outward * (2.0 * arrow + text_width * 0.5)
    } else {
        (first_along + second_along) * 0.5
    };
    Vector3::new(
        defpt.x + ax * along + perpendicular_x * perp_off * perpendicular_sign,
        defpt.y + ay * along + perpendicular_y * perp_off * perpendicular_sign,
        defpt.z,
    )
}

/// The stored text middle point (group 11) applies to automatic and hand-placed
/// text alike. A regenerated dimension keeps any non-zero point; only a cleared
/// point is placed from the style.
pub(crate) fn stored_text_point(dim: &Dimension) -> Option<Vector3> {
    let p = dim.base().text_middle_point;
    (p.x * p.x + p.y * p.y + p.z * p.z > 1e-16).then_some(p)
}
