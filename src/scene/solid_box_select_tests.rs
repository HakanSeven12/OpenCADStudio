//! A selection box over a drawing of many solids looks into the solids at its
//! edges only: one that lies clear inside or clear outside the box is told
//! from a box around its vertices.

use super::Scene;
use crate::scene::pick::hit_test::MESH_VERTS_PROJECTED;
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
