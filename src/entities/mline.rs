use acadrust::entities::{MLine, MLineFlags, MLineSegment};
use acadrust::types::Vector3;
use cadkernel::geom2d::{intersect::line_line, Vec2};
use cadkernel::space::Plane;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, RenderConvertible, Transformable};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::t;

/// One styled line or cap of a multiline.
pub struct MLineLine {
    pub points: Vec<[f64; 3]>,
    pub color: acadrust::types::Color,
    pub linetype: String,
}

pub(crate) fn rebuild_mline_geometry(mline: &mut MLine) -> bool {
    let count = mline.vertices.len();
    mline.flags.set(MLineFlags::HAS_VERTICES, count > 0);
    let Some(first) = mline.vertices.first() else {
        mline.start_point = Vector3::ZERO;
        return true;
    };
    mline.start_point = first.position;
    if count == 1 {
        return true;
    }

    let points: Vec<[f64; 3]> = mline
        .vertices
        .iter()
        .map(|vertex| [vertex.position.x, vertex.position.y, vertex.position.z])
        .collect();
    let axis = points
        .windows(2)
        .find_map(|pair| {
            (cadkernel::space::Vec3::from(pair[1]) - cadkernel::space::Vec3::from(pair[0]))
                .normalize()
                .map(cadkernel::space::Vec3::to_array)
        })
        .or_else(|| {
            mline.is_closed()
                .then(|| {
                    (cadkernel::space::Vec3::from(points[0])
                        - cadkernel::space::Vec3::from(points[count - 1]))
                    .normalize()
                })
                .flatten()
                .map(cadkernel::space::Vec3::to_array)
        });
    let Some(plane) = axis.and_then(|axis| {
        Plane::orthonormal(
            points[0],
            axis,
            [mline.normal.x, mline.normal.y, mline.normal.z],
        )
    }) else {
        return false;
    };
    let Some(flat): Option<Vec<Vec2>> = points
        .iter()
        .map(|point| plane.project(*point).map(Vec2::from))
        .collect()
    else {
        return false;
    };
    let closed = mline.is_closed() && count >= 3;
    let direction_after = |index: usize| {
        let attempts = if closed {
            count - 1
        } else {
            count.saturating_sub(index + 1)
        };
        (1..=attempts).find_map(|step| {
            let next = if closed {
                (index + step) % count
            } else {
                index + step
            };
            (flat[next] - flat[index]).normalize()
        })
    };
    let direction_before = |index: usize| {
        let attempts = if closed { count - 1 } else { index };
        (1..=attempts).find_map(|step| {
            let previous = if closed {
                (index + count - step % count) % count
            } else {
                index - step
            };
            (flat[index] - flat[previous]).normalize()
        })
    };

    for index in 0..count {
        let before = direction_before(index);
        let after = direction_after(index);
        let Some(direction) = after.or(before) else {
            continue;
        };
        let miter = match (before, after) {
            (Some(before), Some(after)) => {
                let first_offset = flat[index] + before.perpendicular();
                let second_offset = flat[index] + after.perpendicular();
                line_line(
                    first_offset.to_array(),
                    before.to_array(),
                    second_offset.to_array(),
                    after.to_array(),
                )
                .and_then(|(at, _)| {
                    (first_offset + before * at - flat[index]).normalize()
                })
                .unwrap_or_else(|| after.perpendicular())
            }
            (Some(direction), None) | (None, Some(direction)) => direction.perpendicular(),
            (None, None) => continue,
        };
        let direction = plane.vector_at(direction.to_array());
        let miter = plane.vector_at(miter.to_array());
        mline.vertices[index].direction = Vector3::new(direction[0], direction[1], direction[2]);
        mline.vertices[index].miter = Vector3::new(miter[0], miter[1], miter[2]);
    }
    true
}

fn adjusted_mline_endpoint(
    mline: &MLine,
    style: &acadrust::objects::MLineStyle,
    vertex: usize,
    point: [f64; 3],
) -> [f64; 3] {
    let angle = if vertex == 0 {
        style.start_angle
    } else {
        style.end_angle
    };
    let Some(tangent) = cadkernel::space::Vec3::new(
        mline.vertices[vertex].direction.x,
        mline.vertices[vertex].direction.y,
        mline.vertices[vertex].direction.z,
    )
    .normalize()
    else {
        return point;
    };
    let Some(normal) =
        cadkernel::space::Vec3::new(mline.normal.x, mline.normal.y, mline.normal.z).normalize()
    else {
        return point;
    };
    let Some(transverse) = normal.cross(tangent).normalize() else {
        return point;
    };
    let base = cadkernel::space::Vec3::new(
        mline.vertices[vertex].position.x,
        mline.vertices[vertex].position.y,
        mline.vertices[vertex].position.z,
    );
    let current = cadkernel::space::Vec3::from(point);
    let tangent = if angle.tan().abs() > 1.0e-9 {
        tangent * ((current - base).dot(transverse) / angle.tan())
    } else {
        cadkernel::space::Vec3::ZERO
    };
    (current + tangent).to_array()
}

pub(crate) fn mline_drawn_ranges(parameters: &[f64], length: f64) -> Vec<(f64, f64)> {
    if parameters.len() <= 1 {
        return vec![(0.0, length)];
    }
    let mut cursor = parameters[1].clamp(0.0, length);
    if parameters.len() == 2 {
        return (cursor < length - 1.0e-9)
            .then_some((cursor, length))
            .into_iter()
            .collect();
    }
    let mut drawn = true;
    let mut ranges = Vec::new();
    for distance in &parameters[2..] {
        let next = (cursor + distance.max(0.0)).clamp(0.0, length);
        if drawn && next - cursor > 1.0e-9 {
            ranges.push((cursor, next));
        }
        cursor = next;
        drawn = !drawn;
    }
    if drawn && cursor < length - 1.0e-9 {
        ranges.push((cursor, length));
    }
    ranges
}

fn mline_fill_ranges(parameters: &[f64], length: f64) -> Vec<(f64, f64)> {
    let mut cuts: Vec<(f64, f64)> = parameters
        .chunks(2)
        .filter_map(|pair| {
            let start = pair[0].clamp(0.0, length);
            let end = pair.get(1).copied().unwrap_or(length).clamp(0.0, length);
            (end - start > 1.0e-9).then_some((start, end))
        })
        .collect();
    cuts.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut drawn = Vec::new();
    let mut start = 0.0;
    for cut in cuts {
        if cut.0 > start + 1.0e-9 {
            drawn.push((start, cut.0));
        }
        start = start.max(cut.1);
    }
    if start < length - 1.0e-9 {
        drawn.push((start, length));
    }
    drawn
}

fn mline_cut_ranges(parameters: &[f64], length: f64) -> Vec<(f64, f64)> {
    parameters
        .chunks(2)
        .filter_map(|pair| {
            let start = pair[0].clamp(0.0, length);
            let end = pair.get(1).copied().unwrap_or(length).clamp(0.0, length);
            (end - start > 1.0e-9).then_some((start, end))
        })
        .collect()
}

pub(crate) fn store_mline_drawn_ranges(
    parameters: &mut Vec<f64>,
    length: f64,
    ranges: &[(f64, f64)],
) {
    let offset = parameters.first().copied().unwrap_or(0.0);
    parameters.clear();
    parameters.push(offset);
    let ranges = merge_mline_ranges(
        ranges
            .iter()
            .filter_map(|range| {
                let start = range.0.clamp(0.0, length);
                let end = range.1.clamp(0.0, length);
                (end - start > 1.0e-9).then_some((start, end))
            })
            .collect(),
    );
    if ranges.is_empty() {
        parameters.extend([0.0, 0.0]);
    } else if ranges.len() == 1
        && ranges[0].0 <= 1.0e-9
        && ranges[0].1 >= length - 1.0e-9
    {
        parameters.push(0.0);
    } else {
        parameters.push(ranges[0].0);
        for (index, (start, end)) in ranges.iter().enumerate() {
            if index > 0 {
                parameters.push(start - ranges[index - 1].1);
            }
            parameters.push(end - start);
        }
    }
}

fn store_mline_cut_ranges(parameters: &mut Vec<f64>, length: f64, ranges: &[(f64, f64)]) {
    parameters.clear();
    for (start, end) in ranges {
        parameters.push(*start);
        if *end < length - 1.0e-9 {
            parameters.push(*end);
        }
    }
}

fn remap_mline_ranges(
    ranges: &[(f64, f64)],
    source_start: f64,
    source_end: f64,
    target_length: f64,
) -> Vec<(f64, f64)> {
    let source_length = source_end - source_start;
    if source_length <= 1.0e-12 || target_length <= 1.0e-12 {
        return Vec::new();
    }
    ranges
        .iter()
        .filter_map(|range| {
            let start = range.0.max(source_start);
            let end = range.1.min(source_end);
            (end - start > 1.0e-9).then_some((
                (start - source_start) / source_length * target_length,
                (end - source_start) / source_length * target_length,
            ))
        })
        .collect()
}

fn shift_mline_ranges(
    ranges: &[(f64, f64)],
    source_length: f64,
    shift: f64,
    target_length: f64,
) -> Vec<(f64, f64)> {
    ranges
        .iter()
        .filter_map(|range| {
            let start = if range.0 <= 1.0e-9 {
                0.0
            } else {
                (range.0 + shift).clamp(0.0, target_length)
            };
            let end = if range.1 >= source_length - 1.0e-9 {
                target_length
            } else {
                (range.1 + shift).clamp(0.0, target_length)
            };
            (end - start > 1.0e-9).then_some((start, end))
        })
        .collect()
}

fn merge_mline_ranges(mut ranges: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.0 <= last.1 + 1.0e-9 {
                last.1 = last.1.max(range.1);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

fn mline_element_endpoints(
    mline: &MLine,
    vertex: usize,
    element: usize,
) -> Option<(cadkernel::space::Vec3, cadkernel::space::Vec3)> {
    let next = if vertex + 1 < mline.vertices.len() {
        vertex + 1
    } else if mline.is_closed() {
        0
    } else {
        return None;
    };
    let point = |index: usize| {
        let vertex = mline.vertices.get(index)?;
        let offset = vertex.segments.get(element)?.parameters.first().copied()?;
        let position = cadkernel::space::Vec3::new(
            vertex.position.x,
            vertex.position.y,
            vertex.position.z,
        );
        let miter = cadkernel::space::Vec3::new(vertex.miter.x, vertex.miter.y, vertex.miter.z);
        Some(position + miter * offset)
    };
    Some((point(vertex)?, point(next)?))
}

fn snapshot_mline_segment(mline: &MLine, vertex: usize) -> Option<Vec<(MLineSegment, f64)>> {
    (0..mline.style_element_count)
        .map(|element| {
            let endpoints = mline_element_endpoints(mline, vertex, element)?;
            Some((
                mline.vertices.get(vertex)?.segments.get(element)?.clone(),
                endpoints.0.distance(endpoints.1),
            ))
        })
        .collect()
}

fn restore_shifted_mline_segment(
    mline: &mut MLine,
    vertex: usize,
    source: Vec<(MLineSegment, f64)>,
    moved_start: bool,
) {
    for (element, (source, source_length)) in source.into_iter().enumerate() {
        let Some(endpoints) = mline_element_endpoints(mline, vertex, element) else {
            continue;
        };
        let target_length = endpoints.0.distance(endpoints.1);
        let shift = if moved_start {
            target_length - source_length
        } else {
            0.0
        };
        let drawn = shift_mline_ranges(
            &mline_drawn_ranges(&source.parameters, source_length),
            source_length,
            shift,
            target_length,
        );
        let cuts = shift_mline_ranges(
            &mline_cut_ranges(&source.area_fill_parameters, source_length),
            source_length,
            shift,
            target_length,
        );
        let Some(target) = mline
            .vertices
            .get_mut(vertex)
            .and_then(|vertex| vertex.segments.get_mut(element))
        else {
            continue;
        };
        store_mline_drawn_ranges(&mut target.parameters, target_length, &drawn);
        store_mline_cut_ranges(&mut target.area_fill_parameters, target_length, &cuts);
    }
}

/// Resolve a multiline into styled parallel lines in WCS.
pub fn resolved_mline_style<'a>(
    m: &MLine,
    document: &'a acadrust::CadDocument,
) -> Option<&'a acadrust::objects::MLineStyle> {
    use acadrust::objects::ObjectType;

    m.style_handle
        .and_then(|handle| match document.objects.get(&handle) {
            Some(ObjectType::MLineStyle(style)) => Some(style),
            _ => None,
        })
        .or_else(|| {
            document.objects.values().find_map(|object| match object {
                ObjectType::MLineStyle(style)
                    if style.name.eq_ignore_ascii_case(&m.style_name) =>
                {
                    Some(style)
                }
                _ => None,
            })
        })
}

pub fn mline_lines(m: &MLine, document: &acadrust::CadDocument) -> Vec<MLineLine> {
    mline_lines_resolved(m, resolved_mline_style(m, document))
}

pub fn mline_lines_with_style(
    m: &MLine,
    style: &acadrust::objects::MLineStyle,
) -> Vec<MLineLine> {
    mline_lines_resolved(m, Some(style))
}

fn mline_lines_resolved(
    m: &MLine,
    style: Option<&acadrust::objects::MLineStyle>,
) -> Vec<MLineLine> {
    use acadrust::entities::{MLineFlags, MLineJustification};
    use acadrust::types::Color;

    if m.vertices.is_empty() {
        return Vec::new();
    }

    // (offset, colour, linetype) per element.
    let elems: Vec<(f64, Color, String)> = match style {
        Some(s) if !s.elements.is_empty() => s
            .elements
            .iter()
            .map(|e| (e.offset, e.color, e.linetype.clone()))
            .collect(),
        _ => vec![
            (0.5, Color::ByLayer, "ByLayer".to_string()),
            (-0.5, Color::ByLayer, "ByLayer".to_string()),
        ],
    };

    // Stored parameters already include justification; this shift is the fallback.
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (o, _, _) in &elems {
        lo = lo.min(*o);
        hi = hi.max(*o);
    }
    let shift = match m.justification {
        MLineJustification::Top => -hi,
        MLineJustification::Bottom => -lo,
        MLineJustification::Zero => 0.0,
    };

    let scale = m.scale_factor;
    let closed = m.flags.contains(MLineFlags::CLOSED);
    let n = m.vertices.len();

    // parameter[0] stores the element distance along the miter.
    let elem_off = |vi: usize, ei: usize| -> f64 {
        m.vertices[vi]
            .segments
            .get(ei)
            .and_then(|sg| sg.parameters.first().copied())
            .unwrap_or_else(|| (elems[ei].0 + shift) * scale)
    };
    let off_pt = |vi: usize, d: f64| -> [f64; 3] {
        let v = &m.vertices[vi];
        (cadkernel::space::Vec3::new(v.position.x, v.position.y, v.position.z)
            + cadkernel::space::Vec3::new(v.miter.x, v.miter.y, v.miter.z) * d)
            .to_array()
    };
    let endpoint_pt = |vi: usize, ei: usize| -> [f64; 3] {
        let point = off_pt(vi, elem_off(vi, ei));
        if closed || (vi != 0 && vi + 1 != n) {
            return point;
        }
        let Some(style) = style else {
            return point;
        };
        adjusted_mline_endpoint(m, style, vi, point)
    };

    let mut out: Vec<MLineLine> = Vec::with_capacity(elems.len() + 2);
    for (ei, (_, color, linetype)) in elems.iter().enumerate() {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        // Parameters store the start offset, then alternating drawn and gap lengths.
        let seg_count = if closed { n } else { n.saturating_sub(1) };
        let mut pen_at_end = false;
        for k in 0..seg_count {
            let vi = k;
            let wi = (k + 1) % n;
            let a = if !closed && vi == 0 {
                endpoint_pt(vi, ei)
            } else {
                off_pt(vi, elem_off(vi, ei))
            };
            let b = if !closed && wi + 1 == n {
                endpoint_pt(wi, ei)
            } else {
                off_pt(wi, elem_off(wi, ei))
            };
            let a = cadkernel::space::Vec3::from(a);
            let b = cadkernel::space::Vec3::from(b);
            let segment = b - a;
            let len = segment.length();
            let Some(direction) = segment.normalize() else {
                continue;
            };
            let at = |t: f64| (a + direction * t).to_array();
            let parameters: &[f64] = m
                .vertices[vi]
                .segments
                .get(ei)
                .map(|segment| segment.parameters.as_slice())
                .unwrap_or(&[]);
            let runs = mline_drawn_ranges(parameters, len);
            for (ri, (t0, t1)) in runs.iter().enumerate() {
                let continuous = pen_at_end && ri == 0 && *t0 <= 1e-9 && !pts.is_empty();
                if !continuous {
                    if !pts.is_empty() {
                        pts.push([f64::NAN; 3]);
                    }
                    pts.push(at(*t0));
                }
                pts.push(at(*t1));
            }
            pen_at_end = runs.last().is_some_and(|(_, t1)| (len - t1).abs() <= 1e-9);
        }
        if pts.len() < 2 {
            continue;
        }
        out.push(MLineLine {
            points: pts,
            color: *color,
            linetype: linetype.clone(),
        });
    }

    // Style-defined joints and end caps.
    if let Some(s) = style {
        let outer_points = |vi: usize, endpoint: bool| -> Option<([f64; 3], [f64; 3])> {
            let mut order: Vec<usize> = (0..elems.len()).collect();
            order.sort_by(|a, b| elem_off(vi, *a).total_cmp(&elem_off(vi, *b)));
            let first = *order.first()?;
            let last = *order.last()?;
            let point = |ei| {
                if endpoint {
                    endpoint_pt(vi, ei)
                } else {
                    off_pt(vi, elem_off(vi, ei))
                }
            };
            Some((point(first), point(last)))
        };

        if s.flags.display_joints {
            let vertices: Box<dyn Iterator<Item = usize>> = if closed {
                Box::new(0..n)
            } else {
                Box::new(1..n.saturating_sub(1))
            };
            for vi in vertices {
                if let Some((a, b)) = outer_points(vi, false) {
                    out.push(MLineLine {
                        points: vec![a, b],
                        color: Color::ByLayer,
                        linetype: "ByLayer".to_string(),
                    });
                }
            }
        }

        if !closed && n >= 2 {
            let start_suppressed = m.flags.contains(MLineFlags::NO_START_CAPS);
            let end_suppressed = m.flags.contains(MLineFlags::NO_END_CAPS);
            for (vi, start, suppressed, square, inner, round) in [
                (
                    0,
                    true,
                    start_suppressed,
                    s.flags.start_square_cap,
                    s.flags.start_inner_arcs_cap,
                    s.flags.start_round_cap,
                ),
                (
                    n - 1,
                    false,
                    end_suppressed,
                    s.flags.end_square_cap,
                    s.flags.end_inner_arcs_cap,
                    s.flags.end_round_cap,
                ),
            ] {
                if suppressed {
                    continue;
                }
                let Some((a, b)) = outer_points(vi, true) else {
                    continue;
                };
                if square {
                    out.push(MLineLine {
                        points: vec![a, b],
                        color: Color::ByLayer,
                        linetype: "ByLayer".to_string(),
                    });
                }
                let direction = [
                    m.vertices[vi].direction.x,
                    m.vertices[vi].direction.y,
                    m.vertices[vi].direction.z,
                ];
                if round {
                    out.push(MLineLine {
                        points: semicircle_cap(a, b, direction, start),
                        color: Color::ByLayer,
                        linetype: "ByLayer".to_string(),
                    });
                }
                if inner && elems.len() > 2 {
                    let mut order: Vec<usize> = (0..elems.len()).collect();
                    order.sort_by(|left, right| {
                        elem_off(vi, *left).total_cmp(&elem_off(vi, *right))
                    });
                    for pair in order.windows(2) {
                        out.push(MLineLine {
                            points: semicircle_cap(
                                endpoint_pt(vi, pair[0]),
                                endpoint_pt(vi, pair[1]),
                                direction,
                                start,
                            ),
                            color: Color::ByLayer,
                            linetype: "ByLayer".to_string(),
                        });
                    }
                }
            }
        }
    }

    out
}

fn semicircle_cap(
    first: [f64; 3],
    second: [f64; 3],
    direction: [f64; 3],
    start: bool,
) -> Vec<[f64; 3]> {
    let first = cadkernel::space::Vec3::from(first);
    let second = cadkernel::space::Vec3::from(second);
    let center = (first + second) * 0.5;
    let transverse = (first - second) * 0.5;
    let radius = transverse.length();
    let Some(x_axis) = transverse.normalize() else {
        return Vec::new();
    };
    let Some(direction) = cadkernel::space::Vec3::from(direction).normalize() else {
        return Vec::new();
    };
    let y_axis = if start { -direction } else { direction };
    cadkernel::space::PlanarCurve::new(
        Plane::from_axes(center.to_array(), x_axis.to_array(), y_axis.to_array()),
        cadkernel::geom2d::Curve::Arc(cadkernel::geom2d::Arc {
            centre: [0.0, 0.0],
            radius,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        }),
    )
    .tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE)
}

pub fn mline_fill_triangles_with_style(
    m: &MLine,
    style: &acadrust::objects::MLineStyle,
) -> Vec<[f64; 3]> {
    if !style.flags.fill_on || m.vertices.len() < 2 || style.elements.len() < 2 {
        return Vec::new();
    }
    let (low_index, high_index) = style
        .elements
        .iter()
        .enumerate()
        .fold((0, 0), |(low, high), (index, element)| {
            let offset = element.offset * m.scale_factor;
            let low = if offset < style.elements[low].offset * m.scale_factor {
                index
            } else {
                low
            };
            let high = if offset > style.elements[high].offset * m.scale_factor {
                index
            } else {
                high
            };
            (low, high)
        });
    let minimum = style
        .elements
        .iter()
        .map(|element| element.offset)
        .fold(f64::INFINITY, f64::min);
    let maximum = style
        .elements
        .iter()
        .map(|element| element.offset)
        .fold(f64::NEG_INFINITY, f64::max);
    let shift = match m.justification {
        acadrust::entities::MLineJustification::Top => -maximum,
        acadrust::entities::MLineJustification::Zero => 0.0,
        acadrust::entities::MLineJustification::Bottom => -minimum,
    };
    let offset_point = |vertex: usize, element: usize, endpoint: bool| -> [f64; 3] {
        let item = &m.vertices[vertex];
        let distance = item
            .segments
            .get(element)
            .and_then(|segment| segment.parameters.first())
            .copied()
            .unwrap_or((style.elements[element].offset + shift) * m.scale_factor);
        let point = (cadkernel::space::Vec3::new(
            item.position.x,
            item.position.y,
            item.position.z,
        ) + cadkernel::space::Vec3::new(item.miter.x, item.miter.y, item.miter.z) * distance)
            .to_array();
        if endpoint {
            adjusted_mline_endpoint(m, style, vertex, point)
        } else {
            point
        }
    };
    let closed = m.flags.contains(MLineFlags::CLOSED);
    let segment_count = if closed {
        m.vertices.len()
    } else {
        m.vertices.len() - 1
    };
    let mut triangles = Vec::with_capacity(segment_count * 6);
    for vertex in 0..segment_count {
        let next = (vertex + 1) % m.vertices.len();
        let start_endpoint = !closed && vertex == 0;
        let end_endpoint = !closed && next + 1 == m.vertices.len();
        let low_start = cadkernel::space::Vec3::from(offset_point(
            vertex,
            low_index,
            start_endpoint,
        ));
        let low_end = cadkernel::space::Vec3::from(offset_point(next, low_index, end_endpoint));
        let high_start = cadkernel::space::Vec3::from(offset_point(
            vertex,
            high_index,
            start_endpoint,
        ));
        let high_end = cadkernel::space::Vec3::from(offset_point(next, high_index, end_endpoint));
        let low_length = low_start.distance(low_end);
        let high_length = high_start.distance(high_end);
        if low_length <= 1.0e-12 || high_length <= 1.0e-12 {
            continue;
        }
        let fill_ranges = |element: usize, length: f64| {
            let parameters = m.vertices[vertex]
                .segments
                .get(element)
                .map(|segment| segment.area_fill_parameters.as_slice())
                .unwrap_or(&[]);
            mline_fill_ranges(parameters, length)
                .into_iter()
                .map(|(start, end)| (start / length, end / length))
                .collect::<Vec<_>>()
        };
        let low_ranges = fill_ranges(low_index, low_length);
        let high_ranges = fill_ranges(high_index, high_length);
        for low in &low_ranges {
            for high in &high_ranges {
                let start = low.0.max(high.0);
                let end = low.1.min(high.1);
                if end - start <= 1.0e-9 {
                    continue;
                }
                let a = low_start.lerp(low_end, start).to_array();
                let b = high_start.lerp(high_end, start).to_array();
                let c = high_start.lerp(high_end, end).to_array();
                let d = low_start.lerp(low_end, end).to_array();
                triangles.extend([a, b, c, a, c, d]);
            }
        }
    }
    triangles
}

impl RenderConvertible for MLine {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.vertices.is_empty() {
            return None;
        }

        // Picking uses one NaN-separated line list; rendering keeps style colours.
        let lines = mline_lines(self, document);
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                pts.push([f64::NAN; 3]);
            }
            pts.extend_from_slice(&l.points);
        }

        let key_verts: Vec<[f64; 3]> = self
            .vertices
            .iter()
            .map(|v| [v.position.x, v.position.y, v.position.z])
            .collect();

        let snap_pts = self
            .vertices
            .iter()
            .map(|v| {
                (
                    glam::DVec3::new(v.position.x, v.position.y, v.position.z),
                    SnapHint::Node,
                )
            })
            .collect();

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices: key_verts,
            fill_tris: vec![],
        })
    }
}

impl Grippable for MLine {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.position.x, v.position.y, v.position.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let Some(vertex) = self.vertices.get(grip_id) else {
            return;
        };
        let position = match apply {
            GripApply::Translate(delta) => acadrust::types::Vector3::new(
                vertex.position.x + delta.x as f64,
                vertex.position.y + delta.y as f64,
                vertex.position.z + delta.z as f64,
            ),
            GripApply::Absolute(point) => {
                acadrust::types::Vector3::new(point.x as f64, point.y as f64, point.z as f64)
            }
        };
        let count = self.vertices.len();
        let segment_count = if self.is_closed() {
            count
        } else {
            count.saturating_sub(1)
        };
        let mut affected = Vec::new();
        let candidates = if self.is_closed() {
            vec![
                ((grip_id + count - 2) % count, false),
                ((grip_id + count - 1) % count, false),
                (grip_id, true),
                ((grip_id + 1) % count, true),
            ]
        } else {
            let mut candidates = Vec::new();
            if grip_id >= 2 {
                candidates.push((grip_id - 2, false));
            }
            if grip_id >= 1 {
                candidates.push((grip_id - 1, false));
            }
            if grip_id < segment_count {
                candidates.push((grip_id, true));
            }
            if grip_id + 1 < segment_count {
                candidates.push((grip_id + 1, true));
            }
            candidates
        };
        for (segment, moved_start) in candidates {
            if segment >= segment_count
                || affected
                    .iter()
                    .any(|(existing, _, _)| *existing == segment)
            {
                continue;
            }
            if let Some(source) = snapshot_mline_segment(self, segment) {
                affected.push((segment, moved_start, source));
            }
        }
        let offsets = mline_perpendicular_offsets(self);
        self.vertices[grip_id].position = position;
        if rebuild_mline_geometry(self) {
            restore_mline_offsets(self, &offsets);
            for (segment, moved_start, source) in affected {
                restore_shifted_mline_segment(self, segment, source, moved_start);
            }
        }
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                let insert = if grip_id + 1 < n {
                    grip_id + 1
                } else if self.is_closed() {
                    n
                } else {
                    return;
                };
                let next = (grip_id + 1) % n;
                let v0 = &self.vertices[grip_id];
                let v1 = &self.vertices[next];
                let mut new_v = v0.clone();
                new_v.position.x = (v0.position.x + v1.position.x) * 0.5;
                new_v.position.y = (v0.position.y + v1.position.y) * 0.5;
                new_v.position.z = (v0.position.z + v1.position.z) * 0.5;
                let point = cadkernel::space::Vec3::new(
                    new_v.position.x,
                    new_v.position.y,
                    new_v.position.z,
                );
                let source: Option<Vec<_>> = (0..self.style_element_count)
                    .map(|element| {
                        let endpoints = mline_element_endpoints(self, grip_id, element)?;
                        let direction = endpoints.1 - endpoints.0;
                        let length = direction.length();
                        let split = (point - endpoints.0)
                            .dot(direction.normalize()?)
                            .clamp(0.0, length);
                        Some((
                            self.vertices[grip_id].segments.get(element)?.clone(),
                            length,
                            split,
                        ))
                    })
                    .collect();
                let Some(source) = source else {
                    return;
                };
                self.vertices.insert(insert, new_v);
                let offsets = mline_perpendicular_offsets(self);
                if !rebuild_mline_geometry(self) {
                    return;
                }
                restore_mline_offsets(self, &offsets);
                for (element, (source, source_length, split)) in source.into_iter().enumerate() {
                    let Some(first) = mline_element_endpoints(self, grip_id, element) else {
                        continue;
                    };
                    let Some(second) = mline_element_endpoints(self, insert, element) else {
                        continue;
                    };
                    let first_length = first.0.distance(first.1);
                    let second_length = second.0.distance(second.1);
                    let drawn = mline_drawn_ranges(&source.parameters, source_length);
                    let cuts = mline_cut_ranges(&source.area_fill_parameters, source_length);
                    let first_drawn = remap_mline_ranges(&drawn, 0.0, split, first_length);
                    let second_drawn =
                        remap_mline_ranges(&drawn, split, source_length, second_length);
                    let first_cuts = remap_mline_ranges(&cuts, 0.0, split, first_length);
                    let second_cuts =
                        remap_mline_ranges(&cuts, split, source_length, second_length);
                    let first = &mut self.vertices[grip_id].segments[element];
                    store_mline_drawn_ranges(
                        &mut first.parameters,
                        first_length,
                        &first_drawn,
                    );
                    store_mline_cut_ranges(
                        &mut first.area_fill_parameters,
                        first_length,
                        &first_cuts,
                    );
                    let second = &mut self.vertices[insert].segments[element];
                    store_mline_drawn_ranges(
                        &mut second.parameters,
                        second_length,
                        &second_drawn,
                    );
                    store_mline_cut_ranges(
                        &mut second.area_fill_parameters,
                        second_length,
                        &second_cuts,
                    );
                }
            }
            A::RemoveVertex
                if grip_id < n && n > 2 && (!self.is_closed() || n > 3) =>
            {
                let merge = self.is_closed() || (grip_id > 0 && grip_id + 1 < n);
                let previous = if grip_id == 0 { n - 1 } else { grip_id - 1 };
                let source = merge.then(|| {
                    let first = snapshot_mline_segment(self, previous)?;
                    let second = snapshot_mline_segment(self, grip_id)?;
                    Some((first, second))
                });
                let source = match source {
                    Some(Some(source)) => Some(source),
                    Some(None) => return,
                    None => None,
                };
                let mut peripheral = Vec::new();
                let candidates = if self.is_closed() {
                    vec![
                        ((previous + n - 1) % n, false),
                        ((grip_id + 1) % n, true),
                    ]
                } else {
                    let mut candidates = Vec::new();
                    if grip_id >= 2 {
                        candidates.push((grip_id - 2, false));
                    }
                    if grip_id + 2 < n {
                        candidates.push((grip_id + 1, true));
                    }
                    candidates
                };
                for (segment, moved_start) in candidates {
                    if (merge && (segment == previous || segment == grip_id))
                        || peripheral
                            .iter()
                            .any(|(existing, _, _)| *existing == segment)
                    {
                        continue;
                    }
                    if let Some(snapshot) = snapshot_mline_segment(self, segment) {
                        let target = if segment > grip_id {
                            segment - 1
                        } else {
                            segment
                        };
                        peripheral.push((target, moved_start, snapshot));
                    }
                }
                let mut offsets = mline_perpendicular_offsets(self);
                self.vertices.remove(grip_id);
                offsets.remove(grip_id);
                if !rebuild_mline_geometry(self) {
                    return;
                }
                restore_mline_offsets(self, &offsets);
                for (segment, moved_start, snapshot) in peripheral {
                    restore_shifted_mline_segment(self, segment, snapshot, moved_start);
                }
                if let Some((first, second)) = source {
                    let previous = if grip_id == 0 {
                        self.vertices.len() - 1
                    } else {
                        grip_id - 1
                    };
                    for (element, ((first, first_length), (second, second_length))) in
                        first.into_iter().zip(second).enumerate()
                    {
                        let Some(endpoints) = mline_element_endpoints(self, previous, element)
                        else {
                            continue;
                        };
                        let target_length = endpoints.0.distance(endpoints.1);
                        let total = first_length + second_length;
                        let mut drawn = mline_drawn_ranges(&first.parameters, first_length);
                        drawn.extend(
                            mline_drawn_ranges(&second.parameters, second_length)
                                .into_iter()
                                .map(|range| {
                                    (range.0 + first_length, range.1 + first_length)
                                }),
                        );
                        let mut cuts =
                            mline_cut_ranges(&first.area_fill_parameters, first_length);
                        cuts.extend(
                            mline_cut_ranges(&second.area_fill_parameters, second_length)
                                .into_iter()
                                .map(|range| {
                                    (range.0 + first_length, range.1 + first_length)
                                }),
                        );
                        let drawn = merge_mline_ranges(remap_mline_ranges(
                            &drawn,
                            0.0,
                            total,
                            target_length,
                        ));
                        let cuts = merge_mline_ranges(remap_mline_ranges(
                            &cuts,
                            0.0,
                            total,
                            target_length,
                        ));
                        let target = &mut self.vertices[previous].segments[element];
                        store_mline_drawn_ranges(
                            &mut target.parameters,
                            target_length,
                            &drawn,
                        );
                        store_mline_cut_ranges(
                            &mut target.area_fill_parameters,
                            target_length,
                            &cuts,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

impl PropertyEditable for MLine {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let just_str = match self.justification {
            acadrust::entities::MLineJustification::Top => "Top",
            acadrust::entities::MLineJustification::Zero => "Zero",
            acadrust::entities::MLineJustification::Bottom => "Bottom",
        };
        vec![PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                ro_prop(t!("Style").as_ref(), "ml_style", self.style_name.clone()),
                Property {
                    label: t!("Style justification").into_owned(),
                    field: "ml_justification",
                    value: PropValue::Choice {
                        selected: just_str.to_string(),
                        options: ["Top", "Zero", "Bottom"]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    },
                },
                edit(t!("Scale").as_ref(), "ml_scale", self.scale_factor),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ml_closed" => {
                let closed = if value == "toggle" {
                    !self.flags.contains(acadrust::entities::MLineFlags::CLOSED)
                } else {
                    value == "true"
                };
                let offsets = mline_perpendicular_offsets(self);
                self.flags.set(
                    acadrust::entities::MLineFlags::CLOSED,
                    closed && self.vertices.len() >= 3,
                );
                rebuild_mline_geometry(self);
                restore_mline_offsets(self, &offsets);
                return;
            }
            "ml_justification" => {
                self.justification = match value {
                    "Top" => acadrust::entities::MLineJustification::Top,
                    "Bottom" => acadrust::entities::MLineJustification::Bottom,
                    _ => acadrust::entities::MLineJustification::Zero,
                };
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        if field == "ml_scale" {
            set_mline_scale(self, v);
        }
    }
}

fn set_mline_scale(mline: &mut MLine, scale: f64) {
    let old = mline.scale_factor;
    if !scale.is_finite() || scale == 0.0 || !old.is_finite() {
        return;
    }
    if old == 0.0 {
        mline.scale_factor = scale;
        return;
    }
    let ratio = scale / old;
    if !ratio.is_finite() {
        return;
    }
    if mline
        .vertices
        .iter()
        .flat_map(|vertex| &vertex.segments)
        .filter_map(|segment| segment.parameters.first())
        .any(|offset| !offset.is_finite() || !(offset * ratio).is_finite())
    {
        return;
    }
    for segment in mline
        .vertices
        .iter_mut()
        .flat_map(|vertex| vertex.segments.iter_mut())
    {
        if let Some(offset) = segment.parameters.first_mut() {
            *offset *= ratio;
        }
    }
    mline.scale_factor = scale;
}

fn mline_vertex_factor(mline: &MLine, index: usize) -> f64 {
    let vertex = &mline.vertices[index];
    let normal = cadkernel::space::Vec3::new(mline.normal.x, mline.normal.y, mline.normal.z)
        .normalize()
        .unwrap_or(cadkernel::space::Vec3::Z);
    let direction =
        cadkernel::space::Vec3::new(vertex.direction.x, vertex.direction.y, vertex.direction.z)
            .normalize()
            .unwrap_or(cadkernel::space::Vec3::X);
    let miter = cadkernel::space::Vec3::new(vertex.miter.x, vertex.miter.y, vertex.miter.z)
        .normalize()
        .unwrap_or(cadkernel::space::Vec3::Y);
    miter.dot(normal.cross(direction)).abs().max(1.0e-9)
}

fn mline_perpendicular_offsets(mline: &MLine) -> Vec<Vec<Option<f64>>> {
    mline
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let factor = mline_vertex_factor(mline, index);
            vertex
                .segments
                .iter()
                .map(|segment| segment.parameters.first().map(|value| value * factor))
                .collect()
        })
        .collect()
}

fn restore_mline_offsets(mline: &mut MLine, offsets: &[Vec<Option<f64>>]) {
    for index in 0..mline.vertices.len().min(offsets.len()) {
        let factor = mline_vertex_factor(mline, index);
        for (segment, offset) in mline.vertices[index]
            .segments
            .iter_mut()
            .zip(&offsets[index])
        {
            if let Some(offset) = offset {
                if let Some(first) = segment.parameters.first_mut() {
                    *first = *offset / factor;
                }
            }
        }
    }
}

impl Transformable for MLine {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.position.x,
                    &mut v.position.y,
                    p1,
                    p2,
                );
            }
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.start_point.x,
                &mut entity.start_point.y,
                p1,
                p2,
            );
        });
    }
}
