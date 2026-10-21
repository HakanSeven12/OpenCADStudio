//! REGION boundaries must participate in ordinary F3 snaps without exploding.

#[test]
fn region_snap_insert_keeps_boundary_wires_hidden() {
    use crate::scene::cache::block_cache::{expand_insert, BlockCache};
    use crate::scene::view::render::InheritStyle;
    use codec::entities::Insert;
    use codec::tables::BlockRecord;
    use codec::types::Vector3;
    for boundary in [
        ring(&[[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]]),
        vec![Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 10.0,
        })],
    ] {
        let mut document = CadDocument::new();
        let mut block = BlockRecord::new("REGION_SNAPS");
        block.handle = document.allocate_handle();
        let owner = block.handle;
        document.block_records.add(block).unwrap();
        let circular = matches!(boundary.first(), Some(Curve::Circle(_)));
        let mut loops = vec![boundary];
        if !circular {
            loops.push(ring(&[
                [10.0, 10.0],
                [10.0, 15.0],
                [15.0, 15.0],
                [15.0, 10.0],
            ]));
        }
        let mut region = region_from_loops(&loops, WorkingPlane::default()).unwrap();
        region.common_mut().owner_handle = owner;
        document.add_entity(region).unwrap();
        let mut line =
            codec::entities::Line::from_points(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0));
        line.common.owner_handle = owner;
        document.add_entity(EntityType::Line(line)).unwrap();
        let insert = Insert::new("REGION_SNAPS", Vector3::ZERO);
        let handle = document
            .add_entity(EntityType::Insert(insert.clone()))
            .unwrap();
        let cache = BlockCache::build(
            &document,
            1.0,
            None,
            true,
            [0.0; 4],
            None,
            &Default::default(),
        );
        for selected in [false, true] {
            let expanded = expand_insert(
                &document,
                &cache,
                &insert,
                handle,
                [1.0; 4],
                0,
                0.0,
                [0.0; 8],
                1.0,
                InheritStyle {
                    color: [1.0; 4],
                    pat_len: 0.0,
                    pat: [0.0; 8],
                    lw_px: 1.0,
                },
                0,
                true,
                selected,
                1.0,
                None,
                None,
                false,
                [0.0; 4],
                1.0,
                crate::scene::BlockScalePolicy::FromInsert,
                false,
            )
            .unwrap();
            let boundaries: Vec<_> = expanded.iter().filter(|wire| wire.snap_only).collect();
            assert!(
                !boundaries.is_empty(),
                "INSERT must retain REGION snap-only metadata"
            );
            assert!(boundaries
                .iter()
                .all(|wire| !wire.display_visible && !wire.plot_visible));
            assert!(boundaries.iter().any(|wire| !wire.points.is_empty()));
            let visible: Vec<_> = expanded
                .iter()
                .filter(|wire| wire.display_visible && !wire.points.is_empty())
                .collect();
            assert_eq!(visible.len(), 1, "only the ordinary LINE is drawn");
            assert!(visible[0].plot_visible && !visible[0].snap_only);
            let (kind, point) = if circular {
                (SnapType::Quadrant, DVec3::new(10.0, 0.0, 0.0))
            } else {
                (SnapType::Endpoint, DVec3::new(20.0, 20.0, 0.0))
            };
            let hit = snap(&expanded, kind, point, None).expect("region reference inside INSERT");
            assert!(hit.world.distance(point) < 1e-6);
            assert_eq!(hit.source_handle(), Some(handle));
            assert!(
                snap(
                    &expanded,
                    SnapType::Midpoint,
                    DVec3::new(5.0, 5.0, 0.0),
                    None
                )
                .is_none(),
                "no midpoint can bridge disconnected contours"
            );
        }
    }
}
use codec::{CadDocument, EntityType, Handle};
use glam::{DVec3, Mat4, Vec3};
use iced::{Point, Rectangle};
use kernel::geom2d::{Circle, Curve, Line};

use crate::command::WorkingPlane;
use crate::scene::model::{presspull_model::region_from_loops, wire_model::WireModel};
use crate::scene::pick::interaction_index::{InteractionIndex, WireSource};
use crate::snap::{SnapResult, SnapType, Snapper};

fn ring(points: &[[f64; 2]]) -> Vec<Curve> {
    (0..points.len())
        .map(|i| {
            Curve::Line(Line {
                start: points[i],
                end: points[(i + 1) % points.len()],
            })
        })
        .collect()
}

fn wires(entity: &EntityType) -> Vec<WireModel> {
    super::tessellate::tessellate(
        &CadDocument::new(),
        Handle::new(123),
        entity,
        false,
        [1.0; 4],
        0.0,
        [0.0; 8],
        1.0,
        1.0,
        None,
        None,
        [0.0; 4],
        false,
    )
}

fn snap<W: WireSource + ?Sized>(
    wires: &W,
    kind: SnapType,
    point: DVec3,
    from: Option<DVec3>,
) -> Option<SnapResult> {
    let mut snapper = Snapper::default();
    snapper.enabled.clear();
    snapper.enabled.insert(kind);
    snapper.snap3d_enabled = false;
    snapper.grid_snap_on = false;
    snapper.from_point = from;
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };
    let scale = 0.02;
    snapper.snap(
        point,
        Point::new(
            500.0 + point.x as f32 * 500.0 * scale,
            500.0 - point.y as f32 * 500.0 * scale,
        ),
        wires,
        Mat4::from_scale(Vec3::splat(scale)),
        DVec3::ZERO,
        bounds,
        Vec3::ZERO,
        (Vec3::X, Vec3::Y, Vec3::Z),
        None,
    )
}

fn assert_snap(wires: &[WireModel], kind: SnapType, point: DVec3, from: Option<DVec3>) {
    let hit =
        snap(wires, kind, point, from).unwrap_or_else(|| panic!("missing {kind:?} at {point:?}"));
    assert_eq!(hit.snap_type, kind);
    assert!(
        hit.world.distance(point) < 1e-6,
        "{kind:?}: {:?} != {point:?}",
        hit.world
    );
    assert_eq!(hit.source_handle(), Some(Handle::new(123)));
    let index = InteractionIndex::build(wires);
    let candidates = index.query_xy(
        std::sync::Arc::new(wires.to_vec()),
        [point.x - 3.0, point.y - 3.0, point.x + 3.0, point.y + 3.0],
    );
    let indexed = snap(&candidates, kind, point, from).expect("indexed region reference");
    assert_eq!(indexed.snap_type, kind);
    assert!(indexed.world.distance(point) < 1e-6);
    assert_eq!(indexed.source_handle(), hit.source_handle());
}

#[test]
fn region_snap_outer_and_inner_boundaries_use_ordinary_modes() {
    let loops = vec![
        ring(&[[-20.0, -20.0], [20.0, -20.0], [20.0, 20.0], [-20.0, 20.0]]),
        ring(&[[-10.0, -10.0], [-10.0, 10.0], [10.0, 10.0], [10.0, -10.0]]),
    ];
    let entity = region_from_loops(&loops, WorkingPlane::default()).unwrap();
    let before = format!("{entity:?}");
    let wires = wires(&entity);
    for (kind, point, from) in [
        (SnapType::Endpoint, DVec3::new(20.0, 20.0, 0.0), None),
        (SnapType::Endpoint, DVec3::new(10.0, 10.0, 0.0), None),
        (SnapType::Midpoint, DVec3::new(0.0, 20.0, 0.0), None),
        (SnapType::Midpoint, DVec3::new(0.0, 10.0, 0.0), None),
        (SnapType::Nearest, DVec3::new(7.0, 20.0, 0.0), None),
        (
            SnapType::Perpendicular,
            DVec3::new(7.0, 10.0, 0.0),
            Some(DVec3::new(7.0, 4.0, 0.0)),
        ),
    ] {
        assert_snap(&wires, kind, point, from);
    }
    // Separate loops must not acquire a fictitious connecting segment.
    assert!(snap(&wires, SnapType::Nearest, DVec3::new(15.0, 15.0, 0.0), None).is_none());
    assert_eq!(
        format!("{entity:?}"),
        before,
        "snap conversion cannot explode or edit the entity"
    );
}

#[test]
fn region_snap_circle_retains_analytic_references_without_fake_endpoints() {
    let loops = vec![vec![Curve::Circle(Circle {
        centre: [0.0, 0.0],
        radius: 10.0,
    })]];
    let entity = region_from_loops(&loops, WorkingPlane::default()).unwrap();
    let wires = wires(&entity);
    assert_snap(&wires, SnapType::Center, DVec3::ZERO, None);
    assert_snap(&wires, SnapType::Quadrant, DVec3::new(10.0, 0.0, 0.0), None);
    assert_snap(
        &wires,
        SnapType::Tangent,
        DVec3::new(5.0, 75.0_f64.sqrt(), 0.0),
        Some(DVec3::new(20.0, 0.0, 0.0)),
    );
    assert_snap(
        &wires,
        SnapType::Perpendicular,
        DVec3::new(10.0, 0.0, 0.0),
        Some(DVec3::new(20.0, 0.0, 0.0)),
    );
    assert!(snap(&wires, SnapType::Endpoint, DVec3::new(10.0, 0.0, 0.0), None).is_none());
    assert!(snap(&wires, SnapType::Midpoint, DVec3::new(10.0, 0.0, 0.0), None).is_none());
}

#[test]
fn region_snap_tilted_plane_keeps_world_coordinates() {
    let plane = WorkingPlane::new(
        DVec3::new(0.0, 0.0, 7.0),
        DVec3::X,
        DVec3::new(0.0, 0.8, 0.6),
    );
    let entity = region_from_loops(
        &[ring(&[[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]])],
        plane,
    )
    .unwrap();
    assert_snap(
        &wires(&entity),
        SnapType::Endpoint,
        DVec3::new(20.0, 16.0, 19.0),
        None,
    );
    assert_snap(
        &wires(&entity),
        SnapType::Midpoint,
        DVec3::new(10.0, 16.0, 19.0),
        None,
    );
}

#[test]
fn region_snap_arcs_ellipses_and_splines_reuse_curve_modes() {
    use kernel::geom2d::{Arc, Ellipse, EllipseArc, NurbsCurve};
    let arc = vec![
        Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        }),
        Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        }),
    ];
    let entity = region_from_loops(&[arc], WorkingPlane::default()).unwrap();
    let arc_wires = wires(&entity);
    assert_snap(
        &arc_wires,
        SnapType::Endpoint,
        DVec3::new(10.0, 0.0, 0.0),
        None,
    );
    assert_snap(
        &arc_wires,
        SnapType::Midpoint,
        DVec3::new(0.0, 10.0, 0.0),
        None,
    );
    assert_snap(&arc_wires, SnapType::Center, DVec3::ZERO, None);

    let ellipse = Curve::Ellipse(EllipseArc::full(Ellipse {
        centre: [0.0, 0.0],
        major_radius: 10.0,
        minor_radius: 5.0,
        major_axis: [1.0, 0.0],
    }));
    let entity = region_from_loops(&[vec![ellipse]], WorkingPlane::default()).unwrap();
    let ellipse_wires = wires(&entity);
    assert_snap(&ellipse_wires, SnapType::Center, DVec3::ZERO, None);
    assert_snap(
        &ellipse_wires,
        SnapType::Quadrant,
        DVec3::new(0.0, 5.0, 0.0),
        None,
    );
    assert!(snap(
        &ellipse_wires,
        SnapType::Endpoint,
        DVec3::new(10.0, 0.0, 0.0),
        None
    )
    .is_none());

    let spline = Curve::Nurbs(
        NurbsCurve::new(
            3,
            vec![[0.0, 0.0], [3.0, 5.0], [7.0, 5.0], [10.0, 0.0]],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            None,
        )
        .unwrap(),
    );
    let entity = region_from_loops(
        &[vec![
            spline,
            Curve::Line(Line {
                start: [10.0, 0.0],
                end: [0.0, 0.0],
            }),
        ]],
        WorkingPlane::default(),
    )
    .unwrap();
    let spline_wires = wires(&entity);
    assert_snap(
        &spline_wires,
        SnapType::Endpoint,
        DVec3::new(10.0, 0.0, 0.0),
        None,
    );
    assert_snap(
        &spline_wires,
        SnapType::Midpoint,
        DVec3::new(5.0, 3.75, 0.0),
        None,
    );
}

#[test]
fn region_snap_intersects_other_geometry_and_keeps_shell_and_display() {
    let entity = region_from_loops(
        &[vec![Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 10.0,
        })]],
        WorkingPlane::default(),
    )
    .unwrap();
    let mut region_wires = wires(&entity);
    assert!(
        region_wires[0].points.is_empty(),
        "F4 shell must remain empty"
    );
    assert!(region_wires[1..]
        .iter()
        .all(|w| w.snap_only && !w.display_visible && !w.plot_visible));
    let mut line = codec::entities::Line::new();
    line.start = codec::types::Vector3::new(-20.0, 0.0, 0.0);
    line.end = codec::types::Vector3::new(20.0, 0.0, 0.0);
    let mut crossing = wires(&EntityType::Line(line));
    for wire in &mut crossing {
        wire.name = "456".into();
    }
    region_wires.extend(crossing);
    assert_snap(
        &region_wires,
        SnapType::Intersection,
        DVec3::new(10.0, 0.0, 0.0),
        None,
    );
}

#[test]
fn region_snap_wire_only_fallback_and_sibling_line_behavior() {
    use codec::entities::{Region, Wire};
    use codec::types::Vector3;
    let mut region = Region::new();
    region.wires = vec![Wire::from_points(vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
    ])];
    let region_wires = wires(&EntityType::Region(region));
    assert_snap(
        &region_wires,
        SnapType::Endpoint,
        DVec3::new(10.0, 0.0, 0.0),
        None,
    );
    assert_snap(
        &region_wires,
        SnapType::Midpoint,
        DVec3::new(5.0, 0.0, 0.0),
        None,
    );
    let mut line = codec::entities::Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(10.0, 0.0, 0.0);
    let line_wires = wires(&EntityType::Line(line));
    assert!(line_wires
        .iter()
        .all(|w| !w.snap_only && w.display_visible && w.plot_visible));
    assert_snap(
        &line_wires,
        SnapType::Endpoint,
        DVec3::new(10.0, 0.0, 0.0),
        None,
    );
    assert_snap(
        &line_wires,
        SnapType::Midpoint,
        DVec3::new(5.0, 0.0, 0.0),
        None,
    );
}
