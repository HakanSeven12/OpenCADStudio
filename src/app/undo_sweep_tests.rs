//! Undo sweep: each command in a list runs on the same small drawing, and
//! must add one undo step, be taken back exactly by UNDO and put back
//! exactly by REDO. "Exactly" is the whole document as a DXF save writes it,
//! so a change the undo recorder misses shows wherever it lands: an entity,
//! a table, a block or an object.

use super::*;
use document::entities::{Arc, Circle, Insert, Line, LwPolyline, Text};
use document::types::{Vector2, Vector3};
use document::EntityType;
use std::collections::HashMap;

/// The drawing every command starts from: lines (one crossing another, one
/// continuing another), a circle, an arc, a polyline, a text and an insert
/// of a one-line block, on layers `0` and `Walls`; nothing to undo.
struct Drawing {
    line: u64,
    upright: u64,
    crossing: u64,
    circle: u64,
    arc: u64,
    polyline: u64,
    text: u64,
    insert: u64,
}

fn drawing() -> (OpenCADStudio, Drawing) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"-LAYER NEW Walls"}"#);
    let tab = app.active_tab;
    let line = |a: [f64; 2], b: [f64; 2]| {
        EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        ))
    };
    let scene = &mut app.tabs[tab].scene;
    scene.define_block_raw("BLK", Vector3::ZERO, vec![line([0.0, 0.0], [4.0, 0.0])]);
    let mut on_walls = line([0.0, 0.0], [10.0, 0.0]);
    on_walls.common_mut().layer = "Walls".to_string();
    let mut arc = Arc::new();
    arc.center = Vector3::new(30.0, 5.0, 0.0);
    arc.radius = 3.0;
    arc.end_angle = std::f64::consts::PI;
    let mut text = Text::new();
    text.value = "Hello".to_string();
    text.insertion_point = Vector3::new(0.0, 40.0, 0.0);
    text.height = 2.0;
    let polyline = LwPolyline::from_points(vec![
        Vector2::new(0.0, 20.0),
        Vector2::new(10.0, 20.0),
        Vector2::new(10.0, 30.0),
    ]);
    let drawing = Drawing {
        line: scene.add_entity(on_walls).value(),
        upright: scene.add_entity(line([10.0, 0.0], [10.0, 10.0])).value(),
        crossing: scene.add_entity(line([5.0, -5.0], [5.0, 15.0])).value(),
        circle: scene
            .add_entity(EntityType::Circle(Circle::from_coords(20.0, 5.0, 0.0, 3.0)))
            .value(),
        arc: scene.add_entity(EntityType::Arc(arc)).value(),
        polyline: scene.add_entity(EntityType::LwPolyline(polyline)).value(),
        text: scene.add_entity(EntityType::Text(text)).value(),
        insert: scene
            .add_entity(EntityType::Insert(Insert::new(
                "BLK",
                Vector3::new(40.0, 0.0, 0.0),
            )))
            .value(),
    };
    app.tabs[tab].history = Default::default();
    (app, drawing)
}

/// The document as a DXF save writes it. Handles at or above the
/// document's next handle are the ones the writer hands out for its own
/// objects during the save; they depend on how far the handle seed has
/// advanced (handles are not reused after an undo), so they are renamed in
/// the order they appear. Every handle the document holds is kept as it is.
fn snapshot(app: &OpenCADStudio) -> String {
    let doc = &app.tabs[app.active_tab].scene.document;
    let seed = doc.next_handle();
    let bytes = crate::io::save_to_bytes(doc, "dxf", document::DxfVersion::AC1032).expect("save");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let mut renamed: HashMap<String, String> = HashMap::new();
    let mut out = String::new();
    for pair in lines.chunks(2) {
        let code: i32 = pair[0].trim().parse().unwrap_or(-1);
        let mut value = pair.get(1).copied().unwrap_or("").to_string();
        // Group codes whose value is a handle: the entity's own, its
        // owner's, and the soft and hard pointers and owners.
        if matches!(code, 5 | 105 | 320..=369 | 390 | 1005)
            && u64::from_str_radix(value.trim(), 16).is_ok_and(|h| h >= seed)
        {
            let next = format!("#{}", renamed.len());
            value = renamed.entry(value).or_insert(next).clone();
        }
        out.push_str(pair[0]);
        out.push('\n');
        out.push_str(&value);
        out.push('\n');
    }
    out
}

/// The first lines where two snapshots differ, with the line before each.
fn first_differences(a: &str, b: &str) -> String {
    let a: Vec<&str> = a.lines().collect();
    let b: Vec<&str> = b.lines().collect();
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("{} lines, then {}", a.len(), b.len()));
    }
    for (k, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            let before = if k > 0 { a[k - 1].trim() } else { "" };
            out.push(format!("line {k} (after {before:?}): {x:?}, then {y:?}"));
            if out.len() >= 4 {
                break;
            }
        }
    }
    out.join("; ")
}

/// The commands, as typed on the command line. Objects are given by handle.
fn commands(d: &Drawing) -> Vec<String> {
    let h = |v: u64| format!("{v:X}");
    vec![
        // Drawing.
        "LINE 0,50 10,50 ".to_string(),
        "CIRCLE 0,60 2".to_string(),
        "PLINE 0,70 10,70 10,80 ".to_string(),
        "ELLIPSE 0,100 10,100 3".to_string(),
        "RECTANG 0,110 10,120".to_string(),
        "POLYGON 6 0,130 C 5".to_string(),
        "SPLINE 0,140 5,145 10,140 ENTER".to_string(),
        "DIMLINEAR 0,0 10,0 5,-3".to_string(),
        "-INSERT BLK 50,50 1 1 0".to_string(),
        // Modifying.
        format!("MOVE {} ENTER 0,0 5,5", h(d.line)),
        format!("COPY {} ENTER 20,5 25,5 ENTER", h(d.circle)),
        format!("ROTATE {} ENTER 10,0 30", h(d.upright)),
        format!("SCALE {} ENTER 20,5 2", h(d.circle)),
        format!("MIRROR {} ENTER 0,10 10,10 N", h(d.line)),
        format!("ERASE {} ENTER", h(d.crossing)),
        format!("ERASE {} {} ENTER", h(d.arc), h(d.text)),
        format!("OFFSET 2 {} 5,1 ENTER", h(d.line)),
        format!("TRIM {} ENTER {} ENTER", h(d.crossing), h(d.line)),
        format!("EXTEND {} ENTER {} ENTER", h(d.upright), h(d.line)),
        "FILLET R 1 2,0 10,2".to_string(),
        "CHAMFER D 1 1 2,0 10,2".to_string(),
        "STRETCH C 9,-1 11,11 ENTER 0,0 2,0".to_string(),
        format!("BREAK {} 2,0 4,0", h(d.line)),
        format!("ARRAY {} ENTER R 2 2 10 10", h(d.circle)),
        format!("EXPLODE {} ENTER", h(d.polyline)),
        format!("EXPLODE {} ENTER", h(d.insert)),
        format!("PEDIT {} C X", h(d.polyline)),
        // The layer table.
        "-LAYER NEW Doors".to_string(),
        "-LAYER OFF Walls ".to_string(),
        "-LAYER COLOR 1 Walls ".to_string(),
        "-LAYER FREEZE Walls ".to_string(),
        "LAYER NEW Doors".to_string(),
        "LAYER OFF Walls".to_string(),
        "LAYER COLOR 1 Walls".to_string(),
        "LAYER FREEZE Walls".to_string(),
        "LAYER LOCK Walls".to_string(),
    ]
}

/// What is wrong with `command` on the drawing, if anything: it must
/// complete, change the document, add one undo step, and be taken back by
/// UNDO and put back by REDO exactly.
fn check(command: &str) -> Vec<String> {
    let (mut app, _) = drawing();
    let tab = app.active_tab;
    let before = snapshot(&app);
    let result = app.automation_op(&serde_json::json!({"op": "run", "cmd": command}).to_string());
    if result["status"] != "completed" {
        return vec![format!("{command}: did not complete: {result}")];
    }
    let after = snapshot(&app);
    if after == before {
        return vec![format!("{command}: changed nothing")];
    }
    let mut failures = Vec::new();
    let steps = app.tabs[tab].history.undo_stack.len();
    if steps != 1 {
        failures.push(format!("{command}: {steps} undo steps"));
    }
    app.undo_active_tab();
    let undone = snapshot(&app);
    if undone != before {
        failures.push(format!(
            "{command}: UNDO: {}",
            first_differences(&before, &undone)
        ));
    }
    app.redo_active_tab();
    let redone = snapshot(&app);
    if redone != after {
        failures.push(format!(
            "{command}: REDO: {}",
            first_differences(&after, &redone)
        ));
    }
    failures
}

/// Each command adds one undo step; UNDO gives back the drawing it started
/// from and REDO the drawing it made, both exactly.
#[test]
fn every_command_is_undone_and_redone_exactly() {
    let (_, d) = drawing();
    let failures: Vec<String> = commands(&d).iter().flat_map(|c| check(c)).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The comparison itself: two saves of one drawing are the same, and a
/// change to one entity shows.
#[test]
fn the_snapshot_is_stable_and_sees_a_change() {
    let (mut app, d) = drawing();
    let first = snapshot(&app);
    assert_eq!(first, snapshot(&app));
    let tab = app.active_tab;
    let circle = document::Handle::new(d.circle);
    if let Some(EntityType::Circle(c)) = app.tabs[tab].scene.document.get_entity_mut(circle) {
        c.radius = 3.5;
    }
    assert_ne!(first, snapshot(&app));
}
