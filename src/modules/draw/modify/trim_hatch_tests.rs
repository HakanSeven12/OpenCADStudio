use super::*;
use codec::entities::{BoundaryEdge, BoundaryPath, Circle, Line as LineEnt, LineEdge};
use codec::types::{Vector2, Vector3};

fn path(points: &[[f64; 2]]) -> BoundaryPath {
    let mut path = BoundaryPath::new();
    for line in ring_lines(points) {
        path.add_edge(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(line.start[0], line.start[1]),
            end: Vector2::new(line.end[0], line.end[1]),
        }));
    }
    path
}

fn rectangle(min: [f64; 2], max: [f64; 2]) -> BoundaryPath {
    path(&[min, [max[0], min[1]], max, [min[0], max[1]]])
}

fn hatch() -> Hatch {
    let mut h = Hatch::new();
    h.common.handle = Handle::new(1);
    h.paths = vec![rectangle([0.0, 0.0], [20.0, 10.0])];
    h.is_associative = true;
    h.paths[0].boundary_handles = vec![Handle::new(20)];
    h
}

fn line(handle: u64, a: [f64; 2], b: [f64; 2]) -> EntityType {
    let mut line =
        LineEnt::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
    line.common.handle = Handle::new(handle);
    EntityType::Line(line)
}

fn cut(h: &Hatch, cutters: &[EntityType], point: [f64; 2]) -> Option<HatchTrim> {
    trim(
        h,
        cutters.iter(),
        DVec3::new(point[0], point[1], 0.0),
        false,
    )
}

fn area(hatch: &Hatch) -> f64 {
    crate::entities::hatch::boundary_area(hatch)
}

fn filled(hatch: &Hatch, point: [f64; 2]) -> bool {
    hatch
        .paths
        .iter()
        .filter_map(hatch_path_ring)
        .filter(|ring| in_ring(ring, point))
        .count()
        % 2
        == 1
}

#[test]
fn trim_hatch_rectangle_removes_clicked_half_without_an_external_outline() {
    let h = hatch();
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    for (point, retained) in [([15.0, 5.0], [5.0, 5.0]), ([5.0, 5.0], [15.0, 5.0])] {
        let result = cut(&h, &cutters, point).unwrap();
        assert!((area(&result.kept) - 100.0).abs() < 1e-8);
        assert!((area(&result.removed) - 100.0).abs() < 1e-8);
        assert!(filled(&result.kept, retained));
        assert!(!filled(&result.kept, point));
        assert_eq!(result.kept.paths.len(), 1);
        assert_eq!(result.kept.paths[0].edges.len(), 4);
        assert!(!result.kept.is_associative);
        assert!(result
            .kept
            .paths
            .iter()
            .all(|path| path.boundary_handles.is_empty()));
        assert!(result
            .preview_wires()
            .iter()
            .any(|wire| wire.name.starts_with("trim_hatch_keep")));
    }
}

#[test]
fn trim_hatch_heavy_and_3d_polyline_cutters_are_projected_segment_by_segment() {
    let points = vec![Vector3::new(10.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0)];
    let mut heavy = codec::entities::Polyline::from_points(points.clone());
    heavy.common.handle = Handle::new(2);
    let mut spatial = codec::entities::Polyline3D::new();
    spatial.common.handle = Handle::new(3);
    spatial.vertices = [
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(10.0, 10.0, 0.0),
        Vector3::new(10.0, 10.0, 3.0),
        Vector3::new(-5.0, 5.0, 3.0),
        Vector3::new(25.0, 5.0, 3.0),
    ]
    .into_iter()
    .map(codec::entities::Vertex3DPolyline::new)
    .collect();

    for cutter in [EntityType::Polyline(heavy), EntityType::Polyline3D(spatial)] {
        let result = cut(&hatch(), &[cutter], [15.0, 5.0])
            .expect("a coplanar polyline's constituent edge must split the fill");
        assert!((area(&result.kept) - 100.0).abs() < 1e-8);
        assert!(filled(&result.kept, [5.0, 5.0]));
        assert!(!filled(&result.kept, [15.0, 5.0]));
    }
}

#[test]
fn trim_hatch_does_not_read_mesh_vertex_order_as_a_cutting_path() {
    let points = vec![Vector3::new(10.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0)];
    let mut polygon_mesh = codec::entities::Polyline::from_points(points.clone());
    polygon_mesh.common.handle = Handle::new(2);
    polygon_mesh.flags |= codec::entities::PolylineFlags::POLYGON_MESH;
    let mut polyface = codec::entities::Polyline::from_points(points.clone());
    polyface.common.handle = Handle::new(3);
    polyface.flags |= codec::entities::PolylineFlags::POLYFACE_MESH;
    let mut spatial_mesh = codec::entities::Polyline3D::new();
    spatial_mesh.common.handle = Handle::new(4);
    spatial_mesh.flags.is_3d_mesh = true;
    spatial_mesh.vertices = points
        .iter()
        .copied()
        .map(codec::entities::Vertex3DPolyline::new)
        .collect();
    let mut spatial_polyface = codec::entities::Polyline3D::new();
    spatial_polyface.common.handle = Handle::new(5);
    spatial_polyface.flags.is_polyface_mesh = true;
    spatial_polyface.vertices = points
        .into_iter()
        .map(codec::entities::Vertex3DPolyline::new)
        .collect();

    for cutter in [
        EntityType::Polyline(polygon_mesh),
        EntityType::Polyline(polyface),
        EntityType::Polyline3D(spatial_mesh),
        EntityType::Polyline3D(spatial_polyface),
    ] {
        assert!(
            cut(&hatch(), &[cutter], [15.0, 5.0]).is_none(),
            "mesh vertices must not be interpreted as an ordered trim path"
        );
    }
}

#[test]
fn trim_hatch_does_not_use_fit_control_points_as_chordal_cutters() {
    let points = vec![Vector3::new(10.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0)];
    let mut curve_fit = codec::entities::Polyline::from_points(points.clone());
    curve_fit.common.handle = Handle::new(2);
    curve_fit.flags |= codec::entities::PolylineFlags::CURVE_FIT;
    let mut spline_fit = codec::entities::Polyline::from_points(points.clone());
    spline_fit.common.handle = Handle::new(3);
    spline_fit.flags |= codec::entities::PolylineFlags::SPLINE_FIT;
    let mut spatial_spline = codec::entities::Polyline3D::from_points(points.clone());
    spatial_spline.common.handle = Handle::new(4);
    spatial_spline.flags.spline_fit = true;
    let mut smoothed = codec::entities::Polyline3D::from_points(points);
    smoothed.common.handle = Handle::new(5);
    smoothed.smooth_type = codec::entities::polyline3d::SmoothSurfaceType::CubicBSpline;

    for cutter in [
        EntityType::Polyline(curve_fit),
        EntityType::Polyline(spline_fit),
        EntityType::Polyline3D(spatial_spline),
        EntityType::Polyline3D(smoothed),
    ] {
        assert!(
            cut(&hatch(), &[cutter], [15.0, 5.0]).is_none(),
            "fit points must not be treated as a sequence of straight cutter edges"
        );
    }
}

#[test]
fn trim_hatch_no_partition_dangling_off_plane_outside_or_boundary_is_noop() {
    let h = hatch();
    assert!(cut(&h, &[], [15.0, 5.0]).is_none());
    for cutter in [
        line(2, [30.0, -1.0], [30.0, 11.0]),
        line(2, [10.0, 2.0], [10.0, 8.0]),
        line(2, [0.0, 0.0], [20.0, 0.0]),
    ] {
        assert!(cut(&h, &[cutter], [15.0, 5.0]).is_none());
    }
    let mut off_plane = line(2, [10.0, 0.0], [10.0, 10.0]);
    if let EntityType::Line(line) = &mut off_plane {
        line.start.z = 3.0;
        line.end.z = 3.0;
    }
    assert!(cut(&h, &[off_plane], [15.0, 5.0]).is_none());
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    for point in [[25.0, 5.0], [10.0, 5.0], [0.0, 5.0]] {
        assert!(cut(&h, &cutters, point).is_none());
    }
}

#[test]
fn trim_hatch_grid_only_removes_clicked_cell_and_merges_survivors() {
    let cutters = [
        line(2, [10.0, -5.0], [10.0, 15.0]),
        line(3, [-5.0, 5.0], [25.0, 5.0]),
    ];
    let result = cut(&hatch(), &cutters, [15.0, 7.0]).unwrap();
    assert!((area(&result.kept) - 150.0).abs() < 1e-8);
    assert_eq!(result.kept.paths.len(), 1);
    for point in [[5.0, 2.0], [15.0, 2.0], [5.0, 7.0]] {
        assert!(filled(&result.kept, point));
    }
    assert!(!filled(&result.kept, [15.0, 7.0]));
}

#[test]
fn trim_hatch_existing_hole_is_not_refilled() {
    let mut h = hatch();
    h.paths.push(rectangle([2.0, 2.0], [4.0, 4.0]));
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    let result = cut(&h, &cutters, [15.0, 5.0]).unwrap();
    assert!((area(&result.kept) - 96.0).abs() < 1e-8);
    assert!(filled(&result.kept, [5.0, 5.0]));
    assert!(!filled(&result.kept, [3.0, 3.0]));
    assert!(cut(&h, &cutters, [3.0, 3.0]).is_none());
}

#[test]
fn trim_hatch_cutting_through_a_hole_preserves_the_open_notch() {
    let mut h = hatch();
    h.paths.push(rectangle([8.0, 2.0], [12.0, 8.0]));
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    let result = cut(&h, &cutters, [15.0, 5.0]).unwrap();
    assert!((area(&result.kept) - 88.0).abs() < 1e-8);
    assert!(!filled(&result.kept, [9.0, 5.0]));
    assert!(filled(&result.kept, [5.0, 5.0]));
}

#[test]
fn trim_hatch_closed_circle_can_remove_inside_or_outside_with_exact_arcs() {
    let mut circle = Circle::from_coords(10.0, 5.0, 0.0, 2.0);
    circle.common.handle = Handle::new(2);
    let cutters = [EntityType::Circle(circle)];
    let h = hatch();
    let inner_removed = cut(&h, &cutters, [10.0, 5.0]).unwrap();
    assert!((area(&inner_removed.kept) - (200.0 - 4.0 * std::f64::consts::PI)).abs() < 1e-6);
    assert!(!filled(&inner_removed.kept, [10.0, 5.0]));
    assert!(inner_removed
        .kept
        .paths
        .iter()
        .flat_map(|p| &p.edges)
        .any(|edge| matches!(edge, BoundaryEdge::CircularArc(_))));
    let outer_removed = cut(&h, &cutters, [2.0, 5.0]).unwrap();
    assert!((area(&outer_removed.kept) - 4.0 * std::f64::consts::PI).abs() < 1e-6);
    assert!(filled(&outer_removed.kept, [10.0, 5.0]));
    assert!(!filled(&outer_removed.kept, [2.0, 5.0]));
}

#[test]
fn trim_hatch_ignore_islands_remain_filled_until_trim_creates_a_real_hole() {
    let mut h = hatch();
    h.style = HatchStyleType::Ignore;
    h.paths.push(rectangle([8.0, 3.0], [12.0, 7.0]));
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    let result = cut(&h, &cutters, [15.0, 5.0]).unwrap();
    assert!((area(&result.kept) - 100.0).abs() < 1e-8);
    assert_eq!(result.kept.style, HatchStyleType::Ignore);
    assert!(filled(&result.kept, [9.0, 5.0]));
    let mut circle = Circle::from_coords(10.0, 5.0, 0.0, 2.0);
    circle.common.handle = Handle::new(2);
    let result = cut(&h, &[EntityType::Circle(circle)], [10.0, 5.0]).unwrap();
    assert_eq!(result.kept.style, HatchStyleType::Normal);
    assert!((area(&result.kept) - (200.0 - 4.0 * std::f64::consts::PI)).abs() < 1e-6);
}

#[test]
fn trim_hatch_repeated_cuts_keep_a_disconnected_original_component() {
    let mut h = hatch();
    h.paths.push(rectangle([30.0, 0.0], [40.0, 10.0]));
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    // The unpartitioned component must not be erased just because another
    // disconnected component happens to contain a cutting edge.
    assert!(cut(&h, &cutters, [35.0, 5.0]).is_none());
    let result = cut(&h, &cutters, [15.0, 5.0]).unwrap();
    assert!((area(&result.kept) - 200.0).abs() < 1e-8);
    assert!(filled(&result.kept, [35.0, 5.0]));
    assert!(cut(&result.kept, &cutters, [5.0, 5.0]).is_none());
}

#[test]
fn trim_hatch_ocs_plane_elevation_and_large_coordinates_are_preserved() {
    let mut h = hatch();
    h.normal = Vector3::new(0.0, -1.0, 0.0);
    h.elevation = 7.0;
    let plane = hatch_plane(&h);
    let a = plane.to_world(DVec3::new(10.0, 0.0, 0.0));
    let b = plane.to_world(DVec3::new(10.0, 10.0, 0.0));
    let mut cutter = LineEnt::from_points(Vector3::new(a.x, a.y, a.z), Vector3::new(b.x, b.y, b.z));
    cutter.common.handle = Handle::new(2);
    let cutters = [EntityType::Line(cutter)];
    let result = trim(
        &h,
        cutters.iter(),
        plane.to_world(DVec3::new(15.0, 5.0, 0.0)),
        false,
    )
    .unwrap();
    assert!((area(&result.kept) - 100.0).abs() < 1e-8);
    assert_eq!(result.kept.elevation, h.elevation);
    assert_eq!(result.kept.normal, h.normal);

    let mut h = hatch();
    h.paths = vec![rectangle([1.0e7, 1.0e7], [1.0e7 + 20.0, 1.0e7 + 10.0])];
    let cutters = [line(2, [1.0e7 + 10.0, 1.0e7], [1.0e7 + 10.0, 1.0e7 + 10.0])];
    let result = cut(&h, &cutters, [1.0e7 + 15.0, 1.0e7 + 5.0]).unwrap();
    assert!((area(&result.kept) - 100.0).abs() < 1e-6);
}

#[test]
fn trim_hatch_implied_line_ray_and_xline_limits_work() {
    let h = hatch();
    let short = [line(2, [10.0, 2.0], [10.0, 8.0])];
    assert!(cut(&h, &short, [15.0, 5.0]).is_none());
    let result = trim(&h, short.iter(), DVec3::new(15.0, 5.0, 0.0), true).unwrap();
    assert!((area(&result.kept) - 100.0).abs() < 1e-8);
    let mut ray = codec::entities::Ray::new(Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
    ray.common.handle = Handle::new(2);
    ray.base_point = Vector3::new(10.0, -5.0, 0.0);
    ray.direction = Vector3::new(0.0, 1.0, 0.0);
    let mut xline = codec::entities::XLine::new(Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
    xline.common.handle = Handle::new(2);
    xline.base_point = Vector3::new(10.0, 5.0, 0.0);
    xline.direction = Vector3::new(0.0, 1.0, 0.0);
    for cutter in [EntityType::Ray(ray), EntityType::XLine(xline)] {
        let result = cut(&h, &[cutter], [15.0, 5.0]).unwrap();
        assert!((area(&result.kept) - 100.0).abs() < 1e-8);
    }
}

#[test]
fn trim_hatch_closed_spline_crossing_parameter_seam_keeps_the_correct_side() {
    let mut h = hatch();
    let mut boundary = BoundaryPath::new();
    boundary.add_edge(BoundaryEdge::Spline(codec::entities::SplineEdge {
        degree: 1,
        rational: false,
        periodic: true,
        knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        control_points: [
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ]
        .map(|p| Vector3::new(p[0], p[1], 1.0))
        .to_vec(),
        fit_points: Vec::new(),
        start_tangent: Vector2::new(0.0, 0.0),
        end_tangent: Vector2::new(0.0, 0.0),
    }));
    h.paths = vec![boundary];
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    let result = cut(&h, &cutters, [15.0, 5.0]).unwrap();
    assert!(
        (area(&result.kept) - 100.0).abs() < 1e-7,
        "kept area {}: {:?}",
        area(&result.kept),
        result.kept.paths
    );
    assert!(filled(&result.kept, [5.0, 5.0]));
    assert!(!filled(&result.kept, [15.0, 5.0]));
}

#[test]
fn trim_hatch_coincident_circle_with_different_sampling_phase_is_noop() {
    let mut h = hatch();
    let mut boundary = BoundaryPath::new();
    let start = std::f64::consts::PI / 48.0;
    boundary.add_edge(BoundaryEdge::CircularArc(
        codec::entities::CircularArcEdge {
            center: Vector2::new(0.0, 0.0),
            radius: 10.0,
            start_angle: start,
            end_angle: start + std::f64::consts::TAU,
            counter_clockwise: true,
        },
    ));
    h.paths = vec![boundary];
    let mut circle = Circle::from_coords(0.0, 0.0, 0.0, 10.0);
    circle.common.handle = Handle::new(2);
    assert!(cut(&h, &[EntityType::Circle(circle)], [0.0, 0.0]).is_none());
}

#[test]
fn trim_hatch_tiny_unrelated_components_and_holes_are_not_silently_lost() {
    for base in [[30.0, 0.0], [3.0, 3.0]] {
        let mut h = hatch();
        h.paths
            .push(rectangle(base, [base[0] + 0.000005, base[1] + 0.000005]));
        let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
        // Refuse the operation if the arrangement cannot represent this tiny
        // component, rather than changing unrelated original fill topology.
        assert!(cut(&h, &cutters, [15.0, 5.0]).is_none());
    }
}

#[test]
fn trim_hatch_tiny_nested_fill_band_cannot_hide_behind_a_hole_probe() {
    let mut h = hatch();
    h.paths.push(rectangle([30.0, 0.0], [30.000009, 0.000009]));
    h.paths
        .push(rectangle([30.0000021, 0.0000021], [30.0000069, 0.0000069]));
    let cutters = [line(2, [10.0, 0.0], [10.0, 10.0])];
    assert!(cut(&h, &cutters, [15.0, 5.0]).is_none());
}

#[test]
fn trim_hatch_straight_cutters_use_the_actual_tilted_plane_not_their_support_plane() {
    let mut h = hatch();
    h.normal = Vector3::new(0.0, -1.0, 0.0);
    h.elevation = 7.0;
    let plane = hatch_plane(&h);
    let a = plane.to_world(DVec3::new(0.0, 5.0, 0.0));
    let b = plane.to_world(DVec3::new(20.0, 5.0, 0.0));
    let mut cutter = LineEnt::from_points(Vector3::new(a.x, a.y, a.z), Vector3::new(b.x, b.y, b.z));
    cutter.common.handle = Handle::new(2);
    let cutters = [EntityType::Line(cutter)];
    let result = trim(
        &h,
        cutters.iter(),
        plane.to_world(DVec3::new(15.0, 7.0, 0.0)),
        false,
    )
    .unwrap();
    assert!((area(&result.kept) - 100.0).abs() < 1e-8);
    assert!(filled(&result.kept, [15.0, 2.0]));
    assert!(!filled(&result.kept, [15.0, 7.0]));
}
