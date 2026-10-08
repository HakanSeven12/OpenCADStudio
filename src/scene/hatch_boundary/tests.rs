use crate::scene::Scene;
use codec::entities::hatch::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge, SplineEdge};
use codec::types::{Vector2, Vector3};
use kernel::geom2d::{refine_spline_boundary, triangulate_rings, Curve, NurbsCurve};
use kernel::tessellation::DEFAULT_ANGLE;

fn narrow(clearance: f64, offset: [f64; 2]) -> NurbsCurve {
    let line = |a: [f64; 2], b: [f64; 2]| {
        [
            a,
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) / 3.0),
            std::array::from_fn(|i| a[i] + 2.0 * (b[i] - a[i]) / 3.0),
            b,
        ]
    };
    let [a, b, c, d, e, f] = [
        [-1.0, -1.0],
        [0.0, -1.0],
        [0.0, 1.0],
        [clearance, 0.999],
        [-0.5, 1.5],
        [-1.0, 1.5],
    ];
    // The returning cubic passes close to the vertical side b--c.
    let spans = [
        line(a, b),
        line(b, c),
        line(c, d),
        [d, [clearance, 1.1], [-0.3, 1.5], e],
        line(e, f),
        line(f, a),
    ];
    let mut controls = vec![a];
    for span in spans {
        controls.extend_from_slice(&span[1..]);
    }
    for p in &mut controls {
        for i in 0..2 {
            p[i] += offset[i];
        }
    }
    let mut knots = vec![0.0; 4];
    for i in 1..6 {
        knots.extend([i as f64; 3]);
    }
    knots.extend([6.0; 4]);
    NurbsCurve::new(3, controls, knots, None).unwrap()
}

fn edge(spline: NurbsCurve) -> BoundaryEdge {
    BoundaryEdge::Spline(SplineEdge {
        degree: spline.degree() as i32,
        rational: false,
        periodic: false,
        knots: spline.knots().to_vec(),
        control_points: spline
            .control_points()
            .iter()
            .map(|p| Vector3::new(p[0], p[1], 0.0))
            .collect(),
        fit_points: Vec::new(),
        start_tangent: Vector2::new(0.0, 0.0),
        end_tangent: Vector2::new(0.0, 0.0),
    })
}

fn rectangle(a: [f64; 2], b: [f64; 2], offset: [f64; 2]) -> BoundaryPath {
    let points = [a, [b[0], a[1]], b, [a[0], b[1]]]
        .map(|p| Vector2::new(p[0] + offset[0], p[1] + offset[1]));
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        points.to_vec(),
        true,
    )));
    path
}

#[test]
fn narrow_fill_preserves_corners_holes_and_shared_origin() {
    let curve = narrow(0.00001, [0.0; 2]);
    let area: f64 = (0..6)
        .map(|i| {
            Curve::Nurbs(curve.trimmed(i as f64 / 6.0, (i + 1) as f64 / 6.0).unwrap())
                .enclosed_area()
        })
        .sum();
    for (offset, shuffled, multiple) in [
        ([0.0; 2], false, false),
        ([1e6, -1e6], false, false),
        ([0.0; 2], true, true),
        ([1e6, -1e6], true, true),
    ] {
        let spline = narrow(0.00001, offset);
        let mut path = BoundaryPath::new();
        path.edges = if shuffled {
            let (a, b) = spline.split_at(0.5).unwrap();
            vec![edge(b), edge(a.reversed())]
        } else {
            vec![edge(spline.clone())]
        };
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.paths.push(path);
        if multiple {
            hatch.paths.extend([
                rectangle([-0.8, -0.8], [-0.2, -0.2], offset),
                rectangle([-0.6, -0.6], [-0.4, -0.4], offset),
                rectangle([1.0, -1.0], [2.0, 2.5], offset),
            ]);
        }
        let model = Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).unwrap();
        if multiple {
            assert_eq!(model.world_origin, [offset[0] + 0.5, offset[1] + 0.75]);
        }
        let project = |p: [f64; 2]| {
            [
                (p[0] - model.world_origin[0]) as f32,
                (p[1] - model.world_origin[1]) as f32,
            ]
        };
        let original = Curve::Nurbs(spline.clone()).tessellate_angle(DEFAULT_ANGLE);
        let original: Vec<_> = original
            .into_iter()
            .map(|p| project(p).map(f64::from))
            .collect();
        assert!(triangulate_rings(&[original]).1.is_empty());
        for i in 0..=6 {
            assert!(model
                .boundary
                .contains(&project(spline.point_at(i as f64 / 6.0))));
        }
        assert_eq!(model.boundary_paths.as_deref().unwrap(), &hatch.paths);
        let (vertices, indices) = model.fill_mesh();
        assert!(!indices.is_empty());
        let mut filled_area = 0.0;
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
                .map(|i| vertices[i as usize].map(f64::from));
            let twice_area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(twice_area > 0.0);
            filled_area += twice_area * 0.5;
            if multiple {
                let p =
                    [0, 1].map(|i| (a[i] + b[i] + c[i]) / 3.0 + model.world_origin[i] - offset[i]);
                let inside = |lo, hi| p.iter().all(|&v| v > lo && v < hi);
                assert!(!inside(-0.8, -0.2) || inside(-0.6, -0.4));
            }
        }
        let expected = area + if multiple { 3.5 - 0.36 + 0.04 } else { 0.0 };
        assert!(
            (filled_area - expected).abs() < 0.001,
            "{filled_area} vs {expected}"
        );
    }
}

#[test]
fn valid_samples_are_unchanged_and_invalid_samples_stay_rejected() {
    for clearance in [0.01, 0.000001, 0.000002, 0.000005] {
        let spline = narrow(clearance, [0.0; 2]);
        let original = Curve::Nurbs(spline.clone()).tessellate_angle(DEFAULT_ANGLE);
        let mut path = BoundaryPath::new();
        path.edges.push(edge(spline));
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.paths.push(path);
        let model = Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).unwrap();
        let expected: Vec<_> = original
            .iter()
            .map(|p| {
                [
                    (p[0] - model.world_origin[0]) as f32,
                    (p[1] - model.world_origin[1]) as f32,
                ]
            })
            .collect();
        assert_eq!(model.boundary.as_ref(), &expected);
        assert_eq!(model.fill_mesh().1.is_empty(), clearance < 0.01);
    }
    let curve = Curve::Nurbs(narrow(0.01, [0.0; 2]));
    assert!(refine_spline_boundary(
        &curve.tessellate_angle(DEFAULT_ANGLE),
        || panic!("valid path resampled"),
        |p| p
    )
    .is_none());
    assert!(refine_spline_boundary(
        &vec![[0.0; 2]; 513],
        || panic!("size cap ignored"),
        |p| p
    )
    .is_none());
}

#[test]
fn recovery_respects_the_total_hatch_vertex_cap() {
    let curve = Curve::Nurbs(narrow(0.00001, [0.0; 2]));
    let original = curve.tessellate_angle(DEFAULT_ANGLE);
    let Curve::Nurbs(spline) = curve.clone() else {
        unreachable!()
    };
    let mut path = BoundaryPath::new();
    path.edges.push(edge(spline));
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    hatch.paths.push(path);
    let count = 16_384 - original.len() - 2;
    let points = (0..count)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / count as f64;
            Vector2::new(1.5 + 0.5 * angle.cos(), 0.75 + 1.75 * angle.sin())
        })
        .collect();
    let mut other = BoundaryPath::new();
    other.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(points, true)));
    hatch.paths.push(other);
    let model = Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).unwrap();
    let project = |p: [f64; 2]| {
        [
            (p[0] - model.world_origin[0]) as f32 as f64,
            (p[1] - model.world_origin[1]) as f32 as f64,
        ]
    };
    assert!(
        refine_spline_boundary(&original, || vec![curve], project)
            .unwrap()
            .len()
            > original.len()
    );
    assert_eq!(model.boundary.len(), 16_384);
    let expected: Vec<_> = original
        .into_iter()
        .map(|p| project(p).map(|v| v as f32))
        .collect();
    assert_eq!(&model.boundary[..expected.len()], expected.as_slice());
}
