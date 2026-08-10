# Entity tessellation paths

How each acadrust `EntityType` becomes drawable geometry in OpenCADStudio.

There are **three ways** an entity gets there, and they must not be conflated:

1. **Kernel B-rep mesh** — the ACIS document is lifted into a cadkernel `Body`
   and each face triangulated in its own surface's parameter space. The only
   path that produces filled *mesh* triangles from analytic surfaces.
2. **Curve-sampled** — the entity's geometry is read through
   `entities::curve::entity_curve`, which is where each type's curve is defined
   once, and tessellated to a chord tolerance. Output is a polyline; no surface
   is involved.
3. **Direct** — points, segments or triangles emitted straight from the
   entity's own fields.

The distinction between (2) and (3) is worth keeping because (2) is a single
definition shared with everything else that needs an entity's geometry — snap
candidates, EXTRUDE and REVOLVE profiles, hatch boundaries, clip outlines. A
circle drawn on screen and a circle handed to the Model tab come from the same
place and cannot disagree.

Entry points: `src/scene/convert/tess.rs` (`tessellate_entity`, the per-entity
dispatcher) → `src/scene/convert/tessellate.rs` (`tessellate`, the
`RenderObject` router), `src/entities/curve.rs` (curve definitions and
sampling) and `src/scene/convert/curve_tol.rs` (the per-frame chord tolerance).

## Summary

| Path | Entities |
|---|---|
| **Kernel B-rep mesh** (lift → per-face triangulation, with a direct per-surface fallback) | `Solid3D`, `Region`, `Body`, `Surface` |
| **Curve-sampled** (`entity_curve` → chord-tolerance tessellation) | `Line`, `Arc`, `Circle`, `Ellipse`, `Spline`, `LwPolyline`, `Polyline`, `Polyline2D`, `Polyline3D` — *non-thick variants only* |
| **Direct** (segments/triangles emitted straight) | everything else |

> The Model/Design-tab primitives (BOX, SPHERE, CYLINDER, CONE, WEDGE, TORUS,
> PYRAMID, EXTRUDE, REVOLVE) are true kernel B-reps too, cached on the Scene as
> `Body` and meshed the same way — but they are created as `Solid3D`
> placeholders carrying that body, not as their own `EntityType`. SWEEP and
> LOFT produce a mesh only, with no B-rep behind it.

ACIS solids dispatch through `solid3d_tess::tessellate_acis`, which lifts the
document into the kernel first (`acis_kernel::tessellate_sat`) and falls back
to a bespoke per-surface LOD sampler (`tessellate_sat_lods`) when the kernel
cannot express a face — so their path is **kernel-with-direct-fallback**. The
fallback is not silent: a mesh whose faces did not all lift is marked
incomplete, because one missing wall looks exactly like a finished solid.

Curved-surface tessellation density is radius-relative (`CURVE_REL_TOL`, a
fraction of the surface radius) so a cylinder's facet count matches the
circle/arc wire tessellation instead of exploding on large radii.

## Full table

| Entity | Output | Path | Notes |
|---|---|---|---|
| Arc | wire | curve-sampled | non-thick: sampled from the arc's own curve; thick: direct `Lines` plus the swept wall |
| AttributeDefinition | wire | direct | routes through the Text/MText LFF glyph stroke pipeline |
| AttributeEntity | wire | direct | same as AttributeDefinition; values supplied per Insert |
| Block | none | n/a | block-definition sentinel; not tessellated (referenced via Insert) |
| BlockEnd | none | n/a | block-definition end marker; no output |
| Body | mesh | kernel-with-direct-fallback | 3D ACIS body lifted into a kernel `Body`; fallback per-surface sampler |
| Circle | wire | curve-sampled | non-thick: sampled from the circle's own curve; thick: direct `Lines` |
| Dimension | wire | direct | baked-block path recurses on `D###` sub-entities; synthesis path emits lines/arrows/LFF text |
| Ellipse | wire | curve-sampled | sampled from the ellipse's own curve |
| Face3D | both | direct | edge `Lines` + direct fan-triangulated `fill_tris`; no B-rep |
| Hatch | both | direct | boundary outline not emitted to the wire set (#131 OOM); fill rasterized on GPU |
| Insert | wire | direct | expands block children and tessellates each via its own path; XCLIP filter applied |
| Leader | wire | direct | leader path + arrowhead + landing, direct `Lines` |
| Line | wire | curve-sampled | non-thick: its two ends; thick: direct `Lines` plus the swept wall |
| LwPolyline | wire | curve-sampled | `plinegen` true: sampled from its own curve, bulges kept as arcs; else direct `SegmentedLines` |
| Mesh | both | direct | SubD mesh: edge `Lines` + direct fan-triangulated `fill_tris` |
| MLine | wire | direct | spine + offset lines + caps, direct `Lines` |
| MText | wire | direct | wrap-aware multi-line LFF glyph layout; inline formatting codes |
| MultiLeader | wire | direct | leader + landing + LFF text + frame + fill |
| Ole2Frame | wire | direct | bounding rectangle + diagonal cross |
| Point | wire | direct | a position → dot or cross marker sized by PDSIZE |
| PolyfaceMesh | both | direct | face list: closed-polyline edges + direct fan-triangulated `fill_tris` |
| PolygonMesh | both | direct | M×N grid wireframe + direct fan-triangulated `fill_tris` |
| Polyline | wire | curve-sampled | heavy 3D polyline; sampled from its own curve, bulges kept as arcs |
| Polyline2D | wire | curve-sampled | 2D polyline with bulge; sampled from its own curve |
| Polyline3D | wire | curve-sampled | straight edges, no bulge or thickness |
| RasterImage | wire | direct | boundary rectangle / clipping polygon |
| Ray | wire | direct | two-point `[base, base + dir×1e6]`, no sampling |
| Region | mesh | kernel-with-direct-fallback | 2D planar ACIS body; same kernel path as Solid3D |
| Seqend | none | n/a | vertex-sequence terminator sentinel; no output |
| Shape | wire | direct | small diamond marker at the insertion point |
| Solid | wire | direct | 2D SOLID: four quad edges as direct `Lines` |
| Solid3D | mesh | kernel-with-direct-fallback | 3DSOLID: parse SAT/SAB → lift into a kernel `Body`; fallback sampler |
| Spline | wire | curve-sampled | NURBS sampled through the kernel's space curve, refined where it bends |
| Table | both | direct | cell fills (`fill_tris`) + LFF cell text + grid lines |
| Text | wire | direct | LFF-font stroked glyph polylines |
| Tolerance | wire | direct | feature-control frame grid + per-cell LFF symbols |
| Underlay | wire | direct | boundary rectangle of the PDF/DWF reference |
| Unknown | none | n/a | unrecognized-entity sentinel; no output |
| Viewport | wire | direct | content-viewport frame rectangle (sheet viewport skipped) |
| Wipeout | wire | direct | boundary rectangle / clipping polygon |
| XLine | wire | direct | three-point infinite line, no sampling |
