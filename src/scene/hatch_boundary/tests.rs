use crate::scene::model::hatch_model::HatchModel;
use crate::scene::Scene;
use document::entities::hatch::{
    BoundaryEdge, BoundaryPath, BoundaryPathFlags, Hatch, LineEdge, PolylineEdge, SplineEdge,
};
use document::types::{Vector2, Vector3};
use kernel::geom2d::{refine_spline_boundary, Curve, NurbsCurve};
use kernel::tessellation::DEFAULT_ANGLE;

#[test]
fn gradient_stops_keep_entity_transparency() {
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    hatch
        .paths
        .push(rectangle([0.0, 0.0], [10.0, 10.0], [0.0; 2]));
    hatch.gradient_color.enabled = true;
    hatch.gradient_color.name = "LINEAR".into();
    hatch.gradient_color.colors = vec![
        document::entities::hatch::GradientColorEntry {
            value: 0.0,
            color: document::types::Color::Rgb { r: 255, g: 0, b: 0 },
            ..Default::default()
        },
        document::entities::hatch::GradientColorEntry {
            value: 1.0,
            color: document::types::Color::Rgb { r: 0, g: 0, b: 255 },
            ..Default::default()
        },
    ];

    let mut model = Scene::hatch_model_from_dxf(&hatch, [0.2, 0.3, 0.4, 0.35]).unwrap();
    assert!((model.color[3] - 0.35).abs() < f32::EPSILON);
    let crate::scene::model::hatch_model::HatchPattern::Gradient { color2, .. } = &model.pattern
    else {
        panic!("gradient hatch must produce a gradient render pattern");
    };
    assert!((color2[3] - 0.35).abs() < f32::EPSILON);

    model.set_alpha(0.6);
    assert!((model.color[3] - 0.6).abs() < f32::EPSILON);
    let crate::scene::model::hatch_model::HatchPattern::Gradient { color2, .. } = &model.pattern
    else {
        unreachable!();
    };
    assert!((color2[3] - 0.6).abs() < f32::EPSILON);
}

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
        // The plain sampling crosses itself: as one polygon it has no inside.
        assert!(kernel::geom2d::triangulate(&original, &[]).1.is_empty());
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
        // A sample that crosses itself is kept as it is and fills by the
        // even-odd rule, its lobes (kernel #106), like a crossing loop.
        assert!(!model.fill_mesh().1.is_empty(), "clearance {clearance}");
    }
    let curve = Curve::Nurbs(narrow(0.01, [0.0; 2]));
    assert!(refine_spline_boundary(
        &curve.tessellate_angle(DEFAULT_ANGLE),
        || panic!("valid path resampled"),
        |p| p
    )
    .is_none());
    assert!(
        refine_spline_boundary(&vec![[0.0; 2]; 513], || panic!("size cap ignored"), |p| p)
            .is_none()
    );
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

fn filled_area(model: &HatchModel) -> f64 {
    let (vertices, indices) = model.fill_mesh();
    indices
        .chunks_exact(3)
        .map(|triangle| {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
                .map(|i| vertices[i as usize].map(f64::from));
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * 0.5
        })
        .sum()
}

/// Square from line edges whose last edge stops `gap` short of the start.
fn square_with_gap(gap: f64) -> BoundaryPath {
    let points = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
        [0.0, gap],
    ];
    let mut path = BoundaryPath::new();
    for pair in points.windows(2) {
        path.add_edge(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(pair[0][0], pair[0][1]),
            end: Vector2::new(pair[1][0], pair[1][1]),
        }));
    }
    path
}

// Same cases as a test sheet checked in DWG TrueView: AutoCAD skips each loop
// flagged not closed (bit 5), closed or not, and still fills the hatch's other
// loops; a loop with a real gap but no flag fills.
#[test]
fn loops_flagged_not_closed_are_skipped_like_autocad() {
    let flagged = |mut path: BoundaryPath| {
        path.flags = BoundaryPathFlags::from_bits(1 | 32);
        path
    };
    let square = |a: [f64; 2], b: [f64; 2]| rectangle(a, b, [0.0; 2]);
    let area = |paths: Vec<BoundaryPath>| {
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.paths = paths;
        Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).map(|model| filled_area(&model))
    };
    let close_to =
        |area: Option<f64>, expected: f64| area.is_some_and(|area| (area - expected).abs() < 1e-3);

    assert!(area(vec![flagged(square([0.0, 0.0], [10.0, 10.0]))]).is_none());
    assert!(area(vec![flagged(square_with_gap(0.02))]).is_none());
    assert!(close_to(area(vec![square_with_gap(0.02)]), 100.0));
    // Flagged island: no hole.
    assert!(close_to(
        area(vec![
            square([0.0, 0.0], [10.0, 10.0]),
            flagged(square([3.0, 3.0], [7.0, 7.0])),
        ]),
        100.0
    ));
    // Flagged outer loop: only the island fills.
    assert!(close_to(
        area(vec![
            flagged(square([0.0, 0.0], [10.0, 10.0])),
            square([3.0, 3.0], [7.0, 7.0]),
        ]),
        16.0
    ));
    // Side by side: only the unflagged loop fills.
    assert!(close_to(
        area(vec![
            square([0.0, 0.0], [4.5, 10.0]),
            flagged(square([5.5, 0.0], [10.0, 10.0])),
        ]),
        45.0
    ));
}

// AutoCAD leaves these loops out of the drawing extents too: DWG TrueView's
// ZOOM Extents frames a drawing without them.
#[test]
fn loops_flagged_not_closed_stay_out_of_the_extents() {
    let mut scene = Scene::new();
    let mut line = document::entities::Line::new();
    line.end = Vector3::new(10.0, 10.0, 0.0);
    scene.add_entity(document::EntityType::Line(line));
    let mut path = rectangle([100.0, 100.0], [110.0, 110.0], [0.0; 2]);
    path.flags = BoundaryPathFlags::from_bits(1 | 32);
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    hatch.paths.push(path);
    scene.add_entity(document::EntityType::Hatch(Box::new(hatch)));

    let (_, max) = scene.model_space_extents().unwrap();
    assert!(max.x < 10.5 && max.y < 10.5, "{max:?}");
}

// The island style picks loops by their path flags: Outer keeps loops flagged
// external (bit 0) or outermost (bit 4), Ignore only those flagged external,
// Normal those plus any first-level island (a deeper one only when
// flagged). The kept loops fill even-odd. Squares of 40, 24 and 10
// (two loops: 40 and 20), each centred in the one before.
#[test]
fn island_style_keeps_loops_by_their_flags() {
    use document::entities::HatchStyleType::{Ignore, Normal, Outer};
    let square = |half: f64, flags: u32| {
        let mut path = rectangle(
            [20.0 - half, 20.0 - half],
            [20.0 + half, 20.0 + half],
            [0.0; 2],
        );
        path.flags = BoundaryPathFlags::from_bits(flags);
        path
    };
    let two = |f0, f1| vec![square(20.0, f0), square(10.0, f1)];
    let three = |f0, f1, f2| vec![square(20.0, f0), square(12.0, f1), square(5.0, f2)];
    let area = |style, paths: Vec<BoundaryPath>| {
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.style = style;
        hatch.paths = paths;
        Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).map_or(0.0, |model| filled_area(&model))
    };
    let cases = [
        // A letter's counter stored as an external loop is a hole.
        ("ignore 1 1", area(Ignore, two(1, 1)), 1200.0),
        ("ignore 1 0", area(Ignore, two(1, 0)), 1600.0),
        ("ignore 1 1 1", area(Ignore, three(1, 1, 1)), 1124.0),
        ("ignore 1 1 0", area(Ignore, three(1, 1, 0)), 1024.0),
        ("ignore 1 16 0", area(Ignore, three(1, 16, 0)), 1600.0),
        ("ignore 0 1", area(Ignore, two(0, 1)), 400.0),
        ("outer 1 0 0", area(Outer, three(1, 0, 0)), 1600.0),
        ("outer 1 1 1", area(Outer, three(1, 1, 1)), 1124.0),
        ("outer 1 16 0", area(Outer, three(1, 16, 0)), 1024.0),
        ("outer 17 16 0", area(Outer, three(17, 16, 0)), 1024.0),
        ("outer 1 0 1", area(Outer, three(1, 0, 1)), 1500.0),
        ("normal 1 0 0", area(Normal, three(1, 0, 0)), 1024.0),
        ("normal 1 16 0", area(Normal, three(1, 16, 0)), 1024.0),
        ("normal 1 16 4", area(Normal, three(1, 16, 4)), 1124.0),
        ("normal 0 0", area(Normal, two(0, 0)), 1200.0),
    ];
    for (name, area, expected) in cases {
        assert!(
            (area - expected).abs() < 1e-3,
            "{name}: {area}, expected {expected}"
        );
    }
}

// With no loop flagged the way the style needs, nesting depth picks the loops
// as before: Ignore the outermost, Outer the outermost and the next.
#[test]
fn island_style_without_flags_falls_back_to_nesting_depth() {
    use document::entities::HatchStyleType::{Ignore, Outer};
    let square = |half: f64| {
        rectangle(
            [20.0 - half, 20.0 - half],
            [20.0 + half, 20.0 + half],
            [0.0; 2],
        )
    };
    let area = |style, paths: Vec<BoundaryPath>| {
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.style = style;
        hatch.paths = paths;
        Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).map_or(0.0, |model| filled_area(&model))
    };
    let ignore = area(Ignore, vec![square(20.0), square(10.0)]);
    let outer = area(Outer, vec![square(20.0), square(12.0), square(5.0)]);
    assert!((ignore - 1600.0).abs() < 1e-3, "{ignore}");
    assert!((outer - 1024.0).abs() < 1e-3, "{outer}");
}

#[test]
fn a_pattern_hatch_keeps_loops_flagged_not_closed() {
    let flagged = |mut path: BoundaryPath, bits: u32| {
        path.flags = BoundaryPathFlags::from_bits(bits);
        path
    };
    let square = |a: [f64; 2], b: [f64; 2]| rectangle(a, b, [0.0; 2]);
    let area = |solid: bool, style, paths: Vec<BoundaryPath>| {
        let mut hatch = Hatch::new();
        hatch.is_solid = solid;
        hatch.pattern.name = if solid { "SOLID" } else { "ANSI31" }.into();
        hatch.style = style;
        hatch.paths = paths;
        Scene::hatch_model_from_dxf(&hatch, [1.0; 4]).map_or(0.0, |model| filled_area(&model))
    };
    use document::entities::HatchStyleType::{Normal, Outer};
    let outer = || square([0.0, 0.0], [10.0, 10.0]);
    let island = || square([3.0, 3.0], [7.0, 7.0]);
    let cases = [
        (
            "pattern, outer 33",
            area(false, Normal, vec![flagged(outer(), 33)]),
            100.0,
        ),
        (
            "pattern, outer 33 round an island 16",
            area(
                false,
                Outer,
                vec![flagged(island(), 16), flagged(outer(), 33)],
            ),
            84.0,
        ),
        (
            "pattern, outer 33, island 0",
            area(
                false,
                Normal,
                vec![flagged(outer(), 33), flagged(island(), 0)],
            ),
            84.0,
        ),
        (
            "pattern, island 32",
            area(
                false,
                Normal,
                vec![flagged(outer(), 1), flagged(island(), 32)],
            ),
            84.0,
        ),
        // A solid fill still leaves them out.
        (
            "solid, outer 33",
            area(true, Normal, vec![flagged(outer(), 33)]),
            0.0,
        ),
        (
            "solid, outer 33 round an island 16",
            area(
                true,
                Outer,
                vec![flagged(island(), 16), flagged(outer(), 33)],
            ),
            16.0,
        ),
    ];
    for (name, area, expected) in cases {
        assert!(
            (area - expected).abs() < 1e-3,
            "{name}: {area}, expected {expected}"
        );
    }
}
