//! The edge outline that highlights a solid is built for the size its edges
//! show at: an edge line that covers a few pixels is outlined with the points
//! that size can show, a longer one with every piece.

use super::Scene;
use crate::scene::view::camera::Projection;
use crate::scene::WireModel;
use document::entities::{AcisData, Insert, Solid3D, ViewportRenderMode};
use document::types::Vector3;
use document::{EntityType, Handle};

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 600.0;

fn solid(body: ocs_solid::model::Brep) -> EntityType {
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body).expect("sat");
    let mut solid = Solid3D::new();
    solid.acis_data = AcisData::from_sat(&sat.to_sat_string());
    EntityType::Solid3D(solid)
}

fn cylinder() -> EntityType {
    solid(crate::scene::model::solid_model::cylinder_solid([0.0; 3], 1.0, 3.0).expect("cylinder"))
}

fn box_solid() -> EntityType {
    solid(crate::scene::model::solid_model::box_solid([0.0; 3], 2.0, 1.0, 1.5).expect("box"))
}

/// Six references of a block that holds `body`, selected: three that only
/// move it, one turned by 37 degrees, one twice the size, one mirrored.
fn selected_references(body: EntityType) -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw("PART", Vector3::ZERO, vec![body]);
    let references: Vec<Handle> = (0..6)
        .map(|index| {
            let mut insert = Insert::new("PART", Vector3::new(index as f64 * 10.0, 5.0, 0.0));
            match index {
                3 => insert.rotation = 37f64.to_radians(),
                4 => insert.set_scale_unchecked(2.0, 2.0, 2.0),
                5 => insert.set_scale_unchecked(-1.0, 1.0, 1.0),
                _ => {}
            }
            scene.add_entity(EntityType::Insert(insert))
        })
        .collect();
    scene.populate_meshes_from_document();
    scene.selection.borrow_mut().view.vp_size = (WIDTH, HEIGHT);
    scene.fit_all();
    scene.select_entities(&references);
    // The view is at rest: fitting it moved the camera a moment ago.
    let _ = scene.navigating_lod();
    scene.nav_changed_at.set(None);
    (scene, references)
}

/// Sets the view so that one unit of the drawing covers `pixels`.
fn show_at(scene: &Scene, pixels: f32) {
    let mut camera = scene.camera.borrow_mut();
    assert_eq!(camera.projection, Projection::Orthographic);
    let half_height = HEIGHT * 0.5 / pixels;
    camera.distance = half_height / (camera.fov_y * 0.5).tan();
    assert!((camera.ortho_size() - half_height).abs() < half_height * 1e-4);
}

fn outlines(scene: &Scene) -> Vec<WireModel> {
    let instance = scene
        .active_viewports(WIDTH, HEIGHT, ViewportRenderMode::Wireframe2D)
        .into_iter()
        .next()
        .expect("the model view");
    scene
        .annotation_context_highlight_wires(&instance, true)
        .iter()
        .filter(|wire| wire.name.starts_with("mesh-edge:"))
        .cloned()
        .collect()
}

fn handle_of(wire: &WireModel) -> Handle {
    Handle::new(wire.name["mesh-edge:".len()..].parse().expect("a handle"))
}

/// The segments an outline draws, in world coordinates: its own points, or
/// the shape it moves.
fn segments(wire: &WireModel) -> Vec<[[f64; 3]; 2]> {
    let (shape, delta) = match &wire.stub {
        Some(stub) => (&stub.prototype[0], stub.delta),
        None => (wire, [0.0; 3]),
    };
    let points: Vec<Option<[f64; 3]>> = shape
        .points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let low = shape.points_low.get(index).unwrap_or([0.0; 3]);
            point[0].is_finite().then(|| {
                [
                    point[0] as f64 + low[0] as f64 + delta[0],
                    point[1] as f64 + low[1] as f64 + delta[1],
                    point[2] as f64 + low[2] as f64 + delta[2],
                ]
            })
        })
        .collect();
    points
        .windows(2)
        .filter_map(|pair| Some([pair[0]?, pair[1]?]))
        .collect()
}

/// The edges of the solid `handle` places, in world coordinates.
fn placed_edges(scene: &Scene, handle: Handle) -> Vec<[[f64; 3]; 2]> {
    let meshes = scene.interaction_meshes_arc();
    let set = meshes
        .iter()
        .find(|set| set.entity_handle() == Some(handle))
        .expect("the reference's solid");
    let transform = set.instance_transform.expect("a placed solid");
    let (edges, edges_low) = set.geometry_edges();
    let placed: Vec<[f64; 3]> = edges
        .iter()
        .enumerate()
        .map(|(index, high)| {
            let low = edges_low.get(index).copied().unwrap_or([0.0; 3]);
            let world = transform.apply(Vector3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ));
            [world.x, world.y, world.z]
        })
        .collect();
    placed
        .chunks_exact(2)
        .map(|pair| [pair[0], pair[1]])
        .collect()
}

fn distance_to_segment(point: [f64; 3], segment: &[[f64; 3]; 2]) -> f64 {
    let [a, b] = *segment;
    let along: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
    let length: f64 = along.iter().map(|value| value * value).sum();
    let t = if length > 0.0 {
        ((0..3)
            .map(|axis| (point[axis] - a[axis]) * along[axis])
            .sum::<f64>()
            / length)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..3)
        .map(|axis| (point[axis] - a[axis] - along[axis] * t).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The farthest any end of `from` is from the segments of `to`.
fn farthest(from: &[[[f64; 3]; 2]], to: &[[[f64; 3]; 2]]) -> f64 {
    from.iter()
        .flatten()
        .map(|point| {
            to.iter()
                .map(|segment| distance_to_segment(*point, segment))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// A cylinder of radius 1, shown with its circles three pixels across and
/// then four hundred: the small ones are outlined with a fraction of the
/// segments.
#[test]
fn edges_that_show_small_are_outlined_with_fewer_segments() {
    let (scene, references) = selected_references(cylinder());
    show_at(&scene, 200.0);
    let large: usize = outlines(&scene)
        .iter()
        .map(|wire| segments(wire).len())
        .sum();
    let edges: usize = references
        .iter()
        .map(|handle| placed_edges(&scene, *handle).len())
        .sum();
    assert_eq!(
        large, edges,
        "a solid that shows large has every edge piece"
    );

    show_at(&scene, 1.0);
    let small: usize = outlines(&scene)
        .iter()
        .map(|wire| segments(wire).len())
        .sum();
    assert!(
        small * 3 <= large,
        "{small} segments for six cylinders a few pixels across, {large} for the same when large"
    );
    assert!(small >= references.len() * 3, "an outline is left: {small}");
}

/// What is left out moves nothing by more than half a pixel: every edge
/// point of each reference is that near to its outline, and every outline
/// point is on an edge. The references that only move, the one that turns,
/// the one that is twice the size and the mirrored one alike.
#[test]
fn a_thinned_outline_stays_within_half_a_pixel_of_the_edges() {
    for pixels in [0.5f32, 2.0, 7.0, 30.0] {
        let (scene, references) = selected_references(cylinder());
        show_at(&scene, pixels);
        let outlines = outlines(&scene);
        assert_eq!(outlines.len(), references.len(), "one outline each");
        let half_pixel = 0.5 / pixels as f64;
        for wire in &outlines {
            let handle = handle_of(wire);
            let outline = segments(wire);
            let edges = placed_edges(&scene, handle);
            assert!(!outline.is_empty(), "{handle:?} has no outline");
            let off = farthest(&edges, &outline);
            assert!(
                off <= half_pixel * 1.0001,
                "{handle:?} at {pixels} px a unit: an edge point is {off} from its outline, \
                 half a pixel is {half_pixel}"
            );
            let stray = farthest(&outline, &edges);
            assert!(
                stray <= 1e-6,
                "{handle:?}: an outline point is off the edges"
            );
        }
    }
}

/// The references that only move their block still share one shape, at the
/// size they show at.
#[test]
fn moved_references_share_the_thinned_shape() {
    let (scene, references) = selected_references(cylinder());
    show_at(&scene, 2.0);
    let outlines = outlines(&scene);
    let moved: Vec<&WireModel> = outlines
        .iter()
        .filter(|wire| references[..3].contains(&handle_of(wire)))
        .collect();
    assert_eq!(moved.len(), 3);
    let shapes: std::collections::HashSet<usize> = moved
        .iter()
        .map(|wire| {
            let stub = wire.stub.as_ref().expect("an outline that moves a shape");
            std::sync::Arc::as_ptr(&stub.prototype) as usize
        })
        .collect();
    assert_eq!(shapes.len(), 1, "the three move the same shape");
    assert!(moved.iter().all(|wire| wire.points.is_empty()));
}

/// The edges of one solid are taken one by one: a cone cut near its tip has
/// a base circle 120 px across, which keeps every piece, and a top circle
/// 1 px across, which is thinned.
#[test]
fn a_long_edge_keeps_every_piece_beside_a_small_one() {
    let frustum = solid(
        crate::scene::model::solid_model::cone_frustum_solid([0.0; 3], 100.0, 100.0, 1.0, 50.0)
            .expect("frustum"),
    );
    let (scene, references) = selected_references(frustum);
    let pixels = 0.6f64;
    show_at(&scene, pixels as f32);
    // The pieces of the base circle are several pixels long, those of the
    // top circle a fraction of one.
    let long = |segment: &[[f64; 3]; 2]| {
        (0..3)
            .map(|axis| (segment[0][axis] - segment[1][axis]).powi(2))
            .sum::<f64>()
            .sqrt()
            * pixels
            > 2.0
    };
    for wire in outlines(&scene) {
        let handle = handle_of(&wire);
        let outline = segments(&wire);
        let edges = placed_edges(&scene, handle);
        let long_edges: Vec<_> = edges.iter().filter(|edge| long(edge)).collect();
        assert!(
            long_edges.len() >= 12,
            "{handle:?}: the base circle has pieces"
        );
        for edge in &long_edges {
            assert!(
                // From either end: an outline does not say which way an
                // edge runs.
                outline.iter().any(|segment| {
                    [[0, 1], [1, 0]].iter().any(|ends| {
                        (0..2).all(|end| {
                            (0..3).all(|axis| {
                                (segment[ends[end]][axis] - edge[end][axis]).abs() < 1e-6
                            })
                        })
                    })
                }),
                "{handle:?}: a piece of the long edge is gone: {edge:?}"
            );
        }
        let short_edges = edges.len() - long_edges.len();
        let short_outline = outline.iter().filter(|segment| !long(segment)).count();
        assert!(short_edges >= 16, "{handle:?}: the top circle has pieces");
        assert!(
            short_outline * 2 <= short_edges && short_outline >= 3,
            "{handle:?}: {short_outline} segments for a top circle of {short_edges} pieces"
        );
    }
    assert_eq!(references.len(), 6);
}

/// A box has no curved edge: its twelve edges are all there at any size.
#[test]
fn a_box_keeps_every_edge_at_any_size() {
    for pixels in [0.05f32, 2.0, 400.0] {
        let (scene, references) = selected_references(box_solid());
        show_at(&scene, pixels);
        for wire in outlines(&scene) {
            let handle = handle_of(&wire);
            let outline = segments(&wire);
            let edges = placed_edges(&scene, handle);
            assert_eq!(
                outline.len(),
                edges.len(),
                "{handle:?} at {pixels} px a unit"
            );
            assert!(farthest(&edges, &outline) <= 1e-6);
        }
        assert_eq!(references.len(), 6);
    }
}

/// A perspective view shows a solid at more than one size: full outlines.
#[test]
fn a_perspective_view_outlines_in_full() {
    let (scene, references) = selected_references(cylinder());
    show_at(&scene, 2.0);
    scene.camera.borrow_mut().projection = Projection::Perspective;
    let drawn: usize = outlines(&scene)
        .iter()
        .map(|wire| segments(wire).len())
        .sum();
    let edges: usize = references
        .iter()
        .map(|handle| placed_edges(&scene, *handle).len())
        .sum();
    assert_eq!(drawn, edges);
}

/// The outline follows the view's scale in steps of two: within a step the
/// wires are the ones already built, across one they are built again.
#[test]
fn the_outline_is_built_again_only_when_the_scale_crosses_a_step() {
    let (scene, _) = selected_references(cylinder());
    let instance = |scene: &Scene| {
        scene
            .active_viewports(WIDTH, HEIGHT, ViewportRenderMode::Wireframe2D)
            .into_iter()
            .next()
            .expect("the model view")
    };
    show_at(&scene, 2.2);
    let first = scene.annotation_context_highlight_wires(&instance(&scene), true);
    // Panning and a zoom inside the step keep the wires.
    scene.camera.borrow_mut().target += glam::DVec3::new(3.0, -2.0, 0.0);
    show_at(&scene, 3.9);
    let same = scene.annotation_context_highlight_wires(&instance(&scene), true);
    assert!(std::sync::Arc::ptr_eq(&first, &same));
    // Twice as close is another step.
    show_at(&scene, 4.4);
    let closer = scene.annotation_context_highlight_wires(&instance(&scene), true);
    assert!(!std::sync::Arc::ptr_eq(&first, &closer));
    // And back: the first wires again.
    show_at(&scene, 2.2);
    let back = scene.annotation_context_highlight_wires(&instance(&scene), true);
    assert!(std::sync::Arc::ptr_eq(&first, &back));
}

/// A zoom crosses a step every few notches of the wheel. While the view is
/// being zoomed the outlines stay the ones it had; they are built for the
/// new scale once it rests.
#[test]
fn a_view_that_is_being_zoomed_keeps_its_outlines_until_it_rests() {
    let (mut scene, _) = selected_references(cylinder());
    let instance = |scene: &Scene| {
        scene
            .active_viewports(WIDTH, HEIGHT, ViewportRenderMode::Wireframe2D)
            .into_iter()
            .next()
            .expect("the model view")
    };
    show_at(&scene, 1.0);
    let rested = scene.annotation_context_highlight_wires(&instance(&scene), true);
    // The camera moves: five steps closer, in the middle of a zoom.
    scene.camera_generation += 1;
    assert!(scene.navigating_lod());
    // Still moving for as long as the test takes, whatever the machine does.
    scene.nav_changed_at.set(Some(
        iced::time::Instant::now() + std::time::Duration::from_secs(600),
    ));
    assert!(scene.navigating_lod());
    show_at(&scene, 30.0);
    let zooming = scene.annotation_context_highlight_wires(&instance(&scene), true);
    assert!(
        std::sync::Arc::ptr_eq(&rested, &zooming),
        "the outlines were built again in the middle of a zoom"
    );
    // The view rests: the outlines of its scale, which thins nothing here.
    scene.nav_changed_at.set(None);
    assert!(!scene.navigating_lod());
    let after = scene.annotation_context_highlight_wires(&instance(&scene), true);
    assert!(!std::sync::Arc::ptr_eq(&rested, &after));
    let count = |wires: &[WireModel]| -> usize {
        wires
            .iter()
            .filter(|wire| wire.name.starts_with("mesh-edge:"))
            .map(|wire| segments(wire).len())
            .sum()
    };
    assert!(count(&after) > count(&rested) * 2, "every piece again");
}
