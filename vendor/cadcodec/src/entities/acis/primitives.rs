//! Parametric ACIS primitive builders.
//!
//! High-level functions that produce a complete [`SatDocument`] for standard
//! 3-D primitives.  Each builder creates the full B-rep topology
//! (body → lump → shell → face → loop → coedge → edge → vertex) with correct
//! geometry so the result can be assigned directly to a [`Solid3D`] entity.
//!
//! # Primitives
//!
//! | Builder | Faces | Edges | Vertices |
//! |---------|-------|-------|----------|
//! | [`build_box`] | 6 | 12 | 8 |
//! | [`build_wedge`] | 5 | 9 | 6 |
//! | [`build_pyramid`] | 5 | 8 | 5 |
//! | [`build_cylinder`] | 3 | 3 | 2 |
//! | [`build_cone`] | 2 | 1 | 1 |
//! | [`build_sphere`] | 1 | 0 | 0 |
//! | [`build_torus`] | 1 | 0 | 0 |
//!
//! # Example
//!
//! ```rust
//! use acadrust::entities::acis::primitives;
//!
//! let sat = primitives::build_box([0.0, 0.0, 0.0], 10.0, 10.0, 10.0);
//! assert_eq!(sat.faces().len(), 6);
//! assert_eq!(sat.edges().len(), 12);
//! assert_eq!(sat.vertices().len(), 8);
//! ```

use super::{SatDocument, SatPointer, SatToken, Sense, Sidedness};

fn ptr(i: i32) -> SatPointer {
    SatPointer::new(i)
}

/// Build an axis-aligned box centered at `center` with the given dimensions.
///
/// The box extends ±`length/2` along X, ±`width/2` along Y, and ±`height/2`
/// along Z from the center point.
pub fn build_box(center: [f64; 3], length: f64, width: f64, height: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);

    let hl = length / 2.0;
    let hw = width / 2.0;
    let hh = height / 2.0;
    let [cx, cy, cz] = center;

    let p0 = sat.add_point(cx - hl, cy - hw, cz - hh);
    let p1 = sat.add_point(cx + hl, cy - hw, cz - hh);
    let p2 = sat.add_point(cx + hl, cy + hw, cz - hh);
    let p3 = sat.add_point(cx - hl, cy + hw, cz - hh);
    let p4 = sat.add_point(cx - hl, cy - hw, cz + hh);
    let p5 = sat.add_point(cx + hl, cy - hw, cz + hh);
    let p6 = sat.add_point(cx + hl, cy + hw, cz + hh);
    let p7 = sat.add_point(cx - hl, cy + hw, cz + hh);

    let surf_top = sat.add_plane_surface([cx, cy, cz + hh], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surf_bottom = sat.add_plane_surface([cx, cy, cz - hh], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let surf_front = sat.add_plane_surface([cx, cy - hw, cz], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let surf_back = sat.add_plane_surface([cx, cy + hw, cz], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    let surf_right = sat.add_plane_surface([cx + hl, cy, cz], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let surf_left = sat.add_plane_surface([cx - hl, cy, cz], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);

    // Bottom edges
    let crv_b0 = sat.add_straight_curve([cx - hl, cy - hw, cz - hh], [1.0, 0.0, 0.0]);
    let crv_b1 = sat.add_straight_curve([cx + hl, cy - hw, cz - hh], [0.0, 1.0, 0.0]);
    let crv_b2 = sat.add_straight_curve([cx + hl, cy + hw, cz - hh], [-1.0, 0.0, 0.0]);
    let crv_b3 = sat.add_straight_curve([cx - hl, cy + hw, cz - hh], [0.0, -1.0, 0.0]);
    // Top edges
    let crv_t0 = sat.add_straight_curve([cx - hl, cy - hw, cz + hh], [1.0, 0.0, 0.0]);
    let crv_t1 = sat.add_straight_curve([cx + hl, cy - hw, cz + hh], [0.0, 1.0, 0.0]);
    let crv_t2 = sat.add_straight_curve([cx + hl, cy + hw, cz + hh], [-1.0, 0.0, 0.0]);
    let crv_t3 = sat.add_straight_curve([cx - hl, cy + hw, cz + hh], [0.0, -1.0, 0.0]);
    // Vertical edges
    let crv_v0 = sat.add_straight_curve([cx - hl, cy - hw, cz - hh], [0.0, 0.0, 1.0]);
    let crv_v1 = sat.add_straight_curve([cx + hl, cy - hw, cz - hh], [0.0, 0.0, 1.0]);
    let crv_v2 = sat.add_straight_curve([cx + hl, cy + hw, cz - hh], [0.0, 0.0, 1.0]);
    let crv_v3 = sat.add_straight_curve([cx - hl, cy + hw, cz - hh], [0.0, 0.0, 1.0]);

    let v0 = sat.add_vertex(SatPointer::NULL, ptr(p0));
    let v1 = sat.add_vertex(SatPointer::NULL, ptr(p1));
    let v2 = sat.add_vertex(SatPointer::NULL, ptr(p2));
    let v3 = sat.add_vertex(SatPointer::NULL, ptr(p3));
    let v4 = sat.add_vertex(SatPointer::NULL, ptr(p4));
    let v5 = sat.add_vertex(SatPointer::NULL, ptr(p5));
    let v6 = sat.add_vertex(SatPointer::NULL, ptr(p6));
    let v7 = sat.add_vertex(SatPointer::NULL, ptr(p7));

    let e0 = sat.add_edge(
        ptr(v0),
        0.0,
        ptr(v1),
        length,
        SatPointer::NULL,
        ptr(crv_b0),
        Sense::Forward,
    );
    let e1 = sat.add_edge(
        ptr(v1),
        0.0,
        ptr(v2),
        width,
        SatPointer::NULL,
        ptr(crv_b1),
        Sense::Forward,
    );
    let e2 = sat.add_edge(
        ptr(v2),
        0.0,
        ptr(v3),
        length,
        SatPointer::NULL,
        ptr(crv_b2),
        Sense::Forward,
    );
    let e3 = sat.add_edge(
        ptr(v3),
        0.0,
        ptr(v0),
        width,
        SatPointer::NULL,
        ptr(crv_b3),
        Sense::Forward,
    );
    let e4 = sat.add_edge(
        ptr(v4),
        0.0,
        ptr(v5),
        length,
        SatPointer::NULL,
        ptr(crv_t0),
        Sense::Forward,
    );
    let e5 = sat.add_edge(
        ptr(v5),
        0.0,
        ptr(v6),
        width,
        SatPointer::NULL,
        ptr(crv_t1),
        Sense::Forward,
    );
    let e6 = sat.add_edge(
        ptr(v6),
        0.0,
        ptr(v7),
        length,
        SatPointer::NULL,
        ptr(crv_t2),
        Sense::Forward,
    );
    let e7 = sat.add_edge(
        ptr(v7),
        0.0,
        ptr(v4),
        width,
        SatPointer::NULL,
        ptr(crv_t3),
        Sense::Forward,
    );
    let e8 = sat.add_edge(
        ptr(v0),
        0.0,
        ptr(v4),
        height,
        SatPointer::NULL,
        ptr(crv_v0),
        Sense::Forward,
    );
    let e9 = sat.add_edge(
        ptr(v1),
        0.0,
        ptr(v5),
        height,
        SatPointer::NULL,
        ptr(crv_v1),
        Sense::Forward,
    );
    let e10 = sat.add_edge(
        ptr(v2),
        0.0,
        ptr(v6),
        height,
        SatPointer::NULL,
        ptr(crv_v2),
        Sense::Forward,
    );
    let e11 = sat.add_edge(
        ptr(v3),
        0.0,
        ptr(v7),
        height,
        SatPointer::NULL,
        ptr(crv_v3),
        Sense::Forward,
    );

    let base = sat.records.len() as i32;
    let co_base = base;
    let loop_base = base + 24;
    let face_base = base + 30;
    let shell_idx = base + 36;
    let lump_idx = base + 37;
    let co = |i: i32| co_base + i;

    // Bottom face
    sat.add_coedge(
        ptr(co(1)),
        ptr(co(3)),
        ptr(co(8)),
        ptr(e0),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(2)),
        ptr(co(0)),
        ptr(co(20)),
        ptr(e3),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(3)),
        ptr(co(1)),
        ptr(co(12)),
        ptr(e2),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(0)),
        ptr(co(2)),
        ptr(co(16)),
        ptr(e1),
        Sense::Reversed,
        ptr(loop_base),
    );
    // Top face
    sat.add_coedge(
        ptr(co(5)),
        ptr(co(7)),
        ptr(co(10)),
        ptr(e4),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(6)),
        ptr(co(4)),
        ptr(co(18)),
        ptr(e5),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(7)),
        ptr(co(5)),
        ptr(co(14)),
        ptr(e6),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(4)),
        ptr(co(6)),
        ptr(co(22)),
        ptr(e7),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    // Front face
    sat.add_coedge(
        ptr(co(9)),
        ptr(co(11)),
        ptr(co(0)),
        ptr(e0),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(10)),
        ptr(co(8)),
        ptr(co(19)),
        ptr(e9),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(11)),
        ptr(co(9)),
        ptr(co(4)),
        ptr(e4),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(8)),
        ptr(co(10)),
        ptr(co(21)),
        ptr(e8),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    // Back face
    sat.add_coedge(
        ptr(co(13)),
        ptr(co(15)),
        ptr(co(2)),
        ptr(e2),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(14)),
        ptr(co(12)),
        ptr(co(23)),
        ptr(e11),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(15)),
        ptr(co(13)),
        ptr(co(6)),
        ptr(e6),
        Sense::Reversed,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(12)),
        ptr(co(14)),
        ptr(co(17)),
        ptr(e10),
        Sense::Reversed,
        ptr(loop_base + 3),
    );
    // Right face
    sat.add_coedge(
        ptr(co(17)),
        ptr(co(19)),
        ptr(co(3)),
        ptr(e1),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(18)),
        ptr(co(16)),
        ptr(co(15)),
        ptr(e10),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(19)),
        ptr(co(17)),
        ptr(co(5)),
        ptr(e5),
        Sense::Reversed,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(16)),
        ptr(co(18)),
        ptr(co(9)),
        ptr(e9),
        Sense::Reversed,
        ptr(loop_base + 4),
    );
    // Left face
    sat.add_coedge(
        ptr(co(21)),
        ptr(co(23)),
        ptr(co(1)),
        ptr(e3),
        Sense::Forward,
        ptr(loop_base + 5),
    );
    sat.add_coedge(
        ptr(co(22)),
        ptr(co(20)),
        ptr(co(11)),
        ptr(e8),
        Sense::Forward,
        ptr(loop_base + 5),
    );
    sat.add_coedge(
        ptr(co(23)),
        ptr(co(21)),
        ptr(co(7)),
        ptr(e7),
        Sense::Reversed,
        ptr(loop_base + 5),
    );
    sat.add_coedge(
        ptr(co(20)),
        ptr(co(22)),
        ptr(co(13)),
        ptr(e11),
        Sense::Reversed,
        ptr(loop_base + 5),
    );

    sat.add_loop(SatPointer::NULL, ptr(co(0)), ptr(face_base));
    sat.add_loop(SatPointer::NULL, ptr(co(4)), ptr(face_base + 1));
    sat.add_loop(SatPointer::NULL, ptr(co(8)), ptr(face_base + 2));
    sat.add_loop(SatPointer::NULL, ptr(co(12)), ptr(face_base + 3));
    sat.add_loop(SatPointer::NULL, ptr(co(16)), ptr(face_base + 4));
    sat.add_loop(SatPointer::NULL, ptr(co(20)), ptr(face_base + 5));

    sat.add_face(
        ptr(face_base + 1),
        ptr(loop_base),
        ptr(shell_idx),
        ptr(surf_bottom),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 2),
        ptr(loop_base + 1),
        ptr(shell_idx),
        ptr(surf_top),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 3),
        ptr(loop_base + 2),
        ptr(shell_idx),
        ptr(surf_front),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 4),
        ptr(loop_base + 3),
        ptr(shell_idx),
        ptr(surf_back),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 5),
        ptr(loop_base + 4),
        ptr(shell_idx),
        ptr(surf_right),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        SatPointer::NULL,
        ptr(loop_base + 5),
        ptr(shell_idx),
        ptr(surf_left),
        Sense::Forward,
        Sidedness::Single,
    );

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a right triangular prism (wedge) with one corner at `origin`.
///
/// The wedge has a right-triangle cross-section in the XY plane:
/// - Leg along X with length `length`
/// - Leg along Y with length `width`
/// - Hypotenuse connecting (length, 0) to (0, width)
///
/// Extruded along Z by `height`.
pub fn build_wedge(origin: [f64; 3], length: f64, width: f64, height: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);
    let [ox, oy, oz] = origin;

    let hyp_len = (length * length + width * width).sqrt();
    let s = 1.0 / hyp_len; // normalization factor
    let nx = width * s; // hypotenuse normal X component
    let ny = length * s; // hypotenuse normal Y component

    let p_a = sat.add_point(ox, oy, oz);
    let p_b = sat.add_point(ox + length, oy, oz);
    let p_c = sat.add_point(ox, oy + width, oz);
    let p_d = sat.add_point(ox, oy, oz + height);
    let p_e = sat.add_point(ox + length, oy, oz + height);
    let p_f = sat.add_point(ox, oy + width, oz + height);

    let surf_bot = sat.add_plane_surface([ox, oy, oz], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let surf_top = sat.add_plane_surface([ox, oy, oz + height], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surf_frt = sat.add_plane_surface([ox, oy, oz], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let surf_lft = sat.add_plane_surface([ox, oy, oz], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let hyp_mid = [ox + length / 2.0, oy + width / 2.0, oz];
    let surf_hyp = sat.add_plane_surface(hyp_mid, [nx, ny, 0.0], [0.0, 0.0, 1.0]);

    // Hypotenuse edge direction (unit vector from B to C)
    let hx = -length / hyp_len;
    let hy = width / hyp_len;

    let crv0 = sat.add_straight_curve([ox, oy, oz], [1.0, 0.0, 0.0]);
    let crv1 = sat.add_straight_curve([ox + length, oy, oz], [hx, hy, 0.0]);
    let crv2 = sat.add_straight_curve([ox, oy + width, oz], [0.0, -1.0, 0.0]);
    let crv3 = sat.add_straight_curve([ox, oy, oz + height], [1.0, 0.0, 0.0]);
    let crv4 = sat.add_straight_curve([ox + length, oy, oz + height], [hx, hy, 0.0]);
    let crv5 = sat.add_straight_curve([ox, oy + width, oz + height], [0.0, -1.0, 0.0]);
    let crv6 = sat.add_straight_curve([ox, oy, oz], [0.0, 0.0, 1.0]);
    let crv7 = sat.add_straight_curve([ox + length, oy, oz], [0.0, 0.0, 1.0]);
    let crv8 = sat.add_straight_curve([ox, oy + width, oz], [0.0, 0.0, 1.0]);

    let v_a = sat.add_vertex(SatPointer::NULL, ptr(p_a));
    let v_b = sat.add_vertex(SatPointer::NULL, ptr(p_b));
    let v_c = sat.add_vertex(SatPointer::NULL, ptr(p_c));
    let v_d = sat.add_vertex(SatPointer::NULL, ptr(p_d));
    let v_e = sat.add_vertex(SatPointer::NULL, ptr(p_e));
    let v_f = sat.add_vertex(SatPointer::NULL, ptr(p_f));

    let e0 = sat.add_edge(
        ptr(v_a),
        0.0,
        ptr(v_b),
        length,
        SatPointer::NULL,
        ptr(crv0),
        Sense::Forward,
    );
    let e1 = sat.add_edge(
        ptr(v_b),
        0.0,
        ptr(v_c),
        hyp_len,
        SatPointer::NULL,
        ptr(crv1),
        Sense::Forward,
    );
    let e2 = sat.add_edge(
        ptr(v_c),
        0.0,
        ptr(v_a),
        width,
        SatPointer::NULL,
        ptr(crv2),
        Sense::Forward,
    );
    let e3 = sat.add_edge(
        ptr(v_d),
        0.0,
        ptr(v_e),
        length,
        SatPointer::NULL,
        ptr(crv3),
        Sense::Forward,
    );
    let e4 = sat.add_edge(
        ptr(v_e),
        0.0,
        ptr(v_f),
        hyp_len,
        SatPointer::NULL,
        ptr(crv4),
        Sense::Forward,
    );
    let e5 = sat.add_edge(
        ptr(v_f),
        0.0,
        ptr(v_d),
        width,
        SatPointer::NULL,
        ptr(crv5),
        Sense::Forward,
    );
    let e6 = sat.add_edge(
        ptr(v_a),
        0.0,
        ptr(v_d),
        height,
        SatPointer::NULL,
        ptr(crv6),
        Sense::Forward,
    );
    let e7 = sat.add_edge(
        ptr(v_b),
        0.0,
        ptr(v_e),
        height,
        SatPointer::NULL,
        ptr(crv7),
        Sense::Forward,
    );
    let e8 = sat.add_edge(
        ptr(v_c),
        0.0,
        ptr(v_f),
        height,
        SatPointer::NULL,
        ptr(crv8),
        Sense::Forward,
    );

    let base = sat.records.len() as i32;
    let co = |i: i32| base + i;
    let loop_base = base + 18;
    let face_base = base + 23;
    let shell_idx = base + 28;
    let lump_idx = base + 29;

    // Bottom: A→C→B
    sat.add_coedge(
        ptr(co(1)),
        ptr(co(2)),
        ptr(co(13)),
        ptr(e2),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(2)),
        ptr(co(0)),
        ptr(co(14)),
        ptr(e1),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(0)),
        ptr(co(1)),
        ptr(co(6)),
        ptr(e0),
        Sense::Reversed,
        ptr(loop_base),
    );
    // Top: D→E→F
    sat.add_coedge(
        ptr(co(4)),
        ptr(co(5)),
        ptr(co(8)),
        ptr(e3),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(5)),
        ptr(co(3)),
        ptr(co(16)),
        ptr(e4),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(3)),
        ptr(co(4)),
        ptr(co(11)),
        ptr(e5),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    // Front: A→B→E→D
    sat.add_coedge(
        ptr(co(7)),
        ptr(co(9)),
        ptr(co(2)),
        ptr(e0),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(8)),
        ptr(co(6)),
        ptr(co(17)),
        ptr(e7),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(9)),
        ptr(co(7)),
        ptr(co(3)),
        ptr(e3),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(6)),
        ptr(co(8)),
        ptr(co(10)),
        ptr(e6),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    // Left: A→D→F→C
    sat.add_coedge(
        ptr(co(11)),
        ptr(co(13)),
        ptr(co(9)),
        ptr(e6),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(12)),
        ptr(co(10)),
        ptr(co(5)),
        ptr(e5),
        Sense::Reversed,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(13)),
        ptr(co(11)),
        ptr(co(15)),
        ptr(e8),
        Sense::Reversed,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(10)),
        ptr(co(12)),
        ptr(co(0)),
        ptr(e2),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    // Hypotenuse: B→C→F→E
    sat.add_coedge(
        ptr(co(15)),
        ptr(co(17)),
        ptr(co(1)),
        ptr(e1),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(16)),
        ptr(co(14)),
        ptr(co(12)),
        ptr(e8),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(17)),
        ptr(co(15)),
        ptr(co(4)),
        ptr(e4),
        Sense::Reversed,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(14)),
        ptr(co(16)),
        ptr(co(7)),
        ptr(e7),
        Sense::Reversed,
        ptr(loop_base + 4),
    );

    sat.add_loop(SatPointer::NULL, ptr(co(0)), ptr(face_base));
    sat.add_loop(SatPointer::NULL, ptr(co(3)), ptr(face_base + 1));
    sat.add_loop(SatPointer::NULL, ptr(co(6)), ptr(face_base + 2));
    sat.add_loop(SatPointer::NULL, ptr(co(10)), ptr(face_base + 3));
    sat.add_loop(SatPointer::NULL, ptr(co(14)), ptr(face_base + 4));

    sat.add_face(
        ptr(face_base + 1),
        ptr(loop_base),
        ptr(shell_idx),
        ptr(surf_bot),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 2),
        ptr(loop_base + 1),
        ptr(shell_idx),
        ptr(surf_top),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 3),
        ptr(loop_base + 2),
        ptr(shell_idx),
        ptr(surf_frt),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 4),
        ptr(loop_base + 3),
        ptr(shell_idx),
        ptr(surf_lft),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        SatPointer::NULL,
        ptr(loop_base + 4),
        ptr(shell_idx),
        ptr(surf_hyp),
        Sense::Forward,
        Sidedness::Single,
    );

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a square-base pyramid with its base centered at `center`.
///
/// - `base_size`: side length of the square base
/// - `height`: distance from the base plane to the apex along Z
pub fn build_pyramid(center: [f64; 3], base_size: f64, height: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);
    let [cx, cy, cz] = center;

    let hb = base_size / 2.0;
    let s5 = (1.0 + (height / hb).powi(2)).sqrt(); // slant normalization
    let n_horiz = 1.0 / s5;
    let n_vert = (height / hb) / s5;
    let lat_len = (hb * hb + hb * hb + height * height).sqrt();
    let s6 = lat_len; // edge length from corner to apex

    // Lateral edge direction factors
    let inv_lat = 1.0 / lat_len;

    let p_a = sat.add_point(cx - hb, cy - hb, cz);
    let p_b = sat.add_point(cx + hb, cy - hb, cz);
    let p_c = sat.add_point(cx + hb, cy + hb, cz);
    let p_d = sat.add_point(cx - hb, cy + hb, cz);
    let p_e = sat.add_point(cx, cy, cz + height);

    let surf_base = sat.add_plane_surface([cx, cy, cz], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let surf_front =
        sat.add_plane_surface([cx, cy - hb, cz], [0.0, -n_horiz, n_vert], [1.0, 0.0, 0.0]);
    let surf_right =
        sat.add_plane_surface([cx + hb, cy, cz], [n_horiz, 0.0, n_vert], [0.0, 1.0, 0.0]);
    let surf_back =
        sat.add_plane_surface([cx, cy + hb, cz], [0.0, n_horiz, n_vert], [1.0, 0.0, 0.0]);
    let surf_left =
        sat.add_plane_surface([cx - hb, cy, cz], [-n_horiz, 0.0, n_vert], [0.0, 1.0, 0.0]);

    let crv0 = sat.add_straight_curve([cx - hb, cy - hb, cz], [1.0, 0.0, 0.0]);
    let crv1 = sat.add_straight_curve([cx + hb, cy - hb, cz], [0.0, 1.0, 0.0]);
    let crv2 = sat.add_straight_curve([cx + hb, cy + hb, cz], [-1.0, 0.0, 0.0]);
    let crv3 = sat.add_straight_curve([cx - hb, cy + hb, cz], [0.0, -1.0, 0.0]);
    let crv4 = sat.add_straight_curve(
        [cx - hb, cy - hb, cz],
        [hb * inv_lat, hb * inv_lat, height * inv_lat],
    );
    let crv5 = sat.add_straight_curve(
        [cx + hb, cy - hb, cz],
        [-hb * inv_lat, hb * inv_lat, height * inv_lat],
    );
    let crv6 = sat.add_straight_curve(
        [cx + hb, cy + hb, cz],
        [-hb * inv_lat, -hb * inv_lat, height * inv_lat],
    );
    let crv7 = sat.add_straight_curve(
        [cx - hb, cy + hb, cz],
        [hb * inv_lat, -hb * inv_lat, height * inv_lat],
    );

    let v_a = sat.add_vertex(SatPointer::NULL, ptr(p_a));
    let v_b = sat.add_vertex(SatPointer::NULL, ptr(p_b));
    let v_c = sat.add_vertex(SatPointer::NULL, ptr(p_c));
    let v_d = sat.add_vertex(SatPointer::NULL, ptr(p_d));
    let v_e = sat.add_vertex(SatPointer::NULL, ptr(p_e));

    let e0 = sat.add_edge(
        ptr(v_a),
        0.0,
        ptr(v_b),
        base_size,
        SatPointer::NULL,
        ptr(crv0),
        Sense::Forward,
    );
    let e1 = sat.add_edge(
        ptr(v_b),
        0.0,
        ptr(v_c),
        base_size,
        SatPointer::NULL,
        ptr(crv1),
        Sense::Forward,
    );
    let e2 = sat.add_edge(
        ptr(v_c),
        0.0,
        ptr(v_d),
        base_size,
        SatPointer::NULL,
        ptr(crv2),
        Sense::Forward,
    );
    let e3 = sat.add_edge(
        ptr(v_d),
        0.0,
        ptr(v_a),
        base_size,
        SatPointer::NULL,
        ptr(crv3),
        Sense::Forward,
    );
    let e4 = sat.add_edge(
        ptr(v_a),
        0.0,
        ptr(v_e),
        s6,
        SatPointer::NULL,
        ptr(crv4),
        Sense::Forward,
    );
    let e5 = sat.add_edge(
        ptr(v_b),
        0.0,
        ptr(v_e),
        s6,
        SatPointer::NULL,
        ptr(crv5),
        Sense::Forward,
    );
    let e6 = sat.add_edge(
        ptr(v_c),
        0.0,
        ptr(v_e),
        s6,
        SatPointer::NULL,
        ptr(crv6),
        Sense::Forward,
    );
    let e7 = sat.add_edge(
        ptr(v_d),
        0.0,
        ptr(v_e),
        s6,
        SatPointer::NULL,
        ptr(crv7),
        Sense::Forward,
    );

    let base = sat.records.len() as i32;
    let co = |i: i32| base + i;
    let loop_base = base + 16;
    let face_base = base + 21;
    let shell_idx = base + 26;
    let lump_idx = base + 27;

    // Base: A→D→C→B
    sat.add_coedge(
        ptr(co(1)),
        ptr(co(3)),
        ptr(co(15)),
        ptr(e3),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(2)),
        ptr(co(0)),
        ptr(co(10)),
        ptr(e2),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(3)),
        ptr(co(1)),
        ptr(co(7)),
        ptr(e1),
        Sense::Reversed,
        ptr(loop_base),
    );
    sat.add_coedge(
        ptr(co(0)),
        ptr(co(2)),
        ptr(co(4)),
        ptr(e0),
        Sense::Reversed,
        ptr(loop_base),
    );
    // Front: A→B→E
    sat.add_coedge(
        ptr(co(5)),
        ptr(co(6)),
        ptr(co(3)),
        ptr(e0),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(6)),
        ptr(co(4)),
        ptr(co(9)),
        ptr(e5),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    sat.add_coedge(
        ptr(co(4)),
        ptr(co(5)),
        ptr(co(14)),
        ptr(e4),
        Sense::Reversed,
        ptr(loop_base + 1),
    );
    // Right: B→C→E
    sat.add_coedge(
        ptr(co(8)),
        ptr(co(9)),
        ptr(co(2)),
        ptr(e1),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(9)),
        ptr(co(7)),
        ptr(co(12)),
        ptr(e6),
        Sense::Forward,
        ptr(loop_base + 2),
    );
    sat.add_coedge(
        ptr(co(7)),
        ptr(co(8)),
        ptr(co(5)),
        ptr(e5),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    // Back: C→D→E
    sat.add_coedge(
        ptr(co(11)),
        ptr(co(12)),
        ptr(co(1)),
        ptr(e2),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(12)),
        ptr(co(10)),
        ptr(co(15)),
        ptr(e7),
        Sense::Forward,
        ptr(loop_base + 3),
    );
    sat.add_coedge(
        ptr(co(10)),
        ptr(co(11)),
        ptr(co(8)),
        ptr(e6),
        Sense::Reversed,
        ptr(loop_base + 3),
    );
    // Left: D→A→E
    sat.add_coedge(
        ptr(co(14)),
        ptr(co(15)),
        ptr(co(0)),
        ptr(e3),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(15)),
        ptr(co(13)),
        ptr(co(6)),
        ptr(e4),
        Sense::Forward,
        ptr(loop_base + 4),
    );
    sat.add_coedge(
        ptr(co(13)),
        ptr(co(14)),
        ptr(co(11)),
        ptr(e7),
        Sense::Reversed,
        ptr(loop_base + 4),
    );

    sat.add_loop(SatPointer::NULL, ptr(co(0)), ptr(face_base));
    sat.add_loop(SatPointer::NULL, ptr(co(4)), ptr(face_base + 1));
    sat.add_loop(SatPointer::NULL, ptr(co(7)), ptr(face_base + 2));
    sat.add_loop(SatPointer::NULL, ptr(co(10)), ptr(face_base + 3));
    sat.add_loop(SatPointer::NULL, ptr(co(13)), ptr(face_base + 4));

    sat.add_face(
        ptr(face_base + 1),
        ptr(loop_base),
        ptr(shell_idx),
        ptr(surf_base),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 2),
        ptr(loop_base + 1),
        ptr(shell_idx),
        ptr(surf_front),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 3),
        ptr(loop_base + 2),
        ptr(shell_idx),
        ptr(surf_right),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 4),
        ptr(loop_base + 3),
        ptr(shell_idx),
        ptr(surf_back),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        SatPointer::NULL,
        ptr(loop_base + 4),
        ptr(shell_idx),
        ptr(surf_left),
        Sense::Forward,
        Sidedness::Single,
    );

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a cylinder centered at `center` with the given `radius` and `height`.
///
/// The cylinder axis is along Z. The base sits at `center.z` and the top at
/// `center.z + height`.
pub fn build_cylinder(center: [f64; 3], radius: f64, height: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);
    let [cx, cy, cz] = center;
    let tau = std::f64::consts::TAU;

    let p0 = sat.add_point(cx + radius, cy, cz);
    let p1 = sat.add_point(cx + radius, cy, cz + height);

    let surf_bot = sat.add_plane_surface([cx, cy, cz], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let surf_top = sat.add_plane_surface([cx, cy, cz + height], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surf_cyl = sat.add_cone_surface(
        [cx, cy, cz],
        [0.0, 0.0, 1.0],
        [radius, 0.0, 0.0],
        1.0,
        1.0,
        0.0,
    );

    let crv_bot = sat.add_ellipse_curve([cx, cy, cz], [0.0, 0.0, -1.0], [radius, 0.0, 0.0], 1.0);
    let crv_top = sat.add_ellipse_curve(
        [cx, cy, cz + height],
        [0.0, 0.0, 1.0],
        [radius, 0.0, 0.0],
        1.0,
    );

    let v0 = sat.add_vertex(SatPointer::NULL, ptr(p0));
    let v1 = sat.add_vertex(SatPointer::NULL, ptr(p1));

    let e_bot = sat.add_edge(
        ptr(v0),
        0.0,
        ptr(v0),
        tau,
        SatPointer::NULL,
        ptr(crv_bot),
        Sense::Forward,
    );
    let e_top = sat.add_edge(
        ptr(v1),
        0.0,
        ptr(v1),
        tau,
        SatPointer::NULL,
        ptr(crv_top),
        Sense::Forward,
    );

    // Cylindrical face uses two separate loops (one per boundary circle),
    // matching the ACIS convention — no seam edge needed.
    let base = sat.records.len() as i32;
    let co = |i: i32| base + i;
    let loop_base = base + 4;
    let face_base = base + 8;
    let shell_idx = base + 11;
    let lump_idx = base + 12;

    // co0: bottom circle on bottom-cap face (single-coedge loop)
    sat.add_coedge(
        ptr(co(0)),
        ptr(co(0)),
        ptr(co(2)),
        ptr(e_bot),
        Sense::Forward,
        ptr(loop_base),
    );
    // co1: top circle on top-cap face (single-coedge loop)
    sat.add_coedge(
        ptr(co(1)),
        ptr(co(1)),
        ptr(co(3)),
        ptr(e_top),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    // co2: bottom circle boundary on cylindrical face (single-coedge loop)
    sat.add_coedge(
        ptr(co(2)),
        ptr(co(2)),
        ptr(co(0)),
        ptr(e_bot),
        Sense::Reversed,
        ptr(loop_base + 2),
    );
    // co3: top circle boundary on cylindrical face (single-coedge loop)
    sat.add_coedge(
        ptr(co(3)),
        ptr(co(3)),
        ptr(co(1)),
        ptr(e_top),
        Sense::Reversed,
        ptr(loop_base + 3),
    );

    // Bottom cap loop, top cap loop, cylindrical face loop (bottom), cylindrical face loop (top)
    sat.add_loop(SatPointer::NULL, ptr(co(0)), ptr(face_base));
    sat.add_loop(SatPointer::NULL, ptr(co(1)), ptr(face_base + 1));
    sat.add_loop(ptr(loop_base + 3), ptr(co(2)), ptr(face_base + 2)); // next_loop = top boundary loop
    sat.add_loop(SatPointer::NULL, ptr(co(3)), ptr(face_base + 2)); // second loop on same cyl face

    sat.add_face(
        ptr(face_base + 1),
        ptr(loop_base),
        ptr(shell_idx),
        ptr(surf_bot),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        ptr(face_base + 2),
        ptr(loop_base + 1),
        ptr(shell_idx),
        ptr(surf_top),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        SatPointer::NULL,
        ptr(loop_base + 2),
        ptr(shell_idx),
        ptr(surf_cyl),
        Sense::Forward,
        Sidedness::Single,
    );

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a cone with its base centered at `center`.
///
/// - `radius`: radius of the circular base
/// - `height`: distance from the base to the apex along Z
///
/// The apex is at `center.z + height`.
/// The cone face has two loops: the base circle boundary and a degenerate
/// (singularity) edge at the apex.
pub fn build_cone(center: [f64; 3], radius: f64, height: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);
    let [cx, cy, cz] = center;
    let tau = std::f64::consts::TAU;
    let hyp = (radius * radius + height * height).sqrt();
    let sin_half = -radius / hyp;
    let cos_half = height / hyp;

    let p_base = sat.add_point(cx + radius, cy, cz);
    let p_apex = sat.add_point(cx, cy, cz + height);

    let surf_base = sat.add_plane_surface([cx, cy, cz], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let surf_cone = sat.add_cone_surface(
        [cx, cy, cz],
        [0.0, 0.0, 1.0],
        [radius, 0.0, 0.0],
        1.0,
        cos_half,
        sin_half,
    );

    let crv_base = sat.add_ellipse_curve([cx, cy, cz], [0.0, 0.0, -1.0], [radius, 0.0, 0.0], 1.0);

    let v0 = sat.add_vertex(SatPointer::NULL, ptr(p_base));
    let v_apex = sat.add_vertex(SatPointer::NULL, ptr(p_apex));

    let e_base = sat.add_edge(
        ptr(v0),
        0.0,
        ptr(v0),
        tau,
        SatPointer::NULL,
        ptr(crv_base),
        Sense::Forward,
    );
    // Singularity (degenerate) edge at the apex — zero-length, no curve
    let e_apex = sat.add_edge(
        ptr(v_apex),
        1.0,
        ptr(v_apex),
        0.0,
        SatPointer::NULL,
        SatPointer::NULL,
        Sense::Forward,
    );

    let base = sat.records.len() as i32;
    let co = |i: i32| base + i;
    let loop_base = base + 3;
    let face_base = base + 6;
    let shell_idx = base + 8;
    let lump_idx = base + 9;

    // co0: base circle on the base-cap face
    sat.add_coedge(
        ptr(co(0)),
        ptr(co(0)),
        ptr(co(1)),
        ptr(e_base),
        Sense::Reversed,
        ptr(loop_base),
    );
    // co1: base circle boundary on the cone face
    sat.add_coedge(
        ptr(co(1)),
        ptr(co(1)),
        ptr(co(0)),
        ptr(e_base),
        Sense::Forward,
        ptr(loop_base + 1),
    );
    // co2: singularity (apex) on the cone face — partner=$-1 (no partner for degenerate edge)
    sat.add_coedge(
        ptr(co(2)),
        ptr(co(2)),
        SatPointer::NULL,
        ptr(e_apex),
        Sense::Reversed,
        ptr(loop_base + 2),
    );

    // Base cap loop
    sat.add_loop(SatPointer::NULL, ptr(co(0)), ptr(face_base));
    // Cone face: two loops — base circle boundary + apex singularity
    sat.add_loop(ptr(loop_base + 2), ptr(co(1)), ptr(face_base + 1)); // next_loop = singularity loop
    sat.add_loop(SatPointer::NULL, ptr(co(2)), ptr(face_base + 1)); // singularity loop on same cone face

    sat.add_face(
        ptr(face_base + 1),
        ptr(loop_base),
        ptr(shell_idx),
        ptr(surf_base),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_face(
        SatPointer::NULL,
        ptr(loop_base + 1),
        ptr(shell_idx),
        ptr(surf_cone),
        Sense::Forward,
        Sidedness::Single,
    );

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a sphere centered at `center` with the given `radius`.
///
/// Produces a single-face closed surface (no loop entity).
pub fn build_sphere(center: [f64; 3], radius: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);

    let surf = sat.add_sphere_surface(center, radius, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let base = sat.records.len() as i32;
    let face_idx = base;
    let shell_idx = base + 1;
    let lump_idx = base + 2;

    sat.add_face(
        SatPointer::NULL,
        SatPointer::NULL,
        ptr(shell_idx),
        ptr(surf),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_shell(ptr(face_idx), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

/// Build a torus centered at `center` with the given radii.
///
/// - `major_radius`: distance from the center to the tube center
/// - `minor_radius`: radius of the tube
///
/// The torus axis is along Z. Produces a single-face closed surface (no loop
/// entity).
pub fn build_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> SatDocument {
    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);

    let surf = sat.add_torus_surface(
        center,
        [0.0, 0.0, 1.0],
        major_radius,
        minor_radius,
        [1.0, 0.0, 0.0],
    );

    let base = sat.records.len() as i32;
    let face_idx = base;
    let shell_idx = base + 1;
    let lump_idx = base + 2;

    sat.add_face(
        SatPointer::NULL,
        SatPointer::NULL,
        ptr(shell_idx),
        ptr(surf),
        Sense::Forward,
        Sidedness::Single,
    );
    sat.add_shell(ptr(face_idx), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);

    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }
    sat.complete_brep_links();
    sat
}

// ── General planar polyhedron ────────────────────────────────────────────────

fn vsub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vcross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn vnorm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Root, outward normal and u-direction of a planar face's supporting plane,
/// derived from its (outward-CCW) vertex ring. `None` if the ring is degenerate
/// (all points collinear).
fn face_plane(vertices: &[[f64; 3]], ring: &[usize]) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let v0 = vertices[ring[0]];
    let a = vsub(vertices[ring[1]], v0);
    let alen = vnorm(a);
    if alen < 1e-12 {
        return None;
    }
    let u = [a[0] / alen, a[1] / alen, a[2] / alen];
    for &k in &ring[2..] {
        let b = vsub(vertices[k], v0);
        let n = vcross(a, b);
        let nl = vnorm(n);
        if nl > 1e-9 {
            return Some((v0, [n[0] / nl, n[1] / nl, n[2] / nl], u));
        }
    }
    None
}

fn set_coedge_partner(sat: &mut SatDocument, coedge_idx: i32, partner_idx: i32) {
    if let Some(rec) = sat.record_mut(coedge_idx as usize) {
        // coedge tokens: [0]=sentinel [1]=next [2]=prev [3]=partner [4]=edge …
        rec.tokens[3] = SatToken::Pointer(SatPointer::new(partner_idx));
    }
}

/// Build an exact ACIS B-rep for a closed polyhedron whose faces are all planar
/// polygons — the general form of [`build_box`], used to export a faceted or
/// natively-planar solid.
///
/// * `vertices` — the distinct corner positions.
/// * `faces` — each face is an ordered ring of indices into `vertices`, wound
///   counter-clockwise **as seen from outside** the solid, so the outward
///   normal is `(v1-v0) × (v2-v0)`. Only single (outer) loops are supported;
///   faces with holes are not.
///
/// The mesh must be a closed 2-manifold: every polygon edge is shared by exactly
/// two faces. Returns `None` (leaving the caller's geometry untouched) on any
/// degeneracy — a face with fewer than 3 vertices, an out-of-range or repeated
/// index, a collinear face, a zero-length edge, a non-manifold edge, or a body
/// that fails [`SatDocument::validate`] — so a malformed solid is never emitted.
pub fn build_planar_body(vertices: &[[f64; 3]], faces: &[Vec<usize>]) -> Option<SatDocument> {
    use std::collections::HashMap;

    if faces.is_empty() || vertices.len() < 4 {
        return None;
    }
    for f in faces {
        if f.len() < 3 {
            return None;
        }
        for &vi in f {
            if vi >= vertices.len() {
                return None;
            }
        }
    }

    let mut sat = SatDocument::new_body();
    let body_idx = SatPointer::new(0);

    // Points + vertices (one per distinct corner).
    let mut vert_idx = Vec::with_capacity(vertices.len());
    for &[x, y, z] in vertices {
        let p = sat.add_point(x, y, z);
        vert_idx.push(sat.add_vertex(SatPointer::NULL, ptr(p)));
    }

    // Unique undirected edges. Canonical direction = first directed occurrence;
    // the two coedges that later reference it get opposite senses.
    struct EdgeInfo {
        edge_idx: i32,
        start: usize,
        end: usize,
        coedges: Vec<i32>,
    }
    let mut edges: HashMap<(usize, usize), EdgeInfo> = HashMap::new();
    let mut edge_order: Vec<(usize, usize)> = Vec::new();
    for f in faces {
        let n = f.len();
        for i in 0..n {
            let a = f[i];
            let b = f[(i + 1) % n];
            if a == b {
                return None;
            }
            let key = if a < b { (a, b) } else { (b, a) };
            if !edges.contains_key(&key) {
                let sp = vertices[a];
                let dir = vsub(vertices[b], sp);
                let len = vnorm(dir);
                if len < 1e-12 {
                    return None;
                }
                let ud = [dir[0] / len, dir[1] / len, dir[2] / len];
                let crv = sat.add_straight_curve(sp, ud);
                let e = sat.add_edge(
                    ptr(vert_idx[a]),
                    0.0,
                    ptr(vert_idx[b]),
                    len,
                    SatPointer::NULL,
                    ptr(crv),
                    Sense::Forward,
                );
                edges.insert(
                    key,
                    EdgeInfo {
                        edge_idx: e,
                        start: a,
                        end: b,
                        coedges: Vec::new(),
                    },
                );
                edge_order.push(key);
            }
        }
    }

    // One plane surface per face.
    let mut surf_idx = Vec::with_capacity(faces.len());
    for f in faces {
        let (root, normal, u_dir) = face_plane(vertices, f)?;
        surf_idx.push(sat.add_plane_surface(root, normal, u_dir));
    }

    // Contiguous index layout: coedges, then loops, faces, shell, lump.
    let co_base = sat.records.len() as i32;
    let num_coedges: usize = faces.iter().map(|f| f.len()).sum();
    let loop_base = co_base + num_coedges as i32;
    let face_base = loop_base + faces.len() as i32;
    let shell_idx = face_base + faces.len() as i32;
    let lump_idx = shell_idx + 1;

    // Coedges: a next/prev ring per face loop; partner filled in afterwards.
    let mut co_cursor = co_base;
    let mut face_first_co = Vec::with_capacity(faces.len());
    for (fi, f) in faces.iter().enumerate() {
        let n = f.len() as i32;
        let start = co_cursor;
        face_first_co.push(start);
        for i in 0..f.len() {
            let a = f[i];
            let b = f[(i + 1) % f.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            let info = edges.get_mut(&key).unwrap();
            let sense = if info.start == a && info.end == b {
                Sense::Forward
            } else {
                Sense::Reversed
            };
            let next = start + ((i as i32 + 1) % n);
            let prev = start + ((i as i32 + n - 1) % n);
            let owner_loop = loop_base + fi as i32;
            let idx = sat.add_coedge(
                ptr(next),
                ptr(prev),
                SatPointer::NULL,
                ptr(info.edge_idx),
                sense,
                ptr(owner_loop),
            );
            info.coedges.push(idx);
            co_cursor += 1;
        }
    }

    // Partner-link the two coedges of every edge (must be exactly two).
    for key in &edge_order {
        let info = &edges[key];
        if info.coedges.len() != 2 {
            return None; // open or non-manifold edge
        }
        let (c0, c1) = (info.coedges[0], info.coedges[1]);
        set_coedge_partner(&mut sat, c0, c1);
        set_coedge_partner(&mut sat, c1, c0);
    }

    // Loops + faces (each face's normal already matches its plane → Forward).
    for fi in 0..faces.len() {
        sat.add_loop(
            SatPointer::NULL,
            ptr(face_first_co[fi]),
            ptr(face_base + fi as i32),
        );
    }
    for fi in 0..faces.len() {
        let next_face = if fi + 1 < faces.len() {
            ptr(face_base + fi as i32 + 1)
        } else {
            SatPointer::NULL
        };
        sat.add_face(
            next_face,
            ptr(loop_base + fi as i32),
            ptr(shell_idx),
            ptr(surf_idx[fi]),
            Sense::Forward,
            Sidedness::Single,
        );
    }

    sat.add_shell(ptr(face_base), ptr(lump_idx));
    sat.add_lump(ptr(shell_idx), body_idx);
    if let Some(body_rec) = sat.record_mut(0) {
        body_rec.tokens[1] = SatToken::Pointer(ptr(lump_idx));
    }

    sat.complete_brep_links();

    if sat.validate().is_empty() {
        Some(sat)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_topology() {
        let sat = build_box([0.0, 0.0, 0.0], 10.0, 10.0, 10.0);
        assert_eq!(sat.faces().len(), 6);
        assert_eq!(sat.edges().len(), 12);
        assert_eq!(sat.vertices().len(), 8);
        assert!(
            sat.validate().is_empty(),
            "box validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn box_offset_center() {
        let sat = build_box([5.0, 5.0, 5.0], 4.0, 6.0, 8.0);
        assert_eq!(sat.faces().len(), 6);
        assert!(sat.validate().is_empty());
    }

    #[test]
    fn wedge_topology() {
        let sat = build_wedge([0.0, 0.0, 0.0], 10.0, 10.0, 10.0);
        assert_eq!(sat.faces().len(), 5);
        assert_eq!(sat.edges().len(), 9);
        assert_eq!(sat.vertices().len(), 6);
        assert!(
            sat.validate().is_empty(),
            "wedge validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn pyramid_topology() {
        let sat = build_pyramid([0.0, 0.0, 0.0], 10.0, 10.0);
        assert_eq!(sat.faces().len(), 5);
        assert_eq!(sat.edges().len(), 8);
        assert_eq!(sat.vertices().len(), 5);
        assert!(
            sat.validate().is_empty(),
            "pyramid validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn cylinder_topology() {
        let sat = build_cylinder([0.0, 0.0, 0.0], 5.0, 10.0);
        assert_eq!(sat.faces().len(), 3);
        assert_eq!(sat.edges().len(), 2);
        assert_eq!(sat.vertices().len(), 2);
        assert!(
            sat.validate().is_empty(),
            "cylinder validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn cone_topology() {
        let sat = build_cone([0.0, 0.0, 0.0], 5.0, 10.0);
        assert_eq!(sat.faces().len(), 2);
        assert_eq!(sat.edges().len(), 2);
        assert_eq!(sat.vertices().len(), 2);
        assert!(
            sat.validate().is_empty(),
            "cone validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn sphere_topology() {
        let sat = build_sphere([0.0, 0.0, 0.0], 5.0);
        assert_eq!(sat.faces().len(), 1);
        assert_eq!(sat.edges().len(), 0);
        assert_eq!(sat.vertices().len(), 0);
        assert!(
            sat.validate().is_empty(),
            "sphere validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn torus_topology() {
        let sat = build_torus([0.0, 0.0, 0.0], 5.0, 2.0);
        assert_eq!(sat.faces().len(), 1);
        assert_eq!(sat.edges().len(), 0);
        assert_eq!(sat.vertices().len(), 0);
        assert!(
            sat.validate().is_empty(),
            "torus validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn planar_body_cube() {
        // Unit cube; every face wound CCW as seen from outside.
        let vertices = [
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];
        let faces = vec![
            vec![0, 3, 2, 1], // bottom -Z
            vec![4, 5, 6, 7], // top    +Z
            vec![0, 1, 5, 4], // front  -Y
            vec![3, 7, 6, 2], // back   +Y
            vec![1, 2, 6, 5], // right  +X
            vec![0, 4, 7, 3], // left   -X
        ];
        let sat = build_planar_body(&vertices, &faces).expect("cube should build");
        assert_eq!(sat.faces().len(), 6);
        assert_eq!(sat.edges().len(), 12);
        assert_eq!(sat.vertices().len(), 8);
        assert!(
            sat.validate().is_empty(),
            "cube validation errors: {:?}",
            sat.validate()
        );
    }

    #[test]
    fn generated_solid_topology_has_modeler_back_links() {
        let sat = build_cylinder([0.0, 0.0, 0.0], 2.0, 5.0);
        for vertex in sat.vertices() {
            assert!(
                !vertex.edge().is_null(),
                "persisted ACIS vertices need an incident edge"
            );
        }
        for edge in sat.edges() {
            assert!(
                !edge.coedge().is_null(),
                "persisted ACIS edges need a representative coedge"
            );
        }
    }

    #[test]
    fn planar_body_rejects_open_mesh() {
        // A single square is not a closed manifold — its edges are used once.
        let vertices = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let faces = vec![vec![0, 1, 2, 3]];
        assert!(build_planar_body(&vertices, &faces).is_none());
    }
}
