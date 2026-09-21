use super::OpenCADStudio;
use crate::command::{DimensionAssociationSource, StepInput};
use crate::entities::dim_override;
use crate::scene::viewport_ref::{AcceptedSnap, MeasurementScale, ViewportFrame};
use crate::scene::Scene;
use crate::snap::{SnapResult, SnapType};
use acadrust::entities::{Circle, Dimension, Line, Viewport};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use glam::DVec3;

fn fixture() -> (OpenCADStudio, Handle, ViewportFrame) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let mut scene = Scene::new();
    let line = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::ZERO,
        Vector3::new(100.0, 0.0, 0.0),
    )));
    scene.document.add_layout("Dimensions").unwrap();
    scene.set_current_layout("Dimensions".into());
    let mut viewport = Viewport::new();
    viewport.id = 2;
    viewport.center = Vector3::new(50.0, 50.0, 0.0);
    viewport.width = 100.0;
    viewport.height = 100.0;
    viewport.view_height = 1000.0;
    viewport.custom_scale = 0.1;
    viewport.view_direction = Vector3::UNIT_Z;
    viewport.status.is_on = true;
    let viewport = scene.add_entity(EntityType::Viewport(viewport));
    let frame = scene.viewport_frame(viewport).unwrap();
    scene.document.header.dimension_associativity = 2;
    let i = app.active_tab;
    app.tabs[i].scene = scene;
    (app, line, frame)
}

fn hit(frame: ViewportFrame, source: Handle, model: DVec3) -> SnapResult {
    SnapResult {
        world: frame.model_to_paper(model),
        model_point: Some(model),
        screen: iced::Point::ORIGIN,
        snap_type: SnapType::Endpoint,
        tangent_obj: None,
        extension_base: None,
        extension_base2: None,
        extension_origin: None,
        extension_dir: None,
        viewport: Some(frame.viewport),
        source: Some(DimensionAssociationSource::inferred(source)),
        secondary_source: None,
    }
}

fn point(app: &mut OpenCADStudio, frame: ViewportFrame, source: Handle, model: DVec3) {
    let i = app.active_tab;
    let hit = hit(frame, source, model);
    let paper = hit.world.with_z(0.0);
    assert!(app.record_accepted_snap(i, Some(hit), Some(frame), paper));
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(paper);
    app.sync_dimension_snaps(i);
    let _ = app.apply_cmd_result(result);
}

fn dimension(doc: &CadDocument) -> &Dimension {
    doc.entities()
        .find_map(|e| match e {
            EntityType::Dimension(d) => Some(d),
            _ => None,
        })
        .unwrap()
}

fn displayed(doc: &CadDocument) -> f64 {
    let dim = dimension(doc);
    if matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)) {
        return dim.measurement();
    }
    let factor = dim_override::real(&dim.base().common.extended_data, dim_override::DIMLFAC)
        .or_else(|| {
            doc.dim_styles
                .get(&dim.base().style_name)
                .map(|s| s.dimlfac)
        })
        .unwrap_or(1.0);
    dim.measurement() * MeasurementScale::user_lfac_for_space(factor, true)
}

pub(super) fn preview_text(app: &OpenCADStudio, entity: &EntityType) -> String {
    let EntityType::Dimension(dimension) = entity else { panic!("expected dimension") };
    let doc = &app.tabs[app.active_tab].scene.document;
    let source = doc.dim_styles.get(&dimension.base().style_name).unwrap();
    let style = crate::entities::dimension::resolved_dimension_style(source, dimension, doc);
    crate::entities::dimension::dimension_text_value(dimension, Some(&style)).unwrap()
}

fn finish_aligned(app: &mut OpenCADStudio, line: Handle, frame: ViewportFrame) {
    let _ = app.dispatch_command("DIMALIGNED");
    point(app, frame, line, DVec3::ZERO);
    point(app, frame, line, DVec3::X * 100.0);
    let _ = app.feed_command(StepInput::Point(DVec3::new(250.0, 200.0, 0.0)));
}

#[test]
fn viewport_dimension_navigation_refreshes_after_the_complete_camera_update() {
    let (mut app, _, _, frame) =
        start_centerline_dimension("DIMALIGNED", true, SnapType::Midpoint, 1.0);
    let _ = app.feed_command(StepInput::Point(DVec3::new(60.0, 80.0, 0.0)));
    let scene = &mut app.tabs[app.active_tab].scene;
    assert!((displayed(&scene.document) - 50.0).abs() < 1e-4);
    scene.active_viewport = Some(frame.viewport);
    let mut camera = scene.navigation_camera();
    camera.target.x += 20.0;
    camera.distance *= 0.5;
    assert!(scene.apply_navigation_camera(camera));
    assert!(
        (displayed(&scene.document) - 55.0).abs() < 1e-4,
        "{}",
        displayed(&scene.document)
    );
}

#[test]
fn viewport_dimension_imported_paper_owner_does_not_require_layout_objects() {
    let (mut app, line, frame) = fixture();
    finish_aligned(&mut app, line, frame);
    let doc = &mut app.tabs[app.active_tab].scene.document;
    doc.objects
        .retain(|_, object| !matches!(object, acadrust::objects::ObjectType::Layout(_)));
    assert!(crate::entities::dimension::dimension_in_paper_space(
        dimension(doc),
        doc
    ));
}

#[test]
fn viewport_dimension_curved_clip_includes_the_exact_boundary() {
    let (mut app, _, frame) = fixture();
    let scene = &mut app.tabs[app.active_tab].scene;
    let mut circle = Circle::new();
    circle.center = Vector3::new(50.0, 50.0, 0.0);
    circle.radius = 10.0;
    let boundary = circle.point_at_angle_wcs(0.123456);
    let clip = scene.add_entity(EntityType::Circle(circle));
    let EntityType::Viewport(vp) = scene.document.get_entity_mut(frame.viewport).unwrap() else {
        panic!()
    };
    vp.clip_boundary_handle = clip;
    assert!(scene
        .viewport_displays_paper_point(frame.viewport, glam::DVec2::new(boundary.x, boundary.y)));
    assert!(!scene.viewport_displays_paper_point(frame.viewport, glam::DVec2::new(60.01, 50.0)));
    assert!(!scene.viewport_displays_paper_point(frame.viewport, glam::DVec2::new(35.0, 60.0)));
}

#[test]
fn viewport_dimension_apparent_intersection_extends_only_the_lines() {
    use crate::scene::dimension_assoc_chain::{feature_point, osnap, FeatureContext};
    use acadrust::objects::AssocDimensionReference;
    let mut scene = Scene::new();
    let first = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::ZERO,
        Vector3::UNIT_X,
    )));
    let second = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(2.0, -1.0, 0.0),
        Vector3::new(2.0, -0.5, 0.0),
    )));
    let mut reference = AssocDimensionReference {
        osnap_type: osnap::APPARENT_INT,
        xrefs: vec![first],
        intersection_objects: vec![second],
        ..Default::default()
    };
    let expected = Vector3::new(2.0, 0.0, 0.0);
    let context = FeatureContext {
        hint: Some(expected),
        from: None,
    };
    let entity = scene.document.get_entity(first).unwrap();
    assert_eq!(
        feature_point(&scene.document, entity, &reference, context),
        Some(expected)
    );
    reference.osnap_type = osnap::INTERSEC;
    assert!(feature_point(&scene.document, entity, &reference, context).is_none());
}

#[test]
fn viewport_dimension_projected_snap_keeps_identity_and_elevation() {
    let (_, line, frame) = fixture();
    let model = DVec3::new(100.0, 0.0, 35.0);
    let hit = hit(frame, line, model);
    let accepted =
        AcceptedSnap::from_snap(&hit, Some(frame)).with_paper_point(hit.world.with_z(0.0));
    assert_eq!(accepted.model_point, model);
    assert_eq!(accepted.source.as_ref().unwrap().source.handle, line);
    let moved = accepted.with_paper_point(hit.world + DVec3::X);
    assert!(moved.source.is_none());
    assert!((moved.model_point - frame.paper_to_model(moved.paper_point)).length() < 1e-9);
}

#[test]
fn viewport_dimension_creation_preserves_active_units_and_roundtrips() {
    for factor in [25.4, -25.4] {
        let (mut app, line, frame) = fixture();
        let i = app.active_tab;
        let mut style = acadrust::tables::DimStyle::new("Millimetres");
        style.dimlfac = factor;
        app.tabs[i].scene.document.dim_styles.add(style).unwrap();
        app.tabs[i].scene.document.header.current_dimstyle_name = "Millimetres".into();
        finish_aligned(&mut app, line, frame);
        let doc = &app.tabs[i].scene.document;
        assert_eq!(dimension(doc).base().style_name, "Millimetres");
        assert!((displayed(doc) - 2540.0).abs() < 1e-5, "{}", displayed(doc));
        let data = MeasurementScale::read(&dimension(doc).base().common.extended_data).unwrap();
        assert_eq!(data.user_lfac, 25.4);
        assert_eq!(doc.header.dimension_associativity, 2);
        for ext in ["dxf", "dwg"] {
            let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
            let loaded = crate::io::load_bytes(&format!("viewport.{ext}"), bytes).unwrap();
            assert!(
                (displayed(&loaded) - 2540.0).abs() < 1e-4,
                "{ext}: {}",
                displayed(&loaded)
            );
            assert_eq!(
                MeasurementScale::read(&dimension(&loaded).base().common.extended_data),
                Some(data)
            );
        }
        app.undo_steps(1);
        assert!(!app.tabs[i]
            .scene
            .document
            .entities()
            .any(|e| matches!(e, EntityType::Dimension(_))));
        app.redo_steps(1);
        assert!((displayed(&app.tabs[i].scene.document) - 2540.0).abs() < 1e-5);
    }
}

#[test]
fn viewport_dimension_conflicting_viewports_and_degenerate_picks_allow_retry() {
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("DIMALIGNED");
    point(&mut app, frame, line, DVec3::ZERO);
    point(&mut app, frame, line, DVec3::ZERO);
    assert_eq!(app.accepted_snaps().len(), 1);
    let other = ViewportFrame {
        viewport: Handle::new(0xFFFF),
        ..frame
    };
    assert!(!app.record_accepted_snap(
        i,
        Some(hit(other, line, DVec3::X * 100.0)),
        Some(other),
        DVec3::new(60.0, 50.0, 0.0)
    ));
    point(&mut app, frame, line, DVec3::X * 100.0);
    let _ = app.feed_command(StepInput::Point(DVec3::new(250.0, 200.0, 0.0)));
    assert!((displayed(&app.tabs[i].scene.document) - 100.0).abs() < 1e-5);
}

#[test]
fn viewport_dimension_typed_sheet_points_keep_paper_units() {
    let (mut app, _, _) = fixture();
    let result = app.automation_op(r#"{"op":"run","cmd":"DIMALIGNED 50,50 60,50 60,55"}"#);
    assert_eq!(result["ok"], true);
    let doc = &app.tabs[app.active_tab].scene.document;
    assert!((displayed(doc) - 10.0).abs() < 1e-9);
    assert!(MeasurementScale::read(&dimension(doc).base().common.extended_data).is_none());
}

#[test]
fn viewport_dimension_linear_aligned_and_radial_object_picks() {
    for (name, expected) in [
        ("DIMLINEAR", 100.0),
        ("DIMALIGNED", 100.0),
        ("DIMRADIUS", 100.0),
        ("DIMDIAMETER", 200.0),
    ] {
        let (mut app, _, frame) = fixture();
        let i = app.active_tab;
        let radial = name == "DIMRADIUS" || name == "DIMDIAMETER";
        let model = if radial {
            let mut circle = Circle::new();
            circle.center = Vector3::new(0.0, 200.0, 0.0);
            circle.radius = 100.0;
            app.tabs[i].scene.set_current_layout("Model".into());
            app.tabs[i].scene.add_entity(EntityType::Circle(circle));
            app.tabs[i].scene.set_current_layout("Dimensions".into());
            DVec3::new(100.0, 200.0, 0.0)
        } else {
            DVec3::new(50.0, 0.0, 0.0)
        };
        let _ = app.dispatch_command(name);
        if !radial {
            let _ = app.feed_command(StepInput::Enter);
        }
        let result = app
            .try_dimension_viewport_entity_pick(i, frame.model_to_paper(model), 0.5)
            .unwrap();
        let _ = app.apply_cmd_result(result);
        assert_eq!(
            app.accepted_snaps().len(),
            if radial { 1 } else { 2 },
            "{name}"
        );
        let place = DVec3::new(55.0, 80.0, 0.0);
        let preview = app.dimension_preview_entities(i, place).unwrap();
        assert_eq!(preview.len(), 1, "{name}");
        let value = preview_text(&app, &preview[0]);
        assert!(!app.dimension_preview_wires(i, place).unwrap().is_empty(), "{name}");
        let _ = app.feed_command(StepInput::Point(place));
        assert_eq!(preview_text(&app, &EntityType::Dimension(dimension(&app.tabs[i].scene.document).clone())), value, "{name}");
        assert!(
            (displayed(&app.tabs[i].scene.document) - expected).abs() < 1e-4,
            "{name}: {}",
            displayed(&app.tabs[i].scene.document)
        );
    }
}

#[test]
fn viewport_dimension_angular_preserves_angle_without_length_factor() {
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.set_current_layout("Model".into());
    let vertical = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(0.0, 100.0, 0.0),
        )));
    app.tabs[i].scene.set_current_layout("Dimensions".into());
    let _ = app.dispatch_command("DIMANGULAR");
    let _ = app.feed_command(StepInput::Enter);
    point(&mut app, frame, line, DVec3::ZERO);
    point(&mut app, frame, line, DVec3::X * 100.0);
    point(&mut app, frame, vertical, DVec3::Y * 100.0);
    let place = DVec3::new(70.0, 70.0, 0.0);
    let preview = app.dimension_preview_entities(i, place).unwrap();
    assert_eq!(preview.len(), 1);
    assert!(MeasurementScale::read(&preview[0].common().extended_data).is_none());
    let value = preview_text(&app, &preview[0]);
    let _ = app.feed_command(StepInput::Point(place));
    let doc = &app.tabs[app.active_tab].scene.document;
    assert_eq!(preview_text(&app, &EntityType::Dimension(dimension(doc).clone())), value);
    assert!((dimension(doc).measurement() - 90.0).abs() < 1e-5);
    assert!(MeasurementScale::read(&dimension(doc).base().common.extended_data).is_none());
}

#[test]
fn viewport_dimension_preview_uses_candidate_scale_only_before_second_pick() {
    for command in ["DIMLINEAR", "DIMALIGNED"] {
        let (mut app, model, frame) = fixture();
        let i = app.active_tab;
        let mut style = acadrust::tables::DimStyle::new("PreviewUnits");
        style.dimlfac = 25.4;
        app.tabs[i].scene.document.dim_styles.add(style).unwrap();
        app.tabs[i].scene.document.header.current_dimstyle_name = "PreviewUnits".into();
        let _ = app.dispatch_command(command);
        let _ = app.feed_command(StepInput::Point(DVec3::new(55.0, 50.0, 0.0)));
        let target = hit(frame, model, DVec3::X * 100.0);
        let sheet = app.dimension_preview_entities(i, target.world).unwrap();
        assert_eq!(preview_text(&app, &sheet[0]), "127");
        app.vp_snap_frame = Some(frame);
        app.tabs[i].snap_result = Some(target);
        let model_preview = app.dimension_preview_entities(i, target.world).unwrap();
        assert_eq!(preview_text(&app, &model_preview[0]), "1270");
        point(&mut app, frame, model, DVec3::X * 100.0);
        let other = ViewportFrame { viewport: Handle::new(0xFFFF), scale: frame.scale * 2.0, ..frame };
        app.vp_snap_frame = Some(other);
        app.tabs[i].snap_result = Some(hit(other, model, DVec3::X * 100.0));
        let placement = app.dimension_preview_entities(i, DVec3::new(60.0, 80.0, 0.0)).unwrap();
        assert_eq!(preview_text(&app, &placement[0]), "1270");
    }
}

#[test]
fn viewport_dimension_snap_query_matches_pan_scale_and_twist() {
    for (height, twist, target) in [
        (1000.0, 0.0, Vector3::ZERO),
        (500.0, 0.45, Vector3::new(80.0, 20.0, 0.0)),
    ] {
        let (mut app, line, initial) = fixture();
        let i = app.active_tab;
        let Some(EntityType::Viewport(vp)) =
            app.tabs[i].scene.document.get_entity_mut(initial.viewport)
        else {
            panic!()
        };
        vp.view_height = height;
        vp.custom_scale = vp.height / height;
        vp.twist_angle = twist;
        vp.view_target = target;
        let frame = app.tabs[i].scene.viewport_frame(initial.viewport).unwrap();
        let model = DVec3::X * 100.0;
        let paper = frame.model_to_paper(model);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        };
        let screen = {
            let cam = app.tabs[i].scene.camera.borrow();
            let ndc = cam
                .view_proj_rte(bounds)
                .project_point3((paper - cam.eye()).as_vec3());
            iced::Point::new((ndc.x + 1.0) * 500.0, (1.0 - ndc.y) * 500.0)
        };
        app.snapper.snap_enabled = true;
        let (hit, acquired_frame) = app
            .paper_viewport_snap(i, screen, (1000.0, 1000.0), paper)
            .unwrap();
        assert_eq!(hit.source.unwrap().handle, line);
        assert!((hit.world - paper).length() < 1e-5, "{hit:?}");
        let accepted = AcceptedSnap::from_snap(&hit, Some(acquired_frame));
        assert!((accepted.model_point - model).length() < 1e-6);
    }
}

#[test]
fn viewport_dimension_eligibility_is_shared_and_uses_clipping() {
    use acadrust::entities::LwPolyline;
    use acadrust::types::Vector2;
    let (mut app, _, first) = fixture();
    let scene = &mut app.tabs[app.active_tab].scene;
    let mut overlay = scene.document.get_entity(first.viewport).unwrap().clone();
    if let EntityType::Viewport(vp) = &mut overlay {
        vp.id = 3;
        vp.width = 200.0;
    }
    overlay.common_mut().handle = Handle::NULL;
    let top = scene.add_entity(overlay);
    let paper = DVec3::new(55.0, 50.0, 0.0);
    assert_eq!(
        scene
            .viewport_frames_at_paper_point(paper)
            .into_iter()
            .next()
            .unwrap()
            .viewport,
        top
    );
    assert_eq!(
        scene
            .dimension_pick_through_viewport(paper, 0.2)
            .unwrap()
            .frame
            .viewport,
        top
    );
    let mut clip = LwPolyline::from_points(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 0.0),
        Vector2::new(0.0, 100.0),
    ]);
    clip.is_closed = true;
    let clip = scene.add_entity(EntityType::LwPolyline(clip));
    if let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(top) {
        vp.clip_boundary_handle = clip;
    }
    assert_eq!(
        scene
            .viewport_frames_at_paper_point(paper)
            .into_iter()
            .next()
            .unwrap()
            .viewport,
        first.viewport
    );
    assert_eq!(
        scene
            .dimension_pick_through_viewport(paper, 0.2)
            .unwrap()
            .frame
            .viewport,
        first.viewport
    );
    if let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(first.viewport) {
        vp.status.is_on = false;
    }
    assert!(scene.viewport_frames_at_paper_point(paper).is_empty());
    assert!(scene.dimension_pick_through_viewport(paper, 0.2).is_none());
}

#[test]
fn viewport_dimension_exploded_commit_uses_compensated_text_and_undo() {
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.document.header.dimension_associativity = 0;
    finish_aligned(&mut app, line, frame);
    let doc = &app.tabs[i].scene.document;
    assert!(!doc
        .entities()
        .any(|e| matches!(e, EntityType::Dimension(_))));
    let texts: Vec<_> = doc
        .entities()
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            EntityType::MText(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t.contains("100")), "{texts:?}");
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.header.dimension_associativity, 0);
    app.redo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .entities()
        .any(|e| matches!(e, EntityType::Text(_) | EntityType::MText(_))));
}

#[test]
fn viewport_dimension_association_scale_edit_picture_and_history() {
    use crate::scene::ChangeKind;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    finish_aligned(&mut app, line, frame);
    let handle = dimension(&app.tabs[i].scene.document).base().common.handle;
    assert_eq!(
        app.tabs[i].scene.dimension_association_status(handle).len(),
        2
    );
    let Some(EntityType::Dimension(d)) = app.tabs[i].scene.document.get_entity_mut(handle) else {
        panic!()
    };
    d.base_mut().text_user_positioned = true;
    d.base_mut().text_middle_point = Vector3::new(250.0, 200.0, 0.0);
    d.base_mut().block_name = "*SavedPicture".into();
    let pending = app.begin_undo(i, "Viewport scale", 1, true).unwrap();
    let before = app.tabs[i].scene.document.get_entity_arc(frame.viewport);
    app.tabs[i].scene.record_undo_before(frame.viewport, before);
    let Some(EntityType::Viewport(vp)) = app.tabs[i].scene.document.get_entity_mut(frame.viewport)
    else {
        panic!()
    };
    vp.view_height = 500.0;
    vp.custom_scale = 0.2;
    app.tabs[i].scene.notify_viewport_changed(frame.viewport);
    app.commit_undo_delta(i, pending);
    assert!((displayed(&app.tabs[i].scene.document) - 100.0).abs() < 1e-5);
    assert!((dimension(&app.tabs[i].scene.document).measurement() - 20.0).abs() < 1e-5);
    assert!(dimension(&app.tabs[i].scene.document)
        .base()
        .block_name
        .is_empty());
    assert_eq!(
        dimension(&app.tabs[i].scene.document)
            .base()
            .text_middle_point,
        Vector3::new(250.0, 200.0, 0.0)
    );
    app.undo_steps(1);
    assert!((dimension(&app.tabs[i].scene.document).measurement() - 10.0).abs() < 1e-5);
    assert_eq!(
        dimension(&app.tabs[i].scene.document).base().block_name,
        "*SavedPicture"
    );
    app.redo_steps(1);
    assert!((displayed(&app.tabs[i].scene.document) - 100.0).abs() < 1e-5);
    let pending = app.begin_undo(i, "Edit source", 1, true).unwrap();
    let before = app.tabs[i].scene.document.get_entity_arc(line);
    app.tabs[i].scene.record_undo_before(line, before);
    let Some(EntityType::Line(source)) = app.tabs[i].scene.document.get_entity_mut(line) else {
        panic!()
    };
    source.end.x = 175.0;
    app.tabs[i]
        .scene
        .bump_entities(&[(line, ChangeKind::Modified)]);
    app.commit_undo_delta(i, pending);
    assert!((displayed(&app.tabs[i].scene.document) - 175.0).abs() < 1e-5);
    let pieces = crate::modules::draw::modify::explode::explode_entity(
        app.tabs[i].scene.document.get_entity(handle).unwrap(),
        &app.tabs[i].scene.document,
    );
    assert!(
        pieces.iter().any(
            |e| matches!(e,EntityType::Text(t) if t.value.contains("175"))
                || matches!(e,EntityType::MText(t) if t.value.contains("175"))
        ),
        "{pieces:?}"
    );
    for ext in ["dxf", "dwg"] {
        let doc = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
        let mut loaded = Scene::new();
        loaded.document = crate::io::load_bytes(&format!("assoc.{ext}"), bytes).unwrap();
        loaded.notify_viewport_changed(frame.viewport);
        assert!((displayed(&loaded.document) - 175.0).abs() < 1e-5, "{ext}");
        assert_eq!(loaded.dimension_association_status(handle).len(), 2);
    }
    app.undo_steps(1);
    assert!((displayed(&app.tabs[i].scene.document) - 100.0).abs() < 1e-5);
}

#[test]
fn viewport_dimension_unsupported_and_erased_sources_keep_last_picture() {
    use crate::scene::dimension_assoc::{resolve_reference_chain, ReferenceStatus};
    use acadrust::objects::AssocDimensionReference;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    finish_aligned(&mut app, line, frame);
    let handle = dimension(&app.tabs[i].scene.document).base().common.handle;
    let reference = AssocDimensionReference {
        xrefs: vec![frame.viewport, line],
        osnap_type: 255,
        ..Default::default()
    };
    assert!(matches!(
        resolve_reference_chain(&app.tabs[i].scene, &reference, None),
        Err(ReferenceStatus::Unresolved)
    ));
    let previous = app.tabs[i]
        .scene
        .document
        .get_entity(handle)
        .unwrap()
        .clone();
    let Some(EntityType::Viewport(vp)) = app.tabs[i].scene.document.get_entity_mut(frame.viewport)
    else {
        panic!()
    };
    vp.status.perspective = true;
    app.tabs[i].scene.notify_viewport_changed(frame.viewport);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(handle).unwrap(),
        &previous
    );
    assert!(app.tabs[i]
        .scene
        .dimension_association_status(handle)
        .iter()
        .all(|(_, s)| *s == ReferenceStatus::Unresolved));
    let Some(EntityType::Viewport(vp)) = app.tabs[i].scene.document.get_entity_mut(frame.viewport)
    else {
        panic!()
    };
    vp.status.perspective = false;
    let pending = app.begin_undo(i, "Erase source", 1, false).unwrap();
    app.tabs[i].scene.erase_entities(&[line]);
    app.commit_undo_delta(i, pending);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(handle).unwrap(),
        &previous
    );
    assert!(app.tabs[i]
        .scene
        .dimension_association_status(handle)
        .iter()
        .all(|(_, s)| *s == ReferenceStatus::Broken(line)));
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .dimension_association_status(handle)
        .iter()
        .all(|(_, s)| *s == ReferenceStatus::Resolved));
}

#[test]
fn viewport_dimension_intersection_tracks_both_entities() {
    use crate::scene::ChangeKind;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.set_current_layout("Model".into());
    let other = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(60.0, -50.0, 0.0),
            Vector3::new(60.0, 50.0, 0.0),
        )));
    app.tabs[i].scene.set_current_layout("Dimensions".into());
    let _ = app.dispatch_command("DIMALIGNED");
    point(&mut app, frame, line, DVec3::ZERO);
    let mut intersection = hit(frame, line, DVec3::X * 60.0);
    intersection.snap_type = SnapType::Intersection;
    intersection.secondary_source = Some(DimensionAssociationSource::inferred(other));
    assert!(app.record_accepted_snap(i, Some(intersection), Some(frame), intersection.world));
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(intersection.world);
    app.sync_dimension_snaps(i);
    let _ = app.apply_cmd_result(result);
    let _ = app.feed_command(StepInput::Point(DVec3::new(80.0, 80.0, 0.0)));
    let handle = dimension(&app.tabs[i].scene.document).base().common.handle;
    assert!(app.tabs[i]
        .scene
        .dimension_association_sources(handle)
        .contains(&other));
    assert!((displayed(&app.tabs[i].scene.document) - 60.0).abs() < 1e-5);
    let Some(EntityType::Line(source)) = app.tabs[i].scene.document.get_entity_mut(other) else {
        panic!()
    };
    source.start.x = 80.0;
    source.end.x = 80.0;
    app.tabs[i]
        .scene
        .bump_entities(&[(other, ChangeKind::Modified)]);
    assert!((displayed(&app.tabs[i].scene.document) - 80.0).abs() < 1e-5);
}

#[test]
fn viewport_dimension_spline_tangency_is_parallel_and_validated() {
    use crate::scene::dimension_assoc_chain::{feature_point, osnap, FeatureContext};
    use acadrust::objects::AssocDimensionReference;
    let mut spline = acadrust::entities::Spline::new();
    spline.degree = 2;
    spline.control_points = vec![
        Vector3::ZERO,
        Vector3::new(0.5, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 0.0),
    ];
    spline.knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let entity = EntityType::Spline(spline);
    let reference = AssocDimensionReference {
        osnap_type: osnap::TAN,
        osnap_distance: 0.8,
        ..Default::default()
    };
    let result = feature_point(
        &CadDocument::new(),
        &entity,
        &reference,
        FeatureContext {
            hint: Some(Vector3::new(1.0, 1.0, 0.0)),
            from: Some(Vector3::new(0.0, -1.0, 0.0)),
        },
    )
    .unwrap();
    assert!(
        (result.x - 1.0).abs() < 1e-6 && (result.y - 1.0).abs() < 1e-6,
        "{result:?}"
    );
    assert!(feature_point(
        &CadDocument::new(),
        &entity,
        &reference,
        FeatureContext {
            hint: None,
            from: Some(Vector3::new(0.0, 1.0, 0.0))
        }
    )
    .is_none());
}

#[test]
#[ignore = "Set OPENCAD_VIEWPORT_REGRESSION_DXF to an external regression fixture"]
fn viewport_dimension_external_fixture_keeps_unchanged_measurements() {
    use crate::scene::ChangeKind;
    let path = std::env::var("OPENCAD_VIEWPORT_REGRESSION_DXF").expect("regression drawing path");
    let doc = acadrust::DxfReader::from_file(std::path::Path::new(&path))
        .unwrap()
        .read()
        .unwrap();
    let mut scene = Scene::new();
    scene.document = doc;
    let dimensions: Vec<_> = scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Dimension(dim) => {
                Some((dim.base().common.handle, dim.base().actual_measurement))
            }
            _ => None,
        })
        .collect();
    let mut sources: Vec<_> = dimensions
        .iter()
        .flat_map(|(h, _)| scene.dimension_association_sources(*h))
        .collect();
    sources.sort();
    sources.dedup();
    scene.bump_entities(
        &sources
            .into_iter()
            .map(|h| (h, ChangeKind::Modified))
            .collect::<Vec<_>>(),
    );
    for (handle, saved) in dimensions {
        if scene.dimension_association_status(handle).is_empty() {
            continue;
        }
        let EntityType::Dimension(dim) = scene.document.get_entity(handle).unwrap() else {
            panic!()
        };
        let factor = dim_override::real(&dim.base().common.extended_data, dim_override::DIMLFAC)
            .unwrap_or(1.0)
            .abs();
        let result = dim.measurement() * factor;
        println!(
            "{:X}: saved={saved:.9}, current={result:.9}, status={:?}",
            handle.value(),
            scene.dimension_association_status(handle)
        );
        assert!(
            (result - saved).abs() < 1e-3,
            "{:X}: {saved} -> {result}",
            handle.value()
        );
    }
}

#[test]
fn viewport_dimension_fixture_regenerates_and_tracks_an_intersection() {
    use crate::scene::ChangeKind;
    let bytes = include_bytes!("../../tests/fixtures/dim_assoc/viewport_associations.dxf").to_vec();
    let mut scene = Scene::new();
    scene.document = crate::io::load_bytes("viewport_associations.dxf", bytes).unwrap();
    scene.notify_viewport_changed(Handle::new(0x200));
    // The cubic's tangent from (0,300) touches at u=.75, C=(90,283.125).
    let EntityType::Spline(spline) = scene.document.get_entity(Handle::new(0x104)).unwrap() else {
        panic!()
    };
    let curve = crate::entities::spline::nurbs3(spline).unwrap();
    let c = DVec3::from_array(curve.point_at_knot(0.75));
    let tangent = DVec3::from_array(curve.derivative_at_knot(0.75));
    assert!((c - DVec3::new(0.0, 300.0, 0.0)).cross(tangent).length() < 1e-8);
    let measured = |scene: &Scene, handle| {
        let EntityType::Dimension(d) = scene.document.get_entity(Handle::new(handle)).unwrap()
        else {
            panic!()
        };
        d.measurement()
            * dim_override::real(&d.base().common.extended_data, dim_override::DIMLFAC)
                .unwrap_or(1.0)
                .abs()
    };
    for (handle, expected) in [
        (0x201, 100.0),
        (0x202, 50.0),
        (0x203, 35.355339),
        (0x204, 141.421356),
        (0x205, 90.0_f64.hypot(16.875)),
        (0x206, 80.0),
        (0x207, 60.0),
    ] {
        assert!(
            (measured(&scene, handle) - expected).abs() < 1e-4,
            "{handle:X}: {}",
            measured(&scene, handle)
        );
    }
    let Some(EntityType::Line(line)) = scene.document.get_entity_mut(Handle::new(0x107)) else {
        panic!()
    };
    line.start.x = 80.0;
    line.end.x = 80.0;
    scene.bump_entities(&[(Handle::new(0x107), ChangeKind::Modified)]);
    assert!((measured(&scene, 0x207) - 80.0).abs() < 1e-5);
    let before = scene
        .document
        .get_entity(Handle::new(0x205))
        .unwrap()
        .clone();
    for types in [(1, 255), (8, 9)] {
        let acadrust::objects::ObjectType::Associative(object) =
            scene.document.objects.get_mut(&Handle::new(0x304)).unwrap()
        else {
            panic!()
        };
        let acadrust::objects::AssociativeData::DimensionAssociation(assoc) = &mut object.data
        else {
            panic!()
        };
        assoc.references[0][0].osnap_type = types.0;
        assoc.references[1][0].osnap_type = types.1;
        scene.notify_viewport_changed(Handle::new(0x200));
        assert_eq!(
            scene.document.get_entity(Handle::new(0x205)).unwrap(),
            &before
        );
        assert!(scene
            .dimension_association_status(Handle::new(0x205))
            .iter()
            .any(|(_, s)| *s == crate::scene::ReferenceStatus::Unresolved));
    }
}

#[test]
fn viewport_dimension_nearest_parameter_survives_source_stretch() {
    use crate::scene::ChangeKind;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("DIMALIGNED");
    point(&mut app, frame, line, DVec3::ZERO);
    let mut near = hit(frame, line, DVec3::X * 75.0);
    near.snap_type = SnapType::Nearest;
    assert!(app.record_accepted_snap(i, Some(near), Some(frame), near.world));
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(near.world);
    app.sync_dimension_snaps(i);
    let _ = app.apply_cmd_result(result);
    let _ = app.feed_command(StepInput::Point(DVec3::new(55.0, 80.0, 0.0)));
    assert!((displayed(&app.tabs[i].scene.document) - 75.0).abs() < 1e-5);
    let Some(EntityType::Line(source)) = app.tabs[i].scene.document.get_entity_mut(line) else {
        panic!()
    };
    source.end.x = 200.0;
    app.tabs[i]
        .scene
        .bump_entities(&[(line, ChangeKind::Modified)]);
    assert!((displayed(&app.tabs[i].scene.document) - 150.0).abs() < 1e-5);
}

#[test]
fn viewport_dimension_nested_block_path_survives_edit_and_copy() {
    use crate::scene::ChangeKind;
    use acadrust::entities::Insert;
    use acadrust::tables::BlockRecord;
    let (mut app, _, frame) = fixture();
    let i = app.active_tab;
    let scene = &mut app.tabs[i].scene;
    let mut inner = BlockRecord::new("Inner");
    inner.handle = scene.document.allocate_handle();
    let inner_h = inner.handle;
    scene.document.block_records.add(inner).unwrap();
    let mut leaf = Line::from_points(Vector3::ZERO, Vector3::new(20.0, 0.0, 0.0));
    leaf.common.owner_handle = inner_h;
    let leaf = scene.document.add_entity(EntityType::Line(leaf)).unwrap();
    let mut outer = BlockRecord::new("Outer");
    outer.handle = scene.document.allocate_handle();
    let outer_h = outer.handle;
    scene.document.block_records.add(outer).unwrap();
    let mut nested = Insert::new("Inner", Vector3::new(30.0, 0.0, 0.0));
    nested.common.owner_handle = outer_h;
    nested.set_x_scale(3.0);
    nested.set_y_scale(3.0);
    nested.rotation = 0.4;
    let nested = scene
        .document
        .add_entity(EntityType::Insert(nested))
        .unwrap();
    scene.set_current_layout("Model".into());
    let mut root = Insert::new("Outer", Vector3::new(20.0, 50.0, 0.0));
    root.set_x_scale(2.0);
    root.set_y_scale(2.0);
    root.rotation = 0.3;
    let root = scene.add_entity(EntityType::Insert(root));
    scene.set_current_layout("Dimensions".into());
    let transform =
        crate::scene::dimension_assoc_chain::walk_chain(&scene.document, &[root, nested, leaf])
            .unwrap()
            .transform;
    let model = |x| {
        let v = transform.apply(Vector3::new(x, 0.0, 0.0));
        DVec3::new(v.x, v.y, v.z)
    };
    let _ = app.dispatch_command("DIMALIGNED");
    point(&mut app, frame, root, model(0.0));
    let accepted = app.accepted_snaps()[0].source.as_ref().unwrap();
    assert_eq!(accepted.source.handle, leaf);
    assert_eq!(accepted.block_path, vec![root, nested]);
    point(&mut app, frame, root, model(20.0));
    let _ = app.feed_command(StepInput::Point(DVec3::new(90.0, 90.0, 0.0)));
    let handle = dimension(&app.tabs[i].scene.document).base().common.handle;
    assert!((displayed(&app.tabs[i].scene.document) - 120.0).abs() < 1e-5);
    let Some(EntityType::Line(source)) = app.tabs[i].scene.document.get_entity_mut(leaf) else {
        panic!()
    };
    source.end.x = 40.0;
    app.tabs[i]
        .scene
        .bump_entities(&[(leaf, ChangeKind::Modified)]);
    assert!((displayed(&app.tabs[i].scene.document) - 240.0).abs() < 1e-5);
    let copied = app.tabs[i].scene.copy_entities(
        &[handle],
        &crate::command::EntityTransform::Translate(DVec3::new(10.0, 10.0, 0.0)),
    );
    assert_eq!(copied.len(), 1);
    assert!(app.tabs[i].scene.dimension_association(copied[0]).is_none());
    assert!(app.tabs[i].scene.dimension_association(handle).is_some());
}

#[test]
fn viewport_dimension_hidden_border_and_lock_keep_content_available() {
    let (mut app, _, frame) = fixture();
    let scene = &mut app.tabs[app.active_tab].scene;
    let mut layer = acadrust::tables::Layer::new("Viewport frames");
    layer.flags.off = true;
    scene.document.layers.add(layer).unwrap();
    let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(frame.viewport) else {
        panic!()
    };
    vp.common.layer = "Viewport frames".into();
    vp.status.locked = true;
    let pick = scene
        .dimension_pick_through_viewport(DVec3::new(55.0, 50.0, 0.0), 0.2)
        .unwrap();
    assert!(pick.frame.locked);
    assert_eq!(pick.frame.viewport, frame.viewport);
}

#[test]
fn viewport_dimension_perpendicular_uses_transformed_geometry() {
    use crate::scene::dimension_assoc::{resolve_reference_chain, ReferenceStatus};
    use acadrust::entities::Insert;
    use acadrust::objects::AssocDimensionReference;
    use acadrust::tables::BlockRecord;
    let mut scene = Scene::new();
    let mut block = BlockRecord::new("Stretched");
    block.handle = scene.document.allocate_handle();
    let owner = block.handle;
    scene.document.block_records.add(block).unwrap();
    let mut source = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 10.0, 0.0));
    source.common.owner_handle = owner;
    let source = scene.document.add_entity(EntityType::Line(source)).unwrap();
    let mut insert = Insert::new("Stretched", Vector3::ZERO);
    insert.set_x_scale(2.0);
    let root = scene.add_entity(EntityType::Insert(insert));
    let reference = AssocDimensionReference {
        xrefs: vec![root, source],
        osnap_type: 8,
        osnap_point: Vector3::new(4.0, 2.0, 0.0),
        ..Default::default()
    };
    let result =
        resolve_reference_chain(&scene, &reference, Some(Vector3::new(0.0, 10.0, 0.0))).unwrap();
    assert!(
        (result.model.x - 4.0).abs() < 1e-8 && (result.model.y - 2.0).abs() < 1e-8,
        "{result:?}"
    );
    let unrelated = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::ZERO,
        Vector3::UNIT_X,
    )));
    let invalid = AssocDimensionReference {
        xrefs: vec![root, unrelated],
        ..reference
    };
    assert!(matches!(
        resolve_reference_chain(&scene, &invalid, None),
        Err(ReferenceStatus::Unresolved)
    ));
}

#[test]
fn viewport_dimension_refresh_preserves_explicit_overrides() {
    use acadrust::xdata::XDataValue;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    finish_aligned(&mut app, line, frame);
    let handle = dimension(&app.tabs[i].scene.document).base().common.handle;
    let entity = app.tabs[i].scene.document.get_entity_mut(handle).unwrap();
    dim_override::set_on_entity(
        entity,
        dim_override::DIMLFAC,
        Some(XDataValue::Real(-254.0)),
    );
    dim_override::set_on_entity(entity, dim_override::DIMTOL, Some(XDataValue::Integer16(1)));
    dim_override::set_on_entity(entity, dim_override::DIMTP, Some(XDataValue::Real(0.05)));
    dim_override::set_on_entity(entity, dim_override::DIMTM, Some(XDataValue::Real(0.02)));
    if let EntityType::Dimension(d) = entity {
        crate::entities::dimension::set_dimension_text_override(d.base_mut(), Some("L=<>".into()));
    }
    let expected = displayed(&app.tabs[i].scene.document);
    let Some(EntityType::Viewport(vp)) = app.tabs[i].scene.document.get_entity_mut(frame.viewport)
    else {
        panic!()
    };
    vp.view_height = 500.0;
    vp.custom_scale = 0.2;
    app.tabs[i].scene.notify_viewport_changed(frame.viewport);
    let d = dimension(&app.tabs[i].scene.document);
    assert!(
        (displayed(&app.tabs[i].scene.document) - expected).abs() < 1e-5,
        "{} vs {expected}",
        displayed(&app.tabs[i].scene.document)
    );
    assert_eq!(
        crate::entities::dimension::dimension_text_override(d.base()),
        Some("L=<>")
    );
    assert_eq!(
        dim_override::real(&d.base().common.extended_data, dim_override::DIMTP),
        Some(0.05)
    );
    assert_eq!(
        dim_override::real(&d.base().common.extended_data, dim_override::DIMTM),
        Some(0.02)
    );
}

#[test]
fn viewport_dimension_block_center_keeps_the_circle_source() {
    use acadrust::entities::Insert;
    use acadrust::tables::BlockRecord;
    let (mut app, _, frame) = fixture();
    let i = app.active_tab;
    let scene = &mut app.tabs[i].scene;
    let mut block = BlockRecord::new("Circle block");
    block.handle = scene.document.allocate_handle();
    let owner = block.handle;
    scene.document.block_records.add(block).unwrap();
    let mut circle = Circle::new();
    circle.radius = 10.0;
    circle.common.owner_handle = owner;
    let circle = scene
        .document
        .add_entity(EntityType::Circle(circle))
        .unwrap();
    // This line is closer to the circle center than its circumference is.
    let mut line = Line::from_points(Vector3::new(-2.0, 1.0, 0.0), Vector3::new(2.0, 1.0, 0.0));
    line.common.owner_handle = owner;
    scene.document.add_entity(EntityType::Line(line)).unwrap();
    scene.set_current_layout("Model".into());
    let root = scene.add_entity(EntityType::Insert(Insert::new(
        "Circle block",
        Vector3::new(40.0, 20.0, 0.0),
    )));
    scene.set_current_layout("Dimensions".into());
    let mut center = hit(frame, root, DVec3::new(40.0, 20.0, 0.0));
    center.snap_type = SnapType::Center;
    assert!(app.record_accepted_snap(i, Some(center), Some(frame), center.world));
    let source = app
        .accepted_snaps()
        .last()
        .unwrap()
        .source
        .as_ref()
        .unwrap();
    assert_eq!(source.source.handle, circle);
    assert_eq!(source.block_path, vec![root]);
}

#[test]
fn viewport_dimension_angular_object_picks_follow_arc_features() {
    use crate::scene::{ChangeKind, ReferenceStatus};
    use acadrust::entities::{Arc, LwPolyline};
    use acadrust::types::Vector2;
    for bulged in [false, true] {
        let (mut app, _, frame) = fixture();
        let i = app.active_tab;
        app.tabs[i].scene.set_current_layout("Model".into());
        let entity = if bulged {
            let mut poly =
                LwPolyline::from_points(vec![Vector2::new(100.0, 200.0), Vector2::new(0.0, 300.0)]);
            poly.vertices[0].bulge = (std::f64::consts::PI / 8.0).tan();
            EntityType::LwPolyline(poly)
        } else {
            let mut arc = Arc::new();
            arc.center = Vector3::new(0.0, 200.0, 0.0);
            arc.radius = 100.0;
            arc.start_angle = 0.0;
            arc.end_angle = std::f64::consts::FRAC_PI_2;
            EntityType::Arc(arc)
        };
        let source = app.tabs[i].scene.add_entity(entity);
        app.tabs[i].scene.set_current_layout("Dimensions".into());
        let _ = app.dispatch_command("DIMANGULAR");
        let model = DVec3::new(100.0 / 2.0_f64.sqrt(), 200.0 + 100.0 / 2.0_f64.sqrt(), 0.0);
        let result = app
            .try_dimension_viewport_entity_pick(i, frame.model_to_paper(model), 0.5)
            .unwrap();
        let _ = app.apply_cmd_result(result);
        assert_eq!(app.accepted_snaps().len(), 3);
        let _ = app.feed_command(StepInput::Point(DVec3::new(65.0, 85.0, 0.0)));
        assert!((displayed(&app.tabs[i].scene.document) - 90.0).abs() < 1e-5);
        let dim = dimension(&app.tabs[i].scene.document).base().common.handle;
        app.tabs[i].scene.notify_viewport_changed(frame.viewport);
        assert!((displayed(&app.tabs[i].scene.document) - 90.0).abs() < 1e-5);
        let status = app.tabs[i].scene.dimension_association_status(dim);
        assert_eq!(status.len(), 3);
        assert!(
            status.iter().all(|(_, s)| *s == ReferenceStatus::Resolved),
            "{bulged}: {status:?}"
        );
        match app.tabs[i].scene.document.get_entity_mut(source).unwrap() {
            EntityType::Arc(arc) => arc.end_angle = std::f64::consts::PI / 3.0,
            EntityType::LwPolyline(poly) => {
                poly.vertices[0].bulge = (std::f64::consts::PI / 12.0).tan()
            }
            _ => unreachable!(),
        }
        app.tabs[i]
            .scene
            .bump_entities(&[(source, ChangeKind::Modified)]);
        assert!(
            (displayed(&app.tabs[i].scene.document) - 60.0).abs() < 1e-5,
            "{bulged}: {}",
            displayed(&app.tabs[i].scene.document)
        );
    }
}

#[test]
fn viewport_dimension_quadrant_keeps_its_feature_after_translation() {
    use crate::scene::ChangeKind;
    let (mut app, line, frame) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.set_current_layout("Model".into());
    let mut circle = Circle::new();
    circle.radius = 100.0;
    let source = app.tabs[i].scene.add_entity(EntityType::Circle(circle));
    app.tabs[i].scene.set_current_layout("Dimensions".into());
    let _ = app.dispatch_command("DIMALIGNED");
    point(&mut app, frame, line, DVec3::ZERO);
    let mut quadrant = hit(frame, source, DVec3::X * 100.0);
    quadrant.snap_type = SnapType::Quadrant;
    assert!(app.record_accepted_snap(i, Some(quadrant), Some(frame), quadrant.world));
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_point(quadrant.world);
    app.sync_dimension_snaps(i);
    let _ = app.apply_cmd_result(result);
    let _ = app.feed_command(StepInput::Point(DVec3::new(60.0, 80.0, 0.0)));
    let EntityType::Circle(circle) = app.tabs[i].scene.document.get_entity_mut(source).unwrap()
    else {
        panic!()
    };
    circle.center.x = 300.0;
    app.tabs[i]
        .scene
        .bump_entities(&[(source, ChangeKind::Modified)]);
    assert!((displayed(&app.tabs[i].scene.document) - 400.0).abs() < 1e-5);
}

fn start_centerline_dimension(
    command: &str,
    paper_first: bool,
    paper_kind: SnapType,
    factor: f64,
) -> (OpenCADStudio, Handle, Handle, ViewportFrame) {
    let (mut app, model, frame) = fixture();
    let i = app.active_tab;
    let paper = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(55.0, 40.0, 0.0),
            Vector3::new(55.0, 60.0, 0.0),
        )));
    let mut style = acadrust::tables::DimStyle::new("CenterlineUnits");
    style.dimlfac = factor;
    app.tabs[i].scene.document.dim_styles.add(style).unwrap();
    app.tabs[i].scene.document.header.current_dimstyle_name = "CenterlineUnits".into();
    let _ = app.dispatch_command(command);
    let mut paper_hit = hit(frame, paper, DVec3::new(55.0, 50.0, 0.0));
    paper_hit.world = DVec3::new(55.0, 50.0, 0.0);
    paper_hit.model_point = None;
    paper_hit.viewport = None;
    paper_hit.snap_type = paper_kind;
    let mut picks = [
        (paper_hit, None),
        (hit(frame, model, DVec3::X * 100.0), Some(frame)),
    ];
    if !paper_first {
        picks.reverse();
    }
    for (snap, context) in picks {
        assert!(app.record_accepted_snap(i, Some(snap), context, snap.world));
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(snap.world);
        app.sync_dimension_snaps(i);
        let _ = app.apply_cmd_result(result);
    }
    (app, model, paper, frame)
}

#[test]
fn viewport_dimension_paper_centerline_uses_viewport_units_in_either_order() {
    for command in ["DIMLINEAR", "DIMALIGNED"] {
        for paper_first in [true, false] {
            let (mut app, _, paper, _) =
                start_centerline_dimension(command, paper_first, SnapType::Midpoint, 25.4);
            let paper_snap = app
                .accepted_snaps()
                .iter()
                .find(|s| s.viewport.is_none())
                .unwrap();
            assert_eq!(paper_snap.source.as_ref().unwrap().source.handle, paper);
            assert_eq!(paper_snap.paper_point, paper_snap.model_point);
            let _ = app.feed_command(StepInput::Point(DVec3::new(60.0, 80.0, 0.0)));
            let doc = &app.tabs[app.active_tab].scene.document;
            assert!(
                (displayed(doc) - 1270.0).abs() < 1e-3,
                "{command}: {}",
                displayed(doc)
            );
            app.undo_steps(1);
            app.redo_steps(1);
            assert!((displayed(&app.tabs[app.active_tab].scene.document) - 1270.0).abs() < 1e-3);
        }
    }
}

#[test]
fn viewport_dimension_paper_centerline_tracks_each_owning_space() {
    use crate::scene::{ChangeKind, ReferenceStatus};
    for (paper_first, kind) in [(true, SnapType::Midpoint), (false, SnapType::Perpendicular)] {
        let (mut app, model, paper, frame) =
            start_centerline_dimension("DIMALIGNED", paper_first, kind, 1.0);
        let i = app.active_tab;
        let _ = app.feed_command(StepInput::Point(DVec3::new(60.0, 80.0, 0.0)));
        let dim = dimension(&app.tabs[i].scene.document).base().common.handle;
        assert!((displayed(&app.tabs[i].scene.document) - 50.0).abs() < 1e-4);
        let assoc = app.tabs[i].scene.dimension_association(dim).unwrap();
        assert!(assoc.trans_space);
        assert!(assoc
            .references
            .iter()
            .flatten()
            .any(|r| r.xrefs == vec![paper]));
        assert!(assoc
            .references
            .iter()
            .flatten()
            .any(|r| r.xrefs == vec![frame.viewport, model]));
        assert!(app.tabs[i]
            .scene
            .dimension_association_status(dim)
            .iter()
            .all(|(_, s)| *s == ReferenceStatus::Resolved));
        let pending = app.begin_undo(i, "Move paper centerline", 1, true).unwrap();
        let before = app.tabs[i].scene.document.get_entity_arc(paper);
        app.tabs[i].scene.record_undo_before(paper, before);
        let EntityType::Line(line) = app.tabs[i].scene.document.get_entity_mut(paper).unwrap()
        else {
            panic!()
        };
        line.start.x = 56.0;
        line.end.x = 56.0;
        app.tabs[i]
            .scene
            .bump_entities(&[(paper, ChangeKind::Modified)]);
        app.commit_undo_delta(i, pending);
        assert!((displayed(&app.tabs[i].scene.document) - 40.0).abs() < 1e-4);
        app.undo_steps(1);
        assert!((displayed(&app.tabs[i].scene.document) - 50.0).abs() < 1e-4);
        app.redo_steps(1);
        let EntityType::Viewport(vp) = app.tabs[i]
            .scene
            .document
            .get_entity_mut(frame.viewport)
            .unwrap()
        else {
            panic!()
        };
        vp.view_height = 500.0;
        vp.custom_scale = 0.2;
        app.tabs[i].scene.notify_viewport_changed(frame.viewport);
        // Model endpoint now appears at paper x=70; paper centerline stays x=56.
        assert!((displayed(&app.tabs[i].scene.document) - 70.0).abs() < 1e-4);
        let EntityType::Viewport(vp) = app.tabs[i]
            .scene
            .document
            .get_entity_mut(frame.viewport)
            .unwrap()
        else {
            panic!()
        };
        vp.view_target.x = 20.0;
        app.tabs[i].scene.notify_viewport_changed(frame.viewport);
        assert!((displayed(&app.tabs[i].scene.document) - 50.0).abs() < 1e-4);
        let EntityType::Line(line) = app.tabs[i].scene.document.get_entity_mut(model).unwrap()
        else {
            panic!()
        };
        line.end.x = 160.0;
        app.tabs[i]
            .scene
            .bump_entities(&[(model, ChangeKind::Modified)]);
        assert!((displayed(&app.tabs[i].scene.document) - 110.0).abs() < 1e-4);
        for ext in ["dxf", "dwg"] {
            let doc = &app.tabs[i].scene.document;
            let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
            let mut scene = Scene::new();
            scene.document = crate::io::load_bytes(&format!("centerline.{ext}"), bytes).unwrap();
            scene.notify_viewport_changed(frame.viewport);
            assert!((displayed(&scene.document) - 110.0).abs() < 1e-4, "{ext}");
            assert!(scene
                .dimension_association_status(dim)
                .iter()
                .all(|(_, s)| *s == ReferenceStatus::Resolved));
        }
        let before = app.tabs[i].scene.document.get_entity(dim).unwrap().clone();
        let model_owner = app.tabs[i]
            .scene
            .document
            .get_entity(model)
            .unwrap()
            .common()
            .owner_handle;
        app.tabs[i]
            .scene
            .document
            .get_entity_mut(paper)
            .unwrap()
            .common_mut()
            .owner_handle = model_owner;
        app.tabs[i].scene.notify_viewport_changed(frame.viewport);
        assert_eq!(app.tabs[i].scene.document.get_entity(dim).unwrap(), &before);
        assert!(app.tabs[i]
            .scene
            .dimension_association_status(dim)
            .iter()
            .any(|(_, s)| *s == ReferenceStatus::Unresolved));
    }
}

#[test]
fn viewport_dimension_invalid_paths_leave_no_partial_association() {
    use crate::scene::viewport_ref::SnapSourceRef;
    use acadrust::entities::{DimensionAligned, Insert};
    use acadrust::tables::BlockRecord;

    let (mut app, model, frame) = fixture();
    let scene = &mut app.tabs[app.active_tab].scene;
    let mut block = BlockRecord::new("Collapsed");
    block.handle = scene.document.allocate_handle();
    let owner = block.handle;
    scene.document.block_records.add(block).unwrap();
    let mut leaf = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    leaf.common.owner_handle = owner;
    let leaf = scene.document.add_entity(EntityType::Line(leaf)).unwrap();
    scene.set_current_layout("Model".into());
    let mut insert = Insert::new("Collapsed", Vector3::ZERO);
    insert.set_x_scale(0.0);
    insert.set_y_scale(0.0);
    let root = scene.add_entity(EntityType::Insert(insert));
    scene.set_current_layout("Dimensions".into());
    let paper = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(55.0, 50.0, 0.0),
        Vector3::new(55.0, 60.0, 0.0),
    )));
    for (source, path) in [(model, vec![Handle::new(u64::MAX)]), (leaf, vec![root])] {
        let dim = scene.add_entity(EntityType::Dimension(Dimension::Aligned(
            DimensionAligned::new(Vector3::new(50.0, 50.0, 0.0), Vector3::new(55.0, 50.0, 0.0)),
        )));
        let invalid = AcceptedSnap {
            paper_point: DVec3::new(50.0, 50.0, 0.0),
            model_point: DVec3::ZERO,
            viewport: Some(frame.viewport),
            frame: Some(frame),
            source: Some(SnapSourceRef {
                source: DimensionAssociationSource::inferred(source),
                block_path: path,
                snap_type: SnapType::Endpoint,
                intersection: None,
            }),
        };
        let mut valid = AcceptedSnap::free(DVec3::new(55.0, 50.0, 0.0));
        valid.source = Some(SnapSourceRef {
            source: DimensionAssociationSource::inferred(paper),
            block_path: Vec::new(),
            snap_type: SnapType::Endpoint,
            intersection: None,
        });
        scene.attach_viewport_dimension_association(dim, &[Some(invalid), Some(valid)]);
        let association = scene.dimension_association(dim).unwrap();
        assert!(!association.trans_space);
        assert!(association.references[0].is_empty());
        assert_eq!(association.references[1][0].xrefs, vec![paper]);
        assert_eq!(scene.dimension_association_sources(dim), vec![paper]);
        assert_eq!(
            scene.dimension_association_status(dim),
            vec![(1, crate::scene::ReferenceStatus::Resolved)]
        );
    }
}
