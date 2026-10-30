//! Exercise HATCH trimming through the document/command driver, not just geometry.

use super::super::OpenCADStudio;
use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::modify::trim::TrimCommand;
use codec::entities::hatch::{BoundaryEdge, BoundaryPath, PolylineEdge};
use codec::entities::{Hatch, Line, LwPolyline, LwVertex};
use codec::tables::layer::Layer;
use codec::types::{Color, Transparency, Vector2, Vector3};
use codec::{EntityType, Handle};
use serde_json::{json, Value};

const FILL_LAYER: &str = "TRIM_HATCH_FILL";
const BOUNDARY_LAYER: &str = "TRIM_HATCH_BOUNDARY";
const RECT: [[f64; 2]; 4] = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]];

struct Fixture {
    hatch: Handle,
    divider: Handle,
    original: Hatch,
    protected: Vec<EntityType>,
}

fn hatch(app: &OpenCADStudio, handle: Handle) -> &Hatch {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Hatch(hatch)) => hatch,
        other => panic!("expected HATCH at its original handle {handle:?}, got {other:?}"),
    }
}

fn add_protected(
    app: &mut OpenCADStudio,
    protected: &mut Vec<EntityType>,
    entity: EntityType,
) -> Handle {
    let scene = &mut app.tabs[app.active_tab].scene;
    let handle = scene.add_entity(entity);
    protected.push(scene.document.get_entity(handle).unwrap().clone());
    handle
}

fn add_divider(app: &mut OpenCADStudio, protected: &mut Vec<EntityType>, x: f64) -> Handle {
    let mut line = Line::from_points(Vector3::new(x, 0.0, 0.0), Vector3::new(x, 10.0, 0.0));
    line.common.layer = BOUNDARY_LAYER.into();
    add_protected(app, protected, EntityType::Line(line))
}

fn fixture() -> (OpenCADStudio, Fixture) {
    let mut app = OpenCADStudio::new_for_test();
    assert_eq!(app.automation_op(r#"{"op":"new"}"#)["ok"], true);
    let scene = &mut app.tabs[app.active_tab].scene;
    for layer in [FILL_LAYER, BOUNDARY_LAYER] {
        scene.document.layers.add_or_replace(Layer::new(layer));
    }

    let mut outline = LwPolyline::new();
    outline.is_closed = true;
    outline.common.layer = BOUNDARY_LAYER.into();
    outline.vertices = RECT
        .iter()
        .map(|p| LwVertex::from_coords(p[0], p[1]))
        .collect();
    let mut protected = Vec::new();
    add_protected(&mut app, &mut protected, EntityType::LwPolyline(outline));
    let divider = add_divider(&mut app, &mut protected, 10.0);

    // Use resolved PAT geometry, not a name-only/solid hatch. The stored origin
    // is ACAD XDATA, distinct from both the boundary and the line base points.
    let entry = crate::scene::model::hatch_patterns::find("ANSI31").unwrap();
    let pattern = crate::scene::model::hatch_patterns::build_dxf_pattern(entry);
    let mut fill = Hatch::with_pattern(pattern);
    fill.common.layer = FILL_LAYER.into();
    fill.common.color = Color::Index(3);
    fill.common.transparency = Transparency::T_20;
    fill.pattern_scale = 2.5;
    fill.scale_pattern_about_origin(fill.pattern_scale);
    fill.pattern_angle = 30.0_f64.to_radians();
    fill.rotate_pattern_about_origin(fill.pattern_angle);
    assert!(fill.set_pattern_origin(Vector2::new(2.25, -1.5)));
    let mut path = BoundaryPath::external();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        RECT.iter().map(|p| Vector2::new(p[0], p[1])).collect(),
        true,
    )));
    fill.paths.push(path);
    let handle = app.tabs[app.active_tab]
        .scene
        .add_entity(EntityType::Hatch(Box::new(fill)));
    let original = hatch(&app, handle).clone();
    assert!(!original.common.owner_handle.is_null());
    assert_eq!(
        original.stored_pattern_origin(),
        Some(Vector2::new(2.25, -1.5))
    );

    // (15,5) is between ANSI31 strokes, so a wire-only hit-test is insufficient.
    assert!(!original.pattern.lines.is_empty());
    for line in &original.pattern.lines {
        let (sin, cos) = line.angle.sin_cos();
        let spacing = (-sin * line.offset.x + cos * line.offset.y).abs();
        assert!(spacing > 0.0);
        let phase = (-sin * (15.0 - line.base_point.x) + cos * (5.0 - line.base_point.y))
            .rem_euclid(spacing);
        assert!(
            phase.min(spacing - phase) > 0.5,
            "pick must miss hatch strokes"
        );
    }

    let fixture = Fixture {
        hatch: handle,
        divider,
        original,
        protected,
    };
    assert_region(&app, &fixture, 0.0, 20.0);
    (app, fixture)
}

fn all_entities(app: &OpenCADStudio) -> Vec<EntityType> {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .cloned()
        .collect()
}

fn assert_metadata(actual: &Hatch, expected: &Hatch) {
    assert_eq!(
        actual.common, expected.common,
        "identity, owner, layer and XDATA"
    );
    assert_eq!(
        actual.pattern, expected.pattern,
        "pattern name and resolved lines"
    );
    assert_eq!(actual.pattern_type, expected.pattern_type);
    assert_eq!(actual.pattern_scale, expected.pattern_scale);
    assert_eq!(actual.pattern_angle, expected.pattern_angle);
    assert_eq!(
        actual.stored_pattern_origin(),
        expected.stored_pattern_origin()
    );
    assert_eq!(actual.pattern_origin(), expected.pattern_origin());
    assert_eq!(actual.is_solid, expected.is_solid);
    assert_eq!(actual.is_double, expected.is_double);
    assert_eq!(actual.style, expected.style);
    assert_eq!(actual.normal, expected.normal);
    assert_eq!(actual.elevation, expected.elevation);
}

fn assert_region(app: &OpenCADStudio, fixture: &Fixture, min_x: f64, max_x: f64) {
    let scene = &app.tabs[app.active_tab].scene;
    for entity in &fixture.protected {
        assert_eq!(
            scene.document.get_entity(entity.common().handle),
            Some(entity),
            "TRIM must not edit/replace the outline or cutting lines"
        );
    }
    assert_eq!(
        scene.document.entities().count(),
        fixture.protected.len() + 1
    );
    let fill = hatch(app, fixture.hatch);
    let members = &scene
        .document
        .block_records
        .get("*Model_Space")
        .unwrap()
        .entity_handles;
    assert_eq!(
        members.len(),
        fixture.protected.len() + 1,
        "flat storage can hide duplicate owning-block membership"
    );
    for entity in scene.document.entities() {
        assert_eq!(
            members
                .iter()
                .filter(|&&h| h == entity.common().handle)
                .count(),
            1
        );
    }
    assert_metadata(fill, &fixture.original);
    let area = crate::entities::hatch::boundary_area(fill);
    assert!(
        (area - (max_x - min_x) * 10.0).abs() < 1e-7,
        "unexpected fill area {area}"
    );

    // Assert the occupied region, without depending on loop winding, vertex
    // ordering, or whether the replacement uses line or polyline edges.
    let boundary: Vec<_> = fill
        .paths
        .iter()
        .flat_map(|path| &path.edges)
        .filter_map(crate::entities::hatch::edge_curve)
        .collect();
    for x in [-1.0, 1.0, 4.0, 6.0, 7.5, 9.0, 11.0, 15.0, 19.0, 21.0] {
        for y in [-1.0, 1.0, 5.0, 9.0, 11.0] {
            let inside =
                kernel::geom2d::contains(&boundary, [x, y], kernel::geom2d::Tolerance::new(1e-8));
            assert_eq!(
                inside,
                x > min_x && x < max_x && y > 0.0 && y < 10.0,
                "wrong fill occupancy at ({x},{y})"
            );
        }
    }
}

fn request(app: &mut OpenCADStudio, id: &str, mut req: Value) -> Value {
    let state = app.control_state();
    req["protocol"] = json!(1);
    req["request_id"] = json!(id);
    req["client_id"] = json!("trim-hatch-tests");
    req["document_id"] = state["document_id"].clone();
    req["revision"] = state["revision"].clone();
    let response = app.automation_op(&req.to_string());
    assert_eq!(response["ok"], true, "request {req}: {response}");
    response
}

fn install_trim(app: &mut OpenCADStudio, command: TrimCommand) {
    assert!(command.needs_entity_pick());
    assert!(
        command.entity_pick_includes_fills(),
        "HATCH interior picks must not require hitting a pattern stroke"
    );
    app.tabs[app.active_tab].active_cmd = Some(Box::new(command));
}

fn apply_trim_pick(app: &mut OpenCADStudio, fixture: &Fixture, x: f64) {
    let result = app.tabs[app.active_tab]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_entity_pick(fixture.hatch, glam::DVec3::new(x, 5.0, 0.0));
    let CmdResult::ReplaceEntity(handle, entities) = &result else {
        panic!("an interior HATCH trim must use the ReplaceEntity contract");
    };
    assert_eq!(*handle, fixture.hatch);
    assert_eq!(entities.len(), 1, "one surviving region stays one HATCH");
    let EntityType::Hatch(replacement) = &entities[0] else {
        panic!("TRIM must not explode a HATCH into strokes");
    };
    assert_metadata(replacement, &fixture.original);
    let _ = app.apply_cmd_result(result);
    assert_eq!(
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .map(|cmd| cmd.name()),
        Some("TRIM"),
        "TRIM must stay active for subsequent clicks"
    );
}

#[test]
fn trim_hatch_quick_interior_pick_preserves_metadata_and_undo_redo() {
    let (mut app, fixture) = fixture();
    // new_for_test supplies the default Quick setting. Do not set the global
    // quick_mode or send mOde/Quick: other tests construct commands in parallel.
    assert_eq!(
        request(&mut app, "start", json!({"op":"start","cmd":"TRIM"}))["status"],
        "waiting_input"
    );
    let command = app.tabs[app.active_tab].active_cmd.as_ref().unwrap();
    assert!(command.entity_pick_includes_fills());
    assert!(!command.options().iter().any(|option| option.keyword == "E"));
    assert_eq!(
        request(
            &mut app,
            "right",
            json!({
                "op":"input","kind":"entity",
                "handle":format!("{:X}", fixture.hatch.value()),"point":[15.0,5.0,0.0]
            })
        )["status"],
        "waiting_input"
    );
    assert_region(&app, &fixture, 0.0, 10.0);
    let trimmed = hatch(&app, fixture.hatch).clone();
    assert_eq!(
        request(&mut app, "done", json!({"op":"input","kind":"enter"}))["status"],
        "completed"
    );
    request(&mut app, "undo", json!({"op":"undo"}));
    assert_region(&app, &fixture, 0.0, 20.0);
    assert_eq!(hatch(&app, fixture.hatch), &fixture.original);
    request(&mut app, "redo", json!({"op":"redo"}));
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_eq!(hatch(&app, fixture.hatch), &trimmed);
}

fn assert_saved_hatch_once(app: &OpenCADStudio, fixture: &Fixture) {
    let doc = &app.tabs[app.active_tab].scene.document;
    let bytes = crate::io::save_to_bytes(doc, "dxf", doc.version).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let handle = format!("{:X}", fixture.hatch.value());
    assert_eq!(
        lines
            .chunks_exact(2)
            .filter(|pair| pair[0].trim() == "5" && pair[1].trim() == handle)
            .count(),
        1,
        "DXF must contain exactly one record for the preserved HATCH handle"
    );
    let loaded = crate::io::load_bytes("trim-hatch.dxf", bytes).unwrap();
    assert_eq!(loaded.entities().count(), doc.entities().count());
    let Some(EntityType::Hatch(restored)) = loaded.get_entity(fixture.hatch) else {
        panic!("saved HATCH identity lost");
    };
    assert!(
        (crate::entities::hatch::boundary_area(restored)
            - crate::entities::hatch::boundary_area(hatch(app, fixture.hatch)))
        .abs()
            < 1e-7
    );
}

#[test]
fn trim_hatch_save_has_no_duplicate_records_before_or_after_undo_redo() {
    let (mut app, fixture) = fixture();
    assert_saved_hatch_once(&app, &fixture);
    let command = TrimCommand::new(all_entities(&app));
    install_trim(&mut app, command);
    apply_trim_pick(&mut app, &fixture, 15.0);
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_saved_hatch_once(&app, &fixture);
    app.undo_active_tab();
    assert_region(&app, &fixture, 0.0, 20.0);
    assert_saved_hatch_once(&app, &fixture);
    app.redo_active_tab();
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_saved_hatch_once(&app, &fixture);
}

#[test]
fn trim_hatch_explicit_cutting_edges_ignore_unselected_dividers() {
    let (mut app, mut fixture) = fixture();
    // If all objects cut despite the explicit edge set, this line would leave
    // an unwanted filled strip at x=16..20 after clicking (15,5).
    add_divider(&mut app, &mut fixture.protected, 16.0);
    let command = TrimCommand::with_cutting_edges(all_entities(&app), vec![fixture.divider]);
    install_trim(&mut app, command);
    apply_trim_pick(&mut app, &fixture, 15.0);
    assert_region(&app, &fixture, 0.0, 10.0);
    let trimmed = hatch(&app, fixture.hatch).clone();

    app.undo_active_tab();
    assert_region(&app, &fixture, 0.0, 20.0);
    assert_eq!(hatch(&app, fixture.hatch), &fixture.original);
    app.redo_active_tab();
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_eq!(hatch(&app, fixture.hatch), &trimmed);
}

#[test]
fn trim_hatch_multiple_clicks_keep_identity_and_replay_each_edit() {
    let (mut app, mut fixture) = fixture();
    add_divider(&mut app, &mut fixture.protected, 5.0);
    let command = TrimCommand::new(all_entities(&app));
    install_trim(&mut app, command);
    apply_trim_pick(&mut app, &fixture, 15.0);
    assert_region(&app, &fixture, 0.0, 10.0);
    let half = hatch(&app, fixture.hatch).clone();

    // A second click in the already removed region must be a no-op, not trim
    // the nearest surviving cell or create an extra undo transaction.
    let result = app.tabs[app.active_tab]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_entity_pick(fixture.hatch, glam::DVec3::new(15.0, 5.0, 0.0));
    assert!(matches!(&result, CmdResult::NeedPoint));
    let _ = app.apply_cmd_result(result);
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_eq!(hatch(&app, fixture.hatch), &half);

    // The driver must have notified on_entity_replaced: using the original
    // 20x10 snapshot here would resurrect the previously removed right half.
    apply_trim_pick(&mut app, &fixture, 7.5);
    assert_region(&app, &fixture, 0.0, 5.0);
    let quarter = hatch(&app, fixture.hatch).clone();

    app.undo_active_tab();
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_eq!(hatch(&app, fixture.hatch), &half);
    app.undo_active_tab();
    assert_region(&app, &fixture, 0.0, 20.0);
    assert_eq!(hatch(&app, fixture.hatch), &fixture.original);
    app.redo_active_tab();
    assert_region(&app, &fixture, 0.0, 10.0);
    assert_eq!(hatch(&app, fixture.hatch), &half);
    app.redo_active_tab();
    assert_region(&app, &fixture, 0.0, 5.0);
    assert_eq!(hatch(&app, fixture.hatch), &quarter);
}
