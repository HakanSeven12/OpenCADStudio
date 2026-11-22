//! Undo sweep: each command in a list runs on the same small drawing, and
//! must add one undo step, be taken back exactly by UNDO and put back
//! exactly by REDO. "Exactly" is the whole document as a DXF save writes it,
//! so a change the undo recorder misses shows wherever it lands: an entity,
//! a table, a block or an object.

use super::*;
use crate::scene::pick::grip::GripEdit;
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
        "TEXT 0,60 2 0 Hello".to_string(),
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

/// One action of a case: what the user does through one of the app's doors.
enum Step {
    /// A command line, as typed. It must complete.
    Run(String),
    /// A command line that must stop with the named editor open (the
    /// `blocked_by` of the automation reply).
    Open(String, &'static str),
    /// An automation request other than `run` (a selection, for example).
    Op(serde_json::Value),
    /// A message, as a palette, a dialog or an editor sends it.
    Send(Box<Message>),
    /// Work with the pointer in the viewport (a grip drag, for example).
    Pointer(Box<dyn Fn(&mut OpenCADStudio)>),
}

/// A case of more than one action: its name for the report, and its steps.
struct Case {
    name: String,
    steps: Vec<Step>,
}

/// Changes made through the Properties palette: the object is selected, then
/// the palette sends its message.
fn palette_cases(d: &Drawing) -> Vec<Case> {
    use document::types::{Color, LineWeight};
    let case = |name: &str, handle: u64, message: Message| Case {
        name: format!("Properties palette: {name}"),
        steps: vec![
            Step::Op(serde_json::json!({
                "op": "select", "clear": true, "handles": [format!("{handle:X}")]
            })),
            Step::Send(Box::new(message)),
        ],
    };
    vec![
        case(
            "layer",
            d.circle,
            Message::PropLayerChanged("Walls".to_string()),
        ),
        case("color", d.line, Message::PropColorChanged(Color::Index(1))),
        case(
            "lineweight",
            d.arc,
            Message::PropLwChanged(LineWeight::W0_50),
        ),
        case(
            "linetype",
            d.polyline,
            Message::PropLinetypeChanged("Continuous".to_string()),
        ),
    ]
}

/// Text made in an editor: the command stops with the editor open, the text
/// goes in, and the editor's OK commits it.
fn editor_cases() -> Vec<Case> {
    vec![
        Case {
            name: "MTEXT in the editor".to_string(),
            steps: vec![
                Step::Open("MTEXT 0,50 20,40".to_string(), "mtext_editor"),
                Step::Send(Box::new(Message::MTextInsert("Notes".to_string()))),
                Step::Send(Box::new(Message::MTextOk)),
            ],
        },
        Case {
            name: "TEXT in the in-place editor".to_string(),
            steps: vec![
                Step::Open("TEXT 0,60 2 0".to_string(), "text_editor"),
                Step::Send(Box::new(Message::TextInlineInput("Hello".to_string()))),
                Step::Send(Box::new(Message::TextInlineOk)),
            ],
        },
    ]
}

/// A drag of each grip of each object of the drawing: the grip is taken, the
/// pointer moves two times, and the button is released.
fn grip_cases(app: &OpenCADStudio, d: &Drawing) -> Vec<Case> {
    let objects = [
        ("line", d.line),
        ("upright line", d.upright),
        ("crossing line", d.crossing),
        ("circle", d.circle),
        ("arc", d.arc),
        ("polyline", d.polyline),
        ("text", d.text),
        ("insert", d.insert),
    ];
    let document = &app.tabs[app.active_tab].scene.document;
    let mut cases = Vec::new();
    for (name, value) in objects {
        let handle = document::Handle::new(value);
        let entity = document.get_entity(handle).expect("the object");
        for grip in crate::scene::view::dispatch::grips(entity) {
            let (id, is_midpoint, world) = (grip.id, grip.is_midpoint, grip.world);
            cases.push(Case {
                name: format!("Grip {id} of the {name}"),
                steps: vec![Step::Pointer(Box::new(move |app: &mut OpenCADStudio| {
                    let tab = app.active_tab;
                    app.snapper.snap_enabled = false;
                    app.snapper.grid_snap_on = false;
                    app.snapper.otrack_enabled = false;
                    app.ortho_mode = false;
                    app.polar_mode = false;
                    app.tabs[tab].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
                    app.tabs[tab].active_grip =
                        Some(GripEdit::single(handle, id, is_midpoint, world));
                    for cursor in [Point::new(520.0, 220.0), Point::new(490.0, 180.0)] {
                        let _ = app.on_viewport_move(cursor);
                    }
                    let _ = app.on_viewport_left_release();
                }))],
            });
        }
    }
    cases
}

/// What is wrong with `command` on the drawing, if anything: it must
/// complete, change the document, add one undo step, and be taken back by
/// UNDO and put back by REDO exactly.
fn check(command: &str) -> Vec<String> {
    check_steps(command, &[Step::Run(command.to_string())])
}

/// `check` for a case of several steps. A step that selects or opens
/// something must not change the document by itself: the steps together
/// must change it, and add one undo step.
fn check_steps(name: &str, steps: &[Step]) -> Vec<String> {
    let (mut app, _) = drawing();
    let tab = app.active_tab;
    let before = snapshot(&app);
    for step in steps {
        match step {
            Step::Run(command) => {
                let result = app
                    .automation_op(&serde_json::json!({"op": "run", "cmd": command}).to_string());
                if result["status"] != "completed" {
                    return vec![format!("{name}: did not complete: {result}")];
                }
            }
            Step::Open(command, editor) => {
                let result = app
                    .automation_op(&serde_json::json!({"op": "run", "cmd": command}).to_string());
                if result["status"] != "waiting_input" || result["blocked_by"] != *editor {
                    return vec![format!("{name}: did not open the {editor}: {result}")];
                }
            }
            Step::Op(request) => {
                let result = app.automation_op(&request.to_string());
                if result["ok"] == false {
                    return vec![format!("{name}: {request} failed: {result}")];
                }
                // The app builds the palette again after each change of the
                // selection; the automation request does not.
                app.refresh_properties();
            }
            Step::Send(message) => {
                let _ = app.update(Message::clone(message));
            }
            Step::Pointer(action) => action(&mut app),
        }
    }
    let after = snapshot(&app);
    if after == before {
        return vec![format!("{name}: changed nothing")];
    }
    let mut failures = Vec::new();
    let steps = app.tabs[tab].history.undo_stack.len();
    if steps != 1 {
        failures.push(format!("{name}: {steps} undo steps"));
    }
    app.undo_active_tab();
    let undone = snapshot(&app);
    if undone != before {
        failures.push(format!(
            "{name}: UNDO: {}",
            first_differences(&before, &undone)
        ));
    }
    app.redo_active_tab();
    let redone = snapshot(&app);
    if redone != after {
        failures.push(format!(
            "{name}: REDO: {}",
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

/// The same for a change that the Properties palette makes to the selection.
#[test]
fn every_palette_change_is_undone_and_redone_exactly() {
    let (_, d) = drawing();
    let failures: Vec<String> = palette_cases(&d)
        .iter()
        .flat_map(|case| check_steps(&case.name, &case.steps))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The same for text that an editor makes.
#[test]
fn every_editor_commit_is_undone_and_redone_exactly() {
    let failures: Vec<String> = editor_cases()
        .iter()
        .flat_map(|case| check_steps(&case.name, &case.steps))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The same for a drag of each grip of each object.
#[test]
fn every_grip_drag_is_undone_and_redone_exactly() {
    let (app, d) = drawing();
    let cases = grip_cases(&app, &d);
    // A sweep with no case would pass: the drawing has 25 grips now.
    assert!(cases.len() >= 25, "{} grips", cases.len());
    let failures: Vec<String> = cases
        .iter()
        .flat_map(|case| check_steps(&case.name, &case.steps))
        .collect();
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
