//! Region-based HATCH trimming. Pattern strokes are display geometry, not
//! independently trimmable lines: partition the stored fill boundary instead.

use codec::entities::{Hatch, HatchStyleType};
use codec::types::Vector2;
use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{
    bounded_faces, contains, distance_to, ring_nesting_depths, signed_area, triangulate, Curve,
    Line, Tolerance, XLine,
};
use rustc_hash::FxHashMap;

use crate::command::WorkingPlane;
use crate::scene::model::wire_model::WireModel;
use crate::scene::{
    entity_boundary_source_on_plane, exact_hatch_paths, hatch_path_ring, live_segments_for_curves,
    BoundarySource,
};

const TOLERANCE: f64 = 1.0e-6;
type Ring = Vec<[f64; 2]>;
type PointKey = [u64; 2];
type EdgeKey = [PointKey; 2];

pub(super) struct HatchTrim {
    pub kept: Hatch,
    removed: Hatch,
}

fn hatch_plane(hatch: &Hatch) -> WorkingPlane {
    let plane = crate::entities::curve::ocs_plane(hatch.normal, hatch.elevation);
    WorkingPlane::new(
        DVec3::from_array(plane.origin),
        DVec3::from_array(plane.x_axis),
        DVec3::from_array(plane.y_axis),
    )
}

fn ring_lines(ring: &[[f64; 2]]) -> Vec<Line> {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
        .filter_map(|(start, end)| {
            ((end[0] - start[0]).hypot(end[1] - start[1]) > TOLERANCE)
                .then_some(Line { start, end })
        })
        .collect()
}

fn in_ring(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    let curves: Vec<_> = ring_lines(ring).into_iter().map(Curve::Line).collect();
    contains(&curves, point, Tolerance::new(TOLERANCE))
}

/// A genuine interior point, also for concave regions and annular cells.
fn interior(ring: &[[f64; 2]], holes: &[Ring]) -> Option<[f64; 2]> {
    let (points, triangles) = triangulate(ring, holes);
    triangles
        .into_iter()
        .map(|triangle| triangle.map(|index| points[index]))
        .max_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs()))
        .map(|[a, b, c]| [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
}

/// Arrangement faces can be disconnected nested rings. Their immediate
/// children bound the holes of each cell, not additional overlapping fill.
fn parents(rings: &[Ring]) -> Vec<Option<usize>> {
    let areas: Vec<_> = rings.iter().map(|ring| signed_area(ring).abs()).collect();
    rings
        .iter()
        .enumerate()
        .map(|(index, ring)| {
            let seed = interior(ring, &[])?;
            (0..rings.len())
                .filter(|&other| {
                    areas[other] > areas[index] + TOLERANCE * TOLERANCE
                        && in_ring(&rings[other], seed)
                })
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        })
        .collect()
}

fn edge_key(line: Line) -> (EdgeKey, i32) {
    // All arrangement rings share welded graph vertices, so these are exact
    // keys, not a second rounding/welding scheme. Canonicalize signed zero.
    let point_key = |point: [f64; 2]| point.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
    let a = point_key(line.start);
    let b = point_key(line.end);
    if a < b {
        ([a, b], 1)
    } else {
        ([b, a], -1)
    }
}

fn cell_edges(rings: &[Ring], parents: &[Option<usize>], index: usize) -> Vec<(EdgeKey, i32)> {
    let mut edges: Vec<_> = ring_lines(&rings[index])
        .into_iter()
        .map(edge_key)
        .collect();
    for (child, parent) in parents.iter().enumerate() {
        if *parent == Some(index) {
            edges.extend(ring_lines(&rings[child]).into_iter().map(|line| {
                let (key, direction) = edge_key(line);
                (key, -direction)
            }));
        }
    }
    edges
}

fn in_cell(rings: &[Ring], parents: &[Option<usize>], index: usize, point: [f64; 2]) -> bool {
    in_ring(&rings[index], point)
        && !rings
            .iter()
            .enumerate()
            .any(|(child, ring)| parents[child] == Some(index) && in_ring(ring, point))
}

/// Split closed spline seams before exact path reconstruction, which trims
/// open intervals and must not take the complementary reversed interval.
fn open_spline_seams(curves: Vec<Curve>) -> Option<Vec<Curve>> {
    let mut result = Vec::new();
    for curve in curves {
        if let Curve::Nurbs(source) = &curve {
            if source.is_closed() {
                let (start, end) = source.domain();
                let mut breaks = vec![0.0, 0.5, 1.0];
                breaks.extend(
                    source
                        .knots()
                        .iter()
                        .map(|u| (u - start) / (end - start))
                        .filter(|t| *t > 0.0 && *t < 1.0),
                );
                breaks.sort_by(f64::total_cmp);
                breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
                for span in breaks.windows(2) {
                    result.push(Curve::Nurbs(source.trimmed(span[0], span[1])?));
                }
                continue;
            }
        }
        result.push(curve);
    }
    Some(result)
}

fn on_full_round_boundary(curve: &Curve, boundaries: &[Curve]) -> bool {
    let circle = |curve: &Curve| match curve {
        Curve::Circle(circle) => Some((circle.centre, circle.radius, true)),
        Curve::Arc(arc) => Some((
            arc.centre,
            arc.radius,
            arc.sweep() >= std::f64::consts::TAU - 1e-9,
        )),
        _ => None,
    };
    let Some((center, radius, _)) = circle(curve) else {
        return false;
    };
    boundaries.iter().any(|boundary| {
        circle(boundary).is_some_and(|(other, r, full)| {
            full && (center[0] - other[0]).hypot(center[1] - other[1]) <= TOLERANCE
                && (radius - r).abs() <= TOLERANCE
        })
    })
}

fn paths_hatch(
    source: &Hatch,
    rings: &[Ring],
    sources: &FxHashMap<Handle, BoundarySource>,
    seeds: Vec<[f64; 2]>,
) -> Option<Hatch> {
    let depths = ring_nesting_depths(rings);
    let exterior: Vec<_> = depths.iter().map(|depth| depth % 2 == 0).collect();
    let paths = exact_hatch_paths(rings, &exterior, sources, TOLERANCE);
    if paths.len() != rings.len() || paths.is_empty() {
        return None;
    }
    let mut result = source.clone();
    result.paths = paths;
    // The old full outline no longer bounds this hatch. Keeping its reactors
    // would let the next associative refresh silently restore the erased fill.
    result.is_associative = false;
    for path in &mut result.paths {
        path.boundary_handles.clear();
    }
    result.seed_points = seeds
        .into_iter()
        .map(|p| Vector2::new(p[0], p[1]))
        .collect();
    // Retain the island policy unless the new topology needs a policy that
    // can actually express it (e.g. removing a closed circle creates a hole).
    if (source.style == HatchStyleType::Ignore && depths.iter().any(|&depth| depth > 0))
        || (source.style == HatchStyleType::Outer && depths.iter().any(|&depth| depth > 1))
    {
        result.style = HatchStyleType::Normal;
    }
    Some(result)
}

/// Remove the clicked *filled cell*, never a pattern stroke or a whole hatch
/// with no usable cutting boundary. All calculations and stored paths use the
/// hatch OCS; cutters must lie in that same plane.
pub(super) fn trim<'a>(
    hatch: &Hatch,
    cutters: impl Iterator<Item = &'a EntityType>,
    point: DVec3,
    implied_edges: bool,
) -> Option<HatchTrim> {
    let plane = hatch_plane(hatch);
    let point = plane.to_local(point);
    if !point.is_finite() || hatch.is_mpolygon {
        return None;
    }
    let point = [point.x, point.y];
    let paths: Vec<_> = hatch
        .paths
        .iter()
        .filter(|path| path.flags.bits() & 8 == 0)
        .collect();
    let original: Vec<_> = paths
        .iter()
        .map(|path| hatch_path_ring(path))
        .collect::<Option<_>>()?;
    let depths = ring_nesting_depths(&original);
    let active = |index: usize| match hatch.style {
        HatchStyleType::Normal => true,
        HatchStyleType::Outer => depths[index] <= 1,
        HatchStyleType::Ignore => depths[index] == 0,
    };
    let original: Vec<_> = original
        .iter()
        .enumerate()
        .filter(|(i, _)| active(*i))
        .map(|(_, ring)| ring.clone())
        .collect();
    let filled = |point| original.iter().filter(|ring| in_ring(ring, point)).count() % 2 == 1;
    if !filled(point) {
        return None;
    }
    let segments: Vec<_> = original.iter().flat_map(|ring| ring_lines(ring)).collect();
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for line in &segments {
        for p in [line.start, line.end] {
            for axis in 0..2 {
                min[axis] = min[axis].min(p[axis] - TOLERANCE);
                max[axis] = max[axis].max(p[axis] + TOLERANCE);
            }
        }
    }
    let mut curves = Vec::new();
    for (_, path) in paths.iter().enumerate().filter(|(i, _)| active(*i)) {
        for edge in &path.edges {
            let curve = crate::entities::hatch::edge_curve(edge)?;
            let pieces = curve.segments();
            if pieces.is_empty() {
                curves.push(curve);
            } else {
                curves.extend(pieces);
            }
        }
    }
    let mut sources = FxHashMap::default();
    let curves = open_spline_seams(curves)?;
    let original_curves = curves.clone();
    sources.insert(hatch.common.handle, BoundarySource { segments, curves });
    for entity in cutters {
        if entity.common().handle == hatch.common.handle {
            continue;
        }
        let Some(mut source) =
            entity_boundary_source_on_plane(entity, plane, TOLERANCE, Some((min, max)))
        else {
            continue;
        };
        // Differently phased tessellations of a coincident full circle are
        // not cutting edges and must not fabricate thin sliver cells.
        source
            .curves
            .retain(|curve| !on_full_round_boundary(curve, &original_curves));
        if source.curves.is_empty() {
            continue;
        }
        source.curves = open_spline_seams(source.curves)?;
        if implied_edges {
            for curve in &mut source.curves {
                if let Curve::Line(line) = curve {
                    *curve = Curve::XLine(XLine {
                        base: line.start,
                        direction: [line.end[0] - line.start[0], line.end[1] - line.start[1]],
                    });
                }
            }
        }
        source.segments = live_segments_for_curves(&source.curves, Some((min, max)), TOLERANCE);
        source.segments.retain(|line| {
            (0..2).all(|axis| {
                line.start[axis].min(line.end[axis]) <= max[axis]
                    && line.start[axis].max(line.end[axis]) >= min[axis]
            })
        });
        if !source.segments.is_empty() {
            sources.insert(entity.common().handle, source);
        }
    }
    if sources.len() < 2 {
        return None;
    }
    // A boundary pick is ambiguous: it must not arbitrarily erase either side.
    let segments: Vec<_> = sources
        .values()
        .flat_map(|source| source.segments.iter().copied())
        .collect();
    if segments
        .iter()
        .any(|line| distance_to(&Curve::Line(*line), point) <= TOLERANCE)
    {
        return None;
    }
    let rings = bounded_faces(&segments, Tolerance::new(TOLERANCE));
    let parents = parents(&rings);
    let mut cells = Vec::new();
    for (index, ring) in rings.iter().enumerate() {
        let holes: Vec<_> = rings
            .iter()
            .enumerate()
            .filter(|(i, _)| parents[*i] == Some(index))
            .map(|(_, ring)| ring.clone())
            .collect();
        let seed = interior(ring, &holes)?;
        if filled(seed) {
            cells.push((index, seed, cell_edges(&rings, &parents, index)));
        }
    }
    // Refuse rather than silently drop a tiny disconnected component/hole
    // below the arrangement's area floor. Check original occupancy first.
    let original_parents = self::parents(&original);
    for (index, ring) in original.iter().enumerate() {
        let holes: Vec<_> = original
            .iter()
            .enumerate()
            .filter(|(child, _)| original_parents[*child] == Some(index))
            .map(|(_, ring)| ring.clone())
            .collect();
        let probe = interior(ring, &holes)?;
        // A ring-only seed can land in its hole and miss the loss of a tiny
        // annular component. Probe each original cell minus its direct holes,
        // and reject cells too narrow for an unambiguous interior probe.
        if std::iter::once(ring)
            .chain(holes.iter())
            .flat_map(|ring| ring_lines(ring))
            .any(|line| distance_to(&Curve::Line(line), probe) <= TOLERANCE)
        {
            return None;
        }
        let occupied = cells
            .iter()
            .any(|(index, _, _)| in_cell(&rings, &parents, *index, probe));
        if occupied != filled(probe) {
            return None;
        }
    }
    let clicked = cells
        .iter()
        .position(|(index, _, _)| in_cell(&rings, &parents, *index, point))?;
    let clicked_edges: FxHashMap<_, _> = cells[clicked].2.iter().copied().collect();
    if !cells.iter().enumerate().any(|(index, (_, _, edges))| {
        index != clicked && edges.iter().any(|(key, _)| clicked_edges.contains_key(key))
    }) {
        return None;
    }
    // Union surviving cells by cancelling shared half-edges before tracing
    // their outline. This preserves holes/islands without overlapping paths
    // or artificial internal boundaries in the resulting HATCH.
    let mut outline = FxHashMap::<EdgeKey, i32>::default();
    let mut seeds = Vec::new();
    for (index, (_, seed, edges)) in cells.iter().enumerate() {
        if index == clicked {
            continue;
        }
        seeds.push(*seed);
        for &(key, direction) in edges {
            *outline.entry(key).or_default() += direction;
        }
    }
    let segments: Vec<_> = outline
        .into_iter()
        .filter(|(_, count)| *count != 0)
        .map(|(key, _)| Line {
            start: key[0].map(f64::from_bits),
            end: key[1].map(f64::from_bits),
        })
        .collect();
    let kept_rings = bounded_faces(&segments, Tolerance::new(TOLERANCE));
    let kept = paths_hatch(hatch, &kept_rings, &sources, seeds)?;
    let removed_index = cells[clicked].0;
    let mut removed_rings = vec![rings[removed_index].clone()];
    removed_rings.extend(
        rings
            .iter()
            .enumerate()
            .filter(|(i, _)| parents[*i] == Some(removed_index))
            .map(|(_, ring)| ring.clone()),
    );
    let mut removed = paths_hatch(hatch, &removed_rings, &sources, vec![cells[clicked].1])?;
    removed.style = HatchStyleType::Normal;
    // Equal areas do not prove that reconstruction kept the correct side.
    let occupied = |hatch: &Hatch, point| {
        hatch
            .paths
            .iter()
            .filter_map(hatch_path_ring)
            .filter(|ring| in_ring(ring, point))
            .count()
            % 2
            == 1
    };
    if occupied(&kept, point)
        || !occupied(&removed, point)
        || cells
            .iter()
            .enumerate()
            .any(|(index, (_, seed, _))| index != clicked && !occupied(&kept, *seed))
        || crate::entities::hatch::boundary_area(&kept) <= TOLERANCE * TOLERANCE
        || crate::entities::hatch::boundary_area(&removed) <= TOLERANCE * TOLERANCE
    {
        return None;
    }
    Some(HatchTrim { kept, removed })
}

impl HatchTrim {
    pub(super) fn preview_wires(&self) -> Vec<WireModel> {
        let plane = hatch_plane(&self.kept);
        let mut wires = Vec::new();
        for (kind, hatch, color) in [
            ("rm", &self.removed, super::TRIM_REMOVE_GRAY),
            ("keep", &self.kept, WireModel::CYAN),
        ] {
            for (index, path) in hatch.paths.iter().enumerate() {
                let Some(mut ring) = hatch_path_ring(path) else {
                    continue;
                };
                if let Some(first) = ring.first().copied() {
                    ring.push(first);
                }
                let points = ring
                    .into_iter()
                    .map(|p| {
                        plane
                            .to_world(DVec3::new(p[0], p[1], 0.0))
                            .as_vec3()
                            .to_array()
                    })
                    .collect();
                wires.push(WireModel::solid(
                    format!("trim_hatch_{kind}_{index}"),
                    points,
                    color,
                    false,
                ));
            }
        }
        wires
    }
}

#[cfg(test)]
#[path = "trim_hatch_tests.rs"]
mod tests;
