//! How an entity lays its dash pattern out: "A"-aligned per run, or fitted
//! around a closed path. The pixels of each layout are in `dash_gpu_tests`.

use std::collections::HashSet;

use document::entities::{
    Circle, Ellipse, Insert, Line, LwPolyline, LwVertex, MLine, Polyline2D, Polyline3D,
    PolylineFlags, Vertex2D,
};
use document::tables::linetype::LineTypeElement;
use document::tables::{BlockRecord, Layer, LineType};
use document::types::{Vector2, Vector3};
use document::{Document, EntityType, Handle};

use crate::scene::model::wire_model::{DashClosure, WireModel};
use crate::scene::pipeline::wire_gpu::{aligned_end_dash, closed_dash_periods, wire_distances};

/// A linetype of a 20 dash and a 20 gap, a 40 period.
fn dashed_document() -> Document {
    let mut document = Document::new();
    let mut dashed = LineType::new("D");
    dashed.handle = document.allocate_handle();
    dashed.add_element(LineTypeElement::dash(20.0));
    dashed.add_element(LineTypeElement::space(20.0));
    dashed.pattern_length = 40.0;
    document.line_types.add(dashed).unwrap();
    document
}

fn lw(vertices: &[(f64, f64, f64)], closed: bool, plinegen: bool) -> LwPolyline {
    let mut polyline = LwPolyline::default();
    polyline.vertices = vertices
        .iter()
        .map(|&(x, y, bulge)| {
            let mut vertex = LwVertex::new(Vector2::new(x, y));
            vertex.bulge = bulge;
            vertex
        })
        .collect();
    polyline.is_closed = closed;
    polyline.plinegen = plinegen;
    polyline
}

fn heavy(points: &[(f64, f64)], closed: bool, plinegen: bool) -> Polyline2D {
    let mut polyline = Polyline2D::new();
    for &(x, y) in points {
        polyline.add_vertex(Vertex2D::new(Vector3::new(x, y, 0.0)));
    }
    if closed {
        polyline.close();
    }
    if plinegen {
        polyline.flags = PolylineFlags::from_bits(
            polyline.flags.bits() | PolylineFlags::LINETYPE_CONTINUOUS.bits(),
        );
    }
    polyline
}

fn tessellated(document: &Document, entity: EntityType) -> Vec<WireModel> {
    crate::scene::tessellate_entity(
        document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
        None,
        &entity,
        None,
        None,
        None,
        None,
        &HashSet::default(),
    )
}

/// The one drawn dashed wire of `entity`, dashed with "D".
fn dashed_wire(mut entity: EntityType) -> WireModel {
    let document = dashed_document();
    entity.common_mut().linetype = "D".to_string();
    let mut wires: Vec<_> = tessellated(&document, entity)
        .into_iter()
        .filter(|wire| !wire.snap_only && wire.pattern_length > 0.0)
        .collect();
    assert_eq!(wires.len(), 1, "one dashed wire expected");
    wires.remove(0)
}

const SQUARE: [(f64, f64, f64); 4] = [
    (0.0, 0.0, 0.0),
    (110.0, 0.0, 0.0),
    (110.0, 110.0, 0.0),
    (0.0, 110.0, 0.0),
];

#[test]
fn a_closed_path_rounds_its_periods_down_on_a_tie_and_keeps_two() {
    for (length, periods) in [
        (295.3, 7.0),
        (377.0, 9.0),
        (420.0, 10.0),
        (140.0, 3.0),
        (100.0, 2.0),
        (60.0, 2.0),
        (40.0, 2.0),
        (420.5, 11.0),
    ] {
        assert_eq!(
            closed_dash_periods(length, 40.0),
            periods,
            "length {length}"
        );
    }
}

#[test]
fn a_closed_path_of_one_period_still_shows_dashes() {
    use crate::scene::pipeline::wire_gpu::closed_dash_solid;
    // A circle's chords come to 39.97 when its arc is 40.
    assert!(!closed_dash_solid(39.97, 40.0));
    assert!(!closed_dash_solid(40.0, 40.0));
    assert!(closed_dash_solid(39.6, 40.0));
}

#[test]
fn a_circle_fits_its_pattern_to_its_length() {
    let wire = dashed_wire(EntityType::Circle(Circle::from_center_radius(
        Vector3::ZERO,
        40.0,
    )));
    assert_eq!(wire.dash_closure, DashClosure::Curve);
    let dashes = wire_distances(&wire);
    let length = dashes.dists.last().copied().unwrap();
    // The length of its chords, a little under the arc's 251.33.
    assert!((length - 251.3).abs() < 0.3, "{length}");
    assert!(dashes.run_totals[..wire.points.len() - 1]
        .iter()
        .all(|&total| total == -length));
    assert_eq!(dashes.dists[0], 0.0);
}

#[test]
fn only_a_full_ellipse_fits_its_pattern() {
    let mut full = Ellipse::default();
    full.major_axis = Vector3::new(70.0, 0.0, 0.0);
    full.minor_axis_ratio = 0.6;
    full.start_parameter = 0.0;
    full.end_parameter = std::f64::consts::TAU;
    let mut partial = full.clone();
    partial.start_parameter = 0.5;
    partial.end_parameter = 4.0;

    let full = dashed_wire(EntityType::Ellipse(full));
    assert_eq!(full.dash_closure, DashClosure::Curve);
    assert!(
        !crate::scene::pipeline::ellipse_gpu::can_extract_ellipse_instances(&full),
        "the analytic ellipse does not lay dashes out"
    );
    let partial = dashed_wire(EntityType::Ellipse(partial));
    assert_eq!(partial.dash_closure, DashClosure::Open);
    assert!(!crate::scene::pipeline::ellipse_gpu::can_extract_ellipse_instances(&partial));
    assert!(wire_distances(&partial).run_totals[0] > 0.0);
}

#[test]
fn each_segment_of_a_polyline_without_plinegen_aligns_on_its_own() {
    let wire = dashed_wire(EntityType::LwPolyline(lw(&SQUARE, true, false)));
    assert_eq!(wire.dash_closure, DashClosure::Open);
    let dashes = wire_distances(&wire);
    let runs: Vec<(f32, f32)> = (0..wire.points.len() - 1)
        .filter(|&i| wire.points[i][0].is_finite() && wire.points[i + 1][0].is_finite())
        .map(|i| (dashes.dists[i], dashes.run_totals[i]))
        .collect();
    assert_eq!(runs, vec![(0.0, 110.0); 4]);
    // k = 2: (110 - 80 + 20) / 2.
    assert_eq!(aligned_end_dash(110.0, 40.0, 20.0), 25.0);
}

#[test]
fn a_closed_polyline_with_plinegen_starts_its_pattern_at_the_second_vertex() {
    let wire = dashed_wire(EntityType::LwPolyline(lw(&SQUARE, true, true)));
    assert_eq!(wire.dash_closure, DashClosure::Polyline { start: 1 });
    let dashes = wire_distances(&wire);
    assert_eq!(dashes.dists[1], 0.0);
    assert_eq!(dashes.dists[0], -110.0);
    assert_eq!(dashes.run_totals[0], -440.0);
}

#[test]
fn an_arc_first_polyline_starts_at_its_second_vertex_too() {
    let vertices = [
        (0.0, 0.0, 1.0),
        (110.0, 0.0, 0.0),
        (110.0, 110.0, 0.0),
        (0.0, 110.0, 0.0),
    ];
    let wire = dashed_wire(EntityType::LwPolyline(lw(&vertices, true, true)));
    let DashClosure::Polyline { start } = wire.dash_closure else {
        panic!("{:?}", wire.dash_closure);
    };
    assert!(start > 1, "the arc's samples come first");
    let second = wire.points[start as usize];
    assert!(
        (second[0] - 110.0).abs() < 1e-3 && second[1].abs() < 1e-3,
        "{second:?}"
    );
    assert_eq!(wire_distances(&wire).dists[start as usize], 0.0);
}

#[test]
fn an_open_polyline_with_plinegen_aligns_on_its_whole_length() {
    let wire = dashed_wire(EntityType::LwPolyline(lw(&SQUARE, false, true)));
    assert_eq!(wire.dash_closure, DashClosure::Open);
    assert_eq!(wire_distances(&wire).run_totals[0], 330.0);
}

#[test]
fn a_dashed_polyline_with_arcs_is_one_wire_and_a_solid_one_stays_split() {
    let vertices = [(0.0, 0.0, 1.0), (110.0, 0.0, 0.0), (110.0, 110.0, 0.0)];
    let wire = dashed_wire(EntityType::LwPolyline(lw(&vertices, false, true)));
    assert!(wire.plinegen);
    assert!(!wire.points.iter().any(|p| p[0].is_nan()));

    let document = dashed_document();
    let solid = tessellated(
        &document,
        EntityType::LwPolyline(lw(&vertices, false, true)),
    );
    assert!(solid.iter().filter(|wire| !wire.points.is_empty()).count() > 1);
}

#[test]
fn a_heavy_polyline_lays_its_dashes_out_like_a_light_one() {
    let points = [(0.0, 0.0), (110.0, 0.0), (110.0, 110.0), (0.0, 110.0)];

    let closed_off = dashed_wire(EntityType::Polyline2D(heavy(&points, true, false)));
    assert_eq!(closed_off.dash_closure, DashClosure::Open);
    assert_eq!(wire_distances(&closed_off).run_totals[0], 110.0);

    let open_off = dashed_wire(EntityType::Polyline2D(heavy(&points, false, false)));
    assert_eq!(wire_distances(&open_off).run_totals[0], 110.0);

    let closed_gen = dashed_wire(EntityType::Polyline2D(heavy(&points, true, true)));
    assert_eq!(closed_gen.dash_closure, DashClosure::Polyline { start: 1 });
    assert_eq!(wire_distances(&closed_gen).run_totals[0], -440.0);

    let open_gen = dashed_wire(EntityType::Polyline2D(heavy(&points, false, true)));
    assert_eq!(wire_distances(&open_gen).run_totals[0], 330.0);
}

#[test]
fn a_heavy_polyline_without_plinegen_is_solid_when_its_vertices_crowd_the_pattern() {
    // A spline-fit polyline: the samples are vertices, each shorter than a period.
    let samples: Vec<(f64, f64)> = (0..=40)
        .map(|i| {
            let x = i as f64 * 4.0;
            (x, (x * 0.05).sin() * 30.0)
        })
        .collect();
    let wire = dashed_wire(EntityType::Polyline2D(heavy(&samples, false, false)));
    let dashes = wire_distances(&wire);
    for index in 0..wire.points.len() - 1 {
        if wire.points[index][0].is_finite() && wire.points[index + 1][0].is_finite() {
            let total = dashes.run_totals[index];
            assert!(total < 40.0);
            assert_eq!(aligned_end_dash(total, 40.0, 20.0), total);
        }
    }
}

#[test]
fn a_three_d_polyline_stays_solid() {
    let mut document = dashed_document();
    let mut polyline = Polyline3D::from_points(vec![
        Vector3::ZERO,
        Vector3::new(100.0, 0.0, 0.0),
        Vector3::new(100.0, 100.0, 0.0),
    ]);
    polyline.common.linetype = "D".to_string();
    let entity = EntityType::Polyline3D(polyline);
    let handle: Handle = document.add_entity(entity.clone()).unwrap();
    let entity = document.get_entity(handle).unwrap().clone();
    assert!(tessellated(&document, entity)
        .iter()
        .all(|wire| wire.pattern_length == 0.0));
}

#[test]
fn a_run_total_survives_its_two_sixteen_bit_slots() {
    for total in [0.0_f32, 110.0, -440.0, 1234.5678, f32::MIN_POSITIVE, 3.0e7] {
        let [low, high] = crate::scene::pipeline::wire_gpu::run_total_slots(total);
        // `dash_run_total` in dash_layout.wgsl, from the sign-extended attribute.
        let bits = ((low as i32 as u32) & 0xffff) | ((high as i32 as u32) << 16);
        assert_eq!(f32::from_bits(bits), total);
    }
}

#[test]
fn stations_win_over_a_closure() {
    let mut wire = WireModel::solid(
        "1".to_string(),
        vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
        [1.0; 4],
        false,
    );
    wire.pattern_length = 4.0;
    wire.pattern[0] = 2.0;
    wire.pattern[1] = -2.0;
    wire.pattern_stations = vec![5.0, 15.0, 100.0];
    wire.dash_closure = DashClosure::Curve;
    let dashes = wire_distances(&wire);
    assert_eq!(dashes.dists[..2], [5.0, 15.0]);
    assert!(dashes.run_totals[0] >= 0.0);
}

// ── Inside a block ───────────────────────────────────────────────────────────

fn block_with(document: &mut Document, name: &str, children: Vec<EntityType>) {
    let mut block = BlockRecord::new(name);
    block.handle = document.allocate_handle();
    let owner = block.handle;
    document.block_records.add(block).unwrap();
    for mut child in children {
        child.common_mut().owner_handle = owner;
        document.add_entity(child).unwrap();
    }
}

fn dashed_block_wires(document: &Document, insert: Insert) -> Vec<WireModel> {
    tessellated(document, EntityType::Insert(insert))
        .into_iter()
        .filter(|wire| !wire.snap_only && wire.pattern_length > 0.0)
        .collect()
}

#[test]
fn a_block_child_closes_its_pattern_at_its_size_on_the_sheet() {
    let mut document = dashed_document();
    let mut circle = Circle::from_center_radius(Vector3::ZERO, 40.0);
    circle.common.linetype = "D".to_string();
    block_with(&mut document, "B", vec![EntityType::Circle(circle)]);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(0.5);
    insert.set_y_scale(0.5);
    insert.set_z_scale(0.5);
    let wires = dashed_block_wires(&document, insert);
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].dash_closure, DashClosure::Curve);
    let total = -wire_distances(&wires[0]).run_totals[0];
    assert!((total - 125.66).abs() < 0.1, "{total}");
}

#[test]
fn block_lines_batched_together_align_each_on_its_own() {
    let mut document = dashed_document();
    let children = [100.0, 250.0, 30.0]
        .into_iter()
        .enumerate()
        .map(|(row, length)| {
            let y = row as f64 * 10.0;
            let mut line =
                Line::from_points(Vector3::new(0.0, y, 0.0), Vector3::new(length, y, 0.0));
            line.common.linetype = "D".to_string();
            EntityType::Line(line)
        })
        .collect();
    block_with(&mut document, "B", children);
    let wires = dashed_block_wires(&document, Insert::new("B", Vector3::ZERO));
    let mut totals: Vec<f32> = wires
        .iter()
        .flat_map(|wire| {
            let dashes = wire_distances(wire);
            (0..wire.points.len() - 1)
                .filter(|&i| wire.points[i][0].is_finite() && wire.points[i + 1][0].is_finite())
                .map(|i| dashes.run_totals[i])
                .collect::<Vec<_>>()
        })
        .collect();
    totals.sort_by(f32::total_cmp);
    assert_eq!(totals, vec![30.0, 100.0, 250.0]);
}

#[test]
fn an_inherited_dash_walks_a_block_polyline_with_arcs_as_one_path() {
    let mut document = dashed_document();
    let vertices = [
        (0.0, 0.0, 1.0),
        (110.0, 0.0, 0.0),
        (110.0, 110.0, 0.0),
        (0.0, 110.0, 0.0),
    ];
    let mut child = lw(&vertices, true, true);
    child.common.linetype = "ByBlock".to_string();
    block_with(&mut document, "B", vec![EntityType::LwPolyline(child)]);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype = "D".to_string();
    let wires = dashed_block_wires(&document, insert);
    assert_eq!(wires.len(), 1, "one path, not one wire per arc");
    assert!(matches!(
        wires[0].dash_closure,
        DashClosure::Polyline { .. }
    ));
    assert!(wire_distances(&wires[0]).run_totals[0] < 0.0);
}

#[test]
fn a_mirrored_block_keeps_its_dashes_at_drawing_length() {
    let mut document = dashed_document();
    let mut circle = Circle::from_center_radius(Vector3::ZERO, 40.0);
    circle.common.linetype = "D".to_string();
    block_with(&mut document, "B", vec![EntityType::Circle(circle)]);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(-1.0);
    let wires = dashed_block_wires(&document, insert);
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].pattern_length, 40.0);
    assert_eq!(wires[0].dash_closure, DashClosure::Curve);
}

// ── MLINE elements ───────────────────────────────────────────────────────────

fn add_mline_style(document: &mut Document, name: &str, elements: &[(f64, &str)]) {
    use document::objects::{MLineStyle, MLineStyleElement, ObjectType};
    let mut style = MLineStyle::new(name);
    style.handle = document.allocate_handle();
    for &(offset, linetype) in elements {
        let mut element = MLineStyleElement::new(offset);
        element.linetype = linetype.to_string();
        style.add_element(element);
    }
    document
        .objects
        .insert(style.handle, ObjectType::MLineStyle(style));
}

fn mline(style: &str, points: &[(f64, f64)]) -> MLine {
    let points: Vec<Vector3> = points
        .iter()
        .map(|&(x, y)| Vector3::new(x, y, 0.0))
        .collect();
    let mut mline = MLine::from_points(&points);
    mline.style_name = style.to_string();
    mline.scale_factor = 1.0;
    mline
}

fn dashed_elements(document: &Document, entity: EntityType) -> Vec<WireModel> {
    tessellated(document, entity)
        .into_iter()
        .filter(|wire| !wire.snap_only && !wire.points.is_empty())
        .collect()
}

#[test]
fn each_mline_element_aligns_on_its_own_length() {
    let mut document = dashed_document();
    add_mline_style(&mut document, "ML", &[(0.0, "D"), (-10.0, "D")]);
    let bent = mline("ML", &[(0.0, 0.0), (500.0, 0.0), (500.0, 700.0)]);
    let elements = dashed_elements(&document, EntityType::MLine(bent));
    assert_eq!(elements.len(), 2);
    let mut totals: Vec<f32> = elements
        .iter()
        .map(|wire| wire_distances(wire).run_totals[0])
        .collect();
    totals.sort_by(f32::total_cmp);
    assert!(totals[1] - totals[0] > 15.0, "{totals:?}");
}

#[test]
fn a_closed_mline_aligns_rather_than_fits() {
    let mut document = dashed_document();
    add_mline_style(&mut document, "ML", &[(0.0, "D")]);
    let points: Vec<Vector3> = [(0.0, 0.0), (600.0, 0.0), (600.0, 600.0), (0.0, 600.0)]
        .iter()
        .map(|&(x, y)| Vector3::new(x, y, 0.0))
        .collect();
    let mut closed = MLine::closed_from_points(&points);
    closed.style_name = "ML".to_string();
    closed.scale_factor = 1.0;
    let elements = dashed_elements(&document, EntityType::MLine(closed));
    assert_eq!(elements[0].dash_closure, DashClosure::Open);
    assert!(wire_distances(&elements[0]).run_totals[0] > 0.0);
}

#[test]
fn a_bylayer_mline_element_follows_the_layer_and_ignores_the_entity() {
    let mut document = dashed_document();
    let mut layer = Layer::new("L");
    layer.line_type = "D".to_string();
    document.layers.add(layer).unwrap();
    add_mline_style(&mut document, "ML", &[(0.0, "ByLayer")]);
    let mut entity = mline("ML", &[(0.0, 0.0), (1000.0, 0.0)]);
    entity.common.layer = "L".to_string();
    entity.common.linetype = "Continuous".to_string();
    let elements = dashed_elements(&document, EntityType::MLine(entity));
    assert_eq!(elements[0].pattern_length, 40.0);
}

#[test]
fn a_byblock_mline_element_is_solid_outside_a_block() {
    let mut document = dashed_document();
    add_mline_style(&mut document, "ML", &[(0.0, "ByBlock")]);
    let mut entity = mline("ML", &[(0.0, 0.0), (1000.0, 0.0)]);
    entity.common.linetype = "D".to_string();
    let elements = dashed_elements(&document, EntityType::MLine(entity));
    assert_eq!(elements[0].pattern_length, 0.0);
}

#[test]
fn block_mline_elements_keep_or_inherit_their_linetypes() {
    let mut document = dashed_document();
    let mut long = LineType::new("LONG");
    long.handle = document.allocate_handle();
    long.add_element(LineTypeElement::dash(70.0));
    long.add_element(LineTypeElement::space(30.0));
    long.pattern_length = 100.0;
    document.line_types.add(long).unwrap();
    add_mline_style(&mut document, "OWN", &[(0.0, "D")]);
    add_mline_style(&mut document, "BB", &[(0.0, "ByBlock")]);
    for (block, style) in [("B_OWN", "OWN"), ("B_BB", "BB")] {
        block_with(
            &mut document,
            block,
            vec![EntityType::MLine(mline(
                style,
                &[(0.0, 0.0), (1000.0, 0.0)],
            ))],
        );
    }
    for (block, expected) in [("B_OWN", 40.0), ("B_BB", 100.0)] {
        let mut insert = Insert::new(block, Vector3::ZERO);
        insert.common.linetype = "LONG".to_string();
        let wires = dashed_block_wires(&document, insert);
        assert_eq!(wires.len(), 1, "{block}");
        assert_eq!(wires[0].pattern_length, expected, "{block}");
    }
}
