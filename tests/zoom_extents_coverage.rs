//! ZOOM EXTENTS must frame every visible entity, the way commercial solutions
//! do — no heuristic allowed to decide that part of the drawing does not count.
//!
//! Two things used to stop it. A quartile filter dropped any wire whose
//! centroid sat far from the consensus of the others, so a detail parked away
//! from the main body was silently left outside the fit. And HATCH tessellates
//! to no wires at all, so a drawing whose outermost geometry is a fill had
//! nothing for the model-space walk to read.
#![cfg(not(target_arch = "wasm32"))]

use codec::entities::hatch::{BoundaryEdge, BoundaryPath, LineEdge};
use codec::entities::{Hatch, Line};
use codec::types::{Vector2, Vector3};
use codec::EntityType;
use OpenCADStudio::scene::Scene;

fn add_line(scene: &mut Scene, x1: f64, y1: f64, x2: f64, y2: f64) {
    let mut line = Line::new();
    line.start = Vector3::new(x1, y1, 0.0);
    line.end = Vector3::new(x2, y2, 0.0);
    scene.add_entity(EntityType::Line(line));
}

/// A solid square fill with corners (x, y) .. (x + size, y + size).
fn square_hatch(x: f64, y: f64, size: f64) -> Hatch {
    let mut path = BoundaryPath::new();
    for (sx, sy, ex, ey) in [
        (0.0, 0.0, size, 0.0),
        (size, 0.0, size, size),
        (size, size, 0.0, size),
        (0.0, size, 0.0, 0.0),
    ] {
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(x + sx, y + sy),
            end: Vector2::new(x + ex, y + ey),
        }));
    }
    let mut hatch = Hatch::new();
    hatch.paths.push(path);
    hatch
}

#[test]
fn fit_all_reaches_an_entity_parked_far_from_the_rest() {
    let mut scene = Scene::new();
    // A dense consensus cluster — more than the eight wires the old quartile
    // filter needed before it switched itself on.
    for i in 0..12 {
        let x = i as f64;
        add_line(&mut scene, x, 0.0, x, 10.0);
    }
    // …and one line far outside it. Nothing about it is invalid: it is simply
    // somewhere else, which is exactly what the filter could not tell apart
    // from parser junk.
    add_line(&mut scene, 50_000.0, 50_000.0, 50_010.0, 50_010.0);

    scene.fit_all();

    // The fit centres on the extents box, so the target alone says whether the
    // remote line counted: dropping it leaves the camera on the cluster at
    // about (5.5, 5) instead of halfway out to (25005, 25005).
    let cam = scene.camera.borrow();
    assert!(
        (cam.target.x - 25_005.0).abs() < 50.0 && (cam.target.y - 25_005.0).abs() < 50.0,
        "the fit must span cluster and remote line, target = {:?}",
        cam.target
    );
    assert!(
        cam.ortho_size() >= 25_005.0,
        "the view must be wide enough to hold both, ortho_size = {}",
        cam.ortho_size()
    );
}

#[test]
fn fit_all_frames_a_hatch_only_drawing() {
    let mut scene = Scene::new();
    scene.add_entity(EntityType::Hatch(square_hatch(100.0, 200.0, 40.0)));

    scene.fit_all();

    // Nothing else is in the drawing, so without the fill the camera never
    // moves and stays on the origin.
    let cam = scene.camera.borrow();
    assert!(
        (cam.target.x - 120.0).abs() < 1.0 && (cam.target.y - 220.0).abs() < 1.0,
        "a hatch is geometry: ZOOM EXTENTS must centre on it, target = {:?}",
        cam.target
    );
    assert!(
        cam.ortho_size() >= 20.0,
        "the view must hold the whole fill, ortho_size = {}",
        cam.ortho_size()
    );
}

#[test]
fn fit_all_keeps_a_hatch_outside_the_wires_in_view() {
    let mut scene = Scene::new();
    for i in 0..12 {
        let x = i as f64;
        add_line(&mut scene, x, 0.0, x, 10.0);
    }
    // The fill is the outermost thing in the drawing; the wires alone would
    // frame a 12 x 10 strip and leave it off screen.
    scene.add_entity(EntityType::Hatch(square_hatch(500.0, 500.0, 50.0)));

    scene.fit_all();

    // Wires alone centre near (5.5, 5); including the fill pulls the centre
    // out to about (275, 275).
    let cam = scene.camera.borrow();
    assert!(
        cam.target.x > 200.0 && cam.target.y > 200.0,
        "the fill must widen the fit past the wires, target = {:?}",
        cam.target
    );
    assert!(
        cam.ortho_size() >= 275.0,
        "the view must reach the far corner of the fill, ortho_size = {}",
        cam.ortho_size()
    );
}
