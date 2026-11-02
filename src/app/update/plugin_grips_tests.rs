//! Plugin grip surface characterisation (API v11). Tests only; no
//! production code changed here.
//!
//! Paths covered (the commission's acceptance set):
//!   * read parity â€” HostApi `entity_grips` equals the automation `grips`
//!     read for the same single-selected entity (one pipeline, three
//!     consumers: viewport cache, MCP read, plugin read)
//!   * generality â€” arc (the 4-grip calibration shape), line, circle, a
//!     Spline (control-point grips) and a boxed-variant Helix
//!   * programmatic `grip_drag`: exactly one "GRIP"-labeled undo step with
//!     byte-exact restore; refusal parity (locked layer, reference-only,
//!     unknown grip/handle)
//!   * custom grips: declare/read parity, hover, press/hit (including
//!     coincident with a reference-only marker), drag phase flow, preview
//!     REPLACING per move with NO host ghost and the anchor entity not
//!     moving, settle writing from inside the callback in exactly one
//!     plugin-labeled step, Esc leaving the drawing byte-identical, axis
//!     hard-constraint, the absolute-point rule while the anchor moves,
//!     anchor-any-type (Spline/Helix), unload/death reaping, panic
//!     containment, budget caps
//!   * the coalescing model: 1000 parked points â†’ one in flight, newest
//!     delivered next, settle never dropped (unit-level in
//!     `crate::plugin::grip_points`)

use std::cell::RefCell;
use std::rc::Rc;

use codec::entities::{Arc, Circle, Line, Spline};
use codec::types::{Handle, Vector3};
use codec::EntityType;
use ocs_plugin_api::host::{
    CustomGripEvent, CustomGripPhase, CustomGripResponse, PluginGrip, PluginGripShape, PreviewWire,
};

use crate::app::{Message, OpenCADStudio};
use crate::plugin::grip_points::{self, PluginGripHover};

fn app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    // A real canvas size (the projection round-trip the grip ops drive has
    // the viewport's resolution; the test default is a stub-sized pane).
    let i = tab(&app);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 800.0);
    app
}

fn tab(app: &OpenCADStudio) -> usize {
    app.active_tab
}

fn add(app: &mut OpenCADStudio, entity: EntityType) -> Handle {
    let i = tab(app);
    let mut host = crate::app::plugin_host::HostSession::new(app, i);
    host.add_entity(entity)
}

fn line(start: [f64; 3], end: [f64; 3]) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(start[0], start[1], start[2]),
        Vector3::new(end[0], end[1], end[2]),
    ))
}

fn arc(center: [f64; 3], radius: f64, start: f64, end: f64) -> EntityType {
    EntityType::Arc(Arc::from_coords(
        center[0], center[1], center[2], radius, start, end,
    ))
}

fn entity_grips(app: &mut OpenCADStudio, handle: Handle) -> Vec<ocs_plugin_api::host::GripInfo> {
    let i = tab(app);
    let host = crate::app::plugin_host::HostSession::new(app, i);
    host.entity_grips(handle).expect("grip read")
}

fn select(app: &mut OpenCADStudio, handle: Handle) {
    let i = tab(app);
    app.tabs[i].scene.deselect_all();
    app.tabs[i].scene.select_entity(handle, false);
    app.refresh_selected_grips();
}

fn control(app: &mut OpenCADStudio, mut op: serde_json::Value) -> serde_json::Value {
    // The protocol envelope the automation bridges supply: document_id,
    // revision, client_id and a unique request_id, with running states
    // drained (the block-ops test helper's shape).
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let state = app.control_state();
    op["protocol"] = serde_json::json!(1);
    op["document_id"] = state["document_id"].clone();
    op["revision"] = state["revision"].clone();
    op["client_id"] = serde_json::json!("test");
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    op["request_id"] = serde_json::json!(format!("grip-{id}"));
    let (response, task) = app.control_request(op.clone());
    let _ = app.drive_headless_task(task);
    if matches!(response["status"].as_str(), Some("accepted" | "running")) {
        app.control_request(serde_json::json!({
            "op": "operation",
            "request_id": op["request_id"],
        }))
        .0
    } else {
        response
    }
}

fn undo_entry_count(app: &OpenCADStudio) -> usize {
    let history = &app.tabs[tab(app)].history;
    history.undo_stack.len() + usize::from(history.pending.is_some())
}

fn last_undo_label(app: &OpenCADStudio) -> Option<String> {
    let history = &app.tabs[tab(app)].history;
    history.undo_stack.last().map(|s| s.label().to_string())
}

fn serialized(app: &OpenCADStudio, handle: Handle) -> Vec<u8> {
    let entity = app.tabs[tab(app)]
        .scene
        .document
        .get_entity(handle)
        .expect("entity exists")
        .clone();
    bincode::serialize(&entity).expect("entity serializes")
}

fn radius_of(app: &OpenCADStudio, handle: Handle) -> f64 {
    match app.tabs[tab(app)]
        .scene
        .document
        .get_entity(handle)
        .expect("entity exists")
    {
        EntityType::Arc(arc) => arc.radius,
        other => panic!("not an arc: {other:?}"),
    }
}

/// Screen point (tile-relative) of a world point through the active tile's
/// camera â€” the same projection the grips overlay uses.
fn screen_of(app: &OpenCADStudio, world: [f64; 3]) -> iced::Point {
    let i = tab(app);
    let (vw, vh) = app.tabs[i].scene.selection.borrow().view.vp_size;
    let tile = app.tabs[i].scene.active_model_tile_bounds(vw, vh);
    let bounds = iced::Rectangle {
        width: tile.width,
        height: tile.height,
        ..iced::Rectangle::default()
    };
    app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(glam::DVec3::new(world[0], world[1], world[2]), bounds)
        .map(|screen| iced::Point::new(screen.x, screen.y))
        .expect("world point is in view")
}

fn declare(app: &mut OpenCADStudio, plugin: &str, grips: Vec<PluginGrip>) {
    let i = tab(app);
    let mut host = crate::app::plugin_host::HostSession::new(app, i);
    host.set_plugin_grips(plugin, grips)
        .expect("declaration accepted");
}

fn declared(grip_id: u64, point: [f64; 3]) -> PluginGrip {
    PluginGrip {
        grip_id,
        anchor: None,
        point,
        axis: None,
        shape: PluginGripShape::Square,
    }
}

/// A sink that records every event and can settle-write like the motivating
/// consumer (one push_undo scope + one entity update from inside the
/// callback).
struct SinkState {
    events: Rc<RefCell<Vec<CustomGripEvent>>>,
    /// (arc handle, new radius) the settle writes, when set.
    settle_write: Rc<RefCell<Option<(Handle, f64)>>>,
    /// Preview wire returned for begin/move.
    preview: Rc<RefCell<Option<PreviewWire>>>,
    /// Live-geometry entities returned for begin/move (API v12): the
    /// plugin-directed visual phase.
    live_geom: Rc<RefCell<Vec<(Handle, EntityType)>>>,
    /// Panic on the settle phase (panic-containment test).
    panic_on_settle: Rc<RefCell<bool>>,
    /// (descriptor design, its per-move tolerance) returned on the Begin
    /// answer (API v14): the descriptor-backed drag's opt-in.
    begin_descriptor: Rc<RefCell<Option<(ocs_plugin_api::host::PluginDisplayDescriptor, f64)>>>,
}

fn install_sink(plugin: &str) -> SinkState {
    let state = SinkState {
        events: Rc::new(RefCell::new(Vec::new())),
        settle_write: Rc::new(RefCell::new(None)),
        preview: Rc::new(RefCell::new(None)),
        live_geom: Rc::new(RefCell::new(Vec::new())),
        panic_on_settle: Rc::new(RefCell::new(false)),
        begin_descriptor: Rc::new(RefCell::new(None)),
    };
    let events = Rc::clone(&state.events);
    let settle_write = Rc::clone(&state.settle_write);
    let preview = Rc::clone(&state.preview);
    let live_geom = Rc::clone(&state.live_geom);
    let panic_on_settle = Rc::clone(&state.panic_on_settle);
    let begin_descriptor = Rc::clone(&state.begin_descriptor);
    grip_points::install_in_process_sink(plugin, move |host, event| {
        events.borrow_mut().push(event.clone());
        match event.phase {
            CustomGripPhase::Begin | CustomGripPhase::Move => {
                if event.phase == CustomGripPhase::Begin {
                    if let Some((chain, tolerance)) = begin_descriptor.borrow().clone() {
                        return CustomGripResponse {
                            preview: Vec::new(),
                            live_entities: Vec::new(),
                            descriptor: Some(chain),
                            descriptor_tolerance: Some(tolerance),
                        };
                    }
                }
                CustomGripResponse {
                    preview: preview.borrow().clone().into_iter().collect(),
                    live_entities: live_geom.borrow().clone(),
                    descriptor: None,
                    descriptor_tolerance: None,
                }
            },
            CustomGripPhase::Settle => {
                if *panic_on_settle.borrow() {
                    panic!("plugin settle handler is buggy");
                }
                if let Some((handle, radius)) = *settle_write.borrow() {
                    // The consumer's exact shape: its own one-scope contract.
                    host.push_undo("PLUGIN: PI radius");
                    if let Some(EntityType::Arc(arc)) = host.document().get_entity(handle).cloned()
                    {
                        let mut updated = arc;
                        updated.radius = radius;
                        assert!(host.update_entity(EntityType::Arc(updated)));
                    }
                }
                CustomGripResponse::default()
            }
            CustomGripPhase::Cancel => CustomGripResponse::default(),
        }
    });
    state
}

// â”€â”€ Read parity + generality (requirement 1/6/9) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn entity_grips_arc_matches_the_four_grip_calibration() {
    let mut app = app();
    // R = 15 at a 90Â° corner: center + both tangent-point endpoints + the
    // radius grip at the parametric mid-angle of the arc.
    let handle = add(
        &mut app,
        arc([10.0, 10.0, 0.0], 15.0, 0.0, std::f64::consts::FRAC_PI_2),
    );
    let grips = entity_grips(&mut app, handle);
    assert_eq!(grips.len(), 4, "arc exposes exactly 4 grips: {grips:?}");
    assert_eq!(grips[0].grip_id, 0);
    assert!(grips[0].moves_entity, "grip 0 is the center translate grip");
    assert_eq!(grips[0].shape, "Square");
    assert_eq!(grips[0].point, [10.0, 10.0, 0.0]);
    let radius_grip = grips
        .iter()
        .find(|g| g.grip_id == 3)
        .expect("radius grip id 3");
    // Mid-angle of [0Â°, 90Â°] is 45Â°: the grip sits at center + RÂ·(cos45, sin45).
    let mid = 15.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!((radius_grip.point[0] - (10.0 + mid)).abs() < 1e-9);
    assert!((radius_grip.point[1] - (10.0 + mid)).abs() < 1e-9);
    assert!(!radius_grip.moves_entity);
}

#[test]
fn entity_grips_line_and_circle_read() {
    let mut app = app();
    let line_handle = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let grips = entity_grips(&mut app, line_handle);
    assert!(grips.len() >= 3, "line: endpoints + midpoint: {grips:?}");
    assert!(grips.iter().any(|g| g.moves_entity));
    assert!(grips.iter().any(|g| g.point == [10.0, 0.0, 0.0]));

    let circle_handle = add(
        &mut app,
        EntityType::Circle(Circle::from_center_radius(Vector3::new(5.0, 5.0, 0.0), 4.0)),
    );
    let grips = entity_grips(&mut app, circle_handle);
    assert!(!grips.is_empty());
    assert!(
        grips
            .iter()
            .any(|g| g.moves_entity && g.point == [5.0, 5.0, 0.0]),
        "circle center translate grip: {grips:?}"
    );
}

#[test]
fn entity_grips_are_generic_over_entity_types_including_boxed_variants() {
    let mut app = app();
    // Spline: control-point grips.
    let spline = add(
        &mut app,
        EntityType::Spline(Spline::from_control_points(
            3,
            vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
                Vector3::new(10.0, 10.0, 0.0),
                Vector3::new(0.0, 10.0, 0.0),
            ],
        )),
    );
    let grips = entity_grips(&mut app, spline);
    assert!(
        grips.len() >= 4,
        "spline exposes control-point grips: {grips:?}"
    );
    assert!(grips.iter().any(|g| g.point == [10.0, 0.0, 0.0]));

    // Helix: a recently-BOXED wide variant (`Box<Helix>`); the read must
    // match through the box.
    let helix = codec::entities::Helix {
        radius: 5.0,
        turns: 3.0,
        axis_base_point: Vector3::new(0.0, 0.0, 0.0),
        start_point: Vector3::new(5.0, 0.0, 0.0),
        ..codec::entities::Helix::default()
    };
    let helix_handle = add(&mut app, EntityType::Helix(Box::new(helix)));
    let grips = entity_grips(&mut app, helix_handle);
    assert!(
        !grips.is_empty(),
        "boxed helix grips must enumerate through the Box: {grips:?}"
    );
}

#[test]
fn host_api_read_matches_the_mcp_grips_read_for_the_same_entity() {
    // Requirement 9's drift test: the viewport cache, the automation read
    // and the plugin read all come from ONE pipeline, so a single-selected
    // entity must agree across all three.
    let mut app = app();
    let handle = add(
        &mut app,
        arc([0.0, 0.0, 0.0], 8.0, 0.0, std::f64::consts::PI),
    );
    select(&mut app, handle);
    // The viewport cache (what the interactive path hit-tests against):
    let cached = app.tabs[tab(&app)]
        .selected_grips
        .iter()
        .map(|g| {
            (
                g.id,
                g.world.to_array(),
                g.is_midpoint,
                format!("{:?}", g.shape),
            )
        })
        .collect::<Vec<_>>();
    // The MCP read:
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    assert_eq!(read["ok"], serde_json::json!(true));
    let mcp: Vec<(u64, [f64; 3], bool, String)> = read["grips"]
        .as_array()
        .expect("grips array")
        .iter()
        .map(|g| {
            (
                g["grip_id"].as_u64().expect("grip_id"),
                [
                    g["point"][0].as_f64().expect("x"),
                    g["point"][1].as_f64().expect("y"),
                    g["point"][2].as_f64().expect("z"),
                ],
                g["moves_entity"].as_bool().expect("moves_entity"),
                g["shape"].as_str().expect("shape").to_string(),
            )
        })
        .collect();
    // The HostApi read:
    let host_api = entity_grips(&mut app, handle)
        .into_iter()
        .map(|g| (g.grip_id, g.point, g.moves_entity, g.shape))
        .collect::<Vec<_>>();
    assert_eq!(cached.len(), mcp.len());
    assert_eq!(cached.len(), host_api.len());
    for (i, ((cid, cpoint, cmoves, cshape), (hid, hpoint, hmoves, hshape))) in
        cached.iter().zip(host_api.iter()).enumerate()
    {
        assert_eq!(cid, hid, "grip {i} id drifts");
        assert_eq!(cshape, hshape, "grip {i} shape drifts");
        assert_eq!(cmoves, hmoves, "grip {i} moves-entity drifts");
        for k in 0..3 {
            assert!(
                (cpoint[k] - hpoint[k]).abs() < 1e-12,
                "grip {i} point drifts: {cpoint:?} vs {hpoint:?}"
            );
        }
    }
    for (i, (mcp_entry, host_entry)) in mcp.iter().zip(host_api.iter()).enumerate() {
        assert_eq!(
            mcp_entry.0 as usize, host_entry.0,
            "MCP/HostApi grip {i} id drifts"
        );
        assert_eq!(mcp_entry.3, host_entry.3);
    }
}

// â”€â”€ Programmatic grip drag (requirement 2) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn grip_drag_composes_one_undo_step_with_byte_exact_restore() {
    let mut app = app();
    let handle = add(
        &mut app,
        arc([0.0, 0.0, 0.0], 8.0, 0.0, std::f64::consts::PI),
    );
    let before = serialized(&app, handle);
    let before_undo = undo_entry_count(&app);

    let i = tab(&mut app);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    // Grip 0 is the center translate grip: the drag moves the whole entity
    // so the center lands exactly on `to` (the rubber-band anchor semantics
    // the interactive path uses).
    host.grip_drag(handle, 0, [12.0, 0.0, 0.0])
        .expect("grip drag applies");
    drop(host);

    // Exactly ONE new undo step, labeled like the interactive grip step.
    assert_eq!(undo_entry_count(&app), before_undo + 1);
    assert_eq!(last_undo_label(&app).as_deref(), Some("GRIP"));
    match app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(handle)
        .expect("entity exists")
    {
        EntityType::Arc(arc) => {
            // The programmatic drag lands through the screen projection, so
            // the tolerance is the projection's own (far below one pixel).
            assert!((arc.center.x - 12.0).abs() < 1e-3);
            assert!(arc.center.y.abs() < 1e-3);
            assert!(
                (arc.radius - 8.0).abs() < 1e-9,
                "translate keeps the radius"
            );
        }
        other => panic!("not an arc: {other:?}"),
    }

    // Byte-exact restore on undo.
    let _ = app.update(Message::Undo);
    assert_eq!(undo_entry_count(&app), before_undo);
    assert_eq!(serialized(&app, handle), before);
}

#[test]
fn grip_drag_refusal_parity() {
    let mut app = app();
    let locked = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let reference = add(&mut app, arc([50.0, 0.0, 0.0], 5.0, 0.0, 1.0));

    // Reference-only first: the designation refuses on locked layers, and
    // both fixtures share layer "0" (only the first fixture gets locked).
    {
        let i = tab(&mut app);
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        host.set_reference_only(reference, true).expect("mark");
        let grips = host.entity_grips(reference).expect("read");
        assert!(grips.is_empty(), "reference-only answers no grips");
        let ref_err = host.grip_drag(reference, 0, [1.0, 0.0, 0.0]).unwrap_err();
        assert!(
            ref_err.contains("reference_only") || ref_err.contains("unknown_grip"),
            "{ref_err}"
        );
    }

    // Lock the first entity's layer through the layers table.
    let i = tab(&app);
    let layer_name = app.tabs[i]
        .scene
        .document
        .get_entity(locked)
        .map(|entity| entity.common().layer.clone())
        .expect("entity layer");
    if let Some(layer) = app.tabs[i].scene.document.layers.get_mut(&layer_name) {
        layer.flags.locked = true;
    }

    // Locked-layer grip drags refuse with parity to the interactive path.
    let i = tab(&mut app);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    let locked_err = host.grip_drag(locked, 0, [1.0, 0.0, 0.0]).unwrap_err();
    assert!(
        locked_err.contains("layer_locked") || locked_err.contains("locked"),
        "{locked_err}"
    );
    drop(host);

    // Unlock, then the unknown-grip/unknown-handle refusals behave normally.
    if let Some(layer) = app.tabs[i].scene.document.layers.get_mut(&layer_name) {
        layer.flags.locked = false;
    }
    let plain = add(&mut app, arc([100.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let i = tab(&mut app);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    let unknown_grip = host.grip_drag(plain, 99, [1.0, 0.0, 0.0]).unwrap_err();
    assert!(unknown_grip.contains("unknown_grip"), "{unknown_grip}");
    let unknown_handle = host.grip_drag(Handle::from(0xFEED_F00Du64), 0, [1.0, 0.0, 0.0]);
    assert!(unknown_handle.is_err());
}

// â”€â”€ Custom grips: declare, read, hover, hit, drag, settle, cancel â”€â”€â”€â”€â”€â”€â”€â”€â”€

const TEST_PLUGIN: &str = "test.grip-points";

#[test]
fn custom_grips_declare_read_and_hover() {
    let mut app = app();
    let state = install_sink(TEST_PLUGIN);
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![
            declared(7, [10.0, 10.0, 0.0]),
            declared(8, [30.0, 0.0, 0.0]),
        ],
    );
    // Read parity: the MCP read lists the plugin grips with their owner.
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    let plugin_grips = read["plugin_grips"].as_array().expect("plugin_grips");
    assert_eq!(plugin_grips.len(), 2);
    assert_eq!(plugin_grips[0]["plugin"], serde_json::json!(TEST_PLUGIN));
    assert_eq!(plugin_grips[0]["grip_id"], serde_json::json!(7));
    assert_eq!(
        plugin_grips[0]["point"],
        serde_json::json!([10.0, 10.0, 0.0])
    );
    assert_eq!(plugin_grips[0]["moves_entity"], serde_json::json!(false));

    // Whole-batch REPLACE: an empty batch clears.
    declare(&mut app, TEST_PLUGIN, Vec::new());
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    assert_eq!(
        read["plugin_grips"].as_array().expect("plugin_grips").len(),
        0
    );

    // Hover: the same overlay pipeline highlights the plugin grip.
    declare(&mut app, TEST_PLUGIN, vec![declared(7, [10.0, 10.0, 0.0])]);
    let i = tab(&mut app);
    let screen = screen_of(&app, [10.0, 10.0, 0.0]);
    app.update_grip_hover(i, screen);
    assert_eq!(
        app.plugin_grip_hover,
        Some(PluginGripHover {
            plugin: TEST_PLUGIN.to_string(),
            grip_id: 7,
        })
    );
    app.update_grip_hover(i, iced::Point::new(screen.x + 500.0, screen.y + 500.0));
    assert_eq!(app.plugin_grip_hover, None);

    grip_points::remove_in_process_sink(TEST_PLUGIN);
    let _ = state;
}

#[test]
fn custom_grip_drag_settles_as_one_plugin_labeled_step() {
    let mut app = app();
    let anchor = add(
        &mut app,
        arc([10.0, 10.0, 0.0], 8.0, 0.0, std::f64::consts::FRAC_PI_2),
    );
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);

    let state = install_sink(TEST_PLUGIN);
    // The calibration consumer: a custom grip at an implied PI â€” the tangent
    // intersection of the adjacent line directions â€” anchored to the arc,
    // with the corner's bisector as the hard drag axis.
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![PluginGrip {
            grip_id: 42,
            anchor: Some(anchor),
            point: [10.0 + 8.0 * std::f64::consts::FRAC_1_SQRT_2, 10.0, 0.0],
            axis: None,
            shape: PluginGripShape::Square,
        }],
    );
    *state.settle_write.borrow_mut() = Some((anchor, 5.0));
    *state.preview.borrow_mut() = Some(PreviewWire::circle(
        [10.0, 10.0, 0.0],
        12.0,
        Some([1.0, 0.5, 0.0, 1.0]),
    ));

    let i = tab(&app);
    // Hover + press: the plugin grip engages through the same pipeline the
    // viewport uses.
    let screen = screen_of(
        &app,
        [10.0 + 8.0 * std::f64::consts::FRAC_1_SQRT_2, 10.0, 0.0],
    );
    let candidate = app
        .hit_plugin_grip(i, screen)
        .expect("the plugin grip is hittable");
    assert_eq!(candidate.grip_id, 42);
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    assert_eq!(state.events.borrow().len(), 1);
    assert_eq!(state.events.borrow()[0].phase, CustomGripPhase::Begin);
    assert_eq!(state.events.borrow()[0].grip_id, 42);
    // The begin answer's preview is already live.
    assert_eq!(app.tabs[i].scene.preview_wires.len(), 1);

    // Move: the plugin's preview REPLACES per event; the host draws no ghost
    // and moves nothing (the anchor arc is untouched mid-drag).
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(18.0, 10.0, 0.0));
    let events = state.events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].phase, CustomGripPhase::Move);
    assert_eq!(events[1].point, [18.0, 10.0, 0.0]);
    assert_eq!(
        app.tabs[i].scene.preview_wires.len(),
        1,
        "the plugin's preview is the only live feedback"
    );
    assert!(
        (radius_of(&app, anchor) - 8.0).abs() < 1e-12,
        "the anchor entity does not move by host action during the drag"
    );
    drop(events);

    // Settle: the plugin writes from INSIDE the callback â€” exactly ONE new
    // undo step, plugin-labeled; the preview clears.
    let _ = app.settle_plugin_grip_drag();
    let events = state.events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(events[2].phase, CustomGripPhase::Settle);
    assert_eq!(events[2].point, [18.0, 10.0, 0.0]);
    drop(events);
    assert_eq!(undo_entry_count(&app), before_undo + 1);
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("PLUGIN: PI radius"),
        "the settled drag totals exactly the plugin's one labeled step"
    );
    assert!((radius_of(&app, anchor) - 5.0).abs() < 1e-12);
    assert!(app.tabs[i].scene.preview_wires.is_empty());

    // Undo restores the arc byte-exactly.
    let _ = app.update(Message::Undo);
    assert_eq!(undo_entry_count(&app), before_undo);
    assert_eq!(serialized(&app, anchor), before);

    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn custom_grip_escape_leaves_the_drawing_byte_identical() {
    let mut app = app();
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [3.0, 3.0, 0.0])]);
    let i = tab(&app);
    let screen = screen_of(&app, [3.0, 3.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(9.0, 9.0, 0.0));
    // Esc cancels through the same lifecycle exit built-in drags use.
    assert!(app.cancel_active_grip_edit());
    let events = state.events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(events[2].phase, CustomGripPhase::Cancel);
    drop(events);
    assert_eq!(undo_entry_count(&app), before_undo, "cancel writes nothing");
    assert_eq!(serialized(&app, anchor), before);
    assert!(app.tabs[i].scene.preview_wires.is_empty());
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn custom_grip_axis_hard_constrains_the_drag() {
    let mut app = app();
    let state = install_sink(TEST_PLUGIN);
    // Axis along +X through the grip point: a drag toward (20, 7) must
    // deliver points ON the constraint line (the hard-constraint decision).
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![PluginGrip {
            grip_id: 3,
            anchor: None,
            point: [10.0, 0.0, 0.0],
            axis: Some([1.0, 0.0, 0.0]),
            shape: PluginGripShape::Circle,
        }],
    );
    let i = tab(&app);
    let screen = screen_of(&app, [10.0, 0.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    // Drive the cursor off-axis through the viewport's own move path: the
    // event carries the projected point on the constraint line.
    let off_axis = screen_of(&app, [20.0, 7.0, 0.0]);
    let (vw, vh) = app.tabs[i].scene.selection.borrow().view.vp_size;
    let tile = app.tabs[i].scene.active_model_tile_bounds(vw, vh);
    let _ = app.on_viewport_move(iced::Point::new(off_axis.x + tile.x, off_axis.y + tile.y));
    let events = state.events.borrow();
    let moved = events
        .iter()
        .rev()
        .find(|event| event.phase == CustomGripPhase::Move)
        .expect("a move was delivered");
    assert!(
        moved.point[1].abs() < 1e-6,
        "hard-constrained axis keeps the drag on the line, got {:?}",
        moved.point
    );
    assert!(moved.point[0] > 10.0, "the drag travels along the axis");
    drop(events);
    let _ = app.settle_plugin_grip_drag();
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn custom_grip_point_is_absolute_while_the_anchor_moves() {
    let mut app = app();
    let state = install_sink(TEST_PLUGIN);
    // Anchor accepts ANY live entity type â€” here a Spline â€” and NEVER adds
    // host-side movement semantics: the point stays exactly where declared.
    let spline = add(
        &mut app,
        EntityType::Spline(Spline::from_control_points(
            2,
            vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)],
        )),
    );
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![PluginGrip {
            grip_id: 9,
            anchor: Some(spline),
            point: [25.0, 40.0, 0.0],
            axis: None,
            shape: PluginGripShape::Triangle,
        }],
    );
    let i = tab(&app);
    // Events route to the owner through the anchor's tab.
    let screen = screen_of(&app, [25.0, 40.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    assert_eq!(candidate.anchor, Some(spline));
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.cancel_plugin_grip_drag();

    // The host re-declares nothing on its own: the plugin's pull model. The
    // anchor entity's move delivers no events at all.
    state.events.borrow_mut().clear();
    // Move the anchor entity: the declared point does not follow.
    let moved = match app.tabs[i].scene.document.get_entity(spline).cloned() {
        Some(EntityType::Spline(spline)) => spline,
        other => panic!("not a spline: {other:?}"),
    };
    let mut moved = moved;
    moved.control_points[0] = Vector3::new(500.0, 500.0, 0.0);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    assert!(host.update_entity(EntityType::Spline(moved)));
    drop(host);
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    let plugin_grips = read["plugin_grips"].as_array().expect("plugin_grips");
    assert_eq!(plugin_grips.len(), 1);
    assert_eq!(
        plugin_grips[0]["point"],
        serde_json::json!([25.0, 40.0, 0.0]),
        "the host never moves, offsets or auto-follows a custom grip's point"
    );
    // The host re-declares nothing on its own: the plugin's pull model.
    assert!(state.events.borrow().is_empty());
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn custom_grip_wins_the_hit_over_a_coincident_reference_only_marker() {
    let mut app = app();
    let marker = add(
        &mut app,
        EntityType::Circle(Circle::from_center_radius(
            Vector3::new(10.0, 10.0, 0.0),
            4.0,
        )),
    );
    {
        let i = tab(&app);
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        host.set_reference_only(marker, true).expect("mark");
    }
    let _state = install_sink(TEST_PLUGIN);
    // The grip sits EXACTLY ON the plugin's own reference-only outline.
    declare(&mut app, TEST_PLUGIN, vec![declared(5, [10.0, 10.0, 0.0])]);
    let i = tab(&app);
    let screen = screen_of(&app, [10.0, 10.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen);
    assert!(
        candidate.is_some(),
        "a custom grip coincident with its own reference-only marker stays fully hittable"
    );
    // And the press engages the drag (not the click-through entity pick).
    let candidate = candidate.unwrap();
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    assert!(app.tabs[i].active_plugin_grip.is_some());
    let _ = app.cancel_plugin_grip_drag();
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn custom_grips_die_with_their_plugin_and_nothing_else_changes() {
    let mut app = app();
    let other_entity = add(&mut app, line([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]));
    let _state_a = install_sink("grip.plugin-a");
    let _state_b = install_sink("grip.plugin-b");
    declare(
        &mut app,
        "grip.plugin-a",
        vec![declared(1, [1.0, 1.0, 0.0])],
    );
    declare(
        &mut app,
        "grip.plugin-b",
        vec![declared(2, [2.0, 2.0, 0.0])],
    );
    let other_before = serialized(&app, other_entity);
    // Plugin A dies (its sink is removed): the reap clears exactly its set.
    grip_points::remove_in_process_sink("grip.plugin-a");
    app.reap_plugin_grips();
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    let plugin_grips = read["plugin_grips"].as_array().expect("plugin_grips");
    assert_eq!(plugin_grips.len(), 1);
    assert_eq!(
        plugin_grips[0]["plugin"],
        serde_json::json!("grip.plugin-b")
    );
    assert_eq!(serialized(&app, other_entity), other_before);
    grip_points::remove_in_process_sink("grip.plugin-b");
}

#[test]
fn disabled_plugins_grips_are_invisible() {
    let mut app = app();
    let _state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [1.0, 1.0, 0.0])]);
    let i = tab(&app);
    let screen = screen_of(&app, [1.0, 1.0, 0.0]);
    assert!(app.hit_plugin_grip(i, screen).is_some());
    app.disabled_plugins.insert(TEST_PLUGIN.to_string());
    assert!(
        app.hit_plugin_grip(i, screen).is_none(),
        "disabled plugins' grips disappear"
    );
    let read = control(&mut app, serde_json::json!({"op": "grips"}));
    assert_eq!(
        read["plugin_grips"].as_array().expect("plugin_grips").len(),
        0
    );
    app.disabled_plugins.remove(TEST_PLUGIN);
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn a_panicking_grip_handler_never_takes_the_host_down() {
    let mut app = app();
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [2.0, 2.0, 0.0])]);
    *state.panic_on_settle.borrow_mut() = true;
    *state.settle_write.borrow_mut() = Some((anchor, 1.0));
    let i = tab(&app);
    let screen = screen_of(&app, [2.0, 2.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(3.0, 3.0, 0.0));
    // The settle handler panics: the guard contains it, the host survives,
    // the drawing is untouched and no undo step is recorded.
    let _ = app.settle_plugin_grip_drag();
    assert_eq!(serialized(&app, anchor), before);
    assert_eq!(undo_entry_count(&app), before_undo);
    assert!(
        !crate::plugin::drain_errors().is_empty(),
        "the panic is reported"
    );
    // The host keeps working afterwards.
    let handle = add(&mut app, line([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]));
    assert!(handle.value() > 0);
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn mcp_grip_drag_routes_a_custom_grip_to_its_owner() {
    // MCP parity: declare â†’ read â†’ drag through the automation op, exactly
    // the recipe the consumer tests with.
    let mut app = app();
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 8.0, 0.0, 1.0));
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(11, [8.0, 0.0, 0.0])]);
    *state.settle_write.borrow_mut() = Some((anchor, 4.0));
    let outcome = control(
        &mut app,
        serde_json::json!({
            "op": "grip_drag",
            "plugin": TEST_PLUGIN,
            "grip_id": 11,
            "point": [9.0, 0.0, 0.0],
        }),
    );
    assert_eq!(outcome["ok"], serde_json::json!(true), "{outcome}");
    let events = state.events.borrow();
    let phases: Vec<CustomGripPhase> = events.iter().map(|e| e.phase).collect();
    assert_eq!(
        phases,
        vec![
            CustomGripPhase::Begin,
            CustomGripPhase::Move,
            CustomGripPhase::Settle
        ]
    );
    drop(events);
    assert_eq!(undo_entry_count(&app), before_undo + 1);
    assert_eq!(last_undo_label(&app).as_deref(), Some("PLUGIN: PI radius"));
    assert!((radius_of(&app, anchor) - 4.0).abs() < 1e-12);

    // Unknown plugin-grip id refuses.
    let refused = control(
        &mut app,
        serde_json::json!({
            "op": "grip_drag",
            "plugin": TEST_PLUGIN,
            "grip_id": 999,
            "point": [9.0, 0.0, 0.0],
        }),
    );
    assert_eq!(refused["ok"], serde_json::json!(false), "{refused}");
    assert!(
        refused.to_string().contains("unknown_grip"),
        "the refusal names the code: {refused}"
    );
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn hover_hits_through_two_thousand_declared_grips() {
    // Requirement 8's scale case: a street network's worth of declared
    // grips; hover goes through the per-tab uniform grid with view culling,
    // never a per-grip linear scan per cursor move.
    let mut app = app();
    let _state = install_sink(TEST_PLUGIN);
    let grips: Vec<PluginGrip> = (0..2000)
        .map(|i| {
            declared(
                i as u64,
                [(i % 100) as f64 * 0.003, (i / 100) as f64 * 0.003, 0.0],
            )
        })
        .collect();
    declare(&mut app, TEST_PLUGIN, grips);
    let i = tab(&app);
    // Grip 53 sits at (0.159, 0); hover right on it.
    let screen = screen_of(&app, [0.159, 0.0, 0.0]);
    let hit = app
        .hit_plugin_grip(i, screen)
        .expect("found through the grid");
    assert_eq!(hit.grip_id, 53);
    // A point far outside the cluster hovers nothing.
    let bare = screen_of(&app, [5.0, 5.0, 0.0]);
    assert!(app.hit_plugin_grip(i, bare).is_none());
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

fn live_arc(anchor: Handle, radius: f64, app: &OpenCADStudio) -> codec::EntityType {
    match app.tabs[tab(app)]
        .scene
        .document
        .get_entity(anchor)
        .expect("anchor exists")
    {
        EntityType::Arc(arc) => {
            let mut live = arc.clone();
            live.radius = radius;
            EntityType::Arc(live)
        }
        other => panic!("not an arc: {other:?}"),
    }
}

#[test]
fn live_geometry_moves_elements_during_the_drag() {
    // v11.2 acceptance: the plugin's move answers carry live entity
    // geometry â€” the drag shows the true final state at every instant
    // through the plugin's returned geometry only (visual parity with the
    // built-in radius grip), ZERO history steps mid-drag, the originals
    // restored before the settle, and the plugin's one labeled step at
    // mouse-up with byte-exact undo.
    let mut app = app();
    let anchor = add(
        &mut app,
        arc([10.0, 10.0, 0.0], 8.0, 0.0, std::f64::consts::FRAC_PI_2),
    );
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![PluginGrip {
            grip_id: 42,
            anchor: Some(anchor),
            point: [12.0, 10.0, 0.0],
            axis: None,
            shape: PluginGripShape::Square,
        }],
    );
    *state.settle_write.borrow_mut() = Some((anchor, 5.0));
    *state.preview.borrow_mut() = Some(PreviewWire::circle(
        [10.0, 10.0, 0.0],
        12.0,
        Some([1.0, 0.5, 0.0, 1.0]),
    ));
    // The plugin's live answer: the arc's candidate geometry for the move.
    *state.live_geom.borrow_mut() = vec![(anchor, live_arc(anchor, 6.25, &app))];

    let i = tab(&app);
    let screen = screen_of(&app, [12.0, 10.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.pump_plugin_grips();
    assert!(
        (radius_of(&app, anchor) - 6.25).abs() < 1e-9,
        "the begin answer's live geometry already moves the element"
    );
    assert_eq!(undo_entry_count(&app), before_undo, "zero steps mid-drag");
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(14.0, 10.0, 0.0));
    assert!(
        (radius_of(&app, anchor) - 6.25).abs() < 1e-9,
        "the move answer's live geometry is applied live"
    );
    assert_eq!(app.tabs[i].scene.preview_wires.len(), 1);
    assert_eq!(
        undo_entry_count(&app),
        before_undo,
        "still zero steps mid-drag"
    );

    // Mouse-up: the visual phase is restored BEFORE the settle, the plugin
    // writes its authoritative edit inside its scope â€” exactly ONE step.
    let _ = app.settle_plugin_grip_drag();
    assert!((radius_of(&app, anchor) - 5.0).abs() < 1e-12);
    assert_eq!(undo_entry_count(&app), before_undo + 1);
    assert_eq!(last_undo_label(&app).as_deref(), Some("PLUGIN: PI radius"));
    let _ = app.update(Message::Undo);
    assert_eq!(serialized(&app, anchor), before, "undo restores byte-exact");
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn live_geometry_esc_discards_the_visual_state_byte_identically() {
    let mut app = app();
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(3, [3.0, 0.0, 0.0])]);
    *state.live_geom.borrow_mut() = vec![(anchor, live_arc(anchor, 9.0, &app))];
    let i = tab(&app);
    let screen = screen_of(&app, [3.0, 0.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.pump_plugin_grips();
    assert!((radius_of(&app, anchor) - 9.0).abs() < 1e-12, "live moved");
    assert!(app.cancel_active_grip_edit());
    assert_eq!(
        serialized(&app, anchor),
        before,
        "Esc restores the visual-only state byte-identically"
    );
    assert_eq!(undo_entry_count(&app), before_undo, "cancel writes nothing");
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn live_geometry_caps_are_refused_with_an_honest_headline() {
    let mut app = app();
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [3.0, 0.0, 0.0])]);
    let overflow = ocs_plugin_api::host::MAX_PLUGIN_LIVE_ENTITIES + 1;
    let way_too_big: Vec<(codec::Handle, codec::EntityType)> = (0..overflow)
        .map(|k| (anchor, live_arc(anchor, 5.0 + k as f64, &app)))
        .collect();
    *state.live_geom.borrow_mut() = way_too_big;
    let i = tab(&app);
    let screen = screen_of(&app, [3.0, 0.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.pump_plugin_grips();
    assert!(
        (radius_of(&app, anchor) - 5.0).abs() < 1e-12,
        "a too-large live payload applies nothing"
    );
    let errors = crate::plugin::drain_errors();
    assert!(
        errors
            .iter()
            .any(|line| line.contains("exceeds the per-response cap")),
        "the refusal names the cap: {errors:?}"
    );
    let _ = app.cancel_plugin_grip_drag();
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn entities_batch_reads_live_handles_without_snapshot_clones() {
    // v11.1 defect 3: per-entity reads are the refresh-pass path â€” the
    // answers mirror the live document exactly, unknown handles answer
    // None, and no snapshot is involved.
    let mut app = app();
    let first = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let second = add(&mut app, line([0.0, 0.0, 0.0], [9.0, 0.0, 0.0]));
    let i = tab(&app);
    let batch = {
        let host = crate::app::plugin_host::HostSession::new(&mut app, i);
        host.entities_batch(&[first, second, Handle::from(0xFEED_F00Du64)])
            .expect("batch read")
    };
    assert_eq!(batch.len(), 3);
    let first_read = batch[0]
        .as_ref()
        .map(|entity| bincode::serialize(entity).unwrap());
    assert_eq!(
        first_read,
        Some(serialized(&app, first)),
        "the first handle reads byte-equal"
    );
    assert!(
        matches!(&batch[1], Some(EntityType::Line(_))),
        "the second handle reads by kind"
    );
    assert!(batch[2].is_none(), "an unknown handle answers None");
}

#[test]
fn settle_after_many_moves_writes_one_plugin_step() {
    // A long move phase, then a settle whose plugin writes land in the
    // plugin's own undo step; an unchanged 1 000-grip re-declare is a no-op.
    let mut app = app();
    let anchor = add(
        &mut app,
        arc([0.0, 0.0, 0.0], 8.0, 0.0, std::f64::consts::FRAC_PI_2),
    );
    let state = install_sink(TEST_PLUGIN);
    declare(
        &mut app,
        TEST_PLUGIN,
        vec![PluginGrip {
            grip_id: 1,
            anchor: Some(anchor),
            point: [4.0, 4.0, 0.0],
            axis: None,
            shape: PluginGripShape::Square,
        }],
    );
    *state.settle_write.borrow_mut() = Some((anchor, 5.0));
    *state.preview.borrow_mut() = Some(PreviewWire::circle(
        [0.0, 0.0, 0.0],
        6.0,
        Some([1.0, 0.6, 0.0, 1.0]),
    ));
    let i = tab(&app);
    let screen = screen_of(&app, [4.0, 4.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");

    // Move-phase: 1 000 cursor points through the channel.
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    for k in 0..1000u32 {
        let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(4.0, 4.0, k as f64 * 0.01));
    }

    // Settle-write: the full begin â†’ move â†’ settle-with-writes window.
    let _ = app.settle_plugin_grip_drag();
    assert_eq!(last_undo_label(&app).as_deref(), Some("PLUGIN: PI radius"));
    assert!((radius_of(&app, anchor) - 5.0).abs() < 1e-12);

    // Re-declare diff at 1 000 grips: the unchanged batch must stay cheap.
    let batch: Vec<PluginGrip> = (0..1000)
        .map(|k| declared(k as u64, [k as f64, 42.0, 0.0]))
        .collect();
    declare(&mut app, TEST_PLUGIN, batch);
    let batch: Vec<PluginGrip> = (0..1000)
        .map(|k| declared(k as u64, [k as f64, 42.0, 0.0]))
        .collect();
    declare(&mut app, TEST_PLUGIN, batch);
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

#[test]
fn declaration_validates_anchors_and_caps() {
    let mut app = app();
    let _state = install_sink(TEST_PLUGIN);
    let i = tab(&app);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    // An anchor that does not exist is refused (any live type is fine).
    let err = host
        .set_plugin_grips(
            TEST_PLUGIN,
            vec![PluginGrip {
                grip_id: 1,
                anchor: Some(Handle::from(0xDEAD_BEEFu64)),
                point: [0.0; 3],
                axis: None,
                shape: PluginGripShape::Square,
            }],
        )
        .unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
    // An unknown plugin cannot declare at all.
    let err = host
        .set_plugin_grips("not.a.plugin", vec![declared(1, [0.0; 3])])
        .unwrap_err();
    assert!(err.contains("not a loaded, alive plugin"), "{err}");
    // Per-plugin cap.
    let huge: Vec<PluginGrip> = (0..=grip_points::MAX_PLUGIN_GRIPS_PER_PLUGIN)
        .map(|i| declared(i as u64, [i as f64, 0.0, 0.0]))
        .collect();
    assert!(host.set_plugin_grips(TEST_PLUGIN, huge).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
mod notification_attach {
    use super::*;

    /// Deterministic NOTIFICATION attach (API v13 hardening, item 2): the
    /// consumer's real-host validation saw ~4/5 fresh boots where COMMAND
    /// dispatch worked but the notification channel never attached â€” the
    /// plugin's boundary work never ran, total silence. The runner now acks
    /// `ChannelReady` at main-loop entry and the host defers boot-window
    /// broadcasts until the ack (flushing them then); this test asserts
    /// DocumentChanged DELIVERY on N consecutive FRESH boots: a plugin
    /// write's epoch must arrive at the plugin (observed through the
    /// template's MP_LAST_NOTIF probe), with bounded retries for the
    /// handshake. `OCS_TEST_PLUGIN`-gated like the spawn tests.
    #[test]
    fn plugin_notifications_attach_deterministically_across_fresh_spawns() {
        // Serialized against the other process-spawning tests: they share the
        // process-global manager and the ChannelReady attach queue.
        let _guard = crate::plugin::external::PROCESS_TESTS_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A busy machine must not cost a runner its liveness: raise the
        // plugin call timeout (never lowers any floor).
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "10");
        let path = match std::env::var_os("OCS_TEST_PLUGIN") {
            Some(p) => std::path::PathBuf::from(p),
            None => return,
        };
        if !path.exists() {
            eprintln!("OCS_TEST_PLUGIN does not exist: {}", path.display());
            return;
        }
        let host_exe = std::path::PathBuf::from(
            std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
                .unwrap_or_else(|| std::env::current_exe().unwrap().into_os_string()),
        );
        assert!(
            host_exe.exists(),
            "host exe not found: {}",
            host_exe.display()
        );
        std::env::set_var("OCS_PLUGIN_RUNNER_EXE", &host_exe);

        const FRESH_BOOTS: usize = 3;
        for boot in 1..=FRESH_BOOTS {
            let mut app = OpenCADStudio::new_for_test();
            let _ = app.automation_op(r#"{"op":"new"}"#);
            let baseline_epoch = app.tabs[0].scene.geometry_epoch;
            let mut host = crate::app::plugin_host::HostSession::new(&mut app, 0);
            // Spawn THROUGH THE MANAGER (like a real boot): broadcasts and
            // dispatches reach the plugin through the same live machinery the
            // app uses.
            let (id, process) = crate::plugin::external::spawn_into_manager_for_tests(
                &path,
                &mut host,
                crate::plugin::v4_support::notification_handler(),
            )
            .unwrap_or_else(|e| panic!("fresh boot {boot}: spawn failed: {e}"));
            // A plugin write bumps the epoch and broadcasts DocumentChanged
            // (deferred until the runner acks ChannelReady, then flushed â€”
            // the deferral is what makes this deterministic).
            let handle =
                host.add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                    codec::types::Vector3::new(0.0, 0.0, 0.0),
                    codec::types::Vector3::new(3.0, 0.0, 0.0),
                )));
            assert!(!handle.is_null());
            drop(host);
            let target_epoch = app.tabs[0].scene.geometry_epoch;
            assert!(
                target_epoch > baseline_epoch,
                "fresh boot {boot}: the plugin write must bump the geometry epoch"
            );
            // Bounded polling: the notification must arrive â€” silence is the
            // exact flake this test forbids.
            let mut observed = None;
            let mut transient = 0;
            for _ in 0..40 {
                // The boundary tick processes the ChannelReady queue (the
                // reader thread only enqueues; the main thread attaches).
                let _ = app.update(crate::app::Message::DrainPluginRequests);
                // The probe goes through the process handle directly (the
                // broadcast under test uses the manager; the probe is test
                // instrumentation and keeps its load surface minimal).
                let handled = process
                    .dispatch(
                        &mut crate::app::plugin_host::HostSession::new(&mut app, 0),
                        "MP_LAST_NOTIF",
                        &mut |_| {},
                    )
                    .unwrap_or_else(|e| panic!("fresh boot {boot}: dispatch failed: {e}"));
                if !handled {
                    transient += 1;
                    assert!(
                        transient < 10,
                        "fresh boot {boot}: MP_LAST_NOTIF never dispatched healthily"
                    );
                }
                // The command line prefixes Info entries with the â“˜ icon;
                // match on the payload, not the prefix.
                if let Some(version) = app.command_line.history.iter().rev().find_map(|entry| {
                    entry
                        .text
                        .strip_prefix("â“˜ ")
                        .unwrap_or(entry.text.as_str())
                        .strip_prefix("LAST_NOTIF ")
                        .and_then(|tail| tail.trim().parse::<u64>().ok())
                }) {
                    if version != u64::MAX {
                        observed = Some(version);
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let observed = observed.unwrap_or_else(|| {
                let tail: Vec<String> = app
                    .command_line
                    .history
                    .iter()
                    .rev()
                    .take(8)
                    .map(|entry| entry.text.clone())
                    .collect();
                panic!(
                    "fresh boot {boot}/{FRESH_BOOTS}: DocumentChanged never reached the \
                     plugin (expected epoch {target_epoch}) â€” the notification-attach \
                     race is a release blocker. History tail: {tail:?}"
                )
            });
            assert!(
                observed >= target_epoch,
                "fresh boot {boot}: stale epoch {observed} < {target_epoch}"
            );
            assert!(
                crate::plugin::external::remove_plugin_for_tests(&id),
                "fresh boot {boot}: cleanup must remove the spawned plugin"
            );
        }
    }
}

// â”€â”€ v13 hardening: RunQuiet, constraint_create, dirty regions, defect 5 â”€â”€

#[test]
fn run_quiet_suppresses_the_command_line_echo() {
    let mut app = app();
    let i = tab(&app);
    // Baseline: a plain machine Run echoes the command into the history.
    let before_plain = app.command_line.history.len();
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        let outcome = host.run_command(ocs_plugin_api::host::CommandRequest::Run {
            line: "CIRCLE 30,0 5".to_string(),
        });
        assert!(outcome.is_ok(), "{outcome:?}");
    }
    let after_plain = app.command_line.history.len();
    assert!(
        after_plain > before_plain,
        "a plain Run echoes into the history (the contrast case)"
    );

    // The same command as a QUIET replay: same geometry, ZERO echo (errors
    // would still surface â€” this command errors none).
    let before_quiet = app.command_line.history.len();
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        let outcome = host.run_command(ocs_plugin_api::host::CommandRequest::RunQuiet {
            line: "CIRCLE 40,0 5".to_string(),
        });
        assert!(outcome.is_ok(), "{outcome:?}");
    }
    assert_eq!(
        app.command_line.history.len(),
        before_quiet,
        "a quiet machine replay leaves no machinery echo in the history"
    );
    // And the command actually ran: a second circle exists.
    let circles = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Circle(_)))
        .count();
    assert_eq!(
        circles, 2,
        "RunQuiet executes the command, it only hides the echo"
    );
}

#[test]
fn constraint_create_bonds_without_echo_in_one_plugin_step() {
    let mut app = app();
    let a = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let b = add(&mut app, line([10.0, 0.5, 0.0], [20.0, 0.5, 0.0]));
    let before_a = serialized(&app, a);
    let before_b = serialized(&app, b);
    let before_undo = undo_entry_count(&app);
    let history_before = app.command_line.history.len();

    let i = tab(&app);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    // The re-bond shape: the plugin opens its one scope, then composes the
    // constraint directly â€” no COINCIDENT command replay, no prompt echo.
    host.push_undo("PLUGIN: re-bond");
    host.constraint_create(ocs_plugin_api::host::PluginConstraint::Coincident {
        first: ocs_plugin_api::host::PluginConstraintPick {
            handle: Some(a),
            point: [10.0, 0.0, 0.0],
            whole_curve: false,
        },
        second: ocs_plugin_api::host::PluginConstraintPick {
            handle: Some(b),
            point: [10.0, 0.5, 0.0],
            whole_curve: false,
        },
    })
    .expect("constraint create bonds the endpoints");
    drop(host);
    app.finish_pending_history(i);

    // ONE plugin-labeled undo step; zero command-line echo.
    assert_eq!(undo_entry_count(&app), before_undo + 1);
    assert_eq!(last_undo_label(&app).as_deref(), Some("PLUGIN: re-bond"));
    assert_eq!(
        app.command_line.history.len(),
        history_before,
        "constraint_create never touches the command line"
    );
    // The constraint exists and references both entities.
    let scope = app.tabs[i].current_parametric_scope();
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(scope)
        .expect("a constraint set exists");
    assert_eq!(set.constraints.len(), 1);

    // Undo removes the bond byte-exactly, constraint included.
    let _ = app.update(Message::Undo);
    assert_eq!(serialized(&app, a), before_a);
    assert_eq!(serialized(&app, b), before_b);
    assert!(app.tabs[i]
        .scene
        .parametric_constraint_set(scope)
        .is_none_or(|set| set.constraints.is_empty()));
}

#[test]
fn dirty_regions_ride_the_boundary_notification() {
    let mut app = app();
    let handle = add(&mut app, line([0.0, 0.0, 0.0], [7.0, 0.0, 0.0]));
    // The plugin-write path marked the entity and published the region set
    // at its immediate boundary; the recorded broadcast names the counts.
    let log = crate::plugin::v4_support::take_test_dispatch_log();
    assert!(
        log.iter()
            .any(|entry| { entry == "DocumentChangedRegions(1 entities, 0 records)" }),
        "the touched entity rides the boundary: {log:?}"
    );
    // After the boundary the accumulation is empty again.
    assert!(!crate::plugin::v4_support::take_test_dispatch_log()
        .iter()
        .any(|entry| entry.contains("DocumentChangedRegions")));
    // And the write path recorded the record kind too when a record lands.
    let i = tab(&app);
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        assert!(host.write_record(
            handle,
            codec::xdata::ExtendedDataRecord {
                application_name: "PLUGIN".to_string(),
                values: vec![codec::xdata::XDataValue::String("pi-grip".to_string())],
            },
        ));
    }
    let log = crate::plugin::v4_support::take_test_dispatch_log();
    assert!(
        log.iter()
            .any(|entry| entry == "DocumentChangedRegions(0 entities, 1 records)"),
        "the touched (entity, app) pair rides the boundary: {log:?}"
    );
}

#[test]
fn live_geometry_does_not_trigger_the_constraint_solver() {
    // v12.1 defect 5, the regression the consumer captured: live geometry on
    // a BONDED chain must stay visual-only â€” the solver must not run mid-
    // drag (no partner movement, no history, byte-identical on Esc).
    let mut app = app();
    let a = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let b = add(&mut app, line([10.0, 0.5, 0.0], [20.0, 0.5, 0.0]));
    let before_a = serialized(&app, a);
    let before_undo = undo_entry_count(&app);

    // Bond the endpoints (coincident) through the new API â€” the chain whose
    // live drag previously dragged the solver in.
    let i = tab(&app);
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        host.constraint_create(ocs_plugin_api::host::PluginConstraint::Coincident {
            first: ocs_plugin_api::host::PluginConstraintPick {
                handle: Some(a),
                point: [10.0, 0.0, 0.0],
                whole_curve: false,
            },
            second: ocs_plugin_api::host::PluginConstraintPick {
                handle: Some(b),
                point: [10.0, 0.5, 0.0],
                whole_curve: false,
            },
        })
        .expect("bond");
        app.finish_pending_history(i);
    }
    let bonded_b = serialized(&app, b);

    // A drag whose live answer moves ONLY entity `a` (the plugin's choice â€”
    // it returns exactly what its settle will commit).
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [5.0, 0.0, 0.0])]);
    let moved_a = match app.tabs[i].scene.document.get_entity(a).cloned() {
        Some(EntityType::Line(line)) => {
            let mut moved = line.clone();
            moved.start = codec::types::Vector3::new(2.0, 0.0, 0.0);
            EntityType::Line(moved)
        }
        other => panic!("not a line: {other:?}"),
    };
    *state.live_geom.borrow_mut() = vec![(a, moved_a)];

    let screen = screen_of(&app, [5.0, 0.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.pump_plugin_grips();

    // Live geometry applied to `a` only; the coincident partner `b` stays
    // byte-identical (the solver did NOT run); zero history mid-drag.
    assert_ne!(serialized(&app, a), before_a, "the live geometry moved a");
    assert_eq!(
        serialized(&app, b),
        bonded_b,
        "the bonded partner must not move by host action mid-drag"
    );
    assert_eq!(
        undo_entry_count(&app),
        before_undo + 1,
        "the bond's own step; the drag adds none"
    );

    // Esc discards the visual phase byte-identically.
    assert!(app.cancel_active_grip_edit());
    assert_eq!(serialized(&app, a), before_a);
    assert_eq!(serialized(&app, b), bonded_b);
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

// â”€â”€ The settle watchdog strike ledger â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// The watchdog floor is lowered from SETTLE_WATCHDOG (10 s) to 10 ms so the
// missed-delivery ledger runs with millisecond budgets; the silent plugin is
// the existing in-process shape (a panicked settle handler is contained as
// "produced no answer", the delivery stays outstanding, and the pump's
// expiry strike fires on a later pass). The OUT-OF-PROCESS semantics the
// ledger decides (keep the connection on strike one, `custom_grip_abandon`
// on the third) sit one layer below: the transport's own abandon test
// covers the tear itself, the ledger covers the host-side decision lines.

/// Let the lowered settle deadline pass, then pump: the watchdog strikes.
fn strike_outstanding_settle(app: &mut OpenCADStudio) {
    std::thread::sleep(app.plugin_grip_settle_watchdog * 3);
    let _ = app.pump_plugin_grips();
}

fn strike_count(app: &OpenCADStudio, plugin: &str) -> usize {
    app.plugin_grip_settle_strikes
        .get(plugin)
        .map(|at| at.len())
        .unwrap_or(0)
}

/// W1: the first strike ends the drag as a cancel while the plugin's
/// connection, declared grips and session all stay; W4: the error line
/// carries the strike counter and the keep decision; a recovered settle
/// then serves the grips again AND resets the strike count.
#[test]
fn first_settle_strike_cancels_the_drag_and_keeps_the_plugin_grips() {
    let mut app = app();
    app.plugin_grip_settle_watchdog = std::time::Duration::from_millis(10);
    let anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let before = serialized(&app, anchor);
    let before_undo = undo_entry_count(&app);
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [2.0, 2.0, 0.0])]);
    *state.panic_on_settle.borrow_mut() = true;
    let i = tab(&app);
    let screen = screen_of(&app, [2.0, 2.0, 0.0]);
    let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(3.0, 3.0, 0.0));
    let _ = app.settle_plugin_grip_drag();
    strike_outstanding_settle(&mut app);

    // The drag ended as a cancel: nothing drawn, no undo step, no pending
    // settle, and the strike surfaced with its counter.
    assert!(app.active_plugin_grip().is_none());
    assert!(!app.is_plugin_grip_settling());
    assert_eq!(serialized(&app, anchor), before);
    assert_eq!(undo_entry_count(&app), before_undo);
    let errors = crate::plugin::drain_errors();
    assert!(
        errors
            .iter()
            .any(|line| line.contains("strike 1/3") && line.contains("the connection is kept")),
        "the first strike keeps the plugin and says so: {errors:?}"
    );
    assert_eq!(strike_count(&app, TEST_PLUGIN), 1);

    // W3: the grips are still served â€” a healthy drag now runs through to
    // the plugin's own labeled undo step, exactly as before the strike, and
    // the recovery resets the strike count.
    *state.panic_on_settle.borrow_mut() = false;
    *state.settle_write.borrow_mut() = Some((anchor, 2.5));
    let candidate = app.hit_plugin_grip(i, screen).expect("still hittable");
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(3.0, 3.0, 0.0));
    let _ = app.settle_plugin_grip_drag();
    assert!(
        app.drive_plugin_grip_settle_to_completion(std::time::Duration::from_millis(500)),
        "the recovered settle finalizes"
    );
    assert_eq!(last_undo_label(&app).as_deref(), Some("PLUGIN: PI radius"));
    assert_eq!(strike_count(&app, TEST_PLUGIN), 0, "the recovery resets");
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

/// W2: the third missed delivery inside the window tears the connection
/// down (the plugin is genuinely wedged) and clears the ledger; W4: every
/// strike landed a line; W3: the teardown queues the grip re-arm, which the
/// reap tick dispatches (the SelectionChanged dedup cleared) the moment the
/// plugin is visible again.
#[test]
fn third_settle_strike_tears_the_connection_down_and_arms_the_rearm() {
    let mut app = app();
    app.plugin_grip_settle_watchdog = std::time::Duration::from_millis(10);
    let _anchor = add(&mut app, arc([0.0, 0.0, 0.0], 5.0, 0.0, 1.0));
    let state = install_sink(TEST_PLUGIN);
    declare(&mut app, TEST_PLUGIN, vec![declared(1, [2.0, 2.0, 0.0])]);
    *state.panic_on_settle.borrow_mut() = true;
    let i = tab(&app);
    let screen = screen_of(&app, [2.0, 2.0, 0.0]);
    for expected in [1usize, 2, 3] {
        let candidate = app.hit_plugin_grip(i, screen).expect("hittable");
        let _ = app.begin_plugin_grip_drag(i, &candidate);
        let _ = app.settle_plugin_grip_drag();
        assert_eq!(strike_count(&app, TEST_PLUGIN), expected - 1);
        strike_outstanding_settle(&mut app);
        if expected < 3 {
            assert_eq!(strike_count(&app, TEST_PLUGIN), expected);
        } else {
            // The teardown branch clears the ledger in the same strike: a
            // relaunched plugin starts from a clean count.
            assert_eq!(strike_count(&app, TEST_PLUGIN), 0);
        }
    }
    let errors = crate::plugin::drain_errors();
    assert!(
        errors
            .iter()
            .any(|line| line.contains("strike 3/3") && line.contains("connection torn down")),
        "the third strike tears the connection down: {errors:?}"
    );
    assert_eq!(
        errors
            .iter()
            .filter(|line| line.contains("custom-grip settle delivery"))
            .count(),
        3,
        "every strike surfaced its line: {errors:?}"
    );
    assert_eq!(
        strike_count(&app, TEST_PLUGIN),
        0,
        "the teardown clears the ledger"
    );

    // The re-arm: queued at the teardown, dispatched by the reap tick with
    // the fingerprint dedup cleared (an unchanged selection still reaches
    // the plugin), and the grips remain servable throughout.
    assert!(app.pending_plugin_grips_rearm.contains(TEST_PLUGIN));
    assert!(
        app.hit_plugin_grip(i, screen).is_some(),
        "the grips survive"
    );
    let _ = crate::plugin::v4_support::take_test_dispatch_log();
    app.reap_plugin_grips();
    assert!(app.pending_plugin_grips_rearm.is_empty());
    let log = crate::plugin::v4_support::take_test_dispatch_log();
    assert!(
        log.iter().any(|entry| entry.contains("SelectionChanged")),
        "the reap re-dispatches the selection broadcast for the re-arm: {log:?}"
    );
    assert!(
        app.hit_plugin_grip(i, screen).is_some(),
        "still servable after the re-arm"
    );
    grip_points::remove_in_process_sink(TEST_PLUGIN);
}

// â”€â”€ The descriptor-backed grip drag (API v14) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[test]
fn descriptor_reanchor_solves_ramp_ends_by_bisection() {
    // The EndCurvatureScale anchor law voices the whole driving ramp
    // through the deterministic bracketed bisection (16 doublings + 60
    // bisects): a taller end needs a bigger scale; the resulting chain
    // passes its own audit (the staging build). The fixture restates the
    // old sigmoid's sheet reading as dense sampled slope data — the same
    // table law a plugin ships for its transcendental records.
    use crate::app::update::plugin_grips::reanchor_profile_chain;
    use ocs_plugin_api::host::AnchorLaw;
    let values: Vec<f64> = (0..101)
        .map(|i| {
            let xi = i as f64 / 100.0;
            0.025 + 0.225 * xi * xi
        })
        .collect();
    let chain = ocs_plugin_api::host::ParametricChain {
        plane: ocs_plugin_api::host::ParametricPlane::Profile,
        segments: vec![ocs_plugin_api::host::ParametricSegment {
            def: ocs_plugin_api::host::CurveSegmentDef {
                length: 100.0,
                ramp: ocs_plugin_api::host::Ramp::Sampled {
                    n: 101,
                    values: values.clone(),
                },
                cross_slope: None,
            },
            start_point: [0.0, 0.0],
            start_direction: values[0].atan(),
        }],
        layer: String::new(),
    };
    // Record-model sanity: the declared end height (the unsigned re-read).
    let (base, _) = crate::parametric::build_display_entities(&chain, 1e-3)
        .expect("the base chain derives");
    let _ = base;

    // The re-anchor: the same run (100.0), the end lifted to 11.0, the
    // scale law solving through the bisection discipline.
    let edited = reanchor_profile_chain(
        &chain,
        0,
        AnchorLaw::EndCurvatureScale,
        glam::DVec3::new(100.0, 11.0, 0.0),
    )
    .expect("the height target solves");
    let ocs_plugin_api::host::Ramp::Sampled {
        values: solved, ..
    } = &edited.segments[0].def.ramp
    else {
        panic!("the scale keeps the sampled arm");
    };
    let end_value = solved[solved.len() - 1];
    assert!(
        (end_value - 0.25).abs() > 1e-6,
        "the ramp's end value moved up to absorb the height: {end_value}"
    );
    let sample = crate::parametric::sample_chain(&edited, 100.0)
        .expect("samples")
        .expect("at the run");
    assert!(
        (sample.point[1] - 11.0).abs() < 1e-7,
        "the solved end height hits the target: {}",
        sample.point[1]
    );
    // The re-derived design passes its own audit (the staging build).
    crate::parametric::build_display_entities(&edited, 1e-3).expect("the audit accepts");
    // Determinism: the same move solves the same value.
    let again = reanchor_profile_chain(
        &chain,
        0,
        AnchorLaw::EndCurvatureScale,
        glam::DVec3::new(100.0, 11.0, 0.0),
    )
    .expect("solves again");
    let ocs_plugin_api::host::Ramp::Sampled {
        values: solved_again,
        ..
    } = &again.segments[0].def.ramp
    else {
        panic!("the scale keeps the sampled arm");
    };
    assert_eq!(
        solved_again[solved_again.len() - 1].to_bits(),
        end_value.to_bits(),
        "the bisection is bit-deterministic"
    );
}

/// The crest chain the drag-policy tests stage (a {constant slope,
/// slope-linear, constant slope} profile sheet as ramp data): the crest's
/// end sits at (300, 8) under the mean-gradient law.
fn crest_profile_chain() -> ocs_plugin_api::host::ParametricChain {
    ocs_plugin_api::host::ParametricChain {
        plane: ocs_plugin_api::host::ParametricPlane::Profile,
        segments: vec![
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    length: 100.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 0,
                        coeffs: [0.01, 0.0, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [0.0, 0.0],
                start_direction: 0.01f64.atan(),
            },
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    length: 200.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 1,
                        coeffs: [0.01, 0.05, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [100.0, 1.0],
                start_direction: 0.01f64.atan(),
            },
            ocs_plugin_api::host::ParametricSegment {
                def: ocs_plugin_api::host::CurveSegmentDef {
                    length: 80.0,
                    ramp: ocs_plugin_api::host::Ramp::Polynomial {
                        degree: 0,
                        coeffs: [0.06, 0.0, 0.0, 0.0],
                    },
                    cross_slope: None,
                },
                start_point: [300.0, 8.0],
                start_direction: 0.06f64.atan(),
            },
        ],
        layer: String::new(),
    }
}

#[test]
fn descriptor_begin_engages_and_moves_restage_in_place() {
    // The full drag-pipeline arc (API v14): the Begin answer hands the host
    // the live design; the coalesced cursor move re-derives the display set
    // LOCALLY (no Move frames cross the IPC, zero history steps, the honest
    // restage patches the same handles); the settle is the plugin's one
    // write window â€” records first, then the re-stage â€” one labeled undo
    // step throughout, and the undo round-trips byte-exactly.
    let mut app = app();
    let plugin = "PLUGIN";
    let i = tab(&app);
    // A sink first: the class register's own aliveness gate treats an in-process test sink as the loaded+alive marker (grip_points); the full handler installed below replaces this placeholder.
    crate::plugin::grip_points::install_in_process_sink(plugin, |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    host.register_plugin_class(
        plugin,
        &ocs_plugin_api::host::PluginClassDeclaration {
            class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
            schema_version: 4,
            grip_schema: vec![ocs_plugin_api::host::PluginGripSchemaEntry {
                grip_id: 7,
                param_name: "peak_gradient".to_string(),
                drives: Some(ocs_plugin_api::host::PluginGripAnchor {
                    segment: 1,
                    at_start: false,
                    anchor: ocs_plugin_api::host::AnchorLaw::EndStationToXY,
                }),
            }],
            parameters: vec![ocs_plugin_api::host::PluginClassParameter {
                name: "peak_gradient".to_string(),
                value_kind: ocs_plugin_api::host::PluginClassParameterKind::Real,
                editable: true,
            }],
        },
    )
    .expect("the class registers");
    let handles = host
        .stage_plugin_class_instance(
            plugin,
            &ocs_plugin_api::host::PluginClassInstance {
                class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
                schema_version: 4,
                design: ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                    crest_profile_chain(),
                ),
                tolerance: 1e-3,
                layer: "PLUGIN_PROFILE".to_string(),
                design_record: "PLUGIN_ALIGN_DISPLAY".to_string(),
            },
        )
        .expect("the crest instance stages");
    assert_eq!(handles.len(), 3, "three exact carriers stage");
    host.create_xrecord(
        &["PLUGIN"],
        "PLUGIN_ALIGN_DISPLAY",
        vec![
            codec::objects::XRecordEntry::string(1, "peak_gradient"),
            codec::objects::XRecordEntry::new(
                140,
                codec::objects::XRecordValue::Double(0.06),
            ),
        ],
    )
    .expect("the design record seeds");
    drop(host);
    let undo_before = undo_entry_count(&app);

    // The plugin's own grip declaration + Begin answer: the live design and
    // the SAME per-move tolerance its staging used.
    let events: Rc<RefCell<Vec<CustomGripEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let crest_cell = Rc::new(RefCell::new(crest_profile_chain()));
    let handles_cell = Rc::new(RefCell::new(handles.clone()));
    let row_cell = Rc::new(RefCell::new(handles[0]));
    let settle_write = Rc::new(RefCell::new(true));
    {
        let events2 = Rc::clone(&events);
        let crest2 = Rc::clone(&crest_cell);
        let handles2 = Rc::clone(&handles_cell);
        let row2 = Rc::clone(&row_cell);
        let settle2 = Rc::clone(&settle_write);
        grip_points::install_in_process_sink(plugin, move |host, event| {
            events2.borrow_mut().push(event.clone());
            match event.phase {
                CustomGripPhase::Begin => CustomGripResponse {
                    preview: Vec::new(),
                    live_entities: Vec::new(),
                    descriptor: Some(
                        ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                            crest_profile_chain(),
                        ),
                    ),
                    descriptor_tolerance: Some(1e-3),
                },
                CustomGripPhase::Move => CustomGripResponse::default(),
                CustomGripPhase::Settle => {
                    if !*settle2.borrow() {
                        return CustomGripResponse::default();
                    }
                    // The plugin's write window: the RECORD row first (its
                    // own labeled step), then the re-stage â€” push_undo +
                    // scope-less entity updates over the same handles, one
                    // labeled step.
                    host.update_plugin_class_instance_parameter(
                        *row2.borrow(),
                        "peak_gradient",
                        "0.09",
                    )
                    .expect("the parameter row writes in the window");
                    host.push_undo("PLUGIN: grip edit");
                    let final_point =
                        glam::DVec3::new(event.point[0], event.point[1], event.point[2]);
                    let design = crate::app::update::plugin_grips::reanchor_profile_chain(
                        &crest2.borrow().clone(),
                        1,
                        ocs_plugin_api::host::AnchorLaw::EndStationToXY,
                        final_point,
                    )
                    .expect("the plugin's own law re-derives the settled design");
                    let (fresh, _) =
                        crate::parametric::build_display_entities(&design, 1e-3)
                            .expect("the settled design derives");
                    let staged = handles2.borrow().clone();
                    assert_eq!(fresh.len(), staged.len(), "the restage is count-stable");
                    for (slot, fresh_entity) in staged.iter().zip(fresh.iter()) {
                        let current = host
                            .document()
                            .get_entity(*slot)
                            .cloned()
                            .expect("the staged carrier lives");
                        let mut patched = current;
                        crate::app::update::plugin_grips::patch_carrier_geometry(
                            &mut patched,
                            fresh_entity,
                        )
                        .expect("the honest patch preserves the carrier shape");
                        assert!(host.update_entity(patched));
                    }
                    CustomGripResponse::default()
                }
                CustomGripPhase::Cancel => CustomGripResponse::default(),
            }
        });
    }
    let i = tab(&app);
    let candidate = crate::plugin::grip_points::GripCandidate {
        plugin: plugin.to_string(),
        grip_id: 7,
        anchor: Some(handles[1]),
        point: glam::DVec3::new(300.0, 8.0, 0.0),
        axis: None,
        shape: crate::scene::model::object::GripShape::Square,
    };
    assert_eq!(candidate.grip_id, 7);
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    assert!(events.borrow().len() == 1, "only the Begin phase crossed");
    assert!(
        app.plugin_grip_descriptor_drag.is_some(),
        "the serviceable declaration engages the descriptor mode"
    );

    // THE DRAG: the crest's end moves to (300, 11); the section re-derives
    // per its own record-model laws (the ramp's end value takes 0.09) and
    // the tail re-bases to the sampled joint. NO Move frame crosses the
    // IPC.
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(300.0, 11.0, 0.0));
    assert_eq!(events.borrow().len(), 1, "no Move frame in the mode");
    assert_eq!(
        undo_entry_count(&app),
        undo_before,
        "the per-move re-derivation records zero history"
    );
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    let EntityType::Spline(spline) = host
        .document()
        .get_entity(handles[1])
        .expect("the crest lives")
    else {
        panic!("the crest's carrier keeps its kind");
    };
    assert!(
        (spline.control_points[2].y - 11.0).abs() < 1e-9,
        "the crest's own crest moved to 11.0 mid-drag"
    );
    let EntityType::Line(tail) = host
        .document()
        .get_entity(handles[2])
        .expect("the tail lives")
    else {
        panic!("the tail keeps its kind");
    };
    assert!(
        (tail.end.y - (11.0 + 0.09 * 80.0)).abs() < 1e-9,
        "the tail follows the re-based joint"
    );
    drop(host);
    // The identity stamps and the designation ride untouched mid-drag
    // (the stamp read goes through its own host session).
    for handle in &handles {
        let mut session = crate::app::plugin_host::HostSession::new(&mut app, i);
        assert!(session.read_record(*handle, "PLUGIN_ALIGN_DISPLAY").is_some());
    }
    for handle in &handles {
        assert!(app.tabs[i].scene.is_reference_only(*handle));
    }

    // THE SETTLE: the plugin's ONE write window (the record row first, the
    // re-stage after) â€” one labeled undo step; the restore had put the
    // pre-drag display back BEFORE the window opened.
    let _ = app.settle_plugin_grip_drag();
    assert_eq!(events.borrow().len(), 2, "the Settle phase crossed");
    assert_eq!(
        undo_entry_count(&app),
        undo_before + 2,
        "the window totals TWO steps: the record row's own + the re-stage's push_undo"
    );
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("PLUGIN: grip edit"),
        "the last step is the re-stage's own label"
    );
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        let EntityType::Spline(spline) = host
            .document()
            .get_entity(handles[1])
            .expect("the crest lives")
        else {
            panic!("the carrier keeps its kind over the settle");
        };
        assert!(
            (spline.control_points[2].y - 11.0).abs() < 1e-9,
            "the re-stage re-applied the settled design"
        );
        assert!(host.read_record(handles[0], "PLUGIN_ALIGN_DISPLAY").is_some());
    }
    // Two undos round-trip the original display set (the re-stage's step,
    // then the record row's step).
    let _ = app.update(Message::Undo);
    let _ = app.update(Message::Undo);
    assert_eq!(undo_entry_count(&app), undo_before);
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        let EntityType::Spline(spline) = host
            .document()
            .get_entity(handles[1])
            .expect("the crest lives")
        else {
            panic!("the carrier keeps its kind over the undo");
        };
        assert!(
            (spline.control_points[2].y - 8.0).abs() < 1e-9,
            "the original crest is back after the one undo step"
        );
    }
    grip_points::remove_in_process_sink(plugin);
}

#[test]
fn descriptor_refusals_keep_the_streaming_path() {
    // A descriptor the host cannot serve (a plan chain has no END re-anchor
    // in the record model's plan laws) refuses the MODE with one headline;
    // the v12 streaming semantics stay exactly as before: Move frames flow,
    // the preview/live channels apply, the settle composes as before.
    let mut app = app();
    let plugin = "PLUGIN";
    let i = tab(&app);
    // A sink first: the class register's own aliveness gate treats an in-process test sink as the loaded+alive marker (grip_points); the full handler installed below replaces this placeholder.
    crate::plugin::grip_points::install_in_process_sink(plugin, |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    host.register_plugin_class(
        plugin,
        &ocs_plugin_api::host::PluginClassDeclaration {
            class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
            schema_version: 4,
            grip_schema: vec![ocs_plugin_api::host::PluginGripSchemaEntry {
                grip_id: 7,
                param_name: "peak_gradient".to_string(),
                drives: Some(ocs_plugin_api::host::PluginGripAnchor {
                    segment: 1,
                    at_start: true,
                    anchor: ocs_plugin_api::host::AnchorLaw::EndStationToXY,
                }),
            }],
            parameters: vec![],
        },
    )
    .expect("the class registers");
    let handles = host
        .stage_plugin_class_instance(
            plugin,
            &ocs_plugin_api::host::PluginClassInstance {
                class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
                schema_version: 4,
                design: ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                    crest_profile_chain(),
                ),
                tolerance: 1e-3,
                layer: "PLUGIN_PLAN".to_string(),
                design_record: "PLUGIN_ALIGN_PLAN".to_string(),
            },
        )
        .expect("the plan instance stages");
    drop(host);
    let undo_before = undo_entry_count(&app);
    let events: Rc<RefCell<Vec<CustomGripEvent>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let events2 = Rc::clone(&events);
        let begin2 = Rc::new(RefCell::new(Some((
                        ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                            crest_profile_chain(),
                        ),
            1e-3,
        ))));
        grip_points::install_in_process_sink(plugin, move |_host, event| {
            events2.borrow_mut().push(event.clone());
            match event.phase {
                CustomGripPhase::Begin => {
                    if let Some((chain, tolerance)) = begin2.borrow_mut().take() {
                        CustomGripResponse {
                            preview: Vec::new(),
                            live_entities: Vec::new(),
                            descriptor: Some(chain),
                            descriptor_tolerance: Some(tolerance),
                        }
                    } else {
                        CustomGripResponse::default()
                    }
                }
                _ => CustomGripResponse::default(),
            }
        });
    }
    // The grip anchors a staged carrier; the class stamp resolves.
    let i = tab(&app);
    let grip_point = [300.0, 8.0, 0.0];
    app.tabs[i]
        .plugin_grips
        .declare(
            plugin,
            vec![PluginGrip {
                grip_id: 7,
                anchor: Some(handles[1]),
                point: grip_point,
                axis: None,
                shape: PluginGripShape::Square,
            }],
        )
        .expect("the grip declares");
    let candidate = crate::plugin::grip_points::GripCandidate {
        plugin: plugin.to_string(),
        grip_id: 7,
        anchor: Some(handles[1]),
        point: glam::DVec3::new(300.0, 8.0, 0.0),
        axis: None,
        shape: crate::scene::model::object::GripShape::Square,
    };
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    assert!(
        app.plugin_grip_descriptor_drag.is_none(),
        "the plan drive refuses the mode"
    );
    let _ = crate::plugin::drain_errors();
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(300.0, 11.0, 0.0));
    assert_eq!(
        events.borrow().len(),
        2,
        "the Move frame still crosses: the streaming path is untouched"
    );
    let _ = app.settle_plugin_grip_drag();
    assert_eq!(events.borrow().len(), 3, "the Settle phase crossed");
    assert_eq!(
        undo_entry_count(&app),
        undo_before,
        "the streaming drag records nothing by itself"
    );
    grip_points::remove_in_process_sink(plugin);
}

#[test]
fn descriptor_moves_refused_by_the_model_keep_the_last_good_design() {
    // In the engaged mode a move the record model refuses (a backward run)
    // keeps the last good design on the screen, notes the constraint ONCE,
    // and the drag continues: the next legal move re-derives fully. The
    // settle without writes stays zero steps.
    let mut app = app();
    let plugin = "PLUGIN";
    let i = tab(&app);
    // A sink first: the class register's own aliveness gate treats an in-process test sink as the loaded+alive marker (grip_points); the full handler installed below replaces this placeholder.
    crate::plugin::grip_points::install_in_process_sink(plugin, |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    host.register_plugin_class(
        plugin,
        &ocs_plugin_api::host::PluginClassDeclaration {
            class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
            schema_version: 4,
            grip_schema: vec![ocs_plugin_api::host::PluginGripSchemaEntry {
                grip_id: 7,
                param_name: "peak_gradient".to_string(),
                drives: Some(ocs_plugin_api::host::PluginGripAnchor {
                    segment: 1,
                    at_start: false,
                    anchor: ocs_plugin_api::host::AnchorLaw::EndStationToXY,
                }),
            }],
            parameters: vec![],
        },
    )
    .expect("the class registers");
    let handles = host
        .stage_plugin_class_instance(
            plugin,
            &ocs_plugin_api::host::PluginClassInstance {
                class_id: "PLUGIN_ALIGN_DISPLAY".to_string(),
                schema_version: 4,
                design: ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                    crest_profile_chain(),
                ),
                tolerance: 1e-3,
                layer: "PLUGIN_PROFILE".to_string(),
                design_record: "PLUGIN_ALIGN_DISPLAY".to_string(),
            },
        )
        .expect("the crest stages");
    drop(host);
    let events: Rc<RefCell<Vec<CustomGripEvent>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let events2 = Rc::clone(&events);
        let begin2 = Rc::new(RefCell::new(Some((
            ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(
                crest_profile_chain(),
            ),
            1e-3,
        ))));
        grip_points::install_in_process_sink(plugin, move |_host, event| {
            events2.borrow_mut().push(event.clone());
            match event.phase {
                CustomGripPhase::Begin => {
                    if let Some((chain, tolerance)) = begin2.borrow_mut().take() {
                        CustomGripResponse {
                            preview: Vec::new(),
                            live_entities: Vec::new(),
                            descriptor: Some(chain),
                            descriptor_tolerance: Some(tolerance),
                        }
                    } else {
                        CustomGripResponse::default()
                    }
                }
                _ => CustomGripResponse::default(),
            }
        });
    }
    let i = tab(&app);
    app.tabs[i]
        .plugin_grips
        .declare(
            plugin,
            vec![PluginGrip {
                grip_id: 7,
                anchor: Some(handles[1]),
                point: [300.0, 8.0, 0.0],
                axis: None,
                shape: PluginGripShape::Square,
            }],
        )
        .expect("the grip declares");
    let candidate = crate::plugin::grip_points::GripCandidate {
        plugin: plugin.to_string(),
        grip_id: 7,
        anchor: Some(handles[1]),
        point: glam::DVec3::new(300.0, 8.0, 0.0),
        axis: None,
        shape: crate::scene::model::object::GripShape::Square,
    };
    let _ = app.begin_plugin_grip_drag(i, &candidate);
    assert!(app.plugin_grip_descriptor_drag.is_some());
    // The illegal move: a backward run (station 90 behind the crest's own
    // start at 100).
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(90.0, 30.0, 0.0));
    assert_eq!(events.borrow().len(), 1, "the move stayed local");
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        let EntityType::Spline(spline) = host
            .document()
            .get_entity(handles[1])
            .expect("the crest lives")
        else {
            panic!("the carrier keeps its kind");
        };
        assert!(
            (spline.control_points[2].y - 8.0).abs() < 1e-9,
            "the refused move kept the last good design"
        );
    }
    let errors = crate::plugin::drain_errors();
    assert!(
        errors
            .iter()
            .any(|note| note.contains("leaves the section's support")
                || note.contains("the last good design stays")),
        "the first refusal surfaced one headline: {errors:?}"
    );
    // The legal move after the refusal re-derives fully (the drag lives on).
    let _ = app.move_plugin_grip_drag(i, glam::DVec3::new(300.0, 11.0, 0.0));
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    let EntityType::Spline(spline) = host
        .document()
        .get_entity(handles[1])
        .expect("the crest lives")
    else {
        panic!("the carrier keeps its kind");
    };
    assert!(
        (spline.control_points[2].y - 11.0).abs() < 1e-9,
        "the next legal move re-derived the design"
    );
    drop(host);
    // The settle writes nothing (the sink's settle writes off): zero steps.
    let score_before = undo_entry_count(&app);
    let _ = app.settle_plugin_grip_drag();
    assert_eq!(events.borrow().len(), 2, "the Settle phase crossed");
    assert_eq!(
        undo_entry_count(&app),
        score_before,
        "a write-free settle composes zero steps"
    );
    grip_points::remove_in_process_sink(plugin);
}
