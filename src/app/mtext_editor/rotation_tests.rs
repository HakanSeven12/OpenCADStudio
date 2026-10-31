use super::*;
use crate::app::{Message, OpenCADStudio};

fn app(normal: Vector3) -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let mut mt = MText::with_value(r"\pxqc,t4;Á{\Lé}", Vector3::new(0., 0., 0.));
    mt.normal = normal;
    mt.rotation = 0.123456789012345;
    mt.dwg_x_direction = Some(Vector3::new(mt.rotation.cos(), mt.rotation.sin(), 0.));
    let h = app.tabs[1]
        .scene
        .document
        .add_entity(EntityType::MText(Box::new(mt)))
        .unwrap();
    let mt = mt_of(&app, h).clone();
    app.open_mtext_editor(DVec3::ZERO, Some(h), &mt.value, mt.height, None);
    (app, h)
}

fn mt_of(app: &OpenCADStudio, h: Handle) -> &MText {
    let EntityType::MText(mt) = app.tabs[1].scene.document.get_entity(h).unwrap() else {
        panic!()
    };
    mt
}

#[test]
fn an_untouched_rotation_is_kept_exactly() {
    let (mut app, h) = app(Vector3::new(0., 0., 1.));
    let original = mt_of(&app, h).clone();
    assert_eq!(app.mtext_editor.as_ref().unwrap().rotation, "7.073553");
    assert!(app.mtext_commit());
    assert_eq!(mt_of(&app, h).rotation, original.rotation);
    assert_eq!(mt_of(&app, h).dwg_x_direction, original.dwg_x_direction);
}

#[test]
fn a_typed_rotation_is_applied_and_saved_without_touching_the_text() {
    let (mut app, h) = app(Vector3::new(0., 0., 1.));
    let original = mt_of(&app, h).clone();
    let _ = app.update(Message::MTextRotation("90,0".into()));
    app.mtext_apply();
    assert_eq!(mt_of(&app, h).rotation, std::f64::consts::FRAC_PI_2);
    assert_eq!(mt_of(&app, h).value, original.value);
    assert_eq!(mt_of(&app, h).normal, original.normal);
    // The stored direction belonged to the old angle.
    assert_eq!(mt_of(&app, h).dwg_x_direction, None);
    let saved = mt_of(&app, h).clone();
    assert!(app.mtext_commit());
    assert_eq!(mt_of(&app, h), &saved);
}

#[test]
fn invalid_rotation_and_unverified_text_planes_never_change_the_entity() {
    let (mut app, h) = app(Vector3::new(0., 0.6, 0.8));
    let original = mt_of(&app, h).clone();
    let before = app.mtext_editor.as_ref().unwrap().rotation.clone();
    let _ = app.update(Message::MTextRotation("90".into()));
    assert_eq!(app.mtext_editor.as_ref().unwrap().rotation, before);
    app.mtext_apply();
    assert_eq!(mt_of(&app, h).rotation, original.rotation);
    for invalid in ["-", "NaN", "inf", "1,2.3"] {
        let (mut app, h) = self::app(Vector3::new(0., 0., 1.));
        let _ = app.update(Message::MTextRotation(invalid.into()));
        assert_eq!(app.mtext_editor.as_ref().unwrap().rotation, invalid);
        // Saving is refused and the draft stays open to be corrected.
        assert!(!app.mtext_commit());
        assert!(app.mtext_editor.is_some());
        assert_eq!(mt_of(&app, h).rotation, original.rotation, "{invalid}");
    }
}

#[test]
fn rotation_dwg_dxf_2018_and_2013_roundtrip_matches_codec_control() {
    let (mut app, h) = app(Vector3::new(0., 0., 1.));
    let mut control = app.tabs[1].scene.document.clone();
    let _ = app.update(Message::MTextRotation("90".into()));
    assert!(app.mtext_commit());
    let Some(EntityType::MText(t)) = control.get_entity_mut(h) else {
        panic!()
    };
    t.rotation = std::f64::consts::FRAC_PI_2;
    t.dwg_x_direction = None;
    for ext in ["dwg", "dxf"] {
        for version in [codec::DxfVersion::AC1032, codec::DxfVersion::AC1027] {
            let reload = |doc: &codec::Document| {
                let bytes = crate::io::save_to_bytes(doc, ext, version).unwrap();
                crate::io::load_bytes(&format!("rotation.{ext}"), bytes).unwrap()
            };
            let actual = reload(&app.tabs[1].scene.document);
            let expected = reload(&control);
            assert_eq!(
                actual.get_entity(h),
                expected.get_entity(h),
                "{ext} {version:?}"
            );
            let Some(EntityType::MText(t)) = actual.get_entity(h) else {
                panic!()
            };
            assert!((t.rotation.to_degrees() - 90.).abs() < 1e-10);
        }
    }
}

#[test]
fn drawing_layout_uses_the_saved_angle_while_editor_preview_stays_upright() {
    let (mut app, h) = app(Vector3::new(0., 0., 1.));
    let _ = app.update(Message::MTextRotation("90".into()));
    let preview = app.mtext_editor.as_ref().unwrap().preview_wires.clone();
    app.mtext_apply();
    let layout =
        crate::entities::mtext::layout_of(mt_of(&app, h), &app.tabs[1].scene.document, false);
    let runs: Vec<_> = layout
        .strokes
        .iter()
        .filter_map(|s| s.run.as_ref())
        .collect();
    assert!(!runs.is_empty());
    assert!(runs
        .iter()
        .all(|r| (r.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6));
    // The editor's strokes are laid out upright; the panel turns them only
    // when drawing, so caret and selection keep their layout frame.
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(preview.len(), ed.preview_wires.len());
    for (before, after) in preview.iter().zip(&ed.preview_wires) {
        // Stroke separators are NaNs, which are never equal via PartialEq.
        assert_eq!(before.points.len(), after.points.len());
        for (a, b) in before.points.iter().zip(&after.points) {
            for (a, b) in a.iter().zip(b) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
    }
}

#[test]
fn new_text_on_an_unverified_working_plane_takes_no_rotation() {
    let (mut app, _) = app(Vector3::new(0., 0., 1.));
    app.mtext_editor = None;
    app.open_mtext_editor(DVec3::ZERO, None, "new", 2.5, None);
    let ed = app.mtext_editor.as_mut().unwrap();
    ed.rotation_plane_supported = false;
    let _ = app.update(Message::MTextRotation("90".into()));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(ed.rotation, "0");
    assert!(!ed.rotation_dirty);
    assert_eq!(ed.build_mtext().rotation, 0.0);
}

/// The in-plane x direction of a text on plane `normal` turned by `r`: the
/// arbitrary-axis OCS x and y axes, combined as the rotation turns them.
fn in_plane_x(normal: Vector3, r: f64) -> Vector3 {
    let n = glam::DVec3::new(normal.x, normal.y, normal.z).normalize();
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 {
        glam::DVec3::Y.cross(n)
    } else {
        glam::DVec3::Z.cross(n)
    }
    .normalize();
    let ay = n.cross(ax);
    let x = ax * r.cos() + ay * r.sin();
    Vector3::new(x.x, x.y, x.z)
}

/// Acceptance for a codec change: a text on a tilted plane keeps its
/// in-plane x direction through DWG. The pinned codec writes
/// `(cos r, sin r, 0)`, which leaves the text plane for any normal but Z;
/// until it is fixed the editor offers rotation on the XY plane only.
#[test]
#[ignore = "needs the codec to write and read an in-plane MTEXT x direction"]
fn tilted_plane_text_keeps_its_in_plane_x_direction_through_dwg() {
    let (app, h) = app(Vector3::new(0., 0.6, 0.8));
    let original = mt_of(&app, h).clone();
    let mut doc = app.tabs[1].scene.document.clone();
    let x = in_plane_x(original.normal, original.rotation);
    let Some(EntityType::MText(t)) = doc.get_entity_mut(h) else {
        panic!()
    };
    t.dwg_x_direction = Some(x);
    for version in [codec::DxfVersion::AC1018, codec::DxfVersion::AC1032] {
        let bytes = crate::io::save_to_bytes(&doc, "dwg", version).unwrap();
        let saved = crate::io::load_bytes("tilted.dwg", bytes).unwrap();
        let Some(EntityType::MText(back)) = saved.get_entity(h) else {
            panic!()
        };
        let got = back
            .dwg_x_direction
            .unwrap_or_else(|| Vector3::new(back.rotation.cos(), back.rotation.sin(), 0.0));
        let n = original.normal;
        let dot = got.x * n.x + got.y * n.y + got.z * n.z;
        assert!(
            dot.abs() < 1e-9,
            "{version:?}: {got:?} leaves the text plane"
        );
        assert!(
            (got.x - x.x).abs() < 1e-9 && (got.y - x.y).abs() < 1e-9 && (got.z - x.z).abs() < 1e-9,
            "{version:?}: {got:?} != {x:?}"
        );
    }
}
