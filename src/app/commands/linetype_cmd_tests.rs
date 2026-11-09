//! `-LINETYPE` Create, Rename and Delete, and the rules of the shared linetype
//! service behind them.
use super::purge::{purge_pass, KINDS, LINETYPES};
use super::*;
use crate::scene::linetype_service as service;
use document::entities::multileader::{LeaderLine, LeaderRoot};
use document::entities::{AttributeEntity, Insert, Line, LwPolyline, LwVertex, MultiLeader, Table};
use document::objects::{MLineStyle, MLineStyleElement, MultiLeaderStyle, ObjectType};
use document::tables::{BlockRecord, Layer, LineType};
use document::types::{Handle, Vector2, Vector3};
use document::{Document, EntityType};
use std::collections::HashSet;

/// Asymmetric on purpose: every element differs, and a dot sits mid-pattern.
const CREATE_FENCE: &str = r#"-LINETYPE CREATE FENCE "Fence line" 1.5,-0.25,0,-0.75"#;

fn fresh() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

fn with_fence() -> OpenCADStudio {
    let mut app = fresh();
    run(&mut app, CREATE_FENCE);
    assert!(doc(&app).line_types.contains("FENCE"), "{}", output(&app));
    app
}

fn run(app: &mut OpenCADStudio, line: &str) {
    let _ = app.run_command_line(line);
}

fn doc(app: &OpenCADStudio) -> &Document {
    &app.tabs[app.active_tab].scene.document
}

fn doc_mut(app: &mut OpenCADStudio) -> &mut Document {
    let i = app.active_tab;
    &mut app.tabs[i].scene.document
}

fn output(app: &OpenCADStudio) -> String {
    app.command_line
        .history
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn lengths(app: &OpenCADStudio, name: &str) -> Vec<f64> {
    doc(app)
        .line_types
        .get(name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .elements
        .iter()
        .map(|element| element.length)
        .collect()
}

fn undo_depth(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.undo_stack.len()
}

fn add_line(app: &mut OpenCADStudio, linetype: &str) -> Handle {
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 4.0, 0.0));
    line.common.linetype = linetype.to_string();
    doc_mut(app).add_entity(EntityType::Line(line)).unwrap()
}

fn add_layer(app: &mut OpenCADStudio, name: &str, linetype: &str) {
    let mut layer = Layer::new(name);
    layer.handle = doc_mut(app).allocate_handle();
    layer.line_type = linetype.to_string();
    doc_mut(app).layers.add(layer).unwrap();
}

/// A block definition whose only content is a line in `linetype`.
fn add_block_content(app: &mut OpenCADStudio, linetype: &str) -> Handle {
    let owner = doc_mut(app).allocate_handle();
    let mut block = BlockRecord::new("HOLDER");
    block.handle = owner;
    doc_mut(app).block_records.add(block).unwrap();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(3.0, 7.0, 0.0));
    line.common.owner_handle = owner;
    line.common.linetype = linetype.to_string();
    doc_mut(app).add_entity(EntityType::Line(line)).unwrap()
}

/// An insert whose attribute is the only thing in `linetype`.
fn add_attribute(app: &mut OpenCADStudio, linetype: &str) -> Handle {
    let mut attribute = AttributeEntity::new("TAG".into(), "VALUE".into());
    attribute.common.linetype = linetype.to_string();
    let mut insert = Insert::new("HOLDER", Vector3::ZERO);
    insert.attributes.push(attribute);
    doc_mut(app).add_entity(EntityType::Insert(insert)).unwrap()
}

fn attribute_linetype(app: &OpenCADStudio, handle: Handle) -> String {
    match doc(app).get_entity(handle) {
        Some(EntityType::Insert(insert)) => insert.attributes[0].common.linetype.clone(),
        other => panic!("not an insert: {other:?}"),
    }
}

fn add_mline_style(app: &mut OpenCADStudio, linetype: &str) -> Handle {
    let handle = doc_mut(app).allocate_handle();
    let mut style = MLineStyle::new("TWIN");
    style.handle = handle;
    let mut element = MLineStyleElement::new(0.5);
    element.linetype = linetype.to_string();
    style.elements.push(element);
    doc_mut(app)
        .objects
        .insert(handle, ObjectType::MLineStyle(style));
    handle
}

fn mline_style_linetype(app: &OpenCADStudio, handle: Handle) -> String {
    match doc(app).objects.get(&handle) {
        Some(ObjectType::MLineStyle(style)) => style.elements[0].linetype.clone(),
        other => panic!("not a multiline style: {other:?}"),
    }
}

fn entity_linetype(app: &OpenCADStudio, handle: Handle) -> String {
    doc(app)
        .get_entity(handle)
        .expect("entity")
        .common()
        .linetype
        .clone()
}

fn fence_handle(app: &OpenCADStudio) -> Handle {
    doc(app).line_types.get("FENCE").unwrap().handle
}

fn purgeable(app: &OpenCADStudio) -> Vec<String> {
    let mut wanted = [false; KINDS];
    wanted[LINETYPES] = true;
    purge_pass(doc(app), &[], wanted, "")[LINETYPES].clone()
}

// ── Create ───────────────────────────────────────────────────────────────

#[test]
fn create_keeps_an_asymmetric_pattern_exactly() {
    let mut app = fresh();
    run(
        &mut app,
        r#"-LINETYPE CREATE "My Fence" "Fence line" 1.5,-0.25,0,-0.75"#,
    );
    let linetype = doc(&app).line_types.get("My Fence").expect(&output(&app));
    assert_eq!(linetype.name, "My Fence", "the typed spelling is kept");
    assert_eq!(linetype.description, "Fence line");
    assert_eq!(linetype.pattern_length, 2.5);
    assert!(!linetype.handle.is_null());
    assert_eq!(lengths(&app, "My Fence"), vec![1.5, -0.25, 0.0, -0.75]);
    assert!(app.tabs[app.active_tab].dirty);
}

#[test]
fn create_without_a_description_takes_the_pattern_as_the_last_word() {
    let mut app = fresh();
    run(&mut app, "-LINETYPE CREATE PLAIN 2,-1");
    assert_eq!(lengths(&app, "PLAIN"), vec![2.0, -1.0]);
    assert_eq!(doc(&app).line_types.get("PLAIN").unwrap().description, "");
}

#[test]
fn create_refuses_bad_input_and_takes_no_undo_step() {
    let thirteen = vec!["1,-1"; 7].join(",");
    let twelve_ok = vec!["1,-1"; 6].join(",");
    let refused: Vec<(String, &str)> = vec![
        (r#"-LINETYPE CREATE "" 1,-1"#.into(), "empty name"),
        (r#"-LINETYPE CREATE "A,B" 1,-1"#.into(), "comma"),
        (r#"-LINETYPE CREATE "A|B" 1,-1"#.into(), "bar"),
        ("-LINETYPE CREATE fence 1,-1".into(), "other case of FENCE"),
        ("-LINETYPE CREATE DASHED 1,-1".into(), "catalog name"),
        ("-LINETYPE CREATE ByLayer 1,-1".into(), "built-in"),
        ("-LINETYPE CREATE X1 1".into(), "one element"),
        (format!("-LINETYPE CREATE X2 {thirteen}"), "thirteen"),
        ("-LINETYPE CREATE X3 1,2".into(), "no gap"),
        ("-LINETYPE CREATE X4 -1,1".into(), "leading gap"),
        ("-LINETYPE CREATE X5 1,NaN".into(), "NaN"),
        ("-LINETYPE CREATE X6 1,-inf".into(), "infinite"),
        ("-LINETYPE CREATE X7 1,-10000000".into(), "too long"),
        ("-LINETYPE CREATE X8 1,abc".into(), "not a number"),
    ];
    let mut app = with_fence();
    let dashed_before = doc(&app).line_types.contains("DASHED");
    for (line, label) in refused {
        let count = doc(&app).line_types.len();
        let depth = undo_depth(&app);
        run(&mut app, &line);
        assert_eq!(doc(&app).line_types.len(), count, "{label}: {line}");
        assert_eq!(undo_depth(&app), depth, "{label}: undo step");
    }
    assert_eq!(
        doc(&app).line_types.contains("DASHED"),
        dashed_before,
        "a refused catalog name does not load the catalog"
    );
    assert_eq!(lengths(&app, "FENCE"), vec![1.5, -0.25, 0.0, -0.75]);
    run(&mut app, &format!("-LINETYPE CREATE TWELVE {twelve_ok}"));
    assert_eq!(lengths(&app, "TWELVE").len(), 12, "twelve is the limit");
}

// ── Rendering ────────────────────────────────────────────────────────────

fn tessellate(
    document: &Document,
    entity: EntityType,
) -> crate::scene::model::wire_model::WireModel {
    let mut wires: Vec<_> = crate::scene::tessellate_entity(
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
    .into_iter()
    .filter(|wire| !wire.snap_only && wire.pattern_length > 0.0)
    .collect();
    assert_eq!(wires.len(), 1, "one dashed wire expected");
    wires.remove(0)
}

fn document_with(pattern: &[f64]) -> Document {
    let mut document = Document::new();
    let linetype = service::plan_create(&document, "MY-FENCE", "", pattern).unwrap();
    service::apply_create(&mut document, linetype).unwrap();
    document
}

/// Every length differs, so a reversed or shifted pattern cannot pass.
const LAYOUT_PATTERN: [f64; 4] = [1.5, -0.25, 0.5, -0.75];

/// The drawn stretches of a dashed `wire`, as distances along it, per segment
/// of the plot's run layout: what the screen and the plot both cut.
fn drawn_spans(wire: &crate::scene::model::wire_model::WireModel) -> Vec<[f32; 2]> {
    let step = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let total: f32 = wire.points.windows(2).map(|p| step(p[0], p[1])).sum();
    let mut spans: Vec<[f32; 2]> = Vec::new();
    let mut at = 0.0_f32;
    for segment in wire.points.windows(2) {
        let length = step(segment[0], segment[1]);
        for range in crate::io::pdf_export::dash_ranges(
            at,
            at + length,
            total,
            wire.pattern_length,
            &wire.pattern,
        ) {
            let span = [at + range[0] * length, at + range[1] * length];
            match spans.last_mut() {
                Some(last) if (last[1] - span[0]).abs() < 1e-3 => last[1] = span[1],
                _ => spans.push(span),
            }
        }
        at += length;
    }
    spans
}

/// A 7.5 run of the 3.0 period: A alignment puts a full dash at each end.
fn assert_layout(spans: &[[f32; 2]]) {
    let want = [
        [0.0, 1.5],
        [1.75, 2.25],
        [3.0, 4.5],
        [4.75, 5.25],
        [6.0, 7.5],
    ];
    assert_eq!(spans.len(), want.len(), "{spans:?}");
    for (span, want) in spans.iter().zip(want) {
        assert!((span[0] - want[0]).abs() < 1e-3, "{spans:?}");
        assert!((span[1] - want[1]).abs() < 1e-3, "{spans:?}");
    }
}

#[test]
fn a_created_linetype_lays_out_along_an_angled_line() {
    let document = document_with(&LAYOUT_PATTERN);
    let (sin, cos) = 30.0_f64.to_radians().sin_cos();
    let mut line = Line::from_points(
        Vector3::new(4.0, -3.0, 0.0),
        Vector3::new(4.0 + 7.5 * cos, -3.0 + 7.5 * sin, 0.0),
    );
    line.common.linetype = "MY-FENCE".into();
    let wire = tessellate(&document, EntityType::Line(line));
    assert_eq!(wire.pattern_length, 3.0);
    assert_eq!(&wire.pattern[..4], &[1.5, -0.25, 0.5, -0.75]);
    let (first, last) = (wire.points[0], *wire.points.last().unwrap());
    assert!(((last[1] - first[1]) / (last[0] - first[0]) - (sin / cos) as f32).abs() < 1e-4);
    assert_layout(&drawn_spans(&wire));
}

#[test]
fn a_created_linetype_walks_around_a_polyline_corner() {
    let document = document_with(&LAYOUT_PATTERN);
    // 3.7 up at 30 degrees, then 3.8 down at 45: the corner falls inside the
    // second long dash (3.0 to 4.5), which must carry across it unbroken.
    let (sin, cos) = 30.0_f64.to_radians().sin_cos();
    let corner = (3.7 * cos, 3.7 * sin);
    let run = 3.8 * std::f64::consts::FRAC_1_SQRT_2;
    let mut polyline = LwPolyline::default();
    polyline.vertices = [(0.0, 0.0), corner, (corner.0 + run, corner.1 - run)]
        .iter()
        .map(|&(x, y)| LwVertex::new(Vector2::new(x, y)))
        .collect();
    polyline.plinegen = true;
    polyline.common.linetype = "MY-FENCE".into();
    let wire = tessellate(&document, EntityType::LwPolyline(polyline));
    assert_eq!(wire.points.len(), 3, "the corner stays a vertex");
    assert_layout(&drawn_spans(&wire));
}

#[test]
fn a_dot_reaches_the_renderer_in_its_slot() {
    let document = document_with(&[1.5, -0.25, 0.0, -0.75]);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(5.0, 3.0, 0.0));
    line.common.linetype = "MY-FENCE".into();
    let wire = tessellate(&document, EntityType::Line(line));
    assert_eq!(wire.pattern_length, 2.5);
    assert_eq!(&wire.pattern[..4], &[1.5, -0.25, 0.0, -0.75]);
}

// ── Rename ───────────────────────────────────────────────────────────────

#[test]
fn rename_follows_every_named_reference_and_keeps_the_handle() {
    let mut app = with_fence();
    let entity = add_line(&mut app, "FENCE");
    let nested = add_block_content(&mut app, "fence");
    let insert = add_attribute(&mut app, "FENCE");
    let mline = add_mline_style(&mut app, "FENCE");
    add_layer(&mut app, "WALLS", "FENCE");
    doc_mut(&mut app).capture_layer_state("SAVED", "");
    doc_mut(&mut app).header.current_linetype_name = "FENCE".into();
    let handle = doc(&app).line_types.get("FENCE").unwrap().handle;
    doc_mut(&mut app)
        .dim_styles
        .iter_mut()
        .next()
        .expect("a dimension style")
        .dimltex1_handle = handle;

    run(&mut app, r#"-LINETYPE RENAME FENCE "Site Fence""#);

    assert!(!doc(&app).line_types.contains("FENCE"), "{}", output(&app));
    let renamed = doc(&app).line_types.get("Site Fence").expect("renamed");
    assert_eq!(renamed.handle, handle);
    assert_eq!(entity_linetype(&app, entity), "Site Fence");
    assert_eq!(entity_linetype(&app, nested), "Site Fence", "block content");
    assert_eq!(attribute_linetype(&app, insert), "Site Fence");
    assert_eq!(mline_style_linetype(&app, mline), "Site Fence");
    assert_eq!(
        doc(&app).layers.get("WALLS").unwrap().line_type,
        "Site Fence"
    );
    assert_eq!(doc(&app).header.current_linetype_name, "Site Fence");
    let state = doc(&app).layer_state("SAVED").expect("state");
    let row = state
        .layers
        .iter()
        .find(|l| l.layer_name == "WALLS")
        .unwrap();
    assert_eq!(row.line_type, "Site Fence", "saved layer state");
    assert_eq!(
        doc(&app).dim_styles.iter().next().unwrap().dimltex1_handle,
        handle,
        "a reference by handle needs no update"
    );
}

#[test]
fn rename_refuses_protected_and_taken_names() {
    let mut app = with_fence();
    run(&mut app, "-LINETYPE CREATE OTHER 1,-1");
    let mut bound = LineType::new("XR|DASH");
    bound.handle = doc_mut(&mut app).allocate_handle();
    bound.xref_dependent = true;
    doc_mut(&mut app).line_types.add(bound).unwrap();
    for line in [
        "-LINETYPE RENAME ByLayer NEW1",
        "-LINETYPE RENAME ByBlock NEW2",
        "-LINETYPE RENAME Continuous NEW3",
        r#"-LINETYPE RENAME "XR|DASH" NEW4"#,
        "-LINETYPE RENAME FENCE other",
        "-LINETYPE RENAME FENCE DASHED",
        "-LINETYPE RENAME FENCE ByBlock",
        r#"-LINETYPE RENAME FENCE "A,B""#,
        "-LINETYPE RENAME MISSING NEW5",
    ] {
        let depth = undo_depth(&app);
        run(&mut app, line);
        assert_eq!(undo_depth(&app), depth, "{line}");
        assert!(doc(&app).line_types.contains("FENCE"), "{line}");
        assert!(doc(&app).line_types.contains("OTHER"), "{line}");
        assert!(doc(&app).line_types.contains("XR|DASH"), "{line}");
    }
    assert!(!doc(&app).line_types.contains("NEW1"));
}

#[test]
fn a_change_of_case_is_a_rename() {
    let mut app = with_fence();
    run(&mut app, "-LINETYPE RENAME FENCE Fence");
    assert_eq!(doc(&app).line_types.get("FENCE").unwrap().name, "Fence");
}

#[test]
fn the_rename_command_also_reaches_attributes() {
    let mut app = with_fence();
    let insert = add_attribute(&mut app, "FENCE");
    run(&mut app, "RENAME LINETYPE FENCE POSTS");
    assert_eq!(
        attribute_linetype(&app, insert),
        "POSTS",
        "{}",
        output(&app)
    );
}

// ── Delete ───────────────────────────────────────────────────────────────

fn assert_delete_refused(app: &mut OpenCADStudio, label: &str) {
    let depth = undo_depth(app);
    run(app, "-LINETYPE DELETE FENCE");
    assert!(doc(app).line_types.contains("FENCE"), "{label}");
    assert_eq!(undo_depth(app), depth, "{label}: undo step");
    assert!(output(app).contains("PURGE"), "{label}: {}", output(app));
    assert!(
        !purgeable(app).iter().any(|name| name == "FENCE"),
        "{label}: PURGE agrees"
    );
}

#[test]
fn delete_refuses_each_kind_of_use() {
    type Setup = fn(&mut OpenCADStudio);
    let uses: [(&str, Setup); 12] = [
        ("entity", |app| {
            add_line(app, "FENCE");
        }),
        ("entity in another case", |app| {
            add_line(app, "fence");
        }),
        ("layer", |app| add_layer(app, "WALLS", "FENCE")),
        ("block content", |app| {
            add_block_content(app, "FENCE");
        }),
        ("attribute only", |app| {
            add_attribute(app, "FENCE");
        }),
        ("current", |app| {
            doc_mut(app).header.current_linetype_name = "FENCE".into();
        }),
        ("dimension style", |app| {
            let handle = doc(app).line_types.get("FENCE").unwrap().handle;
            doc_mut(app)
                .dim_styles
                .iter_mut()
                .next()
                .expect("a dimension style")
                .dimltex2_handle = handle;
        }),
        ("multiline style", |app| {
            add_mline_style(app, "FENCE");
        }),
        ("multileader", |app| {
            let mut leader = MultiLeader::new();
            leader.line_type_handle = Some(fence_handle(app));
            let entity = EntityType::MultiLeader(Box::new(leader));
            doc_mut(app).add_entity(entity).unwrap();
        }),
        ("multileader leader line", |app| {
            let mut root = LeaderRoot::default();
            let mut line = LeaderLine::default();
            line.line_type_handle = Some(fence_handle(app));
            root.lines.push(line);
            let mut leader = MultiLeader::new();
            leader.context.leader_roots.push(root);
            let entity = EntityType::MultiLeader(Box::new(leader));
            doc_mut(app).add_entity(entity).unwrap();
        }),
        ("multileader style", |app| {
            let handle = doc_mut(app).allocate_handle();
            let mut style = MultiLeaderStyle::new("CALLOUT");
            style.handle = handle;
            style.line_type_handle = Some(fence_handle(app));
            doc_mut(app)
                .objects
                .insert(handle, ObjectType::MultiLeaderStyle(style));
        }),
        ("table cell border", |app| {
            let mut table = Table::new(Vector3::ZERO, 2, 2);
            table.rows[1].cells[1]
                .style
                .get_or_insert_with(Default::default)
                .left_border
                .line_type_handle = Some(fence_handle(app));
            let entity = EntityType::Table(Box::new(table));
            doc_mut(app).add_entity(entity).unwrap();
        }),
    ];
    for (label, setup) in uses {
        let mut app = with_fence();
        setup(&mut app);
        assert_delete_refused(&mut app, label);
    }
}

#[test]
fn delete_refuses_a_linetype_a_saved_layer_state_restores() {
    let mut app = with_fence();
    add_layer(&mut app, "WALLS", "FENCE");
    doc_mut(&mut app).capture_layer_state("SAVED", "");
    doc_mut(&mut app).layers.get_mut("WALLS").unwrap().line_type = "Continuous".into();
    assert_delete_refused(&mut app, "layer state");
}

#[test]
fn delete_refuses_builtins_and_xref_linetypes() {
    let mut app = fresh();
    let mut bound = LineType::new("XR|DASH");
    bound.handle = doc_mut(&mut app).allocate_handle();
    bound.xref_dependent = true;
    doc_mut(&mut app).line_types.add(bound).unwrap();
    run(&mut app, r#"-LINETYPE DELETE "XR|DASH""#);
    assert!(doc(&app).line_types.contains("XR|DASH"));
    for builtin in ["ByLayer", "ByBlock", "Continuous"] {
        assert!(
            matches!(
                service::plan_delete(doc(&app), builtin),
                Err(service::LinetypeError::Protected(_) | service::LinetypeError::NotFound(_))
            ),
            "{builtin}"
        );
    }
}

#[test]
fn an_unused_linetype_can_be_deleted_and_purge_agrees() {
    let mut app = with_fence();
    assert!(purgeable(&app).iter().any(|name| name == "FENCE"));
    run(&mut app, "-LINETYPE DELETE fence");
    assert!(!doc(&app).line_types.contains("FENCE"), "{}", output(&app));
}

// ── Undo ─────────────────────────────────────────────────────────────────

#[test]
fn create_undoes_in_one_step() {
    let mut app = fresh();
    let depth = undo_depth(&app);
    run(&mut app, CREATE_FENCE);
    app.finish_pending_history(app.active_tab);
    assert_eq!(undo_depth(&app), depth + 1);
    app.undo_steps(1);
    assert!(!doc(&app).line_types.contains("FENCE"));
}

#[test]
fn rename_undoes_in_one_step_and_restores_references() {
    let mut app = with_fence();
    let entity = add_line(&mut app, "FENCE");
    let insert = add_attribute(&mut app, "FENCE");
    let mline = add_mline_style(&mut app, "FENCE");
    add_layer(&mut app, "WALLS", "FENCE");
    doc_mut(&mut app).capture_layer_state("SAVED", "");
    app.finish_pending_history(app.active_tab);
    let depth = undo_depth(&app);
    run(&mut app, "-LINETYPE RENAME FENCE POSTS");
    app.finish_pending_history(app.active_tab);
    assert_eq!(undo_depth(&app), depth + 1);
    app.undo_steps(1);
    assert!(doc(&app).line_types.contains("FENCE"));
    assert!(!doc(&app).line_types.contains("POSTS"));
    assert_eq!(entity_linetype(&app, entity), "FENCE");
    assert_eq!(attribute_linetype(&app, insert), "FENCE");
    assert_eq!(mline_style_linetype(&app, mline), "FENCE");
    assert_eq!(doc(&app).layers.get("WALLS").unwrap().line_type, "FENCE");
    let state = doc(&app).layer_state("SAVED").expect("state");
    let row = state
        .layers
        .iter()
        .find(|l| l.layer_name == "WALLS")
        .unwrap();
    assert_eq!(row.line_type, "FENCE", "saved layer state");
}

#[test]
fn delete_undoes_in_one_step() {
    let mut app = with_fence();
    app.finish_pending_history(app.active_tab);
    let depth = undo_depth(&app);
    run(&mut app, "-LINETYPE DELETE FENCE");
    app.finish_pending_history(app.active_tab);
    assert_eq!(undo_depth(&app), depth + 1);
    app.undo_steps(1);
    assert_eq!(lengths(&app, "FENCE"), vec![1.5, -0.25, 0.0, -0.75]);
}

// ── New objects ──────────────────────────────────────────────────────────

/// Through the messages the command line sends when the user presses Enter.
fn typed(app: &mut OpenCADStudio, line: &str) {
    let _ = app.update(Message::CommandInput(line.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn drawn_after(app: &mut OpenCADStudio, known: usize) -> String {
    typed(app, "LINE");
    typed(app, "0,0");
    typed(app, "@20<30");
    let _ = app.update(Message::CommandEscape);
    let _ = app.update(Message::CommandEscape);
    let entity = doc(app).entities().nth(known).expect("the line was drawn");
    entity.common().linetype.clone()
}

#[test]
fn the_first_line_after_set_takes_the_new_linetype() {
    let mut app = with_fence();
    app.tabs[app.active_tab].is_start = false;
    assert!(
        app.ribbon.available_linetypes.iter().any(|item| item.name == "FENCE"),
        "the ribbon lists a new linetype"
    );
    typed(&mut app, "-LINETYPE SET FENCE");
    assert_eq!(app.ribbon.active_linetype, "FENCE");
    assert_eq!(drawn_after(&mut app, 0), "FENCE");
    typed(&mut app, "LINETYPE SET ByLayer");
    assert_eq!(app.ribbon.active_linetype, "ByLayer");
}

#[test]
fn renaming_the_current_linetype_keeps_new_lines_on_it() {
    let mut app = with_fence();
    app.tabs[app.active_tab].is_start = false;
    typed(&mut app, "-LINETYPE SET FENCE");
    typed(&mut app, "-LINETYPE RENAME FENCE POSTS");
    assert_eq!(app.ribbon.active_linetype, "POSTS");
    assert_eq!(drawn_after(&mut app, 0), "POSTS");
}

// ── Scripts and the existing options ─────────────────────────────────────

fn script(app: &mut OpenCADStudio, line: &str) {
    let _ = app.update(Message::ScriptLine(line.into()));
}

#[test]
fn quoted_names_work_from_a_script() {
    let mut app = fresh();
    script(
        &mut app,
        r#"-LINETYPE CREATE "My Fence" "A fence, two words" 1.5,-0.25"#,
    );
    assert_eq!(
        doc(&app)
            .line_types
            .get("My Fence")
            .expect(&output(&app))
            .description,
        "A fence, two words"
    );
    script(&mut app, r#"-LINETYPE SET "My Fence""#);
    assert_eq!(doc(&app).header.current_linetype_name, "My Fence");
    script(&mut app, "-LINETYPE SET Continuous");
    script(&mut app, r#"-LINETYPE RENAME "My Fence" "Your Fence""#);
    assert!(doc(&app).line_types.contains("Your Fence"));
    script(&mut app, r#"-LINETYPE DELETE "Your Fence""#);
    assert!(
        !doc(&app).line_types.contains("Your Fence"),
        "{}",
        output(&app)
    );
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
}

#[test]
fn the_bare_command_prompts_for_each_value() {
    let mut app = fresh();
    for answer in ["-LINETYPE", "C", "ASKED", "", "2,-1"] {
        script(&mut app, answer);
    }
    assert_eq!(lengths(&app, "ASKED"), vec![2.0, -1.0], "{}", output(&app));
    assert_eq!(doc(&app).line_types.get("ASKED").unwrap().description, "");
    for answer in ["-LINETYPE", "R", "ASKED", "ANSWERED"] {
        script(&mut app, answer);
    }
    assert!(
        doc(&app).line_types.contains("ANSWERED"),
        "{}",
        output(&app)
    );
    for answer in ["-LINETYPE", "D", "ANSWERED"] {
        script(&mut app, answer);
    }
    assert!(
        !doc(&app).line_types.contains("ANSWERED"),
        "{}",
        output(&app)
    );
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
}

#[test]
fn list_set_and_load_keep_working() {
    let mut app = with_fence();
    run(&mut app, "-LINETYPE LIST");
    assert!(
        output(&app).contains("FENCE (Fence line)"),
        "{}",
        output(&app)
    );
    run(&mut app, "LINETYPE LIST");
    run(&mut app, "LINETYPE SET FENCE");
    assert_eq!(doc(&app).header.current_linetype_name, "FENCE");
    run(&mut app, "-LINETYPE SET ByLayer");
    assert_eq!(doc(&app).header.current_linetype_name, "ByLayer");
    let before = output(&app);
    run(&mut app, "-LINETYPE LOAD");
    assert!(
        !output(&app)[before.len()..].contains("Usage"),
        "{}",
        output(&app)
    );
}

#[test]
fn a_lowercase_command_line_works() {
    let mut app = fresh();
    run(&mut app, "-linetype create lower 1,-1");
    assert!(doc(&app).line_types.contains("lower"), "{}", output(&app));
    run(&mut app, "-linetype");
    assert!(
        app.tabs[app.active_tab].active_cmd.is_some(),
        "the prompts start"
    );
}

#[test]
fn a_wrong_argument_count_prints_the_usage() {
    let mut app = fresh();
    run(&mut app, "-LINETYPE RENAME ONLY");
    assert!(
        output(&app).contains("Usage: -LINETYPE"),
        "{}",
        output(&app)
    );
}

#[test]
fn split_quoted_keeps_quoted_runs_together() {
    assert_eq!(
        dash::split_quoted(r#"CREATE "My Fence" "" 1,-1"#),
        vec!["CREATE", "My Fence", "", "1,-1"]
    );
    assert_eq!(dash::split_quoted("  A   B "), vec!["A", "B"]);
    assert!(dash::split_quoted("").is_empty());
}
