//! OpenCADStudio paper-space per-viewport hidden-handle benchmark.
//!
//! `Scene::build_viewports` runs once per frame. For every content viewport on
//! a paper sheet it asks `Scene::viewport_hidden_handles` which handles that
//! viewport hides: its VP-frozen layers plus, in Inventor-style drawings, every
//! model-space reference (INSERT with a view-representation binding) attached
//! to a *different* viewport. That lookup walked the whole entity list on every
//! call, so a sheet with several content viewports paid one full document scan
//! per viewport per frame.
//!
//! Fixture: one paper sheet with `VIEWPORTS` content viewports, `ENTITIES`
//! model-space lines on a frozen layer (so the hidden-handle lookup is the only
//! per-entity work the frame does) and `VIEWPORTS` view-representation INSERTS,
//! each bound to one content viewport. `ACIDBLOCKREFERENCE` is declared in the
//! class table, which is what gates the reference scan in production.
//!
//! Sink: the primitive is built every call; after timing, the sheet must still
//! expose exactly the fixture's content viewports through `viewport_list` and
//! the entity count must be unchanged. The hidden-set contents themselves are
//! covered by a crate unit test, since the lookup is not public.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench paper_viewport_hidden_handles`

use std::hint::black_box;
use std::time::Instant;

use codec::classes::DxfClass;
use codec::entities::{EntityType, Insert, Line, Viewport};
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::Scene;

const ENTITIES: usize = 100_000;
const VIEWPORTS: usize = 8;
const CANVAS: (f32, f32) = (1920.0, 1080.0);
const RUNS: usize = 3;
const ITERATIONS: usize = 10;
const WARMUP: usize = 3;

fn fixture() -> (Scene, Handle) {
    let mut scene = Scene::new();

    // A frozen layer keeps the 100k model lines out of every viewport's wire
    // set, so a frame's cost is the hidden-handle lookup, not tessellation.
    let mut frozen = codec::tables::layer::Layer::new("HIDDEN");
    frozen.flags.frozen = true;
    scene.document.layers.add_or_replace(frozen);

    // The sheet and its content viewports.
    scene
        .document
        .add_layout("Sheet")
        .expect("the layout name is unique");
    scene.set_current_layout("Sheet".to_string());
    let ((x0, y0), (x1, y1)) = scene.paper_limits().expect("the sheet has paper limits");
    let (w, h) = (x1 - x0, y1 - y0);
    let mut viewports = Vec::with_capacity(VIEWPORTS);
    for index in 0..VIEWPORTS {
        let mut vp = Viewport::new();
        vp.id = index as i16 + 2;
        vp.width = w * 0.4;
        vp.height = h * 0.2;
        vp.view_height = vp.height * 0.8;
        // A 4x2 grid well inside the sheet, so every viewport is drawn.
        let column = index % 2;
        let row = index / 2;
        vp.center = Vector3::new(
            x0 + w * (0.25 + 0.5 * column as f64),
            y0 + h * (0.125 + 0.25 * row as f64),
            0.0,
        );
        viewports.push(scene.add_entity(EntityType::Viewport(Box::new(vp))));
    }
    let content: Vec<Handle> = {
        let sheet = scene.current_layout_block_handle_pub();
        scene
            .document
            .block_records
            .iter()
            .find(|block| block.handle == sheet)
            .expect("the sheet block record")
            .entity_handles
            .clone()
    };
    // `add_layout` also gives the sheet its own viewport entity.
    assert!(content.len() > VIEWPORTS, "every content viewport is registered");

    // Model space: the many entities, plus one view-representation INSERT per
    // viewport, each bound to a different viewport than the ones it hides in.
    scene.set_current_layout("Model".to_string());
    let mut lines = Vec::with_capacity(ENTITIES);
    for index in 0..ENTITIES {
        let mut line = Line::from_points(
            Vector3::new(0.0, index as f64, 0.0),
            Vector3::new(100.0, index as f64, 0.0),
        );
        line.common.layer = "HIDDEN".to_string();
        lines.push(EntityType::Line(line));
    }
    scene.add_entities(lines);
    let mut references = Vec::with_capacity(VIEWPORTS);
    for (index, &vp) in viewports.iter().enumerate() {
        let mut insert = Insert::new("ViewRep", Vector3::new(index as f64 * 10.0, 0.0, 0.0));
        insert.view_rep_handle = Some(vp);
        references.push(scene.add_entity(EntityType::Insert(insert)));
    }
    scene.document.classes.add_or_update(DxfClass {
        dxf_name: "ACIDBLOCKREFERENCE".to_string(),
        cpp_class_name: "AcDbBlockRepresentationContext".to_string(),
        application_name: "ObjectDBX Classes".to_string(),
        proxy_flags: codec::classes::ProxyFlags(1153),
        instance_count: 0,
        was_zombie: false,
        is_an_entity: false,
        class_number: 0,
        item_class_id: 0x1F3,
        dwg_version: 25,
        maintenance_version: 0,
        unknown1: 0,
        unknown2: 0,
    });
    scene.set_current_layout("Sheet".to_string());

    // Frame the sheet. The paper camera decides which content viewports land
    // on-canvas, and an unfitted camera keeps whatever zoom the model extents
    // set — which pushes all but one viewport off screen and hides the cost.
    {
        let aspect = CANVAS.0 / CANVAS.1;
        let half_h = ((h * 0.5) as f32).max((w * 0.5) as f32 / aspect) * 1.05;
        let mut camera = scene.camera.borrow_mut();
        camera.target = glam::DVec3::new((x0 + x1) * 0.5, (y0 + y1) * 0.5, 0.0);
        camera.distance = (half_h / (camera.fov_y * 0.5).tan()).max(1e-3);
    }
    (scene, viewports[0])
}

fn main() {
    let (scene, first) = fixture();
    let entities = scene.document.entities().count();
    assert!(
        entities >= ENTITIES + VIEWPORTS + VIEWPORTS,
        "model lines + references + viewports"
    );
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: CANVAS.0,
        height: CANVAS.1,
    };
    let frame = |scene: &Scene| {
        scene.build_viewports(
            bounds,
            codec::entities::ViewportRenderMode::Wireframe2D,
            None,
            false,
            false,
            [1.0, 1.0, 1.0, 1.0],
        )
    };
    for _ in 0..WARMUP {
        black_box(frame(&scene));
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(frame(&scene));
        }
        let per_frame = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_frame);
        println!("paper_viewport_hidden_handles: run {run} {ITERATIONS} frames: {per_frame:.3} ms/frame");
    }

    // The sheet still resolves its four content viewports, and each still
    // owns the empty VP-frozen list the fixture gave it.
    let listed = scene.viewport_list();
    assert_eq!(listed.len(), VIEWPORTS, "the sheet keeps its content viewports");
    assert!(listed.iter().all(|vp| vp.frozen.is_empty()));
    assert!(listed.iter().any(|vp| vp.handle == first));
    assert_eq!(
        scene.document.entities().count(),
        entities,
        "a frame must not add or drop entities"
    );
    println!(
        "paper_viewport_hidden_handles: {ENTITIES} model entities, {VIEWPORTS} content viewports, \
         {VIEWPORTS} view references; best {best:.3} ms/frame"
    );
}
