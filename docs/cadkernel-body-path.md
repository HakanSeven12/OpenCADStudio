# Scripting ACIS bodies through opencadkernel: findings and plan

Investigation of the blocker on **Solid3D, Body, Region and Surface**
(`docs/plugin-host-model-coverage-ledger.md`). The blocker said a script can
neither produce a valid ACIS payload nor edit one, and that the handover
forbids rewriting an opaque payload without a proven lossless path. This note
records what OCS's own kernel path can already do, measured with two spike tests
(`spike_kernel_body_round_trip`, `spike_kernel_timings` in
`src/app/plugin_host.rs`, both `#[ignore]` because they are slow in a debug
build). Measured on opencadcodec `5b682ed` / opencadkernel `6f046af`, debug build.

**Conclusion:** the blocker was overstated. OCS already owns a kernel-backed
create/transform/boolean pipeline whose output is verified lossless before it is
accepted. The missing piece is not geometry but an *operation channel* from a
script to that pipeline. Solid3D and Body are then straightforward; Region and
Surface need a little more; the one real cost is boolean speed.

## What exists

| Capability | Where | Notes |
|---|---|---|
| Primitives: box, wedge, cylinder, cone/frustum, elliptical cylinder, sphere, torus, pyramid, pyramid frustum | `scene/model/solid_model.rs` (`box_solid`, ...) | return a kernel `Body` |
| Transforms: place, turn about an axis, mirror, column-major 4x4 | `placed`, `turned`, `mirrored`, `by_matrix` | analytic surfaces move with their frames, no point rewriting |
| Booleans: union, subtract, intersect | `boolean_result` | refuses with a reason instead of returning a broken solid |
| Body to entity payload | `acis_export::solid_to_sat` | **self-verifying**: it lifts its own SAT text and SAB binary back and returns `None` unless both are lossless (`loss.is_empty()`) and validate |
| Entity payload to body | `solid3d_tess::kernel_body` / `kernel_region_body` / `kernel_surface_body` / `kernel_acis_body` | returns `None` when the lift is lossy or has more than one body |
| Regenerating derived data | `app/model_ops.rs::entity_with_boolean_body` | rewrites `acis_data`, `wires`, clears `silhouettes` and `history_handle` for Solid3D, Region and Surface |
| Volume, extent, edge wires | `volume`, `extent`, `edge_wires` | used as oracles below |

`kernel_acis_body` is exactly the "proven lossless path" the handover asked for:
a payload is edited only if it lifts with no loss, otherwise it is left alone.

## Spike results

Everything below passed, with the kernel's own volume as the oracle.

| Test | Result |
|---|---|
| Box 10x6x4 to Solid3D | volume 240, 12 edge wires, lifts back |
| DWG save/reopen | lifts back, volume 240, 12 wires kept. `acis_data` changes from SAT text to **SAB binary** (same solid, different bytes) |
| DXF save/reopen | lifts back, volume 240. Cached `wires` are dropped (0) and are regenerated on demand; silhouettes were already empty |
| Translate by (5,5,5) | volume 240, extent shifted exactly, lifts back |
| Rotate 45 degrees about Z | volume 240, extent -5.657..5.657, lifts back |
| Mirror in X | volume 240, lifts back |
| Box union cylinder | volume 315.277, 16 wires, lifts back |
| Box subtract cylinder | volume 189.815, 14 wires, lifts back |
| Box intersect cylinder | volume 50.185, 2 wires, lifts back |
| Sphere, torus, wedge, pyramid | volumes 112.696, 98.415, 12.000, 28.532; all lift back |
| Same payload in a `Body` entity | lifts back; still lifts back after DWG and after DXF |

Timings (debug build): primitive, transform, `solid_to_sat` and `edge_wires` are
effectively instant (under 0.05 s). **Each boolean took about 23 s**, most likely
a debug-build artifact for a kernel doing heavy numeric work, but it has not been
measured in release and must be before booleans are exposed.

Two things to understand about "lossless":

- DWG rewrites SAT text as SAB binary. The solid is identical (same volume,
  topology, faces) but the bytes are not, so the honest oracle is "lifts back with
  no loss and the same volume", not byte equality. An untouched body that a
  script never edits is written exactly as OCS writes any other body.
- DXF does not carry the cached wires; they are derived data.

## Design

The obstacle is the channel. The generic entity path moves an opencadcodec
`EntityType` from the plugin to the host; the plugin has no kernel and must never
see or rewrite ACIS bytes. Three ways to give a script the operations:

**A. Host-side normalization with a request carrier (no ABI change).** A
hand-written converter (as for Dimension) turns `create_entity('Solid3D',
primitive='box', ...)` into a Solid3D that carries the request in an otherwise
unused field, and the host's `normalize_scripted_entity` replaces it with the real
payload, exactly as it already does for MLine geometry, Helix splines and image
definitions. It works inside today's v7 protocol, but the carrier field is a hack.

**B. New host operation (recommended).** Add one additive operation to `HostApi`
and the v4 IPC protocol, for example `solid_operation(op) -> Result<Handle, String>`,
with `op` one of: create primitive, transform, boolean, and (later) region from
curves. The host runs it through the pipeline above inside an undo step and
returns the new or updated handle. Because it is a new method with a default
"unsupported" implementation it is backward compatible; bump the minor/negotiate
by capability rather than the major version. Python side:

```python
box  = doc.solids.box(center=(0, 0, 0), size=(10, 6, 4))
cyl  = doc.solids.cylinder(base=(0, 0, -3), radius=2, height=10)
hole = box.subtract(cyl)            # new Solid3D; operands untouched
box.transform(translate=(5, 5, 5))  # in-place, transactional, undoable
```

**C. Do nothing but layer edits** (the current state).

Recommended: **B**, with `solid_operation` covering create, transform and boolean.
Every operation goes through `solid_to_sat` (self-verifying) and the kernel's own
refusal path, so a failure is an error before any mutation, and an untouched
payload is never rewritten. In-place edits of an existing entity are gated by
`kernel_acis_body` returning `Some`: a payload the kernel cannot lift losslessly
is refused with a clear message instead of being rewritten.

## Per-kind feasibility

| Kind | Create | Edit | Notes |
|---|---|---|---|
| **Solid3D** | 9 primitives, boolean of two solids | transform, boolean | Straightforward. Regenerate `wires`; clear `silhouettes`; `history_handle` is cleared by `entity_with_boolean_body`, so a history-carrying solid needs a decision (refuse, or accept losing parametric history). |
| **Body** | same payload container | transform | Same pipeline, `kernel_acis_body` works on a `Body` payload (spike). A true non-solid (sheet or wire) body may not lift; those stay layer-only. |
| **Region** | from closed curve entities (the REGION command's `add_region_model`) | transform | Needs a "region from these curve handles" operation and planar-face construction. |
| **Surface** | plane, extruded, revolved from a profile | transform | The most involved: surface kinds carry `surface_data` besides `acis_data`, and only `Plane` keeps its kind on regeneration (`SurfaceKind::Generic` otherwise). |

## Risks and open questions

1. **Boolean speed.** Measured in release, see "Boolean timings" below: about
   1 to 2.2 s for a box and a cylinder, 0.2 s for two spheres and 1 to 2 ms for
   planar-only operands, against 23 s in a debug build. The kernel also refuses
   many curved cases, which matters more than speed.
2. **Interoperability cannot be verified here.** `solid_to_sat` is verified by
   OCS's own lifter. Whether AutoCAD accepts the SAT/SAB that OCS writes is not
   testable without AutoCAD, so completion should be scoped as "OCS round trip",
   as for RasterImage.
3. **Parametric history.** Regenerating a payload clears `history_handle`.
4. **Frames.** Some solids carry a body-to-world transform in the SAT
   (`body_transform` in `kernel_acis_body`); the lift already applies it, and the
   spike's translated/rotated bodies confirm it, but a foreign DWG with a
   scaled or sheared body transform should be added as a fixture.
5. **API surface.** `solid_operation` is a protocol addition; decide whether it is
   one generic operation enum or separate methods before starting.

## Suggested phases

1. `solid_operation` protocol and host executor for **create primitive** and
   **transform** on Solid3D, with the full lifecycle test through the real runner
   (volume oracle, DWG and DXF reopen, undo, refusal of a lossy payload).
2. **Booleans** (after release-build timing) and the **Body** container.
3. **Region** from curve handles.
4. **Surface** (plane first).

Phase 1 alone would move Solid3D from blocked to complete-in-OCS.

**Status (2026-09-20): phase 1 is done.** `HostApi::solid_operation` (create primitive and rigid transform), the `doc.solids` Python API and `audit_python_solid3d_lifecycle_over_real_ipc` are in; Solid3D is complete, Body moves through the same channel. Design B was implemented as recommended. Region creation from a closed profile followed (`RegionFromProfile`). Plane surfaces and extrusions (`SurfaceFromProfile`, `Extrude`) followed. Only booleans remain, plus lofted, revolved, swept and NURBS surfaces.

## Boolean timings (release build)

Measured 2026-09-20 in a scratch crate built with `--release` (opt-level 3)
against the same opencadkernel revision OCS pins (`6f046af`, features `acis` and
`offset`), calling exactly what OCS's `solid_model::boolean_result` calls:
`brep::operation_tolerance` then `brep::combine`. A debug build of OCS is about
22 times slower, so booleans must never be exercised in a debug test run. The
scratch program is not in the repository; to repeat it, depend on opencadkernel at
that revision and time `brep::combine` on `brep::make::{cuboid, cylinder,
sphere, torus}` operands.

| Operation | Time | Result |
|---|---|---|
| box union / subtract / intersect cylinder (10x6x4 box, r=2 cylinder) | 1.05 to 1.08 s each, identical on repeat runs | ok |
| same, operands scaled 10x and 100x | 2.21 s and 2.21 s | ok |
| box union / subtract box (planar only) | 0.001 to 0.002 s | ok |
| sphere union sphere (overlapping) | 0.22 s | ok |
| sphere subtract sphere (overlapping) | 0.23 s | refused `CutRefused` |
| box subtract sphere (sphere cutting the box faces) | 1.28 s | refused `Coincident` |
| sphere union torus (disjoint) | under 1 ms | refused `NoClosedForm` |
| torus subtract cylinder through the ring | under 1 ms | refused `NoClosedForm` |
| drill a first cylinder hole in a box | 0.03 s | ok |
| drill a second hole in the drilled box | under 1 ms | refused `CutRefused` |

What this means:

- **Speed is acceptable.** A curved boolean costs 0.2 to 2.2 s, the same cost as
  the GUI BOOLEAN command, and planar work is instant. The 23 s seen earlier was
  a debug-build artifact.
- **It blocks the host thread.** The plugin's `PY_RUN` is a synchronous dispatch
  and nested host requests run inline, so a script freezes the UI while a
  boolean runs. One to two seconds per operation is tolerable; a script with
  dozens is not.
- **The whole script has a budget.** A `Dispatch` call uses the 30 s default
  timeout (`OCS_PLUGIN_CALL_TIMEOUT_SECS`, with a 10 s floor), and nested host
  work counts against one deadline. Roughly fifteen curved booleans in a single
  script would exceed it.
- **Refusals are the real limit.** The kernel says no often: coincident faces, a
  cut it has no closed form for, torus operands, and, surprisingly, a second
  cut in a body that was already drilled. Refusals are fast and leave the inputs
  intact, which is the behavior the API already relies on, but it means booleans
  will be a partial feature that fails with a reason, not a general one.
- **Timing is not proportional to size.** Scaling the operands 10x doubled the
  time, 100x changed nothing more, so the cost is dominated by the surface
  intersection setup rather than the geometry size.

### Recommendation

Add `SolidOperation::Boolean { first, second, operation, layer }` and run it
synchronously on the host thread, surfacing the kernel's refusal (`Coincident`,
`CutRefused`, `NoClosedForm`, ...) verbatim in the error and leaving both
operands untouched on failure. Build the result with the existing
`entity_with_boolean_body` path so it inherits the lossless verification.
Test it with **planar** operands (millisecond cost, safe in debug) plus one
`#[ignore]` curved case that documents the release-only runtime. Document the
30 s script budget and that chained cuts on a drilled body are often refused.
Do not move booleans to a background thread yet: it would need cancellation and
undo coordination for a cost that is one to two seconds.

**Status (2026-09-20): implemented.** `SolidOperation::Boolean` and `doc.solids.union`, `subtract` and `intersect` follow the recommendation above, with planar-operand lifecycle testing and an ignored curved refusal test. Every operation this note proposed is now built.
