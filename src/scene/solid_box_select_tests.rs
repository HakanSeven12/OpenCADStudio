//! A selection box over a drawing of many solids looks into the solids at its
//! edges only: one that lies clear inside or clear outside the box is told
//! from a box around its vertices.

use super::Scene;
use crate::scene::pick::hit_test::{EVERY_VERTEX, MESH_VERTS_PROJECTED};
use crate::scene::view::camera::Projection;
use document::entities::{AcisData, Insert, Solid3D};
use document::types::Vector3;
use document::{EntityType, Handle};
use iced::{Point, Rectangle, Size};

const COLUMNS: usize = 20;
const ROWS: usize = 20;
/// Distance between the references, whose solid is a sphere of radius 1.
const PITCH: f64 = 4.0;

fn sphere() -> EntityType {
    let body = crate::scene::model::solid_model::sphere_solid([0.0; 3], 1.0).expect("sphere");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body).expect("sat");
    let mut solid = Solid3D::new();
    solid.acis_data = AcisData::from_sat(&sat.to_sat_string());
    EntityType::Solid3D(solid)
}

struct Fixture {
    scene: Scene,
    /// The references, row by row.
    references: Vec<Handle>,
    /// Vertices of the mesh of the block's solid.
    vertices: usize,
    bounds: Rectangle,
}

/// A grid of references of one block that holds a sphere, seen from above.
fn references_of_a_sphere() -> Fixture {
    let mut scene = Scene::new();
    scene.define_block_raw("SPHERE", Vector3::ZERO, vec![sphere()]);
    let references = (0..COLUMNS * ROWS)
        .map(|index| {
            scene.add_entity(EntityType::Insert(Insert::new(
                "SPHERE",
                Vector3::new(
                    (index % COLUMNS) as f64 * PITCH,
                    (index / COLUMNS) as f64 * PITCH,
                    0.0,
                ),
            )))
        })
        .collect();
    scene.populate_meshes_from_document();
    let vertices = scene
        .block_meshes
        .values()
        .next()
        .and_then(|set| set.geometry_lods().first())
        .map(|mesh| mesh.verts.len())
        .expect("the block's solid has a mesh");
    scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    scene.fit_all();
    Fixture {
        scene,
        references,
        vertices,
        bounds: Rectangle::with_size(Size::new(800.0, 600.0)),
    }
}

impl Fixture {
    fn screen(&self, x: f64, y: f64) -> Point {
        let point = self
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(x, y, 0.0), self.bounds)
            .expect("a point in front of the eye");
        Point::new(point.x, point.y)
    }

    /// The references a box between two model points takes, in handle order,
    /// and the mesh vertices it projected to tell.
    fn boxed(&self, from: (f64, f64), to: (f64, f64), crossing: bool) -> (Vec<Handle>, usize) {
        let (view_rot, eye) = {
            let camera = self.scene.camera.borrow();
            (camera.view_proj_rte(self.bounds), camera.eye())
        };
        MESH_VERTS_PROJECTED.with(|count| count.set(0));
        let mut picked = self.scene.block_mesh_box_hit(
            self.screen(from.0, from.1),
            self.screen(to.0, to.1),
            crossing,
            view_rot,
            eye,
            self.bounds,
            None,
        );
        picked.sort_unstable_by_key(|handle| handle.value());
        (picked, MESH_VERTS_PROJECTED.with(std::cell::Cell::get))
    }

    fn in_handle_order(&self, mut references: Vec<Handle>) -> Vec<Handle> {
        references.sort_unstable_by_key(|handle| handle.value());
        references
    }
}

/// A crossing box inside one sphere takes that reference. The other 399 lie
/// clear of the box, so their vertices are not what tells.
#[test]
fn a_box_over_one_reference_looks_into_the_solids_at_its_edges_only() {
    let fixture = references_of_a_sphere();
    let (column, row) = (7, 5);
    let (x, y) = (column as f64 * PITCH, row as f64 * PITCH);

    let (picked, projected) = fixture.boxed((x - 0.5, y - 0.5), (x + 0.5, y + 0.5), true);

    assert_eq!(picked, vec![fixture.references[row * COLUMNS + column]]);
    assert!(
        projected <= 4 * fixture.vertices,
        "{projected} mesh vertices projected for a box over one of {} references, {} vertices each",
        fixture.references.len(),
        fixture.vertices,
    );
}

/// A window around every reference takes them all, and each lies clear
/// inside it.
#[test]
fn a_window_around_every_reference_takes_them_without_looking_into_them() {
    let fixture = references_of_a_sphere();
    let far = (
        (COLUMNS - 1) as f64 * PITCH + 1.5,
        (ROWS - 1) as f64 * PITCH + 1.5,
    );

    let (picked, projected) = fixture.boxed((-1.5, -1.5), far, false);

    assert_eq!(picked, fixture.in_handle_order(fixture.references.clone()));
    assert!(
        projected <= 4 * fixture.vertices,
        "{projected} mesh vertices projected for a window around {} references, {} vertices each",
        fixture.references.len(),
        fixture.vertices,
    );
}

fn solid(body: &kernel::brep::Body) -> EntityType {
    let sat = crate::scene::convert::acis_export::solid_to_sat(body).expect("sat");
    let mut solid = Solid3D::new();
    solid.acis_data = AcisData::from_sat(&sat.to_sat_string());
    EntityType::Solid3D(solid)
}

/// References of a pyramid and of a long bar, turned and stretched every
/// which way and at several heights, with solids of their own among them.
/// The box around a pyramid reaches well past it, and the one around a
/// turned bar past the bar.
fn solids_every_which_way() -> Scene {
    use crate::scene::model::solid_model::{box_solid, pyramid_solid};
    let mut scene = Scene::new();
    scene.define_block_raw(
        "PYRAMID",
        Vector3::ZERO,
        vec![solid(&pyramid_solid([0.0; 3], 1.0, 2.0, 5).expect("pyramid"))],
    );
    scene.define_block_raw(
        "BAR",
        Vector3::ZERO,
        vec![solid(&box_solid([0.0; 3], 6.0, 1.0, 1.0).expect("bar"))],
    );
    for index in 0..120usize {
        let (column, row) = (index % 12, index / 12);
        let mut insert = Insert::new(
            if index % 3 == 0 { "BAR" } else { "PYRAMID" },
            Vector3::new(
                column as f64 * 5.0,
                row as f64 * 5.0,
                (index % 5) as f64 * 1.5,
            ),
        )
        .with_scale(
            0.5 + (index % 4) as f64 * 0.5,
            0.75 + (index % 3) as f64 * 0.5,
            1.0 + (index % 2) as f64,
        );
        insert.rotation = index as f64 * 0.37;
        scene.add_entity(EntityType::Insert(insert));
    }
    for index in 0..8usize {
        let centre = [index as f64 * 8.0 - 2.0, -6.0 + (index % 3) as f64, 2.0];
        let body = if index % 2 == 0 {
            pyramid_solid(centre, 1.5, 3.0, 6)
        } else {
            box_solid(centre, 3.0, 2.0, 4.0)
        }
        .expect("a solid of its own");
        scene.add_entity(solid(&body));
    }
    scene.populate_meshes_from_document();
    assert_eq!(scene.block_meshes.len(), 2, "a mesh for each block's solid");
    assert_eq!(scene.meshes.len(), 8, "a mesh for each solid of its own");
    scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    scene.fit_all();
    scene
}

fn every_vertex<T>(run: impl FnOnce() -> T) -> T {
    EVERY_VERTEX.with(|flag| flag.set(true));
    let result = run();
    EVERY_VERTEX.with(|flag| flag.set(false));
    result
}

/// The solids of their own and the references a box takes, each in handle
/// order.
fn boxed(scene: &Scene, a: Point, b: Point, crossing: bool) -> (Vec<u64>, Vec<u64>) {
    let bounds = Rectangle::with_size(Size::new(800.0, 600.0));
    let (view_rot, eye) = {
        let camera = scene.camera.borrow();
        (camera.view_proj_rte(bounds), camera.eye())
    };
    let values = |handles: Vec<Handle>| {
        let mut values: Vec<u64> = handles.iter().map(|handle| handle.value()).collect();
        values.sort_unstable();
        values
    };
    (
        values(scene.mesh_box_hit(a, b, crossing, view_rot, eye, bounds, None)),
        values(scene.block_mesh_box_hit(a, b, crossing, view_rot, eye, bounds, None)),
    )
}

/// The same for a lasso.
fn lassoed(scene: &Scene, poly: &[Point], crossing: bool) -> (Vec<u64>, Vec<u64>) {
    let bounds = Rectangle::with_size(Size::new(800.0, 600.0));
    let (view_rot, eye) = {
        let camera = scene.camera.borrow();
        (camera.view_proj_rte(bounds), camera.eye())
    };
    let values = |handles: Vec<Handle>| {
        let mut values: Vec<u64> = handles.iter().map(|handle| handle.value()).collect();
        values.sort_unstable();
        values
    };
    (
        values(scene.mesh_poly_hit(poly, crossing, view_rot, eye, bounds, None)),
        values(scene.block_mesh_poly_hit(poly, crossing, view_rot, eye, bounds, None)),
    )
}

/// In every view, for boxes and lassos of every size, window and crossing,
/// the selection is what looking at every vertex of every solid gives.
#[test]
fn a_selection_takes_what_looking_at_every_vertex_takes() {
    let mut scene = solids_every_which_way();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |below: u32| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as u32) % below
    };
    let mut taken = 0usize;
    let mut told = 0usize;
    let mut looked_at = 0usize;
    for view in ["from above", "turned", "in perspective", "from close by"] {
        match view {
            "turned" => scene.camera.borrow_mut().orbit(140.0, -90.0, None),
            "in perspective" => scene
                .camera
                .borrow_mut()
                .set_projection_preserving_frame(Projection::Perspective, 800.0 / 600.0),
            "from close by" => scene.camera.borrow_mut().zoom(6.0),
            _ => {}
        }
        for round in 0..40 {
            // Boxes from a few pixels across to wider than the view.
            let reach = [8, 40, 200, 900][round % 4];
            let a = Point::new(next(900) as f32 - 50.0, next(700) as f32 - 50.0);
            let b = Point::new(
                a.x + next(2 * reach) as f32 - reach as f32,
                a.y + next(2 * reach) as f32 - reach as f32,
            );
            let poly: Vec<Point> = (0..3 + next(4))
                .map(|_| {
                    Point::new(
                        a.x + next(2 * reach) as f32 - reach as f32,
                        a.y + next(2 * reach) as f32 - reach as f32,
                    )
                })
                .collect();
            for crossing in [false, true] {
                MESH_VERTS_PROJECTED.with(|count| count.set(0));
                let by_box = boxed(&scene, a, b, crossing);
                let by_lasso = lassoed(&scene, &poly, crossing);
                let projected = MESH_VERTS_PROJECTED.with(std::cell::Cell::get);
                MESH_VERTS_PROJECTED.with(|count| count.set(0));
                let (every_box, every_lasso) = every_vertex(|| {
                    (
                        boxed(&scene, a, b, crossing),
                        lassoed(&scene, &poly, crossing),
                    )
                });
                let every = MESH_VERTS_PROJECTED.with(std::cell::Cell::get);
                assert_eq!(
                    by_box, every_box,
                    "{view}, round {round}: box {a:?} {b:?}, crossing {crossing}"
                );
                assert_eq!(
                    by_lasso, every_lasso,
                    "{view}, round {round}: lasso {poly:?}, crossing {crossing}"
                );
                assert!(projected <= every, "{view}, round {round}");
                taken += by_box.0.len() + by_box.1.len() + by_lasso.0.len() + by_lasso.1.len();
                told += every - projected;
                looked_at += projected;
            }
        }
    }
    assert!(taken > 0, "the rounds must reach selections that take solids");
    assert!(told > 0, "the rounds must reach solids their box tells");
    assert!(
        looked_at > 0,
        "the rounds must reach solids at the edge of a selection"
    );
}

/// A solid whose box comes up to an edge of the selection box has its
/// vertices looked at: the box around a sphere reaches where the sphere does
/// not, and only the vertices tell that the selection passes it by.
#[test]
fn a_solid_at_the_edge_of_the_box_has_its_vertices_looked_at() {
    let fixture = references_of_a_sphere();
    let (column, row) = (7, 5);
    let (x, y) = (column as f64 * PITCH, row as f64 * PITCH);

    // Over the corner of the box around the sphere, outside the sphere.
    let (picked, projected) = fixture.boxed((x + 0.8, y + 0.8), (x + 1.4, y + 1.4), true);

    assert_eq!(picked, Vec::<Handle>::new(), "the corner is outside the sphere");
    assert_eq!(
        projected, fixture.vertices,
        "the one solid at the edge is looked at, and no other"
    );
}
