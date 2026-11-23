use super::*;
use std::f64::consts::PI;

fn rect_hatch(handle: u64, associative: bool) -> document::entities::Hatch {
    use document::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
    use document::types::Vector2;
    let mut hatch = document::entities::Hatch::new();
    hatch.common.handle = Handle::new(handle);
    hatch.is_associative = associative;
    hatch.is_solid = true;
    let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let mut boundary = BoundaryPath::new();
    boundary.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        corners
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    if associative {
        boundary.boundary_handles.push(Handle::new(50));
        boundary.flags.set_external(true);
    }
    hatch.paths.push(boundary);
    hatch
}

fn add_rect_hole(
    hatch: &mut document::entities::Hatch,
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
) {
    use document::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
    use document::types::Vector2;
    let points = [[xmin, ymin], [xmax, ymin], [xmax, ymax], [xmin, ymax]];
    let mut hole = BoundaryPath::new();
    hole.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        points
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(hole);
}

#[test]
fn trim_hover_spatial_index_is_lazily_cached_and_invalidated_on_rebuild() {
    let target = xline_at(0.0, 0.0, 10.0, 0.0, 1);
    let cutter = xline_at(5.0, -5.0, 5.0, 5.0, 2);
    let mut command = TrimCommand::new(vec![target, cutter]);
    assert!(command.geo_spatial.borrow().is_none());
    let nearby = command.nearby_geos(Handle::new(1));
    assert!(!nearby.is_empty());
    assert!(command.geo_spatial.borrow().is_some());
    command.rebuild_geos();
    assert!(command.geo_spatial.borrow().is_none());
}

#[test]
fn spatial_cut_index_retains_remote_unbounded_edges_but_omits_distant_lines() {
    let mut geos = vec![Geo::Line {
        handle: Handle::new(11),
        p1: [5.0, -1.0],
        p2: [5.0, 1.0],
    }];
    for index in 0..2_000u64 {
        let x = 100.0 + index as f64;
        geos.push(Geo::Line {
            handle: Handle::new(index + 100),
            p1: [x, 10.0],
            p2: [x, 20.0],
        });
    }
    geos.push(Geo::InfLine {
        handle: Handle::new(20_000),
        bx: 1000.0,
        by: 1000.0,
        dx: 0.0,
        dy: 1.0,
    });
    let index = TrimAabbIndex::new(geos.iter().map(trim_geo_aabb));
    let nearby = index.overlapping([0.0, -2.0, 10.0, 2.0]);
    assert_eq!(nearby, vec![0, geos.len() - 1]);
    let retained: Vec<_> = nearby.into_iter().map(|slot| geos[slot].clone()).collect();
    assert_eq!(
        line_seg_ts(0.0, 0.0, 10.0, 0.0, Handle::new(1), &retained),
        line_seg_ts(0.0, 0.0, 10.0, 0.0, Handle::new(1), &geos),
    );
}

#[test]
fn indexed_lowpoly_cut_matches_crossing_positions_with_thousands_of_edges() {
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(1);
    poly.vertices = (0..=200)
        .map(|x| LwVertex::from_coords(x as f64, 0.0))
        .collect();
    let mut geos = vec![
        Geo::Line {
            handle: Handle::new(2),
            p1: [35.5, -3.0],
            p2: [35.5, 3.0],
        },
        Geo::Line {
            handle: Handle::new(3),
            p1: [155.5, -3.0],
            p2: [155.5, 3.0],
        },
    ];
    for index in 0..2_000u64 {
        let x = 400.0 + index as f64;
        geos.push(Geo::Line {
            handle: Handle::new(index + 100),
            p1: [x, -1.0],
            p2: [x, 1.0],
        });
    }
    let cuts = lwpoly_cut_params(&poly, &geos);
    assert_eq!(cuts.len(), 2);
    assert!((cuts[0] - 35.5).abs() < 1e-7);
    assert!((cuts[1] - 155.5).abs() < 1e-7);
}

#[test]
fn indexed_self_intersection_preserves_nonadjacent_bowtie_crossings() {
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(1);
    poly.vertices = [[0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]]
        .into_iter()
        .map(|xy| LwVertex::from_coords(xy[0], xy[1]))
        .collect();
    let geos = vec![Geo::Line {
        handle: poly.common.handle,
        p1: [0.0, 0.0],
        p2: [2.0, 2.0],
    }];
    assert_eq!(lwpoly_self_cut_params(&poly, &geos), vec![0.5, 2.5]);
}

#[test]
fn broadphase_trim_keeps_real_crossings_and_discards_distant_lines() {
    let target = Curve::Line(KernelLine {
        start: [0.0, 0.0],
        end: [10.0, 0.0],
    });
    let mut geos = vec![Geo::Line {
        handle: Handle::new(2),
        p1: [5.0, -3.0],
        p2: [5.0, 3.0],
    }];
    for n in 0..1000 {
        let x = 100.0 + n as f64;
        geos.push(Geo::Line {
            handle: Handle::new(n + 3),
            p1: [x, -1.0],
            p2: [x, 1.0],
        });
    }
    let cuts = cut_params(&target, Handle::new(1), &geos);
    assert_eq!(cuts.len(), 1);
    assert!((cuts[0] - 0.5).abs() < 1e-8);
    assert_eq!(
        trim_target_bounds(&Curve::Ray(KernelRay {
            origin: [0.0, 0.0],
            direction: [1.0, 0.0],
        })),
        None
    );
}

#[test]
fn hover_cut_cache_reuses_cuts_and_clears_on_boundary_rebuild() {
    let all = vec![
        xline_at(0.0, 5.0, 10.0, 5.0, 1),
        xline_at(5.0, -5.0, 5.0, 15.0, 2),
    ];
    let mut cmd = TrimCommand::new(all);
    let target = Handle::new(1);
    let first = cmd.cached_hover_cut_params(target).unwrap();
    assert_eq!(first.len(), 1);
    assert!(cmd.hover_cut_cache.contains_key(&target));
    assert_eq!(first, cmd.cached_hover_cut_params(target).unwrap());

    cmd.all_entities[1] = xline_at(50.0, -5.0, 50.0, 15.0, 2);
    cmd.rebuild_geos();
    assert!(cmd.hover_cut_cache.is_empty());
    assert!(cmd.cached_hover_cut_params(target).unwrap().is_empty());
}

#[test]
fn coincident_line_boundaries_produce_one_cut_even_with_reversed_duplicates() {
    let all = vec![
        xline_at(0.0, 5.0, 10.0, 5.0, 1),
        xline_at(5.0, -5.0, 5.0, 15.0, 2),
        xline_at(5.0, -5.0, 5.0, 15.0, 3),
        xline_at(5.0, 15.0, 5.0, -5.0, 4),
    ];
    let geos = build_geos(&all);
    let cuts = line_seg_ts(0.0, 5.0, 10.0, 5.0, Handle::new(1), &geos);
    assert_eq!(cuts.len(), 1, "duplicate cutters should create one cut");
    assert!((cuts[0] - 0.5).abs() < 1e-8);
}

#[test]
fn fence_candidates_skip_far_entities_without_losing_long_crossing_lines() {
    let all = vec![
        xline_at(-100.0, 5.0, 100.0, 5.0, 1),
        xline_at(3.0, 3.0, 8.0, 8.0, 2),
        xline_at(500.0, 500.0, 600.0, 600.0, 3),
    ];
    let index = ModifyEntityIndex::build(&all);
    let window = CrossingWindow {
        min: [0.0, 0.0],
        max: [10.0, 10.0],
    };
    let hits = trim_fence_candidates(&index, &[], Some(window)).unwrap();
    assert!(hits.contains(&Handle::new(1)));
    assert!(hits.contains(&Handle::new(2)));
    assert!(!hits.contains(&Handle::new(3)));
}

#[test]
fn crowded_crossing_preview_is_skipped_but_does_not_affect_final_cut() {
    let mut all: Vec<_> = (0..70)
        .map(|i| {
            xline_at(
                -2.0,
                1.0 + i as f64 * 0.1,
                12.0,
                1.0 + i as f64 * 0.1,
                i + 1,
            )
        })
        .collect();
    all.push(xline_at(5.0, -2.0, 5.0, 12.0, 100));
    let index = ModifyEntityIndex::build(&all);
    let geos = build_geos(&all);
    let window = CrossingWindow {
        min: [1.0, 1.0],
        max: [9.0, 9.0],
    };
    let fence = [[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0], [1.0, 1.0]];
    assert!(
        fence_result_preview(&all, &index, &geos, &fence, Some(window), false, false).is_empty()
    );
    let replacements = fence_pass(&all, &index, &geos, &fence, Some(window), false);
    assert!(
        !replacements.is_empty(),
        "actual TRIM must not inherit the preview cap"
    );
}

#[test]
fn trim_hatch_removes_clicked_side_and_detaches_association() {
    let hatch = rect_hatch(1, true);
    let boundary = xline_at(5.0, -2.0, 5.0, 12.0, 50);
    let all = vec![EntityType::Hatch(Box::new(hatch.clone())), boundary];
    let mut cmd = TrimCommand::new(all);
    assert!(cmd.entity_pick_includes_fills());
    assert!(!cmd
        .on_hover_entity(Handle::new(1), DVec3::new(2.0, 5.0, 0.0))
        .is_empty());
    let CmdResult::ReplaceEntity(handle, edited) =
        cmd.on_entity_pick(Handle::new(1), DVec3::new(2.0, 5.0, 0.0))
    else {
        panic!("TRIM should clip a picked hatch");
    };
    assert_eq!(handle, Handle::new(1));
    let EntityType::Hatch(trimmed) = &edited[0] else {
        panic!("HATCH expected");
    };
    assert!(!trimmed.is_associative);
    assert!(trimmed.paths.iter().all(|p| p.boundary_handles.is_empty()));
    assert_eq!(trimmed.common.handle, Handle::new(1));
    assert_eq!(trimmed.paths.len(), 1);
    let ring = crate::scene::hatch_path_ring(&trimmed.paths[0]).unwrap();
    assert!(ring.iter().all(|p| p[0] >= 5.0 - 1e-8));
    assert!((crate::entities::hatch::boundary_area(trimmed) - 50.0).abs() < 1e-7);
    assert!(hatch.is_associative, "input is unchanged");
    assert!(!hatch.paths[0].boundary_handles.is_empty());
}

#[test]
fn trim_hatch_preserves_untouched_hole_and_refuses_hole_crossing() {
    let mut hatch = rect_hatch(1, false);
    add_rect_hole(&mut hatch, 6.0, 3.0, 8.0, 7.0);
    let cutter = [Geo::Line {
        handle: Handle::new(50),
        p1: [5.0, -2.0],
        p2: [5.0, 12.0],
    }];
    let kept = trim_hatch_at(&hatch, &cutter, 2.0, 5.0).expect("hole retained");
    assert_eq!(kept.paths.len(), 2);
    assert!((crate::entities::hatch::boundary_area(&kept) - 42.0).abs() < 1e-7);
    let mut crossing = rect_hatch(1, false);
    add_rect_hole(&mut crossing, 4.0, 3.0, 6.0, 7.0);
    assert!(trim_hatch_at(&crossing, &cutter, 2.0, 5.0).is_none());
}

#[test]
fn trim_circle_inside_hatch_creates_exact_round_hole() {
    use std::f64::consts::PI;
    let hatch = rect_hatch(1, true);
    let cutter = [Geo::Circle {
        handle: Handle::new(70),
        cx: 5.0,
        cy: 5.0,
        r: 3.0,
    }];
    let result =
        trim_hatch_at(&hatch, &cutter, 5.0, 5.0).expect("circle inside hatch must make a hole");
    assert_eq!(result.paths.len(), 2);
    assert!(!result.is_associative);
    assert!(result
        .paths
        .iter()
        .all(|path| path.boundary_handles.is_empty()));
    assert!(matches!(result.paths[1].edges.first(),
            Some(document::entities::BoundaryEdge::CircularArc(arc))
                if arc.counter_clockwise && (arc.end_angle - arc.start_angle - std::f64::consts::TAU).abs() < 1e-9));
    assert!((crate::entities::hatch::boundary_area(&result) - (100.0 - PI * 9.0)).abs() < 1e-5);
    let render = crate::scene::Scene::hatch_model_from_dxf(&result, [1.0; 4])
        .expect("round hole must render as a HATCH");
    assert!(!render.fill_mesh().1.is_empty());
}

#[test]
fn trim_circle_intersecting_side_creates_exact_arc_notch() {
    let hatch = rect_hatch(1, false);
    let cutter = [Geo::Circle {
        handle: Handle::new(70),
        cx: 10.5,
        cy: 5.0,
        r: 3.0,
    }];
    let result = trim_hatch_at(&hatch, &cutter, 9.0, 5.0)
        .expect("circle crossing one side must cut a curved notch");
    assert_eq!(result.paths.len(), 1);
    let arcs: Vec<_> = result.paths[0]
        .edges
        .iter()
        .filter_map(|edge| {
            let document::entities::BoundaryEdge::CircularArc(arc) = edge else {
                return None;
            };
            Some(arc)
        })
        .collect();
    assert_eq!(arcs.len(), 1);
    assert!((arcs[0].center.x - 10.5).abs() < 1e-9);
    assert!((arcs[0].radius - 3.0).abs() < 1e-9);
    let area = crate::entities::hatch::boundary_area(&result);
    assert!(area > 0.0 && area < 100.0, "clipped area={area}");
    let render = crate::scene::Scene::hatch_model_from_dxf(&result, [1.0; 4])
        .expect("mixed lines and arc must render as a HATCH");
    assert!(!render.fill_mesh().1.is_empty());
}

#[test]
fn trim_command_click_and_hover_accepts_circle_as_cutting_boundary() {
    let mut circle = CircleEnt::from_coords(5.0, 5.0, 0.0, 3.0);
    circle.common.handle = Handle::new(70);
    let all = vec![
        EntityType::Hatch(Box::new(rect_hatch(1, true))),
        EntityType::Circle(circle),
    ];
    let mut cmd = TrimCommand::with_cutting_edges(all, vec![Handle::new(70)]);
    assert!(cmd.entity_pick_includes_fills());
    let preview = cmd.on_hover_entity(Handle::new(1), DVec3::new(5.0, 5.0, 0.0));
    assert!(
        !preview.is_empty(),
        "circle cutter must produce hatch hover preview"
    );
    let CmdResult::ReplaceEntity(handle, results) =
        cmd.on_entity_pick(Handle::new(1), DVec3::new(5.0, 5.0, 0.0))
    else {
        panic!("circle cutting edge must trim the picked hatch");
    };
    assert_eq!(handle, Handle::new(1));
    let EntityType::Hatch(hatch) = &results[0] else {
        panic!("hatch expected");
    };
    assert_eq!(hatch.paths.len(), 2);
}

#[test]
fn line_trim_preserves_exact_round_hole_after_circle_trim() {
    use std::f64::consts::PI;
    let source = rect_hatch(1, true);
    let circle = [Geo::Circle {
        handle: Handle::new(70),
        cx: 7.0,
        cy: 7.0,
        r: 1.0,
    }];
    let cut_circle =
        trim_hatch_at(&source, &circle, 7.0, 7.0).expect("first trim must make a round hole");
    assert_eq!(cut_circle.paths.len(), 2);
    let line = [Geo::Line {
        handle: Handle::new(71),
        p1: [5.0, -1.0],
        p2: [5.0, 11.0],
    }];
    let cut_line = trim_hatch_at(&cut_circle, &line, 2.0, 5.0)
        .expect("next trim by line must retain round hole");
    assert_eq!(cut_line.paths.len(), 2);
    assert!(matches!(
        cut_line.paths[1].edges.first(),
        Some(document::entities::BoundaryEdge::CircularArc(_))
    ));
    assert!((crate::entities::hatch::boundary_area(&cut_line) - (50.0 - PI)).abs() < 1.0e-5);
}

#[test]
fn line_trim_after_circular_notch_keeps_exact_analytic_arc() {
    let source = rect_hatch(1, false);
    let circle = [Geo::Circle {
        handle: Handle::new(70),
        cx: 10.5,
        cy: 5.0,
        r: 3.0,
    }];
    let notch =
        trim_hatch_at(&source, &circle, 9.0, 5.0).expect("first trim must create arc notch");
    let original_area = crate::entities::hatch::boundary_area(&notch);
    let line = [Geo::Line {
        handle: Handle::new(71),
        p1: [3.0, -1.0],
        p2: [3.0, 11.0],
    }];
    let trimmed = trim_hatch_at(&notch, &line, 1.0, 5.0)
        .expect("subsequent line must trim the already notched hatch");
    assert_eq!(trimmed.paths.len(), 1);
    assert!(trimmed.paths[0].edges.iter().any(|edge| matches!(
        edge, document::entities::BoundaryEdge::CircularArc(arc)
            if (arc.center.x - 10.5).abs() < 1.0e-9
                && (arc.radius - 3.0).abs() < 1.0e-9
    )));
    let area = crate::entities::hatch::boundary_area(&trimmed);
    assert!(area > 0.0 && area < original_area);
    assert!(crate::scene::Scene::hatch_model_from_dxf(&trimmed, [1.0; 4]).is_some());
}

#[test]
fn trim_click_erases_standalone_circular_hatch_not_plain_rectangle() {
    use document::entities::{BoundaryEdge, BoundaryPath, CircularArcEdge};
    use document::types::Vector2;
    let mut round = document::entities::Hatch::new();
    round.common.handle = Handle::new(90);
    round.is_solid = true;
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
        center: Vector2::new(5.0, 5.0),
        radius: 3.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::TAU,
        counter_clockwise: true,
    }));
    round.paths.push(path);
    let all = vec![
        EntityType::Hatch(Box::new(round)),
        EntityType::Hatch(Box::new(rect_hatch(91, false))),
    ];
    let mut cmd = TrimCommand::new(all);
    cmd.mode = TrimMode::Pick;
    assert!(!cmd
        .on_hover_entity(Handle::new(90), DVec3::new(5.0, 5.0, 0.0))
        .is_empty());
    assert!(matches!(
        cmd.on_entity_pick(Handle::new(90), DVec3::new(5.0, 5.0, 0.0)),
        CmdResult::ReplaceEntity(handle, pieces)
            if handle == Handle::new(90) && pieces.is_empty()
    ));
    assert!(
        matches!(
            cmd.on_entity_pick(Handle::new(91), DVec3::new(1.0, 1.0, 0.0)),
            CmdResult::NeedPoint
        ),
        "ordinary polygon hatch without cutting boundary must survive"
    );
}

#[test]
fn trim_circle_never_cuts_holes_or_unrelated_regions() {
    let mut hatch = rect_hatch(1, false);
    add_rect_hole(&mut hatch, 4.0, 4.0, 6.0, 6.0);
    let cutter = [Geo::Circle {
        handle: Handle::new(70),
        cx: 5.0,
        cy: 5.0,
        r: 3.0,
    }];
    assert!(trim_hatch_at(&hatch, &cutter, 5.0, 5.0).is_none());
    let clean = rect_hatch(1, false);
    assert!(trim_hatch_at(&clean, &cutter, 1.0, 1.0).is_none());
    let outside = [Geo::Circle {
        handle: Handle::new(70),
        cx: 20.0,
        cy: 20.0,
        r: 3.0,
    }];
    assert!(trim_hatch_at(&clean, &outside, 5.0, 5.0).is_none());
}

#[test]
fn trim_hatch_rejects_cutter_that_does_not_span_boundary() {
    let hatch = rect_hatch(1, false);
    let too_short = [Geo::Line {
        handle: Handle::new(50),
        p1: [5.0, 3.0],
        p2: [5.0, 7.0],
    }];
    assert!(trim_hatch_at(&hatch, &too_short, 2.0, 5.0).is_none());
    let tangent = [Geo::Line {
        handle: Handle::new(50),
        p1: [10.0, -2.0],
        p2: [10.0, 12.0],
    }];
    assert!(trim_hatch_at(&hatch, &tangent, 2.0, 5.0).is_none());
}

fn xline_at(x1: f64, y1: f64, x2: f64, y2: f64, h: u64) -> EntityType {
    let mut l = LineEnt::new();
    l.start = Vector3::new(x1, y1, 0.0);
    l.end = Vector3::new(x2, y2, 0.0);
    l.common.handle = Handle::new(h);
    EntityType::Line(l)
}

fn vertical_boundary(x: f64, y1: f64, y2: f64, handle: u64) -> EntityType {
    xline_at(x, y1, x, y2, handle)
}

#[test]
fn extend_live_box_preview_shows_all_targets_without_modifying_them() {
    let all = vec![
        xline_at(0.0, 0.0, 0.0, 4.0, 1),
        xline_at(2.0, 0.0, 2.0, 5.0, 2),
        xline_at(-2.0, 10.0, 5.0, 10.0, 100),
    ];
    let mut cmd = ExtendCommand::new(all);
    cmd.mode = TrimMode::Pick;
    cmd.edge_set = vec![Handle::new(100)];
    cmd.rebuild_geos();
    let fence = [
        [-1.0, 3.0],
        [3.0, 3.0],
        [3.0, 6.0],
        [-1.0, 6.0],
        [-1.0, 3.0],
    ];
    let window = Some(([-1.0, 3.0], [3.0, 6.0]));
    let preview = cmd
        .on_drag_selection_preview(
            &fence,
            window,
            true,
            &[Handle::new(1), Handle::new(2), Handle::new(100)],
        )
        .expect("EXTEND must preview crossing boxes");
    // Same cyan complete-object overlay and dashed added tail as a single
    // hover, once for every valid target (not for the cutting boundary).
    assert_eq!(
        preview.iter().filter(|w| w.name == "extend_prev").count(),
        2
    );
    assert_eq!(
        preview.iter().filter(|w| w.name == "extend_tail").count(),
        2
    );
    for wire in preview.iter().filter(|w| w.name == "extend_prev") {
        assert!(wire.points.iter().any(|p| (p[1] - 10.0).abs() < 1e-6));
    }
    assert!(cmd.pending.is_empty(), "preview must not commit geometry");
    assert_eq!(cmd.all_entities.len(), 3);
    // Mouse-up still computes actual edits even if a cached preview exists.
    assert!(matches!(
        cmd.on_drag_selection(&fence, window, true),
        Some(CmdResult::ReplaceManyContinue(replacements)) if replacements.len() == 2
    ));
}

#[test]
fn extend_crossing_box_reaches_one_common_horizontal_boundary() {
    let all = vec![
        xline_at(0.0, 0.0, 0.0, 4.0, 1),
        // Reversed direction: the upper endpoint is START, not END.
        xline_at(2.0, 5.0, 2.0, 1.0, 2),
        // Both ends in the box: only the upward extension is reachable.
        xline_at(4.0, 2.0, 4.0, 3.0, 3),
        xline_at(8.0, 0.0, 8.0, 4.0, 4),
        xline_at(-2.0, 10.0, 10.0, 10.0, 100),
    ];
    let mut cmd = ExtendCommand::new(all);
    cmd.mode = TrimMode::Pick;
    cmd.edge_set = vec![Handle::new(100)];
    cmd.rebuild_geos();
    let fence = [
        [-0.5, 1.5],
        [4.5, 1.5],
        [4.5, 6.0],
        [-0.5, 6.0],
        [-0.5, 1.5],
    ];
    let CmdResult::ReplaceManyContinue(replacements) = cmd
        .on_drag_selection(&fence, Some(([-0.5, 1.5], [4.5, 6.0])), true)
        .expect("EXTEND accepts crossing-box selection")
    else {
        panic!("the crossing box must extend all selected lines");
    };
    assert_eq!(replacements.len(), 3);
    assert!(replacements
        .iter()
        .all(|(handle, _)| *handle != Handle::new(4) && *handle != Handle::new(100)));
    for (handle, entities) in replacements {
        let EntityType::Line(line) = &entities[0] else {
            panic!("line expected");
        };
        if handle == Handle::new(2) {
            assert!((line.start.y - 10.0).abs() < 1e-9);
            assert!((line.end.y - 1.0).abs() < 1e-9);
        } else {
            assert!((line.end.y - 10.0).abs() < 1e-9);
            assert!((line.start.y - if handle == Handle::new(1) { 0.0 } else { 2.0 }).abs() < 1e-9);
        }
    }
}

#[test]
fn extend_box_does_not_modify_targets_without_reachable_boundary() {
    let lines = vec![
        xline_at(0.0, 0.0, 0.0, 4.0, 1),
        xline_at(2.0, 0.0, 2.0, 4.0, 2),
    ];
    let mut cmd = ExtendCommand::new(lines);
    cmd.mode = TrimMode::Pick;
    let fence = [
        [-1.0, 3.0],
        [3.0, 3.0],
        [3.0, 5.0],
        [-1.0, 5.0],
        [-1.0, 3.0],
    ];
    assert!(matches!(
        cmd.on_drag_selection(&fence, Some(([-1.0, 3.0], [3.0, 5.0])), true),
        Some(CmdResult::NeedPoint)
    ));
}

#[test]
fn extend_box_window_left_to_right_requires_full_containment() {
    let all = vec![
        xline_at(0.0, 0.0, 0.0, 4.0, 1),
        xline_at(-1.0, 10.0, 1.0, 10.0, 100),
    ];
    let mut cmd = ExtendCommand::new(all);
    cmd.mode = TrimMode::Pick;
    let fence = [
        [-0.5, 3.0],
        [0.5, 3.0],
        [0.5, 5.0],
        [-0.5, 5.0],
        [-0.5, 3.0],
    ];
    assert!(matches!(
        cmd.on_drag_selection(&fence, Some(([-0.5, 3.0], [0.5, 5.0])), false),
        Some(CmdResult::NeedPoint)
    ));
}

#[test]
fn edge_drag_window_selects_only_fully_contained_objects() {
    let entities = vec![
        xline_at(2.0, 2.0, 4.0, 2.0, 1),
        xline_at(0.0, 3.0, 5.0, 3.0, 2),
        xline_at(6.0, 3.0, 8.0, 3.0, 3),
    ];
    let window = CrossingWindow {
        min: [1.0, 1.0],
        max: [5.0, 4.0],
    };
    let rectangle = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];

    assert_eq!(
        drag_selected_handles(
            &entities,
            &ModifyEntityIndex::build(&entities),
            &rectangle,
            Some(window),
            false,
        ),
        vec![Handle::new(1)]
    );
}

#[test]
fn edge_drag_crossing_selects_inside_and_intersecting_objects() {
    let entities = vec![
        xline_at(2.0, 2.0, 4.0, 2.0, 1),
        xline_at(0.0, 3.0, 5.0, 3.0, 2),
        xline_at(6.0, 3.0, 8.0, 3.0, 3),
    ];
    let window = CrossingWindow {
        min: [1.0, 1.0],
        max: [5.0, 4.0],
    };
    let rectangle = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];

    assert_eq!(
        drag_selected_handles(
            &entities,
            &ModifyEntityIndex::build(&entities),
            &rectangle,
            Some(window),
            true,
        ),
        vec![Handle::new(1), Handle::new(2)]
    );
}

#[test]
fn trim_cutting_edge_selection_drag_adds_and_shift_removes_edges() {
    let entities = vec![
        xline_at(2.0, 2.0, 4.0, 2.0, 1),
        xline_at(2.0, 3.0, 4.0, 3.0, 2),
    ];
    let mut command = TrimCommand::new(entities);
    command.mode = TrimMode::SelectEdges;
    let fence = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];
    let window = Some(([1.0, 1.0], [5.0, 4.0]));

    assert!(matches!(
        command.on_drag_selection(&fence, window, false),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(command.edge_set, vec![Handle::new(1), Handle::new(2)]);
    command.set_shift(true);
    command.on_drag_selection(&fence, window, false);
    assert!(command.edge_set.is_empty());
}

#[test]
fn trim_selected_edge_glow_is_persistent_and_subtle() {
    let mut command = TrimCommand::new(vec![xline_at(0.0, 0.0, 5.0, 0.0, 1)]);
    command.mode = TrimMode::SelectEdges;
    command.edge_set.push(Handle::new(1));

    let initial = command.persistent_preview_wires();
    command.on_hover_entity(Handle::NULL, DVec3::ZERO);
    let after_empty_hover = command.persistent_preview_wires();

    assert_eq!(initial.len(), 3);
    assert_eq!(after_empty_hover.len(), initial.len());
    assert!(after_empty_hover.iter().all(|wire| wire.color[3] <= 0.34));
}

#[test]
fn selected_legacy_polyline_boundary_has_visible_glow() {
    use document::entities::{Polyline2D, Vertex2D};

    let mut boundary = Polyline2D::new();
    boundary.vertices = vec![
        Vertex2D::new(Vector3::new(2.0, -1.0, 0.0)),
        Vertex2D::new(Vector3::new(2.0, 1.0, 0.0)),
    ];
    boundary.common.handle = Handle::new(2);
    let mut command = TrimCommand::new(vec![EntityType::Polyline2D(boundary)]);
    command.mode = TrimMode::SelectEdges;

    assert!(matches!(
        command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
    let preview = command.persistent_preview_wires();
    assert_eq!(preview.len(), 3);
    assert!(preview.iter().all(|wire| wire.points.len() >= 2));
}

#[test]
fn trim_can_select_a_polyline2d_as_cutting_edge() {
    use document::entities::{Polyline2D, Vertex2D};

    let target = xline_at(0.0, 0.0, 4.0, 0.0, 1);
    let mut boundary = Polyline2D::new();
    boundary.vertices = vec![
        Vertex2D::new(Vector3::new(2.0, -1.0, 0.0)),
        Vertex2D::new(Vector3::new(2.0, 1.0, 0.0)),
    ];
    boundary.common.handle = Handle::new(2);
    let mut command = TrimCommand::new(vec![target, EntityType::Polyline2D(boundary)]);
    command.quick = false;
    command.mode = TrimMode::SelectEdges;

    command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0));
    assert_eq!(command.persistent_preview_wires().len(), 3);
    assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
    let result = command.on_entity_pick(Handle::new(1), DVec3::new(3.0, 0.0, 0.0));
    let CmdResult::ReplaceEntity(handle, entities) = result else {
        panic!("expected target line to trim against selected POLYLINE2D");
    };
    assert_eq!(handle, Handle::new(1));
    let EntityType::Line(line) = &entities[0] else {
        panic!("expected a line survivor");
    };
    assert!((line.start.x - 0.0).abs() < 1e-6);
    assert!((line.end.x - 2.0).abs() < 1e-6);
}

#[test]
fn trim_line_removed_preview_marks_only_the_deleted_end_or_gap_in_gray() {
    let line = match xline_at(0.0, 0.0, 10.0, 0.0, 1) {
        EntityType::Line(line) => line,
        _ => unreachable!(),
    };
    let survivors = vec![
        xline_at(0.0, 0.0, 3.0, 0.0, 2),
        xline_at(7.0, 0.0, 10.0, 0.0, 3),
    ];
    let removed = trimmed_line_removed_preview(&line, &survivors, 0.5).unwrap();
    let removed_end =
        trimmed_line_removed_preview(&line, &[xline_at(0.0, 0.0, 6.0, 0.0, 2)], 0.8).unwrap();

    assert_eq!(removed, vec![[3.0, 0.0, 0.0], [7.0, 0.0, 0.0]]);
    assert_eq!(removed_end, vec![[6.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
}

#[test]
fn quick_trim_without_intersection_previews_and_erases_line() {
    let all = vec![xline_at(0.0, 0.0, 10.0, 0.0, 1)];
    assert!(pick_trim_at(&all, &build_geos(&all), Handle::new(1), 5.0, 0.0).is_none());
    assert!(
        quick_trim_at(&all, &build_geos(&all), Handle::new(1), 5.0, 0.0)
            .is_some_and(|pieces| pieces.is_empty())
    );

    let mut standard = TrimCommand::new(all.clone());
    standard.quick = false;
    standard.mode = TrimMode::Pick;
    assert!(matches!(
        standard.on_entity_pick(Handle::new(1), glam::DVec3::new(5.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));

    let mut command = TrimCommand::new(all);
    command.quick = true;
    command.mode = TrimMode::Pick;
    let cursor = glam::DVec3::new(5.0, 0.0, 0.0);
    assert!(!command.on_hover_entity(Handle::new(1), cursor).is_empty());
    match command.on_entity_pick(Handle::new(1), cursor) {
        CmdResult::ReplaceEntity(handle, entities) => {
            assert_eq!(handle, Handle::new(1));
            assert!(entities.is_empty());
        }
        _ => panic!("expected full-line erase"),
    }
    command.on_entity_replaced(Handle::new(1), &[]);
    assert!(command.all_entities.is_empty());
}

#[test]
fn lightweight_polyline_hover_reuses_click_independent_cut_points() {
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(10);
    poly.vertices = vec![
        LwVertex::from_coords(0.0, 0.0),
        LwVertex::from_coords(5.0, 0.0),
        LwVertex::from_coords(10.0, 0.0),
    ];
    let all = vec![
        EntityType::LwPolyline(poly.clone()),
        xline_at(7.5, -1.0, 7.5, 1.0, 20),
    ];
    let geos = build_geos(&all);
    let cuts = lwpoly_cut_params(&poly, &geos);
    assert_eq!(cuts.len(), 1);

    let direct_first = trim_lwpolyline(&poly, 2.0, 0.0, &geos).unwrap();
    let cached_first = trim_lwpolyline_with_cuts(&poly, 2.0, 0.0, &cuts).unwrap();
    assert_eq!(format!("{direct_first:?}"), format!("{cached_first:?}"));

    let direct_second = trim_lwpolyline(&poly, 9.0, 0.0, &geos).unwrap();
    let cached_second = trim_lwpolyline_with_cuts(&poly, 9.0, 0.0, &cuts).unwrap();
    assert_eq!(format!("{direct_second:?}"), format!("{cached_second:?}"));

    let mut command = TrimCommand::new(all);
    assert_eq!(
        command.cached_hover_cut_params(Handle::new(10)).unwrap(),
        cuts
    );
    assert!(command.hover_cut_cache.contains_key(&Handle::new(10)));
    assert_eq!(
        command.cached_hover_cut_params(Handle::new(10)).unwrap(),
        cuts
    );
}

#[test]
fn virtual_block_reference_is_not_a_trim_hover_or_pick_target() {
    let virtual_handle = Handle::new(90);
    let mut command = TrimCommand::with_block_boundaries(
        vec![xline_at(0.0, 5.0, 10.0, 5.0, 1)],
        vec![(virtual_handle, vec![xline_at(5.0, 0.0, 5.0, 10.0, 101)])],
        Vec::new(),
    );
    let point = DVec3::new(5.0, 5.0, 0.0);
    assert!(command.on_hover_entity(virtual_handle, point).is_empty());
    assert!(command.hover_cut_cache.is_empty());
    assert!(matches!(
        command.on_entity_pick(virtual_handle, point),
        CmdResult::NeedPoint
    ));
    // The block is still a valid cutting boundary for a real LINE.
    assert_eq!(
        command
            .cached_hover_cut_params(Handle::new(1))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn virtual_block_cutters_are_filtered_by_bounds_without_scanning_all_owners() {
    let target = xline_at(0.0, 5.0, 10.0, 5.0, 1);
    let near_handle = Handle::new(90);
    let far_handle = Handle::new(91);
    let command = TrimCommand::with_block_boundaries(
        vec![target],
        vec![
            (near_handle, vec![xline_at(5.0, 0.0, 5.0, 10.0, 101)]),
            (far_handle, vec![xline_at(500.0, 0.0, 500.0, 10.0, 102)]),
        ],
        Vec::new(),
    );
    assert!(command.block_boundary_owners.contains(&near_handle));
    assert!(command.block_boundary_owners.contains(&far_handle));
    let nearby = command.nearby_geos(Handle::new(1));
    assert!(nearby.iter().any(|g| g.handle() == near_handle));
    assert!(!nearby.iter().any(|g| g.handle() == far_handle));
    assert_eq!(
        line_seg_ts(0.0, 5.0, 10.0, 5.0, Handle::new(1), &nearby).len(),
        1
    );
}

#[test]
fn trim_against_block_contents_keeps_only_the_clicked_block_interval() {
    let target = xline_at(0.0, 5.0, 10.0, 5.0, 1);
    let block_handle = Handle::new(90);
    let triangle = vec![
        xline_at(3.0, 3.0, 7.0, 5.0, 101),
        xline_at(7.0, 5.0, 3.0, 7.0, 102),
        xline_at(3.0, 7.0, 3.0, 3.0, 103),
    ];

    for (quick, initial_edges) in [(true, Vec::new()), (false, vec![block_handle])] {
        let mut command = TrimCommand::with_block_boundaries(
            vec![target.clone()],
            vec![(block_handle, triangle.clone())],
            initial_edges,
        );
        command.quick = quick;
        command.mode = TrimMode::Pick;

        let CmdResult::ReplaceEntity(handle, survivors) =
            command.on_entity_pick(Handle::new(1), DVec3::new(5.0, 5.0, 0.0))
        else {
            panic!("TRIM should cut against the block contents");
        };
        assert_eq!(handle, Handle::new(1));
        assert_eq!(survivors.len(), 2);
        let spans: Vec<_> = survivors
            .iter()
            .map(|entity| match entity {
                EntityType::Line(line) => (line.start.x, line.end.x),
                _ => panic!("expected line survivors"),
            })
            .collect();
        assert!((spans[0].0 - 0.0).abs() < 1e-6);
        assert!((spans[0].1 - 3.0).abs() < 1e-6);
        assert!((spans[1].0 - 7.0).abs() < 1e-6);
        assert!((spans[1].1 - 10.0).abs() < 1e-6);
    }
}

#[test]
fn extend_shift_uses_quick_trim_full_erase_for_line_without_boundaries() {
    let all = vec![xline_at(0.0, 0.0, 10.0, 0.0, 1)];
    let mut command = ExtendCommand::new(all);
    command.quick = true;
    command.mode = TrimMode::Pick;
    command.shift = true;
    let cursor = glam::DVec3::new(5.0, 0.0, 0.0);

    assert!(!command.on_hover_entity(Handle::new(1), cursor).is_empty());
    assert!(matches!(
        command.on_entity_pick(Handle::new(1), cursor),
        CmdResult::ReplaceEntity(handle, entities)
            if handle == Handle::new(1) && entities.is_empty()
    ));
}

#[test]
fn implied_edge_option_is_available_only_in_standard_mode() {
    let mut trim = TrimCommand::new(Vec::new());
    trim.mode = TrimMode::Pick;
    trim.quick = true;
    assert!(!trim.options().iter().any(|option| option.keyword == "E"));
    assert!(matches!(
        trim.on_text_input("E"),
        Some(CmdResult::ReportError(_))
    ));
    assert!(!trim.implied_edges);
    trim.quick = false;
    assert!(trim.options().iter().any(|option| option.keyword == "E"));
    assert!(matches!(
        trim.on_text_input("E"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(trim.implied_edges);

    let mut extend = ExtendCommand::new(Vec::new());
    extend.mode = TrimMode::Pick;
    extend.quick = true;
    assert!(!extend.options().iter().any(|option| option.keyword == "E"));
    assert!(matches!(
        extend.on_text_input("E"),
        Some(CmdResult::ReportError(_))
    ));
    assert!(!extend.implied_edges);
    extend.quick = false;
    assert!(extend.options().iter().any(|option| option.keyword == "E"));
    assert!(matches!(
        extend.on_text_input("E"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(extend.implied_edges);

    // Outside the main prompt the keywords are swallowed, never handed
    // back to be read as hex handles.
    trim.mode = TrimMode::SelectEdges;
    extend.mode = TrimMode::SelectEdges;
    for keyword in ["E", "F", "C"] {
        assert!(trim.on_text_input(keyword).is_some());
        assert!(extend.on_text_input(keyword).is_some());
    }
}

#[test]
fn canceling_boundary_selection_restores_quick_mode_and_discards_temporary_edges() {
    let line = xline_at(0.0, 0.0, 10.0, 0.0, 1);
    let mut trim = TrimCommand::new(vec![line.clone()]);
    trim.mode = TrimMode::Pick;
    trim.quick = true;
    assert!(matches!(
        trim.on_text_input("B"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(!trim.quick);
    trim.on_entity_pick(Handle::new(1), glam::DVec3::ZERO);
    assert_eq!(trim.edge_set, vec![Handle::new(1)]);
    assert!(matches!(trim.on_escape(), CmdResult::NeedPoint));
    assert!(trim.quick);
    assert!(trim.edge_set.is_empty());
    assert!(matches!(trim.mode, TrimMode::Pick));

    let mut extend = ExtendCommand::new(vec![line]);
    extend.mode = TrimMode::Pick;
    extend.quick = true;
    assert!(matches!(
        extend.on_text_input("B"),
        Some(CmdResult::NeedPoint)
    ));
    extend.on_entity_pick(Handle::new(1), glam::DVec3::ZERO);
    assert!(matches!(extend.on_escape(), CmdResult::NeedPoint));
    assert!(extend.quick);
    assert!(extend.edge_set.is_empty());
    assert!(matches!(extend.mode, TrimMode::Pick));
}

#[test]
fn fence_edits_commit_as_one_undoable_gesture_and_leave_commands_active() {
    let trim_all = vec![
        xline_at(0.0, 0.0, 10.0, 0.0, 1),
        vertical_boundary(6.0, -2.0, 2.0, 2),
    ];
    let mut trim = TrimCommand::new(trim_all);
    trim.mode = TrimMode::Pick;
    let trim_result = trim.fence_run(&[[8.0, -1.0], [8.0, 1.0]], None);
    assert!(matches!(trim_result, CmdResult::ReplaceManyContinue(_)));
    assert!(matches!(trim.mode, TrimMode::Pick));
    // A rejected edit (locked layer) never calls back: snapshot untouched.
    let trim_result = trim.fence_run(&[[8.0, -1.0], [8.0, 1.0]], None);
    assert!(matches!(trim_result, CmdResult::ReplaceManyContinue(_)));
    assert!(trim
        .all_entities
        .iter()
        .all(|e| !e.common().handle.is_null()));
    // An accepted one swaps in the pieces under their document handles.
    trim.on_entities_replaced(&[(Handle::new(1), vec![Handle::new(10)])]);
    let handles: Vec<_> = trim
        .all_entities
        .iter()
        .map(|e| e.common().handle)
        .collect();
    assert_eq!(handles, vec![Handle::new(2), Handle::new(10)]);

    let extend_all = vec![
        xline_at(0.0, 0.0, 4.0, 0.0, 1),
        vertical_boundary(8.0, -2.0, 2.0, 2),
    ];
    let mut extend = ExtendCommand::new(extend_all);
    extend.mode = TrimMode::Pick;
    let extend_result = extend.fence_run(&[[3.5, -1.0], [3.5, 1.0]], None);
    assert!(matches!(extend_result, CmdResult::ReplaceManyContinue(_)));
    assert!(matches!(extend.mode, TrimMode::Pick));
    extend.on_entities_replaced(&[(Handle::new(1), vec![Handle::new(10)])]);
    let handles: Vec<_> = extend
        .all_entities
        .iter()
        .map(|e| e.common().handle)
        .collect();
    assert_eq!(handles, vec![Handle::new(2), Handle::new(10)]);
}

#[test]
fn extend_polyline_terminal_bulge_follows_arc_and_keeps_circle_center() {
    let mut poly = LwPolyline::new();
    let mut start = LwVertex::from_coords(0.0, 0.0);
    start.bulge = 0.5;
    poly.vertices = vec![start, LwVertex::from_coords(2.0, 0.0)];
    poly.common.handle = Handle::new(1);
    let boundary = xline_at(1.5, 0.75, 3.0, 0.75, 2);
    let all = vec![EntityType::LwPolyline(poly.clone()), boundary];

    let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 1.9, 0.05)
        .expect("the terminal arc meets the boundary ahead of its endpoint");
    let EntityType::LwPolyline(extended) = result else {
        panic!("expected an extended lightweight polyline");
    };
    let old_arc = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], 0.5).unwrap();
    let new_end = extended.vertices.last().unwrap();
    let distance_to_center =
        (new_end.location.x - old_arc.center[0]).hypot(new_end.location.y - old_arc.center[1]);
    assert!((distance_to_center - old_arc.radius).abs() < 1e-6);
    assert!((new_end.location.x - 2.25).abs() < 1e-6);
    assert!((new_end.location.y - 0.75).abs() < 1e-6);
    assert!(extended.vertices[0].bulge > 0.5);
    assert!(extended.vertices[0].bulge.is_finite());
}

#[test]
fn trim_circle_with_two_boundaries_is_stable_under_entity_order() {
    let mut circle = CircleEnt::new();
    circle.center = Vector3::new(0.0, 0.0, 0.0);
    circle.radius = 10.0;
    circle.common.handle = Handle::new(1);
    let left = vertical_boundary(-5.0, -12.0, 12.0, 2);
    let right = vertical_boundary(5.0, -12.0, 12.0, 3);
    let variants = [
        vec![
            EntityType::Circle(circle.clone()),
            left.clone(),
            right.clone(),
        ],
        vec![right, EntityType::Circle(circle.clone()), left],
    ];
    let mut canonical: Option<Vec<(f64, f64)>> = None;
    for all in variants {
        let result = pick_trim_at(&all, &build_geos(&all), Handle::new(1), 0.0, 10.0)
            .expect("circle has two cutting boundaries");
        let mut arcs: Vec<_> = result
            .iter()
            .map(|entity| match entity {
                EntityType::Arc(arc) => (arc.start_angle, arc.end_angle),
                other => panic!("expected circle trim arcs, got {other:?}"),
            })
            .collect();
        arcs.sort_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(expected) = &canonical {
            assert_eq!(arcs.len(), expected.len());
            for (actual, expected) in arcs.iter().zip(expected) {
                assert!((actual.0 - expected.0).abs() < 1e-7);
                assert!((actual.1 - expected.1).abs() < 1e-7);
            }
        } else {
            canonical = Some(arcs);
        }
    }
}

#[test]
fn polyline2d_bulge_segment_is_used_as_a_curved_trim_boundary() {
    use document::entities::{Polyline2D, Vertex2D};

    let target = xline_at(-2.0, 0.0, 4.0, 0.0, 1);
    let mut boundary = Polyline2D::new();
    let mut first = Vertex2D::new(Vector3::new(0.0, -2.0, 0.0));
    first.bulge = 1.0;
    boundary.vertices = vec![first, Vertex2D::new(Vector3::new(0.0, 2.0, 0.0))];
    boundary.common.handle = Handle::new(2);
    let all = vec![target, EntityType::Polyline2D(boundary)];

    let result = pick_trim_at(&all, &build_geos(&all), Handle::new(1), 3.0, 0.0)
        .expect("the bulged POLYLINE2D boundary crosses the target line twice");
    assert_eq!(result.len(), 1);
    let EntityType::Line(line) = &result[0] else {
        panic!("expected a line survivor");
    };
    // The half-circle bulge reaches x=2, so the clicked right interval is
    // removed and the left interval survives.
    assert!((line.start.x + 2.0).abs() < 1e-6);
    assert!((line.end.x - 2.0).abs() < 1e-6);
}

#[test]
fn extend_line_reaches_polyline2d_boundary() {
    use document::entities::{Polyline2D, Vertex2D};

    let target = xline_at(0.0, 0.0, 1.0, 0.0, 1);
    let mut boundary = Polyline2D::new();
    boundary.vertices = vec![
        Vertex2D::new(Vector3::new(3.0, -1.0, 0.0)),
        Vertex2D::new(Vector3::new(3.0, 1.0, 0.0)),
    ];
    boundary.common.handle = Handle::new(2);
    let all = vec![target, EntityType::Polyline2D(boundary)];
    let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 0.9, 0.0)
        .expect("the line should extend to the POLYLINE2D boundary");
    let EntityType::Line(line) = result else {
        panic!("expected an extended line");
    };
    assert!((line.end.x - 3.0).abs() < 1e-6);
}

#[test]
fn trim_can_select_a_lightweight_polyline_as_cutting_edge() {
    let target = xline_at(0.0, 0.0, 4.0, 0.0, 1);
    let mut boundary = LwPolyline::new();
    boundary.common.handle = Handle::new(2);
    boundary.vertices = vec![
        LwVertex::from_coords(2.0, -1.0),
        LwVertex::from_coords(2.0, 1.0),
    ];
    let mut command =
        TrimCommand::with_cutting_edges(vec![target, EntityType::LwPolyline(boundary)], Vec::new());
    command.quick = false;
    command.mode = TrimMode::SelectEdges;

    assert!(matches!(
        command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
    assert_eq!(command.edge_set, vec![Handle::new(2)]);
    assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
    let result = command.on_entity_pick(Handle::new(1), DVec3::new(3.0, 0.0, 0.0));
    let CmdResult::ReplaceEntity(handle, entities) = result else {
        panic!("expected target line to trim against selected polyline");
    };
    assert_eq!(handle, Handle::new(1));
    assert_eq!(entities.len(), 1);
    let EntityType::Line(line) = &entities[0] else {
        panic!("expected a line survivor");
    };
    assert!((line.start.x - 0.0).abs() < 1e-6);
    assert!((line.end.x - 2.0).abs() < 1e-6);
}

#[test]
fn trim_uses_projected_fallback_for_imported_lwpolyline_with_tilted_ocs() {
    let target = xline_at(-5.0, 0.0, 5.0, 0.0, 1);
    let mut boundary = LwPolyline::new();
    boundary.common.handle = Handle::new(2);
    boundary.normal = Vector3::new(0.5, 0.0, 3.0_f64.sqrt() / 2.0);
    boundary.is_closed = true;
    boundary.vertices = vec![
        LwVertex::from_coords(-1.0, -1.0),
        LwVertex::from_coords(1.0, -1.0),
        LwVertex::from_coords(1.0, 1.0),
        LwVertex::from_coords(-1.0, 1.0),
    ];
    let boundary = EntityType::LwPolyline(boundary);

    // This is the imported OCS case: the existing explode helper cannot
    // materialize world-XY segments, but the shared curve projection can.
    assert!(crate::modules::draw::modify::explode::explode_polyline_segments(&boundary).is_empty());
    let mut command = TrimCommand::new(vec![target, boundary]);
    command.quick = false;
    command.mode = TrimMode::SelectEdges;
    command.on_entity_pick(Handle::new(2), DVec3::ZERO);
    assert_eq!(command.persistent_preview_wires().len(), 3);
    assert!(matches!(command.on_enter(), CmdResult::NeedPoint));

    let result = command.on_entity_pick(Handle::new(1), DVec3::ZERO);
    let CmdResult::ReplaceEntity(handle, pieces) = result else {
        panic!("expected the imported polyline boundary to trim the target");
    };
    assert_eq!(handle, Handle::new(1));
    assert_eq!(pieces.len(), 2);
}

#[test]
fn extend_line_can_reach_a_lightweight_polyline_boundary() {
    let target = xline_at(0.0, 0.0, 1.0, 0.0, 1);
    let mut boundary = LwPolyline::new();
    boundary.common.handle = Handle::new(2);
    boundary.vertices = vec![
        LwVertex::from_coords(3.0, -1.0),
        LwVertex::from_coords(3.0, 1.0),
    ];
    let all = vec![target, EntityType::LwPolyline(boundary)];
    let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 0.9, 0.0)
        .expect("the line should extend to the polyline boundary");
    let EntityType::Line(line) = result else {
        panic!("expected an extended line");
    };
    assert!((line.start.x - 0.0).abs() < 1e-6);
    assert!((line.end.x - 3.0).abs() < 1e-6);
}

#[test]
fn fence_handles_repeated_crossings_and_multiple_targets_independent_of_order() {
    let targets = [
        xline_at(0.0, 0.0, 10.0, 0.0, 1),
        xline_at(0.0, 4.0, 10.0, 4.0, 4),
    ];
    let boundaries = [
        vertical_boundary(3.0, -2.0, 6.0, 2),
        vertical_boundary(7.0, -2.0, 6.0, 3),
    ];
    let fence = [[2.0, -1.0], [2.0, 5.0], [5.0, -1.0], [5.0, 5.0]];
    let mut expected: Option<Vec<(Handle, Vec<([f64; 2], [f64; 2])>)>> = None;
    for all in [
        vec![
            targets[0].clone(),
            targets[1].clone(),
            boundaries[0].clone(),
            boundaries[1].clone(),
        ],
        vec![
            boundaries[1].clone(),
            targets[1].clone(),
            boundaries[0].clone(),
            targets[0].clone(),
        ],
    ] {
        let replacements = fence_pass(
            &all,
            &ModifyEntityIndex::build(&all),
            &build_geos(&all),
            &fence,
            None,
            false,
        );
        let mut actual: Vec<_> = replacements
            .into_iter()
            .map(|(handle, pieces)| {
                let mut segments: Vec<_> = pieces
                    .iter()
                    .map(|piece| match piece {
                        EntityType::Line(line) => {
                            ([line.start.x, line.start.y], [line.end.x, line.end.y])
                        }
                        other => panic!("expected line piece, got {other:?}"),
                    })
                    .collect();
                segments.sort_by(|a, b| a.0[1].total_cmp(&b.0[1]).then(a.0[0].total_cmp(&b.0[0])));
                (handle, segments)
            })
            .collect();
        actual.sort_by_key(|(handle, _)| format!("{handle:?}"));
        assert!(actual.iter().any(|(handle, _)| *handle == Handle::new(1)));
        assert!(actual.iter().any(|(handle, _)| *handle == Handle::new(4)));
        assert!(actual.iter().all(|(_, pieces)| !pieces.is_empty()));
        if let Some(expected) = &expected {
            assert_eq!(actual.len(), expected.len());
            for ((ha, pa), (he, pe)) in actual.iter().zip(expected) {
                assert_eq!(ha, he);
                assert_eq!(pa.len(), pe.len());
                for (a, e) in pa.iter().zip(pe) {
                    for axis in 0..2 {
                        assert!((a.0[axis] - e.0[axis]).abs() < 1e-6);
                        assert!((a.1[axis] - e.1[axis]).abs() < 1e-6);
                    }
                }
            }
        } else {
            expected = Some(actual);
        }
    }
}

#[test]
fn crossing_window_trims_objects_inside_and_crossing_it() {
    let all = vec![
        xline_at(2.0, 2.0, 8.0, 2.0, 1),
        xline_at(-2.0, 5.0, 12.0, 5.0, 2),
        vertical_boundary(5.0, -1.0, 11.0, 3),
    ];
    let window = CrossingWindow {
        min: [1.0, 1.0],
        max: [9.0, 9.0],
    };
    let replacements = fence_pass(
        &all,
        &ModifyEntityIndex::build(&all),
        &build_geos(&all),
        &[],
        Some(window),
        false,
    );
    assert!(replacements
        .iter()
        .any(|(handle, _)| *handle == Handle::new(1)));
    assert!(replacements
        .iter()
        .any(|(handle, _)| *handle == Handle::new(2)));
}

#[test]
fn drag_lasso_applies_to_multiple_entities_in_any_entity_order() {
    let target_a = xline_at(0.0, 0.0, 10.0, 0.0, 1);
    let target_b = xline_at(0.0, 4.0, 10.0, 4.0, 2);
    let boundary_a = vertical_boundary(5.0, -2.0, 6.0, 3);
    let boundary_b = vertical_boundary(6.0, -2.0, 6.0, 4);
    let mut canonical: Option<Vec<(Handle, Vec<([f64; 2], [f64; 2])>)>> = None;
    for all in [
        vec![
            target_a.clone(),
            target_b.clone(),
            boundary_a.clone(),
            boundary_b.clone(),
        ],
        vec![
            boundary_b.clone(),
            target_b.clone(),
            boundary_a.clone(),
            target_a.clone(),
        ],
    ] {
        let mut command = TrimCommand::new(all);
        command.mode = TrimMode::Pick;
        let fence = [[4.0, -1.0], [4.0, 5.0], [7.0, 5.0]];
        let result = command.on_drag_selection(&fence, None, false);
        let Some(CmdResult::ReplaceManyContinue(replacements)) = result else {
            panic!("expected a multi-entity lasso trim");
        };
        assert!(replacements
            .iter()
            .any(|(handle, _)| *handle == Handle::new(1)));
        assert!(replacements
            .iter()
            .any(|(handle, _)| *handle == Handle::new(2)));
        let mut actual: Vec<_> = replacements
            .into_iter()
            .map(|(handle, pieces)| {
                let mut segments: Vec<_> = pieces
                    .iter()
                    .map(|piece| match piece {
                        EntityType::Line(line) => {
                            ([line.start.x, line.start.y], [line.end.x, line.end.y])
                        }
                        other => panic!("expected lasso line survivor, got {other:?}"),
                    })
                    .collect();
                segments.sort_by(|a, b| a.0[1].total_cmp(&b.0[1]));
                (handle, segments)
            })
            .collect();
        actual.sort_by_key(|(handle, _)| format!("{handle:?}"));
        assert!(actual.iter().all(|(_, pieces)| !pieces.is_empty()));
        if let Some(expected) = &canonical {
            assert_eq!(actual, *expected, "lasso output must ignore entity order");
        } else {
            canonical = Some(actual);
        }
    }
}

/// Edge: Extend — the target must lengthen to the BOUNDARY'S extrapolation
/// when the boundary doesn't physically cross the target's path (#336).
#[test]
fn edge_extend_reaches_implied_boundary() {
    let target = xline_at(0.0, 0.0, 2.0, 0.0, 1);
    let boundary = xline_at(5.0, 2.0, 5.0, 8.0, 2);
    let all = vec![target.clone(), boundary];
    // Edge off: the boundary's drawn extent never meets y=0 — no extend.
    let geos = build_geos(&all);
    assert!(pick_extend_at(&all, &geos, Handle::new(1), 1.9, 0.0).is_none());
    // Edge on: the implied boundary crosses y=0 at x=5 — extend to (5,0).
    let mut implied = build_geos(&all);
    imply_edge_geos(&mut implied);
    match pick_extend_at(&all, &implied, Handle::new(1), 1.9, 0.0) {
        Some(EntityType::Line(l)) => {
            assert!(
                (l.end.x - 5.0).abs() < 1e-6 && l.end.y.abs() < 1e-6,
                "extends to the implied boundary, got ({}, {})",
                l.end.x,
                l.end.y
            );
        }
        other => panic!("expected an extended Line, got {other:?}"),
    }
}

/// #336 repro: two lines forming an X; a fence across one overhanging arm
/// must trim that arm back to the intersection.
#[test]
fn fence_trims_x_arm() {
    let all = vec![
        xline_at(0.0, 0.0, 10.0, 10.0, 1),
        xline_at(0.0, 10.0, 10.0, 0.0, 2),
    ];
    let geos = build_geos(&all);
    let fence = [[8.0, 10.0], [10.0, 8.0]];
    let repl = fence_pass(
        &all,
        &ModifyEntityIndex::build(&all),
        &geos,
        &fence,
        None,
        false,
    );
    assert_eq!(
        repl.len(),
        1,
        "exactly the crossed line is trimmed: {repl:?}"
    );
    assert_eq!(repl[0].0, Handle::new(1));
    assert_eq!(repl[0].1.len(), 1);
    match &repl[0].1[0] {
        EntityType::Line(l) => {
            assert!(
                (l.end.x - 5.0).abs() < 1e-6 && (l.end.y - 5.0).abs() < 1e-6,
                "arm cut back to the intersection, got end ({}, {})",
                l.end.x,
                l.end.y
            );
        }
        other => panic!("expected a Line, got {other:?}"),
    }
}

fn circle(r: f64) -> CircleEnt {
    let mut c = CircleEnt::new();
    c.center = Vector3::new(0.0, 0.0, 0.0);
    c.radius = r;
    c
}

/// A circle crossed by a horizontal cutter (cuts at angle 0 and π, i.e.
/// t = 0.0 and 0.5) becomes a single Arc; clicking the top half removes it
/// and leaves the bottom half (π → 0 CCW), clicking the bottom does the
/// reverse.
#[test]
fn trims_circle_into_arc_on_clicked_half() {
    let c = circle(10.0);
    let ts = [0.0, 0.5];

    // Click top (t = 0.25) → removes top, keeps bottom half (start π, end 0).
    let top = trim_circle(&c, &ts, 0.25);
    assert_eq!(top.len(), 1, "circle should trim to exactly one arc");
    match &top[0] {
        EntityType::Arc(a) => {
            assert!((norm(a.start_angle) - PI).abs() < 1e-9);
            assert!(norm(a.end_angle).abs() < 1e-9);
            assert_eq!(a.radius, 10.0);
            assert!(a.common.handle.is_null());
        }
        _ => panic!("expected an Arc"),
    }

    // Click bottom (t = 0.75) → keeps top half (start 0, end π).
    let bottom = trim_circle(&c, &ts, 0.75);
    match &bottom[0] {
        EntityType::Arc(a) => {
            assert!(norm(a.start_angle).abs() < 1e-9);
            assert!((norm(a.end_angle) - PI).abs() < 1e-9);
        }
        _ => panic!("expected an Arc"),
    }
}

/// Fewer than two crossings can't cut a closed circle, so it is left as-is.
#[test]
fn circle_with_one_or_zero_cuts_is_left_unchanged() {
    let c = circle(5.0);
    assert!(trim_circle(&c, &[], 0.3).is_empty());
    assert!(trim_circle(&c, &[0.4], 0.3).is_empty());
}

#[test]
fn trim_polyline_arc_segment_preserves_bulge() {
    use std::f64::consts::FRAC_PI_8;
    // Arc from (0, 0) to (2, 0) with bulge 1.0 (semicircle sweeping CCW under the chord)
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(10);
    let mut v0 = LwVertex::from_coords(0.0, 0.0);
    v0.bulge = 1.0;
    let v1 = LwVertex::from_coords(2.0, 0.0);
    poly.vertices = vec![v0, v1];

    // Vertical cutter line at x = 1.0 crossing the arc at (1.0, -1.0)
    let mut cutter = LineEnt::new();
    cutter.start = Vector3::new(1.0, -2.0, 0.0);
    cutter.end = Vector3::new(1.0, 2.0, 0.0);
    cutter.common.handle = Handle::new(20);

    let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
    let geos = build_geos(&all);

    // Click first half near (0.5, -0.8) -> first half removed, second half kept from (1.0, -1.0) to (2.0, 0.0)
    let res =
        pick_trim_at(&all, &geos, Handle::new(10), 0.5, -0.8).expect("should trim polyline arc");
    assert_eq!(res.len(), 1);
    match &res[0] {
        EntityType::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 2);
            let p0 = &p.vertices[0];
            let p1 = &p.vertices[1];
            assert!(
                (p0.location.x - 1.0).abs() < 1e-5,
                "expected x=1.0, got {}",
                p0.location.x
            );
            assert!(
                (p0.location.y + 1.0).abs() < 1e-5,
                "expected y=-1.0, got {}",
                p0.location.y
            );
            assert!(
                (p1.location.x - 2.0).abs() < 1e-5,
                "expected x=2.0, got {}",
                p1.location.x
            );
            assert!(
                p1.location.y.abs() < 1e-5,
                "expected y=0.0, got {}",
                p1.location.y
            );
            // The surviving quarter circle should have bulge = tan(pi / 8)
            let expected_bulge = FRAC_PI_8.tan();
            assert!(
                (p0.bulge - expected_bulge).abs() < 1e-4,
                "surviving arc should retain quarter-circle bulge {}, got {}",
                expected_bulge,
                p0.bulge
            );
            assert_eq!(p1.bulge, 0.0);
        }
        other => panic!("expected LwPolyline, got {other:?}"),
    }

    // Click second half near (1.5, -0.8) -> second half removed, first half kept from (0.0, 0.0) to (1.0, -1.0)
    let res2 =
        pick_trim_at(&all, &geos, Handle::new(10), 1.5, -0.8).expect("should trim polyline arc");
    assert_eq!(res2.len(), 1);
    match &res2[0] {
        EntityType::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 2);
            let p0 = &p.vertices[0];
            let p1 = &p.vertices[1];
            assert!(
                p0.location.x.abs() < 1e-5,
                "expected x=0.0, got {}",
                p0.location.x
            );
            assert!(
                p0.location.y.abs() < 1e-5,
                "expected y=0.0, got {}",
                p0.location.y
            );
            assert!(
                (p1.location.x - 1.0).abs() < 1e-5,
                "expected x=1.0, got {}",
                p1.location.x
            );
            assert!(
                (p1.location.y + 1.0).abs() < 1e-5,
                "expected y=-1.0, got {}",
                p1.location.y
            );
            let expected_bulge = FRAC_PI_8.tan();
            assert!(
                (p0.bulge - expected_bulge).abs() < 1e-4,
                "surviving arc should retain quarter-circle bulge {}, got {}",
                expected_bulge,
                p0.bulge
            );
            assert_eq!(p1.bulge, 0.0);
        }
        other => panic!("expected LwPolyline, got {other:?}"),
    }
}

#[test]
fn trim_multi_segment_polyline_with_arc() {
    use std::f64::consts::FRAC_PI_8;
    // Segment 0: straight line from (-2, 0) to (0, 0)
    // Segment 1: arc from (0, 0) to (2, 0) with bulge 1.0
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(30);
    let v0 = LwVertex::from_coords(-2.0, 0.0);
    let mut v1 = LwVertex::from_coords(0.0, 0.0);
    v1.bulge = 1.0;
    let v2 = LwVertex::from_coords(2.0, 0.0);
    poly.vertices = vec![v0, v1, v2];

    // Vertical cutter at x = 1.0
    let mut cutter = LineEnt::new();
    cutter.start = Vector3::new(1.0, -2.0, 0.0);
    cutter.end = Vector3::new(1.0, 2.0, 0.0);
    cutter.common.handle = Handle::new(40);

    let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
    let geos = build_geos(&all);

    // Click the far end of the arc near (1.5, -0.8) -> removes end, keeps straight seg + first half of arc
    let res = pick_trim_at(&all, &geos, Handle::new(30), 1.5, -0.8).expect("should trim");
    assert_eq!(res.len(), 1);
    match &res[0] {
        EntityType::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 3);
            // Vertex 0: (-2, 0), straight
            assert!((p.vertices[0].location.x + 2.0).abs() < 1e-5);
            assert!(p.vertices[0].bulge.abs() < 1e-5);
            // Vertex 1: (0, 0), arc with quarter-turn bulge
            assert!(p.vertices[1].location.x.abs() < 1e-5);
            let expected_bulge = FRAC_PI_8.tan();
            assert!((p.vertices[1].bulge - expected_bulge).abs() < 1e-4);
            // Vertex 2: (1, -1), end
            assert!((p.vertices[2].location.x - 1.0).abs() < 1e-5);
            assert!((p.vertices[2].location.y + 1.0).abs() < 1e-5);
        }
        other => panic!("expected LwPolyline, got {other:?}"),
    }
}

#[test]
fn trim_negative_bulge_polyline_arc() {
    use std::f64::consts::FRAC_PI_8;
    // Negative bulge means CW arc: from (0, 0) to (2, 0) arches above the chord (y > 0)
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(50);
    let mut v0 = LwVertex::from_coords(0.0, 0.0);
    v0.bulge = -1.0;
    let v1 = LwVertex::from_coords(2.0, 0.0);
    poly.vertices = vec![v0, v1];

    // Vertical cutter at x = 1.0 crossing at (1.0, 1.0)
    let mut cutter = LineEnt::new();
    cutter.start = Vector3::new(1.0, -2.0, 0.0);
    cutter.end = Vector3::new(1.0, 2.0, 0.0);
    cutter.common.handle = Handle::new(60);

    let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
    let geos = build_geos(&all);

    // Click first half near (0.5, 0.8) -> removes first half, keeps second half from (1.0, 1.0) to (2.0, 0.0)
    let res = pick_trim_at(&all, &geos, Handle::new(50), 0.5, 0.8).expect("should trim CW arc");
    assert_eq!(res.len(), 1);
    match &res[0] {
        EntityType::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 2);
            let p0 = &p.vertices[0];
            let p1 = &p.vertices[1];
            assert!((p0.location.x - 1.0).abs() < 1e-5);
            assert!((p0.location.y - 1.0).abs() < 1e-5);
            assert!((p1.location.x - 2.0).abs() < 1e-5);
            assert!(p1.location.y.abs() < 1e-5);
            let expected_bulge = -FRAC_PI_8.tan();
            assert!(
                (p0.bulge - expected_bulge).abs() < 1e-4,
                "expected negative bulge {}, got {}",
                expected_bulge,
                p0.bulge
            );
        }
        other => panic!("expected LwPolyline, got {other:?}"),
    }
}

#[test]
fn trim_closed_polyline_with_arc() {
    use std::f64::consts::FRAC_PI_8;
    // Closed polyline:
    // Vertex 0: (0, 0), bulge 1.0 (arc to (2, 0) below chord, peak at y=-1)
    // Vertex 1: (2, 0), bulge 0.0 (straight to (2, 2))
    // Vertex 2: (2, 2), bulge 0.0 (straight to (0, 2))
    // Vertex 3: (0, 2), bulge 0.0 (straight back to (0, 0))
    let mut poly = LwPolyline::new();
    poly.common.handle = Handle::new(70);
    poly.is_closed = true;
    let mut v0 = LwVertex::from_coords(0.0, 0.0);
    v0.bulge = 1.0;
    let v1 = LwVertex::from_coords(2.0, 0.0);
    let v2 = LwVertex::from_coords(2.0, 2.0);
    let v3 = LwVertex::from_coords(0.0, 2.0);
    poly.vertices = vec![v0, v1, v2, v3];

    // Cutter 1: x = 1.0 cuts the arc at (1.0, -1.0)
    let mut c1 = LineEnt::new();
    c1.start = Vector3::new(1.0, -3.0, 0.0);
    c1.end = Vector3::new(1.0, -0.5, 0.0);
    c1.common.handle = Handle::new(80);

    // Cutter 2: y = 1.0 cuts vertical edges at x=0 and x=2
    let mut c2 = LineEnt::new();
    c2.start = Vector3::new(-1.0, 1.0, 0.0);
    c2.end = Vector3::new(3.0, 1.0, 0.0);
    c2.common.handle = Handle::new(81);

    let all = vec![
        EntityType::LwPolyline(poly),
        EntityType::Line(c1),
        EntityType::Line(c2),
    ];
    let geos = build_geos(&all);

    // Click first half of arc near (0.5, -0.8):
    // Nearest cuts are at (1.0, -1.0) on seg 0 and at (0.0, 1.0) on seg 3.
    // Trim removes that corner/arc portion and leaves an open polyline
    let res =
        pick_trim_at(&all, &geos, Handle::new(70), 0.5, -0.8).expect("should trim closed polyline");
    assert_eq!(res.len(), 1);
    match &res[0] {
        EntityType::LwPolyline(p) => {
            assert!(!p.is_closed);
            // Starts at (1.0, -1.0) with remaining quarter-circle arc
            let start_v = &p.vertices[0];
            assert!((start_v.location.x - 1.0).abs() < 1e-5);
            assert!((start_v.location.y + 1.0).abs() < 1e-5);
            let expected_bulge = FRAC_PI_8.tan();
            assert!((start_v.bulge - expected_bulge).abs() < 1e-4);
        }
        other => panic!("expected LwPolyline, got {other:?}"),
    }
}

/// #1318 repro: a 3D polyline whose first segment is not horizontal has no
/// plan-view shape, so it samples to nothing. The seam test then compared
/// two empty ends as equal and sliced the empty sample from index 1.
#[test]
fn sampling_skips_segments_with_no_plan_shape() {
    use document::entities::{Polyline3D, Vertex3DPolyline};

    let mut pl = Polyline3D::new();
    pl.vertices = vec![
        // Vertical: an upright curve plane, dropped by `entity_curve_xy`.
        Vertex3DPolyline::from_xyz(0.0, 0.0, 0.0),
        Vertex3DPolyline::from_xyz(0.0, 0.0, 10.0),
        // Horizontal, so this one does sample.
        Vertex3DPolyline::from_xyz(10.0, 0.0, 10.0),
    ];

    let pts = sample_entity_xy(&EntityType::Polyline3D(pl));
    assert!(
        !pts.is_empty(),
        "the horizontal segment still has to be sampled"
    );
    assert!(
        pts.iter().all(|p| p.iter().all(|c| c.is_finite())),
        "sampled points stay finite: {pts:?}"
    );
}

/// The same skip must not swallow a leading segment that does sample, and
/// must keep dropping the duplicated seam vertex between two of them.
#[test]
fn sampling_still_joins_segments_at_their_seam() {
    use document::entities::{Polyline3D, Vertex3DPolyline};

    let mut pl = Polyline3D::new();
    pl.vertices = vec![
        Vertex3DPolyline::from_xyz(0.0, 0.0, 0.0),
        Vertex3DPolyline::from_xyz(10.0, 0.0, 0.0),
        Vertex3DPolyline::from_xyz(10.0, 10.0, 0.0),
    ];

    let pts = sample_entity_xy(&EntityType::Polyline3D(pl));
    let seam = [10.0, 0.0];
    assert_eq!(
        pts.iter().filter(|p| **p == seam).count(),
        1,
        "the shared vertex appears once, not twice: {pts:?}"
    );
}

#[test]
fn preview_sampling_skips_edge_on_polyline2d_arc_segments() {
    use document::entities::{Polyline2D, Vertex2D};

    let mut pl = Polyline2D::new();
    let mut start = Vertex2D::new(Vector3::new(0.0, 0.0, 0.0));
    start.bulge = 1.0;
    pl.vertices = vec![start, Vertex2D::new(Vector3::new(2.0, 0.0, 0.0))];
    pl.normal = Vector3::new(1.0, 0.0, 0.0);

    let pts = preview_sample_xy(&EntityType::Polyline2D(pl));
    assert!(
        pts.is_empty(),
        "edge-on arc segments are skipped instead of panicking: {pts:?}"
    );
}

#[test]
fn trim_circle_with_tangent_arcs_touching_at_endpoints() {
    use document::entities::{Arc as ArcEnt, Circle as CircleEnt};
    let mut c = CircleEnt::new();
    c.common.handle = Handle::new(1);
    c.center = Vector3::new(0.0, 0.0, 0.0);
    c.radius = 10.0;

    // Arc 1: center (20, 0), r = 10, from PI/2 to PI (endpoint at (10, 0))
    let mut a1 = ArcEnt::new();
    a1.common.handle = Handle::new(2);
    a1.center = Vector3::new(20.0, 0.0, 0.0);
    a1.radius = 10.0;
    a1.start_angle = std::f64::consts::FRAC_PI_2;
    a1.end_angle = std::f64::consts::PI;

    // Arc 2: center (-20, 0), r = 10, from 0 to PI/2 (endpoint at (-10, 0))
    let mut a2 = ArcEnt::new();
    a2.common.handle = Handle::new(3);
    a2.center = Vector3::new(-20.0, 0.0, 0.0);
    a2.radius = 10.0;
    a2.start_angle = 0.0;
    a2.end_angle = std::f64::consts::FRAC_PI_2;

    let all = vec![
        EntityType::Circle(c),
        EntityType::Arc(a1),
        EntityType::Arc(a2),
    ];
    let geos = build_geos(&all);

    // Click on top of the circle at (0, 10)
    let survivors = pick_trim_at(&all, &geos, Handle::new(1), 0.0, 10.0);
    assert!(
        survivors.is_some(),
        "trimming circle between two tangent arc endpoints must succeed"
    );
    let pieces = survivors.unwrap();
    assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
    if let EntityType::Arc(arc) = &pieces[0] {
        assert!((arc.radius - 10.0).abs() < 1e-6);
        let span = {
            let s = (arc.end_angle - arc.start_angle).rem_euclid(TAU);
            if s == 0.0 {
                TAU
            } else {
                s
            }
        };
        let mid_ang = arc.start_angle + span * 0.5;
        let mid_y = mid_ang.sin();
        assert!(
            mid_y < 0.0,
            "bottom half should survive when clicking top half"
        );
    } else {
        panic!("expected Arc survivor");
    }
}

#[test]
fn test_trim_circle_between_two_tangent_lines() {
    use document::entities::{Circle as CircleEnt, Line as LineEnt};
    let mut c = CircleEnt::new();
    c.common.handle = Handle::new(1);
    c.center = Vector3::new(0.0, 0.0, 0.0);
    c.radius = 10.0;

    // Line 1: tangent at top (0, 10), from (-20, 10) to (20, 10)
    let mut l1 = LineEnt::new();
    l1.common.handle = Handle::new(2);
    l1.start = Vector3::new(-20.0, 10.0, 0.0);
    l1.end = Vector3::new(20.0, 10.0, 0.0);

    // Line 2: tangent at bottom (0, -10), from (-20, -10) to (20, -10)
    let mut l2 = LineEnt::new();
    l2.common.handle = Handle::new(3);
    l2.start = Vector3::new(-20.0, -10.0, 0.0);
    l2.end = Vector3::new(20.0, -10.0, 0.0);

    let all = vec![
        EntityType::Circle(c),
        EntityType::Line(l1),
        EntityType::Line(l2),
    ];
    let geos = build_geos(&all);

    // Trim circle by clicking right side at (10, 0)
    let survivors = pick_trim_at(&all, &geos, Handle::new(1), 10.0, 0.0);
    assert!(
        survivors.is_some(),
        "trimming circle between two tangent lines must succeed"
    );
    let pieces = survivors.unwrap();
    assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
    let EntityType::Arc(arc) = &pieces[0] else {
        panic!("expected Arc")
    };

    let arc_start = glam::DVec3::new(
        arc.radius * arc.start_angle.cos(),
        arc.radius * arc.start_angle.sin(),
        0.0,
    );
    let arc_end = glam::DVec3::new(
        arc.radius * arc.end_angle.cos(),
        arc.radius * arc.end_angle.sin(),
        0.0,
    );

    println!("arc start: {:?}, end: {:?}", arc_start, arc_end);

    // Now trim line 1 by clicking at (10, 10)
    let l1_survivors = pick_trim_at(&all, &geos, Handle::new(2), 10.0, 10.0);
    assert!(l1_survivors.is_some(), "trimming line 1 must succeed");
    let l1_pieces = l1_survivors.unwrap();
    assert_eq!(l1_pieces.len(), 1);
    let EntityType::Line(trimmed_l1) = &l1_pieces[0] else {
        panic!("expected Line")
    };
    println!(
        "trimmed line 1 start: {:?}, end: {:?}",
        trimmed_l1.start, trimmed_l1.end
    );

    // Check distance between trimmed line 1 end and arc start
    let line_end = glam::DVec3::new(trimmed_l1.end.x, trimmed_l1.end.y, trimmed_l1.end.z);
    let diff = (line_end - arc_start).length();
    println!(
        "Distance between line endpoint and arc endpoint: {:e}",
        diff
    );

    // Now test an angled / non-axis-aligned case from external point
    let c_center = glam::DVec3::new(123.45, 67.89, 0.0);
    let c_radius = 25.0;
    let p_ext1 = glam::DVec3::new(200.0, 150.0, 0.0);
    let p_ext2 = glam::DVec3::new(50.0, -20.0, 0.0);

    let get_tangents = |p: glam::DVec3| {
        let k_curve = ocs_geom::geom2d::Curve::Circle(ocs_geom::geom2d::Circle {
            centre: [c_center.x, c_center.y],
            radius: c_radius,
        });
        let pts = ocs_geom::geom2d::tangent_from(&k_curve, [p.x, p.y]);
        (
            glam::DVec3::new(pts[0].point[0], pts[0].point[1], 0.0),
            glam::DVec3::new(pts[1].point[0], pts[1].point[1], 0.0),
        )
    };
    let t1 = get_tangents(p_ext1).0;
    let t2 = get_tangents(p_ext2).1;

    let mut c_rot = CircleEnt::new();
    c_rot.common.handle = Handle::new(10);
    c_rot.center = Vector3::new(c_center.x, c_center.y, c_center.z);
    c_rot.radius = c_radius;

    // Line 1 from p_ext1 extending PAST t1 to make it a boundary line crossing tangent
    let dir1 = (t1 - p_ext1).normalize();
    let mut l_rot1 = LineEnt::new();
    l_rot1.common.handle = Handle::new(11);
    l_rot1.start = Vector3::new(p_ext1.x, p_ext1.y, 0.0);
    l_rot1.end = Vector3::new((t1 + dir1 * 20.0).x, (t1 + dir1 * 20.0).y, 0.0);

    let dir2 = (t2 - p_ext2).normalize();
    let mut l_rot2 = LineEnt::new();
    l_rot2.common.handle = Handle::new(12);
    l_rot2.start = Vector3::new(p_ext2.x, p_ext2.y, 0.0);
    l_rot2.end = Vector3::new((t2 + dir2 * 20.0).x, (t2 + dir2 * 20.0).y, 0.0);

    let all_rot = vec![
        EntityType::Circle(c_rot.clone()),
        EntityType::Line(l_rot1.clone()),
        EntityType::Line(l_rot2.clone()),
    ];
    let geos_rot = build_geos(&all_rot);

    // Click circle between them
    let mid_click = c_center + (t1 + t2 - 2.0 * c_center).normalize() * c_radius;
    let rot_survivors = pick_trim_at(
        &all_rot,
        &geos_rot,
        Handle::new(10),
        mid_click.x,
        mid_click.y,
    );
    assert!(
        rot_survivors.is_some(),
        "trimming circle between angled tangent lines must succeed"
    );
    let pieces = rot_survivors.unwrap();
    assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
    let EntityType::Arc(a) = &pieces[0] else {
        panic!("expected Arc")
    };
    let a_start = c_center
        + glam::DVec3::new(
            a.radius * a.start_angle.cos(),
            a.radius * a.start_angle.sin(),
            0.0,
        );
    let a_end = c_center
        + glam::DVec3::new(
            a.radius * a.end_angle.cos(),
            a.radius * a.end_angle.sin(),
            0.0,
        );
    let err_t1 = (a_start - t1).length().min((a_end - t1).length());
    let err_t2 = (a_start - t2).length().min((a_end - t2).length());
    assert!(
        err_t1 < 1e-9,
        "cut point must land within grip tolerance of t1: got {err_t1:e}"
    );
    assert!(
        err_t2 < 1e-9,
        "cut point must land within grip tolerance of t2: got {err_t2:e}"
    );
}

#[test]
fn trim_ellipse_with_tangent_arcs_touching_at_endpoints() {
    use document::entities::{Arc as ArcEnt, Ellipse as EllipseEnt};
    let mut e = EllipseEnt::new();
    e.common.handle = Handle::new(10);
    e.center = Vector3::new(0.0, 0.0, 0.0);
    e.major_axis = Vector3::new(20.0, 0.0, 0.0);
    e.minor_axis_ratio = 0.5; // b = 10
    e.start_parameter = 0.0;
    e.end_parameter = std::f64::consts::TAU;

    // Arc 1 touches right vertex at (20, 0): center (30, 0), r = 10, from PI/2 to PI
    let mut a1 = ArcEnt::new();
    a1.common.handle = Handle::new(11);
    a1.center = Vector3::new(30.0, 0.0, 0.0);
    a1.radius = 10.0;
    a1.start_angle = std::f64::consts::FRAC_PI_2;
    a1.end_angle = std::f64::consts::PI;

    // Arc 2 touches left vertex at (-20, 0): center (-30, 0), r = 10, from 0 to PI/2
    let mut a2 = ArcEnt::new();
    a2.common.handle = Handle::new(12);
    a2.center = Vector3::new(-30.0, 0.0, 0.0);
    a2.radius = 10.0;
    a2.start_angle = 0.0;
    a2.end_angle = std::f64::consts::FRAC_PI_2;

    let all = vec![
        EntityType::Ellipse(e),
        EntityType::Arc(a1),
        EntityType::Arc(a2),
    ];
    let geos = build_geos(&all);

    // Click top of ellipse at (0, 10)
    let survivors = pick_trim_at(&all, &geos, Handle::new(10), 0.0, 10.0);
    assert!(
        survivors.is_some(),
        "trimming ellipse between two tangent arc endpoints must succeed"
    );
    let pieces = survivors.unwrap();
    assert_eq!(pieces.len(), 1, "expected 1 surviving ellipse piece");
    if let EntityType::Ellipse(ell) = &pieces[0] {
        assert!((ell.major_axis.x - 20.0).abs() < 1e-6);
    } else {
        panic!("expected Ellipse survivor");
    }
}

#[test]
fn trim_ellipse_selection_tracking_near_breakpoint() {
    use document::entities::{Ellipse as EllipseEnt, Line as LineEnt};
    use std::f64::consts::TAU;

    let mut e = EllipseEnt::new();
    e.common.handle = Handle::new(10);
    e.center = Vector3::new(0.0, 0.0, 0.0);
    e.major_axis = Vector3::new(20.0, 0.0, 0.0);
    e.minor_axis_ratio = 0.5; // a = 20, b = 10
    e.start_parameter = 0.0;
    e.end_parameter = TAU;

    // Cutting line 1 crossing at t = PI/4 (45 degrees parametric):
    // At t = PI/4: x = 20 * cos(PI/4) = 14.142, y = 10 * sin(PI/4) = 7.071
    // (Polar angle is atan2(7.071, 14.142) = 26.56 degrees!)
    let mut l1 = LineEnt::new();
    l1.common.handle = Handle::new(11);
    l1.start = Vector3::new(0.0, 0.0, 0.0);
    l1.end = Vector3::new(30.0, 15.0, 0.0); // passes through (14.142, 7.071)

    // Cutting line 2 crossing at t = 3*PI/4:
    let mut l2 = LineEnt::new();
    l2.common.handle = Handle::new(12);
    l2.start = Vector3::new(0.0, 0.0, 0.0);
    l2.end = Vector3::new(-30.0, 15.0, 0.0);

    let all = vec![
        EntityType::Ellipse(e),
        EntityType::Line(l1),
        EntityType::Line(l2),
    ];
    let geos = build_geos(&all);

    // Click 1: Click at t = 40 deg parametric (x = 20*cos(40°), y = 10*sin(40°))
    // Polar angle is atan(0.5 * tan(40°)) ≈ 22.7°.
    // This is before PI/4. Surviving piece should run between the cuts.
    let t_before = 40.0_f64.to_radians();
    let px_before = 20.0 * t_before.cos();
    let py_before = 10.0 * t_before.sin();
    let res_before = pick_trim_at(&all, &geos, Handle::new(10), px_before, py_before);
    assert!(res_before.is_some(), "trimming at 40 deg must succeed");
    let pieces_before = res_before.unwrap();
    assert_eq!(pieces_before.len(), 1);

    // Click 2: Click at t = 50 deg parametric (x = 20*cos(50°), y = 10*sin(50°))
    // Polar angle is atan(0.5 * tan(50°)) ≈ 30.77°.
    // Visually and parametrically, 50° is in the top segment (between PI/4 and 3*PI/4).
    // Without the fix, 30.77° polar angle would be < 45° parametric, selecting the WRONG segment!
    let t_after = 50.0_f64.to_radians();
    let px_after = 20.0 * t_after.cos();
    let py_after = 10.0 * t_after.sin();
    let res_after = pick_trim_at(&all, &geos, Handle::new(10), px_after, py_after);
    assert!(res_after.is_some(), "trimming at 50 deg must succeed");
    let pieces_after = res_after.unwrap();
    assert_eq!(pieces_after.len(), 1);

    // Click 3 & 4: Click slightly INSIDE and OUTSIDE the ellipse near 50 deg.
    // Even when displaced by 3 units (pick-box offset when zoomed out),
    // orthogonal projection must still correctly identify the top segment!
    let nx = 10.0 * t_after.cos();
    let ny = 20.0 * t_after.sin();
    let nlen = nx.hypot(ny);
    let (unx, uny) = (nx / nlen, ny / nlen);

    // Displaced outside:
    let res_out = pick_trim_at(
        &all,
        &geos,
        Handle::new(10),
        px_after + 3.0 * unx,
        py_after + 3.0 * uny,
    );
    assert!(
        res_out.is_some(),
        "trimming at displaced outside point must succeed"
    );

    // Displaced inside:
    let res_in = pick_trim_at(
        &all,
        &geos,
        Handle::new(10),
        px_after - 3.0 * unx,
        py_after - 3.0 * uny,
    );
    assert!(
        res_in.is_some(),
        "trimming at displaced inside point must succeed"
    );

    if let (
        EntityType::Ellipse(surv_after),
        EntityType::Ellipse(surv_out),
        EntityType::Ellipse(surv_in),
    ) = (&pieces_after[0], &res_out.unwrap()[0], &res_in.unwrap()[0])
    {
        assert!(
            (surv_out.start_parameter - surv_after.start_parameter).abs() < 1e-4,
            "displaced outside point should select the same segment as point on curve"
        );
        assert!(
            (surv_in.start_parameter - surv_after.start_parameter).abs() < 1e-4,
            "displaced inside point should select the same segment as point on curve"
        );
    }
}

#[test]
fn trim_arc_selection_tracking_near_start_endpoint() {
    use document::entities::{Arc as ArcEnt, Line as LineEnt};
    // Arc from 0.5 to 2.0 rad (~28.6° to 114.6°), radius 10.0
    let mut a = ArcEnt::new();
    a.common.handle = Handle::new(20);
    a.center = Vector3::new(0.0, 0.0, 0.0);
    a.radius = 10.0;
    a.start_angle = 0.5;
    a.end_angle = 2.0;

    // Boundary line crossing arc at midpoint ~ 1.25 rad
    let mid = 1.25_f64;
    let mut l = LineEnt::new();
    l.common.handle = Handle::new(21);
    l.start = Vector3::new(0.0, 0.0, 0.0);
    l.end = Vector3::new(20.0 * mid.cos(), 20.0 * mid.sin(), 0.0);

    let all = vec![EntityType::Arc(a), EntityType::Line(l)];
    let geos = build_geos(&all);

    // Hover slightly CW of start endpoint (e.g. angle = 0.49 rad, outside the arc by ~0.5 degree)
    let click_ang = 0.49_f64;
    let px = 10.0 * click_ang.cos();
    let py = 10.0 * click_ang.sin();
    let res = pick_trim_at(&all, &geos, Handle::new(20), px, py);
    assert!(res.is_some(), "trim near start endpoint must succeed");
    let pieces = res.unwrap();
    assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
    if let EntityType::Arc(surv) = &pieces[0] {
        // It must remove the start-half [0.5, 1.25] and keep the end-half [1.25, 2.0].
        // If the bug were present, it would snap to 1.0 (end-half removed, start-half kept).
        assert!(
            (surv.end_angle - 2.0).abs() < 0.05,
            "survivor should be the end-half [1.25, 2.0], got start={} end={}",
            surv.start_angle,
            surv.end_angle
        );
    } else {
        panic!("expected Arc survivor");
    }
}

#[test]
fn test_tangent_circles_or_arcs_trim() {
    use document::entities::{Arc as ArcEnt, Circle as CircleEnt};
    let mut c1 = CircleEnt::new();
    c1.common.handle = Handle::new(1);
    c1.center = Vector3::new(0.0, 0.0, 0.0);
    c1.radius = 10.0;

    let mut c2 = CircleEnt::new();
    c2.common.handle = Handle::new(2);
    c2.center = Vector3::new(20.0, 0.0, 0.0);
    c2.radius = 10.0;

    let all = vec![
        EntityType::Circle(c1.clone()),
        EntityType::Circle(c2.clone()),
    ];
    let geos = build_geos(&all);
    let curve1 = entity_curve_xy(&EntityType::Circle(c1.clone())).unwrap();
    let ts = cut_params(&curve1, Handle::new(1), &geos);
    assert_eq!(
        ts,
        vec![0.0],
        "circle1 tangent to circle2 must have cut at t=0"
    );

    // Arc1 touching circle2 tangently
    let mut a1 = ArcEnt::new();
    a1.common.handle = Handle::new(3);
    a1.center = Vector3::new(0.0, 0.0, 0.0);
    a1.radius = 10.0;
    a1.start_angle = -1.0;
    a1.end_angle = 1.0;

    let all_arc = vec![EntityType::Arc(a1.clone()), EntityType::Circle(c2.clone())];
    let geos_arc = build_geos(&all_arc);
    let curve_arc = entity_curve_xy(&EntityType::Arc(a1.clone())).unwrap();
    let ts_arc = cut_params(&curve_arc, Handle::new(3), &geos_arc);
    assert_eq!(
        ts_arc,
        vec![0.5],
        "arc1 tangent to circle2 at midpoint must have cut at t=0.5"
    );

    let trim_res = pick_trim_at(&all_arc, &geos_arc, Handle::new(3), 10.0, 5.0);
    assert!(
        trim_res.is_some(),
        "trimming arc1 at tangent circle2 must succeed"
    );
    let pieces = trim_res.unwrap();
    assert_eq!(
        pieces.len(),
        1,
        "trimming half of arc should leave 1 survivor piece"
    );

    // 1. External tangency: Test across multiple angles and slight epsilons
    for angle in [0.0f64, 0.5, 1.0, 1.57, 2.5, std::f64::consts::PI, 4.0, 5.0] {
        for eps in [-1e-8, 0.0, 1e-8] {
            let dist = 20.0 + eps;
            let dx = dist * angle.cos();
            let dy = dist * angle.sin();
            let mut c_off = CircleEnt::new();
            c_off.common.handle = Handle::new(4);
            c_off.center = Vector3::new(dx, dy, 0.0);
            c_off.radius = 10.0;
            let all_off = vec![EntityType::Circle(c1.clone()), EntityType::Circle(c_off)];
            let geos_off = build_geos(&all_off);
            let ts_off = cut_params(&curve1, Handle::new(1), &geos_off);
            assert_eq!(
                ts_off.len(),
                1,
                "must find exactly 1 tangency cut at angle {angle}, eps {eps}"
            );
            let expected_t = (angle / TAU).rem_euclid(1.0);
            let actual_t = ts_off[0];
            let diff = (actual_t - expected_t).abs();
            let cyclic_diff = diff.min(1.0 - diff);
            assert!(
                cyclic_diff < 1e-5,
                "tangency t must match angle: got {actual_t}, expected {expected_t}"
            );
        }
    }

    // 2. Internal tangency: c1 (r=10) and c_inner (r=4) at (6, 0)
    let mut c_inner = CircleEnt::new();
    c_inner.common.handle = Handle::new(5);
    c_inner.center = Vector3::new(6.0, 0.0, 0.0);
    c_inner.radius = 4.0;
    let all_int = vec![
        EntityType::Circle(c1.clone()),
        EntityType::Circle(c_inner.clone()),
    ];
    let geos_int = build_geos(&all_int);
    let ts_int = cut_params(&curve1, Handle::new(1), &geos_int);
    assert_eq!(
        ts_int,
        vec![0.0],
        "internal tangency on c1 must have cut at t=0"
    );

    // And from c_inner's perspective (r=4 inside r=10):
    let curve_inner = entity_curve_xy(&EntityType::Circle(c_inner.clone())).unwrap();
    let ts_inner = cut_params(&curve_inner, Handle::new(5), &geos_int);
    assert_eq!(
        ts_inner,
        vec![0.0],
        "internal tangency on c_inner must have cut at t=0"
    );

    // 3. Two arcs touching tangently at (10, 0)
    // a_left: center (0, 0), radius 10, start=0, end=PI (passes through (10,0) at start t=0)
    let mut a_left = ArcEnt::new();
    a_left.common.handle = Handle::new(6);
    a_left.center = Vector3::new(0.0, 0.0, 0.0);
    a_left.radius = 10.0;
    a_left.start_angle = 0.0;
    a_left.end_angle = std::f64::consts::PI;

    // a_right: center (20, 0), radius 10, start=PI/2, end=3*PI/2 (passes through (10,0) at angle PI, midpoint t=0.5)
    let mut a_right = ArcEnt::new();
    a_right.common.handle = Handle::new(7);
    a_right.center = Vector3::new(20.0, 0.0, 0.0);
    a_right.radius = 10.0;
    a_right.start_angle = std::f64::consts::FRAC_PI_2;
    a_right.end_angle = 3.0 * std::f64::consts::FRAC_PI_2;

    let all_arcs = vec![
        EntityType::Arc(a_left.clone()),
        EntityType::Arc(a_right.clone()),
    ];
    let geos_arcs = build_geos(&all_arcs);
    let curve_right = entity_curve_xy(&EntityType::Arc(a_right.clone())).unwrap();
    let ts_right = cut_params(&curve_right, Handle::new(7), &geos_arcs);
    assert_eq!(
        ts_right,
        vec![0.5],
        "a_right must be cut at midpoint t=0.5 by tangent a_left"
    );

    let trim_right = pick_trim_at(&all_arcs, &geos_arcs, Handle::new(7), 15.0, 8.0);
    assert!(
        trim_right.is_some(),
        "trimming a_right at tangent a_left must succeed"
    );

    // 4. Circle trimmed by 2 tangent circles
    // c1 at (0, 0) r=10. c_right at (20, 0) r=10. c_left at (-20, 0) r=10.
    let mut c_left = CircleEnt::new();
    c_left.common.handle = Handle::new(8);
    c_left.center = Vector3::new(-20.0, 0.0, 0.0);
    c_left.radius = 10.0;
    let all_3c = vec![
        EntityType::Circle(c1.clone()),
        EntityType::Circle(c2.clone()),
        EntityType::Circle(c_left.clone()),
    ];
    let geos_3c = build_geos(&all_3c);
    let ts_3c = cut_params(&curve1, Handle::new(1), &geos_3c);
    assert_eq!(
        ts_3c.len(),
        2,
        "c1 touched by 2 tangent circles must have 2 cut points"
    );
    assert_eq!(ts_3c, vec![0.0, 0.5]);

    let trim_3c = pick_trim_at(&all_3c, &geos_3c, Handle::new(1), 0.0, 10.0);
    assert!(
        trim_3c.is_some(),
        "c1 with 2 tangent circles must be trimmable"
    );
    if let Some(surv) = trim_3c {
        assert_eq!(surv.len(), 1, "trimming c1 should leave 1 arc survivor");
    }

    // 5. Extending an arc to meet a tangent circle
    // a_short: center (0, 0), radius 10, start=0.5, end=1.5 (span ~ 1.0 rad)
    // Tangent circle c2 is at (20, 0), tangent point is at (10, 0) which is angle 0.
    // Clicking near start extends start CW towards 0.0.
    let mut a_short = ArcEnt::new();
    a_short.common.handle = Handle::new(9);
    a_short.center = Vector3::new(0.0, 0.0, 0.0);
    a_short.radius = 10.0;
    a_short.start_angle = 0.5;
    a_short.end_angle = 1.5;
    let all_ext = vec![
        EntityType::Arc(a_short.clone()),
        EntityType::Circle(c2.clone()),
    ];
    let geos_ext = build_geos(&all_ext);
    let ext_res = pick_extend_at(
        &all_ext,
        &geos_ext,
        Handle::new(9),
        10.0 * 0.6_f64.cos(),
        10.0 * 0.6_f64.sin(),
    );
    assert!(
        ext_res.is_some(),
        "extending arc towards tangent circle must succeed"
    );
    if let Some(EntityType::Arc(ext_arc)) = ext_res {
        let start_norm = norm(ext_arc.start_angle);
        assert!(
            start_norm < 1e-5 || (TAU - start_norm) < 1e-5,
            "extended arc start must reach 0.0 (tangent point)"
        );
    }
}

#[test]
fn test_skinny_ellipse_crossing_arc() {
    use document::entities::{Arc as ArcEnt, Ellipse as EllipseEnt};
    let mut e = EllipseEnt::new();
    e.common.handle = Handle::new(100);
    e.center = Vector3::new(0.0, 0.0, 0.0);
    e.major_axis = Vector3::new(100.0, 0.0, 0.0);
    e.minor_axis_ratio = 0.05; // b = 5, very skinny!
    e.start_parameter = 0.0;
    e.end_parameter = std::f64::consts::TAU;

    // Test arcs that physically cross the ellipse near its ends (x = 80, 90, 95, 98, 99)
    for &r in &[80.0, 90.0, 95.0, 98.0, 99.0] {
        let mut a = ArcEnt::new();
        a.common.handle = Handle::new(101);
        a.center = Vector3::new(0.0, 0.0, 0.0);
        a.radius = r;
        a.start_angle = -0.3;
        a.end_angle = 0.3;

        let all = vec![EntityType::Ellipse(e.clone()), EntityType::Arc(a)];
        let geos = build_geos(&all);

        // Trim the ellipse tip beyond the arc (near x = 99.5, y = 0)
        let ell_res = pick_trim_at(&all, &geos, Handle::new(100), 99.9, 0.0);
        // Trim the arc near y = 0 (x = r, y = 0)
        let arc_res = pick_trim_at(&all, &geos, Handle::new(101), r, 0.0);
        assert!(
            ell_res.is_some(),
            "ellipse trim should succeed for crossing arc at r={r}"
        );
        assert!(
            arc_res.is_some(),
            "arc trim should succeed for crossing arc at r={r}"
        );
    }

    // Test off-axis arc crossing near the tip (center at (96, 20), radius 20, passing near (96, 0))
    {
        let mut a = ArcEnt::new();
        a.common.handle = Handle::new(102);
        a.center = Vector3::new(96.0, 20.0, 0.0);
        a.radius = 20.0;
        // Angle around -PI/2 (bottom point is (96, 0))
        a.start_angle = -std::f64::consts::FRAC_PI_2 - 0.4;
        a.end_angle = -std::f64::consts::FRAC_PI_2 + 0.4;

        let all = vec![EntityType::Ellipse(e.clone()), EntityType::Arc(a)];
        let geos = build_geos(&all);

        let ell_res = pick_trim_at(&all, &geos, Handle::new(100), 99.0, 0.0);
        let arc_res = pick_trim_at(&all, &geos, Handle::new(102), 96.0, 0.0);
        assert!(
            ell_res.is_some(),
            "ellipse trim should succeed with off-axis crossing arc"
        );
        assert!(
            arc_res.is_some(),
            "arc trim should succeed with off-axis crossing arc"
        );
    }
}

#[test]
fn test_trim_line_tangent_to_arc() {
    use document::entities::{Arc as ArcEnt, Line as LineEnt};
    let mut l1 = LineEnt::new();
    l1.common.handle = Handle::new(1);
    l1.start = Vector3::new(-10.0, 0.0, 0.0);
    l1.end = Vector3::new(10.0, 0.0, 0.0);

    let mut a = ArcEnt::new();
    a.common.handle = Handle::new(2);
    a.center = Vector3::new(0.0, 5.0, 0.0);
    a.radius = 5.0;
    a.start_angle = -std::f64::consts::FRAC_PI_2;
    a.end_angle = std::f64::consts::PI;

    let all = vec![EntityType::Line(l1.clone()), EntityType::Arc(a.clone())];
    let geos = build_geos(&all);
    let curve_line = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
    let ts_line = cut_params(&curve_line, Handle::new(1), &geos);
    assert!(
        !ts_line.is_empty(),
        "line must have cut point where tangent arc touches it: ts={:?}",
        ts_line
    );

    let curve_arc = entity_curve_xy(&EntityType::Arc(a.clone())).unwrap();
    let ts_arc = cut_params(&curve_arc, Handle::new(2), &geos);
    println!("ts_arc: {:?}", ts_arc);
}

#[test]
fn test_trim_arc_tangent_to_two_lines() {
    use crate::command::{TangentObject, WorkingPlane};
    use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
    use document::entities::{Arc as CadArc, Line as LineEnt};

    let mut l1 = LineEnt::new();
    l1.common.handle = Handle::new(1);
    l1.start = Vector3::new(-10.0, 0.0, 0.0);
    l1.end = Vector3::new(10.0, 0.0, 0.0);

    let mut l2 = LineEnt::new();
    l2.common.handle = Handle::new(2);
    l2.start = Vector3::new(0.0, -10.0, 0.0);
    l2.end = Vector3::new(0.0, 10.0, 0.0);

    let t1 = TangentObject::Line {
        p1: DVec3::new(-10.0, 0.0, 0.0),
        p2: DVec3::new(10.0, 0.0, 0.0),
    };
    let t2 = TangentObject::Line {
        p1: DVec3::new(0.0, -10.0, 0.0),
        p2: DVec3::new(0.0, 10.0, 0.0),
    };

    // Middle point: (1 - sqrt(0.5), 1 - sqrt(0.5)) -> radius ~ 1
    let mid = DVec3::new(1.0 - (0.5_f64).sqrt(), 1.0 - (0.5_f64).sqrt(), 0.0);
    let sol = solve_arc_3_inputs(
        ArcPick::Tangent {
            target: t1,
            hit: DVec3::new(2.0, 0.0, 0.0),
        },
        ArcPick::Point(mid),
        ArcPick::Tangent {
            target: t2,
            hit: DVec3::new(0.0, 2.0, 0.0),
        },
        WorkingPlane::default(),
    );

    let (center, radius, sa, ea) = sol.expect("must solve arc tangent to two lines");
    let mut arc = CadArc::new();
    arc.common.handle = Handle::new(3);
    arc.center = Vector3::new(center.x, center.y, center.z);
    arc.radius = radius;
    arc.start_angle = sa;
    arc.end_angle = ea;

    let all = vec![
        EntityType::Line(l1.clone()),
        EntityType::Line(l2.clone()),
        EntityType::Arc(arc.clone()),
    ];
    let geos = build_geos(&all);

    let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
    let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);
    println!("ts_l1: {:?}", ts_l1);

    let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
    let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);
    println!("ts_l2: {:?}", ts_l2);

    let curve_arc = entity_curve_xy(&EntityType::Arc(arc.clone())).unwrap();
    let ts_arc = cut_params(&curve_arc, Handle::new(3), &geos);
    println!("ts_arc: {:?}", ts_arc);

    assert!(!ts_l1.is_empty(), "Line 1 must be cut: ts_l1={:?}", ts_l1);
    assert!(!ts_l2.is_empty(), "Line 2 must be cut: ts_l2={:?}", ts_l2);

    // Test trimming line 1 arm between (1, 0) and (10, 0)
    let trim_l1 = pick_trim_at(&all, &geos, Handle::new(1), 5.0, 0.0);
    println!("trim_l1: {:?}", trim_l1);
    assert!(trim_l1.is_some(), "trimming line 1 arm must succeed");

    // Test trimming line 2 arm between (0, 1) and (0, 10)
    let trim_l2 = pick_trim_at(&all, &geos, Handle::new(2), 0.0, 5.0);
    println!("trim_l2: {:?}", trim_l2);
    assert!(trim_l2.is_some(), "trimming line 2 arm must succeed");

    // Test trimming the arc
    let trim_arc = pick_trim_at(&all, &geos, Handle::new(3), mid.x, mid.y);
    println!("trim_arc: {:?}", trim_arc);
}

#[test]
fn test_trim_arc_tangent_angled_lines() {
    use crate::command::{TangentObject, WorkingPlane};
    use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
    use document::entities::{Arc as CadArc, Line as LineEnt};

    // Two lines meeting at (0, 0): one along (cos(20 deg), sin(20 deg)), other along (cos(80 deg), sin(80 deg))
    let a1 = 20.0_f64.to_radians();
    let a2 = 80.0_f64.to_radians();
    let dir1 = DVec3::new(a1.cos(), a1.sin(), 0.0);
    let dir2 = DVec3::new(a2.cos(), a2.sin(), 0.0);

    let mut l1 = LineEnt::new();
    l1.common.handle = Handle::new(1);
    l1.start = Vector3::new(-10.0 * dir1.x, -10.0 * dir1.y, 0.0);
    l1.end = Vector3::new(50.0 * dir1.x, 50.0 * dir1.y, 0.0);

    let mut l2 = LineEnt::new();
    l2.common.handle = Handle::new(2);
    l2.start = Vector3::new(-10.0 * dir2.x, -10.0 * dir2.y, 0.0);
    l2.end = Vector3::new(50.0 * dir2.x, 50.0 * dir2.y, 0.0);

    let t1 = TangentObject::Line {
        p1: DVec3::new(l1.start.x, l1.start.y, 0.0),
        p2: DVec3::new(l1.end.x, l1.end.y, 0.0),
    };
    let t2 = TangentObject::Line {
        p1: DVec3::new(l2.start.x, l2.start.y, 0.0),
        p2: DVec3::new(l2.end.x, l2.end.y, 0.0),
    };

    // Midpoint between points at distance 20 on the bisector
    let bisector_angle = (a1 + a2) * 0.5;
    let mid = DVec3::new(
        15.0 * bisector_angle.cos(),
        15.0 * bisector_angle.sin(),
        0.0,
    );

    let sol = solve_arc_3_inputs(
        ArcPick::Tangent {
            target: t1,
            hit: DVec3::new(20.0 * dir1.x, 20.0 * dir1.y, 0.0),
        },
        ArcPick::Point(mid),
        ArcPick::Tangent {
            target: t2,
            hit: DVec3::new(20.0 * dir2.x, 20.0 * dir2.y, 0.0),
        },
        WorkingPlane::default(),
    );

    let (center, radius, sa, ea) = sol.expect("must solve arc tangent to two angled lines");
    let mut arc = CadArc::new();
    arc.common.handle = Handle::new(3);
    arc.center = Vector3::new(center.x, center.y, center.z);
    arc.radius = radius;
    arc.start_angle = sa;
    arc.end_angle = ea;

    let all = vec![
        EntityType::Line(l1.clone()),
        EntityType::Line(l2.clone()),
        EntityType::Arc(arc.clone()),
    ];
    let geos = build_geos(&all);

    let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
    let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);
    println!("angled ts_l1: {:?}", ts_l1);

    let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
    let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);
    println!("angled ts_l2: {:?}", ts_l2);

    // We expect l1 to have at least 2 cuts: the intersection with l2 at (0,0) and the tangent point with arc!
    assert!(
        ts_l1.len() >= 2,
        "Line 1 must have intersection and tangent cut: got {:?}",
        ts_l1
    );
    assert!(
        ts_l2.len() >= 2,
        "Line 2 must have intersection and tangent cut: got {:?}",
        ts_l2
    );

    // Trimming l1 arm past the tangent point
    let click_l1 = 40.0 * dir1;
    let trim_l1 = pick_trim_at(&all, &geos, Handle::new(1), click_l1.x, click_l1.y);
    println!("angled trim_l1: {:?}", trim_l1);
    assert!(trim_l1.is_some(), "trimming line 1 must succeed");
}

#[test]
fn test_arc_tangent_line_precision_sweep() {
    use crate::command::{TangentObject, WorkingPlane};
    use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
    use document::entities::{Arc as CadArc, Line as LineEnt};

    let mut max_dist = 0.0_f64;
    let mut failed_cuts = 0;

    for i in 1..36 {
        let angle_deg = i as f64 * 5.0; // 5 to 175 degrees
        let a2 = angle_deg.to_radians();

        let dir1 = DVec3::new(1.0, 0.0, 0.0);
        let dir2 = DVec3::new(a2.cos(), a2.sin(), 0.0);

        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(1);
        l1.start = Vector3::new(-50.0, 0.0, 0.0);
        l1.end = Vector3::new(150.0, 0.0, 0.0);

        let mut l2 = LineEnt::new();
        l2.common.handle = Handle::new(2);
        l2.start = Vector3::new(-50.0 * dir2.x, -50.0 * dir2.y, 0.0);
        l2.end = Vector3::new(150.0 * dir2.x, 150.0 * dir2.y, 0.0);

        let t1 = TangentObject::Line {
            p1: DVec3::new(l1.start.x, l1.start.y, 0.0),
            p2: DVec3::new(l1.end.x, l1.end.y, 0.0),
        };
        let t2 = TangentObject::Line {
            p1: DVec3::new(l2.start.x, l2.start.y, 0.0),
            p2: DVec3::new(l2.end.x, l2.end.y, 0.0),
        };

        let bisector = (dir1 + dir2).normalize();
        let mid = bisector * 30.0;

        let sol = solve_arc_3_inputs(
            ArcPick::Tangent {
                target: t1,
                hit: DVec3::new(50.0, 0.0, 0.0),
            },
            ArcPick::Point(mid),
            ArcPick::Tangent {
                target: t2,
                hit: dir2 * 50.0,
            },
            WorkingPlane::default(),
        );

        if let Some((center, radius, sa, ea)) = sol {
            let mut arc = CadArc::new();
            arc.common.handle = Handle::new(3);
            arc.center = Vector3::new(center.x, center.y, center.z);
            arc.radius = radius;
            arc.start_angle = sa;
            arc.end_angle = ea;

            let all = vec![
                EntityType::Line(l1.clone()),
                EntityType::Line(l2.clone()),
                EntityType::Arc(arc.clone()),
            ];
            let geos = build_geos(&all);

            let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
            let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);

            let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
            let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);

            if ts_l1.len() < 2 || ts_l2.len() < 2 {
                failed_cuts += 1;
                println!(
                    "FAILED at angle {angle_deg} deg: ts_l1={:?}, ts_l2={:?}",
                    ts_l1, ts_l2
                );
            }

            // Check distance of arc endpoints to line
            let ep1 = [center.x + radius * sa.cos(), center.y + radius * sa.sin()];
            let d1 = (ep1[1] - 0.0).abs(); // distance to l1 (y=0)
            if d1 > max_dist {
                max_dist = d1;
            }
        }
    }
    println!("Sweep results: failed_cuts={failed_cuts}, max_dist={max_dist:e}");
    assert_eq!(failed_cuts, 0, "No cuts should fail across angle sweep");
}
