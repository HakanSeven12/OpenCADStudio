use std::collections::HashMap;

use super::{
    arrangement::{bounded_face_edges, TaggedLine},
    curve::Line,
    BulgeArc, Polyline, Tolerance,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandBoundaryEdge {
    pub points: [[f64; 2]; 2],
    pub source_segment: usize,
    pub source_distances: [f64; 2],
    pub segment_distances: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandStationPiece {
    pub points: [[f64; 2]; 2],
    pub source_segment: usize,
    pub source_distances: [f64; 2],
    pub segment_distances: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PolylineBandBoundary {
    pub edges: Vec<BandBoundaryEdge>,
    pub station_pieces: Vec<BandStationPiece>,
    pub source_length: f64,
}

#[derive(Clone, Copy)]
struct SourceEdge {
    points: [[f64; 2]; 2],
    source_segment: usize,
    source_distances: [f64; 2],
    segment_distances: [f64; 2],
}

struct Rail {
    points: Vec<[f64; 2]>,
    tags: Vec<usize>,
    source_segment: usize,
}

/// Returns the topologically cleaned boundary of a variable-width band.
pub fn polyline_band_boundary(
    source: &Polyline,
    widths: &[[f64; 2]],
    max_angle: f64,
) -> PolylineBandBoundary {
    let count = source.vertices.len();
    let segment_count = if source.closed {
        count
    } else {
        count.saturating_sub(1)
    };
    if count < 2
        || widths.len() != segment_count
        || widths
            .iter()
            .flatten()
            .any(|width| !width.is_finite() || *width < 0.0)
        || !widths.iter().flatten().any(|width| *width > 1e-12)
    {
        return PolylineBandBoundary::default();
    }

    let (source_edges, pieces, source_length) = sampled_source(source, max_angle);
    let (mut left, mut right) = rails(source, widths, &source_edges, &pieces);
    if left.is_empty() {
        return PolylineBandBoundary::default();
    }
    join_rails(source, &mut left, widths, segment_count, 1.0);
    join_rails(source, &mut right, widths, segment_count, -1.0);

    let mut raw = rail_chain(&left, source.closed, false);
    raw.extend(rail_chain(&right, source.closed, true));
    if !source.closed {
        if let (Some(left_end), Some(right_end), Some(tag)) = (
            left.last().and_then(|rail| rail.points.last()),
            right.last().and_then(|rail| rail.points.last()),
            left.last().and_then(|rail| rail.tags.last()),
        ) {
            raw.push(tagged(*left_end, *right_end, *tag));
        }
        if let (Some(left_start), Some(right_start), Some(tag)) = (
            left.first().and_then(|rail| rail.points.first()),
            right.first().and_then(|rail| rail.points.first()),
            left.first().and_then(|rail| rail.tags.first()),
        ) {
            raw.push(tagged(*right_start, *left_start, *tag));
        }
    }

    let cleaned = clean_boundary(
        raw,
        source.vertices[0].position,
        widths.iter().flatten().copied().fold(0.0, f64::max),
    );
    let edges = cleaned
        .into_iter()
        .filter_map(|edge| boundary_edge(edge, &source_edges))
        .collect();
    let station_pieces = source_edges
        .into_iter()
        .map(|edge| BandStationPiece {
            points: edge.points,
            source_segment: edge.source_segment,
            source_distances: edge.source_distances,
            segment_distances: edge.segment_distances,
        })
        .collect();
    PolylineBandBoundary {
        edges,
        station_pieces,
        source_length,
    }
}

fn sampled_source(
    source: &Polyline,
    max_angle: f64,
) -> (Vec<SourceEdge>, Vec<Vec<usize>>, f64) {
    let count = source.vertices.len();
    let segment_count = if source.closed {
        count
    } else {
        count.saturating_sub(1)
    };
    let mut edges = Vec::new();
    let mut pieces = vec![Vec::new(); segment_count];
    let mut source_distance = 0.0;
    for index in 0..segment_count {
        let start = source.vertices[index].position;
        let end = source.vertices[(index + 1) % count].position;
        let (samples, length) = match source.segment_arc(index) {
            Some(arc) => (
                arc.tessellate_angle(max_angle),
                arc.radius * arc.sweep.abs(),
            ),
            None => (vec![start, end], (end[0] - start[0]).hypot(end[1] - start[1])),
        };
        let piece_count = samples.len().saturating_sub(1);
        for piece in 0..piece_count {
            let local_start = length * piece as f64 / piece_count as f64;
            let local_end = length * (piece + 1) as f64 / piece_count as f64;
            let tag = edges.len();
            edges.push(SourceEdge {
                points: [samples[piece], samples[piece + 1]],
                source_segment: index,
                source_distances: [
                    source_distance + local_start,
                    source_distance + local_end,
                ],
                segment_distances: [local_start, local_end],
            });
            pieces[index].push(tag);
        }
        source_distance += length;
    }
    (edges, pieces, source_distance)
}

fn rails(
    source: &Polyline,
    widths: &[[f64; 2]],
    source_edges: &[SourceEdge],
    pieces: &[Vec<usize>],
) -> (Vec<Rail>, Vec<Rail>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (index, tags) in pieces.iter().enumerate() {
        let Some(&last_tag) = tags.last() else {
            continue;
        };
        let length = source_edges[last_tag].segment_distances[1];
        let mut left_points = Vec::with_capacity(tags.len() + 1);
        let mut right_points = Vec::with_capacity(tags.len() + 1);
        for (piece, &tag) in tags.iter().enumerate() {
            let edge = source_edges[tag];
            if piece == 0 {
                push_rail_point(
                    source,
                    index,
                    widths[index],
                    length,
                    edge.points[0],
                    edge.segment_distances[0],
                    &mut left_points,
                    &mut right_points,
                );
            }
            push_rail_point(
                source,
                index,
                widths[index],
                length,
                edge.points[1],
                edge.segment_distances[1],
                &mut left_points,
                &mut right_points,
            );
        }
        if left_points.len() >= 2 {
            left.push(Rail {
                points: left_points,
                tags: tags.clone(),
                source_segment: index,
            });
            right.push(Rail {
                points: right_points,
                tags: tags.clone(),
                source_segment: index,
            });
        }
    }
    (left, right)
}

#[allow(clippy::too_many_arguments)]
fn push_rail_point(
    source: &Polyline,
    segment: usize,
    widths: [f64; 2],
    segment_length: f64,
    point: [f64; 2],
    station: f64,
    left: &mut Vec<[f64; 2]>,
    right: &mut Vec<[f64; 2]>,
) {
    let count = source.vertices.len();
    let start = source.vertices[segment].position;
    let end = source.vertices[(segment + 1) % count].position;
    let tangent = segment_tangent(source.segment_arc(segment), start, end, point);
    let length = tangent[0].hypot(tangent[1]);
    if length <= 1e-12 {
        return;
    }
    let t = if segment_length > 1e-12 {
        station / segment_length
    } else {
        0.0
    };
    let half_width = (widths[0] + (widths[1] - widths[0]) * t) * 0.5;
    let normal = [-tangent[1] / length, tangent[0] / length];
    left.push([
        point[0] + normal[0] * half_width,
        point[1] + normal[1] * half_width,
    ]);
    right.push([
        point[0] - normal[0] * half_width,
        point[1] - normal[1] * half_width,
    ]);
}

fn join_rails(
    source: &Polyline,
    rails: &mut [Rail],
    widths: &[[f64; 2]],
    segment_count: usize,
    side: f64,
) {
    let join_count = if source.closed {
        rails.len()
    } else {
        rails.len().saturating_sub(1)
    };
    for index in 0..join_count {
        let next = (index + 1) % rails.len();
        let source_segment = rails[index].source_segment;
        let next_source = rails[next].source_segment;
        if next_source != (source_segment + 1) % segment_count {
            continue;
        }
        let width = widths[source_segment][1];
        let next_width = widths[next_source][0];
        if (width - next_width).abs() > 1e-9 * width.max(next_width).max(1.0) {
            continue;
        }
        let Some(corner) = rail_intersection(source, widths, &rails[index], &rails[next], side)
        else {
            continue;
        };
        if let Some(end) = rails[index].points.last_mut() {
            *end = corner;
        }
        rails[next].points[0] = corner;
    }
}

fn rail_intersection(
    source: &Polyline,
    widths: &[[f64; 2]],
    previous: &Rail,
    next: &Rail,
    side: f64,
) -> Option<[f64; 2]> {
    let d = rail_tangent(source, widths[previous.source_segment], previous.source_segment, true, side)?;
    let e = rail_tangent(source, widths[next.source_segment], next.source_segment, false, side)?;
    let p0 = *previous.points.last()?;
    let q0 = *next.points.first()?;
    let (t, _) = super::intersect::line_line(p0, d, q0, e)?;
    Some([p0[0] + t * d[0], p0[1] + t * d[1]])
}

fn rail_tangent(
    source: &Polyline,
    widths: [f64; 2],
    segment: usize,
    at_end: bool,
    side: f64,
) -> Option<[f64; 2]> {
    let count = source.vertices.len();
    let start = source.vertices[segment].position;
    let end = source.vertices[(segment + 1) % count].position;
    let point = if at_end { end } else { start };
    let arc = source.segment_arc(segment);
    let tangent = segment_tangent(arc, start, end, point);
    let tangent_length = tangent[0].hypot(tangent[1]);
    if tangent_length <= 1e-12 {
        return None;
    }
    let unit = [tangent[0] / tangent_length, tangent[1] / tangent_length];
    let normal = [-unit[1], unit[0]];
    let length = arc.map_or_else(
        || (end[0] - start[0]).hypot(end[1] - start[1]),
        |arc| arc.radius * arc.sweep.abs(),
    );
    if length <= 1e-12 {
        return None;
    }
    let half_width = widths[usize::from(at_end)] * 0.5;
    let gradient = (widths[1] - widths[0]) * 0.5 / length;
    let curvature = arc.map_or(0.0, |arc| arc.sweep.signum() / arc.radius);
    let along = 1.0 - side * half_width * curvature;
    Some([
        along * unit[0] + side * gradient * normal[0],
        along * unit[1] + side * gradient * normal[1],
    ])
}

fn rail_chain(rails: &[Rail], closed: bool, reverse: bool) -> Vec<TaggedLine> {
    let order: Vec<usize> = if reverse {
        (0..rails.len()).rev().collect()
    } else {
        (0..rails.len()).collect()
    };
    let mut edges = Vec::new();
    for (position, &index) in order.iter().enumerate() {
        let rail = &rails[index];
        if reverse {
            for edge in (0..rail.tags.len()).rev() {
                edges.push(tagged(
                    rail.points[edge + 1],
                    rail.points[edge],
                    rail.tags[edge],
                ));
            }
        } else {
            for edge in 0..rail.tags.len() {
                edges.push(tagged(
                    rail.points[edge],
                    rail.points[edge + 1],
                    rail.tags[edge],
                ));
            }
        }
        let has_next = position + 1 < order.len() || closed;
        if has_next {
            let next = &rails[order[(position + 1) % order.len()]];
            let (start, end, tag) = if reverse {
                (
                    rail.points[0],
                    *next.points.last().unwrap(),
                    rail.tags[0],
                )
            } else {
                (
                    *rail.points.last().unwrap(),
                    next.points[0],
                    *rail.tags.last().unwrap(),
                )
            };
            edges.push(tagged(start, end, tag));
        }
    }
    edges
}

fn tagged(start: [f64; 2], end: [f64; 2], tag: usize) -> TaggedLine {
    TaggedLine {
        line: Line { start, end },
        tag,
    }
}

fn clean_boundary(
    edges: Vec<TaggedLine>,
    origin: [f64; 2],
    scale: f64,
) -> Vec<TaggedLine> {
    let to_local = |point: [f64; 2]| {
        [
            (point[0] - origin[0]) / scale,
            (point[1] - origin[1]) / scale,
        ]
    };
    let local: Vec<TaggedLine> = edges
        .into_iter()
        .map(|edge| tagged(to_local(edge.line.start), to_local(edge.line.end), edge.tag))
        .collect();
    let tolerance = Tolerance::new(1e-8);
    let faces = bounded_face_edges(&local, tolerance);
    let mut counts: HashMap<([u64; 2], [u64; 2]), (TaggedLine, usize)> = HashMap::new();
    for edge in faces.into_iter().flatten() {
        let entry = counts.entry(edge_key(edge.line)).or_insert((edge, 0));
        entry.1 += 1;
    }
    let outside: Vec<TaggedLine> = counts
        .into_values()
        .filter_map(|(edge, count)| (count == 1).then_some(edge))
        .collect();
    bounded_face_edges(&outside, tolerance)
        .into_iter()
        .flatten()
        .map(|mut edge| {
            for point in [&mut edge.line.start, &mut edge.line.end] {
                point[0] = origin[0] + point[0] * scale;
                point[1] = origin[1] + point[1] * scale;
            }
            edge
        })
        .collect()
}

fn boundary_edge(edge: TaggedLine, source_edges: &[SourceEdge]) -> Option<BandBoundaryEdge> {
    let source = source_edges.get(edge.tag)?;
    let station = |point| {
        let t = point_segment_parameter(point, source.points);
        (
            lerp(source.source_distances, t),
            lerp(source.segment_distances, t),
        )
    };
    let (source_start, segment_start) = station(edge.line.start);
    let (source_end, segment_end) = station(edge.line.end);
    Some(BandBoundaryEdge {
        points: [edge.line.start, edge.line.end],
        source_segment: source.source_segment,
        source_distances: [source_start, source_end],
        segment_distances: [segment_start, segment_end],
    })
}

fn point_segment_parameter(point: [f64; 2], segment: [[f64; 2]; 2]) -> f64 {
    let along = [
        segment[1][0] - segment[0][0],
        segment[1][1] - segment[0][1],
    ];
    let length_squared = along[0] * along[0] + along[1] * along[1];
    if length_squared <= f64::EPSILON {
        return 0.0;
    }
    (((point[0] - segment[0][0]) * along[0] + (point[1] - segment[0][1]) * along[1])
        / length_squared)
        .clamp(0.0, 1.0)
}

fn lerp(range: [f64; 2], t: f64) -> f64 {
    range[0] + (range[1] - range[0]) * t
}

fn edge_key(line: Line) -> ([u64; 2], [u64; 2]) {
    let key = |point: [f64; 2]| {
        point.map(|value| if value == 0.0 { 0 } else { value.to_bits() })
    };
    let (start, end) = (key(line.start), key(line.end));
    if start < end {
        (start, end)
    } else {
        (end, start)
    }
}

fn segment_tangent(
    arc: Option<BulgeArc>,
    start: [f64; 2],
    end: [f64; 2],
    point: [f64; 2],
) -> [f64; 2] {
    match arc {
        Some(arc) => {
            let sign = arc.sweep.signum();
            [
                -(point[1] - arc.center[1]) * sign,
                (point[0] - arc.center[0]) * sign,
            ]
        }
        None => [end[0] - start[0], end[1] - start[1]],
    }
}
