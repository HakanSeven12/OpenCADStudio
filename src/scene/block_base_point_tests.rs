//! Base-point reproduction. Tests only; no production code changes.
//!
//! Convention under test:
//!   world = insert_point + Rot(rotation) * Scale * (P - block.base_point)
//!
//! Block "BP_TEST": base (10,5,0); LINE (10,5,0)-(30,5,0);
//! CIRCLE center (20,15,0) r=3; ATTDEF tag TAG1 at (12,8,0).
//! INSERT at (100,100,0), rotation 30 degrees, uniform scale 2.
//!
//! Hand derivation (c = cos30 = 0.8660254038, s = sin30 = 0.5, k = scale 2):
//!   Rot(30)*Scale maps (x,y) to (k*(c*x - s*y), k*(s*x + c*y)).
//!   LINE start (10,5)-base = (0,0)     -> (100, 100)
//!   LINE end   (30,5)-base = (20,0)    -> 100 + (2*17.320508, 2*10)
//!                                       = (134.641016, 120)
//!   CIRCLE center (20,15)-base = (10,10) -> 100 + (2*(8.660254-5), 2*(5+8.660254))
//!                                        = (107.320508, 127.320508)
//!   CIRCLE radius 3 * scale 2 -> 6
//!   ATTDEF (12,8)-base = (2,3) -> 100 + (2*(1.732051-1.5), 2*(1+2.598076))
//!                                 = (100.464102, 107.196152)
//!
//! Offset between the codec's `Insert::get_transform` (no base subtraction)
//! and the app's `insert_transform` (base-subtracting), measured at the block's
//! base point, is exactly Rot(30)*Scale*base(10,5,0):
//!   (2*(c*10 - s*5), 2*(s*10 + c*5)) = (12.320508, 18.660254, 0)
//!
//! Naming: `locked_*` = behaviour to keep; `documenting_*` = behaviour expected
//! to change. Tests that fail today carry
//! `#[ignore = "base-point repro: fails today"]`.

use crate::command::{CadCommand, CmdResult};
use crate::modules::insert::insert_block::InsertBlockCommand;
use crate::scene::render_graph::{
    block_base_point, insert_transform, BlockRoot, BlockRootRole, RenderSceneGraph, SceneRoot,
};
use crate::scene::{CreateBlockOptions, Scene};
use crate::ui::window::block_definition::{BlockDefinitionState, BlockObjectMode};
use codec::entities::{AttributeDefinition, AttributeEntity, Circle, Insert, Line};
use codec::types::{Handle, Matrix4, Transform, Vector3};
use codec::{Document, EntityType};
use glam::DVec3;

const BP_TEST: &str = "BP_TEST";
const DEG30: f64 = std::f64::consts::PI / 6.0;

/// Expected world positions, hand-derived from the convention above.
pub const EXPECTED_LINE_START: [f64; 3] = [100.0, 100.0, 0.0];
pub const EXPECTED_LINE_END: [f64; 3] = [134.641016, 120.0, 0.0];
pub const EXPECTED_CIRCLE_CENTER: [f64; 3] = [107.320508, 127.320508, 0.0];
pub const EXPECTED_ATTR_POINT: [f64; 3] = [100.464102, 107.196152, 0.0];
/// `Rot(30)*Scale(2)*base(10,5,0)` — the gap between the two transform paths.
pub const GET_TO_INSERT_OFFSET: [f64; 3] = [12.320508, 18.660254, 0.0];
/// Step-1 geometry pushed through the codec transform (no base subtraction) —
/// the pre-fix position, kept for the picking test's negative case.
pub const CODEC_LINE_END: [f64; 3] = [146.961524, 138.660254, 0.0];

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn assert_close(actual: Vector3, expected: [f64; 3], tol: f64, label: &str) {
    let delta = [
        (actual.x - expected[0]).abs(),
        (actual.y - expected[1]).abs(),
        (actual.z - expected[2]).abs(),
    ];
    assert!(
        delta.iter().all(|d| *d <= tol),
        "{label}: actual=({:.6},{:.6},{:.6}) expected=({:.6},{:.6},{:.6})",
        actual.x,
        actual.y,
        actual.z,
        expected[0],
        expected[1],
        expected[2]
    );
}

/// Two points must agree — used where both sides are computed by the app.
fn assert_same(actual: Vector3, expected: Vector3, tol: f64, label: &str) {
    assert!(
        (actual - expected).length() <= tol,
        "{label}: actual=({:.9},{:.9},{:.9}) expected=({:.9},{:.9},{:.9})",
        actual.x,
        actual.y,
        actual.z,
        expected.x,
        expected.y,
        expected.z
    );
}

fn attdef(tag: &str) -> AttributeDefinition {
    AttributeDefinition {
        insertion_point: v(12.0, 8.0, 0.0),
        height: 1.0,
        ..AttributeDefinition::new(tag.to_string(), "Tag one".to_string(), String::new())
    }
}

fn block_entities() -> Vec<EntityType> {
    vec![
        EntityType::Line(Line::from_points(v(10.0, 5.0, 0.0), v(30.0, 5.0, 0.0))),
        EntityType::Circle(Circle::from_coords(20.0, 15.0, 0.0, 3.0)),
        EntityType::AttributeDefinition(attdef("TAG1")),
    ]
}

/// A scene holding "BP_TEST": base (10,5,0) plus the LINE/CIRCLE/ATTDEF above.
fn scene_with_bp_test() -> Scene {
    let mut scene = Scene::new();
    scene.define_block_raw(BP_TEST, v(10.0, 5.0, 0.0), block_entities());
    scene
}

/// The LINE stored in `block`, in block-local coordinates.
fn block_line(document: &Document, block: &str) -> Line {
    document
        .block_records
        .get(block)
        .unwrap()
        .entity_handles
        .iter()
        .find_map(|h| match document.get_entity(*h) {
            Some(EntityType::Line(line)) => Some(line.clone()),
            _ => None,
        })
        .expect("the block's LINE")
}

fn block_attdefs(document: &Document, block: &str) -> Vec<AttributeDefinition> {
    let Some(br) = document.block_records.get(block) else {
        return Vec::new();
    };
    br.entity_handles
        .iter()
        .filter_map(|h| match document.get_entity(*h) {
            Some(EntityType::AttributeDefinition(ad)) => Some(ad.clone()),
            _ => None,
        })
        .collect()
}

/// The model-space INSERT referencing `block`.
fn instance_of(scene: &Scene, block: &str) -> Insert {
    scene
        .document
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(ins) if ins.block_name == block => Some(ins.clone()),
            _ => None,
        })
        .expect("the placed INSERT")
}

/// World positions of the instance's LINE, as the scene computes them.
fn instance_line_world(scene: &Scene, block: &str) -> (Vector3, Vector3) {
    let line = block_line(&scene.document, block);
    let insert = instance_of(scene, block);
    let transform = insert_transform(&scene.document, &insert);
    (transform.apply(line.start), transform.apply(line.end))
}

/// Drive the real `-INSERT` command through its prompt sequence: insertion
/// point, X scale, Y scale, rotation, then the attribute fill — the same code
/// path the INSERT command uses. The host injects the definition's base point
/// when it resolves the block's ATTDEFs (`handle_attreq_needed` in
/// app/command_driver/entity_commit.rs), which is what makes attribute
/// placement follow the render path's convention.
fn insert_via_command(scene: &mut Scene) -> Insert {
    let mut cmd =
        InsertBlockCommand::classic_for_block(BP_TEST.to_string(), Vec::new(), String::new());
    let _ = cmd.on_point(DVec3::new(100.0, 100.0, 0.0));
    assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
    assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
    assert!(matches!(
        cmd.on_text_input("30"),
        Some(CmdResult::AttreqNeeded { .. })
    ));
    let attdefs = block_attdefs(&scene.document, BP_TEST);
    assert_eq!(attdefs.len(), 1, "one ATTDEF is expected in the block");
    cmd.set_block_base(block_base_point(&scene.document, BP_TEST));
    assert!(cmd.attreq_set_attdefs(attdefs).is_none());
    match cmd.on_text_input("VALUE1") {
        Some(CmdResult::CommitAndExit(EntityType::Insert(insert)))
        | Some(CmdResult::CommitEntity(EntityType::Insert(insert))) => {
            scene.add_entity(EntityType::Insert(insert.clone()));
            insert
        }
        _ => panic!("the command did not commit an INSERT"),
    }
}

fn placed_instance(scene: &mut Scene) -> Insert {
    insert_via_command(scene)
}

/// Walk the render graph from model space, collecting (entity, instance
/// transform) pairs — the traversal the renderer itself uses.
fn render_graph_leaves(scene: &Scene) -> Vec<(EntityType, Transform)> {
    let model_space = scene
        .document
        .block_records
        .iter()
        .find(|record| record.is_model_space())
        .map(|record| record.handle)
        .expect("a model space block record");
    let depths = rustc_hash::FxHashMap::default();
    let graph = RenderSceneGraph::new(&scene.document, None, None, true, &depths);
    let mut leaves = Vec::new();
    graph.walk_root(
        SceneRoot::Block(BlockRoot {
            record: model_space,
            role: BlockRootRole::ModelSpace,
        }),
        |_, _| true,
        |entity, context| leaves.push((entity.clone(), context.transform)),
    );
    leaves
}

/// A view where world coordinates equal screen coordinates, so a click at a
/// world point can be named directly.
struct WorldAsScreen {
    view_rot: glam::Mat4,
    eye: DVec3,
    bounds: iced::Rectangle,
}

impl WorldAsScreen {
    fn new(extent: f32) -> Self {
        Self {
            view_rot: glam::Mat4::from_scale(glam::Vec3::new(2.0 / extent, -2.0 / extent, 1.0)),
            eye: DVec3::new(extent as f64 / 2.0, extent as f64 / 2.0, 0.0),
            bounds: iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(extent, extent)),
        }
    }

    fn at(&self, point: [f64; 3]) -> iced::Point {
        iced::Point::new(point[0] as f32, point[1] as f32)
    }
}

fn click_at(
    point: [f64; 3],
    view: &WorldAsScreen,
    wires: &[crate::scene::model::wire_model::WireModel],
    depths: &rustc_hash::FxHashMap<u64, [f32; 2]>,
) -> Option<Handle> {
    crate::scene::pick::hit_test::click_hit(
        view.at(point),
        wires,
        view.view_rot,
        view.eye,
        view.bounds,
        false,
        8.0,
        depths,
    )
    .and_then(Scene::handle_from_wire_name)
}

#[test]
fn locked_block_base_point_is_stored_on_the_definition() {
    let scene = scene_with_bp_test();
    assert_close(
        block_base_point(&scene.document, BP_TEST),
        [10.0, 5.0, 0.0],
        1e-9,
        "block base point",
    );
}

#[test]
fn locked_insert_command_builds_the_step1_reference() {
    let mut scene = scene_with_bp_test();
    let insert = insert_via_command(&mut scene);
    assert_close(
        insert.insert_point,
        [100.0, 100.0, 0.0],
        1e-9,
        "insert point",
    );
    assert_eq!(
        (insert.x_scale(), insert.y_scale(), insert.z_scale()),
        (2.0, 2.0, 2.0)
    );
    assert!((insert.rotation.to_degrees() - 30.0).abs() < 1e-9);
    let attribute = insert.attributes.first().expect("TAG1 is filled");
    assert_eq!(attribute.tag, "TAG1");
}

#[test]
fn locked_render_path_subtracts_the_block_base_point() {
    let mut scene = scene_with_bp_test();
    let insert = placed_instance(&mut scene);
    let transform = insert_transform(&scene.document, &insert);
    assert_close(
        transform.apply(v(10.0, 5.0, 0.0)),
        EXPECTED_LINE_START,
        1e-6,
        "line start",
    );
    assert_close(
        transform.apply(v(30.0, 5.0, 0.0)),
        EXPECTED_LINE_END,
        1e-6,
        "line end",
    );
    assert_close(
        transform.apply(v(20.0, 15.0, 0.0)),
        EXPECTED_CIRCLE_CENTER,
        1e-6,
        "circle center",
    );
}

#[test]
fn locked_render_graph_instances_at_convention_positions() {
    let mut scene = scene_with_bp_test();
    let _ = placed_instance(&mut scene);
    let leaves = render_graph_leaves(&scene);
    let line = leaves
        .iter()
        .find_map(|(entity, _)| match entity {
            EntityType::Line(line) => Some(line.clone()),
            _ => None,
        })
        .expect("the rendered LINE instance");
    let transform = leaves
        .iter()
        .find(|(entity, _)| matches!(entity, EntityType::Line(_)))
        .map(|(_, transform)| *transform)
        .unwrap();
    assert_close(
        transform.apply(line.start),
        EXPECTED_LINE_START,
        1e-6,
        "render graph: line start",
    );
    assert_close(
        transform.apply(line.end),
        EXPECTED_LINE_END,
        1e-6,
        "render graph: line end",
    );
}

#[test]
fn locked_hit_test_hits_the_base_point_adjusted_line() {
    let mut scene = scene_with_bp_test();
    let _ = placed_instance(&mut scene);
    let wires = scene.hit_test_wires();
    let depth_map = scene.draw_depth_map();
    let view = WorldAsScreen::new(2000.0);
    let insert_handle = instance_of(&scene, BP_TEST).common.handle;

    // Mid-point of the LINE under the convention.
    let convention_mid = [
        (EXPECTED_LINE_START[0] + EXPECTED_LINE_END[0]) / 2.0,
        (EXPECTED_LINE_START[1] + EXPECTED_LINE_END[1]) / 2.0,
        0.0,
    ];
    let hit = click_at(convention_mid, &view, wires.as_slice(), depth_map.as_ref());
    assert_eq!(
        hit,
        Some(insert_handle),
        "clicking the convention position must select the INSERT"
    );

    // Mid-point of the LINE under the codec transform (base not subtracted).
    let codec_mid = [
        (EXPECTED_LINE_END[0] + CODEC_LINE_END[0]) / 2.0,
        (EXPECTED_LINE_END[1] + CODEC_LINE_END[1]) / 2.0,
        0.0,
    ];
    let hit = click_at(codec_mid, &view, wires.as_slice(), depth_map.as_ref());
    assert_eq!(
        hit, None,
        "clicking the codec position must not hit anything"
    );
}

#[test]
fn locked_attribute_placement_matches_the_convention() {
    // base-point: the host now injects the definition's base point into the INSERT
    // command, so `finish_insert` places the attribute through
    // `insert_transform_with_base` — the same convention as the render path.
    // Hand derivation (see the module header): ATTDEF (12,8) - base (10,5) =
    // (2,3); Rot(30)*Scale(2)*(2,3) = (2*(1.732051-1.5), 2*(1+2.598076)) =
    // (0.464102, 7.196152); + insert point (100,100) = (100.464102, 107.196152).
    let mut scene = scene_with_bp_test();
    let insert = placed_instance(&mut scene);
    let attribute = insert.attributes.first().expect("TAG1");
    assert_close(
        attribute.insertion_point,
        EXPECTED_ATTR_POINT,
        1e-6,
        "attribute insertion point",
    );
    // And it is exactly the render path's placement of the ATTDEF.
    let attdef = block_attdefs(&scene.document, BP_TEST)
        .into_iter()
        .next()
        .expect("TAG1");
    let render = insert_transform(&scene.document, &insert).apply(attdef.insertion_point);
    assert_close(
        attribute.insertion_point,
        [render.x, render.y, render.z],
        1e-9,
        "attribute vs render path",
    );
}

#[test]
fn documenting_get_transform_offset_from_insert_transform() {
    // Ran and observed: the two paths differ by exactly Rot(30)*Scale(2)*base
    // for the step-1 case, measured at the block's own base point.
    let mut scene = scene_with_bp_test();
    let insert = placed_instance(&mut scene);
    let p = v(10.0, 5.0, 0.0);
    let render = insert_transform(&scene.document, &insert).apply(p);
    let codec = insert.get_transform().apply(p);
    let delta = render - codec;
    for (actual, expected) in [delta.x, delta.y, delta.z].iter().zip(GET_TO_INSERT_OFFSET) {
        assert!((actual.abs() - expected).abs() < 1e-6, "offset {delta:?}");
    }
}

#[test]
fn locked_creation_with_a_picked_base_bakes_the_origin_into_the_geometry() {
    // base-point fix (B): the caller bakes the picked base into `world_to_block`, so
    // the contents are stored relative to it and the definition's own base
    // point stays ZERO — the picked base becomes the block origin. Storing the
    // base in the definition as well would subtract it twice. The geometry of
    // existing / imported definitions is never touched.
    const NAME: &str = "BP_CREATE_BASE";
    let mut scene = Scene::new();
    let line = scene.add_entity(EntityType::Line(Line::from_points(
        v(10.0, 5.0, 0.0),
        v(30.0, 5.0, 0.0),
    )));
    let options = CreateBlockOptions {
        name: NAME.to_string(),
        handles: vec![line],
        base_point: DVec3::new(10.0, 5.0, 0.0),
        world_to_block: Transform::from_translation(v(-10.0, -5.0, 0.0)),
        block_to_world: Transform::from_translation(v(10.0, 5.0, 0.0)),
        mode: BlockObjectMode::Retain,
        annotative: false,
        match_orientation: false,
        scale_uniformly: false,
        allow_exploding: true,
        unit: 0,
        description: String::new(),
        hyperlink_url: String::new(),
        hyperlink_desc: String::new(),
        redefine: false,
    };
    assert!(scene.create_block_with_options(options).is_ok());

    // The stored LINE is the drawn LINE translated by -(10,5): (0,0)-(20,0).
    let stored = block_line(&scene.document, NAME);
    assert_close(stored.start, [0.0, 0.0, 0.0], 1e-9, "stored line start");
    assert_close(stored.end, [20.0, 0.0, 0.0], 1e-9, "stored line end");
    // The definition's base point stays ZERO, so the picked base — now the
    // block origin — is what an insertion point lands on.
    assert_close(
        block_base_point(&scene.document, NAME),
        [0.0, 0.0, 0.0],
        1e-9,
        "stored base point after create_block_with_options",
    );
    // Inserted at (100,100) with no rotation or scale the LINE therefore lands
    // at (100,100)-(120,100): the picked base was honoured on creation.
    let _ = scene.add_entity(EntityType::Insert(Insert::new(
        NAME.to_string(),
        v(100.0, 100.0, 0.0),
    )));
    let (start, end) = instance_line_world(&scene, NAME);
    assert_close(start, [100.0, 100.0, 0.0], 1e-6, "instance line start");
    assert_close(end, [120.0, 100.0, 0.0], 1e-6, "instance line end");
}

#[test]
fn locked_redefine_with_a_changed_base_moves_the_instance() {
    // base-point fix (C). Step 3: create through the dialog's options with a
    // non-zero base, insert it, then redefine it through the
    // `redefine_block_raw` call dialog.rs makes, with a different base. A
    // changed base is allowed to move instances — the contents stay put in
    // block coordinates while the base they are measured from moves.
    const NAME: &str = "BP_REDEF_MOVED";
    let mut scene = Scene::new();
    let line = scene.add_entity(EntityType::Line(Line::from_points(
        v(10.0, 5.0, 0.0),
        v(30.0, 5.0, 0.0),
    )));
    let options = CreateBlockOptions {
        name: NAME.to_string(),
        handles: vec![line],
        base_point: DVec3::new(10.0, 5.0, 0.0),
        world_to_block: Transform::from_translation(v(-10.0, -5.0, 0.0)),
        block_to_world: Transform::from_translation(v(10.0, 5.0, 0.0)),
        mode: BlockObjectMode::Retain,
        annotative: false,
        match_orientation: false,
        scale_uniformly: false,
        allow_exploding: true,
        unit: 0,
        description: String::new(),
        hyperlink_url: String::new(),
        hyperlink_desc: String::new(),
        redefine: false,
    };
    assert!(scene.create_block_with_options(options).is_ok());

    let mut insert = Insert::new(NAME.to_string(), v(100.0, 100.0, 0.0));
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    insert.set_z_scale(2.0);
    insert.rotation = DEG30;
    let instance = scene.add_entity(EntityType::Insert(insert));

    // Block-local LINE is world (10,5)-(30,5) shifted by -(10,5) = (0,0)-(20,0),
    // and the stored base point is (0,0,0), so the instance renders at
    // (100,100) and (134.641016, 120).
    let (start, end) = instance_line_world(&scene, NAME);
    assert_close(
        start,
        [100.0, 100.0, 0.0],
        1e-6,
        "line start before redefine",
    );
    assert_close(
        end,
        [134.641016, 120.0, 0.0],
        1e-6,
        "line end before redefine",
    );

    // The redefine path: the same contents, but a new base point (0,20,0).
    assert!(scene
        .redefine_block_raw(
            NAME,
            v(0.0, 20.0, 0.0),
            vec![EntityType::Line(Line::from_points(
                v(0.0, 0.0, 0.0),
                v(20.0, 0.0, 0.0),
            ))],
        )
        .is_ok());
    assert_close(
        block_base_point(&scene.document, NAME),
        [0.0, 20.0, 0.0],
        1e-9,
        "stored base point after redefine",
    );

    // The contents did not move, so every instance moves by exactly
    // Rot(30)*Scale(2)*(newBase - oldBase) = Rot(30)*2*((0,20) - (0,0))
    //   = 2*(0.866025*0 - 0.5*20, 0.5*0 + 0.866025*20) = (20, -34.641016).
    let (start_after, end_after) = instance_line_world(&scene, NAME);
    // start: (100,100) + Rot(30)*2*((0,0) - (0,20)) = (120, 65.358984)
    assert_close(
        start_after,
        [120.0, 65.358984, 0.0],
        1e-6,
        "line start after redefine",
    );
    // end: (100,100) + Rot(30)*2*((20,0) - (0,20))
    //    = (100,100) + 2*(17.320508 + 10, 10 - 17.320508) = (154.641016, 85.358984)
    assert_close(
        end_after,
        [154.641016, 85.358984, 0.0],
        1e-6,
        "line end after redefine",
    );
    let jump = Vector3::new(
        start_after.x - start.x,
        start_after.y - start.y,
        start_after.z - start.z,
    );
    assert_close(jump, [20.0, -34.641016, 0.0], 1e-6, "instance movement");
    assert!(
        (jump.x.abs() + jump.y.abs()) > 1.0,
        "the instance moved by {jump:?}"
    );
    let _ = instance;
}

#[test]
fn locked_noop_redefine_leaves_the_instance_where_it_is() {
    // base-point fix (C): redefining a block with the same contents and the same
    // base point must not move any instance — the definition is replaced
    // verbatim.
    const NAME: &str = "BP_REDEF_NOOP";
    let mut scene = Scene::new();
    let base = v(6.0, 4.0, 0.0);
    scene.define_block_raw(
        NAME,
        base,
        vec![EntityType::Line(Line::from_points(
            v(6.0, 4.0, 0.0),
            v(16.0, 4.0, 0.0),
        ))],
    );
    let mut insert = Insert::new(NAME.to_string(), v(50.0, 20.0, 0.0));
    insert.set_x_scale(1.5);
    insert.set_y_scale(1.5);
    insert.set_z_scale(1.5);
    insert.rotation = DEG30;
    let _ = scene.add_entity(EntityType::Insert(insert));

    // Render path before: (50,20) + Rot(30)*1.5*((6,4)-(6,4)) = (50,20) and
    // (50,20) + Rot(30)*1.5*((16,4)-(6,4)) = (50,20) + 1.5*Rot(30)*(10,0)
    //   = (50,20) + 1.5*(8.660254, 5) = (62.990381, 27.5).
    let (start, end) = instance_line_world(&scene, NAME);
    assert_close(start, [50.0, 20.0, 0.0], 1e-6, "line start before");
    assert_close(end, [62.990381, 27.5, 0.0], 1e-6, "line end before");

    // No-op redefine: identical contents, same base.
    assert!(scene
        .redefine_block_raw(
            NAME,
            base,
            vec![EntityType::Line(Line::from_points(
                v(6.0, 4.0, 0.0),
                v(16.0, 4.0, 0.0),
            ))],
        )
        .is_ok());
    assert_close(
        block_base_point(&scene.document, NAME),
        [6.0, 4.0, 0.0],
        1e-9,
        "base point after no-op redefine",
    );
    let (start_after, end_after) = instance_line_world(&scene, NAME);
    assert_same(start_after, start, 1e-12, "line start after no-op redefine");
    assert_same(end_after, end, 1e-12, "line end after no-op redefine");
}

#[test]
fn locked_block_definition_dialog_prefills_the_existing_definition_base_point() {
    // base-point fix (D): choosing an existing block in the Block Definition dialog
    // prefills the base point from that definition, so a redefine starts from
    // the base it already has. A new block starts at 0.
    let scene = scene_with_bp_test();
    assert_close(
        block_base_point(&scene.document, BP_TEST),
        [10.0, 5.0, 0.0],
        1e-9,
        "the drawing's block base point",
    );

    // A new block: the fields start at 0.
    let fresh = BlockDefinitionState::new(vec![BP_TEST.to_string()], Vec::new(), 0);
    assert_eq!(fresh.base_point_x, "0");
    assert_eq!(fresh.base_point_y, "0");
    assert_eq!(fresh.base_point_z, "0");
    assert_eq!(fresh.parse_base_point(), DVec3::ZERO);

    // Picking the existing block (what `Message::BlockDefNameSelect` does).
    let mut state = fresh;
    let base = block_base_point(&scene.document, BP_TEST);
    state.set_base_point(DVec3::new(base.x, base.y, base.z));
    assert_eq!(state.base_point_x, "10.0000");
    assert_eq!(state.base_point_y, "5.0000");
    assert_eq!(state.base_point_z, "0.0000");
    assert_eq!(state.parse_base_point(), DVec3::new(10.0, 5.0, 0.0));
}

#[test]
fn locked_explode_places_geometry_at_convention_positions() {
    // The document-aware codec explosion uses the definition's base point,
    // so the pieces land where the render path puts them.
    let mut scene = scene_with_bp_test();
    let insert = placed_instance(&mut scene);
    let pieces = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(insert),
        &scene.document,
    );
    let mut line_start = None;
    let mut line_end = None;
    let mut circle_center = None;
    for piece in &pieces {
        match piece {
            EntityType::Line(line) => {
                if line_end.is_none() {
                    line_start = Some(line.start);
                    line_end = Some(line.end);
                }
            }
            EntityType::Circle(circle) => circle_center = Some(circle.center),
            EntityType::Ellipse(ellipse) => circle_center = Some(ellipse.center),
            _ => {}
        }
    }
    assert_close(
        line_start.expect("the exploded LINE"),
        EXPECTED_LINE_START,
        1e-6,
        "exploded line start",
    );
    assert_close(
        line_end.expect("the exploded LINE"),
        EXPECTED_LINE_END,
        1e-6,
        "exploded line end",
    );
    assert_close(
        circle_center.expect("the exploded CIRCLE"),
        EXPECTED_CIRCLE_CENTER,
        1e-6,
        "exploded circle center",
    );
    // Exploding an INSERT drops attribute values and the
    // attribute definitions (tags) reappear as ATTDEF entities.
    let attdef_piece = pieces.iter().find_map(|p| match p {
        EntityType::AttributeDefinition(ad) => Some(ad.clone()),
        _ => None,
    });
    let ad = attdef_piece.expect("the exploded ATTDEF");
    assert_eq!(ad.tag, "TAG1");
    assert_close(
        ad.insertion_point,
        EXPECTED_ATTR_POINT,
        1e-6,
        "exploded attdef insertion point",
    );
    // Same numbers as the render path: EXPLODE and the renderer agree.
    let (render_start, render_end) = instance_line_world(&scene, BP_TEST);
    assert_close(render_end, EXPECTED_LINE_END, 1e-6, "render path: line end");
    assert_close(
        render_start,
        EXPECTED_LINE_START,
        1e-6,
        "render path: line start",
    );
}

#[test]
fn locked_minsert_explode_restores_attdefs_in_each_rotated_cell() {
    let scene = scene_with_bp_test();
    let mut insert = Insert::new(BP_TEST, v(100.0, 100.0, 0.0))
        .with_uniform_scale(2.0)
        .with_rotation(std::f64::consts::FRAC_PI_2)
        .with_array(2, 2, 10.0, 20.0);
    insert
        .attributes
        .push(AttributeEntity::new("TAG1".into(), "Instance value".into()));
    let pieces = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(insert),
        &scene.document,
    );
    let definitions: Vec<_> = pieces
        .iter()
        .filter_map(|entity| match entity {
            EntityType::AttributeDefinition(definition) => Some(definition),
            _ => None,
        })
        .collect();
    assert_eq!(definitions.len(), 4);
    for (definition, expected) in definitions.iter().zip([
        [94.0, 104.0, 0.0],
        [94.0, 114.0, 0.0],
        [74.0, 104.0, 0.0],
        [74.0, 114.0, 0.0],
    ]) {
        assert_close(definition.insertion_point, expected, 1e-6, "array ATTDEF");
        assert_eq!(definition.tag, "TAG1");
        assert_eq!(definition.default_value, "");
    }
    let lines: Vec<_> = pieces
        .iter()
        .filter_map(|entity| match entity {
            EntityType::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 4);
    for (line, expected) in lines.iter().zip([
        [100.0, 100.0, 0.0],
        [100.0, 110.0, 0.0],
        [80.0, 100.0, 0.0],
        [80.0, 110.0, 0.0],
    ]) {
        assert_close(line.start, expected, 1e-6, "array LINE");
    }
    assert_close(
        block_attdefs(&scene.document, BP_TEST)[0].insertion_point,
        [12.0, 8.0, 0.0],
        1e-6,
        "source ATTDEF stays in definition coordinates",
    );
}

#[test]
fn locked_xclip_boundary_round_trips_through_the_base_point_convention() {
    // base-point follow-up fix: the XCLIP ring is stored as WCS points plus the
    // inverse of the transform every consumer composes it back with. That
    // consumer is the base-point-aware instance transform, so the stored
    // inverse has to be the base-aware one; a base-ignoring inverse displaces
    // the ring by exactly Rot(30)*Scale(2)*base = (12.320508, 18.660254).
    const NAME: &str = "BP_XCLIP";
    let mut scene = Scene::new();
    // Same fixture as BP_TEST: base (10,5,0), LINE (10,5)-(30,5) local
    // (0,0)-(20,0), INSERT at (100,100) with 30° and scale 2, so the rendered
    // LINE runs (100,100)-(134.641016,120).
    scene.define_block_raw(
        NAME,
        v(10.0, 5.0, 0.0),
        vec![EntityType::Line(Line::from_points(
            v(10.0, 5.0, 0.0),
            v(30.0, 5.0, 0.0),
        ))],
    );
    let mut insert = Insert::new(NAME.to_string(), v(100.0, 100.0, 0.0));
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    insert.set_z_scale(2.0);
    insert.rotation = DEG30;
    let handle = scene.add_entity(EntityType::Insert(insert));
    let insert = match scene.document.get_entity(handle) {
        Some(EntityType::Insert(ins)) => ins.clone(),
        _ => panic!("the INSERT was stored"),
    };

    // A rectangle drawn in WCS around the rendered LINE.
    let boundary = [[98.0, 98.0], [136.0, 122.0]];
    assert!(crate::scene::pick::xclip::set_insert_clip(
        &mut scene.document,
        handle,
        &boundary,
        false,
        ([100.0, 100.0], [134.641016, 120.0]),
    ));

    let filter = crate::scene::pick::xclip::insert_spatial_filter(&scene.document, &insert)
        .expect("the stored spatial filter");
    let world = crate::scene::pick::xclip::world_clip_polygon_for_transform(
        filter,
        &insert_transform(&scene.document, &insert),
    );
    // Two stored points expand to the rectangle in drawn order.
    let expected = [[98.0, 98.0], [136.0, 98.0], [136.0, 122.0], [98.0, 122.0]];
    assert_eq!(world.len(), expected.len());
    for (got, want) in world.iter().zip(expected) {
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "ring corner ({:.9},{:.9}), expected ({:.9},{:.9})",
            got[0],
            got[1],
            want[0],
            want[1]
        );
    }

    // The same stored filter composed with the codec's base-ignoring transform
    // displaces the ring by exactly Rot(30)*Scale(2)*base — the error the
    // base-aware storage removes.
    let base_ignoring = crate::scene::pick::xclip::world_clip_polygon_for_transform(
        filter,
        &insert.get_transform(),
    );
    for (got, want) in base_ignoring.iter().zip(expected) {
        assert!(
            ((got[0] - want[0]) - GET_TO_INSERT_OFFSET[0]).abs() < 1e-6
                && ((got[1] - want[1]) - GET_TO_INSERT_OFFSET[1]).abs() < 1e-6,
            "base-ignoring corner ({:.9},{:.9}) is not the drawn corner plus {:?}",
            got[0],
            got[1],
            GET_TO_INSERT_OFFSET
        );
    }
}

#[test]
fn locked_reframe_conjugates_attributes_through_the_convention() {
    // base-point follow-up fix: `reframe_block_definition` (BEDIT UCS bake)
    // conjugates the block-local reframe through the INSERT transform for the
    // ATTRIBs living on external instances. That transform is the
    // base-point-aware one, because it is the map the attribute positions on
    // the INSERT were placed through.
    const NAME: &str = "BP_REFRAME";
    let mut scene = Scene::new();
    scene.define_block_raw(
        NAME,
        v(10.0, 5.0, 0.0),
        vec![
            EntityType::Line(Line::from_points(v(10.0, 5.0, 0.0), v(30.0, 5.0, 0.0))),
            EntityType::AttributeDefinition(attdef("TAG1")),
        ],
    );
    let mut insert = Insert::new(NAME.to_string(), v(100.0, 100.0, 0.0));
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    insert.set_z_scale(2.0);
    insert.rotation = DEG30;
    // The attribute sits where the convention puts the ATTDEF at (12,8).
    let mut attribute = AttributeEntity::simple("TAG1", "VALUE1");
    attribute.insertion_point = insert_transform(&scene.document, &insert).apply(v(12.0, 8.0, 0.0));
    insert.attributes.push(attribute);
    let insert_handle = scene.add_entity(EntityType::Insert(insert));

    // Reframe: a 90° turn of the block-local frame, the kind of UCS change
    // `commit_active_ucs_change` bakes in. local_from_old maps (x,y) to (-y,x).
    let local = Transform::from_matrix(Matrix4::rotation_z(std::f64::consts::FRAC_PI_2));
    let record = scene
        .document
        .block_records
        .get(NAME)
        .expect("the block record")
        .handle;
    let changed = scene.reframe_block_definition(record, &local);
    assert!(
        changed >= 2,
        "the LINE and the ATTDEF were reframed: {changed}"
    );

    // The block-local contents turned 90°: (10,5)→(-5,10), (30,5)→(-5,30),
    // ATTDEF (12,8)→(-8,12).
    let stored = block_line(&scene.document, NAME);
    assert_close(stored.start, [-5.0, 10.0, 0.0], 1e-9, "reframed line start");
    assert_close(stored.end, [-5.0, 30.0, 0.0], 1e-9, "reframed line end");
    let attdefs = block_attdefs(&scene.document, NAME);
    assert_close(
        attdefs[0].insertion_point,
        [-8.0, 12.0, 0.0],
        1e-9,
        "reframed attdef",
    );

    // The attribute on the instance must now sit where the convention puts the
    // reframed ATTDEF: (100,100) + Rot(30)*2*((-8,12) - (10,5))
    //   = (100,100) + 2*Rot(30)*(-18,7)
    //   Rot(30)*(-18,7) = (-18*0.8660254 - 7*0.5, -18*0.5 + 7*0.8660254)
    //                   = (-19.088457, -2.937822)
    //   -> (100,100) + (-38.176914, -5.875644) = (61.823086, 94.124356).
    // Conjugating through the base-ignoring transform instead lands on
    // (92.803848, 100.464102): get_transform⁻¹ of the old position is the
    // base-relative (2,3), which the 90° turn maps to (-3,2).
    let attribute = match scene.document.get_entity(insert_handle) {
        Some(EntityType::Insert(ins)) => ins.attributes.first().cloned(),
        _ => None,
    }
    .expect("the attribute on the instance");
    assert_close(
        attribute.insertion_point,
        [61.823086, 94.124356, 0.0],
        1e-6,
        "reframed attribute position",
    );
}

/// Back-to-back nested reference: block `OUTER` (base (10,5,0)) holds a LINE
/// and an INSERT of block `INNER` (base (4,2,0)) at OUTER-local (5,3) with
/// 60° rotation and scale 1; `INNER` holds the LINE (4,2)-(10,2).
fn nested_scene() -> (Scene, Insert) {
    const INNER: &str = "BP_NESTED_INNER";
    const OUTER: &str = "BP_NESTED_OUTER";
    let mut scene = Scene::new();
    // INNER: base (4,2,0), so its LINE is local (0,0)-(6,0).
    scene.define_block_raw(
        INNER,
        v(4.0, 2.0, 0.0),
        vec![EntityType::Line(Line::from_points(
            v(4.0, 2.0, 0.0),
            v(10.0, 2.0, 0.0),
        ))],
    );
    // OUTER: base (10,5,0), LINE (10,5)-(30,5) is local (0,0)-(20,0), plus the
    // nested INSERT at OUTER-local (5,3), 60°, scale 1.
    let mut nested = Insert::new(INNER.to_string(), v(5.0, 3.0, 0.0));
    nested.set_x_scale(1.0);
    nested.set_y_scale(1.0);
    nested.set_z_scale(1.0);
    nested.rotation = std::f64::consts::PI / 3.0;
    scene.define_block_raw(
        OUTER,
        v(10.0, 5.0, 0.0),
        vec![
            EntityType::Line(Line::from_points(v(10.0, 5.0, 0.0), v(30.0, 5.0, 0.0))),
            EntityType::Insert(nested),
        ],
    );
    // Model space: INSERT of OUTER at (100,100), 30°, scale 2.
    let mut insert = Insert::new(OUTER.to_string(), v(100.0, 100.0, 0.0));
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    insert.set_z_scale(2.0);
    insert.rotation = DEG30;
    let handle = scene.add_entity(EntityType::Insert(insert.clone()));
    let placed = scene
        .document
        .get_entity(handle)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.clone()),
            _ => None,
        })
        .expect("the OUTER INSERT");
    (scene, placed)
}

#[test]
fn locked_nested_reference_explode_matches_the_render_path() {
    // base-point: exploding the outer reference one level yields the nested
    // reference, and placing the inner block through that piece lands on
    // exactly the position the render graph computes for it.
    let (scene, outer) = nested_scene();

    // Hand derivation (c30 = 0.8660254, s30 = 0.5; c60 = 0.5, s60 = 0.8660254):
    //   INNER-local (0,0) [the base point] -> OUTER-local
    //     (5,3) + Rot(60)*(0,0) = (5,3)
    //   OUTER-local (5,3) -> world
    //     (100,100) + 2*Rot(30)*((5,3) - (10,5)) = (100,100) + 2*Rot(30)*(-5,-2)
    //     Rot(30)*(-5,-2) = (-5*0.8660254 + 2*0.5, -5*0.5 - 2*0.8660254)
    //                     = (-3.330127, -4.232051)
    //     -> (100,100) + (-6.660254, -8.464102) = (93.339746, 91.535898)
    let mut render_start = None;
    for (entity, transform) in render_graph_leaves(&scene) {
        let EntityType::Line(line) = entity else {
            continue;
        };
        if (line.start.x - 4.0).abs() < 1e-9 && (line.start.y - 2.0).abs() < 1e-9 {
            let world = transform.apply(line.start);
            render_start = Some(world);
        }
    }
    let render_start = render_start.expect("the INNER LINE on the render path");
    assert_close(
        render_start,
        [93.339746, 91.535898, 0.0],
        1e-6,
        "render path: inner line start",
    );

    // Explode path: one level down gives back the nested reference.
    let pieces = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(outer.clone()),
        &scene.document,
    );
    let nested_piece = pieces
        .iter()
        .find_map(|piece| match piece {
            EntityType::Insert(ins) if ins.block_name == "BP_NESTED_INNER" => Some(ins.clone()),
            _ => None,
        })
        .expect("the nested reference among the exploded pieces");
    // The nested reference itself lands where the render path puts it: the
    // codec applies Rot(30+60)=90° and scale 2*1=2 to it.
    assert_close(
        nested_piece.insert_point,
        [93.339746, 91.535898, 0.0],
        1e-6,
        "exploded nested reference insert point",
    );
    assert!(
        (nested_piece.rotation.to_degrees() - 90.0).abs() < 1e-6,
        "nested rotation = 90, got {}",
        nested_piece.rotation.to_degrees()
    );
    assert!((nested_piece.x_scale() - 2.0).abs() < 1e-9);

    // And placing the inner block through that piece reproduces the render
    // path: (93.339746, 91.535898) + Rot(90)*2*((10,2) - (4,2))
    //   = (93.339746, 91.535898) + 2*Rot(90)*(6,0) = ... + (0,12)
    //   = (93.339746, 103.535898)
    let inner_line = scene
        .document
        .block_records
        .get("BP_NESTED_INNER")
        .and_then(|br| {
            br.entity_handles
                .iter()
                .find_map(|h| match scene.document.get_entity(*h) {
                    Some(EntityType::Line(line)) => Some(line.clone()),
                    _ => None,
                })
        })
        .expect("the INNER LINE");
    let piece_transform = insert_transform(&scene.document, &nested_piece);
    assert_close(
        piece_transform.apply(inner_line.start),
        [93.339746, 91.535898, 0.0],
        1e-6,
        "explode path: inner line start",
    );
    assert_close(
        piece_transform.apply(inner_line.end),
        [93.339746, 103.535898, 0.0],
        1e-6,
        "explode path: inner line end",
    );

    let mut render_end = None;
    for (entity, transform) in render_graph_leaves(&scene) {
        let EntityType::Line(line) = entity else {
            continue;
        };
        if (line.start.x - 4.0).abs() < 1e-9 && (line.start.y - 2.0).abs() < 1e-9 {
            let world = transform.apply(line.end);
            render_end = Some(world);
        }
    }
    assert_close(
        render_end.expect("the INNER LINE end on the render path"),
        [93.339746, 103.535898, 0.0],
        1e-6,
        "render path: inner line end",
    );
}

#[test]
fn locked_nested_insert_with_attributes_exploded_one_level() {
    // When an outer block containing an inner INSERT and attributes is exploded one level,
    // the outer block's attributes drop their values and its ATTDEFs reappear, while
    // the inner INSERT remains an intact INSERT with its attributes preserved.
    const INNER: &str = "BP_ATTR_INNER";
    const OUTER: &str = "BP_ATTR_OUTER";
    let mut scene = Scene::new();

    // INNER block: base (0,0,0), has a LINE and an ATTDEF
    let inner_ad = AttributeDefinition {
        insertion_point: v(1.0, 1.0, 0.0),
        height: 1.0,
        ..AttributeDefinition::new(
            "INNER_TAG".to_string(),
            "Inner prompt".to_string(),
            "Inner val".to_string(),
        )
    };
    scene.define_block_raw(
        INNER,
        v(0.0, 0.0, 0.0),
        vec![
            EntityType::Line(Line::from_points(v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0))),
            EntityType::AttributeDefinition(inner_ad),
        ],
    );

    // OUTER block: base (10.0, 5.0, 0.0), has a nested INSERT of INNER (with an attribute instance) and an ATTDEF
    let mut inner_ins = Insert::new(INNER.to_string(), v(5.0, 3.0, 0.0));
    let mut inner_attr =
        AttributeEntity::new("INNER_TAG".to_string(), "Actual inner value".to_string());
    inner_attr.insertion_point = v(6.0, 4.0, 0.0);
    inner_ins.attributes.push(inner_attr);

    let outer_ad = AttributeDefinition {
        insertion_point: v(12.0, 8.0, 0.0),
        height: 1.0,
        ..AttributeDefinition::new(
            "OUTER_TAG".to_string(),
            "Outer prompt".to_string(),
            "Outer val".to_string(),
        )
    };

    scene.define_block_raw(
        OUTER,
        v(10.0, 5.0, 0.0),
        vec![
            EntityType::Insert(inner_ins),
            EntityType::AttributeDefinition(outer_ad),
        ],
    );

    // Model space: instance of OUTER at (100, 100), 30°, scale 2
    let mut outer_instance = Insert::new(OUTER.to_string(), v(100.0, 100.0, 0.0));
    outer_instance.set_x_scale(2.0);
    outer_instance.set_y_scale(2.0);
    outer_instance.set_z_scale(2.0);
    outer_instance.rotation = DEG30;
    let mut outer_attr =
        AttributeEntity::new("OUTER_TAG".to_string(), "Actual outer value".to_string());
    outer_attr.insertion_point = v(100.464102, 107.196152, 0.0);
    outer_instance.attributes.push(outer_attr);

    // Explode outer instance one level
    let pieces = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(outer_instance),
        &scene.document,
    );

    // Expected pieces: the nested inner INSERT (with its attribute still intact) and the outer ATTDEF
    let nested_insert = pieces
        .iter()
        .find_map(|p| match p {
            EntityType::Insert(ins) if ins.block_name == INNER => Some(ins.clone()),
            _ => None,
        })
        .expect("nested inner INSERT among pieces");

    assert_eq!(nested_insert.attributes.len(), 1);
    assert_eq!(nested_insert.attributes[0].tag, "INNER_TAG");
    assert_eq!(
        nested_insert.attributes[0].get_value(),
        "Actual inner value"
    );

    let outer_attdef = pieces
        .iter()
        .find_map(|p| match p {
            EntityType::AttributeDefinition(ad) if ad.tag == "OUTER_TAG" => Some(ad.clone()),
            _ => None,
        })
        .expect("reappeared outer ATTDEF among pieces");

    assert_close(
        outer_attdef.insertion_point,
        EXPECTED_ATTR_POINT,
        1e-6,
        "outer attdef insertion point",
    );
}

#[test]
fn locked_redefine_block_raw_refuses_cycle() {
    let mut scene = Scene::new();
    let blk_a = "REDEF_A";
    let blk_b = "REDEF_B";

    scene.define_block_raw(
        blk_a,
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
        ))],
    );

    // B contains A
    scene.define_block_raw(
        blk_b,
        Vector3::ZERO,
        vec![EntityType::Insert(Insert::new(
            blk_a.to_string(),
            Vector3::ZERO,
        ))],
    );

    // Redefining A to contain B would create cycle A -> B -> A; must refuse
    let res = scene.redefine_block_raw(
        blk_a,
        Vector3::ZERO,
        vec![EntityType::Insert(Insert::new(
            blk_b.to_string(),
            Vector3::ZERO,
        ))],
    );

    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
        err.contains("circular dependency detected"),
        "expected cycle error, got: {err}"
    );
}

#[test]
fn locked_redefine_block_raw_refuses_open_block() {
    let mut scene = Scene::new();
    let open_name = "REDEF_OPEN";
    let other_name = "REDEF_CLOSED";

    scene.define_block_raw(
        open_name,
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
        ))],
    );
    scene.define_block_raw(
        other_name,
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(2.0, 0.0, 0.0),
        ))],
    );

    let open_record = scene.document.block_records.get(open_name).unwrap().handle;
    let members_before = scene
        .document
        .block_records
        .get(open_name)
        .unwrap()
        .entity_handles
        .clone();
    scene.block_edit_block = Some(open_record);

    let res = scene.redefine_block_raw(
        open_name,
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(9.0, 9.0, 0.0),
        ))],
    );
    let err = res.expect_err("redefining the open block must be refused");
    assert!(
        err.contains(open_name) && err.contains("Block Editor"),
        "expected refusal naming the block and the editor, got: {err}"
    );
    assert_eq!(
        scene
            .document
            .block_records
            .get(open_name)
            .unwrap()
            .entity_handles,
        members_before,
        "the open definition must be untouched"
    );

    let res = scene.redefine_block_raw(
        other_name,
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(8.0, 0.0, 0.0),
        ))],
    );
    assert!(res.is_ok(), "a closed block must still redefine: {res:?}");
}
