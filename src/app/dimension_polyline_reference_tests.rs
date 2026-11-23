//! An associative dimension on polyline vertices is stored the way the file
//! format names them: an edge (subentity 2) with a one-based marker and its
//! start (0) or end (1), so other programs re-evaluate it in place (#3502).
use super::OpenCADStudio;
use crate::scene::dimension_assoc::ReferenceStatus;
use document::{EntityType, Handle};

fn dimension(from: glam::DVec3, to: glam::DVec3) -> EntityType {
    crate::modules::annotate::linear_dim::linear_dimension_entity(
        from,
        to,
        (from + to) * 0.5 + glam::DVec3::new(0.0, -800.0, 0.0),
        (to - from).normalize(),
        None,
    )
}

#[test]
fn a_dimension_on_polyline_vertices_names_edges() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"PLINE 0,0 6000,0 6000,4500 "}"#);
    let i = app.active_tab;
    let polyline: Handle = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|entity| matches!(entity, EntityType::LwPolyline(_)))
        .map(|entity| entity.common().handle)
        .expect("the drawn polyline");
    let first = app.tabs[i].scene.add_entity(dimension(
        glam::DVec3::ZERO,
        glam::DVec3::new(6000.0, 0.0, 0.0),
    ));
    let last = app.tabs[i].scene.add_entity(dimension(
        glam::DVec3::new(6000.0, 0.0, 0.0),
        glam::DVec3::new(6000.0, 4500.0, 0.0),
    ));
    for dim in [first, last] {
        app.tabs[i]
            .scene
            .attach_dimension_association(dim, vec![Some(polyline), Some(polyline)]);
    }
    let stored = |dim: Handle| {
        let association = app.tabs[i]
            .scene
            .dimension_association(dim)
            .expect("an association");
        association.references[..2]
            .iter()
            .map(|references| {
                let reference = &references[0];
                (
                    reference.main_subent_type,
                    reference.main_gs_marker,
                    reference.osnap_distance,
                )
            })
            .collect::<Vec<_>>()
    };
    // Vertices 0 and 1 start edges 1 and 2; the open polyline's last vertex
    // ends edge 2.
    assert_eq!(stored(first), vec![(2, 1, 0.0), (2, 2, 0.0)]);
    assert_eq!(stored(last), vec![(2, 2, 0.0), (2, 2, 1.0)]);
    for dim in [first, last] {
        assert!(app.tabs[i]
            .scene
            .dimension_association_status(dim)
            .iter()
            .all(|(_, status)| *status == ReferenceStatus::Resolved));
    }
}
