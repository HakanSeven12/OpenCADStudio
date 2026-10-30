use super::*;
use crate::scene::{block_grips, block_orientation_parameter as orientation, Scene};
use codec::entities::Line;

fn fixture(
    perpendicular: bool,
    relocated: bool,
    plane: WorkingPlane,
) -> (Scene, InsertBlockCommand, Vector3) {
    let mut scene = Scene::new();
    let base = Vector3::new(5., 7., 0.);
    let anchor = if relocated {
        base + Vector3::new(2., 3., 0.)
    } else {
        base
    };
    scene.define_block_raw(
        "ALIGN_INSERT",
        base,
        vec![EntityType::Line(Line::from_points(
            anchor,
            anchor + Vector3::UNIT_X,
        ))],
    );
    let def = scene
        .document
        .block_records
        .get("ALIGN_INSERT")
        .unwrap()
        .handle;
    let parameter = orientation::create(
        &mut scene.document,
        def,
        orientation::Kind::Alignment,
        base,
        base + Vector3::UNIT_X,
        base,
    )
    .unwrap();
    orientation::set_alignment_type(&mut scene.document, def, parameter, perpendicular).unwrap();
    if relocated {
        block_grips::config::set_location(&mut scene.document, def, parameter, 0, anchor).unwrap();
    }
    let a = plane.to_world(DVec3::new(100., 0., 0.));
    let b = plane.to_world(DVec3::new(100., 100., 0.));
    scene
        .document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(a.x, a.y, a.z),
            Vector3::new(b.x, b.y, b.z),
        )))
        .unwrap();
    let wire = WireModel::solid_f64(
        "alignment-insertion".into(),
        vec![
            [anchor.x, anchor.y, anchor.z],
            [anchor.x + 1., anchor.y, anchor.z],
        ],
        WireModel::CYAN,
        false,
    );
    let options = BlockInsertOptions {
        x: 2.,
        y: 3.,
        z: 1.,
        ..Default::default()
    };
    let mut cmd = InsertBlockCommand::palette("ALIGN_INSERT".into(), vec![wire], &options);
    cmd.set_working_plane(plane);
    cmd.set_insertion_points(base, vec![anchor]);
    if relocated {
        assert!(cmd.cycle_insertion_point());
    }
    cmd.refresh_point_pick_scene(&scene);
    (scene, cmd, anchor)
}

#[test]
fn block_alignment_insertion_preview_matches_placement_for_native_modes_scales_and_planes() {
    for perpendicular in [false, true] {
        for relocated in [false, true] {
            for mirrored in [false, true] {
                for plane in [
                    WorkingPlane::default(),
                    WorkingPlane::new(DVec3::new(10., 20., 30.), DVec3::Y, DVec3::Z),
                ] {
                    let (scene, mut cmd, anchor) = fixture(perpendicular, relocated, plane);
                    if mirrored {
                        cmd.preset_scale[0] = Some(-2.);
                    }
                    let before = scene.document.clone();
                    let point = plane.to_world(DVec3::new(100., 20., 0.));
                    let preview = cmd.on_preview_wires(point);
                    assert!(matches!(
                        cmd.on_point(point),
                        CmdResult::AttreqNeeded { .. }
                    ));
                    let EntityType::Insert(insert) = cmd.attreq_set_attdefs(vec![]).unwrap() else {
                        panic!()
                    };
                    let transform =
                        crate::scene::render_graph::insert_transform(&scene.document, &insert);
                    let actual = transform.apply(anchor);
                    assert!(DVec3::new(actual.x, actual.y, actual.z).distance(point) < 1e-7);
                    let axis = transform.apply_rotation(Vector3::UNIT_X);
                    let axis = DVec3::new(axis.x, axis.y, axis.z).normalize();
                    let expected =
                        plane.vector_to_world(if perpendicular { DVec3::X } else { DVec3::Y });
                    assert!(axis.dot(expected).abs() > 1. - 1e-8);
                    for (index, local) in [anchor, anchor + Vector3::UNIT_X].into_iter().enumerate()
                    {
                        let actual = transform.apply(local);
                        let preview = DVec3::from_array(preview[0].points[index].map(f64::from));
                        assert!(preview.distance(DVec3::new(actual.x, actual.y, actual.z)) < 1e-4);
                    }
                    assert_eq!(scene.document, before);
                }
            }
        }
    }
}

#[test]
fn block_alignment_insertion_respects_active_anchor_paste_and_fresh_geometry() {
    let (mut scene, mut cmd, _) = fixture(false, true, WorkingPlane::default());
    let point = DVec3::new(100., 20., 0.);
    assert!(
        (cmd.aligned_pose(point, [1.; 3], 0.).1.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-8
    );
    cmd.base = DVec3::new(8., 9., 0.);
    assert_eq!(cmd.aligned_pose(point, [1.; 3], 0.), (point, 0.));
    cmd.base = DVec3::new(2., 3., 0.);
    cmd.mode = Mode::Paste;
    assert_eq!(cmd.aligned_pose(point, [1.; 3], 0.), (point, 0.));
    cmd.mode = Mode::Palette;
    let off_plane = point + DVec3::Z;
    assert_eq!(cmd.aligned_pose(off_plane, [1.; 3], 0.), (off_plane, 0.));
    let far = point + DVec3::X;
    assert_eq!(cmd.aligned_pose(far, [1.; 3], 0.), (far, 0.));
    let target = scene
        .document
        .entities()
        .find(|e| matches!(e, EntityType::Line(line) if line.start.x == 100.))
        .unwrap()
        .common()
        .handle;
    scene.document.remove_entity(target);
    cmd.refresh_point_pick_scene(&scene);
    assert_eq!(cmd.aligned_pose(point, [1.; 3], 0.), (point, 0.));
}
