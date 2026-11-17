//! `import` on a hand-written file: a tetrahedron and a disk.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // tests

use crate::map::{import, Limits};
use solidcore_record::step::model::{self, Bound, Curve, Surface};

/// A tetrahedron (0,0,0) (10,0,0) (0,10,0) (0,0,10), written by hand, in
/// centimetres (so every length maps x10), plus a disk face with a circular edge.
const TETRA: &str = include_str!("tetra_disk.stp");

#[test]
fn imports_hand_written_tetrahedron() {
    let m = import(TETRA.as_bytes(), &Limits::default()).unwrap();
    assert!(m.skipped.is_empty(), "{:?}", m.skipped);
    assert_eq!(m.bodies.len(), 2);
    let t = &m.bodies[0];
    assert_eq!(t.step_id, 101);
    assert_eq!(t.unit_mm, 10.0);
    assert!((t.uncertainty_mm.unwrap() - 1e-4).abs() < 1e-18);
    assert_eq!(
        (
            t.shells.len(),
            t.faces.len(),
            t.edges.len(),
            t.vertices.len()
        ),
        (1, 4, 6, 4)
    );
    assert!(t.shells[0].closed);
    let v12 = t.vertices.iter().find(|v| v.step_id == 12).unwrap();
    assert_eq!(v12.point, [10.0, 0.0, 0.0]);
    // Edge #53 runs (10,0,0) -> (0,10,0) along a line of magnitude 10 sqrt 2.
    let e = &t.edges[t.edges.iter().position(|e| e.step_id == 53).unwrap()];
    assert!((e.t_start - 0.0).abs() < 1e-12 && (e.t_end - 1.0).abs() < 1e-12);
    let Curve::Line { magnitude, .. } = e.curve else {
        panic!()
    };
    assert!((magnitude - 10.0 * 2f64.sqrt()).abs() < 1e-12);
    let f = &t.faces[0];
    assert!(!f.same_sense);
    let Surface::Plane { frame } = f.surface else {
        panic!()
    };
    assert_eq!(frame.z, [0.0, 0.0, 1.0]);
    let Bound::Edges(es) = &f.loops[0].bound else {
        panic!()
    };
    assert_eq!(es.len(), 3);
    assert!(es[0].forward && !es[1].forward);
    // Every edge is used twice, once in each direction.
    for (i, _) in t.edges.iter().enumerate() {
        let uses: Vec<bool> = t
            .faces
            .iter()
            .flat_map(|f| match &f.loops[0].bound {
                Bound::Edges(es) => es.clone(),
                Bound::Vertex(_) => vec![],
            })
            .filter(|oe| oe.edge == i)
            .map(|oe| oe.forward)
            .collect();
        assert_eq!(uses.len(), 2);
        assert_ne!(uses[0], uses[1]);
    }
    // The disk: a closed circle edge spanning one period from angle 0.
    let d = &m.bodies[1];
    assert_eq!(d.kind, model::BodyKind::ShellModel);
    assert!(!d.shells[0].closed);
    let e = &d.edges[0];
    assert_eq!((e.start, e.end), (0, 0));
    assert!(e.t_start.abs() < 1e-12 && (e.t_end - 360.0).abs() < 1e-12);
    let Curve::Circle { radius, frame } = e.curve else {
        panic!()
    };
    assert_eq!(radius, 10.0);
    assert_eq!(frame.origin, [40.0, 0.0, 0.0]);
}

#[test]
fn no_brep_and_garbage() {
    let m = import(
        b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=PRODUCT('a','b','',());ENDSEC;END-ISO-10303-21;",
        &Limits::default(),
    )
    .unwrap();
    assert!(m.bodies.is_empty() && m.skipped.is_empty());
    assert!(import(b"\x00\xff garbage", &Limits::default()).is_err());
}
