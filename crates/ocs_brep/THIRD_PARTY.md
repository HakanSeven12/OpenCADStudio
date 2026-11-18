# Third-party code

## Solidcore

[Solidcore](https://github.com/tangency-labs/solidcore) is a Rust port of
NVIDIA's SMLib (USD BRep v1.0.1). Copyright (c) 2026 The solidcore
contributors; the upstream work is Copyright (c) 2020-2026 NVIDIA
CORPORATION & AFFILIATES. Licensed under the Apache License, Version 2.0.
"SMLib" and "NVIDIA" identify the origin of the code only.

### NLib NURBS layer (`crates/opencadkernel-nlib`)

Taken from `crates/solidcore` at commit `e261b7d`, with its `LICENSE` and
`NOTICE` alongside (see the crate's `README.md` for the exact file list and
the shim around it):

- `src/nurbs/{geom, math, knots, basis_basic, basis_adv, bezier, index,
  fit_arrays, funcs_basic, funcs_adv, crv_basic, crv_nurb, crv_geom,
  crv_tool/, crv_adv/, crv_fit/, crv_approx, crv_offset, crv_common,
  crv_deviation, crv_proportion, srf_basic, srf_nurb, srf_tool/, srf_fit,
  srf_common, srf_adv/, srf_offset, srf_match/, srf_deviation, srf_seam}.rs`
- `src/limits.rs`, `src/geom/{polynomial, vector2d, vector3d}.rs`
- `src/error/nurbs.rs` (`NurbsFailure`)

Ported onto the kernel's own types:

- `src/space/nurbs_tools.rs`: the surface curvatures from
  `src/surface/surface/p2.rs` (`evaluate_geometric`); the rest wraps the
  vendored routines.
- `src/brep/nurbs_form.rs`: the quarter-grid scheme of exact NURBS forms
  from `src/eval/form.rs`.
- `src/brep/geometry.rs` (torus ray hits): the closed-form quartic of
  `src/geom/polynomial.rs`.

### Mass properties, validity, draft and certified offsets

Ported from `crates/solidcore` at commit `e261b7d` onto the kernel's own
types (algorithms and constants adapted, not copied file for file):

- `src/brep/mass.rs` (`integrated_mass_properties`, `kronrod`): the
  Gauss–Kronrod 7/15 tables, the adaptive quadrature and the trim integral
  (volume integrals as boundary fluxes, Green's theorem into iso-line
  integrals split at knots) of `src/mass/integrator/properties.rs`
  (`kronrod_integrate_with`, `trim_integrand`).
- `src/brep/audit.rs`: the validity tests of `docs/validity-and-tolerances.md`
  §4 and `src/topology/brep/validation/assert_valid/` (edge ends on its
  vertices, edge on its faces, pcurves, loop nesting).
- `src/brep/edit.rs` (`draft_faces`): `plane_draft` and `cone_draft` of
  `src/offset/draft.rs`.
- `src/space/bernstein.rs`: `Ivl` of `src/blend/fillet_section/interval.rs`
  and `Bernstein2` of `src/offset/construction/bernstein.rs`.
- `src/brep/certified_offset.rs`: `certified_offset`, `certify_regular`,
  `largest_wall` and their forms from `src/offset/construction/sheets.rs`.
- `src/brep/wall_room.rs`: `shell_wall_room` of `src/offset/wall_room.rs`.

### Blends: corners and sizes that fit

Ported from `crates/solidcore` at commit `e261b7d` onto the kernel's own
types (algorithms adapted, not copied file for file):

- `src/brep/blend.rs` (`fillet_fit`, `chamfer_fit`, `band_room`): the room
  a round's band has along a straight edge between two planes, a radius `r`
  touching each face `r / tan(phi / 2)` from the edge, read at stations of
  the edge against the face's other boundary edges, from
  `src/blend/band_room.rs` (`band_room`, `EdgeBand::room_at`), without its
  interval certification.
- `src/brep/blend.rs` (`corner_blends`): the ball's centre at a corner of
  any three planes by Cramer's rule, from `planes_meet` of
  `src/blend/blend_corner/ball.rs`; the corner itself stays the kernel's
  analytic sphere rather than Solidcore's rational triangle patch.

### Booleans

Ideas and algorithms from `crates/solidcore` at commit `e261b7d`, rewritten
onto the kernel's own types:

- `src/brep/boolean.rs` (`local_side`): the radial-sector classification of
  a face at an edge it shares with the other body, from
  `src/query/edge.rs` (`radial_sector_classify_between`) as driven by
  `src/boolean/merge/p4.rs` (`classify_edge_faces`), reduced to the smooth
  sector; within ten degrees of tangency it leaves the face to ray casting.
- `src/brep/boolean.rs` (`combine_all`): one Boolean against a list of tools
  combined into a single operand first, after `src/boolean/merge/p3.rs`
  (`boolean_lists`, `merge_breps`).
- `src/brep/imprint.rs` (`tangent_cap`): a crossing curve bounding a cap
  within the tolerance of both surfaces is a touch, from
  `src/topology/face/p3.rs` (`cap_centre_within_tolerance`).
- `src/brep/ssi.rs` (`apart`): proving two faces apart on the convex hulls of
  their surfaces' Bézier pieces, nearest pair first with de Casteljau
  subdivision, after `src/query/distance_bound.rs` and `src/query/contact.rs`.

### Sewing and trim-loop repair of imported bodies

Ported from `crates/solidcore` at commit `e261b7d` onto the kernel's own
types (algorithms adapted, not copied file for file):

- `src/brep/heal.rs` (`sew`, `sew_into_solid`): the stitching of
  `src/heal/stitch/p2.rs` (`Stitch::do_stitching`, `stitch_into_solid`) as
  `docs/validity-and-tolerances.md` §7.3 describes it: vertices sorted along
  (1, 1, 1) and glued within the tolerance, edges split where a vertex lands
  inside them, coincident edges glued, the tolerance doubled while free edges
  remain up to 1/200 of the box diagonal.
- `src/brep/heal.rs` (`fix_loops`): `order_curves_into_loops`,
  `fix_trim_loop_order` and `fix_loop_orientation` of
  `src/topology/trimming_tools.rs`.

### Kernel-case corpus runner

- `examples/kernel_cases.rs` reads Solidcore's kernel-case corpus
  (`crates/solidcore/tests/kernel_cases`) and its portable B-rep record
  (`crates/solidcore-record/src/brep.rs`, whose documented parameterisations
  the reader follows). The corpus is not part of this repository:
  `scripts/fetch-kernel-cases.sh` unpacks it from a local Solidcore clone at
  the pinned commit.
